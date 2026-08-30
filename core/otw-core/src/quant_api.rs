//! HTTP API for the Quant Tools module.
//!
//! - `POST /api/quant/single`     one dataset → HV, Max DD, VaR, CVaR (+ curves)
//! - `POST /api/quant/kelly`      manual win-rate/payoff → Kelly fractions
//! - `POST /api/quant/portfolio`  N datasets → correlation, efficient frontier, risk parity
//!
//! Stateless like the backtest module: dataset ids in, metrics out. Bars come from the
//! histdata catalog (the same data the visualization and backtest modules consume), so the
//! Historical Data module must be installed and have downloaded datasets. Multi-asset
//! endpoints align series through [`crate::align`] (by period, not by raw timestamp), drop a
//! mixed-calendar basket to weekly, and annualize on the measured clock.

use axum::{routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::quant;
use crate::{align, timeframe};
use crate::{ApiError, AppState};
use otw_store::histdata as hd;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/quant/single", post(single))
        .route("/api/quant/kelly", post(kelly))
        .route("/api/quant/size", post(size))
        .route("/api/quant/asset-signals", post(asset_signals))
        .route("/api/quant/seasonality", post(seasonality))
        .route("/api/quant/portfolio", post(portfolio))
}

const MAX_BARS: i64 = 200_000;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SingleBody {
    dataset_id: Uuid,
    /// Confidence level for VaR/CVaR (e.g. 0.95). Clamped to a sane band.
    #[serde(default = "default_confidence")]
    confidence: f64,
    /// Optional RFC3339 inclusive lower bound on bar timestamps (start of the analysis window).
    #[serde(default)]
    from: Option<String>,
    /// Optional RFC3339 inclusive upper bound on bar timestamps (end of the analysis window).
    #[serde(default)]
    until: Option<String>,
}

fn default_confidence() -> f64 {
    0.95
}

/// Parse an optional RFC3339 bound; a malformed value is a client error rather than silently
/// ignored so the returned window matches what was asked for.
fn parse_bound(s: &Option<String>, field: &str) -> Result<Option<OffsetDateTime>, ApiError> {
    match s.as_deref().filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => OffsetDateTime::parse(v, &Rfc3339)
            .map(Some)
            .map_err(|_| ApiError::bad_request(&format!("invalid {field} timestamp"))),
    }
}

/// Load a dataset's close series + RFC3339 timestamps in ascending time order, optionally
/// restricted to the [from, until] window.
async fn load_closes(
    state: &AppState,
    id: Uuid,
    from: Option<OffsetDateTime>,
    until: Option<OffsetDateTime>,
) -> Result<(hd::Dataset, Vec<String>, Vec<f64>), ApiError> {
    let ds = hd::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let bars = hd::read_bars(&state.pool, id, from, until, MAX_BARS).await?;
    if bars.len() < 2 {
        return Err(ApiError::bad_request(
            "dataset has too few bars in the selected range to analyze",
        ));
    }
    let ts: Vec<String> =
        bars.iter().map(|b| b.ts.format(&Rfc3339).unwrap_or_default()).collect();
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    Ok((ds, ts, closes))
}

/// Like [`load_closes`] but also returns high/low series, for asset-signal derivation (ATR, swings).
async fn load_ohlc(
    state: &AppState,
    id: Uuid,
    from: Option<OffsetDateTime>,
    until: Option<OffsetDateTime>,
) -> Result<(hd::Dataset, Vec<String>, Vec<f64>, Vec<f64>, Vec<f64>), ApiError> {
    let ds = hd::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let bars = hd::read_bars(&state.pool, id, from, until, MAX_BARS).await?;
    if bars.len() < 2 {
        return Err(ApiError::bad_request(
            "dataset has too few bars in the selected range to analyze",
        ));
    }
    let ts: Vec<String> =
        bars.iter().map(|b| b.ts.format(&Rfc3339).unwrap_or_default()).collect();
    let highs: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let lows: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    Ok((ds, ts, highs, lows, closes))
}

/// Parse a "long"/"short" side string; anything else is a client error.
fn parse_side(s: &str) -> Result<quant::Side, ApiError> {
    match s.trim().to_lowercase().as_str() {
        "long" | "buy" => Ok(quant::Side::Long),
        "short" | "sell" => Ok(quant::Side::Short),
        _ => Err(ApiError::bad_request("side must be \"long\" or \"short\"")),
    }
}

async fn single(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<SingleBody>,
) -> Result<Json<Value>, ApiError> {
    let conf = body.confidence.clamp(0.5, 0.999);
    let from = parse_bound(&body.from, "from")?;
    let until = parse_bound(&body.until, "until")?;
    let (ds, ts, closes) = load_closes(&state, body.dataset_id, from, until).await?;
    let result = quant::analyze_single(&closes, &ts, &ds.timeframe, conf);
    Ok(Json(json!({
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "result": result,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct KellyBody {
    /// Win rate as a fraction (0.55 = 55%).
    win_rate: f64,
    /// Average winning trade magnitude (positive).
    avg_win: f64,
    /// Average losing trade magnitude (positive).
    avg_loss: f64,
}

async fn kelly(Json(body): Json<KellyBody>) -> Result<Json<Value>, ApiError> {
    if body.avg_win < 0.0 || body.avg_loss < 0.0 {
        return Err(ApiError::bad_request("avg_win and avg_loss must be positive"));
    }
    Ok(Json(json!(quant::kelly(body.win_rate, body.avg_win, body.avg_loss))))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SizeBody {
    /// Account/stack capital.
    stack: f64,
    /// "long" or "short".
    side: String,
    entry: f64,
    stop: f64,
    /// Risk as a fraction of stack (0.01 = 1%). Ignored when `risk_amount` is set.
    #[serde(default)]
    risk_pct: Option<f64>,
    /// Fixed risk in currency; takes precedence over `risk_pct` when present.
    #[serde(default)]
    risk_amount: Option<f64>,
    /// Contract/lot multiplier (P&L per 1.0 price move per unit). Defaults to 1.
    #[serde(default = "default_multiplier")]
    multiplier: f64,
    /// Optional leverage (>1) for the margin/over-leverage read-through.
    #[serde(default)]
    leverage: Option<f64>,
    /// Optional take-profit price for the reward read-through.
    #[serde(default)]
    target: Option<f64>,
}

fn default_multiplier() -> f64 {
    1.0
}

async fn size(Json(body): Json<SizeBody>) -> Result<Json<Value>, ApiError> {
    if body.stack < 0.0 || body.entry <= 0.0 {
        return Err(ApiError::bad_request("stack must be ≥ 0 and entry > 0"));
    }
    let side = parse_side(&body.side)?;
    // Resolve the risk budget: a fixed amount wins, else risk_pct × stack (default 1%).
    let risk_amount = match body.risk_amount {
        Some(a) if a >= 0.0 => a,
        Some(_) => return Err(ApiError::bad_request("risk_amount must be ≥ 0")),
        None => body.risk_pct.unwrap_or(0.01).clamp(0.0, 10.0) * body.stack,
    };
    let result = quant::position_size(
        body.stack,
        risk_amount,
        body.entry,
        body.stop,
        side,
        body.multiplier,
        body.leverage,
        body.target,
    );
    Ok(Json(json!(result)))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AssetSignalsBody {
    dataset_id: Uuid,
    /// "long" or "short".
    side: String,
    /// Entry price; when ≤ 0 the window's last close is used.
    #[serde(default)]
    entry: f64,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default = "default_atr_period")]
    atr_period: usize,
    #[serde(default = "default_swing_lookback")]
    swing_lookback: usize,
}

fn default_atr_period() -> usize {
    14
}
fn default_swing_lookback() -> usize {
    20
}

async fn asset_signals(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<AssetSignalsBody>,
) -> Result<Json<Value>, ApiError> {
    let side = parse_side(&body.side)?;
    let from = parse_bound(&body.from, "from")?;
    let until = parse_bound(&body.until, "until")?;
    let (ds, ts, highs, lows, closes) = load_ohlc(&state, body.dataset_id, from, until).await?;
    let result = quant::asset_signals(
        &highs,
        &lows,
        &closes,
        &ts,
        &ds.timeframe,
        body.entry,
        side,
        body.atr_period.clamp(1, 500),
        body.swing_lookback.clamp(1, 5_000),
    );
    Ok(Json(json!({
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "signals": result,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SeasonalityBody {
    dataset_id: Uuid,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    /// "return" (mean period return, default) or "volatility" (stddev of period returns).
    #[serde(default)]
    metric: Option<String>,
}

/// Seasonality heatmaps (month / weekday / hour) of one dataset's period returns. Hour axis is
/// only meaningful for intraday timeframes, so it is suppressed for daily-and-slower data.
async fn seasonality(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<SeasonalityBody>,
) -> Result<Json<Value>, ApiError> {
    let from = parse_bound(&body.from, "from")?;
    let until = parse_bound(&body.until, "until")?;
    let metric_vol = matches!(body.metric.as_deref(), Some("volatility") | Some("vol"));

    let ds = hd::get_dataset(&state.pool, body.dataset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let bars = hd::read_bars(&state.pool, body.dataset_id, from, until, MAX_BARS).await?;
    if bars.len() < 2 {
        return Err(ApiError::bad_request(
            "dataset has too few bars in the selected range to analyze",
        ));
    }

    // Intraday timeframes (≥ ~2 bars/day) get the hour axis; daily+ don't.
    let has_hour = quant::periods_per_year(&ds.timeframe) >= 2.0 * 252.0;
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    // Calendar components per bar: month 0=Jan, weekday 0=Mon, hour 0..23 (UTC — the clock the
    // bars are stored on).
    let months: Vec<u8> = bars.iter().map(|b| b.ts.month() as u8 - 1).collect();
    let weekdays: Vec<u8> =
        bars.iter().map(|b| b.ts.weekday().number_days_from_monday()).collect();
    let hours: Vec<u8> = bars.iter().map(|b| b.ts.hour()).collect();

    let result =
        quant::seasonality(&closes, &months, &weekdays, &hours, metric_vol, has_hour);
    Ok(Json(json!({
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "result": result,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PortfolioBody {
    /// Two or more dataset ids to analyze together.
    dataset_ids: Vec<Uuid>,
    /// Monte-Carlo samples for the efficient frontier. Clamped.
    #[serde(default = "default_samples")]
    samples: usize,
    /// Annual risk-free rate for Sharpe (e.g. 0.0).
    #[serde(default)]
    risk_free: f64,
    /// Granularity the statistics are measured at: `"auto"` (default) drops a basket that
    /// mixes a 24/7 market with an exchange-hours one to weekly, `"stored"` forces the
    /// datasets' own timeframe, or name one explicitly (`"1w"`, `"1M"`). Coarser than the
    /// stored timeframe only: nothing here can invent finer bars.
    #[serde(default)]
    measure: Option<String>,
}

fn default_samples() -> usize {
    5000
}

async fn portfolio(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<PortfolioBody>,
) -> Result<Json<Value>, ApiError> {
    if body.dataset_ids.len() < 2 {
        return Err(ApiError::bad_request("portfolio needs at least 2 datasets"));
    }
    if body.dataset_ids.len() > 20 {
        return Err(ApiError::bad_request("at most 20 datasets"));
    }

    // Load each asset's timestamps + closes, in ascending time order.
    let mut labels = Vec::new();
    let mut providers = Vec::new();
    let mut timeframes = Vec::new();
    let mut stamps: Vec<Vec<String>> = Vec::new();
    let mut closes: Vec<Vec<f64>> = Vec::new();
    for id in &body.dataset_ids {
        let (ds, ts, c) = load_closes(&state, *id, None, None).await?;
        labels.push(ds.ticker.clone());
        providers.push(ds.provider.clone());
        timeframes.push(ds.timeframe.clone());
        stamps.push(ts);
        closes.push(c);
    }

    // A shared timeframe is a precondition, not an assumption: mixing daily and hourly bars
    // in one covariance matrix has no meaning, and the annualization factor would silently be
    // the first dataset's.
    if timeframes.iter().any(|t| t != &timeframes[0]) {
        return Err(ApiError::bad_request(&format!(
            "datasets must share one timeframe, got {}",
            timeframes.join(", ")
        )));
    }
    let stored = timeframe::Timeframe::parse(&timeframes[0])
        .map_err(|e| ApiError::bad_request(&format!("unknown timeframe: {e}")))?;

    let series: Vec<align::Series<'_>> = labels
        .iter()
        .zip(&stamps)
        .map(|(l, ts)| align::Series { label: l.as_str(), ts })
        .collect();

    // Intraday bars cannot be aligned across stamping conventions (a 09:30 session bar and an
    // 08:00 epoch-grid one are 90 minutes apart), so a mixed-provider intraday request is
    // refused rather than approximated.
    if stored.is_intraday() && providers.iter().any(|p| p != &providers[0]) {
        return Err(ApiError::bad_request(
            "intraday bars from different providers cannot be aligned (their sessions are \
             anchored differently): use daily datasets, or one provider for the whole basket",
        ));
    }

    // What granularity to measure at.
    let (grain, resample) = match body.measure.as_deref().map(str::trim).unwrap_or("auto") {
        "auto" | "" => align::measure_grain(stored, &series),
        "stored" => (stored, None),
        other => {
            let tf = timeframe::Timeframe::parse(other)
                .map_err(|e| ApiError::bad_request(&format!("invalid `measure`: {e}")))?;
            (tf, None)
        }
    };
    if let (Some(g), Some(st)) = (grain.secs(), stored.secs()) {
        if g < st {
            return Err(ApiError::bad_request(&format!(
                "cannot measure at {} from {} bars: a coarser timeframe only",
                grain.label(),
                stored.label()
            )));
        }
    }

    let merged = align::merge(&series, align::Grain::At(grain), align::Join::Intersection);
    if merged.rows() < 3 {
        return Err(ApiError::bad_request(&format!(
            "datasets share only {} period(s) at {}: check their date ranges overlap",
            merged.rows(),
            grain.label()
        )));
    }

    // Aligned close series → per-asset return series. An intersection join fills every row
    // for every asset, so the series come out the same length; the check keeps that a fact
    // rather than an assumption, since a ragged matrix would quietly skew the covariance.
    let rets: Vec<Vec<f64>> = closes
        .iter()
        .zip(&merged.maps)
        .map(|(c, m)| {
            let aligned: Vec<f64> = m.iter().filter_map(|b| b.map(|bi| c[bi])).collect();
            quant::returns(&aligned)
        })
        .collect();
    if rets.iter().any(|r| r.len() != rets[0].len()) {
        return Err(ApiError::internal("aligned series came back ragged"));
    }

    // Annualization is measured on the clock, never assumed from the timeframe: the same
    // daily timeframe is 252 periods a year on an exchange and 365 on a 24/7 market.
    let ppy = align::observed_ppy(&merged.clock, grain.periods_per_year());
    let samples = body.samples.clamp(500, 50_000);

    let corr = quant::correlation_matrix(&labels, &rets);
    let frontier = quant::efficient_frontier(&labels, &rets, ppy, samples, body.risk_free);
    let parity = quant::risk_parity(&labels, &rets, ppy);

    Ok(Json(json!({
        "labels": labels,
        "timeframes": timeframes,
        "periods": merged.rows(),
        "periods_per_year": ppy,
        "measured_at": grain.label(),
        "nominal_periods_per_year": grain.periods_per_year(),
        "resampled": resample.map(|r| json!({
            "from": r.from.label(),
            "to": r.to.label(),
            "reason": r.reason,
        })),
        "alignment": json!({
            "from": merged.clock.first(),
            "to": merged.clock.last(),
            "assets": merged.labels.iter().zip(&stamps).zip(&merged.collapsed).map(|((l, ts), collapsed)| json!({
                "ticker": l,
                "bars": ts.len(),
                "collapsed": collapsed,
            })).collect::<Vec<_>>(),
        }),
        "correlation": corr,
        "frontier": frontier,
        "risk_parity": parity,
    })))
}
