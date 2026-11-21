//! HTTP API for the Settings "API Rate" dashboard.
//!
//! Read-only: per-provider outbound-call volume for a window (default today), the published
//! rate limit for each provider where one is documented, and the recent over-limit events.
//! Tracking is observe-and-alert only — nothing here (or anywhere in the tracker) throttles.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{rate, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/rate/usage", get(usage))
}

#[derive(Deserialize)]
struct UsageQuery {
    /// Look-back window in days (1 = today only). Defaults to 1.
    #[serde(default)]
    days: Option<i64>,
}

/// Per-provider usage for the window, each annotated with its published limit (if any), plus
/// the recent over-limit event list.
async fn usage(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Value>, ApiError> {
    let days = q.days.unwrap_or(1).clamp(1, 30);
    let rows = otw_store::api_rate::usage_since(&state.pool, days).await?;
    let events = otw_store::api_rate::recent_events(&state.pool, 50).await?;

    let providers: Vec<Value> = rows
        .into_iter()
        .map(|u| {
            json!({
                "provider": u.provider,
                "host": u.host,
                "requests": u.requests,
                "limited": u.limited,
                "errors": u.errors,
                "last_at": u.last_at.format(&time::format_description::well_known::Rfc3339).ok(),
                "limit": rate::known_limit(&u.provider),
            })
        })
        .collect();

    Ok(Json(json!({
        "days": days,
        "providers": providers,
        "events": events,
    })))
}
