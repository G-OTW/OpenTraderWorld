//! Aligning a portfolio on a broker account's holdings.
//!
//! The other import reads a file of operations; this one reads a **balance sheet**. A
//! broker's spot API says what the account owns, not how it was acquired, so nothing here
//! reconstructs a trade history: what it writes is the one operation that makes the
//! portfolio agree with the broker, and it writes it only where the user said so.
//!
//! Three things it refuses to do on its own:
//!
//!   - **guess which asset a symbol is.** "BTC" on an exchange is a string; an asset here
//!     is a price source (provider + id). A symbol already held answers itself, everything
//!     else is asked, exactly as the file import asks it.
//!   - **invent a cost basis.** Binance, Kraken and Coinbase publish a quantity and nothing
//!     else. Interactive Brokers publishes its cost basis and that is used. Everywhere else
//!     the line says so and the price is the user's to enter, defaulting to today's, which
//!     is the one price nobody can mistake for a claim about the past.
//!   - **align a line nobody asked to align.** A difference between the broker and the
//!     ledger is reported per line and acted on per line: it is a transaction that happened
//!     on a day this import does not know, and writing it silently would date it wrong
//!     without telling anyone.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use otw_store::{portfolios as store, portfolios_import as istore};

use super::{resolve_choice, sym_key, SymbolChoice, SymbolInfo};
use crate::brokers::Holding;
use crate::portfolios::prices;

/// Below this many units a holding counts as dust, not a position: exchanges leave
/// fractions of a cent behind after a conversion and they are not worth a ledger row.
const DUST: f64 = 1e-12;

/// How far the ledger may sit from the broker before the line is called a difference.
/// Relative, because 0.00000001 BTC and 0.01 USDC are not the same kind of rounding.
const TOLERANCE: f64 = 1e-9;

// ── Preview ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub account_id: Uuid,
    /// What the user has already decided each symbol is, keyed on the upper-cased symbol.
    /// A symbol the portfolio already holds resolves itself and is not in here.
    #[serde(default)]
    pub symbols: BTreeMap<String, SymbolChoice>,
}

/// One line of the broker's balance sheet, put next to the portfolio's own books.
#[derive(Debug, Serialize)]
pub struct HoldingLine {
    /// The asset as the broker spells it. The identity everything else keys on.
    pub source: String,
    pub name: String,
    /// Units the broker reports.
    pub broker_qty: f64,
    /// Units the ledger says are held. 0 for an asset the portfolio does not have yet.
    pub held_qty: f64,
    /// `broker_qty - held_qty`: what an alignment would write, as a buy when positive and
    /// a sell when negative.
    pub delta: f64,
    pub side: String,
    pub asset_class: String,
    pub venue: String,
    /// matched | chosen | new | skip | unresolved, as the file import means them.
    pub state: &'static str,
    pub asset_id: Option<Uuid>,
    /// Best known price for the operation, in the asset's currency.
    pub price: Option<f64>,
    /// Where `price` came from: `broker` (a real cost basis), `spot` (today's price, a
    /// default and not a claim), or `none` (nothing to go on, the user must type one).
    pub price_source: &'static str,
    pub currency: String,
}

impl HoldingLine {
    /// Whether this line would write an operation as it stands.
    fn writes(&self) -> bool {
        self.delta.abs() > DUST && matches!(self.state, "matched" | "chosen" | "new")
    }
}

#[derive(Debug, Serialize)]
pub struct Preview {
    /// The account's name, for the header.
    pub account: String,
    pub broker: String,
    pub lines: Vec<HoldingLine>,
    /// The same symbols in the shape the shared symbol picker reads.
    pub symbols: Vec<SymbolInfo>,
    /// Lines whose price is only today's price, so the modal can say it once instead of
    /// per row.
    pub priced_at_spot: usize,
    /// Holdings that match nothing yet and so cannot be imported until they are answered.
    pub unresolved: usize,
}

/// Read the account, put it beside the portfolio, and say what an import would do.
/// Writes nothing.
pub async fn preview(
    pool: &PgPool,
    portfolio_id: Uuid,
    account_name: &str,
    broker: &str,
    holdings: &[Holding],
    req: &PreviewRequest,
) -> anyhow::Result<Preview> {
    let pf = store::get_portfolio(pool, portfolio_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("portfolio not found"))?;
    let assets = store::list_assets(pool, portfolio_id).await?;
    let views = store::asset_views(pool, &pf).await?;

    let mut lines = Vec::new();
    let mut symbols = Vec::new();
    for h in holdings {
        if h.qty.abs() <= DUST {
            continue;
        }
        let key = sym_key(&h.symbol);
        let r = resolve_choice(&h.symbol, req.symbols.get(&key), &assets);
        let view = r
            .asset_id
            .and_then(|id| views.iter().find(|v| v.asset.id == id));
        let held = view.map(|v| v.quantity).unwrap_or(0.0);
        // A short is negative units in the ledger: the broker reports the size and the
        // side separately, and the two have to be put back together before comparing.
        let broker_qty = if h.side == "short" { -h.qty } else { h.qty };

        let (price, price_source) = match h.avg_price.filter(|p| *p > 0.0) {
            Some(p) => (Some(p), "broker"),
            // No broker cost basis: today's price is a default the user can overwrite,
            // never a statement about what was paid.
            None => match view.and_then(|v| v.price).filter(|p| *p > 0.0) {
                Some(p) => (Some(p), "spot"),
                None => (None, "none"),
            },
        };

        symbols.push(SymbolInfo {
            source: h.symbol.clone(),
            rows: 1,
            state: r.state,
            asset_id: r.asset_id,
            symbol: r.symbol.clone(),
            name: r
                .name
                .clone()
                .filter(|n| !n.is_empty())
                .or_else(|| Some(h.name.clone()).filter(|n| !n.is_empty())),
            asset_class: r
                .asset_class
                .clone()
                .or_else(|| Some(h.asset_class.clone())),
            provider: r.provider.clone(),
            provider_id: r.provider_id.clone(),
            currency: r.currency.clone(),
            file_currency: Some(h.currency.clone()).filter(|c| !c.is_empty()),
            // Only Interactive Brokers names a currency on a holding. When it names one the
            // asset is not kept in, the price about to be entered is in the wrong money and
            // the line has to say so rather than let it through.
            currency_clash: r
                .currency
                .as_deref()
                .filter(|c| !c.is_empty() && !h.currency.is_empty() && *c != h.currency)
                .map(str::to_string),
        });

        lines.push(HoldingLine {
            source: h.symbol.clone(),
            name: h.name.clone(),
            broker_qty,
            held_qty: held,
            delta: broker_qty - held,
            side: h.side.clone(),
            asset_class: r
                .asset_class
                .clone()
                .unwrap_or_else(|| h.asset_class.clone()),
            venue: h.venue.clone(),
            state: r.state,
            asset_id: r.asset_id,
            price,
            price_source,
            currency: r
                .currency
                .clone()
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| pf.currency.clone()),
        });
    }

    let priced_at_spot = lines
        .iter()
        .filter(|l| l.writes() && l.price_source != "broker")
        .count();
    let unresolved = lines.iter().filter(|l| l.state == "unresolved").count();
    Ok(Preview {
        account: account_name.to_string(),
        broker: broker.to_string(),
        lines,
        symbols,
        priced_at_spot,
        unresolved,
    })
}

// ── Commit ───────────────────────────────────────────────────────────────────

/// What to do with one line of the balance sheet.
#[derive(Debug, Deserialize)]
pub struct LineChoice {
    /// The broker's own spelling, as the preview returned it.
    pub symbol: String,
    /// Price per unit for the operation, in the asset's currency. Required to write.
    #[serde(default)]
    pub price: Option<f64>,
    /// Leave this line alone. A line the user chose not to align.
    #[serde(default)]
    pub skip: bool,
}

#[derive(Debug, Deserialize)]
pub struct CommitRequest {
    pub account_id: Uuid,
    /// Per-symbol decisions, keyed by the broker's spelling.
    #[serde(default)]
    pub lines: Vec<LineChoice>,
    /// What each unanswered symbol is. Same document as the file import's.
    #[serde(default)]
    pub symbols: BTreeMap<String, SymbolChoice>,
    /// Day the alignment operations are dated. Today when absent.
    #[serde(default)]
    pub op_date: Option<Date>,
}

#[derive(Debug, Serialize)]
pub struct CommitReport {
    pub batch_id: Uuid,
    /// Operations written.
    pub imported: usize,
    /// Operations an earlier identical import had already written.
    pub duplicates: usize,
    /// Lines left alone: skipped, unresolved, or already in agreement.
    pub skipped: usize,
    pub assets_created: usize,
    /// Lines that could not be written and why.
    pub errors: Vec<String>,
}

/// Write the alignment. `holdings` is what the broker answers **now**, read again rather
/// than taken from the preview: the number that lands in the ledger is the broker's, not
/// one that travelled through a form.
pub async fn commit(
    pool: &PgPool,
    portfolio_id: Uuid,
    account_id: Uuid,
    account_name: &str,
    holdings: &[Holding],
    req: &CommitRequest,
) -> anyhow::Result<CommitReport> {
    let pf = store::get_portfolio(pool, portfolio_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("portfolio not found"))?;
    let assets = store::list_assets(pool, portfolio_id).await?;
    let views = store::asset_views(pool, &pf).await?;
    let op_date = req
        .op_date
        .unwrap_or_else(|| OffsetDateTime::now_utc().date());
    let choices: BTreeMap<String, &LineChoice> =
        req.lines.iter().map(|l| (sym_key(&l.symbol), l)).collect();

    let mut report = CommitReport {
        batch_id: Uuid::nil(),
        imported: 0,
        duplicates: 0,
        skipped: 0,
        assets_created: 0,
        errors: Vec::new(),
    };

    // Resolve everything before writing anything: an import that creates half its assets
    // and then stops has already changed the portfolio for nothing.
    let mut plan: Vec<(&Holding, &LineChoice, super::Resolution)> = Vec::new();
    for h in holdings {
        let key = sym_key(&h.symbol);
        let Some(choice) = choices.get(&key) else {
            report.skipped += 1;
            continue;
        };
        if choice.skip {
            report.skipped += 1;
            continue;
        }
        let r = resolve_choice(&h.symbol, req.symbols.get(&key), &assets);
        if !r.writes() {
            report.skipped += 1;
            continue;
        }
        plan.push((h, choice, r));
    }

    let mut created: BTreeMap<String, Uuid> = BTreeMap::new();
    for (h, _, r) in &plan {
        if r.state != "new" || created.contains_key(&sym_key(&h.symbol)) {
            continue;
        }
        let provider = r.provider.clone().unwrap_or_default();
        if !super::PROVIDERS.contains(&provider.as_str()) {
            anyhow::bail!("unknown price source \"{provider}\"");
        }
        let provider_id = r.provider_id.clone().unwrap_or_default();
        anyhow::ensure!(
            !provider_id.trim().is_empty(),
            "a new asset needs a price source id"
        );
        let asset = store::add_asset(
            pool,
            portfolio_id,
            r.asset_class.as_deref().unwrap_or("crypto"),
            &provider,
            provider_id.trim(),
            r.symbol.clone().unwrap_or_else(|| h.symbol.clone()).trim(),
            r.name.clone().unwrap_or_else(|| h.name.clone()).trim(),
            r.currency.as_deref().unwrap_or(&pf.currency),
        )
        .await?;
        if let Err(e) = prices::price_new_asset(pool, &asset).await {
            tracing::warn!("initial price for {} failed: {e:#}", asset.symbol);
        }
        created.insert(sym_key(&h.symbol), asset.id);
        report.assets_created += 1;
    }

    let batch = istore::add_broker_batch(
        pool,
        portfolio_id,
        account_id,
        account_name,
        plan.len() as i32,
    )
    .await?;
    report.batch_id = batch;

    for (h, choice, r) in &plan {
        let key = sym_key(&h.symbol);
        let Some(asset_id) = r.asset_id.or_else(|| created.get(&key).copied()) else {
            report.skipped += 1;
            continue;
        };
        let held = views
            .iter()
            .find(|v| v.asset.id == asset_id)
            .map(|v| v.quantity)
            .unwrap_or(0.0);
        let broker_qty = if h.side == "short" { -h.qty } else { h.qty };
        let delta = broker_qty - held;
        // Already in agreement: there is nothing to write, and writing a zero-unit
        // operation to record that would be a row about nothing.
        if delta.abs() <= held.abs().max(1.0) * TOLERANCE || delta.abs() <= DUST {
            report.skipped += 1;
            continue;
        }
        let Some(price) = choice.price.filter(|p| *p >= 0.0) else {
            report.errors.push(format!(
                "{}: no price for the {} units to file. Enter one, or leave the line out",
                h.symbol,
                fmt_qty(delta.abs())
            ));
            continue;
        };
        let side = if delta > 0.0 { "buy" } else { "sell" };
        let hash = row_hash(account_id, &h.symbol, op_date, side, delta.abs());
        match istore::add_imported_operation(
            pool,
            portfolio_id,
            Some(asset_id),
            side,
            op_date,
            delta.abs(),
            price,
            0.0,
            &format!("{account_name} sync"),
            None,
            batch,
            &hash,
        )
        .await
        {
            Ok(true) => report.imported += 1,
            Ok(false) => report.duplicates += 1,
            Err(e) => report.errors.push(format!("{}: {e:#}", h.symbol)),
        }
    }

    istore::finish_batch(
        pool,
        batch,
        report.imported as i32,
        report.duplicates as i32,
        report.errors.len() as i32,
        report.assets_created as i32,
    )
    .await?;
    istore::set_broker_sync(pool, portfolio_id, account_id).await?;
    Ok(report)
}

/// The identity a re-run deduplicates on: the same account aligning the same asset the
/// same way on the same day is one operation, not two. The quantity is in it because a
/// second, different move on the same day is a different fact.
fn row_hash(account_id: Uuid, symbol: &str, date: Date, side: &str, qty: f64) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(
        format!(
            "brkpf:{account_id}:{}:{date}:{side}:{qty:.12}",
            sym_key(symbol)
        )
        .as_bytes(),
    );
    format!("{:x}", h.finalize())
}

fn fmt_qty(q: f64) -> String {
    let s = format!("{q:.8}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() {
        "0".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn the_same_alignment_hashes_the_same_and_a_different_one_does_not() {
        let d = Date::from_calendar_date(2026, time::Month::September, 18).unwrap();
        let a = row_hash(uid(1), "BTC", d, "buy", 0.1);
        assert_eq!(
            a,
            row_hash(uid(1), "btc", d, "buy", 0.1),
            "case is not an identity"
        );
        assert_ne!(
            a,
            row_hash(uid(1), "BTC", d, "buy", 0.2),
            "a different size is a different fact"
        );
        assert_ne!(a, row_hash(uid(1), "BTC", d, "sell", 0.1));
        assert_ne!(
            a,
            row_hash(uid(2), "BTC", d, "buy", 0.1),
            "another account is another book"
        );
    }

    #[test]
    fn a_quantity_reads_without_scientific_notation_or_trailing_zeros() {
        assert_eq!(fmt_qty(0.1), "0.1");
        assert_eq!(fmt_qty(1.0), "1");
        assert_eq!(fmt_qty(0.00000001), "0.00000001");
    }
}
