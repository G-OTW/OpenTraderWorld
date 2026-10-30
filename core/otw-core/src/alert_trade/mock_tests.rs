//! Trades on alert driven end to end: the fire, the sender, the venue (a local server that
//! answers like Binance spot testnet), the fallback, the fill followed, and the routes.
//!
//! Over a scratch database (`OTW_TEST_DATABASE_URL`, like the sender's own mock tests): each
//! test returns at once without one. Every account and alert carries a fresh name and key.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use serde_json::{json, Value};
use time::OffsetDateTime;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::{execute, follow, Fired, Trigger};
use crate::brokers::mock_tests::{mock_binance_spot, scratch_state, sign_background, testnet_trading_account, venue_order};
use crate::brokers::serve_from;
use crate::AppState;
use otw_store::alert_trades as store;
use otw_store::histviz::{self as alerts, Alert, AlertInput};

/// A spot limit at the price that fired, 0.0002 BTC.
fn limit_ticket() -> Value {
    json!({ "side": "buy", "order_type": "limit", "tif": "gtc", "size": { "kind": "units", "value": "0.0002" }, "limit_offset": { "kind": "abs", "value": "0" } })
}

async fn alert_on(state: &AppState, tag: &str, frequency: &str) -> Alert {
    alerts::create_alert(
        &state.pool,
        AlertInput {
            name: &format!("trade {tag} {}", Uuid::new_v4()),
            provider: "binance",
            asset_type: "crypto",
            ticker: "BTCUSDT",
            timeframe: "1h",
            connector_id: None,
            lhs: &json!({ "kind": "price", "field": "close" }),
            rhs: &json!({ "kind": "const", "value": 1.0 }),
            inputs: &json!({}),
            op: "above",
            // In the app only: no channel is pushed to from a test.
            channels: &json!([]),
            mode: "background",
            frequency,
            cooldown_secs: if frequency == "cooldown" { 60 } else { 0 },
            session_id: None,
            // Paused: no evaluator reads it; the tests fire it themselves.
            enabled: false,
            expires_at: None,
            fire_at: None,
        },
    )
    .await
    .unwrap()
}

async fn armed_trade(state: &AppState, a: &Alert, account: Uuid, fallback: &str, arm: bool) {
    store::save(&state.pool, a.id, account, "enter", &limit_ticket(), fallback).await.unwrap();
    if arm {
        store::arm(&state.pool, a.id, Uuid::nil()).await.unwrap().unwrap();
    }
}

fn fired_now() -> Option<Trigger> {
    Some(Trigger { price: 70_000.0, at: OffsetDateTime::now_utc() })
}

fn posts(reqs: &[Request]) -> usize {
    reqs.iter().filter(|r| r.method.as_str() == "POST" && r.url.path() == "/api/v3/order").count()
}

/// Notifications written for an alert, by title.
async fn told(state: &AppState, a: &Alert) -> Vec<String> {
    sqlx::query_scalar("SELECT name FROM notifications WHERE linked_id = $1 ORDER BY created_at").bind(a.id).fetch_all(&state.pool).await.unwrap()
}

/// The venue looks an order up by the client id it was asked for, and has it working: the
/// answer to the place itself was lost (503), which is what the sender's tests use too.
struct Echo;
impl Respond for Echo {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let id = req.url.query_pairs().find(|(k, _)| k == "origClientOrderId").map(|(_, v)| v.to_string()).unwrap_or_default();
        ResponseTemplate::new(200).set_body_json(venue_order(&id, "NEW"))
    }
}

async fn venue_has_it(server: &MockServer) {
    Mock::given(method("GET")).and(path("/api/v3/order")).respond_with(Echo).mount(server).await;
    Mock::given(method("GET")).and(path("/api/v3/openOrders")).respond_with(ResponseTemplate::new(200).set_body_json(json!([]))).mount(server).await;
}

/// Fire, placed, then filled: `sent`, the fill followed from what the reconciliation stored,
/// notified once.
#[tokio::test]
async fn a_fired_trade_is_placed_then_its_fill_is_told_once() {
    let Some(state) = scratch_state().await else { return };
    let server = MockServer::start().await;
    mock_binance_spot(&server).await;
    venue_has_it(&server).await;
    serve_from(&server.uri());
    let account = testnet_trading_account(&state, "atfill").await;
    sign_background(&state, &account).await;
    let a = alert_on(&state, "fill", "once").await;
    armed_trade(&state, &a, account.id, "none", true).await;

    let fired = execute(&state, &a, fired_now()).await.unwrap().unwrap();
    let Fired::Sent { keys, placed, order } = &fired else { panic!("placed: {fired:?}") };
    assert_eq!(order, "buy 0.0002 BTCUSDT limit 70000");
    assert_eq!(placed[0].key, keys[0]);
    let row = store::get(&state.pool, a.id).await.unwrap().unwrap();
    assert_eq!(row.status, "sent");
    assert!(!row.armed(), "a fire disarms its trade");
    assert_eq!(posts(&server.received_requests().await.unwrap()), 1);
    let log = otw_store::order_log::list(&state.pool, Some(account.id), 10).await.unwrap();
    assert_eq!(log.iter().find(|e| e.action == "place").and_then(|e| e.source.clone()), Some(format!("alert:{}", a.id)));

    // The reconciliation cycle reads the fill at the venue and stores it.
    otw_store::broker_orders::set_status(&state.pool, account.id, &keys[0], "filled", None, None, Some("0.0002")).await.unwrap();
    follow(&state).await.unwrap();
    follow(&state).await.unwrap();
    let row = store::get(&state.pool, a.id).await.unwrap().unwrap();
    assert_eq!(row.status, "filled");
    assert_eq!(row.last_result["filled_qty"], "0.0002");
    let names = told(&state, &a).await;
    assert_eq!(names.iter().filter(|n| n.starts_with("Trade sent")).count(), 1, "{names:?}");
    assert_eq!(names.iter().filter(|n| n.starts_with("Trade filled")).count(), 1, "told once: {names:?}");

    // Fired again: the trade was disarmed, so nothing more leaves.
    assert!(matches!(execute(&state, &a, fired_now()).await.unwrap(), Some(Fired::NotArmed)));
    assert_eq!(posts(&server.received_requests().await.unwrap()), 1);
}

/// The venue refuses: `refused`, the fallback runs (here `close_symbol`), and the user is told
/// the refusal with what the fallback did.
#[tokio::test]
async fn a_refused_trade_runs_its_fallback() {
    let Some(state) = scratch_state().await else { return };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v3/order"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "code": -2010, "msg": "Account has insufficient balance for requested action." })))
        .with_priority(1)
        .mount(&server)
        .await;
    mock_binance_spot(&server).await;
    venue_has_it(&server).await;
    serve_from(&server.uri());
    let account = testnet_trading_account(&state, "atrej").await;
    sign_background(&state, &account).await;
    let a = alert_on(&state, "reject", "once").await;
    armed_trade(&state, &a, account.id, "close_symbol", true).await;

    let fired = execute(&state, &a, fired_now()).await.unwrap().unwrap();
    let Fired::Refused { message, fallback: Some(fb), .. } = &fired else { panic!("refused with its fallback: {fired:?}") };
    assert!(message.contains("insufficient balance"), "{message}");
    assert_eq!(fb.kind, "close_symbol");
    let row = store::get(&state.pool, a.id).await.unwrap().unwrap();
    assert!(["fallback_done", "fallback_failed"].contains(&row.status.as_str()), "{}", row.status);
    assert_eq!(posts(&server.received_requests().await.unwrap()), 1, "a refusal is never sent again");
    let names = told(&state, &a).await;
    assert!(names.iter().any(|n| n.starts_with("Trade refused")), "{names:?}");
}

/// No answer and no lookup: `unknown`, told to check at the broker, and the fallback cut down
/// to cancelling whatever was chosen: no cancel-all, no close, nothing sent again.
#[tokio::test]
async fn an_unknown_outcome_only_cancels_on_the_instrument() {
    let Some(state) = scratch_state().await else { return };
    let server = MockServer::start().await;
    mock_binance_spot(&server).await;
    Mock::given(method("GET")).and(path("/api/v3/order")).respond_with(ResponseTemplate::new(503).set_body_string("upstream timed out")).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v3/openOrders")).respond_with(ResponseTemplate::new(200).set_body_json(json!([]))).mount(&server).await;
    serve_from(&server.uri());
    let account = testnet_trading_account(&state, "atunk").await;
    sign_background(&state, &account).await;
    let a = alert_on(&state, "unknown", "once").await;
    armed_trade(&state, &a, account.id, "flatten_account", true).await;

    let fired = execute(&state, &a, fired_now()).await.unwrap().unwrap();
    let Fired::Unknown { keys, fallback: Some(fb), .. } = &fired else { panic!("unknown: {fired:?}") };
    assert_eq!(keys.len(), 1);
    assert_eq!(fb.kind, "cancel_symbol", "an unknown order never has exposure added next to it");
    assert_eq!(store::get(&state.pool, a.id).await.unwrap().unwrap().status, "unknown");
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(posts(&reqs), 1);
    assert!(!reqs.iter().any(|r| r.method.as_str() == "DELETE" && r.url.path() == "/api/v3/openOrders"), "no cancel-all");
    assert!(told(&state, &a).await.iter().any(|n| n.starts_with("Trade outcome unknown")));
}

/// A trade not armed: the fire is told, nothing is sent. A fire caught mid-send by a restart is
/// unknown, never sent again; an armed trade that did not fire stays armed.
#[tokio::test]
async fn a_disarmed_trade_sends_nothing_and_a_restart_resends_nothing() {
    let Some(state) = scratch_state().await else { return };
    let server = MockServer::start().await;
    mock_binance_spot(&server).await;
    venue_has_it(&server).await;
    serve_from(&server.uri());
    let account = testnet_trading_account(&state, "atoff").await;
    sign_background(&state, &account).await;

    let idle = alert_on(&state, "idle", "once").await;
    armed_trade(&state, &idle, account.id, "none", false).await;
    assert!(matches!(execute(&state, &idle, fired_now()).await.unwrap(), Some(Fired::NotArmed)));
    assert!(told(&state, &idle).await.iter().any(|n| n.starts_with("Trade not armed")));

    // Taken by a fire, then the process died before an answer.
    let caught = alert_on(&state, "caught", "once").await;
    armed_trade(&state, &caught, account.id, "none", true).await;
    let waiting = alert_on(&state, "waiting", "once").await;
    armed_trade(&state, &waiting, account.id, "none", true).await;
    assert!(store::take(&state.pool, caught.id).await.unwrap().is_some());
    assert!(store::take(&state.pool, caught.id).await.unwrap().is_none(), "taken once");
    let moved = store::sending_to_unknown(&state.pool).await.unwrap();
    assert!(moved.iter().any(|r| r.alert_id == caught.id));
    assert_eq!(store::get(&state.pool, caught.id).await.unwrap().unwrap().status, "unknown");
    assert!(matches!(execute(&state, &caught, fired_now()).await.unwrap(), Some(Fired::NotArmed)));
    assert!(store::get(&state.pool, waiting.id).await.unwrap().unwrap().armed(), "the row is the truth: still armed");
    assert_eq!(posts(&server.received_requests().await.unwrap()), 0, "nothing was sent");
}

/// Ten trades armed on one instrument go out; the eleventh stops at the account's position
/// limit, refused with the limit named.
#[tokio::test]
async fn armed_trades_stop_at_the_position_limit() {
    let Some(state) = scratch_state().await else { return };
    let server = MockServer::start().await;
    let sent = Arc::new(AtomicUsize::new(0));
    struct Lost(Arc<AtomicUsize>);
    impl Respond for Lost {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            self.0.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(503).set_body_string("upstream timed out")
        }
    }
    // Every order fills at once: the account holds 0.0002 BTC more after each.
    struct Holds(Arc<AtomicUsize>);
    impl Respond for Holds {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let held = otw_connector_model::Decimal::new(2, 4) * otw_connector_model::Decimal::from(self.0.load(Ordering::SeqCst) as u64);
            ResponseTemplate::new(200).set_body_json(json!({ "balances": [{ "asset": "BTC", "free": held.to_string(), "locked": "0" }] }))
        }
    }
    Mock::given(method("POST")).and(path("/api/v3/order")).respond_with(Lost(sent.clone())).with_priority(1).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v3/account")).respond_with(Holds(sent.clone())).mount(&server).await;
    mock_binance_spot(&server).await;
    venue_has_it(&server).await;
    serve_from(&server.uri());
    let account = testnet_trading_account(&state, "atlim").await;
    sign_background(&state, &account).await;
    let limits = otw_store::broker_risk::Limits { max_position_qty: Some("0.002".parse().unwrap()), ..Default::default() };
    otw_store::broker_risk::set_limits(&state.pool, account.id, &limits).await.unwrap();

    let mut armed = Vec::new();
    for i in 0..11 {
        let a = alert_on(&state, &format!("ten{i}"), "once").await;
        armed_trade(&state, &a, account.id, "none", true).await;
        armed.push(a);
    }
    for (i, a) in armed.iter().enumerate() {
        let fired = execute(&state, a, fired_now()).await.unwrap().unwrap();
        if i < 10 {
            assert!(matches!(fired, Fired::Sent { .. }), "trade {i}: {fired:?}");
        } else {
            let Fired::Refused { code, message, .. } = &fired else { panic!("the eleventh is refused: {fired:?}") };
            assert_eq!(code, "risk_limit");
            assert!(message.contains("position limit of 0.002"), "{message}");
        }
    }
    assert_eq!(sent.load(Ordering::SeqCst), 10);
}

// ── Routes ───────────────────────────────────────────────────────────────────

fn user(id: Uuid) -> otw_store::User {
    otw_store::User { id, username: format!("u{id}"), password_hash: String::new(), is_admin: true, must_change_password: false, totp_enabled: true }
}

async fn call(app: &axum::Router, method: &str, uri: &str, body: Value, cookie: bool) -> (StatusCode, Value) {
    let mut req = HttpRequest::builder().method(method).uri(uri).header("content-type", "application/json");
    if cookie {
        req = req.header("cookie", format!("{}=t-{}", crate::SESSION_COOKIE, Uuid::new_v4()));
    }
    let res = app.clone().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// The trade row's routes: written (the alert then fires once), refused to arm without
/// background trading, armed with it, refused without the code when the trading lock is on,
/// refused to an agent, previewed with what is already armed, disarmed, removed.
#[tokio::test]
async fn the_trade_routes_arm_only_what_the_user_signed_for() {
    let Some(state) = scratch_state().await else { return };
    let server = MockServer::start().await;
    mock_binance_spot(&server).await;
    venue_has_it(&server).await;
    serve_from(&server.uri());
    let account = testnet_trading_account(&state, "atapi").await;
    let a = alert_on(&state, "api", "every").await;
    let me = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, username, password_hash, totp_enabled) VALUES ($1, $2, '', true)").bind(me).bind(format!("u{me}")).execute(&state.pool).await.unwrap();
    let app = crate::alert_trade_api::routes().with_state(state.clone()).layer(axum::Extension(user(me)));
    let base = format!("/api/histviz/alerts/{}/trade", a.id);

    let body = json!({ "account_id": account.id, "intent": "enter", "ticket": limit_ticket(), "fallback": "cancel_symbol" });
    let (st, v) = call(&app, "PUT", &base, body.clone(), false).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(alerts::get_alert(&state.pool, a.id).await.unwrap().unwrap().frequency, "once", "an alert with a trade fires once");
    let (st, v) = call(&app, "PUT", &base, json!({ "account_id": account.id, "intent": "enter", "ticket": { "side": "buy" }, "fallback": "none" }), false).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "an incomplete ticket is refused: {v}");

    let (st, v) = call(&app, "PUT", &format!("{base}/arm"), json!({}), false).await;
    assert_eq!((st, v["code"].as_str()), (StatusCode::FORBIDDEN, Some("background_trading_required")), "{v}");
    sign_background(&state, &account).await;
    let (st, v) = call(&app, "PUT", &format!("{base}/arm"), json!({}), false).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["trade"]["armed_at"].is_string());

    // Written again as it was: still armed. Changed: disarmed.
    let (_, v) = call(&app, "PUT", &base, body.clone(), false).await;
    assert_eq!(v["disarmed"], false);
    let mut changed = body.clone();
    changed["fallback"] = json!("none");
    let (_, v) = call(&app, "PUT", &base, changed, false).await;
    assert_eq!(v["disarmed"], true);

    // The trading lock asks for the code in the same request.
    sqlx::query("UPDATE users SET trading_lock = true WHERE id = $1").bind(me).execute(&state.pool).await.unwrap();
    let (st, _) = call(&app, "PUT", &format!("{base}/arm"), json!({}), false).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "no session, no arming under the lock");
    let (st, v) = call(&app, "PUT", &format!("{base}/arm"), json!({}), true).await;
    assert_eq!((st, v["code"].as_str()), (StatusCode::FORBIDDEN, Some("code_required")), "{v}");
    sqlx::query("UPDATE users SET trading_lock = false WHERE id = $1").bind(me).execute(&state.pool).await.unwrap();

    // Never by an agent or a workflow.
    let agent = crate::alert_trade_api::routes().with_state(state.clone()).layer(axum::Extension(crate::internal_call::Automated)).layer(axum::Extension(user(me)));
    let (st, _) = call(&agent, "PUT", &format!("{base}/arm"), json!({}), false).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Preview at the chart's price, with another trade armed on the instrument.
    let other = alert_on(&state, "other", "once").await;
    armed_trade(&state, &other, account.id, "none", true).await;
    let (st, v) = call(&app, "POST", &format!("{base}/preview"), json!({ "price": "70000" }), false).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!((v["ok"].as_bool(), v["qty"].as_str(), v["limit"].as_str()), (Some(true), Some("0.0002"), Some("70000")), "{v}");
    assert_eq!((v["exposure"]["count"].as_u64(), v["exposure"]["buy"].as_str()), (Some(1), Some("0.0002")), "{v}");
    // A draft is previewed before it is saved, and a size the venue refuses says why.
    let mut draft = body.clone();
    draft["ticket"]["size"] = json!({ "kind": "quote_amount", "value": "0.5" });
    let (_, v) = call(&app, "POST", &format!("{base}/preview"), json!({ "price": "70000", "trade": draft }), false).await;
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["message"].as_str().unwrap_or("").contains("less than one size step"), "{v}");
    assert_eq!(posts(&server.received_requests().await.unwrap()), 0, "a preview places nothing");

    let (st, _) = call(&app, "PUT", &format!("{base}/arm"), json!({}), false).await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = call(&app, "DELETE", &format!("{base}/arm"), json!({}), false).await;
    assert_eq!(v["disarmed"], true);
    let (st, _) = call(&app, "DELETE", &base, json!({}), false).await;
    assert_eq!(st, StatusCode::OK);
    assert!(store::get(&state.pool, a.id).await.unwrap().is_none());
}

/// Turning background trading off disarms every trade armed on the account.
#[tokio::test]
async fn a_disarm_hook_disarms_the_accounts_trades() {
    let Some(state) = scratch_state().await else { return };
    let account = testnet_trading_account(&state, "athook").await;
    let a = alert_on(&state, "hook", "once").await;
    armed_trade(&state, &a, account.id, "none", true).await;
    let d = crate::brokers::dispatch::disarm_account(&state, account.id).await.unwrap();
    assert_eq!(d.alert_trades, 1);
    assert!(!store::get(&state.pool, a.id).await.unwrap().unwrap().armed());
    // Deleting the alert deletes its trade.
    assert!(alerts::delete_alert(&state.pool, a.id).await.unwrap());
    assert!(store::get(&state.pool, a.id).await.unwrap().is_none());
}
