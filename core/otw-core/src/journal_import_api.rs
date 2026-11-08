//! HTTP API for the Trading Journal's trade-book import.
//!
//! `analyze` is the whole read-only half of the feature: it reads the uploaded file,
//! proposes (or applies) a mapping, and returns the trades that mapping would create —
//! so the mapping step and the visual check run on exactly what `commit` will write.
//! Nothing is stored until `commit`, and a commit can be reverted in one call.
//!
//! The file rides in the request body as base64 on every call (analyze is re-run as the
//! user edits the mapping). No server-side upload state, no temp files, no cleanup job.

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::journal_import::{self, Mapping};
use crate::{ApiError, AppState};
use otw_store::journal_import as store;

/// The grant a broker account needs before the journal may pull from it.
const MODULE: &str = "journal";

/// Base64 inflates by 4/3; 20 MB of CSV is a very large trade book.
const MAX_BODY: usize = 30 * 1024 * 1024;

/// A file the user picked is their problem to fix, not a server fault — and the whole
/// anyhow chain ("reading the file: invalid delimiter") is what makes it fixable.
fn bad_file(e: anyhow::Error) -> ApiError {
    ApiError::bad_request(&format!("{e:#}"))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/journal/import/analyze",
            post(analyze).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route(
            "/api/journal/import/commit",
            post(commit).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route("/api/journal/import/mappings", get(list_mappings).post(save_mapping))
        .route(
            "/api/journal/import/mappings/{id}",
            axum::routing::patch(update_mapping).delete(delete_mapping),
        )
        .route("/api/journal/import/broker/preview", post(broker_preview))
        .route("/api/journal/import/broker/commit", post(broker_commit))
        .route("/api/journal/import/broker/last", get(broker_last))
        .route("/api/journal/import/broker/schedule", axum::routing::put(broker_schedule))
        .route("/api/journal/import/broker/conflicts", get(broker_conflicts))
        .route("/api/journal/import/broker/conflicts/{id}", post(broker_answer))
        .route("/api/journal/import/broker/rules/forget", post(broker_forget_rule))
        .route("/api/journal/import/batches", get(list_batches))
        .route("/api/journal/import/batches/{id}/revert", post(revert_batch))
        .route("/api/journal/import/batches/{id}", axum::routing::delete(forget_batch))
}

// ── Analyze / preview ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AnalyzeBody {
    #[serde(default)]
    filename: String,
    /// The file as base64 (a `data:` URI is accepted as-is).
    #[serde(default)]
    content: String,
    /// Omit to auto-detect; send the edited mapping to re-preview with it.
    mapping: Option<Mapping>,
    /// Which table of a stacked statement to detect against ("" = read the file flat).
    /// Only meaningful when `mapping` is omitted.
    section: Option<String>,
    /// How many built trades to return (the stats always cover the whole file).
    limit: Option<usize>,
}

async fn analyze(
    State(state): State<AppState>,
    Json(body): Json<AnalyzeBody>,
) -> Result<Json<Value>, ApiError> {
    let bytes = journal_import::parse::decode_upload(&body.content)
        .map_err(bad_file)?;
    let limit = body.limit.unwrap_or(25).clamp(1, 200);
    let analysis =
        journal_import::analyze(&state.pool, &body.filename, &bytes, body.mapping, body.section, limit)
        .await
        .map_err(bad_file)?;
    Ok(Json(json!({ "analysis": analysis })))
}

// ── Commit ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CommitBody {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    content: String,
    mapping: Mapping,
    /// Name to save this mapping under for future imports (omit to not save it).
    save_as: Option<String>,
}

async fn commit(
    State(state): State<AppState>,
    Json(body): Json<CommitBody>,
) -> Result<Json<Value>, ApiError> {
    let bytes = journal_import::parse::decode_upload(&body.content)
        .map_err(bad_file)?;
    let report =
        journal_import::commit(&state.pool, &body.filename, &bytes, body.mapping, body.save_as)
            .await
            .map_err(bad_file)?;
    Ok(Json(json!({ "report": report })))
}

// ── Broker sync ──────────────────────────────────────────────────────────────

/// The background scope and the page a finished pull is read back on, for one journal.
fn task_scope(req: &journal_import::broker::SyncRequest) -> (String, String) {
    let book = req.category_id.map(|c| c.to_string()).unwrap_or_default();
    let key = if book.is_empty() { "default" } else { book.as_str() };
    (format!("journal:{key}"), format!("/journal?view=import&broker={book}"))
}

fn task_estimate(broker: &str, req: &journal_import::broker::SyncRequest) -> f64 {
    crate::brokers::estimate::executions(broker, req.from, req.to, req.symbols.len())
}

/// What was asked, handed back with the answer: the modal reopens on the same form, so the
/// import after a background preview asks for the window the preview read.
fn task_request(req: &journal_import::broker::SyncRequest) -> Value {
    json!({
        "account_id": req.account_id,
        "from": req.from.format(&time::format_description::well_known::Rfc3339).ok(),
        "to": req.to.format(&time::format_description::well_known::Rfc3339).ok(),
        "symbols": req.symbols,
        "allow_short": req.allow_short,
    })
}

/// Pull a period of executions from a broker account, build the trades it would file, and
/// write nothing. Same contract as `analyze`: the preview is exactly what commit writes.
/// `?background=true` runs the pull as a task and answers with it.
async fn broker_preview(
    State(state): State<AppState>,
    Query(bg): Query<crate::tasks::Background>,
    Json(req): Json<journal_import::broker::SyncRequest>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(&state, req.account_id, Some(MODULE)).await?;
    if bg.background {
        let (scope, link) = task_scope(&req);
        let estimate = task_estimate(&account.broker, &req);
        let request = task_request(&req);
        let pool = state.pool.clone();
        let task = crate::tasks::spawn(
            &state,
            scope,
            "Journal: broker pull".into(),
            link,
            MODULE,
            estimate,
            async move {
                let preview = journal_import::broker::preview(
                    &pool,
                    connector.as_ref(),
                    &settings,
                    &account,
                    &req,
                    60,
                )
                .await?;
                Ok(crate::tasks::Done {
                    summary: format!(
                        "{} fill(s) read, {} trade(s) to review before importing.",
                        preview.executions, preview.stats.trades
                    ),
                    result: json!({ "preview": preview, "request": request }),
                })
            },
        )
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        return Ok(Json(json!({ "task": task })));
    }
    let preview = journal_import::broker::preview(
        &state.pool,
        connector.as_ref(),
        &settings,
        &account,
        &req,
        60,
    )
    .await
    .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "preview": preview })))
}

/// Pull the same window and write it, in one revertible batch.
async fn broker_commit(
    State(state): State<AppState>,
    Query(bg): Query<crate::tasks::Background>,
    Json(req): Json<journal_import::broker::SyncRequest>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(&state, req.account_id, Some(MODULE)).await?;
    if bg.background {
        let (scope, link) = task_scope(&req);
        let estimate = task_estimate(&account.broker, &req);
        let request = task_request(&req);
        let pool = state.pool.clone();
        let task = crate::tasks::spawn(
            &state,
            scope,
            "Journal: broker import".into(),
            link,
            MODULE,
            estimate,
            async move {
                let report = journal_import::broker::commit(
                    &pool,
                    connector.as_ref(),
                    &settings,
                    &account,
                    &req,
                )
                .await?;
                Ok(crate::tasks::Done {
                    summary: format!(
                        "{} trade(s) imported, {} updated, {} waiting for an answer.",
                        report.imported, report.updated, report.conflicts
                    ),
                    result: json!({ "report": report, "request": request }),
                })
            },
        )
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        return Ok(Json(json!({ "task": task })));
    }
    let report =
        journal_import::broker::commit(&state.pool, connector.as_ref(), &settings, &account, &req)
            .await
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "report": report })))
}

#[derive(Deserialize)]
struct LastQuery {
    category_id: Option<Uuid>,
}

/// What this journal was last pulled from, so the modal reopens on it rather than empty,
/// with its schedule, its past answers and the conflicts still waiting.
async fn broker_last(
    State(state): State<AppState>,
    Query(q): Query<LastQuery>,
) -> Result<Json<Value>, ApiError> {
    let Some(category_id) = q.category_id else {
        return Ok(Json(json!({ "sync": Value::Null, "conflicts": [] })));
    };
    let sync = store::get_broker_sync(&state.pool, category_id).await?;
    let conflicts = otw_store::jobs::list_conflicts(
        &state.pool,
        journal_import::broker::MODULE,
        Some(category_id),
    )
    .await?;
    Ok(Json(json!({ "sync": sync, "conflicts": conflicts })))
}

#[derive(Deserialize)]
struct ScheduleBody {
    category_id: Uuid,
    enabled: bool,
    #[serde(default)]
    paused: bool,
    #[serde(default = "default_interval")]
    interval_minutes: i32,
}

fn default_interval() -> i32 {
    60
}

/// Schedule the journal's last sync to run again on its own. Only a sync the user has
/// already run by hand can be scheduled: the first window is theirs to check in the
/// preview, and the schedule replays it from there.
async fn broker_schedule(
    State(state): State<AppState>,
    Json(b): Json<ScheduleBody>,
) -> Result<Json<Value>, ApiError> {
    if b.enabled {
        crate::jobs::check_interval(b.interval_minutes)
            .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    }
    let sync = store::get_broker_sync(&state.pool, b.category_id)
        .await?
        .filter(|s| s.account_id.is_some() && s.last_synced_at.is_some())
        .ok_or_else(|| {
            ApiError::bad_request("run a first sync by hand, then schedule it")
        })?;
    store::set_broker_schedule(
        &state.pool,
        b.category_id,
        b.enabled,
        b.paused,
        if b.enabled { b.interval_minutes } else { sync.interval_minutes },
    )
    .await?;
    Ok(Json(json!({ "sync": store::get_broker_sync(&state.pool, b.category_id).await? })))
}

/// Conflicts waiting for an answer, for one journal or all of them (the pending tasks).
async fn broker_conflicts(
    State(state): State<AppState>,
    Query(q): Query<LastQuery>,
) -> Result<Json<Value>, ApiError> {
    let conflicts = otw_store::jobs::list_conflicts(
        &state.pool,
        journal_import::broker::MODULE,
        q.category_id,
    )
    .await?;
    Ok(Json(json!({ "conflicts": conflicts })))
}

async fn broker_answer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(answer): Json<journal_import::broker::Answer>,
) -> Result<Json<Value>, ApiError> {
    journal_import::broker::answer_conflict(&state.pool, id, &answer)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ForgetBody {
    category_id: Uuid,
    /// seed | trusted | excluded
    kind: String,
    /// A seed's id, or a symbol.
    value: String,
}

async fn broker_forget_rule(
    State(state): State<AppState>,
    Json(b): Json<ForgetBody>,
) -> Result<Json<Value>, ApiError> {
    journal_import::broker::forget_rule(&state.pool, b.category_id, &b.kind, &b.value)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "sync": store::get_broker_sync(&state.pool, b.category_id).await? })))
}

// ── Saved mappings ───────────────────────────────────────────────────────────

async fn list_mappings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mappings = store::list_mappings(&state.pool).await?;
    Ok(Json(json!({ "mappings": mappings })))
}

#[derive(Deserialize)]
struct SaveMappingBody {
    #[serde(default)]
    name: String,
    /// The file's header row, as returned by `analyze` — the fingerprint is derived from
    /// it, so saving doesn't need the file itself.
    #[serde(default)]
    headers: Vec<String>,
    mapping: Mapping,
}

/// Save (or replace) a mapping on its own, without importing anything. Saving and
/// importing are separate decisions: a mapping is worth keeping even on a run the user
/// ends up cancelling.
async fn save_mapping(
    State(state): State<AppState>,
    Json(body): Json<SaveMappingBody>,
) -> Result<Json<Value>, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("mapping name required"));
    }
    let doc = serde_json::to_value(&body.mapping)
        .map_err(|_| ApiError::internal("serializing mapping"))?;
    let normalized: Vec<String> = body
        .headers
        .iter()
        .map(|h| journal_import::parse::normalize(h))
        .filter(|h| !h.is_empty())
        .collect();
    let fingerprint = journal_import::detect::fingerprint(&body.headers);
    let headers = serde_json::to_value(&normalized)
        .map_err(|_| ApiError::internal("serializing headers"))?;
    let saved = store::upsert_mapping(&state.pool, name, &fingerprint, &headers, &doc).await?;
    Ok(Json(json!({ "mapping": saved })))
}

async fn update_mapping(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<store::MappingPatch>,
) -> Result<Json<Value>, ApiError> {
    if !store::update_mapping(&state.pool, id, &patch).await? {
        return Err(ApiError::not_found("import mapping not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_mapping(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_mapping(&state.pool, id).await? {
        return Err(ApiError::not_found("import mapping not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Batches ──────────────────────────────────────────────────────────────────

async fn list_batches(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let batches = store::list_batches(&state.pool).await?;
    Ok(Json(json!({ "batches": batches })))
}

/// Undo an import: delete the trades it created, then the batch itself.
async fn revert_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    match store::revert_batch(&state.pool, id).await? {
        Some(removed) => Ok(Json(json!({ "ok": true, "removed": removed }))),
        None => Err(ApiError::not_found("import batch not found")),
    }
}

/// Drop the batch from the history but keep its trades (they can no longer be
/// reverted in one click).
async fn forget_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::forget_batch(&state.pool, id).await? {
        return Err(ApiError::not_found("import batch not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
