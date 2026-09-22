//! Pull a period of executions from a broker account into the journal.
//!
//! The other half of the import: same destination, no file. The broker returns fills, the
//! fills fold into positions through the very same `build::fold_fills` the "executions"
//! shape of a file uses, and the result is previewed before anything is written. There is
//! no mapping to detect: an API answers typed fields, so the questions a CSV raises
//! (which column is the date, is the decimal a comma) do not exist here.
//!
//! **A re-sync is not a second import.** The identity of an imported position is its
//! *opening* execution, not the group of its fills, so widening the window and pulling
//! again lands on the rows already written: a position that has closed since is refreshed
//! in place, everything else is left alone. Hashing the whole group would file the same
//! position twice the day it acquires its exit.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::brokers::BrokerRow;
use otw_store::journal_import::{self as store, SyncOutcome};
use otw_store::{journal, journal_fx};

use super::build::{self, Fill, RowError, Warning};
use super::{Mapping, PreviewTrade, Stats};
use crate::brokers::{Broker, BrokerCapability, ExecQuery, Execution};

/// What the caller asked to pull.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncRequest {
    pub account_id: Uuid,
    /// The journal (category) to file the trades into. Absent = the default one.
    #[serde(default)]
    pub category_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub to: OffsetDateTime,
    /// Instruments to ask for, in the broker's own spelling. Required by the brokers whose
    /// history is per instrument, a filter for the others.
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Whether a sell with nothing open opens a short. Absent = what the broker declares
    /// (a cash account cannot go short, so the fill is reported instead of invented).
    #[serde(default)]
    pub allow_short: Option<bool>,
    #[serde(default)]
    pub strategy_id: Option<Uuid>,
    #[serde(default)]
    pub template_id: Option<Uuid>,
}

/// Everything the modal shows before the user commits.
#[derive(Debug, Serialize)]
pub struct SyncPreview {
    pub account: AccountRef,
    pub capability: &'static BrokerCapability,
    #[serde(with = "time::serde::rfc3339")]
    pub from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub to: OffsetDateTime,
    pub category_id: Uuid,
    /// Fills the broker returned for the window.
    pub executions: usize,
    pub preview: Vec<PreviewTrade>,
    pub errors: Vec<RowError>,
    pub stats: Stats,
}

#[derive(Debug, Serialize)]
pub struct AccountRef {
    pub id: Uuid,
    pub name: String,
    pub broker: String,
}

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub batch_id: Uuid,
    pub imported: usize,
    /// Positions a previous sync already wrote and that closed since: refreshed in place.
    pub updated: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub executions: usize,
}

/// A pull, built but not written.
struct Built {
    trades: Vec<build::BuiltTrade>,
    errors: Vec<RowError>,
    executions: usize,
    category_id: Uuid,
}

/// Pull the window and build what it would import. Writes nothing.
pub async fn preview(
    pool: &PgPool,
    connector: &dyn Broker,
    settings: &std::collections::HashMap<String, String>,
    account: &BrokerRow,
    req: &SyncRequest,
    preview_limit: usize,
) -> Result<SyncPreview> {
    let built = pull(pool, connector, settings, account, req).await?;
    let hashes: Vec<String> = built.trades.iter().map(|t| t.row_hash.clone()).collect();
    let known = store::existing_hashes(pool, &hashes).await.unwrap_or_default();

    let mut stats = Stats {
        rows: built.executions,
        trades: built.trades.len(),
        errors: built.errors.len(),
        ..Default::default()
    };
    let mut net: BTreeMap<String, f64> = BTreeMap::new();
    let mut preview = Vec::new();
    for (i, t) in built.trades.iter().enumerate() {
        let computed = journal::preview_pnl(&t.input);
        let duplicate = known.contains(&(t.input.category_id, t.row_hash.clone()));
        if duplicate {
            stats.duplicates += 1;
        }
        if computed.net_pnl.is_some() {
            stats.closed += 1;
        } else {
            stats.open += 1;
        }
        stats.warnings += t.warnings.len();
        if !duplicate {
            if let Some(n) = computed.net_pnl {
                *net.entry(t.input.currency.clone()).or_insert(0.0) += n;
            }
        }
        for d in [t.input.entry_at, t.input.exit_at].into_iter().flatten() {
            let iso = d.format(&Rfc3339).unwrap_or_default();
            if stats.first_date.as_ref().is_none_or(|f| iso < *f) {
                stats.first_date = Some(iso.clone());
            }
            if stats.last_date.as_ref().is_none_or(|l| iso > *l) {
                stats.last_date = Some(iso);
            }
        }
        if i < preview_limit {
            preview.push(PreviewTrade {
                index: i,
                source_rows: t.source_rows.clone(),
                trade: super::clone_input(&t.input),
                computed,
                warnings: t.warnings.clone(),
                duplicate,
                pnl_source: None,
                pnl_mismatch: false,
            });
        }
    }
    stats.net_by_currency = net.into_iter().map(|(c, v)| (c, journal::round_dp(v, 2))).collect();

    Ok(SyncPreview {
        account: AccountRef {
            id: account.id,
            name: account.name.clone(),
            broker: account.broker.clone(),
        },
        capability: connector.capability(),
        from: req.from,
        to: req.to,
        category_id: built.category_id,
        executions: built.executions,
        preview,
        errors: built.errors,
        stats,
    })
}

/// Pull the window and write it, in one revertible batch.
pub async fn commit(
    pool: &PgPool,
    connector: &dyn Broker,
    settings: &std::collections::HashMap<String, String>,
    account: &BrokerRow,
    req: &SyncRequest,
) -> Result<SyncReport> {
    let built = pull(pool, connector, settings, account, req).await?;
    let label = format!(
        "{} · {} → {}",
        account.name,
        req.from.date(),
        req.to.date()
    );
    let batch = store::add_broker_batch(
        pool,
        account.id,
        &label,
        built.category_id,
        built.executions as i32,
    )
    .await?;

    let (mut imported, mut updated, mut duplicates) = (0usize, 0usize, 0usize);
    let mut insert_error = None;
    for t in &built.trades {
        match store::sync_imported_trade(pool, &t.input, batch, &t.row_hash).await {
            Ok(SyncOutcome::Inserted) => imported += 1,
            Ok(SyncOutcome::Updated) => updated += 1,
            Ok(SyncOutcome::Kept) => duplicates += 1,
            // Stop, but keep the batch: whatever landed is tagged, so the half-import can
            // be reverted in one click instead of being hunted for by hand.
            Err(e) => {
                insert_error = Some(e);
                break;
            }
        }
    }
    let failed = built.errors.len();
    store::finish_batch(
        pool,
        batch,
        imported as i32,
        (duplicates + updated) as i32,
        failed as i32,
    )
    .await?;
    if let Some(e) = insert_error {
        return Err(e.context(format!(
            "the sync stopped after {imported} trades. Revert this import to undo them"
        )));
    }
    let symbols = serde_json::to_value(&req.symbols).unwrap_or_else(|_| serde_json::json!([]));
    store::set_broker_sync(pool, built.category_id, account.id, &symbols, req.from, req.to)
        .await
        .ok();

    Ok(SyncReport {
        batch_id: batch,
        imported,
        updated,
        duplicates,
        failed,
        executions: built.executions,
    })
}

// ── Executions → trades ──────────────────────────────────────────────────────

/// Ask the broker, fold the answer, and stamp each position with a stable identity.
async fn pull(
    pool: &PgPool,
    connector: &dyn Broker,
    settings: &std::collections::HashMap<String, String>,
    account: &BrokerRow,
    req: &SyncRequest,
) -> Result<Built> {
    let cap = connector.capability();
    if !cap.executions {
        return Err(anyhow!("{} does not publish an execution history", cap.label));
    }
    if req.to <= req.from {
        return Err(anyhow!("the period ends before it starts"));
    }
    if cap.needs_symbols && req.symbols.is_empty() {
        return Err(anyhow!(
            "{} answers its trade history one instrument at a time. Name the symbols to pull",
            cap.label
        ));
    }
    // A venue with a bounded archive answers an older window with an empty page and no
    // error, which would file as "nothing was traded then".
    crate::brokers::check_window(cap, req.from)?;
    let category_id = match req.category_id {
        Some(id) => id,
        None => default_category(pool).await?,
    };

    let spot_only = !req.allow_short.unwrap_or(!cap.spot_only);
    let q = ExecQuery {
        from: req.from,
        to: req.to,
        symbols: req.symbols.clone(),
    };
    let mut folded = fold_window(
        pool,
        connector,
        settings,
        &q,
        category_id,
        req.strategy_id,
        req.template_id,
        spot_only,
    )
    .await?;
    let executions = folded.executions;
    // A fee in a currency nothing prices is a real hole in the numbers, so it becomes a
    // task the user can resolve, exactly like the breakdown's. The preview writes it too:
    // seeing the hole before committing is the point of a preview.
    for (date, quote) in &folded.fx_misses {
        journal_fx::mark_pending(pool, *date, quote, "needed to convert a broker fee")
            .await
            .ok();
    }
    let ids = std::mem::take(&mut folded.ids);
    for t in &mut folded.trades {
        // The opening fill is the identity: a position that gains an exit tomorrow is the
        // same position, and must land on the row it already has.
        let opener = t
            .source_rows
            .first()
            .and_then(|i| ids.get(*i))
            .cloned()
            .unwrap_or_else(|| t.row_hash.clone());
        t.row_hash = position_hash(account.id, &opener);
    }
    Ok(Built {
        trades: folded.trades,
        errors: folded.errors,
        executions,
        category_id,
    })
}

/// Everything a window of executions folds into, before anyone decides what to do with it.
pub(crate) struct Folded {
    pub trades: Vec<build::BuiltTrade>,
    pub errors: Vec<RowError>,
    /// Fills the broker returned for the window.
    pub executions: usize,
    /// The broker's execution ids in fill order, so a position can be named by the fill
    /// that opened it (`source_rows` indexes into this).
    pub ids: Vec<String>,
    /// (date, currency) pairs no rate could price, from the fee conversions. Handed back
    /// rather than queued here: the tax module folds the same window and writes nothing.
    pub fx_misses: Vec<(time::Date, String)>,
}

/// Ask a broker for a window of fills and fold them into positions. Writes nothing and
/// knows nothing about journals: the tax module folds the same executions to find what a
/// disposal realized, and passes `Uuid::nil()` as the book, since nothing is ever stored.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fold_window(
    pool: &PgPool,
    connector: &dyn Broker,
    settings: &std::collections::HashMap<String, String>,
    q: &ExecQuery,
    category_id: Uuid,
    strategy_id: Option<Uuid>,
    template_id: Option<Uuid>,
    spot_only: bool,
) -> Result<Folded> {
    let http = crate::brokers::client()?;
    let mut execs = connector.executions(&http, settings, q).await?;
    // Chronological per instrument, so the line number a fill carries is also its rank:
    // the position's first source row is the fill that opened it.
    execs.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then(a.at.cmp(&b.at))
            .then_with(|| cmp_exec_id(&a.id, &b.id))
    });
    let executions = execs.len();

    let mapping = Mapping {
        defaults: super::Defaults {
            category_id: Some(category_id),
            template_id,
            strategy_id,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut fx = journal_fx::FxCache::new();
    let mut fills = Vec::with_capacity(execs.len());
    let mut ids: Vec<String> = Vec::with_capacity(execs.len());
    for (i, e) in execs.iter().enumerate() {
        fills.push(fill_of(pool, &mut fx, e, i, category_id, strategy_id).await);
        ids.push(e.id.clone());
    }

    let folded = build::fold_fills(fills, &mapping, spot_only);
    let fx_misses = fx.misses().map(|(d, q)| (d, q.to_string())).collect();
    Ok(Folded {
        trades: folded.trades,
        errors: folded.errors,
        executions,
        ids,
        fx_misses,
    })
}

/// Break a tie between two fills stamped at the same instant.
///
/// Compared as numbers when both are: broker ids are usually counters, and "10" sorts
/// before "9" as text, which would hand FIFO the lots in the wrong order.
fn cmp_exec_id(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.trim().parse::<u128>(), b.trim().parse::<u128>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

/// One execution as a fill, with its fee brought into the trade's own currency.
async fn fill_of(
    pool: &PgPool,
    fx: &mut journal_fx::FxCache,
    e: &Execution,
    line: usize,
    category_id: Uuid,
    strategy_id: Option<Uuid>,
) -> Fill {
    let mut warnings: Vec<Warning> = Vec::new();
    let mut fees = e.fee.abs();
    let fee_ccy = e.fee_currency.trim().to_uppercase();
    let currency = e.currency.trim().to_uppercase();
    // A broker bills in the asset it chooses (Binance in BNB, IBKR in the account's base
    // currency); a trade carries one currency and nets its PnL in it.
    if !fee_ccy.is_empty() && !currency.is_empty() && fee_ccy != currency && fees != 0.0 {
        match fx.convert(pool, fees, &fee_ccy, &currency, e.at.date()).await {
            Ok(Some(converted)) => {
                warnings.push(Warning {
                    code: "fee_currency".into(),
                    message: format!("fee converted {fee_ccy} to {currency} ({})", e.at.date()),
                });
                fees = converted;
            }
            // Leaving the number as it stands would subtract BNB from a USDT position:
            // a cost we cannot express in the trade's currency is not a cost we can net,
            // so it is dropped and said out loud. The currency is queued as an FX task
            // (see `pull`), and a re-sync nets it once a rate exists.
            _ => {
                warnings.push(Warning {
                    code: "fee_currency".into(),
                    message: format!(
                        "{} {fee_ccy} of fees left out: no {fee_ccy} to {currency} rate for {}. \
                         Resolve it in the journal's FX tasks, then sync this period again",
                        crate::journal_import::fmt_qty(fees),
                        e.at.date()
                    ),
                });
                fees = 0.0;
            }
        }
    }
    Fill {
        line,
        at: Some(e.at),
        ticker: e.symbol.clone(),
        side: if e.side.eq_ignore_ascii_case("sell") { "short".into() } else { "long".into() },
        price: e.price,
        qty: e.qty.abs(),
        fees,
        hash_key: e.id.clone(),
        warnings,
        currency,
        asset_class: e.asset_class.clone(),
        unit_type: unit_type_for(&e.asset_class).into(),
        exchange: Some(e.venue.clone()).filter(|v| !v.is_empty()),
        category_id,
        strategy_id,
        signal_name: None,
        feedback: None,
        fields: serde_json::json!({}),
        signal: None,
        multiplier: if e.multiplier > 0.0 { e.multiplier } else { 1.0 },
    }
}

/// What one unit of this instrument is called in the journal.
fn unit_type_for(asset_class: &str) -> &'static str {
    match asset_class {
        "stock" | "etf" => "share",
        "future" | "option" => "contract",
        _ => "unit",
    }
}

/// The identity of an imported position: the account it came from and the execution that
/// opened it. Stable across pulls, and different from the hash a file import of the same
/// statement produces: the two sources dedupe against themselves, not against each other.
fn position_hash(account_id: Uuid, opening_exec: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("brk:{account_id}:{opening_exec}").as_bytes());
    format!("{:x}", h.finalize())
}

async fn default_category(pool: &PgPool) -> Result<Uuid> {
    let categories = journal::list_categories(pool).await?;
    categories
        .iter()
        .find(|c| c.is_default)
        .or_else(|| categories.first())
        .map(|c| c.id)
        .ok_or_else(|| anyhow!("the journal has no category to import into"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(id: &str, side: &str, qty: f64, price: f64, at: i64) -> Execution {
        Execution {
            id: id.into(),
            order_id: String::new(),
            at: OffsetDateTime::from_unix_timestamp(at).unwrap(),
            symbol: "BTCUSDT".into(),
            side: side.into(),
            qty,
            price,
            fee: 0.1,
            fee_currency: "USDT".into(),
            currency: "USDT".into(),
            asset_class: "crypto".into(),
            multiplier: 1.0,
            venue: "Binance".into(),
            account: String::new(),
        }
    }

    fn fills(execs: &[Execution]) -> Vec<Fill> {
        execs
            .iter()
            .enumerate()
            .map(|(i, e)| Fill {
                line: i,
                at: Some(e.at),
                ticker: e.symbol.clone(),
                side: if e.side == "sell" { "short".into() } else { "long".into() },
                price: e.price,
                qty: e.qty,
                fees: e.fee,
                hash_key: e.id.clone(),
                warnings: vec![],
                currency: e.currency.clone(),
                asset_class: e.asset_class.clone(),
                unit_type: "unit".into(),
                exchange: None,
                category_id: Uuid::nil(),
                strategy_id: None,
                signal_name: None,
                feedback: None,
                fields: serde_json::json!({}),
                signal: None,
                multiplier: 1.0,
            })
            .collect()
    }

    #[test]
    fn a_position_keeps_its_identity_when_it_closes_later() {
        let account = Uuid::new_v4();
        let mapping = Mapping::default();

        // First pull: one buy, still open.
        let open = vec![exec("e1", "buy", 1.0, 100.0, 1_700_000_000)];
        let built = build::fold_fills(fills(&open), &mapping, true);
        assert_eq!(built.trades.len(), 1);
        let first = position_hash(account, &open[built.trades[0].source_rows[0]].id);

        // Second pull, wider window: the same buy plus the sell that closed it.
        let both = vec![
            exec("e1", "buy", 1.0, 100.0, 1_700_000_000),
            exec("e2", "sell", 1.0, 110.0, 1_700_100_000),
        ];
        let built = build::fold_fills(fills(&both), &mapping, true);
        assert_eq!(built.trades.len(), 1);
        let second = position_hash(account, &both[built.trades[0].source_rows[0]].id);

        assert_eq!(first, second, "the closed position must land on the open one's row");
    }

    #[test]
    fn a_cash_account_never_invents_a_short() {
        let mapping = Mapping::default();
        let sold = vec![exec("e9", "sell", 0.5, 120.0, 1_700_200_000)];
        let built = build::fold_fills(fills(&sold), &mapping, true);
        assert!(built.trades.is_empty());
        assert_eq!(built.errors.len(), 1);
        assert!(built.errors[0].message.contains("widen the period"));

        // The same fills on a margin account are a real short.
        let built = build::fold_fills(fills(&sold), &mapping, false);
        assert_eq!(built.trades.len(), 1);
        assert_eq!(built.trades[0].input.side, "short");
    }

    /// Ids are counters: "10" sorts before "9" as text, which would hand FIFO the lots
    /// in the wrong order when two fills share a timestamp.
    #[test]
    fn a_tie_is_broken_by_the_number_not_the_string() {
        use std::cmp::Ordering;
        assert_eq!(cmp_exec_id("9", "10"), Ordering::Less);
        assert_eq!(cmp_exec_id("10", "9"), Ordering::Greater);
        // Non-numeric ids (IBKR's ibExecID) still compare, just as text.
        assert_eq!(cmp_exec_id("0001f4.65a.01", "0001f4.65a.02"), Ordering::Less);
    }

    #[test]
    fn two_accounts_never_share_a_hash() {
        assert_ne!(
            position_hash(Uuid::nil(), "e1"),
            position_hash(Uuid::new_v4(), "e1")
        );
    }
}
