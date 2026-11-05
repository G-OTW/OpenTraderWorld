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
//! - `GET    /api/backtest/paper/{id}/dashboard` book, fills, events and market marks
//! - `GET    /api/backtest/paper/{id}/execution` a broker session: intended vs broker book,
//!   orders, fills simulated vs real, drift, the account's level and limits
//! - `PUT/DELETE /api/backtest/paper/{id}/arm`  arm (the code when the trading lock is on and
//!   the account is not paper) or disarm a broker session
//! - `POST   /api/backtest/paper/{id}/flatten`  stop and flatten: disarm, cancel the session's
//!   orders, close what it holds at the broker
//! - `POST   /api/backtest/paper/{id}/adopt`    accept the account's difference as outside the
//!   strategy, so a degraded session adds exposure again
//!
//! A session executes on a broker account when it is created with `execution.target = broker`
//! (`paper_exec`). That target is refused to an agent or a workflow (`internal_call::Automated`),
//! and arming, disarming, flattening and adopting are not in the MCP catalog; demo mode denies
//! all four.
//!
//! A session freezes the settings it was created with: editing the strategy afterwards must
//! not silently change what is being traded forward. The recurrence half is validated by the
//! automator's own resolver, so there is one cron vocabulary in the app and not two.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Extension, Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::paper as store;

use crate::automator::schedule;
use crate::backtest::Settings;
use crate::internal_call::Automated;
use crate::{paper, paper_exec};
use crate::{security, ApiError, AppState};
use otw_store::paper_exec as exec_store;
use otw_store::User;

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
        .route("/api/backtest/paper/{id}/dashboard", get(dashboard))
        .route("/api/backtest/paper/{id}/execution", get(execution))
        .route("/api/backtest/paper/{id}/arm", put(arm).delete(disarm))
        .route("/api/backtest/paper/{id}/flatten", post(flatten))
        .route("/api/backtest/paper/{id}/adopt", post(adopt))
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
pub(crate) fn probe_session(input: &store::SessionInput) -> store::Session {
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
        execution: None,
        exec_account_id: None,
        armed_at: None,
        armed_by: None,
        exec_state: "idle".into(),
        exec_note: String::new(),
        exec_synced_at: None,
        exec_book: None,
        exec_baseline: json!({}),
    }
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sessions = store::list_sessions(&state.pool).await?;
    let overview = store::overview(&state.pool).await?;
    // A broker session's card says what its orders move: paper, sandbox or live.
    let mut levels: std::collections::HashMap<Uuid, &'static str> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(sessions.len());
    for s in sessions {
        let level = match s.exec_account_id.filter(|_| s.on_broker()) {
            Some(a) => match levels.get(&a) {
                Some(l) => Some(*l),
                None => {
                    let l = otw_store::brokers::get(&state.pool, a).await?.as_ref().map_or("missing", paper_exec::level);
                    levels.insert(a, l);
                    Some(l)
                }
            },
            None => None,
        };
        let mut v = serde_json::to_value(&s).unwrap_or(Value::Null);
        if let (Some(m), Some(l)) = (v.as_object_mut(), level) {
            m.insert("exec_level".into(), json!(l));
        }
        out.push(v);
    }
    Ok(Json(json!({ "sessions": out, "overview": overview })))
}

async fn create(
    State(state): State<AppState>,
    automated: Option<Extension<Automated>>,
    Json(mut input): Json<store::SessionInput>,
) -> Result<Json<store::Session>, ApiError> {
    vet(&state, &mut input).await?;
    let settings: Settings = serde_json::from_value(input.settings.clone())
        .map_err(|e| ApiError::bad_request(&format!("invalid settings: {e}")))?;
    let broker = paper_exec::vet(&state, input.execution.as_ref(), &settings, &input.dataset_ids).await?;
    if broker.is_some() {
        // Orders sent at a broker are put there by the user, never by an agent or a workflow.
        if automated.is_some() {
            return Err(ApiError::forbidden(
                "a session that executes on a broker account is started by you from the backtest page, never by an agent or a workflow: start it simulated",
            ));
        }
        if crate::demo::enabled() {
            return Err(ApiError::forbidden_code("demo_disabled", "strategies never execute on a broker in the public demo"));
        }
    }
    let next = paper::next_occurrence(&probe_session(&input))
        .filter(|_| input.status == "active");
    let mut session = store::create_session(&state.pool, &input, next).await?;
    if let Some((ex, symbols)) = broker {
        let target = serde_json::to_value(&ex).unwrap_or(Value::Null);
        let account = ex.account_id.unwrap_or_default();
        if let Some(symbol) = exec_store::attach(&state.pool, session.id, account, &target, &symbols).await? {
            // Taken between the check and the write: the session goes, the refusal stays.
            let _ = store::delete_session(&state.pool, session.id).await;
            return Err(ApiError::bad_request(&format!(
                "another session took {symbol} on “{}” a moment ago: one strategy per instrument and account",
                ex.account
            )));
        }
        session.execution = Some(target);
        session.exec_account_id = Some(account);
    }
    Ok(Json(session))
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

/// The monitor reads the book and live fills together. Marks are stored closes until the
/// browser joins the same provider feed used by the paper worker; no provider REST calls
/// are made by polling this endpoint.
async fn dashboard(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    use otw_store::{connectors, histdata as hd};
    let session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    let trades = store::list_trades(&state.pool, id, false).await?;
    let events = store::list_events(&state.pool, Some(id), false, EVENT_LIMIT).await?;
    let fills = store::list_live_fills(&state.pool, id).await?;
    let mut instruments = Vec::new();
    for &dataset_id in &session.dataset_ids {
        let Some(ds) = hd::get_dataset(&state.pool, dataset_id).await? else { continue };
        let bars = hd::read_bars(&state.pool, dataset_id, None, None, 1).await?;
        let cap = crate::histdata::capabilities().into_iter().find(|c| c.provider == ds.provider);
        let timeframe = if cap.as_ref().is_some_and(|c| c.stream_timeframes.contains(&"1m")) {
            "1m"
        } else {
            &ds.timeframe
        };
        let stream = crate::live::stream_capable_for(&ds.provider, &ds.asset_type, timeframe);
        // The chart draws the strategy's own timeframe, which may not stream even when 1m does.
        let chart_stream = crate::live::stream_capable_for(&ds.provider, &ds.asset_type, &ds.timeframe);
        let connector = match connectors::default_for_module(&state.pool, &ds.provider, "backtest").await? {
            Some(c) => Some(c),
            None => connectors::default_for(&state.pool, &ds.provider).await?,
        };
        instruments.push(json!({
            "id": dataset_id, "ticker": ds.ticker, "provider": ds.provider,
            "asset_type": ds.asset_type, "timeframe": ds.timeframe,
            "stream_timeframe": timeframe, "stream": stream && connector.is_some(),
            "chart_stream": chart_stream && connector.is_some(),
            "connector_id": connector.map(|c| c.id),
            "price": bars.last().map(|b| b.close),
            "price_at": bars.last().and_then(|b| b.ts.format(&time::format_description::well_known::Rfc3339).ok()),
        }));
    }
    Ok(Json(json!({ "session": session, "trades": trades, "events": events,
        "live_fills": fills, "instruments": instruments })))
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
    // Where the fills happen is set at creation: a different target is a new session.
    if let Some(raw) = input.execution.as_ref().filter(|v| !v.is_null()) {
        let asked = raw.get("target").and_then(Value::as_str).unwrap_or("simulate");
        let stored_target = if stored.on_broker() { "broker" } else { "simulate" };
        let same_account = !stored.on_broker() || raw.get("account_id").and_then(Value::as_str) == stored.exec_account_id.map(|a| a.to_string()).as_deref();
        if (asked == "broker") != (stored_target == "broker") || !same_account {
            return Err(ApiError::bad_request(
                "where a session's fills happen is set when it starts: stop this one and start a new session to change it",
            ));
        }
    }
    let next = paper::next_occurrence(&probe_session(&input)).filter(|_| input.status == "active");
    let mut saved = store::update_session(&state.pool, id, &input, next)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    // Changing the strategy or the instruments makes the stored book describe a simulation
    // that no longer exists. An empty book is honest; a stale one is not. The session is
    // unseeded with it, so the next tick re-learns its book in one summary instead of
    // announcing every inherited round trip as a fresh fill.
    if stored.settings != saved.settings || stored.dataset_ids != saved.dataset_ids {
        // A broker session armed on the old strategy is disarmed: the user arms the new one
        // knowingly.
        if stored.armed_at.is_some() && exec_store::disarm(&state.pool, id).await? {
            saved.armed_at = None;
            saved.armed_by = None;
            saved.exec_state = "idle".into();
        }
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
    // An armed session may hold a position at the broker: it is stopped first, on purpose.
    if store::get_session(&state.pool, id).await?.is_some_and(|s| s.armed_at.is_some()) {
        return Err(ApiError::bad_request(
            "this session is armed on a broker account: disarm it, or stop and flatten it, before deleting it",
        ));
    }
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
    let session = match store::claim_manual(&state.pool, id).await? {
        Some(session) => session,
        None if store::get_session(&state.pool, id).await?.is_none() => {
            return Err(ApiError::not_found("paper session not found"));
        }
        None => return Err(ApiError::bad_request("paper session is already running")),
    };
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
    /// Which of the three messages to render: open (entry), close (exit) or digest
    /// (summary). Absent means close.
    #[serde(default)]
    event: Option<PaperEvent>,
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
    let (title, message) = paper::preview(&state, &session, event_of(body.event)).await;
    Ok(Json(json!({ "title": title, "message": message })))
}

crate::util::vocab::vocab! {
    /// Which message a preview renders and whose variables are listed: the entry (open),
    /// the exit (close) or the grouped summary (digest).
    pub PaperEvent("event") {
        Open = "open",
        Close = "close",
        Digest = "digest",
    }
}

/// Which sample a preview renders. Absent is the exit message, as it always was; anything
/// else outside the vocabulary was refused by serde before reaching here.
fn event_of(event: Option<PaperEvent>) -> &'static str {
    event.unwrap_or(PaperEvent::Close).as_str()
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
        execution: None,
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
    let (title, message) = paper::preview(&state, &session, event_of(body.event)).await;
    Ok(Json(json!({ "title": title, "message": message })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TradeQuery {
    #[serde(default, deserialize_with = "crate::util::lenient_bool::deserialize_lenient_bool")]
    #[schemars(with = "bool")]
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct EventQuery {
    #[serde(default)]
    session: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::util::lenient_bool::deserialize_lenient_bool")]
    #[schemars(with = "bool")]
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

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct VarQuery {
    #[serde(default)]
    session: Option<Uuid>,
    /// Which message's variables: open (entry), close (exit) or digest (summary). Absent
    /// means close. Each resolves a different context, so one list would offer paths the
    /// other two cannot read.
    #[serde(default)]
    kind: Option<PaperEvent>,
}

/// The helper list behind the message editor. Generated from the renderer's own context, so
/// it cannot drift from what a template can actually resolve.
async fn variables(
    State(state): State<AppState>,
    Query(q): Query<VarQuery>,
) -> Result<Json<Value>, ApiError> {
    // An unknown session is a 404, not the blank sample: that would look like a session
    // whose templates resolve nothing.
    let session = match q.session {
        Some(id) => store::get_session(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("paper session not found"))?,
        None => blank_session(),
    };
    Ok(Json(json!({
        "variables": paper::variables(&session, event_of(q.kind)),
        "filters": paper::FILTERS,
    })))
}

// ── Broker execution ─────────────────────────────────────────────────────────

async fn broker_session(state: &AppState, id: Uuid) -> Result<store::Session, ApiError> {
    let session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    if !session.on_broker() {
        return Err(ApiError::bad_request("this session simulates: it executes on no broker account"));
    }
    Ok(session)
}

/// A person at the page, never an agent or a workflow, and never in the demo.
fn by_hand(automated: &Option<Extension<Automated>>, what: &str) -> Result<(), ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden_code("demo_disabled", &format!("{what} is never done in the public demo")));
    }
    if automated.is_some() {
        return Err(ApiError::forbidden(&format!("{what} is done by you from the session's page, never by an agent or a workflow")));
    }
    Ok(())
}

/// The execution panel: intended vs broker book, orders, fills simulated vs real, drift, the
/// account's level and limits. Read from what the last sync stored: polling it calls no broker.
async fn execution(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let session = store::get_session(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("paper session not found"))?;
    let view = paper_exec::view(&state, &session).await.map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    Ok(Json(view))
}

#[derive(Deserialize, Default)]
pub(crate) struct ArmBody {
    /// The authenticator code, when the trading lock is on and the account is not paper.
    #[serde(default)]
    code: String,
}

/// Arm a broker session: from now on it may send orders with nobody watching. Refused without
/// the account's background trading permission (`background_trading_required`, the page offers
/// to sign it), on an account whose orders move real money without live trading on and limits
/// set (`live_off`, `live_limits_missing`), and, with the trading lock on and an account that is
/// not paper, without a current authenticator code in this request (`code_required`). One
/// arming, one session: there is no arming in bulk.
async fn arm(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    automated: Option<Extension<Automated>>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    body: Option<Json<ArmBody>>,
) -> Result<Json<Value>, ApiError> {
    by_hand(&automated, "arming a strategy")?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let session = broker_session(&state, id).await?;
    let ex = paper_exec::Execution::of(&session).ok_or_else(|| ApiError::bad_request("this session names no account"))?;
    let account_id = ex.account_id.ok_or_else(|| ApiError::bad_request("this session names no account"))?;
    let (connector, mut settings, account) = crate::brokers_api::resolve_trading(&state, account_id, Some(paper_exec::MODULE)).await?;
    if account.background_trading_ack.is_none() {
        return Err(ApiError::forbidden_code(
            crate::brokers::dispatch::BACKGROUND_REQUIRED,
            &format!("background trading is off for “{}”: sign it for the account, then arm the session", account.name),
        ));
    }
    crate::brokers_api::require_live(&state, &account, connector.as_ref(), &mut settings).await?;
    if account.broker != crate::brokers::paper::ID && otw_store::trading_lock(&state.pool, user.id).await? {
        let Some(key) = security::session_key(&jar) else {
            return Err(ApiError::forbidden("a strategy can only be armed from a signed-in browser"));
        };
        if body.code.trim().is_empty() {
            return Err(ApiError::forbidden_code("code_required", "the trading lock is on: enter your authenticator code to arm the session"));
        }
        crate::brokers_api::check_trading_code(&state, &user, &key, &body.code, "to arm a strategy").await?;
    }
    let baseline = paper_exec::baseline(&state, &session)
        .await
        .map_err(|e| ApiError::bad_request(&format!("the account could not be read, so the session was not armed: {e:#}")))?;
    exec_store::arm(&state.pool, id, user.id, &baseline).await?;
    let title = format!("{}: armed", session.name);
    let message = format!("Armed on “{}”: from the next close, the strategy's orders are sent to the broker.", account.name);
    let _ = store::add_event(&state.pool, id, "broker", &json!({ "armed": true }), &title, &message, true).await;
    crate::security_events::log("strategy_armed", &format!("strategy “{}” armed on “{}”", session.name, account.name));
    paper_exec::poke();
    Ok(Json(json!({ "ok": true, "session": store::get_session(&state.pool, id).await? })))
}

/// Disarm: nothing more is sent. What works at the broker stays there (a disarm never cancels:
/// Stop and flatten does). Asks nothing, the safe direction; still a person's act.
async fn disarm(
    State(state): State<AppState>,
    automated: Option<Extension<Automated>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    by_hand(&automated, "disarming a strategy")?;
    let session = broker_session(&state, id).await?;
    let was = exec_store::disarm(&state.pool, id).await?;
    if was {
        let title = format!("{}: disarmed", session.name);
        let _ = store::add_event(&state.pool, id, "broker", &json!({ "armed": false }), &title, "Disarmed: nothing more is sent. Orders already at the broker stay there.", true).await;
    }
    Ok(Json(json!({ "ok": true, "was_armed": was, "session": store::get_session(&state.pool, id).await? })))
}

/// Stop and flatten: disarm, cancel the session's working orders, close what its fills hold at
/// the market. A close is an order: with the trading lock on and an account that is not paper,
/// this session must have opened trading, or send the code with the request.
async fn flatten(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    automated: Option<Extension<Automated>>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    body: Option<Json<ArmBody>>,
) -> Result<Json<Value>, ApiError> {
    by_hand(&automated, "flattening a strategy")?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let session = broker_session(&state, id).await?;
    let ex = paper_exec::Execution::of(&session).ok_or_else(|| ApiError::bad_request("this session names no account"))?;
    let (_, _, account) = crate::brokers_api::resolve(&state, ex.account_id.unwrap_or_default(), None).await?;
    if let Err(e) = crate::brokers_api::require_unlocked(&state, &user, &jar, &account).await {
        let key = security::session_key(&jar);
        match key {
            Some(key) if !body.code.trim().is_empty() => {
                crate::brokers_api::check_trading_code(&state, &user, &key, &body.code, "to flatten a strategy").await?;
                security::TRADING_UNLOCK.grant(&key);
            }
            _ => return Err(e),
        }
    }
    let out = paper_exec::flatten(&state, &session).await?;
    Ok(Json(out))
}

/// Adopt what the account holds outside the strategy: the difference becomes part of the
/// baseline, and a degraded session adds exposure again. The strategy's own position is not
/// changed: what its orders filled is still what it holds.
async fn adopt(
    State(state): State<AppState>,
    automated: Option<Extension<Automated>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    by_hand(&automated, "adopting a difference at the broker")?;
    let session = broker_session(&state, id).await?;
    let baseline = paper_exec::baseline(&state, &session)
        .await
        .map_err(|e| ApiError::bad_request(&format!("the account could not be read: {e:#}")))?;
    exec_store::set_baseline(&state.pool, id, &baseline).await?;
    let title = format!("{}: difference adopted", session.name);
    let _ = store::add_event(&state.pool, id, "broker", &json!({ "baseline": baseline }), &title, "What the account holds outside this strategy is now its own: the session follows its own orders from here.", true).await;
    paper_exec::poke();
    Ok(Json(json!({ "ok": true, "baseline": baseline })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(qs: &str) -> Result<Option<PaperEvent>, String> {
        let uri: axum::http::Uri = format!("/x?{qs}").parse().unwrap();
        Query::<VarQuery>::try_from_uri(&uri).map(|Query(q)| q.kind).map_err(|e| e.body_text())
    }

    #[test]
    fn the_event_vocabulary_is_open_close_digest() {
        assert_eq!(kind("kind=Open").unwrap(), Some(PaperEvent::Open));
        assert_eq!(event_of(kind("").unwrap()), "close");
        assert_eq!(event_of(kind("kind=digest").unwrap()), "digest");
        let e = kind("kind=entry").unwrap_err();
        assert!(e.contains(r#"unknown event "entry"; valid: open, close, digest"#), "{e}");
        let body: Result<PreviewBody, _> = serde_json::from_value(json!({ "event": "summary" }));
        assert!(body.is_err());
        let body: PreviewBody = serde_json::from_value(json!({})).unwrap();
        assert_eq!(event_of(body.event), "close");
    }
}
