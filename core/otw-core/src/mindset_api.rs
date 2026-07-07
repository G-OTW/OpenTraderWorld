//! HTTP API for the Mindset module.
//!
//! `/day` returns the active prompts plus the (date, phase) check-ins in one call, seeding
//! the starter prompt set on first use. Saving a check-in is a full-map upsert. Prompts have
//! CRUD plus a reset-to-defaults. `/history` feeds the past-entries list and trend charts.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::mindset as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mindset/day", get(day))
        .route("/api/mindset/entries", put(save_entry).delete(delete_entry))
        .route("/api/mindset/history", get(history))
        .route("/api/mindset/prompts", get(list_prompts).post(add_prompt).delete(clear_prompts))
        .route("/api/mindset/prompts/reset", post(reset_prompts))
        .route("/api/mindset/prompts/{id}", axum::routing::patch(update_prompt).delete(delete_prompt))
}

const PHASES: &[&str] = &["pre", "post"];
const KINDS: &[&str] = &["scale", "choice", "tags", "text"];

fn parse_date(s: Option<&str>) -> Result<Date, ApiError> {
    match s {
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
        None => Ok(time::OffsetDateTime::now_utc().date()),
    }
}

// ── Day view ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DayQuery {
    date: Option<String>,
}

async fn day(
    State(state): State<AppState>,
    Query(q): Query<DayQuery>,
) -> Result<Json<Value>, ApiError> {
    let date = parse_date(q.date.as_deref())?;
    store::seed_if_first_run(&state.pool).await?;
    let prompts = store::list_prompts(&state.pool, true).await?;
    let entries = store::entries_for_date(&state.pool, date).await?;
    Ok(Json(json!({
        "date": date.format(&Iso8601::DATE).ok(),
        "prompts": prompts,
        "entries": entries
    })))
}

// ── Entries ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SaveEntryBody {
    date: Option<String>,
    phase: String,
    answers: Value,
}

async fn save_entry(
    State(state): State<AppState>,
    Json(b): Json<SaveEntryBody>,
) -> Result<Json<Value>, ApiError> {
    if !PHASES.contains(&b.phase.as_str()) {
        return Err(ApiError::bad_request("phase must be pre|post"));
    }
    if !b.answers.is_object() {
        return Err(ApiError::bad_request("answers must be an object"));
    }
    let date = parse_date(b.date.as_deref())?;
    let entry = store::save_entry(&state.pool, date, &b.phase, &b.answers).await?;
    Ok(Json(json!({ "entry": entry })))
}

#[derive(Deserialize)]
struct DeleteEntryBody {
    date: String,
    phase: String,
}

async fn delete_entry(
    State(state): State<AppState>,
    Json(b): Json<DeleteEntryBody>,
) -> Result<Json<Value>, ApiError> {
    if !PHASES.contains(&b.phase.as_str()) {
        return Err(ApiError::bad_request("phase must be pre|post"));
    }
    let date = parse_date(Some(&b.date))?;
    store::delete_entry(&state.pool, date, &b.phase).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<i64>,
}

async fn history(
    State(state): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(60).clamp(1, 365);
    store::seed_if_first_run(&state.pool).await?;
    let prompts = store::list_prompts(&state.pool, false).await?;
    let entries = store::recent_entries(&state.pool, limit).await?;
    Ok(Json(json!({ "prompts": prompts, "entries": entries })))
}

// ── Prompts ───────────────────────────────────────────────────────────────────

async fn list_prompts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::seed_if_first_run(&state.pool).await?;
    let prompts = store::list_prompts(&state.pool, false).await?;
    Ok(Json(json!({ "prompts": prompts })))
}

#[derive(Deserialize)]
struct AddPromptBody {
    phase: String,
    kind: String,
    label: String,
    #[serde(default)]
    config: Value,
}

fn validate_config(kind: &str, config: &Value) -> Result<(), ApiError> {
    if matches!(kind, "choice" | "tags") {
        let ok = config
            .get("options")
            .and_then(|o| o.as_array())
            .map(|a| !a.is_empty() && a.iter().all(|v| v.is_string()))
            .unwrap_or(false);
        if !ok {
            return Err(ApiError::bad_request(
                "choice/tags prompts need config.options: [string, …]",
            ));
        }
    }
    Ok(())
}

async fn add_prompt(
    State(state): State<AppState>,
    Json(b): Json<AddPromptBody>,
) -> Result<Json<Value>, ApiError> {
    if !PHASES.contains(&b.phase.as_str()) {
        return Err(ApiError::bad_request("phase must be pre|post"));
    }
    if !KINDS.contains(&b.kind.as_str()) {
        return Err(ApiError::bad_request("kind must be scale|choice|tags|text"));
    }
    if b.label.trim().is_empty() {
        return Err(ApiError::bad_request("label is required"));
    }
    let config = if b.config.is_object() { b.config.clone() } else { json!({}) };
    validate_config(&b.kind, &config)?;
    let position = store::next_position(&state.pool, &b.phase).await?;
    let prompt =
        store::add_prompt(&state.pool, &b.phase, &b.kind, b.label.trim(), &config, position).await?;
    Ok(Json(json!({ "prompt": prompt })))
}

#[derive(Deserialize)]
struct UpdatePromptBody {
    label: Option<String>,
    config: Option<Value>,
    position: Option<f64>,
    active: Option<bool>,
}

async fn update_prompt(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdatePromptBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(l) = &b.label {
        if l.trim().is_empty() {
            return Err(ApiError::bad_request("label cannot be empty"));
        }
    }
    let prompt = store::update_prompt(
        &state.pool,
        id,
        b.label.as_deref().map(str::trim),
        b.config.as_ref(),
        b.position,
        b.active,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("prompt not found"))?;
    Ok(Json(json!({ "prompt": prompt })))
}

async fn delete_prompt(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_prompt(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn reset_prompts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::reset_prompts(&state.pool).await?;
    let prompts = store::list_prompts(&state.pool, false).await?;
    Ok(Json(json!({ "prompts": prompts })))
}

/// Delete every prompt so the user can build a check-in template from scratch
/// (the starter set does not respawn — seeding is first-run only).
async fn clear_prompts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::clear_prompts(&state.pool).await?;
    Ok(Json(json!({ "prompts": [] })))
}
