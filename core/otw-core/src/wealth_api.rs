//! HTTP API for the MyWealth module.
//!
//! Asset templates (reserved + custom fields), assets, revisions (each "Update" adds one),
//! a net-worth time-series breakdown, and the display-currency setting.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::wealth::{
    self, AssetFilter, AssetInput, RevisionInput, TemplateInput, TemplatePatch, ASSET_TYPES,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/wealth/templates", get(list_templates).post(add_template))
        .route(
            "/api/wealth/templates/{id}",
            axum::routing::patch(update_template).delete(delete_template),
        )
        .route("/api/wealth/assets", get(list_assets).post(add_asset))
        .route(
            "/api/wealth/assets/{id}",
            axum::routing::patch(update_asset).delete(delete_asset),
        )
        .route(
            "/api/wealth/assets/{id}/revisions",
            get(list_revisions).post(add_revision),
        )
        .route(
            "/api/wealth/revisions/{id}",
            axum::routing::patch(update_revision).delete(delete_revision),
        )
        .route("/api/wealth/breakdown", get(breakdown))
        .route("/api/wealth/settings", get(get_settings).patch(update_settings))
}

// ── Templates ──

async fn list_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let templates = wealth::list_templates(&state.pool).await?;
    Ok(Json(json!({ "templates": templates })))
}

fn check_type(t: &str) -> Result<(), ApiError> {
    if !ASSET_TYPES.contains(&t) {
        return Err(ApiError::bad_request("unknown asset type"));
    }
    Ok(())
}

async fn add_template(
    State(state): State<AppState>,
    Json(mut input): Json<TemplateInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        input.name = "Untitled template".to_string();
    }
    check_type(&input.asset_type)?;
    let id = wealth::add_template(&state.pool, &input).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<TemplatePatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(t) = &patch.asset_type {
        check_type(t)?;
    }
    if !wealth::update_template(&state.pool, id, &patch).await? {
        return Err(ApiError::bad_request("template not found or is built-in"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_template(&state.pool, id).await? {
        return Err(ApiError::bad_request("template not found or is built-in"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Assets ──

#[derive(Deserialize)]
struct AssetQuery {
    asset_type: Option<String>,
    category: Option<String>,
}

async fn list_assets(
    State(state): State<AppState>,
    Query(q): Query<AssetQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = AssetFilter {
        asset_type: q.asset_type.filter(|s| !s.trim().is_empty()),
        category: q.category.filter(|s| !s.trim().is_empty()),
    };
    let assets = wealth::list_assets(&state.pool, &filter).await?;
    Ok(Json(json!({ "assets": assets })))
}

fn validate_asset(input: &AssetInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("asset name required"));
    }
    check_type(&input.asset_type)?;
    if !CURRENCIES.contains(&input.currency.as_str()) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    Ok(())
}

async fn add_asset(
    State(state): State<AppState>,
    Json(mut input): Json<AssetInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate_asset(&input)?;
    let id = wealth::add_asset(&state.pool, &input).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<AssetInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate_asset(&input)?;
    if !wealth::update_asset(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("asset not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_asset(&state.pool, id).await? {
        return Err(ApiError::not_found("asset not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Revisions ──

async fn list_revisions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let revisions = wealth::list_revisions(&state.pool, id).await?;
    Ok(Json(json!({ "revisions": revisions })))
}

async fn add_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RevisionInput>,
) -> Result<Json<Value>, ApiError> {
    let rev_id = wealth::add_revision(&state.pool, id, &input).await?;
    Ok(Json(json!({ "id": rev_id })))
}

async fn update_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<RevisionInput>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::update_revision(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("revision not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !wealth::delete_revision(&state.pool, id).await? {
        return Err(ApiError::not_found("revision not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Breakdown + settings ──

#[derive(Deserialize)]
struct BreakdownQuery {
    asset_type: Option<String>,
    category: Option<String>,
    #[serde(default = "default_points")]
    points_back: i64,
    /// "month" (default) or "year".
    granularity: Option<String>,
}
fn default_points() -> i64 {
    11
}

async fn breakdown(
    State(state): State<AppState>,
    Query(q): Query<BreakdownQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = AssetFilter {
        asset_type: q.asset_type.filter(|s| !s.trim().is_empty()),
        category: q.category.filter(|s| !s.trim().is_empty()),
    };
    let granularity = match q.granularity.as_deref() {
        Some("year") => "year",
        _ => "month",
    };
    let settings = wealth::get_settings(&state.pool).await?;
    let data = wealth::breakdown(
        &state.pool,
        &filter,
        &settings.display_currency,
        q.points_back.clamp(0, 60),
        granularity,
    )
    .await?;
    Ok(Json(json!({ "breakdown": data })))
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = wealth::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

#[derive(Deserialize)]
struct SettingsBody {
    display_currency: Option<String>,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(c) = body.display_currency {
        if !CURRENCIES.contains(&c.as_str()) {
            return Err(ApiError::bad_request("unsupported currency"));
        }
        wealth::set_display_currency(&state.pool, &c).await?;
    }
    let settings = wealth::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}
