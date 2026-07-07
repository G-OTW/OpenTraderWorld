//! HTTP API for the Resources module — master categories plus the bookmarks they hold.

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::resources::{self, CategoryInput, ResourceInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/resources/categories", get(list_categories).post(add_category))
        .route(
            "/api/resources/categories/{id}",
            axum::routing::patch(update_category).delete(remove_category),
        )
        .route("/api/resources", get(list_resources).post(add_resource))
        .route("/api/resources/{id}", axum::routing::patch(update_resource).delete(remove_resource))
}

// ── Categories ─────────────────────────────────────────────────────────────

async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let categories = resources::list_categories(&state.pool).await?;
    Ok(Json(json!({ "categories": categories })))
}

async fn add_category(
    State(state): State<AppState>,
    Json(mut input): Json<CategoryInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        return Err(ApiError::bad_request("category name required"));
    }
    let category = resources::add_category(&state.pool, &input).await?;
    Ok(Json(json!({ "category": category })))
}

async fn update_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<CategoryInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        return Err(ApiError::bad_request("category name required"));
    }
    if !resources::update_category(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("category not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !resources::delete_category(&state.pool, id).await? {
        return Err(ApiError::not_found("category not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Resources ──────────────────────────────────────────────────────────────

async fn list_resources(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let resources = resources::list_resources(&state.pool).await?;
    Ok(Json(json!({ "resources": resources })))
}

fn validate(input: &ResourceInput) -> Result<(), ApiError> {
    if input.category_id.is_none() {
        return Err(ApiError::bad_request("category required"));
    }
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("resource name required"));
    }
    Ok(())
}

async fn add_resource(
    State(state): State<AppState>,
    Json(mut input): Json<ResourceInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    input.link = input.link.trim().to_string();
    validate(&input)?;
    let resource = resources::add_resource(&state.pool, &input).await?;
    Ok(Json(json!({ "resource": resource })))
}

async fn update_resource(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<ResourceInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    input.link = input.link.trim().to_string();
    validate(&input)?;
    if !resources::update_resource(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("resource not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove_resource(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !resources::delete_resource(&state.pool, id).await? {
        return Err(ApiError::not_found("resource not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
