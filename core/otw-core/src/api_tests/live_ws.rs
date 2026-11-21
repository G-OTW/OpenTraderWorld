//! The tab's live socket (`GET /api/live/ws`) over a real TCP connection: an upgrade needs
//! one, a `oneshot` request cannot be upgraded. The protocol tests serve the route alone
//! (no session guard, no database); the guard tests serve the whole app.

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::{app, offline_state, signed_in, state_over};
use crate::notifications_stream::Push;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn serve(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// The socket route alone, over a state with no database behind it.
async fn bare() -> (SocketAddr, crate::AppState) {
    let state = offline_state().await;
    let addr = serve(crate::live_ws::routes().with_state(state.clone())).await;
    (addr, state)
}

async fn connect(addr: SocketAddr, headers: &[(&str, &str)]) -> Result<Ws, WsError> {
    let mut req = format!("ws://{addr}/api/live/ws").into_client_request().unwrap();
    for (k, v) in headers {
        let name = axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap();
        req.headers_mut().insert(name, v.parse().unwrap());
    }
    tokio_tungstenite::connect_async(req).await.map(|(ws, _)| ws)
}

async fn subscribe(ws: &mut Ws, id: &str, path: &str) {
    let msg = json!({ "op": "subscribe", "id": id, "path": path }).to_string();
    ws.send(Message::Text(msg)).await.unwrap();
}

async fn unsubscribe(ws: &mut Ws, id: &str) {
    let msg = json!({ "op": "unsubscribe", "id": id }).to_string();
    ws.send(Message::Text(msg)).await.unwrap();
}

/// The next frame that is not a ping, or `None` if nothing comes within `wait`.
async fn next_within(ws: &mut Ws, wait: Duration) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next()).await.ok()??.ok()?;
        if let Message::Text(text) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            if v["op"] != "ping" {
                return Some(v);
            }
        }
    }
}

async fn next(ws: &mut Ws) -> Value {
    next_within(ws, Duration::from_secs(5)).await.expect("a frame")
}

fn refusal_status(e: WsError) -> u16 {
    match e {
        WsError::Http(resp) => resp.status().as_u16(),
        other => panic!("expected an HTTP refusal, got {other}"),
    }
}

#[tokio::test]
async fn a_subscription_relays_its_route_events_until_unsubscribed() {
    let (addr, state) = bare().await;
    let mut ws = connect(addr, &[]).await.unwrap();

    subscribe(&mut ws, "n1", "/api/notifications/stream").await;
    assert_eq!(next(&mut ws).await, json!({ "id": "n1", "event": "open", "data": null }));
    state.notifications.publish(Push::Resync);
    assert_eq!(next(&mut ws).await, json!({ "id": "n1", "event": "resync", "data": null }));

    // Two subscriptions share the socket, each event routed by its id.
    subscribe(&mut ws, "n2", "/api/notifications/stream").await;
    assert_eq!(next(&mut ws).await, json!({ "id": "n2", "event": "open", "data": null }));
    unsubscribe(&mut ws, "n1").await;
    // Frames are handled in order: once n3 is open, n1 is gone.
    subscribe(&mut ws, "n3", "/api/feeds/stream").await;
    assert_eq!(next(&mut ws).await, json!({ "id": "n3", "event": "open", "data": null }));

    state.notifications.publish(Push::Resync);
    assert_eq!(next(&mut ws).await, json!({ "id": "n2", "event": "resync", "data": null }));
    assert_eq!(next_within(&mut ws, Duration::from_millis(300)).await, None, "n1 was unsubscribed");
}

#[tokio::test]
async fn a_refused_subscription_gets_an_error_frame_and_the_socket_stays() {
    let (addr, state) = bare().await;
    let mut ws = connect(addr, &[]).await.unwrap();

    subscribe(&mut ws, "u", "/api/todos").await;
    let v = next(&mut ws).await;
    assert_eq!((&v["id"], &v["event"], &v["data"]["status"]), (&json!("u"), &json!("error"), &json!(404)), "{v}");

    // A query the route's own extractor refuses.
    subscribe(&mut ws, "q", "/api/histviz/stream?provider=binance").await;
    let v = next(&mut ws).await;
    assert_eq!((&v["event"], &v["data"]["status"]), (&json!("error"), &json!(400)), "{v}");
    assert!(v["data"]["message"].as_str().unwrap().contains("missing field"), "{v}");

    // A query the handler refuses.
    subscribe(&mut ws, "f", "/api/fundamentals/stream?series=nope").await;
    let v = next(&mut ws).await;
    assert_eq!(v, json!({ "id": "f", "event": "error", "data": { "status": 400, "message": "\"nope\" is not a series id" } }));

    // The socket still serves.
    subscribe(&mut ws, "n", "/api/notifications/stream").await;
    assert_eq!(next(&mut ws).await["event"], "open");
    state.notifications.publish(Push::Resync);
    assert_eq!(next(&mut ws).await["id"], "n");
}

#[tokio::test]
async fn a_socket_carries_a_bounded_number_of_streams() {
    let (addr, _) = bare().await;
    let mut ws = connect(addr, &[]).await.unwrap();
    for i in 0..32 {
        subscribe(&mut ws, &format!("s{i}"), "/api/notifications/stream").await;
        assert_eq!(next(&mut ws).await["event"], "open");
    }
    subscribe(&mut ws, "over", "/api/notifications/stream").await;
    let v = next(&mut ws).await;
    assert_eq!((&v["id"], &v["data"]["status"]), (&json!("over"), &json!(429)), "{v}");
    // Letting one go makes room.
    unsubscribe(&mut ws, "s0").await;
    subscribe(&mut ws, "again", "/api/notifications/stream").await;
    assert_eq!(next(&mut ws).await, json!({ "id": "again", "event": "open", "data": null }));
}

/// The demo's live budget holds per subscription: a full budget refuses the chart stream,
/// and a refused or failed stream gives its slot back.
#[tokio::test]
async fn the_demo_slot_budget_refuses_a_chart_stream_when_full() {
    crate::demo::cap_live_slots_for_test(Some(1));
    let (addr, _) = bare().await;
    let mut ws = connect(addr, &[]).await.unwrap();
    // Valid coordinates on an unknown provider: refused with 400 once a slot is granted.
    let path = "/api/histviz/stream?provider=nowhere&asset_type=crypto&ticker=BTCUSDT&timeframe=1m";

    let held = crate::demo::live_slot().expect("one slot");
    subscribe(&mut ws, "a", path).await;
    let v = next(&mut ws).await;
    assert_eq!(v["data"]["status"], 429, "the budget is full: {v}");

    drop(held);
    for id in ["b", "c"] {
        subscribe(&mut ws, id, path).await;
        let v = next(&mut ws).await;
        assert_eq!(v["data"]["status"], 400, "a slot was granted, then given back on refusal: {v}");
    }
    crate::demo::cap_live_slots_for_test(None);
}

#[tokio::test]
async fn an_upgrade_from_another_site_is_refused() {
    let (addr, _) = bare().await;
    let host = addr.to_string();
    let e = connect(addr, &[("origin", "http://evil.example")]).await.unwrap_err();
    assert_eq!(refusal_status(e), 403);
    let e = connect(addr, &[("sec-fetch-site", "cross-site")]).await.unwrap_err();
    assert_eq!(refusal_status(e), 403);
    let origin = format!("http://{host}");
    assert!(connect(addr, &[("origin", &origin), ("sec-fetch-site", "same-origin")]).await.is_ok());
}

#[tokio::test]
async fn an_unauthenticated_upgrade_is_refused() {
    let addr = serve(app(offline_state().await)).await;
    let e = connect(addr, &[]).await.unwrap_err();
    assert_eq!(refusal_status(e), 401);
}

#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_signed_in_tab_opens_the_socket_through_the_whole_app(pool: PgPool) {
    let token = signed_in(&pool).await;
    let state = state_over(pool).await;
    let addr = serve(app(state.clone())).await;
    let cookie = format!("{}={token}", crate::SESSION_COOKIE);
    let mut ws = connect(addr, &[("cookie", &cookie)]).await.unwrap();
    subscribe(&mut ws, "n", "/api/notifications/stream").await;
    assert_eq!(next(&mut ws).await["event"], "open");
    state.notifications.publish(Push::Resync);
    assert_eq!(next(&mut ws).await, json!({ "id": "n", "event": "resync", "data": null }));

    let e = connect(addr, &[("cookie", &format!("{}=forged", crate::SESSION_COOKIE))]).await.unwrap_err();
    assert_eq!(refusal_status(e), 401);
}
