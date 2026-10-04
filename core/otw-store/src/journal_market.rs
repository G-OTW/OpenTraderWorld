//! Persistence for the journal's market-data enrichment.
//!
//! Two things live here and nothing else: the settings that say **where** the candles
//! come from, and the per-trade measurements taken **against** them
//! (`journal_trade_metrics`).
//!
//! The measurements are a cache, never a source of truth: every column is a pure
//! function of (the trade, the bars covering it), so a recompute is always allowed to
//! throw the row away and build it again. What is stored in the trade's own currency
//! stays in the trade's own currency, exactly like `journal.net_pnl`, and the analytics
//! layer converts it in the same FX pass as everything else.
//!
//! The fetching itself is not here and not anywhere in this crate: `otw-core`'s
//! `journal_market` queues the missing windows through the ordinary histdata download
//! queue, so the journal owns no provider code.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

// ── Settings ─────────────────────────────────────────────────────────────────

/// How the journal is allowed to fetch and read market data.
#[derive(Debug, Clone, Serialize)]
pub struct MarketSettings {
    /// Connector chosen per histdata asset type, `{"crypto": "<uuid>"}`. Empty means
    /// "pick the first connector granted to the journal that serves the asset type".
    pub connectors: JsonValue,
    /// `auto` (derive the grain from how long trades are held) or an explicit timeframe.
    pub timeframe: String,
    /// `off` | `manual` | `auto`.
    pub sync_mode: String,
    /// The symbol handed to the provider per instrument, `{"future:MNQ": "MNQU6"}`,
    /// keyed `<asset_class>:<TICKER>`. Only instruments whose journal ticker is not a
    /// provider symbol need one: a future names no contract until its month is written.
    pub symbols: JsonValue,
}

pub const SYNC_MODES: [&str; 3] = ["off", "manual", "auto"];

impl Default for MarketSettings {
    fn default() -> Self {
        MarketSettings {
            connectors: JsonValue::Object(Default::default()),
            timeframe: "auto".into(),
            sync_mode: "manual".into(),
            symbols: JsonValue::Object(Default::default()),
        }
    }
}

/// Patch of the market block. Absent fields keep their stored value.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct MarketPatch {
    #[schemars(with = "Option<serde_json::Value>")]
    pub connectors: Option<JsonValue>,
    pub timeframe: Option<String>,
    pub sync_mode: Option<String>,
    #[schemars(with = "Option<serde_json::Value>")]
    pub symbols: Option<JsonValue>,
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<MarketSettings> {
    let row: Option<(JsonValue, String, String, JsonValue)> = sqlx::query_as(
        "SELECT market_connectors, market_timeframe, market_sync_mode, market_symbols \
         FROM journal_settings WHERE id = TRUE",
    )
    .fetch_optional(pool)
    .await
    .context("loading journal market settings")?;
    Ok(match row {
        Some((connectors, timeframe, sync_mode, symbols)) => MarketSettings {
            connectors,
            timeframe,
            sync_mode,
            symbols,
        },
        None => MarketSettings::default(),
    })
}

/// Write the market block, creating the singleton settings row if it is not there yet.
/// Every field is passed through: the caller validated them.
pub async fn set_settings(pool: &PgPool, s: &MarketSettings) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_settings (id, market_connectors, market_timeframe, market_sync_mode, \
                                       market_symbols, updated_at) \
         VALUES (TRUE, $1, $2, $3, $4, now()) \
         ON CONFLICT (id) DO UPDATE SET market_connectors = $1, market_timeframe = $2, \
                                        market_sync_mode = $3, market_symbols = $4, \
                                        updated_at = now()",
    )
    .bind(&s.connectors)
    .bind(&s.timeframe)
    .bind(&s.sync_mode)
    .bind(&s.symbols)
    .execute(pool)
    .await
    .context("saving journal market settings")?;
    Ok(())
}

// ── Per-trade measurements ───────────────────────────────────────────────────

/// What the bars said about one trade. Money columns are in the **trade's own
/// currency**; `*_r` columns are ratios and carry no currency at all.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TradeMetrics {
    pub trade_id: Uuid,
    pub dataset_id: Option<Uuid>,
    pub timeframe: String,
    pub bars: i32,
    /// Worst unrealized loss while the position was open, reported positive.
    pub mae: Option<f64>,
    /// Best unrealized profit while the position was open.
    pub mfe: Option<f64>,
    pub mae_price: Option<f64>,
    pub mfe_price: Option<f64>,
    pub mae_r: Option<f64>,
    pub mfe_r: Option<f64>,
    /// Captured result over the best available one, in percent.
    pub exit_efficiency: Option<f64>,
    /// MFE minus what was captured: money left on the table.
    pub giveback: Option<f64>,
    pub time_to_mae_min: Option<i64>,
    pub time_to_mfe_min: Option<i64>,
    pub atr_entry: Option<f64>,
    pub vol_pct: Option<f64>,
    /// `low` | `normal` | `high`, from the realized volatility before entry.
    pub regime: Option<String>,
    /// `up` | `down` | `flat`, from the close against a slow average before entry.
    pub trend: Option<String>,
    /// Planned stop distance in ATR: below ~1 the stop sits inside the noise.
    pub stop_distance_atr: Option<f64>,
    /// Whether price actually traded through the planned stop while open.
    pub stop_hit: Option<bool>,
    #[serde(with = "time::serde::rfc3339")]
    pub computed_at: OffsetDateTime,
}

const METRIC_COLS: &str = "trade_id, dataset_id, timeframe, bars, mae, mfe, mae_price, \
                           mfe_price, mae_r, mfe_r, exit_efficiency, giveback, \
                           time_to_mae_min, time_to_mfe_min, atr_entry, vol_pct, regime, \
                           trend, stop_distance_atr, stop_hit, computed_at";

/// Replace the measurements of the given trades. One statement per row: the batch is one
/// recompute of a filtered scope, not a bulk import, and a per-row upsert keeps the
/// "trades that could not be measured" case (no row written) trivially correct.
pub async fn upsert_metrics(pool: &PgPool, rows: &[TradeMetrics]) -> anyhow::Result<u64> {
    let mut written = 0u64;
    for m in rows {
        let r = sqlx::query(
            "INSERT INTO journal_trade_metrics \
             (trade_id, dataset_id, timeframe, bars, mae, mfe, mae_price, mfe_price, \
              mae_r, mfe_r, exit_efficiency, giveback, time_to_mae_min, time_to_mfe_min, \
              atr_entry, vol_pct, regime, trend, stop_distance_atr, stop_hit, computed_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20, now()) \
             ON CONFLICT (trade_id) DO UPDATE SET \
               dataset_id = $2, timeframe = $3, bars = $4, mae = $5, mfe = $6, \
               mae_price = $7, mfe_price = $8, mae_r = $9, mfe_r = $10, \
               exit_efficiency = $11, giveback = $12, time_to_mae_min = $13, \
               time_to_mfe_min = $14, atr_entry = $15, vol_pct = $16, regime = $17, \
               trend = $18, stop_distance_atr = $19, stop_hit = $20, computed_at = now()",
        )
        .bind(m.trade_id)
        .bind(m.dataset_id)
        .bind(&m.timeframe)
        .bind(m.bars)
        .bind(m.mae)
        .bind(m.mfe)
        .bind(m.mae_price)
        .bind(m.mfe_price)
        .bind(m.mae_r)
        .bind(m.mfe_r)
        .bind(m.exit_efficiency)
        .bind(m.giveback)
        .bind(m.time_to_mae_min)
        .bind(m.time_to_mfe_min)
        .bind(m.atr_entry)
        .bind(m.vol_pct)
        .bind(&m.regime)
        .bind(&m.trend)
        .bind(m.stop_distance_atr)
        .bind(m.stop_hit)
        .execute(pool)
        .await
        .context("writing trade metrics")?;
        written += r.rows_affected();
    }
    Ok(written)
}

/// Measurements for the given trades, keyed by trade id. An id with no row simply has no
/// entry: the caller renders that trade as unmeasured rather than as a zero.
pub async fn metrics_for(
    pool: &PgPool,
    trade_ids: &[Uuid],
) -> anyhow::Result<HashMap<Uuid, TradeMetrics>> {
    if trade_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!("SELECT {METRIC_COLS} FROM journal_trade_metrics WHERE trade_id = ANY($1)");
    let rows = sqlx::query_as::<_, TradeMetrics>(sqlx::AssertSqlSafe(sql))
        .bind(trade_ids)
        .fetch_all(pool)
        .await
        .context("loading trade metrics")?;
    Ok(rows.into_iter().map(|m| (m.trade_id, m)).collect())
}

/// How many of these trades carry a measurement. Cheaper than loading the rows when the
/// caller only wants to know whether the enriched cards can be drawn.
pub async fn measured_count(pool: &PgPool, trade_ids: &[Uuid]) -> anyhow::Result<i64> {
    if trade_ids.is_empty() {
        return Ok(0);
    }
    let n: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM journal_trade_metrics WHERE trade_id = ANY($1)",
    )
    .bind(trade_ids)
    .fetch_one(pool)
    .await
    .context("counting trade metrics")?;
    Ok(n.0)
}

/// Drop the measurements of the given trades, or of every trade when `trade_ids` is
/// `None`. Used before a full recompute and when the user changes the grain.
pub async fn clear_metrics(pool: &PgPool, trade_ids: Option<&[Uuid]>) -> anyhow::Result<u64> {
    let r = match trade_ids {
        Some([]) => return Ok(0),
        Some(ids) => sqlx::query("DELETE FROM journal_trade_metrics WHERE trade_id = ANY($1)")
            .bind(ids)
            .execute(pool)
            .await
            .context("clearing trade metrics")?,
        None => sqlx::query("DELETE FROM journal_trade_metrics")
            .execute(pool)
            .await
            .context("clearing trade metrics")?,
    };
    Ok(r.rows_affected())
}

// ── Aggregates over the measured trades ──────────────────────────────────────
//
// Same shape as `journal_behavior`: pure functions over the closed-trade projection the
// analytics loader already built, so the enriched cards cost no extra query and read the
// same filter, the same period and the same display currency as everything else.

use crate::journal_analytics::{mean, total, Bucket, Closed, GroupRow};

/// Volatility regimes and trend states, in the order the tables read.
pub const REGIMES: [&str; 3] = ["low", "normal", "high"];
pub const TRENDS: [&str; 3] = ["up", "flat", "down"];

/// One trade's measurements as the analytics layer sees them: money already converted
/// into the display currency, ratios untouched.
#[derive(Debug, Clone)]
pub struct ClosedMarket {
    pub mae: Option<f64>,
    pub mfe: Option<f64>,
    pub mae_r: Option<f64>,
    pub mfe_r: Option<f64>,
    pub exit_efficiency: Option<f64>,
    pub giveback: Option<f64>,
    pub time_to_mae_min: Option<i64>,
    pub time_to_mfe_min: Option<i64>,
    pub vol_pct: Option<f64>,
    pub regime: Option<String>,
    pub trend: Option<String>,
    pub stop_distance_atr: Option<f64>,
    pub stop_hit: Option<bool>,
    pub timeframe: String,
    pub bars: i32,
}

/// One measured trade, for the excursion scatter. Joined to `Analytics.points` by id on
/// the client, so nothing here repeats what that array already carries.
#[derive(Debug, Serialize)]
pub struct MarketPoint {
    pub id: Uuid,
    pub mae: Option<f64>,
    pub mfe: Option<f64>,
    pub mae_r: Option<f64>,
    pub mfe_r: Option<f64>,
    pub exit_efficiency: Option<f64>,
    pub giveback: Option<f64>,
    pub vol_pct: Option<f64>,
    pub regime: Option<String>,
    pub trend: Option<String>,
    pub stop_distance_atr: Option<f64>,
    pub stop_hit: Option<bool>,
}

/// What the candles say about the filtered trades.
///
/// Every count below is paired with the money it represents: "34 trades gave back a
/// profit" is an observation, "34 trades gave back 12 400 EUR" is a decision.
#[derive(Debug, Serialize)]
pub struct MarketStats {
    /// Measured trades, and the closed trades they were taken from.
    pub measured: i64,
    pub closed: i64,
    /// The grain most of the measurements were taken on.
    pub timeframe: String,
    pub avg_mae: Option<f64>,
    pub avg_mfe: Option<f64>,
    pub avg_mae_r: Option<f64>,
    pub avg_mfe_r: Option<f64>,
    /// Mean share of the available move the exit actually kept, in percent.
    pub avg_efficiency: Option<f64>,
    pub median_efficiency: Option<f64>,
    /// Money that was on the table at the best moment and did not come home.
    pub total_giveback: f64,
    /// Trades that were in profit at some point and closed red, and what they had shown.
    pub gave_back: i64,
    pub gave_back_mfe: f64,
    pub gave_back_net: f64,
    /// Winners that first went at least 80 % of the way to their own stop: the stop is
    /// sitting inside the noise of trades that end up working.
    pub near_stop_winners: i64,
    pub winners_with_stop: i64,
    /// Planned stop distance measured in ATR, and how many sat under one ATR.
    pub median_stop_atr: Option<f64>,
    pub tight_stops: i64,
    pub stops_with_atr: i64,
    /// Trades where price traded through the planned stop while the position was open.
    pub stop_hits: i64,
    /// Winners that kept under half of the move they were offered.
    pub short_targets: i64,
    pub winners_measured: i64,
    /// Median minutes from entry to the worst point, and to the best one.
    pub median_time_to_mae: Option<f64>,
    pub median_time_to_mfe: Option<f64>,
    /// Outcome by volatility regime and by trend state, same columns as every other
    /// group table on the screen.
    pub regimes: Vec<GroupRow>,
    pub trends: Vec<GroupRow>,
    /// MAE in units of risk: how deep the book goes before it works.
    pub mae_buckets: Vec<Bucket>,
    pub points: Vec<MarketPoint>,
}

/// Upper bounds of the MAE histogram, in R. The last bucket is open-ended: everything
/// past 1R went through the planned stop.
const MAE_EDGES: [(f64, f64, &str); 6] = [
    (0.0, 0.25, "r025"),
    (0.25, 0.5, "r05"),
    (0.5, 0.75, "r075"),
    (0.75, 1.0, "r1"),
    (1.0, 1.5, "r15"),
    (1.5, f64::INFINITY, "gt15"),
];

fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    Some(if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    })
}

/// Build one group table over a labelled slice of the measured trades. Mirrors
/// `journal_analytics::group_by`, but the key comes from the measurement rather than
/// from the trade, and the row order is the fixed vocabulary so the table never
/// reshuffles between two loads.
fn group_measured<F>(closed: &[Closed], keys: &[&str], key_of: F) -> Vec<GroupRow>
where
    F: Fn(&ClosedMarket) -> Option<String>,
{
    let total_measured = closed.iter().filter(|c| c.market.is_some()).count() as i64;
    keys.iter()
        .map(|k| {
            let members: Vec<&Closed> = closed
                .iter()
                .filter(|c| {
                    c.market
                        .as_ref()
                        .and_then(&key_of)
                        .is_some_and(|got| got == *k)
                })
                .collect();
            let nets: Vec<f64> = members.iter().map(|c| c.net).collect();
            let wins = nets.iter().filter(|n| **n > 0.0).count() as i64;
            let losses = nets.iter().filter(|n| **n < 0.0).count() as i64;
            let gross_win = total(nets.iter().copied().filter(|n| *n > 0.0));
            let gross_loss = total(nets.iter().copied().filter(|n| *n < 0.0)).abs();
            let rs: Vec<f64> = members.iter().filter_map(|c| c.r).collect();
            GroupRow {
                key: (*k).to_string(),
                name: (*k).to_string(),
                kind: String::new(),
                trades: nets.len() as i64,
                wins,
                losses,
                net: total(nets.iter().copied()),
                fees: total(members.iter().map(|c| c.fees)),
                win_rate: (!nets.is_empty())
                    .then(|| wins as f64 / nets.len() as f64 * 100.0),
                expectancy: mean(&nets),
                profit_factor: (gross_loss > 0.0).then(|| gross_win / gross_loss),
                avg_r: mean(&rs),
                best: nets.iter().copied().reduce(f64::max),
                worst: nets.iter().copied().reduce(f64::min),
                share: (total_measured > 0)
                    .then(|| nets.len() as f64 / total_measured as f64 * 100.0),
            }
        })
        .collect()
}

/// The enriched block, or `None` when not one trade in the scope has been measured yet
/// (the screen then draws the cards disabled instead of drawing zeros).
pub(crate) fn stats(closed: &[Closed]) -> Option<MarketStats> {
    let measured: Vec<&Closed> = closed.iter().filter(|c| c.market.is_some()).collect();
    if measured.is_empty() {
        return None;
    }
    fn m(c: &Closed) -> &ClosedMarket {
        c.market.as_ref().expect("the vector was filtered on Some")
    }

    // The grain most measurements were taken on. A mixed scope is possible (a recompute
    // after a grain change), and naming the majority is more honest than naming the
    // setting, which may already have moved on.
    let mut grains: HashMap<&str, i64> = HashMap::new();
    for c in &measured {
        *grains.entry(m(c).timeframe.as_str()).or_default() += 1;
    }
    let timeframe = grains
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(tf, _)| (*tf).to_string())
        .unwrap_or_default();

    let maes: Vec<f64> = measured.iter().filter_map(|c| m(c).mae).collect();
    let mfes: Vec<f64> = measured.iter().filter_map(|c| m(c).mfe).collect();
    let mae_rs: Vec<f64> = measured.iter().filter_map(|c| m(c).mae_r).collect();
    let mfe_rs: Vec<f64> = measured.iter().filter_map(|c| m(c).mfe_r).collect();
    let effs: Vec<f64> = measured.iter().filter_map(|c| m(c).exit_efficiency).collect();
    let stop_atrs: Vec<f64> = measured
        .iter()
        .filter_map(|c| m(c).stop_distance_atr)
        .collect();

    // A trade that showed a profit and closed red. The floor is the measurement grain
    // itself: an excursion smaller than a tick is noise, not a give-back.
    let gave_back: Vec<&&Closed> = measured
        .iter()
        .filter(|c| c.net < 0.0 && m(c).mfe.is_some_and(|v| v > 0.0))
        .collect();

    let winners: Vec<&&Closed> = measured.iter().filter(|c| c.net > 0.0).collect();
    let winners_with_stop = winners.iter().filter(|c| m(c).mae_r.is_some()).count() as i64;
    let near_stop_winners = winners
        .iter()
        .filter(|c| m(c).mae_r.is_some_and(|v| v >= 0.8))
        .count() as i64;
    let short_targets = winners
        .iter()
        .filter(|c| m(c).exit_efficiency.is_some_and(|e| e < 50.0))
        .count() as i64;

    let mut mae_buckets: Vec<Bucket> = MAE_EDGES
        .iter()
        .map(|(lo, hi, key)| {
            let mut b = Bucket::new(*key);
            b.lo = Some(*lo);
            b.hi = hi.is_finite().then_some(*hi);
            b
        })
        .collect();
    for c in &measured {
        let Some(r) = m(c).mae_r else { continue };
        let idx = MAE_EDGES
            .iter()
            .position(|(lo, hi, _)| r >= *lo && r < *hi)
            .unwrap_or(MAE_EDGES.len() - 1);
        mae_buckets[idx].push(c.net);
    }

    let points = measured
        .iter()
        .map(|c| {
            let mk = m(c);
            MarketPoint {
                id: c.id,
                mae: mk.mae,
                mfe: mk.mfe,
                mae_r: mk.mae_r,
                mfe_r: mk.mfe_r,
                exit_efficiency: mk.exit_efficiency,
                giveback: mk.giveback,
                vol_pct: mk.vol_pct,
                regime: mk.regime.clone(),
                trend: mk.trend.clone(),
                stop_distance_atr: mk.stop_distance_atr,
                stop_hit: mk.stop_hit,
            }
        })
        .collect();

    Some(MarketStats {
        measured: measured.len() as i64,
        closed: closed.len() as i64,
        timeframe,
        avg_mae: mean(&maes),
        avg_mfe: mean(&mfes),
        avg_mae_r: mean(&mae_rs),
        avg_mfe_r: mean(&mfe_rs),
        avg_efficiency: mean(&effs),
        median_efficiency: median(&effs),
        total_giveback: total(measured.iter().filter_map(|c| m(c).giveback)),
        gave_back: gave_back.len() as i64,
        gave_back_mfe: total(gave_back.iter().filter_map(|c| m(c).mfe)),
        gave_back_net: total(gave_back.iter().map(|c| c.net)),
        near_stop_winners,
        winners_with_stop,
        median_stop_atr: median(&stop_atrs),
        tight_stops: stop_atrs.iter().filter(|v| **v < 1.0).count() as i64,
        stops_with_atr: stop_atrs.len() as i64,
        stop_hits: measured
            .iter()
            .filter(|c| m(c).stop_hit == Some(true))
            .count() as i64,
        short_targets,
        winners_measured: winners.len() as i64,
        median_time_to_mae: median(
            &measured
                .iter()
                .filter_map(|c| m(c).time_to_mae_min.map(|v| v as f64))
                .collect::<Vec<_>>(),
        ),
        median_time_to_mfe: median(
            &measured
                .iter()
                .filter_map(|c| m(c).time_to_mfe_min.map(|v| v as f64))
                .collect::<Vec<_>>(),
        ),
        regimes: group_measured(closed, &REGIMES, |mk| mk.regime.clone()),
        trends: group_measured(closed, &TRENDS, |mk| mk.trend.clone()),
        mae_buckets: mae_buckets.into_iter().map(Bucket::seal).collect(),
        points,
    })
}
