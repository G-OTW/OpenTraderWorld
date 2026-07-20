//! HTTP API for the editor module's databases (typed tables: columns + rows).
//!
//! A database is a document (kind = 'database'); these routes manage the columns
//! and rows that hang off it. View configuration (table/kanban/gallery) is saved
//! through the normal document `content` PATCH, so it isn't handled here.

use axum::{
    extract::{Path, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::types::JsonValue;
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::databases::{self, ColumnPatch};

pub fn routes() -> Router<AppState> {
    Router::new()
        // Whole database payload (columns + rows).
        .route("/api/databases/{id}", get(load))
        // Columns.
        .route("/api/databases/{id}/columns", post(add_column))
        .route(
            "/api/databases/columns/{col_id}",
            patch(update_column).delete(delete_column),
        )
        // Rows.
        .route("/api/databases/{id}/rows", post(add_row))
        .route(
            "/api/databases/rows/{row_id}",
            patch(update_row).delete(delete_row),
        )
        .route("/api/databases/rows/{row_id}/move", post(move_row))
}

/// GET /api/databases/{id} — columns + rows for a database document.
async fn load(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let data = databases::load(&state.pool, id).await?;
    Ok(Json(json!({ "columns": data.columns, "rows": data.rows })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddColumnBody {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default = "default_type")]
    kind: String,
    #[serde(default = "empty_object")]
    options: JsonValue,
}
fn default_type() -> String {
    "text".to_string()
}
fn empty_object() -> JsonValue {
    json!({})
}

/// POST /api/databases/{id}/columns — add a typed column.
async fn add_column(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddColumnBody>,
) -> Result<Json<Value>, ApiError> {
    if !databases::COLUMN_TYPES.contains(&body.kind.as_str()) {
        return Err(ApiError::bad_request("unknown column type"));
    }
    let col = databases::add_column(&state.pool, id, &body.name, &body.kind, &body.options).await?;
    Ok(Json(json!({ "column": col })))
}

/// PATCH /api/databases/columns/{col_id} — rename / retype / reorder.
async fn update_column(
    State(state): State<AppState>,
    Path(col_id): Path<Uuid>,
    Json(patch): Json<ColumnPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(kind) = &patch.kind {
        if !databases::COLUMN_TYPES.contains(&kind.as_str()) {
            return Err(ApiError::bad_request("unknown column type"));
        }
    }
    let ok = databases::update_column(&state.pool, col_id, &patch).await?;
    if !ok {
        return Err(ApiError::not_found("column not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /api/databases/columns/{col_id}.
async fn delete_column(
    State(state): State<AppState>,
    Path(col_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ok = databases::delete_column(&state.pool, col_id).await?;
    if !ok {
        return Err(ApiError::not_found("column not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddRowBody {
    #[serde(default = "empty_object")]
    cells: JsonValue,
}

/// POST /api/databases/{id}/rows — append a row.
async fn add_row(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddRowBody>,
) -> Result<Json<Value>, ApiError> {
    let row = databases::add_row(&state.pool, id, &body.cells).await?;
    Ok(Json(json!({ "row": row })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct UpdateRowBody {
    cells: JsonValue,
}

/// PATCH /api/databases/rows/{row_id} — replace a row's cells.
async fn update_row(
    State(state): State<AppState>,
    Path(row_id): Path<Uuid>,
    Json(body): Json<UpdateRowBody>,
) -> Result<Json<Value>, ApiError> {
    let ok = databases::update_row(&state.pool, row_id, &body.cells).await?;
    if !ok {
        return Err(ApiError::not_found("row not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct MoveRowBody {
    position: f64,
}

/// POST /api/databases/rows/{row_id}/move — reorder a row.
async fn move_row(
    State(state): State<AppState>,
    Path(row_id): Path<Uuid>,
    Json(body): Json<MoveRowBody>,
) -> Result<Json<Value>, ApiError> {
    let ok = databases::move_row(&state.pool, row_id, body.position).await?;
    if !ok {
        return Err(ApiError::not_found("row not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /api/databases/rows/{row_id}.
async fn delete_row(
    State(state): State<AppState>,
    Path(row_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ok = databases::delete_row(&state.pool, row_id).await?;
    if !ok {
        return Err(ApiError::not_found("row not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
