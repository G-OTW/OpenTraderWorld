//! HTTP API for the Time Tracker module.
//!
//! Projects CRUD + reordering, server-side timer start/stop, entries, a heartbeat the
//! client pings to record "last seen" (for the revert-on-reopen flow), a breakdown by
//! day/week/month, and the display-currency setting.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::time_tracker::{self, BreakdownQuery, ProjectInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/time/projects", get(list_projects).post(add_project))
        .route(
            "/api/time/projects/{id}",
            get(get_project).patch(update_project).delete(delete_project),
        )
        .route("/api/time/projects/{id}/position", post(set_position))
        .route("/api/time/projects/{id}/start", post(start))
        .route("/api/time/projects/{id}/stop", post(stop))
        .route(
            "/api/time/projects/{id}/entries",
            get(list_entries).post(create_entry),
        )
        .route("/api/time/entries/{id}", axum::routing::delete(delete_entry))
        .route("/api/time/state", get(get_state))
        .route("/api/time/heartbeat", post(heartbeat))
        .route("/api/time/revert", post(revert))
        .route("/api/time/settings", axum::routing::patch(update_settings))
        .route("/api/time/breakdown", get(breakdown))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    include_archived: bool,
}

async fn list_projects(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let projects = time_tracker::list_projects(&state.pool, q.include_archived).await?;
    Ok(Json(json!({ "projects": projects })))
}

async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let projects = time_tracker::list_projects(&state.pool, true).await?;
    let p = projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| ApiError::not_found("project not found"))?;
    Ok(Json(json!({ "project": p })))
}

fn validate(input: &ProjectInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("project name required"));
    }
    if !CURRENCIES.contains(&input.rate_currency.as_str()) {
        return Err(ApiError::bad_request("unsupported currency"));
    }
    if let Some(b) = input.time_budget_hours {
        if !(b.is_finite() && b >= 0.0) {
            return Err(ApiError::bad_request("time budget must be a non-negative number"));
        }
    }
    if let Some(r) = input.hourly_rate {
        if !(r.is_finite() && r >= 0.0) {
            return Err(ApiError::bad_request("hourly rate must be a non-negative number"));
        }
    }
    Ok(())
}

async fn add_project(
    State(state): State<AppState>,
    Json(mut input): Json<ProjectInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    let id = time_tracker::add_project(&state.pool, &input).await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<ProjectInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    if !time_tracker::update_project(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("project not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !time_tracker::delete_project(&state.pool, id).await? {
        return Err(ApiError::not_found("project not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PositionBody {
    position: f64,
}

async fn set_position(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PositionBody>,
) -> Result<Json<Value>, ApiError> {
    time_tracker::set_position(&state.pool, id, body.position).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn start(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    time_tracker::start_timer(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn stop(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    time_tracker::stop_timer(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct EntriesQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}
fn default_limit() -> i64 {
    50
}

async fn list_entries(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<EntriesQuery>,
) -> Result<Json<Value>, ApiError> {
    let entries = time_tracker::list_entries(&state.pool, id, q.limit.clamp(1, 500)).await?;
    Ok(Json(json!({ "entries": entries })))
}

async fn delete_entry(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !time_tracker::delete_entry(&state.pool, id).await? {
        return Err(ApiError::not_found("entry not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Manually add a time range to a project. Provide `started_at` (RFC3339) plus either an
/// explicit `ended_at` (RFC3339) or a positive `duration_seconds` (end = start + duration).
#[derive(Deserialize)]
struct CreateEntryBody {
    started_at: String,
    ended_at: Option<String>,
    duration_seconds: Option<f64>,
    note: Option<String>,
}

async fn create_entry(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateEntryBody>,
) -> Result<Json<Value>, ApiError> {
    use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

    if !time_tracker::project_exists(&state.pool, project_id).await? {
        return Err(ApiError::not_found("project not found"));
    }

    let started = OffsetDateTime::parse(body.started_at.trim(), &Rfc3339)
        .map_err(|_| ApiError::bad_request("invalid start time"))?;

    let ended = match (body.ended_at.as_deref(), body.duration_seconds) {
        (Some(s), _) if !s.trim().is_empty() => OffsetDateTime::parse(s.trim(), &Rfc3339)
            .map_err(|_| ApiError::bad_request("invalid end time"))?,
        (_, Some(secs)) => {
            if !(secs.is_finite() && secs > 0.0) {
                return Err(ApiError::bad_request("duration must be a positive number"));
            }
            started + Duration::seconds_f64(secs)
        }
        _ => return Err(ApiError::bad_request("provide an end time or a duration")),
    };

    if ended <= started {
        return Err(ApiError::bad_request("end must be after start"));
    }
    let note = body.note.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let id = time_tracker::create_entry(&state.pool, project_id, started, ended, note).await?;
    Ok(Json(json!({ "id": id })))
}

async fn get_state(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let s = time_tracker::get_state(&state.pool).await?;
    Ok(Json(json!({ "state": s })))
}

async fn heartbeat(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    time_tracker::heartbeat(&state.pool).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn revert(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let n = time_tracker::revert_running_to_last_seen(&state.pool).await?;
    Ok(Json(json!({ "reverted": n })))
}

#[derive(Deserialize)]
struct SettingsBody {
    display_currency: Option<String>,
}

async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(c) = body.display_currency {
        if !CURRENCIES.contains(&c.as_str()) {
            return Err(ApiError::bad_request("unsupported currency"));
        }
        time_tracker::set_display_currency(&state.pool, &c).await?;
    }
    let s = time_tracker::get_state(&state.pool).await?;
    Ok(Json(json!({ "state": s })))
}

async fn breakdown(
    State(state): State<AppState>,
    Query(q): Query<BreakdownQuery>,
) -> Result<Json<Value>, ApiError> {
    let s = time_tracker::get_state(&state.pool).await?;
    let data = time_tracker::breakdown(&state.pool, &q, &s.display_currency).await?;
    Ok(Json(json!({ "breakdown": data })))
}
