//! HTTP API for social sign-in and recovery codes.
//!
//! Two halves. The **public** one is the sign-in handshake (which methods are on, start a
//! social sign-in, finish it, the second factor, a recovery code). The **settings** one is
//! behind the session and the step-up grant: configure the provider, link or unlink the
//! identity, mint recovery codes, switch password login.
//!
//! Password login can only be turned off while an identity is linked and unused recovery
//! codes exist; the store enforces the first half of that too. Every way out of the linked
//! state turns password login back on, and so does a recovery-code sign-in: the social
//! account is presumed unusable at that point, and the owner should not need a second code
//! for the next sign-in.

use axum::{
    extract::State,
    http::HeaderMap,
    middleware,
    routing::{get, post},
    Extension, Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use otw_store::social_login as store;
use otw_store::User;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::social_login::{self as social, Config, Provider, Purpose};
use crate::{auth, security, security_events, ApiError, AppState};

/// The flow cookie's path: the callback and the second-factor step both live under it.
const FLOW_COOKIE_PATH: &str = "/api/auth";

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/methods", get(methods))
        .route("/api/auth/social/start", post(start_login))
        .route("/api/auth/social/callback", post(callback))
        .route("/api/auth/social/totp", post(social_totp))
        .route("/api/auth/recovery", post(recovery_login))
        .layer(middleware::from_fn(security::same_origin))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/social", get(status).put(save).delete(remove))
        .route("/api/settings/social/link", post(link_start))
        .route("/api/settings/social/unlink", post(unlink))
        .route("/api/settings/social/recovery-codes", post(regenerate_codes))
        .route("/api/settings/password-login", post(set_password_login))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn provider_of(row: &store::SocialLogin) -> Result<Provider, ApiError> {
    Provider::parse(&row.provider)
        .ok_or_else(|| ApiError::internal("the stored sign-in provider is unknown"))
}

fn config(state: &AppState, row: &store::SocialLogin) -> Result<Config, ApiError> {
    let client_secret = match row.secret() {
        Some((nonce, ct)) => Some(state.cipher.open(nonce, ct)?),
        None => None,
    };
    Ok(Config {
        provider: provider_of(row)?,
        issuer: row.issuer.clone(),
        client_id: row.client_id.clone(),
        client_secret,
    })
}

async fn flow_cookie(state: &AppState, value: String) -> Cookie<'static> {
    let https = crate::is_https_mode(&crate::network_api::effective_mode(&state.pool).await);
    Cookie::build((social::FLOW_COOKIE, value))
        .http_only(true)
        // Lax: the browser comes back from the provider on a top-level navigation, which
        // Lax still carries the cookie on.
        .same_site(SameSite::Lax)
        .secure(https)
        .path(FLOW_COOKIE_PATH)
        .max_age(time::Duration::seconds(social::FLOW_TTL.as_secs() as i64))
        .build()
}

fn without_flow_cookie(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build(social::FLOW_COOKIE).path(FLOW_COOKIE_PATH))
}

fn flow_state(jar: &CookieJar) -> Option<String> {
    jar.get(social::FLOW_COOKIE).map(|c| c.value().to_string())
}

/// A landing path the SPA may navigate to: same-origin only. `//host` and `/\host` would
/// leave the app (browsers read `\` as `/`).
fn same_origin_path(next: &str) -> String {
    let ok = next.starts_with('/') && !next.starts_with("//") && !next.contains('\\');
    if ok { next.to_string() } else { "/".to_string() }
}

/// The owner account. Single-user: whoever signs in, signs in as this one.
async fn owner(state: &AppState) -> Result<User, ApiError> {
    otw_store::find_admin(&state.pool)
        .await?
        .ok_or_else(|| ApiError::unauthorized("this instance has no account yet"))
}

fn provider_label(provider: &str) -> &str {
    match provider {
        "google" => "Google",
        "microsoft" => "Microsoft",
        "github" => "GitHub",
        _ => "the identity provider",
    }
}

// ── Public: sign-in ──────────────────────────────────────────────────────────

/// What the sign-in page offers. Public: the login form has to know before anyone is
/// signed in, and "password sign-in is off" is not a secret worth hiding from it.
async fn methods(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(user) = otw_store::find_admin(&state.pool).await? else {
        return Ok(Json(json!({ "password": true, "social": null, "recovery": false })));
    };
    let row = store::get(&state.pool).await?;
    let linked = row.as_ref().filter(|r| r.linked().is_some());
    let codes = store::recovery_codes_left(&state.pool, user.id).await?;
    Ok(Json(json!({
        "password": store::password_login(&state.pool, user.id).await?,
        "social": linked.map(|r| json!({ "provider": r.provider })),
        "recovery": codes > 0,
    })))
}

#[derive(Deserialize)]
struct StartInput {
    /// The browser's own origin: the redirect URI is built from it, so it matches the one
    /// the owner registered from the same address.
    origin: String,
    #[serde(default)]
    next: String,
}

async fn start_login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<StartInput>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let row = store::get(&state.pool)
        .await?
        .filter(|r| r.linked().is_some())
        .ok_or_else(|| ApiError::bad_request("social sign-in is not set up on this instance"))?;
    let redirect = social::redirect_uri(&input.origin)
        .ok_or_else(|| ApiError::bad_request("the page origin is not an http(s) address"))?;
    let purpose = Purpose::Login { next: same_origin_path(&input.next) };
    let (flow, url) = social::start(&state.http, config(&state, &row)?, purpose, &redirect)
        .await
        .map_err(|e| ApiError::bad_gateway(&format!("{e:#}")))?;
    let jar = jar.add(flow_cookie(&state, flow).await);
    Ok((jar, Json(json!({ "authorize_url": url }))))
}

#[derive(Deserialize)]
struct CallbackInput {
    #[serde(default)]
    state: String,
    #[serde(default)]
    code: String,
    /// The provider's refusal (the user declined, a policy blocked it), in its own words.
    #[serde(default)]
    error: String,
}

/// The provider sent the browser back. Settles a link or a sign-in.
async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<CallbackInput>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let cookie_state = flow_state(&jar);
    if !input.error.trim().is_empty() {
        if cookie_state.as_deref() == Some(input.state.as_str()) {
            social::forget(&input.state);
        }
        return Err(ApiError::bad_request(input.error.trim()));
    }
    if input.state.is_empty() || input.code.is_empty() {
        return Err(ApiError::bad_request("the provider sent back no code"));
    }
    let (purpose, identity) =
        social::finish(&state.http, &input.state, cookie_state.as_deref(), &input.code)
            .await
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;

    let row = store::get(&state.pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("social sign-in was removed meanwhile"))?;

    match purpose {
        Purpose::Link { user_id } => {
            store::link(&state.pool, user_id, &identity.issuer, &identity.subject, &identity.label)
                .await?;
            security_events::alert(
                &state,
                security_events::SOCIAL_LOGIN,
                security_events::SOCIAL_LOGIN,
                "Social sign-in linked",
                &format!(
                    "Signing in with {} is now possible, locked to {}.",
                    provider_label(&row.provider),
                    if identity.label.is_empty() { "the account just chosen" } else { &identity.label }
                ),
            );
            Ok((without_flow_cookie(jar), Json(json!({ "ok": true, "kind": "link" }))))
        }
        Purpose::Login { next } => {
            let ip = security::client_ip(&headers);
            let Some((user_id, iss, sub)) = row.linked() else {
                return Err(ApiError::bad_request("social sign-in is not set up on this instance"));
            };
            if identity.issuer != iss || identity.subject != sub {
                crate::record_login_failure(
                    &state,
                    &ip,
                    &format!("social sign-in with an account that is not the linked one ({})", identity.label),
                );
                return Err(ApiError::unauthorized(&format!(
                    "this {} account is not the one linked to this instance",
                    provider_label(&row.provider)
                )));
            }
            let user = otw_store::find_user_by_id(&state.pool, user_id)
                .await?
                .ok_or_else(|| ApiError::unauthorized("the linked account no longer exists"))?;
            if user.totp_enabled {
                social::await_code(&input.state, user.id, next);
                return Err(ApiError::unauthorized_code(
                    "totp_required",
                    "enter the six-digit code from your authenticator",
                ));
            }
            let method = provider_label(&row.provider).to_string();
            let jar = crate::complete_sign_in(&state, without_flow_cookie(jar), &user, &headers, &method)
                .await?;
            Ok((
                jar,
                Json(json!({
                    "ok": true,
                    "kind": "login",
                    "next": next,
                    "must_change_password": user.must_change_password,
                })),
            ))
        }
    }
}

#[derive(Deserialize)]
struct CodeInput {
    code: String,
}

/// The second factor owed by a social sign-in on an account that has one.
async fn social_totp(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<CodeInput>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let ip = security::client_ip(&headers);
    tokio::time::sleep(crate::login_delay(&ip)).await;

    let expired = || ApiError::unauthorized("this sign-in has expired; start it again");
    let flow = flow_state(&jar).ok_or_else(expired)?;
    let (user_id, next) = social::awaiting(&flow).ok_or_else(expired)?;
    let user = otw_store::find_user_by_id(&state.pool, user_id).await?.ok_or_else(expired)?;

    if !crate::verify_totp(&state, &user, input.code.trim()).await? {
        social::code_failed(&flow);
        crate::record_login_failure(&state, &ip, &format!("wrong second factor for '{}'", user.username));
        return Err(ApiError::unauthorized("that code is not valid"));
    }
    social::forget(&flow);
    let row = store::get(&state.pool).await?;
    let method = provider_label(row.as_ref().map(|r| r.provider.as_str()).unwrap_or("")).to_string();
    let jar = crate::complete_sign_in(&state, without_flow_cookie(jar), &user, &headers, &method).await?;
    Ok((
        jar,
        Json(json!({ "ok": true, "next": next, "must_change_password": user.must_change_password })),
    ))
}

#[derive(Deserialize)]
struct RecoveryInput {
    code: String,
    /// Authenticator code, when the account has a second factor.
    #[serde(default)]
    totp: Option<String>,
}

/// Sign in with a recovery code: the way back when the social account is unusable.
///
/// The second factor is still asked for when enrolled: a recovery code stands in for the
/// social identity, not for the phone. A code is only spent once everything else checked
/// out, so a missing authenticator code does not burn one.
async fn recovery_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<RecoveryInput>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let ip = security::client_ip(&headers);
    tokio::time::sleep(crate::login_delay(&ip)).await;

    let user = owner(&state).await?;
    let invalid = || ApiError::unauthorized("that recovery code is not valid");
    let Some(hash) = auth::recovery::hash(&input.code) else {
        crate::record_login_failure(&state, &ip, "malformed recovery code");
        return Err(invalid());
    };
    if !store::recovery_code_valid(&state.pool, user.id, &hash).await? {
        crate::record_login_failure(&state, &ip, "wrong recovery code");
        return Err(invalid());
    }
    if user.totp_enabled {
        let Some(code) = input.totp.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            return Err(ApiError::unauthorized_code(
                "totp_required",
                "enter the six-digit code from your authenticator",
            ));
        };
        if !crate::verify_totp(&state, &user, code).await? {
            crate::record_login_failure(&state, &ip, &format!("wrong second factor for '{}'", user.username));
            return Err(ApiError::unauthorized("that code is not valid"));
        }
    }
    if !store::consume_recovery_code(&state.pool, user.id, &hash).await? {
        // Spent by a concurrent request between the check and here.
        return Err(invalid());
    }
    store::set_password_login(&state.pool, user.id, true).await?;
    let left = store::recovery_codes_left(&state.pool, user.id).await?;

    let jar = crate::complete_sign_in(&state, jar, &user, &headers, "recovery code").await?;
    // The code proved more than a password does, so the session may relink or unlink the
    // social account straight away without being asked for a password it may not remember.
    if let Some(key) = crate::session_token(&jar).map(|t| otw_store::mcp::hash_token(&t)) {
        security::STEP_UP.grant(&key);
    }
    security_events::alert(
        &state,
        security_events::RECOVERY_CODE_USED,
        security_events::RECOVERY_CODE_USED,
        "A recovery code was used to sign in",
        &format!(
            "Source: {ip}\nRecovery codes left: {left}\n\nPassword sign-in is on again. If this \
             was not you, generate new codes and change the password now."
        ),
    );
    Ok((
        jar,
        Json(json!({
            "ok": true,
            "codes_left": left,
            "must_change_password": user.must_change_password,
        })),
    ))
}

// ── Settings ─────────────────────────────────────────────────────────────────

async fn status(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<Value>, ApiError> {
    let row = store::get(&state.pool).await?;
    let linked = row.as_ref().and_then(|r| {
        let linked_at = r
            .linked_at
            .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok());
        r.linked().map(|_| json!({ "label": r.email, "linked_at": linked_at }))
    });
    Ok(Json(json!({
        "config": row.as_ref().map(|r| json!({
            "provider": r.provider,
            "issuer": r.issuer,
            "client_id": r.client_id,
            "has_secret": r.secret().is_some(),
        })),
        "linked": linked,
        "password_login": store::password_login(&state.pool, user.id).await?,
        "recovery_codes_left": store::recovery_codes_left(&state.pool, user.id).await?,
        "callback_path": social::CALLBACK_PATH,
    })))
}

#[derive(Deserialize)]
struct SaveInput {
    provider: String,
    #[serde(default)]
    issuer: String,
    client_id: String,
    /// Empty or absent keeps the stored secret.
    #[serde(default)]
    client_secret: Option<String>,
}

async fn save(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(input): Json<SaveInput>,
) -> Result<Json<Value>, ApiError> {
    security::require_step_up(&jar)?;
    let existing = store::get(&state.pool).await?;
    let provider = Provider::parse(input.provider.trim())
        .ok_or_else(|| ApiError::bad_request("unknown provider"))?;
    let client_id = input.client_id.trim();
    if client_id.is_empty() || client_id.len() > 512 {
        return Err(ApiError::bad_request("the client ID is required"));
    }
    let issuer = match provider {
        Provider::Google | Provider::Github => String::new(),
        Provider::Microsoft => {
            let tenant = input.issuer.trim();
            if !tenant.is_empty() && !social::valid_tenant(tenant) {
                return Err(ApiError::bad_request(
                    "the tenant is `common`, `organizations`, `consumers`, a tenant ID or a domain",
                ));
            }
            tenant.to_string()
        }
        Provider::Oidc => {
            social::normalize_issuer(&input.issuer).map_err(|e| ApiError::bad_request(&e))?
        }
    };
    // While an account is linked, only the secret may change (an expired one is rotated in
    // place): another provider, tenant, issuer or client would not issue the same subject.
    if let Some(r) = existing.as_ref().filter(|r| r.linked().is_some()) {
        if r.provider != provider.as_str() || r.issuer != issuer || r.client_id != client_id {
            return Err(ApiError::conflict(
                "an account is linked: unlink it before changing the provider, tenant, issuer \
                 or client ID (the client secret alone can be replaced)",
            ));
        }
    }
    let new_secret = input
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    // A stored secret belongs to the client it was issued with.
    let keeps_secret = existing
        .as_ref()
        .is_some_and(|r| r.secret().is_some() && r.provider == provider.as_str() && r.client_id == client_id);
    if new_secret.is_none() && !keeps_secret && matches!(provider, Provider::Google | Provider::Github) {
        return Err(ApiError::bad_request("this provider needs the client secret"));
    }

    let client_secret = match &new_secret {
        Some(s) => Some(s.clone()),
        None if keeps_secret => existing
            .as_ref()
            .map(|r| config(&state, r))
            .transpose()?
            .and_then(|c| c.client_secret),
        None => None,
    };
    let cfg = Config { provider, issuer: issuer.clone(), client_id: client_id.to_string(), client_secret };
    social::check(&state.http, &cfg)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;

    let sealed = new_secret.as_deref().map(|s| state.cipher.seal(s)).transpose()?;
    store::save_config(
        &state.pool,
        provider.as_str(),
        &issuer,
        client_id,
        sealed.as_ref().map(|(n, c)| (n.as_slice(), c.as_slice())),
    )
    .await?;
    if new_secret.is_none() && !keeps_secret {
        store::clear_secret(&state.pool).await?;
    }
    security_events::log(
        security_events::SOCIAL_LOGIN,
        &format!("social sign-in configured for {}", provider_label(provider.as_str())),
    );
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    security::require_step_up(&jar)?;
    store::delete(&state.pool, user.id).await?;
    security_events::alert(
        &state,
        security_events::SOCIAL_LOGIN,
        security_events::SOCIAL_LOGIN,
        "Social sign-in removed",
        "Social sign-in was removed. Password sign-in is on.",
    );
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct LinkInput {
    origin: String,
}

/// Send the owner to the provider to choose the account sign-in will be locked to.
async fn link_start(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Json(input): Json<LinkInput>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    security::require_step_up(&jar)?;
    let row = store::get(&state.pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("configure a provider first"))?;
    if row.linked().is_some() {
        return Err(ApiError::conflict("an account is already linked: unlink it first"));
    }
    let redirect = social::redirect_uri(&input.origin)
        .ok_or_else(|| ApiError::bad_request("the page origin is not an http(s) address"))?;
    let (flow, url) = social::start(
        &state.http,
        config(&state, &row)?,
        Purpose::Link { user_id: user.id },
        &redirect,
    )
    .await
    .map_err(|e| ApiError::bad_gateway(&format!("{e:#}")))?;
    let jar = jar.add(flow_cookie(&state, flow).await);
    Ok((jar, Json(json!({ "authorize_url": url }))))
}

async fn unlink(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    security::require_step_up(&jar)?;
    store::unlink(&state.pool, user.id).await?;
    security_events::alert(
        &state,
        security_events::SOCIAL_LOGIN,
        security_events::SOCIAL_LOGIN,
        "Social sign-in unlinked",
        "The linked account was removed. Password sign-in is on.",
    );
    Ok(Json(json!({ "ok": true })))
}

/// Mint a fresh set of recovery codes, replacing the old ones. Shown once.
async fn regenerate_codes(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    security::require_step_up(&jar)?;
    let linked = store::get(&state.pool).await?.is_some_and(|r| r.linked().is_some());
    if !linked {
        return Err(ApiError::bad_request("link a social account first"));
    }
    let codes = (0..auth::recovery::COUNT)
        .map(|_| auth::recovery::generate())
        .collect::<anyhow::Result<Vec<_>>>()?;
    let hashes: Vec<String> = codes.iter().filter_map(|c| auth::recovery::hash(c)).collect();
    store::replace_recovery_codes(&state.pool, user.id, &hashes).await?;
    security_events::log(
        security_events::SOCIAL_LOGIN,
        "a new set of recovery codes was generated; the previous ones no longer work",
    );
    Ok(Json(json!({ "codes": codes })))
}

#[derive(Deserialize)]
struct PasswordLoginInput {
    enabled: bool,
}

async fn set_password_login(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Json(input): Json<PasswordLoginInput>,
) -> Result<Json<Value>, ApiError> {
    security::require_step_up(&jar)?;
    if !input.enabled {
        let linked = store::get(&state.pool)
            .await?
            .and_then(|r| r.linked().map(|(u, _, _)| u))
            == Some(user.id);
        if !linked {
            return Err(ApiError::bad_request(
                "link a social account before turning password sign-in off",
            ));
        }
        if store::recovery_codes_left(&state.pool, user.id).await? == 0 {
            return Err(ApiError::bad_request(
                "generate recovery codes first: they are the way back in if the social account \
                 is locked or deleted",
            ));
        }
    }
    if !store::set_password_login(&state.pool, user.id, input.enabled).await? {
        return Err(ApiError::bad_request("password sign-in cannot be turned off right now"));
    }
    security_events::alert(
        &state,
        security_events::SOCIAL_LOGIN,
        &format!("{}:password:{}", security_events::SOCIAL_LOGIN, input.enabled),
        if input.enabled { "Password sign-in turned on" } else { "Password sign-in turned off" },
        if input.enabled {
            "Username and password sign in again."
        } else {
            "Only the linked social account and the recovery codes sign in now."
        },
    );
    Ok(Json(json!({ "ok": true, "password_login": input.enabled })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn landing_paths_stay_on_this_origin() {
        assert_eq!(same_origin_path("/journal"), "/journal");
        assert_eq!(same_origin_path("//evil.example"), "/");
        assert_eq!(same_origin_path("/\\evil.example"), "/");
        assert_eq!(same_origin_path("https://evil.example"), "/");
        assert_eq!(same_origin_path(""), "/");
    }
}
