//! HTTP API for the Backtest module.
//!
//! - `POST /api/backtest/run`        run a strategy over a dataset, return trades+stats+equity
//! - `GET  /api/backtest/runs`       saved-run history
//! - `POST /api/backtest/runs`       save a run (settings + stats snapshot)
//! - `DELETE /api/backtest/runs/{id}`
//!
//! The run endpoint is stateless: settings + dataset id in, full result out. Saving
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
use crate::{ApiError, AppState};
use otw_store::backtest as store;
use otw_store::histdata as hd;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/backtest/run", post(run))
        .route("/api/backtest/runs", get(list_runs).post(save_run))
        .route("/api/backtest/runs/{id}", delete(delete_run))
}

#[derive(Deserialize)]
struct RunBody {
    dataset_id: Uuid,
    settings: Settings,
    /// Optional cap on bars loaded (defaults to a generous window).
    #[serde(default)]
    limit: Option<i64>,
}

/// Load the dataset's bars and simulate. Returns trades, equity curve, and stats.
async fn run(
    State(state): State<AppState>,
    Json(body): Json<RunBody>,
) -> Result<Json<Value>, ApiError> {
    let ds = hd::get_dataset(&state.pool, body.dataset_id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let limit = body.limit.unwrap_or(50_000).clamp(1, 200_000);
    let bars = hd::read_bars(&state.pool, body.dataset_id, None, None, limit).await?;
    if bars.len() < 2 {
        return Err(ApiError::bad_request("dataset has too few bars to backtest"));
    }

    // Split into the parallel arrays the engine consumes.
    let ts: Vec<String> = bars
        .iter()
        .map(|b| b.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default())
        .collect();
    let open: Vec<f64> = bars.iter().map(|b| b.open).collect();
    let high: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let low: Vec<f64> = bars.iter().map(|b| b.low).collect();
    let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let volume: Vec<f64> = bars.iter().map(|b| b.volume).collect();

    let result = backtest::run(
        &body.settings,
        &Bars { ts: &ts, open: &open, high: &high, low: &low, close: &close, volume: &volume },
    );

    Ok(Json(json!({
        "ticker": ds.ticker,
        "timeframe": ds.timeframe,
        "bars": bars.len(),
        "trades": result.trades,
        "equity": result.equity,
        "stats": result.stats,
    })))
}

async fn list_runs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let runs = store::list_runs(&state.pool).await?;
    Ok(Json(json!({ "runs": runs })))
}

#[derive(Deserialize)]
struct SaveBody {
    name: String,
    dataset_id: Uuid,
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
    let id = store::save_run(
        &state.pool,
        &store::NewRun {
            name,
            dataset_id: body.dataset_id,
            ticker: &ds.ticker,
            timeframe: &ds.timeframe,
            settings: &body.settings,
            stats: &body.stats,
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
