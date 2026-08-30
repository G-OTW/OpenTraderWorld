//! HTTP API for external notification channels — the shared notification broker.
//!
//! - `GET  /api/notif-channels/modules`         module ids a channel can be granted to
//! - `GET  /api/notif-channels?module=watchlists`  channels (all, or just that module's)
//! - `POST /api/notif-channels`                 create (kind, name, config, modules?)
//! - `PATCH/DELETE /api/notif-channels/{id}`    edit / re-grant / remove
//! - `POST /api/notif-channels/{id}/test`       send a sample message
//!
//! One channel list serves every module that notifies; which modules may push to a
//! channel is the `modules` grant list (`["*"]` = all), mirroring the data broker.
//! Secrets (SMTP password / bot token / webhook URL) are write-only: they are sealed at
//! rest and never returned. The test endpoint lets the user verify a setup before
//! enabling it.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{notif_send, ApiError, AppState};
use otw_store::notif_channels::{self, ChannelInput, ALL_MODULES, CHANNEL_KINDS};

/// Modules that produce notifications and can therefore be granted a channel. Adding a
/// notifying module means adding it here (the wildcard grant `*` covers it retroactively)
/// and passing the same id to [`notif_send::dispatch`].
pub const NOTIF_MODULES: &[&str] =
    &["remindme", "watchlists", "mailbox", "webhooks", "histdata", "automator"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/notif-channels/modules", get(modules))
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

/// A module id a channel can be granted to.
fn valid_module(m: &str) -> Result<(), ApiError> {
    if NOTIF_MODULES.contains(&m) {
        Ok(())
    } else {
        Err(ApiError::bad_request(&format!("unknown module: {m}")))
    }
}

/// Normalize a grant list: `["*"]` collapses everything else; ids are validated and
/// de-duplicated. An empty list is allowed — the channel then receives nothing, which is
/// how a half-configured destination stays out of the way without being deleted.
fn normalize_modules(input: &[String]) -> Result<Vec<String>, ApiError> {
    if input.iter().any(|m| m == ALL_MODULES) {
        return Ok(vec![ALL_MODULES.to_string()]);
    }
    let mut out: Vec<String> = Vec::new();
    for m in input {
        let m = m.trim();
        valid_module(m)?;
        if !out.iter().any(|x| x == m) {
            out.push(m.to_string());
        }
    }
    Ok(out)
}

/// Grantable module ids, in display order. Labels are the client's (module registry).
async fn modules() -> Json<Value> {
    Json(json!({ "modules": NOTIF_MODULES }))
}

#[derive(Deserialize)]
struct ListQuery {
    /// Restrict to channels granted to this module. Absent = the whole channel list.
    module: Option<String>,
}

async fn list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let channels = match q.module.as_deref() {
        Some(m) => {
            valid_module(m)?;
            notif_channels::list_for_module(&s.pool, m).await?
        }
        None => notif_channels::list(&s.pool).await?,
    };
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
    Json(mut input): Json<ChannelInput>,
) -> Result<Json<Value>, ApiError> {
    validate(&input)?;
    if let Some(m) = &input.modules {
        input.modules = Some(normalize_modules(m)?);
    }
    let ch = notif_channels::add(&s.pool, &s.cipher, &input).await?;
    Ok(Json(json!({ "channel": ch })))
}

async fn update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<ChannelInput>,
) -> Result<Json<Value>, ApiError> {
    validate(&input)?;
    if let Some(m) = &input.modules {
        input.modules = Some(normalize_modules(m)?);
    }
    if !notif_channels::update(&s.pool, &s.cipher, id, &input).await? {
        return Err(ApiError::not_found("channel not found"));
    }
    let ch = notif_channels::get(&s.pool, id).await?;
    Ok(Json(json!({ "ok": true, "channel": ch })))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `ApiError` isn't Debug, so assert on the Ok value through a helper.
    fn ok(input: &[&str]) -> Vec<String> {
        match normalize_modules(&input.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
            Ok(v) => v,
            Err(_) => panic!("expected {input:?} to normalize"),
        }
    }

    #[test]
    fn wildcard_collapses_and_ids_are_validated() {
        assert_eq!(ok(&["remindme", "*"]), vec!["*".to_string()]);
        assert_eq!(
            ok(&["remindme", "remindme", "watchlists"]),
            vec!["remindme".to_string(), "watchlists".to_string()]
        );
        assert!(ok(&[]).is_empty());
        // A module that never notifies can't be granted.
        assert!(normalize_modules(&["journal".to_string()]).is_err());
    }
}
