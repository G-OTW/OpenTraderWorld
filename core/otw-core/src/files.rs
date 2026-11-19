//! HTTP API for uploaded files (images embedded in documents / database covers).
//!
//! POST /api/files          multipart, field "file" -> { id, url, ... }
//! GET  /api/files/{id}      streams the bytes, typed from the bytes themselves
//!
//! Bytes are written to `state.upload_dir/{id}`; metadata goes in the `files` table.
//!
//! Security: these bytes are served from the app's own origin, so the content type they
//! come back with decides whether an upload can execute as us. The uploader's declared
//! type is therefore never echoed. Instead the stored bytes are sniffed at read time
//! ([`link_preview::sniff`]):
//!   * a real raster image → served `inline` with the *sniffed* type, so document embeds
//!     and thumbnails keep working, including rows stored before this rule (a legacy
//!     mislabelled image still renders, because the bytes decide, not the old column);
//!   * anything else (PDF, CSV, and any HTML/SVG smuggled in as `image/png`) →
//!     `application/octet-stream` + `Content-Disposition: attachment`, which no browser
//!     will render in-origin.
//! `X-Content-Type-Options: nosniff` rides on both so the browser cannot second-guess the
//! octet-stream back into markup. Uploads of any type are still accepted — a database
//! "file" cell takes arbitrary attachments — they simply can never be *rendered* here.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use crate::{link_preview, ApiError, AppState};

/// Hard cap on a single upload (25 MiB) to avoid filling the disk by accident.
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

/// Enough bytes for every magic number [`link_preview::sniff`] tests.
const SNIFF_BYTES: usize = 32;

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
        let filename = safe_filename(field.file_name().unwrap_or("file"));
        let declared = field
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

        // Record what the bytes actually are when they are an image, so the stored column
        // agrees with what `download` will serve. A non-image keeps its declared type
        // (a PDF cell should still read "application/pdf" in the UI) but is sanitised
        // first — the value is a header token, and a raw one can carry CR/LF.
        let content_type = match link_preview::sniff(&bytes) {
            Some(sniffed) => sniffed.to_string(),
            None => safe_content_type(&declared),
        };

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

/// GET /api/files/{id} — stream the stored bytes, typed from the bytes.
async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let meta = otw_store::files::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("file not found"))?;

    let path = state.upload_dir.join(id.to_string());
    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| ApiError::not_found("file bytes missing"))?;

    // Read the head to identify the payload, then rewind and stream the whole file.
    let mut head = [0u8; SNIFF_BYTES];
    let read = file.read(&mut head).await.unwrap_or(0);
    file.rewind()
        .await
        .map_err(|e| anyhow::anyhow!("rewinding upload: {e}"))?;

    // Only a byte-sniffed raster image is allowed to render in our origin. Everything
    // else is handed over as an opaque download, whatever the database says it is.
    let (content_type, disposition) = match link_preview::sniff(&head[..read]) {
        Some(image) => (image.to_string(), format!("inline; {}", filename_param(&meta.filename))),
        None => (
            "application/octet-stream".to_string(),
            format!("attachment; {}", filename_param(&meta.filename)),
        ),
    };

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
        ],
        body,
    )
        .into_response())
}

// ── Header hygiene ───────────────────────────────────────────────────────────

/// Strip path separators and control characters from an uploaded filename. The name is
/// only ever metadata here (bytes are stored under the uuid), but it is echoed in
/// `Content-Disposition`, where a CR/LF would make the header unbuildable.
fn safe_filename(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '"'))
        .take(200)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() { "file".to_string() } else { cleaned }
}

/// A `filename="…"` parameter that is always a valid header value: non-ASCII is dropped
/// from the quoted form and carried by the RFC 5987 `filename*` form instead, so accented
/// names survive in every browser that reads it.
fn filename_param(name: &str) -> String {
    let name = safe_filename(name);
    let ascii: String = name
        .chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' })
        .collect();
    format!("filename=\"{ascii}\"; filename*=UTF-8''{}", percent_encode(&name))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Reduce a caller-declared content type to a safe `type/subtype` token, or
/// `application/octet-stream` when it is not one. Never used for rendering — only so the
/// stored column and the API response stay clean.
fn safe_content_type(raw: &str) -> String {
    let base = raw.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let shaped = base.split_once('/').is_some_and(|(t, s)| {
        !t.is_empty()
            && !s.is_empty()
            && base
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'+' | b'-'))
    });
    if shaped && base.len() <= 100 { base } else { "application/octet-stream".to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_cannot_break_a_header() {
        assert_eq!(safe_filename("a\r\nb.png"), "ab.png");
        assert_eq!(safe_filename("../../etc/passwd"), "....etcpasswd");
        assert_eq!(safe_filename("   "), "file");
        assert!(!filename_param("re\r\nport.pdf").contains('\r'));
    }

    #[test]
    fn declared_content_types_are_reduced_to_a_token() {
        assert_eq!(safe_content_type("application/pdf"), "application/pdf");
        assert_eq!(safe_content_type("IMAGE/PNG; charset=x"), "image/png");
        assert_eq!(safe_content_type("text/html\r\nX-Evil: 1"), "application/octet-stream");
        assert_eq!(safe_content_type("nonsense"), "application/octet-stream");
    }

    #[test]
    fn only_real_images_are_served_inline() {
        // The download path keys off sniff(), never off the stored/declared type.
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        assert_eq!(link_preview::sniff(&png), Some("image/png"));
        assert_eq!(link_preview::sniff(b"<svg onload=alert(1)>"), None);
        assert_eq!(link_preview::sniff(b"<html><script>alert(1)</script>"), None);
    }
}
