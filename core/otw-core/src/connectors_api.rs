//! HTTP API for the centralized data broker (market-data connectors).
//!
//! - `GET  /api/connectors/providers`        capability matrix (drives the create form)
//! - `GET  /api/connectors/modules`          module ids a connector can be granted to
//! - `GET  /api/connectors?module=histviz`   connectors + creds status + quota usage
//! - `POST /api/connectors`                  create (provider, name, modules, limit?)
//! - `PATCH/DELETE /api/connectors/{id}`     rename / re-grant / set limit / remove
//! - `POST /api/connectors/{id}/secrets`     set a credential (write-only)
//! - `DELETE /api/connectors/{id}/secrets/{name}`
//! - `POST /api/connectors/{id}/test`        reach the provider and report what answered
//!
//! Two kinds of provider settings live on a connector. A **credential** is sealed and
//! write-only (the API only reports which names are set). A **setting** is not a secret:
//! an IB Gateway host and port are addresses, they are stored and returned in the clear,
//! and the provider declares them as `config_fields`. Hiding an address behind the
//! vault's write-only shape would make a failed connection impossible to diagnose.
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
pub const DATA_MODULES: &[&str] = &["histdata", "watchlists", "histviz", "journal", "portfolios"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/connectors/providers", get(providers))
        .route("/api/connectors/modules", get(modules))
        .route("/api/connectors", get(list).post(create))
        .route("/api/connectors/{id}", patch(update).delete(remove))
        .route("/api/connectors/{id}/secrets", post(set_secret))
        .route("/api/connectors/{id}/secrets/{name}", delete(delete_secret))
        .route("/api/connectors/{id}/test", post(test))
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
                "config_fields": cap.config_fields,
                "testable": cap.testable,
                "stream": live::stream_capable(cap.provider),
                "stream_asset_types": cap.stream_asset_types,
                "stream_timeframes": cap.stream_timeframes,
                "stream_note": cap.stream_note,
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
            "config_fields": cap.config_fields,
            "config": c.config,
            "testable": cap.testable,
            "stream": live::stream_capable(cap.provider),
            "stream_asset_types": cap.stream_asset_types,
            "stream_timeframes": cap.stream_timeframes,
            "stream_note": cap.stream_note,
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

/// Check a settings object against what the provider declares, and hand back both the
/// JSON to store and the flat map the connector reads.
///
/// Unknown keys are refused rather than dropped: a typo in a field name would otherwise be
/// stored silently and the connector would report a missing address it can see on screen.
///
/// Settings are all-or-nothing, because half of an address is not an address: any non-empty
/// object must carry every required field, and a PATCH clears the settings by sending an
/// empty one. `require_all` is the extra turn of the screw at creation time, where a
/// provider that needs settings may not be created without them.
fn normalize_config(
    provider: &str,
    input: &serde_json::Map<String, Value>,
    require_all: bool,
) -> Result<(Value, std::collections::HashMap<String, String>), ApiError> {
    let connector = histdata::connector_for(provider)
        .map_err(|_| ApiError::not_found("unknown provider"))?;
    let fields = connector.capability().config_fields;
    let mut out = serde_json::Map::new();
    let mut flat = std::collections::HashMap::new();
    for (key, value) in input {
        let Some(field) = fields.iter().find(|f| f.name == key) else {
            return Err(ApiError::bad_request(&format!(
                "{} has no setting called {key:?}",
                connector.capability().label
            )));
        };
        // A form posts everything as text; a number field is stored as one so the value
        // reads back the way it was declared.
        let text = match value {
            Value::String(s) => s.trim().to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        if text.is_empty() {
            continue;
        }
        let stored = if field.kind == "number" {
            text.parse::<i64>()
                .map(|n| Value::from(n))
                .map_err(|_| ApiError::bad_request(&format!("{key} must be a number")))?
        } else {
            Value::from(text.clone())
        };
        out.insert(key.clone(), stored);
        flat.insert(key.clone(), text);
    }
    if require_all || !flat.is_empty() {
        for f in fields.iter().filter(|f| f.required) {
            if !flat.contains_key(f.name) {
                return Err(ApiError::bad_request(&format!("{} is required", f.label)));
            }
        }
        // The provider gets the last word on its own settings (a port that is not a port).
        connector
            .validate_config(&flat)
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    }
    Ok((Value::Object(out), flat))
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
    /// Non-secret provider settings, keyed by the names in the provider's `config_fields`.
    config: Option<serde_json::Map<String, Value>>,
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
    // Settings are validated before the row exists: a connector created without the
    // address its provider needs would be listed as usable and fail on first use.
    let empty = serde_json::Map::new();
    let (config, _) =
        normalize_config(&body.provider, body.config.as_ref().unwrap_or(&empty), true)?;
    let row = store::create(&state.pool, &body.provider, name, &modules, &config)
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
    /// Replaces the stored settings wholesale (absent = leave them alone).
    config: Option<serde_json::Map<String, Value>>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchBody>,
) -> Result<Json<Value>, ApiError> {
    let Some(existing) = store::get(&state.pool, id).await? else {
        return Err(ApiError::not_found("connector not found"));
    };
    if let Some(cfg) = &body.config {
        let (config, _) = normalize_config(&existing.provider, cfg, false)?;
        store::set_config(&state.pool, id, &config).await?;
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

/// Reach the provider with this connector's own settings and credentials and report what
/// answered. Only providers declaring `testable` have anything to say: a REST provider
/// fails loudly on the first download, while a socket provider can fail for half a dozen
/// reasons that all look like silence and are all fixed in another application.
async fn test(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let conn = store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("connector not found"))?;
    let connector = histdata::connector_for(&conn.provider)
        .map_err(|_| ApiError::not_found("unknown provider"))?;
    if !connector.capability().testable {
        return Err(ApiError::bad_request(&format!(
            "{} has no connection test",
            connector.capability().label
        )));
    }
    let settings = store::load_creds(&state.pool, &state.cipher, id).await?;
    // A failed test is an answer, not a server error: it carries the setting to change.
    match connector.test(&state.http, &settings).await {
        Ok(detail) => Ok(Json(json!({ "ok": true, "detail": detail }))),
        Err(e) => Ok(Json(json!({ "ok": false, "detail": format!("{e:#}") }))),
    }
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

    fn cfg(pairs: &[(&str, &str)]) -> serde_json::Map<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), Value::from(*v))).collect()
    }

    #[test]
    fn settings_are_checked_against_what_the_provider_declares() {
        // A provider with no settings accepts none.
        assert!(normalize_config("binance", &cfg(&[("host", "x")]), false).is_err());
        assert!(normalize_config("binance", &cfg(&[]), true).is_ok());

        // Half an address is not an address, on create and on update alike; an empty
        // object is how a PATCH clears the settings instead of half-writing them.
        assert!(normalize_config("ibkr", &cfg(&[("host", "127.0.0.1")]), true).is_err());
        assert!(normalize_config("ibkr", &cfg(&[("host", "127.0.0.1")]), false).is_err());
        assert!(normalize_config("ibkr", &cfg(&[]), false).is_ok());
        assert!(normalize_config("ibkr", &cfg(&[]), true).is_err());

        // A number field is stored as a number, and trimmed on the way in.
        let Ok((stored, flat)) =
            normalize_config("ibkr", &cfg(&[("host", " 127.0.0.1 "), ("port", "4002")]), true)
        else {
            panic!("a host and a port is a complete IB connector");
        };
        assert_eq!(stored["port"], Value::from(4002));
        assert_eq!(flat["host"], "127.0.0.1");
        assert!(normalize_config("ibkr", &cfg(&[("host", "h"), ("port", "n/a")]), true).is_err());
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
        assert!(normalize_modules(&["editor".to_string()]).is_err());
    }
}
