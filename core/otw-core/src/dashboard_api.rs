//! HTTP API for the Dashboard layout.
//!
//! The dashboard is a user-arrangeable grid of module link-tiles. The whole layout is a
//! single JSON document (rows → placed tiles + spacer rows) persisted under the
//! `dashboard_layout` key in `app_settings`. We store it opaquely: the frontend owns the
//! schema, the backend just round-trips the JSON. An unset/blank value yields `null`,
//! letting the client build a default from the installed-module set.

use axum::{routing::get, Json, Router};
use axum::extract::State;
use serde_json::Value;

use crate::{ApiError, AppState};

const KEY: &str = "dashboard_layout";

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/dashboard/layout", get(get_layout).put(set_layout))
}

/// Current saved layout, or `null` if the user has never customized it.
async fn get_layout(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let raw = otw_store::settings::get(&state.pool, KEY)
        .await
        .map_err(ApiError::from)?;
    let layout = match raw {
        Some(s) if !s.trim().is_empty() => {
            serde_json::from_str::<Value>(&s).unwrap_or(Value::Null)
        }
        _ => Value::Null,
    };
    Ok(Json(layout))
}

/// Replace the saved layout with the posted JSON document.
async fn set_layout(
    State(state): State<AppState>,
    Json(layout): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let serialized = serde_json::to_string(&layout)
        .map_err(|e| ApiError::bad_request(&format!("invalid layout: {e}")))?;
    otw_store::settings::set(&state.pool, KEY, &serialized)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(layout))
}
