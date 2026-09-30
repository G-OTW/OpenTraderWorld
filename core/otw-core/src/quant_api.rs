//! HTTP API for the Quant Tools module.
//!
//! - `POST /api/quant/single`     one dataset → HV, Max DD, VaR, CVaR (+ curves)
//! - `POST /api/quant/kelly`      manual win-rate/payoff → Kelly fractions
//! - `POST /api/quant/portfolio`  N datasets → correlation, efficient frontier, risk parity
//! - `POST /api/quant/stats`      one dataset → distribution, serial dependence, Hurst,
//!                                 variance ratio, unit-root tests, Sharpe significance
//! - `POST /api/quant/volatility` one dataset → range estimators, cones, GARCH(1,1) forecast
//! - `POST /api/quant/regimes`    one dataset → Gaussian HMM regimes
//! - `POST /api/quant/events`     one dataset + a condition → forward returns, event path
//! - `POST /api/quant/pairs`      two datasets → cointegration, spread, rolling co-movement,
//!                                 lead-lag, Granger causality
//! - `POST /api/quant/basket`     N datasets → PCA, clustering, HRP, relative strength, stress
//! - `POST /api/quant/regression` one dataset on factor datasets → alpha, betas, capture
//! - `POST /api/quant/trades`     one saved backtest run → expectancy, R-multiples, SQN, MAE/MFE
//! - `POST /api/quant/compare`    several saved runs → curves, correlation, deflated Sharpe, PBO
//! - `POST /api/quant/ruin`, `/options`, `/iv`, `/basis`, `/compound`, `/sharpe`: calculators
//! - `POST /api/quant/voltarget`  one dataset → volatility-targeted exposure and its track
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
        .route("/api/quant/stats", post(stats))
        .route("/api/quant/volatility", post(volatility))
        .route("/api/quant/regimes", post(regimes))
        .route("/api/quant/events", post(events))
        .route("/api/quant/pairs", post(pairs))
        .route("/api/quant/basket", post(basket))
        .route("/api/quant/regression", post(regression))
        .route("/api/quant/trades", post(trades))
        .route("/api/quant/compare", post(compare))
        .route("/api/quant/ruin", post(ruin))
        .route("/api/quant/options", post(options))
        .route("/api/quant/iv", post(iv))
        .route("/api/quant/basis", post(basis))
        .route("/api/quant/compound", post(compound))
        .route("/api/quant/voltarget", post(voltarget))
        .route("/api/quant/sharpe", post(sharpe))
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
    let rets = quant::returns(&closes);
    // The worst drawdowns as a table (depth, dates, time to trough and back), and the tail
    // beyond the VaR: Cornish-Fisher, tail ratio and a peaks-over-threshold fit.
    let mut episodes = quant::underwater(&closes, &ts, 0.0);
    episodes.truncate(10);
    let tail = quant::stats::tail_risk(&rets, conf, 0.10);
    Ok(Json(json!({
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "result": result,
        "episodes": episodes,
        "tail": tail,
    })))
}

/// Full OHLCV of a dataset over an optional window, ascending.
async fn load_bars(
    state: &AppState,
    id: Uuid,
    from: &Option<String>,
    until: &Option<String>,
    min_bars: usize,
) -> Result<(hd::Dataset, Vec<hd::Bar>, Vec<String>), ApiError> {
    let from = parse_bound(from, "from")?;
    let until = parse_bound(until, "until")?;
    let ds = hd::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let bars = hd::read_bars(&state.pool, id, from, until, MAX_BARS).await?;
    if bars.len() < min_bars {
        return Err(ApiError::bad_request(&format!(
            "this analysis needs at least {min_bars} bars in the selected range, the dataset has {}",
            bars.len()
        )));
    }
    let ts = bars.iter().map(|b| b.ts.format(&Rfc3339).unwrap_or_default()).collect();
    Ok((ds, bars, ts))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StatsBody {
    dataset_id: Uuid,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    /// Lags shown in the correlogram (default 20).
    #[serde(default = "default_lags")]
    lags: usize,
    /// Annual risk-free rate for the Sharpe tests (e.g. 0.03).
    #[serde(default)]
    risk_free: f64,
    /// Annualized Sharpe the PSR and minimum track record are measured against (default 0).
    #[serde(default)]
    benchmark_sharpe: f64,
}

fn default_lags() -> usize {
    20
}

/// Distribution, serial dependence, Hurst, variance ratios, unit-root tests and Sharpe
/// significance of one dataset. Tests run on log returns (and log prices for the level tests).
async fn stats(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<StatsBody>,
) -> Result<Json<Value>, ApiError> {
    use quant::stats as st;
    let (ds, bars, _) = load_bars(&state, body.dataset_id, &body.from, &body.until, 30).await?;
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    if closes.iter().any(|c| *c <= 0.0) {
        return Err(ApiError::bad_request("the series has non-positive closes: log returns are undefined"));
    }
    let ppy = quant::periods_per_year(&ds.timeframe);
    let lr = st::log_returns(&closes);
    let lp: Vec<f64> = closes.iter().map(|c| c.ln()).collect();
    let abs: Vec<f64> = lr.iter().map(|r| r.abs()).collect();
    let simple = quant::returns(&closes);
    let lags = body.lags.clamp(5, 100);
    let vr: Vec<_> = [2usize, 4, 8, 16].into_iter().filter_map(|q| st::variance_ratio(&lp, q)).collect();
    Ok(Json(json!({
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "periods_per_year": ppy,
        "moments": st::moments(&lr),
        "histogram": quant::histogram(&lr, 60),
        "qq": st::qq_points(&lr, 400),
        "correlogram": st::correlogram(&lr, lags),
        "correlogram_abs": st::correlogram(&abs, lags),
        "hurst": st::hurst(&lr),
        "variance_ratio": vr,
        "adf_price": st::adf(&lp, true, 1),
        "adf_returns": st::adf(&lr, true, 1),
        "kpss_price": st::kpss(&lp),
        "kpss_returns": st::kpss(&lr),
        "half_life": st::half_life(&lp),
        "sharpe": st::sharpe_inference(&simple, ppy, body.risk_free, body.benchmark_sharpe, 0.95),
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct VolatilityBody {
    dataset_id: Uuid,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    /// Rolling window in bars for the estimator chart (default 21).
    #[serde(default = "default_vol_window")]
    window: usize,
    /// GARCH forecast horizon in bars (default 21).
    #[serde(default = "default_vol_window")]
    horizon: usize,
}

fn default_vol_window() -> usize {
    21
}

/// Range-based volatility estimators, volatility cones and a GARCH(1,1) forecast.
async fn volatility(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<VolatilityBody>,
) -> Result<Json<Value>, ApiError> {
    let (ds, bars, ts) = load_bars(&state, body.dataset_id, &body.from, &body.until, 60).await?;
    let ppy = quant::periods_per_year(&ds.timeframe);
    let ohlc: Vec<quant::vol::Bar> =
        bars.iter().map(|b| quant::vol::Bar { open: b.open, high: b.high, low: b.low, close: b.close }).collect();
    let report = quant::vol::vol_report(&ohlc, &ts, ppy, body.window, body.horizon.clamp(1, 252))
        .ok_or_else(|| ApiError::bad_request("not enough valid OHLC bars (positive prices, high ≥ low)"))?;
    Ok(Json(json!({ "ticker": ds.ticker, "timeframe": ds.timeframe, "result": report })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RegimesBody {
    dataset_id: Uuid,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    /// Number of hidden states, 2 to 4 (default 2).
    #[serde(default = "default_states")]
    states: usize,
}

fn default_states() -> usize {
    2
}

/// Gaussian hidden Markov regimes of the period returns.
async fn regimes(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<RegimesBody>,
) -> Result<Json<Value>, ApiError> {
    let (ds, bars, ts) = load_bars(&state, body.dataset_id, &body.from, &body.until, 200).await?;
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let pct: Vec<f64> = quant::returns(&closes).iter().map(|r| r * 100.0).collect();
    let ppy = quant::periods_per_year(&ds.timeframe);
    let fit = quant::regime::fit(&pct, &ts[1..], ppy, body.states, 1000)
        .ok_or_else(|| ApiError::bad_request("not enough bars for this many states (50 per state)"))?;
    // Closes for the price chart the segments are drawn over, thinned for the payload.
    let stride = (closes.len() / 2000).max(1);
    let price: Vec<Value> = (0..closes.len()).step_by(stride).map(|i| json!([ts[i], closes[i]])).collect();
    Ok(Json(json!({ "ticker": ds.ticker, "timeframe": ds.timeframe, "result": fit, "price": price })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct EventsBody {
    dataset_id: Uuid,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    condition: quant::events::Condition,
    /// Forward horizons in bars (default 1, 5, 10, 20).
    #[serde(default)]
    horizons: Option<Vec<usize>>,
    /// Bars shown before and after the event on the average path (default 10 / 20).
    #[serde(default)]
    pre: Option<usize>,
    #[serde(default)]
    post: Option<usize>,
    /// Minimum bars between two counted events, so overlapping windows are not double counted
    /// (default 1 = every trigger).
    #[serde(default)]
    min_gap: Option<usize>,
}

/// Forward returns after a condition fires, against the unconditional baseline.
async fn events(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<EventsBody>,
) -> Result<Json<Value>, ApiError> {
    use quant::events as ev;
    let (ds, bars, ts) = load_bars(&state, body.dataset_id, &body.from, &body.until, 30).await?;
    let col = |f: fn(&hd::Bar) -> f64| bars.iter().map(f).collect::<Vec<f64>>();
    let (open, close, volume) = (col(|b| b.open), col(|b| b.close), col(|b| b.volume));
    let series = ev::Series { open: &open, close: &close, volume: &volume };
    let raw = ev::fire(&body.condition, &series);
    let kept = ev::thin(&raw, body.min_gap.unwrap_or(1).clamp(1, 10_000));
    let mut horizons = body.horizons.unwrap_or_else(|| vec![1, 5, 10, 20]);
    horizons.retain(|h| (1..=close.len() / 2).contains(h));
    horizons.sort_unstable();
    horizons.dedup();
    horizons.truncate(8);
    let study = ev::study(
        &close,
        &ts,
        &kept,
        raw.len(),
        &horizons,
        body.pre.unwrap_or(10).min(250),
        body.post.unwrap_or(20).min(500),
    );
    Ok(Json(json!({ "ticker": ds.ticker, "timeframe": ds.timeframe, "result": study })))
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
    /// "return" (mean period return, default), "volatility" (stddev of period returns),
    /// "volume" (mean bar volume) or "range" (mean (high − low) / close).
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
    let metric = body.metric.as_deref().unwrap_or("return");
    let metric_vol = matches!(metric, "volatility" | "vol");

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

    let result = match metric {
        "volume" => {
            let samples: Vec<(usize, f64)> = bars.iter().enumerate().map(|(i, b)| (i, b.volume)).collect();
            quant::seasonality_of(&samples, &months, &weekdays, &hours, false, has_hour, "volume")
        }
        "range" => {
            let samples: Vec<(usize, f64)> = bars
                .iter()
                .enumerate()
                .filter(|(_, b)| b.close > 0.0)
                .map(|(i, b)| (i, (b.high - b.low) / b.close))
                .collect();
            quant::seasonality_of(&samples, &months, &weekdays, &hours, false, has_hour, "range")
        }
        _ => quant::seasonality(&closes, &months, &weekdays, &hours, metric_vol, has_hour),
    };
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

/// A set of datasets aligned on one clock: closes per asset, same length, plus what the
/// alignment did. Shared by every multi-asset endpoint.
struct Basket {
    labels: Vec<String>,
    timeframes: Vec<String>,
    /// Stamp of each aligned row.
    clock: Vec<String>,
    /// Aligned closes, one vector per asset, all `clock.len()` long.
    closes: Vec<Vec<f64>>,
    /// Periods per year measured on the aligned clock.
    ppy: f64,
    grain: timeframe::Timeframe,
    resample: Option<align::Resample>,
    bars: Vec<usize>,
    collapsed: Vec<usize>,
}

impl Basket {
    fn returns(&self) -> Vec<Vec<f64>> {
        self.closes.iter().map(|c| quant::returns(c)).collect()
    }

    /// The alignment facts every multi-asset response carries.
    fn meta(&self) -> Value {
        json!({
            "labels": self.labels,
            "timeframes": self.timeframes,
            "periods": self.clock.len(),
            "periods_per_year": self.ppy,
            "measured_at": self.grain.label(),
            "nominal_periods_per_year": self.grain.periods_per_year(),
            "resampled": self.resample.as_ref().map(|r| json!({
                "from": r.from.label(),
                "to": r.to.label(),
                "reason": r.reason,
            })),
            "alignment": json!({
                "from": self.clock.first(),
                "to": self.clock.last(),
                "assets": self.labels.iter().zip(&self.bars).zip(&self.collapsed).map(|((l, bars), collapsed)| json!({
                    "ticker": l,
                    "bars": bars,
                    "collapsed": collapsed,
                })).collect::<Vec<_>>(),
            }),
        })
    }
}

/// Load `ids` and align them by period (see [`crate::align`]). `measure` picks the grain:
/// "auto" (default), "stored", or an explicit coarser timeframe.
async fn load_basket(
    state: &AppState,
    ids: &[Uuid],
    measure: Option<&str>,
    min: usize,
    max: usize,
) -> Result<Basket, ApiError> {
    if ids.len() < min {
        return Err(ApiError::bad_request(&format!("this analysis needs at least {min} datasets")));
    }
    if ids.len() > max {
        return Err(ApiError::bad_request(&format!("at most {max} datasets")));
    }

    // Load each asset's timestamps + closes, in ascending time order.
    let mut labels = Vec::new();
    let mut providers = Vec::new();
    let mut timeframes = Vec::new();
    let mut stamps: Vec<Vec<String>> = Vec::new();
    let mut closes: Vec<Vec<f64>> = Vec::new();
    for id in ids {
        let (ds, ts, c) = load_closes(state, *id, None, None).await?;
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
    let (grain, resample) = match measure.map(str::trim).unwrap_or("auto") {
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

    // An intersection join fills every row for every asset, so the series come out the same
    // length; the check keeps that a fact rather than an assumption, since a ragged matrix
    // would quietly skew every statistic computed across the assets.
    let aligned: Vec<Vec<f64>> = closes
        .iter()
        .zip(&merged.maps)
        .map(|(c, m)| m.iter().filter_map(|b| b.map(|bi| c[bi])).collect())
        .collect();
    if aligned.iter().any(|r: &Vec<f64>| r.len() != merged.clock.len()) {
        return Err(ApiError::internal("aligned series came back ragged"));
    }

    // Annualization is measured on the clock, never assumed from the timeframe: the same
    // daily timeframe is 252 periods a year on an exchange and 365 on a 24/7 market.
    let ppy = align::observed_ppy(&merged.clock, grain.periods_per_year());
    Ok(Basket {
        labels,
        timeframes,
        clock: merged.clock.clone(),
        closes: aligned,
        ppy,
        grain,
        resample,
        bars: stamps.iter().map(|s| s.len()).collect(),
        collapsed: merged.collapsed.clone(),
    })
}

async fn portfolio(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<PortfolioBody>,
) -> Result<Json<Value>, ApiError> {
    let b = load_basket(&state, &body.dataset_ids, body.measure.as_deref(), 2, 20).await?;
    let rets = b.returns();
    let samples = body.samples.clamp(500, 50_000);

    let corr = quant::correlation_matrix(&b.labels, &rets);
    let frontier = quant::efficient_frontier(&b.labels, &rets, b.ppy, samples, body.risk_free);
    let parity = quant::risk_parity(&b.labels, &rets, b.ppy);

    let mut out = b.meta();
    out["correlation"] = json!(corr);
    out["frontier"] = json!(frontier);
    out["risk_parity"] = json!(parity);
    Ok(Json(out))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PairsBody {
    /// Exactly two dataset ids: the first is y, the second x (y = alpha + beta x).
    dataset_ids: Vec<Uuid>,
    /// Measurement grain, as for the portfolio endpoint ("auto", "stored", "1w", …).
    #[serde(default)]
    measure: Option<String>,
    /// Rolling window in bars for z-scores, correlation and beta (default 60).
    #[serde(default = "default_pair_window")]
    window: usize,
    /// Largest lag for the lead-lag correlation and Granger tests (default 5).
    #[serde(default = "default_pair_lags")]
    max_lag: usize,
    /// Work on log prices (default true) or raw prices.
    #[serde(default = "default_true")]
    log: bool,
}

fn default_pair_window() -> usize {
    60
}
fn default_pair_lags() -> usize {
    5
}
fn default_true() -> bool {
    true
}

/// Cointegration (Engle-Granger both ways, Johansen), the hedged spread and its z-score,
/// rolling correlation and beta, lead-lag correlation and Granger causality for a pair.
async fn pairs(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<PairsBody>,
) -> Result<Json<Value>, ApiError> {
    use quant::pairs as pr;
    let b = load_basket(&state, &body.dataset_ids, body.measure.as_deref(), 2, 2).await?;
    if b.closes.iter().flatten().any(|c| *c <= 0.0) && body.log {
        return Err(ApiError::bad_request("non-positive prices: switch off log prices"));
    }
    let lv: Vec<Vec<f64>> =
        b.closes.iter().map(|c| if body.log { c.iter().map(|v| v.ln()).collect() } else { c.clone() }).collect();
    let (y, x) = (&lv[0], &lv[1]);
    let rets = b.returns();
    let window = body.window.clamp(10, 1000);
    let lags = body.max_lag.clamp(1, 20);
    let levels: Vec<Vec<f64>> = (0..y.len()).map(|t| vec![y[t], x[t]]).collect();
    let mut out = b.meta();
    out["log"] = json!(body.log);
    out["engle_granger_yx"] = json!(pr::engle_granger(y, x));
    out["engle_granger_xy"] = json!(pr::engle_granger(x, y));
    out["johansen"] = json!(pr::johansen(&levels, 1));
    out["spread"] = json!(pr::spread(y, x, &b.clock, window));
    out["rolling"] = json!(pr::rolling(&rets[0], &rets[1], &b.clock[1..], window));
    out["correlation"] = json!(quant::correlation_matrix(&b.labels, &rets).matrix[0][1]);
    out["lead_lag"] = json!(pr::lagged_correlation(&rets[0], &rets[1], lags)
        .into_iter()
        .map(|(k, c)| json!({ "lag": k, "corr": c }))
        .collect::<Vec<_>>());
    out["granger_x_to_y"] = json!(pr::granger(&rets[0], &rets[1], lags));
    out["granger_y_to_x"] = json!(pr::granger(&rets[1], &rets[0], lags));
    Ok(Json(out))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct BasketBody {
    /// Two or more dataset ids (the basket).
    dataset_ids: Vec<Uuid>,
    #[serde(default)]
    measure: Option<String>,
    /// Linkage for the dendrogram and HRP: single (default), complete, average or ward.
    #[serde(default)]
    linkage: Option<quant::basket::Linkage>,
    /// Weights for the stress test, one per dataset (normalized). Default: equal weights.
    #[serde(default)]
    weights: Option<Vec<f64>>,
}

/// PCA, hierarchical clustering, HRP weights, relative-strength ranking and historical
/// stress windows for a basket.
async fn basket(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<BasketBody>,
) -> Result<Json<Value>, ApiError> {
    use quant::basket as bk;
    let b = load_basket(&state, &body.dataset_ids, body.measure.as_deref(), 2, 30).await?;
    let rets = b.returns();
    let method = body.linkage.unwrap_or(bk::Linkage::Single);
    let n = b.labels.len();
    let weights = match body.weights {
        Some(w) if w.len() == n && w.iter().all(|v| *v >= 0.0) && w.iter().sum::<f64>() > 0.0 => {
            let s: f64 = w.iter().sum();
            w.iter().map(|v| v / s).collect()
        }
        Some(_) => return Err(ApiError::bad_request("weights: one non-negative number per dataset, not all zero")),
        None => vec![1.0 / n as f64; n],
    };
    let mut out = b.meta();
    out["pca"] = json!(bk::pca(&b.labels, &rets));
    out["clustering"] = json!(bk::clustering(&b.labels, &rets, method));
    out["hrp"] = json!(bk::hrp(&b.labels, &rets, b.ppy, method));
    out["strength"] = json!(bk::relative_strength(&b.labels, &b.closes, b.ppy));
    out["stress"] = json!(bk::stress(&b.labels, &b.clock, &b.closes, &weights));
    Ok(Json(out))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RegressionBody {
    /// The asset whose returns are explained.
    dataset_id: Uuid,
    /// One or more factor datasets (a market index, bonds, gold, a sector ETF…).
    factor_ids: Vec<Uuid>,
    #[serde(default)]
    measure: Option<String>,
    /// Annual risk-free rate subtracted from every series (e.g. 0.03).
    #[serde(default)]
    risk_free: f64,
    /// Rolling window in bars for the beta on the first factor (default 60).
    #[serde(default = "default_pair_window")]
    window: usize,
}

/// Regression of one asset's returns on factor returns: alpha, betas with t-stats, R²,
/// tracking error, information ratio, up/down capture, rolling beta on the first factor.
async fn regression(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<RegressionBody>,
) -> Result<Json<Value>, ApiError> {
    if body.factor_ids.contains(&body.dataset_id) {
        return Err(ApiError::bad_request("the asset cannot also be a factor"));
    }
    let mut ids = vec![body.dataset_id];
    ids.extend(&body.factor_ids);
    let b = load_basket(&state, &ids, body.measure.as_deref(), 2, 11).await?;
    let rets = b.returns();
    let factors: Vec<(String, Vec<f64>)> = b.labels[1..].iter().cloned().zip(rets[1..].iter().cloned()).collect();
    let fit = quant::basket::regression(&b.labels[0], &rets[0], &factors, b.ppy, body.risk_free)
        .ok_or_else(|| ApiError::bad_request("not enough aligned periods for this many factors"))?;
    let mut out = b.meta();
    out["regression"] = json!(fit);
    out["rolling"] = json!(quant::pairs::rolling(&rets[0], &rets[1], &b.clock[1..], body.window.clamp(10, 1000)));
    Ok(Json(out))
}

// ── Strategy: one saved run's trades, several runs side by side ──────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TradesBody {
    /// A saved backtest run (history). It is replayed over its own datasets and window.
    run_id: Uuid,
}

/// Expectancy, R-multiple distribution, SQN and the MAE/MFE picture of a saved run's trades.
/// 1R is each trade's initial stop risk when the run used a fixed percentage stop on every
/// side it traded, else the average loss.
async fn trades(
    axum::extract::State(state): axum::extract::State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(body): Json<TradesBody>,
) -> Result<Json<Value>, ApiError> {
    crate::backtest_api::demo_sim_allowed(ip)?;
    let (run, settings, _, result) = crate::backtest_api::replay_run(&state, body.run_id).await?;
    let mult = if settings.instrument.multiplier > 0.0 { settings.instrument.multiplier } else { 1.0 };
    // Side rules only drive signal strategies: a grid or DCA run can carry stale ones.
    let signals = settings.kind != "grid" && settings.kind != "dca";
    let stop_pct = |long: bool| {
        if !signals {
            return None;
        }
        let side = if long { settings.long.as_ref() } else { settings.short.as_ref() };
        side.and_then(|s| s.sl_rule()).filter(|r| r.kind == "pct" && r.value > 0.0).map(|r| r.value)
    };
    let rows: Vec<quant::strategy::TradeRow> = result
        .trades
        .iter()
        .map(|t| {
            let long = t.direction != "short";
            quant::strategy::TradeRow {
                exit_ts: t.exit_ts.clone(),
                pnl: t.pnl,
                mae: t.mae,
                mfe: t.mfe,
                risk: stop_pct(long).map(|p| t.entry_price * t.qty.abs() * mult * p),
            }
        })
        .collect();
    let a = quant::strategy::trade_analytics(&rows)
        .ok_or_else(|| ApiError::bad_request("this run needs at least 2 trades, and a losing one when it has no fixed stop"))?;
    Ok(Json(json!({ "name": run.name, "ticker": run.ticker, "timeframe": run.timeframe, "result": a })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CompareBody {
    /// Two to twenty saved backtest runs.
    run_ids: Vec<Uuid>,
    /// Blocks for the PBO cross-validation (even, 4 to 16; default 16).
    #[serde(default)]
    partitions: Option<usize>,
}

/// Equity curves of several saved runs on their common timestamps: rebased curves, return
/// correlation, per-run risk figures, the deflated Sharpe of the best one against all of them,
/// and the probability that picking the best in-sample overfits (CSCV).
async fn compare(
    axum::extract::State(state): axum::extract::State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(body): Json<CompareBody>,
) -> Result<Json<Value>, ApiError> {
    if !(2..=20).contains(&body.run_ids.len()) {
        return Err(ApiError::bad_request("pick between 2 and 20 runs"));
    }
    crate::backtest_api::demo_sim_allowed(ip)?;
    let mut labels = Vec::new();
    let mut curves = Vec::new();
    let mut tf = String::new();
    for id in &body.run_ids {
        let (run, _, _, result) = crate::backtest_api::replay_run(&state, *id).await?;
        let mut label = format!("{} · {}", run.name, run.ticker);
        if labels.contains(&label) {
            label = format!("{label} #{}", labels.len() + 1);
        }
        labels.push(label);
        if tf.is_empty() {
            tf = run.timeframe.clone();
        }
        curves.push(result.equity.iter().map(|p| (p.ts.clone(), p.equity)).collect::<Vec<_>>());
    }
    // Annualize on what the common clock actually holds.
    let common: Vec<String> = {
        let first: std::collections::HashSet<&str> = curves[0].iter().map(|(t, _)| t.as_str()).collect();
        let mut v: Vec<String> = first
            .into_iter()
            .filter(|t| curves[1..].iter().all(|c| c.binary_search_by(|(x, _)| x.as_str().cmp(t)).is_ok()))
            .map(String::from)
            .collect();
        v.sort_unstable();
        v
    };
    let ppy = align::observed_ppy(&common, quant::periods_per_year(&tf));
    let cmp = quant::strategy::compare(&labels, &curves, ppy, body.partitions.unwrap_or(16))
        .ok_or_else(|| ApiError::bad_request("these runs share fewer than 30 timestamps: pick runs over the same period"))?;
    Ok(Json(json!({ "periods_per_year": ppy, "result": cmp })))
}

// ── Calculators ──────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RuinBody {
    /// Win rate as a fraction (0.45 = 45%).
    win_rate: f64,
    /// Average win in R (average win over the amount risked per trade).
    payoff: f64,
    /// Capital risked per trade, fraction (0.01 = 1%).
    risk: f64,
    /// Drawdown that counts as ruin, fraction of the starting capital (0.5 = 50%).
    ruin: f64,
    /// "fixed" (the same amount every trade) or "fractional" (a share of current capital).
    sizing: quant::strategy::Sizing,
    /// Trades simulated per path (default 1000).
    #[serde(default)]
    horizon: Option<usize>,
    /// Simulated paths (default 20,000).
    #[serde(default)]
    paths: Option<usize>,
}

async fn ruin(Json(b): Json<RuinBody>) -> Result<Json<Value>, ApiError> {
    if !(0.0..=1.0).contains(&b.win_rate) || b.payoff <= 0.0 || !(0.0..1.0).contains(&b.risk) || b.risk <= 0.0 || !(0.0..=1.0).contains(&b.ruin) || b.ruin <= 0.0 {
        return Err(ApiError::bad_request("win_rate in 0..1, payoff > 0, risk in (0, 1), ruin in (0, 1]"));
    }
    Ok(Json(json!(quant::strategy::risk_of_ruin(
        b.win_rate,
        b.payoff,
        b.risk,
        b.ruin,
        b.sizing,
        b.horizon.unwrap_or(1000),
        b.paths.unwrap_or(20_000),
        0x5eed_1234_abcd,
    ))))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct OptionBody {
    /// "call" or "put".
    kind: String,
    /// "european" (default) or "american" (binomial tree).
    #[serde(default)]
    style: Option<String>,
    spot: f64,
    strike: f64,
    /// Calendar days to expiry.
    days: f64,
    /// Annual volatility, fraction (0.2 = 20%).
    vol: f64,
    /// Continuously compounded annual rate, fraction.
    #[serde(default)]
    rate: f64,
    /// Continuous dividend or carry yield, fraction.
    #[serde(default)]
    dividend: f64,
    /// Binomial steps (default 500).
    #[serde(default)]
    steps: Option<usize>,
}

fn is_call(kind: &str) -> Result<bool, ApiError> {
    match kind {
        "call" => Ok(true),
        "put" => Ok(false),
        _ => Err(ApiError::bad_request("kind must be call or put")),
    }
}

/// Black-Scholes-Merton price and greeks, a binomial (CRR) price for either exercise style,
/// and the value and delta across spot for the chart.
async fn options(Json(b): Json<OptionBody>) -> Result<Json<Value>, ApiError> {
    let call = is_call(&b.kind)?;
    let american = b.style.as_deref() == Some("american");
    let t = b.days / 365.0;
    let steps = b.steps.unwrap_or(500).clamp(10, 5000);
    let g = quant::calc::bsm(call, b.spot, b.strike, t, b.rate, b.dividend, b.vol)
        .ok_or_else(|| ApiError::bad_request("spot, strike, days and vol must be positive"))?;
    let tree = quant::calc::binomial(call, american, b.spot, b.strike, t, b.rate, b.dividend, b.vol, steps);
    // Tree against tree: against the closed form the gap would mix in the tree's discretization.
    let tree_eu = quant::calc::binomial(call, false, b.spot, b.strike, t, b.rate, b.dividend, b.vol, steps);
    let curve: Vec<Value> = (0..=60)
        .filter_map(|i| {
            let s = b.spot * (0.7 + 0.6 * i as f64 / 60.0);
            let v = quant::calc::bsm(call, s, b.strike, t, b.rate, b.dividend, b.vol)?;
            let intrinsic = if call { (s - b.strike).max(0.0) } else { (b.strike - s).max(0.0) };
            Some(json!({ "spot": s, "price": v.price, "delta": v.delta, "intrinsic": intrinsic }))
        })
        .collect();
    Ok(Json(json!({
        "bsm": g,
        "binomial": tree,
        "steps": steps,
        "american": american,
        // An American option is worth at least the European one; the gap is the early-exercise premium.
        "early_exercise_premium": tree.zip(tree_eu).filter(|_| american).map(|(a, e)| a - e),
        "curve": curve,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct IvBody {
    kind: String,
    /// Observed option price (European).
    price: f64,
    spot: f64,
    strike: f64,
    days: f64,
    #[serde(default)]
    rate: f64,
    #[serde(default)]
    dividend: f64,
}

async fn iv(Json(b): Json<IvBody>) -> Result<Json<Value>, ApiError> {
    let call = is_call(&b.kind)?;
    let t = b.days / 365.0;
    let v = quant::calc::implied_vol(call, b.price, b.spot, b.strike, t, b.rate, b.dividend)
        .ok_or_else(|| ApiError::bad_request("no volatility reproduces this price: it is outside the no-arbitrage bounds"))?;
    Ok(Json(json!({ "iv": v, "greeks": quant::calc::bsm(call, b.spot, b.strike, t, b.rate, b.dividend, v) })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct BasisBody {
    spot: f64,
    /// Futures price.
    future: f64,
    /// Calendar days to the future's expiry.
    days: f64,
    /// Financing rate (annual, continuous) for the fair value; omit to skip it.
    #[serde(default)]
    rate: Option<f64>,
    /// Dividend, coupon or storage-adjusted yield (annual, continuous).
    #[serde(default)]
    yield_rate: f64,
    /// Next contract's price and days to expiry, for the roll yield.
    #[serde(default)]
    next_future: Option<f64>,
    #[serde(default)]
    next_days: Option<f64>,
}

async fn basis(Json(b): Json<BasisBody>) -> Result<Json<Value>, ApiError> {
    let next = b.next_future.zip(b.next_days);
    let r = quant::calc::basis(b.spot, b.future, b.days, b.rate, b.yield_rate, next)
        .ok_or_else(|| ApiError::bad_request("spot, future and days must be positive"))?;
    Ok(Json(json!(r)))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CompoundBody {
    principal: f64,
    /// Return per period, fraction (0.07 = 7%).
    rate: f64,
    periods: usize,
    /// Added at the end of every period.
    #[serde(default)]
    contribution: f64,
    /// Optional target value for the required rate and the time to reach it.
    #[serde(default)]
    target: Option<f64>,
}

async fn compound(Json(b): Json<CompoundBody>) -> Result<Json<Value>, ApiError> {
    if b.principal < 0.0 || b.rate <= -1.0 || b.periods > 10_000 {
        return Err(ApiError::bad_request("principal ≥ 0, rate > -100%, at most 10,000 periods"));
    }
    Ok(Json(json!(quant::calc::compounding(b.principal, b.rate, b.periods, b.contribution, b.target))))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct VolTargetBody {
    dataset_id: Uuid,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    until: Option<String>,
    /// Target annualized volatility, fraction (0.1 = 10%).
    target: f64,
    /// Estimation window in bars (default 20).
    #[serde(default)]
    window: Option<usize>,
    #[serde(default = "default_estimator")]
    estimator: quant::calc::VolEstimator,
    /// Exposure cap (default 2 = 200%).
    #[serde(default)]
    max_leverage: Option<f64>,
    /// Account equity for the unit count at the last close.
    #[serde(default)]
    equity: Option<f64>,
}

fn default_estimator() -> quant::calc::VolEstimator {
    quant::calc::VolEstimator::Rolling
}

async fn voltarget(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(b): Json<VolTargetBody>,
) -> Result<Json<Value>, ApiError> {
    let (ds, bars, ts) = load_bars(&state, b.dataset_id, &b.from, &b.until, 40).await?;
    let closes: Vec<f64> = bars.iter().map(|x| x.close).collect();
    if closes.iter().any(|c| *c <= 0.0) {
        return Err(ApiError::bad_request("the series has non-positive closes"));
    }
    let ppy = quant::periods_per_year(&ds.timeframe);
    let r = quant::calc::vol_target(
        &ts,
        &closes,
        ppy,
        b.target,
        b.window.unwrap_or(20).clamp(2, 1000),
        b.estimator,
        b.max_leverage.unwrap_or(2.0).clamp(0.0, 20.0),
        b.equity,
    )
    .ok_or_else(|| ApiError::bad_request("not enough bars for this window, or a non-positive target"))?;
    Ok(Json(json!({ "ticker": ds.ticker, "timeframe": ds.timeframe, "last_close": closes.last(), "result": r })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SharpeBody {
    /// Annualized Sharpe ratio.
    sharpe: f64,
    /// Return observations behind it.
    observations: usize,
    /// Observations per year (252 daily, 52 weekly, 12 monthly).
    periods_per_year: f64,
    /// Skewness of the returns (0 for normal).
    #[serde(default)]
    skew: f64,
    /// Kurtosis of the returns, not in excess (3 for normal).
    #[serde(default = "default_kurtosis")]
    kurtosis: f64,
    /// Annualized Sharpe to beat (default 0).
    #[serde(default)]
    benchmark: f64,
    /// Confidence for the minimum track record length (default 0.95).
    #[serde(default = "default_confidence")]
    confidence: f64,
    /// Strategies tried before this one was kept, and the standard deviation of their
    /// annualized Sharpes: when both are given the deflated Sharpe is computed.
    #[serde(default)]
    trials: Option<usize>,
    #[serde(default)]
    trials_sharpe_std: Option<f64>,
}

fn default_kurtosis() -> f64 {
    3.0
}

/// Sharpe significance from summary numbers: t-stat, p-value, standard errors, confidence
/// interval, PSR, minimum track record, and the deflated Sharpe when the trial count is known.
async fn sharpe(Json(b): Json<SharpeBody>) -> Result<Json<Value>, ApiError> {
    use quant::special::norm_ppf;
    if b.observations < 2 || b.periods_per_year <= 0.0 {
        return Err(ApiError::bad_request("observations ≥ 2 and periods_per_year > 0"));
    }
    let conf = b.confidence.clamp(0.5, 0.999);
    let inf = quant::stats::sharpe_from_summary(b.sharpe, b.observations, b.periods_per_year, b.skew, b.kurtosis, b.benchmark, conf)
        .ok_or_else(|| ApiError::bad_request("these inputs give no defined Sharpe variance"))?;
    // Expected maximum Sharpe of N unskilled trials (Bailey & López de Prado), then PSR against it.
    let deflated = match (b.trials, b.trials_sharpe_std) {
        (Some(n), Some(sd)) if n >= 2 && sd > 0.0 => {
            const EULER: f64 = 0.577_215_664_901_532_9;
            let nf = n as f64;
            let emax = sd * ((1.0 - EULER) * norm_ppf(1.0 - 1.0 / nf) + EULER * norm_ppf(1.0 - 1.0 / (nf * std::f64::consts::E)));
            quant::stats::sharpe_from_summary(b.sharpe, b.observations, b.periods_per_year, b.skew, b.kurtosis, emax, conf)
                .map(|d| json!({ "expected_max_sharpe": emax, "deflated_sharpe": d.psr, "trials": n }))
        }
        _ => None,
    };
    Ok(Json(json!({ "inference": inf, "deflated": deflated })))
}
