//! The generic trading broker, end to end through the `Broker` trait on Bybit, against a local
//! server that answers like the venue (`serve_from`). The scenario catalog itself runs on the
//! trader in the provider's tests; these cover what the broker adds: the book each instrument
//! trades on, the ids, the live gate (E2D), the lookup the reconciliation cycle asks, the
//! instrument's grid and leverage, and the account reads left as they were.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;

use otw_connector_model::{Decimal, ExecLevel, OrderStatus, OrderType, Side, TifKind};
use otw_connector_sdk::trade::TradeMapper;
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::super::{broker_for, client, serve_from, Lookup, OrderLeg, OrderTicket, OrderUnknown, INSTRUMENT_BOOK, LIVE_CANARY, LIVE_UNTESTED};

/// An account's settings, its chart on a Bybit linear connector (the core names the book from
/// it). The traders' instrument rules are kept per host for the process, and a mock server's
/// port is reused across tests: naming the book keeps one test's listing out of another's.
fn creds(tag: &str, extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = [("api_key", format!("mock-{tag}-{}", uuid::Uuid::new_v4().simple())), ("api_secret", "s".to_string()), (INSTRUMENT_BOOK, "linear".to_string())]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    for (k, v) in extra {
        m.insert(k.to_string(), v.to_string());
    }
    m
}

fn ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "retCode": 0, "retMsg": "OK", "result": result, "retExtInfo": {}, "time": 1 }))
}

fn limit(key: &str) -> OrderTicket {
    let mut leg = OrderLeg::new(key, Side::Buy, OrderType::Limit, Decimal::new(1, 2));
    leg.limit = Some(Decimal::from(30_000));
    leg.tif = Some(TifKind::Gtc);
    OrderTicket { ticker: "BTCUSDT".into(), asset_type: "crypto".into(), legs: vec![leg], provider: None, connector: None, reference: None, reference_at: None, confirm_duplicate: false }
}

const LINEAR: &str = include_str!("../../../connectors/providers/src/bybit/fixtures/trade/doc_instruments_linear.json");
const SPOT: &str = include_str!("../../../connectors/providers/src/bybit/fixtures/trade/doc_instruments_spot.json");

/// `BTCUSDT` listed on the linear book only (instruments-info example; the spot list answers
/// empty), and a one-way flat position.
async fn venue(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v5/market/instruments-info"))
        .and(query_param("category", "linear"))
        .respond_with(ResponseTemplate::new(200).set_body_string(LINEAR))
        .mount(server)
        .await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).respond_with(ok(json!({ "list": [], "nextPageCursor": "" }))).mount(server).await;
    Mock::given(method("GET"))
        .and(path("/v5/position/list"))
        .respond_with(ok(json!({ "list": [{ "symbol": "BTCUSDT", "side": "", "size": "0", "positionIdx": 0 }], "nextPageCursor": "", "category": "linear" })))
        .mount(server)
        .await;
}

fn order_row(cid: &str, oid: &str, status: &str) -> Value {
    json!({ "list": [{ "orderId": oid, "orderLinkId": cid, "symbol": "BTCUSDT", "side": "Buy", "orderType": "Limit", "orderStatus": status,
        "qty": "0.01", "cumExecQty": "0", "price": "30000", "triggerPrice": "0", "avgPrice": "", "reduceOnly": false, "positionIdx": 0, "createdTime": "1" }],
        "nextPageCursor": "", "category": "linear" })
}

#[test]
fn bybit_takes_orders_at_e2d_on_spot_and_linear_only() {
    let b = broker_for("bybit").unwrap();
    let caps = b.order_caps().unwrap();
    assert_eq!(caps.level, ExecLevel::E2D);
    assert!(caps.types.contains(&OrderType::Stop) && caps.reduce_only, "{caps:?}");
    assert!(caps.brackets && !caps.oca && !caps.amend, "{caps:?}");
    assert!(caps.position_exits, "the linear book sets a position's own take profit and stop loss");
    assert_eq!(super::super::live::level(b.as_ref()), ExecLevel::E2D);
    assert!(b.has_dry_run());
}

#[test]
fn bybit_eu_takes_spot_orders_at_e2d() {
    let b = broker_for("bybit_eu").unwrap();
    let caps = b.order_caps().unwrap();
    assert_eq!(caps.level, ExecLevel::E2D);
    assert!(caps.types.contains(&OrderType::Market) && caps.types.contains(&OrderType::Limit) && !caps.types.contains(&OrderType::Stop), "{caps:?}");
    assert!(!caps.reduce_only && !caps.brackets && !caps.amend && !caps.position_exits && caps.position_mode.is_none(), "{caps:?}");
    assert!(!b.has_dry_run());
}

/// Bybit EU end to end: a testnet order goes out on the EU spot book under `spot:SYMBOL:id`, a
/// live one needs the core's mark; no private stream (polled), no position to close.
#[tokio::test]
async fn a_bybit_eu_order_goes_to_its_spot_book_and_is_polled() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).and(query_param("category", "spot")).respond_with(ResponseTemplate::new(200).set_body_string(SPOT)).mount(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ok(json!({ "orderId": "o-eu", "orderLinkId": "eu1" }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit_eu").unwrap();
    let mut s = creds("eu", &[("site", "eu-testnet")]);
    s.remove(INSTRUMENT_BOOK);
    assert!(!b.is_live(&s).await.unwrap());
    let placed = b.place(&s, &limit("eu1")).await.unwrap();
    assert_eq!(placed[0].id, "spot:BTCUSDT:o-eu");
    let create = server.received_requests().await.unwrap().into_iter().find(|r| r.url.path() == "/v5/order/create").unwrap();
    assert!(String::from_utf8_lossy(&create.body).contains("\"category\":\"spot\""));
    assert!(b.events(&s).await.unwrap().is_none(), "no EU private stream: polled");
    let mut live = creds("eu-live", &[("site", "eu")]);
    live.remove(INSTRUMENT_BOOK);
    let e = b.place(&live, &limit("eu2")).await.unwrap_err().to_string();
    assert!(e.contains("never tested against the venue"), "{e}");
    let e = b.close_position(&s, "BTCUSDT", "", "c").await.unwrap_err().to_string();
    assert!(e.contains("not positions"), "{e}");
}

/// `BTCUSDT` is a spot pair and a perpetual at once: with both books listing it and no data
/// connector to say which, the order is refused with the fix; the chart's connector names it.
#[tokio::test]
async fn an_instrument_two_books_list_goes_to_the_book_its_connector_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).and(query_param("category", "spot")).respond_with(ResponseTemplate::new(200).set_body_string(SPOT)).mount(&server).await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ok(json!({ "orderId": "o-s", "orderLinkId": "two1" }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let mut s = creds("two", &[("site", "testnet")]);
    s.remove(INSTRUMENT_BOOK);
    let e = b.place(&s, &limit("two0")).await.unwrap_err().to_string();
    assert!(e.contains("spot and linear") && e.contains("data connector"), "{e}");
    assert!(server.received_requests().await.unwrap().iter().all(|r| r.url.path() != "/v5/order/create"), "nothing sent");
    s.insert(INSTRUMENT_BOOK.into(), "spot".into());
    let placed = b.place(&s, &limit("two1")).await.unwrap();
    assert_eq!(placed[0].id, "spot:BTCUSDT:o-s");
    let create = server.received_requests().await.unwrap().into_iter().find(|r| r.url.path() == "/v5/order/create").unwrap();
    assert!(String::from_utf8_lossy(&create.body).contains("\"category\":\"spot\""));
}

#[tokio::test]
async fn a_testnet_order_goes_out_under_symbol_and_venue_id() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ok(json!({ "orderId": "o-1", "orderLinkId": "nt1" }))).mount(&server).await;
    serve_from(&server.uri());
    let s = creds("tn", &[("site", "testnet")]);
    let b = broker_for("bybit").unwrap();
    assert!(!b.is_live(&s).await.unwrap());
    let placed = b.place(&s, &limit("nt1")).await.unwrap();
    assert_eq!((placed[0].key.as_str(), placed[0].id.as_str()), ("nt1", "linear:BTCUSDT:o-1"));
    assert_eq!(placed[0].tick.map(|t| t.normalize().to_string()).as_deref(), Some("0.1"));
    let reqs = server.received_requests().await.unwrap();
    assert!(reqs.iter().all(|r| r.url.path() != "/v5/order/pre-check"), "a testnet order needs no check");
    let create = reqs.iter().find(|r| r.url.path() == "/v5/order/create").unwrap();
    assert!(create.headers.contains_key("x-bapi-sign"));
}

#[tokio::test]
async fn a_live_order_at_e2d_needs_the_cores_mark_then_the_venues_check_once_per_session() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/pre-check")).respond_with(ok(json!({ "orderId": "", "orderLinkId": "", "preImrE4": "30", "preMmrE4": "21", "postImrE4": "357", "postMmrE4": "294" }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ok(json!({ "orderId": "o-live", "orderLinkId": "x" }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let mut s = creds("live", &[]);
    assert!(b.is_live(&s).await.unwrap());
    let e = b.place(&s, &limit("lv0")).await.unwrap_err().to_string();
    assert!(e.contains("never tested against the venue"), "{e}");
    assert!(server.received_requests().await.unwrap().iter().all(|r| r.url.path() != "/v5/order/create"), "refused before any order left");
    s.insert(LIVE_UNTESTED.into(), "1".into());
    b.place(&s, &limit("lv1")).await.unwrap();
    b.place(&s, &limit("lv2")).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    let n = |p: &str| reqs.iter().filter(|r| r.url.path() == p).count();
    assert_eq!((n("/v5/order/pre-check"), n("/v5/order/create")), (1, 2), "checked once, before the first live order of the session");
    // The canary passes too (E3 to E4 path, unchanged).
    let mut c = creds("canary", &[]);
    c.insert(LIVE_CANARY.into(), "1".into());
    b.place(&c, &limit("lv3")).await.unwrap();
}

#[tokio::test]
async fn a_live_order_the_venues_check_refuses_places_nothing() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/pre-check")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "retCode": 110007, "retMsg": "Available balance is insufficient", "result": {}, "retExtInfo": {}, "time": 1 }))).mount(&server).await;
    serve_from(&server.uri());
    let mut s = creds("liveno", &[]);
    s.insert(LIVE_UNTESTED.into(), "1".into());
    let e = broker_for("bybit").unwrap().place(&s, &limit("ln1")).await.unwrap_err().to_string();
    assert!(e.contains("check refused"), "{e}");
    assert!(server.received_requests().await.unwrap().iter().all(|r| r.url.path() != "/v5/order/create"));
}

#[tokio::test]
async fn a_lost_answer_is_unknown_and_a_lookup_answers_the_reconciliation_cycle() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ResponseTemplate::new(502)).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "lost1")).respond_with(ResponseTemplate::new(502)).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "found1")).respond_with(ok(order_row("found1", "o-f", "PartiallyFilled"))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "odd1")).respond_with(ok(order_row("odd1", "o-odd", "Sleeping"))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "none1")).respond_with(ok(json!({ "list": [], "nextPageCursor": "" }))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/order/history")).respond_with(ok(json!({ "list": [], "nextPageCursor": "" }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let s = creds("lost", &[("site", "testnet")]);
    let e = b.place(&s, &limit("lost1")).await.unwrap_err();
    let u = e.downcast_ref::<OrderUnknown>().unwrap_or_else(|| panic!("{e:#}"));
    assert_eq!((u.keys.clone(), u.alert), (vec!["lost1".to_string()], false));
    // Secrets scan: what reaches the audit log names neither the key nor a signature.
    let said = format!("{e:#}");
    assert!(!said.contains(s["api_key"].as_str()) && !said.to_ascii_lowercase().contains("x-bapi-sign"), "{said}");
    match b.lookup(&s, "BTCUSDT", "crypto", "found1").await.unwrap() {
        Lookup::Found { id, status, filled_qty, fills_as } => {
            assert_eq!((id.as_str(), status, filled_qty, fills_as.as_deref()), ("linear:BTCUSDT:o-f", OrderStatus::PartiallyFilled, Decimal::ZERO, Some("o-f")));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.lookup(&s, "BTCUSDT", "crypto", "none1").await.unwrap(), Lookup::NotFound);
    // An unknown status word is an answer nobody mapped, said as such.
    let odd = b.lookup(&s, "BTCUSDT", "crypto", "odd1").await.unwrap_err();
    assert!(odd.downcast_ref::<super::UnmappedAnswer>().is_some(), "{odd:#}");
}

#[tokio::test]
async fn cancel_and_close_go_to_the_book_named_in_their_id() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/cancel")).and(body_string_contains("o-c")).respond_with(ok(json!({ "orderId": "o-c", "orderLinkId": "" }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let s = creds("cc", &[("site", "testnet")]);
    b.cancel(&s, "linear:BTCUSDT:o-c").await.unwrap();
    assert!(b.cancel(&s, "BTCUSDT:o-c").await.unwrap_err().to_string().contains("BOOK:SYMBOL:id"));
    assert!(b.cancel(&s, "inverse:BTCUSD:o-c").await.unwrap_err().to_string().contains("no orders on its inverse book"));
    // No position held: nothing to close, nothing sent.
    let e = b.close_position(&s, "linear:BTCUSDT:long", "", "close1").await.unwrap_err().to_string();
    assert!(e.contains("no position"), "{e}");
    let reqs = server.received_requests().await.unwrap();
    let cancel = reqs.iter().find(|r| r.url.path() == "/v5/order/cancel").unwrap();
    assert!(String::from_utf8_lossy(&cancel.body).contains("\"category\":\"linear\""));
    assert!(reqs.iter().all(|r| r.url.path() != "/v5/order/create"));
}

#[tokio::test]
async fn a_spot_balance_is_not_a_position_to_close() {
    let server = MockServer::start().await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let s = creds("spot", &[("site", "testnet"), (INSTRUMENT_BOOK, "spot")]);
    let e = b.close_position(&s, "BTCUSDT", "", "c").await.unwrap_err().to_string();
    assert!(e.contains("not positions"), "{e}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_reads_are_left_as_they_were() {
    // The account read goes through the wrapped read broker: a pull still pages per book.
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v5/execution/list")).respond_with(ok(json!({ "list": [], "nextPageCursor": "" }))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).respond_with(ok(json!({ "list": [] }))).mount(&server).await;
    serve_from(&server.uri());
    let now = time::OffsetDateTime::now_utc();
    let q = super::super::ExecQuery { from: now - time::Duration::days(3), to: now, symbols: vec![] };
    broker_for("bybit").unwrap().executions(&client().unwrap(), &creds("read", &[("books", "spot")]), &q).await.unwrap();
    assert!(server.received_requests().await.unwrap().iter().any(|r| r.url.path() == "/v5/execution/list"));
}

/// E2D against the store (needs `OTW_TEST_DATABASE_URL`, like the other scratch-database
/// tests): the live gate asks for the opt-in, the limits, then the acknowledgement.
#[tokio::test]
async fn e2d_gate_and_acknowledgement_in_the_store() {
    use otw_store::broker_risk::Limits;
    let Some(state) = super::super::mock_tests::scratch_state().await else { return };
    let run = uuid::Uuid::new_v4();
    let modules = vec!["*".to_string(), "trading".to_string()];
    let account = otw_store::brokers::create(&state.pool, "bybit", &format!("e2d {run}"), &modules, &json!({})).await.unwrap();
    otw_store::brokers::set_cred(&state.pool, &state.cipher, account.id, "bybit", "api_key", &format!("mock-e2d-{run}")).await.unwrap();
    otw_store::brokers::set_cred(&state.pool, &state.cipher, account.id, "bybit", "api_secret", "s").await.unwrap();
    let gate = |account: otw_store::brokers::BrokerRow| {
        let (state, connector) = (state.clone(), broker_for("bybit").unwrap());
        async move {
            let mut s = otw_store::brokers::load_creds(&state.pool, &state.cipher, account.id).await.unwrap();
            let out = crate::brokers_api::require_live(&state, &account, connector.as_ref(), &mut s).await;
            (out.map_err(|e| e.code.unwrap_or("none")), s.get(LIVE_UNTESTED).cloned())
        }
    };
    let row = || async { otw_store::brokers::get(&state.pool, account.id).await.unwrap().unwrap() };
    assert_eq!(gate(row().await).await.0, Err("live_off"));
    otw_store::brokers::set_live(&state.pool, account.id, true).await.unwrap();
    assert_eq!(gate(row().await).await.0, Err("live_limits_missing"));
    let full = Limits { max_order_qty: Some(Decimal::ONE), max_position_qty: Some(Decimal::ONE), max_loss: Some(Decimal::ONE_HUNDRED), ..Default::default() };
    otw_store::broker_risk::set_limits(&state.pool, account.id, &full).await.unwrap();
    assert_eq!(gate(row().await).await.0, Err(super::super::live::UNTESTED_ACK_REQUIRED));
    otw_store::broker_exec::sign_untested(&state.pool, account.id, super::super::live::UNTESTED_STATEMENT).await.unwrap();
    assert_eq!(gate(row().await).await, (Ok(()), Some("1".to_string())), "acknowledged: the adapter is marked");
    // Turning live off drops the acknowledgement.
    otw_store::broker_exec::drop_untested(&state.pool, account.id).await.unwrap();
    assert!(otw_store::broker_exec::untested_ack(&state.pool, account.id).await.unwrap().is_none());
    otw_store::brokers::delete(&state.pool, account.id).await.unwrap();
}

/// The private stream through the mapper, against a mock socket answering the documented
/// frames: login and subscription answered first; a working order, a position that changed
/// size, an ADL execution and the order's end come through as the book's events; the socket
/// closing is followed by a reconnect and a `Resync`.
#[tokio::test]
async fn the_private_stream_follows_the_account_and_resyncs_on_reconnect() {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    const ORDER: &str = include_str!("../../../connectors/providers/src/bybit/fixtures/trade/doc_ws_order.json");
    const EXEC: &str = include_str!("../../../connectors/providers/src/bybit/fixtures/trade/doc_ws_execution.json");
    const POSITION: &str = include_str!("../../../connectors/providers/src/bybit/fixtures/trade/doc_ws_position.json");
    // `synthetic`: the documented frames with the category, symbol, client id, status and sizes
    // changed.
    let order = |status: &str| {
        let mut v: Value = serde_json::from_str(ORDER).unwrap();
        let o = &mut v["data"][0];
        o["category"] = json!("linear");
        o["symbol"] = json!("BTCUSDT");
        o["orderLinkId"] = json!("ws-k1");
        o["orderStatus"] = json!(status);
        v.to_string()
    };
    let position = |size: &str| {
        let mut v: Value = serde_json::from_str(POSITION).unwrap();
        v["data"][0]["positionIdx"] = json!(0);
        v["data"][0]["size"] = json!(size);
        v.to_string()
    };
    let adl = {
        let mut v: Value = serde_json::from_str(EXEC).unwrap();
        v["data"][0]["execType"] = json!("AdlTrade");
        v.to_string()
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let frames = vec![order("New"), position("0"), position("0.5"), adl, order("Filled")];
    tokio::spawn(async move {
        let mut n = 0;
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            // The REST read of the positions at connect lands here too: not a socket, dropped.
            let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else { continue };
            n += 1;
            let Some(Ok(Message::Text(auth))) = ws.next().await else { continue };
            assert!(auth.contains("\"op\":\"auth\"") && auth.contains("mock-ws-"), "{auth}");
            ws.send(Message::Text(r#"{"success":true,"ret_msg":"","op":"auth","conn_id":"c"}"#.into())).await.unwrap();
            let Some(Ok(Message::Text(sub))) = ws.next().await else { continue };
            assert!(sub.contains("order.linear") && sub.contains("execution.linear") && sub.contains("position.linear"), "{sub}");
            ws.send(Message::Text(r#"{"success":true,"ret_msg":"","op":"subscribe","conn_id":"c"}"#.into())).await.unwrap();
            if n == 1 {
                for f in &frames {
                    ws.send(Message::Text(f.clone().into())).await.unwrap();
                }
                let _ = ws.close(None).await;
            } else {
                // Held open until the test ends.
                while ws.next().await.is_some() {}
            }
        }
    });
    serve_from(&format!("http://{addr}"));
    let b = broker_for("bybit").unwrap();
    // The account reads the linear book: its stream is followed.
    let mut stream = b.events(&creds("ws", &[("books", "linear")])).await.unwrap().expect("a stream");
    let mut got = Vec::new();
    while got.len() < 5 {
        let ev = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next()).await.expect("an event in time").expect("the stream goes on");
        got.push(ev);
    }
    match &got[0] {
        super::super::AccountEvent::Order { order, key } => {
            assert_eq!((order.id.as_str(), key.as_deref(), order.qty), ("linear:BTCUSDT:5cf98598-39a7-459e-97bf-76ca765ee020", Some("ws-k1"), 1.0));
        }
        other => panic!("{other:?}"),
    }
    // The first position frame is recorded; the next, a different size, reads the book.
    assert!(matches!(got[1], super::super::AccountEvent::Resync), "{:?}", got[1]);
    // An ADL execution moved the position without an order: read the book.
    assert!(matches!(got[2], super::super::AccountEvent::Resync), "{:?}", got[2]);
    assert!(matches!(&got[3], super::super::AccountEvent::Done { status, .. } if status == "filled"), "{:?}", got[3]);
    // Closed by the venue: connected again, and the book is read again.
    assert!(matches!(got[4], super::super::AccountEvent::Resync), "{:?}", got[4]);
}

/// A key whose socket login the venue does not document (RSA) is followed by polling.
#[tokio::test]
async fn an_rsa_key_is_followed_by_polling() {
    let server = MockServer::start().await;
    serve_from(&server.uri());
    let pem = include_str!("../../../connectors/providers/src/bybit/fixtures/test_rsa_pkcs8.txt");
    let mut s = creds("rsa", &[("books", "linear")]);
    s.insert("api_secret".into(), pem.into());
    assert!(broker_for("bybit").unwrap().events(&s).await.unwrap().is_none());
}

/// The ticket's caps follow the instrument's book: spot caps on a spot pair, linear caps on a
/// contract; Binance answers its own caps whatever the instrument.
#[tokio::test]
async fn the_ticket_gets_the_caps_of_the_instruments_book() {
    let server = MockServer::start().await;
    venue(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let linear = b.order_caps_on(&creds("caps", &[("site", "testnet")]), "BTCUSDT", "crypto").await.unwrap().unwrap();
    assert!(linear.types.contains(&OrderType::Stop) && linear.reduce_only && linear.position_mode.is_some(), "{linear:?}");
    let spot = b.order_caps_on(&creds("caps", &[("site", "testnet"), (INSTRUMENT_BOOK, "spot")]), "BTCUSDT", "crypto").await.unwrap().unwrap();
    assert!(!spot.types.contains(&OrderType::Stop) && !spot.reduce_only && spot.position_mode.is_none(), "{spot:?}");
    let bn = broker_for("binance").unwrap();
    assert_eq!(bn.order_caps_on(&creds("caps", &[]), "BTCUSDT", "crypto").await.unwrap(), bn.order_caps());
}

/// The grid a ticket snaps to is the venue's own, through the shared mechanism: Bybit's tick,
/// size step and smallest size from its instrument list, spot and linear (instruments-info
/// examples). Spot states no smallest size (`minOrderQty` "no longer check"), and no venue
/// grid of its own for market orders.
#[tokio::test]
async fn the_grid_is_the_instruments_own_on_each_book() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).and(query_param("category", "spot")).respond_with(ResponseTemplate::new(200).set_body_string(SPOT)).mount(&server).await;
    venue(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let d = |v: &str| v.parse::<Decimal>().unwrap();
    let lin = creds("grid", &[("site", "testnet"), (INSTRUMENT_BOOK, "linear")]);
    let g = b.grid(&lin, None, "BTCUSDT", "crypto").await.unwrap().unwrap();
    assert_eq!((g.tick, g.lot_step, g.min_qty, g.market_step, g.market_min_qty), (Some(d("0.1")), Some(d("0.001")), Some(d("0.001")), None, None));
    assert_eq!(b.qty_step(&lin, "BTCUSDT").await.unwrap(), Some(d("0.001")));
    let spot = creds("grid", &[("site", "testnet"), (INSTRUMENT_BOOK, "spot")]);
    let g = b.grid(&spot, None, "BTCUSDT", "crypto").await.unwrap().unwrap();
    assert_eq!((g.tick, g.lot_step, g.min_qty), (Some(d("0.1")), Some(d("0.000001")), None));
    assert_eq!(b.qty_step(&spot, "BTCUSDT").await.unwrap(), Some(d("0.000001")));
}

/// Leverage like Binance USDⓈ-M: read only, the position list's `leverage` and the account's
/// margin mode (Get Account Info `marginMode`), held once read; a spot pair has none.
#[tokio::test]
async fn leverage_and_margin_mode_are_read_once_and_spot_has_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).and(query_param("category", "spot")).respond_with(ResponseTemplate::new(200).set_body_string(SPOT)).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/v5/market/instruments-info"))
        .and(query_param("category", "linear"))
        .respond_with(ResponseTemplate::new(200).set_body_string(LINEAR))
        .mount(&server)
        .await;
    // Position list example (doc), on the linear symbol.
    let mut pos: Value = serde_json::from_str(include_str!("../../../connectors/providers/src/bybit/fixtures/trade/doc_position_list.json")).unwrap();
    pos["result"]["list"][0]["symbol"] = json!("BTCUSDT");
    Mock::given(method("GET")).and(path("/v5/position/list")).respond_with(ResponseTemplate::new(200).set_body_json(pos)).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/account/info")).respond_with(ok(json!({ "marginMode": "REGULAR_MARGIN", "unifiedMarginStatus": 5 }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let lin = creds("lev", &[("site", "testnet"), (INSTRUMENT_BOOK, "linear")]);
    let first = b.leverage(&lin, "BTCUSDT").await.unwrap();
    assert_eq!(first, Some((Some(Decimal::from(10)), Some("cross".to_string()))));
    assert_eq!(b.leverage(&lin, "BTCUSDT").await.unwrap(), first);
    let reqs = server.received_requests().await.unwrap();
    let n = |p: &str| reqs.iter().filter(|r| r.url.path() == p).count();
    assert_eq!((n("/v5/position/list"), n("/v5/account/info")), (1, 1), "held once read");
    let spot = creds("lev", &[("site", "testnet"), (INSTRUMENT_BOOK, "spot")]);
    assert_eq!(b.leverage(&spot, "BTCUSDT").await.unwrap(), None);
}

/// A ticket's take profit and stop loss on a linear contract: one create call; the exits are
/// followed under ids that name their entry; changing or removing one goes through Set
/// Trading Stop; after a restart they are known again from their stored ids.
#[tokio::test]
async fn attached_exits_go_out_with_the_entry_and_change_through_trading_stop() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ok(json!({ "orderId": "o-e", "orderLinkId": "bx-e" }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/v5/position/trading-stop")).respond_with(ok(json!({}))).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "bx-e")).respond_with(ok(order_row("bx-e", "o-e", "New"))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let s = creds("bx", &[("site", "testnet")]);
    let mut t = limit("bx-e");
    let mut tp = OrderLeg::new("bx-tp", Side::Sell, OrderType::Limit, Decimal::new(1, 2));
    tp.limit = Some(Decimal::from(31_000));
    tp.tif = Some(TifKind::Gtc);
    tp.parent = Some("bx-e".into());
    let mut sl = OrderLeg::new("bx-sl", Side::Sell, OrderType::Stop, Decimal::new(1, 2));
    sl.stop = Some(Decimal::from(29_000));
    sl.tif = Some(TifKind::Gtc);
    sl.parent = Some("bx-e".into());
    t.legs.push(tp);
    t.legs.push(sl);
    let placed = b.place(&s, &t).await.unwrap();
    assert_eq!(placed[2].id, "linear:BTCUSDT:tpsl:bx-e:sl:buy");
    match b.lookup(&s, "BTCUSDT", "crypto", "bx-sl").await.unwrap() {
        Lookup::Found { id, status, .. } => assert_eq!((id.as_str(), status), ("linear:BTCUSDT:tpsl:bx-e:sl:buy", OrderStatus::New)),
        other => panic!("{other:?}"),
    }
    let change = otw_connector_model::OrderAmend { stop: Some(otw_connector_model::Price::new(Decimal::from(29_500))), ..Default::default() };
    b.modify(&s, &placed[2].id, &change).await.unwrap();
    b.cancel(&s, &placed[1].id).await.unwrap();
    let stops: Vec<String> = server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == "/v5/position/trading-stop").map(|r| String::from_utf8_lossy(&r.body).to_string()).collect();
    assert_eq!(stops.len(), 2);
    assert!(stops[0].contains("\"stopLoss\":\"29500\"") && stops[1].contains("\"takeProfit\":\"0\""), "{stops:?}");
    // A fresh process knows the exit again from its stored id.
    let other = creds("bx2", &[("site", "testnet")]);
    let leg = super::super::StoredLeg { key: "bx-tp2".into(), id: Some("linear:BTCUSDT:tpsl:bx-e:tp:buy".into()), ticker: "BTCUSDT".into(), asset_type: "crypto".into(), parent: Some("bx-e".into()), group: None, status: "working".into() };
    b.resume(&other, &[leg]).await.unwrap();
    assert!(matches!(b.lookup(&other, "BTCUSDT", "crypto", "bx-tp2").await.unwrap(), Lookup::Found { .. }));
}

/// A stop loss added from the positions panel on a position that has none: the id the panel
/// builds from the position (`BOOK:SYMBOL:tpsl::sl:buy`, no entry) goes through Set Trading Stop.
#[tokio::test]
async fn a_stop_loss_is_set_on_a_position_that_has_none() {
    let server = MockServer::start().await;
    venue(&server).await;
    Mock::given(method("POST")).and(path("/v5/position/trading-stop")).respond_with(ok(json!({}))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bybit").unwrap();
    let s = creds("px", &[("site", "testnet")]);
    let change = otw_connector_model::OrderAmend { stop: Some(otw_connector_model::Price::new(Decimal::from(28_500))), ..Default::default() };
    assert!(b.modify(&s, "linear:BTCUSDT:tpsl::sl:buy", &change).await.unwrap().is_none());
    let tp = otw_connector_model::OrderAmend { limit: Some(otw_connector_model::Price::new(Decimal::from(33_000))), ..Default::default() };
    b.modify(&s, "linear:BTCUSDT:tpsl::tp:buy", &tp).await.unwrap();
    let stops: Vec<String> = server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == "/v5/position/trading-stop").map(|r| String::from_utf8_lossy(&r.body).to_string()).collect();
    assert_eq!(stops.len(), 2);
    assert!(stops[0].contains("\"stopLoss\":\"28500\"") && stops[1].contains("\"takeProfit\":\"33000\""), "{stops:?}");
}

/// The private socket variants, through the follower and a fake venue (`synthetic` frames):
/// two sockets on one account, the first opened at the URL a signed bootstrap call hands out,
/// each subscribe frame signed and answered in turn, and frames read with the contract sizes.
#[tokio::test]
async fn socket_variants_bootstrap_sign_each_subscribe_and_read_contract_sizes() {
    use futures::{SinkExt, StreamExt};
    use otw_connector_engine::trading::Trader;
    use otw_connector_testkit::fake::{FakeSigner, FakeVenue, Knobs};
    use tokio_tungstenite::tungstenite::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = paths.clone();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut at = String::new();
                let cb = |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
                    at = req.uri().to_string();
                    Ok(resp)
                };
                let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await else { return };
                seen.lock().unwrap().push(at.clone());
                for _ in 0..2 {
                    let Some(Ok(Message::Text(sub))) = ws.next().await else { return };
                    assert!(sub.contains("\"sig\":\"fake\"") && sub.contains("subscribe"), "{sub}");
                    ws.send(Message::Text(r#"{"op":"subscribe","ok":true}"#.into())).await.unwrap();
                }
                let id = if at.starts_with("/dyn") { "A1" } else { "B1" };
                let frame = json!({ "order": { "id": id, "cid": "k", "symbol": "BTC", "side": "buy", "status": "new", "qty": "5", "filled": "0" } });
                ws.send(Message::Text(frame.to_string())).await.unwrap();
                while ws.next().await.is_some() {}
            });
        }
    });
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/rules")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0, "rows": [{ "symbol": "BTC", "contract": "0.01" }] }))).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/bullet"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0, "endpoint": format!("ws://{addr}/dyn"), "token": "t1", "ping_ms": 30000 })))
        .mount(&server)
        .await;
    let fake = FakeVenue::new(Knobs { contracts: true, sockets: vec!["/a", "/b"], bootstrap: true, ..Knobs::default() });
    let cfg = otw_connector_sdk::DriverConfig::new("fake").base("rest", &server.uri()).base("ws", &format!("http://{addr}"));
    let driver = std::sync::Arc::new(otw_connector_sdk::Driver::new(fake.clone(), cfg, reqwest::Client::new()));
    let trader = std::sync::Arc::new(Trader::new());
    let settings: HashMap<String, String> = HashMap::new();
    let mut streams = Vec::new();
    for (i, socket) in fake.private_sockets(&settings).into_iter().enumerate() {
        let f = super::super::native_trade_events::Follow {
            book: "fake",
            driver: driver.clone(),
            signer: std::sync::Arc::new(FakeSigner),
            trader: trader.clone(),
            scope: "fake-ws".into(),
            settings: settings.clone(),
            socket,
            venue: "Fake".into(),
            asset_class: "perp".into(),
            prefill: i == 0,
        };
        streams.push(super::super::native_trade_events::follow(f).await.unwrap());
    }
    let mut all = futures::stream::select_all(streams);
    let mut got = Vec::new();
    while got.len() < 2 {
        let ev = tokio::time::timeout(std::time::Duration::from_secs(10), all.next()).await.expect("an event in time").expect("the stream goes on");
        if let super::super::AccountEvent::Order { order, .. } = ev {
            got.push((order.id, order.qty));
        }
    }
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, vec![("fake:BTC:A1".to_string(), 0.05), ("fake:BTC:B1".to_string(), 0.05)]);
    let at = paths.lock().unwrap().clone();
    assert!(at.contains(&"/dyn?token=t1".to_string()) && at.contains(&"/b".to_string()), "{at:?}");
    let bullets = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/bullet").count();
    assert_eq!(bullets, 1, "only the first socket is bootstrapped");
}

/// A book named by another spelling the data connector's `market` takes (Bitget `futures`,
/// Gate.io `usdt-futures`) is the book it names; an unknown name is refused, never guessed.
#[test]
fn a_market_alias_names_its_book() {
    let bitget = super::super::native_trade_bitget::bitget().unwrap();
    assert_eq!(bitget.named("futures").map(|b| b.name), Some("usdt-futures"));
    assert_eq!(bitget.named("USDT-futures").map(|b| b.name), Some("usdt-futures"));
    assert!(bitget.named("coin-futures").is_none());
    let gate = super::super::native_trade_gateio::gateio().unwrap();
    assert_eq!(gate.named("usdt-futures").map(|b| b.name), Some("futures"));
    assert_eq!(gate.named("spot").map(|b| b.name), Some("spot"));
}

/// Every working order of the account is listed, plan orders included, each under an id a
/// cancel or change reaches: `BOOK:SYMBOL:cond:id` for a conditional order, `BOOK:SYMBOL:tpsl:...`
/// for a position's take profit or stop loss.
#[tokio::test]
async fn bitget_plan_orders_are_listed_under_ids_that_reach_them() {
    let server = MockServer::start().await;
    let bg = |data: Value| ResponseTemplate::new(200).set_body_json(json!({ "code": "00000", "msg": "success", "requestTime": 1, "data": data }));
    Mock::given(method("GET")).and(path("/api/v2/spot/trade/unfilled-orders")).respond_with(bg(json!([]))).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/spot/trade/current-plan-order"))
        .respond_with(bg(json!({ "nextFlag": false, "orderList": [{ "orderId": "p1", "symbol": "BTCUSDT", "size": "0.01", "triggerPrice": "29000", "orderType": "market", "side": "sell", "planType": "amount", "status": "live", "cTime": "1" }] })))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/api/v2/mix/order/orders-pending")).respond_with(bg(json!({ "entrustedList": null }))).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v2/mix/order/orders-plan-pending"))
        .and(query_param("planType", "profit_loss"))
        .respond_with(bg(json!({ "entrustedList": [{ "planType": "pos_loss", "symbol": "ETHUSDT", "size": "", "orderId": "t9", "triggerPrice": "1800", "planStatus": "live", "side": "sell", "posSide": "short", "marginCoin": "USDT", "orderType": "market", "cTime": "2" }] })))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/api/v2/mix/order/orders-plan-pending")).respond_with(bg(json!({ "entrustedList": null }))).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("bitget").unwrap();
    let s: HashMap<String, String> = [("api_key", "k"), ("api_secret", "s"), ("api_passphrase", "p")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let ids: Vec<String> = b.orders(&client().unwrap(), &s).await.unwrap().into_iter().map(|o| o.id).collect();
    assert_eq!(ids, vec!["spot:BTCUSDT:cond:p1".to_string(), "usdt-futures:ETHUSDT:tpsl::sl:sell".to_string()]);
}

/// A socket a venue opens with its login alone: nothing subscribed, no heartbeat sent (the
/// silence check still runs), a keepalive call on its timer; a binary frame read like text; a
/// frame that does not carry the whole order reads it again; a pushed mode is held.
#[tokio::test]
async fn a_bare_socket_reads_binary_and_partial_frames_and_keeps_alive() {
    use futures::{SinkExt, StreamExt};
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::trade::PositionMode;
    use otw_connector_testkit::fake::{FakeSigner, FakeVenue, Knobs};
    use tokio_tungstenite::tungstenite::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = heard.clone();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let seen = seen.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else { return };
                let order = json!({ "order": { "id": "B1", "cid": "k", "symbol": "BTC", "side": "buy", "status": "new", "qty": "5", "filled": "0" } });
                ws.send(Message::Binary(order.to_string().into_bytes().into())).await.unwrap();
                ws.send(Message::Text(json!({ "changed": { "id": "C1", "symbol": "BTC" } }).to_string().into())).await.unwrap();
                ws.send(Message::Text(json!({ "mode_push": "one_way", "symbol": "BTC" }).to_string().into())).await.unwrap();
                while let Some(Ok(m)) = ws.next().await {
                    if let Message::Text(t) = m {
                        seen.lock().unwrap().push(t.to_string());
                    }
                }
            });
        }
    });
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("id", "C1")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0, "rows": [{ "id": "C1", "cid": "c", "symbol": "BTC", "side": "sell", "status": "partial", "qty": "2", "filled": "1" }] }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/keep")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0 }))).mount(&server).await;
    let fake = FakeVenue::new(Knobs { sockets: vec!["/bare"], bare_socket: true, ..Knobs::default() });
    let cfg = otw_connector_sdk::DriverConfig::new("fake").base("rest", &server.uri()).base("ws", &format!("http://{addr}"));
    let driver = std::sync::Arc::new(otw_connector_sdk::Driver::new(fake.clone(), cfg, reqwest::Client::new()));
    let trader = std::sync::Arc::new(Trader::new());
    let settings: HashMap<String, String> = HashMap::new();
    let socket = fake.private_sockets(&settings).remove(0);
    let f = super::super::native_trade_events::Follow {
        book: "fake",
        driver: driver.clone(),
        signer: std::sync::Arc::new(FakeSigner),
        trader: trader.clone(),
        scope: "fake-bare".into(),
        settings: settings.clone(),
        socket,
        venue: "Fake".into(),
        asset_class: "perp".into(),
        prefill: false,
    };
    let mut events = super::super::native_trade_events::follow(f).await.unwrap();
    let mut got = Vec::new();
    while got.len() < 2 {
        let ev = tokio::time::timeout(std::time::Duration::from_secs(10), events.next()).await.expect("an event in time").expect("the stream goes on");
        if let super::super::AccountEvent::Order { order, key } = ev {
            got.push((order.id, key));
        }
    }
    assert_eq!(got, vec![("fake:BTC:B1".to_string(), Some("k".to_string())), ("fake:BTC:C1".to_string(), Some("c".to_string()))]);
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
    let a = Account { settings: &settings, signer: &FakeSigner, scope: "fake-bare" };
    assert_eq!(trader.held_mode(&a, "BTC"), Some(PositionMode::OneWay), "the pushed mode is held");
    assert!(heard.lock().unwrap().is_empty(), "nothing subscribed, no heartbeat sent: {:?}", heard.lock().unwrap());
    let keeps = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/keep").count();
    assert!(keeps >= 1, "the keepalive call runs on its timer");
}

/// Ids whose symbol holds colons (Bitfinex `tBTCF0:USTF0`, `tAVAX:USD`): the book before the
/// first colon, the venue id or side after the last one, a tagged id (`cond:`, `tpsl:`) whole.
#[test]
fn ids_split_on_the_first_and_last_colon_and_keep_tagged_ids_whole() {
    use super::split_book_id;
    assert_eq!(split_book_id("derivatives:tBTCF0:USTF0:123"), Some(("derivatives", "tBTCF0:USTF0", "123")));
    assert_eq!(split_book_id("spot:tAVAX:USD:9"), Some(("spot", "tAVAX:USD", "9")));
    assert_eq!(split_book_id("derivatives:tBTCF0:USTF0:long"), Some(("derivatives", "tBTCF0:USTF0", "long")));
    assert_eq!(split_book_id("linear:BTCUSDT:77"), Some(("linear", "BTCUSDT", "77")));
    assert_eq!(split_book_id("futures:BTC_USDT:cond:plan:7001"), Some(("futures", "BTC_USDT", "cond:plan:7001")));
    assert_eq!(split_book_id("derivatives:tBTCF0:USTF0:cond:5"), Some(("derivatives", "tBTCF0:USTF0", "cond:5")));
    assert_eq!(split_book_id("linear:BTCUSDT:tpsl:k1:tp:buy"), Some(("linear", "BTCUSDT", "tpsl:k1:tp:buy")));
    assert_eq!(split_book_id("77"), None);
    assert_eq!(split_book_id("cond:77"), None);
    assert_eq!(split_book_id("linear::77"), None);
}

/// Accounts are told apart by their key whatever the venue calls it: two Coinbase keys
/// (`key_name`) are two scopes, never one shared by every Coinbase account.
#[test]
fn the_lock_scope_follows_the_key_whatever_its_setting_is_called() {
    use super::{key_identity, lock_scope};
    let cb = |name: &str| HashMap::from([("key_name".to_string(), name.to_string()), ("api_private_key".to_string(), "secret".to_string())]);
    let (a, b) = (cb("organizations/o/apiKeys/a"), cb("organizations/o/apiKeys/b"));
    assert_ne!(lock_scope(key_identity(&a), false, "coinbase"), lock_scope(key_identity(&b), false, "coinbase"));
    assert_eq!(key_identity(&a), "organizations/o/apiKeys/a");
    let bybit = HashMap::from([("api_key".to_string(), " k1 ".to_string())]);
    assert_eq!(lock_scope(key_identity(&bybit), true, "bybit"), lock_scope("k1", true, "bybit"), "the api_key scope unchanged");
}

/// Numbered frames: a jump of more than one is a dropped frame; a lower or repeated number is
/// passed over; the first frame of a connection starts the count.
#[test]
fn a_jump_in_the_frame_numbers_is_a_gap_and_a_lower_number_is_not() {
    use super::super::native_trade_events::sequence_gap;
    let mut last = None;
    assert!(!sequence_gap(&mut last, 7), "the first frame starts the count");
    assert!(!sequence_gap(&mut last, 8));
    assert!(sequence_gap(&mut last, 10), "9 was dropped");
    assert!(!sequence_gap(&mut last, 9), "out of order: passed over");
    assert!(!sequence_gap(&mut last, 10), "repeated: passed over");
    assert_eq!(last, Some(10));
    assert!(!sequence_gap(&mut last, 11));
}

/// Through the follower and a fake venue (`synthetic` frames): announced maintenance is held by
/// the trader; a frame naming its order by client id only reads it again by that id; a jump in
/// the frame numbers reads the book (a lower number does not); a reconnect clears the
/// maintenance and reads the book.
#[tokio::test]
async fn the_stream_holds_maintenance_reads_by_client_id_and_resyncs_on_a_numbering_gap() {
    use futures::{SinkExt, StreamExt};
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_testkit::fake::{FakeSigner, FakeVenue, Knobs};
    use tokio_tungstenite::tungstenite::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut n = 0;
        loop {
            let Ok((tcp, _)) = listener.accept().await else { return };
            let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else { continue };
            n += 1;
            if n > 1 {
                tokio::spawn(async move { while ws.next().await.is_some() {} });
                continue;
            }
            let order = |id: &str, seq: u64| json!({ "seq": seq, "order": { "id": id, "cid": "k", "symbol": "BTC", "side": "buy", "status": "new", "qty": "5", "filled": "0" } });
            for f in [
                json!({ "seq": 1, "maintenance": "on" }),
                json!({ "seq": 2, "changed_cid": { "cid": "k7", "symbol": "BTC" } }),
                order("B5", 5),
                order("B3", 3),
            ] {
                ws.send(Message::Text(f.to_string().into())).await.unwrap();
            }
            // Held open until the test has read the frames, then closed: the stream reconnects.
            tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
            let _ = ws.close(None).await;
        }
    });
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/order"))
        .and(query_param("cid", "k7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 0, "rows": [{ "id": "C7", "cid": "k7", "symbol": "BTC", "side": "sell", "status": "partial", "qty": "2", "filled": "1" }] })))
        .mount(&server)
        .await;
    let fake = FakeVenue::new(Knobs { sockets: vec!["/bare"], bare_socket: true, ..Knobs::default() });
    let cfg = otw_connector_sdk::DriverConfig::new("fake").base("rest", &server.uri()).base("ws", &format!("http://{addr}"));
    let driver = std::sync::Arc::new(otw_connector_sdk::Driver::new(fake.clone(), cfg, reqwest::Client::new()));
    let trader = std::sync::Arc::new(Trader::new());
    let settings: HashMap<String, String> = HashMap::new();
    let socket = fake.private_sockets(&settings).remove(0);
    let f = super::super::native_trade_events::Follow {
        book: "fake",
        driver: driver.clone(),
        signer: std::sync::Arc::new(FakeSigner),
        trader: trader.clone(),
        scope: "fake-seq".into(),
        settings: settings.clone(),
        socket,
        venue: "Fake".into(),
        asset_class: "perp".into(),
        prefill: false,
    };
    let a = Account { settings: &settings, signer: &FakeSigner, scope: "fake-seq" };
    let mut events = super::super::native_trade_events::follow(f).await.unwrap();
    let mut got = Vec::new();
    while got.len() < 4 {
        let ev = tokio::time::timeout(std::time::Duration::from_secs(10), events.next()).await.expect("an event in time").expect("the stream goes on");
        got.push(match ev {
            super::super::AccountEvent::Order { order, key } => format!("{} {}", order.id, key.unwrap_or_default()),
            super::super::AccountEvent::Resync => "resync".to_string(),
            other => format!("{other:?}"),
        });
    }
    assert_eq!(got, vec!["fake:BTC:C7 k7", "resync", "fake:BTC:B5 k", "fake:BTC:B3 k"]);
    assert!(trader.maintenance(&a).is_some_and(|m| m.contains("fake maintenance on")), "held for the order path");
    // The socket closes: connected again, the book is read again, the maintenance cleared.
    let ev = tokio::time::timeout(std::time::Duration::from_secs(10), events.next()).await.expect("an event in time").expect("the stream goes on");
    assert!(matches!(ev, super::super::AccountEvent::Resync), "{ev:?}");
    assert!(trader.maintenance(&a).is_none(), "a reconnect clears it");
}
