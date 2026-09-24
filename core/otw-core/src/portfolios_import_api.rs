//! HTTP API for the Portfolio Tracker's operations-ledger import.
//!
//! `analyze` is the whole read-only half: it reads the uploaded file, proposes (or
//! applies) a mapping, resolves each symbol against the portfolio's assets, and returns
//! the operations that mapping would create — so the mapping step and the preview run on
//! exactly what `commit` will write. Nothing is stored until `commit`, and a commit can
//! be reverted in one call.
//!
//! The file rides in the request body as base64 on every call (analyze is re-run as the
//! user edits the mapping). No server-side upload state, no temp files, no cleanup job.
//!
//! `import/broker/*` is the same two halves over a broker account instead of a file: the
//! preview puts the account's holdings next to the ledger, the commit writes the operations
//! that align them. The account is read again at commit time, so what lands in the ledger
//! is the broker's number and not one that travelled through a form.

use axum::{
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::import::parse;
use crate::portfolios_import::{self, broker, Mapping};
use crate::{ApiError, AppState};
use otw_store::portfolios_import as store;

/// The module id a broker account must be granted to for this import to use it.
const MODULE: &str = "portfolios";

/// Base64 inflates by 4/3; 20 MB of CSV is a very large ledger.
const MAX_BODY: usize = 30 * 1024 * 1024;

/// A file the user picked is their problem to fix, not a server fault — and the whole
/// anyhow chain ("reading the file: invalid delimiter") is what makes it fixable.
fn bad_file(e: anyhow::Error) -> ApiError {
    ApiError::bad_request(&format!("{e:#}"))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/portfolios/{id}/import/analyze",
            post(analyze).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route(
            "/api/portfolios/{id}/import/commit",
            post(commit).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route("/api/portfolios/{id}/import/batches", get(list_batches))
        .route("/api/portfolios/import/mappings", get(list_mappings).post(save_mapping))
        .route(
            "/api/portfolios/import/mappings/{id}",
            axum::routing::patch(update_mapping).delete(delete_mapping),
        )
        .route(
            "/api/portfolios/import/batches/{id}/revert",
            post(revert_batch),
        )
        .route(
            "/api/portfolios/import/batches/{id}",
            axum::routing::delete(forget_batch),
        )
        .route(
            "/api/portfolios/{id}/import/broker/preview",
            post(broker_preview),
        )
        .route(
            "/api/portfolios/{id}/import/broker/commit",
            post(broker_commit),
        )
        .route(
            "/api/portfolios/{id}/import/broker/prices",
            post(broker_prices),
        )
        .route("/api/portfolios/{id}/import/broker/last", get(broker_last))
}

// ── Broker holdings ──────────────────────────────────────────────────────────

/// Read an account's holdings with its own credentials. Both halves go through here, so
/// the grant is proved twice and the commit works on a fresh balance sheet.
async fn holdings_of(
    state: &AppState,
    account_id: Uuid,
) -> Result<(Vec<crate::brokers::Holding>, otw_store::brokers::BrokerRow), ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(state, account_id, Some(MODULE)).await?;
    if !connector.capability().holdings {
        return Err(ApiError::bad_request(&format!(
            "{} does not list what an account holds",
            connector.capability().label
        )));
    }
    let http = crate::brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let rows = connector
        .holdings(&http, &settings)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok((rows, account))
}

/// What an import would do. Writes nothing.
async fn broker_preview(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<broker::PreviewRequest>,
) -> Result<Json<Value>, ApiError> {
    let (rows, account) = holdings_of(&state, req.account_id).await?;
    let preview =
        broker::preview(&state.pool, id, &account.name, &account.broker, &rows, &req).await?;
    Ok(Json(json!(preview)))
}

/// Write the alignment the user validated.
async fn broker_commit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<broker::CommitRequest>,
) -> Result<Json<Value>, ApiError> {
    let (rows, account) = holdings_of(&state, req.account_id).await?;
    let report = broker::commit(&state.pool, id, account.id, &account.name, &rows, &req).await?;
    Ok(Json(json!(report)))
}

/// Which assets to price, in the broker's own spelling.
#[derive(Deserialize)]
struct PricesBody {
    account_id: Uuid,
    #[serde(default)]
    symbols: Vec<String>,
}

/// What the venue trades these assets at right now, read with the account's own
/// credentials. Writes nothing and decides nothing: the modal puts the number in the
/// field, where it is still the user's to accept or overwrite.
///
/// An exchange prices BTC in USDT, not in dollars, so each line carries the money it was
/// quoted in and the market that answered. Converting that into the asset's currency is
/// not this endpoint's job and never silently happens.
async fn broker_prices(
    State(state): State<AppState>,
    Path(_id): Path<Uuid>,
    Json(body): Json<PricesBody>,
) -> Result<Json<Value>, ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(&state, body.account_id, Some(MODULE)).await?;
    if !connector.capability().quotes {
        return Err(ApiError::bad_request(&format!(
            "{} does not publish a live price. Enter the price yourself.",
            connector.capability().label
        )));
    }
    let http = crate::brokers::client().map_err(|e| ApiError::internal(&format!("{e:#}")))?;
    let quotes = connector
        .quotes(&http, &settings, &body.symbols)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(
        json!({ "account": account.name, "quotes": quotes }),
    ))
}

/// The account this portfolio was last aligned with, so the modal reopens on it.
async fn broker_last(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({ "last": store::get_broker_sync(&state.pool, id).await? }),
    ))
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
    /// How many built operations to return (the stats always cover the whole file).
    limit: Option<usize>,
}

async fn analyze(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AnalyzeBody>,
) -> Result<Json<Value>, ApiError> {
    let bytes = parse::decode_upload(&body.content).map_err(bad_file)?;
    let limit = body.limit.unwrap_or(60).clamp(1, 500);
    let analysis = portfolios_import::analyze(
        &state.pool,
        id,
        &body.filename,
        &bytes,
        body.mapping,
        body.section,
        limit,
    )
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
    Path(id): Path<Uuid>,
    Json(body): Json<CommitBody>,
) -> Result<Json<Value>, ApiError> {
    let bytes = parse::decode_upload(&body.content).map_err(bad_file)?;
    let report = portfolios_import::commit(
        &state.pool,
        id,
        &body.filename,
        &bytes,
        body.mapping,
        body.save_as,
    )
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
        .map(|h| parse::normalize(h))
        .filter(|h| !h.is_empty())
        .collect();
    let fingerprint = crate::import::detect::fingerprint(&body.headers);
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

/// The import history of one portfolio.
async fn list_batches(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let batches = store::list_batches(&state.pool, id).await?;
    Ok(Json(json!({ "batches": batches })))
}

/// Undo an import: delete the operations it created, then the batch itself.
async fn revert_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    match store::revert_batch(&state.pool, id).await? {
        Some(removed) => Ok(Json(json!({ "ok": true, "removed": removed }))),
        None => Err(ApiError::not_found("import batch not found")),
    }
}

/// Drop the batch from the history but keep its operations (they can no longer be
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
