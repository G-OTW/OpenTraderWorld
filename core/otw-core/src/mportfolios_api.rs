//! HTTP API for the Managers' Portfolios module.
//!
//! Reads come straight from the cache (refreshed on a schedule by `mportfolios_job`); the UI
//! never hits Dataroma. `GET /api/mportfolios` lists portfolios with optional name (`q`) and
//! holding (`ticker`) filters; the detail route returns one portfolio's holdings. A manual
//! refresh is offered for the "data looks stale" case. Attribution to Dataroma is shown in the
//! UI. Distinct from the future user "portfolios" module.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::{mportfolios_job, ApiError, AppState};
use otw_store::mportfolios as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mportfolios", get(list))
        .route("/api/mportfolios/refresh", post(refresh))
        // Snapshots — declared before the `{slug}` catch-all so these literal paths win.
        .route(
            "/api/mportfolios/snapshots",
            get(list_snapshots).post(create_snapshot),
        )
        .route(
            "/api/mportfolios/snapshots/{id}",
            get(snapshot_detail).delete(delete_snapshot),
        )
        .route(
            "/api/mportfolios/snapshots/by-slug/{slug}",
            axum::routing::delete(delete_snapshots_by_slug),
        )
        .route("/api/mportfolios/{slug}", get(detail))
}

#[derive(Deserialize)]
struct ListQuery {
    /// Name substring filter (case-insensitive).
    q: Option<String>,
    /// Keep only portfolios holding this ticker (case-insensitive).
    ticker: Option<String>,
}

fn clean(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let portfolios = store::list_portfolios(&state.pool, clean(&q.q), clean(&q.ticker)).await?;
    let updated_at = store::last_refreshed(&state.pool)
        .await?
        .and_then(|t| t.format(&Rfc3339).ok());
    Ok(Json(json!({ "portfolios": portfolios, "updated_at": updated_at })))
}

async fn detail(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let portfolio = store::get_portfolio(&state.pool, &slug)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    let holdings = store::list_holdings(&state.pool, portfolio.id).await?;
    Ok(Json(json!({ "portfolio": portfolio, "holdings": holdings })))
}

async fn refresh(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    // Run inline so the caller sees completion; the lock serializes against the scheduled job.
    mportfolios_job::refresh(&state.pool, &state.mportfolios_refresh).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SnapshotQuery {
    /// Name substring filter (case-insensitive).
    q: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SnapshotBody {
    /// Slug of the live portfolio to snapshot.
    slug: String,
}

/// List all saved snapshots (newest first), optionally filtered by portfolio name.
async fn list_snapshots(
    State(state): State<AppState>,
    Query(q): Query<SnapshotQuery>,
) -> Result<Json<Value>, ApiError> {
    let snapshots = store::list_snapshots(&state.pool, clean(&q.q)).await?;
    Ok(Json(json!({ "snapshots": snapshots })))
}

/// Take a snapshot of the given live portfolio.
async fn create_snapshot(
    State(state): State<AppState>,
    Json(body): Json<SnapshotBody>,
) -> Result<Json<Value>, ApiError> {
    let slug = body.slug.trim();
    if slug.is_empty() {
        return Err(ApiError::bad_request("slug is required"));
    }
    let id = store::create_snapshot(&state.pool, slug)
        .await?
        .ok_or_else(|| ApiError::not_found("portfolio not found"))?;
    Ok(Json(json!({ "id": id })))
}

/// One snapshot + its frozen holdings.
async fn snapshot_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (snapshot, holdings) = store::get_snapshot(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("snapshot not found"))?;
    Ok(Json(json!({ "snapshot": snapshot, "holdings": holdings })))
}

async fn delete_snapshot(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_snapshot(&state.pool, id).await? {
        return Err(ApiError::not_found("snapshot not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Delete every snapshot for one source portfolio.
async fn delete_snapshots_by_slug(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let deleted = store::delete_snapshots_by_slug(&state.pool, &slug).await?;
    Ok(Json(json!({ "deleted": deleted })))
}
