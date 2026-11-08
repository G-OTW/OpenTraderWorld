//! Request to response, through the real Axum router: the guards in front of every handler
//! (session, same origin, demo gate), the validation and status codes handlers answer with,
//! and the MCP gateway over JSON-RPC.
//!
//! The happy-path CRUD and the shape of the main GET responses are the testbook's job
//! (`testbook/part1-api`, against a built stack); this module covers what it does not and
//! runs in plain `cargo test`.
//!
//! Tests that need a database use `#[sqlx::test]` (one throwaway database each, migrated
//! from `otw-store/migrations`) and are ignored unless the `pg-tests` feature is on:
//!
//! ```text
//! DATABASE_URL=postgres://postgres:test@127.0.0.1:55432/postgres \
//!     cargo test -p otw-core --features pg-tests api_tests
//! ```

mod auth;
mod demo;
mod mcp;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::AppState;

/// An `AppState` over `pool`. Nothing here is spawned that reaches the network.
pub(crate) async fn state_over(pool: PgPool) -> AppState {
    let cipher = otw_store::crypto::SecretCipher::from_master("api-tests").unwrap();
    let http = reqwest::Client::new();
    let live = crate::live::hub::LiveHub::new(pool.clone(), http.clone(), cipher.clone());
    AppState {
        pool: pool.clone(),
        upload_dir: std::env::temp_dir(),
        cipher: cipher.clone(),
        scheduler: otw_scheduler::Scheduler::new(pool.clone(), cipher.clone()),
        findb_importing: Default::default(),
        mportfolios_refresh: crate::mportfolios_job::new_lock(),
        watchlists_refresh: crate::watchlists::new_lock(),
        http: http.clone(),
        live: live.clone(),
        live_alerts: crate::histviz_live_alerts::spawn(
            pool.clone(),
            live,
            crate::histviz_alerts::AlertCx { cipher: cipher.clone(), http },
        ),
        exchanges: crate::exchanges::load(&pool).await,
        submit_limiter: crate::community_docs_api::SubmitLimiter::new(),
        optimizer: std::sync::Arc::new(crate::backtest::optimize::Jobs::new()),
        notifications: crate::notifications_stream::NotifHub::new(),
    }
}

/// An `AppState` whose database is unreachable and answers at once. For requests that a
/// guard must refuse before any handler reads anything.
pub(crate) async fn offline_state() -> AppState {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(200))
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .unwrap();
    state_over(pool).await
}

/// Every module's routes, merged the way `main` merges them.
pub(crate) fn api_routes() -> Router<AppState> {
    use crate::*;
    Router::new()
        .merge(documents::routes())
        .merge(versioning_api::routes())
        .merge(databases::routes())
        .merge(files::routes())
        .merge(video_embed::routes())
        .merge(feeds_api::routes())
        .merge(journal_api::routes())
        .merge(journal_lab_api::routes())
        .merge(journal_import_api::routes())
        .merge(jobs_api::routes())
        .merge(tasks_api::routes())
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
        .merge(ui_prefs_api::routes())
        .merge(social_login_api::routes())
        .merge(data_transfer_api::routes())
        .merge(network_api::routes())
        .merge(dashboard_api::routes())
        .merge(findb_api::routes())
        .merge(calendar_api::routes())
        .merge(fundamentals_api::routes())
        .merge(connectors_api::routes())
        .merge(brokers_api::routes())
        .merge(brokers_risk_api::routes())
        .merge(paper_broker_api::routes())
        .merge(histdata_api::routes())
        .merge(instrument_meta_api::routes())
        .merge(histdata_import_api::routes())
        .merge(histviz_api::routes())
        .merge(alert_trade_api::routes())
        .merge(market_sessions_api::routes())
        .merge(exchanges_api::routes())
        .merge(paper_api::routes())
        .merge(backtest_manifest_api::routes())
        .merge(
            Router::new()
                .merge(backtest_api::routes())
                .merge(backtest_optimize_api::routes())
                .merge(insights_api::routes())
                .merge(quant_api::routes())
                .merge(quant_market_api::routes())
                .layer(middleware::from_fn(security::compute_gate)),
        )
        .merge(options_api::routes())
        .merge(mportfolios_api::routes())
        .merge(portfolios_api::routes())
        .merge(portfolios_analytics_api::routes())
        .merge(portfolios_import_api::routes())
        .merge(watchlists_api::routes())
        .merge(scanner_api::routes())
        .merge(rate_api::routes())
        .merge(taxcalc_api::routes())
        .merge(taxcalc_broker::routes())
        .merge(vault_api::routes())
        .merge(voice_api::routes())
        .merge(vocab_api::routes())
        .merge(control_api::routes())
        .merge(mcp_api::routes())
        .merge(crate::mcp::oauth::session_routes())
        .merge(agent_api::routes())
        .merge(webhooks_api::routes())
        .merge(mailbox_api::routes())
        .merge(automator_api::routes())
}

/// The app as `main` serves it: the public routes, the MCP gateway, and every module route
/// behind the same-origin and session guards, in the same order.
pub(crate) fn app(state: AppState) -> Router {
    let protected = api_routes()
        .route_layer(middleware::from_fn_with_state(state.clone(), crate::require_auth))
        .route_layer(middleware::from_fn(crate::security::same_origin));
    Router::new()
        .route("/api/health", get(crate::api_health))
        .route("/api/mcp", post(crate::mcp::handle))
        .route("/api/demo", get(crate::demo::status))
        .merge(protected)
        .layer(middleware::from_fn(crate::security::request_timeout))
        .with_state(state)
}

/// The app wrapped in the demo sandbox gate, its outermost layer in `main`.
pub(crate) fn demo_app(state: AppState) -> Router {
    app(state).layer(middleware::from_fn(crate::demo::gate))
}

/// A request with a JSON body (or none), an optional session cookie and extra headers.
pub(crate) fn request(method: &str, path: &str, body: Option<&str>, cookie: Option<&str>, headers: &[(&str, &str)]) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(path).header(header::HOST, "otw.test");
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(token) = cookie {
        b = b.header(header::COOKIE, format!("{}={token}", crate::SESSION_COOKIE));
    }
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(body.map(|s| Body::from(s.to_string())).unwrap_or_else(Body::empty)).unwrap()
}

/// Send one request and read its status and JSON body (`Null` when the body is not JSON).
pub(crate) async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// The admin account and a live session for it; returns the cookie value.
pub(crate) async fn signed_in(pool: &PgPool) -> String {
    let user = match otw_store::find_admin(pool).await.unwrap() {
        Some(u) => u,
        None => otw_store::create_admin(pool, "owner", "not-a-real-hash", false).await.unwrap(),
    };
    let token = format!("api-tests-{}", uuid::Uuid::new_v4());
    otw_store::create_session(pool, &token, user.id, 1, "127.0.0.1", "api-tests").await.unwrap();
    token
}
