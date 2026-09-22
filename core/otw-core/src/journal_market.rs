//! Market-data enrichment for the Trading Journal.
//!
//! The journal knows what the trader did. The candles know what price did while the
//! position was open. Everything in this file exists to join the two, and it does so
//! **without adding one line of provider code**: bars are read from the histdata catalog,
//! and missing windows are queued through the ordinary histdata download queue.
//!
//! Three verbs, in the order the screen uses them:
//!
//! * [`coverage`]: for the filtered trades, what is stored, what is missing, and which
//!   connector would serve it. Reads only, writes nothing, queues nothing.
//! * [`sync`]: queue the missing windows as normal download jobs, sharing one batch id so
//!   the page can follow them on `/api/histdata/jobs` like any other batch.
//! * [`compute`]: walk each trade against its bars and write `journal_trade_metrics`.
//!
//! Two rules hold everywhere below:
//!
//! * **Never guess an instrument.** A journal asset class maps to a histdata asset type
//!   or it does not; an option is reported as unsupported rather than silently matched to
//!   the underlying's equity series. A future maps, but its journal ticker does not: the
//!   root alone names no contract, so the trader states the provider's symbol once
//!   (`market_symbols`) and nothing is downloaded until they have.
//! * **Never measure on the wrong grain.** The measurement window is the trade's own
//!   window plus a warm-up, and the grain either comes from the user or is derived from
//!   how long the trades are actually held.

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata as store;
use otw_store::journal::{self, Trade, TradeFilter};
use otw_store::journal_analytics::trade_risk;
use otw_store::journal_exposure as exposure_store;
use otw_store::journal_market as market_store;

use crate::align;
use crate::histdata;

/// The module id the journal presents to the data broker. Listed in
/// `connectors_api::DATA_MODULES`, so a connector is granted to it like any other.
pub const MODULE: &str = "journal";

/// Bars read before the entry, for the ATR, the volatility regime and the trend state.
/// Thirty is enough for a 14-period ATR to settle and short enough that a young dataset
/// still measures its trades.
const WARMUP_BARS: i64 = 30;
/// ATR period, the Wilder convention every chart uses.
const ATR_PERIOD: usize = 14;
/// A trade needs at least this many bars inside its own window before an excursion means
/// anything. Below it the "worst point" is just the entry candle.
const MIN_WINDOW_BARS: usize = 2;
/// Hard cap on the candles pulled for one trade, so a 1m grain on a six-month position
/// cannot pull a quarter of a million rows.
const MAX_BARS_PER_TRADE: i64 = 20_000;

// ── Instruments ──────────────────────────────────────────────────────────────

/// The histdata asset types the journal can serve, in the order the settings list them.
/// One connector is picked per entry; anything outside this list has no provider.
pub const ASSET_TYPES: [&str; 5] = ["equity", "etf", "crypto", "fx", "future"];

/// Journal asset class to histdata asset type.
///
/// `option` maps to nothing on purpose: the journal ticker of an option is rarely the
/// OCC symbol a provider indexes chains by, and matching it to the underlying's equity
/// series would measure a different instrument than the one that was traded.
pub fn asset_type_for(asset_class: &str) -> Option<&'static str> {
    match asset_class {
        "stock" => Some("equity"),
        "etf" => Some("etf"),
        "crypto" => Some("crypto"),
        "forex" => Some("fx"),
        "future" => Some("future"),
        _ => None,
    }
}

/// Asset classes whose journal ticker is not a provider symbol.
///
/// A stock is `AAPL` everywhere. A future is not: `MNQ` is a root, and every provider
/// spells the contract its own way (IBKR `MNQU6` or `MNQ.202609@CME`, Polygon its vendor
/// prefix). Downloading the root would fetch the continuous front month, which is a
/// different instrument than the December contract that was traded, so the journal asks
/// for the symbol instead of picking one.
fn needs_mapping(asset_class: &str) -> bool {
    asset_class == "future"
}

/// Key of a symbol mapping: the asset class and the journal ticker, upper-cased.
/// `ES` the stock and `ES` the future are two instruments and get two entries.
pub fn symbol_key(asset_class: &str, ticker: &str) -> String {
    format!("{}:{}", asset_class.trim(), ticker.trim().to_uppercase())
}

/// The symbol handed to the provider for one instrument.
///
/// `None` means "the trader has not said yet", which is a status, not an error: coverage
/// reports it, sync refuses to queue it and compute leaves the trades unmeasured.
fn provider_symbol(symbols: &Value, asset_class: &str, ticker: &str) -> Option<String> {
    let mapped = symbols
        .get(symbol_key(asset_class, ticker))
        .and_then(Value::as_str)
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());
    match mapped {
        Some(s) => Some(s),
        None if needs_mapping(asset_class) => None,
        None => Some(ticker.trim().to_uppercase()),
    }
}

/// Grain that resolves a trade of this length: the coarsest supported timeframe that
/// still leaves about twenty candles inside the position.
///
/// A scalper holding four minutes needs 1m; a swing trader holding three weeks does not
/// want three weeks of 1m candles, and would not learn anything more from them.
fn grain_for(median_hold_min: i64) -> &'static str {
    const LADDER: [(i64, &str); 6] = [
        (1, "1m"),
        (5, "5m"),
        (15, "15m"),
        (60, "1h"),
        (240, "4h"),
        (1440, "1d"),
    ];
    let mut best = "1m";
    for (minutes, tf) in LADDER {
        if median_hold_min >= minutes * 20 {
            best = tf;
        }
    }
    best
}

/// The grain the enrichment will use: the setting when it names one, otherwise derived
/// from the median hold of the trades in scope.
pub fn resolve_timeframe(setting: &str, trades: &[Trade]) -> String {
    if setting != "auto" && histdata::SUPPORTED_TIMEFRAMES.contains(&setting) {
        return setting.to_string();
    }
    let mut holds: Vec<i64> = trades
        .iter()
        .filter_map(trade_window)
        .filter(|(a, b)| b > a)
        .map(|(a, b)| (b - a).whole_minutes())
        .collect();
    if holds.is_empty() {
        return "1d".to_string();
    }
    holds.sort_unstable();
    grain_for(holds[holds.len() / 2]).to_string()
}

/// The wall-clock window a trade was open for.
///
/// The typed columns win when they are filled. An advanced trade may carry its dates on
/// the legs instead (that is how an executions import files them), so the first entry
/// fill and the last exit fill are the fallback: without it every imported trade would
/// report "no dates" and never be measured.
fn trade_window(t: &Trade) -> Option<(OffsetDateTime, OffsetDateTime)> {
    let legs = |v: &serde_json::Value| {
        serde_json::from_value::<Vec<journal::Leg>>(v.clone()).unwrap_or_default()
    };
    let entry = t.entry_at.or_else(|| {
        t.advanced
            .then(|| legs(&t.entries).iter().filter_map(|l| l.at).min())
            .flatten()
    })?;
    let exit = t.exit_at.or_else(|| {
        t.advanced
            .then(|| legs(&t.exits).iter().filter_map(|l| l.at).max())
            .flatten()
    })?;
    (exit >= entry).then_some((entry, exit))
}

/// Merge overlapping or touching intervals, sorted by start.
///
/// A day-trader logs dozens of windows a day on one symbol; merged they are one range.
/// Everything downstream (the coverage check, the download list) then works on a handful
/// of intervals instead of one per trade.
fn merge_spans(mut spans: Vec<(OffsetDateTime, OffsetDateTime)>) -> Vec<(OffsetDateTime, OffsetDateTime)> {
    spans.sort_by_key(|(a, _)| *a);
    let mut out: Vec<(OffsetDateTime, OffsetDateTime)> = Vec::with_capacity(spans.len());
    for (lo, hi) in spans {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// One instrument the filtered trades touch, keyed exactly as histdata keys a dataset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    asset_class: String,
    ticker: String,
}

/// A window of wall-clock time that has to exist in store before a group can be measured.
#[derive(Debug, Serialize, Clone)]
pub struct Window {
    #[serde(with = "time::serde::rfc3339")]
    pub from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub to: OffsetDateTime,
}

/// Coverage of one instrument: what the trades need, what the catalog holds, what is
/// missing, and who would fetch it.
#[derive(Debug, Serialize)]
pub struct InstrumentCoverage {
    pub ticker: String,
    pub asset_class: String,
    /// The symbol actually asked of the provider: the journal ticker, unless the trader
    /// mapped it. Empty while a future is waiting for its contract to be named.
    pub symbol: String,
    /// `None` when no connector in the catalog serves this asset class.
    pub asset_type: Option<String>,
    pub timeframe: String,
    pub trades: i64,
    pub measured: i64,
    /// Trades that carry no entry or no exit stamp: nothing can be measured on them, and
    /// no download would help.
    pub undated: i64,
    pub needed: Window,
    pub dataset_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub stored_from: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub stored_to: Option<OffsetDateTime>,
    pub bar_count: i64,
    /// The windows a download would have to cover. Empty = nothing to fetch.
    pub missing: Vec<Window>,
    pub connector_id: Option<Uuid>,
    pub connector_name: String,
    pub provider: String,
    /// `ready` | `partial` | `missing` | `unsupported` | `no_connector` | `needs_symbol`.
    pub status: String,
    /// Why a group cannot be served, when the status says so.
    pub reason: String,
}

/// The whole picture for the filtered scope, as the header bar reads it.
#[derive(Debug, Serialize)]
pub struct Coverage {
    pub timeframe: String,
    /// Whether the timeframe was derived rather than chosen.
    pub timeframe_auto: bool,
    pub sync_mode: String,
    pub closed_trades: i64,
    pub measurable: i64,
    pub measured: i64,
    /// Trades sitting on an instrument no connector serves.
    pub unsupported: i64,
    /// Trades on an instrument whose provider symbol has not been stated yet.
    pub needs_symbol: i64,
    /// Trades whose window is fully stored and could be measured right now.
    pub ready: i64,
    pub instruments: Vec<InstrumentCoverage>,
    /// Instruments with something to download, and the total windows that implies.
    pub missing_instruments: i64,
    pub missing_windows: i64,
}

/// Connector that will serve an asset type: the user's explicit pick when it still
/// exists and still supports the type, otherwise the first granted connector that does.
fn pick_connector<'a>(
    granted: &'a [conn_store::ConnectorRow],
    chosen: &Value,
    asset_type: &str,
) -> Option<&'a conn_store::ConnectorRow> {
    let supports = |c: &conn_store::ConnectorRow| {
        histdata::connector_for(&c.provider)
            .map(|k| k.capability().asset_types.contains(&asset_type))
            .unwrap_or(false)
    };
    let explicit = chosen
        .get(asset_type)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    if let Some(id) = explicit {
        if let Some(c) = granted.iter().find(|c| c.id == id) {
            if supports(c) {
                return Some(c);
            }
        }
    }
    granted.iter().find(|c| supports(c))
}

/// Group the closed, dated trades by instrument and report what each one needs.
///
/// Reads only. A trade with no exit stamp is counted as `undated` and never widens a
/// window: a download would not make it measurable.
pub async fn coverage(
    pool: &sqlx::PgPool,
    filter: &TradeFilter,
) -> Result<Coverage> {
    let settings = market_store::get_settings(pool).await?;
    let trades = journal::list_trades(pool, filter).await?;
    let closed: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl.is_some()).collect();
    let timeframe = resolve_timeframe(&settings.timeframe, &trades);
    let tf_secs = histdata::timeframe_secs(&timeframe)?;
    let warmup = Duration::seconds(tf_secs * WARMUP_BARS);

    let granted = conn_store::list_for_module(pool, MODULE).await?;
    let measured_ids: Vec<Uuid> = closed.iter().map(|t| t.id).collect();
    let metrics = market_store::metrics_for(pool, &measured_ids).await?;

    // Per instrument: how many trades, how many already measured, and the windows they
    // need. The individual spans are kept, not just their union: a hole between two
    // trades is only worth downloading if a trade actually lived in it.
    #[derive(Default)]
    struct Acc {
        trades: i64,
        measured: i64,
        undated: i64,
        spans: Vec<(OffsetDateTime, OffsetDateTime)>,
    }
    let mut groups: BTreeMap<Key, Acc> = BTreeMap::new();
    for t in &closed {
        let key = Key {
            asset_class: t.asset_class.clone(),
            ticker: t.ticker.trim().to_uppercase(),
        };
        let a = groups.entry(key).or_default();
        a.trades += 1;
        if metrics.contains_key(&t.id) {
            a.measured += 1;
        }
        match trade_window(t) {
            Some((entry, exit)) => a.spans.push((entry - warmup, exit)),
            None => a.undated += 1,
        }
    }

    let mut instruments = Vec::new();
    let (mut measurable, mut measured_total, mut unsupported, mut ready_trades) = (0, 0, 0, 0);
    let mut needs_symbol = 0i64;
    let (mut missing_instruments, mut missing_windows) = (0, 0);

    for (key, acc) in groups {
        let dated = acc.trades - acc.undated;
        measured_total += acc.measured;
        let asset_type = asset_type_for(&key.asset_class);
        let spans = merge_spans(acc.spans);
        let (Some(needed_from), Some(needed_to)) =
            (spans.first().map(|s| s.0), spans.last().map(|s| s.1))
        else {
            // Every trade of this instrument is undated: nothing to fetch, nothing to
            // measure, but the group is still listed so the reason is visible.
            instruments.push(InstrumentCoverage {
                ticker: key.ticker.clone(),
                asset_class: key.asset_class.clone(),
                symbol: provider_symbol(&settings.symbols, &key.asset_class, &key.ticker)
                    .unwrap_or_default(),
                asset_type: asset_type.map(str::to_string),
                timeframe: timeframe.clone(),
                trades: acc.trades,
                measured: acc.measured,
                undated: acc.undated,
                needed: Window {
                    from: OffsetDateTime::UNIX_EPOCH,
                    to: OffsetDateTime::UNIX_EPOCH,
                },
                dataset_id: None,
                stored_from: None,
                stored_to: None,
                bar_count: 0,
                missing: Vec::new(),
                connector_id: None,
                connector_name: String::new(),
                provider: String::new(),
                status: "undated".into(),
                reason: "no entry or exit time".into(),
            });
            continue;
        };
        let needed = Window {
            from: needed_from,
            to: needed_to,
        };

        let Some(asset_type) = asset_type else {
            unsupported += dated;
            instruments.push(InstrumentCoverage {
                ticker: key.ticker,
                asset_class: key.asset_class.clone(),
                symbol: String::new(),
                asset_type: None,
                timeframe: timeframe.clone(),
                trades: acc.trades,
                measured: acc.measured,
                undated: acc.undated,
                needed,
                dataset_id: None,
                stored_from: None,
                stored_to: None,
                bar_count: 0,
                missing: Vec::new(),
                connector_id: None,
                connector_name: String::new(),
                provider: String::new(),
                status: "unsupported".into(),
                reason: format!("no provider serves {}", key.asset_class),
            });
            continue;
        };
        // The contract, when the ticker alone does not name one. Reported before the
        // catalog is even looked at: a future with no stated symbol has no dataset to
        // find and no window worth downloading.
        let Some(symbol) = provider_symbol(&settings.symbols, &key.asset_class, &key.ticker) else {
            needs_symbol += dated;
            instruments.push(InstrumentCoverage {
                ticker: key.ticker,
                asset_class: key.asset_class.clone(),
                symbol: String::new(),
                asset_type: Some(asset_type.to_string()),
                timeframe: timeframe.clone(),
                trades: acc.trades,
                measured: acc.measured,
                undated: acc.undated,
                needed,
                dataset_id: None,
                stored_from: None,
                stored_to: None,
                bar_count: 0,
                missing: Vec::new(),
                connector_id: None,
                connector_name: String::new(),
                provider: String::new(),
                status: "needs_symbol".into(),
                reason: "name the contract your source uses".into(),
            });
            continue;
        };
        measurable += dated;

        let connector = pick_connector(&granted, &settings.connectors, asset_type);
        let dataset =
            store::find_dataset_any(pool, asset_type, &symbol, &timeframe, connector.map(|c| c.provider.as_str()))
                .await?;

        // What a download would still have to cover. Three sources, and the third is the
        // one a range comparison alone misses:
        //
        //   head    the span before what is stored
        //   tail    the span after it
        //   holes   spans inside the stored range that hold no bar at all
        //
        // The holes are asked of the bars, never inferred from a gap width: a daily
        // equity series is "missing" every weekend and a 24/7 crypto one never is, so any
        // threshold on gap size would be wrong on one of them. `windows_without_bars`
        // answers the only question that matters, "was there a candle where this trade
        // lived", in one round trip.
        let mut missing = Vec::new();
        match &dataset {
            Some(d) => match (d.range_from, d.range_to) {
                (Some(sf), Some(st)) if d.bar_count > 0 => {
                    if needed_from < sf {
                        missing.push(Window {
                            from: needed_from,
                            to: sf.min(needed_to),
                        });
                    }
                    if needed_to > st {
                        missing.push(Window {
                            from: st.max(needed_from),
                            to: needed_to,
                        });
                    }
                    // Only the spans that actually sit inside the stored range: the ones
                    // outside it are already covered by the head and the tail above.
                    let inside: Vec<(OffsetDateTime, OffsetDateTime)> = spans
                        .iter()
                        .map(|(a, b)| ((*a).max(sf), (*b).min(st)))
                        .filter(|(a, b)| a < b)
                        .collect();
                    for i in store::windows_without_bars(pool, d.id, &inside).await? {
                        missing.push(Window {
                            from: inside[i].0,
                            to: inside[i].1,
                        });
                    }
                }
                // A dataset row with no bars is a queued download, not coverage.
                _ => missing.push(needed.clone()),
            },
            None => missing.push(needed.clone()),
        }
        // Head, tail and holes can touch; the download list should not repeat itself.
        let missing: Vec<Window> = merge_spans(missing.into_iter().map(|w| (w.from, w.to)).collect())
            .into_iter()
            .map(|(from, to)| Window { from, to })
            .collect();

        let status = if connector.is_none() && !missing.is_empty() {
            "no_connector"
        } else if missing.is_empty() {
            ready_trades += dated;
            "ready"
        } else if dataset.as_ref().is_some_and(|d| d.bar_count > 0) {
            "partial"
        } else {
            "missing"
        };
        if !missing.is_empty() {
            missing_instruments += 1;
            missing_windows += missing.len() as i64;
        }

        instruments.push(InstrumentCoverage {
            ticker: key.ticker,
            asset_class: key.asset_class,
            symbol,
            asset_type: Some(asset_type.to_string()),
            timeframe: timeframe.clone(),
            trades: acc.trades,
            measured: acc.measured,
            undated: acc.undated,
            needed,
            dataset_id: dataset.as_ref().map(|d| d.id),
            stored_from: dataset.as_ref().and_then(|d| d.range_from),
            stored_to: dataset.as_ref().and_then(|d| d.range_to),
            bar_count: dataset.as_ref().map_or(0, |d| d.bar_count),
            missing,
            connector_id: connector.map(|c| c.id),
            connector_name: connector.map(|c| c.name.clone()).unwrap_or_default(),
            provider: connector.map(|c| c.provider.clone()).unwrap_or_default(),
            status: status.into(),
            reason: if status == "no_connector" {
                format!("no connector granted to the journal serves {asset_type}")
            } else {
                String::new()
            },
        });
    }

    // Busiest instrument first: that is where a download buys the most.
    instruments.sort_by(|a, b| b.trades.cmp(&a.trades).then(a.ticker.cmp(&b.ticker)));

    Ok(Coverage {
        timeframe,
        timeframe_auto: settings.timeframe == "auto",
        sync_mode: settings.sync_mode,
        closed_trades: closed.len() as i64,
        measurable,
        measured: measured_total,
        unsupported,
        needs_symbol,
        ready: ready_trades,
        missing_instruments,
        missing_windows,
        instruments,
    })
}

// ── Sync ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncResult {
    /// One batch for the whole submission, so the page can cancel what is left in one
    /// click on the existing endpoint.
    pub batch_id: Option<Uuid>,
    pub queued: i64,
    pub skipped: Vec<Value>,
}

/// Queue every missing window of the filtered scope as an ordinary histdata download.
///
/// Nothing here talks to a provider: it writes the same `histdata_jobs` rows the
/// historical-data page writes, so the worker, the quota, the retries and the progress
/// endpoint are the ones already in place.
pub async fn sync(pool: &sqlx::PgPool, filter: &TradeFilter) -> Result<SyncResult> {
    let cov = coverage(pool, filter).await?;
    let mut jobs: Vec<(&InstrumentCoverage, &Window, Uuid, String)> = Vec::new();
    let mut skipped = Vec::new();

    for inst in &cov.instruments {
        if inst.missing.is_empty() {
            continue;
        }
        if inst.symbol.is_empty() {
            skipped.push(serde_json::json!({
                "ticker": inst.ticker,
                "reason": "no contract symbol: say which contract your source calls it",
            }));
            continue;
        }
        let (Some(asset_type), Some(connector_id)) = (inst.asset_type.as_deref(), inst.connector_id)
        else {
            skipped.push(serde_json::json!({
                "ticker": inst.ticker,
                "reason": if inst.reason.is_empty() { "no connector".into() } else { inst.reason.clone() },
            }));
            continue;
        };
        // The capability matrix is enforced here as well as in the picker: a connector
        // can lose an asset type between the pick and the queue.
        if let Err(e) = histdata::validate_request(&inst.provider, asset_type, &inst.timeframe) {
            skipped.push(serde_json::json!({ "ticker": inst.ticker, "reason": e.to_string() }));
            continue;
        }
        for w in &inst.missing {
            jobs.push((inst, w, connector_id, asset_type.to_string()));
        }
    }

    if jobs.is_empty() {
        return Ok(SyncResult {
            batch_id: None,
            queued: 0,
            skipped,
        });
    }
    let batch_id = (jobs.len() > 1).then(Uuid::new_v4);
    let mut queued = 0i64;
    for (inst, w, connector_id, asset_type) in jobs {
        let dataset_id =
            store::upsert_dataset(pool, &inst.provider, &asset_type, &inst.symbol, &inst.timeframe)
                .await?;
        store::enqueue_job(
            pool,
            &store::NewJob {
                dataset_id,
                connector_id: Some(connector_id),
                provider: &inst.provider,
                asset_type: &asset_type,
                ticker: &inst.symbol,
                timeframe: &inst.timeframe,
                range_from: w.from,
                range_to: w.to,
                kind: "download",
                batch_id,
            },
        )
        .await?;
        queued += 1;
    }
    Ok(SyncResult {
        batch_id,
        queued,
        skipped,
    })
}

// ── Compute ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ComputeResult {
    pub timeframe: String,
    /// Trades looked at, measured, and left alone with the reason.
    pub considered: i64,
    pub measured: i64,
    /// Trades whose stored measurement was still valid and was not recomputed.
    pub reused: i64,
    /// Kept rows whose regime label moved because the batch's volatility spread changed.
    pub relabelled: i64,
    pub skipped_no_bars: i64,
    pub skipped_undated: i64,
    pub skipped_unsupported: i64,
    /// Trades on an instrument whose provider symbol has not been stated yet.
    pub skipped_unmapped: i64,
    pub skipped_thin: i64,
}

/// Whether a stored measurement still describes the trade in front of us.
///
/// Three things can invalidate it, and all three are answered by a timestamp rather than
/// by a guess:
///
/// * the **trade** was edited (`journal_trades.updated_at`): new dates, new stop, new
///   size all move every column of the row;
/// * the **bars** moved (`histdata_datasets.last_updated`): a download landed, so a
///   window that was thin or empty may now resolve;
/// * the **grain** changed, which the row records itself.
///
/// Anything else (a new trade elsewhere, a filter change) leaves the row alone. This is
/// what keeps a recompute proportional to what actually changed instead of to the size of
/// the book.
fn still_valid(
    stored: &market_store::TradeMetrics,
    trade_updated: OffsetDateTime,
    timeframe: &str,
    dataset_updated: Option<OffsetDateTime>,
) -> bool {
    stored.timeframe == timeframe
        && stored.computed_at >= trade_updated
        && dataset_updated.is_none_or(|u| stored.computed_at >= u)
}

/// One bar reduced to what the walk needs.
struct Candle {
    ts: OffsetDateTime,
    high: f64,
    low: f64,
    close: f64,
}

/// Wilder's true range over consecutive candles, averaged over the last `ATR_PERIOD`
/// available. Fewer candles than the period is not an error: the average is simply taken
/// over what exists, and `bars` on the row records how thin it was.
fn atr(pre: &[Candle]) -> Option<f64> {
    if pre.len() < 2 {
        return None;
    }
    let trs: Vec<f64> = pre
        .windows(2)
        .map(|w| {
            let (prev, cur) = (&w[0], &w[1]);
            (cur.high - cur.low)
                .max((cur.high - prev.close).abs())
                .max((cur.low - prev.close).abs())
        })
        .collect();
    let take = trs.len().min(ATR_PERIOD);
    let slice = &trs[trs.len() - take..];
    (take > 0).then(|| slice.iter().sum::<f64>() / take as f64)
}

/// Annualized realized volatility over the pre-entry candles, in percent.
fn realized_vol(pre: &[Candle], timeframe: &str) -> Option<f64> {
    let rets: Vec<f64> = pre
        .windows(2)
        .filter(|w| w[0].close > 0.0 && w[1].close > 0.0)
        .map(|w| (w[1].close / w[0].close).ln())
        .collect();
    if rets.len() < 5 {
        return None;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() - 1) as f64;
    Some(var.sqrt() * crate::quant::periods_per_year(timeframe).sqrt() * 100.0)
}

/// Trend state at entry: the last close against the average of the pre-entry window.
/// The 0.5 % dead band keeps a flat market from being labelled a trend by rounding.
fn trend_state(pre: &[Candle]) -> Option<&'static str> {
    if pre.len() < 10 {
        return None;
    }
    let sma = pre.iter().map(|c| c.close).sum::<f64>() / pre.len() as f64;
    let last = pre.last()?.close;
    if sma <= 0.0 {
        return None;
    }
    Some(if last > sma * 1.005 {
        "up"
    } else if last < sma * 0.995 {
        "down"
    } else {
        "flat"
    })
}

/// Measure the filtered trades against the stored bars and write the result.
///
/// Trades that cannot be measured leave no row at all, which is what makes "measured"
/// countable and a partial dataset honest rather than half full of zeros.
pub async fn compute(
    pool: &sqlx::PgPool,
    filter: &TradeFilter,
    full: bool,
) -> Result<ComputeResult> {
    let settings = market_store::get_settings(pool).await?;
    let trades = journal::list_trades(pool, filter).await?;
    let timeframe = resolve_timeframe(&settings.timeframe, &trades);
    let tf_secs = histdata::timeframe_secs(&timeframe)?;
    let warmup = Duration::seconds(tf_secs * WARMUP_BARS);
    let granted = conn_store::list_for_module(pool, MODULE).await?;
    let trade_ids: Vec<Uuid> = trades.iter().map(|t| t.id).collect();
    // What is already on file. `full` throws it away and re-measures everything.
    let mut stored = if full {
        HashMap::new()
    } else {
        market_store::metrics_for(pool, &trade_ids).await?
    };

    let mut result = ComputeResult {
        timeframe: timeframe.clone(),
        considered: 0,
        measured: 0,
        reused: 0,
        relabelled: 0,
        skipped_no_bars: 0,
        skipped_undated: 0,
        skipped_unsupported: 0,
        skipped_unmapped: 0,
        skipped_thin: 0,
    };

    // Dataset lookups are per instrument, not per trade: the id and the moment its bars
    // last moved, which is half of the freshness test below.
    let mut datasets: HashMap<(String, String), Option<(Uuid, OffsetDateTime)>> = HashMap::new();
    let mut fresh: Vec<market_store::TradeMetrics> = Vec::new();
    let mut kept: Vec<market_store::TradeMetrics> = Vec::new();
    let mut stale: Vec<Uuid> = Vec::new();

    for t in &trades {
        if t.net_pnl.is_none() {
            continue;
        }
        result.considered += 1;
        let Some(asset_type) = asset_type_for(&t.asset_class) else {
            result.skipped_unsupported += 1;
            stale.push(t.id);
            continue;
        };
        let Some((entry_at, exit_at)) = trade_window(t) else {
            result.skipped_undated += 1;
            stale.push(t.id);
            continue;
        };
        let Some(ticker) = provider_symbol(&settings.symbols, &t.asset_class, &t.ticker) else {
            result.skipped_unmapped += 1;
            stale.push(t.id);
            continue;
        };

        let key = (asset_type.to_string(), ticker.clone());
        let dataset = match datasets.get(&key) {
            Some(d) => *d,
            None => {
                let preferred = pick_connector(&granted, &settings.connectors, asset_type)
                    .map(|c| c.provider.clone());
                let d = store::find_dataset_any(
                    pool,
                    asset_type,
                    &ticker,
                    &timeframe,
                    preferred.as_deref(),
                )
                .await?
                .map(|d| (d.id, d.last_updated));
                datasets.insert(key, d);
                d
            }
        };
        let Some((dataset_id, dataset_updated)) = dataset else {
            result.skipped_no_bars += 1;
            stale.push(t.id);
            continue;
        };

        // The cheap path, and the point of the whole function: a row that nothing has
        // invalidated is kept as it is, and its bars are never read.
        if let Some(row) = stored.remove(&t.id) {
            if still_valid(&row, t.updated_at, &timeframe, Some(dataset_updated)) {
                result.reused += 1;
                kept.push(row);
                continue;
            }
        }

        let bars = store::read_bars(
            pool,
            dataset_id,
            Some(entry_at - warmup),
            Some(exit_at),
            MAX_BARS_PER_TRADE,
        )
        .await?;
        let candles: Vec<Candle> = bars
            .iter()
            .map(|b| Candle {
                ts: b.ts,
                high: b.high,
                low: b.low,
                close: b.close,
            })
            .collect();
        // A bar is a period: the one stamped at the entry belongs to the position.
        let split = candles.partition_point(|c| c.ts < entry_at);
        let (pre, during) = candles.split_at(split);
        if during.len() < MIN_WINDOW_BARS {
            result.skipped_thin += 1;
            stale.push(t.id);
            continue;
        }

        let Some(metrics) = measure(t, pre, during, entry_at, &timeframe, dataset_id) else {
            result.skipped_thin += 1;
            stale.push(t.id);
            continue;
        };
        fresh.push(metrics);
        result.measured += 1;
    }

    // The regime bucket is relative to this trader's own book, so it is decided over the
    // whole scope, not over the slice that happened to need remeasuring: a fresh row and
    // a kept row must not be labelled against two different sets of terciles.
    let boundary = fresh.len();
    let mut all = fresh;
    let before: Vec<Option<String>> = kept.iter().map(|r| r.regime.clone()).collect();
    all.extend(kept);
    assign_regimes(&mut all);

    // Write the fresh rows, plus only the kept ones whose label actually moved.
    let mut writes: Vec<market_store::TradeMetrics> = Vec::with_capacity(boundary);
    for (i, row) in all.into_iter().enumerate() {
        if i < boundary {
            writes.push(row);
        } else if before[i - boundary] != row.regime {
            result.relabelled += 1;
            writes.push(row);
        }
    }

    // A trade that can no longer be measured must not keep a stale row from a previous
    // run (an edited date, a deleted dataset, a changed grain).
    market_store::clear_metrics(pool, Some(&stale)).await?;
    market_store::upsert_metrics(pool, &writes).await?;
    Ok(result)
}

/// Walk one trade against its candles. Returns `None` when the trade carries no usable
/// entry price or quantity, in which case an excursion in money cannot be formed.
fn measure(
    t: &Trade,
    pre: &[Candle],
    during: &[Candle],
    entry_at: OffsetDateTime,
    timeframe: &str,
    dataset_id: Uuid,
) -> Option<market_store::TradeMetrics> {
    let entry = t.avg_entry?;
    let qty = journal::trade_entry_qty(t).abs();
    if entry <= 0.0 || qty <= 0.0 {
        return None;
    }
    let unit = qty * t.multiplier;
    // +1 for a long, -1 for anything short: the excursion is signed by the direction the
    // position profits from.
    let dir = if t.side.eq_ignore_ascii_case("short") {
        -1.0
    } else {
        1.0
    };

    // Best and worst price the position saw, and when.
    let mut best = (entry, entry_at);
    let mut worst = (entry, entry_at);
    for c in during {
        let (fav, adv) = if dir > 0.0 {
            (c.high, c.low)
        } else {
            (c.low, c.high)
        };
        if (fav - best.0) * dir > 0.0 {
            best = (fav, c.ts);
        }
        if (adv - worst.0) * dir < 0.0 {
            worst = (adv, c.ts);
        }
    }

    let mfe = ((best.0 - entry) * dir * unit).max(0.0);
    let mae = ((entry - worst.0) * dir * unit).max(0.0);
    // Captured is measured the same way as the excursions, from the average prices, so
    // "kept 40 % of the move" compares two numbers built the same way. The trade's own
    // `net_pnl` stays the reported result everywhere else.
    let captured = journal::trade_avg_exit(t).map(|exit| (exit - entry) * dir * unit);
    let exit_efficiency = match (captured, mfe > 0.0) {
        (Some(c), true) => Some(c / mfe * 100.0),
        _ => None,
    };
    let giveback = captured.map(|c| (mfe - c).max(0.0));

    let risk = trade_risk(t);
    let atr_entry = atr(pre);
    let stop = otw_store::journal_analytics::trade_stop(t);
    let stop_hit = stop.map(|s| {
        during
            .iter()
            .any(|c| if dir > 0.0 { c.low <= s } else { c.high >= s })
    });

    Some(market_store::TradeMetrics {
        trade_id: t.id,
        dataset_id: Some(dataset_id),
        timeframe: timeframe.to_string(),
        bars: during.len() as i32,
        mae: Some(mae),
        mfe: Some(mfe),
        mae_price: Some(worst.0),
        mfe_price: Some(best.0),
        mae_r: risk.map(|r| mae / r),
        mfe_r: risk.map(|r| mfe / r),
        exit_efficiency,
        giveback,
        time_to_mae_min: Some((worst.1 - entry_at).whole_minutes().max(0)),
        time_to_mfe_min: Some((best.1 - entry_at).whole_minutes().max(0)),
        atr_entry,
        vol_pct: realized_vol(pre, timeframe),
        // Filled by `assign_regimes` once the whole batch is known.
        regime: None,
        trend: trend_state(pre).map(str::to_string),
        stop_distance_atr: match (stop, atr_entry) {
            (Some(s), Some(a)) if a > 0.0 => Some((entry - s).abs() / a),
            _ => None,
        },
        stop_hit,
        computed_at: OffsetDateTime::now_utc(),
    })
}

/// Label each measured trade `low` / `normal` / `high` by the terciles of the realized
/// volatility across the batch.
///
/// Deliberately relative: an absolute threshold would call every crypto trade "high" and
/// every bond trade "low", which says nothing about when *this* strategy works.
fn assign_regimes(rows: &mut [market_store::TradeMetrics]) {
    let mut vols: Vec<f64> = rows.iter().filter_map(|r| r.vol_pct).collect();
    // Under a dozen observations the terciles are noise, so nothing is labelled.
    if vols.len() < 12 {
        return;
    }
    vols.sort_by(f64::total_cmp);
    let lo = vols[vols.len() / 3];
    let hi = vols[vols.len() * 2 / 3];
    for r in rows.iter_mut() {
        r.regime = r.vol_pct.map(|v| {
            if v < lo {
                "low"
            } else if v >= hi {
                "high"
            } else {
                "normal"
            }
            .to_string()
        });
    }
}

// ── Auto mode ────────────────────────────────────────────────────────────────

/// Queue the missing bars for one instrument a trade was just written on, in the
/// background.
///
/// Called on both create and edit: an edit moves the window as surely as a create makes
/// one, and a trade whose dates changed needs bars the book may not hold.
///
/// Auto mode is a convenience, never a blocker: it must not slow down or fail the write,
/// so it runs detached and reports through a notification rather than through the HTTP
/// response. It is also narrow on purpose, filtering down to that one instrument, so
/// touching a trade never triggers a download of the whole book.
pub fn auto_sync_spawn(
    pool: sqlx::PgPool,
    cipher: otw_store::crypto::SecretCipher,
    http: reqwest::Client,
    category_id: Uuid,
    asset_class: &str,
    ticker: &str,
) {
    let filter = TradeFilter {
        category_id: Some(category_id),
        asset_class: Some(asset_class.to_string()),
        ticker: Some(ticker.trim().to_string()),
        ..Default::default()
    };
    let ticker = ticker.trim().to_uppercase();
    tokio::spawn(async move {
        let settings = match market_store::get_settings(&pool).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("journal auto-sync: loading settings: {e:#}");
                return;
            }
        };
        if settings.sync_mode != "auto" {
            return;
        }
        let (name, details) = match sync(&pool, &filter).await {
            Ok(r) if r.queued > 0 => (
                format!("Journal: downloading market data for {ticker}"),
                format!(
                    "{} window(s) queued so the new trade can be measured against its candles.",
                    r.queued
                ),
            ),
            // Nothing queued and nothing refused: the bars were already there.
            Ok(r) if r.skipped.is_empty() => return,
            Ok(r) => (
                format!("Journal: no market data for {ticker}"),
                r.skipped
                    .iter()
                    .filter_map(|s| s.get("reason").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            Err(e) => (
                format!("Journal: market data sync failed for {ticker}"),
                e.to_string(),
            ),
        };
        match otw_store::reminders::add_notification(&pool, &name, &details).await {
            Ok(n) => {
                crate::notif_send::dispatch(&pool, &cipher, &http, &n, MODULE, None).await;
            }
            Err(e) => tracing::warn!("journal auto-sync notification: {e:#}"),
        }
    });
}

/// Validate a settings patch and store it. Returns the stored block.
pub async fn save_settings(
    pool: &sqlx::PgPool,
    patch: market_store::MarketPatch,
) -> Result<market_store::MarketSettings> {
    let mut s = market_store::get_settings(pool).await?;
    if let Some(v) = patch.connectors {
        if !v.is_object() {
            return Err(anyhow!("connectors must be an object"));
        }
        // Only asset types the catalog knows, only connectors that exist and are granted.
        let granted = conn_store::list_for_module(pool, MODULE).await?;
        for (asset_type, id) in v.as_object().expect("checked above") {
            let id = id
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| anyhow!("connector id for {asset_type} is not a uuid"))?;
            let c = granted
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow!("connector is not granted to the journal"))?;
            histdata::connector_for(&c.provider)?
                .capability()
                .asset_types
                .contains(&asset_type.as_str())
                .then_some(())
                .ok_or_else(|| anyhow!("{} does not serve {asset_type}", c.name))?;
        }
        s.connectors = v;
    }
    if let Some(tf) = patch.timeframe {
        if tf != "auto" && !histdata::SUPPORTED_TIMEFRAMES.contains(&tf.as_str()) {
            return Err(anyhow!("unknown timeframe {tf}"));
        }
        s.timeframe = tf;
    }
    if let Some(mode) = patch.sync_mode {
        if !market_store::SYNC_MODES.contains(&mode.as_str()) {
            return Err(anyhow!("unknown sync mode {mode}"));
        }
        s.sync_mode = mode;
    }
    if let Some(v) = patch.symbols {
        if !v.is_object() {
            return Err(anyhow!("symbols must be an object"));
        }
        // Keys are normalized here rather than trusted: the map is read back with
        // `symbol_key`, so an entry written any other way would never be found again.
        let mut out = serde_json::Map::new();
        for (key, value) in v.as_object().expect("checked above") {
            let (class, ticker) = key
                .split_once(':')
                .ok_or_else(|| anyhow!("symbol key {key} is not <asset_class>:<ticker>"))?;
            if asset_type_for(class).is_none() {
                return Err(anyhow!("no provider serves {class}"));
            }
            let symbol = value
                .as_str()
                .ok_or_else(|| anyhow!("symbol for {key} is not a string"))?
                .trim()
                .to_uppercase();
            // An empty value is how the UI clears a mapping, not an error.
            if symbol.is_empty() || ticker.trim().is_empty() {
                continue;
            }
            out.insert(symbol_key(class, ticker), Value::String(symbol));
        }
        s.symbols = Value::Object(out);
    }
    market_store::set_settings(pool, &s).await?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(mins: i64, high: f64, low: f64, close: f64) -> Candle {
        Candle {
            ts: OffsetDateTime::UNIX_EPOCH + Duration::minutes(mins),
            high,
            low,
            close,
        }
    }

    #[test]
    fn grain_follows_the_hold() {
        assert_eq!(grain_for(4), "1m");
        assert_eq!(grain_for(30), "1m");
        assert_eq!(grain_for(120), "5m");
        assert_eq!(grain_for(400), "15m");
        assert_eq!(grain_for(60 * 30), "1h");
        assert_eq!(grain_for(60 * 24 * 15), "4h");
        // Twenty daily candles inside the hold is exactly where 1d takes over.
        assert_eq!(grain_for(60 * 24 * 20), "1d");
        assert_eq!(grain_for(60 * 24 * 40), "1d");
    }

    #[test]
    fn asset_classes_map_or_refuse() {
        assert_eq!(asset_type_for("stock"), Some("equity"));
        assert_eq!(asset_type_for("forex"), Some("fx"));
        assert_eq!(asset_type_for("future"), Some("future"));
        // An option is not its underlying, and a journal ticker is not an OCC symbol.
        assert_eq!(asset_type_for("option"), None);
        assert_eq!(asset_type_for("other"), None);
    }

    #[test]
    fn a_future_waits_for_its_contract_and_a_stock_never_does() {
        let none = serde_json::json!({});
        // A stock is its own symbol, whatever the trader's spacing and case.
        assert_eq!(
            provider_symbol(&none, "stock", " aapl "),
            Some("AAPL".to_string())
        );
        // A future root names no contract: nothing is downloaded until it is stated.
        assert_eq!(provider_symbol(&none, "future", "MNQ"), None);

        let mapped = serde_json::json!({ "future:MNQ": "MNQU6" });
        assert_eq!(
            provider_symbol(&mapped, "future", "mnq"),
            Some("MNQU6".to_string())
        );
        // The mapping is per instrument, not per root: the stock keeps its own key.
        assert_eq!(
            provider_symbol(&mapped, "stock", "MNQ"),
            Some("MNQ".to_string())
        );
    }

    #[test]
    fn atr_averages_the_true_range() {
        // Each bar is 2 wide, and the closes do not gap, so the true range is 2.
        let pre: Vec<Candle> = (0..5).map(|i| c(i, 101.0, 99.0, 100.0)).collect();
        assert_eq!(atr(&pre), Some(2.0));
        assert!(atr(&pre[..1]).is_none());
    }

    #[test]
    fn a_flat_tape_has_no_volatility_and_no_trend() {
        let pre: Vec<Candle> = (0..20).map(|i| c(i, 100.5, 99.5, 100.0)).collect();
        assert_eq!(realized_vol(&pre, "1m"), Some(0.0));
        assert_eq!(trend_state(&pre), Some("flat"));
    }

    #[test]
    fn trend_reads_the_close_against_the_window() {
        let mut pre: Vec<Candle> = (0..20).map(|i| c(i, 101.0, 99.0, 100.0)).collect();
        pre.push(c(20, 111.0, 109.0, 110.0));
        assert_eq!(trend_state(&pre), Some("up"));
        assert!(trend_state(&pre[..5]).is_none());
    }

    fn metrics(vol: Option<f64>) -> market_store::TradeMetrics {
        market_store::TradeMetrics {
            trade_id: Uuid::new_v4(),
            dataset_id: None,
            timeframe: "1h".into(),
            bars: 10,
            mae: None,
            mfe: None,
            mae_price: None,
            mfe_price: None,
            mae_r: None,
            mfe_r: None,
            exit_efficiency: None,
            giveback: None,
            time_to_mae_min: None,
            time_to_mfe_min: None,
            atr_entry: None,
            vol_pct: vol,
            regime: None,
            trend: None,
            stop_distance_atr: None,
            stop_hit: None,
            computed_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    /// A bare open position: only the fields `correlate` reads.
    fn position(ticker: &str, risk: Option<f64>) -> exposure_store::Position {
        exposure_store::Position {
            id: Uuid::new_v4(),
            ticker: ticker.into(),
            asset_class: "stock".into(),
            side: "long".into(),
            currency: "EUR".into(),
            open_qty: 1.0,
            avg_entry: Some(100.0),
            stop_price: None,
            leverage: 1.0,
            entry_at: None,
            days_open: None,
            notional: 1000.0,
            risk,
            risk_pct: None,
            risk_share: None,
            last_price: None,
            last_at: None,
            unrealized: None,
            unrealized_pct: None,
            strategy_id: None,
            strategy: String::new(),
        }
    }

    /// A deterministic daily close series, `n` bars.
    fn closes(n: usize, f: impl Fn(usize) -> f64) -> Vec<(OffsetDateTime, f64)> {
        let base = OffsetDateTime::UNIX_EPOCH;
        (0..n).map(|i| (base + Duration::days(i as i64), f(i))).collect()
    }

    #[test]
    fn two_names_moving_as_one_are_reported_as_one_bet() {
        // Identical paths: perfect correlation, so the correlated risk is the plain sum
        // and the book is one bet wearing two names.
        let series = closes(120, |i| 100.0 * (1.0 + 0.01 * ((i % 7) as f64 - 3.0)));
        let mut map: BTreeMap<String, Vec<(OffsetDateTime, f64)>> = BTreeMap::new();
        map.insert("AAA".into(), series.clone());
        map.insert("BBB".into(), series);
        let positions = vec![position("AAA", Some(100.0)), position("BBB", Some(100.0))];

        let c = correlate(&positions, &map).expect("two covered instruments");
        assert_eq!(c.labels, vec!["AAA".to_string(), "BBB".to_string()]);
        assert!((c.avg_corr.unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(c.risk_sum, Some(200.0));
        // sqrt(100² + 100²) if independent, but they are not.
        assert!((c.risk_independent.unwrap() - 141.421356).abs() < 1e-4);
        assert!((c.risk_correlated.unwrap() - 200.0).abs() < 1e-6);
        assert!((c.stacking.unwrap() - std::f64::consts::SQRT_2).abs() < 1e-6);
        assert!((c.effective_bets.unwrap() - 1.0).abs() < 1e-9);
        assert!(c.uncovered.is_empty());
    }

    #[test]
    fn an_instrument_without_bars_is_named_not_guessed() {
        let mut map: BTreeMap<String, Vec<(OffsetDateTime, f64)>> = BTreeMap::new();
        map.insert("AAA".into(), closes(120, |i| 100.0 + (i % 5) as f64));
        map.insert("BBB".into(), closes(120, |i| 100.0 + (i % 3) as f64));
        let positions = vec![
            position("AAA", Some(100.0)),
            position("BBB", Some(100.0)),
            position("CCC", Some(100.0)),
        ];
        let c = correlate(&positions, &map).expect("two covered instruments");
        assert_eq!(c.labels.len(), 2);
        assert_eq!(c.uncovered, vec!["CCC".to_string()]);
    }

    #[test]
    fn one_instrument_or_too_few_bars_produces_no_matrix() {
        let mut map: BTreeMap<String, Vec<(OffsetDateTime, f64)>> = BTreeMap::new();
        map.insert("AAA".into(), closes(120, |i| 100.0 + i as f64));
        // A single covered instrument has nothing to correlate against.
        assert!(correlate(&[position("AAA", Some(100.0))], &map).is_none());

        // Two instruments, but a clock far too short to mean anything.
        let mut short: BTreeMap<String, Vec<(OffsetDateTime, f64)>> = BTreeMap::new();
        short.insert("AAA".into(), closes(10, |i| 100.0 + i as f64));
        short.insert("BBB".into(), closes(10, |i| 200.0 - i as f64));
        assert!(correlate(
            &[position("AAA", Some(1.0)), position("BBB", Some(1.0))],
            &short
        )
        .is_none());
    }

    #[test]
    fn a_measurement_expires_on_the_trade_the_bars_or_the_grain() {
        let mut row = metrics(None);
        row.computed_at = OffsetDateTime::UNIX_EPOCH + Duration::days(10);
        let older = OffsetDateTime::UNIX_EPOCH + Duration::days(5);
        let newer = OffsetDateTime::UNIX_EPOCH + Duration::days(20);

        assert!(still_valid(&row, older, "1h", Some(older)));
        // A dataset that has never been written cannot invalidate anything.
        assert!(still_valid(&row, older, "1h", None));
        // The trade was edited after the measurement.
        assert!(!still_valid(&row, newer, "1h", Some(older)));
        // A download landed after the measurement.
        assert!(!still_valid(&row, older, "1h", Some(newer)));
        // The grain moved.
        assert!(!still_valid(&row, older, "1d", Some(older)));
    }

    #[test]
    fn spans_merge_when_they_touch() {
        let d = |h: i64| OffsetDateTime::UNIX_EPOCH + Duration::hours(h);
        assert_eq!(
            merge_spans(vec![(d(5), d(7)), (d(0), d(2)), (d(1), d(3)), (d(7), d(9))]),
            vec![(d(0), d(3)), (d(5), d(9))]
        );
        assert!(merge_spans(vec![]).is_empty());
        // Disjoint spans stay disjoint: the hole between them is the whole point.
        assert_eq!(
            merge_spans(vec![(d(0), d(1)), (d(10), d(11))]),
            vec![(d(0), d(1)), (d(10), d(11))]
        );
    }

    #[test]
    fn regimes_are_terciles_of_the_book() {
        let mut rows: Vec<market_store::TradeMetrics> =
            (0..15).map(|i| metrics(Some(i as f64))).collect();
        assign_regimes(&mut rows);
        assert_eq!(rows[0].regime.as_deref(), Some("low"));
        assert_eq!(rows[7].regime.as_deref(), Some("normal"));
        assert_eq!(rows[14].regime.as_deref(), Some("high"));

        // Too small a batch is left unlabelled rather than split into thirds of noise.
        let mut few: Vec<market_store::TradeMetrics> = rows.drain(..5).collect();
        few.iter_mut().for_each(|r| r.regime = None);
        assign_regimes(&mut few);
        assert!(few.iter().all(|r| r.regime.is_none()));
    }
}

// ── Open-position risk ───────────────────────────────────────────────────────
//
// The store side (`otw_store::journal_exposure`) builds the position list, the totals and
// the concentration tables from the trade log alone. The two things it cannot do live
// here, because both need bars: marking the positions to the last stored close, and
// measuring how much the open instruments move together.

/// Bars read per instrument for the correlation. About a year of daily candles: long
/// enough for a correlation to mean something, short enough that it describes the market
/// the positions are actually in.
const CORR_BARS: i64 = 260;
/// Below this many aligned rows a correlation is noise and the block is not produced.
const MIN_CORR_ROWS: usize = 30;
/// Correlations are measured on daily candles whatever grain the enrichment uses: a
/// session-anchored 4h bar and an epoch-anchored one are 90 minutes apart, so an intraday
/// cross-asset correlation measures the stamping convention, not the market.
const CORR_TIMEFRAME: &str = "1d";

/// The open book, marked and correlated.
///
/// Everything past the trade log is best effort: an instrument with no stored bars is
/// simply not marked and not correlated, and it is named in `uncovered` so the screen can
/// say which positions the picture is missing.
pub async fn exposure(
    pool: &sqlx::PgPool,
    filter: &TradeFilter,
    display_currency: &str,
) -> Result<exposure_store::Exposure> {
    let mut e = exposure_store::exposure(pool, filter, display_currency).await?;
    if e.positions.is_empty() {
        return Ok(e);
    }
    let settings = market_store::get_settings(pool).await?;
    let granted = conn_store::list_for_module(pool, MODULE).await?;

    // One dataset lookup per distinct instrument, not per position.
    let mut daily: HashMap<String, Option<Uuid>> = HashMap::new();
    for p in &e.positions {
        let Some(asset_type) = asset_type_for(&p.asset_class) else {
            continue;
        };
        if daily.contains_key(&p.ticker) {
            continue;
        }
        // An unmapped future has no series to mark against; it lands in `uncovered`
        // below like any instrument with no stored bars.
        let Some(symbol) = provider_symbol(&settings.symbols, &p.asset_class, &p.ticker) else {
            continue;
        };
        let preferred =
            pick_connector(&granted, &settings.connectors, asset_type).map(|c| c.provider.clone());
        let id = store::find_dataset_any(
            pool,
            asset_type,
            &symbol,
            CORR_TIMEFRAME,
            preferred.as_deref(),
        )
        .await?
        .map(|d| d.id);
        daily.insert(p.ticker.clone(), id);
    }

    // Closes per instrument, newest window first, for both the mark and the correlation.
    let mut closes: BTreeMap<String, Vec<(OffsetDateTime, f64)>> = BTreeMap::new();
    for (ticker, id) in &daily {
        let Some(id) = id else { continue };
        let bars = store::read_bars(pool, *id, None, None, CORR_BARS).await?;
        if bars.is_empty() {
            continue;
        }
        closes.insert(
            ticker.clone(),
            bars.iter().map(|b| (b.ts, b.close)).collect(),
        );
    }

    // Mark to the last stored close. The FX pass already converted the entry notional, so
    // the unrealized amount rides the same rate: a position's PnL and its size have to be
    // in the same currency or the percentage is meaningless.
    let mut positions = std::mem::take(&mut e.positions);
    for p in positions.iter_mut() {
        let Some(series) = closes.get(&p.ticker) else {
            continue;
        };
        let Some((last_at, last)) = series.last().copied() else {
            continue;
        };
        let Some(entry) = p.avg_entry.filter(|v| *v > 0.0) else {
            continue;
        };
        let dir = if p.side.eq_ignore_ascii_case("short") {
            -1.0
        } else {
            1.0
        };
        p.last_price = Some(last);
        p.last_at = Some(last_at);
        let move_pct = (last - entry) / entry * dir * 100.0;
        p.unrealized_pct = Some(move_pct);
        p.unrealized = Some(p.notional * move_pct / 100.0);
    }

    let correlation = correlate(&positions, &closes);
    let total_risk: f64 = positions.iter().filter_map(|p| p.risk).sum();
    let mut sealed = exposure_store::seal(
        positions,
        e.invested_capital,
        e.unconverted,
        display_currency,
        total_risk,
    );
    sealed.correlation = correlation;
    // The correlated statements can only fire once the block exists.
    sealed.warnings = exposure_store::warnings(&sealed);
    Ok(sealed)
}

/// Pairwise correlation of the open instruments, and what it does to the total risk.
///
/// Returns `None` rather than a matrix nobody should read: fewer than two covered
/// instruments, or an aligned clock too short to mean anything.
fn correlate(
    positions: &[exposure_store::Position],
    closes: &BTreeMap<String, Vec<(OffsetDateTime, f64)>>,
) -> Option<exposure_store::CorrelationBlock> {
    // One entry per *instrument*, but the risk of every position on it: two lines on the
    // same ticker are one exposure for correlation purposes.
    let mut risk_of: BTreeMap<&str, f64> = BTreeMap::new();
    let mut uncovered: Vec<String> = Vec::new();
    for p in positions {
        if closes.contains_key(&p.ticker) {
            *risk_of.entry(p.ticker.as_str()).or_insert(0.0) += p.risk.unwrap_or(0.0);
        } else if !uncovered.contains(&p.ticker) {
            uncovered.push(p.ticker.clone());
        }
    }
    if risk_of.len() < 2 {
        return None;
    }

    // Align on the merged clock rather than on raw stamps: providers stamp the same daily
    // close differently, and an intersection over raw strings comes back empty across two
    // markets. Daily is never intraday, so `merge` keeps its bucketing path.
    let tickers: Vec<&str> = risk_of.keys().copied().collect();
    let stamps: Vec<Vec<String>> = tickers
        .iter()
        .map(|t| {
            closes[*t]
                .iter()
                .map(|(ts, _)| ts.format(&Rfc3339).unwrap_or_default())
                .collect()
        })
        .collect();
    let series: Vec<align::Series<'_>> = tickers
        .iter()
        .zip(&stamps)
        .map(|(t, ts)| align::Series { label: t, ts })
        .collect();
    let aligned = align::merge(&series, align::Grain::Infer, align::Join::Intersection);
    if aligned.rows() < MIN_CORR_ROWS + 1 {
        return None;
    }

    // Log returns over the shared clock, so every series has the same length and the same
    // periods. A row with a non-positive close is dropped from every series at once, which
    // is the only way to keep the columns comparable.
    let mut rets: Vec<Vec<f64>> = vec![Vec::with_capacity(aligned.rows()); tickers.len()];
    for row in 1..aligned.rows() {
        let mut step = Vec::with_capacity(tickers.len());
        let mut usable = true;
        for (ai, t) in tickers.iter().enumerate() {
            let (Some(prev), Some(cur)) = (aligned.maps[ai][row - 1], aligned.maps[ai][row]) else {
                usable = false;
                break;
            };
            let (a, b) = (closes[*t][prev].1, closes[*t][cur].1);
            if a <= 0.0 || b <= 0.0 {
                usable = false;
                break;
            }
            step.push((b / a).ln());
        }
        if usable {
            for (ai, r) in step.into_iter().enumerate() {
                rets[ai].push(r);
            }
        }
    }
    let observations = rets[0].len();
    if observations < MIN_CORR_ROWS {
        return None;
    }

    let labels: Vec<String> = tickers.iter().map(|t| (*t).to_string()).collect();
    let m = crate::quant::correlation_matrix(&labels, &rets);
    let n = labels.len();

    // Off-diagonal summary: the average pair, and the worst one.
    let mut sum = 0.0;
    let mut count = 0usize;
    let mut max_pair: Option<(String, String, f64)> = None;
    for i in 0..n {
        for j in (i + 1)..n {
            let c = m.matrix[i][j];
            sum += c;
            count += 1;
            if max_pair.as_ref().is_none_or(|(_, _, best)| c > *best) {
                max_pair = Some((labels[i].clone(), labels[j].clone(), c));
            }
        }
    }

    // What the correlations do to the money. `risk_sum` is what a trader assumes when
    // adding stops up; `risk_independent` is what it would be if nothing moved together;
    // `risk_correlated` is the quadratic form that actually describes the book.
    let risks: Vec<f64> = tickers.iter().map(|t| risk_of[*t]).collect();
    let risk_sum: f64 = risks.iter().sum();
    let has_risk = risk_sum > 0.0;
    let risk_independent = has_risk.then(|| risks.iter().map(|r| r * r).sum::<f64>().sqrt());
    let risk_correlated = has_risk.then(|| {
        let mut acc = 0.0;
        for i in 0..n {
            for j in 0..n {
                acc += risks[i] * risks[j] * m.matrix[i][j];
            }
        }
        // A negatively correlated book can drive the form to (a rounding of) zero.
        acc.max(0.0).sqrt()
    });

    let avg_corr = (count > 0).then(|| sum / count as f64);
    Some(exposure_store::CorrelationBlock {
        labels,
        matrix: m.matrix,
        observations: observations as i64,
        timeframe: CORR_TIMEFRAME.to_string(),
        uncovered,
        avg_corr,
        max_pair,
        risk_sum: has_risk.then_some(risk_sum),
        risk_independent,
        risk_correlated,
        stacking: match (risk_correlated, risk_independent) {
            (Some(c), Some(i)) if i > 0.0 => Some(c / i),
            _ => None,
        },
        // Equal-weight effective bets: N / (1 + (N−1)·ρ̄). One at perfect correlation,
        // N when nothing moves together.
        effective_bets: avg_corr.and_then(|rho| {
            let d = 1.0 + (n as f64 - 1.0) * rho;
            (d > 0.0).then(|| n as f64 / d)
        }),
    })
}
