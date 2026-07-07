//! HTTP API for Settings → MCP — manage the global toggle and access tokens.
//!
//! Session-protected (browser only); the MCP endpoint itself lives in `mcp` and uses
//! bearer tokens. Token plaintext is returned exactly once, from the create call.

use axum::{
    extract::{Path, State},
    routing::{get, patch},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{mcp::catalog, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mcp/settings", get(get_settings).post(set_settings))
        .route("/api/mcp/tokens", get(list_tokens).post(create_token))
        .route("/api/mcp/tokens/{id}", patch(update_token).delete(delete_token))
}

/// Global toggle + the module list the permission matrix is built from.
async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let enabled = otw_store::settings::get_or(&state.pool, "mcp_enabled", "false").await?;
    let modules: Vec<Value> = catalog::MODULES
        .iter()
        .map(|(id, label)| {
            let endpoints = catalog::CATALOG.iter().filter(|e| e.module == *id).count();
            json!({ "id": id, "label": label, "endpoints": endpoints })
        })
        .collect();
    Ok(Json(json!({ "enabled": enabled == "true", "modules": modules })))
}

#[derive(Deserialize)]
struct SetSettings {
    enabled: bool,
}

async fn set_settings(
    State(state): State<AppState>,
    Json(input): Json<SetSettings>,
) -> Result<Json<Value>, ApiError> {
    otw_store::settings::set(&state.pool, "mcp_enabled", if input.enabled { "true" } else { "false" })
        .await?;
    tracing::info!("mcp endpoint {}", if input.enabled { "enabled" } else { "disabled" });
    Ok(Json(json!({ "ok": true, "enabled": input.enabled })))
}

async fn list_tokens(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let tokens = otw_store::mcp::list_tokens(&state.pool).await?;
    Ok(Json(json!({ "tokens": tokens })))
}

/// Permissions must map known module ids to "r" or "rw"; anything else is rejected
/// so a typo can never silently widen (or dead-letter) a token's access.
fn validate_permissions(perms: &Value) -> Result<(), ApiError> {
    let Some(map) = perms.as_object() else {
        return Err(ApiError::bad_request("permissions must be an object"));
    };
    for (module, lvl) in map {
        if !catalog::MODULES.iter().any(|(id, _)| id == module) {
            return Err(ApiError::bad_request(&format!("unknown module: {module}")));
        }
        if !matches!(lvl.as_str(), Some("r") | Some("rw")) {
            return Err(ApiError::bad_request(&format!(
                "permission for {module} must be \"r\" or \"rw\""
            )));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct CreateToken {
    name: String,
    permissions: Value,
}

async fn create_token(
    State(state): State<AppState>,
    Json(input): Json<CreateToken>,
) -> Result<Json<Value>, ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("token name is required"));
    }
    validate_permissions(&input.permissions)?;
    let (row, plaintext) = otw_store::mcp::create_token(&state.pool, name, &input.permissions).await?;
    tracing::info!("mcp token created: {name}");
    Ok(Json(json!({ "token": plaintext, "record": row })))
}

#[derive(Deserialize)]
struct UpdateToken {
    name: Option<String>,
    permissions: Option<Value>,
}

async fn update_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateToken>,
) -> Result<Json<Value>, ApiError> {
    if let Some(name) = &input.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("token name cannot be empty"));
        }
    }
    if let Some(perms) = &input.permissions {
        validate_permissions(perms)?;
    }
    let row = otw_store::mcp::update_token(
        &state.pool,
        id,
        input.name.as_deref().map(str::trim),
        input.permissions.as_ref(),
    )
    .await?
    .ok_or_else(|| ApiError::not_found("token not found"))?;
    Ok(Json(json!({ "record": row })))
}

async fn delete_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !otw_store::mcp::delete_token(&state.pool, id).await? {
        return Err(ApiError::not_found("token not found"));
    }
    tracing::info!("mcp token revoked: {id}");
    Ok(Json(json!({ "ok": true })))
}
