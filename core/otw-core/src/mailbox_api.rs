//! HTTP API for the Mailbox module.
//!
//! Reading endpoints are ordinary CRUD over what ingest stored. The two that reach
//! outside the box — testing a mailbox and one-click unsubscribe — do so only from an
//! explicit user action, never on a schedule.

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use otw_store::mailbox;

use crate::{mailbox as ingest, ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/mailbox/presets", get(presets))
        .route("/api/mailbox/accounts", get(list_accounts).post(create_account))
        .route(
            "/api/mailbox/accounts/{id}",
            axum::routing::patch(update_account).delete(delete_account),
        )
        .route("/api/mailbox/accounts/{id}/test", post(test_account))
        .route("/api/mailbox/accounts/{id}/oauth/start", post(oauth_start))
        .route("/api/mailbox/oauth/callback", post(oauth_callback))
        .route("/api/mailbox/oauth/poll", post(oauth_poll))
        .route("/api/mailbox/accounts/{id}/poll", post(poll_account))
        .route("/api/mailbox/poll", post(poll_all))
        .route("/api/mailbox/senders", get(list_senders))
        .route(
            "/api/mailbox/senders/{id}",
            axum::routing::patch(update_sender).delete(delete_sender),
        )
        .route("/api/mailbox/messages", get(list_messages))
        .route("/api/mailbox/messages/read-all", post(read_all))
        .route(
            "/api/mailbox/messages/{id}",
            get(get_message).patch(update_message).delete(delete_message),
        )
        .route("/api/mailbox/messages/{id}/unsubscribe", post(unsubscribe))
        .route("/api/mailbox/messages/{id}/remind", post(remind))
        .route("/api/mailbox/attachments/{id}", get(download_attachment))
        .route("/api/mailbox/links", get(list_links).post(create_link))
        .route(
            "/api/mailbox/links/{id}",
            axum::routing::patch(update_link).delete(delete_link),
        )
        .route("/api/mailbox/settings", get(get_settings).put(put_settings))
}

// ── Validation ───────────────────────────────────────────────────────────────

const SECURITY: &[&str] = &["ssl", "starttls", "none"];
const AUTH_KINDS: &[&str] = &["password", "oauth"];
const CATEGORIES: &[&str] = &["news", "newsletter", "broker", "other"];
const STATUSES: &[&str] = &["kept", "pending", "ignored"];
const TOPICS: &[&str] =
    &["mindset", "finance", "trading", "geopolitics", "economics", "other"];

fn check(value: &str, allowed: &[&str], field: &str) -> Result<(), ApiError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(&format!("unknown {field}: {value}")))
    }
}

fn check_opt(value: Option<&String>, allowed: &[&str], field: &str) -> Result<(), ApiError> {
    match value {
        Some(v) => check(v, allowed, field),
        None => Ok(()),
    }
}

// ── Presets & accounts ───────────────────────────────────────────────────────

async fn presets() -> Json<Value> {
    Json(json!({ "presets": ingest::presets::PRESETS }))
}

async fn list_accounts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let accounts = mailbox::list_accounts(&state.pool).await?;
    let (unread, total, pending) = mailbox::counts(&state.pool).await?;
    Ok(Json(json!({
        "accounts": accounts,
        "presets": ingest::presets::PRESETS,
        "counts": { "unread": unread, "total": total, "pending_senders": pending },
    })))
}

async fn create_account(
    State(state): State<AppState>,
    Json(input): Json<mailbox::AccountInput>,
) -> Result<Json<Value>, ApiError> {
    if input.host.trim().is_empty() {
        return Err(ApiError::bad_request("a server address is required"));
    }
    if input.username.trim().is_empty() {
        return Err(ApiError::bad_request("a username is required"));
    }
    check(&input.security, SECURITY, "security mode")?;
    check(&input.auth_kind, AUTH_KINDS, "authentication mode")?;
    if !(1..=65535).contains(&input.port) {
        return Err(ApiError::bad_request("port must be between 1 and 65535"));
    }
    if input.auth_kind == "oauth" {
        if input.oauth_client_id.trim().is_empty() {
            return Err(ApiError::bad_request(
                "an application (client) ID is required for this provider",
            ));
        }
        if input.email.trim().is_empty() {
            return Err(ApiError::bad_request(
                "the mailbox address is required — it is what the sign-in is issued for",
            ));
        }
    } else if input.vault_item_id.is_none() {
        return Err(ApiError::bad_request("a vault item holding the password is required"));
    }
    let account = mailbox::create_account(&state.pool, &input).await?;
    tracing::info!("mailbox account added: {}", account.host);
    Ok(Json(json!({ "account": account })))
}

async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<mailbox::AccountPatch>,
) -> Result<Json<Value>, ApiError> {
    check_opt(patch.security.as_ref(), SECURITY, "security mode")?;
    let account = mailbox::update_account(&state.pool, id, &patch)
        .await?
        .ok_or_else(|| ApiError::not_found("mailbox not found"))?;
    Ok(Json(json!({ "account": account })))
}

async fn delete_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !mailbox::delete_account(&state.pool, id).await? {
        return Err(ApiError::not_found("mailbox not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn account_or_404(
    state: &AppState,
    id: Uuid,
) -> Result<otw_store::mailbox::Account, ApiError> {
    mailbox::get_account(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("mailbox not found"))
}

async fn test_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let account = account_or_404(&state, id).await?;
    match ingest::test_account(&state.pool, &state.cipher, &state.http, &account).await {
        Ok(report) => Ok(Json(json!({ "ok": true, "messages": report.exists }))),
        // The provider's own words are the most useful thing here ("app-specific password
        // required", "invalid credentials"), so they are passed through verbatim.
        Err(e) => Ok(Json(json!({ "ok": false, "error": format!("{e:#}") }))),
    }
}

async fn poll_account(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let account = account_or_404(&state, id).await?;
    match ingest::poll_account(&state.pool, &state.cipher, &state.http, &account).await {
        Ok(n) => Ok(Json(json!({ "ok": true, "new_messages": n }))),
        Err(e) => Ok(Json(json!({ "ok": false, "error": format!("{e:#}") }))),
    }
}

async fn poll_all(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let accounts = mailbox::list_accounts(&state.pool).await?;
    let mut new_messages = 0;
    let mut errors: Vec<Value> = Vec::new();
    for account in accounts.iter().filter(|a| a.enabled) {
        match ingest::poll_account(&state.pool, &state.cipher, &state.http, account).await {
            Ok(n) => new_messages += n,
            Err(e) => errors.push(json!({ "account": account.name, "error": format!("{e:#}") })),
        }
    }
    Ok(Json(json!({ "new_messages": new_messages, "errors": errors })))
}

// ── OAuth sign-in ────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct OauthStartBody {
    /// `device` forces the code fallback; otherwise the redirect flow is used whenever
    /// this instance is reachable at an address Microsoft will accept as a redirect URI.
    #[serde(default)]
    mode: Option<String>,
}

/// Seal a refresh token against its account and mark the mailbox connected.
async fn store_sign_in(
    state: &AppState,
    account_id: Uuid,
    refresh_token: &str,
) -> Result<(), ApiError> {
    let account = mailbox::get_account(&state.pool, account_id)
        .await?
        .ok_or_else(|| ApiError::not_found("mailbox not found"))?;
    let item =
        ingest::store_refresh_token(&state.pool, &state.cipher, &account, refresh_token).await?;
    mailbox::set_oauth_credential(&state.pool, account.id, item).await?;
    tracing::info!("mailbox {}: signed in", account.name);
    Ok(())
}

/// Begin a sign-in for one account. The browser's own origin decides the flow: a redirect
/// URI Microsoft accepts (https, or http on loopback) gets authorization code + PKCE,
/// anything else falls back to the device code. Nothing is stored until it completes.
async fn oauth_start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: axum::http::HeaderMap,
    body: Option<Json<OauthStartBody>>,
) -> Result<Json<Value>, ApiError> {
    let account = account_or_404(&state, id).await?;
    if account.auth_kind != "oauth" {
        return Err(ApiError::bad_request("this mailbox signs in with a password"));
    }
    if account.oauth_client_id.trim().is_empty() {
        return Err(ApiError::bad_request("this mailbox has no application (client) ID"));
    }
    let body = body.map(|Json(b)| b).unwrap_or_default();

    let redirect = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(ingest::oauth::redirect_uri);
    if body.mode.as_deref() != Some("device") {
        if let Some(redirect_uri) = redirect {
            let (handle, sign_in) = ingest::oauth::start_code(
                account.id,
                &account.oauth_tenant,
                &account.oauth_client_id,
                &account.email,
                &redirect_uri,
            )
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
            tracing::info!("mailbox {}: sign-in started (redirect)", account.name);
            return Ok(Json(json!({ "flow": handle, "mode": "code", "sign_in": sign_in })));
        }
    }

    let (handle, code) = ingest::oauth::start_device(
        &state.http,
        account.id,
        &account.oauth_tenant,
        &account.oauth_client_id,
    )
    .await
    .map_err(|e| ApiError::bad_gateway(&format!("{e:#}")))?;
    tracing::info!("mailbox {}: sign-in started (device code)", account.name);
    Ok(Json(json!({ "flow": handle, "mode": "device", "device": code })))
}

#[derive(Deserialize)]
struct OauthCallbackBody {
    /// The `state` Microsoft echoed back — the handle of the flow that issued it.
    flow: Uuid,
    #[serde(default)]
    code: Option<String>,
    /// Set instead of `code` when the user declined or the tenant refused.
    #[serde(default)]
    error: Option<String>,
}

/// The redirect landed. Exchange the code here rather than in the polling window, so a
/// mailbox is connected even if the window that started the sign-in is gone.
async fn oauth_callback(
    State(state): State<AppState>,
    Json(body): Json<OauthCallbackBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(error) = body.error.filter(|e| !e.trim().is_empty()) {
        ingest::oauth::settle(body.flow, Err(error.clone()));
        return Ok(Json(json!({ "ok": false, "error": error })));
    }
    let Some(code) = body.code.filter(|c| !c.trim().is_empty()) else {
        return Err(ApiError::bad_request("this redirect carries no authorization code"));
    };
    match ingest::oauth::redeem(&state.http, body.flow, &code).await {
        Ok((account_id, refresh_token)) => {
            if let Err(e) = store_sign_in(&state, account_id, &refresh_token).await {
                ingest::oauth::settle(body.flow, Err("the sign-in could not be stored".into()));
                return Err(e);
            }
            ingest::oauth::settle(body.flow, Ok(()));
            Ok(Json(json!({ "ok": true })))
        }
        Err(e) => {
            let message = format!("{e:#}");
            ingest::oauth::settle(body.flow, Err(message.clone()));
            Ok(Json(json!({ "ok": false, "error": message })))
        }
    }
}

#[derive(Deserialize)]
struct OauthPollBody {
    flow: Uuid,
}

/// Poll a sign-in. `pending` until it completes; a device-code sign-in seals its refresh
/// token here, a redirect one was already sealed by the callback.
async fn oauth_poll(
    State(state): State<AppState>,
    Json(body): Json<OauthPollBody>,
) -> Result<Json<Value>, ApiError> {
    match ingest::oauth::poll(&state.http, body.flow).await {
        Ok(ingest::oauth::Progress::Pending { interval }) => {
            Ok(Json(json!({ "status": "pending", "interval": interval })))
        }
        Ok(ingest::oauth::Progress::Done { refresh_token, account_id }) => {
            if let Some(token) = refresh_token {
                store_sign_in(&state, account_id, &token).await?;
            }
            Ok(Json(json!({ "status": "done" })))
        }
        Ok(ingest::oauth::Progress::Failed(message)) => {
            Ok(Json(json!({ "status": "failed", "error": message })))
        }
        Err(e) => Err(ApiError::bad_request(&format!("{e:#}"))),
    }
}

// ── Senders ──────────────────────────────────────────────────────────────────

async fn list_senders(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let senders = mailbox::list_senders(&state.pool).await?;
    Ok(Json(json!({ "senders": senders })))
}

async fn update_sender(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<mailbox::SenderPatch>,
) -> Result<Json<Value>, ApiError> {
    check_opt(patch.category.as_ref(), CATEGORIES, "category")?;
    check_opt(patch.status.as_ref(), STATUSES, "status")?;
    let sender = mailbox::update_sender(&state.pool, id, &patch)
        .await?
        .ok_or_else(|| ApiError::not_found("sender not found"))?;
    Ok(Json(json!({ "sender": sender })))
}

async fn delete_sender(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !mailbox::delete_sender(&state.pool, id).await? {
        return Err(ApiError::not_found("sender not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Messages ─────────────────────────────────────────────────────────────────

async fn list_messages(
    State(state): State<AppState>,
    Query(q): Query<mailbox::MessageQuery>,
) -> Result<Json<Value>, ApiError> {
    check_opt(q.category.as_ref().filter(|c| c.as_str() != "all"), CATEGORIES, "category")?;
    let messages = mailbox::list_messages(&state.pool, &q).await?;
    Ok(Json(json!({ "messages": messages })))
}

#[derive(Deserialize)]
struct ImagesQuery {
    /// Load the remote images this message parked at ingest.
    #[serde(default)]
    images: bool,
}

async fn get_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ImagesQuery>,
) -> Result<Json<Value>, ApiError> {
    let mut message = mailbox::get_message(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("message not found"))?;
    let settings = mailbox::get_settings(&state.pool).await?;
    if q.images || settings.load_remote_images {
        message.body_html = message.body_html.map(|h| ingest::sanitize::restore_images(&h));
    }
    let attachments = mailbox::list_attachments(&state.pool, id).await?;
    let unsubscribe_url = ingest::parse::unsubscribe_url(&message.list_unsubscribe);
    Ok(Json(json!({
        "message": message,
        "attachments": attachments,
        "unsubscribe_url": unsubscribe_url,
    })))
}

async fn update_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<mailbox::MessagePatch>,
) -> Result<Json<Value>, ApiError> {
    if !mailbox::update_message(&state.pool, id, &patch).await? {
        return Err(ApiError::not_found("message not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !mailbox::delete_message(&state.pool, id).await? {
        return Err(ApiError::not_found("message not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ReadAllBody {
    #[serde(default)]
    sender_id: Option<Uuid>,
}

async fn read_all(
    State(state): State<AppState>,
    Json(body): Json<ReadAllBody>,
) -> Result<Json<Value>, ApiError> {
    let n = mailbox::mark_all_read(&state.pool, body.sender_id).await?;
    Ok(Json(json!({ "ok": true, "marked": n })))
}

/// Unsubscribe from the list this message came from. When the sender supports RFC 8058
/// the request is sent from here; otherwise the URL is handed back for the user to open,
/// because a GET on an unsubscribe link can mean anything at all.
async fn unsubscribe(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let message = mailbox::get_message(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("message not found"))?;
    let Some(url) = ingest::parse::unsubscribe_url(&message.list_unsubscribe) else {
        return Err(ApiError::bad_request(
            "this sender offers no unsubscribe link — open the message and use its own link",
        ));
    };
    if !message.list_unsub_post {
        return Ok(Json(json!({ "ok": false, "open_url": url })));
    }
    match ingest::one_click_unsubscribe(&state.http, &url).await {
        Ok(()) => {
            let patch = mailbox::SenderPatch {
                status: Some("ignored".into()),
                ..Default::default()
            };
            mailbox::update_sender(&state.pool, message.sender_id, &patch).await?;
            Ok(Json(json!({ "ok": true })))
        }
        Err(e) => Ok(Json(json!({ "ok": false, "open_url": url, "error": format!("{e:#}") }))),
    }
}

#[derive(Deserialize)]
struct RemindBody {
    /// `YYYY-MM-DD`; defaults to today.
    #[serde(default)]
    date: Option<String>,
    /// `HH:MM` local wall-clock.
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    tz_offset_minutes: i32,
}

/// "Remind me to read this" — one reminder, once, pointing back at the message.
async fn remind(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<RemindBody>,
) -> Result<Json<Value>, ApiError> {
    let message = mailbox::get_message(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("message not found"))?;
    let subject =
        if message.subject.is_empty() { "(no subject)" } else { message.subject.as_str() };
    let input = otw_store::reminders::ReminderInput {
        name: format!("Read: {subject}"),
        kind: "custom".into(),
        linked_id: None,
        details: format!("{} — {}", message.sender_name, message.sender_addr),
        url: String::new(),
        link_label: String::new(),
        frequency: "once".into(),
        weekdays: None,
        start_date: body.date,
        start_time: body.time,
        tz_offset_minutes: body.tz_offset_minutes,
        end_date: None,
        max_count: None,
        active: true,
    };
    let reminder = otw_store::reminders::add_reminder(&state.pool, &input).await?;
    Ok(Json(json!({ "reminder": reminder })))
}

async fn download_attachment(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let (filename, mime, content) = mailbox::open_attachment(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("attachment not found"))?;
    // Always an attachment, never inline: a mail payload is never rendered in our origin.
    let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', ""));
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, disposition),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        content,
    ))
}

// ── Store links ──────────────────────────────────────────────────────────────

async fn list_links(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let links = mailbox::list_links(&state.pool).await?;
    Ok(Json(json!({ "links": links })))
}

async fn create_link(
    State(state): State<AppState>,
    Json(input): Json<mailbox::StoreLinkInput>,
) -> Result<Json<Value>, ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("a name is required"));
    }
    check(&input.topic, TOPICS, "topic")?;
    let link = mailbox::create_link(&state.pool, &input).await?;
    Ok(Json(json!({ "link": link })))
}

async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<mailbox::StoreLinkPatch>,
) -> Result<Json<Value>, ApiError> {
    check_opt(patch.topic.as_ref(), TOPICS, "topic")?;
    let link = mailbox::update_link(&state.pool, id, &patch)
        .await?
        .ok_or_else(|| ApiError::not_found("link not found"))?;
    Ok(Json(json!({ "link": link })))
}

async fn delete_link(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !mailbox::delete_link(&state.pool, id).await? {
        return Err(ApiError::not_found("link not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Settings ─────────────────────────────────────────────────────────────────

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = mailbox::get_settings(&state.pool).await?;
    Ok(Json(json!({ "settings": settings })))
}

async fn put_settings(
    State(state): State<AppState>,
    Json(input): Json<mailbox::Settings>,
) -> Result<Json<Value>, ApiError> {
    let settings = mailbox::put_settings(&state.pool, &input).await?;
    Ok(Json(json!({ "settings": settings })))
}
