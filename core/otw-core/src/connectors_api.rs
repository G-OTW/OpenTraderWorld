//! HTTP API for the centralized data broker (market-data connectors).
//!
//! - `GET  /api/connectors/providers`        capability matrix (drives the create form)
//! - `GET  /api/connectors/modules`          module ids a connector can be granted to
//! - `GET  /api/connectors?module=histviz`   connectors + creds status + quota usage
//! - `POST /api/connectors`                  create (provider, name, modules, limit?)
//! - `PATCH/DELETE /api/connectors/{id}`     rename / re-grant / set limit / remove
//! - `POST /api/connectors/{id}/secrets`     set a credential (write-only)
//! - `DELETE /api/connectors/{id}/secrets/{name}`
//!
//! One connector list serves every data module; which modules may use a connector is the
//! `modules` grant list (`["*"]` = all). Credentials are write-only — the API only ever
//! reports *which* secret names are set, never a value.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::histdata;
use crate::live;
use crate::{ApiError, AppState};
use otw_store::connectors as store;

/// Modules that consume market data and can therefore be granted a connector. Adding a
/// data module means adding it here (the wildcard grant `*` covers it retroactively).
pub const DATA_MODULES: &[&str] = &["histdata", "watchlists", "histviz"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/connectors/providers", get(providers))
        .route("/api/connectors/modules", get(modules))
        .route("/api/connectors", get(list).post(create))
        .route("/api/connectors/{id}", patch(update).delete(remove))
        .route("/api/connectors/{id}/secrets", post(set_secret))
        .route("/api/connectors/{id}/secrets/{name}", delete(delete_secret))
}

/// The api_quota scope of a connector (kept from the histdata era — the ids are the same).
pub fn quota_scope(id: Uuid) -> String {
    format!("histconn:{id}")
}

/// Capability matrix only — connector-independent facts about each supported provider.
/// The create form, the module pickers and MCP clients read this.
async fn providers(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let out: Vec<Value> = histdata::capabilities()
        .into_iter()
        .map(|cap| {
            json!({
                "provider": cap.provider,
                "label": cap.label,
                "website": cap.website,
                "docs_url": cap.docs_url,
                "rate_limit": cap.rate_limit,
                "required_secrets": cap.required_secrets,
                "asset_types": cap.asset_types,
                "timeframes": cap.timeframes,
                "adjusted": cap.adjusted,
                "searchable": cap.searchable,
                "stream": live::stream_capable(cap.provider),
            })
        })
        .collect();
    Ok(Json(json!({ "providers": out })))
}

/// Grantable module ids, in display order. Labels are the client's (module registry).
async fn modules() -> Json<Value> {
    Json(json!({ "modules": DATA_MODULES }))
}

#[derive(Deserialize)]
struct ListQuery {
    /// Restrict to connectors granted to this module. Absent = the whole broker list.
    module: Option<String>,
}

/// Connectors with capability facts, credential status and current quota usage merged
/// in — one call drives the broker admin and every module's source picker.
async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = match q.module.as_deref() {
        Some(m) => {
            valid_module(m)?;
            store::list_for_module(&state.pool, m).await?
        }
        None => store::list(&state.pool).await?,
    };
    let quotas = otw_store::api_quota::list_prefixed(&state.pool, "histconn:").await?;
    let mut out = Vec::with_capacity(rows.len());
    for c in rows {
        // A connector whose provider was removed from the build is skipped, not fatal.
        let Ok(cap) = histdata::connector_for(&c.provider).map(|k| k.capability()) else {
            continue;
        };
        let names = store::list_cred_names(&state.pool, c.id).await?;
        let secrets: Vec<Value> = store::list_cred_meta(&state.pool, c.id)
            .await?
            .into_iter()
            .map(|(name, vault_item_id)| json!({ "name": name, "vault_item_id": vault_item_id }))
            .collect();
        let quota = quotas.iter().find(|q| q.scope == quota_scope(c.id));
        out.push(json!({
            "id": c.id,
            "provider": c.provider,
            "name": c.name,
            "modules": c.modules,
            "label": cap.label,
            "website": cap.website,
            "docs_url": cap.docs_url,
            "rate_limit": cap.rate_limit,
            "required_secrets": cap.required_secrets,
            "set_secrets": names,
            "secrets": secrets,
            "asset_types": cap.asset_types,
            "timeframes": cap.timeframes,
            "adjusted": cap.adjusted,
            "searchable": cap.searchable,
            "stream": live::stream_capable(cap.provider),
            "quota": quota,
        }));
    }
    Ok(Json(json!({ "connectors": out })))
}

/// A module id a connector can be granted to.
fn valid_module(m: &str) -> Result<(), ApiError> {
    if DATA_MODULES.contains(&m) {
        Ok(())
    } else {
        Err(ApiError::bad_request(&format!("unknown module: {m}")))
    }
}

/// Normalize a grant list: `["*"]` collapses everything else; ids are validated and
/// de-duplicated. An empty list is allowed — the connector then shows up nowhere but the
/// broker admin, which is how a half-configured provider stays out of the way.
fn normalize_modules(input: &[String]) -> Result<Vec<String>, ApiError> {
    if input.iter().any(|m| m == store::ALL_MODULES) {
        return Ok(vec![store::ALL_MODULES.to_string()]);
    }
    let mut out: Vec<String> = Vec::new();
    for m in input {
        let m = m.trim();
        valid_module(m)?;
        if !out.iter().any(|x| x == m) {
            out.push(m.to_string());
        }
    }
    Ok(out)
}

/// Optional request limit on a connector: `enabled` toggles tracking, `max_requests`
/// NULL/absent = unlimited (still tracked), `period` per api_quota::PERIODS.
#[derive(Deserialize)]
pub struct LimitBody {
    enabled: bool,
    max_requests: Option<i64>,
    #[serde(default)]
    period: String,
}

/// Apply a limit declaration to a connector's quota scope.
async fn apply_limit(state: &AppState, id: Uuid, limit: &LimitBody) -> Result<(), ApiError> {
    if !limit.enabled {
        otw_store::api_quota::remove(&state.pool, &quota_scope(id)).await?;
        return Ok(());
    }
    if !otw_store::api_quota::valid_period(&limit.period) {
        return Err(ApiError::bad_request("period must be minute|hour|day|week|month"));
    }
    if limit.max_requests.is_some_and(|n| n < 1) {
        return Err(ApiError::bad_request("max_requests must be at least 1"));
    }
    otw_store::api_quota::set(&state.pool, &quota_scope(id), limit.max_requests, &limit.period)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct CreateBody {
    provider: String,
    name: String,
    /// Module ids allowed to use this connector, or `["*"]` for all. Default: all.
    modules: Option<Vec<String>>,
    limit: Option<LimitBody>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("connector name is required"));
    }
    histdata::connector_for(&body.provider).map_err(|_| ApiError::not_found("unknown provider"))?;
    let modules = match &body.modules {
        Some(m) => normalize_modules(m)?,
        None => vec![store::ALL_MODULES.to_string()],
    };
    let row = store::create(&state.pool, &body.provider, name, &modules)
        .await
        .map_err(|_| ApiError::bad_request("a connector with this name already exists"))?;
    if let Some(limit) = &body.limit {
        apply_limit(&state, row.id, limit).await?;
    }
    Ok(Json(json!({ "connector": row })))
}

#[derive(Deserialize)]
struct PatchBody {
    name: Option<String>,
    modules: Option<Vec<String>>,
    limit: Option<LimitBody>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchBody>,
) -> Result<Json<Value>, ApiError> {
    if store::get(&state.pool, id).await?.is_none() {
        return Err(ApiError::not_found("connector not found"));
    }
    if let Some(name) = &body.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("connector name is required"));
        }
        store::rename(&state.pool, id, name)
            .await
            .map_err(|_| ApiError::bad_request("a connector with this name already exists"))?;
    }
    if let Some(modules) = &body.modules {
        let modules = normalize_modules(modules)?;
        store::set_modules(&state.pool, id, &modules).await?;
    }
    if let Some(limit) = &body.limit {
        apply_limit(&state, id, limit).await?;
    }
    let row = store::get(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true, "connector": row })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("connector not found"));
    }
    otw_store::api_quota::remove(&state.pool, &quota_scope(id)).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SecretBody {
    name: String,
    /// Plaintext value (write-only), or empty when plugging a vault item instead.
    #[serde(default)]
    value: String,
    /// Centralized vault item to resolve this credential from (wins over `value`).
    #[serde(default)]
    vault_item_id: Option<Uuid>,
}

async fn set_secret(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SecretBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("secret name is required"));
    }
    let conn = store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("connector not found"))?;
    match body.vault_item_id {
        Some(item) => {
            store::set_cred_ref(&state.pool, id, &conn.provider, body.name.trim(), item).await?
        }
        None => {
            if body.value.is_empty() {
                return Err(ApiError::bad_request("secret value is required"));
            }
            store::set_cred(
                &state.pool,
                &state.cipher,
                id,
                &conn.provider,
                body.name.trim(),
                &body.value,
            )
            .await?;
        }
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_secret(
    State(state): State<AppState>,
    Path((id, name)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_cred(&state.pool, id, &name).await? {
        return Err(ApiError::not_found("secret not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ApiError` isn't Debug, so assert on the Ok value through a helper.
    fn ok(input: &[&str]) -> Vec<String> {
        match normalize_modules(&input.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
            Ok(v) => v,
            Err(_) => panic!("expected {input:?} to normalize"),
        }
    }

    #[test]
    fn wildcard_collapses_and_ids_are_validated() {
        assert_eq!(ok(&["histdata", "*"]), vec!["*".to_string()]);
        assert_eq!(
            ok(&["histdata", "histdata", "histviz"]),
            vec!["histdata".to_string(), "histviz".to_string()]
        );
        assert!(ok(&[]).is_empty());
        // A module that doesn't consume market data can't be granted.
        assert!(normalize_modules(&["journal".to_string()]).is_err());
    }
}
