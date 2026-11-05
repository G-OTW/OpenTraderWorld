//! Rebuilding the daily curve from the ledger and stored bars.
//!
//! The tracker's history starts the day the user opts into the daily job, so every measure
//! that reads a series is blank on a portfolio that has been kept for years. This module
//! fills it in, and it is deliberately the same three verbs the journal's enrichment uses,
//! in the same order:
//!
//! - [`coverage`] reads what the held instruments need against the histdata catalog and
//!   **writes and queues nothing**.
//! - [`sync`] queues the missing windows as ordinary histdata download jobs. No provider
//!   code, no new endpoint on that side; the page follows the batch on `/api/histdata/jobs`.
//! - [`rebuild`] walks the ledger day by day, marks each position on that day's close, and
//!   writes one snapshot per day.
//!
//! A rebuilt snapshot never overwrites a live one: a price fetched on the day is better
//! evidence than a close read back out of a daily bar months later.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata as hist;
use otw_store::journal_fx;
use otw_store::portfolios::{self as store, Portfolio};

use super::analytics::substrate::{MODULE, TIMEFRAME};

/// Bars kept in front of the first operation, so the first marked day is not the first bar.
const WARMUP_DAYS: i64 = 7;

// ── Coverage ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AssetCoverage {
    pub asset_id: Uuid,
    pub symbol: String,
    /// The bar ticker, empty when the user has not said what it is.
    pub hist_symbol: String,
    /// The currency the bars are quoted in, so the picker shows what is stored rather than
    /// defaulting every row to USD and offering to overwrite it.
    pub hist_currency: String,
    pub asset_class: String,
    /// `ok` | `needs_symbol` | `no_connector` | `missing` | `partial`
    pub status: &'static str,
    /// The last download failure for this ticker, in the worker's own words, with the
    /// provider that produced it. A capability matrix knows which asset *types* a provider
    /// serves and never which listings, so this is the only place "the broker could not
    /// fetch it" is actually said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_provider: Option<String>,
    pub from: String,
    pub to: String,
    /// Days of the span with no stored bar at all.
    pub missing_days: i64,
    pub bars: i64,
    pub provider: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub assets: Vec<AssetCoverage>,
    /// First operation of the whole ledger: the day a rebuild would start from.
    pub from: Option<String>,
    pub to: String,
    /// Assets that cannot be rebuilt until the user names their bar ticker.
    pub needs_symbol: usize,
    /// Assets no granted connector can download: the broker the portfolio is allowed to use
    /// does not carry that asset type. Counted apart from `missing` because pressing Sync
    /// again will never fix it.
    pub no_connector: usize,
    /// Assets whose last download attempt failed. The provider was willing, the fetch was not.
    pub failed: usize,
    pub missing: usize,
    /// The stored curve's own span, so the screen can say what a rebuild would add.
    pub curve_from: Option<String>,
    pub curve_to: Option<String>,
    pub curve_days: i64,
}

/// What the held instruments need. Writes nothing, queues nothing.
pub async fn coverage(pool: &PgPool, pf: &Portfolio) -> Result<Coverage> {
    let today = OffsetDateTime::now_utc().date();
    let assets = store::list_assets(pool, pf.id).await?;
    let firsts = first_operation_dates(pool, pf.id).await?;
    let ledger_start = firsts.values().min().copied();

    let granted = conn_store::list_for_module(pool, MODULE).await.unwrap_or_default();
    let preferred = granted.first().map(|c| c.provider.clone());
    // Serving an asset type is not the same as being granted. Alpaca is a fine connector to
    // hand the portfolios module and it still cannot download a Paris listing, and the old
    // code queued the job anyway: it failed in the worker, hours later, with nothing on this
    // screen saying why.
    let servable = |asset_type: &str| {
        granted
            .iter()
            .any(|c| crate::histdata::validate_request(&c.provider, asset_type, TIMEFRAME).is_ok())
    };

    let mut out = Vec::new();
    for a in &assets {
        let Some(start) = firsts.get(&a.id).copied() else {
            // An asset with no operation has no history to rebuild.
            continue;
        };
        let from = start - Duration::days(WARMUP_DAYS);
        let symbol = a.bar_symbol();
        if symbol.is_empty() {
            out.push(AssetCoverage {
                asset_id: a.id,
                symbol: a.symbol.clone(),
                hist_symbol: String::new(),
                hist_currency: a.hist_currency.clone(),
                asset_class: a.asset_class.clone(),
                status: "needs_symbol",
                last_error: None,
                error_provider: None,
                from: from.to_string(),
                to: today.to_string(),
                missing_days: (today - from).whole_days(),
                bars: 0,
                provider: None,
            });
            continue;
        }
        let ds = hist::find_dataset_any(pool, &asset_type_of(&a.asset_class), symbol, TIMEFRAME, preferred.as_deref())
            .await?;

        let ds = match ds {
            Some(d) => Some(d),
            // Filed under another asset type by whoever downloaded it. Bars are bars.
            None => hist::find_dataset_by_ticker(pool, symbol, TIMEFRAME).await?,
        };
        let asset_type = asset_type_of(&a.asset_class);
        let (status, bars, missing_days, provider) = match &ds {
            None => ("missing", 0, (today - from).whole_days(), None),
            Some(d) => {
                let covered_from = d.range_from.map(|t| t.date());
                let covered_to = d.range_to.map(|t| t.date());
                // Head and tail only. Holes inside are the bars' business, and asking the
                // bars for them (as the journal does) needs a window list this verb has no
                // reason to build: a rebuild carries the previous close forward anyway.
                let head = covered_from.map(|c| (c - from).whole_days().max(0)).unwrap_or_else(|| (today - from).whole_days());
                let tail = covered_to.map(|c| (today - c).whole_days().max(0)).unwrap_or(0);
                let missing = head + tail;
                (if missing > 3 { "partial" } else { "ok" }, d.bar_count, missing, Some(d.provider.clone()))
            }
        };
        // Said before the download is asked for, not after it fails. A row still short of
        // bars that nothing granted can fetch is a grant problem, and it reads as one.
        let status = if status != "ok" && !servable(&asset_type) { "no_connector" } else { status };
        out.push(AssetCoverage {
            asset_id: a.id,
            symbol: a.symbol.clone(),
            hist_symbol: symbol.to_string(),
            hist_currency: a.hist_currency.clone(),
            asset_class: a.asset_class.clone(),
            status,
            last_error: None,
            error_provider: None,
            from: from.to_string(),
            to: today.to_string(),
            missing_days,
            bars,
            provider,
        });
    }

    // One round trip for every row's last failure, after the walk rather than inside it.
    let tickers: Vec<String> =
        out.iter().filter(|a| a.status != "ok").map(|a| a.hist_symbol.clone()).collect();
    let failures = hist::last_job_errors(pool, &tickers, TIMEFRAME).await.unwrap_or_default();
    for a in out.iter_mut() {
        if let Some((provider, error)) = failures.get(&a.hist_symbol) {
            a.error_provider = Some(provider.clone());
            a.last_error = Some(error.clone());
        }
    }

    let span = store::snapshot_span(pool, pf.id).await?;
    Ok(Coverage {
        needs_symbol: out.iter().filter(|a| a.status == "needs_symbol").count(),
        no_connector: out.iter().filter(|a| a.status == "no_connector").count(),
        failed: out.iter().filter(|a| a.last_error.is_some()).count(),
        missing: out
            .iter()
            .filter(|a| matches!(a.status, "missing" | "partial" | "no_connector"))
            .count(),
        from: ledger_start.map(|d| d.to_string()),
        to: today.to_string(),
        curve_from: span.map(|(a, _)| a.to_string()),
        curve_to: span.map(|(_, b)| b.to_string()),
        curve_days: span.map(|(a, b)| (b - a).whole_days() + 1).unwrap_or(0),
        assets: out,
    })
}

// ── Sync ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncResult {
    pub batch_id: Option<Uuid>,
    pub queued: usize,
    /// Assets skipped because nobody has said what their bar ticker is. Named, not guessed.
    pub needs_symbol: Vec<String>,
    /// Assets skipped because no granted connector serves them.
    pub no_connector: Vec<String>,
    /// How many connectors the portfolios module is granted. Zero and "none of them carries
    /// this instrument" are two different problems with two different fixes, and the count is
    /// the only thing that tells them apart.
    pub connectors: usize,
}

/// Queue the missing windows as ordinary histdata jobs.
pub async fn sync(pool: &PgPool, pf: &Portfolio) -> Result<SyncResult> {
    let cov = coverage(pool, pf).await?;
    let granted = conn_store::list_for_module(pool, MODULE).await.unwrap_or_default();
    let mut out = SyncResult {
        batch_id: None,
        queued: 0,
        needs_symbol: Vec::new(),
        no_connector: Vec::new(),
        connectors: granted.len(),
    };
    if granted.is_empty() {
        out.no_connector = cov.assets.iter().map(|a| a.symbol.clone()).collect();
        return Ok(out);
    }
    let batch = Uuid::new_v4();
    for a in &cov.assets {
        if a.status == "needs_symbol" {
            out.needs_symbol.push(a.symbol.clone());
            continue;
        }
        if a.status == "ok" {
            continue;
        }
        let asset_type = asset_type_of(&a.asset_class);
        // The connector has to *serve* the instrument, not merely be granted. Taking the
        // first granted one queued a job Alpaca could never fill for a Paris listing, and the
        // only trace was a worker failure hours later.
        let Some(conn) = granted
            .iter()
            .find(|c| crate::histdata::validate_request(&c.provider, &asset_type, TIMEFRAME).is_ok())
        else {
            out.no_connector.push(a.symbol.clone());
            continue;
        };
        let from = parse_date(&a.from);
        let (Some(from), Some(to)) = (from, parse_date(&a.to)) else { continue };
        let dataset_id = hist::upsert_dataset(pool, &conn.provider, &asset_type, &a.hist_symbol, TIMEFRAME).await?;
        hist::enqueue_job(
            pool,
            &hist::NewJob {
                dataset_id,
                connector_id: Some(conn.id),
                provider: &conn.provider,
                asset_type: &asset_type,
                ticker: &a.hist_symbol,
                timeframe: TIMEFRAME,
                range_from: from.midnight().assume_utc(),
                range_to: to.midnight().assume_utc(),
                kind: "backfill",
                batch_id: Some(batch),
            },
        )
        .await?;
        out.queued += 1;
    }
    if out.queued > 0 {
        out.batch_id = Some(batch);
    }
    Ok(out)
}

// ── Rebuild ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RebuildResult {
    pub days: usize,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Days whose value is short because a held instrument had no close yet.
    pub incomplete_days: usize,
    pub uncovered: Vec<String>,
}

/// Walk the ledger day by day and write one snapshot per day.
///
/// Positions are marked at the day's close in the bar's own currency, then converted to the
/// portfolio's at that day's rate. Two conversions, both dated, because a 2019 position
/// marked at today's exchange rate is a currency bet nobody made.
pub async fn rebuild(pool: &PgPool, pf: &Portfolio, from: Option<Date>) -> Result<RebuildResult> {
    let today = OffsetDateTime::now_utc().date();
    let assets = store::list_assets(pool, pf.id).await?;
    let by_id: HashMap<Uuid, &store::Asset> = assets.iter().map(|a| (a.id, a)).collect();
    let ops = store::portfolio_operations_asc(pool, pf.id).await?;
    let Some(first_op) = ops.first().map(|o| o.op_date) else {
        return Ok(RebuildResult { days: 0, from: None, to: None, incomplete_days: 0, uncovered: Vec::new() });
    };
    let start = from.unwrap_or(first_op).max(first_op);

    // Closes per asset, by day. Read once; the walk only looks things up.
    let mut closes: HashMap<Uuid, BTreeMap<Date, f64>> = HashMap::new();
    let mut uncovered = Vec::new();
    for a in &assets {
        let symbol = a.bar_symbol();
        if symbol.is_empty() {
            uncovered.push(a.symbol.clone());
            continue;
        }
        let ds = match hist::find_dataset_any(pool, &asset_type_of(&a.asset_class), symbol, TIMEFRAME, None).await? {
            Some(d) => Some(d),
            None => hist::find_dataset_by_ticker(pool, symbol, TIMEFRAME).await?,
        };
        let Some(ds) = ds else {
            uncovered.push(a.symbol.clone());
            continue;
        };
        let bars = hist::read_bars(
            pool,
            ds.id,
            Some((start - Duration::days(WARMUP_DAYS)).midnight().assume_utc()),
            None,
            200_000,
        )
        .await?;
        if bars.is_empty() {
            uncovered.push(a.symbol.clone());
            continue;
        }
        closes.insert(a.id, bars.iter().map(|b| (b.ts.date(), b.close)).collect());
    }
    // The last day each series actually covers. Past it there is no close to carry forward,
    // only the last one repeated, and a repeated close is not a mark.
    let covered_to: HashMap<Uuid, Date> = closes
        .iter()
        .filter_map(|(id, m)| m.keys().next_back().map(|d| (*id, *d)))
        .collect();

    // Running book, in each asset's own operation currency for cost and units.
    let mut units: HashMap<Uuid, f64> = HashMap::new();
    let mut basis: HashMap<Uuid, f64> = HashMap::new();
    let mut cash = 0.0_f64;
    let mut last_close: HashMap<Uuid, f64> = HashMap::new();
    let mut fx = journal_fx::FxCache::new();
    let mut op_idx = 0usize;
    let mut days = 0usize;
    let mut incomplete = 0usize;
    // Same rule as `store::book`: no deposit and no withdrawal means there is no cash account
    // to debit. Without it the curve is worse than the book, because `flow` also stays zero:
    // every purchase reads as a market gain and the whole return series is the noise around a
    // net worth that hovers near nothing.
    let cash_tracked = ops.iter().any(|o| matches!(o.side.as_str(), "deposit" | "withdraw"));

    // Operations before the start day still shape the book: replay them into the running
    // position without writing a snapshot, or the first rebuilt day would show an empty book.
    // Movements of the days that could not be valued, waiting for the next day that can.
    // Dropping them would hand the curve a deposit it never saw: the day after would read the
    // new money as a market gain, which is the exact error the flow series exists to prevent.
    let (mut carry_flow, mut carry_income, mut carry_fees) = (0.0, 0.0, 0.0);
    let mut date = first_op;
    while date <= today {
        let (mut flow, mut income, mut fees) = (carry_flow, carry_income, carry_fees);
        while op_idx < ops.len() && ops[op_idx].op_date == date {
            let op = &ops[op_idx];
            op_idx += 1;
            let ccy = op
                .currency
                .clone()
                .or_else(|| op.asset_id.and_then(|id| by_id.get(&id).map(|a| a.currency.clone())))
                .unwrap_or_else(|| pf.currency.clone());
            let gross = op.quantity * op.price;
            let (gross_pf, fee_pf) = if ccy == pf.currency {
                (gross, op.fee)
            } else {
                (
                    fx.convert(pool, gross, &ccy, &pf.currency, date).await?.unwrap_or(gross),
                    fx.convert(pool, op.fee, &ccy, &pf.currency, date).await?.unwrap_or(op.fee),
                )
            };
            // What the row does to the cash account, decided before it is known whether
            // there is one. `trade` is the half that also moves market value, and it is the
            // only half that becomes external flow when there is no cash account.
            let (dcash, trade) = match op.side.as_str() {
                "buy" => {
                    let id = op.asset_id.unwrap_or_default();
                    *units.entry(id).or_default() += op.quantity;
                    *basis.entry(id).or_default() += gross_pf + fee_pf;
                    fees += fee_pf;
                    (-(gross_pf + fee_pf), true)
                }
                "sell" => {
                    let id = op.asset_id.unwrap_or_default();
                    let q = units.entry(id).or_default();
                    let b = basis.entry(id).or_default();
                    let avg = if *q > 0.0 { *b / *q } else { 0.0 };
                    let sold = op.quantity.min(q.max(0.0));
                    *q -= op.quantity;
                    *b -= sold * avg;
                    if *q <= 0.0 {
                        *q = 0.0;
                        *b = 0.0;
                    }
                    fees += fee_pf;
                    (gross_pf - fee_pf, true)
                }
                "deposit" => {
                    flow += gross_pf - fee_pf;
                    (gross_pf - fee_pf, false)
                }
                "withdraw" => {
                    flow -= gross_pf + fee_pf;
                    (-(gross_pf + fee_pf), false)
                }
                "dividend" | "interest" | "coupon" => {
                    income += gross_pf;
                    (gross_pf - fee_pf, false)
                }
                "fee" | "tax" => {
                    fees += gross_pf + fee_pf;
                    (-(gross_pf + fee_pf), false)
                }
                _ => (0.0, false),
            };
            if cash_tracked {
                cash += dcash;
            } else if trade {
                // The money for a buy came from outside the book and the proceeds of a sell
                // left it. Calling that a flow is what keeps the curve a return series
                // instead of a picture of the savings rate.
                flow -= dcash;
            }
        }

        if date >= start {
            let mut value = 0.0;
            let mut cost = 0.0;
            let mut short = false;
            for (id, qty) in units.iter().filter(|(_, q)| **q > 0.0) {
                cost += basis.get(id).copied().unwrap_or(0.0);
                // Past the end of the series there is nothing to read: repeating the last
                // close would date-stamp a stale price as that day's mark, which is how the
                // rebuild ends up overwriting today's live snapshot with a week-old one.
                let stale = covered_to.get(id).is_none_or(|last| date > *last);
                let close = closes
                    .get(id)
                    .and_then(|m| m.range(..=date).next_back().map(|(_, c)| *c))
                    .or_else(|| last_close.get(id).copied())
                    .filter(|_| !stale);
                let Some(close) = close else {
                    // No close on this day: the asset had not listed, the download has not
                    // reached back this far, or it stops before it. The day is short, and a
                    // short day is not written.
                    short = true;
                    continue;
                };
                last_close.insert(*id, close);
                let a = by_id.get(id);
                let bar_ccy = a.map(|a| a.hist_currency.as_str()).unwrap_or("USD");
                let marked = qty * close;
                value += if bar_ccy == pf.currency {
                    marked
                } else {
                    fx.convert(pool, marked, bar_ccy, &pf.currency, date).await?.unwrap_or(marked)
                };
            }
            // **A day that could not be fully valued is not written.** It would store a
            // book short by an entire holding, and a stored day is permanent: it becomes a
            // crater in the curve, owns the max drawdown from then on, and (when it lands on
            // today) contradicts the header the user is reading. A missing day is
            // recoverable, a wrong one is not. Same rule as `store::snapshot_today`.
            if short {
                incomplete += 1;
                carry_flow = flow;
                carry_income = income;
                carry_fees = fees;
            } else {
                (carry_flow, carry_income, carry_fees) = (0.0, 0.0, 0.0);
                store::write_snapshot(
                    pool, pf.id, date, &pf.currency, value, cost, cash, flow, income, fees, None,
                    "rebuilt",
                )
                .await?;
                days += 1;
            }
        }
        match date.next_day() {
            Some(d) => date = d,
            None => break,
        }
    }

    store::clear_rebuild_from(pool, pf.id).await?;
    Ok(RebuildResult {
        days,
        from: Some(start.to_string()),
        to: Some(today.to_string()),
        incomplete_days: incomplete,
        uncovered,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// The histdata asset type an asset class maps to. Never a guess that could match the wrong
/// instrument: an unknown class falls back to `equity`, which is what the catalog files
/// anything it cannot classify under.
pub fn asset_type_of(asset_class: &str) -> String {
    match asset_class {
        "crypto" => "crypto",
        "etf" => "etf",
        "bond" | "commodity" | "real_estate" | "alternative" => "etf",
        _ => "equity",
    }
    .to_string()
}

fn parse_date(iso: &str) -> Option<Date> {
    Date::parse(iso, &time::format_description::well_known::Iso8601::DATE).ok()
}

async fn first_operation_dates(pool: &PgPool, portfolio_id: Uuid) -> Result<HashMap<Uuid, Date>> {
    let rows: Vec<(Uuid, Date)> = sqlx::query_as(
        "SELECT asset_id, MIN(op_date) FROM portfolio_operations \
         WHERE portfolio_id = $1 AND asset_id IS NOT NULL GROUP BY asset_id",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_classes_map_to_a_catalog_type() {
        assert_eq!(asset_type_of("crypto"), "crypto");
        assert_eq!(asset_type_of("stock"), "equity");
        assert_eq!(asset_type_of("bond"), "etf");
        assert_eq!(asset_type_of("something new"), "equity");
    }
}

// ── Automatic catch-up ────────────────────────────────────────────────────────

/// Portfolios whose catch-up is already running, so a second refresh does not start a second
/// download of the same windows. In-process, like every other single-node lock here: the
/// worker is the only thing that could race, and it dedupes on the dataset anyway.
static RUNNING: std::sync::Mutex<Option<std::collections::HashSet<Uuid>>> =
    std::sync::Mutex::new(None);

fn claim(id: Uuid) -> bool {
    let mut g = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(Default::default).insert(id)
}

fn release(id: Uuid) {
    let mut g = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(set) = g.as_mut() {
        set.remove(&id);
    }
}

/// How long the catch-up waits for its downloads before rebuilding with what landed.
const DOWNLOAD_WAIT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const POLL: std::time::Duration = std::time::Duration::from_secs(10);

/// Bring the curve up to date behind a refresh: queue the missing bars, wait for them, rebuild.
///
/// **Detached on purpose.** A refresh is a button and a daily tick; a backfill is minutes of
/// downloads. Making one wait for the other would either time the request out or teach the
/// user that Refresh hangs. So this runs on its own and the result is a notification, the
/// same shape the journal's market enrichment uses.
///
/// Rebuilding after the downloads is the whole point: `sync` only queues, and a rebuild run
/// before the bars land writes nothing (a day it cannot value is skipped), so the two have to
/// be sequenced or the curve stays empty until somebody presses a button by hand.
pub fn spawn_catch_up(pool: PgPool, pf: Portfolio) {
    if !claim(pf.id) {
        return;
    }
    tokio::spawn(async move {
        let name = pf.name.clone();
        let outcome = catch_up(&pool, &pf).await;
        release(pf.id);
        let told = match outcome {
            // Nothing queued and nothing rebuilt: the curve was already current. The whole
            // point of running this behind every refresh is that it is usually silent.
            Ok(None) => return,
            Ok(Some(msg)) => (format!("Portfolio: {name} history updated"), msg),
            Err(e) => {
                tracing::warn!("portfolio {} catch-up failed: {e:#}", pf.id);
                (format!("Portfolio: {name} history could not be updated"), e.to_string())
            }
        };
        if let Err(e) = otw_store::reminders::add_notification(&pool, &told.0, &told.1).await {
            tracing::warn!("portfolio catch-up notification: {e:#}");
        }
    });
}

/// The catch-up itself. `Ok(None)` means there was nothing to do and nothing to say.
async fn catch_up(pool: &PgPool, pf: &Portfolio) -> Result<Option<String>> {
    let queued = sync(pool, pf).await?;
    if queued.queued > 0 {
        if let Some(batch) = queued.batch_id {
            wait_for_batch(pool, batch).await;
        }
    }
    // Rebuilt even when nothing was queued: the bars may already be on disk and the curve
    // stale for another reason (a backdated operation moved the watermark).
    let from = store::get_portfolio(pool, pf.id).await?.and_then(|p| p.rebuild_from);
    let built = rebuild(pool, pf, from).await?;
    if queued.queued == 0 && built.days == 0 {
        return Ok(None);
    }
    let mut msg = format!("{} day(s) rebuilt", built.days);
    if queued.queued > 0 {
        msg = format!("{} download(s) fetched, {msg}", queued.queued);
    }
    if built.incomplete_days > 0 {
        msg.push_str(&format!(
            ", {} day(s) skipped for want of a candle",
            built.incomplete_days
        ));
    }
    if !queued.no_connector.is_empty() {
        msg.push_str(&format!(
            ". No granted connector serves {}",
            queued.no_connector.join(", ")
        ));
    }
    Ok(Some(msg))
}

/// Poll a download batch until nothing in it is still live, or the wait runs out.
///
/// A timeout is not a failure: the rebuild that follows uses whatever landed and skips the
/// days it still cannot value, so a slow provider costs a partial curve and not a wrong one.
async fn wait_for_batch(pool: &PgPool, batch: Uuid) {
    let deadline = std::time::Instant::now() + DOWNLOAD_WAIT;
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(POLL).await;
        let counts = match hist::batch_counts(pool, batch).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("portfolio catch-up: reading batch {batch}: {e:#}");
                return;
            }
        };
        let live: i64 = counts
            .iter()
            // The same set `histdata::LIVE_STATUSES` cancels on: anything else is terminal.
            .filter(|(status, _)| {
                matches!(status.as_str(), "queued" | "running" | "waiting" | "cancelling")
            })
            .map(|(_, n)| *n)
            .sum();
        if live == 0 {
            return;
        }
    }
    tracing::warn!("portfolio catch-up: batch {batch} still running after the wait; rebuilding anyway");
}
