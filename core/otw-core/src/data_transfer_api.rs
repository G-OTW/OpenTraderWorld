//! Download and load your data, per module.
//!
//! The whole-database path (pg_dump / psql, Settings → Backup & restore, Full) stays where it is: it is the
//! disaster path, run on the host with core stopped. These two routes are the portability
//! path, for moving a journal to another instance or putting last year's portfolios back
//! without touching anything else.
//!
//! Loading is the dangerous direction, so it is guarded in three ways: one transaction (a
//! failure leaves the database untouched), an automatic safety download taken before any
//! Replace, and a refusal to read a bundle written by a newer app version.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use otw_store::data_transfer::{self, ImportMode, MAX_BUNDLE_BYTES};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/data/export", get(export))
        .route(
            "/api/settings/data/import",
            // The bundle is read whole; the handler enforces the real ceiling precisely.
            post(import).layer(DefaultBodyLimit::max(MAX_BUNDLE_BYTES + 1024 * 1024)),
        )
        .route(
            "/api/settings/data/preview",
            post(preview).layer(DefaultBodyLimit::max(MAX_BUNDLE_BYTES + 1024 * 1024)),
        )
}

#[derive(Deserialize)]
struct ExportQuery {
    /// Comma-separated module ids. Empty or absent exports every module that owns data.
    #[serde(default)]
    modules: String,
    /// Include the sealed credential stores. Off unless asked: they are encrypted with this
    /// instance's `OTW_SECRET_KEY` and are unreadable anywhere else.
    #[serde(default)]
    credentials: bool,
}

fn wanted_modules(raw: &str) -> Vec<String> {
    let ids: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.is_empty() {
        otw_store::data_admin::known_module_ids()
    } else {
        ids
    }
}

/// GET /api/settings/data/export — a zip of the selected modules.
async fn export(
    State(state): State<AppState>,
    jar: axum_extra::extract::cookie::CookieJar,
    Query(q): Query<ExportQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // A plain bundle is the user's own data. A bundle *with* the sealed credential stores is
    // every API key, vault item, MCP token and channel secret this instance holds, in one
    // GET. The ciphertext is useless without OTW_SECRET_KEY, but a stolen session should not
    // be able to walk off with it on its own, so this half asks for the password again.
    if q.credentials {
        crate::security::require_step_up(&jar)?;
    }
    let modules = wanted_modules(&q.modules);
    if q.credentials {
        crate::security_events::alert(
            &state,
            crate::security_events::EXPORT_CREDENTIALS,
            crate::security_events::EXPORT_CREDENTIALS,
            "A data export included the sealed credentials",
            &format!(
                "Modules: {}\n\nThe bundle carries every API key, vault item, agent token and \
                 channel secret for those modules. The ciphertext is useless without this \
                 instance's OTW_SECRET_KEY, so keep the two apart.",
                modules.join(", ")
            ),
        );
    }
    let bundle = data_transfer::export_bundle(
        &state.pool,
        &modules,
        q.credentials,
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .map_err(|e| ApiError::bad_request(&e.to_string()))?;

    let filename = format!("otw-data-{}.zip", today());
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bundle,
    ))
}

/// The upload as sent: the zip plus the two optional text fields. Shared by the load and
/// its preview so the two never disagree about what was uploaded.
struct Upload {
    bundle: Vec<u8>,
    modules_raw: String,
    mode_raw: String,
}

async fn read_upload(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let mut bundle: Option<Vec<u8>> = None;
    let mut modules_raw = String::new();
    let mut mode_raw = String::from("merge");

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(&format!("malformed upload: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::bad_request(&format!("reading the data file: {e}")))?;
                if bytes.len() > MAX_BUNDLE_BYTES {
                    return Err(ApiError::bad_request(
                        "this data file is too large to load through the browser. Restore the full database backup instead (Settings → Backup & restore).",
                    ));
                }
                bundle = Some(bytes.to_vec());
            }
            "modules" => modules_raw = field.text().await.unwrap_or_default(),
            "mode" => mode_raw = field.text().await.unwrap_or_default(),
            _ => {}
        }
    }

    let Some(bundle) = bundle.filter(|b| !b.is_empty()) else {
        return Err(ApiError::bad_request("no data file was sent"));
    };
    Ok(Upload { bundle, modules_raw, mode_raw })
}

/// POST /api/settings/data/preview — multipart: `file` (the zip). Reads nothing but the
/// manifest and counts what is here, so the user sees what a load would meet before
/// choosing merge or replace. Changes nothing.
async fn preview(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let up = read_upload(multipart).await?;
    let preview =
        data_transfer::preview_bundle(&state.pool, &up.bundle, env!("CARGO_PKG_VERSION"))
            .await
            .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    Ok(Json(serde_json::to_value(preview).unwrap_or(Value::Null)))
}

/// POST /api/settings/data/import — multipart: `file` (the zip), `modules` (csv, optional),
/// `mode` (`replace` | `merge`).
///
/// With no `modules` field, every module the bundle carries is loaded.
async fn import(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let Upload { bundle, modules_raw, mode_raw } = read_upload(multipart).await?;
    let mode = ImportMode::parse(mode_raw.trim())
        .ok_or_else(|| ApiError::bad_request("mode must be \"replace\" or \"merge\""))?;

    let manifest = data_transfer::read_manifest(&bundle)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    // No selection = everything the file carries.
    let modules: Vec<String> = if modules_raw.trim().is_empty() {
        manifest.modules.iter().map(|m| m.id.clone()).collect()
    } else {
        wanted_modules(&modules_raw)
            .into_iter()
            .filter(|id| manifest.modules.iter().any(|m| &m.id == id))
            .collect()
    };
    if modules.is_empty() {
        return Err(ApiError::bad_request(
            "none of the selected modules are in this data file",
        ));
    }

    // Safety net: before overwriting anything, write a bundle of the modules about to be
    // replaced next to the uploads. Merge cannot destroy data, so it skips this.
    let mut safety_file: Option<String> = None;
    if mode == ImportMode::Replace {
        let before = data_transfer::export_bundle(
            &state.pool,
            &modules,
            true,
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(|e| ApiError::internal(&format!("preparing the safety copy: {e}")))?;
        let name = format!("otw-before-load-{}.zip", stamp());
        let path = state.upload_dir.join(&name);
        tokio::fs::write(&path, &before)
            .await
            .map_err(|e| ApiError::internal(&format!("writing the safety copy: {e}")))?;
        tracing::warn!("replacing modules {:?}; safety copy at {}", modules, path.display());
        safety_file = Some(name);
    }

    let report = data_transfer::import_bundle(
        &state.pool,
        &bundle,
        &modules,
        mode,
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .map_err(|e| ApiError::bad_request(&e.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "mode": mode_raw,
        "modules": report,
        "safety_file": safety_file,
        "from_version": manifest.app_version,
    })))
}

fn today() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}-{:02}-{:02}", now.year(), now.month() as u8, now.day())
}

fn stamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}
