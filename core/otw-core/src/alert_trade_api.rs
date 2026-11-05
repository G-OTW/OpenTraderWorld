//! HTTP API for trades on alert ([`crate::alert_trade`]).
//!
//! - `GET    /api/histviz/alert-trades`               every trade on alert, armed or not
//! - `GET/PUT/DELETE /api/histviz/alerts/{id}/trade`  read, write, remove an alert's trade
//! - `PUT    /api/histviz/alerts/{id}/trade/arm`      arm it (the code when the lock is on)
//! - `DELETE /api/histviz/alerts/{id}/trade/arm`      disarm it
//! - `POST   /api/histviz/alerts/{id}/trade/preview`  the order it would send at a price,
//!   the broker's dry run where it has one, and what is already armed on the instrument
//!
//! Arming is the standing authorization a background send needs on top of the account's
//! background trading permission. It is the user's act: refused to an agent or a workflow
//! (`Automated`), refused in demo mode, and it asks for the authenticator code in the same
//! request when the trading lock is on and the account is not paper. A live account also needs
//! live trading on and its limits set, checked again at every send. One arming, one row: there
//! is no bulk arming.
//!
//! None of these routes is in the MCP catalog, on purpose.

use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Extension, Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use otw_connector_model::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::alert_trade::{self, template};
use crate::brokers::{self, dispatch};
use crate::internal_call::Automated;
use crate::{brokers_api, security, ApiError, AppState};
use otw_store::alert_trades as store;
use otw_store::histviz as alerts_store;
use otw_store::User;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/histviz/alert-trades", get(list))
        .route("/api/histviz/alerts/{id}/trade", get(read).put(save).delete(remove))
        .route("/api/histviz/alerts/{id}/trade/arm", put(arm).delete(disarm))
        .route("/api/histviz/alerts/{id}/trade/preview", post(preview))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "trades": store::list(&state.pool).await? })))
}

async fn alert(state: &AppState, id: Uuid) -> Result<alerts_store::Alert, ApiError> {
    alerts_store::get_alert(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("alert not found"))
}

async fn read(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    alert(&state, id).await?;
    Ok(Json(json!({ "trade": store::get(&state.pool, id).await? })))
}

/// The trade as the dialog writes it.
#[derive(Deserialize, Clone)]
struct TradeBody {
    account_id: Uuid,
    /// `enter` | `exit`.
    intent: String,
    #[serde(default)]
    ticket: Value,
    /// `none` | `cancel_symbol` | `close_symbol` | `flatten_account`.
    #[serde(default)]
    fallback: Option<String>,
}

impl TradeBody {
    /// The intent, the template checked on its own, the fallback.
    fn checked(&self) -> Result<(template::Intent, template::TicketTemplate, String), ApiError> {
        let intent = template::Intent::parse(&self.intent).map_err(|m| ApiError::bad_request(&m))?;
        let raw = if self.ticket.is_null() { json!({}) } else { self.ticket.clone() };
        let t: template::TicketTemplate = serde_json::from_value(raw).map_err(|e| ApiError::bad_request(&format!("the ticket cannot be read: {e}")))?;
        let t = t.validate(intent).map_err(|m| ApiError::bad_request(&m))?;
        let fallback = self.fallback.clone().unwrap_or_else(|| "none".into());
        if !template::FALLBACKS.contains(&fallback.as_str()) {
            return Err(ApiError::bad_request("the fallback must be none, cancel_symbol, close_symbol or flatten_account"));
        }
        Ok((intent, t, fallback))
    }
}

/// What the account's broker takes, against the template: an order type, a time in force,
/// exits or reduce-only it does not take are refused now rather than at 3 a.m.
fn check_caps(caps: Option<&otw_connector_model::OrderCaps>, t: &template::TicketTemplate, account: &str) -> Result<(), ApiError> {
    let Some(c) = caps else { return Ok(()) };
    if let Some(ty) = t.order_type {
        if !c.types.is_empty() && !c.types.contains(&ty.order_type()) {
            return Err(ApiError::bad_request(&format!("“{account}” does not take this order type: pick another")));
        }
    }
    if let Some(tif) = t.tif {
        if !c.tif.is_empty() && !c.tif.contains(&tif) {
            return Err(ApiError::bad_request(&format!("“{account}” does not take this time in force: pick another")));
        }
    }
    if (t.take_profit.is_some() || t.stop_loss.is_some()) && !c.brackets {
        return Err(ApiError::bad_request(&format!("“{account}” does not attach a take profit or a stop loss to an order: remove them")));
    }
    if t.reduce_only && !c.reduce_only {
        return Err(ApiError::bad_request(&format!("“{account}” has no reduce-only orders: untick it")));
    }
    Ok(())
}

/// Write the alert's trade. A change to the account, the intent, the ticket or the fallback
/// disarms it. An alert with a trade fires once: its frequency is set so.
async fn save(State(state): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<TradeBody>) -> Result<Json<Value>, ApiError> {
    alert(&state, id).await?;
    let (intent, t, fallback) = b.checked()?;
    let (connector, _, account) = brokers_api::resolve(&state, b.account_id, Some("histviz")).await?;
    if intent == template::Intent::Enter {
        check_caps(connector.order_caps().as_ref(), &t, &account.name)?;
    }
    let ticket = serde_json::to_value(&t).map_err(|e| ApiError::internal(&e.to_string()))?;
    let (row, disarmed) = store::save(&state.pool, id, account.id, &b.intent, &ticket, &fallback).await?;
    store::force_once(&state.pool, id).await?;
    state.live_alerts.poke();
    Ok(Json(json!({ "trade": row, "disarmed": disarmed })))
}

async fn remove(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("this alert has no trade"));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize, Default)]
struct ArmBody {
    /// The authenticator code, when the trading lock is on and the account is not paper.
    #[serde(default)]
    code: String,
}

/// Arm the alert's trade. Behind the account's trading grant and the chart's grant; needs
/// background trading on the account (`background_trading_required` otherwise, which the
/// dialog answers by opening the signature), the code in this request when the trading lock is
/// on and the account is not paper (`code_required`), and on a live account live trading on
/// with its limits set.
async fn arm(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    automated: Option<Extension<Automated>>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    body: Option<Json<ArmBody>>,
) -> Result<Json<Value>, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden_code("demo_disabled", "trades on alert are never armed in the public demo"));
    }
    if automated.is_some() {
        return Err(ApiError::forbidden("a trade on alert is armed by you at the chart, never by an agent or a workflow"));
    }
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let a = alert(&state, id).await?;
    let row = store::get(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("this alert has no trade: save one first"))?;
    let intent = template::Intent::parse(&row.intent).map_err(|m| ApiError::bad_request(&m))?;
    let t: template::TicketTemplate =
        serde_json::from_value(row.ticket.clone()).map_err(|e| ApiError::bad_request(&format!("the ticket cannot be read ({e}): save the trade again")))?;
    t.validate(intent).map_err(|m| ApiError::bad_request(&m))?;
    let (connector, mut settings, account) = brokers_api::resolve_trading(&state, row.account_id, Some("histviz")).await?;
    if account.background_trading_ack.is_none() {
        return Err(ApiError::forbidden_code(
            dispatch::BACKGROUND_REQUIRED,
            &format!("background trading is off for “{}”: sign it to arm a trade on this account", account.name),
        ));
    }
    // An entry opens exposure: the live gate, as at every send. An exit only takes it off.
    if intent == template::Intent::Enter {
        brokers_api::require_live(&state, &account, connector.as_ref(), &mut settings).await?;
    }
    let second_factor = account.broker != brokers::paper::ID && otw_store::trading_lock(&state.pool, user.id).await?;
    if second_factor {
        let Some(key) = security::session_key(&jar) else {
            return Err(ApiError::forbidden("a trade on alert can only be armed from a signed-in browser"));
        };
        if b.code.trim().is_empty() {
            return Err(ApiError::forbidden_code("code_required", "the trading lock is on: enter your authenticator code to arm this trade"));
        }
        brokers_api::check_trading_code(&state, &user, &key, &b.code, "to arm a trade on alert").await?;
    }
    store::force_once(&state.pool, id).await?;
    let armed = store::arm(&state.pool, id, user.id).await?.ok_or_else(|| ApiError::not_found("this alert has no trade"))?;
    state.live_alerts.poke();
    crate::security_events::log(
        "trade_on_alert_armed",
        &format!("trade on alert “{}” ({}) armed on “{}”", crate::histviz_alerts::title_of(&a), a.ticker, account.name),
    );
    Ok(Json(json!({ "trade": armed, "second_factor": second_factor })))
}

/// Disarm the alert's trade. Asks nothing: it is the safe direction.
async fn disarm(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    alert(&state, id).await?;
    let was = store::disarm(&state.pool, id).await?;
    Ok(Json(json!({ "disarmed": was, "trade": store::get(&state.pool, id).await? })))
}

#[derive(Deserialize)]
struct PreviewBody {
    /// The chart's current price, as a decimal string.
    price: Decimal,
    /// When the chart saw it, epoch ms (now when absent).
    #[serde(default)]
    at: Option<i64>,
    /// The trade as the dialog holds it, unsaved; the stored one when absent.
    #[serde(default)]
    trade: Option<TradeBody>,
}

/// The order the trade would send at `price`, or the sentence it would be refused with; the
/// broker's dry run where it has one (audited, behind the trading grant); and what is already
/// armed on the account and instrument at the same price. An exit lists what it would act on.
async fn preview(State(state): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<PreviewBody>) -> Result<Json<Value>, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden_code("demo_disabled", "trades on alert are never previewed in the public demo"));
    }
    if b.price <= Decimal::ZERO {
        return Err(ApiError::bad_request("the chart has no price to preview the order at yet"));
    }
    let a = alert(&state, id).await?;
    let draft = match b.trade.clone() {
        Some(t) => t,
        None => {
            let row = store::get(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("this alert has no trade: save one first"))?;
            TradeBody { account_id: row.account_id, intent: row.intent, ticket: row.ticket, fallback: Some(row.fallback) }
        }
    };
    let (intent, t, _) = draft.checked()?;
    let at_ms = b.at.unwrap_or_else(|| (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64);
    let mut p = match alert_trade::prepare(&state, &a, draft.account_id, &[&t]).await {
        Ok(p) => p,
        Err((code, message)) => return Ok(Json(json!({ "ok": false, "code": code, "message": message }))),
    };
    let exposure = alert_trade::exposure(&state, &a, &mut p, b.price, at_ms).await?;
    if intent == template::Intent::Exit {
        let now = alert_trade::exit_preview(&p, &a).await;
        return Ok(Json(json!({ "ok": true, "intent": "exit", "exit": now, "exposure": exposure, "background": p.account.background_trading_ack.is_some() })));
    }
    check_caps(p.connector.order_caps().as_ref(), &t, &p.account.name)?;
    let key = alert_trade::mint_key();
    let (m, order) = match alert_trade::preview_order(&p, &a, &t, b.price, at_ms, &key) {
        Ok(v) => v,
        Err(message) => {
            return Ok(Json(json!({ "ok": false, "code": "ticket_refused", "message": message, "exposure": exposure })));
        }
    };
    let dry_run = if p.connector.has_dry_run() && p.account.trades() {
        let request = serde_json::to_value(&m.ticket).unwrap_or(Value::Null);
        match brokers_api::audited(&state, &p.account, "dry_run", request, p.connector.dry_run(&p.settings, &m.ticket)).await {
            Ok(detail) => json!({ "ok": true, "detail": detail }),
            Err(e) => json!({ "ok": false, "detail": e.message() }),
        }
    } else {
        Value::Null
    };
    Ok(Json(json!({
        "ok": true,
        "intent": "enter",
        "order": order,
        "qty": m.qty.normalize().to_string(),
        "entry": m.entry.normalize().to_string(),
        "limit": m.limit.map(|v| v.normalize().to_string()),
        "stop": m.stop.map(|v| v.normalize().to_string()),
        "take_profit": m.take_profit.map(|v| v.normalize().to_string()),
        "stop_loss": m.stop_loss.map(|v| v.normalize().to_string()),
        "ticket": m.ticket,
        "dry_run": dry_run,
        "exposure": exposure,
        "background": p.account.background_trading_ack.is_some(),
    })))
}
