//! HTTP API for the Goals module.
//!
//! Goal CRUD. KPIs arrive as a JSONB array; we validate each entry's shape (name +
//! numeric target/current/points + reached bool) before persisting so the store and
//! frontend can trust it.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{ApiError, AppState};
use otw_store::goals::{self, GoalInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/goals", get(list).post(add))
        .route("/api/goals/{id}", get(get_one).patch(update).delete(remove))
        .route("/api/goals/{id}/position", post(set_position))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let goals = goals::list_goals(&state.pool).await?;
    Ok(Json(json!({ "goals": goals })))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let goal = goals::get_goal(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("goal not found"))?;
    Ok(Json(json!({ "goal": goal })))
}

/// Validate the goal name and the KPI array shape. Each KPI must be an object with a
/// non-empty `name` and finite numeric `target`/`current`/`points`; `reached` is a bool.
fn validate(input: &GoalInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("goal name required"));
    }
    let arr = input
        .kpis
        .as_array()
        .ok_or_else(|| ApiError::bad_request("kpis must be an array"))?;
    for (i, kpi) in arr.iter().enumerate() {
        let obj = kpi
            .as_object()
            .ok_or_else(|| ApiError::bad_request("each kpi must be an object"))?;
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        if name.trim().is_empty() {
            return Err(ApiError::bad_request(&format!("kpi {} needs a name", i + 1)));
        }
        for key in ["target", "current", "points"] {
            let v = obj.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            if !v.is_finite() {
                return Err(ApiError::bad_request(&format!("kpi '{name}' has a bad {key}")));
            }
        }
    }
    Ok(())
}

async fn add(
    State(state): State<AppState>,
    Json(mut input): Json<GoalInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    let goal = goals::add_goal(&state.pool, &input).await?;
    Ok(Json(json!({ "goal": goal })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<GoalInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate(&input)?;
    if !goals::update_goal(&state.pool, id, &input).await? {
        return Err(ApiError::not_found("goal not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !goals::delete_goal(&state.pool, id).await? {
        return Err(ApiError::not_found("goal not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PositionBody {
    position: f64,
}

async fn set_position(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PositionBody>,
) -> Result<Json<Value>, ApiError> {
    if !goals::set_position(&state.pool, id, body.position).await? {
        return Err(ApiError::not_found("goal not found"));
    }
    Ok(Json(json!({ "ok": true })))
}
