//! Broker imports running in the background: list them, take a finished one's answer
//! back, or stop one. See `crate::tasks`.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{tasks, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list))
        .route("/api/tasks/{id}/take", post(take))
        .route("/api/tasks/{id}", delete(cancel))
}

#[derive(Deserialize)]
struct ListQuery {
    scope: Option<String>,
}

async fn list(
    State(_state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "tasks": tasks::list(q.scope.as_deref()) })))
}

async fn take(
    State(_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let task = tasks::take(id).map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "task": task })))
}

async fn cancel(
    State(_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !tasks::cancel(id) {
        return Err(ApiError::not_found("background pull not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
