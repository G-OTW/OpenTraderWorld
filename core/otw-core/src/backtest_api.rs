//! HTTP API for the Backtest module.
//!
//! - `POST /api/backtest/run`        run a strategy over one *or more* datasets (portfolio)
//! - `POST /api/backtest/align`      multi-asset alignment preview (no simulation)
//! - `GET  /api/backtest/runs`       saved-run history
//! - `POST /api/backtest/runs`       save a run (settings + stats snapshot)
//! - `DELETE /api/backtest/runs/{id}`
//!
//! The run endpoint is stateless: settings + dataset id(s) in, full result out. Saving
//! persists only the settings (to rerun) and the summary stats (for the history list).

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::backtest::{self, Bars, Settings};
use crate::quant;
use crate::{ApiError, AppState};
use otw_store::backtest as store;
use otw_store::histdata as hd;

/// Cap on the total simulated points `Σ (n_assets × bars)`. Keeps a portfolio run synchronous
/// and bounded (single-user; no job queue). Per-dataset load is also capped.
const MAX_TOTAL_BARS: usize = 400_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/backtest/run", post(run))
        .route("/api/backtest/align", post(align))
        .route("/api/backtest/runs", get(list_runs).post(save_run))
        .route("/api/backtest/runs/{id}", delete(delete_run))
        .route("/api/backtest/runs/{id}/report.md", get(report_md))
        .route("/api/backtest/runs/{id}/report.pdf", get(report_pdf))
        .route("/api/backtest/runs/{id}/montecarlo", post(montecarlo))
        // Expert-mode library: strategies + custom indicators (CRUD).
        .route("/api/backtest/strategies", get(list_strategies).post(create_strategy))
        .route(
            "/api/backtest/strategies/{id}",
            get(get_strategy).put(update_strategy).delete(delete_strategy),
        )
        .route("/api/backtest/indicators", get(list_indicators).post(create_indicator))
        .route(
            "/api/backtest/indicators/{id}",
            get(get_indicator).put(update_indicator).delete(delete_indicator),
        )
}

#[derive(Deserialize)]
struct RunBody {
    /// Legacy single dataset. Optional when `dataset_ids` is given.
    #[serde(default)]
    dataset_id: Option<Uuid>,
    /// Portfolio dataset set (2–8). Falls back to `[dataset_id]` when absent.
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    settings: Settings,
    /// Optional cap on bars loaded per dataset (defaults to a generous window).
    #[serde(default)]
    limit: Option<i64>,
}

impl RunBody {
    /// Resolve the effective dataset id list from either field. Errors if none given.
    fn ids(&self) -> Result<Vec<Uuid>, ApiError> {
        let ids = if !self.dataset_ids.is_empty() {
            self.dataset_ids.clone()
        } else if let Some(id) = self.dataset_id {
            vec![id]
        } else {
            return Err(ApiError::bad_request("no dataset selected"));
        };
        if ids.len() > 8 {
            return Err(ApiError::bad_request("at most 8 datasets per portfolio run"));
        }
        Ok(ids)
    }
}

/// One asset's OHLCV split into the parallel arrays the engine consumes.
struct AssetArrays {
    ticker: String,
    timeframe: String,
    ts: Vec<String>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl AssetArrays {
    fn as_bars(&self) -> Bars<'_> {
        Bars {
            ticker: &self.ticker,
            ts: &self.ts,
            open: &self.open,
            high: &self.high,
            low: &self.low,
            close: &self.close,
            volume: &self.volume,
        }
    }
}

/// Load each dataset, enforce identical timeframe + the bar budget, and split into arrays.
async fn load_assets(
    state: &AppState,
    ids: &[Uuid],
    limit: i64,
) -> Result<Vec<AssetArrays>, ApiError> {
    let mut assets = Vec::with_capacity(ids.len());
    let mut timeframe: Option<String> = None;
    let mut total = 0usize;
    for &id in ids {
        let ds = hd::get_dataset(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("dataset not found"))?;
        match &timeframe {
            None => timeframe = Some(ds.timeframe.clone()),
            Some(tf) if *tf != ds.timeframe => {
                return Err(ApiError::bad_request(&format!(
                    "all datasets must share a timeframe ({} vs {})",
                    tf, ds.timeframe
                )));
            }
            _ => {}
        }
        let bars = hd::read_bars(&state.pool, id, None, None, limit).await?;
        if bars.len() < 2 {
            return Err(ApiError::bad_request(&format!(
                "{} has too few bars to backtest",
                ds.ticker
            )));
        }
        total += bars.len();
        if total > MAX_TOTAL_BARS {
            return Err(ApiError::bad_request(&format!(
                "too many bars: {} exceeds the {} point budget — narrow the range or drop a dataset",
                total, MAX_TOTAL_BARS
            )));
        }
        let ts = bars
            .iter()
            .map(|b| b.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default())
            .collect();
        assets.push(AssetArrays {
            ticker: ds.ticker,
            timeframe: ds.timeframe,
            ts,
            open: bars.iter().map(|b| b.open).collect(),
            high: bars.iter().map(|b| b.high).collect(),
            low: bars.iter().map(|b| b.low).collect(),
            close: bars.iter().map(|b| b.close).collect(),
            volume: bars.iter().map(|b| b.volume).collect(),
        });
    }
    Ok(assets)
}

/// Load the dataset(s) and simulate the portfolio. Returns trades, equity, stats, per-asset
/// breakdown, warm-up info and (multi-asset) the alignment report.
async fn run(
    State(state): State<AppState>,
    Json(body): Json<RunBody>,
) -> Result<Json<Value>, ApiError> {
    let ids = body.ids()?;
    if let Some(err) = body.settings.validate() {
        return Err(ApiError::bad_request(&err));
    }
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let assets = load_assets(&state, &ids, limit).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();
    let result = backtest::run_portfolio(&body.settings, &bar_refs);

    let total_bars: usize = assets.iter().map(|a| a.ts.len()).sum();
    Ok(Json(json!({
        // Legacy single-asset fields (kept for the current UI + saved-run history).
        "ticker": assets[0].ticker,
        "timeframe": assets[0].timeframe,
        "bars": total_bars,
        "trades": result.trades,
        "equity": result.equity,
        "stats": result.stats,
        // Phase-1 additions.
        "per_asset": result.per_asset,
        "warmup_bars": result.warmup_bars,
        "trading_start_ts": result.trading_start_ts,
        "alignment": result.alignment,
        // Phase-2 additions (risk layer).
        "skipped_min_size": result.skipped_min_size,
        "skipped_margin": result.skipped_margin,
        "halted_bars": result.halted_bars,
        "oos": result.oos,
        // Phase-4 additions (presentation).
        "benchmark": result.benchmark,
        "total_funding": result.total_funding,
        "grid": result.grid,
    })))
}

#[derive(Deserialize)]
struct AlignBody {
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    #[serde(default)]
    dataset_id: Option<Uuid>,
    /// Settings drive the warm-up figure; optional (defaults to a no-indicator strategy).
    #[serde(default)]
    settings: Option<Settings>,
    #[serde(default)]
    limit: Option<i64>,
}

/// Cheap alignment preview — loads the bars but does NOT simulate. Powers the "alert the user
/// before running" banner (overlap window, warm-up, per-asset inactive-bar counts).
async fn align(
    State(state): State<AppState>,
    Json(body): Json<AlignBody>,
) -> Result<Json<Value>, ApiError> {
    let ids = if !body.dataset_ids.is_empty() {
        body.dataset_ids.clone()
    } else if let Some(id) = body.dataset_id {
        vec![id]
    } else {
        return Err(ApiError::bad_request("no dataset selected"));
    };
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let assets = load_assets(&state, &ids, limit).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();
    // A default no-signal strategy gives warmup 0 when the caller omits settings.
    let settings = body.settings.unwrap_or_else(default_align_settings);
    let report = backtest::align(&settings, &bar_refs);
    Ok(Json(json!({ "alignment": report })))
}

/// Minimal settings whose only purpose is a valid `warmup_bars` computation for /align when the
/// client sends no strategy yet. No sides ⇒ warm-up 0.
fn default_align_settings() -> Settings {
    serde_json::from_value(json!({
        "mode": "long",
        "sizing": { "mode": "fixed_qty", "qty": 1 }
    }))
    .expect("static default settings")
}

async fn list_runs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let runs = store::list_runs(&state.pool).await?;
    Ok(Json(json!({ "runs": runs })))
}

#[derive(Deserialize)]
struct SaveBody {
    name: String,
    dataset_id: Uuid,
    /// Full portfolio dataset set; defaults to `[dataset_id]` for single-asset saves.
    #[serde(default)]
    dataset_ids: Vec<Uuid>,
    /// Optional provenance link to the strategy the run came from.
    #[serde(default)]
    strategy_id: Option<Uuid>,
    settings: Value,
    stats: Value,
}

async fn save_run(
    State(state): State<AppState>,
    Json(body): Json<SaveBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let ds = hd::get_dataset(&state.pool, body.dataset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let dataset_ids = if body.dataset_ids.is_empty() {
        vec![body.dataset_id]
    } else {
        body.dataset_ids.clone()
    };
    // The stats snapshot carries the engine version it was produced under; fall back to the
    // current engine for older client payloads that omit it.
    let engine_version = body
        .stats
        .get("engine_version")
        .and_then(Value::as_u64)
        .map(|v| v as i32)
        .unwrap_or(backtest::ENGINE_VERSION as i32);
    let id = store::save_run(
        &state.pool,
        &store::NewRun {
            name,
            dataset_id: body.dataset_id,
            dataset_ids: &dataset_ids,
            ticker: &ds.ticker,
            timeframe: &ds.timeframe,
            settings: &body.settings,
            stats: &body.stats,
            engine_version,
            strategy_id: body.strategy_id,
        },
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

async fn delete_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_run(&state.pool, id).await? {
        return Err(ApiError::not_found("run not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Deterministic Markdown report for a saved run, generated from its stored settings + stats
/// snapshot (no rerun — always available, diffable between runs, feeds the editor later).
async fn report_md(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let md = crate::backtest::report::run_report_md(&run.name, &run.ticker, &run.timeframe, &run.settings, &run.stats);
    Ok(([(axum::http::header::CONTENT_TYPE, "text/markdown; charset=utf-8")], md).into_response())
}

/// Same report as `report.md`, rendered to PDF by the shared report engine.
async fn report_pdf(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let doc = crate::backtest::report::run_report(
        &run.name,
        &run.ticker,
        &run.timeframe,
        &run.settings,
        &run.stats,
    );
    let pdf = crate::report::pdf::render(&doc);
    let safe: String = run
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"backtest_{safe}.pdf\""),
            ),
        ],
        pdf,
    )
        .into_response())
}

#[derive(Deserialize)]
struct MonteCarloBody {
    /// Simulated paths. Clamped in the engine.
    #[serde(default = "default_mc_iterations")]
    iterations: usize,
    /// Trades per path. `None`/0 → use the run's own trade count.
    #[serde(default)]
    horizon: Option<usize>,
    /// Resampling block length; 1 = IID bootstrap, >1 = streak-preserving block bootstrap.
    #[serde(default = "default_mc_block")]
    block: usize,
    /// Ruin threshold as a fraction of starting capital (0.5 = a 50% drawdown from start).
    #[serde(default = "default_mc_ruin")]
    ruin_pct: f64,
}

fn default_mc_iterations() -> usize {
    5_000
}
fn default_mc_block() -> usize {
    1
}
fn default_mc_ruin() -> f64 {
    0.5
}

/// Monte-Carlo resampling of a saved run's realized per-trade P&L (spec §2, option A: rerun the
/// stored settings to regenerate the exact trade sequence — nothing extra is persisted). Draws
/// many equity paths → drawdown / risk-of-ruin bands.
async fn montecarlo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<MonteCarloBody>,
) -> Result<Json<Value>, ApiError> {
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;

    // Rebuild the engine Settings from the stored snapshot; a shape mismatch (older engine) is a
    // clear client-facing error rather than a 500.
    let settings: Settings = serde_json::from_value(run.settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("run settings are not replayable: {e}")))?;
    let starting_capital = settings.starting_capital;

    // Resolve the dataset set (portfolio runs carry `dataset_ids`; legacy rows use `dataset_id`).
    let ids: Vec<Uuid> = match run.dataset_ids {
        Some(v) if !v.is_empty() => v,
        _ => run.dataset_id.map(|d| vec![d]).unwrap_or_default(),
    };
    if ids.is_empty() {
        return Err(ApiError::bad_request("run has no dataset to replay"));
    }

    // Replay the run to regenerate its exact trade list.
    let assets = load_assets(&state, &ids, 200_000).await?;
    let bars: Vec<Bars> = assets.iter().map(|a| a.as_bars()).collect();
    let bar_refs: Vec<&Bars> = bars.iter().collect();
    let result = backtest::run_portfolio(&settings, &bar_refs);

    let pnls: Vec<f64> = result.trades.iter().map(|t| t.pnl).collect();
    if pnls.len() < 2 {
        return Err(ApiError::bad_request(
            "this run has too few trades to resample (need at least 2)",
        ));
    }

    let horizon = body.horizon.filter(|h| *h > 0).unwrap_or(pnls.len());
    // Seed off the run id so repeated calls on the same run are reproducible.
    let seed = id.as_u128() as u64 ^ (id.as_u128() >> 64) as u64;
    let mc = quant::monte_carlo(
        &pnls,
        starting_capital,
        body.iterations,
        horizon,
        body.block,
        body.ruin_pct,
        seed,
    );

    Ok(Json(json!({
        "name": run.name,
        "ticker": run.ticker,
        "timeframe": run.timeframe,
        "result": mc,
    })))
}

// ── Strategies (named Settings) ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct StrategyBody {
    name: String,
    #[serde(default)]
    description: String,
    settings: Value,
}

/// Map a unique-name violation to a friendly 400 instead of a 500.
fn name_taken(err: anyhow::Error, what: &str) -> ApiError {
    let s = err.to_string();
    if s.contains("duplicate key") || s.contains("unique") {
        ApiError::bad_request(&format!("a {what} with that name already exists"))
    } else {
        ApiError::internal(&s)
    }
}

async fn list_strategies(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "strategies": store::list_strategies(&state.pool).await? })))
}

async fn get_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let s = store::get_strategy(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("strategy not found"))?;
    Ok(Json(json!({ "strategy": s })))
}

/// Validate a strategy body: non-empty name + the settings must deserialize + pass engine checks.
fn check_strategy(body: &StrategyBody) -> Result<(), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("strategy name is required"));
    }
    let settings: Settings = serde_json::from_value(body.settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("invalid strategy settings: {e}")))?;
    if let Some(err) = settings.validate() {
        return Err(ApiError::bad_request(&err));
    }
    Ok(())
}

async fn create_strategy(
    State(state): State<AppState>,
    Json(body): Json<StrategyBody>,
) -> Result<Json<Value>, ApiError> {
    check_strategy(&body)?;
    let id = store::create_strategy(&state.pool, body.name.trim(), body.description.trim(), &body.settings)
        .await
        .map_err(|e| name_taken(e, "strategy"))?;
    Ok(Json(json!({ "id": id })))
}

async fn update_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StrategyBody>,
) -> Result<Json<Value>, ApiError> {
    check_strategy(&body)?;
    let ok = store::update_strategy(&state.pool, id, body.name.trim(), body.description.trim(), &body.settings)
        .await
        .map_err(|e| name_taken(e, "strategy"))?;
    if !ok {
        return Err(ApiError::not_found("strategy not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_strategy(&state.pool, id).await? {
        return Err(ApiError::not_found("strategy not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Custom indicators (node-graph DAG) ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct IndicatorBody {
    name: String,
    #[serde(default)]
    description: String,
    definition: Value,
}

async fn list_indicators(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "indicators": store::list_indicators(&state.pool).await? })))
}

async fn get_indicator(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let i = store::get_indicator(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("indicator not found"))?;
    Ok(Json(json!({ "indicator": i })))
}

/// Validate an indicator body: non-empty name + the definition must deserialize into the node
/// graph and pass structural validation (bounded size, no forward/self references).
fn check_indicator(body: &IndicatorBody) -> Result<(), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("indicator name is required"));
    }
    let def: backtest::CustomIndicatorDef = serde_json::from_value(body.definition.clone())
        .map_err(|e| ApiError::bad_request(&format!("invalid indicator definition: {e}")))?;
    if let Some(err) = def.validate() {
        return Err(ApiError::bad_request(&err));
    }
    Ok(())
}

async fn create_indicator(
    State(state): State<AppState>,
    Json(body): Json<IndicatorBody>,
) -> Result<Json<Value>, ApiError> {
    check_indicator(&body)?;
    let id = store::create_indicator(&state.pool, body.name.trim(), body.description.trim(), &body.definition)
        .await
        .map_err(|e| name_taken(e, "indicator"))?;
    Ok(Json(json!({ "id": id })))
}

async fn update_indicator(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<IndicatorBody>,
) -> Result<Json<Value>, ApiError> {
    check_indicator(&body)?;
    let ok = store::update_indicator(&state.pool, id, body.name.trim(), body.description.trim(), &body.definition)
        .await
        .map_err(|e| name_taken(e, "indicator"))?;
    if !ok {
        return Err(ApiError::not_found("indicator not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_indicator(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_indicator(&state.pool, id).await? {
        return Err(ApiError::not_found("indicator not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
