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
    extract::{DefaultBodyLimit, Path, Query, State},
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
        .route(
            "/api/portfolios/{id}/import/broker/schedule",
            axum::routing::put(broker_schedule),
        )
        .route(
            "/api/portfolios/{id}/import/broker/conflicts/{cid}/ignore",
            post(broker_ignore),
        )
        .route(
            "/api/portfolios/{id}/import/broker/rules/forget",
            post(broker_forget_rule),
        )
}

// ── Broker holdings ──────────────────────────────────────────────────────────

type Account = (
    Box<dyn crate::brokers::Broker>,
    std::collections::HashMap<String, String>,
    otw_store::brokers::BrokerRow,
);

/// The account, its credentials, and proof it is granted here and lists its holdings.
async fn holdings_account(state: &AppState, account_id: Uuid) -> Result<Account, ApiError> {
    let (connector, settings, account) =
        crate::brokers_api::resolve(state, account_id, Some(MODULE)).await?;
    if !connector.capability().holdings {
        return Err(ApiError::bad_request(&format!(
            "{} does not list what an account holds",
            connector.capability().label
        )));
    }
    Ok((connector, settings, account))
}

/// Read an account's holdings with its own credentials. Both halves go through here, so
/// the grant is proved twice, and the commit works on the balance sheet the preview showed
/// (a reading a few minutes old at most, see `brokers::cache`).
async fn read_holdings(
    (connector, settings, account): &Account,
) -> anyhow::Result<Vec<crate::brokers::Holding>> {
    crate::brokers::cache::holdings(account.id, connector.as_ref(), settings).await
}

/// The alignment the user validated, written, and whatever a scheduled pass left waiting
/// cleared: the user just answered by hand, so each line is either settled or will be
/// asked again by the next pass.
async fn commit_with(
    pool: &sqlx::PgPool,
    id: Uuid,
    acc: &Account,
    req: &broker::CommitRequest,
) -> anyhow::Result<broker::CommitReport> {
    let rows = read_holdings(acc).await?;
    let account = &acc.2;
    let report = broker::commit(pool, id, account.id, &account.name, &rows, req).await?;
    let waiting: Vec<String> = otw_store::jobs::list_conflicts(pool, broker::MODULE, Some(id))
        .await?
        .into_iter()
        .filter(|c| c.account_id == account.id)
        .map(|c| c.symbol)
        .collect();
    otw_store::jobs::clear_conflicts(pool, broker::MODULE, id, account.id, &waiting).await?;
    Ok(report)
}

fn task_scope(id: Uuid) -> (String, String) {
    (format!("portfolios:{id}"), format!("/portfolios?id={id}&broker=1"))
}

/// What an import would do. Writes nothing. `?background=true` reads the account as a
/// task and answers with it.
async fn broker_preview(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(bg): Query<crate::tasks::Background>,
    Json(req): Json<broker::PreviewRequest>,
) -> Result<Json<Value>, ApiError> {
    let acc = holdings_account(&state, req.account_id).await?;
    if bg.background {
        let (scope, link) = task_scope(id);
        let estimate = crate::brokers::estimate::holdings(&acc.2.broker);
        let pool = state.pool.clone();
        let task = crate::tasks::spawn(
            &state,
            scope,
            "Portfolio: broker holdings".into(),
            link,
            broker::MODULE,
            estimate,
            async move {
                let rows = read_holdings(&acc).await?;
                let (_, _, account) = &acc;
                let p = broker::preview(&pool, id, &account.name, &account.broker, &rows, &req)
                    .await?;
                Ok(crate::tasks::Done {
                    summary: format!(
                        "{} holding(s) read, {} to answer before aligning.",
                        p.lines.len(),
                        p.unresolved
                    ),
                    result: json!({ "preview": p, "account_id": account.id }),
                })
            },
        )
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        return Ok(Json(json!({ "task": task })));
    }
    let rows = read_holdings(&acc)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let (_, _, account) = &acc;
    let preview =
        broker::preview(&state.pool, id, &account.name, &account.broker, &rows, &req).await?;
    Ok(Json(json!(preview)))
}

/// Write the alignment the user validated.
async fn broker_commit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(bg): Query<crate::tasks::Background>,
    Json(req): Json<broker::CommitRequest>,
) -> Result<Json<Value>, ApiError> {
    let acc = holdings_account(&state, req.account_id).await?;
    if bg.background {
        let (scope, link) = task_scope(id);
        let estimate = crate::brokers::estimate::holdings(&acc.2.broker);
        let pool = state.pool.clone();
        let task = crate::tasks::spawn(
            &state,
            scope,
            "Portfolio: broker alignment".into(),
            link,
            broker::MODULE,
            estimate,
            async move {
                let report = commit_with(&pool, id, &acc, &req).await?;
                Ok(crate::tasks::Done {
                    summary: format!("{} operation(s) written.", report.imported),
                    result: json!({ "report": report, "account_id": acc.2.id }),
                })
            },
        )
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        return Ok(Json(json!({ "task": task })));
    }
    let report = commit_with(&state.pool, id, &acc, &req)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
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

/// The account this portfolio was last aligned with, so the modal reopens on it, with its
/// schedule and the holdings a scheduled pass left waiting.
async fn broker_last(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let conflicts =
        otw_store::jobs::list_conflicts(&state.pool, broker::MODULE, Some(id)).await?;
    Ok(Json(json!({
        "last": store::get_broker_sync(&state.pool, id).await?,
        "conflicts": conflicts,
    })))
}

#[derive(Deserialize)]
struct ScheduleBody {
    enabled: bool,
    #[serde(default)]
    paused: bool,
    #[serde(default = "default_interval")]
    interval_minutes: i32,
}

fn default_interval() -> i32 {
    1440
}

/// Schedule the alignment with the account this portfolio was last aligned with. Only
/// after one alignment by hand: that is where the user sees what it writes.
async fn broker_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ScheduleBody>,
) -> Result<Json<Value>, ApiError> {
    if b.enabled {
        crate::jobs::check_interval(b.interval_minutes)
            .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    }
    let sync = store::get_broker_sync(&state.pool, id)
        .await?
        .filter(|s| s.account_id.is_some() && s.last_synced_at.is_some())
        .ok_or_else(|| ApiError::bad_request("align once by hand, then schedule it"))?;
    store::set_broker_schedule(
        &state.pool,
        id,
        b.enabled,
        b.paused,
        if b.enabled { b.interval_minutes } else { sync.interval_minutes },
    )
    .await?;
    Ok(Json(json!({ "last": store::get_broker_sync(&state.pool, id).await? })))
}

/// Leave a waiting holding out of every later scheduled pass.
async fn broker_ignore(
    State(state): State<AppState>,
    Path((id, cid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let conflict = otw_store::jobs::get_conflict(&state.pool, cid)
        .await?
        .filter(|c| c.module == broker::MODULE && c.target_id == id)
        .ok_or_else(|| ApiError::not_found("conflict not found"))?;
    let sync = store::get_broker_sync(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("this portfolio has no broker sync"))?;
    let mut excluded: Vec<String> =
        serde_json::from_value(sync.excluded.clone()).unwrap_or_default();
    let sym = conflict.symbol.trim().to_uppercase();
    if !excluded.contains(&sym) {
        excluded.push(sym);
    }
    store::set_broker_excluded(&state.pool, id, &json!(excluded)).await?;
    otw_store::jobs::delete_conflict(&state.pool, cid).await?;
    Ok(Json(json!({ "last": store::get_broker_sync(&state.pool, id).await? })))
}

#[derive(Deserialize)]
struct ForgetBody {
    symbol: String,
}

/// Bring a left-out symbol back into the scheduled passes.
async fn broker_forget_rule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ForgetBody>,
) -> Result<Json<Value>, ApiError> {
    let sync = store::get_broker_sync(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("this portfolio has no broker sync"))?;
    let sym = b.symbol.trim().to_uppercase();
    let excluded: Vec<String> = serde_json::from_value::<Vec<String>>(sync.excluded.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| *s != sym)
        .collect();
    store::set_broker_excluded(&state.pool, id, &json!(excluded)).await?;
    Ok(Json(json!({ "last": store::get_broker_sync(&state.pool, id).await? })))
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
