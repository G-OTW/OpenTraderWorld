//! HTTP API for the account broker (broker accounts).
//!
//! - `GET  /api/brokers/providers`       capability matrix (drives the create form)
//! - `GET  /api/brokers/modules`         module ids an account can be granted to
//! - `GET  /api/brokers?module=journal`  accounts + credential status
//! - `POST /api/brokers`                 create (broker, name, modules, config?)
//! - `PATCH/DELETE /api/brokers/{id}`    rename / re-grant / re-configure / remove
//! - `POST /api/brokers/{id}/secrets`    set a credential (write-only)
//! - `DELETE /api/brokers/{id}/secrets/{name}`
//! - `POST /api/brokers/{id}/test`       reach the broker and report what answered
//! - `GET  /api/brokers/{id}/symbols`    instruments worth offering in a picker
//! - `GET  /api/brokers/{id}/positions`  open positions, read-only
//! - `GET  /api/brokers/{id}/orders`     working orders, read-only
//! - `GET  /api/brokers/{id}/book`       positions + orders in one read, for a chart overlay
//! - `GET  /api/brokers/{id}/holdings`   what the account owns, one line per asset
//!
//! Same shape as `connectors_api` on purpose, and deliberately a different list: a market
//! data key and an account key are not the same credential and do not deserve the same
//! grants. **Everything here reads.** No route places, changes or cancels an order.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::brokers;
use crate::{ApiError, AppState};
use otw_store::brokers as store;

/// Modules that read a broker account and can therefore be granted one. Adding a module
/// means adding it here (the wildcard grant `*` covers it retroactively).
pub const ACCOUNT_MODULES: &[&str] = &["journal", "portfolios", "histviz", "taxcalc"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/brokers/providers", get(providers))
        .route("/api/brokers/modules", get(modules))
        .route("/api/brokers", get(list).post(create))
        .route("/api/brokers/{id}", patch(update).delete(remove))
        .route("/api/brokers/{id}/secrets", post(set_secret))
        .route("/api/brokers/{id}/secrets/{name}", delete(delete_secret))
        .route("/api/brokers/{id}/test", post(test))
        .route("/api/brokers/{id}/symbols", get(symbols))
        .route("/api/brokers/{id}/positions", get(positions))
        .route("/api/brokers/{id}/orders", get(orders))
        .route("/api/brokers/{id}/book", get(book))
        .route("/api/brokers/{id}/holdings", get(holdings))
}

/// Capability matrix only: account-independent facts about each supported broker.
async fn providers() -> Json<Value> {
    Json(json!({ "providers": brokers::capabilities() }))
}

/// Grantable module ids, in display order. Labels are the client's (module registry).
async fn modules() -> Json<Value> {
    Json(json!({ "modules": ACCOUNT_MODULES }))
}

#[derive(Deserialize)]
struct ListQuery {
    /// Restrict to accounts granted to this module. Absent = the whole broker list.
    module: Option<String>,
}

/// A module asking for an account's book proves its grant the same way.

/// Accounts with their broker's capability facts and credential status merged in, so one
/// call drives the settings section and every module's account picker.
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
    let mut out = Vec::with_capacity(rows.len());
    for a in rows {
        // An account whose broker was removed from the build is skipped, not fatal.
        let Ok(cap) = brokers::broker_for(&a.broker).map(|b| b.capability()) else {
            continue;
        };
        let names = store::list_cred_names(&state.pool, a.id).await?;
        let secrets: Vec<Value> = store::list_cred_meta(&state.pool, a.id)
            .await?
            .into_iter()
            .map(|(name, vault_item_id)| json!({ "name": name, "vault_item_id": vault_item_id }))
            .collect();
        out.push(json!({
            "id": a.id,
            "broker": a.broker,
            "name": a.name,
            "modules": a.modules,
            "config": a.config,
            "set_secrets": names,
            "secrets": secrets,
            "capability": cap,
        }));
    }
    Ok(Json(json!({ "accounts": out })))
}

/// A module id an account can be granted to.
fn valid_module(m: &str) -> Result<(), ApiError> {
    if ACCOUNT_MODULES.contains(&m) {
        Ok(())
    } else {
        Err(ApiError::bad_request(&format!("unknown module: {m}")))
    }
}

/// Normalize a grant list: `["*"]` collapses everything else; ids are validated and
/// de-duplicated. An empty list is allowed: the account then shows up nowhere but the
/// settings section, which is how a half-configured broker stays out of the way.
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

/// Check a settings object against what the broker declares, and hand back both the JSON to
/// store and the flat map the connector reads. Unknown keys are refused rather than
/// dropped, and settings are all-or-nothing: any non-empty object must carry every required
/// field, and an empty one clears them.
fn normalize_config(
    broker: &str,
    input: &serde_json::Map<String, Value>,
    require_all: bool,
) -> Result<(Value, std::collections::HashMap<String, String>), ApiError> {
    let connector = brokers::broker_for(broker).map_err(|_| ApiError::not_found("unknown broker"))?;
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
                .map(Value::from)
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
        connector
            .validate_config(&flat)
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    }
    Ok((Value::Object(out), flat))
}

#[derive(Deserialize)]
struct CreateBody {
    broker: String,
    name: String,
    /// Module ids allowed to use this account, or `["*"]` for all. Default: all.
    modules: Option<Vec<String>>,
    /// Non-secret settings, keyed by the names in the broker's `config_fields`.
    config: Option<serde_json::Map<String, Value>>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("account name is required"));
    }
    brokers::broker_for(&body.broker).map_err(|_| ApiError::not_found("unknown broker"))?;
    let modules = match &body.modules {
        Some(m) => normalize_modules(m)?,
        None => vec![store::ALL_MODULES.to_string()],
    };
    let empty = serde_json::Map::new();
    let (config, _) = normalize_config(&body.broker, body.config.as_ref().unwrap_or(&empty), true)?;
    let row = store::create(&state.pool, &body.broker, name, &modules, &config)
        .await
        .map_err(|_| ApiError::bad_request("an account with this name already exists"))?;
    Ok(Json(json!({ "account": row })))
}

#[derive(Deserialize)]
struct PatchBody {
    name: Option<String>,
    modules: Option<Vec<String>>,
    /// Replaces the stored settings wholesale (absent = leave them alone).
    config: Option<serde_json::Map<String, Value>>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchBody>,
) -> Result<Json<Value>, ApiError> {
    let Some(existing) = store::get(&state.pool, id).await? else {
        return Err(ApiError::not_found("broker account not found"));
    };
    if let Some(cfg) = &body.config {
        let (config, _) = normalize_config(&existing.broker, cfg, false)?;
        store::set_config(&state.pool, id, &config).await?;
    }
    if let Some(name) = &body.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::bad_request("account name is required"));
        }
        store::rename(&state.pool, id, name)
            .await
            .map_err(|_| ApiError::bad_request("an account with this name already exists"))?;
    }
    if let Some(modules) = &body.modules {
        let modules = normalize_modules(modules)?;
        store::set_modules(&state.pool, id, &modules).await?;
    }
    let row = store::get(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true, "account": row })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("broker account not found"));
    }
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
    let account = store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("broker account not found"))?;
    match body.vault_item_id {
        Some(item) => {
            store::set_cred_ref(&state.pool, id, &account.broker, body.name.trim(), item).await?
        }
        None => {
            if body.value.is_empty() {
                return Err(ApiError::bad_request("secret value is required"));
            }
            store::set_cred(
                &state.pool,
                &state.cipher,
                id,
                &account.broker,
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

/// Everything a broker call needs: the account row, its connector, and its settings.
pub(crate) async fn resolve(
    state: &AppState,
    id: Uuid,
    module: Option<&str>,
) -> Result<
    (
        Box<dyn brokers::Broker>,
        std::collections::HashMap<String, String>,
        store::BrokerRow,
    ),
    ApiError,
> {
    let account = store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("broker account not found"))?;
    if let Some(m) = module {
        if !account.allows(m) {
            return Err(ApiError::bad_request(&format!(
                "“{}” is not granted to {m}. Allow it in Settings, Brokers.",
                account.name
            )));
        }
    }
    let connector =
        brokers::broker_for(&account.broker).map_err(|_| ApiError::not_found("unknown broker"))?;
    let settings = store::load_creds(&state.pool, &state.cipher, id).await?;
    Ok((connector, settings, account))
}

/// Reach the broker with this account's own credentials and report what answered. A failed
/// test is an answer, not a server error: it carries the thing to change.
async fn test(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, None).await?;
    if !connector.capability().testable {
        return Err(ApiError::bad_request(&format!(
            "{} has no connection test",
            connector.capability().label
        )));
    }
    let http = brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    match connector.test(&http, &settings).await {
        Ok(detail) => Ok(Json(json!({ "ok": true, "detail": detail }))),
        Err(e) => Ok(Json(json!({ "ok": false, "detail": format!("{e:#}") }))),
    }
}

async fn symbols(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, None).await?;
    let http = brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let symbols = connector
        .symbols(&http, &settings)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "symbols": symbols })))
}

async fn positions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, None).await?;
    let http = brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let positions = connector
        .positions(&http, &settings)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "positions": positions })))
}

async fn orders(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, None).await?;
    let http = brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let orders = connector
        .orders(&http, &settings)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "orders": orders })))
}

/// Positions and working orders in one read, for a module that draws a book rather than
/// listing it (the chart overlay). Only what the broker declares is asked for, and a side
/// that fails comes back as a warning instead of sinking the other: an account that reports
/// orders but whose positions call is refused still puts its orders on the chart.
async fn book(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, q.module.as_deref()).await?;
    let cap = connector.capability();
    let http = brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let mut warnings: Vec<String> = Vec::new();

    let positions = if cap.positions {
        match connector.positions(&http, &settings).await {
            Ok(rows) => rows,
            Err(e) => {
                warnings.push(format!("{e:#}"));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let orders = if cap.orders {
        match connector.orders(&http, &settings).await {
            Ok(rows) => rows,
            Err(e) => {
                warnings.push(format!("{e:#}"));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    Ok(Json(json!({
        "positions": positions,
        "orders": orders,
        "warnings": warnings,
    })))
}

/// What the account owns right now: a balance sheet, not a trade history. The portfolio
/// import reads this; nothing about how a line was acquired is claimed here.
async fn holdings(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, q.module.as_deref()).await?;
    let http = brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let holdings = connector
        .holdings(&http, &settings)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "holdings": holdings })))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn wildcard_collapses_and_ids_are_validated() {
        assert_eq!(ok(&["journal", "*"]), vec!["*".to_string()]);
        assert_eq!(ok(&["journal", "journal"]), vec!["journal".to_string()]);
        assert!(ok(&[]).is_empty());
        // A module that reads no account can't be granted one.
        assert!(normalize_modules(&["editor".to_string()]).is_err());
    }

    #[test]
    fn settings_are_checked_against_what_the_broker_declares() {
        // A key-only broker accepts no settings at all.
        assert!(normalize_config("binance", &cfg(&[("query_id", "1")]), false).is_err());
        assert!(normalize_config("binance", &cfg(&[]), true).is_ok());
        // A Flex account cannot exist without the query it reads.
        assert!(normalize_config("ibkr_flex", &cfg(&[]), true).is_err());
        assert!(normalize_config("ibkr_flex", &cfg(&[("query_id", "abc")]), true).is_err());
        let Ok((stored, flat)) =
            normalize_config("ibkr_flex", &cfg(&[("query_id", " 1234567 "), ("tz_offset", "-300")]), true)
        else {
            panic!("a query id is a complete Flex account");
        };
        assert_eq!(stored["tz_offset"], Value::from(-300));
        assert_eq!(flat["query_id"], "1234567");
    }
}
