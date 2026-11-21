//! KuCoin and KuCoin Futures through the generic trading broker, end to end on the `Broker`
//! trait against a local server that answers like the venue (`serve_from`): the caps each book
//! declares, the live gate (no sandbox: E2D on an acknowledged account only), the venue's order
//! test before the first order of the session, and the ids orders are followed by. The catalog
//! itself runs on the trader in the provider's tests.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;

use otw_connector_model::{Decimal, ExecLevel, OrderType, PositionModeCap, Side, TifKind};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::super::{broker_for, serve_from, OrderLeg, OrderTicket, LIVE_UNTESTED};

const CONTRACTS: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_fut_contracts_active.json");
const SYMBOLS: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_spot_symbols.json");
const MODE: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_fut_position_mode.json");
const MARGIN: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_fut_margin_mode.json");
const FUT_ADD: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_fut_add_order.json");
const FUT_TEST: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_fut_add_order_test.json");
const SPOT_ADD: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_spot_add_order.json");
const SPOT_TEST: &str = include_str!("../../../connectors/providers/src/kucoin/fixtures/trade/doc_spot_add_order_test.json");

fn creds(tag: &str, untested: bool) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = [("api_key", format!("mock-{tag}-{}", uuid::Uuid::new_v4().simple())), ("api_secret", "s".to_string()), ("api_passphrase", "p".to_string())]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    if untested {
        m.insert(LIVE_UNTESTED.into(), "1".into());
    }
    m
}

fn limit(ticker: &str, key: &str, qty: Decimal) -> OrderTicket {
    let mut leg = OrderLeg::new(key, Side::Buy, OrderType::Limit, qty);
    leg.limit = Some(Decimal::from(30_000));
    leg.tif = Some(TifKind::Gtc);
    OrderTicket { ticker: ticker.into(), asset_type: "crypto".into(), legs: vec![leg], provider: None, connector: None, reference: None, reference_at: None, confirm_duplicate: false }
}

fn ok(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(body.to_string())
}

#[test]
fn each_platform_takes_orders_at_e2d_with_its_own_caps() {
    let spot = broker_for("kucoin").unwrap();
    let caps = spot.order_caps().unwrap();
    assert_eq!(caps.level, ExecLevel::E2D);
    assert!(caps.types.contains(&OrderType::Limit) && !caps.types.contains(&OrderType::Stop) && !caps.reduce_only && caps.position_mode.is_none(), "{caps:?}");
    assert!(spot.has_dry_run());
    let fut = broker_for("kucoin_futures").unwrap();
    let caps = fut.order_caps().unwrap();
    assert_eq!(caps.level, ExecLevel::E2D);
    assert!(caps.types.contains(&OrderType::StopLimit) && caps.reduce_only && !caps.brackets && !caps.amend, "{caps:?}");
    assert_eq!(caps.position_mode, Some(PositionModeCap::Both));
    assert_eq!(super::super::live::level(fut.as_ref()), ExecLevel::E2D);
}

/// A futures order: refused live without the owner's acknowledgement (KuCoin has no sandbox);
/// with it, the venue's order test first, then the order with the account's position mode and
/// the symbol's margin as read, followed as `futures:SYMBOL:orderId`.
#[tokio::test]
async fn a_kucoin_futures_order_runs_the_venue_test_first_and_carries_what_was_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v1/contracts/active")).respond_with(ok(CONTRACTS)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v2/position/getPositionMode")).respond_with(ok(MODE)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v2/position/getMarginMode")).respond_with(ok(MARGIN)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/orders/test")).respond_with(ok(FUT_TEST)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/orders")).respond_with(ok(FUT_ADD)).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("kucoin_futures").unwrap();
    let e = b.place(&creds("kf", false), &limit("XBTUSDTM", "kf0", Decimal::new(1, 2))).await.unwrap_err().to_string();
    assert!(e.contains("never tested against the venue"), "{e}");
    let s = creds("kf", true);
    assert!(b.is_live(&s).await.unwrap(), "no sandbox: every account is live");
    let placed = b.place(&s, &limit("XBTUSDTM", "kf1", Decimal::new(1, 2))).await.unwrap();
    assert_eq!(placed[0].id, "futures:XBTUSDTM:234125150956625920");
    b.place(&s, &limit("XBTUSDTM", "kf2", Decimal::new(1, 2))).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.iter().filter(|r| r.url.path() == "/api/v1/orders/test").count(), 1, "checked once per session");
    let sent: Vec<String> = reqs.iter().filter(|r| r.url.path() == "/api/v1/orders").map(|r| String::from_utf8_lossy(&r.body).to_string()).collect();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].contains("\"positionSide\":\"LONG\"") && sent[0].contains("\"marginMode\":\"ISOLATED\"") && sent[0].contains("\"qty\":\"0.01\""), "{}", sent[0]);
    // A size that is not whole contracts is refused before anything leaves.
    let e = b.place(&s, &limit("XBTUSDTM", "kf3", Decimal::new(15, 4))).await.unwrap_err().to_string();
    assert!(e.contains("0.001 or 0.002"), "{e}");
}

#[tokio::test]
async fn a_kucoin_spot_order_goes_to_its_spot_book() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v2/symbols")).respond_with(ok(SYMBOLS)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/hf/orders/test")).respond_with(ok(SPOT_TEST)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/hf/orders")).respond_with(ok(SPOT_ADD)).mount(&server).await;
    serve_from(&server.uri());
    let b = broker_for("kucoin").unwrap();
    let s = creds("ks", true);
    let placed = b.place(&s, &limit("BTC-USDT", "ks1", Decimal::new(1, 3))).await.unwrap();
    assert_eq!(placed[0].id, "spot:BTC-USDT:670fd33bf9406e0007ab3945");
    let e = b.close_position(&s, "BTC-USDT", "", "c").await.unwrap_err().to_string();
    assert!(e.contains("not positions"), "{e}");
}
