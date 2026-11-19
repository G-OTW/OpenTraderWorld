//! Bybit orders, from the documentation alone. Fixtures under `fixtures/trade/`:
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_create_order.json` | https://bybit-exchange.github.io/docs/v5/order/create-order (Response Example) |
//! | `doc_cancel_order.json` | https://bybit-exchange.github.io/docs/v5/order/cancel-order (Response Example) |
//! | `doc_cancel_all.json` | https://bybit-exchange.github.io/docs/v5/order/cancel-all (Response Example) |
//! | `doc_open_orders.json` | https://bybit-exchange.github.io/docs/v5/order/open-order (Response Example) |
//! | `doc_order_history.json` | https://bybit-exchange.github.io/docs/v5/order/order-list (Response Example) |
//! | `doc_execution_list.json` | https://bybit-exchange.github.io/docs/v5/order/execution (Response Example) |
//! | `doc_position_list.json` | https://bybit-exchange.github.io/docs/v5/position (Response Example) |
//! | `doc_pre_check.json` | https://bybit-exchange.github.io/docs/v5/order/pre-check-order (Response Example) |
//! | `doc_instruments_linear.json` | https://bybit-exchange.github.io/docs/v5/market/instrument (Response Example, Linear, first object) |
//! | `doc_instruments_spot.json` | https://bybit-exchange.github.io/docs/v5/market/instrument (Response Example, Spot) |
//! | `doc_ws_order.json` | https://bybit-exchange.github.io/docs/v5/websocket/private/order (Stream Example) |
//! | `doc_ws_execution.json` | https://bybit-exchange.github.io/docs/v5/websocket/private/execution (Stream Example) |
//! | `doc_ws_position.json` | https://bybit-exchange.github.io/docs/v5/websocket/private/position (Stream Example) |
//!
//! Copied verbatim from the docs (github.com/bybit-exchange/docs, tree 6233a5a0, read
//! 2026-11-11). The catalog script ([`Script`]) answers with these examples with only their ids,
//! sizes, prices, symbol and status changed: those answers are `synthetic`, built on the
//! documented shape. Every error body is `synthetic` too: the doc lists codes, not bodies, and
//! the envelope is the one every documented answer carries.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{CorrId, Decimal, InstrumentId, Mapped, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce};
use otw_connector_sdk::trade::{Canon, Placing, PositionSide, PrivateEvent, Refusal, TradeMapper, TradingStatus};
use otw_connector_sdk::{Mapper, Method, Signer, SignableRequest};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript, CATALOG_NOW_MS};
use serde_json::{json, Value};

use super::super::signer::BybitSigner;
use super::super::Bybit;

const CREATE: &str = include_str!("fixtures/trade/doc_create_order.json");
const CANCEL: &str = include_str!("fixtures/trade/doc_cancel_order.json");
const CANCEL_ALL: &str = include_str!("fixtures/trade/doc_cancel_all.json");
const OPEN: &str = include_str!("fixtures/trade/doc_open_orders.json");
const HISTORY: &str = include_str!("fixtures/trade/doc_order_history.json");
const EXECUTIONS: &str = include_str!("fixtures/trade/doc_execution_list.json");
const POSITIONS: &str = include_str!("fixtures/trade/doc_position_list.json");
const PRE_CHECK: &str = include_str!("fixtures/trade/doc_pre_check.json");
const LINEAR: &str = include_str!("fixtures/trade/doc_instruments_linear.json");
const SPOT: &str = include_str!("fixtures/trade/doc_instruments_spot.json");
const WS_ORDER: &str = include_str!("fixtures/trade/doc_ws_order.json");
const WS_EXECUTION: &str = include_str!("fixtures/trade/doc_ws_execution.json");

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "XXXXXXXXXX"), ("api_secret", "secret-test")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn order(m: &Bybit, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>) -> OrderRequest {
    let (l, s) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new(m.venue(), "BTCUSDT"),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: if l { limit.map(|p| Price::new(d(p))) } else { None },
        stop: if s { limit.map(|p| Price::new(d(p))) } else { None },
        tif: TimeInForce::Gtc,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts::default(),
    }
}

fn body(req: &otw_connector_sdk::HttpRequest) -> Value {
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

fn placing(cid: &str, position: PositionSide) -> Placing {
    Placing::new(cid, position, CorrId(1))
}

// ── Requests ──

#[test]
fn a_linear_close_is_the_documented_body_field_for_field() {
    // create-order, Request Example "USDT Perp close long position (one-way mode)".
    let doc: Value = serde_json::from_str(
        r#"{"category": "linear", "symbol": "BTCUSDT", "side": "Sell", "orderType": "Limit", "qty": "1", "price": "30000", "timeInForce": "GTC", "positionIdx": 0, "orderLinkId": "usdt-test-02", "reduceOnly": true}"#,
    )
    .unwrap();
    let m = Bybit::linear().unwrap();
    let mut o = order(&m, "usdt-test-02", Side::Sell, OrderType::Limit, "1", Some("30000"));
    o.opts.reduce_only = true;
    let req = m.place(&settings(), &o, &placing("usdt-test-02", PositionSide::Net)).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed), (Method::Post, "/v5/order/create", true));
    assert_eq!(body(&req), doc);
}

#[test]
fn a_stop_names_its_trigger_and_the_direction_its_side_implies() {
    let m = Bybit::linear().unwrap();
    let buy = m.place(&settings(), &order(&m, "s1", Side::Buy, OrderType::Stop, "0.01", Some("31000")), &placing("s1", PositionSide::Net)).unwrap();
    let b = body(&buy);
    assert_eq!((b["orderType"].as_str(), b["triggerPrice"].as_str(), b["triggerDirection"].as_i64(), b["triggerBy"].as_str()), (Some("Market"), Some("31000"), Some(1), Some("LastPrice")));
    assert!(b.get("timeInForce").is_none(), "a market order always works IOC: no time in force sent");
    let sell = m.place(&settings(), &order(&m, "s2", Side::Sell, OrderType::StopLimit, "0.01", Some("29000")), &placing("s2", PositionSide::Short)).unwrap();
    let b = body(&sell);
    assert_eq!((b["orderType"].as_str(), b["price"].as_str(), b["triggerDirection"].as_i64(), b["positionIdx"].as_i64()), (Some("Limit"), Some("29000"), Some(2), Some(2)));
}

#[test]
fn a_spot_market_order_is_sized_in_the_base_coin_and_spot_takes_no_stop_or_reduce_only() {
    let m = Bybit::spot().unwrap();
    let req = m.place(&settings(), &order(&m, "m1", Side::Buy, OrderType::Market, "0.01", None), &placing("m1", PositionSide::Net)).unwrap();
    let b = body(&req);
    assert_eq!((b["category"].as_str(), b["marketUnit"].as_str(), b["qty"].as_str()), (Some("spot"), Some("baseCoin"), Some("0.01")));
    assert!(b.get("positionIdx").is_none() && b.get("reduceOnly").is_none());
    assert!(m.place(&settings(), &order(&m, "x", Side::Buy, OrderType::Stop, "0.01", Some("1")), &placing("x", PositionSide::Net)).is_err());
    let mut ro = order(&m, "r", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    ro.opts.reduce_only = true;
    assert!(m.place(&settings(), &ro, &placing("r", PositionSide::Net)).is_err());
    // Inverse and options take no orders here.
    for other in [Bybit::inverse().unwrap(), Bybit::option().unwrap()] {
        assert!(other.place(&settings(), &order(&other, "z", Side::Buy, OrderType::Limit, "1", Some("1")), &placing("z", PositionSide::Net)).is_err());
    }
}

#[test]
fn the_pre_check_serves_linear_plain_orders_only() {
    let lin = Bybit::linear().unwrap();
    let plain = lin.dry_run(&settings(), &order(&lin, "p", Side::Buy, OrderType::Limit, "1", Some("25000")), &placing("p", PositionSide::Net)).unwrap().unwrap();
    assert_eq!(plain.path, "/v5/order/pre-check");
    assert!(lin.dry_run(&settings(), &order(&lin, "p", Side::Buy, OrderType::Stop, "1", Some("25000")), &placing("p", PositionSide::Net)).unwrap().is_none());
    let spot = Bybit::spot().unwrap();
    assert!(spot.dry_run(&settings(), &order(&spot, "p", Side::Buy, OrderType::Limit, "1", Some("25000")), &placing("p", PositionSide::Net)).unwrap().is_none());
    assert_eq!(lin.decode_dry_run(PRE_CHECK.as_bytes()).unwrap(), "initial margin rate 0.3% to 3.57%, maintenance margin rate 0.21% to 2.94%");
}

#[test]
fn a_testnet_site_is_the_sandbox() {
    let m = Bybit::linear().unwrap();
    let mut s = settings();
    assert!(!m.is_sandbox(&s));
    s.insert("site".into(), "testnet".into());
    assert!(m.is_sandbox(&s));
    let req = m.cancel(&s, "BTCUSDT", "o1", CorrId(1)).unwrap();
    assert_eq!(req.base, "testnet");
}

/// The signature of an order body: the documented plain text (`timestamp + key + recv_window +
/// body`), signed by an independent HMAC-SHA256 (Python's `hmac` module, computed once and
/// pinned here) over the exact bytes this mapper sends.
#[test]
fn an_order_body_signs_like_an_independent_implementation() {
    let m = Bybit::linear().unwrap();
    let mut o = order(&m, "usdt-test-02", Side::Sell, OrderType::Limit, "1", Some("30000"));
    o.opts.reduce_only = true;
    let req = m.place(&settings(), &o, &placing("usdt-test-02", PositionSide::Net)).unwrap();
    let sent = String::from_utf8(req.body.clone().unwrap().to_vec()).unwrap();
    assert_eq!(sent, SIGNED_BODY);
    let signer = BybitSigner::from_secrets(&settings()).unwrap();
    let mut s = SignableRequest { method: Method::Post, path: req.path.clone(), query: vec![], headers: vec![], body: req.body.clone() };
    signer.sign(&mut s, otw_connector_model::Timestamp::from_millis(1_672_211_928_338).unwrap()).unwrap();
    let sign = s.headers.iter().find(|(k, _)| k == "X-BAPI-SIGN").map(|(_, v)| v.clone());
    assert_eq!(sign.as_deref(), Some(SIGNED_BODY_HMAC));
    // Content-Type is set once: by the mapper's POST, the signer adds none.
    let mut s = SignableRequest { method: req.method, path: req.path.clone(), query: req.query.clone(), headers: req.headers.clone(), body: req.body.clone() };
    signer.sign(&mut s, otw_connector_model::Timestamp::from_millis(1_672_211_928_338).unwrap()).unwrap();
    assert_eq!(s.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1);
}

const SIGNED_BODY: &str = r#"{"category":"linear","orderLinkId":"usdt-test-02","orderType":"Limit","positionIdx":0,"price":"30000","qty":"1","reduceOnly":true,"side":"Sell","symbol":"BTCUSDT","timeInForce":"GTC"}"#;
const SIGNED_BODY_HMAC: &str = "794b5b1a9358632a807c20f7c143beac0195fa325b47a7ecb008091c17e0099a";

// ── Answers ──

#[test]
fn documented_answers_decode() {
    let m = Bybit::linear().unwrap();
    let ack = m.decode_ack(CREATE.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("1321003749386327552", Some("spot-test-postonly")));
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "x", 200, CANCEL)).is_none());
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "x", 200, CANCEL_ALL)).is_none());
    let (open, next) = m.decode_orders(OPEN.as_bytes()).unwrap();
    let o = &open[0];
    assert_eq!((o.client_id.as_deref(), o.status.clone(), o.qty, o.filled_qty, o.limit, o.position), (Some("test-000005"), Mapped::Known(OrderStatus::New), d("0.10"), d("0"), Some(d("1600.00")), PositionSide::Long));
    assert!(next.is_some());
    let (hist, _) = m.decode_orders(HISTORY.as_bytes()).unwrap();
    assert_eq!(hist[0].status, Mapped::Known(OrderStatus::Cancelled));
    let (fills, _) = m.decode_fills(EXECUTIONS.as_bytes()).unwrap();
    assert_eq!((fills[0].exec_id.as_str(), fills[0].qty, fills[0].price, fills[0].fee), ("e0cbe81d-0f18-5866-9415-cf319b5dab3b", d("0.1"), d("1190.15"), d("0.071409")));
    let pos = Bybit::inverse().unwrap().decode_positions(POSITIONS.as_bytes()).unwrap();
    assert_eq!((pos[0].side, pos[0].direction, pos[0].qty), (PositionSide::Net, Some(Side::Sell), d("300")));
}

#[test]
fn a_spot_conditional_order_is_followed_and_cancelled_under_its_order_filter() {
    let spot = Bybit::spot().unwrap();
    let rows = |kind: &str| with(OPEN, |v| v["result"]["list"][0]["stopOrderType"] = json!(kind));
    let id = |kind: &str| spot.decode_orders(rows(kind).as_bytes()).unwrap().0[0].venue_id.clone();
    let plain = first_row(OPEN)["orderId"].as_str().unwrap().to_string();
    assert_eq!(id(""), plain);
    assert_eq!(id("UNKNOWN"), plain);
    assert_eq!(id("tpslOrder"), format!("cond:tpslOrder:{plain}"));
    assert_eq!(id("Stop"), format!("cond:StopOrder:{plain}"));
    let c = spot.cancel(&settings(), "BTCUSDT", "cond:tpslOrder:42", CorrId(1)).unwrap();
    assert_eq!(body(&c), json!({ "category": "spot", "symbol": "BTCUSDT", "orderId": "42", "orderFilter": "tpslOrder" }));
    assert_eq!(body(&spot.cancel(&settings(), "BTCUSDT", "42", CorrId(1)).unwrap()), json!({ "category": "spot", "symbol": "BTCUSDT", "orderId": "42" }));
    let q = spot.query_by_venue_id(&settings(), "BTCUSDT", "cond:StopOrder:42", CorrId(1)).unwrap();
    assert!(q[0].query.contains(&("orderId".to_string(), "42".to_string())));
    // Linear ids stay the venue's: its cancel takes no filter.
    let lin = Bybit::linear().unwrap();
    assert_eq!(lin.decode_orders(rows("Stop").as_bytes()).unwrap().0[0].venue_id, plain);
}

#[test]
fn documented_instrument_lists_decode_to_sound_rules() {
    let (lin, _) = Bybit::linear().unwrap().decode_rules(LINEAR.as_bytes()).unwrap();
    let r = &lin[0];
    assert_eq!((r.tick, r.lot_step, r.min_qty, r.max_qty, r.market_max_qty, r.min_notional), (Some(d("0.10")), Some(d("0.001")), Some(d("0.001")), Some(d("1190")), Some(d("500")), Some(d("5"))));
    assert_eq!(r.status, TradingStatus::Trading);
    r.sound().unwrap();
    let (spot, _) = Bybit::spot().unwrap().decode_rules(SPOT.as_bytes()).unwrap();
    let r = &spot[0];
    // Spot: basePrecision steps the size, minOrderAmt bounds the value; minOrderQty is no
    // longer checked by Bybit, so it is not either.
    assert_eq!((r.tick, r.lot_step, r.min_qty, r.min_notional, r.max_qty, r.market_max_qty), (Some(d("0.1")), Some(d("0.000001")), None, Some(d("5")), Some(d("83")), Some(d("41.5"))));
    r.sound().unwrap();
}

#[test]
fn every_status_word_maps_and_an_unknown_one_stays_unknown() {
    let m = Bybit::spot().unwrap();
    for (w, s) in [
        ("New", OrderStatus::New),
        ("PartiallyFilled", OrderStatus::PartiallyFilled),
        ("Untriggered", OrderStatus::New),
        ("Rejected", OrderStatus::Rejected),
        ("PartiallyFilledCanceled", OrderStatus::Cancelled),
        ("Filled", OrderStatus::Filled),
        ("Cancelled", OrderStatus::Cancelled),
        ("Triggered", OrderStatus::New),
        ("Deactivated", OrderStatus::Cancelled),
    ] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    assert_eq!(m.status_of("Created"), Mapped::Unknown("Created".into()));
    // `Untriggered` is New with the trigger still pending; `Triggered` clears it.
    let row = |w: &str| with(OPEN, |v| v["result"]["list"][0]["orderStatus"] = json!(w));
    let lin = Bybit::linear().unwrap();
    let waiting = &lin.decode_orders(row("Untriggered").as_bytes()).unwrap().0[0];
    assert_eq!((waiting.status.clone(), waiting.trigger_pending), (Mapped::Known(OrderStatus::New), true));
    let fired = &lin.decode_orders(row("Triggered").as_bytes()).unwrap().0[0];
    assert_eq!((fired.status.clone(), fired.trigger_pending), (Mapped::Known(OrderStatus::New), false));
    let off = &lin.decode_orders(row("Deactivated").as_bytes()).unwrap().0[0];
    assert_eq!(off.status, Mapped::Known(OrderStatus::Cancelled));
}

#[test]
fn a_delivering_instrument_is_halted_not_close_only() {
    let lin = Bybit::linear().unwrap();
    let body = with(LINEAR, |v| v["result"]["list"][0]["status"] = json!("Delivering"));
    let r = &lin.decode_rules(body.as_bytes()).unwrap().0[0];
    assert_eq!(r.status, TradingStatus::Closed("Delivering".into()));
}

#[test]
fn the_private_stream_maps_the_documented_frames() {
    // The order example is an option order: the linear book leaves it out, the option one
    // would read it. The execution example is linear.
    let lin = Bybit::linear().unwrap();
    assert!(lin.decode_private(WS_ORDER.as_bytes()).unwrap().is_empty());
    let ev = lin.decode_private(WS_EXECUTION.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.exec_id.as_str(), f.qty, f.price, f.fee_currency.as_str()), ("0ab1bdf7-4219-438b-b30a-32ec863018f7", d("0.5"), d("95900.1"), "USDT"));
    let opt = Bybit::option().unwrap();
    let ev = opt.decode_private(WS_ORDER.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.status.clone(), o.filled_qty), (Mapped::Known(OrderStatus::Filled), d("1")));
}

#[test]
fn errors_map_to_their_kind() {
    let m = Bybit::linear().unwrap();
    let e = |code: i64| m.decode_error(&otw_connector_testkit::http_frame(1, "order_create", 200, &format!(r#"{{"retCode":{code},"retMsg":"m"}}"#))).unwrap();
    assert_eq!(e(110007).canon, Canon::Rejected(Refusal::Funds));
    assert_eq!(e(110001).canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(e(110072).canon, Canon::Unknown, "a duplicate orderLinkId: looked up, never resent");
    assert_eq!(e(10006).canon, Canon::RateLimited);
    assert!(!e(123456).mapped);
}

// ── The catalog ──

/// The catalog's answers: the documented examples with their ids, sizes and states changed
/// (`synthetic`, built on the documented shape).
struct Script {
    m: Bybit,
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

fn first_row(template: &str) -> Value {
    let v: Value = serde_json::from_str(template).unwrap();
    v["result"]["list"][0].clone()
}

fn envelope(code: i64, msg: &str) -> String {
    json!({ "retCode": code, "retMsg": msg, "result": {}, "retExtInfo": {}, "time": 1672211918471_i64 }).to_string()
}

impl VenueScript for Script {
    type M = Bybit;

    fn mapper(&self) -> Bybit {
        self.m.clone()
    }

    fn settings(&self) -> HashMap<String, String> {
        settings()
    }

    fn signer(&self) -> Arc<dyn Signer> {
        Arc::new(BybitSigner::from_secrets(&settings()).unwrap())
    }

    fn symbol(&self) -> &'static str {
        "BTCUSDT"
    }

    fn qty(&self) -> Decimal {
        d("0.01")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        let src = if self.m.category() == super::super::Category::Spot { SPOT } else { LINEAR };
        match closed {
            None => src.to_string(),
            Some(w) => with(src, |v| v["result"]["list"][0]["status"] = json!(w)),
        }
    }

    fn closed_word(&self) -> &'static str {
        "PreLaunch"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::PartiallyFilled => "PartiallyFilled",
            OrderStatus::Filled => "Filled",
            OrderStatus::Cancelled => "Cancelled",
            OrderStatus::Rejected => "Rejected",
            _ => "New",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        with(CREATE, |v| {
            v["result"]["orderId"] = json!(venue_id);
            v["result"]["orderLinkId"] = json!(client_id);
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut o = first_row(OPEN);
                o["orderId"] = json!(r.venue_id);
                o["orderLinkId"] = json!(r.client_id.unwrap_or(""));
                o["symbol"] = json!("BTCUSDT");
                o["side"] = json!(if r.side == Side::Buy { "Buy" } else { "Sell" });
                o["orderStatus"] = json!(r.word);
                o["qty"] = json!(r.qty.to_string());
                o["cumExecQty"] = json!(r.filled.to_string());
                o["leavesQty"] = json!((r.qty - r.filled).to_string());
                o["positionIdx"] = json!(0);
                o
            })
            .collect();
        with(OPEN, |v| {
            v["result"]["list"] = json!(list);
            v["result"]["nextPageCursor"] = json!("");
            v["result"]["category"] = json!(self.m.category().name());
        })
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut f = first_row(EXECUTIONS);
                f["execId"] = json!(r.exec_id);
                f["orderId"] = json!(r.venue_id);
                f["orderLinkId"] = json!(r.client_id);
                f["symbol"] = json!("BTCUSDT");
                f["execQty"] = json!(r.qty.to_string());
                f["execPrice"] = json!(r.price.to_string());
                f["execFee"] = json!(r.fee.to_string());
                f["feeCurrency"] = json!("USDT");
                f
            })
            .collect();
        with(EXECUTIONS, |v| {
            v["result"]["list"] = json!(list);
            v["result"]["nextPageCursor"] = json!("");
        })
    }

    fn fee_currency(&self) -> &'static str {
        "USDT"
    }

    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|(side, dir, qty)| {
                let mut p = first_row(POSITIONS);
                p["symbol"] = json!("BTCUSDT");
                p["positionIdx"] = json!(match side {
                    PositionSide::Net => 0,
                    PositionSide::Long => 1,
                    PositionSide::Short => 2,
                });
                p["side"] = json!(match dir {
                    Some(Side::Buy) => "Buy",
                    Some(Side::Sell) => "Sell",
                    None => "",
                });
                p["size"] = json!(qty.to_string());
                p
            })
            .collect();
        with(POSITIONS, |v| {
            v["result"]["list"] = json!(list);
            v["result"]["category"] = json!("linear");
        })
    }

    fn ok(&self) -> String {
        CANCEL.to_string()
    }

    fn dry_run_ok(&self) -> String {
        PRE_CHECK.to_string()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        let spot = self.m.category() == super::super::Category::Spot;
        match r {
            VenueRefusal::Funds => Answer::new(200, if spot { envelope(170131, "Balance insufficient") } else { envelope(110007, "Available balance is insufficient") }),
            // The doc names no code for a reduce-only order that would add (Doc gaps): Bybit's
            // "orderQty will be truncated to zero" is what one with nothing to reduce gets.
            VenueRefusal::ReduceOnly => Answer::new(200, envelope(110017, "orderQty will be truncated to zero")),
            VenueRefusal::NotFound => Answer::new(200, if spot { envelope(170213, "Order does not exist.") } else { envelope(110001, "Order does not exist") }),
            VenueRefusal::Final => Answer::new(200, if spot { envelope(170139, "Order has been filled.") } else { envelope(110008, "The order has been completed or cancelled.") }),
            VenueRefusal::RateLimited(s) => Answer {
                status: 200,
                body: envelope(10006, "Too many visits!"),
                headers: vec![("X-Bapi-Limit-Reset-Timestamp", (CATALOG_NOW_MS + i64::try_from(s).unwrap() * 1000).to_string())],
            },
            VenueRefusal::ClockSkew => Answer::new(200, envelope(10002, "invalid request, please check your server timestamp or recv_window param")),
            VenueRefusal::Unmapped => Answer::new(200, envelope(999_999, "something new")),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            // The doc lists no code for it: a parameter error is what Bybit can answer.
            VenueRefusal::PositionModeMismatch => Answer::new(200, envelope(10001, "params error: position idx not match position mode")),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_passes_on_spot() {
    let r = run_catalog(&Script { m: Bybit::spot().unwrap() }).await;
    println!("spot ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    assert!(r.ran.len() >= 25, "{:?}", r.ran);
}

#[tokio::test]
async fn the_trading_catalog_passes_on_bybit_eu() {
    let r = run_catalog(&Script { m: Bybit::eu_spot().unwrap() }).await;
    assert!(r.ran.len() >= 25, "{:?}", r.ran);
}

#[tokio::test]
async fn the_trading_catalog_passes_on_linear() {
    let r = run_catalog(&Script { m: Bybit::linear().unwrap() }).await;
    println!("linear ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    assert!(r.ran.contains(&"E2D session check") && r.ran.contains(&"T16"), "{:?}", r.ran);
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live instrument lists decode to sound rules for every symbol, and an order call signed
/// with a key Bybit never issued, or not signed at all, is answered with the error the table
/// expects (and nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers bybit::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    for m in [Bybit::spot().unwrap(), Bybit::linear().unwrap()] {
        let label = m.capabilities().label.clone();
        let d = Driver::new(m.clone(), DriverConfig::new("bybit"), reqwest::Client::new());
        let junk = settings();
        let signer = BybitSigner::from_secrets(&junk).unwrap();
        let a = Account { settings: &junk, signer: &signer, scope: "net" };
        let trader: Trader<Bybit> = Trader::new();
        let rules = trader.all_rules(&d, &a).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(rules.len() > 10, "{label}: {} instruments", rules.len());
        for r in &rules {
            r.sound().unwrap_or_else(|e| panic!("{label}: {e}"));
            assert!(r.tick.is_some() && r.lot_step.is_some(), "{label}: {} states no tick or step", r.instrument.native);
        }
        let btc = rules.iter().find(|r| r.instrument.native == "BTCUSDT").unwrap_or_else(|| panic!("{label}: no BTCUSDT"));
        println!("{label}: {} instruments, BTCUSDT {btc:?}", rules.len());
        // An order no account could fill: the venue must stop at the key.
        let o = order(&m, "otw-net-probe", Side::Buy, OrderType::Limit, "0.001", Some("1000"));
        let req = m.place(&junk, &o, &placing("otw-net-probe", PositionSide::Net)).unwrap();
        // Seen 2026-11-11: a key Bybit never issued is `10003` (Auth); no key at all is
        // `10001` "apiKey is missing", a parameter error in the table, not an auth code.
        for (what, signed, want) in [("junk key", true, Canon::Auth), ("unsigned", false, Canon::InvalidRequest)] {
            let mut r = req.clone();
            r.signed = signed;
            let frame = d.send_as(r, Some(&signer)).await.unwrap_or_else(|e| panic!("{label} {what}: {e}"));
            let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label} {what}: Bybit accepted an order with no valid key"));
            println!("{label} {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
            assert_eq!((e.canon, e.mapped), (want, true), "{label} {what}: {e:?}");
        }
    }
}

// ── Review findings (independent doc review, 2026-11-11) ──

#[test]
fn a_reduce_only_stop_closes_on_trigger_and_a_reduce_only_limit_does_not() {
    let m = Bybit::linear().unwrap();
    let mut sl = order(&m, "sl", Side::Sell, OrderType::Stop, "0.01", Some("29000"));
    sl.opts.reduce_only = true;
    let b = body(&m.place(&settings(), &sl, &placing("sl", PositionSide::Net)).unwrap());
    assert_eq!((b["reduceOnly"].as_bool(), b["closeOnTrigger"].as_bool()), (Some(true), Some(true)));
    let mut tp = order(&m, "tp", Side::Sell, OrderType::Limit, "0.01", Some("31000"));
    tp.opts.reduce_only = true;
    assert!(body(&m.place(&settings(), &tp, &placing("tp", PositionSide::Net)).unwrap()).get("closeOnTrigger").is_none());
}

#[test]
fn a_market_order_fill_or_kill_is_refused_before_io() {
    for m in [Bybit::spot().unwrap(), Bybit::linear().unwrap()] {
        let mut o = order(&m, "f", Side::Buy, OrderType::Market, "0.01", None);
        o.tif = TimeInForce::Fok;
        assert_eq!(m.check_order(&o).unwrap_err().kind, otw_connector_model::ErrorKind::Unsupported);
        o.tif = TimeInForce::Ioc;
        assert!(m.check_order(&o).is_ok());
        o.tif = TimeInForce::Gtc;
        assert!(m.check_order(&o).is_ok());
    }
    let lin = Bybit::linear().unwrap();
    let mut stop = order(&lin, "s", Side::Buy, OrderType::Stop, "0.01", Some("31000"));
    stop.tif = TimeInForce::Fok;
    assert!(lin.check_order(&stop).is_err());
    let mut limit = order(&lin, "l", Side::Buy, OrderType::Limit, "0.01", Some("31000"));
    limit.tif = TimeInForce::Fok;
    assert!(lin.check_order(&limit).is_ok(), "a limit order takes FOK");
}

#[test]
fn a_dry_run_carries_no_client_id() {
    let m = Bybit::linear().unwrap();
    let req = m.dry_run(&settings(), &order(&m, "live-id", Side::Buy, OrderType::Limit, "1", Some("25000")), &placing("", PositionSide::Net)).unwrap().unwrap();
    assert!(body(&req).get("orderLinkId").is_none());
}

#[test]
fn a_cancel_all_that_says_success_0_failed() {
    let m = Bybit::linear().unwrap();
    assert!(m.decode_cancel_all(CANCEL_ALL.as_bytes()).is_ok());
    let failed = with(CANCEL_ALL, |v| v["result"]["success"] = json!("0"));
    assert!(m.decode_cancel_all(failed.as_bytes()).is_err());
}

#[test]
fn executions_that_move_a_position_without_an_order_are_kept_and_funding_is_not() {
    let m = Bybit::linear().unwrap();
    let kind = |t: &str| with(EXECUTIONS, |v| v["result"]["list"][0]["execType"] = json!(t));
    for t in ["AdlTrade", "BustTrade", "Delivery", "MovePosition", "SomethingNew"] {
        let (f, _) = m.decode_fills(kind(t).as_bytes()).unwrap();
        assert!(f.len() == 1 && f[0].position_only, "{t}");
    }
    assert!(!m.decode_fills(kind("Trade").as_bytes()).unwrap().0[0].position_only);
    assert!(m.decode_fills(kind("Funding").as_bytes()).unwrap().0.is_empty());
}

#[test]
fn the_new_codes_map_as_the_review_found() {
    let m = Bybit::linear().unwrap();
    let e = |code: i64| m.decode_error(&otw_connector_testkit::http_frame(1, "order_create", 200, &format!(r#"{{"retCode":{code},"retMsg":"m"}}"#))).unwrap().canon;
    assert_eq!(e(170195), Canon::Unknown);
    assert_eq!(e(170197), Canon::Rejected(Refusal::VenueRule));
    assert_eq!(e(30133), Canon::Rejected(Refusal::Other));
    assert_eq!(e(110039), Canon::Rejected(Refusal::Funds));
    assert_eq!(e(110071), Canon::Permission);
    assert_eq!(e(3100197), Canon::Permission);
    assert_eq!(e(170221), Canon::InvalidRequest);
    assert_eq!(e(110035), Canon::Unknown);
}

/// The trader walks a fills period in Bybit's seven-day windows, and an instrument missing from
/// the default list is looked for by status: listed as `Delivering`, it is not trading (refused,
/// not "unknown"), and its rules are not kept.
#[tokio::test]
async fn fills_walk_seven_day_windows_and_a_listed_instrument_not_trading_is_refused() {
    use otw_connector_engine::trading::{Account, TicketOptions, TradeError, Trader};
    use otw_connector_testkit::wiremock::matchers::{method, path, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let empty = r#"{"retCode":0,"retMsg":"OK","result":{"list":[],"nextPageCursor":""}}"#;
    Mock::given(method("GET")).and(path("/v5/execution/list")).respond_with(ResponseTemplate::new(200).set_body_string(empty)).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).and(query_param("status", "Delivering"))
        .respond_with(ResponseTemplate::new(200).set_body_string(with(LINEAR, |v| v["result"]["list"][0]["status"] = json!("Delivering"))))
        .with_priority(1).mount(&server).await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).respond_with(ResponseTemplate::new(200).set_body_string(empty)).mount(&server).await;
    let now = otw_connector_testkit::ts(1_767_225_600);
    let d = otw_connector_testkit::driver_against(Bybit::linear().unwrap(), &server, otw_connector_testkit::ManualClock::at(now));
    let s = settings();
    let signer = BybitSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "review" };
    let t: Trader<Bybit> = Trader::new();
    t.fills_since(&d, &a, "BTCUSDT", now.millis() - 20 * 86_400_000).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    let windows: Vec<(i64, i64)> = reqs
        .iter()
        .filter(|r| r.url.path() == "/v5/execution/list")
        .map(|r| {
            let q = |k: &str| r.url.query_pairs().find(|(n, _)| n == k).unwrap().1.parse::<i64>().unwrap();
            (q("startTime"), q("endTime"))
        })
        .collect();
    assert_eq!(windows.len(), 3, "{windows:?}");
    assert!(windows.iter().all(|(a, b)| b - a <= 7 * 86_400_000));
    assert_eq!(windows.last().unwrap().1, now.millis());
    let o = order(&Bybit::linear().unwrap(), "dl", Side::Buy, OrderType::Limit, "0.01", Some("30000"));
    for _ in 0..2 {
        let out = t.place(&d, &a, std::slice::from_ref(&o), &TicketOptions::default()).await;
        assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("Delivering")), "{out:?}");
    }
    let status_reads = server.received_requests().await.unwrap().iter().filter(|r| r.url.query_pairs().any(|(k, v)| k == "status" && v == "Delivering")).count();
    assert_eq!(status_reads, 2, "a non-trading instrument's rules are read again, not kept");
}

// ── Attached take profit and stop loss (linear, `tpslMode=Full`) ──

const TRADING_STOP: &str = include_str!("fixtures/trade/doc_trading_stop.json");

fn bracket(m: &Bybit) -> Vec<OrderRequest> {
    let entry = order(m, "br-e", Side::Buy, OrderType::Limit, "0.01", Some("30000"));
    let mut tp = order(m, "br-tp", Side::Sell, OrderType::Limit, "0.01", Some("31000"));
    tp.parent = Some("br-e".into());
    let mut sl = order(m, "br-sl", Side::Sell, OrderType::Stop, "0.01", Some("29000"));
    sl.parent = Some("br-e".into());
    vec![entry, tp, sl]
}

#[test]
fn an_entry_carries_its_take_profit_and_stop_loss_full_at_the_market() {
    let m = Bybit::linear().unwrap();
    let exits = [(otw_connector_sdk::trade::ExitKind::TakeProfit, d("31000")), (otw_connector_sdk::trade::ExitKind::StopLoss, d("29000"))];
    let req = m.place_with_exits(&settings(), &bracket(&m)[0], &exits, &placing("br-e", PositionSide::Net)).unwrap();
    let b = body(&req);
    assert_eq!(req.path, "/v5/order/create");
    assert_eq!(
        (b["tpslMode"].as_str(), b["takeProfit"].as_str(), b["stopLoss"].as_str(), b["tpOrderType"].as_str(), b["slOrderType"].as_str()),
        (Some("Full"), Some("31000"), Some("29000"), Some("Market"), Some("Market"))
    );
    assert_eq!((b["orderType"].as_str(), b["price"].as_str(), b["orderLinkId"].as_str()), (Some("Limit"), Some("30000"), Some("br-e")));
    // Not on spot, not on a reduce-only entry ("take profit/stop loss cannot be set"), not on a
    // conditional entry.
    let spot = Bybit::spot().unwrap();
    assert!(spot.place_with_exits(&settings(), &bracket(&spot)[0], &exits, &placing("x", PositionSide::Net)).is_err());
    let mut ro = bracket(&m)[0].clone();
    ro.opts.reduce_only = true;
    assert!(m.place_with_exits(&settings(), &ro, &exits, &placing("x", PositionSide::Net)).is_err());
    let stop = order(&m, "st", Side::Buy, OrderType::Stop, "0.01", Some("31000"));
    assert!(m.place_with_exits(&settings(), &stop, &exits, &placing("x", PositionSide::Net)).is_err());
}

#[test]
fn an_exit_changes_and_goes_through_set_trading_stop() {
    use otw_connector_sdk::trade::{ExitChange, ExitKind};
    let m = Bybit::linear().unwrap();
    let x = |position, kind, price| ExitChange { symbol: "BTCUSDT", position, entry_side: Side::Buy, kind, price, current: None };
    let set = body(&m.change_exit(&settings(), &x(PositionSide::Long, ExitKind::StopLoss, Some(d("29500"))), CorrId(1)).unwrap());
    assert_eq!(
        (set["tpslMode"].as_str(), set["stopLoss"].as_str(), set["slOrderType"].as_str(), set["positionIdx"].as_i64(), set.get("takeProfit").is_none()),
        (Some("Full"), Some("29500"), Some("Market"), Some(1), true)
    );
    let off = m.change_exit(&settings(), &x(PositionSide::Net, ExitKind::TakeProfit, None), CorrId(1)).unwrap();
    assert_eq!((off.path.as_str(), body(&off)["takeProfit"].as_str()), ("/v5/position/trading-stop", Some("0")));
    // The documented answer is a success.
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "trading_stop", 200, TRADING_STOP)).is_none());
}

#[test]
fn the_exits_a_position_tp_sl_creates_are_read_as_attached_to_their_entry() {
    use otw_connector_sdk::trade::ExitKind;
    let m = Bybit::linear().unwrap();
    // `synthetic`: the open-orders example as a TP/SL child (stopOrderType, parentOrderLinkId).
    let child = with(OPEN, |v| {
        let o = &mut v["result"]["list"][0];
        o["stopOrderType"] = json!("StopLoss");
        o["parentOrderLinkId"] = json!("br-e");
        o["orderStatus"] = json!("Untriggered");
    });
    let o = &m.decode_orders(child.as_bytes()).unwrap().0[0];
    assert_eq!((o.exit, o.parent_client_id.as_deref(), o.trigger_pending), (Some(ExitKind::StopLoss), Some("br-e"), true));
    let plain = &m.decode_orders(OPEN.as_bytes()).unwrap().0[0];
    assert_eq!((plain.exit, plain.parent_client_id.as_deref()), (None, None));
}

/// Through the trader: one create call for the three legs; the exits are followed through their
/// entry (working with it until it fills, then the venue's own conditional order); a ticket
/// whose exits do not close the entry is refused before I/O.
#[tokio::test]
async fn a_bracket_goes_out_in_one_call_and_its_exits_follow_the_entry() {
    use otw_connector_engine::trading::{Account, TicketOptions, TradeError, Trader};
    use otw_connector_model::OrderStatus as S;
    use otw_connector_testkit::wiremock::matchers::{method, path, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v5/market/instruments-info")).respond_with(ResponseTemplate::new(200).set_body_string(LINEAR)).mount(&server).await;
    let flat = with(POSITIONS, |v| {
        let p = &mut v["result"]["list"][0];
        p["symbol"] = json!("BTCUSDT");
        p["positionIdx"] = json!(0);
        p["size"] = json!("0");
        p["side"] = json!("");
    });
    Mock::given(method("GET")).and(path("/v5/position/list")).respond_with(ResponseTemplate::new(200).set_body_string(flat)).mount(&server).await;
    let ack = with(CREATE, |v| {
        v["result"]["orderId"] = json!("o-entry");
        v["result"]["orderLinkId"] = json!("br-e");
    });
    Mock::given(method("POST")).and(path("/v5/order/create")).respond_with(ResponseTemplate::new(200).set_body_string(ack)).mount(&server).await;
    let s = settings();
    let signer = BybitSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "bracket" };
    let d = otw_connector_testkit::driver_against(Bybit::linear().unwrap(), &server, otw_connector_testkit::ManualClock::at(otw_connector_testkit::ts(1_767_225_600)));
    let t: Trader<Bybit> = Trader::new();
    let m = Bybit::linear().unwrap();
    let legs = bracket(&m);
    let placed = t.place(&d, &a, &legs, &TicketOptions::default()).await.unwrap();
    let ids: Vec<&str> = placed.iter().map(|p| p.venue_id.as_str()).collect();
    assert_eq!(ids, vec!["o-entry", "tpsl:br-e:tp:buy", "tpsl:br-e:sl:buy"]);
    let creates = server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == "/v5/order/create").count();
    assert_eq!(creates, 1, "one call for the entry and its exits");
    // Entry working, unfilled: the stop loss works with it.
    let entry_row = |status: &str, filled: &str| {
        with(OPEN, |v| {
            let o = &mut v["result"]["list"][0];
            o["orderId"] = json!("o-entry");
            o["orderLinkId"] = json!("br-e");
            o["symbol"] = json!("BTCUSDT");
            o["orderStatus"] = json!(status);
            o["cumExecQty"] = json!(filled);
            o["positionIdx"] = json!(0);
        })
    };
    let lookup = Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "br-e")).respond_with(ResponseTemplate::new(200).set_body_string(entry_row("New", "0")));
    let guard = lookup.mount_as_scoped(&server).await;
    let x = t.exit_of(&a, "br-sl").expect("followed");
    let st = t.exit_status(&d, &a, &x).await.unwrap().unwrap();
    assert_eq!((st.status, st.trigger_pending), (Mapped::Known(S::New), true));
    drop(guard);
    // Filled: the venue's own stop loss, found by its entry's client id.
    Mock::given(method("GET")).and(path("/v5/order/realtime")).and(query_param("orderLinkId", "br-e")).respond_with(ResponseTemplate::new(200).set_body_string(entry_row("Filled", "0.01"))).with_priority(1).mount(&server).await;
    let child = with(OPEN, |v| {
        let o = &mut v["result"]["list"][0];
        o["orderId"] = json!("o-sl");
        o["orderLinkId"] = json!("");
        o["symbol"] = json!("BTCUSDT");
        o["stopOrderType"] = json!("StopLoss");
        o["parentOrderLinkId"] = json!("br-e");
        o["orderStatus"] = json!("Untriggered");
        o["positionIdx"] = json!(0);
        v["result"]["nextPageCursor"] = json!("");
    });
    Mock::given(method("GET")).and(path("/v5/order/realtime")).respond_with(ResponseTemplate::new(200).set_body_string(child)).with_priority(5).mount(&server).await;
    let st = t.exit_status(&d, &a, &x).await.unwrap().unwrap();
    assert_eq!((st.venue_id.as_str(), st.exit), ("o-sl", Some(otw_connector_sdk::trade::ExitKind::StopLoss)));
    // An exit of another size than the entry is refused before any order left.
    let mut bad = bracket(&m);
    bad[2].qty = otw_connector_model::Qty::new(Decimal::new(2, 2));
    bad[0].client_order_id = "br-x".into();
    bad[1].parent = Some("br-x".into());
    bad[2].parent = Some("br-x".into());
    let out = t.place(&d, &a, &bad, &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("of its size")), "{out:?}");
    let creates = server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == "/v5/order/create").count();
    assert_eq!(creates, 1);
}

// ── Bybit EU ──

fn eu_settings(site: &str) -> HashMap<String, String> {
    let mut s = settings();
    s.insert("site".into(), site.into());
    s
}

/// Bybit EU spot goes through the spot mapper on the EU host only: `api.bybit.eu`, its testnet
/// the sandbox, a bybit.com site refused; spot calls (no position, no reduce-only, no stop, no
/// pre-check); polled only (the doc names no EU private stream host).
#[test]
fn bybit_eu_orders_go_to_the_eu_host_on_spot_only() {
    let m = Bybit::eu_spot().unwrap();
    let o = order(&m, "eu1", Side::Buy, OrderType::Market, "0.01", None);
    let req = m.place(&eu_settings("eu"), &o, &placing("eu1", PositionSide::Net)).unwrap();
    let b = body(&req);
    assert_eq!((req.base, req.path.as_str(), b["category"].as_str(), b["marketUnit"].as_str()), ("eu", "/v5/order/create", Some("spot"), Some("baseCoin")));
    assert_eq!(m.place(&settings(), &o, &placing("eu1", PositionSide::Net)).unwrap().base, "eu", "the default site is eu");
    let t = m.place(&eu_settings("eu-testnet"), &o, &placing("eu1", PositionSide::Net)).unwrap();
    assert_eq!(t.base, "eu-testnet");
    assert!(m.is_sandbox(&eu_settings("eu-testnet")) && !m.is_sandbox(&eu_settings("eu")));
    assert!(m.place(&eu_settings("global"), &o, &placing("eu1", PositionSide::Net)).is_err(), "a bybit.com site is not an EU one");
    for site in ["eu", "eu-testnet"] {
        assert!(m.private_socket(&eu_settings(site)).is_none(), "{site}: polled only");
    }
    assert!(m.positions(&settings(), "BTCEUR", CorrId(1)).unwrap().is_none() && m.margin_mode(&settings(), CorrId(1)).unwrap().is_none());
    assert!(m.dry_run(&settings(), &order(&m, "p", Side::Buy, OrderType::Limit, "1", Some("25000")), &placing("p", PositionSide::Net)).unwrap().is_none());
    assert!(m.place(&settings(), &order(&m, "x", Side::Buy, OrderType::Stop, "0.01", Some("1")), &placing("x", PositionSide::Net)).is_err());
    let mut ro = order(&m, "r", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    ro.opts.reduce_only = true;
    assert!(m.place(&settings(), &ro, &placing("r", PositionSide::Net)).is_err());
    let caps = &m.capabilities().orders;
    assert_eq!(caps.level, otw_connector_model::ExecLevel::E2D);
    assert!(caps.position_mode.is_none() && !caps.reduce_only && !caps.brackets && !caps.types.contains(&OrderType::Stop), "{caps:?}");
    assert_eq!(m.instrument_rules(&eu_settings("eu"), Some("BTCEUR"), CorrId(1)).unwrap().base, "eu");
}

/// `extraFees`, EU only per the doc: a list on the stream, a string over REST. Each line is a
/// fee of its own beside `execFee`; an empty string is none; a REST string that is not the
/// documented list, or a line with no amount, is left out (never a guess at a fee) while the
/// fill itself is kept.
#[test]
fn bybit_eu_extra_fees_are_fee_lines_of_their_own() {
    let m = Bybit::eu_spot().unwrap();
    // `synthetic`: the documented execution frame on the spot category with an EU withholding tax line.
    let frame = with(WS_EXECUTION, |v| {
        v["data"][0]["category"] = json!("spot");
        v["data"][0]["extraFees"] = json!([{ "feeCoin": "EUR", "feeType": "WHT", "subFeeType": "", "feeRate": "0.001", "fee": "0.05" }]);
    });
    let ev = m.decode_private(frame.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!(f.extra_fees.len(), 1);
    assert_eq!((f.extra_fees[0].fee, f.extra_fees[0].currency.as_str(), f.extra_fees[0].kind.as_str()), (d("0.05"), "EUR", "WHT"));
    assert_eq!(f.fees(), vec![(d("26.3725275"), "USDT"), (d("0.05"), "EUR")]);
    // REST: the documented empty string, then the same list written as a string.
    let (rows, _) = m.decode_fills(EXECUTIONS.as_bytes()).unwrap();
    assert!(rows.iter().all(|f| f.extra_fees.is_empty() && !f.fee_pending));
    let listed = with(EXECUTIONS, |v| v["result"]["list"][0]["extraFees"] = json!(r#"[{"feeCoin":"EUR","feeType":"AUT_WITHHOLDING_TAX","subFeeType":"","feeRate":"0.001","fee":"0.02"}]"#));
    let (rows, _) = m.decode_fills(listed.as_bytes()).unwrap();
    assert_eq!((rows[0].extra_fees[0].fee, rows[0].extra_fees[0].kind.as_str()), (d("0.02"), "AUT_WITHHOLDING_TAX"));
    // An undocumented string: that fee is left out, the page and its fills are kept.
    let (all, _) = m.decode_fills(EXECUTIONS.as_bytes()).unwrap();
    let odd = with(EXECUTIONS, |v| v["result"]["list"][0]["extraFees"] = json!("WHT:0.02"));
    let (rows, _) = m.decode_fills(odd.as_bytes()).unwrap(); // an unreadable extra fee does not fail the page
    assert_eq!(rows.len(), all.len());
    assert_eq!((rows[0].exec_id.as_str(), rows[0].fee), (all[0].exec_id.as_str(), all[0].fee));
    assert!(rows[0].extra_fees.is_empty());
    // A line with no amount is not a fee of zero: that line is left out, the readable one kept.
    let blank = with(EXECUTIONS, |v| {
        v["result"]["list"][0]["extraFees"] = json!(r#"[{"feeCoin":"EUR","feeType":"WHT"},{"feeCoin":"EUR","feeType":"WHT","fee":"0.01"}]"#)
    });
    let (rows, _) = m.decode_fills(blank.as_bytes()).unwrap();
    assert_eq!(rows.len(), all.len());
    assert_eq!(rows[0].extra_fees.iter().map(|l| l.fee).collect::<Vec<_>>(), vec![d("0.01")]);
}

/// The global book reads the documented stream line too (the doc's own example carries a GST
/// line), as a fee of its own.
#[test]
fn the_documented_stream_extra_fee_line_is_kept() {
    let lin = Bybit::linear().unwrap();
    let ev = lin.decode_private(WS_EXECUTION.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.extra_fees[0].fee, f.extra_fees[0].currency.as_str(), f.extra_fees[0].kind.as_str()), (d("0.006403779"), "USDT", "GST IND_GST"));
}
