//! HTTP API for the Mindset module.
//!
//! Two faces of the same data, mirroring the Routines module. The **day view**
//! (`/api/mindset/day`) returns the active templates with their prompts, the check-ins
//! already filled for that date, the categories and the consistency marks. The **template**
//! endpoints are the authoring side: categories plus full CRUD on a template's name,
//! description, notes, phase and prompt list.
//!
//! Rich text (a template's `notes`) is sanitised here, at the edge, so nothing dangerous
//! ever reaches the database.

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use time::{format_description::well_known::Iso8601, Date};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::mindset::{self as store, NewPrompt, NewTemplate, TemplatePatch};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mindset/day", get(day))
        .route("/api/mindset/entries", put(save_entry).delete(delete_entry))
        .route("/api/mindset/history", get(history))
        .route("/api/mindset/marks", get(list_marks).put(set_mark))
        .route(
            "/api/mindset/templates",
            get(list_templates).post(create_template).delete(clear_templates),
        )
        .route("/api/mindset/templates/reset", post(reset_templates))
        .route(
            "/api/mindset/templates/{id}",
            get(template_detail).patch(update_template).delete(delete_template),
        )
        .route("/api/mindset/templates/{id}/duplicate", post(duplicate_template))
        .route("/api/mindset/categories", get(list_categories).post(create_category))
        .route(
            "/api/mindset/categories/{id}",
            patch(update_category).delete(delete_category),
        )
        // Kept from the pre-template API: the check-in editor still tweaks a single prompt
        // (enable, rename, re-option) without resubmitting the whole template.
        .route("/api/mindset/prompts", get(list_prompts).post(add_prompt))
        .route(
            "/api/mindset/prompts/{id}",
            patch(update_prompt).delete(delete_prompt),
        )
}

/// When in the trading day a check-in belongs. `live` covers a session break — the
/// routines module splits the day the same three ways.
const PHASES: &[&str] = &["pre", "live", "post"];
const KINDS: &[&str] = &["scale", "choice", "tags", "text"];

fn parse_date(s: Option<&str>) -> Result<Date, ApiError> {
    match s {
        Some(s) => Date::parse(s, &Iso8601::DATE)
            .map_err(|_| ApiError::bad_request("invalid date (expected YYYY-MM-DD)")),
        None => Ok(time::OffsetDateTime::now_utc().date()),
    }
}

// ── Rich text ─────────────────────────────────────────────────────────────────

/// Template notes are small formatted blocks: text marks, lists, headings and links. No
/// images, no tables — a note that needs those belongs in the editor module.
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
    let templates = store::list_templates(&state.pool, true).await?;
    let prompts = store::day_prompts(&state.pool).await?;
    let entries = store::entries_for_date(&state.pool, date).await?;
    let categories = store::list_categories(&state.pool).await?;
    // A year of marks rides along: it feeds the week/month/year counter and the calendar
    // without a second round trip on load.
    let marks = store::list_marks(&state.pool, date - time::Duration::days(365), date).await?;
    let mark = store::get_mark(&state.pool, date).await?;
    Ok(Json(json!({
        "date": date.format(&Iso8601::DATE).ok(),
        "templates": templates,
        "prompts": prompts,
        "entries": entries,
        "categories": categories,
        "marks": marks,
        "mark": mark
    })))
}

// ── Entries ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SaveEntryBody {
    date: Option<String>,
    /// The check-in being saved. Omitted only by pre-template clients, which then have to
    /// name a `phase` and land on that phase's first template.
    template_id: Option<Uuid>,
    phase: Option<String>,
    answers: Value,
}

async fn save_entry(
    State(state): State<AppState>,
    Json(b): Json<SaveEntryBody>,
) -> Result<Json<Value>, ApiError> {
    if !b.answers.is_object() {
        return Err(ApiError::bad_request("answers must be an object"));
    }
    let template = resolve_template(&state, b.template_id, b.phase.as_deref()).await?;
    let date = parse_date(b.date.as_deref())?;
    let entry =
        store::save_entry(&state.pool, date, template.id, &template.phase, &b.answers).await?;
    Ok(Json(json!({ "entry": entry })))
}

/// Accept either a template id or a bare phase (legacy/MCP callers), so an older client
/// saving "the pre-mortem" still lands somewhere sensible.
async fn resolve_template(
    state: &AppState,
    template_id: Option<Uuid>,
    phase: Option<&str>,
) -> Result<store::Template, ApiError> {
    if let Some(id) = template_id {
        return store::get_template(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("template not found"));
    }
    let phase = phase.ok_or_else(|| ApiError::bad_request("template_id or phase is required"))?;
    if !PHASES.contains(&phase) {
        return Err(ApiError::bad_request("phase must be pre|live|post"));
    }
    store::list_templates(&state.pool, true)
        .await?
        .into_iter()
        .find(|t| t.phase == phase)
        .ok_or_else(|| ApiError::not_found("no template for that phase"))
}

#[derive(Deserialize)]
struct DeleteEntryBody {
    date: String,
    template_id: Option<Uuid>,
    phase: Option<String>,
}

async fn delete_entry(
    State(state): State<AppState>,
    Json(b): Json<DeleteEntryBody>,
) -> Result<Json<Value>, ApiError> {
    let template = resolve_template(&state, b.template_id, b.phase.as_deref()).await?;
    let date = parse_date(Some(&b.date))?;
    store::delete_entry(&state.pool, date, template.id).await?;
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
    let templates = store::list_templates(&state.pool, false).await?;
    let categories = store::list_categories(&state.pool).await?;
    let entries = store::recent_entries(&state.pool, limit).await?;
    Ok(Json(json!({
        "prompts": prompts,
        "templates": templates,
        "categories": categories,
        "entries": entries
    })))
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

#[derive(Deserialize)]
struct CategoryPatch {
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

// ── Templates ─────────────────────────────────────────────────────────────────

async fn list_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::seed_if_first_run(&state.pool).await?;
    let templates = store::list_templates(&state.pool, false).await?;
    let categories = store::list_categories(&state.pool).await?;
    let mut out = Vec::with_capacity(templates.len());
    for t in templates {
        let prompts = store::list_prompts_for(&state.pool, t.id).await?;
        out.push(json!({ "template": t, "prompts": prompts }));
    }
    Ok(Json(json!({ "templates": out, "categories": categories })))
}

async fn template_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let template = store::get_template(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("template not found"))?;
    let prompts = store::list_prompts_for(&state.pool, id).await?;
    Ok(Json(json!({ "template": template, "prompts": prompts })))
}

/// A prompt as the template form sends it.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PromptBody {
    #[serde(default)]
    id: Option<Uuid>,
    kind: String,
    label: String,
    #[serde(default)]
    hint: String,
    #[serde(default)]
    config: Value,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateTemplateBody {
    name: String,
    #[serde(default)]
    description: String,
    /// Rich text (HTML) — sanitised before it is stored.
    #[serde(default)]
    notes: String,
    #[serde(default)]
    category_id: Option<Uuid>,
    #[serde(default = "default_phase")]
    phase: String,
    #[serde(default)]
    prompts: Vec<PromptBody>,
}
fn default_phase() -> String {
    "pre".into()
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

/// Trim, validate and drop blank rows. A prompt with no label is not a question.
fn clean_prompts(prompts: Vec<PromptBody>) -> Result<Vec<NewPrompt>, ApiError> {
    prompts
        .into_iter()
        .filter(|p| !p.label.trim().is_empty())
        .map(|p| {
            if !KINDS.contains(&p.kind.as_str()) {
                return Err(ApiError::bad_request("kind must be scale|choice|tags|text"));
            }
            let config = if p.config.is_object() { p.config.clone() } else { json!({}) };
            validate_config(&p.kind, &config)?;
            Ok(NewPrompt {
                id: p.id,
                kind: p.kind,
                label: p.label.trim().to_string(),
                hint: p.hint.trim().to_string(),
                config,
            })
        })
        .collect()
}

async fn create_template(
    State(state): State<AppState>,
    Json(b): Json<CreateTemplateBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    if !PHASES.contains(&b.phase.as_str()) {
        return Err(ApiError::bad_request("phase must be pre|live|post"));
    }
    let prompts = clean_prompts(b.prompts)?;
    let template = store::create_template(
        &state.pool,
        NewTemplate {
            name: b.name.trim().to_string(),
            description: b.description.trim().to_string(),
            notes: clean_note(&b.notes),
            category_id: b.category_id,
            phase: b.phase.clone(),
        },
    )
    .await?;
    store::set_prompts(&state.pool, template.id, &b.phase, &prompts).await?;
    Ok(Json(json!({ "template": template })))
}

#[derive(Deserialize)]
struct UpdateTemplateBody {
    name: Option<String>,
    description: Option<String>,
    notes: Option<String>,
    /// Present-and-null clears the category.
    #[serde(default, deserialize_with = "double_option")]
    category_id: Option<Option<Uuid>>,
    phase: Option<String>,
    active: Option<bool>,
    /// Full replacement list when present; entries keep their id so past answers stay
    /// attached to the prompt they answered.
    prompts: Option<Vec<PromptBody>>,
}

/// Distinguishes an absent JSON field (None) from an explicit null (Some(None)).
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<UpdateTemplateBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ApiError::bad_request("name cannot be empty"));
    }
    if let Some(p) = &b.phase {
        if !PHASES.contains(&p.as_str()) {
            return Err(ApiError::bad_request("phase must be pre|live|post"));
        }
    }
    let template = store::update_template(
        &state.pool,
        id,
        TemplatePatch {
            name: b.name.map(|n| n.trim().to_string()),
            description: b.description.map(|d| d.trim().to_string()),
            notes: b.notes.map(|n| clean_note(&n)),
            category_id: b.category_id,
            phase: b.phase,
            active: b.active,
            position: None,
        },
    )
    .await?
    .ok_or_else(|| ApiError::not_found("template not found"))?;
    if let Some(prompts) = b.prompts {
        let cleaned = clean_prompts(prompts)?;
        store::set_prompts(&state.pool, id, &template.phase, &cleaned).await?;
    }
    Ok(Json(json!({ "template": template })))
}

async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    store::delete_template(&state.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DuplicateBody {
    name: Option<String>,
}

async fn duplicate_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<DuplicateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = match b.name.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => {
            let src = store::get_template(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("template not found"))?;
            format!("{} (copy)", src.name)
        }
    };
    let template = store::duplicate_template(&state.pool, id, &name).await?;
    Ok(Json(json!({ "template": template })))
}

async fn reset_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::reset_templates(&state.pool).await?;
    let templates = store::list_templates(&state.pool, false).await?;
    Ok(Json(json!({ "templates": templates })))
}

/// Delete every template so the user can build their check-ins from scratch (the starter
/// set does not respawn — seeding is first-run only).
async fn clear_templates(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::clear_templates(&state.pool).await?;
    Ok(Json(json!({ "templates": [] })))
}

// ── Prompts (single-prompt edits) ─────────────────────────────────────────────

async fn list_prompts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    store::seed_if_first_run(&state.pool).await?;
    let prompts = store::list_prompts(&state.pool, false).await?;
    Ok(Json(json!({ "prompts": prompts })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddPromptBody {
    template_id: Uuid,
    kind: String,
    label: String,
    #[serde(default)]
    hint: String,
    #[serde(default)]
    config: Value,
}

async fn add_prompt(
    State(state): State<AppState>,
    Json(b): Json<AddPromptBody>,
) -> Result<Json<Value>, ApiError> {
    if !KINDS.contains(&b.kind.as_str()) {
        return Err(ApiError::bad_request("kind must be scale|choice|tags|text"));
    }
    if b.label.trim().is_empty() {
        return Err(ApiError::bad_request("label is required"));
    }
    let template = store::get_template(&state.pool, b.template_id)
        .await?
        .ok_or_else(|| ApiError::not_found("template not found"))?;
    let config = if b.config.is_object() { b.config.clone() } else { json!({}) };
    validate_config(&b.kind, &config)?;
    let position = store::next_position(&state.pool, template.id).await?;
    let prompt = store::add_prompt(
        &state.pool,
        template.id,
        &template.phase,
        &b.kind,
        b.label.trim(),
        b.hint.trim(),
        &config,
        position,
    )
    .await?;
    Ok(Json(json!({ "prompt": prompt })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdatePromptBody {
    label: Option<String>,
    hint: Option<String>,
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
        b.hint.as_deref().map(str::trim),
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
