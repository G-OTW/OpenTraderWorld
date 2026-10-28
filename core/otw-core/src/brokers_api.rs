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
//! - `GET  /api/brokers/{id}/orders`     working orders
//! - `POST /api/brokers/{id}/orders`     send a ticket (trading accounts only)
//! - `PATCH/DELETE /api/brokers/{id}/orders/{order}`  move or cancel a working order
//! - `POST /api/brokers/{id}/orders/cancel-all`  kill switch: cancel every working order
//! - `POST /api/brokers/{id}/orders/unsent/resend`  send again orders the broker never received
//! - `POST /api/brokers/{id}/orders/unsent/dismiss`  drop them instead
//! - `POST /api/brokers/{id}/positions/close`  close a position at the market
//! - `POST /api/brokers/unlock`          open trading for this session (trading lock)
//! - `POST /api/brokers/{id}/live`       turn live trading on or off for an account
//! - `POST /api/brokers/{id}/dry-run`    check a ticket at the venue without placing it
//! - `GET  /api/brokers/order-log?account=`  the order audit log, newest first
//! - `GET  /api/brokers/order-stats`     per-account order metrics from that log
//! - `GET  /api/brokers/{id}/book`       positions + orders in one read, for a chart overlay
//! - `GET  /api/brokers/events?accounts=`  those books followed live (SSE)
//! - `GET  /api/brokers/{id}/marks?ids=`  live marks of held instruments (trading accounts only)
//! - `GET  /api/brokers/{id}/symbol-for?ticker=&asset_type=`  the account's spelling of a chart's instrument
//! - `GET  /api/brokers/{id}/holdings`   what the account owns, one line per asset
//! - `GET  /api/brokers/{id}/account`    its money: equity, cash, margin, available
//! - `POST /api/brokers/{id}/estimate`   how long a pull would take, before asking for it
//!
//! Same shape as `connectors_api` on purpose, and deliberately a different list: a market
//! data key and an account key are not the same credential and do not deserve the same
//! grants. Everything reads, except the order routes, each of which is written to the
//! append-only audit log (`broker_order_log`) with the broker's answer.
//!
//! The `trading` grant marks an account as one orders may be sent to, and the order routes
//! refuse any account without it. It sits in the module list, is never implied by `*`, and
//! turning it on asks for the password every time (`security::require_password_once`).
//! Turning it off asks nothing: that is the safe direction. The order routes are not in the
//! MCP catalog, and demo mode denies `/api/brokers` wholesale.
//!
//! With the trading lock on (Settings, Security), the routes that can open or grow exposure
//! (place, modify, close) also need trading opened in *this* session with a current
//! authenticator code (`POST /api/brokers/unlock`), once per sign-in for every account.
//! Cancelling never asks: a lock must not stop anyone taking risk off.
//!
//! An account whose orders move real money also needs live trading turned on for it and its
//! risk limits set (`brokers::live`, spec 9); with the trading lock on, that asks for the code.
//! Rating a broker E4 opens no account by itself.
//!
//! A paper account (`brokers::paper`) has neither guard: it is created with the trading
//! grant, turning the grant back on asks nothing, and the lock does not apply. No money is at
//! risk; its order calls are audited like any other.

use std::convert::Infallible;

use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, Sse},
    routing::{delete, get, patch, post},
    Extension, Json, Router,
};
use futures::{stream, Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use axum_extra::extract::cookie::CookieJar;

use crate::brokers;
use crate::{security, ApiError, AppState};
use otw_store::brokers as store;
use otw_store::User;

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
        .route("/api/brokers/{id}/orders", get(orders).post(place_orders))
        .route("/api/brokers/{id}/orders/{order}", patch(modify_order).delete(cancel_order))
        .route("/api/brokers/{id}/orders/cancel-all", post(cancel_all_orders))
        .route("/api/brokers/{id}/orders/unsent/resend", post(resend_unsent))
        .route("/api/brokers/{id}/orders/unsent/dismiss", post(dismiss_unsent))
        .route("/api/brokers/{id}/positions/close", post(close_position))
        .route("/api/brokers/unlock", post(unlock))
        .route("/api/brokers/{id}/live", post(set_live))
        .route("/api/brokers/{id}/dry-run", post(dry_run))
        .route("/api/brokers/order-log", get(order_log))
        .route("/api/brokers/order-stats", get(order_stats))
        .route("/api/brokers/{id}/book", get(book))
        .route("/api/brokers/events", get(events))
        .route("/api/brokers/{id}/marks", get(marks))
        .route("/api/brokers/{id}/fills", get(fills))
        .route("/api/brokers/{id}/symbol-for", get(symbol_for))
        .route("/api/brokers/{id}/holdings", get(holdings))
        .route("/api/brokers/{id}/account", get(account_summary))
        .route("/api/brokers/{id}/estimate", post(estimate))
}

/// Capability matrix only: account-independent facts about each supported broker.
async fn providers() -> Json<Value> {
    Json(json!({ "providers": brokers::capabilities() }))
}

/// Grantable module ids, in display order. Labels are the client's (module registry).
async fn modules() -> Json<Value> {
    Json(json!({ "modules": ACCOUNT_MODULES, "trading": store::TRADING_GRANT }))
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
        let Ok(b) = brokers::broker_for(&a.broker) else {
            continue;
        };
        let (cap, order_caps) = (b.capability(), b.order_caps());
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
            "order_caps": order_caps,
            "live_since": a.live_since.map(|t| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()),
            "live_canary": brokers::live::is_canary(a.id),
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
///
/// The trading grant is not a module: it survives the wildcard's collapse, because `*` says
/// where the account is read and nothing about whether it may be traded.
fn normalize_modules(input: &[String]) -> Result<Vec<String>, ApiError> {
    let trading = input.iter().any(|m| m.trim() == store::TRADING_GRANT);
    let mut out: Vec<String> = Vec::new();
    if input.iter().any(|m| m == store::ALL_MODULES) {
        out.push(store::ALL_MODULES.to_string());
    } else {
        for m in input {
            let m = m.trim();
            if m == store::TRADING_GRANT {
                continue;
            }
            valid_module(m)?;
            if !out.iter().any(|x| x == m) {
                out.push(m.to_string());
            }
        }
    }
    if trading {
        out.push(store::TRADING_GRANT.to_string());
    }
    Ok(out)
}

/// Turning trading on spends a password typed for this one change. Asked before anything
/// is written, so a refused request leaves the account exactly as it was.
fn guard_trading(jar: &CookieJar, before: bool, after: &[String]) -> Result<(), ApiError> {
    if !before && after.iter().any(|m| m == store::TRADING_GRANT) {
        security::require_password_once(jar)?;
    }
    Ok(())
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
    jar: CookieJar,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("account name is required"));
    }
    brokers::broker_for(&body.broker).map_err(|_| ApiError::not_found("unknown broker"))?;
    let paper = body.broker == brokers::paper::ID;
    let modules = match &body.modules {
        Some(m) => normalize_modules(m)?,
        // A paper account is made to be traded: no money is at risk.
        None if paper => vec![store::ALL_MODULES.to_string(), store::TRADING_GRANT.to_string()],
        None => vec![store::ALL_MODULES.to_string()],
    };
    let empty = serde_json::Map::new();
    let (config, _) = normalize_config(&body.broker, body.config.as_ref().unwrap_or(&empty), true)?;
    if !paper {
        guard_trading(&jar, false, &modules)?;
    }
    if store::name_taken(&state.pool, name, None).await? {
        return Err(ApiError::bad_request("an account with this name already exists"));
    }
    let row = store::create(&state.pool, &body.broker, name, &modules, &config).await?;
    if paper {
        brokers::paper::open(&state.pool, &row).await?;
    }
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
    jar: CookieJar,
    Json(body): Json<PatchBody>,
) -> Result<Json<Value>, ApiError> {
    let Some(existing) = store::get(&state.pool, id).await? else {
        return Err(ApiError::not_found("broker account not found"));
    };
    // Validated and password-checked before any write: a refused grant change must not
    // leave a rename or new settings half applied.
    let modules = body.modules.as_deref().map(normalize_modules).transpose()?;
    if let Some(m) = &modules {
        if existing.broker != brokers::paper::ID {
            guard_trading(&jar, existing.trades(), m)?;
        }
    }
    let name = body.name.as_deref().map(str::trim);
    if let Some(name) = name {
        if name.is_empty() {
            return Err(ApiError::bad_request("account name is required"));
        }
        if store::name_taken(&state.pool, name, Some(id)).await? {
            return Err(ApiError::bad_request("an account with this name already exists"));
        }
    }
    let config = body.config.as_ref().map(|cfg| normalize_config(&existing.broker, cfg, false)).transpose()?;
    if let Some((config, _)) = &config {
        store::set_config(&state.pool, id, config).await?;
    }
    if let Some(name) = name {
        store::rename(&state.pool, id, name).await?;
    }
    if let Some(modules) = &modules {
        store::set_modules(&state.pool, id, modules).await?;
        // Live trading rides on trading: off with it, and asked for again when it comes back.
        if !modules.iter().any(|m| m == store::TRADING_GRANT) && existing.live_since.is_some() {
            store::set_live(&state.pool, id, false).await?;
        }
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
        brokers::for_account(&state.pool, &account).map_err(|_| ApiError::not_found("unknown broker"))?;
    let settings = store::load_creds(&state.pool, &state.cipher, id).await?;
    Ok((connector, settings, account))
}

/// [`resolve`] for a background job: no request to answer, so a refusal is an error the
/// job logs and reports, worded the same way.
pub(crate) async fn resolve_for_job(
    pool: &sqlx::PgPool,
    cipher: &otw_store::crypto::SecretCipher,
    id: Uuid,
    module: &str,
) -> anyhow::Result<(
    Box<dyn brokers::Broker>,
    std::collections::HashMap<String, String>,
    store::BrokerRow,
)> {
    let account = store::get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the broker account was deleted"))?;
    anyhow::ensure!(
        account.allows(module),
        "“{}” is not granted to {module}. Allow it in Settings, Brokers.",
        account.name
    );
    let connector = brokers::for_account(pool, &account)
        .map_err(|_| anyhow::anyhow!("unknown broker “{}”", account.broker))?;
    let settings = store::load_creds(pool, cipher, id).await?;
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

/// [`resolve`] for an order route: the account must carry the `trading` grant, and the
/// module asking (the chart) must be granted the account.
async fn resolve_trading(
    state: &AppState,
    id: Uuid,
    module: Option<&str>,
) -> Result<(Box<dyn brokers::Broker>, std::collections::HashMap<String, String>, store::BrokerRow), ApiError> {
    let (connector, settings, account) = resolve(state, id, module).await?;
    if !account.trades() {
        return Err(ApiError::forbidden(&format!(
            "trading is off for “{}”. Turn it on in Settings, Brokers.",
            account.name
        )));
    }
    Ok((connector, settings, account))
}

/// The trading lock's gate on an order route. Off: nothing to check. On: this session must
/// have opened trading with an authenticator code. Refused with `trading_locked`, which the
/// client answers by asking for the code and retrying. No session cookie (an internal
/// dispatch) is never unlocked.
async fn require_unlocked(state: &AppState, user: &User, jar: &CookieJar, account: &store::BrokerRow) -> Result<(), ApiError> {
    // No money at risk on paper.
    if account.broker == brokers::paper::ID || !otw_store::trading_lock(&state.pool, user.id).await? {
        return Ok(());
    }
    if security::session_key(jar).is_some_and(|k| security::TRADING_UNLOCK.holds(&k)) {
        return Ok(());
    }
    Err(ApiError::forbidden_code("trading_locked", "enter your authenticator code to trade"))
}

/// The live gate on a route that opens or changes exposure ([`brokers::live`]): an account
/// that trades real money needs live trading on and its limits set. The live canary's
/// settings are marked here, so its adapter lets it through below E4.
async fn require_live(
    state: &AppState,
    account: &store::BrokerRow,
    connector: &dyn brokers::Broker,
    settings: &mut std::collections::HashMap<String, String>,
) -> Result<(), ApiError> {
    brokers::live::mark_canary(account.id, settings);
    if account.broker == brokers::paper::ID {
        return Ok(());
    }
    let live = connector.is_live(settings).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    if !live {
        return Ok(());
    }
    let limits = otw_store::broker_risk::limits(&state.pool, account.id).await?;
    brokers::live::check(account, live, &limits).map_err(|r| ApiError::forbidden_code(r.code, &r.message))
}

#[derive(Deserialize)]
struct LiveBody {
    enabled: bool,
}

/// Turn live trading on or off for an account (spec 9). On needs the trading grant, an order
/// path rated E4 (or the account named for the live canary) and the account's limits set.
/// With the trading lock on, this session must have opened it with an authenticator code.
/// Off asks nothing.
async fn set_live(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    Json(b): Json<LiveBody>,
) -> Result<Json<Value>, ApiError> {
    let account = store::get(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("broker account not found"))?;
    if account.broker == brokers::paper::ID {
        return Err(ApiError::bad_request("a paper account trades no real money"));
    }
    if !b.enabled {
        store::set_live(&state.pool, id, false).await?;
        crate::security_events::log("trading_live_off", &format!("live trading turned off for “{}”", account.name));
        return Ok(Json(json!({ "ok": true, "account": store::get(&state.pool, id).await? })));
    }
    let connector = brokers::for_account(&state.pool, &account).map_err(|_| ApiError::not_found("unknown broker"))?;
    let limits = otw_store::broker_risk::limits(&state.pool, id).await?;
    brokers::live::can_opt_in(&account, brokers::live::level(connector.as_ref()), brokers::live::is_canary(id), &limits)
        .map_err(|r| ApiError::forbidden_code(r.code, &r.message))?;
    // The trading lock is the user's choice: when on, turning live trading on asks for the code.
    if otw_store::trading_lock(&state.pool, user.id).await?
        && !security::session_key(&jar).is_some_and(|k| security::TRADING_UNLOCK.holds(&k))
    {
        return Err(ApiError::forbidden_code("trading_locked", "enter your authenticator code to turn live trading on"));
    }
    store::set_live(&state.pool, id, true).await?;
    crate::security_events::log("trading_live_on", &format!("live trading turned on for “{}”", account.name));
    Ok(Json(json!({ "ok": true, "account": store::get(&state.pool, id).await? })))
}

#[derive(Deserialize)]
struct UnlockBody {
    code: String,
}

/// Open trading on every account with the trading grant, for this session, with a current
/// authenticator code.
///
/// The code goes through the same check as sign-in, replay guard included, so a code seen
/// over a shoulder cannot be typed again in another browser. Wrong codes count per session:
/// past [`security::TRADING_UNLOCK_MAX_FAILS`] the session is refused for the rest of the
/// window, and the owner is told on the first run of failures.
async fn unlock(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Json(b): Json<UnlockBody>,
) -> Result<Json<Value>, ApiError> {
    let Some(key) = security::session_key(&jar) else {
        return Err(ApiError::forbidden("trading can only be opened from a signed-in browser"));
    };
    if !otw_store::trading_lock(&state.pool, user.id).await? {
        return Ok(Json(json!({ "ok": true })));
    }
    if !user.totp_enabled {
        return Err(ApiError::forbidden(
            "the trading lock needs two-factor authentication, which is not set up: finish \
             setting it up in Settings, Security",
        ));
    }
    if security::TRADING_UNLOCK_FAILS.count(&key) >= security::TRADING_UNLOCK_MAX_FAILS {
        return Err(ApiError::too_many(
            "too many wrong codes from this session. Wait 15 minutes, or sign out and back in.",
        ));
    }
    if !crate::verify_totp(&state, &user, b.code.trim()).await? {
        let n = security::TRADING_UNLOCK_FAILS.record(&key);
        if n >= 3 {
            crate::security_events::alert(
                &state,
                crate::security_events::TRADING_UNLOCK_FAILED,
                crate::security_events::TRADING_UNLOCK_FAILED,
                "Wrong codes to open trading",
                &format!(
                    "Wrong authenticator codes from one session: {n}\n\nIf it was not you, a \
                     signed-in session is in someone else's hands: close it from Settings, \
                     Security, and change your password."
                ),
            );
        } else {
            crate::security_events::log(
                crate::security_events::TRADING_UNLOCK_FAILED,
                &format!("wrong authenticator code to open trading (attempt {n} in this session)"),
            );
        }
        return Err(ApiError::forbidden("that code is not valid"));
    }
    security::TRADING_UNLOCK_FAILS.clear(&key);
    security::TRADING_UNLOCK.grant(&key);
    crate::security_events::log(crate::security_events::TRADING_UNLOCKED, "trading opened for this session");
    Ok(Json(json!({ "ok": true })))
}

/// Run one order call and write it to the audit log (`broker_order_log`) with the broker's
/// answer and how long it took, refused or not. A log write that fails is reported in the
/// server log but never turns a placed order into an error: the order is out either way.
pub(crate) async fn audited<T: serde::Serialize>(
    state: &AppState,
    account: &store::BrokerRow,
    action: &str,
    request: Value,
    call: impl std::future::Future<Output = anyhow::Result<T>>,
) -> Result<T, ApiError> {
    let started = std::time::Instant::now();
    let result = call.await;
    let latency_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let response = match &result {
        Ok(v) => serde_json::to_value(v).unwrap_or(Value::Null),
        Err(e) => json!({ "error": format!("{e:#}") }),
    };
    let entry = otw_store::order_log::NewEntry {
        account_id: account.id,
        account: &account.name,
        broker: &account.broker,
        action,
        request: &request,
        ok: result.is_ok(),
        response: &response,
        latency_ms,
    };
    if let Err(e) = otw_store::order_log::append(&state.pool, &entry).await {
        tracing::error!("broker order log: could not record a {action} on “{}”: {e:#}", account.name);
    }
    result.map_err(|e| {
        // A risk limit or a halt: the chart answers the code (a duplicate asks to confirm).
        if let Some(r) = e.downcast_ref::<brokers::risk::RiskRefusal>() {
            return ApiError::bad_request_code(r.code, &r.message);
        }
        match e.downcast_ref::<brokers::paper::MarginMissing>() {
            // The chart offers to set the margin, or to count the contract at its full value.
            Some(m) => ApiError::bad_request_code("margin_missing", &m.to_string())
                .with_details(json!({ "provider": m.provider, "asset_type": m.asset_type, "ticker": m.ticker })),
            None => ApiError::bad_request(&format!("{e:#}")),
        }
    })
}

/// Send a ticket. Its legs are stored under their client ids before anything leaves
/// (`brokers::reconcile`), so an id is sent once per account across restarts and an answer
/// lost on the way is looked up instead of resent. The adapter checks the ticket whole against
/// the venue's declared order capabilities; the answer pairs each of the chart's keys with the
/// broker's order id.
async fn place_orders(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
    Json(ticket): Json<brokers::OrderTicket>,
) -> Result<Json<Value>, ApiError> {
    let placed = send_ticket(&state, &user, &jar, id, q.module.as_deref(), ticket).await?;
    Ok(Json(json!({ "placed": placed })))
}

/// [`place_orders`]'s path for any ticket: the lock, the live gate, the risk gate, the legs
/// stored before anything leaves, the audit log.
async fn send_ticket(state: &AppState, user: &User, jar: &CookieJar, id: Uuid, module: Option<&str>, ticket: brokers::OrderTicket) -> Result<Vec<brokers::Placed>, ApiError> {
    let (connector, mut settings, account) = resolve_trading(state, id, module).await?;
    require_unlocked(state, user, jar, &account).await?;
    require_live(state, &account, connector.as_ref(), &mut settings).await?;
    let request = serde_json::to_value(&ticket).unwrap_or(Value::Null);
    let keys: Vec<String> = ticket.legs.iter().map(|l| l.key.clone()).collect();
    let call = async {
        brokers::risk::gate(state, &account, connector.as_ref(), &settings, &ticket).await?;
        brokers::reconcile::claim(state, &account, &ticket).await?;
        let out = connector.place(&settings, &ticket).await;
        brokers::reconcile::record(state, &account, &keys, &out).await;
        out
    };
    audited(state, &account, "place", request, call).await
}

#[derive(Deserialize)]
struct UnsentBody {
    client_ids: Vec<String>,
}

/// Send again orders the broker never received: one ticket under new client ids, through
/// every check a new ticket passes. They close as `resent` once it is placed.
async fn resend_unsent(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
    Json(b): Json<UnsentBody>,
) -> Result<Json<Value>, ApiError> {
    let waiting = otw_store::broker_orders::not_received(&state.pool, id).await?;
    let picked: Vec<_> = waiting.into_iter().filter(|u| b.client_ids.contains(&u.client_id)).collect();
    if picked.is_empty() || picked.len() != b.client_ids.len() {
        return Err(ApiError::bad_request("an order to send again is no longer waiting: read the account again"));
    }
    let ticket = brokers::reconcile::resend_ticket(&picked).map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let placed = send_ticket(&state, &user, &jar, id, q.module.as_deref(), ticket).await?;
    otw_store::broker_orders::close_unsent(&state.pool, id, &b.client_ids, "resent").await?;
    Ok(Json(json!({ "placed": placed })))
}

/// Drop orders the broker never received: nothing is sent, they close as `dismissed`.
async fn dismiss_unsent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
    Json(b): Json<UnsentBody>,
) -> Result<Json<Value>, ApiError> {
    let (_, _, account) = resolve_trading(&state, id, q.module.as_deref()).await?;
    let request = json!({ "client_ids": b.client_ids });
    let call = otw_store::broker_orders::close_unsent(&state.pool, id, &b.client_ids, "dismissed");
    let dismissed = audited(&state, &account, "dismiss", request, call).await?;
    Ok(Json(json!({ "dismissed": dismissed })))
}

/// Check a ticket at the venue and place nothing (Binance's test route, IB's what-if): the live
/// dry run before the live canary (spec 9). Behind the trading grant, since it is signed with
/// the trading key, and audited like an order call.
async fn dry_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
    Json(ticket): Json<brokers::OrderTicket>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) = resolve_trading(&state, id, q.module.as_deref()).await?;
    let request = serde_json::to_value(&ticket).unwrap_or(Value::Null);
    let detail = audited(&state, &account, "dry_run", request, connector.dry_run(&settings, &ticket)).await?;
    Ok(Json(json!({ "ok": true, "detail": detail })))
}

/// A change to a working order: any of its prices, its quantity, its time in force (with the
/// expiry, epoch ms, a `gtd` one needs). Numbers arrive as decimal strings.
#[derive(Deserialize, serde::Serialize)]
struct MoveBody {
    #[serde(default)]
    limit: Option<otw_connector_model::Decimal>,
    #[serde(default)]
    stop: Option<otw_connector_model::Decimal>,
    #[serde(default)]
    qty: Option<otw_connector_model::Decimal>,
    #[serde(default)]
    tif: Option<otw_connector_model::TifKind>,
    #[serde(default)]
    expires: Option<i64>,
}

impl MoveBody {
    fn amend(&self) -> Result<otw_connector_model::OrderAmend, ApiError> {
        use otw_connector_model::{OrderAmend, Price, Qty, TifKind, TimeInForce, Timestamp};
        let tif = match self.tif {
            None => None,
            Some(TifKind::Day) => Some(TimeInForce::Day),
            Some(TifKind::Gtc) => Some(TimeInForce::Gtc),
            Some(TifKind::Ioc) => Some(TimeInForce::Ioc),
            Some(TifKind::Fok) => Some(TimeInForce::Fok),
            Some(TifKind::Gtd) => Some(TimeInForce::Gtd(
                self.expires
                    .and_then(Timestamp::from_millis)
                    .ok_or_else(|| ApiError::bad_request("a good-till-date order needs an expiry"))?,
            )),
        };
        Ok(OrderAmend { limit: self.limit.map(Price::new), stop: self.stop.map(Price::new), qty: self.qty.map(Qty::new), tif })
    }
}

/// Change a working order in place. The adapter checks the change against the venue's
/// declared capabilities and waits for the broker to echo it.
async fn modify_order(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Path((id, order)): Path<(Uuid, String)>,
    Query(q): Query<ListQuery>,
    Json(b): Json<MoveBody>,
) -> Result<Json<Value>, ApiError> {
    let change = b.amend()?;
    let (connector, mut settings, account) = resolve_trading(&state, id, q.module.as_deref()).await?;
    require_unlocked(&state, &user, &jar, &account).await?;
    require_live(&state, &account, connector.as_ref(), &mut settings).await?;
    let request = json!({ "order": order, "limit": b.limit, "stop": b.stop, "qty": b.qty, "tif": b.tif, "expires": b.expires });
    let call = async {
        brokers::risk::gate_modify(&state, &account, &change).await?;
        connector.modify(&settings, &order, &change).await
    };
    let moved = audited(&state, &account, "modify", request, call).await?;
    // Changed as a new order: the store follows it under its new ids, and so does the chart.
    if let Some(p) = &moved {
        if let Err(e) = otw_store::broker_orders::replaced(&state.pool, account.id, &order, &p.key, &p.id).await {
            tracing::error!("broker orders: could not follow {order} to {} on “{}”: {e:#}", p.id, account.name);
        }
    }
    Ok(Json(json!({ "ok": true, "replaced_by": moved })))
}

/// Not behind the `trading` grant, like the kill switch: cancelling is the safe direction, and
/// an account whose trading was just turned off may still have an order out.
async fn cancel_order(
    State(state): State<AppState>,
    Path((id, order)): Path<(Uuid, String)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) = resolve(&state, id, q.module.as_deref()).await?;
    audited(&state, &account, "cancel", json!({ "order": order }), connector.cancel(&settings, &order)).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct CloseBody {
    /// The broker's id for the instrument, as the position was reported with it.
    native_id: String,
    #[serde(default)]
    account: String,
    /// The caller's id for the closing order.
    key: String,
}

/// Close a position at the market: an order like any other, so behind the `trading` grant
/// and written to the audit log.
async fn close_position(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
    Json(b): Json<CloseBody>,
) -> Result<Json<Value>, ApiError> {
    if b.native_id.trim().is_empty() || b.key.trim().is_empty() {
        return Err(ApiError::bad_request("name the position to close"));
    }
    let (connector, mut settings, account) = resolve_trading(&state, id, q.module.as_deref()).await?;
    require_unlocked(&state, &user, &jar, &account).await?;
    // Closing takes risk off: no live gate, but the canary's close goes out like its orders.
    brokers::live::mark_canary(account.id, &mut settings);
    let request = json!({ "native_id": b.native_id, "account": b.account, "key": b.key });
    let keys = vec![b.key.clone()];
    let call = async {
        brokers::reconcile::claim_close(&state, &account, &b.native_id, &b.key).await?;
        let out = connector.close_position(&settings, &b.native_id, &b.account, &b.key).await.map(|p| vec![p]);
        brokers::reconcile::record(&state, &account, &keys, &out).await;
        out.map(|mut v| v.remove(0))
    };
    let placed = audited(&state, &account, "close", request, call).await?;
    Ok(Json(json!({ "placed": placed })))
}

/// The kill switch: every working order of the account cancelled at the broker, those placed
/// elsewhere (TWS by hand) included. Not behind the `trading` grant: cancelling is the safe
/// direction, and an account whose trading was just turned off may still have orders out.
async fn cancel_all_orders(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) = resolve(&state, id, None).await?;
    let n = audited(&state, &account, "cancel_all", json!({}), connector.cancel_all(&settings)).await?;
    Ok(Json(json!({ "cancelled": n })))
}

#[derive(Deserialize)]
struct LogQuery {
    account: Option<Uuid>,
    limit: Option<i64>,
}

/// The order audit log, newest first, for one account or every one.
async fn order_log(
    State(state): State<AppState>,
    Query(q): Query<LogQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = otw_store::order_log::list(&state.pool, q.account, q.limit.unwrap_or(200)).await?;
    Ok(Json(json!({ "entries": rows })))
}

/// Per-account order metrics (calls, placed, refused, latency), from the audit log.
async fn order_stats(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "stats": otw_store::order_log::stats(&state.pool).await? })))
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

    // Orders the broker never received, waiting for the user to send them again or drop them.
    let unsent = otw_store::broker_orders::not_received(&state.pool, id).await.unwrap_or_default();

    Ok(Json(json!({
        "positions": positions,
        "orders": orders,
        "unsent": unsent,
        "warnings": warnings,
    })))
}

/// Accounts one events stream may follow: a page follows its trading accounts, a handful.
const EVENTS_MAX: usize = 16;

#[derive(Deserialize)]
struct EventsQuery {
    module: Option<String>,
    /// The accounts to follow, comma separated.
    #[serde(default)]
    accounts: String,
}

/// The books of these accounts followed live: each change to an order or a position as the
/// broker pushes it, so a chart shows a fill when it happens rather than at its next read.
/// One connection for every account, so a page's streams stay within the browser's six per
/// host. Granted like [`book`].
///
/// The first event, `ready`, says which accounts are live (`live`) and why the others are not
/// (`off`: a broker that pushes nothing, an account that failed to resolve); those keep being
/// read on the page's cycle. Then `account` events, each a [`brokers::AccountEvent`] carrying
/// its `account`, and `off` if a broker stops pushing.
async fn events(
    State(state): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let mut ids: Vec<Uuid> = Vec::new();
    for raw in q.accounts.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let id = Uuid::parse_str(raw).map_err(|_| ApiError::bad_request(&format!("{raw:?} is not an account id")))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(ApiError::bad_request("accounts is required"));
    }
    if ids.len() > EVENTS_MAX {
        return Err(ApiError::bad_request(&format!("one stream follows at most {EVENTS_MAX} accounts")));
    }
    let mut live = Vec::new();
    let mut off = serde_json::Map::new();
    let mut streams = Vec::new();
    for id in ids {
        let followed = match resolve(&state, id, q.module.as_deref()).await {
            Ok((connector, settings, _)) => connector.events(&settings).await.map_err(|e| format!("{e:#}")),
            Err(e) => Err(e.message().to_string()),
        };
        match followed {
            Ok(Some(s)) => {
                live.push(id);
                let stopped = stream::once(async move { off_event(id, "the broker stopped pushing this account's changes") });
                // Missed events: the account's orders are reconciled with the broker too.
                let s = s.inspect(move |ev| {
                    if matches!(ev, brokers::AccountEvent::Resync) {
                        brokers::reconcile::poke(id);
                    }
                });
                streams.push(s.map(move |ev| account_event(id, &ev)).chain(stopped).boxed());
            }
            Ok(None) => {
                off.insert(id.to_string(), json!("this broker pushes no changes: its book is read on a cycle"));
            }
            Err(message) => {
                off.insert(id.to_string(), json!(message));
            }
        }
    }
    let ready = Event::default().event("ready").data(json!({ "live": live, "off": off }).to_string());
    // Open until the page leaves, even once every account has stopped: the pings say the
    // server is up, and the page reconnects on its own when it is not.
    let body = stream::once(async move { ready }).chain(stream::select_all(streams)).chain(stream::pending()).map(Ok);
    Ok(Sse::new(body).keep_alive(crate::sse::keep_alive()))
}

fn account_event(account: Uuid, ev: &brokers::AccountEvent) -> Event {
    let mut data = serde_json::to_value(ev).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut data {
        m.insert("account".into(), json!(account));
    }
    Event::default().event("account").data(data.to_string())
}

fn off_event(account: Uuid, message: &str) -> Event {
    Event::default().event("off").data(json!({ "account": account, "message": message }).to_string())
}

#[derive(Deserialize)]
struct MarksQuery {
    module: Option<String>,
    /// The broker's ids of the held instruments, comma separated.
    #[serde(default)]
    ids: String,
}

/// Live marks of the instruments a trading account holds, by the broker's id: what the
/// Positions tab values them at between two book reads. Trading accounts only: a mark line
/// is a market-data subscription held open at the broker, worth it where positions are
/// worked from here. A broker that streams none answers an empty map.
async fn marks(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<MarksQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve_trading(&state, id, q.module.as_deref()).await?;
    let ids: Vec<String> = q.ids.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    if ids.is_empty() {
        return Ok(Json(json!({ "marks": {} })));
    }
    let marks = connector.marks(&settings, &ids).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "marks": marks })))
}

#[derive(Deserialize)]
struct SymbolQuery {
    ticker: String,
    asset_type: String,
}

/// How the account spells the instrument a chart shows, so its book rows land on that chart:
/// `null` when the broker cannot tell, and the chart matches on its own ticker. `step` is the
/// size step its orders must be a multiple of, `null` when the broker does not say;
/// `leverage` and `margin_mode` what the account set on it, where the venue sets them per
/// instrument (Binance USDⓈ-M), `null` otherwise.
async fn symbol_for(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<SymbolQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, None).await?;
    let symbol = connector
        .symbol_for(&settings, &q.ticker, &q.asset_type)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let step = connector.qty_step(&settings, &q.ticker).await.ok().flatten().map(|d| d.normalize().to_string());
    let (leverage, margin_mode) = connector.leverage(&settings, &q.ticker).await.ok().flatten().unwrap_or_default();
    Ok(Json(json!({
        "symbol": symbol,
        "step": step,
        "leverage": leverage.map(|d| d.normalize().to_string()),
        "margin_mode": margin_mode
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

#[derive(Deserialize)]
struct AccountQuery {
    module: Option<String>,
    /// The pair a chart trades: its base and quote assets are answered too, where the broker
    /// holds assets by name.
    symbol: Option<String>,
}

/// The account's money right now (equity, cash, results, margin, what is available), the
/// same figures for every broker that states them. Granted like [`book`].
async fn account_summary(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<AccountQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, q.module.as_deref()).await?;
    let symbol = q.symbol.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let accounts = connector.account(&settings, symbol).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "accounts": accounts })))
}

#[derive(Deserialize)]
struct FillsQuery {
    module: Option<String>,
    ticker: String,
    #[serde(default)]
    asset_type: String,
    #[serde(with = "time::serde::rfc3339")]
    from: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    to: time::OffsetDateTime,
}

/// Furthest back a chart asks for its fills: a venue that pages by the day (Binance) pays one
/// call per day of the window.
const CHART_FILLS_DAYS: i64 = 30;

/// The account's fills on the instrument a chart shows, for its entry and exit markers. The
/// window is the chart's, cut to [`CHART_FILLS_DAYS`] and to how far back the broker answers;
/// a broker that publishes no fills answers an empty list. Only the rows on the chart's
/// instrument come back: the broker's spelling of it, or the chart's ticker up to punctuation
/// (`BTC-USD` is `BTCUSD`), never a lookalike.
async fn fills(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<FillsQuery>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, _) = resolve(&state, id, q.module.as_deref()).await?;
    let cap = connector.capability();
    if !connector.chart_fills() {
        return Ok(Json(json!({ "fills": [] })));
    }
    let mut from = q.from.max(q.to - time::Duration::days(CHART_FILLS_DAYS));
    if cap.history_days > 0 {
        from = from.max(q.to - time::Duration::days(i64::from(cap.history_days)));
    }
    if q.to <= from {
        return Ok(Json(json!({ "fills": [] })));
    }
    let symbol = connector
        .symbol_for(&settings, &q.ticker, &q.asset_type)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| q.ticker.clone());
    let bare = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase();
    let wanted = [bare(&symbol), bare(&q.ticker)];
    let eq = brokers::ExecQuery { from, to: q.to, symbols: vec![symbol] };
    let fills: Vec<brokers::Execution> = brokers::cache::executions(id, connector.as_ref(), &settings, &eq)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?
        .into_iter()
        .filter(|f| wanted.contains(&bare(&f.symbol)))
        .collect();
    Ok(Json(json!({ "fills": fills })))
}

#[derive(Deserialize)]
struct EstimateBody {
    /// executions | holdings
    kind: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    from: Option<time::OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    to: Option<time::OffsetDateTime>,
    #[serde(default)]
    symbols: Vec<String>,
}

/// How long the pull the modal is about to ask for would take, and whether that is long
/// enough to offer the background. A window already pulled a moment ago costs nothing:
/// the import answers it from the last pull.
async fn estimate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<EstimateBody>,
) -> Result<Json<Value>, ApiError> {
    let account = store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("broker account not found"))?;
    let seconds = match b.kind.as_str() {
        "executions" => {
            let (Some(from), Some(to)) = (b.from, b.to) else {
                return Err(ApiError::bad_request("a pull of fills needs a period"));
            };
            let q = brokers::ExecQuery { from, to, symbols: b.symbols };
            if brokers::cache::has_executions(id, &q) {
                0.0
            } else {
                brokers::estimate::executions(&account.broker, from, to, q.symbols.len())
            }
        }
        "holdings" => {
            if brokers::cache::has_holdings(id) {
                0.0
            } else {
                brokers::estimate::holdings(&account.broker)
            }
        }
        _ => return Err(ApiError::bad_request("kind is executions or holdings")),
    };
    Ok(Json(json!({
        "seconds": seconds.round(),
        "long": seconds >= brokers::estimate::LONG_SECS,
    })))
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
    fn trading_is_its_own_grant_and_survives_the_wildcard() {
        assert_eq!(ok(&["trading", "*", "journal"]), vec!["*".to_string(), "trading".to_string()]);
        assert_eq!(ok(&["trading", "journal", "trading"]), vec!["journal".to_string(), "trading".to_string()]);
        // Not a module: no account list can be filtered by it.
        assert!(valid_module("trading").is_err());
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
