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
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::journal_import::{self, Mapping};
use crate::{ApiError, AppState};
use otw_store::journal_import as store;

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
