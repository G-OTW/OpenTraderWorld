//! HTTP API for the guards on manual trading from the charts (`brokers::chart_guards`).
//!
//! - `GET    /api/brokers/chart-guards`           guards, session settings, each trading account's session, recent firings
//! - `POST   /api/brokers/chart-guards`           add a guard
//! - `PUT    /api/brokers/chart-guards/{id}`      change one
//! - `DELETE /api/brokers/chart-guards/{id}`      remove one
//! - `PUT    /api/brokers/chart-guards/settings`  when a session ends on its own (`day` | `manual`, timezone)
//! - `POST   /api/brokers/chart-guards/reset`     a new session on one account (`{account_id}`) or every one (`{}`)
//! - `POST   /api/brokers/chart-guards/ack`       a firing seen (`{id}`), or every one (`{}`)
//! - `POST   /api/brokers/chart-guards/check`     read the rules now (after a fill on the chart): the new firings
//!
//! Under `/api/brokers`, so demo mode denies it with the rest. Not in the MCP catalog.

use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::brokers::chart_guards::{self as guards, Rule, ACTIONS, MAX_GUARDS};
use crate::{ApiError, AppState};
use otw_store::chart_guards::{self as store, NewGuard, Settings};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/brokers/chart-guards", get(overview).post(create))
        .route("/api/brokers/chart-guards/settings", put(set_settings))
        .route("/api/brokers/chart-guards/reset", post(reset))
        .route("/api/brokers/chart-guards/ack", post(ack))
        .route("/api/brokers/chart-guards/check", post(check))
        .route("/api/brokers/chart-guards/{id}", put(update).delete(remove))
}

fn rfc(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

async fn overview(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let now = OffsetDateTime::now_utc();
    let settings = store::settings(&state.pool).await?;
    let resets: std::collections::HashMap<Uuid, Option<OffsetDateTime>> = store::resets(&state.pool).await?.into_iter().collect();
    let mut sessions = Vec::new();
    let mut earliest = now;
    for a in otw_store::brokers::list(&state.pool).await?.into_iter().filter(|a| a.trades() && a.allows("histviz")) {
        let start = guards::session_start(&settings, resets.get(&a.id).copied().flatten(), now);
        earliest = earliest.min(start);
        let stopped = store::stopped(&state.pool, a.id, start).await?;
        sessions.push(json!({ "account_id": a.id, "name": a.name, "start": rfc(start), "stopped": stopped }));
    }
    let events = store::recent(&state.pool, earliest).await?;
    Ok(Json(json!({
        "guards": store::list(&state.pool).await?,
        "settings": settings,
        "sessions": sessions,
        "events": events,
    })))
}

async fn valid(state: &AppState, g: &NewGuard) -> Result<(), ApiError> {
    let rule = Rule::parse(&g.rule).map_err(|e| ApiError::bad_request(&e))?;
    rule.validate().map_err(|e| ApiError::bad_request(&e))?;
    if !ACTIONS.contains(&g.action.as_str()) {
        return Err(ApiError::bad_request("the action is alert, acknowledge, stop or kill_switch"));
    }
    if g.close_positions && g.action != "stop" {
        return Err(ApiError::bad_request("closing the positions goes with stopping the session"));
    }
    if let Some(id) = g.account_id {
        let a = otw_store::brokers::get(&state.pool, id).await?.ok_or_else(|| ApiError::bad_request("this broker account does not exist"))?;
        if !(a.trades() && a.allows("histviz")) {
            return Err(ApiError::bad_request(&format!("“{}” does not trade from the charts: grant it trading and the charts in Settings, Brokers", a.name)));
        }
    }
    Ok(())
}

async fn create(State(state): State<AppState>, Json(g): Json<NewGuard>) -> Result<Json<Value>, ApiError> {
    valid(&state, &g).await?;
    if store::list(&state.pool).await?.len() >= MAX_GUARDS {
        return Err(ApiError::bad_request(&format!("at most {MAX_GUARDS} guards: remove one first")));
    }
    let guard = store::insert(&state.pool, &g).await?;
    Ok(Json(json!({ "guard": guard })))
}

async fn update(State(state): State<AppState>, Path(id): Path<i64>, Json(g): Json<NewGuard>) -> Result<Json<Value>, ApiError> {
    valid(&state, &g).await?;
    let guard = store::update(&state.pool, id, &g).await?.ok_or_else(|| ApiError::not_found("guard not found"))?;
    Ok(Json(json!({ "guard": guard })))
}

async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Value>, ApiError> {
    if !store::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("guard not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn set_settings(State(state): State<AppState>, Json(s): Json<Settings>) -> Result<Json<Value>, ApiError> {
    if !["day", "manual"].contains(&s.reset.as_str()) {
        return Err(ApiError::bad_request("a session ends each day or at a reset only (day | manual)"));
    }
    if time_tz::timezones::get_by_name(s.timezone.trim()).is_none() {
        return Err(ApiError::bad_request(&format!("{:?} is not a time zone name (Europe/Paris, America/New_York, UTC)", s.timezone)));
    }
    let s = Settings { timezone: s.timezone.trim().to_string(), ..s };
    store::set_settings(&state.pool, &s).await?;
    Ok(Json(json!({ "settings": store::settings(&state.pool).await? })))
}

#[derive(Deserialize)]
struct ResetBody {
    #[serde(default)]
    account_id: Option<Uuid>,
}

async fn reset(State(state): State<AppState>, Json(b): Json<ResetBody>) -> Result<Json<Value>, ApiError> {
    let now = OffsetDateTime::now_utc();
    let accounts: Vec<Uuid> = match b.account_id {
        Some(id) => vec![otw_store::brokers::get(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("broker account not found"))?.id],
        None => otw_store::brokers::list(&state.pool).await?.into_iter().filter(|a| a.trades() && a.allows("histviz")).map(|a| a.id).collect(),
    };
    for id in &accounts {
        store::reset(&state.pool, *id, now).await?;
    }
    crate::security_events::log("chart_guard_reset", &format!("chart session reset on {} account(s)", accounts.len()));
    Ok(Json(json!({ "reset": accounts.len() })))
}

#[derive(Deserialize)]
struct AckBody {
    #[serde(default)]
    id: Option<i64>,
}

async fn ack(State(state): State<AppState>, Json(b): Json<AckBody>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "acknowledged": store::acknowledge(&state.pool, b.id).await? })))
}

async fn check(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let fired = guards::check_now(&state).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "fired": fired })))
}
