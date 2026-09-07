//! HTTP API for the Trader Tasks module.
//!
//! Two faces of the same data. The **board** (`/api/trader/board`) is the day view: the
//! templates due that date with per-item tick state, the quick tasks, and the trailing
//! consistency strip. The **templates** endpoints are the authoring side: categories plus
//! full CRUD on a routine's name, description, notes, recurrence and checklist.
//!
//! Rich text (a template's `notes`, an item's `note`) is sanitised here, at the edge, so
//! nothing dangerous ever reaches the database — same rule as the mailbox reader.

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::trader_tasks::{self as store, NewItem, NewRoutine, RoutinePatch, Schedule};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/trader/board", get(board))
        .route("/api/trader/routines", get(list_routines).post(create_routine))
        .route(
            "/api/trader/routines/{id}",
            get(routine_detail).patch(update_routine).delete(delete_routine),
        )
        .route("/api/trader/routines/{id}/duplicate", post(duplicate_routine))
        .route("/api/trader/routines/reorder", post(reorder_routines))
        .route("/api/trader/categories", get(list_categories).post(create_category))
        .route(
            "/api/trader/categories/{id}",
            patch(update_category).delete(delete_category),
        )
        .route("/api/trader/marks", get(list_marks).put(set_mark))
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

/// `""` and absent both mean "no date"; anything else must parse.
fn parse_opt_date(s: Option<&str>) -> Result<Option<Date>, ApiError> {
    match s {
        None | Some("") => Ok(None),
        Some(s) => Ok(Some(parse_date(Some(s))?)),
    }
}

const SESSIONS: &[&str] = &["pre", "live", "post", "any"];
const PRIORITIES: &[&str] = &["low", "normal", "high"];

// ── Rich text ─────────────────────────────────────────────────────────────────

/// Template/item notes are small formatted blocks, not documents: text marks, lists,
/// headings and links. No images, no tables, no media — a checklist annotation that needs
/// those belongs in the editor module, linked from the item.
fn note_cleaner() -> &'static ammonia::Builder<'static> {
    static CLEANER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    CLEANER.get_or_init(|| {
        let mut b = ammonia::Builder::empty();
        b.tags(
            [
                "p", "br", "strong", "b", "em", "i", "u", "s", "code", "pre", "blockquote", "ul",
                "ol", "li", "h1", "h2", "h3", "a", "span",
            ]
            .into_iter()
            .collect(),
        )
        .add_tag_attributes("a", ["href", "title"])
        .link_rel(Some("noopener noreferrer nofollow"))
        .set_tag_attribute_value("a", "target", "_blank")
        .url_schemes(["http", "https", "mailto"].into_iter().collect());
        b
    })
}

fn clean_note(html: &str) -> String {
    note_cleaner().clean(html).to_string()
}

/// Item links are plain URLs, not markup: only http(s) survives, so a stored `javascript:`
/// can never become an href on the board.
fn clean_url(url: &str) -> Result<String, ApiError> {
    let u = url.trim();
    if u.is_empty() {
        return Ok(String::new());
    }
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Err(ApiError::bad_request("link must be an http(s) URL"));
    }
    Ok(u.to_string())
}

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
    let categories = store::list_categories(&state.pool).await?;
    let tick_dates = store::tick_dates(&state.pool, date, 14).await?;
    let dates: Vec<String> = tick_dates
        .iter()
        .filter_map(|d| d.format(&Iso8601::DATE).ok())
        .collect();
    // A year of marks rides along: it feeds the week/month/year counter and the calendar
    // without a second round trip on load.
    let marks = store::list_marks(&state.pool, date - time::Duration::days(365), date).await?;
    let mark = store::get_mark(&state.pool, date).await?;
    Ok(Json(json!({
        "date": date.format(&Iso8601::DATE).ok(),
        "routines": routines,
        "categories": categories,
        "tasks": tasks,
        "tick_dates": dates,
        "marks": marks,
        "mark": mark
    })))
}

// ── Categories ────────────────────────────────────────────────────────────────

async fn list_categories(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::seed_if_first_run(&state.pool).await?;
    let categories = store::list_categories(&state.pool).await?;
    Ok(Json(json!({ "categories": categories })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CategoryBody {
    name: String,
    #[serde(default)]
    color: String,
}

async fn create_category(
    State(state): State<AppState>,
    Json(b): Json<CategoryBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let category = store::create_category(&state.pool, b.name.trim(), b.color.trim()).await?;
    Ok(Json(json!({ "category": category })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CategoryPatch {
    name: Option<String>,
    color: Option<String>,
    position: Option<f64>,
}

async fn update_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<CategoryPatch>,
) -> Result<Json<Value>, ApiError> {
    if b.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ApiError::bad_request("name cannot be empty"));
    }
    let category = store::update_category(
        &state.pool,
        id,
        b.name.as_deref().map(str::trim),
        b.color.as_deref().map(str::trim),
        b.position,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("category not found"))?;
    Ok(Json(json!({ "category": category })))
}

async fn delete_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_category(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── Routines (templates) ──────────────────────────────────────────────────────

async fn list_routines(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::seed_if_first_run(&state.pool).await?;
    let routines = store::list_routines(&state.pool).await?;
    let categories = store::list_categories(&state.pool).await?;
    // Item counts, so the template list can show "7 steps" without an N+1 of detail calls.
    let mut with_items = Vec::with_capacity(routines.len());
    for r in routines {
        let items = store::list_items(&state.pool, r.id).await?;
        with_items.push(json!({ "routine": r, "items": items }));
    }
    Ok(Json(json!({ "routines": with_items, "categories": categories })))
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

/// A checklist line as the template form sends it. `label` is required; the rest is optional
/// enrichment (a rich note unfolded on the board, and one link).
#[derive(Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct ItemBody {
    #[serde(default)]
    id: Option<Uuid>,
    label: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    link_label: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateRoutineBody {
    name: String,
    #[serde(default)]
    description: String,
    /// Rich text (HTML) — sanitised before it is stored.
    #[serde(default)]
    notes: String,
    #[serde(default = "default_session")]
    session: String,
    #[serde(default)]
    category_id: Option<Uuid>,
    /// Recurrence rule; omitted means the legacy `weekdays` mask.
    #[serde(default)]
    schedule: Option<Schedule>,
    #[serde(default = "default_weekdays")]
    weekdays: i32,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    items: Vec<ItemBody>,
}
fn default_session() -> String {
    "pre".into()
}
fn default_weekdays() -> i32 {
    31 // Mon–Fri
}

fn validate_session(session: &str) -> Result<(), ApiError> {
    if !SESSIONS.contains(&session) {
        return Err(ApiError::bad_request("session must be pre|live|post|any"));
    }
    Ok(())
}

/// Reject rules that can never fire (an empty weekday mask, a day-of-month list with nothing
/// in it) — they would silently produce a template that is never due.
fn validate_schedule(s: &Schedule) -> Result<(), ApiError> {
    match s {
        Schedule::Weekly { weekdays, interval } => {
            if !(1..=127).contains(weekdays) {
                return Err(ApiError::bad_request("weekdays mask must be 1..127"));
            }
            if !(1..=52).contains(interval) {
                return Err(ApiError::bad_request("weekly interval must be 1..52"));
            }
        }
        Schedule::Daily { interval } => {
            if !(1..=365).contains(interval) {
                return Err(ApiError::bad_request("daily interval must be 1..365"));
            }
        }
        Schedule::Monthly { days } => {
            if days.is_empty() {
                return Err(ApiError::bad_request("pick at least one day of the month"));
            }
            if days.iter().any(|d| *d != -1 && !(1..=31).contains(d)) {
                return Err(ApiError::bad_request("month days must be 1..31 or -1 (last)"));
            }
        }
        Schedule::NthWeekday { weekday, nth } => {
            if !(0..=6).contains(weekday) {
                return Err(ApiError::bad_request("weekday must be 0 (Mon) .. 6 (Sun)"));
            }
            if *nth != -1 && !(1..=5).contains(nth) {
                return Err(ApiError::bad_request("nth must be 1..5 or -1 (last)"));
            }
        }
        Schedule::Once { .. } => {}
    }
    Ok(())
}

/// Trim, sanitise and drop blank lines. An item with no label is not a step.
fn clean_items(items: Vec<ItemBody>) -> Result<Vec<NewItem>, ApiError> {
    items
        .into_iter()
        .filter(|i| !i.label.trim().is_empty())
        .map(|i| {
            Ok(NewItem {
                id: i.id,
                label: i.label.trim().to_string(),
                note: clean_note(&i.note),
                url: clean_url(&i.url)?,
                link_label: i.link_label.trim().to_string(),
            })
        })
        .collect()
}

/// A window is only valid if it can contain a day.
fn validate_window(start: Option<Date>, end: Option<Date>) -> Result<(), ApiError> {
    if let (Some(s), Some(e)) = (start, end) {
        if e < s {
            return Err(ApiError::bad_request("end date is before the start date"));
        }
    }
    Ok(())
}

async fn create_routine(
    State(state): State<AppState>,
    Json(b): Json<CreateRoutineBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    validate_session(&b.session)?;
    let schedule = match b.schedule {
        Some(s) => s,
        None => {
            if !(1..=127).contains(&b.weekdays) {
                return Err(ApiError::bad_request("weekdays mask must be 1..127"));
            }
            Schedule::Weekly { weekdays: b.weekdays, interval: 1 }
        }
    };
    validate_schedule(&schedule)?;
    let start_date = parse_opt_date(b.start_date.as_deref())?;
    let end_date = parse_opt_date(b.end_date.as_deref())?;
    validate_window(start_date, end_date)?;
    let items = clean_items(b.items)?;
    let routine = store::create_routine(
        &state.pool,
        NewRoutine {
            name: b.name.trim().to_string(),
            description: b.description.trim().to_string(),
            notes: clean_note(&b.notes),
            session: b.session,
            category_id: b.category_id,
            schedule,
            start_date,
            end_date,
        },
        &items,
    )
    .await?;
    Ok(Json(json!({ "routine": routine })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdateRoutineBody {
    name: Option<String>,
    description: Option<String>,
    notes: Option<String>,
    session: Option<String>,
    /// Present-and-null clears the category (template becomes uncategorised).
    #[serde(default, deserialize_with = "double_option")]
    category_id: Option<Option<Uuid>>,
    schedule: Option<Schedule>,
    /// Legacy: a bare mask still lands as a weekly rule.
    weekdays: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    start_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    end_date: Option<Option<String>>,
    active: Option<bool>,
    /// Full replacement list when present; entries keep their id to preserve tick history.
    items: Option<Vec<ItemBody>>,
}

async fn update_routine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateRoutineBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ApiError::bad_request("name cannot be empty"));
    }
    if let Some(s) = &b.session {
        validate_session(s)?;
    }
    let schedule = match (b.schedule, b.weekdays) {
        (Some(s), _) => Some(s),
        (None, Some(w)) => {
            if !(1..=127).contains(&w) {
                return Err(ApiError::bad_request("weekdays mask must be 1..127"));
            }
            Some(Schedule::Weekly { weekdays: w, interval: 1 })
        }
        (None, None) => None,
    };
    if let Some(s) = &schedule {
        validate_schedule(s)?;
    }
    let start_date = match &b.start_date {
        None => None,
        Some(v) => Some(parse_opt_date(v.as_deref())?),
    };
    let end_date = match &b.end_date {
        None => None,
        Some(v) => Some(parse_opt_date(v.as_deref())?),
    };
    // Validate the window as it will be after the patch, not just what this call sends.
    let current = store::get_routine(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("routine not found"))?;
    validate_window(
        start_date.unwrap_or(current.start_date),
        end_date.unwrap_or(current.end_date),
    )?;

    let routine = store::update_routine(
        &state.pool,
        id,
        RoutinePatch {
            name: b.name.map(|n| n.trim().to_string()),
            description: b.description.map(|d| d.trim().to_string()),
            notes: b.notes.map(|n| clean_note(&n)),
            session: b.session,
            category_id: b.category_id,
            schedule,
            start_date,
            end_date,
            active: b.active,
            position: None,
        },
    )
    .await?
    .ok_or_else(|| ApiError::not_found("routine not found"))?;
    if let Some(items) = b.items {
        store::set_items(&state.pool, id, &clean_items(items)?).await?;
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DuplicateBody {
    name: Option<String>,
}

async fn duplicate_routine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<DuplicateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = match b.name.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => {
            let src = store::get_routine(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("routine not found"))?;
            format!("{} (copy)", src.name)
        }
    };
    let routine = store::duplicate_routine(&state.pool, id, &name).await?;
    Ok(Json(json!({ "routine": routine })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ReorderBody {
    /// Template ids in their new display order.
    ids: Vec<Uuid>,
}

async fn reorder_routines(
    State(state): State<AppState>,
    Json(b): Json<ReorderBody>,
) -> Result<Json<Value>, ApiError> {
    for (i, id) in b.ids.iter().enumerate() {
        store::update_routine(
            &state.pool,
            *id,
            RoutinePatch { position: Some(i as f64), ..RoutinePatch::default() },
        )
        .await?;
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Day marks (consistency) ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct MarksQuery {
    from: Option<String>,
    to: Option<String>,
}

async fn list_marks(
    State(state): State<AppState>,
    Query(q): Query<MarksQuery>,
) -> Result<Json<Value>, ApiError> {
    let to = parse_date(q.to.as_deref())?;
    // Default window is the year ending at `to` — enough for the calendar and the year
    // counter in one call.
    let from = match q.from.as_deref() {
        Some(s) => parse_date(Some(s))?,
        None => to - time::Duration::days(365),
    };
    if from > to {
        return Err(ApiError::bad_request("from is after to"));
    }
    let marks = store::list_marks(&state.pool, from, to).await?;
    Ok(Json(json!({ "marks": marks })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SetMarkBody {
    /// Day to mark; defaults to today.
    date: Option<String>,
    /// `full` | `action`, or null to clear the day.
    mark: Option<String>,
}

async fn set_mark(
    State(state): State<AppState>,
    Json(b): Json<SetMarkBody>,
) -> Result<Json<Value>, ApiError> {
    let day = parse_date(b.date.as_deref())?;
    let mark = match b.mark.as_deref() {
        None | Some("") => None,
        Some(m) if store::DAY_MARKS.contains(&m) => Some(m),
        Some(_) => return Err(ApiError::bad_request("mark must be full|action or null")),
    };
    store::set_mark(&state.pool, day, mark).await?;
    Ok(Json(json!({ "ok": true, "date": day.format(&Iso8601::DATE).ok(), "mark": mark })))
}

// ── Item ticks ────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CheckBody {
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddTaskBody {
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdateTaskBody {
    title: Option<String>,
    note: Option<String>,
    priority: Option<String>,
    /// Present-and-null clears the due date; absent leaves it unchanged.
    #[serde(default, deserialize_with = "double_option")]
    due_date: Option<Option<String>>,
    done: Option<bool>,
}

/// Distinguishes an absent JSON field (None) from an explicit null (Some(None)) — the
/// difference between "leave this alone" and "clear it".
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(d).map(Some)
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
