//! HTTP API for the Trader Tasks module.
//!
//! One board endpoint returns everything a day needs: the routines due that date (with per-item
//! tick state), the quick tasks, and the trailing consistency strip. Routines/items/tasks have
//! plain CRUD; ticking an item is an idempotent (item, date) upsert/delete.

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::trader_tasks as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/trader/board", get(board))
        .route("/api/trader/routines", get(list_routines).post(create_routine))
        .route(
            "/api/trader/routines/{id}",
            get(routine_detail).patch(update_routine).delete(delete_routine),
        )
        .route("/api/trader/items/{id}/check", post(check_item))
        .route("/api/trader/tasks", post(add_task))
        .route("/api/trader/tasks/{id}", patch(update_task).delete(delete_task))
}

fn parse_date(s: Option<&str>) -> Result<Date, ApiError> {
    match s {
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
        None => Ok(time::OffsetDateTime::now_utc().date()),
    }
}

const SESSIONS: &[&str] = &["pre", "live", "post", "any"];
const PRIORITIES: &[&str] = &["low", "normal", "high"];

// ── Board ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BoardQuery {
    date: Option<String>,
}

async fn board(
    State(state): State<AppState>,
    Query(q): Query<BoardQuery>,
) -> Result<Json<Value>, ApiError> {
    let date = parse_date(q.date.as_deref())?;
    store::seed_if_first_run(&state.pool).await?;
    let routines = store::board_routines(&state.pool, date).await?;
    let tasks = store::board_tasks(&state.pool, date).await?;
    let tick_dates = store::tick_dates(&state.pool, date, 14).await?;
    let dates: Vec<String> = tick_dates
        .iter()
        .filter_map(|d| d.format(&Iso8601::DATE).ok())
        .collect();
    Ok(Json(json!({
        "date": date.format(&Iso8601::DATE).ok(),
        "routines": routines,
        "tasks": tasks,
        "tick_dates": dates
    })))
}

// ── Routines ──────────────────────────────────────────────────────────────────

async fn list_routines(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let routines = store::list_routines(&state.pool).await?;
    Ok(Json(json!({ "routines": routines })))
}

async fn routine_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let routine = store::get_routine(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("routine not found"))?;
    let items = store::list_items(&state.pool, id).await?;
    Ok(Json(json!({ "routine": routine, "items": items })))
}

#[derive(Deserialize)]
struct CreateRoutineBody {
    name: String,
    #[serde(default = "default_session")]
    session: String,
    #[serde(default = "default_weekdays")]
    weekdays: i32,
    #[serde(default)]
    items: Vec<String>,
}
fn default_session() -> String {
    "pre".into()
}
fn default_weekdays() -> i32 {
    31 // Mon–Fri
}

fn validate_routine(name: &str, session: &str, weekdays: i32) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if !SESSIONS.contains(&session) {
        return Err(ApiError::bad_request("session must be pre|live|post|any"));
    }
    if !(1..=127).contains(&weekdays) {
        return Err(ApiError::bad_request("weekdays mask must be 1..127"));
    }
    Ok(())
}

async fn create_routine(
    State(state): State<AppState>,
    Json(b): Json<CreateRoutineBody>,
) -> Result<Json<Value>, ApiError> {
    validate_routine(&b.name, &b.session, b.weekdays)?;
    let items: Vec<String> = b
        .items
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let routine =
        store::create_routine(&state.pool, b.name.trim(), &b.session, b.weekdays, &items).await?;
    Ok(Json(json!({ "routine": routine })))
}

#[derive(Deserialize)]
struct ItemPatch {
    id: Option<Uuid>,
    label: String,
}

#[derive(Deserialize)]
struct UpdateRoutineBody {
    name: Option<String>,
    session: Option<String>,
    weekdays: Option<i32>,
    active: Option<bool>,
    /// Full replacement list when present; entries keep their id to preserve tick history.
    items: Option<Vec<ItemPatch>>,
}

async fn update_routine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateRoutineBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(s) = &b.session {
        if !SESSIONS.contains(&s.as_str()) {
            return Err(ApiError::bad_request("session must be pre|live|post|any"));
        }
    }
    if let Some(w) = b.weekdays {
        if !(1..=127).contains(&w) {
            return Err(ApiError::bad_request("weekdays mask must be 1..127"));
        }
    }
    let routine = store::update_routine(
        &state.pool,
        id,
        b.name.as_deref().map(str::trim),
        b.session.as_deref(),
        b.weekdays,
        b.active,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("routine not found"))?;
    if let Some(items) = b.items {
        let cleaned: Vec<(Option<Uuid>, String)> = items
            .into_iter()
            .map(|i| (i.id, i.label.trim().to_string()))
            .filter(|(_, l)| !l.is_empty())
            .collect();
        store::set_items(&state.pool, id, &cleaned).await?;
    }
    Ok(Json(json!({ "routine": routine })))
}

async fn delete_routine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_routine(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── Item ticks ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckBody {
    date: Option<String>,
    checked: bool,
}

async fn check_item(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<CheckBody>,
) -> Result<Json<Value>, ApiError> {
    let date = parse_date(b.date.as_deref())?;
    store::set_check(&state.pool, id, date, b.checked).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── Quick tasks ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddTaskBody {
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default = "default_priority")]
    priority: String,
    due_date: Option<String>,
}
fn default_priority() -> String {
    "normal".into()
}

async fn add_task(
    State(state): State<AppState>,
    Json(b): Json<AddTaskBody>,
) -> Result<Json<Value>, ApiError> {
    if b.title.trim().is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }
    if !PRIORITIES.contains(&b.priority.as_str()) {
        return Err(ApiError::bad_request("priority must be low|normal|high"));
    }
    let due = match b.due_date.as_deref() {
        Some("") | None => None,
        Some(s) => Some(parse_date(Some(s))?),
    };
    let task = store::add_task(&state.pool, b.title.trim(), b.note.trim(), &b.priority, due).await?;
    Ok(Json(json!({ "task": task })))
}

#[derive(Deserialize)]
struct UpdateTaskBody {
    title: Option<String>,
    note: Option<String>,
    priority: Option<String>,
    /// Present-and-null clears the due date; absent leaves it unchanged.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    due_date: Option<Option<String>>,
    done: Option<bool>,
}

/// Distinguishes an absent JSON field (None) from an explicit null (Some(None)).
fn deserialize_double_option<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateTaskBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(p) = &b.priority {
        if !PRIORITIES.contains(&p.as_str()) {
            return Err(ApiError::bad_request("priority must be low|normal|high"));
        }
    }
    let due: Option<Option<Date>> = match &b.due_date {
        None => None,
        Some(None) => Some(None),
        Some(Some(s)) if s.is_empty() => Some(None),
        Some(Some(s)) => Some(Some(parse_date(Some(s))?)),
    };
    let task = store::update_task(
        &state.pool,
        id,
        b.title.as_deref().map(str::trim),
        b.note.as_deref().map(str::trim),
        b.priority.as_deref(),
        due,
        b.done,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("task not found"))?;
    Ok(Json(json!({ "task": task })))
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_task(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}
