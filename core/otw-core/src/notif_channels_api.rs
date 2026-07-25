//! HTTP API for RemindMe external notification channels.
//!
//! CRUD over channels the user plugs in (email/telegram/slack/discord), each of which a
//! fired reminder is also pushed to *when enabled*. Secrets (SMTP password / bot token /
//! webhook URL) are write-only: they are sealed at rest and never returned. A "test"
//! endpoint sends a sample message so the user can verify the setup before enabling it.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{notif_send, ApiError, AppState};
use otw_store::notif_channels::{self, ChannelInput, CHANNEL_KINDS};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/notif-channels", get(list).post(add))
        .route(
            "/api/notif-channels/{id}",
            get(get_one).patch(update).delete(remove),
        )
        .route("/api/notif-channels/{id}/test", post(test))
}

fn validate(input: &ChannelInput) -> Result<(), ApiError> {
    if !CHANNEL_KINDS.contains(&input.kind.as_str()) {
        return Err(ApiError::bad_request(
            "kind must be email, telegram, slack or discord",
        ));
    }
    if !input.config.is_object() {
        return Err(ApiError::bad_request("config must be a JSON object"));
    }
    Ok(())
}

async fn list(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let channels = notif_channels::list(&s.pool).await?;
    Ok(Json(json!({ "channels": channels })))
}

async fn get_one(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ch = notif_channels::get(&s.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("channel not found"))?;
    Ok(Json(json!({ "channel": ch })))
}

async fn add(
    State(s): State<AppState>,
    Json(input): Json<ChannelInput>,
) -> Result<Json<Value>, ApiError> {
    validate(&input)?;
    let ch = notif_channels::add(&s.pool, &s.cipher, &input).await?;
    Ok(Json(json!({ "channel": ch })))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ChannelInput>,
) -> Result<Json<Value>, ApiError> {
    validate(&input)?;
    if !notif_channels::update(&s.pool, &s.cipher, id, &input).await? {
        return Err(ApiError::not_found("channel not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !notif_channels::delete(&s.pool, id).await? {
        return Err(ApiError::not_found("channel not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Send a test message through a single channel and report success/failure inline (also
/// recorded as its last result). Lets the user verify creds before enabling the channel.
async fn test(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ch = notif_channels::load_with_secret(&s.pool, &s.cipher, id)
        .await?
        .ok_or_else(|| ApiError::not_found("channel not found"))?;
    let res = notif_send::send_one(
        &s.http,
        &ch,
        "OpenTraderWorld test",
        "This is a test notification. If you can read this, the channel works.",
    )
    .await;
    match res {
        Ok(()) => {
            let _ = notif_channels::record_result(&s.pool, id, true, None).await;
            Ok(Json(json!({ "ok": true })))
        }
        Err(e) => {
            let msg = format!("{e:#}");
            let _ = notif_channels::record_result(&s.pool, id, false, Some(&msg)).await;
            Err(ApiError::bad_request(&format!("test failed: {msg}")))
        }
    }
}
