//! HTTP API for uploaded files (images embedded in documents / database covers).
//!
//! POST /api/files          multipart, field "file" -> { id, url, ... }
//! GET  /api/files/{id}      streams the bytes with its stored content-type
//!
//! Bytes are written to `state.upload_dir/{id}`; metadata goes in the `files` table.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};

/// Hard cap on a single upload (25 MiB) to avoid filling the disk by accident.
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/files",
            // Raise the body limit above the default 2 MiB so real photos fit;
            // the handler still enforces MAX_UPLOAD_BYTES precisely.
            post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 1024 * 1024)),
        )
        .route("/api/files/{id}", get(download))
}

/// POST /api/files — accept a single multipart field named "file".
async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(&format!("malformed upload: {e}")))?
    {
        // Take the first file-bearing field.
        let filename = field.file_name().unwrap_or("file").to_string();
        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(&format!("reading upload: {e}")))?;

        if bytes.is_empty() {
            return Err(ApiError::bad_request("uploaded file is empty"));
        }
        if bytes.len() > MAX_UPLOAD_BYTES {
            return Err(ApiError::bad_request("file too large (max 25 MiB)"));
        }

        let id = Uuid::new_v4();
        let path = state.upload_dir.join(id.to_string());
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| anyhow::anyhow!("writing upload: {e}"))?;

        let meta = otw_store::files::record(
            &state.pool,
            id,
            &filename,
            &content_type,
            bytes.len() as i64,
        )
        .await?;

        return Ok(Json(json!({
            "id": meta.id,
            "url": format!("/api/files/{}", meta.id),
            "filename": meta.filename,
            "content_type": meta.content_type,
            "size": meta.size,
        })));
    }

    Err(ApiError::bad_request("no file field in upload"))
}

/// GET /api/files/{id} — stream the stored bytes.
async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let meta = otw_store::files::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("file not found"))?;

    let path = state.upload_dir.join(id.to_string());
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| ApiError::not_found("file bytes missing"))?;

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, meta.content_type),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
        ],
        body,
    )
        .into_response())
}
