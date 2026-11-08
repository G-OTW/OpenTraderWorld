//! `GET /api/vocab`: the closed vocabularies the API enforces, so a caller can read the
//! valid values of a field before sending one. See `util::vocab`.

use axum::{extract::Path, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::util::vocab;
use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/vocab", get(list))
        .route("/api/vocab/{field}", get(one))
}

async fn list() -> Json<Value> {
    Json(json!({ "vocabs": vocab::all() }))
}

async fn one(Path(field): Path<String>) -> Result<Json<Value>, ApiError> {
    match vocab::find(&field) {
        Some(e) => Ok(Json(json!(e))),
        None => {
            let known: Vec<&str> = vocab::all().iter().map(|e| e.field).collect();
            Err(ApiError::not_found(&format!(
                "unknown vocab field {field:?}; known: {}",
                if known.is_empty() { "(none yet)".to_string() } else { known.join(", ") }
            )))
        }
    }
}
