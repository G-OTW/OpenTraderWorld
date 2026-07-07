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

use crate::{auth, ApiError, AppState, SESSION_COOKIE};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/me", get(me))
        .route("/api/settings/account", post(update_account))
        .route("/api/settings/logout", post(logout))
        .route("/api/settings/defaults", get(get_defaults).post(set_defaults))
        .route("/api/settings/version", get(version))
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
    Ok(Json(json!({ "username": user.username, "is_admin": user.is_admin })))
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
            if p.len() < 8 {
                return Err(ApiError::bad_request("new password must be at least 8 characters"));
            }
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

/// Clear the current session and expire the cookie.
async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        otw_store::delete_session(&state.pool, c.value()).await?;
    }
    let jar = jar.remove(SESSION_COOKIE);
    Ok((jar, Json(json!({ "ok": true }))))
}

// ── Defaults ─────────────────────────────────────────────────────────────────

async fn get_defaults(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let currency = otw_store::settings::get_or(&state.pool, "default_currency", "USD").await?;
    let timezone = otw_store::settings::get_or(&state.pool, "default_timezone", "UTC").await?;
    let locale = otw_store::settings::get_or(&state.pool, "locale", "en").await?;
    Ok(Json(
        json!({ "default_currency": currency, "default_timezone": timezone, "locale": locale }),
    ))
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
    get_defaults(State(state)).await
}

// ── Version / about ──────────────────────────────────────────────────────────

async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
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
