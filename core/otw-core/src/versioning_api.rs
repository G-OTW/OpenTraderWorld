//! Versioning of editor documents and saved backtest strategies.
//!
//! Two switches gate it: a global one per area (Settings), then one per document or
//! strategy. Taking or restoring a version needs both; reading and deleting history never
//! does, so a user who switched versioning off can still clean up what is left. Deleting a
//! document takes its versions with it (`documents.rs`); deleting a strategy may keep them
//! (`backtest_api.rs`, `?versions=keep`), listed as deleted strategies.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use otw_store::versions;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};

/// Longest note a version may carry.
const NOTE_MAX: usize = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/versioning", get(get_settings).post(set_settings))
        .route("/api/settings/versioning/editor", get(get_editor_area).post(set_editor_area))
        .route(
            "/api/settings/versioning/strategies",
            get(get_strategies_area).post(set_strategies_area),
        )
        // Documents (pages and databases).
        .route("/api/documents/{id}/versioning", post(toggle_doc))
        .route("/api/documents/{id}/versions", get(list_doc).post(create_doc).delete(purge_doc))
        .route("/api/documents/{id}/versions/{vid}", get(get_doc).delete(delete_doc))
        .route("/api/documents/{id}/versions/{vid}/restore", post(restore_doc))
        // Saved strategies.
        .route("/api/backtest/strategies/deleted-versions", get(deleted_strategies))
        .route("/api/backtest/strategies/{id}/versioning", post(toggle_strategy))
        .route(
            "/api/backtest/strategies/{id}/versions",
            get(list_strategy).post(create_strategy).delete(purge_strategy),
        )
        .route(
            "/api/backtest/strategies/{id}/versions/{vid}",
            get(get_strategy).delete(delete_strategy),
        )
        .route("/api/backtest/strategies/{id}/versions/{vid}/restore", post(restore_strategy))
}

/// `?versions=keep|delete` on a strategy delete. Anything but `keep` drops the
/// history with the item, so a caller unaware of versioning never leaves orphans behind.
#[derive(Deserialize, Default, schemars::JsonSchema)]
pub(crate) struct DeleteQuery {
    /// `keep` leaves the version history behind (restorable from the deleted list);
    /// anything else, or nothing, deletes it with the item.
    #[serde(default)]
    versions: Option<String>,
}

impl DeleteQuery {
    pub(crate) fn keep(&self) -> bool {
        self.versions.as_deref() == Some("keep")
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let pool = &state.pool;
    Ok(Json(json!({
        "editor": versions::enabled(pool, versions::EDITOR_KEY).await?,
        "strategies": versions::enabled(pool, versions::STRATEGIES_KEY).await?,
        "usage": {
            "editor": versions::editor_usage(pool).await?,
            "strategies": versions::strategies_usage(pool).await?,
        },
    })))
}

#[derive(Deserialize)]
struct SettingsBody {
    /// `editor` or `strategies`.
    scope: String,
    enabled: bool,
    /// When switching off: also delete every version of that area.
    #[serde(default)]
    purge: bool,
}

async fn set_settings(
    State(state): State<AppState>,
    Json(body): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    let key = match body.scope.as_str() {
        "editor" => versions::EDITOR_KEY,
        "strategies" => versions::STRATEGIES_KEY,
        _ => return Err(ApiError::bad_request("scope must be 'editor' or 'strategies'")),
    };
    otw_store::settings::set(&state.pool, key, if body.enabled { "1" } else { "0" }).await?;
    if !body.enabled && body.purge {
        match body.scope.as_str() {
            "editor" => versions::purge_all_docs(&state.pool).await?,
            _ => versions::purge_all_strategies(&state.pool).await?,
        }
    }
    get_settings(State(state)).await
}

// One route per area, so a caller (MCP) is gated by that area's module alone. Switching
// off keeps every version (hidden until switched back on); deleting a whole area's history
// stays on the Settings route above.

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AreaBody {
    enabled: bool,
}

async fn area(state: &AppState, key: &str) -> Result<Json<Value>, ApiError> {
    let usage = if key == versions::EDITOR_KEY {
        versions::editor_usage(&state.pool).await?
    } else {
        versions::strategies_usage(&state.pool).await?
    };
    Ok(Json(json!({ "enabled": versions::enabled(&state.pool, key).await?, "usage": usage })))
}

async fn set_area(state: &AppState, key: &str, body: AreaBody) -> Result<Json<Value>, ApiError> {
    otw_store::settings::set(&state.pool, key, if body.enabled { "1" } else { "0" }).await?;
    area(state, key).await
}

async fn get_editor_area(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    area(&state, versions::EDITOR_KEY).await
}

async fn set_editor_area(
    State(state): State<AppState>,
    Json(body): Json<AreaBody>,
) -> Result<Json<Value>, ApiError> {
    set_area(&state, versions::EDITOR_KEY, body).await
}

async fn get_strategies_area(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    area(&state, versions::STRATEGIES_KEY).await
}

async fn set_strategies_area(
    State(state): State<AppState>,
    Json(body): Json<AreaBody>,
) -> Result<Json<Value>, ApiError> {
    set_area(&state, versions::STRATEGIES_KEY, body).await
}

// ── Shared bodies ────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct NoteBody {
    /// What this version is, like a commit message. Optional, at most 500 characters.
    #[serde(default)]
    note: String,
}

fn clean_note(note: &str) -> Result<String, ApiError> {
    let note = note.trim();
    if note.chars().count() > NOTE_MAX {
        return Err(ApiError::bad_request(&format!(
            "a version note is at most {NOTE_MAX} characters"
        )));
    }
    Ok(note.to_string())
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ToggleBody {
    enabled: bool,
    /// When switching off: also delete this item's versions.
    #[serde(default)]
    purge: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RestoreBody {
    /// Note put on the version the restore creates (the restored state).
    #[serde(default)]
    note: String,
}

async fn require_on(state: &AppState, key: &str, area: &str) -> Result<(), ApiError> {
    if !versions::enabled(&state.pool, key).await? {
        return Err(ApiError::bad_request(&format!(
            "{area} versioning is off: switch it on in Settings → Versioning"
        )));
    }
    Ok(())
}

// ── Documents ────────────────────────────────────────────────────────────────

async fn toggle_doc(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ToggleBody>,
) -> Result<Json<Value>, ApiError> {
    if body.enabled {
        require_on(&state, versions::EDITOR_KEY, "Editor").await?;
    }
    if !versions::set_doc_versioned(&state.pool, id, body.enabled).await? {
        return Err(ApiError::not_found("page or database not found"));
    }
    if !body.enabled && body.purge {
        versions::purge_doc_versions(&state.pool, id).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn list_doc(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "versions": versions::doc_versions(&state.pool, id).await?,
        "current": versions::current_doc_version(&state.pool, id).await?,
    })))
}

async fn create_doc(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<NoteBody>,
) -> Result<Json<Value>, ApiError> {
    require_on(&state, versions::EDITOR_KEY, "Editor").await?;
    let note = clean_note(&body.note)?;
    let doc = otw_store::documents::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    if doc.kind == "folder" {
        return Err(ApiError::bad_request("folders are not versioned"));
    }
    if !doc.versioned {
        return Err(ApiError::bad_request(
            "versioning is off for this document: switch it on from its versions menu",
        ));
    }
    let v = versions::snapshot_doc(&state.pool, id, &note)
        .await?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    Ok(Json(json!({ "version": v })))
}

async fn purge_doc(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let n = versions::purge_doc_versions(&state.pool, id).await?;
    Ok(Json(json!({ "deleted": n })))
}

async fn get_doc(
    State(state): State<AppState>,
    Path((id, vid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let v = versions::get_doc_version(&state.pool, id, vid)
        .await?
        .ok_or_else(|| ApiError::not_found("version not found"))?;
    Ok(Json(json!({ "version": v })))
}

async fn delete_doc(
    State(state): State<AppState>,
    Path((id, vid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    if !versions::delete_doc_version(&state.pool, id, vid).await? {
        return Err(ApiError::not_found("version not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn restore_doc(
    State(state): State<AppState>,
    Path((id, vid)): Path<(Uuid, Uuid)>,
    Json(body): Json<RestoreBody>,
) -> Result<Json<Value>, ApiError> {
    require_on(&state, versions::EDITOR_KEY, "Editor").await?;
    let note = clean_note(&body.note)?;
    versions::restore_doc_version(&state.pool, id, vid, &note)
        .await
        .map_err(|e| ApiError::bad_request(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("version not found"))?;
    Ok(Json(json!({ "ok": true })))
}

// ── Strategies ───────────────────────────────────────────────────────────────

async fn deleted_strategies(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "items": versions::deleted_strategies(&state.pool).await? })))
}

async fn toggle_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ToggleBody>,
) -> Result<Json<Value>, ApiError> {
    if body.enabled {
        require_on(&state, versions::STRATEGIES_KEY, "Strategy").await?;
    }
    if !versions::set_strategy_versioned(&state.pool, id, body.enabled).await? {
        return Err(ApiError::not_found("strategy not found"));
    }
    if !body.enabled && body.purge {
        versions::purge_strategy_versions(&state.pool, id).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn list_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "versions": versions::strategy_versions(&state.pool, id).await?,
        "current": versions::current_strategy_version(&state.pool, id).await?,
    })))
}

async fn create_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<NoteBody>,
) -> Result<Json<Value>, ApiError> {
    require_on(&state, versions::STRATEGIES_KEY, "Strategy").await?;
    let note = clean_note(&body.note)?;
    let s = otw_store::backtest::get_strategy(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("strategy not found"))?;
    if !s.versioned {
        return Err(ApiError::bad_request(
            "versioning is off for this strategy: switch it on from its versions menu",
        ));
    }
    let v = versions::snapshot_strategy(&state.pool, id, &note)
        .await?
        .ok_or_else(|| ApiError::not_found("strategy not found"))?;
    Ok(Json(json!({ "version": v })))
}

async fn purge_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let n = versions::purge_strategy_versions(&state.pool, id).await?;
    Ok(Json(json!({ "deleted": n })))
}

async fn get_strategy(
    State(state): State<AppState>,
    Path((id, vid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let v = versions::get_strategy_version(&state.pool, id, vid)
        .await?
        .ok_or_else(|| ApiError::not_found("version not found"))?;
    Ok(Json(json!({ "version": v })))
}

async fn delete_strategy(
    State(state): State<AppState>,
    Path((id, vid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    if !versions::delete_strategy_version(&state.pool, id, vid).await? {
        return Err(ApiError::not_found("version not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn restore_strategy(
    State(state): State<AppState>,
    Path((id, vid)): Path<(Uuid, Uuid)>,
    Json(body): Json<RestoreBody>,
) -> Result<Json<Value>, ApiError> {
    require_on(&state, versions::STRATEGIES_KEY, "Strategy").await?;
    let note = clean_note(&body.note)?;
    versions::restore_strategy_version(&state.pool, id, vid, &note)
        .await?
        .ok_or_else(|| ApiError::not_found("version not found"))?;
    Ok(Json(json!({ "ok": true })))
}
