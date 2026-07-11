//! HTTP API for the FinanceDatabase module ("findb").
//!
//! Catalog search + install (bulk import on first use) + favorites organized in folders.
//! Install runs in the background; the frontend polls `GET /api/findb/status`.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{findb_import, ApiError, AppState};
use otw_store::findb::{self, FavoriteInput, FolderInput, SearchFilters, SortBy};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/findb/status", get(status))
        .route("/api/findb/install", axum::routing::post(install))
        .route("/api/findb/search", get(search))
        .route("/api/findb/facets", get(facets))
        .route("/api/findb/folders", get(list_folders).post(add_folder))
        .route(
            "/api/findb/folders/{id}",
            axum::routing::patch(update_folder).delete(delete_folder),
        )
        .route("/api/findb/favorites", get(list_favorites).post(add_favorite))
        .route(
            "/api/findb/favorites/{id}",
            axum::routing::patch(update_favorite).delete(delete_favorite),
        )
}

// --- install / status ---

async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let meta = findb::get_meta(&state.pool).await?;
    let importing = state.findb_importing.load(std::sync::atomic::Ordering::Relaxed);
    Ok(Json(json!({
        "installed": meta.installed,
        "importing": importing,
        "version": meta.version,
        "count": meta.count,
    })))
}

async fn install(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    use std::sync::atomic::Ordering;
    // Guard against concurrent installs: claim the flag, refuse if already running.
    if state
        .findb_importing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return Err(ApiError::conflict("install already in progress"));
    }
    let pool = state.pool.clone();
    let flag = state.findb_importing.clone();
    tokio::spawn(async move {
        if let Err(e) = findb_import::run(&pool).await {
            tracing::error!("findb import failed: {e:#}");
        }
        flag.store(false, Ordering::Release);
    });
    Ok(Json(json!({ "ok": true, "importing": true })))
}

// --- search ---

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    exchange: Option<String>,
    currency: Option<String>,
    country: Option<String>,
    sector: Option<String>,
    industry: Option<String>,
    category: Option<String>,
    family: Option<String>,
    sort: Option<SortBy>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn search(
    State(state): State<AppState>,
    Query(p): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let filters = SearchFilters {
        asset_type: p.asset_type,
        exchange: p.exchange,
        currency: p.currency,
        country: p.country,
        sector: p.sector,
        industry: p.industry,
        category: p.category,
        family: p.family,
    };
    let page = findb::search(
        &state.pool,
        &p.q,
        &filters,
        p.sort.unwrap_or_default(),
        p.limit.unwrap_or(40),
        p.offset.unwrap_or(0),
    )
    .await?;
    Ok(Json(json!({ "results": page.results, "has_more": page.has_more })))
}

// --- facets (distinct values for filter dropdowns) ---

#[derive(Deserialize)]
struct FacetQuery {
    column: String,
    #[serde(rename = "type")]
    asset_type: Option<String>,
}

async fn facets(
    State(state): State<AppState>,
    Query(p): Query<FacetQuery>,
) -> Result<Json<Value>, ApiError> {
    let at = p.asset_type.as_deref().filter(|s| !s.is_empty());
    let values = findb::facet_values(&state.pool, &p.column, at)
        .await
        .map_err(|_| ApiError::bad_request("invalid facet column"))?;
    Ok(Json(json!({ "values": values })))
}

// --- folders ---

async fn list_folders(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let folders = findb::list_folders(&state.pool).await?;
    Ok(Json(json!({ "folders": folders })))
}

async fn add_folder(
    State(state): State<AppState>,
    Json(mut input): Json<FolderInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        return Err(ApiError::bad_request("folder name required"));
    }
    let folder = findb::add_folder(&state.pool, &input).await?;
    Ok(Json(json!({ "folder": folder })))
}

async fn update_folder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<FolderInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        return Err(ApiError::bad_request("folder name required"));
    }
    if !findb::update_folder(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("folder not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !findb::delete_folder(&state.pool, id).await? {
        return Err(ApiError::not_found("folder not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// --- favorites ---

async fn list_favorites(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let favorites = findb::list_favorites(&state.pool).await?;
    Ok(Json(json!({ "favorites": favorites })))
}

async fn add_favorite(
    State(state): State<AppState>,
    Json(input): Json<FavoriteInput>,
) -> Result<Json<Value>, ApiError> {
    if findb::get_instrument(&state.pool, input.instrument_id)
        .await?
        .is_none()
    {
        return Err(ApiError::bad_request("unknown instrument"));
    }
    let id = findb::add_favorite(&state.pool, &input).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_favorite(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<FavoriteInput>,
) -> Result<Json<Value>, ApiError> {
    if !findb::update_favorite(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("favorite not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_favorite(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !findb::delete_favorite(&state.pool, id).await? {
        return Err(ApiError::not_found("favorite not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
