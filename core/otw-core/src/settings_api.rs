//! HTTP API for the Settings page.
//!
//! Covers: current account + credential change, logout, global defaults
//! (currency/timezone), the app-logs viewer with a runtime log-level changer, per-module
//! data usage + wipe, and a version/about endpoint. Heavy host-level actions (full DB
//! backup, app update, DB restore) are intentionally *not* here — the frontend guides the
//! operator to run them on the host, keeping the distroless container free of shell/Docker
//! access.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    routing::{get, post},
    Extension, Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use otw_store::User;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{auth, security, ApiError, AppState, SESSION_COOKIE};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/me", get(me))
        .route("/api/settings/account", post(update_account))
        .route("/api/settings/logout", post(logout))
        .route("/api/settings/reauth", get(reauth_status).post(reauth))
        .route("/api/settings/sessions", get(list_sessions))
        .route("/api/settings/sessions/revoke", post(revoke_session))
        .route("/api/settings/sessions/revoke-others", post(revoke_other_sessions))
        .route("/api/settings/totp", get(totp_status).delete(totp_disable))
        .route("/api/settings/totp/enroll", post(totp_enroll))
        .route("/api/settings/totp/confirm", post(totp_confirm))
        .route("/api/settings/request-timeout", get(get_request_timeout).post(set_request_timeout))
        .route("/api/settings/defaults", get(get_defaults).post(set_defaults))
        .route("/api/settings/version", get(version))
        .route("/api/settings/backup/status", get(backup_status))
        .route("/api/settings/update-check", get(update_check))
        .route("/api/settings/data", get(data_usage))
        .route("/api/settings/data/wipe", post(wipe_data))
        .route("/api/settings/modules", get(list_modules))
        .route("/api/settings/modules/install", post(install_module))
        .route("/api/settings/modules/detach", post(detach_module))
        .route("/api/settings/logs", get(list_logs).delete(clear_logs))
        .route("/api/settings/logs/level", get(get_log_level).post(set_log_level))
}

// The authenticated `User` is injected by the `require_auth` middleware (main.rs) on all
// protected routes, so handlers extract it directly via `Extension`.

// ── Account ──────────────────────────────────────────────────────────────────

async fn me(Extension(user): Extension<User>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "username": user.username,
        "is_admin": user.is_admin,
        "totp_enabled": user.totp_enabled,
    })))
}

#[derive(Deserialize)]
struct AccountUpdate {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    new_password: Option<String>,
    /// Required to authorize any change.
    current_password: String,
}

/// Change username and/or password. Requires the current password. A password change
/// revokes all sessions (the client must sign in again).
async fn update_account(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(input): Json<AccountUpdate>,
) -> Result<Json<Value>, ApiError> {
    if !auth::verify_password(&input.current_password, &user.password_hash) {
        return Err(ApiError::unauthorized("current password is incorrect"));
    }

    let username = input
        .username
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty() && *u != user.username);

    let mut password_revoked = false;
    let hash = match input.new_password {
        Some(p) if !p.is_empty() => {
            // Screened against the same policy as the first-run wizard: a password chosen
            // later is not held to a lower bar than the one chosen on day one.
            let name = username.as_deref().unwrap_or(&user.username);
            auth::check_password(&p, name).map_err(|e| ApiError::bad_request(&e))?;
            password_revoked = true;
            Some(auth::hash_password(&p)?)
        }
        _ => None,
    };

    if username.is_none() && hash.is_none() {
        return Err(ApiError::bad_request("nothing to change"));
    }

    otw_store::update_credentials(&state.pool, user.id, username.as_deref(), hash.as_deref())
        .await?;
    Ok(Json(json!({ "ok": true, "password_changed": password_revoked })))
}

/// Clear the current session and expire the cookie. Both cookie names are removed: a
/// deployment that switched between HTTP and HTTPS may have left the other one behind.
async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    if let Some(token) = crate::session_token(&jar) {
        otw_store::delete_session(&state.pool, &token).await?;
    }
    if let Some(key) = security::session_key(&jar) {
        security::STEP_UP.revoke(&key);
    }
    let jar = jar
        .remove(axum_extra::extract::cookie::Cookie::from(SESSION_COOKIE))
        .remove(axum_extra::extract::cookie::Cookie::from(
            crate::SESSION_COOKIE_HOST,
        ));
    Ok((jar, Json(json!({ "ok": true }))))
}

// ── Step-up re-authentication ────────────────────────────────────────────────

#[derive(Deserialize)]
struct Reauth {
    password: String,
    /// Required when the account has a second factor: re-proving identity means both
    /// factors, or the step-up would be weaker than the sign-in it stands in for.
    #[serde(default)]
    code: Option<String>,
}

/// Whether this session already holds a step-up grant.
///
/// Exists for the one action that cannot be refused and retried: exporting a bundle is a
/// plain browser navigation, so the page has to know beforehand whether to prompt.
async fn reauth_status(jar: CookieJar) -> Json<Value> {
    let valid = security::session_key(&jar)
        .map(|k| security::STEP_UP.is_valid(&k))
        .unwrap_or(false);
    Json(json!({ "valid": valid }))
}

/// Re-enter the password (and the TOTP code, if enrolled) to unlock sensitive actions for
/// the next few minutes. The grant is tied to this session and dies with it.
async fn reauth(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
    Json(input): Json<Reauth>,
) -> Result<Json<Value>, ApiError> {
    let Some(key) = security::session_key(&jar) else {
        return Err(ApiError::unauthorized("not signed in"));
    };
    if !auth::verify_password(&input.password, &user.password_hash) {
        // Same counter the login route feeds: a password guessed here is a password guessed.
        return Err(ApiError::unauthorized("that password is not correct"));
    }
    if user.totp_enabled {
        let code = input.code.as_deref().map(str::trim).unwrap_or("");
        if code.is_empty() {
            return Err(ApiError::unauthorized_code(
                "totp_required",
                "enter the six-digit code from your authenticator",
            ));
        }
        if !crate::verify_totp(&state, &user, code).await? {
            return Err(ApiError::unauthorized("that code is not valid"));
        }
    }
    security::STEP_UP.grant(&key);
    Ok(Json(json!({
        "ok": true,
        "valid_for_seconds": security::STEP_UP_TTL.as_secs(),
    })))
}

// ── Sessions ─────────────────────────────────────────────────────────────────

/// Every live session of this account, with where and when it was last used. The row that
/// made this request is marked, so the UI can label it and keep it out of the way.
async fn list_sessions(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let current = security::session_key(&jar).unwrap_or_default();
    let rows = otw_store::list_sessions(&state.pool, user.id).await?;
    let sessions: Vec<Value> = rows
        .into_iter()
        .map(|s| {
            let current = s.id == current;
            let mut v = serde_json::to_value(&s).unwrap_or_else(|_| json!({}));
            v["current"] = json!(current);
            v
        })
        .collect();
    Ok(Json(json!({ "sessions": sessions })))
}

#[derive(Deserialize)]
struct SessionRef {
    id: String,
}

/// Close one other session. Closing the current one is what logout is for.
async fn revoke_session(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(input): Json<SessionRef>,
) -> Result<Json<Value>, ApiError> {
    let n = otw_store::revoke_session(&state.pool, user.id, input.id.trim()).await?;
    if n == 0 {
        return Err(ApiError::not_found("no such session"));
    }
    security::STEP_UP.revoke(input.id.trim());
    tracing::warn!("a session was revoked from Settings");
    Ok(Json(json!({ "ok": true })))
}

/// Close every session but this one. The button for "I signed in somewhere I should not
/// have", and the reason the list above exists at all.
async fn revoke_other_sessions(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    let Some(token) = crate::session_token(&jar) else {
        return Err(ApiError::unauthorized("not signed in"));
    };
    let n = otw_store::revoke_other_sessions(&state.pool, user.id, &token).await?;
    tracing::warn!("{n} other session(s) revoked from Settings");
    Ok(Json(json!({ "ok": true, "revoked": n })))
}

// ── Second factor ────────────────────────────────────────────────────────────

async fn totp_status(Extension(user): Extension<User>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "enabled": user.totp_enabled })))
}

/// Start enrolment: mint a secret, seal it, and hand back the `otpauth://` URI for the QR
/// code. Nothing changes about signing in until [`totp_confirm`] sees a correct code.
///
/// Re-enrolling while a second factor is already active needs the step-up grant: otherwise
/// a stolen session could quietly swap the factor for one the attacker holds.
async fn totp_enroll(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    if user.totp_enabled {
        security::require_step_up(&jar)?;
    }
    let secret = auth::totp::generate_secret()?;
    let (nonce, ct) = state.cipher.seal(&secret)?;
    otw_store::set_totp_secret(&state.pool, user.id, &nonce, &ct).await?;
    Ok(Json(json!({
        "secret": secret,
        "uri": auth::totp::uri(&secret, &user.username),
    })))
}

#[derive(Deserialize)]
struct TotpCode {
    code: String,
}

/// Finish enrolment. A correct code proves the authenticator holds the same secret, which
/// is the only evidence that turning the requirement on will not lock the user out.
async fn totp_confirm(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(input): Json<TotpCode>,
) -> Result<Json<Value>, ApiError> {
    if !crate::verify_totp(&state, &user, input.code.trim()).await? {
        return Err(ApiError::bad_request(
            "that code is not valid. Check the time on the device running the authenticator.",
        ));
    }
    otw_store::enable_totp(&state.pool, user.id).await?;
    tracing::warn!("two-factor authentication enabled for '{}'", user.username);
    Ok(Json(json!({ "ok": true })))
}

/// Turn the second factor off. Needs the step-up grant: removing a factor is exactly what
/// someone holding a stolen session would want to do first.
async fn totp_disable(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    jar: CookieJar,
) -> Result<Json<Value>, ApiError> {
    security::require_step_up(&jar)?;
    otw_store::disable_totp(&state.pool, user.id).await?;
    tracing::warn!("two-factor authentication disabled for '{}'", user.username);
    Ok(Json(json!({ "ok": true })))
}

// ── Request timeout ──────────────────────────────────────────────────────────

async fn get_request_timeout() -> Json<Value> {
    Json(json!({
        "seconds": security::request_timeout_secs(),
        "default": security::DEFAULT_REQUEST_TIMEOUT_SECS,
        "min": security::MIN_REQUEST_TIMEOUT_SECS,
        "max": security::MAX_REQUEST_TIMEOUT_SECS,
    }))
}

#[derive(Deserialize)]
struct TimeoutBody {
    seconds: u64,
}

/// Change how long a single request may take. Applies immediately, no restart.
async fn set_request_timeout(
    State(state): State<AppState>,
    Json(input): Json<TimeoutBody>,
) -> Result<Json<Value>, ApiError> {
    let secs = input.seconds;
    if !(security::MIN_REQUEST_TIMEOUT_SECS..=security::MAX_REQUEST_TIMEOUT_SECS).contains(&secs) {
        return Err(ApiError::bad_request(&format!(
            "the request timeout must be between {} and {} seconds",
            security::MIN_REQUEST_TIMEOUT_SECS,
            security::MAX_REQUEST_TIMEOUT_SECS
        )));
    }
    otw_store::settings::set(&state.pool, "request_timeout_secs", &secs.to_string()).await?;
    security::set_request_timeout_secs(secs);
    Ok(Json(json!({ "ok": true, "seconds": secs })))
}

// ── Defaults ─────────────────────────────────────────────────────────────────

async fn get_defaults(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let currency = otw_store::settings::get_or(&state.pool, "default_currency", "USD").await?;
    let timezone = otw_store::settings::get_or(&state.pool, "default_timezone", "UTC").await?;
    let locale = otw_store::settings::get_or(&state.pool, "locale", "en").await?;
    // Empty string = follow the theme default (no custom app accent).
    let accent = otw_store::settings::get_or(&state.pool, "accent", "").await?;
    Ok(Json(json!({
        "default_currency": currency,
        "default_timezone": timezone,
        "locale": locale,
        "accent": accent,
    })))
}

/// Supported UI locales. Keep in sync with the frontend `LOCALES` registry.
const SUPPORTED_LOCALES: &[&str] = &["en", "fr", "it", "es", "de", "pt", "zh"];

#[derive(Deserialize)]
struct Defaults {
    #[serde(default)]
    default_currency: Option<String>,
    #[serde(default)]
    default_timezone: Option<String>,
    #[serde(default)]
    locale: Option<String>,
    /// Custom app-accent as `#rrggbb`, or an empty string to clear back to the theme default.
    #[serde(default)]
    accent: Option<String>,
}

/// A 6-digit hex color, `#rrggbb`.
fn is_hex_color(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

async fn set_defaults(
    State(state): State<AppState>,
    Json(input): Json<Defaults>,
) -> Result<Json<Value>, ApiError> {
    if let Some(c) = input.default_currency {
        let c = c.trim().to_uppercase();
        if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return Err(ApiError::bad_request("currency must be a 3-letter code"));
        }
        otw_store::settings::set(&state.pool, "default_currency", &c).await?;
    }
    if let Some(tz) = input.default_timezone {
        let tz = tz.trim().to_string();
        if tz.is_empty() {
            return Err(ApiError::bad_request("timezone required"));
        }
        otw_store::settings::set(&state.pool, "default_timezone", &tz).await?;
    }
    if let Some(loc) = input.locale {
        let loc = loc.trim().to_lowercase();
        if !SUPPORTED_LOCALES.contains(&loc.as_str()) {
            return Err(ApiError::bad_request("unsupported locale"));
        }
        otw_store::settings::set(&state.pool, "locale", &loc).await?;
    }
    if let Some(a) = input.accent {
        let a = a.trim();
        if a.is_empty() {
            // Clear back to the theme default.
            otw_store::settings::set(&state.pool, "accent", "").await?;
        } else if is_hex_color(a) {
            otw_store::settings::set(&state.pool, "accent", &a.to_lowercase()).await?;
        } else {
            return Err(ApiError::bad_request("accent must be a #rrggbb hex color"));
        }
    }
    get_defaults(State(state)).await
}

// ── Version / about ──────────────────────────────────────────────────────────

async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

// ── Backup status ────────────────────────────────────────────────────────────

/// Status file written by the host-side backup script after every run. Core never runs the
/// backup itself (distroless, no shell, no Docker access): it only reads this one small
/// file, which the script drops into a shared volume.
///
/// Absent is the normal case, not an error. A laptop install that backs up by hand has no
/// such file, and then this answers `configured: false` and the UI shows exactly what it
/// showed before. Nothing in the app depends on a scheduled backup existing, and there is
/// no switch to turn on: the file *is* the switch, so an assisted server install and a
/// hand-run laptop install need no mode, no checkbox, and can be mixed freely.
const BACKUP_STATUS_DEFAULT: &str = "/data/backup/last-backup.json";

/// Beyond this, a scheduled backup is treated as overdue by the UI. A nightly job that has
/// not reported in two days is broken, whatever it says about its last success.
const BACKUP_STALE_AFTER: Duration = Duration::from_secs(48 * 3600);

/// Last scheduled-backup result, or `configured: false` when no script reports here.
///
/// Every field beyond `at` is passed through as the script wrote it: the app is a viewer,
/// not the source of truth, so a newer script can add fields without a core release. Unparsable
/// or unreadable is reported as not configured rather than as an error, because a broken
/// marker must never make the Settings page fail.
async fn backup_status() -> Json<Value> {
    let path = std::env::var("OTW_BACKUP_STATUS")
        .unwrap_or_else(|_| BACKUP_STATUS_DEFAULT.to_string());
    let Some(marker) = tokio::fs::read_to_string(&path)
        .await
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(Value::is_object)
    else {
        return Json(json!({ "configured": false }));
    };

    // `at` is RFC3339 UTC, written by the script at the end of its run. Age is computed
    // here rather than stored, so a stopped scheduler shows an ageing timestamp instead of
    // a frozen "ok".
    let age_seconds = marker
        .get("at")
        .and_then(Value::as_str)
        .and_then(|at| {
            time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|at| (time::OffsetDateTime::now_utc() - at).whole_seconds().max(0));

    let ok = marker.get("result").and_then(Value::as_str) == Some("ok");
    let stale = age_seconds.is_none_or(|age| age as u64 > BACKUP_STALE_AFTER.as_secs());

    let mut out = marker;
    let obj = out.as_object_mut().expect("filtered to objects above");
    obj.insert("configured".into(), json!(true));
    obj.insert("age_seconds".into(), json!(age_seconds));
    obj.insert("healthy".into(), json!(ok && !stale));
    obj.insert("stale".into(), json!(stale));
    Json(out)
}

// ── Update check ─────────────────────────────────────────────────────────────

/// Raw workspace manifest on master. Updates ship as `git pull` from master (see the
/// Update section), so master's `[workspace.package] version` *is* the latest release.
const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/core/Cargo.toml";

/// Successful checks are cached in-process so reopening the Update page doesn't re-hit
/// GitHub. Failures are not cached: the next open retries.
const UPDATE_CHECK_TTL: Duration = Duration::from_secs(6 * 3600);

static UPDATE_CHECK_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Compare the running version against master. An unreachable GitHub is a normal
/// condition for a self-hosted box, so that reports `latest: null` rather than an error.
async fn update_check(State(state): State<AppState>) -> Json<Value> {
    let current = env!("CARGO_PKG_VERSION");
    let cached = UPDATE_CHECK_CACHE
        .lock()
        .unwrap()
        .clone()
        .filter(|(at, _)| at.elapsed() < UPDATE_CHECK_TTL)
        .map(|(_, v)| v);
    let latest = match cached {
        Some(v) => Some(v),
        None => {
            let fetched = fetch_latest_version(&state).await;
            if let Some(v) = &fetched {
                *UPDATE_CHECK_CACHE.lock().unwrap() = Some((Instant::now(), v.clone()));
            }
            fetched
        }
    };
    let newer = latest.as_deref().is_some_and(|l| version_newer(l, current));
    Json(json!({ "current": current, "latest": latest, "update_available": newer }))
}

async fn fetch_latest_version(state: &AppState) -> Option<String> {
    let res = match state.http.get(MANIFEST_URL).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("update check: manifest returned HTTP {}", r.status().as_u16());
            return None;
        }
        Err(e) => {
            tracing::warn!("update check: manifest fetch failed: {e}");
            return None;
        }
    };
    let body = res.text().await.ok()?;
    parse_workspace_version(&body)
}

/// The first `version = "…"` inside the `[workspace.package]` table — never a dependency
/// pin, which all live under other section headers.
fn parse_workspace_version(manifest: &str) -> Option<String> {
    let mut in_section = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[workspace.package]";
        } else if in_section {
            if let Some(rest) = line.strip_prefix("version") {
                let v = rest.trim_start().strip_prefix('=')?.trim().trim_matches('"');
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Strictly-newer x.y.z compare. Non-numeric components read as 0, so a malformed remote
/// version can never announce an update.
fn version_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> [u64; 3] {
        let mut out = [0u64; 3];
        for (i, p) in v.split('.').take(3).enumerate() {
            out[i] = p.trim().parse().unwrap_or(0);
        }
        out
    }
    parts(latest) > parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_version_ignores_dependency_pins() {
        let manifest = r#"
[workspace]
members = ["otw-core"]

[workspace.dependencies]
serde = { version = "1.0" }

[workspace.package]
version = "0.2.3"
edition = "2021"
"#;
        assert_eq!(parse_workspace_version(manifest).as_deref(), Some("0.2.3"));
        assert_eq!(parse_workspace_version("[workspace]\nmembers = []"), None);
    }

    #[test]
    fn version_compare() {
        assert!(version_newer("0.0.2", "0.0.1"));
        assert!(version_newer("0.0.10", "0.0.9"));
        assert!(version_newer("1.0.0", "0.9.9"));
        assert!(!version_newer("0.0.1", "0.0.1"));
        assert!(!version_newer("0.0.1", "0.0.2"));
        assert!(!version_newer("garbage", "0.0.1"));
    }
}

// ── Data management ──────────────────────────────────────────────────────────

async fn data_usage(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let usage = otw_store::data_admin::usage(&state.pool).await?;
    Ok(Json(json!(usage)))
}

#[derive(Deserialize)]
struct WipeInput {
    module: String,
}

async fn wipe_data(
    State(state): State<AppState>,
    Json(input): Json<WipeInput>,
) -> Result<Json<Value>, ApiError> {
    // The route is already behind require_auth; reaching here means a valid session.
    match otw_store::data_admin::wipe_module(&state.pool, &input.module).await? {
        Some(name) => {
            tracing::warn!("wiped all data for module {}", input.module);
            Ok(Json(json!({ "ok": true, "module": name })))
        }
        None => Err(ApiError::bad_request("unknown module")),
    }
}

// ── Modules (install / detach) ───────────────────────────────────────────────

/// The installed module ids. The frontend registry holds names/icons/descriptions; the
/// backend only tracks which feature modules are available.
async fn list_modules(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let installed = otw_store::data_admin::installed_modules(&state.pool).await?;
    Ok(Json(json!({ "installed": installed })))
}

#[derive(Deserialize)]
struct ModuleInput {
    module: String,
}

async fn install_module(
    State(state): State<AppState>,
    Json(input): Json<ModuleInput>,
) -> Result<Json<Value>, ApiError> {
    if !otw_store::data_admin::install_module(&state.pool, &input.module).await? {
        return Err(ApiError::bad_request("unknown module"));
    }
    tracing::info!("installed module {}", input.module);
    let installed = otw_store::data_admin::installed_modules(&state.pool).await?;
    Ok(Json(json!({ "ok": true, "installed": installed })))
}

#[derive(Deserialize)]
struct DetachInput {
    module: String,
    /// When true, also wipe the module's stored data on detach.
    #[serde(default)]
    wipe_data: bool,
}

async fn detach_module(
    State(state): State<AppState>,
    Json(input): Json<DetachInput>,
) -> Result<Json<Value>, ApiError> {
    if !otw_store::data_admin::detach_module(&state.pool, &input.module, input.wipe_data).await? {
        return Err(ApiError::bad_request("unknown module"));
    }
    if input.wipe_data {
        tracing::warn!("detached module {} and wiped its data", input.module);
    } else {
        tracing::info!("detached module {}", input.module);
    }
    let installed = otw_store::data_admin::installed_modules(&state.pool).await?;
    Ok(Json(json!({ "ok": true, "installed": installed })))
}

// ── Logs ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_logs(
    State(state): State<AppState>,
    Query(q): Query<LogQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let logs = otw_store::logs::list(
        &state.pool,
        q.level.as_deref(),
        q.search.as_deref().filter(|s| !s.is_empty()),
        limit,
    )
    .await?;
    Ok(Json(json!({ "logs": logs })))
}

async fn clear_logs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let removed = otw_store::logs::clear(&state.pool).await?;
    Ok(Json(json!({ "ok": true, "removed": removed })))
}

async fn get_log_level() -> Json<Value> {
    Json(json!({
        "level": otw_store::logs::min_level_name(),
        "levels": ["error", "warn", "info", "debug", "trace"],
    }))
}

#[derive(Deserialize)]
struct LevelInput {
    level: String,
}

async fn set_log_level(
    State(state): State<AppState>,
    Json(input): Json<LevelInput>,
) -> Result<Json<Value>, ApiError> {
    if !otw_store::logs::set_min_level(&input.level) {
        return Err(ApiError::bad_request("unknown log level"));
    }
    // Persist so the level survives restarts (re-seeded in main()).
    otw_store::settings::set(&state.pool, "log_level", otw_store::logs::min_level_name())
        .await?;
    tracing::info!("log capture level set to {}", input.level);
    Ok(Json(json!({ "ok": true, "level": otw_store::logs::min_level_name() })))
}
