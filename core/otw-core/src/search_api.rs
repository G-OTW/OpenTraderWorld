//! Global search API — title-only lookups backing the top-bar search.
//!
//! The frontend matches module names and settings sections client-side; this endpoint
//! covers the DB-backed scopes. Deliberately NOT in the MCP catalog.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};
use otw_store::search;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/search", get(search_titles))
}

#[derive(Deserialize)]
struct Params {
    q: String,
    /// Comma-separated scope keys (see `search::known_scopes`); unknown keys are skipped.
    #[serde(default)]
    scopes: String,
}

async fn search_titles(
    State(state): State<AppState>,
    Query(p): Query<Params>,
) -> Result<Json<Value>, ApiError> {
    let q = p.q.trim();
    if q.is_empty() {
        return Err(ApiError::bad_request("query required"));
    }
    if q.chars().count() > 200 {
        return Err(ApiError::bad_request("query too long"));
    }
    let scopes: Vec<&str> = p
        .scopes
        .split(',')
        .map(str::trim)
        .filter(|s| search::known_scopes().any(|k| k == *s))
        .collect();
    let hits = search::search_titles(&state.pool, &scopes, q).await?;
    Ok(Json(json!({ "hits": hits })))
}
