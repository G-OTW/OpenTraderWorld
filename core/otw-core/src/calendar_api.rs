//! HTTP API for the Calendar module — personal-event CRUD.
//!
//! The Economics and Earnings tabs are rendered entirely client-side from embedded
//! investing.com widgets, so they need no backend. Only personal events live here.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::calendar::{self, CalendarEventInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/calendar/events", get(list).post(add))
        .route(
            "/api/calendar/events/{id}",
            get(get_one).patch(update).delete(remove),
        )
}

#[derive(Deserialize)]
struct ListQuery {
    /// RFC3339 window start (inclusive); pairs with `to`.
    from: Option<String>,
    /// RFC3339 window end (exclusive); pairs with `from`.
    to: Option<String>,
}

fn parse_ts(s: Option<String>) -> Option<OffsetDateTime> {
    s.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let events = calendar::list_events(&state.pool, parse_ts(q.from), parse_ts(q.to)).await?;
    Ok(Json(json!({ "events": events })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let event = calendar::get_event(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("event not found"))?;
    Ok(Json(json!({ "event": event })))
}

fn validate(input: &CalendarEventInput) -> Result<(), ApiError> {
    if input.title.trim().is_empty() {
        return Err(ApiError::bad_request("event title required"));
    }
    input
        .start()
        .map_err(|_| ApiError::bad_request("valid start time required"))?;
    Ok(())
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<CalendarEventInput>,
) -> Result<Json<Value>, ApiError> {
    input.title = input.title.trim().to_string();
    validate(&input)?;
    let event = calendar::add_event(&state.pool, &input).await?;
    Ok(Json(json!({ "event": event })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<CalendarEventInput>,
) -> Result<Json<Value>, ApiError> {
    input.title = input.title.trim().to_string();
    validate(&input)?;
    if !calendar::update_event(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("event not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !calendar::delete_event(&state.pool, id).await? {
        return Err(ApiError::not_found("event not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
