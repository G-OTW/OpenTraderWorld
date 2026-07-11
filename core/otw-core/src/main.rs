//! OpenTraderWorld core service.
//!
//! Phase 0/3 skeleton: health check, setup-state, first-run admin creation, and login.
//! The engine crates (scheduler, runner, modules, queue, i18n, theme) are wired into the
//! workspace as stubs and fleshed out in later phases.

mod auth;
mod backtest;
mod backtest_api;
mod quant;
mod quant_api;
mod calendar_api;
mod community_docs_api;
mod dashboard_api;
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
mod reminder_job;
mod reminders_api;
mod resources_api;
mod subscriptions_api;
mod taxcalc;
mod taxcalc_api;
mod time_api;
mod todos_api;
mod trader_tasks_api;
mod wealth_api;

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
        .route("/api/login", post(login));

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
        .merge(rate_api::routes())
        .merge(taxcalc_api::routes())
        .merge(mcp_api::routes());

    // MCP serves tool calls by running the same handlers in-process, minus the
    // session middleware (it authenticates with bearer tokens and injects the admin
    // user itself). Endpoint + permission checks live in `mcp`.
    mcp::init_dispatch(api.clone().with_state(state.clone()));

    let protected =
        api.route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    let app = public
        .route("/api/mcp", post(mcp::handle))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

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

/// Authenticate an existing user and start a session.
async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let user = otw_store::find_user_by_username(&state.pool, creds.username.trim())
        .await?
        .filter(|u| auth::verify_password(&creds.password, &u.password_hash))
        .ok_or_else(|| ApiError::unauthorized("invalid username or password"))?;

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

/// Issue a session token, persist it, and set an HttpOnly cookie.
async fn start_session(
    state: &AppState,
    jar: CookieJar,
    user_id: uuid::Uuid,
) -> Result<CookieJar, ApiError> {
    let token = auth::generate_token()?;
    otw_store::create_session(&state.pool, &token, user_id, SESSION_TTL_HOURS).await?;

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Lax)
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
    let token = jar
        .get(SESSION_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| ApiError::unauthorized("not signed in"))?;
    let user = otw_store::user_for_session(&state.pool, &token)
        .await?
        .ok_or_else(|| ApiError::unauthorized("session expired"))?;
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
    fn conflict(m: &str) -> Self {
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
