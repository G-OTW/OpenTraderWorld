//! OpenTraderWorld core service.
//!
//! Phase 0/3 skeleton: health check, setup-state, first-run admin creation, and login.
//! The engine crates (scheduler, runner, modules, queue, i18n, theme) are wired into the
//! workspace as stubs and fleshed out in later phases.

mod agent;
mod agent_api;
mod auth;
mod backtest;
mod backtest_api;
mod quant;
mod quant_api;
mod calendar_api;
mod community_docs_api;
mod dashboard_api;
mod demo;
mod demo_seed;
mod log_layer;
mod settings_api;
mod network_api;
mod databases;
mod documents;
mod feeds_api;
mod files;
mod findb_api;
mod findb_import;
mod fx;
mod fx_api;
mod fx_job;
mod goals_api;
mod histdata;
mod histdata_api;
mod histdata_cipher;
mod histdata_job;
mod journal_api;
mod journal_report;
mod mcp;
mod mcp_api;
mod mindset_api;
mod mportfolios;
mod mportfolios_api;
mod mportfolios_job;
mod notif_channels_api;
mod notif_send;
mod portfolios;
mod portfolios_api;
mod prompts_api;
mod rate;
mod rate_api;
mod report;
mod reminder_job;
mod reminders_api;
mod resources_api;
mod search_api;
mod subscriptions_api;
mod taxcalc;
mod taxcalc_api;
mod time_api;
mod todos_api;
mod trader_tasks_api;
mod vault_api;
mod watchlists;
mod watchlists_api;
mod wealth_api;
mod webhooks_api;
mod webhooks_inbound;

use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

pub const SESSION_COOKIE: &str = "otw_session";
const SESSION_TTL_HOURS: i64 = 24 * 7;

// Failed-login throttle (same window-counter pattern as the MCP and webhook endpoints):
// argon2 slows each attempt, but a `web`-mode deployment must not expose an unthrottled
// password oracle. Single-user app → one global counter is the right scope.
const LOGIN_FAIL_LIMIT: u32 = 10;
const LOGIN_FAIL_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

static LOGIN_FAILS: std::sync::Mutex<Option<(std::time::Instant, u32)>> =
    std::sync::Mutex::new(None);

fn login_throttled() -> bool {
    let guard = LOGIN_FAILS.lock().unwrap();
    matches!(*guard, Some((start, n)) if n >= LOGIN_FAIL_LIMIT && start.elapsed() < LOGIN_FAIL_WINDOW)
}

fn record_login_failure() {
    let mut guard = LOGIN_FAILS.lock().unwrap();
    *guard = match *guard {
        Some((start, n)) if start.elapsed() < LOGIN_FAIL_WINDOW => Some((start, n + 1)),
        _ => Some((std::time::Instant::now(), 1)),
    };
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// Directory where uploaded file bytes are stored (one file per id).
    pub upload_dir: std::path::PathBuf,
    /// AEAD cipher for feed secrets at rest.
    pub cipher: otw_store::crypto::SecretCipher,
    /// News-feed poll scheduler + live event bus.
    pub scheduler: otw_scheduler::Scheduler,
    /// True while a FinanceDatabase bulk import is running (guards concurrent installs).
    pub findb_importing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Serializes Managers' Portfolios refreshes (scheduled job vs. manual trigger).
    pub mportfolios_refresh: mportfolios_job::RefreshLock,
    /// Serializes Watchlists quote refreshes (auto-sync loop vs. manual trigger).
    pub watchlists_refresh: watchlists::RefreshLock,
    /// Shared HTTP client for outbound relays (e.g. doc submissions to the website).
    pub http: reqwest::Client,
    /// In-memory rate limiter guarding the doc-submission relay.
    pub submit_limiter: community_docs_api::SubmitLimiter,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = std::env::var("OTW_CORE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("OTW_CORE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;

    // `--seed-demo`: migrate DATABASE_URL, load the demo fixtures, exit. Used by the demo
    // deploy to build the `otw_seed` template database — never runs against a live install
    // unless explicitly pointed at one.
    if std::env::args().any(|a| a == "--seed-demo") {
        let pool = otw_store::connect_and_migrate(&database_url).await?;
        demo_seed::seed(&pool).await?;
        // Never echo the URL: it carries the DB password and this line lands in logs.
        println!("demo seed applied");
        return Ok(());
    }

    // Connect + migrate before the subscriber so the DB log layer can persist from boot.
    let pool = otw_store::connect_and_migrate(&database_url).await?;

    // Logging: console (EnvFilter from OTW_LOG) + a layer that persists into app_logs for
    // the in-app Logs viewer. The persisted minimum level is runtime-adjustable from
    // Settings; seed it from the stored `log_level` setting (falling back to OTW_LOG).
    let filter = std::env::var("OTW_LOG").unwrap_or_else(|_| "info".to_string());
    let stored_level = otw_store::settings::get_or(&pool, "log_level", &filter).await?;
    otw_store::logs::set_min_level(&stored_level);
    {
        use tracing_subscriber::prelude::*;
        let env_filter = tracing_subscriber::EnvFilter::new(filter);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().with_target(false))
            .with(log_layer::DbLogLayer::new(pool.clone()))
            .init();
    }

    tracing::info!("database connected and migrations applied");

    // Bootstrap the admin from the environment on first boot. Lets a headless install create
    // the account with no browser and no HTTP round-trip (setup.sh passes these through the
    // compose env). Idempotent: skipped once any admin exists; a blank password is ignored.
    bootstrap_admin(&pool).await?;

    // API rate tracker: publish the pool so any outbound-call site can record volume + flag
    // over-limit responses (observe-and-alert only; never throttles). Trim old rollups on a
    // slow cadence so the table stays bounded.
    otw_store::api_rate::init(pool.clone());
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = otw_store::api_rate::trim(&pool).await {
                    tracing::debug!("api rate trim failed: {e:#}");
                }
                tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
            }
        });
    }

    let upload_dir = std::path::PathBuf::from(
        std::env::var("OTW_UPLOAD_DIR").unwrap_or_else(|_| "/data/uploads".to_string()),
    );
    tokio::fs::create_dir_all(&upload_dir)
        .await
        .with_context(|| format!("creating upload dir {}", upload_dir.display()))?;
    tracing::info!("uploads stored in {}", upload_dir.display());

    // Master key for encrypting feed secrets at rest.
    let master_key = std::env::var("OTW_SECRET_KEY")
        .map_err(|_| anyhow::anyhow!("OTW_SECRET_KEY is required (run deploy/setup.sh)"))?;
    let cipher = otw_store::crypto::SecretCipher::from_master(&master_key)?;

    // Demo sandbox: the seeded `:free` model may have lost its free tier upstream since the
    // `otw_seed` template was built. Repin onto a live one. Backgrounded so an unreachable
    // provider delays nothing; no-op outside demo mode.
    if demo::enabled() {
        let (p, c) = (pool.clone(), cipher.clone());
        tokio::spawn(async move { demo::resolve_free_model(&p, &c).await });
    }

    // News-feed scheduler: starts polling due feeds in the background.
    let scheduler = otw_scheduler::Scheduler::new(pool.clone(), cipher.clone());
    scheduler.spawn();
    tracing::info!("news-feed scheduler started");

    // Daily FX catch-up: backfills history then pulls each new business day's close, so the
    // journal breakdown can convert mixed-currency trades into the display currency.
    fx_job::spawn(pool.clone());
    tracing::info!("fx catch-up job started");


    // Historical Data: publish the cipher for the worker, then drain the download queue.
    histdata_cipher::init(cipher.clone());
    histdata_job::spawn(pool.clone());
    tracing::info!("histdata download worker started");

    // Managers' Portfolios: scheduled Dataroma scrape into the cache.
    let mportfolios_refresh = mportfolios_job::new_lock();
    mportfolios_job::spawn(pool.clone(), mportfolios_refresh.clone());
    tracing::info!("mportfolios refresh job started");

    // Portfolio Tracker: daily re-price + valuation snapshot for auto-refresh portfolios.
    portfolios::spawn(pool.clone());
    tracing::info!("portfolio tracker daily job started");

    // Watchlists: re-quotes each sync-enabled list on its own refresh interval.
    let watchlists_refresh = watchlists::new_lock();
    watchlists::spawn(pool.clone(), watchlists_refresh.clone());
    tracing::info!("watchlists auto-refresh loop started");

    let http = reqwest::Client::new();

    // RemindMe tick: fires due reminders into in-app notifications every minute, and
    // pushes each to the user's enabled external channels (email/telegram/slack/discord).
    reminder_job::spawn(pool.clone(), cipher.clone(), http.clone());
    tracing::info!("reminder tick started");

    let state = AppState {
        pool,
        upload_dir,
        cipher,
        scheduler,
        findb_importing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        mportfolios_refresh,
        watchlists_refresh,
        http,
        submit_limiter: community_docs_api::SubmitLimiter::new(),
    };

    // Public routes: health and the auth handshake (setup + login). Everything else is
    // behind the session-cookie middleware.
    let public = Router::new()
        .route("/health", get(health))
        .route("/api/health", get(health))
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup", post(setup_admin))
        .route("/api/login", post(login))
        // Inbound webhooks (many senders can't set auth headers): the URL token is the
        // credential, checked in the handler. Body capped — payloads are tiny messages.
        .route(
            "/api/hooks/{token}",
            post(webhooks_inbound::handle).layer(axum::extract::DefaultBodyLimit::max(
                webhooks_inbound::MAX_BODY_BYTES,
            )),
        );

    // Protected routes: require a valid session. The middleware injects the authenticated
    // User as a request extension for handlers that want it.
    let api = Router::new()
        .merge(documents::routes())
        .merge(databases::routes())
        .merge(files::routes())
        .merge(feeds_api::routes())
        .merge(journal_api::routes())
        .merge(fx_api::routes())
        .merge(subscriptions_api::routes())
        .merge(time_api::routes())
        .merge(wealth_api::routes())
        .merge(todos_api::routes())
        .merge(trader_tasks_api::routes())
        .merge(mindset_api::routes())
        .merge(goals_api::routes())
        .merge(reminders_api::routes())
        .merge(notif_channels_api::routes())
        .merge(resources_api::routes())
        .merge(search_api::routes())
        .merge(prompts_api::routes())
        .merge(community_docs_api::routes())
        .merge(settings_api::routes())
        .merge(network_api::routes())
        .merge(dashboard_api::routes())
        .merge(findb_api::routes())
        .merge(calendar_api::routes())
        .merge(histdata_api::routes())
        .merge(backtest_api::routes())
        .merge(quant_api::routes())
        .merge(mportfolios_api::routes())
        .merge(portfolios_api::routes())
        .merge(watchlists_api::routes())
        .merge(rate_api::routes())
        .merge(taxcalc_api::routes())
        .merge(vault_api::routes())
        .merge(mcp_api::routes())
        .merge(agent_api::routes())
        .merge(webhooks_api::routes());

    // MCP serves tool calls by running the same handlers in-process, minus the
    // session middleware (it authenticates with bearer tokens and injects the admin
    // user itself). Endpoint + permission checks live in `mcp`.
    mcp::init_dispatch(api.clone().with_state(state.clone()));

    let protected =
        api.route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    let app = public
        .route("/api/mcp", post(mcp::handle))
        .route("/api/demo", get(demo::status))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Demo sandbox: wrap everything in the default-deny route gate (outermost layer).
    let app = if demo::enabled() {
        tracing::warn!("OTW_DEMO=1 — demo sandbox gate active (default-deny route allowlist)");
        app.layer(middleware::from_fn(demo::gate))
    } else {
        app
    };

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    tracing::info!("otw-core listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Aggregate health: core is up if this handler responds; postgres is probed with `SELECT 1`.
/// `status` is the worst service state so the frontend can color a single indicator.
async fn health(State(state): State<AppState>) -> Json<Value> {
    let postgres_up = sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .is_ok();

    let services = json!({
        "core": "up",
        "postgres": if postgres_up { "up" } else { "down" },
    });
    // Aggregate: all up → ok; some up → degraded; none up → down. (core is always up here.)
    let status = if postgres_up { "ok" } else { "degraded" };

    Json(json!({
        "status": status,
        "service": "otw-core",
        "version": env!("CARGO_PKG_VERSION"),
        "services": services,
    }))
}

/// Create the admin account at boot from `OTW_ADMIN_USER` / `OTW_ADMIN_PASSWORD` if it is
/// not already present. This is the headless install path: it runs in-process against the
/// database core already owns — no HTTP call, no external tooling, no network resolution —
/// so it works identically on every host. Idempotent (any existing admin short-circuits),
/// and a missing/blank password or a too-short one is a no-op that leaves the browser wizard
/// as the fallback. Never aborts boot.
async fn bootstrap_admin(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let (Ok(username), Ok(password)) = (
        std::env::var("OTW_ADMIN_USER"),
        std::env::var("OTW_ADMIN_PASSWORD"),
    ) else {
        return Ok(());
    };
    let username = username.trim();
    if username.is_empty() || password.len() < 8 {
        return Ok(());
    }
    if otw_store::admin_exists(pool).await? {
        return Ok(());
    }
    let hash = auth::hash_password(&password)?;
    // Force a password change on first login: this password was auto-generated by the
    // installer and sits in .env, so the operator should replace it with their own.
    otw_store::create_admin(pool, username, &hash, true).await?;
    tracing::info!("bootstrapped admin account '{username}' from environment (must change password on first login)");
    Ok(())
}

/// Tells the frontend whether the first-run wizard is needed.
async fn setup_status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let configured = otw_store::admin_exists(&state.pool).await?;
    Ok(Json(json!({ "configured": configured })))
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

/// First-run wizard: create the single admin account. Refuses if one already exists.
async fn setup_admin(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    if creds.username.trim().is_empty() || creds.password.len() < 8 {
        return Err(ApiError::bad_request(
            "username required and password must be at least 8 characters",
        ));
    }
    if otw_store::admin_exists(&state.pool).await? {
        return Err(ApiError::conflict("admin account already exists"));
    }

    let hash = auth::hash_password(&creds.password)?;
    // Wizard-created admins chose their own password, so no forced change.
    let user = otw_store::create_admin(&state.pool, creds.username.trim(), &hash, false).await?;

    let jar = start_session(&state, jar, user.id).await?;
    Ok((jar, Json(json!({ "ok": true, "username": user.username }))))
}

/// Authenticate an existing user and start a session. Failed attempts are throttled
/// (window counter); the throttle rejects before any password work happens.
async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    if login_throttled() {
        return Err(ApiError::too_many("too many failed login attempts; wait a minute"));
    }
    let user = otw_store::find_user_by_username(&state.pool, creds.username.trim())
        .await?
        .filter(|u| auth::verify_password(&creds.password, &u.password_hash))
        .ok_or_else(|| {
            record_login_failure();
            ApiError::unauthorized("invalid username or password")
        })?;

    let jar = start_session(&state, jar, user.id).await?;
    Ok((
        jar,
        Json(json!({
            "ok": true,
            "username": user.username,
            "must_change_password": user.must_change_password,
        })),
    ))
}

/// Issue a session token, persist it, and set an HttpOnly cookie. The cookie carries
/// Max-Age so the browser keeps it as long as the server honors it, and the Secure flag
/// whenever the network mode serves over HTTPS (plain-HTTP local/LAN modes can't set it —
/// the browser would drop the cookie entirely).
async fn start_session(
    state: &AppState,
    jar: CookieJar,
    user_id: uuid::Uuid,
) -> Result<CookieJar, ApiError> {
    let token = auth::generate_token()?;
    otw_store::create_session(&state.pool, &token, user_id, SESSION_TTL_HOURS).await?;

    let https = matches!(
        network_api::effective_mode(&state.pool).await.as_str(),
        "lan_https" | "web"
    );
    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(https)
        .max_age(time::Duration::hours(SESSION_TTL_HOURS))
        .path("/")
        .build();
    Ok(jar.add(cookie))
}

/// Session-cookie auth guard for all protected routes. Resolves the cookie to a live user
/// and stores it as a request extension; otherwise rejects with 401. The frontend treats
/// a 401 as "not signed in" and redirects to the login screen.
async fn require_auth(
    State(state): State<AppState>,
    jar: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let session_user = match jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) {
        Some(token) => otw_store::user_for_session(&state.pool, &token).await?,
        None => None,
    };
    let user = match session_user {
        Some(u) => u,
        // Demo sandbox: frictionless access — no valid session falls back to the seeded
        // admin (single-user app; the route gate decides what that identity may do).
        None if demo::enabled() => otw_store::first_admin(&state.pool)
            .await?
            .ok_or_else(|| ApiError::unauthorized("demo account missing"))?,
        None => return Err(ApiError::unauthorized("not signed in")),
    };
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

// ── Error handling ───────────────────────────────────────────────────────────

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn bad_request(m: &str) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: m.into() }
    }
    fn unauthorized(m: &str) -> Self {
        Self { status: StatusCode::UNAUTHORIZED, message: m.into() }
    }
    pub fn conflict(m: &str) -> Self {
        Self { status: StatusCode::CONFLICT, message: m.into() }
    }
    pub fn not_found(m: &str) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: m.into() }
    }
    pub fn internal(m: &str) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: m.into() }
    }
    pub fn too_many(m: &str) -> Self {
        Self { status: StatusCode::TOO_MANY_REQUESTS, message: m.into() }
    }
    pub fn bad_gateway(m: &str) -> Self {
        Self { status: StatusCode::BAD_GATEWAY, message: m.into() }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: "internal error".into() }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}
