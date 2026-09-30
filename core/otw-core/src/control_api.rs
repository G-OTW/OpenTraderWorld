//! HTTP API for Settings → External control — the global switch and the chat bindings.
//!
//! Session-protected, browser only. Nothing here is in the MCP catalog and nothing here
//! ever will be: this is where a bot credential is set and a pairing code is minted, so an
//! agent that could call it could grant itself a way back in.
//!
//! The validation worth naming: a binding may only point at a token carrying `external`.
//! The flag is the user saying "this envelope may be reached from outside", and it is
//! checked again when the transport starts and once more per message.

use axum::{
    extract::{Path, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use otw_store::control::{self, BindingInput, CONTROL_KINDS, PAIR_TTL_MAX, PAIR_TTL_MIN};

use crate::{security_events, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/control/settings", get(get_settings).post(set_settings))
        .route("/api/control/bindings", get(list).post(create))
        .route("/api/control/bindings/{id}", patch(update).delete(remove))
        .route("/api/control/bindings/{id}/pair", post(pair))
        .route("/api/control/bindings/{id}/senders/{sender_id}", axum::routing::delete(unpair))
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let enabled =
        otw_store::settings::get_or(&state.pool, crate::control::ENABLED_SETTING, "false").await?;
    Ok(Json(json!({
        "enabled": enabled == "true",
        // Which channel kinds can carry control at all, so the UI can say why an email
        // channel is not offered instead of silently omitting it.
        "kinds": CONTROL_KINDS,
        "pair_ttl_minutes": control::pair_ttl_minutes(&state.pool).await?,
        "pair_ttl_min": PAIR_TTL_MIN,
        "pair_ttl_max": PAIR_TTL_MAX,
    })))
}

#[derive(Deserialize)]
struct Toggle {
    enabled: bool,
    /// Optional: how long a minted pairing code stays usable. Absent leaves it alone.
    #[serde(default)]
    pair_ttl_minutes: Option<i64>,
}

async fn set_settings(
    State(state): State<AppState>,
    Json(input): Json<Toggle>,
) -> Result<Json<Value>, ApiError> {
    if let Some(mins) = input.pair_ttl_minutes {
        if !(PAIR_TTL_MIN..=PAIR_TTL_MAX).contains(&mins) {
            return Err(ApiError::bad_request(&format!(
                "a pairing code has to last between {PAIR_TTL_MIN} and {PAIR_TTL_MAX} minutes"
            )));
        }
        otw_store::settings::set(
            &state.pool,
            otw_store::control::PAIR_TTL_SETTING,
            &mins.to_string(),
        )
        .await?;
    }
    otw_store::settings::set(
        &state.pool,
        crate::control::ENABLED_SETTING,
        if input.enabled { "true" } else { "false" },
    )
    .await?;
    let state_word = if input.enabled { "enabled" } else { "disabled" };
    security_events::alert(
        &state,
        security_events::CONTROL_GATEWAY,
        security_events::CONTROL_GATEWAY,
        &format!("External control {state_word}"),
        &format!(
            "Chat bindings are now {state_word}. While they are on, a paired sender can drive \
             this instance through the agent, with whatever the binding's token allows."
        ),
    );
    Ok(Json(json!({
        "enabled": input.enabled,
        "pair_ttl_minutes": control::pair_ttl_minutes(&state.pool).await?,
    })))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "bindings": control::list(&state.pool).await? })))
}

/// Everything a binding must satisfy before it is stored. Checked here rather than left to
/// the schema so the answer names the fix instead of being a constraint violation.
async fn validate(
    state: &AppState,
    input: &BindingInput,
    existing: Option<Uuid>,
) -> Result<(), ApiError> {
    let channel = otw_store::notif_channels::get(&state.pool, input.channel_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("that channel no longer exists"))?;
    if !CONTROL_KINDS.contains(&channel.kind.as_str()) {
        return Err(ApiError::bad_request(&format!(
            "a '{}' channel can only send — control needs {}",
            channel.kind,
            CONTROL_KINDS.join(", ")
        )));
    }
    let token = otw_store::mcp::get_token(&state.pool, input.token_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("that access token no longer exists"))?;
    if !token.external {
        return Err(ApiError::bad_request(
            "that token is not allowed to be used from outside — turn on external access for it \
             in Settings → AI agents first",
        ));
    }
    if otw_store::agent::get_agent(&state.pool, input.agent_id).await?.is_none() {
        return Err(ApiError::bad_request("that agent no longer exists"));
    }
    // A provider override is optional; a broken one is not. Refuse it here rather than at
    // the first message, which would be read in a chat client with no way to fix it.
    if let Some(Some(provider_id)) = input.provider_id {
        match otw_store::agent::get_provider(&state.pool, provider_id).await? {
            None => return Err(ApiError::bad_request("that provider no longer exists")),
            Some(p) if !p.enabled => {
                return Err(ApiError::bad_request("that provider is disabled"))
            }
            Some(_) => {}
        }
    }
    // One binding per channel and one per token. Pre-checked for the message; the schema
    // still enforces it.
    for b in control::list(&state.pool).await? {
        if Some(b.id) == existing {
            continue;
        }
        if b.channel_id == input.channel_id {
            return Err(ApiError::conflict(&format!(
                "'{}' already drives that channel",
                b.name
            )));
        }
        if b.token_id == input.token_id {
            return Err(ApiError::conflict(&format!("'{}' already uses that token", b.name)));
        }
    }
    Ok(())
}

/// A binding that is switched on but has nothing to listen with would sit there reporting
/// a failure; say so at the point the choice is made.
fn needs_credential(input: &BindingInput, has_secret: bool) -> Result<(), ApiError> {
    let will_have = input.secret_vault_item.is_some()
        || input.secret.as_deref().is_some_and(|s| !s.is_empty())
        || (has_secret && input.secret.is_none());
    if input.enabled && !will_have {
        return Err(ApiError::bad_request(
            "set the bot token before enabling this binding — a webhook URL can only send",
        ));
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<BindingInput>,
) -> Result<Json<Value>, ApiError> {
    validate(&state, &input, None).await?;
    needs_credential(&input, false)?;
    let row = control::add(&state.pool, &state.cipher, &input).await?;
    tracing::info!("control binding created: {}", row.name);
    Ok(Json(json!({ "binding": row })))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<BindingInput>,
) -> Result<Json<Value>, ApiError> {
    let current = control::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("binding not found"))?;
    validate(&state, &input, Some(id)).await?;
    needs_credential(&input, current.has_secret)?;
    if !control::update(&state.pool, &state.cipher, id, &input).await? {
        return Err(ApiError::not_found("binding not found"));
    }
    Ok(Json(json!({ "binding": control::get(&state.pool, id).await? })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !control::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("binding not found"));
    }
    Ok(Json(json!({ "deleted": true })))
}

/// Mint a pairing code. Returned once, here: the next screen the user looks at is their
/// chat client, and the code is only useful for the next few minutes.
async fn pair(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let code = control::mint_pair_code(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("binding not found"))?;
    Ok(Json(json!({
        "code": code,
        "expires_in_minutes": control::pair_ttl_minutes(&state.pool).await?,
    })))
}

async fn unpair(
    State(state): State<AppState>,
    Path((id, sender_id)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiError> {
    if !control::remove_sender(&state.pool, id, &sender_id).await? {
        return Err(ApiError::not_found("that sender was not paired"));
    }
    Ok(Json(json!({ "removed": true })))
}
