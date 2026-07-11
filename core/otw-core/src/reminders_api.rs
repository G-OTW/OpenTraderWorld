//! HTTP API for the RemindMe module.
//!
//! Reminder CRUD plus the notifications surface: a `since`-based unread poll (drives the
//! slide-in toast bandeau), an unread count (drives the topbar badge), the full list, and
//! acknowledge/delete actions.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::reminders::{self, ReminderInput, FREQUENCIES, KINDS};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/reminders", get(list).post(add))
        .route("/api/reminders/{id}", get(get_one).patch(update).delete(remove))
        .route("/api/notifications", get(list_notifs))
        .route("/api/notifications/unread", get(unread))
        .route("/api/notifications/ack-all", post(ack_all))
        .route("/api/notifications/{id}/read", post(mark_read))
        .route("/api/notifications/{id}", axum::routing::delete(remove_notif))
}

// ── Reminders ────────────────────────────────────────────────────────────────

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let reminders = reminders::list_reminders(&state.pool).await?;
    Ok(Json(json!({ "reminders": reminders })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let r = reminders::get_reminder(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("reminder not found"))?;
    Ok(Json(json!({ "reminder": r })))
}

fn validate(input: &ReminderInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("reminder name required"));
    }
    if !KINDS.contains(&input.kind.as_str()) {
        return Err(ApiError::bad_request("kind must be goal, todo or custom"));
    }
    if !FREQUENCIES.contains(&input.frequency.as_str()) {
        return Err(ApiError::bad_request(
            "frequency must be once, daily, weekly, monthly or yearly",
        ));
    }
    if input.kind != "custom" && input.linked_id.is_none() {
        return Err(ApiError::bad_request("a goal/todo reminder needs a linked item"));
    }
    if let Some(m) = input.max_count {
        if m < 1 {
            return Err(ApiError::bad_request("max_count must be at least 1"));
        }
    }
    Ok(())
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<ReminderInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    let r = reminders::add_reminder(&state.pool, &input).await?;
    Ok(Json(json!({ "reminder": r })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<ReminderInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    if !reminders::update_reminder(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("reminder not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !reminders::delete_reminder(&state.pool, id).await? {
        return Err(ApiError::not_found("reminder not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Notifications ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListNotifQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}
fn default_limit() -> i64 {
    200
}

async fn list_notifs(
    State(state): State<AppState>,
    Query(q): Query<ListNotifQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.clamp(1, 500);
    let notifications = reminders::list_notifications(&state.pool, limit).await?;
    let count = reminders::unread_count(&state.pool).await?;
    Ok(Json(json!({ "notifications": notifications, "unread": count })))
}

#[derive(Deserialize)]
struct UnreadQuery {
    /// RFC3339 timestamp; only notifications created after it are returned.
    since: Option<String>,
}

async fn unread(
    State(state): State<AppState>,
    Query(q): Query<UnreadQuery>,
) -> Result<Json<Value>, ApiError> {
    let since = match q.since.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => Some(
            OffsetDateTime::parse(s, &Rfc3339)
                .map_err(|_| ApiError::bad_request("invalid 'since' timestamp"))?,
        ),
        None => None,
    };
    let notifications = reminders::unread_since(&state.pool, since).await?;
    let count = reminders::unread_count(&state.pool).await?;
    Ok(Json(json!({ "notifications": notifications, "unread": count })))
}

async fn ack_all(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let cleared = reminders::mark_all_read(&state.pool).await?;
    Ok(Json(json!({ "cleared": cleared })))
}

async fn mark_read(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    reminders::mark_read(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove_notif(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !reminders::delete_notification(&state.pool, id).await? {
        return Err(ApiError::not_found("notification not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
