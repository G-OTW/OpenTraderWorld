//! Exchanges (Settings → Exchanges, and the top-bar badge).
//!
//! - `GET  /api/exchanges`       every venue: hours, status now, next holidays, calendar info
//! - `GET  /api/exchanges/home`  the venue picked in Settings → Defaults, with its status
//! - `POST /api/exchanges/sync`  look for a newer published calendar now

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use time::OffsetDateTime;
use time_tz::OffsetDateTimeExt;

use crate::automator::schedule;
use crate::exchanges::{self, Exchange};
use crate::{ApiError, AppState};

/// Settings key of the venue shown in the top bar (a MIC, empty = none).
pub const HOME_KEY: &str = "home_exchange";
/// Holidays and half days listed per venue.
const UPCOMING: usize = 6;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/exchanges", get(list))
        .route("/api/exchanges/home", get(home))
        .route("/api/exchanges/sync", post(sync))
}

fn row(e: &Exchange, now: OffsetDateTime) -> Value {
    let today = schedule::timezone(&e.timezone)
        .map(|tz| now.to_timezone(tz).date())
        .unwrap_or(now.date());
    json!({
        "mic": e.mic,
        "code": e.code,
        "name": e.name,
        "city": e.city,
        "timezone": e.timezone,
        "open": e.open,
        "close": e.close,
        "break_start": e.break_start,
        "break_end": e.break_end,
        "prior_day_open": e.prior_day_open,
        "weekdays": e.weekdays,
        "covered_to": e.covered_to,
        "status": e.status(now),
        "upcoming": e.upcoming(today, UPCOMING),
    })
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let cal = state.exchanges.current();
    let now = OffsetDateTime::now_utc();
    let home = otw_store::settings::get_or(&state.pool, HOME_KEY, "").await?;
    Ok(Json(json!({
        "generated": cal.generated,
        "source": cal.source,
        "checked_at": exchanges::checked_at(&state.pool).await,
        "home": home,
        "exchanges": cal.exchanges.iter().map(|e| row(e, now)).collect::<Vec<_>>(),
    })))
}

async fn home(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mic = otw_store::settings::get_or(&state.pool, HOME_KEY, "").await?;
    let cal = state.exchanges.current();
    let Some(e) = cal.find(&mic) else {
        return Ok(Json(json!({ "mic": null })));
    };
    let s = e.status(OffsetDateTime::now_utc());
    Ok(Json(json!({
        "mic": e.mic,
        "code": e.code,
        "name": e.name,
        "open": s.open,
        "next_change": s.next_change.and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
        "holidays_unknown": s.holidays_unknown,
    })))
}

async fn sync(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let updated = exchanges::sync(&state.pool, &state.http, &state.exchanges)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "updated": updated, "generated": state.exchanges.current().generated })))
}
