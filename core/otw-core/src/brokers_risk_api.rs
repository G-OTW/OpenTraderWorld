//! HTTP API for the user's risk limits, the halt policy and the global kill switch
//! (`brokers::risk`).
//!
//! - `GET  /api/brokers/risk`               policy, halts in force, every account's limits
//! - `PUT  /api/brokers/risk/policy`        how halts end (hour | day | week | manual, password lift)
//! - `PUT  /api/brokers/{id}/risk`          one account's limits
//! - `POST /api/brokers/kill-switch`        cancel every working order on every trading account, then halt them all
//! - `POST /api/brokers/halts/lift`         lift one halt (`{id}`) or every one (`{}`)
//!
//! Changing limits or the policy and lifting a halt ask for the password (`reauth_once_required`,
//! answered by the client's password prompt): each of them can open more risk. The kill switch
//! asks nothing, like cancelling: it only takes risk off. Demo mode denies `/api/brokers`
//! wholesale, and none of this is in the MCP catalog.

use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::brokers::risk;
use crate::{brokers_api, security, ApiError, AppState};
use otw_store::broker_risk::{self as store, Limits, Policy};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/brokers/risk", get(overview))
        .route("/api/brokers/risk/policy", put(set_policy))
        .route("/api/brokers/{id}/risk", put(set_limits))
        .route("/api/brokers/kill-switch", post(kill_switch))
        .route("/api/brokers/halts/lift", post(lift))
}

async fn overview(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let now = OffsetDateTime::now_utc();
    let policy = store::policy(&state.pool).await?;
    let halts = store::halts(&state.pool, None, now).await?;
    let limits: serde_json::Map<String, Value> = store::all_limits(&state.pool)
        .await?
        .into_iter()
        .map(|(id, l)| (id.to_string(), serde_json::to_value(l).unwrap_or(Value::Null)))
        .collect();
    let (start, next) = risk::period(&policy.reset, &policy.timezone, now);
    Ok(Json(json!({
        "policy": policy,
        "halts": halts,
        "limits": limits,
        "period": { "start": rfc(start), "next": rfc(next) },
    })))
}

fn rfc(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

async fn set_policy(State(state): State<AppState>, jar: CookieJar, Json(p): Json<Policy>) -> Result<Json<Value>, ApiError> {
    if !["hour", "day", "week", "manual"].contains(&p.reset.as_str()) {
        return Err(ApiError::bad_request("reset is hour, day, week or manual"));
    }
    if time_tz::timezones::get_by_name(p.timezone.trim()).is_none() {
        return Err(ApiError::bad_request(&format!("{:?} is not a time zone name (Europe/Paris, America/New_York, UTC)", p.timezone)));
    }
    if p.reset == "manual" && !p.password_lift {
        return Err(ApiError::bad_request("a halt that never ends on its own must be lifted with the password: keep password lifting on"));
    }
    security::require_password_once(&jar)?;
    let p = Policy { timezone: p.timezone.trim().to_string(), ..p };
    store::set_policy(&state.pool, &p).await?;
    Ok(Json(json!({ "policy": p })))
}

async fn set_limits(State(state): State<AppState>, jar: CookieJar, Path(id): Path<Uuid>, Json(l): Json<Limits>) -> Result<Json<Value>, ApiError> {
    let (_, _, account) = brokers_api::resolve(&state, id, None).await?;
    risk::validate(&l).map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    security::require_password_once(&jar)?;
    store::set_limits(&state.pool, account.id, &l).await?;
    Ok(Json(json!({ "limits": l })))
}

/// Every working order on every trading account cancelled at its broker (orders placed
/// elsewhere included), each cancel written to its audit log; then every account halted. An
/// account whose broker could not be reached is named, and the halt holds for it too.
async fn kill_switch(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let policy = store::policy(&state.pool).await?;
    let halt = store::add_halt(&state.pool, None, "kill_switch", "", risk::halt_until(&policy, OffsetDateTime::now_utc())).await?;
    let mut results = serde_json::Map::new();
    for account in otw_store::brokers::list(&state.pool).await?.into_iter().filter(|a| a.trades()) {
        let id = account.id;
        let out = match brokers_api::resolve(&state, id, None).await {
            Ok((connector, settings, account)) => {
                match brokers_api::audited(&state, &account, "cancel_all", json!({ "kill_switch": true }), connector.cancel_all(&settings)).await {
                    Ok(n) => json!({ "name": account.name, "cancelled": n }),
                    Err(e) => json!({ "name": account.name, "error": e.message() }),
                }
            }
            Err(e) => json!({ "error": e.message() }),
        };
        results.insert(id.to_string(), out);
    }
    crate::security_events::log("trading_kill_switch", &format!("kill switch: {} account(s)", results.len()));
    Ok(Json(json!({ "halt": halt, "accounts": results })))
}

#[derive(Deserialize)]
struct LiftBody {
    #[serde(default)]
    id: Option<i64>,
}

/// Lift a halt before it ends on its own, with the password, when the policy allows it. A
/// manual policy always does: the password is then the only way out.
async fn lift(State(state): State<AppState>, jar: CookieJar, Json(b): Json<LiftBody>) -> Result<Json<Value>, ApiError> {
    let policy = store::policy(&state.pool).await?;
    let now = OffsetDateTime::now_utc();
    let halts = store::halts(&state.pool, None, now).await?;
    let wanted: Vec<_> = halts.iter().filter(|h| b.id.is_none_or(|id| h.id == id)).collect();
    if wanted.is_empty() {
        return Err(ApiError::bad_request("no halt in force to lift"));
    }
    // A drift is acknowledged, not waited out: the password always lifts it.
    let acknowledging = wanted.iter().all(|h| h.reason == crate::brokers::reconcile::DRIFT);
    if !policy.password_lift && policy.reset != "manual" && !acknowledging {
        let until = wanted.iter().filter_map(|h| h.until).max().map(rfc).unwrap_or_default();
        return Err(ApiError::bad_request(&format!(
            "halts end on their own ({until}) and lifting them sooner is off: turn on lifting with the password in Settings, Brokers"
        )));
    }
    security::require_password_once(&jar)?;
    let mut n = 0;
    for h in wanted {
        n += store::lift(&state.pool, Some(h.id)).await?;
    }
    crate::security_events::log("trading_halt_lifted", &format!("{n} trading halt(s) lifted"));
    Ok(Json(json!({ "lifted": n })))
}
