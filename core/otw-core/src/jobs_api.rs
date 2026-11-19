//! Background jobs, one list: `GET /api/jobs`, then pause, change the interval, run now,
//! delete a schedule, and read the run log. See `crate::jobs`.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{jobs, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/jobs", get(list))
        .route("/api/jobs/runs", get(runs))
        .route("/api/jobs/{id}", axum::routing::patch(update).delete(remove))
        .route("/api/jobs/{id}/run", post(run))
}

fn refused(e: anyhow::Error) -> ApiError {
    ApiError::bad_request(&format!("{e:#}"))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "jobs": jobs::list(&state).await? })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<jobs::Patch>,
) -> Result<Json<Value>, ApiError> {
    jobs::update(&state, &id, &patch).await.map_err(refused)?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    jobs::delete(&state, &id).await.map_err(refused)?;
    Ok(Json(json!({ "ok": true })))
}

async fn run(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    jobs::run_now(&state, &id).await.map_err(refused)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct RunsQuery {
    job: Option<String>,
    limit: Option<i64>,
}

async fn runs(
    State(state): State<AppState>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let runs = otw_store::jobs::list_runs(&state.pool, q.job.as_deref(), limit).await?;
    Ok(Json(json!({ "runs": runs })))
}
