//! Kraken Futures orders, from the documentation alone. Fixtures under `fixtures/trade/` are the
//! derivatives OpenAPI's examples (https://docs.kraken.com/openapi/futures-rest.yaml, snapshot
//! read 2026-11-12, last-modified Mon, 09 Nov 2026 11:24:12 GMT), turned from YAML into JSON
//! value for value:
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_sendorder_placed.json` | `#/components/examples/PlacedOrderExample` (Send order, `placed`) |
//! | `doc_sendorder_rejected.json` | `#/components/examples/RejectedOrderExample` (Send order, `rejected`) |
//! | `doc_sendorder_executed.json` | `#/components/examples/ExecutedOrderExample` (Send order, `executed`) |
//! | `doc_editorder.json` | `#/components/examples/EditOrderExample` (Edit order) |
//! | `doc_cancelorder.json` | `#/components/examples/CancelOrderExample` (Cancel order) |
//! | `doc_cancelallorders.json` | `#/components/examples/CancelAllOrdersExample` (Cancel all orders) |
//! | `../doc_openorders.json`, `../doc_openpositions.json`, `../doc_instruments.json` | the account read's: `OpenOrdersExample`, `OpenPositionsSuccessExample`, `InstrumentsExample` |
//!
//! **Synthetic** (no example in the doc, built field for field on the documented schema):
//! the `POST /orders/status` answer (`OrderStatusDetailsJson`), the history API's executions
//! (`HistoricalExecutionElement`) and order events (`HistoricalOrderElement`), and every catalog
//! answer ([`Script`]): the doc examples above with their ids, sizes, prices, symbol and state
//! changed. The `Retry-After` header on the rate-limit answer is synthetic too (the doc states
//! no wait for `apiLimitExceeded`).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce, TriggerKind};
use otw_connector_sdk::trade::{Canon, CancelAllWay, ContractSizes, DecodeCtx, LookupAt, Placing, PositionMode, PositionSide, Refusal, TradeMapper, TradingStatus, VenueOrder};
use otw_connector_sdk::{HttpRequest, Method, SignableRequest, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript};
use secrecy::SecretString;
use serde_json::{json, Value};

use super::super::signer::{authent, KrakenFuturesSigner};
use super::super::KrakenFutures;

const PLACED: &str = include_str!("fixtures/trade/doc_sendorder_placed.json");
const EXECUTED: &str = include_str!("fixtures/trade/doc_sendorder_executed.json");
const CANCELLED: &str = include_str!("fixtures/trade/doc_cancelorder.json");
const CANCEL_ALL: &str = include_str!("fixtures/trade/doc_cancelallorders.json");
const OPEN: &str = include_str!("fixtures/doc_openorders.json");
const POSITIONS: &str = include_str!("fixtures/doc_openpositions.json");
const INSTRUMENTS: &str = include_str!("fixtures/doc_instruments.json");

/// The secret of the documented socket example (the REST guide's own is not base64).
const DOC_SECRET: &str = "7zxMEF5p/Z8l2p2U7Ghv6x14Af+Fx+92tPgUdVQ748FOIrEoT9bgT+bTRfXc5pz8na+hL/QdrCVG7bh9KpT0eMTm"; // gitleaks:allow
const SYM: &str = "PF_XBTUSD";

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn m() -> KrakenFutures {
    KrakenFutures::new().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "k"), ("api_secret", DOC_SECRET)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn signer() -> KrakenFuturesSigner {
    KrakenFuturesSigner::new(SecretString::from("k".to_string()), SecretString::from(DOC_SECRET.to_string())).unwrap()
}

fn order(symbol: &str, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>, stop: Option<&str>) -> OrderRequest {
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("KRAKEN_FUTURES", symbol),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: limit.map(|p| Price::new(d(p))),
        stop: stop.map(|p| Price::new(d(p))),
        tif: TimeInForce::Gtc,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts::default(),
    }
}

fn placing(cid: &str) -> Placing {
    Placing::new(cid, PositionSide::Net, CorrId(1))
}

/// The form body as pairs (the tests' values need no decoding).
fn form(req: &HttpRequest) -> Vec<(String, String)> {
    let body = String::from_utf8_lossy(req.body.as_deref().unwrap_or_default()).to_string();
    body.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap_or((p.to_string(), String::new()))).collect()
}

fn get<'a>(pairs: &'a [(String, String)], k: &str) -> Option<&'a str> {
    pairs.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

// ── Requests ──

/// Send order: a signed POST, the parameters in one form body (Content-Type sent once), the
/// size in the base coin, `cliOrdId` the OTW key; the body is what `Authent` hashes.
#[test]
fn a_limit_order_is_a_signed_form_post() {
    let req = m().place(&settings(), &order(SYM, "k1", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None), &placing("k1")).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.weight), (Method::Post, "/derivatives/api/v3/sendorder", true, 10));
    assert!(req.query.is_empty());
    assert_eq!(req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1);
    assert_eq!(String::from_utf8_lossy(req.body.as_deref().unwrap()), "orderType=lmt&symbol=PF_XBTUSD&side=buy&size=0.01&limitPrice=30000&cliOrdId=k1");
}

/// `Authent` over a POST: `postData` is the body as sent. Reference computed with Python's
/// hashlib and hmac over the same three steps (independent implementation):
/// `orderType=lmt&symbol=PF_XBTUSD&side=buy&size=0.01&limitPrice=30000&cliOrdId=k1` ++
/// `1415957147987` ++ `/api/v3/sendorder` gives `7MW3NjevpeJ8...87aCSg==`.
#[test]
fn a_post_hashes_its_body_and_the_trimmed_path() {
    use base64::Engine as _;
    let secret = base64::engine::general_purpose::STANDARD.decode(DOC_SECRET).unwrap();
    let body = "orderType=lmt&symbol=PF_XBTUSD&side=buy&size=0.01&limitPrice=30000&cliOrdId=k1";
    assert_eq!(
        authent(&secret, &format!("{body}1415957147987/api/v3/sendorder")).unwrap(),
        "7MW3NjevpeJ8Bm/kAZ00pcAH2JQZHGTimBXVC9FROR5W0KTlk0+M7xqji/n75IdA3sPXYNci4DQU7Xzv87aCSg=="
    );
    let req = m().place(&settings(), &order(SYM, "k1", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None), &placing("k1")).unwrap();
    let mut s = SignableRequest { method: req.method, path: req.path.clone(), query: req.query.clone(), headers: req.headers.clone(), body: req.body.clone() };
    signer().sign(&mut s, otw_connector_model::Timestamp::from_millis(1_415_957_147_987).unwrap()).unwrap();
    let h = |k: &str| s.headers.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(h("Authent"), authent(&secret, &format!("{body}{}/api/v3/sendorder", h("Nonce"))).unwrap());
    assert_eq!(s.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1);
}

#[test]
fn every_declared_type_and_tif_has_its_order_type() {
    let k = m();
    let cases: Vec<(OrderRequest, &str, Option<&str>, Option<&str>)> = vec![
        (order(SYM, "a", Side::Buy, OrderType::Market, "0.01", None, None), "mkt", None, None),
        (OrderRequest { tif: TimeInForce::Ioc, ..order(SYM, "b", Side::Sell, OrderType::Market, "0.01", None, None) }, "mkt", None, None),
        (order(SYM, "c", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None), "lmt", Some("30000"), None),
        (OrderRequest { opts: OrderOpts { post_only: true, ..Default::default() }, ..order(SYM, "e", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None) }, "post", Some("30000"), None),
        (OrderRequest { tif: TimeInForce::Ioc, ..order(SYM, "f", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None) }, "ioc", Some("30000"), None),
        (OrderRequest { tif: TimeInForce::Fok, ..order(SYM, "g", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None) }, "fok", Some("30000"), None),
        (order(SYM, "h", Side::Sell, OrderType::Stop, "0.01", None, Some("29000")), "stp", None, Some("29000")),
        (order(SYM, "i", Side::Sell, OrderType::StopLimit, "0.01", Some("28900"), Some("29000")), "stp", Some("28900"), Some("29000")),
        (order(SYM, "j", Side::Sell, OrderType::Mit, "0.01", None, Some("31000")), "take_profit", None, Some("31000")),
        (order(SYM, "l", Side::Sell, OrderType::Lit, "0.01", Some("30900"), Some("31000")), "take_profit", Some("30900"), Some("31000")),
    ];
    for (o, kind, limit, stop) in cases {
        k.check_order(&o).unwrap();
        let f = form(&k.place(&settings(), &o, &placing(&o.client_order_id)).unwrap());
        assert_eq!(get(&f, "orderType"), Some(kind), "{o:?}");
        assert_eq!((get(&f, "limitPrice"), get(&f, "stopPrice")), (limit, stop), "{o:?}");
        assert_eq!(get(&f, "cliOrdId"), Some(o.client_order_id.as_str()));
        assert_eq!(get(&f, "side"), Some(if o.side == Side::Buy { "buy" } else { "sell" }));
        assert!(get(&f, "triggerSignal").is_none() && get(&f, "reduceOnly").is_none());
    }
}

#[test]
fn a_trigger_and_reduce_only_are_sent_as_asked() {
    let k = m();
    for (t, w) in [(TriggerKind::Mark, "mark"), (TriggerKind::Last, "last"), (TriggerKind::Index, "index")] {
        let o = OrderRequest { opts: OrderOpts { trigger: Some(t), reduce_only: true, ..Default::default() }, ..order(SYM, "t", Side::Sell, OrderType::Stop, "0.01", None, Some("29000")) };
        k.check_order(&o).unwrap();
        let f = form(&k.place(&settings(), &o, &placing("t")).unwrap());
        assert_eq!((get(&f, "triggerSignal"), get(&f, "reduceOnly")), (Some(w), Some("true")));
    }
    let bid = OrderRequest { opts: OrderOpts { trigger: Some(TriggerKind::BidAsk), ..Default::default() }, ..order(SYM, "t", Side::Sell, OrderType::Stop, "0.01", None, Some("29000")) };
    assert!(k.check_order(&bid).is_err());
}

/// Refused before I/O: an inverse or vanilla contract, a combination the venue's words cannot
/// carry. Every declared one goes out.
#[test]
fn what_the_venue_cannot_carry_is_refused_before_io() {
    let k = m();
    for sym in ["PI_XBTUSD", "FI_XBTUSD_220930", "FV_XRPXBT_180615", "OF_XBTUSD_1234"] {
        let o = order(sym, "x", Side::Buy, OrderType::Limit, "1", Some("30000"), None);
        let e = k.check_order(&o).unwrap_err();
        assert!(e.message.contains("PF_, FF_"), "{}", e.message);
        assert!(k.place(&settings(), &o, &placing("x")).is_err());
    }
    assert!(k.check_order(&order("ff_xbtusd_261225", "x", Side::Buy, OrderType::Limit, "1", Some("30000"), None)).is_ok());
    let fok_market = OrderRequest { tif: TimeInForce::Fok, ..order(SYM, "x", Side::Buy, OrderType::Market, "0.01", None, None) };
    assert!(k.check_order(&fok_market).is_err());
    let stop_ioc = OrderRequest { tif: TimeInForce::Ioc, ..order(SYM, "x", Side::Buy, OrderType::Stop, "0.01", None, Some("1")) };
    assert!(k.check_order(&stop_ioc).is_err());
    assert!(k.client_id("0190b8f2-6b1a-7c3e-9d4f-1a2b3c4d5e6f").is_ok());
    assert!(k.client_id("a_b").is_err());
    assert!(k.client_id(&"a".repeat(101)).is_err());
}

/// Edit order: `orderId`, the new total size with `qtyMode=ABSOLUTE` always, prices; what the
/// order cannot take is refused before I/O.
#[test]
fn an_edit_sends_the_absolute_size_and_refuses_what_cannot_change() {
    let k = m();
    let cur = VenueOrder {
        venue_id: "022774bc".into(),
        client_id: Some("k".into()),
        symbol: SYM.into(),
        side: Mapped::Known(Side::Buy),
        order_type: "lmt".into(),
        status: Mapped::Known(OrderStatus::PartiallyFilled),
        qty: d("0.02"),
        filled_qty: d("0.005"),
        limit: Some(d("30000")),
        stop: None,
        avg_price: None,
        reduce_only: false,
        position: PositionSide::Net,
        created_ms: None,
        trigger_pending: false,
        parent_client_id: None,
        exit: None,
        successor_id: None,
    };
    let req = k.amend(&settings(), &cur, &OrderAmend { qty: Some(Qty::new(d("0.03"))), limit: Some(Price::new(d("29999"))), ..Default::default() }, &placing("k")).unwrap();
    assert_eq!((req.method, req.path.as_str()), (Method::Post, "/derivatives/api/v3/editorder"));
    assert_eq!(String::from_utf8_lossy(req.body.as_deref().unwrap()), "orderId=022774bc&size=0.03&limitPrice=29999&qtyMode=ABSOLUTE");
    let price_only = form(&k.amend(&settings(), &cur, &OrderAmend { limit: Some(Price::new(d("1"))), ..Default::default() }, &placing("k")).unwrap());
    assert_eq!(get(&price_only, "qtyMode"), Some("ABSOLUTE"));
    assert!(k.amend(&settings(), &cur, &OrderAmend { qty: Some(Qty::new(d("0.005"))), ..Default::default() }, &placing("k")).is_err(), "at or under what filled");
    assert!(k.amend(&settings(), &cur, &OrderAmend { stop: Some(Price::new(d("1"))), ..Default::default() }, &placing("k")).is_err(), "no trigger to change");
    assert!(k.amend(&settings(), &cur, &OrderAmend { tif: Some(TimeInForce::Ioc), ..Default::default() }, &placing("k")).is_err());
    let stop = VenueOrder { limit: None, stop: Some(d("29000")), order_type: "stop".into(), ..cur };
    assert!(k.amend(&settings(), &stop, &OrderAmend { limit: Some(Price::new(d("1"))), ..Default::default() }, &placing("k")).is_err(), "a stop-market has no limit to change");
    assert!(k.amend(&settings(), &stop, &OrderAmend { stop: Some(Price::new(d("28000"))), ..Default::default() }, &placing("k")).is_ok());
}

#[test]
fn cancel_cancel_all_and_lookups_name_the_documented_calls() {
    let k = m();
    let c = k.cancel(&settings(), SYM, "cb4e34f6", CorrId(1)).unwrap();
    assert_eq!((c.method, c.path.as_str(), form(&c)), (Method::Post, "/derivatives/api/v3/cancelorder", vec![("order_id".to_string(), "cb4e34f6".to_string())]));
    assert_eq!(k.cancel_all_way(), CancelAllWay::Book);
    let all = k.cancel_all(&settings(), &[], CorrId(1)).unwrap();
    assert_eq!((all.len(), all[0].path.as_str(), all[0].body.is_none(), all[0].weight), (1, "/derivatives/api/v3/cancelallorders", true, 25));
    k.decode_cancel_all(CANCEL_ALL.as_bytes()).unwrap();
    k.decode_cancel_all(br#"{"result":"success","cancelStatus":{"status":"noOrdersToCancel","cancelOnly":"all","cancelledOrders":[],"orderEvents":[],"receivedTime":"t"}}"#).unwrap();
    // By client id, the order made at T: the live call, then the history from a minute before T.
    let at = LookupAt { created_ms: Some(1_767_225_600_000), now_ms: 1_767_225_700_000 };
    let q = k.query_by_client_id_at(&settings(), SYM, "k1", &at, CorrId(1)).unwrap();
    assert_eq!(q.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/derivatives/api/v3/orders/status", "/api/history/v3/orders", "/api/history/v3/executions"]);
    assert_eq!(form(&q[0]), vec![("cliOrdIds".to_string(), "k1".to_string())]);
    for h in &q[1..] {
        assert!(h.signed && h.method == Method::Get);
        for p in [("tradeable", SYM), ("sort", "desc"), ("count", "1000"), ("since", "1767225540000")] {
            assert!(h.query.contains(&(p.0.to_string(), p.1.to_string())), "{p:?} in {:?}", h.query);
        }
    }
    let v = k.query_by_venue_id(&settings(), SYM, "v1", CorrId(1)).unwrap();
    assert_eq!(form(&v[0]), vec![("orderIds".to_string(), "v1".to_string())]);
    assert!(v[1].query.iter().all(|(q, _)| q != "since"));
    let f = k.fills_since(&settings(), SYM, 1, 2, CorrId(1)).unwrap();
    assert_eq!(f.path, "/api/history/v3/executions");
    assert!(f.query.contains(&("sort".into(), "asc".into())) && f.query.contains(&("tradeable".into(), SYM.into())));
    let next = k.with_cursor(&k.with_cursor(&f, "a"), "b");
    assert_eq!(next.query.iter().filter(|(q, _)| q == "continuation_token").map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["b"]);
    let r = k.instrument_rules(&settings(), Some(SYM), CorrId(1)).unwrap();
    assert!(!r.signed && r.query.is_empty() && k.counts_contracts(), "every type: the inverse sizes are read too");
    assert!(k.is_sandbox(&[("env".to_string(), "demo".to_string())].into()) && !k.is_sandbox(&settings()));
}

// ── Answers ──

#[test]
fn the_acks_name_the_order() {
    let k = m();
    assert_eq!(k.decode_ack(PLACED.as_bytes()).unwrap().venue_id, "179f9af8-e45e-469d-b3e9-2fd4675cb7d0");
    assert_eq!(k.decode_ack(EXECUTED.as_bytes()).unwrap().venue_id, "61ca5732-3478-42fe-8362-abbfd9465294");
    let none = k.decode_ack(br#"{"result":"success","sendStatus":{"status":"placed"}}"#).unwrap();
    assert!(!none.has_id());
}

/// The rules: tick, the size step from the precision, the venue's state words; an inverse
/// contract is listed closed and never sent an order.
#[test]
fn rules_come_from_the_documented_list() {
    let (rows, next) = m().decode_rules(INSTRUMENTS.as_bytes()).unwrap();
    assert!(next.is_none());
    let pf = rows.iter().find(|r| r.instrument.native == SYM).unwrap();
    assert_eq!((pf.tick, pf.lot_step, pf.min_qty, pf.max_qty, &pf.status), (Some(d("1")), Some(d("0.0001")), None, None, &TradingStatus::Trading));
    let pi = rows.iter().find(|r| r.instrument.native == "PI_XBTUSD").unwrap();
    assert!(matches!(&pi.status, TradingStatus::Closed(w) if w.contains("futures_inverse")), "{:?}", pi.status);
    // An inverse contract carries its size (USD) for its open orders; a traded one none.
    assert_eq!((pi.contract_size, pf.contract_size), (Some(d("1")), None));
    let flag = |k: &str, v: Value| with(INSTRUMENTS, |x| x["instruments"][2][k] = v);
    let st = |body: String| m().decode_rules(body.as_bytes()).unwrap().0.into_iter().find(|r| r.instrument.native == SYM).unwrap().status;
    assert_eq!(st(flag("suspended", json!(true))), TradingStatus::Closed("suspended".into()));
    assert_eq!(st(flag("postOnly", json!(true))), TradingStatus::PostOnly("postOnly".into()));
    assert_eq!(st(flag("isExpired", json!(true))), TradingStatus::Closed("expired".into()));
    assert!(matches!(st(flag("contractSize", json!(10))), TradingStatus::Closed(_)));
    let neg = m().decode_rules(flag("contractValueTradePrecision", json!(-2)).as_bytes()).unwrap().0;
    assert_eq!(neg.iter().find(|r| r.instrument.native == SYM).unwrap().lot_step, Some(d("100")));
    for r in &rows {
        r.sound().unwrap();
    }
}

/// Open orders: the documented list; every row is working, limit and trigger orders alike.
#[test]
fn open_orders_are_read_from_the_documented_list() {
    let (rows, _) = m().decode_orders_in(&DecodeCtx::new("openorders", &ContractSizes::default()), OPEN.as_bytes()).unwrap();
    assert_eq!(rows.len(), 7);
    let r = &rows[0];
    assert_eq!((r.venue_id.as_str(), r.side.clone(), r.qty, r.filled_qty, r.limit, r.reduce_only), ("59302619-41d2-4f0b-941f-7e7914760ad3", Mapped::Known(Side::Sell), d("304"), d("0"), Some(d("10640")), true));
    assert_eq!(r.status, Mapped::Known(OrderStatus::New));
    assert!(!r.trigger_pending);
    let stop = with(OPEN, |v| {
        v["openOrders"][0]["orderType"] = json!("stop");
        v["openOrders"][0]["stopPrice"] = json!(9000);
        v["openOrders"][0]["status"] = json!("partiallyFilled");
        v["openOrders"][0]["filledSize"] = json!(4);
    });
    let (rows, _) = m().decode_orders(stop.as_bytes()).unwrap();
    assert_eq!((rows[0].stop, rows[0].trigger_pending, rows[0].status.clone(), rows[0].qty), (Some(d("9000")), true, Mapped::Known(OrderStatus::PartiallyFilled), d("308")));
}

/// An inverse contract's open orders are in coins at their own price, as the account read
/// states them: `size × contractSize / price`, the size read from the contract list.
#[test]
fn inverse_open_orders_are_in_coins_as_on_the_read_path() {
    let k = m();
    let mut sizes = ContractSizes::new();
    for r in k.decode_rules(INSTRUMENTS.as_bytes()).unwrap().0 {
        if let Some(c) = r.contract_size {
            sizes.insert(&r.instrument.native, c);
        }
    }
    let (rows, _) = k.decode_orders_in(&DecodeCtx::new("openorders", &sizes), OPEN.as_bytes()).unwrap();
    // 304 USD contracts of 1 USD at 10640.
    assert_eq!((rows[0].qty, rows[0].filled_qty), (d("304").checked_div(d("10640")).unwrap(), d("0")));
    // A trigger with no limit is counted at its trigger; a linear row is left as it is.
    let mixed = with(OPEN, |v| {
        v["openOrders"][0]["limitPrice"] = Value::Null;
        v["openOrders"][0]["stopPrice"] = json!(8000);
        v["openOrders"][0]["filledSize"] = json!(96);
        v["openOrders"][1]["symbol"] = json!(SYM);
        v["openOrders"][1]["unfilledSize"] = json!(0.5);
    });
    let (rows, _) = k.decode_orders_in(&DecodeCtx::new("openorders", &sizes), mixed.as_bytes()).unwrap();
    assert_eq!((rows[0].qty, rows[0].filled_qty), (d("0.05"), d("0.012")));
    assert_eq!(rows[1].qty, d("0.5"));
    // No size read: kept as the venue states it, so cancel all still sees it.
    let (rows, _) = k.decode_orders_in(&DecodeCtx::new("openorders", &ContractSizes::default()), OPEN.as_bytes()).unwrap();
    assert_eq!((rows.len(), rows[0].qty), (7, d("304")));
}

/// One `orders/status` row: type, order id, client id, status word, quantity, filled.
type StatusRowSpec<'a> = (&'a str, &'a str, Option<&'a str>, &'a str, &'a str, &'a str);

/// Synthetic `orders/status` answer (`OrderStatusDetailsJson` fields): each status word.
fn status_answer(rows: &[StatusRowSpec<'_>]) -> String {
    let orders: Vec<Value> = rows
        .iter()
        .map(|(kind, id, cid, status, qty, filled)| {
            json!({
                "order": {
                    "type": kind, "orderId": id, "cliOrdId": cid, "symbol": SYM, "side": "buy",
                    "quantity": qty.parse::<f64>().unwrap(), "filled": filled.parse::<f64>().unwrap(), "limitPrice": 30000, "reduceOnly": false,
                    "timestamp": "2026-11-12T10:00:00.000Z", "lastUpdateTimestamp": "2026-11-12T10:00:00.000Z", "algoId": null,
                    "priceTriggerOptions": if *kind == "TRIGGER_ORDER" { json!({"triggerPrice": 29000, "triggerSide": "TRIGGER_BELOW", "triggerSignal": "MARK_PRICE"}) } else { Value::Null },
                    "triggerTime": null
                },
                "status": status, "updateReason": null, "error": null
            })
        })
        .collect();
    json!({ "result": "success", "serverTime": "2026-11-12T10:00:01.000Z", "orders": orders }).to_string()
}

#[test]
fn the_live_lookup_maps_every_status_word() {
    let body = status_answer(&[
        ("ORDER", "a", Some("ka"), "ENTERED_BOOK", "0.02", "0"),
        ("ORDER", "b", Some("kb"), "ENTERED_BOOK", "0.02", "0.01"),
        ("ORDER", "c", Some("kc"), "FULLY_EXECUTED", "0.02", "0.02"),
        ("ORDER", "e", Some("ke"), "CANCELLED", "0.02", "0.01"),
        ("ORDER", "f", None, "REJECTED", "0.02", "0"),
        ("TRIGGER_ORDER", "g", Some("kg"), "TRIGGER_PLACED", "0.02", "0"),
        ("TRIGGER_ORDER", "h", Some("kh"), "TRIGGER_ACTIVATION_FAILURE", "0.02", "0"),
        ("ORDER", "i", Some("ki"), "SOMETHING_NEW", "0.02", "0"),
    ]);
    let (rows, _) = m().decode_orders_in(&DecodeCtx::new("orders_status", &ContractSizes::default()), body.as_bytes()).unwrap();
    let st: Vec<Mapped<OrderStatus>> = rows.iter().map(|r| r.status.clone()).collect();
    use OrderStatus::*;
    assert_eq!(
        st,
        vec![
            Mapped::Known(New),
            Mapped::Known(PartiallyFilled),
            Mapped::Known(Filled),
            Mapped::Known(Cancelled),
            Mapped::Known(Rejected),
            Mapped::Known(New),
            Mapped::Known(Rejected),
            Mapped::Unknown("SOMETHING_NEW".into())
        ]
    );
    assert_eq!((rows[5].stop, rows[5].trigger_pending, rows[0].created_ms), (Some(d("29000")), true, Some(1_794_477_600_000)));
    assert_eq!((rows[4].client_id.clone(), rows[1].filled_qty), (None, d("0.01")));
}

/// Synthetic history executions (`HistoricalExecutionElement` fields).
fn executions(rows: &[(&str, &str, &str, &str, &str, &str)], token: Option<&str>) -> String {
    let elements: Vec<Value> = rows
        .iter()
        .map(|(uid, order, cid, qty, price, order_qty)| {
            json!({
                "uid": format!("e-{uid}"), "timestamp": 1_767_225_601_000_i64,
                "event": { "execution": { "execution": {
                    "uid": uid,
                    "order": {
                        "uid": order, "accountUid": "a", "tradeable": SYM, "direction": "Buy", "quantity": order_qty, "filled": "0",
                        "timestamp": 1_767_225_600_500_i64, "limitPrice": "30000", "orderType": "Limit", "clientId": cid, "reduceOnly": false,
                        "lastUpdateTimestamp": 1_767_225_601_000_i64
                    },
                    "timestamp": 1_767_225_601_000_i64, "quantity": qty, "price": price, "markPrice": "30001", "executionType": "taker",
                    "limitFilled": false, "usdValue": "300", "orderData": { "fee": "0.15", "positionSize": "0.01", "feeCalculationInfo": { "percentageFee": "0.05", "userFeeDiscountApplied": null, "marketShareRebateCredited": null } }
                }, "takerReducedQuantity": "" } }
            })
        })
        .collect();
    let mut v = json!({ "accountUid": "a", "len": elements.len(), "elements": elements });
    if let Some(t) = token {
        v["continuationToken"] = json!(t);
    }
    v.to_string()
}

/// Fills: the history executions, the fee as written and no currency (the doc states none);
/// a lookup reads an order no longer listed live from its executions.
#[test]
fn executions_are_fills_and_settle_an_order_no_longer_listed() {
    let k = m();
    let body = executions(&[("x1", "o1", "k1", "0.01", "30000", "0.02"), ("x2", "o1", "k1", "0.01", "30002", "0.02"), ("x3", "o2", "k2", "0.005", "30000", "0.02")], Some("dG9r"));
    let (fills, next) = k.decode_fills(body.as_bytes()).unwrap();
    assert_eq!(next.as_deref(), Some("dG9r"));
    assert_eq!(fills.len(), 3);
    let f = &fills[0];
    assert_eq!((f.exec_id.as_str(), f.venue_order_id.as_str(), f.client_id.as_deref(), f.qty, f.price, f.fee, f.fee_currency.as_str(), f.at_ms, f.position_only), ("x1", "o1", Some("k1"), d("0.01"), d("30000"), d("0.15"), "", 1_767_225_601_000, false));
    assert_eq!(f.side, Mapped::Known(Side::Buy));
    let (orders, _) = k.decode_orders_in(&DecodeCtx::new("executions:lookup_executions", &ContractSizes::default()), body.as_bytes()).unwrap();
    assert_eq!(orders.len(), 2);
    assert_eq!((orders[0].venue_id.as_str(), orders[0].status.clone(), orders[0].filled_qty, orders[0].avg_price), ("o1", Mapped::Known(OrderStatus::Filled), d("0.02"), Some(d("30001"))));
    assert_eq!((orders[1].status.clone(), orders[1].filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.005")));
    // A liquidation moved the position without an order of the account.
    let liq = body.replace("\"orderType\":\"Limit\"", "\"orderType\":\"Liquidation\"");
    assert!(k.decode_fills(liq.as_bytes()).unwrap().0.iter().all(|f| f.position_only));
    // An inverse contract's executions are not read as fills.
    let inv = body.replace(SYM, "PI_XBTUSD");
    assert!(k.decode_fills(inv.as_bytes()).unwrap().0.is_empty());
}

/// The next page's token where the history answer carries it in the `Next-Continuation-Token`
/// header only (`synthetic`): read from the header, on the fills read and the lookups alike;
/// the body's token is read first when both are there.
#[test]
fn the_continuation_token_is_read_from_the_header_when_the_body_has_none() {
    let k = m();
    let sizes = ContractSizes::default();
    let headers = vec![("next-continuation-token".to_string(), "aGRy".to_string())];
    let bare = executions(&[("x1", "o1", "k1", "0.01", "30000", "0.02")], None);
    let ctx = |endpoint| DecodeCtx::new(endpoint, &sizes).with_headers(&headers);
    assert_eq!(k.decode_fills(bare.as_bytes()).unwrap().1, None, "no header handed, no token");
    assert_eq!(k.decode_fills_in(&ctx("executions"), bare.as_bytes()).unwrap().1.as_deref(), Some("aGRy"));
    assert_eq!(k.decode_orders_in(&ctx("executions:lookup_executions"), bare.as_bytes()).unwrap().1.as_deref(), Some("aGRy"));
    let orders = json!({ "accountUid": "a", "len": 1, "elements": [{ "uid": "e1", "timestamp": 1, "event": {} }] }).to_string();
    assert_eq!(k.decode_orders_in(&ctx("executions:lookup_orders"), orders.as_bytes()).unwrap().1.as_deref(), Some("aGRy"));
    let both = executions(&[("x1", "o1", "k1", "0.01", "30000", "0.02")], Some("Ym9keQ"));
    assert_eq!(k.decode_fills_in(&ctx("executions"), both.as_bytes()).unwrap().1.as_deref(), Some("Ym9keQ"));
    // An empty page ends the walk whatever the header says.
    let empty = json!({ "accountUid": "a", "len": 0, "elements": [] }).to_string();
    assert_eq!(k.decode_fills_in(&ctx("executions"), empty.as_bytes()).unwrap().1, None);
}

/// An order whose executions run over two pages (newest first): the page holding its last
/// execution says it filled whole by the order's own `filled`, not part filled and cancelled.
#[test]
fn an_order_filled_over_two_pages_is_filled() {
    let k = m();
    let page = |rows: &[(&str, &str, &str, &str, &str, &str)], filled: &str| {
        with(&executions(rows, Some("dG9r")), |v| {
            for e in v["elements"].as_array_mut().unwrap() {
                e["event"]["execution"]["execution"]["order"]["filled"] = json!(filled);
            }
        })
    };
    let ctx = ContractSizes::default();
    let first = page(&[("x3", "o1", "k1", "0.01", "30002", "0.03")], "0.03");
    let (orders, next) = k.decode_orders_in(&DecodeCtx::new("executions:lookup_executions", &ctx), first.as_bytes()).unwrap();
    assert_eq!(next.as_deref(), Some("dG9r"));
    assert_eq!((orders[0].status.clone(), orders[0].filled_qty, orders[0].avg_price), (Mapped::Known(OrderStatus::Filled), d("0.03"), None), "the page holds a third of its fills: no average");
    // The same order with its `filled` short of its size, its executions short too: part filled.
    let short = page(&[("x3", "o1", "k1", "0.01", "30002", "0.03")], "0.02");
    let (orders, _) = k.decode_orders_in(&DecodeCtx::new("executions:lookup_executions", &ctx), short.as_bytes()).unwrap();
    assert_eq!((orders[0].status.clone(), orders[0].filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.02")));
}

/// Every history read, the lookups' included, counts in the history pool (100 per 10 minutes):
/// once 100 lookup pages are read, a fill page waits for a refill (6 s a token).
#[tokio::test]
async fn the_lookups_count_in_the_history_pool() {
    use otw_connector_sdk::Limiter;
    use std::time::Duration;
    let k = m();
    let at = LookupAt { created_ms: Some(1_767_225_600_000), now_ms: 1_767_225_700_000 };
    let q = k.query_by_client_id_at(&settings(), SYM, "k1", &at, CorrId(1)).unwrap();
    let history: Vec<&HttpRequest> = q.iter().filter(|r| r.path.starts_with("/api/history")).collect();
    assert_eq!(history.len(), 2);
    let limiter = otw_connector_engine::limiter::TokenBucketLimiter::new(&otw_connector_sdk::Mapper::capabilities(&k).rate_limits);
    for i in 0..100 {
        let r = history[i % 2];
        assert!(tokio::time::timeout(Duration::from_millis(500), limiter.acquire(r.endpoint_id, r.weight)).await.is_ok(), "a full pool does not wait");
    }
    let fills = k.fills_since(&settings(), SYM, 1, 2, CorrId(1)).unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(200), limiter.acquire(fills.endpoint_id, fills.weight)).await.is_err(), "the pool is spent");
}

/// Synthetic history order events (`HistoricalOrderElement`): a cancel and a rejection settle an
/// order, a placement or an update does not.
#[test]
fn the_order_history_settles_a_cancel_or_a_rejection() {
    let order = |uid: &str, cid: &str, filled: &str| json!({"uid": uid, "accountUid": "a", "tradeable": SYM, "direction": "Sell", "quantity": "0.02", "filled": filled, "timestamp": 1_767_225_600_500_i64, "limitPrice": "31000", "orderType": "Limit", "clientId": cid, "reduceOnly": true, "lastUpdateTimestamp": 1_767_225_600_900_i64});
    let body = json!({ "accountUid": "a", "len": 3, "serverTime": "2026-11-12T10:00:00Z", "elements": [
        { "uid": "1", "timestamp": 1, "event": { "OrderCancelled": { "order": order("o1", "k1", "0.005"), "reason": "cancelled_by_user" } } },
        { "uid": "2", "timestamp": 2, "event": { "OrderRejected": { "order": order("o2", "k2", "0"), "orderError": "POST_WOULD_EXECUTE", "reason": "x" } } },
        { "uid": "3", "timestamp": 3, "event": { "OrderPlaced": { "order": order("o3", "k3", "0"), "reason": "new_placed_order_by_user", "reducedQuantity": "" } } }
    ]})
    .to_string();
    let (rows, _) = m().decode_orders_in(&DecodeCtx::new("executions:lookup_orders", &ContractSizes::default()), body.as_bytes()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].client_id.as_deref(), rows[0].status.clone(), rows[0].filled_qty, rows[0].side.clone()), (Some("k1"), Mapped::Known(OrderStatus::Cancelled), d("0.005"), Mapped::Known(Side::Sell)));
    assert_eq!(rows[1].status, Mapped::Known(OrderStatus::Rejected));
}

/// Positions: the documented answer; only the multi-collateral contract's row is one an order
/// acts on, with its leverage and margin as the row states them.
#[test]
fn positions_are_one_per_contract_with_their_leverage() {
    let k = m();
    let rows = k.decode_positions(POSITIONS.as_bytes()).unwrap();
    assert_eq!(rows.len(), 1);
    let p = &rows[0];
    assert_eq!((p.symbol.as_str(), p.side, p.direction, p.qty, p.leverage), ("PF_DEFIUSD", PositionSide::Net, Some(Side::Buy), d("1"), Some(d("5"))));
    assert_eq!(p.margin.as_ref().map(|m| m.word()), Some("isolated"));
    // Neither leverage field: cross with no cap ("Absent when the position is uncapped or
    // isolated"); `crossMaxLeverage`: cross at that cap.
    let uncapped = with(POSITIONS, |v| {
        v["openPositions"][2].as_object_mut().unwrap().remove("maxFixedLeverage");
    });
    let rows = k.decode_positions(uncapped.as_bytes()).unwrap();
    assert_eq!((rows[0].leverage, rows[0].margin.as_ref().map(|m| m.word())), (None, Some("cross")));
    let capped = with(POSITIONS, |v| {
        v["openPositions"][2].as_object_mut().unwrap().remove("maxFixedLeverage");
        v["openPositions"][2]["crossMaxLeverage"] = json!(10);
    });
    let rows = k.decode_positions(capped.as_bytes()).unwrap();
    assert_eq!((rows[0].leverage, rows[0].margin.as_ref().map(|m| m.word())), (Some(d("10")), Some("cross")));
    assert_eq!(k.decode_account_position_mode(POSITIONS.as_bytes()).unwrap(), Some(PositionMode::OneWay));
    assert_eq!(k.decode_account_position_mode(br#"{"result":"success","openPositions":[]}"#).unwrap(), Some(PositionMode::OneWay));
    // The list answers every contract: a read for one keeps its rows only (a close never
    // reads another contract's position).
    let both = with(POSITIONS, |v| {
        let mut p = v["openPositions"][2].clone();
        p["symbol"] = json!(SYM);
        p["side"] = json!("short");
        v["openPositions"].as_array_mut().unwrap().push(p);
    });
    assert_eq!(k.decode_positions(both.as_bytes()).unwrap().len(), 2);
    let req = k.positions(&settings(), SYM, CorrId(1)).unwrap().unwrap();
    let rows = k.decode_positions_in(&DecodeCtx::new(req.endpoint_id, &ContractSizes::default()), both.as_bytes()).unwrap();
    assert_eq!(rows.iter().map(|r| (r.symbol.as_str(), r.direction)).collect::<Vec<_>>(), [(SYM, Some(Side::Sell))]);
    let open = k.open_orders(&settings(), Some("PI_XBTUSD"), CorrId(1)).unwrap();
    assert_eq!(k.decode_orders_in(&DecodeCtx::new(open[0].endpoint_id, &ContractSizes::default()), OPEN.as_bytes()).unwrap().0.len(), 7);
    let open = k.open_orders(&settings(), Some(SYM), CorrId(1)).unwrap();
    assert!(k.decode_orders_in(&DecodeCtx::new(open[0].endpoint_id, &ContractSizes::default()), OPEN.as_bytes()).unwrap().0.is_empty());
}

// ── The catalog ──

/// The catalog's answers: the documented examples with their ids, sizes and states changed
/// (`synthetic`, built on the documented shapes). One `orders` answer serves the live lookup
/// (`orders`), the open list (`openOrders`) and the history reads (`elements`, empty).
struct Script;

fn word_of(st: OrderStatus) -> &'static str {
    match st {
        OrderStatus::New | OrderStatus::PartiallyFilled => "ENTERED_BOOK",
        OrderStatus::Filled => "FULLY_EXECUTED",
        OrderStatus::Cancelled | OrderStatus::Expired => "CANCELLED",
        _ => "REJECTED",
    }
}

fn status_body(word: &str) -> String {
    json!({ "result": "success", "serverTime": "2026-11-12T10:00:00.000Z", "sendStatus": { "status": word, "receivedTime": "2026-11-12T10:00:00.000Z", "orderEvents": [] } }).to_string()
}

fn cancel_body(word: &str) -> String {
    json!({ "result": "success", "serverTime": "2026-11-12T10:00:00.000Z", "cancelStatus": { "status": word, "receivedTime": "2026-11-12T10:00:00.000Z", "orderEvents": [] } }).to_string()
}

impl VenueScript for Script {
    type M = KrakenFutures;

    fn mapper(&self) -> KrakenFutures {
        m()
    }

    fn settings(&self) -> HashMap<String, String> {
        settings()
    }

    fn signer(&self) -> Arc<dyn Signer> {
        Arc::new(signer())
    }

    fn symbol(&self) -> &'static str {
        SYM
    }

    fn qty(&self) -> Decimal {
        d("0.01")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        match closed {
            None => INSTRUMENTS.to_string(),
            Some(_) => with(INSTRUMENTS, |v| v["instruments"][2]["suspended"] = json!(true)),
        }
    }

    fn closed_word(&self) -> &'static str {
        "suspended"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        word_of(s)
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        with(PLACED, |v| {
            v["sendStatus"]["order_id"] = json!(venue_id);
            v["sendStatus"]["cliOrdId"] = json!(client_id);
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let status: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "order": { "type": "ORDER", "orderId": r.venue_id, "cliOrdId": r.client_id, "symbol": SYM, "side": if r.side == Side::Buy { "buy" } else { "sell" },
                        "quantity": r.qty.to_string().parse::<f64>().unwrap(), "filled": r.filled.to_string().parse::<f64>().unwrap(), "limitPrice": 30000, "reduceOnly": false,
                        "timestamp": "2026-11-12T10:00:00.000Z", "lastUpdateTimestamp": "2026-11-12T10:00:00.000Z", "algoId": null, "priceTriggerOptions": null, "triggerTime": null },
                    "status": r.word, "updateReason": null, "error": null
                })
            })
            .collect();
        let template: Value = serde_json::from_str(OPEN).unwrap();
        let open: Vec<Value> = rows
            .iter()
            .filter(|r| r.word == "ENTERED_BOOK")
            .map(|r| {
                let mut o = template["openOrders"][0].clone();
                o["order_id"] = json!(r.venue_id);
                o["symbol"] = json!(SYM);
                o["side"] = json!(if r.side == Side::Buy { "buy" } else { "sell" });
                o["limitPrice"] = json!(30000);
                o["filledSize"] = json!(r.filled.to_string().parse::<f64>().unwrap());
                o["unfilledSize"] = json!((r.qty - r.filled).to_string().parse::<f64>().unwrap());
                o["status"] = json!(if r.filled.is_zero() { "untouched" } else { "partiallyFilled" });
                o["reduceOnly"] = json!(false);
                if let Some(c) = r.client_id {
                    o["cliOrdId"] = json!(c);
                }
                o
            })
            .collect();
        json!({ "result": "success", "serverTime": "2026-11-12T10:00:00.000Z", "orders": status, "openOrders": open, "elements": [] }).to_string()
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let mut v: Value = serde_json::from_str(&executions(&[], None)).unwrap();
        let one: Value = serde_json::from_str(&executions(&[("x", "o", "c", "0", "0", "0")], None)).unwrap();
        v["elements"] = json!(rows
            .iter()
            .map(|r| {
                let mut e = one["elements"][0].clone();
                let x = &mut e["event"]["execution"]["execution"];
                x["uid"] = json!(r.exec_id);
                x["order"]["uid"] = json!(r.venue_id);
                x["order"]["clientId"] = json!(r.client_id);
                x["quantity"] = json!(r.qty.to_string());
                x["price"] = json!(r.price.to_string());
                x["orderData"]["fee"] = json!(r.fee.to_string());
                e
            })
            .collect::<Vec<_>>());
        v.to_string()
    }

    /// The doc states no fee currency on an execution (PROVIDER.md, doc gaps): none is carried.
    fn fee_currency(&self) -> &'static str {
        ""
    }

    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        let template: Value = serde_json::from_str(POSITIONS).unwrap();
        let list: Vec<Value> = rows
            .iter()
            .map(|(side, dir, qty)| {
                let mut p = template["openPositions"][2].clone();
                p["symbol"] = json!(SYM);
                p["side"] = json!(if *side == PositionSide::Short || *dir == Some(Side::Sell) { "short" } else { "long" });
                p["size"] = json!(qty.to_string().parse::<f64>().unwrap());
                p
            })
            .collect();
        with(POSITIONS, |v| v["openPositions"] = json!(list))
    }

    fn ok(&self) -> String {
        CANCELLED.to_string()
    }

    fn dry_run_ok(&self) -> String {
        CANCELLED.to_string()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        match r {
            // The REST guide's own example of a refused order (Calls and Returns).
            VenueRefusal::Funds => Answer::new(200, r#"{"result":"success","serverTime":"2016-02-25T09:45:53.818Z","sendStatus":{"receivedTime":"2016-02-25T09:45:53.601Z","status":"insufficientAvailableFunds"}}"#),
            VenueRefusal::ReduceOnly => Answer::new(200, status_body("wouldNotReducePosition")),
            VenueRefusal::NotFound => Answer::new(200, cancel_body("notFound")),
            VenueRefusal::Final => Answer::new(200, cancel_body("filled")),
            // The rate limits guide's example, verbatim; the header is synthetic.
            VenueRefusal::RateLimited(s) => Answer {
                status: 200,
                body: r#"{"result":"error","serverTime":"2016-02-25T09:45:53.818Z","error":"apiLimitExceeded"}"#.into(),
                headers: vec![("Retry-After", s.to_string())],
            },
            VenueRefusal::ClockSkew => Answer::new(200, r#"{"result":"error","serverTime":"2016-02-25T09:45:53.818Z","error":"nonceBelowThreshold"}"#),
            VenueRefusal::Unmapped => Answer::new(200, status_body("somethingNew")),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            // No Kraken equivalent: one position per contract, no mode to switch.
            VenueRefusal::PositionModeMismatch => Answer::new(200, r#"{"result":"error","serverTime":"2026-11-12T00:00:00Z","error":"invalidArgument"}"#),
        }
    }
}

/// The catalog. Two checks are written for other venues' calls and fail here by construction:
/// - "position mode" expects an order refused when the rows tell no mode; Kraken Futures has
///   one position per contract, read at account level as one-way
///   (`a_flat_account_trades_one_way_after_one_read`).
/// - "unmapped" mounts two lookups on one mock told apart by their URL; Kraken's client id
///   travels in the form body of `orders/status`, so the second lookup reads the first one's
///   answer. Its two checks are run on their own (`an_unmapped_answer_alerts_and_an_unknown_word_stays_unknown`).
///
/// Every other failure fails this test.
#[tokio::test]
async fn the_trading_catalog_passes() {
    let out = tokio::spawn(async move { run_catalog(&Script).await }).await;
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
    assert_eq!(failed, vec!["unmapped".to_string(), "position mode".to_string()], "only the checks written for other venues' calls may fail");
}

// ── Through the trader, what the catalog does not mount ──

mod trader {
    use super::*;
    use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, Trader};
    use otw_connector_testkit::wiremock::matchers::{body_string_contains, method, path, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer, Request, ResponseTemplate};
    use otw_connector_testkit::{driver_against, ManualClock};

    const NOW_MS: i64 = 1_767_225_600_000;

    fn ok(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_string(body.to_string()).insert_header("content-type", "application/json")
    }

    async fn venue() -> (MockServer, otw_connector_sdk::Driver<KrakenFutures>) {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/instruments")).respond_with(ok(INSTRUMENTS)).mount(&server).await;
        let dv = driver_against(m(), &server, ManualClock::at(otw_connector_testkit::ts(NOW_MS / 1000)));
        (server, dv)
    }

    fn calls(reqs: &[Request], p: &str) -> usize {
        reqs.iter().filter(|r| r.url.path() == p).count()
    }

    /// Flat everywhere, the account still trades: one position per contract, read once at
    /// account level as one-way, never from the rows.
    #[tokio::test]
    async fn a_flat_account_trades_one_way_after_one_read() {
        let (server, dv) = venue().await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openpositions")).respond_with(ok(r#"{"result":"success","openPositions":[],"serverTime":"t"}"#)).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/sendorder")).respond_with(ok(PLACED)).mount(&server).await;
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "flat" };
        let t: Trader<KrakenFutures> = Trader::new();
        for k in ["f1", "f2"] {
            let placed = t.place(&dv, &a, &[order(SYM, k, Side::Buy, OrderType::Limit, "0.01", Some("30000"), None)], &TicketOptions::default()).await.unwrap();
            assert_eq!(placed[0].venue_id, "179f9af8-e45e-469d-b3e9-2fd4675cb7d0");
        }
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(calls(&reqs, "/derivatives/api/v3/openpositions"), 1, "the mode is read once");
        assert_eq!(calls(&reqs, "/derivatives/api/v3/sendorder"), 2);
        let sent = reqs.iter().find(|r| r.url.path() == "/derivatives/api/v3/sendorder").unwrap();
        assert!(sent.headers.get("authent").is_some() && sent.headers.get("apikey").is_some());
        assert_eq!(sent.headers.get_all("content-type").iter().count(), 1);
        // An inverse contract never reaches the venue.
        let out = t.place(&dv, &a, &[order("PI_XBTUSD", "f3", Side::Buy, OrderType::Limit, "1", Some("30000"), None)], &TicketOptions::default()).await;
        assert!(out.is_err());
        assert_eq!(calls(&server.received_requests().await.unwrap(), "/derivatives/api/v3/sendorder"), 2);
    }

    /// A lost answer looked up past the live call's 5 seconds: the history's executions say it
    /// filled; one that never filled and was cancelled is found by its cancel event. Each from
    /// the order's time, never sent again.
    #[tokio::test]
    async fn a_lost_order_is_found_in_the_history_after_five_seconds() {
        let (server, dv) = venue().await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openpositions")).respond_with(ok(r#"{"result":"success","openPositions":[]}"#)).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/sendorder")).respond_with(ResponseTemplate::new(502)).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/orders/status")).respond_with(ok(r#"{"result":"success","orders":[]}"#)).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/history/v3/executions"))
            .and(query_param("sort", "desc"))
            .respond_with(ok(&executions(&[("x1", "o-lost", "lost1", "0.01", "30000", "0.01")], None)))
            .mount(&server)
            .await;
        let cancelled = json!({ "accountUid": "a", "len": 1, "elements": [ { "uid": "1", "timestamp": 1, "event": { "OrderCancelled": { "order": {"uid": "o-gone", "tradeable": SYM, "direction": "Buy", "quantity": "0.01", "filled": "0", "timestamp": NOW_MS, "limitPrice": "30000", "orderType": "Limit", "clientId": "gone1", "reduceOnly": false, "lastUpdateTimestamp": NOW_MS}, "reason": "cancelled_by_user" } } } ] });
        Mock::given(method("GET")).and(path("/api/history/v3/orders")).respond_with(ok(&cancelled.to_string())).mount(&server).await;
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "lost" };
        let t: Trader<KrakenFutures> = Trader::new();
        let placed = t.place(&dv, &a, &[order(SYM, "lost1", Side::Buy, OrderType::Market, "0.01", None, None)], &TicketOptions::default()).await.unwrap();
        assert_eq!(placed[0].venue_id, "o-lost", "adopted from its executions");
        let got = t.lookup(&dv, &a, SYM, "lost1").await.unwrap().unwrap();
        assert_eq!((got.status, got.filled_qty), (Mapped::Known(OrderStatus::Filled), d("0.01")));
        let gone = t.lookup(&dv, &a, SYM, "gone1").await.unwrap().unwrap();
        assert_eq!(gone.status, Mapped::Known(OrderStatus::Cancelled));
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(calls(&reqs, "/derivatives/api/v3/sendorder"), 1, "never sent again");
        let hist = reqs.iter().find(|r| r.url.path() == "/api/history/v3/executions").unwrap();
        let since: i64 = hist.url.query_pairs().find(|(k, _)| k == "since").unwrap().1.parse().unwrap();
        assert_eq!(since, NOW_MS - 60_000, "from a minute before the order");
        let status = reqs.iter().find(|r| r.url.path() == "/derivatives/api/v3/orders/status").unwrap();
        assert_eq!(String::from_utf8_lossy(&status.body), "cliOrdIds=lost1");
    }

    /// A cancel that finds the order filled answers `notFound` (maker protection guide): the
    /// order is read and its real end said, no error. Cancel all reaches triggers too.
    #[tokio::test]
    async fn a_cancel_of_a_filled_order_and_cancel_all() {
        let (server, dv) = venue().await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/cancelorder")).respond_with(ok(&cancel_body("notFound"))).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/derivatives/api/v3/orders/status"))
            .and(body_string_contains("orderIds=v9"))
            .respond_with(ok(&status_answer(&[("ORDER", "v9", Some("k9"), "FULLY_EXECUTED", "0.01", "0.01")])))
            .mount(&server)
            .await;
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "cancel" };
        let t: Trader<KrakenFutures> = Trader::new();
        assert_eq!(t.cancel(&dv, &a, SYM, "v9").await.unwrap(), Cancelled::AlreadyFinal(OrderStatus::Filled));
        // Cancel all: one call for the book, the trigger order included; nothing left after.
        let open_once = with(OPEN, |v| {
            v["openOrders"] = json!([v["openOrders"][0].clone(), {"order_id": "trig-1", "symbol": SYM, "side": "sell", "orderType": "stop", "stopPrice": 29000, "unfilledSize": 0.01, "filledSize": 0, "status": "untouched", "reduceOnly": true, "receivedTime": "2026-11-12T10:00:00.000Z", "lastUpdateTime": "2026-11-12T10:00:00.000Z"}]);
        });
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openorders")).respond_with(ok(&open_once)).up_to_n_times(1).with_priority(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openorders")).respond_with(ok(r#"{"result":"success","openOrders":[]}"#)).with_priority(2).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/cancelallorders")).respond_with(ok(CANCEL_ALL)).mount(&server).await;
        assert_eq!(t.cancel_all(&dv, &a).await.unwrap(), 2);
        let reqs = server.received_requests().await.unwrap();
        let all: Vec<&Request> = reqs.iter().filter(|r| r.url.path() == "/derivatives/api/v3/cancelallorders").collect();
        assert_eq!(all.len(), 1);
        assert!(all[0].body.is_empty(), "no symbol: every contract");
    }

    /// A change goes out by `orderId` with the absolute size; `edited` is a success.
    #[tokio::test]
    async fn a_change_goes_out_in_place() {
        let (server, dv) = venue().await;
        Mock::given(method("POST"))
            .and(path("/derivatives/api/v3/orders/status"))
            .respond_with(ok(&status_answer(&[("ORDER", "022774bc-2c4a-4f26-9317-436c8d85746d", Some("k5"), "ENTERED_BOOK", "0.02", "0")])))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/editorder")).respond_with(ok(include_str!("fixtures/trade/doc_editorder.json"))).mount(&server).await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openpositions")).respond_with(ok(r#"{"result":"success","openPositions":[]}"#)).mount(&server).await;
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "edit" };
        let t: Trader<KrakenFutures> = Trader::new();
        let out = t.modify(&dv, &a, SYM, "022774bc-2c4a-4f26-9317-436c8d85746d", &OrderAmend { qty: Some(Qty::new(d("0.03"))), limit: Some(Price::new(d("29000"))), ..Default::default() }).await.unwrap();
        assert!(out.is_none());
        let reqs = server.received_requests().await.unwrap();
        let edit = reqs.iter().find(|r| r.url.path() == "/derivatives/api/v3/editorder").unwrap();
        assert_eq!(String::from_utf8_lossy(&edit.body), "orderId=022774bc-2c4a-4f26-9317-436c8d85746d&size=0.03&limitPrice=29000&qtyMode=ABSOLUTE");
        // A refused edit that may end the order: the order is read again once, and still
        // working at its old price, the change is refused with the venue's word.
        let (server, dv) = venue().await;
        Mock::given(method("POST"))
            .and(path("/derivatives/api/v3/orders/status"))
            .respond_with(ok(&status_answer(&[("ORDER", "v6", Some("k6"), "ENTERED_BOOK", "0.02", "0")])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/derivatives/api/v3/editorder"))
            .respond_with(ok(r#"{"result":"success","editStatus":{"status":"postWouldExecute","orderEvents":[]}}"#))
            .mount(&server)
            .await;
        let e = t.modify(&dv, &a, SYM, "v6", &OrderAmend { limit: Some(Price::new(d("31000"))), ..Default::default() }).await.unwrap_err();
        assert!(e.message().contains("still works unchanged") && e.message().contains("postWouldExecute"), "{}", e.message());
        assert!(matches!(e, otw_connector_engine::trading::TradeError::Refused(_)), "{e:?}");
        let reads = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/derivatives/api/v3/orders/status").count();
        assert_eq!(reads, 2, "read before the change, and once after its answer");
    }

    /// An answer the table does not map is unknown and alerts, the order looked up and never
    /// resent; an unknown status word stays unknown (the catalog's "unmapped", on its own).
    #[tokio::test]
    async fn an_unmapped_answer_alerts_and_an_unknown_word_stays_unknown() {
        let (server, dv) = venue().await;
        Mock::given(method("GET")).and(path("/derivatives/api/v3/openpositions")).respond_with(ok(r#"{"result":"success","openPositions":[]}"#)).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/sendorder")).respond_with(ok(&status_body("somethingNew"))).mount(&server).await;
        Mock::given(method("POST")).and(path("/derivatives/api/v3/orders/status")).and(body_string_contains("cliOrdIds=unmk")).respond_with(ok(r#"{"result":"success","orders":[]}"#)).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/derivatives/api/v3/orders/status"))
            .and(body_string_contains("cliOrdIds=unmw"))
            .respond_with(ok(&status_answer(&[("ORDER", "v-w", Some("unmw"), "NoSuchWord", "0.01", "0")])))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/api/history/v3/executions")).respond_with(ok(r#"{"accountUid":"a","len":0,"elements":[]}"#)).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/history/v3/orders")).respond_with(ok(r#"{"accountUid":"a","len":0,"elements":[]}"#)).mount(&server).await;
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "unm" };
        let t: Trader<KrakenFutures> = Trader::new();
        let out = t.place(&dv, &a, &[order(SYM, "unmk", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None)], &TicketOptions::default()).await;
        assert!(matches!(&out, Err(otw_connector_engine::trading::TradeError::Unknown { alert: true, keys, .. }) if keys == &vec!["unmk".to_string()]), "{out:?}");
        assert_eq!(calls(&server.received_requests().await.unwrap(), "/derivatives/api/v3/sendorder"), 1);
        let got = t.lookup(&dv, &a, SYM, "unmw").await.unwrap().unwrap();
        assert!(matches!(got.status, Mapped::Unknown(ref w) if w == "NoSuchWord"), "{:?}", got.status);
    }

    /// The refusals the catalog does not reach, each with its kind.
    #[test]
    fn refusals_carry_their_kind() {
        let k = m();
        let f = |body: &str| k.decode_error(&otw_connector_testkit::http_frame(1, "sendorder", 200, body)).unwrap();
        assert_eq!(f(&status_body("postWouldExecute")).canon, Canon::Rejected(Refusal::PostOnly));
        assert_eq!(f(&status_body("marketSuspended")).canon, Canon::Rejected(Refusal::VenueRule));
        assert_eq!(f(&status_body("marketIsCancelOnly")).canon, Canon::Rejected(Refusal::MarketClosed));
        assert_eq!(f(&status_body("outsidePriceCollar")).canon, Canon::Rejected(Refusal::Price));
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live contract list decodes to sound rules for every contract, and an order call with a
/// key Kraken never issued, or with no key, is answered with the auth error the table expects
/// (nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers kraken_futures::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    let k = m();
    let dv = Driver::new(k.clone(), DriverConfig::new("kraken_futures"), reqwest::Client::new());
    let junk = settings();
    let sg = signer();
    let a = Account { settings: &junk, signer: &sg, scope: "net" };
    let trader: Trader<KrakenFutures> = Trader::new();
    let rules = trader.all_rules(&dv, &a).await.unwrap();
    assert!(rules.len() > 5, "{} contracts", rules.len());
    for r in &rules {
        r.sound().unwrap();
        if r.status == TradingStatus::Trading {
            assert!(r.tick.is_some() && r.lot_step.is_some(), "{} states no tick or step", r.instrument.native);
        }
    }
    let trading = rules.iter().filter(|r| r.status == TradingStatus::Trading).count();
    println!("{} contracts, {trading} trading", rules.len());
    let btc = rules.iter().find(|r| r.instrument.native == SYM).unwrap();
    println!("{btc:?}");
    let o = order(SYM, "otwnetprobe", Side::Buy, OrderType::Limit, &btc.lot_step.unwrap().to_string(), Some(&btc.tick.unwrap().to_string()), None);
    let req = k.place(&junk, &o, &placing("otwnetprobe")).unwrap();
    for (what, signed) in [("junk key", true), ("unsigned", false)] {
        let mut r = req.clone();
        r.signed = signed;
        let frame = dv.send_as(r, Some(&sg)).await.unwrap();
        let e = k.decode_error(&frame).unwrap_or_else(|| panic!("{what}: Kraken Futures accepted an order with no valid key"));
        println!("{what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
        assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{what}: {e:?}");
    }
}
