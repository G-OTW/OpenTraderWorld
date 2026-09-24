//! HTTP API for importing an OHLCV series from a file.
//!
//! `analyze` is the whole read-only half: it reads the uploaded file, proposes (or
//! applies) a mapping, and returns the bars that mapping would write together with what
//! is wrong with them, so the mapping step and the preview run on exactly what `commit`
//! will store. Nothing is written until `commit`.
//!
//! The file rides in the request body as base64 on every call (analyze is re-run as the
//! user edits the mapping). No server-side upload state, no temp file, no cleanup job.

use axum::{
    extract::{DefaultBodyLimit, State},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::histdata_import::{self, Destination, Mapping};
use crate::import::parse;
use crate::{ApiError, AppState};

/// Base64 inflates by 4/3; 20 MB of CSV is roughly a million daily bars.
const MAX_BODY: usize = 30 * 1024 * 1024;

/// A file the user picked is their problem to fix, not a server fault, and the whole
/// anyhow chain ("reading the file: invalid delimiter") is what makes it fixable.
fn bad_file(e: anyhow::Error) -> ApiError {
    ApiError::bad_request(&format!("{e:#}"))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/histdata/import/analyze",
            post(analyze).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route(
            "/api/histdata/import/commit",
            post(commit).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
}

#[derive(Deserialize)]
struct AnalyzeBody {
    #[serde(default)]
    filename: String,
    /// The file as base64 (a `data:` URI is accepted as-is).
    #[serde(default)]
    content: String,
    /// Omit to auto-detect; send the edited mapping to re-preview with it.
    mapping: Option<Mapping>,
    /// The instrument the bars are of. Incomplete is fine here: analyze reports what is
    /// missing rather than refusing, so the preview works while the form is being filled.
    #[serde(default)]
    destination: Destination,
    /// Which table of a stacked export to detect against ("" = read the file flat).
    /// Only meaningful when `mapping` is omitted.
    section: Option<String>,
    /// How many built bars to return (the stats always cover the whole file).
    limit: Option<usize>,
}

async fn analyze(
    State(state): State<AppState>,
    Json(body): Json<AnalyzeBody>,
) -> Result<Json<Value>, ApiError> {
    let bytes = parse::decode_upload(&body.content).map_err(bad_file)?;
    let limit = body.limit.unwrap_or(60).clamp(1, 500);
    let analysis = histdata_import::analyze(
        &state.pool,
        &body.filename,
        &bytes,
        body.mapping,
        body.destination,
        body.section,
        limit,
    )
    .await
    .map_err(bad_file)?;
    Ok(Json(json!({ "analysis": analysis })))
}

#[derive(Deserialize)]
struct CommitBody {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    content: String,
    mapping: Mapping,
    destination: Destination,
}

async fn commit(
    State(state): State<AppState>,
    Json(body): Json<CommitBody>,
) -> Result<Json<Value>, ApiError> {
    let bytes = parse::decode_upload(&body.content).map_err(bad_file)?;
    let report = histdata_import::commit(
        &state.pool,
        &body.filename,
        &bytes,
        body.mapping,
        body.destination,
    )
    .await
    .map_err(bad_file)?;
    Ok(Json(json!({ "report": report })))
}
