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
//!
//! **A fill says buy or sell, never open or close.** The fold decides from what it has
//! seen open in the window, which is only right when the window starts flat. Two checks
//! catch the windows that do not: on an account that reports its positions, the fills
//! must explain what is held now (held − net fills = what was already there), and on a
//! cash account a sell must find something to close. An instrument failing either is a
//! [`Conflict`]: it is held back whole, the rest of the sync goes through, and the user
//! answers once (a position opened before: describe it; the fills are right: trust them;
//! or leave the instrument out). The answers are kept on the journal's sync and replayed
//! by every later pull, manual or scheduled.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::brokers::BrokerRow;
use otw_store::journal_import::{self as store, SyncOutcome};
use otw_store::{jobs, journal, journal_fx};

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
    /// Instruments held back until the user answers.
    pub conflicts: Vec<Conflict>,
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
    /// Instruments held back until the user answers.
    pub conflicts: usize,
    /// Of those, the ones this pull found for the first time.
    pub new_conflicts: usize,
}

/// A position opened before any pull, as the user described it when answering a
/// conflict. It stands in for the history the broker never sent: fills of the symbol
/// before `cutoff` are dropped and the seed opens the position instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Seed {
    pub id: Uuid,
    pub symbol: String,
    /// long | short
    pub side: String,
    pub qty: f64,
    pub price: f64,
    #[serde(with = "time::serde::rfc3339")]
    pub entry_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub cutoff: OffsetDateTime,
    pub currency: String,
    pub asset_class: String,
    pub multiplier: f64,
}

/// An instrument the sync could not place on its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub symbol: String,
    /// `unexplained_position`: the fills do not add up to what the account holds now.
    /// `sell_without_open`: a cash account sold something the window never bought.
    pub kind: String,
    /// Units with no opening in the window, signed (a long is positive).
    pub qty: f64,
    /// Fills of the instrument in the window.
    pub fills: usize,
    #[serde(with = "time::serde::rfc3339::option")]
    pub first_fill_at: Option<OffsetDateTime>,
    /// Start of the window it was found in: a position the user describes takes over
    /// from there.
    #[serde(with = "time::serde::rfc3339")]
    pub window_from: OffsetDateTime,
    /// Latest fill price, a default for the price the user is asked.
    pub last_price: f64,
    pub currency: String,
    pub asset_class: String,
    pub multiplier: f64,
}

/// What the journal's past answers and the account's positions say about a window.
#[derive(Default)]
pub(crate) struct Rules {
    pub seeds: Vec<Seed>,
    /// Upper-cased symbols taken as they fold.
    pub trusted: HashSet<String>,
    /// Upper-cased symbols left out.
    pub excluded: HashSet<String>,
    /// Net signed quantity the account holds now, per upper-cased symbol.
    pub snapshot: Option<HashMap<String, f64>>,
    /// Hold an instrument back whole when it conflicts. The tax module folds without it:
    /// it reports, it never writes, so a partial answer is still an answer there.
    pub strict: bool,
}

impl Rules {
    fn of(sync: &store::BrokerSync) -> Self {
        let upper = |v: &serde_json::Value| -> HashSet<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(|s| s.trim().to_uppercase())
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            seeds: serde_json::from_value(sync.seeds.clone()).unwrap_or_default(),
            trusted: upper(&sync.trusted),
            excluded: upper(&sync.excluded),
            snapshot: None,
            strict: true,
        }
    }
}

/// A pull, built but not written.
struct Built {
    trades: Vec<build::BuiltTrade>,
    errors: Vec<RowError>,
    executions: usize,
    category_id: Uuid,
    conflicts: Vec<Conflict>,
    /// Symbols looked at and found in order, whose old conflicts can go.
    settled: Vec<String>,
    /// Opening of the oldest position still open, where the next pull has to start.
    open_since: Option<OffsetDateTime>,
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
        conflicts: built.conflicts,
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

    let mut new_conflicts = 0;
    for c in &built.conflicts {
        let details = serde_json::to_value(c).unwrap_or_default();
        if jobs::upsert_conflict(pool, MODULE, built.category_id, account.id, &c.symbol, &c.kind, &details)
            .await?
        {
            new_conflicts += 1;
        }
    }
    jobs::clear_conflicts(pool, MODULE, built.category_id, account.id, &built.settled).await?;

    // The next pull starts where a position still open began, or where a conflict still
    // waiting was found: a later start would see only the end of either.
    let waiting = jobs::list_conflicts(pool, MODULE, Some(built.category_id))
        .await?
        .into_iter()
        .filter(|c| c.account_id == account.id)
        .filter_map(|c| serde_json::from_value::<Conflict>(c.details).ok())
        .map(|c| c.window_from)
        .min();
    let resume_from = [built.open_since, waiting, Some(req.to)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(req.to);

    let symbols = serde_json::to_value(&req.symbols).unwrap_or_else(|_| serde_json::json!([]));
    store::set_broker_sync(
        pool,
        built.category_id,
        account.id,
        &store::SyncMemo {
            symbols: &symbols,
            from: req.from,
            to: req.to,
            allow_short: req.allow_short,
            strategy_id: req.strategy_id,
            template_id: req.template_id,
            resume_from,
        },
    )
    .await?;

    Ok(SyncReport {
        batch_id: batch,
        imported,
        updated,
        duplicates,
        failed,
        executions: built.executions,
        conflicts: built.conflicts.len(),
        new_conflicts,
    })
}

/// The broker module id a journal sync writes conflicts under.
pub const MODULE: &str = "journal";

// ── Conflicts and schedule ───────────────────────────────────────────────────

/// The user's answer to a conflict.
#[derive(Debug, Deserialize)]
pub struct Answer {
    /// `closing`: the fills close a position opened before, described by `price` and
    /// `entry_at`. `opening`: the fills are right as they fold. `ignore`: leave the
    /// instrument out of every later sync.
    pub action: String,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub entry_at: Option<OffsetDateTime>,
}

/// Record the answer on the journal's sync, so every later pull replays it, and pull the
/// instrument's window again next time.
pub async fn answer_conflict(pool: &PgPool, id: Uuid, answer: &Answer) -> Result<()> {
    let row = jobs::get_conflict(pool, id)
        .await?
        .filter(|c| c.module == MODULE)
        .ok_or_else(|| anyhow!("conflict not found"))?;
    let c: Conflict = serde_json::from_value(row.details.clone())
        .map_err(|e| anyhow!("unreadable conflict: {e}"))?;
    let sync = store::get_broker_sync(pool, row.target_id)
        .await?
        .filter(|s| s.account_id == Some(row.account_id))
        .ok_or_else(|| anyhow!("this journal is no longer synced from that account"))?;
    let mut seeds: Vec<Seed> = serde_json::from_value(sync.seeds.clone()).unwrap_or_default();
    let mut trusted: Vec<String> = serde_json::from_value(sync.trusted.clone()).unwrap_or_default();
    let mut excluded: Vec<String> =
        serde_json::from_value(sync.excluded.clone()).unwrap_or_default();
    let sym = c.symbol.trim().to_uppercase();

    match answer.action.as_str() {
        "closing" => {
            let price = answer
                .price
                .filter(|p| p.is_finite() && *p > 0.0)
                .ok_or_else(|| anyhow!("enter the price the position was opened at"))?;
            let entry_at = answer
                .entry_at
                .unwrap_or(c.window_from - time::Duration::seconds(1));
            if entry_at > c.window_from {
                return Err(anyhow!(
                    "the position was open before {}: its opening date cannot be later",
                    c.window_from.date()
                ));
            }
            seeds.push(Seed {
                id: Uuid::new_v4(),
                symbol: c.symbol.clone(),
                side: if c.qty < 0.0 { "short".into() } else { "long".into() },
                qty: c.qty.abs(),
                price,
                entry_at,
                cutoff: c.window_from,
                currency: c.currency.clone(),
                asset_class: c.asset_class.clone(),
                multiplier: c.multiplier,
            });
        }
        "opening" => {
            if c.kind != "unexplained_position" {
                return Err(anyhow!(
                    "a cash account cannot sell what it does not hold. Describe the position \
                     the sell closed, or leave {} out",
                    c.symbol
                ));
            }
            if !trusted.contains(&sym) {
                trusted.push(sym);
            }
        }
        "ignore" => {
            if !excluded.contains(&sym) {
                excluded.push(sym);
            }
        }
        other => return Err(anyhow!("unknown answer \"{other}\"")),
    }

    store::set_broker_rules(
        pool,
        row.target_id,
        &serde_json::to_value(&seeds)?,
        &serde_json::to_value(&trusted)?,
        &serde_json::to_value(&excluded)?,
        Some(c.window_from),
    )
    .await?;
    jobs::delete_conflict(pool, id).await?;
    Ok(())
}

/// Take back a past answer: a seed by its id, a trusted or excluded symbol by name.
pub async fn forget_rule(pool: &PgPool, category_id: Uuid, kind: &str, value: &str) -> Result<()> {
    let sync = store::get_broker_sync(pool, category_id)
        .await?
        .ok_or_else(|| anyhow!("this journal has no broker sync"))?;
    let mut seeds: Vec<Seed> = serde_json::from_value(sync.seeds.clone()).unwrap_or_default();
    let mut trusted: Vec<String> = serde_json::from_value(sync.trusted.clone()).unwrap_or_default();
    let mut excluded: Vec<String> =
        serde_json::from_value(sync.excluded.clone()).unwrap_or_default();
    let v = value.trim().to_uppercase();
    match kind {
        "seed" => seeds.retain(|s| s.id.to_string() != value.trim()),
        "trusted" => trusted.retain(|s| *s != v),
        "excluded" => excluded.retain(|s| *s != v),
        other => return Err(anyhow!("unknown rule \"{other}\"")),
    }
    store::set_broker_rules(
        pool,
        category_id,
        &serde_json::to_value(&seeds)?,
        &serde_json::to_value(&trusted)?,
        &serde_json::to_value(&excluded)?,
        None,
    )
    .await?;
    Ok(())
}

/// One scheduled pass: pull from where the last one left off up to now, and write it.
pub async fn run_scheduled(
    pool: &PgPool,
    cipher: &otw_store::crypto::SecretCipher,
    sync: &store::BrokerSync,
) -> Result<SyncReport> {
    let account_id = sync
        .account_id
        .ok_or_else(|| anyhow!("the broker account was deleted"))?;
    let (connector, settings, account) =
        crate::brokers_api::resolve_for_job(pool, cipher, account_id, MODULE).await?;
    let to = OffsetDateTime::now_utc();
    let from = sync
        .resume_from
        .or(sync.period_to)
        .ok_or_else(|| anyhow!("run a first sync by hand from the journal's import page"))?
        .min(to - time::Duration::minutes(1));
    let req = SyncRequest {
        account_id,
        category_id: Some(sync.category_id),
        from,
        to,
        symbols: serde_json::from_value(sync.symbols.clone()).unwrap_or_default(),
        allow_short: sync.allow_short,
        strategy_id: sync.strategy_id,
        template_id: sync.template_id,
    };
    if let Err(e) = crate::brokers::check_window(connector.capability(), from) {
        return Err(e.context(format!(
            "the next pull starts on {}, where a position still open or a conflict still \
             waiting holds it",
            from.date()
        )));
    }
    commit(pool, connector.as_ref(), &settings, &account, &req).await
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
    let mut rules = match store::get_broker_sync(pool, category_id).await? {
        // Answers belong to the account they were given for.
        Some(sync) if sync.account_id == Some(account.id) => Rules::of(&sync),
        _ => Rules {
            strict: true,
            ..Default::default()
        },
    };
    rules.snapshot = snapshot(connector, settings, req.to).await?;

    let execs = crate::brokers::cache::executions(account.id, connector, settings, &q).await?;
    let mut folded = fold_execs(
        pool,
        execs,
        &q,
        category_id,
        req.strategy_id,
        req.template_id,
        spot_only,
        &rules,
    )
    .await;
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
    let mut open_since: Option<OffsetDateTime> = None;
    for t in &mut folded.trades {
        // The opening fill is the identity: a position that gains an exit tomorrow is the
        // same position, and must land on the row it already has.
        let opener = t
            .source_rows
            .first()
            .and_then(|i| ids.get(*i))
            .cloned()
            .unwrap_or_else(|| t.row_hash.clone());
        if still_open(&t.input) {
            // A seeded position starts where the broker's history takes over, not on the
            // day the user said it was bought: that day may be years past its archive.
            let since = match opener.strip_prefix("seed:") {
                Some(id) => rules
                    .seeds
                    .iter()
                    .find(|s| s.id.to_string() == id)
                    .map(|s| s.cutoff),
                None => t.input.entry_at,
            };
            open_since = [open_since, since].into_iter().flatten().min();
        }
        t.row_hash = position_hash(account.id, &opener);
    }
    Ok(Built {
        trades: folded.trades,
        errors: folded.errors,
        executions,
        category_id,
        conflicts: folded.conflicts,
        settled: folded.settled,
        open_since,
    })
}

/// Whether a built position still has units open.
fn still_open(t: &otw_store::journal::TradeInput) -> bool {
    let bought: f64 = t.entries.iter().map(|l| l.qty).sum();
    let sold: f64 = t.exits.iter().map(|l| l.qty).sum();
    bought - sold > 1e-9
}

/// What the account holds now, per symbol, when that can be compared with the fills.
///
/// Only when the window runs to now (a past `to` would compare today's book with an old
/// window), on an account that reports positions and can hold a short (a cash account's
/// "positions" are balances: coins deposited, not traded), and whose fills are live (a
/// broker-saved report such as IBKR Flex lags a day, so today's fills would be missing
/// from the sum and every instrument traded today would look unexplained).
async fn snapshot(
    connector: &dyn Broker,
    settings: &HashMap<String, String>,
    to: OffsetDateTime,
) -> Result<Option<HashMap<String, f64>>> {
    let cap = connector.capability();
    let live = to >= OffsetDateTime::now_utc() - time::Duration::minutes(10);
    if !live || !cap.positions || cap.spot_only || cap.window_from_broker {
        return Ok(None);
    }
    let http = crate::brokers::client()?;
    let positions = connector.positions(&http, settings).await.map_err(|e| {
        anyhow!(
            "could not read the open positions the fills are checked against: {e:#}"
        )
    })?;
    let mut held: HashMap<String, f64> = HashMap::new();
    for p in positions {
        let signed = if p.side == "short" { -p.qty.abs() } else { p.qty.abs() };
        *held.entry(p.symbol.trim().to_uppercase()).or_insert(0.0) += signed;
    }
    Ok(Some(held))
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
    /// Instruments held back (strict rules only).
    pub conflicts: Vec<Conflict>,
    /// Symbols that folded without a conflict.
    pub settled: Vec<String>,
}

/// Fold a window of fills under `rules`: exclusions out, seeds in, then the positions
/// check, then the fold, then (strict) the cash account's orphan sells.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fold_execs(
    pool: &PgPool,
    mut execs: Vec<Execution>,
    q: &ExecQuery,
    category_id: Uuid,
    strategy_id: Option<Uuid>,
    template_id: Option<Uuid>,
    spot_only: bool,
    rules: &Rules,
) -> Folded {
    let executions = execs.len();
    let key = |s: &str| s.trim().to_uppercase();
    execs.retain(|e| !rules.excluded.contains(&key(&e.symbol)));

    // A seed replaces the history before its cutoff, so a later seed of the same symbol
    // (a second position opened outside the window) wins over an earlier one.
    let mut seeds: Vec<&Seed> = rules
        .seeds
        .iter()
        .filter(|s| s.cutoff >= q.from && s.entry_at <= q.to)
        .filter(|s| !rules.excluded.contains(&key(&s.symbol)))
        .collect();
    seeds.sort_by_key(|s| s.cutoff);
    for seed in seeds {
        let sym = key(&seed.symbol);
        execs.retain(|e| !(key(&e.symbol) == sym && e.at < seed.cutoff));
        execs.push(seed_exec(seed));
    }

    // Per instrument, what the window says, for the checks and for the conflicts.
    let mut per: BTreeMap<String, Vec<Execution>> = BTreeMap::new();
    for e in &execs {
        per.entry(key(&e.symbol)).or_default().push(e.clone());
    }
    let mut conflicts: Vec<Conflict> = Vec::new();
    if let (true, Some(held)) = (rules.strict, rules.snapshot.as_ref()) {
        for (sym, list) in &per {
            if rules.trusted.contains(sym) {
                continue;
            }
            let net: f64 = list.iter().map(signed_qty).sum();
            let gross: f64 = list.iter().map(|e| e.qty.abs()).sum();
            let before = held.get(sym).copied().unwrap_or(0.0) - net;
            if before.abs() > (gross * 1e-6).max(1e-9) {
                conflicts.push(conflict_of("unexplained_position", before, list, q.from));
            }
        }
    }
    let held_back: HashSet<String> = conflicts.iter().map(|c| key(&c.symbol)).collect();
    execs.retain(|e| !held_back.contains(&key(&e.symbol)));

    // Chronological per instrument, so the line number a fill carries is also its rank:
    // the position's first source row is the fill that opened it.
    execs.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then(a.at.cmp(&b.at))
            .then_with(|| cmp_exec_id(&a.id, &b.id))
    });

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

    let (mut folded, orphans) = build::fold_fills_tracked(fills, &mapping, spot_only);
    if rules.strict && !orphans.is_empty() {
        // The sells found nothing to close: the holding was bought before the window.
        // Whatever else the instrument folded into would change identity the day the
        // user describes that holding, so none of it is written until then.
        let mut orphaned: BTreeMap<String, f64> = BTreeMap::new();
        for o in &orphans {
            *orphaned.entry(key(&o.ticker)).or_insert(0.0) += o.qty;
        }
        for (sym, qty) in &orphaned {
            if let Some(list) = per.get(sym) {
                conflicts.push(conflict_of("sell_without_open", *qty, list, q.from));
            }
        }
        folded.trades.retain(|t| !orphaned.contains_key(&key(&t.input.ticker)));
    }
    let conflicted: HashSet<String> = conflicts.iter().map(|c| key(&c.symbol)).collect();
    let settled = per.keys().filter(|s| !conflicted.contains(*s)).cloned().collect();

    let fx_misses = fx.misses().map(|(d, q)| (d, q.to_string())).collect();
    Folded {
        trades: folded.trades,
        errors: folded.errors,
        executions,
        ids,
        fx_misses,
        conflicts,
        settled,
    }
}

/// A buy adds, a sell takes away.
fn signed_qty(e: &Execution) -> f64 {
    if e.side.eq_ignore_ascii_case("sell") {
        -e.qty.abs()
    } else {
        e.qty.abs()
    }
}

fn conflict_of(kind: &str, qty: f64, list: &[Execution], window_from: OffsetDateTime) -> Conflict {
    let first = list.iter().min_by_key(|e| e.at);
    let last = list.iter().max_by_key(|e| e.at);
    let sample = last.or(first);
    Conflict {
        symbol: sample.map(|e| e.symbol.clone()).unwrap_or_default(),
        kind: kind.into(),
        qty,
        fills: list.len(),
        first_fill_at: first.map(|e| e.at),
        window_from,
        last_price: last.map(|e| e.price).unwrap_or(0.0),
        currency: sample.map(|e| e.currency.trim().to_uppercase()).unwrap_or_default(),
        asset_class: sample.map(|e| e.asset_class.clone()).unwrap_or_default(),
        multiplier: sample
            .map(|e| e.multiplier)
            .filter(|m| *m > 0.0)
            .unwrap_or(1.0),
    }
}

/// A seed as the fill that opens its position.
fn seed_exec(seed: &Seed) -> Execution {
    Execution {
        id: format!("seed:{}", seed.id),
        order_id: String::new(),
        at: seed.entry_at,
        symbol: seed.symbol.clone(),
        side: if seed.side == "short" { "sell".into() } else { "buy".into() },
        qty: seed.qty.abs(),
        price: seed.price,
        fee: 0.0,
        fee_currency: seed.currency.clone(),
        currency: seed.currency.clone(),
        asset_class: seed.asset_class.clone(),
        multiplier: seed.multiplier,
        venue: String::new(),
        account: String::new(),
    }
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
    hex::encode(h.finalize())
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

    fn window() -> ExecQuery {
        ExecQuery {
            from: OffsetDateTime::from_unix_timestamp(1_699_000_000).unwrap(),
            to: OffsetDateTime::from_unix_timestamp(1_701_000_000).unwrap(),
            symbols: vec![],
        }
    }

    /// Fees here are billed in the trade's own currency, so the fold never reaches the
    /// pool for a rate: a lazy handle that never connects is enough.
    fn no_db() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap()
    }

    fn strict(snapshot: Option<&[(&str, f64)]>) -> Rules {
        Rules {
            snapshot: snapshot
                .map(|s| s.iter().map(|(k, v)| (k.to_string(), *v)).collect()),
            strict: true,
            ..Default::default()
        }
    }

    async fn fold(execs: Vec<Execution>, rules: &Rules, spot_only: bool) -> Folded {
        fold_execs(&no_db(), execs, &window(), Uuid::nil(), None, None, spot_only, rules).await
    }

    #[tokio::test]
    async fn a_sell_the_fills_cannot_explain_is_held_back_not_shorted() {
        // Margin account, nothing held now, one sell in the window: the sell closed a
        // long opened before, it did not open a short.
        let execs = vec![exec("e1", "sell", 1.0, 110.0, 1_700_000_000)];
        let f = fold(execs, &strict(Some(&[])), false).await;
        assert!(f.trades.is_empty());
        assert_eq!(f.conflicts.len(), 1);
        assert_eq!(f.conflicts[0].kind, "unexplained_position");
        assert!((f.conflicts[0].qty - 1.0).abs() < 1e-12, "a long of 1 was already open");
        assert!(f.settled.is_empty());
    }

    #[tokio::test]
    async fn fills_that_add_up_to_the_book_fold_as_they_are() {
        let execs = vec![exec("e1", "buy", 1.0, 100.0, 1_700_000_000)];
        let f = fold(execs, &strict(Some(&[("BTCUSDT", 1.0)])), false).await;
        assert!(f.conflicts.is_empty());
        assert_eq!(f.trades.len(), 1);
        assert_eq!(f.settled, vec!["BTCUSDT".to_string()]);
    }

    #[tokio::test]
    async fn a_seed_opens_the_position_the_broker_never_sent() {
        let q = window();
        let mut rules = strict(Some(&[]));
        rules.seeds.push(Seed {
            id: Uuid::nil(),
            symbol: "BTCUSDT".into(),
            side: "long".into(),
            qty: 1.0,
            price: 90.0,
            entry_at: q.from - time::Duration::days(30),
            cutoff: q.from,
            currency: "USDT".into(),
            asset_class: "crypto".into(),
            multiplier: 1.0,
        });
        let execs = vec![exec("e1", "sell", 1.0, 110.0, 1_700_000_000)];
        let f = fold(execs, &rules, false).await;
        assert!(f.conflicts.is_empty());
        assert_eq!(f.trades.len(), 1);
        let t = &f.trades[0].input;
        assert_eq!(t.side, "long");
        assert_eq!(t.entries[0].price, 90.0);
        assert!(!still_open(t));
        assert!(f.ids[f.trades[0].source_rows[0]].starts_with("seed:"));
    }

    #[tokio::test]
    async fn a_cash_account_holds_the_whole_instrument_back_on_an_orphan_sell() {
        let execs = vec![
            exec("e1", "sell", 0.5, 120.0, 1_700_000_000),
            exec("e2", "buy", 1.0, 100.0, 1_700_100_000),
        ];
        let f = fold(execs, &strict(None), true).await;
        assert!(f.trades.is_empty(), "the later buy would change identity once answered");
        assert_eq!(f.conflicts.len(), 1);
        assert_eq!(f.conflicts[0].kind, "sell_without_open");
        assert!((f.conflicts[0].qty - 0.5).abs() < 1e-12);

        // The tax module folds without strict rules and keeps what it can.
        let execs = vec![
            exec("e1", "sell", 0.5, 120.0, 1_700_000_000),
            exec("e2", "buy", 1.0, 100.0, 1_700_100_000),
        ];
        let f = fold(execs, &Rules::default(), true).await;
        assert_eq!(f.trades.len(), 1);
        assert!(f.conflicts.is_empty());
    }

    #[tokio::test]
    async fn trusted_and_excluded_symbols_skip_the_check() {
        let mut rules = strict(Some(&[]));
        rules.trusted.insert("BTCUSDT".into());
        let f = fold(vec![exec("e1", "sell", 1.0, 110.0, 1_700_000_000)], &rules, false).await;
        assert!(f.conflicts.is_empty());
        assert_eq!(f.trades[0].input.side, "short");

        let mut rules = strict(Some(&[]));
        rules.excluded.insert("BTCUSDT".into());
        let f = fold(vec![exec("e1", "sell", 1.0, 110.0, 1_700_000_000)], &rules, false).await;
        assert!(f.conflicts.is_empty() && f.trades.is_empty() && f.settled.is_empty());
    }

    #[test]
    fn two_accounts_never_share_a_hash() {
        assert_ne!(
            position_hash(Uuid::nil(), "e1"),
            position_hash(Uuid::new_v4(), "e1")
        );
    }
}
