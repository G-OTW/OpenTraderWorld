//! OpenTraderWorld core service.
//!
//! Phase 0/3 skeleton: health check, setup-state, first-run admin creation, and login.
//! The engine crates (scheduler, runner, modules, queue, i18n, theme) are wired into the
//! workspace as stubs and fleshed out in later phases.

mod agent;
mod agent_api;
mod align;
mod auth;
mod automator;
mod automator_api;
mod automator_job;
mod backtest;
mod backtest_api;
mod backtest_optimize_api;
mod brokers;
mod brokers_api;
mod calendar_api;
mod cli;
mod community_docs_api;
mod connectors_api;
mod control;
mod control_api;
mod dashboard_api;
mod data_transfer_api;
mod databases;
mod demo;
mod demo_seed;
mod documents;
mod feeds_api;
mod files;
mod findb_api;
mod findb_import;
mod fx;
mod fx_api;
mod fx_histdata;
mod fx_job;
mod goals_api;
mod histdata;
mod histdata_api;
mod histdata_cipher;
mod histdata_export;
mod histdata_import;
mod histdata_import_api;
mod histdata_job;
mod histviz_alerts;
mod histviz_api;
mod import;
mod internal_call;
mod journal_api;
mod journal_import;
mod journal_import_api;
mod journal_market;
mod journal_report;
mod link_preview;
mod live;
mod log_layer;
mod mailbox;
mod mailbox_api;
mod mailbox_job;
mod mcp;
mod mcp_api;
mod mindset_api;
mod mportfolios;
mod mportfolios_api;
mod mportfolios_job;
mod network_api;
mod notif_channels_api;
mod notif_send;
mod paper;
mod paper_api;
mod paper_job;
mod portfolios;
mod portfolios_analytics_api;
mod portfolios_api;
mod portfolios_import;
mod portfolios_import_api;
mod prompts_api;
mod quant;
mod quant_api;
mod rate;
mod rate_api;
mod reminder_job;
mod reminders_api;
mod report;
mod resources_api;
mod search_api;
mod security;
mod security_events;
mod settings_api;
mod subscriptions_api;
mod taxcalc;
mod taxcalc_api;
mod taxcalc_broker;
mod time_api;
mod timeframe;
mod todos_api;
mod trader_tasks_api;
mod vault_api;
mod video_embed;
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

/// Session cookie name in plain-HTTP modes.
pub const SESSION_COOKIE: &str = "otw_session";

/// Session cookie name whenever the app serves over HTTPS.
///
/// The `__Host-` prefix is a browser-enforced contract: the cookie must carry `Secure`,
/// must have `Path=/` and must have no `Domain`, and in exchange no other host (a sibling
/// subdomain, a plain-HTTP page on the same name) can set or overwrite it. That closes
/// session fixation from a neighbouring host, which a bare cookie name leaves open. It
/// cannot be used without TLS, hence the two names.
pub const SESSION_COOKIE_HOST: &str = "__Host-otw_session";

const SESSION_TTL_HOURS: i64 = 24 * 7;

/// Sliding inactivity limit applied on top of the absolute TTL, in the modes that face a
/// network. A session untouched for this long is dead even though its absolute expiry is
/// days away. `local` and `lan` keep the absolute ceiling as the only rule: a laptop on a
/// desk is not the threat model, and the friction would buy nothing.
const SESSION_IDLE_HOURS: i64 = 24;

/// Read the session token out of whichever cookie this deployment set.
///
/// Both names are accepted on the way in, always. The network mode can change under a
/// browser that is already holding a cookie, and a user who switches their instance to
/// HTTPS should not be signed out by the rename.
pub fn session_token(jar: &CookieJar) -> Option<String> {
    jar.get(SESSION_COOKIE_HOST)
        .or_else(|| jar.get(SESSION_COOKIE))
        .map(|c| c.value().to_string())
}

// ── Failed-login throttle ────────────────────────────────────────────────────
//
// Two counters, because they answer two different attacks.
//
// **Per source.** A wrong password from one address makes that address wait longer next
// time, up to LOGIN_DELAY_MAX. This is the one that matters: an online guessing run comes
// from somewhere, and making it pay per attempt is what turns millions of guesses an hour
// into a few hundred.
//
// **Global.** A spray from many addresses keys a fresh bucket every time, so the per-source
// counter never bites. The global counter catches it. It is deliberately more forgiving,
// because it is also the one an attacker could use against the owner.
//
// Both are *delays*, never rejections. A hard global lockout would let any unauthenticated
// visitor lock the owner out of their own app by spraying bad passwords: the attacker's
// cost becomes the defender's outage. Single-user means there is no per-account bucket to
// fall back on, so instead a correct password always eventually gets in, just slowly.
const LOGIN_FAIL_GRACE: u32 = 3;
const LOGIN_DELAY_STEP: std::time::Duration = std::time::Duration::from_millis(500);
const LOGIN_DELAY_MAX: std::time::Duration = std::time::Duration::from_secs(20);
const LOGIN_FAIL_WINDOW: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// Failures from *any* source before the shared delay starts. Well above what one person
/// mistyping produces, so a legitimate user never meets it.
const LOGIN_GLOBAL_GRACE: u32 = 20;

static LOGIN_FAILS_BY_IP: security::Attempts = security::Attempts::new(LOGIN_FAIL_WINDOW);
static LOGIN_FAILS_GLOBAL: security::Attempts = security::Attempts::new(LOGIN_FAIL_WINDOW);

/// How long this attempt waits before any password work happens. The larger of the two
/// counters decides.
fn login_delay(ip: &str) -> std::time::Duration {
    let by_ip = LOGIN_FAILS_BY_IP.count(ip).saturating_sub(LOGIN_FAIL_GRACE);
    let global = LOGIN_FAILS_GLOBAL
        .count("all")
        .saturating_sub(LOGIN_GLOBAL_GRACE);
    LOGIN_DELAY_STEP
        .saturating_mul(by_ip.max(global))
        .min(LOGIN_DELAY_MAX)
}

/// Failures from one source before the owner is told about it. A typo or two is not an
/// event; a run of them past the point where the delay has already started biting is.
const LOGIN_ALERT_AFTER: u32 = 10;

/// Count one failure, write it to the security log, and tell the owner once a single
/// source crosses from "mistyped it" into "working through a list".
fn record_login_failure(state: &AppState, ip: &str, what: &str) {
    let n = LOGIN_FAILS_BY_IP.record(ip);
    LOGIN_FAILS_GLOBAL.record("all");
    security_events::log(
        security_events::LOGIN_FAILED,
        &format!("{what} from {ip} (attempt {n} in the last 15 minutes)"),
    );
    if n >= LOGIN_ALERT_AFTER {
        security_events::alert(
            state,
            security_events::LOGIN_FAILED,
            security_events::LOGIN_FAILED,
            "Repeated failed sign-ins",
            &format!(
                "Source: {ip}\nFailed attempts in the last 15 minutes: {n}\n\nEach one from \
                 this address now waits longer than the last. If it was not you, nothing has \
                 been granted, but a password this instance shares with anywhere else is worth \
                 changing."
            ),
        );
    }
}

/// Clear this source's counter: a correct password proves its traffic was not an attack.
/// The global counter is left alone — one success elsewhere says nothing about a spray in
/// progress.
fn clear_login_failures(ip: &str) {
    LOGIN_FAILS_BY_IP.clear(ip);
}

// ── TOTP replay guard ────────────────────────────────────────────────────────

/// Codes already spent, so a six-digit value cannot be used twice inside its 30-second
/// life. Keyed `user:step`; entries older than a couple of steps are pruned on write.
static TOTP_SPENT: std::sync::Mutex<Option<Vec<(String, std::time::Instant)>>> =
    std::sync::Mutex::new(None);

/// Claim a `(user, step)` pair. False when it was already used.
fn claim_totp_step(user_id: uuid::Uuid, step: u64) -> bool {
    let key = format!("{user_id}:{step}");
    let mut guard = TOTP_SPENT.lock().unwrap();
    let spent = guard.get_or_insert_with(Vec::new);
    let ttl = std::time::Duration::from_secs(auth::totp::PERIOD * 3);
    spent.retain(|(_, at)| at.elapsed() < ttl);
    if spent.iter().any(|(k, _)| *k == key) {
        return false;
    }
    spent.push((key, std::time::Instant::now()));
    true
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
    /// Live market-data hub: one supervised WS feed per live dataset, shared by viewers.
    pub live: live::hub::LiveHub,
    /// In-memory rate limiter guarding the doc-submission relay.
    pub submit_limiter: community_docs_api::SubmitLimiter,
    /// Backtest parameter grids: the last few optimization jobs, running or finished. In memory
    /// on purpose — a sweep is an exploration, the run history is where a result is kept.
    pub optimizer: std::sync::Arc<backtest::optimize::Jobs>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Latch the process start now, so uptime counts from boot and not from the first
    // /health call (which would report ~0s forever on an instance nobody probes).
    std::sync::LazyLock::force(&STARTED_AT);

    let host = std::env::var("OTW_CORE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("OTW_CORE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;

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

    // Host-CLI account recovery (`reset-password`, `list-users`): does its work against the
    // database and exits before any listener exists. Argv-only on purpose — a self-hosted
    // single-user install has no mailbox to send a reset link to, so the recovery credential
    // is "can run a process next to the database" and there is no HTTP surface to attack.
    // See `cli.rs` for the full reasoning.
    if let Some(cmd) = cli::from_args() {
        return cli::run(cmd, &database_url).await;
    }

    // Connect + migrate before the subscriber so the DB log layer can persist from boot.
    let pool = otw_store::connect_and_migrate(&database_url).await?;

    // Logging: console (EnvFilter from OTW_LOG) + a layer that persists into app_logs for
    // the in-app Logs viewer. The persisted minimum level is runtime-adjustable from
    // Settings; seed it from the stored `log_level` setting (falling back to OTW_LOG).
    let filter = std::env::var("OTW_LOG").unwrap_or_else(|_| "info".to_string());
    let stored_level = otw_store::settings::get_or(&pool, "log_level", &filter).await?;
    otw_store::logs::set_min_level(&stored_level);
    // Request timeout: runtime-adjustable from Settings, seeded here so a restart keeps the
    // operator's value.
    if let Ok(raw) = otw_store::settings::get_or(
        &pool,
        "request_timeout_secs",
        &security::DEFAULT_REQUEST_TIMEOUT_SECS.to_string(),
    )
    .await
    {
        if let Ok(secs) = raw.parse::<u64>() {
            security::set_request_timeout_secs(secs);
        }
    }
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

    // Seed the shipped agent personas + skills. Insert-if-absent, so user edits survive every
    // boot. Non-fatal: without its shelf the agent is still a working chat agent.
    if let Err(e) = agent::builtin::seed::run(&pool).await {
        tracing::warn!("agent: seeding builtin personas/skills failed: {e:#}");
    }

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

    let http = reqwest::Client::new();

    // Watchlists: re-quotes each sync-enabled list on its own refresh interval, then fires
    // any price alert that hit — server-side, so alerts work with no browser open.
    let watchlists_refresh = watchlists::new_lock();
    watchlists::spawn(
        pool.clone(),
        watchlists_refresh.clone(),
        watchlists::AlertCx {
            cipher: cipher.clone(),
            http: http.clone(),
        },
    );
    tracing::info!("watchlists auto-refresh loop started");

    // Chart alerts: a level drawn on a chart is watched here, on closed bars, so it fires
    // with no browser open.
    histviz_alerts::spawn(
        pool.clone(),
        histviz_alerts::AlertCx {
            cipher: cipher.clone(),
            http: http.clone(),
        },
    );
    tracing::info!("chart alerts loop started");

    // RemindMe tick: fires due reminders into in-app notifications every minute, and
    // pushes each to the user's enabled external channels (email/telegram/slack/discord).
    reminder_job::spawn(pool.clone(), cipher.clone(), http.clone());
    tracing::info!("reminder tick started");

    // Mailbox: polls each connected IMAP account on its own interval (read-only).
    mailbox_job::spawn(pool.clone(), cipher.clone(), http.clone());
    tracing::info!("mailbox poll loop started");

    // Live market data: ref-counted WS feeds, started on demand when a chart opens a stream.
    let live = live::hub::LiveHub::new(pool.clone(), http.clone(), cipher.clone());

    let pool_for_setup = pool.clone();
    let state = AppState {
        pool,
        upload_dir,
        cipher,
        scheduler,
        findb_importing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        mportfolios_refresh,
        watchlists_refresh,
        http,
        live,
        submit_limiter: community_docs_api::SubmitLimiter::new(),
        optimizer: std::sync::Arc::new(backtest::optimize::Jobs::new()),
    };

    // External control: one outbound connection per enabled chat binding, so a message
    // sent to the bot becomes an agent run under that binding's MCP token. Does nothing
    // until the switch in Settings is on and a binding exists.
    control::spawn(state.clone());

    // Public routes that a browser posts to. The same-origin guard rides on them because
    // they are the ones a cross-site page would try to drive.
    let handshake = Router::new()
        .route("/api/setup", post(setup_admin))
        .route("/api/login", post(login))
        .layer(middleware::from_fn(security::same_origin));

    // Public routes: health and the auth handshake (setup + login). Everything else is
    // behind the session-cookie middleware.
    let public = Router::new()
        .route("/health", get(health))
        .route("/api/health", get(api_health))
        .route("/api/setup/status", get(setup_status))
        .merge(handshake)
        // Inbound webhooks (many senders can't set auth headers): the URL token is the
        // credential, checked in the handler. Body capped — payloads are tiny messages.
        // No origin guard: the senders are servers, not browsers, and they hold no cookie.
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
        .merge(video_embed::routes())
        .merge(feeds_api::routes())
        .merge(journal_api::routes())
        .merge(journal_import_api::routes())
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
        .merge(data_transfer_api::routes())
        .merge(network_api::routes())
        .merge(dashboard_api::routes())
        .merge(findb_api::routes())
        .merge(calendar_api::routes())
        .merge(connectors_api::routes())
        .merge(brokers_api::routes())
        .merge(histdata_api::routes())
        .merge(histdata_import_api::routes())
        .merge(histviz_api::routes())
        .merge(paper_api::routes())
        // Backtests, parameter sweeps and Monte-Carlo each take the machine for minutes.
        // They queue on a shared permit rather than all running at once; the gate is layered
        // here, before `api` is cloned for the in-process dispatch, so an MCP or Automator
        // tool call waits on the same permits a browser does.
        .merge(
            Router::new()
                .merge(backtest_api::routes())
                .merge(backtest_optimize_api::routes())
                .merge(quant_api::routes())
                .layer(middleware::from_fn(security::compute_gate)),
        )
        .merge(mportfolios_api::routes())
        .merge(portfolios_api::routes())
        .merge(portfolios_analytics_api::routes())
        .merge(portfolios_import_api::routes())
        .merge(watchlists_api::routes())
        .merge(rate_api::routes())
        .merge(taxcalc_api::routes())
        .merge(taxcalc_broker::routes())
        .merge(vault_api::routes())
        .merge(control_api::routes())
        .merge(mcp_api::routes())
        .merge(agent_api::routes())
        .merge(webhooks_api::routes())
        .merge(mailbox_api::routes())
        .merge(automator_api::routes());

    // MCP serves tool calls by running the same handlers in-process, minus the
    // session middleware (it authenticates with bearer tokens and injects the admin
    // user itself). Endpoint + permission checks live in `mcp`.
    mcp::init_dispatch(api.clone().with_state(state.clone()));

    // The Automator dispatches into that same router, so its tick loop starts only once
    // the router exists.
    automator_job::spawn(state.clone());
    paper_job::spawn(state.clone());

    // Order matters: the same-origin guard runs *before* the session guard, so a cross-site
    // request is refused as such rather than as a valid session doing something odd. Layers
    // apply outside-in in the order they are listed here.
    let protected = api
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .route_layer(middleware::from_fn(security::same_origin));

    let app = public
        // The MCP gateway runs its own origin check and its own bearer auth.
        .route("/api/mcp", post(mcp::handle))
        .route("/api/demo", get(demo::status))
        .merge(protected)
        // A handler that never returns holds a connection and a database permit forever.
        // This bounds the time to *produce* a response, not the life of a streaming body,
        // so SSE feeds and WebSocket upgrades are unaffected: their handler returns as soon
        // as the stream is handed over.
        .layer(middleware::from_fn(security::request_timeout))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Demo sandbox: wrap everything in the default-deny route gate (outermost layer).
    let app = if demo::enabled() {
        tracing::warn!("OTW_DEMO=1 — demo sandbox gate active (default-deny route allowlist)");
        app.layer(middleware::from_fn(demo::gate))
    } else {
        app
    };

    // First-run wizard on a network-facing instance: mint and print the setup token, so the
    // operator can claim the admin account and nobody who merely finds the hostname can.
    if !otw_store::admin_exists(&pool_for_setup).await? {
        let mode = network_api::effective_mode(&pool_for_setup).await;
        if let Some(token) = bootstrap_token(&mode) {
            tracing::warn!(
                "no account exists yet and this instance is reachable from the network \
                 ({mode}). The first-run wizard will ask for this setup token: {token}"
            );
        }
    }

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    tracing::info!("otw-core listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Process start, captured on first read (main touches it at boot, before serving), so
/// `/health` can report uptime. A server-side installer polls this after an update to tell
/// "the new version answered" from "the old process never restarted": same version string,
/// but an uptime that went backwards is the proof the restart happened.
static STARTED_AT: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);

/// Aggregate health: core is up if this handler responds; postgres is probed with `SELECT 1`.
/// `status` is the worst service state so the frontend can color a single indicator.
///
/// Two audiences, two payloads. `/health` is not routed by the reverse proxy (Caddy proxies
/// `/api/*` and serves the SPA for everything else), so it is reachable only from inside the
/// compose network: the Docker healthcheck and the host-side installer, which needs the
/// version and the uptime to tell "the new binary answered" from "the old process never
/// restarted". `/api/health` is public, and a public endpoint naming the exact build is what
/// turns a published CVE into a targeted one, so an unauthenticated caller gets the
/// indicator and nothing else. A signed-in browser gets the whole thing.
async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(health_payload(&state).await)
}

/// `/api/health` — the same probe, reduced to a status for anyone not signed in.
async fn api_health(State(state): State<AppState>, jar: CookieJar) -> Json<Value> {
    let full = health_payload(&state).await;
    let idle = idle_limit(&state).await;
    let signed_in = match session_token(&jar) {
        Some(token) => matches!(
            otw_store::user_for_session(&state.pool, &token, idle).await,
            Ok(Some(_))
        ),
        None => demo::enabled(),
    };
    if signed_in {
        return Json(full);
    }
    Json(json!({ "status": full["status"] }))
}

async fn health_payload(state: &AppState) -> Value {
    let postgres_up = sqlx::query("SELECT 1").execute(&state.pool).await.is_ok();

    let services = json!({
        "core": "up",
        "postgres": if postgres_up { "up" } else { "down" },
    });
    // Aggregate: all up → ok; some up → degraded; none up → down. (core is always up here.)
    let status = if postgres_up { "ok" } else { "degraded" };

    json!({
        "status": status,
        "service": "otw-core",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": STARTED_AT.elapsed().as_secs(),
        "services": services,
    })
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
    if username.is_empty() {
        return Ok(());
    }
    // Same policy as every other way of setting a password. `setup.sh` mints a long random
    // value so this never fires there; a hand-edited `.env` with a weak value is refused and
    // the browser wizard takes over, which is the safe direction for a silent boot path.
    if let Err(e) = auth::check_password(&password, username) {
        tracing::warn!("OTW_ADMIN_PASSWORD was not used: {e}");
        return Ok(());
    }
    if otw_store::admin_exists(pool).await? {
        // The account is already there, so this value does nothing except sit in `.env` and
        // in the container's environment, where `docker inspect` hands it to anyone who can
        // reach the daemon. `setup.sh` blanks it automatically; say so for every other path.
        tracing::warn!(
            "OTW_ADMIN_PASSWORD is still set but the admin account already exists. Blank that \
             line in deploy/.env and restart: it grants nothing and is one more copy of a \
             password on disk."
        );
        return Ok(());
    }
    let hash = auth::hash_password(&password)?;
    // Force a password change on first login: this password was auto-generated by the
    // installer and sits in .env, so the operator should replace it with their own.
    otw_store::create_admin(pool, username, &hash, true).await?;
    tracing::info!("bootstrapped admin account '{username}' from environment (must change password on first login)");
    Ok(())
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
    /// Six-digit TOTP code. Absent on the first leg of a two-factor sign-in: the server
    /// answers `totp_required` and the client asks for it.
    #[serde(default)]
    code: Option<String>,
}

/// First-run wizard: create the single admin account. Refuses if one already exists.
///
/// On a network-facing instance this route is also gated by a bootstrap token
/// ([`setup_guard`]): an open `/api/setup` is a race between the owner and whoever finds
/// the hostname first.
async fn setup_admin(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let username = creds.username.trim();
    if username.is_empty() {
        return Err(ApiError::bad_request("username required"));
    }
    if otw_store::admin_exists(&state.pool).await? {
        return Err(ApiError::conflict("admin account already exists"));
    }
    setup_guard(&state, &headers).await?;
    auth::check_password(&creds.password, username).map_err(|e| ApiError::bad_request(&e))?;

    let hash = auth::hash_password(&creds.password)?;
    // Wizard-created admins chose their own password, so no forced change.
    let user = otw_store::create_admin(&state.pool, username, &hash, false).await?;

    let jar = start_session(&state, jar, user.id, &headers).await?;
    Ok((jar, Json(json!({ "ok": true, "username": user.username }))))
}

/// Bootstrap token for the first-run wizard, minted once per process and printed to the
/// log at startup. `None` until [`bootstrap_token`] decides the instance needs one.
static SETUP_TOKEN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The token this process requires on `/api/setup`, if any.
///
/// Only network-facing modes get one. On `local` and `lan` the wizard is reachable from the
/// desk or the living room and a token would be pure friction; on `lan_https` and `web` the
/// wizard is reachable from the internet, and "whoever loads the page first becomes the
/// administrator" is a takeover waiting for a slow DNS propagation or a recreated database
/// volume. Minted per process, so a restart issues a fresh one and an abandoned token dies.
fn bootstrap_token(network_mode: &str) -> Option<&'static String> {
    SETUP_TOKEN
        .get_or_init(|| {
            setup_token_required(network_mode)
                .then(|| auth::generate_token().ok().map(|t| t[..16].to_string()))
                .flatten()
        })
        .as_ref()
}

/// Whether the first-run wizard on this network mode has to be gated.
fn setup_token_required(network_mode: &str) -> bool {
    is_https_mode(network_mode)
}

/// Enforce the bootstrap token when this instance has one.
async fn setup_guard(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), ApiError> {
    let mode = network_api::effective_mode(&state.pool).await;
    let Some(expected) = bootstrap_token(&mode) else {
        return Ok(());
    };
    let supplied = headers
        .get("x-otw-setup-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("");
    use subtle::ConstantTimeEq;
    if supplied.len() == expected.len()
        && bool::from(supplied.as_bytes().ct_eq(expected.as_bytes()))
    {
        return Ok(());
    }
    LOGIN_FAILS_GLOBAL.record("setup");
    security_events::log(
        security_events::LOGIN_FAILED,
        &format!(
            "wrong bootstrap token from {} while claiming the first account",
            security::client_ip(headers)
        ),
    );
    Err(ApiError::forbidden_code(
        "setup_token_required",
        "this instance is reachable from the network, so the first account needs the setup \
         token printed in the server log at startup (`docker compose logs core`)",
    ))
}

/// Tells the frontend whether the first-run wizard is needed, and whether it will ask for
/// the bootstrap token.
async fn setup_status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let configured = otw_store::admin_exists(&state.pool).await?;
    let mode = network_api::effective_mode(&state.pool).await;
    Ok(Json(json!({
        "configured": configured,
        "token_required": !configured && bootstrap_token(&mode).is_some(),
    })))
}

/// Authenticate an existing user and start a session.
///
/// Three things gate it: the per-source delay ([`login_delay`]), the password, and the
/// second factor when the account has one. A wrong code counts as a failed login exactly
/// like a wrong password, so the delay covers both.
async fn login(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    jar: CookieJar,
    Json(creds): Json<Credentials>,
) -> Result<(CookieJar, Json<Value>), ApiError> {
    let ip = security::client_ip(&headers);
    tokio::time::sleep(login_delay(&ip)).await;

    let found = otw_store::find_user_by_username(&state.pool, creds.username.trim()).await?;
    // Verify against a dummy hash when the username is unknown, so a miss costs the same
    // argon2 work as a wrong password and the response time stops naming valid usernames.
    let user = match found {
        Some(u) if auth::verify_password(&creds.password, &u.password_hash) => u,
        other => {
            if other.is_none() {
                auth::verify_dummy(&creds.password);
            }
            record_login_failure(&state, &ip, "wrong username or password");
            return Err(ApiError::unauthorized("invalid username or password"));
        }
    };

    if user.totp_enabled {
        // The password was right, so saying "now the code" discloses nothing an attacker
        // holding the password does not already have.
        let Some(code) = creds.code.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            return Err(ApiError::unauthorized_code(
                "totp_required",
                "enter the six-digit code from your authenticator",
            ));
        };
        if !verify_totp(&state, &user, code).await? {
            record_login_failure(&state, &ip, &format!("wrong second factor for '{}'", user.username));
            return Err(ApiError::unauthorized("that code is not valid"));
        }
    }

    clear_login_failures(&ip);
    let jar = start_session(&state, jar, user.id, &headers).await?;

    // A sign-in from an address never seen before is the one event worth interrupting the
    // owner for. Keyed by the address, not by the kind: two unfamiliar sources inside the
    // cooldown are two things to look at, and collapsing them would hide the second.
    if let Ok(true) = otw_store::note_login_source(&state.pool, user.id, &ip).await {
        security_events::alert(
            &state,
            security_events::LOGIN_NEW_SOURCE,
            &format!("{}:{ip}", security_events::LOGIN_NEW_SOURCE),
            "New sign-in from an unrecognised source",
            &format!(
                "Account: {}\nSource: {ip}\nBrowser: {}",
                user.username,
                security::user_agent(&headers)
            ),
        );
    }

    Ok((
        jar,
        Json(json!({
            "ok": true,
            "username": user.username,
            "must_change_password": user.must_change_password,
        })),
    ))
}

/// Check a TOTP code against a user's sealed secret, refusing a replay of a code already
/// spent inside its own step.
pub async fn verify_totp(
    state: &AppState,
    user: &otw_store::User,
    code: &str,
) -> Result<bool, ApiError> {
    let Some((nonce, ct)) = otw_store::totp_secret(&state.pool, user.id).await? else {
        return Ok(false);
    };
    let secret = state.cipher.open(&nonce, &ct)?;
    match auth::totp::verify(&secret, code) {
        Some(step) => Ok(claim_totp_step(user.id, step)),
        None => Ok(false),
    }
}

/// Issue a session token, persist it with its provenance, and set the cookie.
///
/// The cookie is `HttpOnly` always, `Secure` + `__Host-` prefixed whenever the network mode
/// serves TLS, and `SameSite=Lax` throughout. Plain-HTTP modes cannot set `Secure` (the
/// browser would drop the cookie entirely), which is also why the name differs per mode.
async fn start_session(
    state: &AppState,
    jar: CookieJar,
    user_id: uuid::Uuid,
    headers: &axum::http::HeaderMap,
) -> Result<CookieJar, ApiError> {
    let token = auth::generate_token()?;
    otw_store::create_session(
        &state.pool,
        &token,
        user_id,
        SESSION_TTL_HOURS,
        &security::client_ip(headers),
        &security::user_agent(headers),
    )
    .await?;

    let https = is_https_mode(&network_api::effective_mode(&state.pool).await);
    let name = if https { SESSION_COOKIE_HOST } else { SESSION_COOKIE };
    let cookie = Cookie::build((name, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(https)
        .max_age(time::Duration::hours(SESSION_TTL_HOURS))
        .path("/")
        .build();
    // Clear the other name so a mode switch cannot leave two cookies racing to authenticate.
    let stale = if https { SESSION_COOKIE } else { SESSION_COOKIE_HOST };
    Ok(jar.remove(Cookie::from(stale)).add(cookie))
}

/// True for the network modes that terminate TLS.
pub fn is_https_mode(mode: &str) -> bool {
    matches!(mode, "lan_https" | "web")
}

/// Inactivity limit for this deployment, or `None` where only the absolute TTL applies.
pub async fn idle_limit(state: &AppState) -> Option<i64> {
    is_https_mode(&network_api::effective_mode(&state.pool).await).then_some(SESSION_IDLE_HOURS)
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
    let idle = idle_limit(&state).await;
    let session_user = match session_token(&jar) {
        Some(token) => otw_store::user_for_session(&state.pool, &token, idle).await?,
        None => None,
    };
    let user = match session_user {
        Some(u) => u,
        // Demo sandbox: frictionless access — no valid session falls back to the seeded
        // admin (single-user app; the route gate decides what that identity may do).
        None if demo::enabled() => otw_store::first_admin(&state.pool)
            .await?
            .ok_or_else(|| ApiError::unauthorized("not signed in"))?,
        None => return Err(ApiError::unauthorized("not signed in")),
    };
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

#[cfg(test)]
mod auth_flow_tests {
    use super::*;

    /// A code is good for one use inside its own 30-second step. Without this, a code read
    /// over someone's shoulder (or off a proxy log) is replayable for half a minute.
    #[test]
    fn a_totp_step_can_only_be_claimed_once() {
        let user = uuid::Uuid::new_v4();
        let step = auth::totp::current_step();
        assert!(claim_totp_step(user, step));
        assert!(!claim_totp_step(user, step), "the same code must not work twice");
        assert!(claim_totp_step(user, step + 1), "the next step is a different code");
        assert!(
            claim_totp_step(uuid::Uuid::new_v4(), step),
            "another account's codes are unrelated"
        );
    }

    /// The delay must grow with failures from one source and stay at zero for a source that
    /// has not failed: one person mistyping must not slow down anything else.
    #[test]
    fn the_login_delay_is_per_source() {
        let ip = format!("test-{}", uuid::Uuid::new_v4());
        assert_eq!(login_delay(&ip), std::time::Duration::ZERO);
        for _ in 0..(LOGIN_FAIL_GRACE + 2) {
            LOGIN_FAILS_BY_IP.record(&ip);
        }
        assert!(login_delay(&ip) > std::time::Duration::ZERO);

        let other = format!("test-{}", uuid::Uuid::new_v4());
        assert_eq!(login_delay(&other), std::time::Duration::ZERO);

        clear_login_failures(&ip);
        assert_eq!(login_delay(&ip), std::time::Duration::ZERO);
    }

    /// Both cookie names resolve, so switching an install between HTTP and HTTPS does not
    /// sign everyone out.
    #[test]
    fn either_cookie_name_carries_the_session() {
        use axum_extra::extract::cookie::{Cookie, CookieJar};

        let plain = CookieJar::new().add(Cookie::new(SESSION_COOKIE, "abc"));
        assert_eq!(session_token(&plain).as_deref(), Some("abc"));

        let host = CookieJar::new().add(Cookie::new(SESSION_COOKIE_HOST, "xyz"));
        assert_eq!(session_token(&host).as_deref(), Some("xyz"));

        // With both present the __Host- one wins: it is the one a foreign host could not
        // have written.
        let both = CookieJar::new()
            .add(Cookie::new(SESSION_COOKIE, "abc"))
            .add(Cookie::new(SESSION_COOKIE_HOST, "xyz"));
        assert_eq!(session_token(&both).as_deref(), Some("xyz"));

        assert_eq!(session_token(&CookieJar::new()), None);
    }

    /// The bootstrap token exists only where the wizard faces a network. `bootstrap_token`
    /// itself latches once per process, so the decision it makes is what gets tested.
    #[test]
    fn only_network_facing_modes_need_a_setup_token() {
        assert!(setup_token_required("web"));
        assert!(setup_token_required("lan_https"));
        assert!(!setup_token_required("lan"), "the living room is not the internet");
        assert!(!setup_token_required("local"));
        assert!(!setup_token_required(""), "an unknown mode is not gated");
    }

    /// The `Secure` flag and the `__Host-` cookie name follow TLS, not the bind address.
    #[test]
    fn https_modes_are_the_tls_terminating_ones() {
        assert!(is_https_mode("web"));
        assert!(is_https_mode("lan_https"));
        assert!(!is_https_mode("lan"));
        assert!(!is_https_mode("local"));
    }
}

// ── Error handling ───────────────────────────────────────────────────────────

pub struct ApiError {
    status: StatusCode,
    message: String,
    /// Machine-readable tag, when the client has to *do* something specific about the
    /// refusal rather than just show it: ask for the password again, ask for a TOTP code.
    /// Absent on ordinary errors, where the message is the whole answer.
    code: Option<&'static str>,
}

impl ApiError {
    fn new(status: StatusCode, m: &str) -> Self {
        Self { status, message: m.into(), code: None }
    }

    pub fn bad_request(m: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, m)
    }

    /// A 403 that names what the client must do: `reauth_required` (the session must
    /// re-enter its password) or `cross_origin` (the request came from another site).
    pub fn forbidden_code(code: &'static str, m: &str) -> Self {
        Self { code: Some(code), ..Self::new(StatusCode::FORBIDDEN, m) }
    }

    /// A 401 that names what the client must supply, e.g. `totp_required`.
    pub fn unauthorized_code(code: &'static str, m: &str) -> Self {
        Self { code: Some(code), ..Self::new(StatusCode::UNAUTHORIZED, m) }
    }
    fn unauthorized(m: &str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, m)
    }
    pub fn conflict(m: &str) -> Self {
        Self::new(StatusCode::CONFLICT, m)
    }
    pub fn forbidden(m: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, m)
    }
    pub fn not_found(m: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, m)
    }
    pub fn internal(m: &str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, m)
    }
    pub fn timeout(m: &str) -> Self {
        Self::new(StatusCode::REQUEST_TIMEOUT, m)
    }

    pub fn too_many(m: &str) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, m)
    }
    pub fn bad_gateway(m: &str) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, m)
    }
    /// The user-facing text. A background worker reports a failed call in its own log and
    /// events rather than as an HTTP response, so it needs to read this back.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("internal error: {e:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let mut body = json!({ "error": self.message });
        if let Some(code) = self.code {
            body["code"] = json!(code);
        }
        (self.status, Json(body)).into_response()
    }
}
