//! HTTP API for the ToDo module — flat task-list CRUD plus a done-toggle.

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::todos::{self, TodoInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/todos", get(list).post(add))
        .route("/api/todos/{id}", get(get_one).patch(update).delete(remove))
        .route("/api/todos/{id}/done", axum::routing::patch(toggle_done))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let todos = todos::list_todos(&state.pool).await?;
    Ok(Json(json!({ "todos": todos })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let todo = todos::get_todo(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("todo not found"))?;
    Ok(Json(json!({ "todo": todo })))
}

fn validate(input: &TodoInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("todo name required"));
    }
    Ok(())
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<TodoInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    let todo = todos::add_todo(&state.pool, &input).await?;
    Ok(Json(json!({ "todo": todo })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<TodoInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    if !todos::update_todo(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("todo not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DoneBody {
    done: bool,
}

async fn toggle_done(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<DoneBody>,
) -> Result<Json<Value>, ApiError> {
    if !todos::set_done(&state.pool, id, body.done).await? {
        return Err(ApiError::not_found("todo not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !todos::delete_todo(&state.pool, id).await? {
        return Err(ApiError::not_found("todo not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
