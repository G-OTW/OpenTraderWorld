//! HTTP API for paper trading sessions.
//!
//! - `GET    /api/backtest/paper`               every session, with the badge counters
//! - `POST   /api/backtest/paper`               start one (the button on a backtest result)
//! - `GET    /api/backtest/paper/variables`     the template vocabulary + filters
//! - `GET    /api/backtest/paper/events`        the inbox (`?unseen=true`), across sessions
//! - `POST   /api/backtest/paper/events/seen`   acknowledge what the page just showed
//! - `GET/PUT/DELETE /api/backtest/paper/{id}`
//! - `POST   /api/backtest/paper/{id}/status`   pause / resume
//! - `POST   /api/backtest/paper/{id}/run`      tick now, and report what it did
//! - `POST   /api/backtest/paper/{id}/preview`  render the message templates on a sample
//! - `GET    /api/backtest/paper/{id}/trades`   the paper book (`?open=true`)
//!
//! A session freezes the settings it was created with: editing the strategy afterwards must
//! not silently change what is being traded forward. The recurrence half is validated by the
//! automator's own resolver, so there is one cron vocabulary in the app and not two.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::paper as store;

use crate::automator::schedule;
use crate::backtest::Settings;
use crate::paper;
use crate::{ApiError, AppState};

/// Events one listing returns. The log is trimmed per session anyway.
const EVENT_LIMIT: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/backtest/paper", get(list).post(create))
        .route("/api/backtest/paper/variables", get(variables))
        .route("/api/backtest/paper/preview", post(preview_draft))
        .route("/api/backtest/paper/events", get(events))
        .route("/api/backtest/paper/events/seen", post(seen))
        .route(
            "/api/backtest/paper/{id}",
            get(get_one).put(update).delete(remove),
        )
        .route("/api/backtest/paper/{id}/status", post(set_status))
        .route("/api/backtest/paper/{id}/run", post(run_now))
        .route("/api/backtest/paper/{id}/preview", post(preview))
        .route("/api/backtest/paper/{id}/trades", get(trades))
}

const FEEDS: &[&str] = &["hist", "live"];
const NOTIFY_WHEN: &[&str] = &["always", "on_change", "never"];
const CADENCE: &[&str] = &["each", "hourly", "daily", "weekly"];
const STATUSES: &[&str] = &["active", "paused"];

/// Validate the whole input and fill the defaults a half-filled form leaves out. The
/// recurrence is checked by the automator resolver, the vocabularies here, the strategy by
/// the engine itself: a session that cannot run is refused at save time, not at 3 a.m.
async fn vet(state: &AppState, input: &mut store::SessionInput) -> Result<(), ApiError> {
    input.name = input.name.trim().to_string();
    if input.name.is_empty() {
        return Err(ApiError::bad_request("name the session"));
    }
    if input.dataset_ids.is_empty() {
        return Err(ApiError::bad_request("pick at least one dataset"));
    }
    if input.dataset_ids.len() > 8 {
        return Err(ApiError::bad_request("a session reads at most 8 datasets"));
    }
    let settings: Settings = serde_json::from_value(input.settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("invalid settings: {e}")))?;
    if let Some(err) = settings.validate() {
        return Err(ApiError::bad_request(&err));
    }
    crate::backtest_api::check_single_asset(&settings.kind, &input.dataset_ids)?;
    if input.window_bars.is_some() && input.start_ts.is_some() {
        return Err(ApiError::bad_request(
            "give a trailing window or a start date, not both",
        ));
    }
    if let Some(n) = input.window_bars {
        if !(50..=200_000).contains(&n) {
            return Err(ApiError::bad_request(
                "the trailing window must be between 50 and 200,000 bars",
            ));
        }
    }
    if input.feed.is_empty() {
        input.feed = "hist".into();
    }
    if !FEEDS.contains(&input.feed.as_str()) {
        return Err(ApiError::bad_request("unknown data feed"));
    }
    if input.feed == "live" {
        // The column and the vocabulary exist so the live path is code and not a migration.
        // Until that code lands, saying so is better than storing a session that never ticks.
        return Err(ApiError::bad_request(
            "the live feed is not available yet — use the stored feed",
        ));
    }
    if input.notify_when.is_empty() {
        input.notify_when = "on_change".into();
    }
    if input.cadence.is_empty() {
        input.cadence = "each".into();
    }
    if input.status.is_empty() {
        input.status = "active".into();
    }
    if !NOTIFY_WHEN.contains(&input.notify_when.as_str()) {
        return Err(ApiError::bad_request("unknown notification policy"));
    }
    if !CADENCE.contains(&input.cadence.as_str()) {
        return Err(ApiError::bad_request("unknown report cadence"));
    }
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(ApiError::bad_request("a session is active or paused"));
    }
    if input.timezone.trim().is_empty() {
        input.timezone = "UTC".into();
    }
    // The instrument filter is a list of tickers, not free text: trimmed, de-duplicated and
    // capped, so a paste accident cannot turn one session into a thousand comparisons.
    let mut tickers: Vec<String> = Vec::new();
    for t in input.notify_tickers.iter() {
        let t = t.trim();
        if t.is_empty() || tickers.iter().any(|x: &String| x.eq_ignore_ascii_case(t)) {
            continue;
        }
        tickers.push(t.to_string());
    }
    if tickers.len() > 20 {
        return Err(ApiError::bad_request("at most 20 instruments in the alert filter"));
    }
    input.notify_tickers = tickers;
    // The rule is built through the same adapter the worker uses, so what is validated here
    // is exactly what will be resolved later.
    let probe = probe_session(input);
    schedule::validate(&paper::rule(&probe)).map_err(|e| ApiError::bad_request(&e))?;
    if paper::next_occurrence(&probe).is_none() && input.status == "active" {
        return Err(ApiError::bad_request(
            "this schedule has no next occurrence — it would never run",
        ));
    }
    for id in &input.channel_ids {
        match otw_store::notif_channels::get(&state.pool, *id).await? {
            Some(ch) if ch.allows(paper::MODULE) => {}
            Some(_) => {
                return Err(ApiError::bad_request(
                    "this notification channel is not allowed for paper trading",
                ))
            }
            None => return Err(ApiError::bad_request("unknown notification channel")),
        }
    }
    Ok(())
}

/// A throwaway session carrying the input's recurrence, so validation and the next-occurrence
/// computation go through the same code the worker runs.
fn probe_session(input: &store::SessionInput) -> store::Session {
    let now = OffsetDateTime::now_utc();
    store::Session {
        id: Uuid::nil(),
        name: input.name.clone(),
        strategy_id: input.strategy_id,
        settings: input.settings.clone(),
        dataset_ids: input.dataset_ids.clone(),
        window_bars: input.window_bars,
        start_ts: input.start_ts,
        feed: input.feed.clone(),
        kind: input.kind.clone(),
        timezone: input.timezone.clone(),
        every_minutes: input.every_minutes,
        at_hour: input.at_hour,
        at_minute: input.at_minute,
        weekdays: input.weekdays,
        day_of_month: input.day_of_month,
        run_at: input.run_at,
        next_run_at: None,
        last_run_at: None,
        notify_when: input.notify_when.clone(),
        cadence: input.cadence.clone(),
        channel_ids: input.channel_ids.clone(),
        entry_title_template: input.entry_title_template.clone(),
        entry_template: input.entry_template.clone(),
        exit_title_template: input.exit_title_template.clone(),
        exit_template: input.exit_template.clone(),
        summary_title_template: input.summary_title_template.clone(),
        summary_template: input.summary_template.clone(),
        last_digest_at: None,
        seeded_at: None,
        notify_open: input.notify_open,
        notify_close: input.notify_close,
        notify_tickers: input.notify_tickers.clone(),
        status: input.status.clone(),
        inputs_hash: String::new(),
        engine_version: 0,
        stats: None,
        bars: 0,
        last_error: String::new(),
        running: false,
        created_at: now,
        updated_at: now,
    }
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sessions = store::list_sessions(&state.pool).await?;
    let overview = store::overview(&state.pool).await?;
    Ok(Json(json!({ "sessions": sessions, "overview": overview })))
}

async fn create(
    State(state): State<AppState>,
    Json(mut input): Json<store::SessionInput>,
) -> Result<Json<store::Session>, ApiError> {
    vet(&state, &mut input).await?;
    let next = paper::next_occurrence(&probe_session(&input))
        .filter(|_| input.status == "active");
    Ok(Json(store::create_session(&state.pool, &input, next).await?))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    let open = store::list_trades(&state.pool, id, true).await?;
    let events = store::list_events(&state.pool, Some(id), false, 50).await?;
    Ok(Json(json!({ "session": session, "open": open, "events": events })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<store::SessionInput>,
) -> Result<Json<store::Session>, ApiError> {
    vet(&state, &mut input).await?;
    let stored = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    let next = paper::next_occurrence(&probe_session(&input)).filter(|_| input.status == "active");
    let mut saved = store::update_session(&state.pool, id, &input, next)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    // Changing the strategy or the instruments makes the stored book describe a simulation
    // that no longer exists. An empty book is honest; a stale one is not. The session is
    // unseeded with it, so the next tick re-learns its book in one summary instead of
    // announcing every inherited round trip as a fresh fill.
    if stored.settings != saved.settings || stored.dataset_ids != saved.dataset_ids {
        store::clear_trades(&state.pool, id).await?;
        saved.inputs_hash = String::new();
        saved.seeded_at = None;
        saved.stats = None;
        saved.bars = 0;
    }
    Ok(Json(saved))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_session(&state.pool, id).await? {
        return Err(ApiError::not_found("paper session not found"));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StatusBody {
    status: String,
}

async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StatusBody>,
) -> Result<Json<store::Session>, ApiError> {
    if !STATUSES.contains(&body.status.as_str()) {
        return Err(ApiError::bad_request("a session is active or paused"));
    }
    let session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    // Resuming re-plans from now: a session paused for a week must not fire a week of
    // catch-up occurrences the moment it comes back.
    let next = if body.status == "active" { paper::next_occurrence(&session) } else { None };
    Ok(Json(
        store::set_status(&state.pool, id, &body.status, next)
            .await?
            .ok_or_else(|| ApiError::not_found("paper session not found"))?,
    ))
}

/// Tick now. `force` re-simulates even when the inputs are unchanged, which is what the
/// button on the page means: the user asked to see it run.
async fn run_now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    let outcome = paper::run_once(&state, &session, true).await;
    let session = store::get_session(&state.pool, id).await?;
    Ok(Json(json!({
        "session": session,
        "opened": outcome.opened.len(),
        "closed": outcome.closed.len(),
        "bars": outcome.bars,
        "skipped": outcome.skipped,
        "error": outcome.error,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PreviewBody {
    /// "open", "close" or "digest" — which of the three messages to render.
    #[serde(default)]
    event: String,
    #[serde(default)]
    entry_title_template: String,
    #[serde(default)]
    entry_template: String,
    #[serde(default)]
    exit_title_template: String,
    #[serde(default)]
    exit_template: String,
    #[serde(default)]
    summary_title_template: String,
    #[serde(default)]
    summary_template: String,
    /// Used by the draft preview, where no session exists yet.
    #[serde(default)]
    name: String,
}

/// The three templates a preview overrides, applied to a session row (stored or blank).
fn apply_templates(session: &mut store::Session, body: &PreviewBody) {
    session.entry_title_template = body.entry_title_template.clone();
    session.entry_template = body.entry_template.clone();
    session.exit_title_template = body.exit_title_template.clone();
    session.exit_template = body.exit_template.clone();
    session.summary_title_template = body.summary_title_template.clone();
    session.summary_template = body.summary_template.clone();
}

/// Preview for a session being written, before it is saved. The editor must be able to show
/// what a template produces while it is still being typed.
async fn preview_draft(
    State(state): State<AppState>,
    Json(body): Json<PreviewBody>,
) -> Result<Json<Value>, ApiError> {
    let mut session = blank_session();
    if !body.name.trim().is_empty() {
        session.name = body.name.trim().to_string();
    }
    apply_templates(&mut session, &body);
    let (title, message) = paper::preview(&state, &session, event_of(&body.event)).await;
    Ok(Json(json!({ "title": title, "message": message })))
}

/// Which sample a preview renders: an entry, an exit, or the grouped message.
fn event_of(raw: &str) -> &'static str {
    match raw {
        "open" => "open",
        "digest" => "digest",
        _ => "close",
    }
}

/// A session with nothing filled in, for the two callers that need a context without a row:
/// the draft preview and the variable catalog.
fn blank_session() -> store::Session {
    probe_session(&store::SessionInput {
        name: "sample".into(),
        strategy_id: None,
        settings: json!({}),
        dataset_ids: Vec::new(),
        window_bars: None,
        start_ts: None,
        feed: "hist".into(),
        kind: "interval".into(),
        timezone: "UTC".into(),
        every_minutes: Some(60),
        at_hour: None,
        at_minute: None,
        weekdays: None,
        day_of_month: None,
        run_at: None,
        notify_when: "on_change".into(),
        cadence: "each".into(),
        channel_ids: Vec::new(),
        entry_title_template: String::new(),
        entry_template: String::new(),
        exit_title_template: String::new(),
        exit_template: String::new(),
        summary_title_template: String::new(),
        summary_template: String::new(),
        notify_open: true,
        notify_close: true,
        notify_tickers: Vec::new(),
        status: "active".into(),
    })
}

/// Render the templates being typed against a representative trade, so the editor shows the
/// message that would actually be sent, including the error a bad path produces.
async fn preview(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PreviewBody>,
) -> Result<Json<Value>, ApiError> {
    let mut session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    apply_templates(&mut session, &body);
    let (title, message) = paper::preview(&state, &session, event_of(&body.event)).await;
    Ok(Json(json!({ "title": title, "message": message })))
}

#[derive(Deserialize)]
struct TradeQuery {
    #[serde(default)]
    open: bool,
}

async fn trades(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TradeQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = store::list_trades(&state.pool, id, q.open).await?;
    Ok(Json(json!({ "trades": rows })))
}

#[derive(Deserialize)]
struct EventQuery {
    #[serde(default)]
    session: Option<Uuid>,
    #[serde(default)]
    unseen: bool,
}

/// The inbox. This is what makes a missed notification recoverable: an event the engine could
/// not deliver (down, no channel, digest pending) is still here on the next login.
async fn events(
    State(state): State<AppState>,
    Query(q): Query<EventQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = store::list_events(&state.pool, q.session, q.unseen, EVENT_LIMIT).await?;
    Ok(Json(json!({ "events": rows })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SeenBody {
    #[serde(default)]
    session: Option<Uuid>,
}

async fn seen(
    State(state): State<AppState>,
    Json(body): Json<SeenBody>,
) -> Result<Json<Value>, ApiError> {
    let n = store::mark_seen(&state.pool, body.session).await?;
    Ok(Json(json!({ "seen": n })))
}

#[derive(Deserialize)]
struct VarQuery {
    #[serde(default)]
    session: Option<Uuid>,
    /// Which message's vocabulary: entry, exit or summary. Each resolves a different
    /// context, so one list would offer paths the other two cannot read.
    #[serde(default)]
    kind: String,
}

/// The helper list behind the message editor. Generated from the renderer's own context, so
/// it cannot drift from what a template can actually resolve.
async fn variables(
    State(state): State<AppState>,
    Query(q): Query<VarQuery>,
) -> Result<Json<Value>, ApiError> {
    let session = match q.session {
        Some(id) => store::get_session(&state.pool, id).await?,
        None => None,
    };
    let session = session.unwrap_or_else(blank_session);
    Ok(Json(json!({
        "variables": paper::variables(&session, event_of(&q.kind)),
        "filters": paper::FILTERS,
    })))
}
