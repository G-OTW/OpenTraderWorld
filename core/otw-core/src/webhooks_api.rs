//! HTTP API for the Webhooks module — manage inbound endpoints (session-protected).
//!
//! The inbound receiver itself lives in `webhooks_inbound` (public route, token in the
//! URL). Endpoint plaintext tokens are returned exactly once, from the create call.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{webhooks_inbound, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/webhooks", get(list).post(create))
        .route("/api/webhooks/{id}", axum::routing::patch(update).delete(remove))
        .route("/api/webhooks/{id}/events", get(events))
}

fn targets_json() -> Vec<Value> {
    webhooks_inbound::TARGETS
        .iter()
        .map(|t| json!({ "id": t.id, "label": t.label, "needs": t.needs }))
        .collect()
}

/// Endpoints plus the redirect targets the create/edit form offers.
async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let webhooks = otw_store::webhooks::list_endpoints(&state.pool).await?;
    Ok(Json(json!({ "webhooks": webhooks, "targets": targets_json() })))
}

fn validate_target(target: &str) -> Result<(), ApiError> {
    if !webhooks_inbound::TARGETS.iter().any(|t| t.id == target) {
        return Err(ApiError::bad_request(&format!("unknown target: {target}")));
    }
    Ok(())
}

/// A target that needs an object (`automator` → `workflow_id`) is only stored once that
/// object resolves: an endpoint pointing at nothing would only fail at delivery time,
/// under a public URL, where nobody is watching.
async fn validate_config(
    state: &AppState,
    target: &str,
    config: &Value,
) -> Result<(), ApiError> {
    match webhooks_inbound::TARGETS.iter().find(|t| t.id == target).and_then(|t| t.needs) {
        Some("workflow_id") => {
            let id = webhooks_inbound::config_workflow_id(config)
                .ok_or_else(|| ApiError::bad_request("pick the workflow this webhook runs"))?;
            if otw_store::automator::get_workflow(&state.pool, id).await?.is_none() {
                return Err(ApiError::not_found("workflow not found"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[derive(Deserialize)]
struct CreateInput {
    name: String,
    #[serde(default = "default_target")]
    target: String,
    #[serde(default)]
    config: Option<Value>,
}
fn default_target() -> String {
    "remindme".to_string()
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateInput>,
) -> Result<Json<Value>, ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("webhook name is required"));
    }
    validate_target(&input.target)?;
    let config = input.config.unwrap_or_else(|| json!({}));
    validate_config(&state, &input.target, &config).await?;
    let (row, token) =
        otw_store::webhooks::create_endpoint(&state.pool, name, &input.target, &config).await?;
    tracing::info!("webhook endpoint created: {name} → {}", input.target);
    // The frontend prepends its own origin; the path is what the backend owns.
    Ok(Json(json!({ "webhook": row, "token": token, "path": format!("/api/hooks/{token}") })))
}

#[derive(Deserialize)]
struct UpdateInput {
    name: Option<String>,
    target: Option<String>,
    config: Option<Value>,
    enabled: Option<bool>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateInput>,
) -> Result<Json<Value>, ApiError> {
    if let Some(name) = &input.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("webhook name cannot be empty"));
        }
    }
    // `update_endpoint` COALESCEs, so a PATCH that only switches the target keeps the
    // stored config: validate the pair the endpoint will actually hold, not the patch.
    if input.target.is_some() || input.config.is_some() {
        let current = otw_store::webhooks::get_endpoint(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("webhook not found"))?;
        let target = input.target.clone().unwrap_or(current.target);
        validate_target(&target)?;
        let config = input.config.clone().unwrap_or(current.config);
        validate_config(&state, &target, &config).await?;
    }
    let row = otw_store::webhooks::update_endpoint(
        &state.pool,
        id,
        input.name.as_deref().map(str::trim),
        input.target.as_deref(),
        input.config.as_ref(),
        input.enabled,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("webhook not found"))?;
    Ok(Json(json!({ "webhook": row })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !otw_store::webhooks::delete_endpoint(&state.pool, id).await? {
        return Err(ApiError::not_found("webhook not found"));
    }
    tracing::info!("webhook endpoint revoked: {id}");
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}
fn default_limit() -> i64 {
    50
}

async fn events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Value>, ApiError> {
    let events =
        otw_store::webhooks::list_events(&state.pool, id, q.limit.clamp(1, 50)).await?;
    Ok(Json(json!({ "events": events })))
}
