//! Kraken spot orders, from the documentation alone. Fixtures under `fixtures/trade/`, copied
//! verbatim from the docs (snapshot read 2026-11-12: the spot REST OpenAPI
//! https://docs.kraken.com/openapi/spot-rest.yaml, last-modified Tue, 27 Oct 2026 13:47:31 GMT,
//! sha256 4e730d0c503de30a57dadd09fd07883af6aa3538e3bc4adabcd0e3576f1d0e1f; the pages of
//! https://docs.kraken.com/llms-full.txt for the socket):
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_add_order.json` | spot-rest.yaml `components/schemas/add-2` example (https://docs.kraken.com/api-reference/trading/add-order) |
//! | `doc_amend_order.json` | `components/schemas/amend-2` example (https://docs.kraken.com/api-reference/trading/amend-order) |
//! | `doc_cancel_order.json` | `components/schemas/cancel-2` example (https://docs.kraken.com/api-reference/trading/cancel-order) |
//! | `doc_open_orders.json` | `/private/OpenOrders` 200 example (https://docs.kraken.com/api-reference/account-data/get-open-orders) |
//! | `doc_closed_orders.json` | `/private/ClosedOrders` 200 example (https://docs.kraken.com/api-reference/account-data/get-closed-orders) |
//! | `doc_query_orders.json` | `/private/QueryOrders` 200 example (https://docs.kraken.com/api-reference/account-data/query-orders-info) |
//! | `doc_trades_history.json` | `/private/TradesHistory` 200 example (https://docs.kraken.com/api-reference/account-data/get-trades-history) |
//! | `doc_asset_pairs.json` | `/public/AssetPairs` 200 example (https://docs.kraken.com/api-reference/market-data/get-tradable-asset-pairs) |
//! | `doc_ws_token.json` | `/private/GetWebSocketsToken` 200 example (https://docs.kraken.com/api-reference/trading/get-websockets-token) |
//! | `doc_ws_subscribe.json`, `doc_ws_pending.json`, `doc_ws_live.json`, `doc_ws_execution.json` | https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/executions (Subscribe Response, Pending, Live Order, Execution) |
//! | `doc_ws_pong.json` | https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/ping (Response) |
//! | `doc_ws_heartbeat.json` | https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/heartbeat (Example) |
//!
//! The YAML examples were turned into JSON with their values unchanged. The catalog script
//! ([`Script`]) answers with these examples with only their ids, sizes, prices, states and
//! client ids changed: those answers are `synthetic`, built on the documented shape. Every
//! error body is `synthetic` too (the doc lists the strings, not whole bodies; the envelope is
//! the documented one), and so is the validate answer (the doc gives none).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{Capabilities, Command, CorrId, Decimal, Envelope, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce, TriggerKind};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, Canon, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, Page, Placing, PositionSide, PrivateEvent, PrivateSocket, Settings, SymbolNames, TradeMapper,
    TradingStatus, VenueError, VenueFill, VenueOrder, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript, CATALOG_NOW_MS};
use secrecy::SecretString;
use serde_json::{json, Value};

use super::super::signer::{sign, KrakenSigner};
use super::super::{Kraken, Session};
use super::{pack, spelled, unpack};

const ADD: &str = include_str!("fixtures/trade/doc_add_order.json");
const AMEND: &str = include_str!("fixtures/trade/doc_amend_order.json");
const CANCEL: &str = include_str!("fixtures/trade/doc_cancel_order.json");
const OPEN: &str = include_str!("fixtures/trade/doc_open_orders.json");
const CLOSED: &str = include_str!("fixtures/trade/doc_closed_orders.json");
const QUERY: &str = include_str!("fixtures/trade/doc_query_orders.json");
const TRADES: &str = include_str!("fixtures/trade/doc_trades_history.json");
const PAIRS: &str = include_str!("fixtures/trade/doc_asset_pairs.json");
const WS_TOKEN: &str = include_str!("fixtures/trade/doc_ws_token.json");
const WS_SUBSCRIBE: &str = include_str!("fixtures/trade/doc_ws_subscribe.json");
const WS_PENDING: &str = include_str!("fixtures/trade/doc_ws_pending.json");
const WS_LIVE: &str = include_str!("fixtures/trade/doc_ws_live.json");
const WS_EXECUTION: &str = include_str!("fixtures/trade/doc_ws_execution.json");
const WS_PONG: &str = include_str!("fixtures/trade/doc_ws_pong.json");
const WS_HEARTBEAT: &str = include_str!("fixtures/trade/doc_ws_heartbeat.json");

/// The Authentication guide's sample private key (a published example, not a secret).
const DOC_SECRET: &str = "kQH5HW/8p1uGOVjbgWA7FunAmGO8lsSUXNsu3eow76sz84Q18fWxnyRzBHCd3pd5nE9qa99HAZtuZuj6F1huXg==";
/// A robot's client id: 24 characters of `[a-z0-9]` (`robots/exec.rs`).
const ROBOT: &str = "otsa0123456789abcdef0123";

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "otw-test-key"), ("api_secret", DOC_SECRET)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn order(symbol: &str, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>, stop: Option<&str>) -> OrderRequest {
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("KRAKEN", symbol),
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

fn body(req: &HttpRequest) -> String {
    String::from_utf8(req.body.clone().unwrap().to_vec()).unwrap()
}

/// The form fields of a request body.
fn fields(req: &HttpRequest) -> HashMap<String, String> {
    body(req).split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.replace("%2C", ",").replace("%2F", "/"))).collect()
}

/// A mapper that has read the doc's pair list (its altname table filled).
fn mapper() -> Kraken {
    let m = Kraken::new().unwrap();
    m.decode_rules(PAIRS.as_bytes()).unwrap();
    m
}

fn names() -> SymbolNames {
    let mut n = SymbolNames::of(&mapper().decode_rules(PAIRS.as_bytes()).unwrap().0);
    // The doc's ClosedOrders example names pairs its AssetPairs example does not list.
    n.insert("XBTGBP", "XBT/GBP");
    n.insert("XBTEUR", "XBT/EUR");
    n
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

// ── Signature ──

/// The doc's worked example ("Authentication", Python: nonce 1616492376594, URI path
/// `/0/private/AddOrder`) is pinned in `signer.rs`. Here: the body `place` builds for the
/// same order (it adds `oflags=fciq`, sorted as the doc's example is) signed with the doc's
/// key and nonce, against an independent implementation (Python's `hmac`, `hashlib`,
/// `base64`, computed once with the guide's formula and pinned; the same script reproduces
/// the doc's own example signature first).
#[test]
fn the_documented_order_is_the_body_sent_and_signs_like_an_independent_implementation() {
    let m = Kraken::new().unwrap();
    let req = m.place(&settings(), &order("XBTUSD", "", Side::Buy, OrderType::Limit, "1.25", Some("37500"), None), &placing("")).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.base), (Method::Post, "/0/private/AddOrder", true, "rest"));
    assert_eq!(body(&req), "oflags=fciq&ordertype=limit&pair=XBTUSD&price=37500&type=buy&volume=1.25");
    let secret = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.decode(DOC_SECRET).unwrap()
    };
    let signed = format!("nonce=1616492376594&{}", body(&req));
    assert_eq!(sign(&secret, &req.path, "1616492376594", &signed).unwrap(), "wCwrlT08X7xWO7qyOsuSh4RytkpAThMLrfoxWgRKiI63pB2Th3iwvJyZcfKqNRQfVTM+h8UpCC2ibmq7qdPXTA==");
    // The doc's own example through the same function.
    assert_eq!(
        sign(&secret, "/0/private/AddOrder", "1616492376594", "nonce=1616492376594&ordertype=limit&pair=XBTUSD&price=37500&type=buy&volume=1.25").unwrap(),
        "4/dpxb3iT4tp/ZCVEwSnEsLxx0bqyhLpdfOpc6fn7OR8+UClSV5n9E6aSS8MPtnRfp32bAb0nmbRn6H8ndwLUQ=="
    );
}

/// The signer puts the nonce in front of the body `place` built, signs that, and sends one
/// Content-Type.
#[test]
fn a_placed_order_is_signed_with_the_nonce_in_front() {
    let m = Kraken::new().unwrap();
    let req = m.place(&settings(), &order("XBTUSD", "k1", Side::Sell, OrderType::Market, "0.5", None, None), &placing("k1")).unwrap();
    let s = KrakenSigner::new(SecretString::from("otw-test-key".to_string()), SecretString::from(DOC_SECRET.to_string())).unwrap();
    let mut sr = otw_connector_sdk::SignableRequest { method: req.method, path: req.path.clone(), query: vec![], headers: vec![], body: req.body.clone() };
    s.sign(&mut sr, otw_connector_model::Timestamp::from_millis(1_767_225_600_000).unwrap()).unwrap();
    let sent = String::from_utf8(sr.body.unwrap().to_vec()).unwrap();
    assert!(sent.starts_with("nonce=") && sent.ends_with(&format!("&{}", body(&req))), "{sent}");
    assert_eq!(sr.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1);
}

// ── Requests ──

#[test]
fn every_type_goes_out_in_the_venues_words() {
    let m = mapper();
    let f = |o: OrderRequest| fields(&m.place(&settings(), &o, &placing(&o.client_order_id)).unwrap());
    let mkt = f(order("XBTUSD", "a", Side::Buy, OrderType::Market, "0.01", None, None));
    assert_eq!((mkt["ordertype"].as_str(), mkt["volume"].as_str(), mkt["oflags"].as_str()), ("market", "0.01", "fciq"));
    assert!(!mkt.contains_key("price") && !mkt.contains_key("timeinforce") && !mkt.contains_key("trigger"));
    let lim = f(order("XBTUSD", "b", Side::Sell, OrderType::Limit, "0.01", Some("30000.1"), None));
    assert_eq!((lim["ordertype"].as_str(), lim["price"].as_str(), lim["type"].as_str(), lim["cl_ord_id"].as_str()), ("limit", "30000.1", "sell", "b"));
    let stop = f(order("XBTUSD", "c", Side::Sell, OrderType::Stop, "0.01", None, Some("29000")));
    assert_eq!((stop["ordertype"].as_str(), stop["price"].as_str(), stop["trigger"].as_str()), ("stop-loss", "29000", "last"));
    let sl = f(order("XBTUSD", "e", Side::Sell, OrderType::StopLimit, "0.01", Some("28900"), Some("29000")));
    assert_eq!((sl["ordertype"].as_str(), sl["price"].as_str(), sl["price2"].as_str()), ("stop-loss-limit", "29000", "28900"));
    let tp = f(order("XBTUSD", "g", Side::Sell, OrderType::Mit, "0.01", None, Some("31000")));
    assert_eq!((tp["ordertype"].as_str(), tp["price"].as_str()), ("take-profit", "31000"));
    let tpl = f(order("XBTUSD", "h", Side::Sell, OrderType::Lit, "0.01", Some("30900"), Some("31000")));
    assert_eq!((tpl["ordertype"].as_str(), tpl["price"].as_str(), tpl["price2"].as_str()), ("take-profit-limit", "31000", "30900"));
}

#[test]
fn time_in_force_post_only_and_the_fee_in_quote() {
    let m = mapper();
    let f = |tif: TimeInForce, post: bool| {
        let mut o = order("XBTUSD", "t", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
        o.tif = tif;
        o.opts.post_only = post;
        fields(&m.place(&settings(), &o, &placing("t")).unwrap())
    };
    assert!(!f(TimeInForce::Gtc, false).contains_key("timeinforce"), "GTC is the default");
    assert_eq!(f(TimeInForce::Ioc, false)["timeinforce"], "IOC");
    assert_eq!(f(TimeInForce::Fok, false)["timeinforce"], "FOK");
    let gtd = f(TimeInForce::Gtd(otw_connector_model::Timestamp::from_secs(1_767_300_000).unwrap()), false);
    assert_eq!((gtd["timeinforce"].as_str(), gtd["expiretm"].as_str()), ("GTD", "1767300000"));
    assert_eq!(f(TimeInForce::Gtc, true)["oflags"], "post,fciq");
}

#[test]
fn what_the_doc_limits_is_refused_before_io() {
    let m = mapper();
    let mut fok_stop = order("XBTUSD", "x", Side::Sell, OrderType::Stop, "0.01", None, Some("29000"));
    fok_stop.tif = TimeInForce::Fok;
    assert!(m.check_order(&fok_stop).unwrap_err().message.contains("limit order only"));
    let mut ioc_stop = fok_stop.clone();
    ioc_stop.tif = TimeInForce::Ioc;
    assert!(m.check_order(&ioc_stop).is_err());
    let mut fok_mkt = order("XBTUSD", "x", Side::Buy, OrderType::Market, "0.01", None, None);
    fok_mkt.tif = TimeInForce::Fok;
    assert!(m.check_order(&fok_mkt).is_err());
    let mut gtd_mkt = fok_mkt.clone();
    gtd_mkt.tif = TimeInForce::Gtd(otw_connector_model::Timestamp::from_secs(1_767_300_000).unwrap());
    assert!(m.check_order(&gtd_mkt).is_err());
    let mut ioc_mkt = fok_mkt.clone();
    ioc_mkt.tif = TimeInForce::Ioc;
    assert!(m.check_order(&ioc_mkt).is_ok());
    let mut gtd_stop = fok_stop.clone();
    gtd_stop.tif = gtd_mkt.tif;
    assert!(m.check_order(&gtd_stop).is_ok());
    // What the words cannot carry.
    let mut reduce = order("XBTUSD", "r", Side::Sell, OrderType::Limit, "0.01", Some("30000"), None);
    reduce.opts.reduce_only = true;
    assert!(m.place(&settings(), &reduce, &placing("r")).is_err());
    let mut mark = order("XBTUSD", "r", Side::Sell, OrderType::Stop, "0.01", None, Some("29000"));
    mark.opts.trigger = Some(TriggerKind::Mark);
    assert!(m.place(&settings(), &mark, &placing("r")).is_err());
    mark.opts.trigger = Some(TriggerKind::Last);
    assert!(m.place(&settings(), &mark, &placing("r")).is_ok());
}

/// The account read names pairs by wsname (`XBT/USD`); AddOrder takes "Asset pair `id` or
/// `altname`": the altname goes out, from the pair list read, never from cutting the slash.
#[test]
fn a_wsname_goes_out_as_its_altname_from_the_list_and_never_guessed() {
    let m = mapper();
    let o = order("XBT/USD", "w", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
    assert_eq!(fields(&m.place(&settings(), &o, &placing("w")).unwrap())["pair"], "XBTUSD");
    assert_eq!(fields(&m.place(&settings(), &order("XXBTZUSD", "w", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None), &placing("w")).unwrap())["pair"], "XXBTZUSD");
    let unread = Kraken::new().unwrap();
    assert!(unread.place(&settings(), &o, &placing("w")).unwrap_err().message.contains("altname"));
}

#[test]
fn amend_cancel_cancel_all_and_lookups_are_the_documented_calls() {
    let m = mapper();
    let (rows, _) = m.decode_orders_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&names()), OPEN.as_bytes()).unwrap();
    let cur = rows.iter().find(|o| o.venue_id == "OQCLML-BW3P3-BUCMWZ").unwrap();
    let a = m.amend(&settings(), cur, &OrderAmend { limit: Some(Price::new(d("30020"))), qty: Some(Qty::new(d("1.5"))), ..Default::default() }, &placing("")).unwrap();
    assert_eq!((a.path.as_str(), a.endpoint_id), ("/0/private/AmendOrder", "private:amendOrder"));
    assert_eq!(body(&a), "limit_price=30020&order_qty=1.5&txid=OQCLML-BW3P3-BUCMWZ");
    assert!(m.amend(&settings(), cur, &OrderAmend { stop: Some(Price::new(d("1"))), ..Default::default() }, &placing("")).is_err(), "a limit has no trigger");
    assert!(m.amend(&settings(), cur, &OrderAmend { tif: Some(TimeInForce::Ioc), ..Default::default() }, &placing("")).is_err());
    // A stop-limit whose limit reads `0` (the doc's examples) still has one to change.
    let (closed, _) = m.decode_orders_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&names()), CLOSED.as_bytes()).unwrap();
    let sl = closed.iter().find(|o| o.order_type == "stop-loss-limit").unwrap();
    assert!(m.amend(&settings(), sl, &OrderAmend { limit: Some(Price::new(d("23000"))), stop: Some(Price::new(d("23100"))), ..Default::default() }, &placing("")).is_ok());
    let c = m.cancel(&settings(), "XBT/USD", "OQCLML-BW3P3-BUCMWZ", CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/0/private/CancelOrder", "txid=OQCLML-BW3P3-BUCMWZ".to_string()));
    let all = m.cancel_all(&settings(), &[], CorrId(1)).unwrap();
    assert_eq!((all.len(), all[0].path.as_str(), body(&all[0])), (1, "/0/private/CancelAll", String::new()));
    assert_eq!(m.cancel_all_way(), CancelAllWay::Book);
    assert!(m.capabilities().orders.cancel_all_native);
    let look = m.query_by_client_id(&settings(), "XBTUSD", "k1", CorrId(1)).unwrap();
    assert_eq!(look.iter().map(|r| (r.path.as_str(), body(r))).collect::<Vec<_>>(), vec![("/0/private/OpenOrders", "cl_ord_id=k1".to_string()), ("/0/private/ClosedOrders", "cl_ord_id=k1".to_string())]);
    let by = m.query_by_venue_id(&settings(), "XBTUSD", "O1", CorrId(1)).unwrap();
    assert_eq!((by[0].path.as_str(), body(&by[0])), ("/0/private/QueryOrders", "txid=O1".to_string()));
    let open = m.open_orders(&settings(), None, CorrId(1)).unwrap();
    assert_eq!(body(&open[0]), "limit=100&with_cursor=true");
    let next = m.with_cursor(&open[0], "abc/=");
    assert_eq!(body(&next), "limit=100&with_cursor=true&cursor=abc%2F%3D");
    assert_eq!(body(&m.with_cursor(&next, "d")), "limit=100&with_cursor=true&cursor=d");
}

/// AmendOrder `post_only`: "If `true`, the limit price change will be rejected if the order
/// cannot be posted passively in the book". A post-only order (`oflags` `post`) keeps it on
/// a new limit; any other order, and a change that moves no price, sends none.
#[test]
fn a_post_only_order_keeps_post_only_when_its_limit_moves() {
    let m = mapper();
    let n = names();
    let sizes = ContractSizes::new();
    let ctx = DecodeCtx::new("", &sizes).with_names(&n);
    let post = with(OPEN, |v| v["result"]["open"]["OQCLML-BW3P3-BUCMWZ"]["oflags"] = json!("post,fciq"));
    let (rows, _) = m.decode_orders_in(&ctx, post.as_bytes()).unwrap();
    let cur = rows.iter().find(|o| o.venue_id == "OQCLML-BW3P3-BUCMWZ").unwrap();
    let limit = OrderAmend { limit: Some(Price::new(d("30020"))), ..Default::default() };
    assert_eq!(body(&m.amend(&settings(), cur, &limit, &placing("")).unwrap()), "limit_price=30020&post_only=true&txid=OQCLML-BW3P3-BUCMWZ");
    let size = OrderAmend { qty: Some(Qty::new(d("1.5"))), ..Default::default() };
    assert_eq!(body(&m.amend(&settings(), cur, &size, &placing("")).unwrap()), "order_qty=1.5&txid=OQCLML-BW3P3-BUCMWZ");
    // Read again without the flag (or ended): no longer post-only.
    let (rows, _) = m.decode_orders_in(&ctx, OPEN.as_bytes()).unwrap();
    let plain = rows.iter().find(|o| o.venue_id == "OQCLML-BW3P3-BUCMWZ").unwrap();
    assert_eq!(body(&m.amend(&settings(), plain, &limit, &placing("")).unwrap()), "limit_price=30020&txid=OQCLML-BW3P3-BUCMWZ");
    m.decode_orders_in(&ctx, post.as_bytes()).unwrap();
    let ended = with(&post, |v| v["result"]["open"]["OQCLML-BW3P3-BUCMWZ"]["status"] = json!("canceled"));
    m.decode_orders_in(&ctx, ended.as_bytes()).unwrap();
    assert!(!body(&m.amend(&settings(), plain, &limit, &placing("")).unwrap()).contains("post_only"));
}

/// The calls that count on the key's REST call counter go out under `private:` (one bucket);
/// AddOrder and CancelOrder "operate on a different limiter" and do not.
#[test]
fn only_the_counted_calls_count_on_the_key_s_counter() {
    let m = mapper();
    let o = order("XBTUSD", "c", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
    let mut counted = vec![
        m.amend(&settings(), &m.decode_orders_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&names()), OPEN.as_bytes()).unwrap().0[0], &OrderAmend { qty: Some(Qty::new(d("1"))), ..Default::default() }, &placing("")).unwrap(),
        m.fills_since(&settings(), "XBTUSD", 0, 1000, CorrId(1)).unwrap(),
        m.private_socket_bootstrap(&settings(), &m.private_socket(&settings()).unwrap(), CorrId(1)).unwrap().unwrap(),
    ];
    counted.extend(m.cancel_all(&settings(), &[], CorrId(1)).unwrap());
    counted.extend(m.query_by_client_id(&settings(), "XBTUSD", "k", CorrId(1)).unwrap());
    counted.extend(m.query_by_venue_id(&settings(), "XBTUSD", "O1", CorrId(1)).unwrap());
    counted.extend(m.open_orders(&settings(), None, CorrId(1)).unwrap());
    for r in &counted {
        assert!(r.endpoint_id.starts_with("private:"), "{} {}", r.path, r.endpoint_id);
    }
    let free = [
        m.place(&settings(), &o, &placing("c")).unwrap(),
        m.dry_run(&settings(), &o, &placing("c")).unwrap().unwrap(),
        m.cancel(&settings(), "XBTUSD", "O1", CorrId(1)).unwrap(),
        m.instrument_rules(&settings(), None, CorrId(1)).unwrap(),
    ];
    for r in &free {
        assert!(!r.endpoint_id.starts_with("private"), "{} {}", r.path, r.endpoint_id);
    }
}

/// A pair the list read does not name is an error, never the raw spelling (the book is read
/// again); a fill's fee currency is the quote of its wsname, never empty.
#[test]
fn a_pair_the_list_does_not_name_is_an_error_not_a_guess() {
    let m = mapper();
    let n = names();
    let sizes = ContractSizes::new();
    let ctx = DecodeCtx::new("", &sizes).with_names(&n);
    let unknown = with(OPEN, |v| v["result"]["open"]["OQCLML-BW3P3-BUCMWZ"]["descr"]["pair"] = json!("NEWPAIR"));
    assert!(m.decode_orders_in(&ctx, unknown.as_bytes()).unwrap_err().message.contains("NEWPAIR"));
    let fill = with(TRADES, |v| v["result"]["trades"]["THVRQM-33VKH-UCI7BS"]["pair"] = json!("NEWPAIR"));
    assert!(m.decode_fills_in(&ctx, fill.as_bytes()).unwrap_err().message.contains("NEWPAIR"));
    // Without the name table nothing is guessed either.
    assert!(m.decode_fills(TRADES.as_bytes()).is_err());
    // A pair the list names without a wsname: its quote, so its fee's currency, is not known.
    let mut bare = SymbolNames::new();
    bare.insert("XXBTZUSD", "XBTUSD");
    let e = m.decode_fills_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&bare), TRADES.as_bytes()).unwrap_err();
    assert!(e.message.contains("fee"), "{e:?}");
}

/// A trailing order placed elsewhere states its prices relative (`+5%`): the row is kept, the
/// rest of the list reads.
#[test]
fn a_relative_price_keeps_the_row() {
    let m = mapper();
    let n = names();
    let sizes = ContractSizes::new();
    let ctx = DecodeCtx::new("", &sizes).with_names(&n);
    let trailing = with(OPEN, |v| {
        let o = &mut v["result"]["open"]["OB5VMB-B4U2U-DK2WRW"]["descr"];
        o["ordertype"] = json!("trailing-stop");
        o["price"] = json!("+5.0000%");
    });
    let (rows, _) = m.decode_orders_in(&ctx, trailing.as_bytes()).unwrap();
    assert_eq!(rows.len(), 2);
    let t = rows.iter().find(|o| o.venue_id == "OB5VMB-B4U2U-DK2WRW").unwrap();
    assert_eq!((t.order_type.as_str(), t.limit, t.stop), ("trailing-stop", None, None));
}

/// A UUID placed elsewhere is never read as an OTW key: every RFC version (1 to 8) has its
/// number in the nibble OTW marks `a` (6.8% of v4 UUIDs read as keys before the mark).
#[test]
fn a_uuid_placed_elsewhere_never_reads_as_an_otw_key() {
    let mut x: u128 = 0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c834;
    for i in 0..20_000u32 {
        x = x.wrapping_mul(0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645).wrapping_add(0x5851_f42d_4c95_7f2d_1405_7b7e_f767_814f);
        let version = u128::from(i % 8 + 1);
        let v = (x & !(0xF_u128 << 76)) | (version << 76);
        let h = format!("{v:032x}");
        let uuid = format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..]);
        assert_eq!(unpack(&uuid), None, "{uuid}");
    }
    assert_eq!(unpack("6d1b345e-2821-40e2-ad83-4ecb18a06876"), None);
}

#[test]
fn fills_are_asked_over_whole_seconds_that_cover_the_window() {
    let m = mapper();
    let r = m.fills_since(&settings(), "XBTUSD", 1_767_222_000_500, 1_767_225_600_001, CorrId(1)).unwrap();
    assert_eq!((r.path.as_str(), r.endpoint_id, r.weight), ("/0/private/TradesHistory", "private:tradesHistory", 2));
    assert_eq!(body(&r), "end=1767225601&limit=100&start=1767221999&type=all&with_cursor=true");
}

/// AddOrder `validate=true`, never with the live order's client id.
#[test]
fn the_dry_run_validates_without_the_client_id() {
    let m = mapper();
    let o = order("XBTUSD", "live", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
    let r = m.dry_run(&settings(), &o, &placing("live")).unwrap().unwrap();
    let f = fields(&r);
    assert_eq!((r.path.as_str(), f["validate"].as_str()), ("/0/private/AddOrder", "true"));
    assert!(!f.contains_key("cl_ord_id"), "{f:?}");
    let ok = r#"{"error":[],"result":{"descr":{"order":"buy 0.01000000 XBTUSD @ limit 30000.0"}}}"#;
    assert!(m.decode_dry_run(ok.as_bytes()).unwrap().contains("buy 0.01000000 XBTUSD"));
}

// ── Client ids ──

#[test]
fn a_short_key_goes_as_free_text_and_a_robot_key_as_a_uuid_both_ways() {
    let m = Kraken::new().unwrap();
    assert_eq!(m.client_id("o1abc2").unwrap(), "o1abc2");
    assert_eq!(m.client_id("at12345678abcdef").unwrap(), "at12345678abcdef");
    let id = m.client_id(ROBOT).unwrap();
    // Python, independently: p = sum(36**l for l in range(19, len)) + int(key, 36), then
    // (p >> 76) << 80 | 0xA << 76 | p & (2**76 - 1), as 8-4-4-4-12 hex.
    assert_eq!(id, "c21b1414-fb3a-aeed-15ee-41a8b2d73c5b");
    assert_eq!(m.otw_id(&id), ROBOT);
    // Whatever UUID spelling the venue echoes (long or short, any case).
    assert_eq!(m.otw_id("C21B1414FB3AAEED15EE41A8B2D73C5B"), ROBOT);
    assert_eq!(m.otw_id("C21B1414-FB3A-AEED-15EE-41A8B2D73C5B"), ROBOT);
    assert_eq!(spelled("C21B1414FB3AAEED15EE41A8B2D73C5B"), id);
    assert_eq!(m.otw_id("o1abc2"), "o1abc2");
    // Leading zeros and every length from 19 to 24 come back.
    for k in ["0000000000000000000", "000000000000000000a1", "zzzzzzzzzzzzzzzzzzzzzzz", "wzzzzzzzzzzzzzzzzzzzzzzz", "abcdefghijklmnopqrstu"] {
        assert_eq!(unpack(&pack(k).unwrap()).as_deref(), Some(k), "{k}");
    }
    assert_eq!(pack("0000000000000000000").unwrap(), "00000000-0000-a000-0000-000000000000");
    assert_eq!(pack("wzzzzzzzzzzzzzzzzzzzzzzz").unwrap(), "ff759572-1705-abab-5e77-ab3fffffffff");
    // The largest 24-character keys do not fit in 124 bits: refused, never cut.
    assert_eq!(pack("zzzzzzzzzzzzzzzzzzzzzzzz"), None);
    assert!(m.client_id("zzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    // A UUID OTW did not pack stays as it is (an order placed elsewhere).
    assert_eq!(m.otw_id("6d1b345e-2821-40e2-ad83-4ecb18a06876"), "6d1b345e-2821-40e2-ad83-4ecb18a06876");
    // Neither free text nor packable: refused naming the rule.
    for bad in ["a".repeat(25), "OTSA0123456789ABCDEF0123".to_string(), "ots-0123456789abcdef0123".to_string(), String::new()] {
        assert!(m.client_id(&bad).unwrap_err().message.contains("18"), "{bad}");
    }
    // The packed id goes out in the order.
    let mm = mapper();
    let o = order("XBTUSD", ROBOT, Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
    assert_eq!(fields(&mm.place(&settings(), &o, &placing(&id)).unwrap())["cl_ord_id"], id);
}

// ── Answers ──

#[test]
fn the_documented_ack_names_the_order() {
    let m = Kraken::new().unwrap();
    assert_eq!(m.decode_ack(ADD.as_bytes()).unwrap(), Ack { venue_id: "OU22CG-KLAF2-FWUDD7".into(), client_id: None });
    assert!(!m.decode_ack(br#"{"error":[],"result":{"descr":{"order":"x"}}}"#).unwrap().has_id());
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "amendOrder", 200, AMEND)).is_none());
    assert!(m.decode_error(&otw_connector_testkit::http_frame(1, "cancelOrder", 200, CANCEL)).is_none());
}

#[test]
fn open_orders_read_in_base_coin_with_the_partial_fill_and_named_like_the_account_read() {
    let m = mapper();
    let n = names();
    let (rows, next) = m.decode_orders_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&n), OPEN.as_bytes()).unwrap();
    assert!(next.is_none());
    let a = rows.iter().find(|o| o.venue_id == "OQCLML-BW3P3-BUCMWZ").unwrap();
    assert_eq!(a.symbol, "XBT/USD");
    assert_eq!((a.status.clone(), a.qty, a.filled_qty), (Mapped::Known(OrderStatus::PartiallyFilled), d("1.25"), d("0.375")));
    assert_eq!((a.limit, a.stop, a.avg_price, a.side.clone()), (Some(d("30010.0")), None, Some(d("30010.0")), Mapped::Known(Side::Buy)));
    assert_eq!((a.created_ms, a.client_id.clone(), a.trigger_pending), (Some(1_688_666_559_897), None, false));
    let b = rows.iter().find(|o| o.venue_id == "OB5VMB-B4U2U-DK2WRW").unwrap();
    assert_eq!(b.status, Mapped::Known(OrderStatus::New));
}

#[test]
fn closed_and_queried_orders_tell_a_fill_from_a_cancel() {
    let m = mapper();
    let n = names();
    let sizes = ContractSizes::new();
    let ctx = DecodeCtx::new("", &sizes).with_names(&n);
    let (rows, _) = m.decode_orders_in(&ctx, CLOSED.as_bytes()).unwrap();
    let sl = rows.iter().find(|o| o.venue_id == "O37652-RJWRT-IMO74O").unwrap();
    // `price` is the trigger of a stop-loss-limit; `price2` reads 0 here (no limit shown).
    assert_eq!((sl.status.clone(), sl.stop, sl.limit, sl.symbol.as_str()), (Mapped::Known(OrderStatus::Cancelled), Some(d("23667.0")), None, "XBT/GBP"));
    let (q, _) = m.decode_orders_in(&ctx, QUERY.as_bytes()).unwrap();
    let full = q.iter().find(|o| o.venue_id == "OBCMZD-JIEE7-77TH3F").unwrap();
    assert_eq!((full.status.clone(), full.filled_qty, full.symbol.as_str()), (Mapped::Known(OrderStatus::Filled), d("1.25"), "XBT/USD"));
    let mkt = q.iter().find(|o| o.venue_id == "OMMDB2-FSB6Z-7W3HPO").unwrap();
    assert_eq!((mkt.limit, mkt.stop, mkt.avg_price), (None, None, Some(d("30000.0"))));
    // `closed` short of its size: cancelled with what it filled ("canceled partial").
    let part = with(QUERY, |v| v["result"]["OBCMZD-JIEE7-77TH3F"]["vol_exec"] = json!("0.5"));
    let (p, _) = m.decode_orders_in(&ctx, part.as_bytes()).unwrap();
    let o = p.iter().find(|o| o.venue_id == "OBCMZD-JIEE7-77TH3F").unwrap();
    assert_eq!((o.status.clone(), o.filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.5")));
    // Every documented status word, and an unknown one never near-matched.
    for (w, s) in [("pending", OrderStatus::PendingNew), ("open", OrderStatus::New), ("closed", OrderStatus::Filled), ("canceled", OrderStatus::Cancelled), ("expired", OrderStatus::Expired)] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    assert!(matches!(m.status_of("cancelled"), Mapped::Unknown(_)));
}

#[test]
fn a_waiting_stop_is_pending_its_trigger_and_a_quote_sized_buy_reads_what_filled() {
    let m = mapper();
    let n = names();
    let sizes = ContractSizes::new();
    let ctx = DecodeCtx::new("", &sizes).with_names(&n);
    let stop = with(OPEN, |v| {
        let o = &mut v["result"]["open"]["OB5VMB-B4U2U-DK2WRW"];
        o["descr"]["ordertype"] = json!("stop-loss");
        o["cl_ord_id"] = json!("C21B1414FB3AAEED15EE41A8B2D73C5B");
    });
    let (rows, _) = m.decode_orders_in(&ctx, stop.as_bytes()).unwrap();
    let s = rows.iter().find(|o| o.venue_id == "OB5VMB-B4U2U-DK2WRW").unwrap();
    assert_eq!((s.trigger_pending, s.stop, s.limit), (true, Some(d("14500.0")), None));
    assert_eq!(m.otw_id(s.client_id.as_deref().unwrap()), ROBOT, "the echoed UUID is read back as the bot's key");
    let fired = with(&stop, |v| v["result"]["open"]["OB5VMB-B4U2U-DK2WRW"]["misc"] = json!("stopped"));
    let (rows, _) = m.decode_orders_in(&ctx, fired.as_bytes()).unwrap();
    assert!(!rows.iter().find(|o| o.venue_id == "OB5VMB-B4U2U-DK2WRW").unwrap().trigger_pending);
    let viqc = with(QUERY, |v| {
        let o = &mut v["result"]["OMMDB2-FSB6Z-7W3HPO"];
        o["oflags"] = json!("viqc");
        o["vol"] = json!("7500");
    });
    let (rows, _) = m.decode_orders_in(&ctx, viqc.as_bytes()).unwrap();
    let o = rows.iter().find(|o| o.venue_id == "OMMDB2-FSB6Z-7W3HPO").unwrap();
    assert_eq!((o.qty, o.status.clone()), (d("0.25"), Mapped::Known(OrderStatus::Filled)));
}

#[test]
fn fills_name_the_pair_like_the_account_read_with_the_fee_in_the_quote() {
    let m = mapper();
    let n = names();
    let (fills, next) = m.decode_fills_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&n), TRADES.as_bytes()).unwrap();
    assert!(next.is_none());
    let f = fills.iter().find(|f| f.exec_id == "THVRQM-33VKH-UCI7BS").unwrap();
    assert_eq!((f.symbol.as_str(), f.venue_order_id.as_str(), f.qty, f.price), ("XBT/USD", "OQCLML-BW3P3-BUCMWZ", d("0.02"), d("30010")));
    assert_eq!((f.fee, f.fee_currency.as_str(), f.at_ms, f.position_only), (d("0"), "USD", 1_688_667_796_880, false));
    let paged = with(TRADES, |v| v["result"]["cursor"] = json!({ "next": "n2" }));
    assert_eq!(m.decode_fills_in(&DecodeCtx::new("", &ContractSizes::new()).with_names(&n), paged.as_bytes()).unwrap().1.as_deref(), Some("n2"));
}

#[test]
fn rules_are_the_pair_lists_under_both_names_and_the_status_words() {
    let m = Kraken::new().unwrap();
    let (rules, _) = m.decode_rules(PAIRS.as_bytes()).unwrap();
    let alt = rules.iter().find(|r| r.instrument.native == "XBTUSD").unwrap();
    let ws = rules.iter().find(|r| r.instrument.native == "XBT/USD").unwrap();
    assert_eq!((alt.tick, alt.lot_step, alt.min_qty, alt.min_notional), (Some(d("0.1")), Some(d("0.00000001")), Some(d("0.0001")), Some(d("0.5"))));
    assert_eq!((alt.status.clone(), alt.quote_step), (TradingStatus::Trading, Some(d("0.00001"))));
    assert_eq!((ws.tick, ws.min_qty), (alt.tick, alt.min_qty));
    assert!(alt.aliases.contains(&"XXBTZUSD".to_string()) && alt.aliases.contains(&"XBT/USD".to_string()));
    for r in &rules {
        r.sound().unwrap();
    }
    let n = SymbolNames::of(&rules);
    assert_eq!((n.native("XXBTZUSD"), n.native("XBTUSD"), n.native("xbt/usd")), (Some("XBT/USD"), Some("XBT/USD"), Some("XBT/USD")));
    let status = |w: &str| {
        let body = with(PAIRS, |v| v["result"]["XXBTZUSD"]["status"] = json!(w));
        m.decode_rules(body.as_bytes()).unwrap().0.into_iter().find(|r| r.instrument.native == "XBTUSD").unwrap().status
    };
    assert_eq!(status("post_only"), TradingStatus::PostOnly("post_only".into()));
    assert_eq!(status("limit_only"), TradingStatus::LimitOnly("limit_only".into()));
    for w in ["cancel_only", "reduce_only", "delisted", "maintenance", "something_new"] {
        assert_eq!(status(w), TradingStatus::Closed(w.into()), "{w}");
    }
    let lots = with(PAIRS, |v| v["result"]["XXBTZUSD"]["lot_multiplier"] = json!(10));
    let r = m.decode_rules(lots.as_bytes()).unwrap().0.into_iter().find(|r| r.instrument.native == "XBTUSD").unwrap();
    assert!(matches!(r.status, TradingStatus::Closed(ref w) if w.contains("lot_multiplier")));
}

// ── Private stream ──

#[test]
fn the_socket_opens_with_a_fresh_token_in_its_subscribe() {
    let m = Kraken::new().unwrap();
    let s = m.private_socket(&settings()).unwrap();
    assert_eq!((s.base, s.path.as_str(), s.ping_every_ms), ("ws_auth", "/v2", 30_000));
    assert_eq!(serde_json::from_slice::<Value>(&s.ping).unwrap(), json!({ "method": "ping" }));
    let boot = m.private_socket_bootstrap(&settings(), &s, CorrId(1)).unwrap().unwrap();
    assert_eq!((boot.path.as_str(), boot.method, boot.signed), ("/0/private/GetWebSocketsToken", Method::Post, true));
    let s = m.decode_private_socket_bootstrap(s, WS_TOKEN.as_bytes()).unwrap();
    let sub: Value = serde_json::from_slice(&s.subscribe).unwrap();
    assert_eq!(sub["method"], "subscribe");
    assert_eq!(sub["params"]["channel"], "executions");
    assert_eq!(sub["params"]["token"], "1Dwc4lzSwNWOAwkMdqhssNNFhs1ed606d1WcF3XfEMw");
    assert_eq!(s.subscribes().len(), 1);
    assert!(m.decode_private_socket_bootstrap(m.private_socket(&settings()).unwrap(), br#"{"error":[],"result":{}}"#).is_err());
}

#[test]
fn every_execution_frame_reads_its_order_again_and_answers_are_answers() {
    let m = Kraken::new().unwrap();
    let n = names();
    let sizes = ContractSizes::new();
    let ctx = DecodeCtx::new("/v2", &sizes).with_names(&n);
    assert_eq!(m.decode_private_in(&ctx, WS_SUBSCRIBE.as_bytes()).unwrap(), vec![PrivateEvent::Answer { op: "subscribe".into(), ok: true, message: String::new() }]);
    let refused = r#"{"method":"subscribe","success":false,"error":"EGeneral:Invalid arguments"}"#;
    assert!(matches!(&m.decode_private_in(&ctx, refused.as_bytes()).unwrap()[0], PrivateEvent::Answer { ok: false, message, .. } if message.contains("Invalid")));
    assert_eq!(m.decode_private_in(&ctx, WS_PONG.as_bytes()).unwrap(), vec![PrivateEvent::Answer { op: "pong".into(), ok: true, message: String::new() }]);
    assert!(m.decode_private_in(&ctx, WS_HEARTBEAT.as_bytes()).unwrap().is_empty());
    // `BTC/USD` is the socket's spelling, not one the pair list gives: no symbol, read by id.
    for frame in [WS_LIVE, WS_EXECUTION] {
        assert_eq!(m.decode_private_in(&ctx, frame.as_bytes()).unwrap(), vec![PrivateEvent::OrderChanged { symbol: None, venue_id: "OK4GJX-KSTLS-7DZZO5".into() }]);
    }
    // Not live yet: its `new` follows, read then.
    assert!(m.decode_private_in(&ctx, WS_PENDING.as_bytes()).unwrap().is_empty());
    // A name the list gives is passed on; one order named twice is read once.
    let two = with(WS_LIVE, |v| {
        let row = v["data"][0].clone();
        v["data"] = json!([row.clone(), row]);
        v["data"][0]["symbol"] = json!("XBT/USD");
    });
    assert_eq!(m.decode_private_in(&ctx, two.as_bytes()).unwrap(), vec![PrivateEvent::OrderChanged { symbol: Some("XBT/USD".into()), venue_id: "OK4GJX-KSTLS-7DZZO5".into() }]);
    assert!(m.decode_private_in(&ctx, b"not json").is_err());
}

// ── The catalog ──

/// The real mapper with two changes the catalog's mock needs, nothing else: Kraken puts every
/// parameter in the POST body, and the mock tells calls apart by path and query alone. So the
/// lookups and the open-order list copy their body into the query (OpenOrders by client id and
/// OpenOrders as a list are one path), and the validate call goes to a path of its own
/// (AddOrder with `validate=true` is the place path). Both are covered on the real mapper above.
#[derive(Clone)]
struct Catalog(Kraken);

type R<T> = Result<T, otw_connector_model::ConnectorError>;

fn mirrored(mut h: HttpRequest) -> HttpRequest {
    let b = body(&h);
    h.query = b.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    h
}

impl Mapper for Catalog {
    type Session = Session;
    fn capabilities(&self) -> &Capabilities {
        self.0.capabilities()
    }
    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.0.base_urls()
    }
    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> R<Vec<NativeRequest>> {
        self.0.encode(cmd, s, ctx)
    }
    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> R<Vec<Envelope>> {
        self.0.decode(frame, s, ctx)
    }
    fn instrument_id(&self, symbol: &str) -> R<InstrumentId> {
        self.0.instrument_id(symbol)
    }
}

impl TradeMapper for Catalog {
    fn venue_name(&self) -> &str {
        self.0.venue_name()
    }
    fn client_id_rule(&self) -> ClientIdRule {
        self.0.client_id_rule()
    }
    fn client_id(&self, otw_id: &str) -> R<String> {
        self.0.client_id(otw_id)
    }
    fn otw_id(&self, v: &str) -> String {
        self.0.otw_id(v)
    }
    fn place(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<HttpRequest> {
        self.0.place(s, o, p)
    }
    fn amend(&self, s: &Settings, cur: &VenueOrder, c: &OrderAmend, p: &Placing) -> R<HttpRequest> {
        self.0.amend(s, cur, c, p)
    }
    fn cancel(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<HttpRequest> {
        self.0.cancel(s, symbol, id, corr)
    }
    fn cancel_all(&self, s: &Settings, symbols: &[String], corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.cancel_all(s, symbols, corr)
    }
    fn cancel_all_way(&self) -> CancelAllWay {
        self.0.cancel_all_way()
    }
    fn query_by_client_id(&self, s: &Settings, symbol: &str, cid: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        Ok(self.0.query_by_client_id(s, symbol, cid, corr)?.into_iter().map(mirrored).collect())
    }
    fn query_by_venue_id(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_venue_id(s, symbol, id, corr)
    }
    fn open_orders(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<Vec<HttpRequest>> {
        Ok(self.0.open_orders(s, symbol, corr)?.into_iter().map(mirrored).collect())
    }
    fn fills_since(&self, s: &Settings, symbol: &str, from: i64, to: i64, corr: CorrId) -> R<HttpRequest> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn instrument_rules(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<HttpRequest> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn check_order(&self, o: &OrderRequest) -> R<()> {
        self.0.check_order(o)
    }
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        self.0.with_cursor(req, cursor)
    }
    fn dry_run(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<Option<HttpRequest>> {
        Ok(self.0.dry_run(s, o, p)?.map(|mut h| {
            h.path = "/0/private/AddOrder/validate".into();
            h
        }))
    }
    fn decode_dry_run(&self, body: &[u8]) -> R<String> {
        self.0.decode_dry_run(body)
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
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        self.0.status_of(word)
    }
    fn uses_symbol_names(&self) -> bool {
        self.0.uses_symbol_names()
    }
    fn private_socket(&self, s: &Settings) -> Option<PrivateSocket> {
        self.0.private_socket(s)
    }
    fn error_table(&self) -> &'static [ErrorRow] {
        self.0.error_table()
    }
}

/// The catalog's answers: the documented examples with ids, sizes, states and client ids
/// changed (`synthetic`, built on the documented shape).
struct Script {
    m: Kraken,
}

fn errors(list: &[&str]) -> String {
    json!({ "error": list }).to_string()
}

impl VenueScript for Script {
    type M = Catalog;

    fn mapper(&self) -> Catalog {
        Catalog(self.m.clone())
    }

    fn settings(&self) -> HashMap<String, String> {
        settings()
    }

    fn signer(&self) -> Arc<dyn Signer> {
        Arc::new(KrakenSigner::new(SecretString::from("otw-test-key".to_string()), SecretString::from(DOC_SECRET.to_string())).unwrap())
    }

    fn symbol(&self) -> &'static str {
        "XBTUSD"
    }

    fn qty(&self) -> Decimal {
        d("0.01")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        match closed {
            None => PAIRS.to_string(),
            Some(w) => with(PAIRS, |v| v["result"]["XXBTZUSD"]["status"] = json!(w)),
        }
    }

    fn closed_word(&self) -> &'static str {
        "cancel_only"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::Filled => "closed",
            OrderStatus::Cancelled => "canceled",
            OrderStatus::Expired => "expired",
            _ => "open",
        }
    }

    fn ack(&self, _client_id: &str, venue_id: &str) -> String {
        with(ADD, |v| v["result"]["txid"] = json!([venue_id]))
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let template: Value = serde_json::from_str(OPEN).unwrap();
        let mut open = serde_json::Map::new();
        for r in rows {
            let mut o = template["result"]["open"]["OQCLML-BW3P3-BUCMWZ"].clone();
            o["status"] = json!(r.word);
            o["cl_ord_id"] = json!(r.client_id);
            o["descr"]["type"] = json!(if r.side == Side::Buy { "buy" } else { "sell" });
            o["descr"]["price"] = json!(self.price().to_string());
            o["vol"] = json!(r.qty.to_string());
            o["vol_exec"] = json!(r.filled.to_string());
            open.insert(r.venue_id.to_string(), o);
        }
        json!({ "error": [], "result": { "open": open } }).to_string()
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let template: Value = serde_json::from_str(TRADES).unwrap();
        let mut trades = serde_json::Map::new();
        for r in rows {
            let mut f = template["result"]["trades"]["THVRQM-33VKH-UCI7BS"].clone();
            f["ordertxid"] = json!(r.venue_id);
            f["vol"] = json!(r.qty.to_string());
            f["price"] = json!(r.price.to_string());
            f["fee"] = json!(r.fee.to_string());
            trades.insert(r.exec_id.to_string(), f);
        }
        json!({ "error": [], "result": { "trades": trades } }).to_string()
    }

    fn fee_currency(&self) -> &'static str {
        "USD"
    }

    fn positions(&self, _rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        String::new()
    }

    fn ok(&self) -> String {
        CANCEL.to_string()
    }

    fn dry_run_ok(&self) -> String {
        r#"{"error":[],"result":{"descr":{"order":"buy 0.01000000 XBTUSD @ limit 30000.0"}}}"#.to_string()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        match r {
            VenueRefusal::Funds => Answer::new(200, errors(&["EOrder:Insufficient funds"])),
            VenueRefusal::ReduceOnly => Answer::new(200, errors(&["EOrder:Reduce only:No position exists"])),
            VenueRefusal::NotFound => Answer::new(200, errors(&["EOrder:Unknown order"])),
            // A cancel that cancelled nothing.
            VenueRefusal::Final => Answer::new(200, r#"{"error":[],"result":{"count":0}}"#),
            VenueRefusal::RateLimited(s) => Answer::new(200, errors(&[&format!("EService: Throttled: {}", CATALOG_NOW_MS / 1000 + i64::try_from(s).unwrap())])),
            // Kraken has no clock window: a nonce out of order is its refusal of the moment.
            VenueRefusal::ClockSkew => Answer::new(200, errors(&["EAPI:Invalid nonce"])),
            VenueRefusal::Unmapped => Answer::new(200, errors(&["EOrder:Something new"])),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            VenueRefusal::PositionModeMismatch => Answer::new(200, errors(&["EGeneral:Invalid arguments"])),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_passes() {
    let r = run_catalog(&Script { m: Kraken::new().unwrap() }).await;
    println!("ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    for t in ["T01", "T02", "T03+T24", "T04", "T05", "T09", "T10", "T11", "T12", "T15", "T19", "T20", "T21", "T22", "T26", "T30", "T31+T34", "T32", "E2D session check"] {
        assert!(r.ran.contains(&t), "{t} not run: {:?}", r.ran);
    }
}

// ── Through the trader, what the catalog does not reach ──

mod trader {
    use super::*;
    use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, TradeError, Trader};
    use otw_connector_sdk::Driver;
    use otw_connector_testkit::{driver_against, ManualClock};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Env {
        server: MockServer,
        driver: Driver<Kraken>,
        trader: Trader<Kraken>,
        signer: KrakenSigner,
        settings: HashMap<String, String>,
    }

    impl Env {
        async fn new() -> Self {
            let server = MockServer::start().await;
            let clock = ManualClock::at(otw_connector_testkit::ts(CATALOG_NOW_MS / 1000));
            let driver = driver_against(Kraken::new().unwrap(), &server, clock);
            Mock::given(method("GET")).and(path("/0/public/AssetPairs")).respond_with(ResponseTemplate::new(200).set_body_string(PAIRS)).mount(&server).await;
            let signer = KrakenSigner::new(SecretString::from("otw-test-key".to_string()), SecretString::from(DOC_SECRET.to_string())).unwrap();
            Env { server, driver, trader: Trader::new(), signer, settings: settings() }
        }

        fn account(&self) -> Account<'_> {
            Account { settings: &self.settings, signer: &self.signer, scope: "kraken-test" }
        }

        async fn answer(&self, p: &str, needle: &str, body: &str) {
            Mock::given(method("POST")).and(path(p)).and(body_string_contains(needle)).respond_with(ResponseTemplate::new(200).set_body_string(body)).mount(&self.server).await;
        }

        async fn bodies(&self, p: &str) -> Vec<String> {
            self.server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == p).map(|r| String::from_utf8_lossy(&r.body).to_string()).collect()
        }
    }

    /// A ticket from the account read's spelling (`XBT/USD`) is checked against that pair's
    /// rules and sent under its altname.
    #[tokio::test]
    async fn a_ticket_on_the_wsname_goes_out_under_the_altname() {
        let e = Env::new().await;
        e.answer("/0/private/AddOrder", "cl_ord_id=w1", &with(ADD, |v| v["result"]["txid"] = json!(["OW1"]))).await;
        let o = order("XBT/USD", "w1", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
        let placed = e.trader.place(&e.driver, &e.account(), &[o], &TicketOptions::default()).await.unwrap();
        assert_eq!(placed[0].venue_id, "OW1");
        let sent = e.bodies("/0/private/AddOrder").await;
        assert!(sent[0].contains("pair=XBTUSD") && sent[0].contains("oflags=fciq"), "{sent:?}");
        // Off the pair's tick: refused before I/O on the wsname as on the altname.
        let off = order("XBT/USD", "w2", Side::Buy, OrderType::Limit, "0.01", Some("30000.05"), None);
        assert!(matches!(e.trader.place(&e.driver, &e.account(), &[off], &TicketOptions::default()).await, Err(TradeError::Refused(_))));
        assert_eq!(e.bodies("/0/private/AddOrder").await.len(), 1);
    }

    /// A robot's 24-character key goes out as a UUID, and the lookup after a lost answer finds
    /// the order however the venue spells the UUID back.
    #[tokio::test]
    async fn a_robot_order_whose_answer_is_lost_is_found_by_its_packed_id() {
        let e = Env::new().await;
        Mock::given(method("POST")).and(path("/0/private/AddOrder")).respond_with(ResponseTemplate::new(502)).mount(&e.server).await;
        let uuid = pack(ROBOT).unwrap();
        let echoed = with(OPEN, |v| {
            let mut o = v["result"]["open"]["OB5VMB-B4U2U-DK2WRW"].clone();
            o["cl_ord_id"] = json!(uuid.replace('-', "").to_ascii_uppercase());
            v["result"]["open"] = json!({ "OROBOT-1": o });
        });
        e.answer("/0/private/OpenOrders", "cl_ord_id=", &echoed).await;
        let o = order("XBTUSD", ROBOT, Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
        let placed = e.trader.place(&e.driver, &e.account(), &[o], &TicketOptions::default()).await.unwrap();
        assert_eq!(placed[0].venue_id, "OROBOT-1");
        let sent = e.bodies("/0/private/AddOrder").await;
        assert_eq!(sent.len(), 1, "never sent twice");
        assert!(sent[0].contains(&format!("cl_ord_id={uuid}")), "{sent:?}");
        let looked = e.bodies("/0/private/OpenOrders").await;
        assert!(looked[0].contains(&format!("cl_ord_id={uuid}")), "{looked:?}");
    }

    /// A placed order the venue answers with a warning is placed, not refused.
    #[tokio::test]
    async fn a_warning_beside_the_txid_is_a_placed_order() {
        let e = Env::new().await;
        e.answer("/0/private/AddOrder", "cl_ord_id=warn", &with(ADD, |v| v["error"] = json!(["WGeneral:Some advisory"]))).await;
        let o = order("XBTUSD", "warn", Side::Buy, OrderType::Limit, "0.01", Some("30000"), None);
        let placed = e.trader.place(&e.driver, &e.account(), &[o], &TicketOptions::default()).await.unwrap();
        assert_eq!(placed[0].venue_id, "OU22CG-KLAF2-FWUDD7");
    }

    /// A cancel answered `count: 0`: the order is read by its id and its end said.
    #[tokio::test]
    async fn a_cancel_that_reached_nothing_reads_how_the_order_ended() {
        let e = Env::new().await;
        e.answer("/0/private/CancelOrder", "txid=OBCMZD-JIEE7-77TH3F", r#"{"error":[],"result":{"count":0}}"#).await;
        e.answer("/0/private/QueryOrders", "txid=OBCMZD-JIEE7-77TH3F", QUERY).await;
        let c = e.trader.cancel(&e.driver, &e.account(), "XBT/USD", "OBCMZD-JIEE7-77TH3F").await.unwrap();
        assert_eq!(c, Cancelled::AlreadyFinal(OrderStatus::Filled));
    }

    /// Amend in place: the order is read by its id, checked against the rules, and changed
    /// under the same ids.
    #[tokio::test]
    async fn an_amend_is_checked_against_the_rules_and_sent_in_place() {
        let e = Env::new().await;
        e.answer("/0/private/QueryOrders", "txid=OQCLML-BW3P3-BUCMWZ", &with(OPEN, |v| v["result"] = v["result"]["open"].clone())).await;
        e.answer("/0/private/AmendOrder", "txid=OQCLML-BW3P3-BUCMWZ", AMEND).await;
        let out = e.trader.modify(&e.driver, &e.account(), "XBT/USD", "OQCLML-BW3P3-BUCMWZ", &OrderAmend { limit: Some(Price::new(d("30020.0"))), ..Default::default() }).await.unwrap();
        assert!(out.is_none(), "changed in place");
        let sent = e.bodies("/0/private/AmendOrder").await;
        assert!(sent.len() == 1 && sent[0].contains("limit_price=30020") && sent[0].contains("txid=OQCLML-BW3P3-BUCMWZ"), "{sent:?}");
        let off = e.trader.modify(&e.driver, &e.account(), "XBT/USD", "OQCLML-BW3P3-BUCMWZ", &OrderAmend { limit: Some(Price::new(d("30020.05"))), ..Default::default() }).await;
        assert!(matches!(off, Err(TradeError::Refused(_))), "{off:?}");
        assert_eq!(e.bodies("/0/private/AmendOrder").await.len(), 1);
    }

    /// A post-only order's limit moved through the trader: the order read by its id carries
    /// `oflags` `post`, and AmendOrder goes with `post_only=true`.
    #[tokio::test]
    async fn a_post_only_amend_through_the_trader_stays_post_only() {
        let e = Env::new().await;
        let post = with(OPEN, |v| {
            v["result"] = v["result"]["open"].clone();
            v["result"]["OQCLML-BW3P3-BUCMWZ"]["oflags"] = json!("post,fciq");
        });
        e.answer("/0/private/QueryOrders", "txid=OQCLML-BW3P3-BUCMWZ", &post).await;
        e.answer("/0/private/AmendOrder", "txid=OQCLML-BW3P3-BUCMWZ", AMEND).await;
        let out = e.trader.modify(&e.driver, &e.account(), "XBT/USD", "OQCLML-BW3P3-BUCMWZ", &OrderAmend { limit: Some(Price::new(d("30020.0"))), ..Default::default() }).await.unwrap();
        assert!(out.is_none());
        let sent = e.bodies("/0/private/AmendOrder").await;
        assert!(sent.len() == 1 && sent[0].contains("post_only=true") && sent[0].contains("limit_price=30020"), "{sent:?}");
    }

    /// The whole account in one CancelAll, the open orders read before and after.
    #[tokio::test]
    async fn the_kill_switch_is_one_cancel_all() {
        let e = Env::new().await;
        Mock::given(method("POST")).and(path("/0/private/OpenOrders")).respond_with(ResponseTemplate::new(200).set_body_string(OPEN)).up_to_n_times(1).mount(&e.server).await;
        Mock::given(method("POST")).and(path("/0/private/OpenOrders")).respond_with(ResponseTemplate::new(200).set_body_string(r#"{"error":[],"result":{"open":{}}}"#)).mount(&e.server).await;
        e.answer("/0/private/CancelAll", "nonce=", r#"{"error":[],"result":{"count":2}}"#).await;
        assert_eq!(e.trader.cancel_all(&e.driver, &e.account()).await.unwrap(), 2);
        assert_eq!(e.bodies("/0/private/CancelAll").await.len(), 1);
    }

    /// A key's signed calls go out one at a time, so their nonces arrive in order.
    #[tokio::test]
    async fn signed_calls_on_one_key_take_turns() {
        let e = Env::new().await;
        Mock::given(method("POST"))
            .and(path("/0/private/QueryOrders"))
            .respond_with(ResponseTemplate::new(200).set_body_string(QUERY).set_delay(std::time::Duration::from_millis(150)))
            .mount(&e.server)
            .await;
        let a = e.account();
        let (x, y) = tokio::join!(e.trader.by_venue_id(&e.driver, &a, "XBTUSD", "OBCMZD-JIEE7-77TH3F"), e.trader.by_venue_id(&e.driver, &a, "XBTUSD", "OMMDB2-FSB6Z-7W3HPO"));
        assert!(x.unwrap().is_some() && y.unwrap().is_some());
        let nonces: Vec<u64> = e.bodies("/0/private/QueryOrders").await.iter().map(|b| b.trim_start_matches("nonce=").split('&').next().unwrap().parse().unwrap()).collect();
        assert_eq!(nonces.len(), 2);
        assert!(nonces[0] < nonces[1], "the venue saw {nonces:?}");
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

/// The live pair list decodes to sound rules for every pair, and an order signed with a key
/// Kraken never issued is answered with `EAPI:Invalid key`, mapped Auth (nothing is placed: no
/// account holds that key). Run once only: repeated unknown keys lock the IP out for about 20
/// minutes (error guide, `EGeneral:Temporary lockout`). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers kraken::trade::tests::public` (add the
/// test harness flag `nocapture` to see the counts).
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if std::env::var("OTW_NET_TEST").as_deref() != Ok("1") {
        return;
    }
    let m = Kraken::new().unwrap();
    let d = Driver::new(m.clone(), DriverConfig::new("kraken"), reqwest::Client::new());
    let junk = settings();
    let signer = KrakenSigner::new(SecretString::from("otw-net-probe-not-a-key".to_string()), SecretString::from(DOC_SECRET.to_string())).unwrap();
    let a = Account { settings: &junk, signer: &signer, scope: "net" };
    let trader: Trader<Kraken> = Trader::new();
    let rules = trader.all_rules(&d, &a).await.unwrap();
    assert!(rules.len() > 100, "{} pairs", rules.len());
    for r in &rules {
        r.sound().unwrap_or_else(|e| panic!("{e}"));
        assert!(r.tick.is_some() && r.lot_step.is_some(), "{} states no tick or step", r.instrument.native);
    }
    let btc = rules.iter().find(|r| r.instrument.native == "XBTUSD").unwrap();
    let ws = rules.iter().find(|r| r.instrument.native == "XBT/USD").unwrap();
    println!("Kraken: {} rule rows; XBTUSD {btc:?}; XBT/USD status {:?}", rules.len(), ws.status);
    let o = order("XBTUSD", "otwnetprobe", Side::Buy, OrderType::Limit, "0.0001", Some("1000"), None);
    let req = m.place(&junk, &o, &placing("otwnetprobe")).unwrap();
    let frame = d.send_as(req, Some(&signer)).await.unwrap();
    let e = m.decode_error(&frame).unwrap_or_else(|| panic!("Kraken accepted an order with no valid key"));
    println!("junk key: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
    assert_eq!((e.canon, e.code.as_deref(), e.mapped), (Canon::Auth, Some("EAPI:Invalid key"), true), "{e:?}");
}
