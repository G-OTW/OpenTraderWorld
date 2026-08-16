//! HTTP API for the PromptStore module — a prompt library with version history,
//! quick voting (thumbs up/down), duplicate, and rollback.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::prompts::{self, PromptInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/prompts", get(list).post(add))
        .route("/api/prompts/tags", get(tags))
        .route("/api/prompts/{id}", get(get_one).patch(update).delete(remove))
        .route("/api/prompts/{id}/vote", axum::routing::patch(vote))
        .route("/api/prompts/{id}/duplicate", post(duplicate))
        .route("/api/prompts/{id}/versions", get(versions))
        .route("/api/prompts/{id}/rollback", post(rollback))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let prompts = prompts::list_prompts(&state.pool).await?;
    Ok(Json(json!({ "prompts": prompts })))
}

async fn tags(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let tags = prompts::distinct_tags(&state.pool).await?;
    Ok(Json(json!({ "tags": tags })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let prompt = prompts::get_prompt(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("prompt not found"))?;
    Ok(Json(json!({ "prompt": prompt })))
}

fn validate(input: &PromptInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("prompt name required"));
    }
    Ok(())
}

fn normalise(input: &mut PromptInput) {
    input.name = input.name.trim().to_string();
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<PromptInput>,
) -> Result<Json<Value>, ApiError> {
    normalise(&mut input);
    validate(&input)?;
    let prompt = prompts::add_prompt(&state.pool, &input).await?;
    Ok(Json(json!({ "prompt": prompt })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<PromptInput>,
) -> Result<Json<Value>, ApiError> {
    normalise(&mut input);
    validate(&input)?;
    if !prompts::update_prompt(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("prompt not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct VoteBody {
    /// 1 = up, -1 = down, 0 = clear.
    vote: i16,
}

async fn vote(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<VoteBody>,
) -> Result<Json<Value>, ApiError> {
    if !prompts::set_vote(&state.pool, id, body.vote).await? {
        return Err(ApiError::not_found("prompt not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn duplicate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let prompt = prompts::duplicate(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("prompt not found"))?;
    Ok(Json(json!({ "prompt": prompt })))
}

async fn versions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let versions = prompts::list_versions(&state.pool, id).await?;
    Ok(Json(json!({ "versions": versions })))
}

#[derive(Deserialize)]
struct RollbackBody {
    version: i32,
}

async fn rollback(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<Value>, ApiError> {
    let prompt = prompts::rollback(&state.pool, id, body.version)
        .await?
        .ok_or_else(|| ApiError::not_found("prompt or version not found"))?;
    Ok(Json(json!({ "prompt": prompt })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !prompts::delete_prompt(&state.pool, id).await? {
        return Err(ApiError::not_found("prompt not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
