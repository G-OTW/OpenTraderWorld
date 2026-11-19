//! OKX orders, from the documentation alone. Fixtures under `fixtures/trade/`, copied verbatim
//! from the single-page v5 doc (https://www.okx.com/docs-v5/en/, snapshot read 2026-11-11,
//! last-modified Wed, 11 Nov 2026 13:00:39 GMT):
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_place_order.json` | #order-book-trading-trade-post-place-order (Response Example) |
//! | `doc_cancel_order.json` | #order-book-trading-trade-post-cancel-order (Response Example) |
//! | `doc_cancel_batch.json` | #order-book-trading-trade-post-cancel-multiple-orders (Response Example) |
//! | `doc_order_details.json` | #order-book-trading-trade-get-order-details (Response Example) |
//! | `doc_orders_pending.json` | #order-book-trading-trade-get-order-list (Response Example) |
//! | `doc_fills_history.json` | #order-book-trading-trade-get-transaction-details-last-3-months (Response Example) |
//! | `doc_instruments_spot.json` | #public-data-rest-api-get-instruments (Response Example) |
//! | `doc_account_config.json` | #trading-account-rest-api-get-account-configuration (Response Example) |
//! | `doc_positions.json` | #trading-account-rest-api-get-positions (Response Example) |
//! | `doc_ws_orders.json` | #order-book-trading-trade-ws-order-channel (Push Data Example) |
//! | `doc_ws_positions.json` | #trading-account-websocket-positions-channel (Push Data Example) |
//! | `doc_ws_login.json`, `doc_ws_login_error.json` | #overview-websocket-login (Successful and Failure Response Example) |
//! | `doc_ws_subscribe.json` | #order-book-trading-trade-ws-order-channel (Successful Response Example) |
//! | `synthetic_instruments_swap.json` | **synthetic**: the instruments example with the SWAP fields the response parameters describe (`ctVal`, `ctType`, contract `lotSz`), one linear and one inverse row: the doc's only example is a spot row |
//!
//! The catalog script ([`Script`]) answers with these examples with only their ids, sizes,
//! prices, symbol and state changed: those answers are `synthetic`, built on the documented
//! shape. Every error body is `synthetic` too (the doc lists codes, not bodies; the envelope and
//! the per-item `sCode` are the documented ones), and so is the `Retry-After` header of the rate
//! limit answer (OKX documents no wait).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, CancelRow, Canon, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, MarginMode, Page, Placing, PositionMode, PositionSide, PrivateEvent,
    PrivateSocket, Refusal, Settings, TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, SignableRequest, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript};
use serde_json::{json, Value};

use super::super::signer::OkxSigner;
use super::super::{Okx, Session};

const PLACE: &str = include_str!("fixtures/trade/doc_place_order.json");
const CANCEL: &str = include_str!("fixtures/trade/doc_cancel_order.json");
const CANCEL_BATCH: &str = include_str!("fixtures/trade/doc_cancel_batch.json");
const DETAILS: &str = include_str!("fixtures/trade/doc_order_details.json");
const PENDING: &str = include_str!("fixtures/trade/doc_orders_pending.json");
const FILLS: &str = include_str!("fixtures/trade/doc_fills_history.json");
const SPOT_LIST: &str = include_str!("fixtures/trade/doc_instruments_spot.json");
const SWAP_LIST: &str = include_str!("fixtures/trade/synthetic_instruments_swap.json");
const CONFIG: &str = include_str!("fixtures/trade/doc_account_config.json");
const POSITIONS: &str = include_str!("fixtures/trade/doc_positions.json");
const WS_ORDERS: &str = include_str!("fixtures/trade/doc_ws_orders.json");
const WS_POSITIONS: &str = include_str!("fixtures/trade/doc_ws_positions.json");
const WS_LOGIN: &str = include_str!("fixtures/trade/doc_ws_login.json");
const WS_LOGIN_ERROR: &str = include_str!("fixtures/trade/doc_ws_login_error.json");
const WS_SUBSCRIBE: &str = include_str!("fixtures/trade/doc_ws_subscribe.json");

/// The doc's example secret key (overview, REST Authentication, Signature).
const DOC_SECRET: &str = "22582BD0CFF14C41EDBF1AB98506286D";
/// The swap list's contract size of BTC-USDT-SWAP.
const CT: &str = "0.01";

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "k"), ("api_secret", DOC_SECRET), ("api_passphrase", "p")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn with_setting(k: &str, v: &str) -> HashMap<String, String> {
    let mut s = settings();
    s.insert(k.into(), v.into());
    s
}

fn signer(s: &HashMap<String, String>) -> OkxSigner {
    OkxSigner {
        key: s["api_key"].clone().into(),
        secret: s["api_secret"].clone().into(),
        passphrase: s["api_passphrase"].clone().into(),
        demo: s.get("env").is_some_and(|e| e == "demo"),
    }
}

fn order(symbol: &str, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>) -> OrderRequest {
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("OKX", symbol),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: limit.map(|p| Price::new(d(p))),
        stop: None,
        tif: TimeInForce::Gtc,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts::default(),
    }
}

fn body(req: &HttpRequest) -> Value {
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

fn placing(cid: &str, position: PositionSide) -> Placing {
    Placing::new(cid, position, CorrId(1))
}

fn contract(cid: &str, position: PositionSide) -> Placing {
    Placing { contract_size: Some(d(CT)), ..placing(cid, position) }
}

fn sizes() -> ContractSizes {
    let mut s = ContractSizes::new();
    s.insert("BTC-USDT-SWAP", d(CT));
    s.insert("BTC-USD-SWAP", d("100"));
    s.insert("ETH-USD-210430", d("10"));
    s
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

fn first_row(template: &str) -> Value {
    let v: Value = serde_json::from_str(template).unwrap();
    v["data"][0].clone()
}

// ── Requests ──

/// Place order, "place order for SPOT": the documented body, field for field, and its
/// signature as an independent HMAC-SHA256 (Python's `hmac`, computed once and pinned here)
/// gives it over the exact bytes sent. The doc gives the formula and an example secret, no
/// signed result.
#[test]
fn a_spot_limit_is_the_documented_body_and_signs_like_an_independent_implementation() {
    let doc: Value = serde_json::from_str(r#"{"instId":"BTC-USDT","tdMode":"cash","clOrdId":"b15","side":"buy","ordType":"limit","px":"2.15","sz":"2"}"#).unwrap();
    let m = Okx::spot().unwrap();
    let req = m.place(&settings(), &order("BTC-USDT", "b15", Side::Buy, OrderType::Limit, "2", Some("2.15")), &placing("b15", PositionSide::Net)).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.base), (Method::Post, "/api/v5/trade/order", true, "rest"));
    assert_eq!(body(&req), doc);
    let sent = String::from_utf8(req.body.clone().unwrap().to_vec()).unwrap();
    assert_eq!(sent, r#"{"clOrdId":"b15","instId":"BTC-USDT","ordType":"limit","px":"2.15","side":"buy","sz":"2","tdMode":"cash"}"#);
    let mut s = SignableRequest { method: Method::Post, path: req.path.clone(), query: vec![], headers: vec![], body: req.body.clone() };
    signer(&settings()).sign(&mut s, otw_connector_model::Timestamp::from_millis(1_607_418_537_715).unwrap()).unwrap();
    let sign = s.headers.iter().find(|(k, _)| k == "OK-ACCESS-SIGN").map(|(_, v)| v.clone());
    assert_eq!(sign.as_deref(), Some("l5bO21yttpjgzHqkCpPCmZ6l0vej0ZB52Oaqrs7jNbs="));
}

/// "Invalid Content-Type" (`50006`): the header goes out once, the signer's, never also the
/// request's (headers are appended, not replaced).
#[test]
fn a_signed_post_carries_one_content_type() {
    let m = Okx::spot().unwrap();
    let req = m.place(&settings(), &order("BTC-USDT", "c1", Side::Buy, OrderType::Limit, "1", Some("10")), &placing("c1", PositionSide::Net)).unwrap();
    let mut s = SignableRequest { method: req.method, path: req.path.clone(), query: req.query.clone(), headers: req.headers.clone(), body: req.body.clone() };
    signer(&settings()).sign(&mut s, otw_connector_model::Timestamp::from_millis(1_607_418_537_715).unwrap()).unwrap();
    let ct: Vec<&String> = s.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v).collect();
    assert_eq!(ct, [&"application/json".to_string()]);
    for r in [m.cancel(&settings(), "BTC-USDT", "1", CorrId(1)).unwrap(), m.cancel(&settings(), "BTC-USDT", "cond:1", CorrId(1)).unwrap()] {
        assert!(r.headers.iter().all(|(k, _)| !k.eq_ignore_ascii_case("content-type")));
    }
}

/// The socket login: timestamp in seconds, the HMAC of `timestamp + GET + /users/self/verify`
/// (an independent HMAC-SHA256, pinned; the doc's example sign is over a secret it does not
/// give).
#[test]
fn the_socket_login_signs_like_an_independent_implementation() {
    let frame = signer(&settings()).ws_login(otw_connector_model::Timestamp::from_millis(1_538_054_050_000).unwrap()).unwrap().unwrap();
    let v: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(v["op"], "login");
    let a = &v["args"][0];
    assert_eq!((a["apiKey"].as_str(), a["passphrase"].as_str(), a["timestamp"].as_str()), (Some("k"), Some("p"), Some("1538054050")));
    assert_eq!(a["sign"].as_str(), Some("+LdIr8lkkvhr5hoA3g9TMC0+uQJ849ftAcocA/ouu4M="));
}

#[test]
fn every_time_in_force_is_an_order_type_and_a_market_fok_is_refused_before_io() {
    let m = Okx::spot().unwrap();
    let kind = |tif: TimeInForce, post: bool| {
        let mut o = order("BTC-USDT", "t", Side::Buy, OrderType::Limit, "1", Some("10"));
        o.tif = tif;
        o.opts.post_only = post;
        body(&m.place(&settings(), &o, &placing("t", PositionSide::Net)).unwrap())["ordType"].as_str().unwrap().to_string()
    };
    assert_eq!(kind(TimeInForce::Gtc, false), "limit");
    assert_eq!(kind(TimeInForce::Ioc, false), "ioc");
    assert_eq!(kind(TimeInForce::Fok, false), "fok");
    assert_eq!(kind(TimeInForce::Gtc, true), "post_only");
    let mut mkt = order("BTC-USDT", "m", Side::Buy, OrderType::Market, "1", None);
    mkt.tif = TimeInForce::Fok;
    assert_eq!(m.check_order(&mkt).unwrap_err().kind, otw_connector_model::ErrorKind::Unsupported);
    for tif in [TimeInForce::Gtc, TimeInForce::Ioc] {
        mkt.tif = tif;
        assert!(m.check_order(&mkt).is_ok());
        assert_eq!(body(&m.place(&settings(), &mkt, &placing("m", PositionSide::Net)).unwrap())["ordType"], "market");
    }
    let mut day = order("BTC-USDT", "t", Side::Buy, OrderType::Limit, "1", Some("10"));
    day.tif = TimeInForce::Day;
    assert!(m.place(&settings(), &day, &placing("t", PositionSide::Net)).is_err());
}

#[test]
fn a_spot_market_order_is_sized_in_the_base_coin_and_never_shrunk_and_spot_takes_no_reduce_only() {
    let m = Okx::spot().unwrap();
    for side in [Side::Buy, Side::Sell] {
        let b = body(&m.place(&settings(), &order("BTC-USDT", "m1", side, OrderType::Market, "0.01", None), &placing("m1", PositionSide::Net)).unwrap());
        assert_eq!((b["tgtCcy"].as_str(), b["banAmend"].as_bool(), b["sz"].as_str(), b["tdMode"].as_str()), (Some("base_ccy"), Some(true), Some("0.01"), Some("cash")));
        assert!(b.get("px").is_none() && b.get("posSide").is_none() && b.get("reduceOnly").is_none());
        // No slippage cap is invented: the doc states neither its reference nor its outcome.
        assert!(b.get("slippagePct").is_none());
    }
    let mut ro = order("BTC-USDT", "r", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    ro.opts.reduce_only = true;
    assert!(m.place(&settings(), &ro, &placing("r", PositionSide::Net)).is_err());
    let caps = &m.capabilities().orders;
    assert!(caps.position_mode.is_none() && !caps.reduce_only && !caps.brackets && caps.amend && caps.post_only);
    assert_eq!(caps.level, otw_connector_model::ExecLevel::E2D);
}

#[test]
fn a_contract_order_counts_contracts_and_names_its_margin_and_position() {
    let m = Okx::swap().unwrap();
    let o = order("BTC-USDT-SWAP", "c1", Side::Buy, OrderType::Limit, "0.05", Some("30000"));
    let b = body(&m.place(&settings(), &o, &contract("c1", PositionSide::Net)).unwrap());
    assert_eq!((b["sz"].as_str(), b["tdMode"].as_str(), b.get("posSide"), b.get("reduceOnly")), (Some("5"), Some("cross"), None, None));
    // The held position's margin.
    let iso = Placing { margin: Some(MarginMode::isolated()), ..contract("c1", PositionSide::Net) };
    assert_eq!(body(&m.place(&settings(), &o, &iso).unwrap())["tdMode"], "isolated");
    // Net mode, reduce-only: the flag. Long/short mode: the side, and no flag.
    let mut ro = order("BTC-USDT-SWAP", "c2", Side::Sell, OrderType::Market, "0.02", None);
    ro.opts.reduce_only = true;
    let b = body(&m.place(&settings(), &ro, &contract("c2", PositionSide::Net)).unwrap());
    assert_eq!((b["reduceOnly"].as_bool(), b.get("posSide"), b["sz"].as_str(), b.get("tgtCcy")), (Some(true), None, Some("2"), None));
    let b = body(&m.place(&settings(), &ro, &contract("c2", PositionSide::Long)).unwrap());
    assert_eq!((b["posSide"].as_str(), b.get("reduceOnly")), (Some("long"), None));
    let b = body(&m.place(&settings(), &order("BTC-USDT-SWAP", "c3", Side::Sell, OrderType::Limit, "0.01", Some("31000")), &contract("c3", PositionSide::Short)).unwrap());
    assert_eq!(b["posSide"], "short");
    // Without the contract size nothing is built; a size off the contract grid is refused.
    assert!(m.place(&settings(), &o, &placing("c1", PositionSide::Net)).is_err());
    assert!(m.place(&settings(), &order("BTC-USDT-SWAP", "c4", Side::Buy, OrderType::Limit, "0.00005", Some("1")), &contract("c4", PositionSide::Net)).is_ok());
    let odd = Placing { contract_size: Some(d("0.03")), ..placing("c5", PositionSide::Net) };
    assert!(m.place(&settings(), &order("BTC-USDT-SWAP", "c5", Side::Buy, OrderType::Limit, "0.01", Some("1")), &odd).is_err());
    let caps = &m.capabilities().orders;
    assert!(caps.reduce_only && caps.position_mode.is_some() && caps.amend && !caps.brackets && !caps.types.contains(&OrderType::Stop));
    assert!(m.counts_contracts() && Okx::futures().unwrap().counts_contracts() && !Okx::spot().unwrap().counts_contracts());
}

#[test]
fn the_read_instance_takes_no_order() {
    let m = Okx::new().unwrap();
    assert!(m.place(&settings(), &order("BTC-USDT", "x", Side::Buy, OrderType::Limit, "1", Some("1")), &placing("x", PositionSide::Net)).is_err());
    assert!(m.cancel(&settings(), "BTC-USDT", "1", CorrId(1)).is_err());
    assert!(m.private_socket(&settings()).is_none());
}

#[test]
fn the_region_picks_the_host_and_demo_is_the_sandbox() {
    let m = Okx::swap().unwrap();
    let o = order("BTC-USDT-SWAP", "r", Side::Buy, OrderType::Limit, "0.01", Some("1"));
    for (region, rest, ws, ws_demo) in [("", "rest", "ws", "ws-demo"), ("global", "rest", "ws", "ws-demo"), ("eea", "rest-eea", "ws-eea", "ws-eea-demo"), ("us", "rest-us", "ws-us", "ws-us-demo")] {
        let s = with_setting("region", region);
        assert_eq!(m.place(&s, &o, &contract("r", PositionSide::Net)).unwrap().base, rest, "{region}");
        assert_eq!(m.instrument_rules(&s, None, CorrId(1)).unwrap().base, rest);
        assert_eq!(m.private_socket(&s).unwrap().base, ws);
        let mut demo = s.clone();
        demo.insert("env".into(), "demo".into());
        assert!(m.is_sandbox(&demo) && !m.is_sandbox(&s));
        assert_eq!(m.private_socket(&demo).unwrap().base, ws_demo);
    }
    let bad = with_setting("region", "mars");
    assert!(m.place(&bad, &o, &contract("r", PositionSide::Net)).unwrap_err().message.contains("eea"));
    assert!(m.check_settings(&bad).is_err() && m.check_settings(&with_setting("region", "eea")).is_ok());
    for (k, v) in m.base_urls() {
        assert!(!v.contains("8443") && !v.contains("www.okx.com"), "{k} {v}");
    }
}

#[test]
fn cancel_batch_amend_and_lookup_are_the_documented_calls() {
    let m = Okx::swap().unwrap();
    let c = m.cancel(&settings(), "BTC-USDT-SWAP", "590908157585625111", CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/api/v5/trade/cancel-order", json!({"instId":"BTC-USDT-SWAP","ordId":"590908157585625111"})));
    // An algo order the account read listed goes to Cancel algo order.
    let c = m.cancel(&settings(), "BTC-USDT-SWAP", "cond:590919993110396111", CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/api/v5/trade/cancel-algos", json!([{"instId":"BTC-USDT-SWAP","algoId":"590919993110396111"}])));
    assert!(m.query_by_venue_id(&settings(), "BTC-USDT-SWAP", "cond:590919993110396111", CorrId(1)).unwrap().is_empty());
    assert_eq!(m.cancel_all_way(), CancelAllWay::ByIds { max: 20 });
    let swap_row = with(PENDING, |v| {
        v["data"][0]["instType"] = json!("SWAP");
        v["data"][0]["instId"] = json!("BTC-USDT-SWAP");
    });
    let row = |id: &str| VenueOrder { venue_id: id.into(), ..m.decode_orders_in(&DecodeCtx::new("", &sizes()), swap_row.as_bytes()).unwrap().0[0].clone() };
    let b = m.cancel_ids(&settings(), &[row("1"), row("2")], CorrId(1)).unwrap();
    assert_eq!((b.path.as_str(), body(&b)), ("/api/v5/trade/cancel-batch-orders", json!([{"instId":"BTC-USDT-SWAP","ordId":"1"},{"instId":"BTC-USDT-SWAP","ordId":"2"}])));
    let many: Vec<VenueOrder> = (0..21).map(|i| row(&i.to_string())).collect();
    assert!(m.cancel_ids(&settings(), &many, CorrId(1)).is_err());
    let q = &m.query_by_client_id(&settings(), "BTC-USDT-SWAP", "k1", CorrId(1)).unwrap()[0];
    assert_eq!((q.path.as_str(), q.query.clone()), ("/api/v5/trade/order", vec![("instId".into(), "BTC-USDT-SWAP".into()), ("clOrdId".into(), "k1".into())]));
    let open = &m.open_orders(&settings(), None, CorrId(1)).unwrap()[0];
    assert_eq!(open.query, vec![("instType".to_string(), "SWAP".to_string()), ("limit".into(), "100".into())]);
    let next = m.with_cursor(open, "77");
    assert!(next.query.contains(&("after".into(), "77".into())));
    let f = m.fills_since(&settings(), "BTC-USDT-SWAP", 1, 2, CorrId(1)).unwrap();
    assert_eq!(f.path, "/api/v5/trade/fills-history");
    assert!(f.query.contains(&("begin".into(), "1".into())) && f.query.contains(&("end".into(), "2".into())) && f.query.contains(&("instType".into(), "SWAP".into())));
}

#[test]
fn an_amend_changes_price_or_size_never_below_what_filled() {
    let spot = Okx::spot().unwrap();
    let (rows, _) = spot.decode_orders(PENDING.as_bytes()).unwrap();
    let cur = &rows[0];
    let pl = || Placing::new("", PositionSide::Net, CorrId(1));
    let px = spot.amend(&settings(), cur, &OrderAmend { limit: Some(Price::new(d("13000"))), ..Default::default() }, &pl()).unwrap();
    assert_eq!((px.path.as_str(), body(&px)), ("/api/v5/trade/amend-order", json!({"instId":"BTC-USDT","ordId":"1752588852617379840","newPx":"13000"})));
    let sz = spot.amend(&settings(), cur, &OrderAmend { qty: Some(Qty::new(d("0.002"))), ..Default::default() }, &pl()).unwrap();
    assert_eq!(body(&sz)["newSz"], "0.002");
    let part = VenueOrder { filled_qty: d("0.0005"), ..cur.clone() };
    assert!(spot.amend(&settings(), &part, &OrderAmend { qty: Some(Qty::new(d("0.0005"))), ..Default::default() }, &pl()).is_err());
    assert!(spot.amend(&settings(), cur, &OrderAmend { tif: Some(TimeInForce::Ioc), ..Default::default() }, &pl()).is_err());
    // A contract's new size is sent in contracts of its size; without the size nothing is sent.
    let swap = Okx::swap().unwrap();
    let c = VenueOrder { symbol: "BTC-USDT-SWAP".into(), filled_qty: Decimal::ZERO, ..cur.clone() };
    let ct = Placing { contract_size: Some(d("0.01")), ..pl() };
    let sz = swap.amend(&settings(), &c, &OrderAmend { qty: Some(Qty::new(d("0.02"))), ..Default::default() }, &ct).unwrap();
    assert_eq!(body(&sz)["newSz"], "2");
    assert!(swap.amend(&settings(), &c, &OrderAmend { qty: Some(Qty::new(d("0.02"))), ..Default::default() }, &pl()).is_err());
    assert!(swap.amend(&settings(), &c, &OrderAmend { limit: Some(Price::new(d("13000"))), ..Default::default() }, &pl()).is_ok());
}

#[test]
fn the_client_id_is_letters_and_digits_up_to_32() {
    let m = Okx::swap().unwrap();
    assert_eq!(m.client_id("o1kz3abc").unwrap(), "o1kz3abc");
    assert!(m.client_id("with-dash").is_err() && m.client_id("under_score").is_err() && m.client_id(&"a".repeat(33)).is_err());
    assert!(m.client_id(&"a".repeat(32)).is_ok());
}

// ── Answers ──

#[test]
fn documented_answers_decode() {
    let m = Okx::spot().unwrap();
    let ack = m.decode_ack(PLACE.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("312269865356374016", Some("oktswap6")));
    let none = with(PLACE, |v| v["data"][0]["ordId"] = json!(""));
    assert_eq!(m.decode_ack(none.as_bytes()).unwrap(), Ack::without_id(Some("oktswap6".into())));
    for ok in [PLACE, CANCEL, CANCEL_BATCH, DETAILS] {
        assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "order", 200, ok)).is_none());
    }
    // A spot market buy sized in the quote coin (placed elsewhere): its base size is what filled.
    let (o, _) = m.decode_orders(DETAILS.as_bytes()).unwrap();
    assert_eq!((o[0].status.clone(), o[0].qty, o[0].filled_qty, o[0].avg_price, o[0].limit), (Mapped::Known(OrderStatus::Filled), d("0.00192834"), d("0.00192834"), Some(d("51858")), None));
    let (o, next) = m.decode_orders(PENDING.as_bytes()).unwrap();
    assert_eq!((o[0].status.clone(), o[0].qty, o[0].limit, o[0].order_type.as_str(), o[0].position, next), (Mapped::Known(OrderStatus::New), d("0.001"), Some(d("13013.5")), "post_only", PositionSide::Net, None));
    let (f, _) = m.decode_fills(FILLS.as_bytes()).unwrap();
    // OKX signs a charge negative: a cost is positive here.
    assert_eq!((f[0].exec_id.as_str(), f[0].qty, f[0].price, f[0].fee, f[0].fee_currency.as_str(), f[0].at_ms, f[0].position_only), ("744876980", d("0.00192834"), d("51858"), d("0.00000192834"), "BTC", 1_708_587_373_361, false));
    let cfg = Okx::swap().unwrap().decode_account_position_mode(CONFIG.as_bytes()).unwrap();
    assert_eq!(cfg, Some(PositionMode::Hedge));
    let net = with(CONFIG, |v| v["data"][0]["posMode"] = json!("net_mode"));
    assert_eq!(Okx::swap().unwrap().decode_account_position_mode(net.as_bytes()).unwrap(), Some(PositionMode::OneWay));
}

#[test]
fn a_full_page_hands_back_the_last_id() {
    let m = Okx::spot().unwrap();
    let full = with(PENDING, |v| {
        let r = v["data"][0].clone();
        v["data"] = json!((0..100).map(|i| { let mut x = r.clone(); x["ordId"] = json!(i.to_string()); x }).collect::<Vec<_>>());
    });
    assert_eq!(m.decode_orders(full.as_bytes()).unwrap().1.as_deref(), Some("99"));
    let fills = with(FILLS, |v| {
        let r = v["data"][0].clone();
        v["data"] = json!((0..100).map(|i| { let mut x = r.clone(); x["billId"] = json!(format!("b{i}")); x["tradeId"] = json!(i.to_string()); x }).collect::<Vec<_>>());
    });
    assert_eq!(m.decode_fills(fills.as_bytes()).unwrap().1.as_deref(), Some("b99"));
}

#[test]
fn fills_that_move_a_position_without_an_order_are_kept_and_marked() {
    let m = Okx::spot().unwrap();
    for (sub, trade) in [("104", "1"), ("125", "1"), ("112", "1"), ("1", "-5"), ("999", "1")] {
        let b = with(FILLS, |v| {
            v["data"][0]["subType"] = json!(sub);
            v["data"][0]["tradeId"] = json!(trade);
        });
        let (f, _) = m.decode_fills(b.as_bytes()).unwrap();
        assert!(f.len() == 1 && f[0].position_only, "{sub} {trade}");
    }
}

#[test]
fn instrument_lists_decode_to_sound_rules_in_the_base_coin() {
    let (spot, _) = Okx::spot().unwrap().decode_rules(SPOT_LIST.as_bytes()).unwrap();
    let r = &spot[0];
    assert_eq!((r.tick, r.lot_step, r.min_qty, r.max_qty, r.market_max_qty, r.min_notional, r.contract_size), (Some(d("0.1")), Some(d("0.00000001")), Some(d("0.00001")), Some(d("9999999999")), None, None, None));
    assert_eq!(r.status, TradingStatus::Trading);
    r.sound().unwrap();
    let (swap, _) = Okx::swap().unwrap().decode_rules(SWAP_LIST.as_bytes()).unwrap();
    let lin = &swap[0];
    assert_eq!((lin.contract_size, lin.lot_step, lin.min_qty, lin.max_qty, lin.market_max_qty), (Some(d("0.01")), Some(d("0.0001")), Some(d("0.0001")), Some(d("1000")), Some(d("120"))));
    assert_eq!(lin.status, TradingStatus::Trading);
    lin.sound().unwrap();
    // Inverse: sized in USD, listed (its rows are read), never sent an order.
    let inv = &swap[1];
    assert!(matches!(&inv.status, TradingStatus::Closed(w) if w.contains("inverse")), "{:?}", inv.status);
    assert_eq!(inv.contract_size, Some(d("100")));
    // A spot row on the swap book is not the swap book's.
    assert!(Okx::swap().unwrap().decode_rules(SPOT_LIST.as_bytes()).unwrap().0.is_empty());
    for (state, trading) in [("live", true), ("suspend", false), ("preopen", false), ("post_only", false), ("rebase", false), ("test", false)] {
        let b = with(SPOT_LIST, |v| v["data"][0]["state"] = json!(state));
        let r = &Okx::spot().unwrap().decode_rules(b.as_bytes()).unwrap().0[0];
        assert_eq!(r.status == TradingStatus::Trading, trading, "{state}");
    }
}

#[test]
fn positions_read_in_the_base_coin_with_their_margin_and_side() {
    let m = Okx::swap().unwrap();
    // The documented row is a MARGIN position: not this book's.
    assert!(m.decode_positions_in(&DecodeCtx::new("", &sizes()), POSITIONS.as_bytes()).unwrap().is_empty());
    let row = |pos: &str, side: &str, mgn: &str| {
        with(POSITIONS, |v| {
            let r = &mut v["data"][0];
            r["instType"] = json!("SWAP");
            r["instId"] = json!("BTC-USDT-SWAP");
            r["pos"] = json!(pos);
            r["posSide"] = json!(side);
            r["mgnMode"] = json!(mgn);
        })
    };
    let p = &m.decode_positions_in(&DecodeCtx::new("", &sizes()), row("-3", "net", "isolated").as_bytes()).unwrap()[0];
    assert_eq!((p.side, p.direction, p.qty, p.leverage, p.margin.clone()), (PositionSide::Net, Some(Side::Sell), d("0.03"), Some(d("5")), Some(MarginMode::isolated())));
    let p = &m.decode_positions_in(&DecodeCtx::new("", &sizes()), row("2", "long", "cross").as_bytes()).unwrap()[0];
    assert_eq!((p.side, p.direction, p.qty), (PositionSide::Long, Some(Side::Buy), d("0.02")));
    // The margin a symbol is sent with: the held position's, else cross.
    assert_eq!(m.decode_symbol_margin("BTC-USDT-SWAP", row("-3", "net", "isolated").as_bytes()).unwrap(), Some(MarginMode::isolated()));
    assert_eq!(m.decode_symbol_margin("BTC-USDT-SWAP", row("0", "net", "isolated").as_bytes()).unwrap(), Some(MarginMode::cross()));
    // Long/short mode: the side the order acts on reads its own row (OKX 4), never the other
    // side's. A long held isolated and a short held cross: the short is sent cross.
    assert!(m.margin_per_side());
    let both = with(POSITIONS, |v| {
        let mut long = v["data"][0].clone();
        long["instType"] = json!("SWAP");
        long["instId"] = json!("BTC-USDT-SWAP");
        long["pos"] = json!("2");
        long["posSide"] = json!("long");
        long["mgnMode"] = json!("isolated");
        let mut short = long.clone();
        short["pos"] = json!("3");
        short["posSide"] = json!("short");
        short["mgnMode"] = json!("cross");
        v["data"] = json!([long, short]);
    });
    assert_eq!(m.decode_symbol_margin_on("BTC-USDT-SWAP", PositionSide::Short, both.as_bytes()).unwrap(), Some(MarginMode::cross()));
    assert_eq!(m.decode_symbol_margin_on("BTC-USDT-SWAP", PositionSide::Long, both.as_bytes()).unwrap(), Some(MarginMode::isolated()));
    // A side not held: cross, whatever the other side is.
    let long_only = row("2", "long", "isolated");
    assert_eq!(m.decode_symbol_margin_on("BTC-USDT-SWAP", PositionSide::Short, long_only.as_bytes()).unwrap(), Some(MarginMode::cross()));
    // An instrument whose contract size is not known is an error, never a guess.
    assert!(m.decode_positions_in(&DecodeCtx::new("", &ContractSizes::new()), row("1", "net", "cross").as_bytes()).is_err());
}

#[test]
fn every_state_word_maps_and_an_unknown_one_stays_unknown() {
    let m = Okx::spot().unwrap();
    for (w, s) in [("live", OrderStatus::New), ("partially_filled", OrderStatus::PartiallyFilled), ("filled", OrderStatus::Filled), ("canceled", OrderStatus::Cancelled), ("mmp_canceled", OrderStatus::Cancelled)] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    assert_eq!(m.status_of("rejected"), Mapped::Unknown("rejected".into()));
}

#[test]
fn the_private_stream_maps_the_documented_frames() {
    let spot = Okx::spot().unwrap();
    let ev = spot.decode_private(WS_ORDERS.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.venue_id.as_str(), o.status.clone(), o.filled_qty, o.side.clone()), ("452197707845865472", Mapped::Known(OrderStatus::Filled), d("0.001"), Mapped::Known(Side::Sell)));
    let PrivateEvent::Fill(f) = &ev[1] else { panic!("{ev:?}") };
    assert_eq!((f.exec_id.as_str(), f.qty, f.price, f.fee, f.fee_currency.as_str(), f.position_only), ("242589207", d("0.001"), d("31527.1"), d("0.02522168"), "USDT", false));
    // The spot frame is not the swap book's.
    assert!(Okx::swap().unwrap().decode_private(WS_ORDERS.as_bytes()).unwrap().is_empty());
    // The positions example is an inverse FUTURES position: the futures book reads it with
    // the contract size of its list.
    let fut = Okx::futures().unwrap();
    let ev = fut.decode_private_in(&DecodeCtx::new("/ws/v5/private", &sizes()), WS_POSITIONS.as_bytes()).unwrap();
    let PrivateEvent::Position(p) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((p.symbol.as_str(), p.side, p.qty, p.margin.clone()), ("ETH-USD-210430", PositionSide::Long, d("10"), Some(MarginMode::isolated())));
    // Answers.
    let ans = |b: &str| spot.decode_private(b.as_bytes()).unwrap();
    assert_eq!(ans(WS_LOGIN), vec![PrivateEvent::Answer { op: "auth".into(), ok: true, message: "0".into() }]);
    assert!(matches!(&ans(WS_LOGIN_ERROR)[..], [PrivateEvent::Answer { op, ok: false, .. }, PrivateEvent::Answer { ok: false, .. }] if op == "auth"));
    assert!(matches!(&ans(WS_SUBSCRIBE)[..], [PrivateEvent::Answer { op, ok: true, .. }] if op == "subscribe"));
    assert!(matches!(&ans("pong")[..], [PrivateEvent::Answer { op, ok: true, .. }] if op == "pong"));
    let soon = r#"{"event":"notice","code":"64008","msg":"The connection will soon be closed for a service upgrade. Please reconnect.","connId":"a4d3ae55"}"#;
    assert!(matches!(&ans(soon)[..], [PrivateEvent::Answer { ok: false, .. }]), "a coming close reconnects");
}

#[test]
fn the_socket_subscribes_the_books_channels() {
    let s = Okx::swap().unwrap().private_socket(&settings()).unwrap();
    assert_eq!((s.path.as_str(), s.ping.as_slice(), s.ping_every_ms), ("/ws/v5/private", b"ping".as_slice(), 20_000));
    let frames: Vec<Value> = s.subscribes().iter().map(|f| serde_json::from_slice(f).unwrap()).collect();
    assert_eq!(frames, vec![json!({"op":"subscribe","args":[{"channel":"orders","instType":"SWAP"}]}), json!({"op":"subscribe","args":[{"channel":"positions","instType":"SWAP"}]})]);
    assert_eq!(Okx::spot().unwrap().private_socket(&settings()).unwrap().subscribes().len(), 1);
}

#[test]
fn errors_map_to_their_kind_and_the_mode_and_margin_are_read_again_on_their_codes() {
    let m = Okx::swap().unwrap();
    let e = |code: &str| m.decode_error(&otw_connector_testkit::http_frame(1, "order", 200, &format!(r#"{{"code":"1","msg":"Operation failed.","data":[{{"sCode":"{code}","sMsg":"m","subCode":""}}]}}"#))).unwrap();
    assert_eq!(e("51008").canon, Canon::Rejected(Refusal::Funds));
    assert_eq!(e("51603").canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(e("51400").canon, Canon::Rejected(Refusal::Final));
    assert_eq!(e("51121").canon, Canon::Rejected(Refusal::VenueRule));
    assert_eq!(e("51169").canon, Canon::Rejected(Refusal::ReduceOnly));
    assert_eq!(e("51006").canon, Canon::Rejected(Refusal::Price));
    assert_eq!(e("51022").canon, Canon::Rejected(Refusal::MarketClosed));
    assert_eq!(e("51511").canon, Canon::Rejected(Refusal::PostOnly));
    for lost in ["51016", "50071", "51149", "51054", "50004", "50001", "50013", "50026", "51784", "50042", "51412"] {
        assert_eq!(e(lost).canon, Canon::Unknown, "{lost}: looked up, never resent");
    }
    for rate in ["50011", "50061", "50040", "51113", "51513"] {
        assert_eq!(e(rate).canon, Canon::RateLimited, "{rate}");
    }
    assert_eq!(e("50113").canon, Canon::Auth);
    assert_eq!(e("50120").canon, Canon::Permission);
    assert!(!e("59999").mapped);
    assert!(m.invalidates_position_mode(&e("51000")) && !m.invalidates_position_mode(&e("51008")));
    assert!(m.invalidates_margin_mode(&e("50072")) && m.invalidates_margin_mode(&e("51043")) && !m.invalidates_margin_mode(&e("51000")));
    assert!(!Okx::spot().unwrap().invalidates_position_mode(&e("51000")));
    assert_eq!(m.error_table().len(), super::super::trade_errors::ERRORS.len());
}

/// The book's untriggered algo orders are working orders too, one list per `ordType` (`chase`
/// on contract books only): the kill switch cancels them through `cancel-algos`, 10 at most.
#[test]
fn algo_orders_are_listed_and_cancelled_in_batches_of_ten() {
    let spot = Okx::spot().unwrap();
    let types = |m: &Okx| m.open_orders(&settings(), Some("BTC-USDT"), CorrId(1)).unwrap().iter().skip(1).map(|h| h.query.iter().find(|(k, _)| k == "ordType").map(|(_, v)| v.clone()).unwrap()).collect::<Vec<_>>();
    assert_eq!(types(&spot), ["conditional,oco", "trigger", "move_order_stop", "iceberg", "twap", "smart_iceberg"]);
    let m = Okx::swap().unwrap();
    assert!(types(&m).contains(&"chase".to_string()));
    let open = m.open_orders(&settings(), None, CorrId(1)).unwrap();
    assert_eq!((open[1].path.as_str(), open[1].endpoint_id), ("/api/v5/trade/orders-algo-pending", "orders_algo_pending"));
    assert!(open[1].query.contains(&("instType".into(), "SWAP".into())) && open[1].query.contains(&("limit".into(), "100".into())));
    // The doc's list row, on the swap: `cond:<algoId>`, its size in the base coin, its trigger.
    let list = r#"{"code":"0","data":[{"algoId":"1753184812254216192","algoClOrdId":"","instType":"SWAP","instId":"BTC-USDT-SWAP","ordType":"conditional","posSide":"net","reduceOnly":"true","side":"sell","slOrdPx":"-1","slTriggerPx":"28000","state":"live","sz":"5","tgtCcy":"","tdMode":"cross","cTime":"1724751378980"}],"msg":""}"#;
    let (rows, next) = m.decode_orders_in(&DecodeCtx::new("orders_algo_pending", &sizes()), list.as_bytes()).unwrap();
    let r = &rows[0];
    assert_eq!((r.venue_id.as_str(), r.qty, r.stop, r.limit, r.reduce_only, r.trigger_pending, next), ("cond:1753184812254216192", d("0.05"), Some(d("28000")), None, true, true, None));
    assert_eq!(r.status, Mapped::Known(OrderStatus::New));
    // A spot algo sized in the quote coin states no base size.
    let q = with(list, |v| {
        v["data"][0]["instType"] = json!("SPOT");
        v["data"][0]["instId"] = json!("BTC-USDT");
        v["data"][0]["tgtCcy"] = json!("quote_ccy");
    });
    assert_eq!(spot.decode_orders_in(&DecodeCtx::new("orders_algo_pending", &sizes()), q.as_bytes()).unwrap().0[0].qty, Decimal::ZERO);
    // Cancel all: a batch opening with algo orders goes to cancel-algos, at most 10.
    let algo = |i: usize| VenueOrder { venue_id: format!("cond:{i}"), ..r.clone() };
    let plain = |i: usize| VenueOrder { venue_id: i.to_string(), ..r.clone() };
    let batch: Vec<VenueOrder> = (0..12).map(algo).collect();
    let c = m.cancel_ids(&settings(), &batch, CorrId(1)).unwrap();
    let sent = body(&c);
    assert_eq!((c.path.as_str(), c.endpoint_id, sent.as_array().unwrap().len()), ("/api/v5/trade/cancel-algos", "cancel_algos_batch", 10));
    assert_eq!(sent[0], json!({"instId":"BTC-USDT-SWAP","algoId":"0"}));
    // A batch opening with plain orders carries only those; the algo orders left are cancelled on their own.
    let mixed = vec![plain(1), plain(2), algo(3)];
    let c = m.cancel_ids(&settings(), &mixed, CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/api/v5/trade/cancel-batch-orders", json!([{"instId":"BTC-USDT-SWAP","ordId":"1"},{"instId":"BTC-USDT-SWAP","ordId":"2"}])));
    // Its answer, row by row, names the algo ids as followed.
    let answer = r#"{"code":"2","data":[{"algoClOrdId":"","algoId":"0","sCode":"0","sMsg":""},{"algoClOrdId":"","algoId":"1","sCode":"51400","sMsg":"failed"}],"msg":""}"#;
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "cancel_algos_batch", 200, answer)).is_none());
    let rows = m.decode_cancel_rows(&DecodeCtx::new("cancel_algos_batch", &ContractSizes::new()), answer.as_bytes()).unwrap();
    assert_eq!((rows[0].venue_id.as_str(), rows[0].error.is_none()), ("cond:0", true));
    assert_eq!((rows[1].venue_id.as_str(), rows[1].error.as_ref().map(|e| e.canon)), ("cond:1", Some(Canon::Rejected(Refusal::Final))));
    // One type the venue refuses to list is an empty list; a rate limit is not.
    let refused = otw_connector_testkit::http_frame(1, "orders_algo_pending", 200, r#"{"code":"51000","msg":"Parameter ordType error","data":[]}"#);
    assert!(m.decode_error(&refused).is_none());
    assert!(m.decode_orders_in(&DecodeCtx::new("orders_algo_pending", &sizes()), br#"{"code":"51000","msg":"Parameter ordType error","data":[]}"#).unwrap().0.is_empty());
    let busy = otw_connector_testkit::http_frame(1, "orders_algo_pending", 429, r#"{"code":"50011","msg":"Too Many Requests"}"#);
    assert_eq!(m.decode_error(&busy).map(|e| e.canon), Some(Canon::RateLimited));
}

/// The 54000 to 54109 block, the quote codes and `52000` are mapped: refusals with their reason,
/// the speed bump and a busy system rate limited; "Order ID already exists" is a duplicate whose
/// fate is unknown; an order lookup on an instrument no longer listed (`51001`) is not found.
#[test]
fn the_later_trade_codes_map_to_their_kind() {
    let m = Okx::swap().unwrap();
    let kind = |code: &str| m.decode_error(&otw_connector_testkit::http_frame(1, "order", 200, &envelope_item(code, "x"))).map(|e| (e.canon, e.mapped)).unwrap();
    assert_eq!(kind("51011"), (Canon::Unknown, true));
    assert_eq!(kind("54035"), (Canon::Rejected(Refusal::ReduceOnly), true));
    assert_eq!(kind("54011"), (Canon::Rejected(Refusal::ReduceOnly), true));
    assert_eq!(kind("54005"), (Canon::Rejected(Refusal::Other), true));
    assert_eq!(kind("54072"), (Canon::Rejected(Refusal::MarketClosed), true));
    assert_eq!(kind("54018"), (Canon::Rejected(Refusal::VenueRule), true));
    assert_eq!(kind("54047"), (Canon::RateLimited, true));
    assert_eq!(kind("54048"), (Canon::RateLimited, true));
    assert_eq!(kind("51820"), (Canon::Rejected(Refusal::Other), true));
    assert_eq!(kind("52000"), (Canon::Rejected(Refusal::Other), true));
    assert_eq!(kind("54109"), (Canon::Rejected(Refusal::Other), true));
    let gone = r#"{"code":"51001","msg":"Instrument ID, Instrument ID code or Spread ID doesn't exist.","data":[]}"#;
    assert_eq!(m.decode_error(&otw_connector_testkit::http_frame(1, "order_get", 200, gone)).map(|e| e.canon), Some(Canon::Rejected(Refusal::NotFound)));
    assert_eq!(m.decode_error(&otw_connector_testkit::http_frame(1, "order", 200, gone)).map(|e| e.canon), Some(Canon::InvalidRequest));
}

#[test]
fn a_batch_cancel_answer_names_each_row_and_its_refusal() {
    let m = Okx::swap().unwrap();
    let rows = m.decode_cancel_rows(&DecodeCtx::new("cancel_batch", &ContractSizes::new()), CANCEL_BATCH.as_bytes()).unwrap();
    assert_eq!(rows, vec![CancelRow { venue_id: "12345689".into(), error: None }, CancelRow { venue_id: "12344".into(), error: None }]);
    let part = with(CANCEL_BATCH, |v| {
        v["code"] = json!("2");
        v["data"][1]["sCode"] = json!("51400");
        v["data"][1]["sMsg"] = json!("Order cancellation failed as the order has been filled, canceled or does not exist.");
    });
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "cancel_batch", 200, &part)).is_none(), "rows are read one by one");
    let rows = m.decode_cancel_rows(&DecodeCtx::new("cancel_batch", &ContractSizes::new()), part.as_bytes()).unwrap();
    assert_eq!(rows[1].error.as_ref().map(|e| e.canon), Some(Canon::Rejected(Refusal::Final)));
}

// ── The catalog ──

fn envelope_item(code: &str, msg: &str) -> String {
    json!({ "code": "1", "msg": "Operation failed.", "data": [{ "clOrdId": "", "ordId": "", "sCode": code, "sMsg": msg, "subCode": "", "tag": "", "ts": "1695190491421" }], "inTime": "1695190491421339", "outTime": "1695190491423240" }).to_string()
}

fn envelope(code: &str, msg: &str) -> String {
    json!({ "code": code, "msg": msg, "data": [] }).to_string()
}

/// The catalog's answers: the documented examples with their ids, sizes and states changed
/// (`synthetic`, built on the documented shape).
struct Script<M> {
    m: M,
    swap: bool,
}

impl<M> Script<M> {
    fn venue_size(&self, q: Decimal) -> Decimal {
        if self.swap {
            (q / d(CT)).normalize()
        } else {
            q
        }
    }

    fn inst_type(&self) -> &'static str {
        if self.swap {
            "SWAP"
        } else {
            "SPOT"
        }
    }
}

impl<M: TradeMapper + Clone> VenueScript for Script<M> {
    type M = M;

    fn mapper(&self) -> M {
        self.m.clone()
    }

    fn settings(&self) -> HashMap<String, String> {
        settings()
    }

    fn signer(&self) -> Arc<dyn Signer> {
        Arc::new(signer(&settings()))
    }

    fn symbol(&self) -> &'static str {
        if self.swap {
            "BTC-USDT-SWAP"
        } else {
            "BTC-USDT"
        }
    }

    fn qty(&self) -> Decimal {
        d("0.01")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        let src = if self.swap { SWAP_LIST } else { SPOT_LIST };
        match closed {
            None => src.to_string(),
            Some(w) => with(src, |v| v["data"][0]["state"] = json!(w)),
        }
    }

    fn closed_word(&self) -> &'static str {
        "suspend"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::PartiallyFilled => "partially_filled",
            OrderStatus::Filled => "filled",
            OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired => "canceled",
            _ => "live",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        with(PLACE, |v| {
            v["data"][0]["ordId"] = json!(venue_id);
            v["data"][0]["clOrdId"] = json!(client_id);
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut o = first_row(PENDING);
                o["instType"] = json!(self.inst_type());
                o["instId"] = json!(self.symbol());
                o["ordId"] = json!(r.venue_id);
                o["clOrdId"] = json!(r.client_id.unwrap_or(""));
                o["side"] = json!(if r.side == Side::Buy { "buy" } else { "sell" });
                o["ordType"] = json!("limit");
                o["state"] = json!(r.word);
                o["sz"] = json!(self.venue_size(r.qty).to_string());
                o["accFillSz"] = json!(self.venue_size(r.filled).to_string());
                o["posSide"] = json!(if self.swap { "net" } else { "" });
                o
            })
            .collect();
        with(PENDING, |v| v["data"] = json!(list))
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut f = first_row(FILLS);
                f["instType"] = json!(self.inst_type());
                f["instId"] = json!(self.symbol());
                f["tradeId"] = json!(r.exec_id);
                f["billId"] = json!(format!("b{}", r.exec_id));
                f["ordId"] = json!(r.venue_id);
                f["clOrdId"] = json!(r.client_id);
                f["fillSz"] = json!(self.venue_size(r.qty).to_string());
                f["fillPx"] = json!(r.price.to_string());
                f["fee"] = json!((-r.fee).to_string());
                f["feeCcy"] = json!("USDT");
                f
            })
            .collect();
        with(FILLS, |v| v["data"] = json!(list))
    }

    fn fee_currency(&self) -> &'static str {
        "USDT"
    }

    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|(side, dir, qty)| {
                let mut p = first_row(POSITIONS);
                p["instType"] = json!("SWAP");
                p["instId"] = json!(self.symbol());
                p["mgnMode"] = json!("cross");
                p["posSide"] = json!(match side {
                    PositionSide::Net => "net",
                    PositionSide::Long => "long",
                    PositionSide::Short => "short",
                });
                let n = self.venue_size(*qty);
                p["pos"] = json!(if *side == PositionSide::Net && *dir == Some(Side::Sell) { (-n).to_string() } else { n.to_string() });
                p
            })
            .collect();
        with(POSITIONS, |v| v["data"] = json!(list))
    }

    fn ok(&self) -> String {
        CANCEL.to_string()
    }

    fn dry_run_ok(&self) -> String {
        CANCEL.to_string()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        match r {
            VenueRefusal::Funds => Answer::new(200, envelope_item("51008", "Order failed. Insufficient USDT balance in account")),
            VenueRefusal::ReduceOnly => Answer::new(200, envelope_item("51169", "Order failed because you don't have any positions in this direction for this contract to reduce or close.")),
            VenueRefusal::NotFound => Answer::new(200, envelope_item("51603", "Order does not exist.")),
            VenueRefusal::Final => Answer::new(200, envelope_item("51400", "Order cancellation failed as the order has been filled, canceled or does not exist.")),
            VenueRefusal::RateLimited(s) => Answer { status: 429, body: envelope("50011", "Too Many Requests"), headers: vec![("Retry-After", s.to_string())] },
            VenueRefusal::ClockSkew => Answer::new(401, envelope("50102", "Timestamp request expired.")),
            VenueRefusal::Unmapped => Answer::new(200, envelope_item("59999", "something new")),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            VenueRefusal::PositionModeMismatch => Answer::new(200, envelope_item("51000", "Parameter posSide error")),
        }
    }
}

/// Run the catalog. One of its checks is written for another venue's calls, and fails here by
/// construction: T31+T34 mounts one cancel call per order (OKX cancels in batches of ids). It is
/// run on OKX's own terms below (`the_kill_switch_cancels_in_batches_of_twenty`). Every other
/// failure fails this test.
async fn catalog<M: TradeMapper + Clone + 'static>(s: Script<M>, expected: &[&str]) {
    let out = tokio::spawn(async move { run_catalog(&s).await }).await;
    let failed: Vec<String> = match out {
        Ok(r) => {
            println!("ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
            Vec::new()
        }
        Err(e) => {
            let p = e.into_panic();
            let text = p.downcast_ref::<String>().cloned().unwrap_or_default();
            println!("{text}");
            text.lines().skip(1).map(|l| l.split(':').next().unwrap_or_default().to_string()).collect()
        }
    };
    let want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(failed, want, "only the checks written for another venue's words may fail");
}

#[tokio::test]
async fn the_trading_catalog_passes_on_spot() {
    catalog(Script { m: Okx::spot().unwrap(), swap: false }, &["T31+T34"]).await;
}

/// The swap book through the catalog. The catalog serves no account configuration, and builds
/// its probe requests with a bare `Placing` (no contract size), so the real mapper runs behind
/// [`CatalogSwap`]: every call and decoder is the mapper's own, except that the position mode
/// comes from the position rows the catalog serves (the account-level read is tested on its own,
/// `the_position_mode_is_read_once_at_account_level_and_again_after_51000`), and a probe built
/// without a contract size is given the list's.
#[tokio::test]
async fn the_trading_catalog_passes_on_swap() {
    catalog(Script { m: CatalogSwap(Okx::swap().unwrap()), swap: true }, &["T31+T34"]).await;
}

#[derive(Clone)]
struct CatalogSwap(Okx);

impl Mapper for CatalogSwap {
    type Session = Session;

    fn capabilities(&self) -> &otw_connector_model::Capabilities {
        self.0.capabilities()
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.0.base_urls()
    }

    fn encode(&self, cmd: &otw_connector_model::Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, otw_connector_model::ConnectorError> {
        self.0.encode(cmd, s, ctx)
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<otw_connector_model::Envelope>, otw_connector_model::ConnectorError> {
        self.0.decode(frame, s, ctx)
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, otw_connector_model::ConnectorError> {
        self.0.instrument_id(symbol)
    }
}

type R<T> = Result<T, otw_connector_model::ConnectorError>;

impl TradeMapper for CatalogSwap {
    fn venue_name(&self) -> &str {
        self.0.venue_name()
    }
    fn client_id_rule(&self) -> ClientIdRule {
        self.0.client_id_rule()
    }
    fn is_sandbox(&self, s: &Settings) -> bool {
        self.0.is_sandbox(s)
    }
    fn place(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<HttpRequest> {
        let p = Placing { contract_size: p.contract_size.or(Some(d(CT))), ..p.clone() };
        self.0.place(s, o, &p)
    }
    fn amend(&self, s: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> R<HttpRequest> {
        self.0.amend(s, cur, change, p)
    }
    fn cancel(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<HttpRequest> {
        self.0.cancel(s, symbol, id, corr)
    }
    fn cancel_all_way(&self) -> CancelAllWay {
        self.0.cancel_all_way()
    }
    fn cancel_ids(&self, s: &Settings, orders: &[VenueOrder], corr: CorrId) -> R<HttpRequest> {
        self.0.cancel_ids(s, orders, corr)
    }
    fn decode_cancel_rows(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> R<Vec<CancelRow>> {
        self.0.decode_cancel_rows(ctx, body)
    }
    fn query_by_client_id(&self, s: &Settings, symbol: &str, cid: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_client_id(s, symbol, cid, corr)
    }
    fn query_by_venue_id(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_venue_id(s, symbol, id, corr)
    }
    fn open_orders(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.open_orders(s, symbol, corr)
    }
    fn fills_since(&self, s: &Settings, symbol: &str, from: i64, to: i64, corr: CorrId) -> R<HttpRequest> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn instrument_rules(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<HttpRequest> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        self.0.with_cursor(req, cursor)
    }
    fn check_order(&self, o: &OrderRequest) -> R<()> {
        self.0.check_order(o)
    }
    fn positions(&self, s: &Settings, symbol: &str, corr: CorrId) -> R<Option<HttpRequest>> {
        self.0.positions(s, symbol, corr)
    }
    fn all_positions(&self, s: &Settings, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.all_positions(s, corr)
    }
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.0.invalidates_position_mode(e)
    }
    fn symbol_margin(&self, s: &Settings, symbol: &str, corr: CorrId) -> R<Option<HttpRequest>> {
        self.0.symbol_margin(s, symbol, corr)
    }
    fn decode_symbol_margin(&self, symbol: &str, body: &[u8]) -> R<Option<MarginMode>> {
        self.0.decode_symbol_margin(symbol, body)
    }
    fn margin_per_side(&self) -> bool {
        self.0.margin_per_side()
    }
    fn decode_symbol_margin_on(&self, symbol: &str, side: PositionSide, body: &[u8]) -> R<Option<MarginMode>> {
        self.0.decode_symbol_margin_on(symbol, side, body)
    }
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.0.invalidates_margin_mode(e)
    }
    fn counts_contracts(&self) -> bool {
        self.0.counts_contracts()
    }
    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        self.0.decode_error(frame)
    }
    fn decode_ack(&self, body: &[u8]) -> R<Ack> {
        self.0.decode_ack(body)
    }
    fn decode_orders(&self, body: &[u8]) -> R<Page<VenueOrder>> {
        self.0.decode_orders(body)
    }
    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> R<Page<VenueOrder>> {
        self.0.decode_orders_in(ctx, body)
    }
    fn decode_fills(&self, body: &[u8]) -> R<Page<VenueFill>> {
        self.0.decode_fills(body)
    }
    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> R<Page<VenueFill>> {
        self.0.decode_fills_in(ctx, body)
    }
    fn decode_rules(&self, body: &[u8]) -> R<Page<VenueRules>> {
        self.0.decode_rules(body)
    }
    fn decode_positions(&self, body: &[u8]) -> R<Vec<VenuePosition>> {
        self.0.decode_positions(body)
    }
    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> R<Vec<VenuePosition>> {
        self.0.decode_positions_in(ctx, body)
    }
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        self.0.status_of(word)
    }
    fn private_socket(&self, s: &Settings) -> Option<PrivateSocket> {
        self.0.private_socket(s)
    }
    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> R<Vec<PrivateEvent>> {
        self.0.decode_private_in(ctx, payload)
    }
    fn error_table(&self) -> &'static [ErrorRow] {
        self.0.error_table()
    }
}

// ── Through the trader, on OKX's own terms ──

mod live {
    use super::*;
    use otw_connector_engine::trading::{Account, TicketOptions, Trader};
    use otw_connector_testkit::wiremock::matchers::{body_string_contains, method, path, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer, ResponseTemplate};
    use otw_connector_testkit::{driver_against, ts, ManualClock};

    async fn server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v5/public/instruments")).respond_with(ResponseTemplate::new(200).set_body_string(SWAP_LIST)).mount(&server).await;
        server
    }

    fn config(mode: &str) -> String {
        with(CONFIG, |v| v["data"][0]["posMode"] = json!(mode))
    }

    fn positions(rows: &[(&str, &str)]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|(pos, side)| {
                let mut p = first_row(POSITIONS);
                p["instType"] = json!("SWAP");
                p["instId"] = json!("BTC-USDT-SWAP");
                p["mgnMode"] = json!("cross");
                p["pos"] = json!(pos);
                p["posSide"] = json!(side);
                p
            })
            .collect();
        with(POSITIONS, |v| v["data"] = json!(list))
    }

    fn ack(cid: &str, id: &str) -> String {
        with(PLACE, |v| {
            v["data"][0]["ordId"] = json!(id);
            v["data"][0]["clOrdId"] = json!(cid);
        })
    }

    async fn bodies(server: &MockServer, p: &str) -> Vec<Value> {
        server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == p).map(|r| serde_json::from_slice(&r.body).unwrap()).collect()
    }

    async fn count(server: &MockServer, p: &str) -> usize {
        server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == p).count()
    }

    /// T10 on OKX's terms: the whole position (long, short) or part of it, at the market,
    /// reduce-only in net mode, the size the venue holds read now and sent in contracts; in
    /// long/short mode the side closed is named and no flag is sent.
    #[tokio::test]
    async fn close_position_sends_the_exact_size_reduce_only() {
        for (mode, rows, dir, part, want_side, want_sz, want_pos) in [
            ("net_mode", vec![("3", "net")], None, None, "sell", "3", None),
            ("net_mode", vec![("-3", "net")], None, None, "buy", "3", None),
            ("net_mode", vec![("3", "net")], None, Some("0.01"), "sell", "1", None),
            ("long_short_mode", vec![("2", "long"), ("4", "short")], Some(Side::Sell), None, "buy", "4", Some("short")),
        ] {
            let server = server().await;
            Mock::given(method("GET")).and(path("/api/v5/account/config")).respond_with(ResponseTemplate::new(200).set_body_string(config(mode))).mount(&server).await;
            Mock::given(method("GET")).and(path("/api/v5/account/positions")).respond_with(ResponseTemplate::new(200).set_body_string(positions(&rows))).mount(&server).await;
            Mock::given(method("POST")).and(path("/api/v5/trade/order")).respond_with(ResponseTemplate::new(200).set_body_string(ack("cl1", "v1"))).mount(&server).await;
            let dv = driver_against(Okx::swap().unwrap(), &server, ManualClock::at(ts(1_767_225_600)));
            let s = settings();
            let sg = signer(&s);
            let a = Account { settings: &s, signer: &sg, scope: "close" };
            let t: Trader<Okx> = Trader::new();
            let placed = t.close_position(&dv, &a, "BTC-USDT-SWAP", dir, part.map(d), "cl1").await.unwrap();
            assert_eq!(placed.venue_id, "v1");
            let sent = bodies(&server, "/api/v5/trade/order").await;
            assert_eq!(sent.len(), 1);
            let b = &sent[0];
            assert_eq!((b["side"].as_str(), b["sz"].as_str(), b["ordType"].as_str(), b["tdMode"].as_str()), (Some(want_side), Some(want_sz), Some("market"), Some("cross")), "{mode} {b}");
            assert_eq!(b["posSide"].as_str(), want_pos);
            assert_eq!(b.get("reduceOnly").and_then(Value::as_bool), if want_pos.is_none() { Some(true) } else { None });
        }
    }

    /// T31 and T34 on OKX's terms: every working order, an external one included, cancelled in
    /// batches of at most 20 ids; a row the batch refused is cancelled on its own after. The
    /// book's untriggered algo orders are working orders too: cancelled through `cancel-algos`.
    #[tokio::test]
    async fn the_kill_switch_cancels_in_batches_of_twenty() {
        let server = server().await;
        let row = |id: usize, cid: &str| {
            let mut o = first_row(PENDING);
            o["instType"] = json!("SWAP");
            o["instId"] = json!("BTC-USDT-SWAP");
            o["ordId"] = json!(id.to_string());
            o["clOrdId"] = json!(cid);
            o["sz"] = json!("1");
            o["accFillSz"] = json!("0");
            o["posSide"] = json!("net");
            o
        };
        let mut working: Vec<Value> = (0..24).map(|i| row(i, &format!("k{i}"))).collect();
        working[0]["clOrdId"] = json!(""); // placed outside OTW
        let first = with(PENDING, |v| v["data"] = json!(working));
        Mock::given(method("GET")).and(path("/api/v5/trade/orders-pending")).respond_with(ResponseTemplate::new(200).set_body_string(first)).up_to_n_times(1).with_priority(1).mount(&server).await;
        // After the batches, order 7 is still listed (its row was refused): cancelled alone.
        let left = with(PENDING, |v| v["data"] = json!([row(7, "k7")]));
        Mock::given(method("GET")).and(path("/api/v5/trade/orders-pending")).respond_with(ResponseTemplate::new(200).set_body_string(left)).with_priority(2).mount(&server).await;
        let batch = with(CANCEL_BATCH, |v| {
            v["code"] = json!("2");
            v["data"][1]["ordId"] = json!("7");
            v["data"][1]["sCode"] = json!("51410");
            v["data"][1]["sMsg"] = json!("Cancellation failed as the order is already in canceling status or pending settlement.");
        });
        Mock::given(method("POST")).and(path("/api/v5/trade/cancel-batch-orders")).respond_with(ResponseTemplate::new(200).set_body_string(batch)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v5/trade/cancel-order")).and(body_string_contains("\"7\"")).respond_with(ResponseTemplate::new(200).set_body_string(CANCEL)).mount(&server).await;
        // Two stop orders placed elsewhere; every other algo list is empty. The batch that
        // reaches them opens with plain orders, so they are still listed after it and are
        // cancelled one by one.
        let algo = |id: &str| json!({"algoId": id, "instType": "SWAP", "instId": "BTC-USDT-SWAP", "ordType": "conditional", "side": "sell", "posSide": "net", "sz": "1", "slTriggerPx": "28000", "slOrdPx": "-1", "state": "live", "cTime": "1724751378980"});
        let stops = json!({"code": "0", "msg": "", "data": [algo("a1"), algo("a2")]}).to_string();
        Mock::given(method("GET")).and(path("/api/v5/trade/orders-algo-pending")).and(query_param("ordType", "conditional,oco")).respond_with(ResponseTemplate::new(200).set_body_string(stops)).up_to_n_times(2).with_priority(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v5/trade/orders-algo-pending")).respond_with(ResponseTemplate::new(200).set_body_string(r#"{"code":"0","msg":"","data":[]}"#)).with_priority(2).mount(&server).await;
        let algo_ok = r#"{"code":"0","msg":"","data":[{"algoClOrdId":"","algoId":"a1","clOrdId":"","sCode":"0","sMsg":"","tag":""},{"algoClOrdId":"","algoId":"a2","clOrdId":"","sCode":"0","sMsg":"","tag":""}]}"#;
        Mock::given(method("POST")).and(path("/api/v5/trade/cancel-algos")).respond_with(ResponseTemplate::new(200).set_body_string(algo_ok)).mount(&server).await;
        let dv = driver_against(Okx::swap().unwrap(), &server, ManualClock::at(ts(1_767_225_600)));
        let s = settings();
        let sg = signer(&s);
        let a = Account { settings: &s, signer: &sg, scope: "kill" };
        let t: Trader<Okx> = Trader::new();
        let n = t.cancel_all(&dv, &a).await.unwrap();
        assert_eq!(n, 26, "every working order, the external one and the two stops included");
        let algos = bodies(&server, "/api/v5/trade/cancel-algos").await;
        let ids: Vec<&str> = algos.iter().flat_map(|b| b.as_array().unwrap().iter().filter_map(|r| r["algoId"].as_str())).collect();
        assert_eq!(ids, ["a1", "a2"], "both stops cancelled by their algo id");
        let batches = bodies(&server, "/api/v5/trade/cancel-batch-orders").await;
        let sizes: Vec<usize> = batches.iter().map(|b| b.as_array().unwrap().len()).collect();
        assert_eq!(sizes, vec![20, 4]);
        assert!(batches[0].as_array().unwrap().iter().any(|r| r["ordId"] == "0"), "the external order is in the batch");
        assert_eq!(count(&server, "/api/v5/trade/cancel-order").await, 1, "the refused row is cancelled on its own");
    }

    /// The account's `posMode` decides `posSide`, read once (not before each order), and read
    /// again after a `51000` refusal; the margin of the held position goes with the order.
    #[tokio::test]
    async fn the_position_mode_is_read_once_at_account_level_and_again_after_51000() {
        let server = server().await;
        Mock::given(method("GET")).and(path("/api/v5/account/config")).respond_with(ResponseTemplate::new(200).set_body_string(config("long_short_mode"))).mount(&server).await;
        let held = with(&positions(&[("2", "short")]), |v| v["data"][0]["mgnMode"] = json!("isolated"));
        Mock::given(method("GET")).and(path("/api/v5/account/positions")).respond_with(ResponseTemplate::new(200).set_body_string(held)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v5/trade/order")).and(body_string_contains("\"bad\"")).respond_with(ResponseTemplate::new(200).set_body_string(envelope_item("51000", "Parameter posSide error"))).with_priority(1).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v5/trade/order")).respond_with(ResponseTemplate::new(200).set_body_string(ack("x", "v"))).with_priority(2).mount(&server).await;
        let dv = driver_against(Okx::swap().unwrap(), &server, ManualClock::at(ts(1_767_225_600)));
        let s = settings();
        let sg = signer(&s);
        let a = Account { settings: &s, signer: &sg, scope: "mode" };
        let t: Trader<Okx> = Trader::new();
        let o = |k: &str| order("BTC-USDT-SWAP", k, Side::Sell, OrderType::Limit, "0.01", Some("30000"));
        t.place(&dv, &a, &[o("one")], &TicketOptions::default()).await.unwrap();
        t.place(&dv, &a, &[o("two")], &TicketOptions::default()).await.unwrap();
        assert_eq!(count(&server, "/api/v5/account/config").await, 1, "held, not read before each order");
        let sent = bodies(&server, "/api/v5/trade/order").await;
        assert_eq!((sent[0]["posSide"].as_str(), sent[0]["tdMode"].as_str(), sent[0]["sz"].as_str()), (Some("short"), Some("isolated"), Some("1")));
        assert!(t.place(&dv, &a, &[o("bad")], &TicketOptions::default()).await.is_err());
        t.place(&dv, &a, &[o("three")], &TicketOptions::default()).await.unwrap();
        assert_eq!(count(&server, "/api/v5/account/config").await, 2, "read again once after 51000");
        assert_eq!(bodies(&server, "/api/v5/trade/order").await.iter().filter(|b| b["clOrdId"] == "bad").count(), 1, "the refused order is never resent");
        // The instrument list (and its contract sizes) was read once for all of it.
        assert_eq!(count(&server, "/api/v5/public/instruments").await, 2, "once for the rules, once for the sizes");
    }

    /// OKX 4: in long/short mode each side carries its own `mgnMode`. A long held isolated and
    /// a short held cross: an order opening the short goes cross, one opening the long goes
    /// isolated, whatever row comes first.
    #[tokio::test]
    async fn in_long_short_mode_each_side_is_sent_with_its_own_margin() {
        let server = server().await;
        Mock::given(method("GET")).and(path("/api/v5/account/config")).respond_with(ResponseTemplate::new(200).set_body_string(config("long_short_mode"))).mount(&server).await;
        let rows = with(&positions(&[("2", "long"), ("3", "short")]), |v| v["data"][0]["mgnMode"] = json!("isolated"));
        Mock::given(method("GET")).and(path("/api/v5/account/positions")).respond_with(ResponseTemplate::new(200).set_body_string(rows)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v5/trade/order")).respond_with(ResponseTemplate::new(200).set_body_string(ack("x", "v"))).mount(&server).await;
        let dv = driver_against(Okx::swap().unwrap(), &server, ManualClock::at(ts(1_767_225_600)));
        let s = settings();
        let sg = signer(&s);
        let a = Account { settings: &s, signer: &sg, scope: "sides" };
        let t: Trader<Okx> = Trader::new();
        t.place(&dv, &a, &[order("BTC-USDT-SWAP", "short1", Side::Sell, OrderType::Limit, "0.01", Some("30000"))], &TicketOptions::default()).await.unwrap();
        t.place(&dv, &a, &[order("BTC-USDT-SWAP", "long1", Side::Buy, OrderType::Limit, "0.01", Some("20000"))], &TicketOptions::default()).await.unwrap();
        let sent = bodies(&server, "/api/v5/trade/order").await;
        let got: Vec<(Option<&str>, Option<&str>)> = sent.iter().map(|b| (b["posSide"].as_str(), b["tdMode"].as_str())).collect();
        assert_eq!(got, vec![(Some("short"), Some("cross")), (Some("long"), Some("isolated"))]);
    }

    /// A change in place: the order read by its id, then amend-order with the new price.
    #[tokio::test]
    async fn an_amend_goes_out_once_by_order_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v5/public/instruments")).respond_with(ResponseTemplate::new(200).set_body_string(SPOT_LIST)).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v5/trade/order")).and(query_param("ordId", "1752588852617379840")).respond_with(ResponseTemplate::new(200).set_body_string(PENDING)).mount(&server).await;
        let amended = json!({"code":"0","msg":"","data":[{"clOrdId":"","ordId":"1752588852617379840","ts":"1695190491421","reqId":"","sCode":"0","sMsg":"","subCode":""}]}).to_string();
        Mock::given(method("POST")).and(path("/api/v5/trade/amend-order")).respond_with(ResponseTemplate::new(200).set_body_string(amended)).mount(&server).await;
        let dv = driver_against(Okx::spot().unwrap(), &server, ManualClock::at(ts(1_767_225_600)));
        let s = settings();
        let sg = signer(&s);
        let a = Account { settings: &s, signer: &sg, scope: "amend" };
        let t: Trader<Okx> = Trader::new();
        let out = t.modify(&dv, &a, "BTC-USDT", "1752588852617379840", &OrderAmend { limit: Some(Price::new(d("13000.1"))), ..Default::default() }).await.unwrap();
        assert!(out.is_none());
        let sent = bodies(&server, "/api/v5/trade/amend-order").await;
        assert_eq!(sent, vec![json!({"instId":"BTC-USDT","ordId":"1752588852617379840","newPx":"13000.1"})]);
        // Off the tick: refused before I/O.
        assert!(t.modify(&dv, &a, "BTC-USDT", "1752588852617379840", &OrderAmend { limit: Some(Price::new(d("13000.15"))), ..Default::default() }).await.is_err());
        assert_eq!(count(&server, "/api/v5/trade/amend-order").await, 1);
    }

    /// An accepted place answer that names no order is found by its client id, never resent.
    #[tokio::test]
    async fn an_ack_without_an_id_is_looked_up() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v5/public/instruments")).respond_with(ResponseTemplate::new(200).set_body_string(SPOT_LIST)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v5/trade/order")).respond_with(ResponseTemplate::new(200).set_body_string(ack("noid", ""))).mount(&server).await;
        let found = with(PENDING, |v| {
            v["data"][0]["clOrdId"] = json!("noid");
            v["data"][0]["ordId"] = json!("v-noid");
        });
        Mock::given(method("GET")).and(path("/api/v5/trade/order")).and(query_param("clOrdId", "noid")).respond_with(ResponseTemplate::new(200).set_body_string(found)).mount(&server).await;
        let dv = driver_against(Okx::spot().unwrap(), &server, ManualClock::at(ts(1_767_225_600)));
        let s = settings();
        let sg = signer(&s);
        let a = Account { settings: &s, signer: &sg, scope: "noid" };
        let t: Trader<Okx> = Trader::new();
        let placed = t.place(&dv, &a, &[order("BTC-USDT", "noid", Side::Buy, OrderType::Limit, "0.001", Some("13000"))], &TicketOptions::default()).await.unwrap();
        assert_eq!(placed[0].venue_id, "v-noid");
        assert_eq!(count(&server, "/api/v5/trade/order").await, 2, "one place, one lookup (same path, GET)");
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live instrument lists decode to sound rules for every instrument, and an order call with
/// a key OKX never issued, or with no key, is answered with the auth error the table expects
/// (and nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers okx::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    for (m, sym) in [(Okx::spot().unwrap(), "BTC-USDT"), (Okx::swap().unwrap(), "BTC-USDT-SWAP"), (Okx::futures().unwrap(), "")] {
        let label = format!("{:?}", m.book());
        let dv = Driver::new(m.clone(), DriverConfig::new("okx"), reqwest::Client::new());
        let junk = settings();
        let sg = signer(&junk);
        let a = Account { settings: &junk, signer: &sg, scope: "net" };
        let trader: Trader<Okx> = Trader::new();
        let rules = trader.all_rules(&dv, &a).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(rules.len() > 5, "{label}: {} instruments", rules.len());
        for r in &rules {
            r.sound().unwrap_or_else(|e| panic!("{label}: {e}"));
            // A listing not trading yet (`preopen`) may state no tick or step: it takes no order.
            if r.status == TradingStatus::Trading {
                assert!(r.tick.is_some() && r.lot_step.is_some(), "{label}: {} states no tick or step", r.instrument.native);
            }
        }
        let traded = rules.iter().filter(|r| r.status == TradingStatus::Trading).count();
        println!("{label}: {} instruments, {traded} trading", rules.len());
        if sym.is_empty() {
            continue;
        }
        let btc = rules.iter().find(|r| r.instrument.native == sym).unwrap_or_else(|| panic!("{label}: no {sym}"));
        println!("{label}: {btc:?}");
        let o = order(sym, "otwnetprobe", Side::Buy, OrderType::Limit, &btc.min_qty.unwrap().to_string(), Some(&btc.tick.unwrap().to_string()));
        let p = Placing { contract_size: btc.contract_size, ..placing("otwnetprobe", PositionSide::Net) };
        let req = m.place(&junk, &o, &p).unwrap();
        for (what, signed) in [("junk key", true), ("unsigned", false)] {
            let mut r = req.clone();
            r.signed = signed;
            let frame = dv.send_as(r, Some(&sg)).await.unwrap_or_else(|e| panic!("{label} {what}: {e}"));
            let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label} {what}: OKX accepted an order with no valid key"));
            println!("{label} {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
            assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{label} {what}: {e:?}");
        }
    }
}
