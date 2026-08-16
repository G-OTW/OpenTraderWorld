//! HTTP API for the centralized secrets vault (Settings → Vault).
//!
//! Session-authed management surface. Values are write-only: they can be set or replaced
//! but never read back — list responses carry item names and metadata only. Deliberately
//! NOT in the MCP catalog (it manages credentials); keep it that way.
//!
//! Rate limits ride the shared quota system under a `vault:<uuid>` scope, so the numbers
//! shown here are vault-wide (any item counts toward the same counter), same
//! observe-and-display semantics as feed/histdata quotas.

use axum::{
    extract::{Path, State},
    routing::{delete, get, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use otw_store::{api_quota, vault};

use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/vault", get(list).post(create))
        .route("/api/vault/{id}", axum::routing::patch(rename).delete(remove))
        .route("/api/vault/{id}/items", put(set_item))
        .route("/api/vault/items/{item_id}", delete(remove_item))
        .route("/api/vault/{id}/quota", put(set_quota).delete(remove_quota))
}

/// All vaults with their item names (never values), reference counts, and quota state.
async fn list(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let vaults = vault::list(&s.pool).await?;
    let items = vault::list_items(&s.pool).await?;
    let quotas = api_quota::list_prefixed(&s.pool, "vault:").await?;

    let out: Vec<Value> = vaults
        .iter()
        .map(|v| {
            let scope = vault::quota_scope(v.id);
            let quota = quotas.iter().find(|q| q.scope == scope);
            let items: Vec<&vault::VaultItem> =
                items.iter().filter(|i| i.vault_id == v.id).collect();
            json!({
                "id": v.id,
                "name": v.name,
                "created_at": v.created_at.format(&time::format_description::well_known::Rfc3339).ok(),
                "items": items,
                "quota": quota,
            })
        })
        .collect();
    Ok(Json(json!({ "vaults": out })))
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

fn clean_name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if name.len() > 80 {
        return Err(ApiError::bad_request("name too long (max 80 chars)"));
    }
    Ok(name)
}

async fn create(
    State(s): State<AppState>,
    Json(body): Json<NameBody>,
) -> Result<Json<Value>, ApiError> {
    let name = clean_name(&body.name)?;
    if vault::find_by_name(&s.pool, name).await?.is_some() {
        return Err(ApiError::conflict("a vault with this name already exists"));
    }
    let v = vault::create(&s.pool, name).await?;
    Ok(Json(json!({ "vault": v })))
}

async fn rename(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<NameBody>,
) -> Result<Json<Value>, ApiError> {
    let name = clean_name(&body.name)?;
    if let Some(existing) = vault::find_by_name(&s.pool, name).await? {
        if existing.id != id {
            return Err(ApiError::conflict("a vault with this name already exists"));
        }
    }
    if !vault::rename(&s.pool, id, name).await? {
        return Err(ApiError::not_found("vault not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let used = vault::vault_usage(&s.pool, id).await?;
    if used > 0 {
        return Err(ApiError::conflict(
            "vault items are still plugged into modules; unplug them first",
        ));
    }
    if !vault::delete(&s.pool, id).await? {
        return Err(ApiError::not_found("vault not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ItemBody {
    name: String,
    /// Write-only plaintext; sealed on arrival, retrievable never.
    value: String,
}

async fn set_item(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ItemBody>,
) -> Result<Json<Value>, ApiError> {
    let name = clean_name(&body.name)?;
    if body.value.is_empty() {
        return Err(ApiError::bad_request("value is required"));
    }
    if vault::get(&s.pool, id).await?.is_none() {
        return Err(ApiError::not_found("vault not found"));
    }
    let item = vault::set_item(&s.pool, &s.cipher, id, name, &body.value).await?;
    Ok(Json(json!({ "item": item })))
}

async fn remove_item(
    State(s): State<AppState>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let used = vault::item_usage(&s.pool, item_id).await?;
    if used > 0 {
        return Err(ApiError::conflict(
            "this key is still plugged into a module; unplug it first",
        ));
    }
    if !vault::delete_item(&s.pool, item_id).await? {
        return Err(ApiError::not_found("item not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct QuotaBody {
    /// None = track usage without a cap.
    max_requests: Option<i64>,
    period: String,
}

async fn set_quota(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<QuotaBody>,
) -> Result<Json<Value>, ApiError> {
    if !api_quota::valid_period(&body.period) {
        return Err(ApiError::bad_request("invalid period"));
    }
    if body.max_requests.is_some_and(|m| m < 1) {
        return Err(ApiError::bad_request("max_requests must be at least 1"));
    }
    if vault::get(&s.pool, id).await?.is_none() {
        return Err(ApiError::not_found("vault not found"));
    }
    api_quota::set(&s.pool, &vault::quota_scope(id), body.max_requests, &body.period).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove_quota(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    api_quota::remove(&s.pool, &vault::quota_scope(id)).await?;
    Ok(Json(json!({ "ok": true })))
}
