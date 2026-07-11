//! HTTP API for the editor module's document tree (folders + pages). Single-user.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::types::JsonValue;
use uuid::Uuid;

use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/documents", get(list).post(create))
        .route(
            "/api/documents/{id}",
            get(get_one).patch(update).delete(delete),
        )
        .route("/api/documents/{id}/move", post(move_doc))
}

/// GET /api/documents — the whole tree (metadata only).
async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let docs = otw_store::documents::list_tree(&state.pool).await?;
    Ok(Json(json!({ "documents": docs })))
}

#[derive(Deserialize)]
struct CreateBody {
    parent_id: Option<Uuid>,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    title: String,
}
fn default_kind() -> String {
    "page".to_string()
}

/// POST /api/documents — create a folder or page.
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(body.kind.as_str(), "page" | "folder" | "database") {
        return Err(ApiError::bad_request(
            "kind must be 'page', 'folder' or 'database'",
        ));
    }
    let doc = otw_store::documents::create(&state.pool, body.parent_id, &body.kind, &body.title)
        .await?;
    Ok(Json(json!({ "document": doc })))
}

/// GET /api/documents/{id} — one document with content.
async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let doc = otw_store::documents::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    Ok(Json(json!({ "document": doc })))
}

#[derive(Deserialize)]
struct UpdateBody {
    title: Option<String>,
    content: Option<JsonValue>,
    layout: Option<String>,
}

/// PATCH /api/documents/{id} — update title, content and/or layout (autosave).
async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(l) = &body.layout {
        if !matches!(l.as_str(), "normal" | "wide") {
            return Err(ApiError::bad_request("layout must be 'normal' or 'wide'"));
        }
    }
    let ok = otw_store::documents::update(
        &state.pool,
        id,
        body.title.as_deref(),
        body.content.as_ref(),
        body.layout.as_deref(),
    )
    .await?;
    if !ok {
        return Err(ApiError::not_found("document not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct MoveBody {
    parent_id: Option<Uuid>,
    position: f64,
}

/// POST /api/documents/{id}/move — reparent / reorder.
async fn move_doc(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<MoveBody>,
) -> Result<Json<Value>, ApiError> {
    let ok = otw_store::documents::mv(&state.pool, id, body.parent_id, body.position).await?;
    if !ok {
        return Err(ApiError::not_found("document not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /api/documents/{id} — delete (cascades to children).
async fn delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ok = otw_store::documents::delete(&state.pool, id).await?;
    if !ok {
        return Err(ApiError::not_found("document not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
