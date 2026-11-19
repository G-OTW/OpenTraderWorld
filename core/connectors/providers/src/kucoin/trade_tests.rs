//! KuCoin orders, from the documentation alone. Fixtures under `fixtures/trade/`, each the
//! example of the page named, converted from the page's YAML (REST) or JSON (stream) example to
//! JSON with its values unchanged, read 2026-11-11 (`D` = https://www.kucoin.com/docs-new/):
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_spot_add_order.json` | D/rest/spot-trading/orders/add-order |
//! | `doc_spot_add_order_test.json` | D/rest/spot-trading/orders/add-order-test |
//! | `doc_spot_cancel_order.json` | D/rest/spot-trading/orders/cancel-order-by-orderld |
//! | `doc_spot_cancel_all_by_symbol.json` | D/rest/spot-trading/orders/cancel-all-orders-by-symbol |
//! | `doc_spot_order_by_clientoid.json` | D/rest/spot-trading/orders/get-order-by-clientoid |
//! | `doc_spot_order_by_id.json` | D/rest/spot-trading/orders/get-order-by-orderld |
//! | `doc_spot_open_orders_page.json` | D/rest/spot-trading/orders/get-open-orders-by-page |
//! | `doc_spot_trade_history.json` | D/rest/spot-trading/orders/get-trade-history |
//! | `doc_spot_symbols.json` | D/rest/spot-trading/market-data/get-all-symbols |
//! | `doc_spot_bullet_private.json` | D/websocket-api/base-info/get-private-token-spot-margin |
//! | `doc_ws_spot_order_{open,update,match,filled,canceled}.json` | D/3470073w0 (Order V2, `/spotMarket/tradeOrdersV2`) |
//! | `doc_fut_add_order.json` | D/rest/futures-trading/orders/add-order |
//! | `doc_fut_add_order_test.json` | D/rest/futures-trading/orders/add-order-test |
//! | `doc_fut_cancel_order.json` | D/rest/futures-trading/orders/cancel-order-by-orderld |
//! | `doc_fut_cancel_all.json` | D/rest/futures-trading/orders/cancel-all-orders |
//! | `doc_fut_cancel_all_stops.json` | D/rest/futures-trading/orders/cancel-all-stop-orders |
//! | `doc_fut_order_by_clientoid.json` | D/rest/futures-trading/get-stop-order-by-clientoid |
//! | `doc_fut_order_by_id.json` | D/rest/futures-trading/orders/get-order-by-orderld |
//! | `doc_fut_order_list.json` | D/rest/futures-trading/orders/get-order-list |
//! | `doc_fut_stop_order_list.json` | D/rest/futures-trading/orders/get-stop-order-list |
//! | `doc_fut_trade_history.json` | D/rest/futures-trading/orders/get-trade-history |
//! | `doc_fut_contracts_active.json` | D/rest/futures-trading/market-data/get-all-symbols |
//! | `doc_fut_contract.json` | D/rest/futures-trading/market-data/get-symbol |
//! | `doc_fut_position_isolated.json`, `doc_fut_position_cross.json` | D/rest/futures-trading/positions/get-position-details (examples 1 and 2) |
//! | `doc_fut_position_list.json` | D/rest/futures-trading/positions/get-position-list (example 1) |
//! | `doc_fut_position_mode.json` | D/rest/futures-trading/positions/get-position-mode |
//! | `doc_fut_margin_mode.json` | D/rest/futures-trading/positions/get-margin-mode |
//! | `doc_fut_bullet_private.json` | D/websocket-api/base-info/get-private-token-futures |
//! | `doc_ws_fut_order_{open,match,match_adl,filled,canceled}.json` | D/3470090w0 (`/contractMarket/tradeOrders`) |
//! | `doc_ws_fut_stop_order.json` | D/3470091w0 (`/contractMarket/advancedOrders`), written there as a JavaScript object literal and transcribed to JSON |
//! | `doc_ws_fut_position.json` | D/3470093w0 (`/contract/positionAll`, "Isolated Margin", its inline comment dropped) |
//!
//! The catalog script ([`Script`]) answers with these examples with only their ids, sizes,
//! prices, symbol and state fields changed: those answers are `synthetic`, built on the
//! documented shape. Every error body is `synthetic` too: the doc lists codes, not bodies, and
//! the envelope is the one every documented answer carries (`code`, `msg`).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{Capabilities, Command, CorrId, Decimal, Envelope, InstrumentId, Mapped, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce};
use otw_connector_sdk::trade::{
    Ack, Canon, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, MarginMode, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper,
    TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, SignableRequest, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript, CATALOG_NOW_MS};
use serde_json::{json, Value};

use super::super::signer::KucoinSigner;
use super::super::{Book, Kucoin, Session};

const SPOT_ADD: &str = include_str!("fixtures/trade/doc_spot_add_order.json");
const SPOT_TEST: &str = include_str!("fixtures/trade/doc_spot_add_order_test.json");
const SPOT_CANCEL: &str = include_str!("fixtures/trade/doc_spot_cancel_order.json");
const SPOT_CANCEL_ALL: &str = include_str!("fixtures/trade/doc_spot_cancel_all_by_symbol.json");
const SPOT_BY_CID: &str = include_str!("fixtures/trade/doc_spot_order_by_clientoid.json");
const SPOT_BY_ID: &str = include_str!("fixtures/trade/doc_spot_order_by_id.json");
const SPOT_OPEN: &str = include_str!("fixtures/trade/doc_spot_open_orders_page.json");
const SPOT_FILLS: &str = include_str!("fixtures/trade/doc_spot_trade_history.json");
const SPOT_SYMBOLS: &str = include_str!("fixtures/trade/doc_spot_symbols.json");
const SPOT_BULLET: &str = include_str!("fixtures/trade/doc_spot_bullet_private.json");
const WS_SPOT_OPEN: &str = include_str!("fixtures/trade/doc_ws_spot_order_open.json");
const WS_SPOT_UPDATE: &str = include_str!("fixtures/trade/doc_ws_spot_order_update.json");
const WS_SPOT_MATCH: &str = include_str!("fixtures/trade/doc_ws_spot_order_match.json");
const WS_SPOT_FILLED: &str = include_str!("fixtures/trade/doc_ws_spot_order_filled.json");
const WS_SPOT_CANCELED: &str = include_str!("fixtures/trade/doc_ws_spot_order_canceled.json");
const FUT_ADD: &str = include_str!("fixtures/trade/doc_fut_add_order.json");
const FUT_TEST: &str = include_str!("fixtures/trade/doc_fut_add_order_test.json");
const FUT_CANCEL: &str = include_str!("fixtures/trade/doc_fut_cancel_order.json");
const FUT_CANCEL_ALL: &str = include_str!("fixtures/trade/doc_fut_cancel_all.json");
const FUT_CANCEL_STOPS: &str = include_str!("fixtures/trade/doc_fut_cancel_all_stops.json");
const FUT_BY_CID: &str = include_str!("fixtures/trade/doc_fut_order_by_clientoid.json");
const FUT_BY_ID: &str = include_str!("fixtures/trade/doc_fut_order_by_id.json");
const FUT_LIST: &str = include_str!("fixtures/trade/doc_fut_order_list.json");
const FUT_STOPS: &str = include_str!("fixtures/trade/doc_fut_stop_order_list.json");
const FUT_FILLS: &str = include_str!("fixtures/trade/doc_fut_trade_history.json");
const FUT_CONTRACTS: &str = include_str!("fixtures/trade/doc_fut_contracts_active.json");
const FUT_CONTRACT: &str = include_str!("fixtures/trade/doc_fut_contract.json");
const FUT_POS_ISOLATED: &str = include_str!("fixtures/trade/doc_fut_position_isolated.json");
const FUT_POS_CROSS: &str = include_str!("fixtures/trade/doc_fut_position_cross.json");
const FUT_POS_LIST: &str = include_str!("fixtures/trade/doc_fut_position_list.json");
const FUT_MODE: &str = include_str!("fixtures/trade/doc_fut_position_mode.json");
const FUT_MARGIN: &str = include_str!("fixtures/trade/doc_fut_margin_mode.json");
const FUT_BULLET: &str = include_str!("fixtures/trade/doc_fut_bullet_private.json");
const WS_FUT_OPEN: &str = include_str!("fixtures/trade/doc_ws_fut_order_open.json");
const WS_FUT_MATCH: &str = include_str!("fixtures/trade/doc_ws_fut_order_match.json");
const WS_FUT_ADL: &str = include_str!("fixtures/trade/doc_ws_fut_order_match_adl.json");
const WS_FUT_FILLED: &str = include_str!("fixtures/trade/doc_ws_fut_order_filled.json");
const WS_FUT_CANCELED: &str = include_str!("fixtures/trade/doc_ws_fut_order_canceled.json");
const WS_FUT_STOP: &str = include_str!("fixtures/trade/doc_ws_fut_stop_order.json");
const WS_FUT_POSITION: &str = include_str!("fixtures/trade/doc_ws_fut_position.json");

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "XXXXXXXXXX"), ("api_secret", "secret-test"), ("api_passphrase", "my passphrase")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn spot() -> Kucoin {
    Kucoin::spot().unwrap()
}

fn futures() -> Kucoin {
    Kucoin::futures().unwrap()
}

fn order(m: &Kucoin, symbol: &str, key: &str, side: Side, t: OrderType, qty: &str, price: Option<&str>) -> OrderRequest {
    let (l, s) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new(m.book().venue(), symbol),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: if l { price.map(|p| Price::new(d(p))) } else { None },
        stop: if s { price.map(|p| Price::new(d(p))) } else { None },
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

fn q<'a>(req: &'a HttpRequest, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// `XBTUSDTM`: 0.001 BTC per contract (Get All Symbols example); `XRPUSDTM` and `PEOPLEUSDTM`
/// (in the order examples, not in the symbol examples) given test sizes of 10 and 1.
fn sizes() -> ContractSizes {
    let mut s = ContractSizes::new();
    s.insert("XBTUSDTM", d("0.001"));
    s.insert("ETHUSDTM", d("0.01"));
    s.insert("XRPUSDTM", d("10"));
    s.insert("PEOPLEUSDTM", d("1"));
    s
}

fn futures_placing(cid: &str, position: PositionSide, margin: MarginMode) -> Placing {
    Placing { margin: Some(margin), contract_size: Some(d("0.001")), ..Placing::new(cid, position, CorrId(1)) }
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

// ── Requests ──

/// Add Order, Request Example 1 ("One-way Position Mode") field for field, except what OTW
/// leaves to the venue on purpose: no `leverage` (read only, never set), no `remark`, and the
/// size as `qty` in the base coin (1 lot of XBTUSDTM = 0.001) rather than `size` in lots.
#[test]
fn a_futures_limit_is_the_documented_body() {
    let m = futures();
    let doc: Value = serde_json::from_str(
        r#"{"clientOid": "5c52e11203aa677f33e493fb", "symbol": "XBTUSDTM", "marginMode": "ISOLATED", "positionSide": "BOTH", "side": "buy", "type": "limit", "qty": "0.001", "price": "100000", "timeInForce": "GTC"}"#,
    )
    .unwrap();
    let o = order(&m, "XBTUSDTM", "5c52e11203aa677f33e493fb", Side::Buy, OrderType::Limit, "0.001", Some("100000"));
    let req = m.place(&settings(), &o, &futures_placing("5c52e11203aa677f33e493fb", PositionSide::Net, MarginMode::isolated())).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.endpoint_id), (Method::Post, "/api/v1/orders", true, "fut_place"));
    assert_eq!(body(&req), doc);
}

/// Request Example 2 ("Stop Order"): a sell stop-limit triggers when the trade price falls to
/// it (`stop` down, `stopPriceType` TP), cross margin.
#[test]
fn a_futures_stop_limit_is_the_documented_stop_body() {
    let m = futures();
    let mut o = order(&m, "XBTUSDTM", "s1", Side::Sell, OrderType::StopLimit, "0.001", Some("30000"));
    o.stop = Some(Price::new(d("29000")));
    let b = body(&m.place(&settings(), &o, &futures_placing("s1", PositionSide::Net, MarginMode::cross())).unwrap());
    assert_eq!(
        (b["type"].as_str(), b["price"].as_str(), b["stop"].as_str(), b["stopPrice"].as_str(), b["stopPriceType"].as_str(), b["marginMode"].as_str()),
        (Some("limit"), Some("30000"), Some("down"), Some("29000"), Some("TP"), Some("CROSS"))
    );
    // A buy stop (market once triggered) rises to its price and sends no time in force.
    let buy = order(&m, "XBTUSDTM", "s2", Side::Buy, OrderType::Stop, "0.002", Some("31000"));
    let b = body(&m.place(&settings(), &buy, &futures_placing("s2", PositionSide::Long, MarginMode::cross())).unwrap());
    assert_eq!((b["type"].as_str(), b["stop"].as_str(), b["positionSide"].as_str(), b["qty"].as_str()), (Some("market"), Some("up"), Some("LONG"), Some("0.002")));
    assert!(b.get("timeInForce").is_none() && b.get("price").is_none());
}

#[test]
fn a_futures_order_is_whole_contracts_with_its_margin_read_or_nothing_leaves() {
    let m = futures();
    let o = order(&m, "XBTUSDTM", "k", Side::Buy, OrderType::Limit, "0.0015", Some("30000"));
    let e = m.place(&settings(), &o, &futures_placing("k", PositionSide::Net, MarginMode::cross())).unwrap_err();
    assert!(e.message.contains("0.001 or 0.002"), "{}", e.message);
    let ok = order(&m, "XBTUSDTM", "k", Side::Buy, OrderType::Limit, "0.002", Some("30000"));
    let no_margin = Placing { contract_size: Some(d("0.001")), ..Placing::new("k", PositionSide::Net, CorrId(1)) };
    assert!(m.place(&settings(), &ok, &no_margin).unwrap_err().message.contains("margin mode"));
    let no_size = Placing { margin: Some(MarginMode::cross()), ..Placing::new("k", PositionSide::Net, CorrId(1)) };
    assert!(m.place(&settings(), &ok, &no_size).is_err(), "a contract size never guessed");
    let mut ro = ok.clone();
    ro.opts.reduce_only = true;
    ro.opts.post_only = true;
    let b = body(&m.place(&settings(), &ro, &futures_placing("k", PositionSide::Short, MarginMode::isolated())).unwrap());
    assert_eq!((b["reduceOnly"].as_bool(), b["postOnly"].as_bool(), b["positionSide"].as_str()), (Some(true), Some(true), Some("SHORT")));
}

#[test]
fn a_spot_order_sizes_in_the_base_coin_and_takes_no_stop_nor_reduce_only() {
    let m = spot();
    let mkt = m.place(&settings(), &order(&m, "BTC-USDT", "m1", Side::Buy, OrderType::Market, "0.001", None), &Placing::new("m1", PositionSide::Net, CorrId(1))).unwrap();
    assert_eq!((mkt.path.as_str(), mkt.endpoint_id), ("/api/v1/hf/orders", "spot_place"));
    let b = body(&mkt);
    assert_eq!((b["type"].as_str(), b["size"].as_str(), b["side"].as_str(), b["clientOid"].as_str()), (Some("market"), Some("0.001"), Some("buy"), Some("m1")));
    assert!(b.get("funds").is_none() && b.get("timeInForce").is_none(), "{b}");
    let mut lim = order(&m, "BTC-USDT", "l1", Side::Sell, OrderType::Limit, "0.001", Some("50000"));
    lim.tif = TimeInForce::Ioc;
    let b = body(&m.place(&settings(), &lim, &Placing::new("l1", PositionSide::Net, CorrId(1))).unwrap());
    assert_eq!((b["type"].as_str(), b["price"].as_str(), b["timeInForce"].as_str()), (Some("limit"), Some("50000"), Some("IOC")));
    assert!(m.place(&settings(), &order(&m, "BTC-USDT", "x", Side::Buy, OrderType::Stop, "0.001", Some("1")), &Placing::new("x", PositionSide::Net, CorrId(1))).is_err());
    let mut ro = order(&m, "BTC-USDT", "r", Side::Sell, OrderType::Limit, "0.001", Some("50000"));
    ro.opts.reduce_only = true;
    assert!(m.place(&settings(), &ro, &Placing::new("r", PositionSide::Net, CorrId(1))).is_err());
    let mut gtd = order(&m, "BTC-USDT", "g", Side::Sell, OrderType::Limit, "0.001", Some("50000"));
    gtd.tif = TimeInForce::Day;
    assert!(m.place(&settings(), &gtd, &Placing::new("g", PositionSide::Net, CorrId(1))).is_err(), "GTT is not offered");
}

/// The signature of an order body: the documented plain text (`timestamp + METHOD + path +
/// body`), signed by an independent HMAC-SHA256 (Python's `hmac` module, computed once and
/// pinned here) over the exact bytes this mapper sends. A cancel signs its query unencoded.
#[test]
fn an_order_and_a_cancel_sign_like_an_independent_implementation() {
    let m = spot();
    let o = order(&m, "BTC-USDT", "5c52e11203aa677f33e493fb", Side::Buy, OrderType::Limit, "0.00001", Some("50000"));
    let req = m.place(&settings(), &o, &Placing::new("5c52e11203aa677f33e493fb", PositionSide::Net, CorrId(1))).unwrap();
    let sent = String::from_utf8(req.body.clone().unwrap().to_vec()).unwrap();
    assert_eq!(sent, r#"{"clientOid":"5c52e11203aa677f33e493fb","price":"50000","side":"buy","size":"0.00001","symbol":"BTC-USDT","timeInForce":"GTC","type":"limit"}"#);
    let signer = KucoinSigner::from_secrets(&settings()).unwrap();
    let now = otw_connector_model::Timestamp::from_millis(1_729_577_515_473).unwrap();
    let sign = |r: &HttpRequest| {
        let mut s = SignableRequest { method: r.method, path: r.path.clone(), query: r.query.clone(), headers: vec![], body: r.body.clone() };
        signer.sign(&mut s, now).unwrap();
        s.headers.iter().find(|(k, _)| k == "KC-API-SIGN").map(|(_, v)| v.clone()).unwrap()
    };
    assert_eq!(sign(&req), "N53yxe+kiup1opQs40CyxzEXKkiAHdLBpUQqQkRQ6l0=");
    let cancel = m.cancel(&settings(), "BTC-USDT", "670fd33bf9406e0007ab3945", CorrId(1)).unwrap();
    assert_eq!((cancel.method, cancel.path.as_str(), q(&cancel, "symbol")), (Method::Delete, "/api/v1/hf/orders/670fd33bf9406e0007ab3945", Some("BTC-USDT")));
    assert_eq!(sign(&cancel), "RtmPc1LTFL45yAXsDy75opY5aI05QAmktL5qXGggnmU=");
}

#[test]
fn every_call_goes_where_the_doc_says() {
    let s = settings();
    let sp = spot();
    let fu = futures();
    let cid = sp.query_by_client_id(&s, "BTC-USDT", "k1", CorrId(1)).unwrap();
    assert_eq!((cid[0].path.as_str(), q(&cid[0], "symbol")), ("/api/v1/hf/orders/client-order/k1", Some("BTC-USDT")));
    let fcid = fu.query_by_client_id(&s, "XBTUSDTM", "k1", CorrId(1)).unwrap();
    assert_eq!((fcid[0].path.as_str(), q(&fcid[0], "clientOid")), ("/api/v1/orders/byClientOid", Some("k1")));
    assert_eq!(fu.query_by_venue_id(&s, "XBTUSDTM", "o1", CorrId(1)).unwrap()[0].path, "/api/v1/orders/o1");
    assert_eq!(sp.query_by_venue_id(&s, "BTC-USDT", "o1", CorrId(1)).unwrap()[0].path, "/api/v1/hf/orders/o1");
    let fc = fu.cancel(&s, "XBTUSDTM", "o1", CorrId(1)).unwrap();
    assert_eq!((fc.method, fc.path.as_str()), (Method::Delete, "/api/v1/orders/o1"));
    let all = fu.cancel_all(&s, &["XBTUSDTM".into()], CorrId(1)).unwrap();
    let paths: Vec<(&str, Option<&str>)> = all.iter().map(|r| (r.path.as_str(), q(r, "symbol"))).collect();
    assert_eq!(paths, vec![("/api/v3/orders", Some("XBTUSDTM")), ("/api/v1/stopOrders", Some("XBTUSDTM"))]);
    assert!(all.iter().all(|r| r.method == Method::Delete && r.signed));
    let sall = sp.cancel_all(&s, &["BTC-USDT".into()], CorrId(1)).unwrap();
    assert_eq!((sall[0].path.as_str(), q(&sall[0], "symbol")), ("/api/v1/hf/orders", Some("BTC-USDT")));
    let open = fu.open_orders(&s, None, CorrId(1)).unwrap();
    assert_eq!((open[0].path.as_str(), q(&open[0], "status"), open[1].path.as_str()), ("/api/v1/orders", Some("active"), "/api/v1/stopOrders"));
    assert_eq!(sp.open_orders(&s, None, CorrId(1)).unwrap()[0].path, "/api/v1/hf/orders/active/symbols");
    assert_eq!(sp.open_orders(&s, Some("BTC-USDT"), CorrId(1)).unwrap()[0].path, "/api/v1/hf/orders/active/page");
    let f = fu.fills_since(&s, "XBTUSDTM", 1, 2, CorrId(1)).unwrap();
    assert_eq!((f.path.as_str(), q(&f, "startAt"), q(&f, "endAt"), q(&f, "pageSize")), ("/api/v1/fills", Some("1"), Some("2"), Some("1000")));
    let f = sp.fills_since(&s, "BTC-USDT", 1, 2, CorrId(1)).unwrap();
    assert_eq!((f.path.as_str(), q(&f, "symbol"), q(&f, "limit")), ("/api/v1/hf/fills", Some("BTC-USDT"), Some("100")));
    assert_eq!(fu.fills_window_ms(), Some(7 * 86_400_000));
    assert_eq!(fu.positions(&s, "XBTUSDTM", CorrId(1)).unwrap().unwrap().path, "/api/v2/position");
    assert!(sp.positions(&s, "BTC-USDT", CorrId(1)).unwrap().is_none() && sp.all_positions(&s, CorrId(1)).unwrap().is_empty());
    assert_eq!(fu.account_position_mode(&s, CorrId(1)).unwrap().unwrap().path, "/api/v2/position/getPositionMode");
    assert!(sp.account_position_mode(&s, CorrId(1)).unwrap().is_none());
    let mm = fu.symbol_margin(&s, "XBTUSDTM", CorrId(1)).unwrap().unwrap();
    assert_eq!((mm.path.as_str(), q(&mm, "symbol")), ("/api/v2/position/getMarginMode", Some("XBTUSDTM")));
    assert_eq!(fu.instrument_rules(&s, Some("XBTUSDTM"), CorrId(1)).unwrap().path, "/api/v1/contracts/active");
    assert_eq!(fu.instrument_rules_any_status(&s, "XBTUSDTM", CorrId(1)).unwrap()[0].path, "/api/v1/contracts/XBTUSDTM");
    assert_eq!(sp.instrument_rules(&s, Some("BTC-USDT"), CorrId(1)).unwrap().path, "/api/v2/symbols");
    for m in [&sp, &fu] {
        let b = m.private_socket_bootstrap(&s, &m.private_socket(&s).unwrap(), CorrId(1)).unwrap().unwrap();
        assert_eq!((b.method, b.path.as_str(), b.signed), (Method::Post, "/api/v1/bullet-private", true));
    }
}

/// The order test takes the order's own fields; spot sends no `clientOid`, futures (where it is
/// required) one of its own, never the live order's.
#[test]
fn the_order_test_never_carries_the_live_client_id() {
    let sp = spot();
    let req = sp.dry_run(&settings(), &order(&sp, "BTC-USDT", "live", Side::Buy, OrderType::Limit, "0.001", Some("50000")), &Placing::new("", PositionSide::Net, CorrId(7))).unwrap().unwrap();
    assert_eq!(req.path, "/api/v1/hf/orders/test");
    assert!(body(&req).get("clientOid").is_none());
    let fu = futures();
    let p = Placing { client_id: String::new(), ..futures_placing("", PositionSide::Net, MarginMode::cross()) };
    let req = fu.dry_run(&settings(), &order(&fu, "XBTUSDTM", "live", Side::Buy, OrderType::Stop, "0.001", Some("50000")), &p).unwrap().unwrap();
    assert_eq!((req.path.as_str(), body(&req)["clientOid"].as_str()), ("/api/v1/orders/test", Some("otw-check-1")));
    assert!(fu.decode_dry_run(FUT_TEST.as_bytes()).unwrap().contains("234125150956625920"));
    assert!(sp.decode_dry_run(SPOT_TEST.as_bytes()).unwrap().contains("not placed"));
}

#[test]
fn a_market_order_fill_or_kill_is_refused_before_io() {
    for m in [spot(), futures()] {
        let symbol = if m.book() == Book::Spot { "BTC-USDT" } else { "XBTUSDTM" };
        let mut o = order(&m, symbol, "f", Side::Buy, OrderType::Market, "0.001", None);
        o.tif = TimeInForce::Fok;
        assert_eq!(m.check_order(&o).unwrap_err().kind, otw_connector_model::ErrorKind::Unsupported);
        o.tif = TimeInForce::Ioc;
        assert!(m.check_order(&o).is_ok());
        let mut l = order(&m, symbol, "l", Side::Buy, OrderType::Limit, "0.001", Some("1"));
        l.tif = TimeInForce::Fok;
        assert!(m.check_order(&l).is_ok(), "a limit takes FOK");
    }
    let fu = futures();
    let mut stop = order(&fu, "XBTUSDTM", "s", Side::Buy, OrderType::Stop, "0.001", Some("1"));
    stop.tif = TimeInForce::Fok;
    assert!(fu.check_order(&stop).is_err());
}

#[test]
fn caps_are_narrow_and_per_book() {
    let sp = spot().capabilities().orders.clone();
    assert_eq!(sp.level, otw_connector_model::ExecLevel::E2D);
    assert!(sp.types.contains(&OrderType::Market) && sp.types.contains(&OrderType::Limit) && !sp.types.contains(&OrderType::Stop));
    assert!(sp.post_only && !sp.reduce_only && sp.position_mode.is_none() && !sp.amend && !sp.brackets && !sp.oca && sp.client_id);
    let fu = futures().capabilities().orders.clone();
    assert!(fu.types.contains(&OrderType::Stop) && fu.types.contains(&OrderType::StopLimit) && fu.reduce_only && fu.post_only);
    assert_eq!(fu.position_mode, Some(otw_connector_model::PositionModeCap::Both));
    assert!(!fu.amend && !fu.brackets && !fu.oca && !fu.close_position && fu.triggers.is_empty());
    for c in [&sp, &fu] {
        assert!(!c.tif.contains(&otw_connector_model::TifKind::Gtd) && !c.tif.contains(&otw_connector_model::TifKind::Day));
        assert_eq!(c.dry_run, otw_connector_model::DryRunCap::Validate);
    }
    assert_eq!(spot().client_id_rule().max_len, 40);
}

// ── Answers ──

#[test]
fn documented_acks_and_orders_decode() {
    let sp = spot();
    let fu = futures();
    let ack = sp.decode_ack(SPOT_ADD.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("670fd33bf9406e0007ab3945", Some("5c52e11203aa677f33e493fb")));
    assert_eq!(fu.decode_ack(FUT_ADD.as_bytes()).unwrap().venue_id, "234125150956625920");
    for ok in [SPOT_CANCEL, SPOT_CANCEL_ALL, FUT_CANCEL, FUT_CANCEL_ALL, FUT_CANCEL_STOPS] {
        assert!(sp.decode_error(&otw_connector_testkit::http_frame(1, "x", 200, ok)).is_none());
    }
    assert!(sp.decode_cancel_all(SPOT_CANCEL_ALL.as_bytes()).is_ok());
    // Spot: a market order by funds, partly filled, the rest cancelled (`cancelExist`).
    let (o, _) = sp.decode_orders(SPOT_BY_CID.as_bytes()).unwrap();
    assert_eq!((o[0].status.clone(), o[0].filled_qty, o[0].client_id.as_deref()), (Mapped::Known(OrderStatus::Cancelled), d("0.00000111"), Some("9a1840ed0b5746459ede8e6f232daf2d")));
    let (o, _) = sp.decode_orders(SPOT_BY_ID.as_bytes()).unwrap();
    assert_eq!(o.len(), 1);
    let (open, next) = sp.decode_orders(SPOT_OPEN.as_bytes()).unwrap();
    assert_eq!((open[0].status.clone(), open[0].limit, open[0].qty, next), (Mapped::Known(OrderStatus::New), Some(d("50000")), d("0.00001"), None));
    // Futures: sizes are contracts, read back in the base coin.
    let s = &sizes();
    let (o, _) = fu.decode_orders_in(&DecodeCtx::new("fut_by_cid", s), FUT_BY_CID.as_bytes()).unwrap();
    assert_eq!((o[0].qty, o[0].status.clone(), o[0].position, o[0].symbol.as_str()), (d("10"), Mapped::Known(OrderStatus::New), PositionSide::Net, "XRPUSDTM"));
    let (o, _) = fu.decode_orders_in(&DecodeCtx::new("fut_by_id", s), FUT_BY_ID.as_bytes()).unwrap();
    assert_eq!((o[0].qty, o[0].limit), (d("0.001"), Some(d("0.1"))));
    let (o, _) = fu.decode_orders_in(&DecodeCtx::new("fut_open", s), FUT_LIST.as_bytes()).unwrap();
    assert_eq!((o[0].qty, o[0].client_id.as_deref()), (d("10"), Some("5a80bd847f1811ef8a7faa665a37b3d7")));
    let (o, _) = fu.decode_orders_in(&DecodeCtx::new("fut_open_stops", s), FUT_STOPS.as_bytes()).unwrap();
    assert_eq!(o[0].status, Mapped::Known(OrderStatus::New));
    assert!(fu.decode_orders_in(&DecodeCtx::new("fut_by_id", &ContractSizes::new()), FUT_BY_ID.as_bytes()).is_err(), "a contract size never guessed");
}

#[test]
fn an_untriggered_stop_waits_for_its_trigger() {
    let fu = futures();
    let s = sizes();
    let body = with(FUT_STOPS, |v| {
        let r = &mut v["data"]["items"][0];
        r["symbol"] = json!("XBTUSDTM");
        r["stop"] = json!("down");
        r["stopPrice"] = json!("29000");
        r["stopTriggered"] = json!(false);
    });
    let (o, _) = fu.decode_orders_in(&DecodeCtx::new("fut_open_stops", &s), body.as_bytes()).unwrap();
    assert_eq!((o[0].trigger_pending, o[0].stop, o[0].order_type.as_str()), (true, Some(d("29000")), "limit_stop"));
}

#[test]
fn documented_fills_decode() {
    let sp = spot();
    let (f, next) = sp.decode_fills(SPOT_FILLS.as_bytes()).unwrap();
    assert_eq!(
        (f[0].exec_id.as_str(), f[0].venue_order_id.as_str(), f[0].qty, f[0].price, f[0].fee, f[0].fee_currency.as_str(), f[0].at_ms),
        ("11029373945659392", "6717422bd51c29000775ea03", d("0.00001"), d("67717.6"), d("0.000677176"), "USDT", 1_729_577_515_473)
    );
    assert!(next.is_none(), "one row, under a page");
    let fu = futures();
    let s = sizes();
    let (f, next) = fu.decode_fills_in(&DecodeCtx::new("fut_fills", &s), FUT_FILLS.as_bytes()).unwrap();
    assert_eq!((f[0].exec_id.as_str(), f[0].qty, f[0].fee, f[0].at_ms, f[0].position_only), ("1828954878212", d("0.001"), d("0.05176506"), 1_740_640_088_244, false));
    assert!(next.is_none());
    let adl = with(FUT_FILLS, |v| v["data"]["items"][0]["tradeType"] = json!("adl"));
    assert!(fu.decode_fills_in(&DecodeCtx::new("fut_fills", &s), adl.as_bytes()).unwrap().0[0].position_only);
    let paged = with(FUT_FILLS, |v| v["data"]["totalPage"] = json!(3));
    assert_eq!(fu.decode_fills_in(&DecodeCtx::new("fut_fills", &s), paged.as_bytes()).unwrap().1.as_deref(), Some("page:2"));
}

#[test]
fn documented_positions_mode_and_margin_decode() {
    let fu = futures();
    let s = sizes();
    let ctx = DecodeCtx::new("fut_position", &s);
    let p = fu.decode_positions_in(&ctx, FUT_POS_ISOLATED.as_bytes()).unwrap();
    assert_eq!((p[0].side, p[0].direction, p[0].qty, p[0].leverage, p[0].margin.clone()), (PositionSide::Long, Some(Side::Buy), d("0.001"), Some(d("6.01")), Some(MarginMode::isolated())));
    let p = fu.decode_positions_in(&ctx, FUT_POS_CROSS.as_bytes()).unwrap();
    assert_eq!(p[0].margin, Some(MarginMode::cross()));
    assert_eq!(fu.decode_positions_in(&ctx, FUT_POS_LIST.as_bytes()).unwrap().len(), 1);
    assert_eq!(fu.decode_account_position_mode(FUT_MODE.as_bytes()).unwrap(), Some(PositionMode::Hedge));
    let one_way = with(FUT_MODE, |v| v["data"]["positionMode"] = json!(0));
    assert_eq!(fu.decode_account_position_mode(one_way.as_bytes()).unwrap(), Some(PositionMode::OneWay));
    assert!(fu.decode_account_position_mode(with(FUT_MODE, |v| v["data"]["positionMode"] = json!(2)).as_bytes()).is_err());
    assert_eq!(fu.decode_symbol_margin("XBTUSDTM", FUT_MARGIN.as_bytes()).unwrap(), Some(MarginMode::isolated()));
    // A one-way short: negative quantity, net row.
    let short = with(FUT_POS_CROSS, |v| {
        v["data"][0]["currentQty"] = json!(-3);
        v["data"][0]["positionSide"] = json!("BOTH");
    });
    let p = fu.decode_positions_in(&ctx, short.as_bytes()).unwrap();
    assert_eq!((p[0].side, p[0].direction, p[0].qty), (PositionSide::Net, Some(Side::Sell), d("0.003")));
}

#[test]
fn documented_instrument_lists_decode_to_sound_rules() {
    let (rules, _) = spot().decode_rules(SPOT_SYMBOLS.as_bytes()).unwrap();
    let r = &rules[0];
    assert_eq!(
        (r.instrument.native.as_str(), r.tick, r.lot_step, r.min_qty, r.max_qty, r.min_notional, r.quote_step),
        ("BTC-USDT", Some(d("0.1")), Some(d("0.00000001")), Some(d("0.00001")), Some(d("10000000000")), Some(d("0.1")), Some(d("0.000001")))
    );
    assert_eq!(r.status, TradingStatus::Trading);
    r.sound().unwrap();
    let off = with(SPOT_SYMBOLS, |v| v["data"][0]["enableTrading"] = json!(false));
    assert!(matches!(spot().decode_rules(off.as_bytes()).unwrap().0[0].status, TradingStatus::Closed(_)));
    let auction = with(SPOT_SYMBOLS, |v| v["data"][0]["callauctionIsEnabled"] = json!(true));
    assert_eq!(spot().decode_rules(auction.as_bytes()).unwrap().0[0].status, TradingStatus::Closed("call auction".into()));
    let (rules, _) = futures().decode_rules(FUT_CONTRACTS.as_bytes()).unwrap();
    let x = rules.iter().find(|r| r.instrument.native == "XBTUSDTM").unwrap();
    assert_eq!(
        (x.contract_size, x.tick, x.lot_step, x.min_qty, x.max_qty, x.market_max_qty, x.max_price),
        (Some(d("0.001")), Some(d("0.1")), Some(d("0.001")), Some(d("0.001")), Some(d("1000")), Some(d("1000")), Some(d("1000000")))
    );
    assert_eq!(rules.len(), 2);
    for r in &rules {
        r.sound().unwrap();
    }
    // One contract by name (a contract missing from the active list), and the closed words.
    let (one, _) = futures().decode_rules(FUT_CONTRACT.as_bytes()).unwrap();
    assert_eq!(one[0].instrument.native, "XBTUSDTM");
    let paused = with(FUT_CONTRACT, |v| v["data"]["status"] = json!("CancelOnly"));
    assert_eq!(futures().decode_rules(paused.as_bytes()).unwrap().0[0].status, TradingStatus::Closed("CancelOnly".into()));
    let inverse = with(FUT_CONTRACT, |v| v["data"]["isInverse"] = json!(true));
    assert!(futures().decode_rules(inverse.as_bytes()).unwrap().0.is_empty(), "coin-margined contracts are not served");
}

#[test]
fn every_status_word_maps_and_an_unknown_one_stays_unknown() {
    let m = spot();
    for (w, s) in [("open", OrderStatus::New), ("partial", OrderStatus::PartiallyFilled), ("filled", OrderStatus::Filled), ("canceled", OrderStatus::Cancelled)] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    assert_eq!(m.status_of("done"), Mapped::Unknown("done".into()));
    // Spot: `active` decides; a done order not all filled had its rest cancelled.
    let row = |active: Value, cancel: bool, deal: &str| {
        with(SPOT_OPEN, |v| {
            let r = &mut v["data"]["items"][0];
            r["active"] = active;
            r["cancelExist"] = json!(cancel);
            r["dealSize"] = json!(deal);
        })
    };
    let st = |b: String| spot().decode_orders(b.as_bytes()).unwrap().0[0].status.clone();
    assert_eq!(st(row(json!(true), false, "0.000005")), Mapped::Known(OrderStatus::PartiallyFilled));
    assert_eq!(st(row(json!(false), false, "0.00001")), Mapped::Known(OrderStatus::Filled));
    assert_eq!(st(row(json!(false), true, "0")), Mapped::Known(OrderStatus::Cancelled));
    assert_eq!(st(row(json!(false), false, "0")), Mapped::Known(OrderStatus::Cancelled), "done, nothing filled: gone unfilled");
    assert!(matches!(st(row(Value::Null, false, "0")), Mapped::Unknown(_)));
    // Futures: `open` or `done`; any other word unknown.
    let s = sizes();
    let frow = |status: &str, cancel: bool, deal: i64| {
        with(FUT_BY_ID, |v| {
            v["data"]["status"] = json!(status);
            v["data"]["cancelExist"] = json!(cancel);
            v["data"]["dealSize"] = json!(deal);
        })
    };
    let fst = |b: String| futures().decode_orders_in(&DecodeCtx::new("fut_by_id", &s), b.as_bytes()).unwrap().0[0].status.clone();
    assert_eq!(fst(frow("done", false, 1)), Mapped::Known(OrderStatus::Filled));
    assert_eq!(fst(frow("done", true, 0)), Mapped::Known(OrderStatus::Cancelled));
    assert_eq!(fst(frow("done", false, 0)), Mapped::Known(OrderStatus::Cancelled), "done, nothing filled: the order went away unfilled");
    assert_eq!(fst(frow("weird", false, 0)), Mapped::Unknown("weird".into()));
}

/// "partially filled orders go to done", price protection cuts a market order short: a done
/// order not all filled is cancelled with what filled, never unknown.
#[test]
fn a_done_order_partly_filled_is_cancelled_with_its_fill() {
    // Spot: half filled, the rest cancelled, no cancellation record on the row.
    let spot_row = with(SPOT_BY_ID, |v| {
        let r = &mut v["data"];
        r["size"] = json!("0.00002");
        r["dealSize"] = json!("0.00001");
        r["dealFunds"] = json!("0.5");
        r["cancelledSize"] = json!("0.00001");
        r["remainSize"] = json!("0");
        r["cancelExist"] = json!(false);
        r["active"] = json!(false);
    });
    let (o, _) = spot().decode_orders(spot_row.as_bytes()).unwrap();
    assert_eq!((o[0].status.clone(), o[0].qty, o[0].filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.00002"), d("0.00001")));
    // Funds-sized with no cancellation of any kind: not guessed.
    let funds = with(&spot_row, |v| {
        let r = &mut v["data"];
        r["size"] = json!("0");
        r["cancelledSize"] = json!("0");
        r["cancelledFunds"] = json!("0");
    });
    assert!(matches!(spot().decode_orders(funds.as_bytes()).unwrap().0[0].status, Mapped::Unknown(_)));
    // Futures: 1 of 3 lots filled, done.
    let s = sizes();
    let fut_row = with(FUT_BY_ID, |v| {
        v["data"]["symbol"] = json!("XBTUSDTM");
        v["data"]["size"] = json!(3);
        v["data"]["dealSize"] = json!(1);
        v["data"]["filledSize"] = json!(1);
        v["data"]["status"] = json!("done");
        v["data"]["cancelExist"] = json!(false);
    });
    let (o, _) = futures().decode_orders_in(&DecodeCtx::new("fut_by_id", &s), fut_row.as_bytes()).unwrap();
    assert_eq!((o[0].status.clone(), o[0].qty, o[0].filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.003"), d("0.001")));
    // The stream: a `filled` event whose own filled size is short of the order is cancelled
    // with that fill; one with no filled size stays filled.
    let none = ContractSizes::new();
    let ctx = DecodeCtx::new("/spotMarket/tradeOrdersV2", &none);
    let short = with(WS_SPOT_FILLED, |v| {
        v["data"]["originSize"] = json!("0.00002");
        v["data"]["size"] = json!("0.00002");
    });
    let ev = spot().decode_private_in(&ctx, short.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = ev.last().unwrap() else { panic!("{ev:?}") };
    assert_eq!((o.status.clone(), o.filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.00001")));
    let unstated = with(&short, |v| {
        v["data"].as_object_mut().unwrap().remove("filledSize");
    });
    let ev = spot().decode_private_in(&ctx, unstated.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = ev.last().unwrap() else { panic!("{ev:?}") };
    assert_eq!(o.status, Mapped::Known(OrderStatus::Filled));
}

/// The futures `open` frame names no `orderType`: an order resting in the book is a limit at
/// its price.
#[test]
fn a_futures_open_frame_without_its_type_is_a_limit_at_its_price() {
    let s = sizes();
    let ev = futures().decode_private_in(&DecodeCtx::new("/contractMarket/tradeOrders", &s), WS_FUT_OPEN.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.order_type.as_str(), o.limit), ("limit", Some(d("91670"))));
}

/// A stop frame that names its reduce-only flag and position side keeps them; the doc's own
/// example names neither.
#[test]
fn a_futures_stop_frame_keeps_what_it_names() {
    let s = sizes();
    let ctx = DecodeCtx::new("/contractMarket/advancedOrders", &s);
    let named = with(WS_FUT_STOP, |v| {
        v["data"]["reduceOnly"] = json!(true);
        v["data"]["positionSide"] = json!("LONG");
    });
    let ev = futures().decode_private_in(&ctx, named.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.reduce_only, o.position), (true, PositionSide::Long));
    let ev = futures().decode_private_in(&ctx, WS_FUT_STOP.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.reduce_only, o.position), (false, PositionSide::Net));
}

/// The whole spot book lists the stop orders after the HF orders, under `cond:<id>`, and a
/// cancel all reaches them with Batch Cancel Stop Orders per symbol.
#[test]
fn spot_cancel_all_reaches_the_stop_orders() {
    let sp = spot();
    let reqs = sp.open_orders(&settings(), None, CorrId(1)).unwrap();
    assert_eq!(reqs.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/api/v1/hf/orders/active/symbols", "/api/v1/stop-order"]);
    assert_eq!((q(&reqs[1], "tradeType"), q(&reqs[1], "pageSize")), (Some("TRADE"), Some("500")));
    // Get Stop Orders List's own example, trimmed.
    let body = r#"{"code":"200000","data":{"currentPage":1,"pageSize":50,"totalNum":2,"totalPage":2,"items":[{"id":"vs93gptvr9t2fsql003l8k5p","symbol":"BTC-USDT","status":"NEW","type":"limit","side":"buy","price":"50000.00000000000000000000","size":"0.00001000000000000000","createdAt":1740626554884,"stop":"loss","stopPrice":"60000.00000000000000000000"}]}}"#;
    let none = ContractSizes::new();
    let (o, next) = sp.decode_orders_in(&DecodeCtx::new("spot_open_stops", &none), body.as_bytes()).unwrap();
    assert_eq!(
        (o[0].venue_id.as_str(), o[0].status.clone(), o[0].trigger_pending, o[0].stop, o[0].limit, next.as_deref()),
        ("cond:vs93gptvr9t2fsql003l8k5p", Mapped::Known(OrderStatus::New), true, Some(d("60000")), Some(d("50000")), Some("page:2"))
    );
    assert_eq!(q(&sp.with_cursor(&reqs[1], "page:2"), "currentPage"), Some("2"));
    let all = sp.cancel_all(&settings(), &["BTC-USDT".to_string()], CorrId(1)).unwrap();
    assert_eq!(all.iter().map(|r| (r.method, r.path.as_str())).collect::<Vec<_>>(), [(Method::Delete, "/api/v1/hf/orders"), (Method::Delete, "/api/v1/stop-order/cancel")]);
    assert_eq!((q(&all[1], "symbol"), q(&all[1], "tradeType")), (Some("BTC-USDT"), Some("TRADE")));
    // Batch Cancel Stop Orders' own example.
    assert!(sp.decode_cancel_all(br#"{"code":"200000","data":{"cancelledOrderIds":["671124f9365ccb00073debd4"]}}"#).is_ok());
}

/// A walk whose remaining symbols are gone fails the listing (a cancel all would otherwise
/// read "nothing left"); walks are dropped by age only, never because others started.
#[test]
fn a_lost_spot_walk_is_an_error_and_running_walks_survive() {
    let sp = spot();
    let reqs = sp.open_orders(&settings(), None, CorrId(1)).unwrap();
    for i in 0..100_u64 {
        let mut r = reqs[0].clone();
        r.corr_id = CorrId(1000 + i);
        sp.with_cursor(&r, "walk:A-USDT,B-USDT");
    }
    assert_eq!(sp.walks.lock().unwrap().len(), 100, "no running walk wiped");
    let mut lost = reqs[0].clone();
    lost.corr_id = CorrId(7);
    let end = sp.with_cursor(&lost, "next");
    let none = ContractSizes::new();
    let e = sp.decode_orders_in(&DecodeCtx::new(end.endpoint_id, &none), br#"{"code":"200000","data":{"symbols":[]}}"#).unwrap_err();
    assert!(e.message.contains("list again"), "{}", e.message);
    // An expired walk is dropped on the next start (when the clock reaches back that far).
    let Some(old) = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(601)) else { return };
    sp.walks.lock().unwrap().get_mut(&1000).unwrap().0 = old;
    sp.with_cursor(&lost, "walk:C-USDT,D-USDT");
    let w = sp.walks.lock().unwrap();
    assert!(!w.contains_key(&1000) && w.contains_key(&1001) && w.contains_key(&7));
}

#[test]
fn the_private_stream_maps_the_documented_frames() {
    let sp = spot();
    let none = ContractSizes::new();
    let ctx = DecodeCtx::new("/spotMarket/tradeOrdersV2", &none);
    let ev = sp.decode_private_in(&ctx, WS_SPOT_OPEN.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.status.clone(), o.limit, o.qty, o.created_ms), (Mapped::Known(OrderStatus::New), Some(d("50000")), d("0.00001"), Some(1_730_211_033_305)));
    // A match: the execution, with no fee amount (pending), then the order.
    let ev = sp.decode_private_in(&ctx, WS_SPOT_MATCH.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.exec_id.as_str(), f.qty, f.price, f.fee_pending, f.position_only), ("11116472408358913", d("0.00001"), d("71171.9"), true, false));
    let PrivateEvent::Order(o) = &ev[1] else { panic!("{ev:?}") };
    assert_eq!(o.status, Mapped::Known(OrderStatus::Filled), "status match, all filled");
    for (frame, want) in [(WS_SPOT_FILLED, OrderStatus::Filled), (WS_SPOT_CANCELED, OrderStatus::Cancelled), (WS_SPOT_UPDATE, OrderStatus::New)] {
        let ev = sp.decode_private_in(&ctx, frame.as_bytes()).unwrap();
        let PrivateEvent::Order(o) = ev.last().unwrap() else { panic!("{ev:?}") };
        assert_eq!(o.status, Mapped::Known(want), "{frame}");
    }
    let fu = futures();
    let s = sizes();
    let ctx = DecodeCtx::new("/contractMarket/tradeOrders", &s);
    let ev = fu.decode_private_in(&ctx, WS_FUT_OPEN.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.qty, o.status.clone(), o.created_ms), (d("0.001"), Mapped::Known(OrderStatus::New), Some(1_731_916_985_768)));
    let ev = fu.decode_private_in(&ctx, WS_FUT_MATCH.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.qty, f.price, f.fee_pending, f.position_only), (d("0.001"), d("91670"), true, false));
    let ev = fu.decode_private_in(&ctx, WS_FUT_ADL.as_bytes()).unwrap_err();
    assert!(ev.message.contains("10PEPEUSDTM"), "an unknown contract size is never guessed: {}", ev.message);
    let mut with_pepe = sizes();
    with_pepe.insert("10PEPEUSDTM", d("100000"));
    let ev = fu.decode_private_in(&DecodeCtx::new("/contractMarket/tradeOrders", &with_pepe), WS_FUT_ADL.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert!(f.position_only, "an ADL execution moved the position without an order of the account");
    for (frame, want) in [(WS_FUT_FILLED, OrderStatus::Filled), (WS_FUT_CANCELED, OrderStatus::Cancelled)] {
        let ev = fu.decode_private_in(&ctx, frame.as_bytes()).unwrap();
        let PrivateEvent::Order(o) = ev.last().unwrap() else { panic!("{ev:?}") };
        assert_eq!(o.status, Mapped::Known(want));
    }
    let ev = fu.decode_private_in(&ctx, WS_FUT_STOP.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.trigger_pending, o.stop, o.limit, o.qty, o.status.clone()), (true, Some(d("1000")), Some(d("0.1")), d("0.001"), Mapped::Known(OrderStatus::New)));
    let ev = fu.decode_private_in(&ctx, WS_FUT_POSITION.as_bytes()).unwrap();
    let PrivateEvent::Position(p) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((p.side, p.direction, p.qty, p.margin.clone()), (PositionSide::Short, Some(Side::Sell), d("0.001"), Some(MarginMode::isolated())));
    // The socket's own answers.
    let ans = |t: &str| sp.decode_private(t.as_bytes()).unwrap();
    assert!(matches!(&ans(r#"{"id":"hQvf8jkno","type":"welcome"}"#)[0], PrivateEvent::Answer { op, ok: true, .. } if op == "welcome"));
    assert!(matches!(&ans(r#"{"id":"1545910660739","type":"ack"}"#)[0], PrivateEvent::Answer { op, ok: true, .. } if op == "subscribe"));
    assert!(matches!(&ans(r#"{"id":"1545910590801","type":"pong","timestamp":1764215232226553}"#)[0], PrivateEvent::Answer { op, ok: true, .. } if op == "pong"));
    // `synthetic`: an error frame in the shape the websocket error page's codes imply.
    assert!(matches!(&ans(r#"{"id":"otw-orders","type":"error","code":401,"data":"token is invalid"}"#)[0], PrivateEvent::Answer { ok: false, .. }));
}

#[test]
fn the_token_call_hands_out_the_url_and_the_heartbeat() {
    for (m, body, host) in [(spot(), SPOT_BULLET, "wss://ws-api-spot.kucoin.***/"), (futures(), FUT_BULLET, "wss://ws-api-futures.kucoin.com/")] {
        let s = m.private_socket(&settings()).unwrap();
        let done = m.decode_private_socket_bootstrap(s.clone(), body.as_bytes()).unwrap();
        let url = done.url.clone().unwrap();
        assert!(url.starts_with(&format!("{host}?token=")) && url.contains("&connectId="), "{url}");
        assert_eq!(done.ping_every_ms, 9_000, "half the venue's pingInterval of 18 s");
        let subs: Vec<Value> = done.subscribes().iter().map(|f| serde_json::from_slice(f).unwrap()).collect();
        assert!(subs.iter().all(|f| f["type"] == "subscribe" && f["privateChannel"] == true && f["response"] == true));
        let topics: Vec<&str> = subs.iter().map(|f| f["topic"].as_str().unwrap()).collect();
        if m.book() == Book::Spot {
            assert_eq!(topics, vec!["/spotMarket/tradeOrdersV2"]);
        } else {
            assert_eq!(topics, vec!["/contractMarket/tradeOrders", "/contractMarket/advancedOrders", "/contract/positionAll"]);
        }
        let ping: Value = serde_json::from_slice(&done.ping).unwrap();
        assert_eq!(ping["type"], "ping");
    }
}

// ── The catalog ──

/// The mapper the catalog drives: KuCoin's own, but for the reads the shared catalog cannot
/// answer (it mounts the order calls, the instrument list and the position rows only): the
/// account-wide position mode and the per-symbol margin read (the mode then comes from the
/// rows, as on Bybit, and the margin from the rows' `marginMode`), and the spot whole-book walk
/// (the catalog's single symbol listed directly). Those are covered through the trader below.
/// A `Placing` the catalog builds without the contract size or margin (to know which call to
/// mount) is completed from the fixture.
#[derive(Clone)]
struct Catalog(Kucoin);

const CATALOG_SPOT: &str = "BTC-USDT";
const CATALOG_FUT: &str = "XBTUSDTM";

impl Catalog {
    fn complete(&self, p: &Placing) -> Placing {
        let mut p = p.clone();
        if self.0.book() == Book::Futures {
            p.contract_size = p.contract_size.or(Some(d("0.001")));
            p.margin = p.margin.or(Some(MarginMode::cross()));
        }
        p
    }
}

impl Mapper for Catalog {
    type Session = Session;
    fn capabilities(&self) -> &Capabilities {
        self.0.capabilities()
    }
    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.0.base_urls()
    }
    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, otw_connector_model::ConnectorError> {
        self.0.encode(cmd, s, ctx)
    }
    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, otw_connector_model::ConnectorError> {
        self.0.decode(frame, s, ctx)
    }
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, otw_connector_model::ConnectorError> {
        self.0.instrument_id(symbol)
    }
}

type R<T> = Result<T, otw_connector_model::ConnectorError>;

impl TradeMapper for Catalog {
    fn venue_name(&self) -> &str {
        self.0.venue_name()
    }
    fn client_id_rule(&self) -> ClientIdRule {
        self.0.client_id_rule()
    }
    fn place(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<HttpRequest> {
        self.0.place(s, o, &self.complete(p))
    }
    fn cancel(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<HttpRequest> {
        self.0.cancel(s, symbol, id, corr)
    }
    fn cancel_all(&self, s: &Settings, symbols: &[String], corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.cancel_all(s, symbols, corr)
    }
    fn decode_cancel_all(&self, body: &[u8]) -> R<()> {
        self.0.decode_cancel_all(body)
    }
    fn query_by_client_id(&self, s: &Settings, symbol: &str, cid: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_client_id(s, symbol, cid, corr)
    }
    fn query_by_venue_id(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_venue_id(s, symbol, id, corr)
    }
    fn open_orders(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<Vec<HttpRequest>> {
        match (self.0.book(), symbol) {
            (Book::Spot, None) => self.0.open_orders(s, Some(CATALOG_SPOT), corr),
            _ => self.0.open_orders(s, symbol, corr),
        }
    }
    fn fills_since(&self, s: &Settings, symbol: &str, from: i64, to: i64, corr: CorrId) -> R<HttpRequest> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn fills_window_ms(&self) -> Option<i64> {
        self.0.fills_window_ms()
    }
    fn instrument_rules(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<HttpRequest> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn instrument_rules_any_status(&self, s: &Settings, symbol: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.instrument_rules_any_status(s, symbol, corr)
    }
    fn check_order(&self, o: &OrderRequest) -> R<()> {
        self.0.check_order(o)
    }
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        self.0.with_cursor(req, cursor)
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
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.0.invalidates_margin_mode(e)
    }
    fn counts_contracts(&self) -> bool {
        self.0.counts_contracts()
    }
    fn dry_run(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<Option<HttpRequest>> {
        self.0.dry_run(s, o, &self.complete(p))
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
    fn error_table(&self) -> &'static [ErrorRow] {
        self.0.error_table()
    }
}

/// The catalog's answers: the documented examples with their ids, sizes and states changed
/// (`synthetic`, built on the documented shape).
struct Script {
    m: Kucoin,
}

impl Script {
    fn fut(&self) -> bool {
        self.m.book() == Book::Futures
    }

    /// Base coin to lots on the catalog's contract (0.001 per lot).
    fn lots(&self, q: Decimal) -> Value {
        json!((q / d("0.001")).normalize().to_string().parse::<i64>().unwrap())
    }
}

fn envelope(code: &str, msg: &str) -> String {
    json!({ "code": code, "msg": msg }).to_string()
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
        Arc::new(KucoinSigner::from_secrets(&settings()).unwrap())
    }

    fn symbol(&self) -> &'static str {
        if self.fut() {
            CATALOG_FUT
        } else {
            CATALOG_SPOT
        }
    }

    fn qty(&self) -> Decimal {
        if self.fut() {
            d("0.01")
        } else {
            d("0.001")
        }
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        match (self.fut(), closed) {
            (true, None) => FUT_CONTRACTS.to_string(),
            (true, Some(w)) => with(FUT_CONTRACTS, |v| v["data"][0]["status"] = json!(w)),
            (false, None) => SPOT_SYMBOLS.to_string(),
            (false, Some(_)) => with(SPOT_SYMBOLS, |v| v["data"][0]["enableTrading"] = json!(false)),
        }
    }

    fn closed_word(&self) -> &'static str {
        if self.fut() {
            "Paused"
        } else {
            "enableTrading false"
        }
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::PartiallyFilled => "partial",
            OrderStatus::Filled => "filled",
            OrderStatus::Cancelled => "canceled",
            _ => "open",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        with(if self.fut() { FUT_ADD } else { SPOT_ADD }, |v| {
            v["data"]["orderId"] = json!(venue_id);
            v["data"]["clientOid"] = json!(client_id);
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let fut = self.fut();
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                let template: Value = serde_json::from_str(if fut { FUT_LIST } else { SPOT_OPEN }).unwrap();
                let mut o = template["data"]["items"][0].clone();
                o["id"] = json!(r.venue_id);
                o["clientOid"] = json!(r.client_id);
                o["symbol"] = json!(self.symbol());
                o["side"] = json!(if r.side == Side::Buy { "buy" } else { "sell" });
                o["price"] = json!(self.price().to_string());
                let (done, cancel) = match r.word {
                    "open" | "partial" => (false, false),
                    "filled" => (true, false),
                    "canceled" => (true, true),
                    _ => (false, false),
                };
                o["cancelExist"] = json!(cancel);
                if fut {
                    o["size"] = self.lots(r.qty);
                    o["dealSize"] = self.lots(r.filled);
                    o["filledSize"] = self.lots(r.filled);
                    o["isActive"] = json!(!done);
                    o["status"] = json!(match r.word {
                        "open" | "partial" => "open",
                        "filled" | "canceled" => "done",
                        other => other,
                    });
                } else {
                    o["size"] = json!(r.qty.to_string());
                    o["dealSize"] = json!(r.filled.to_string());
                    o["remainSize"] = json!((r.qty - r.filled).to_string());
                    match r.word {
                        "open" | "partial" | "filled" | "canceled" => o["active"] = json!(!done),
                        // A word the venue never writes: no `active` flag at all.
                        _ => {
                            o.as_object_mut().unwrap().remove("active");
                        }
                    }
                }
                o
            })
            .collect();
        json!({ "code": "200000", "data": { "currentPage": 1, "pageSize": 50, "totalNum": items.len(), "totalPage": 1, "items": items } }).to_string()
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let fut = self.fut();
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                let template: Value = serde_json::from_str(if fut { FUT_FILLS } else { SPOT_FILLS }).unwrap();
                let mut f = template["data"]["items"][0].clone();
                f["tradeId"] = json!(r.exec_id);
                f["orderId"] = json!(r.venue_id);
                f["symbol"] = json!(self.symbol());
                f["size"] = if fut { self.lots(r.qty) } else { json!(r.qty.to_string()) };
                f["price"] = json!(r.price.to_string());
                f["fee"] = json!(r.fee.to_string());
                f["feeCurrency"] = json!("USDT");
                f
            })
            .collect();
        if fut {
            json!({ "code": "200000", "data": { "currentPage": 1, "pageSize": 50, "totalNum": items.len(), "totalPage": 1, "items": items } }).to_string()
        } else {
            json!({ "code": "200000", "data": { "items": items, "lastId": 19814995255305_i64 } }).to_string()
        }
    }

    fn fee_currency(&self) -> &'static str {
        "USDT"
    }

    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|(side, dir, qty)| {
                let template: Value = serde_json::from_str(FUT_POS_CROSS).unwrap();
                let mut p = template["data"][0].clone();
                p["symbol"] = json!(CATALOG_FUT);
                p["positionSide"] = json!(match side {
                    PositionSide::Net => "BOTH",
                    PositionSide::Long => "LONG",
                    PositionSide::Short => "SHORT",
                });
                let lots = (*qty / d("0.001")).normalize().to_string().parse::<i64>().unwrap();
                p["currentQty"] = json!(if *dir == Some(Side::Sell) || *side == PositionSide::Short { -lots } else { lots });
                p["marginMode"] = json!("CROSS");
                p
            })
            .collect();
        json!({ "code": "200000", "data": list }).to_string()
    }

    fn ok(&self) -> String {
        if self.fut() {
            FUT_CANCEL.to_string()
        } else {
            SPOT_CANCEL.to_string()
        }
    }

    fn dry_run_ok(&self) -> String {
        if self.fut() {
            FUT_TEST.to_string()
        } else {
            SPOT_TEST.to_string()
        }
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        let fut = self.fut();
        match r {
            VenueRefusal::Funds => Answer::new(400, if fut { envelope("300003", "Balance not enough, please first deposit at least 2 USDT before you go to battle") } else { envelope("200004", "Balance insufficient") }),
            // The doc names no code for a reduce-only order that would add ("it will be
            // rejected"): `300009` is what one with nothing to reduce gets.
            VenueRefusal::ReduceOnly => Answer::new(400, envelope("300009", "Current position size: 0, unable to close the position.")),
            // No code is named for an unknown or ended order on a spot cancel: a parameter error
            // there is looked up (trade_errors.rs).
            VenueRefusal::NotFound => Answer::new(400, envelope("400100", "Parameter Error")),
            VenueRefusal::Final => Answer::new(400, if fut { envelope("100004", "Order is in not cancelable state") } else { envelope("400100", "Parameter Error") }),
            VenueRefusal::RateLimited(s) => Answer {
                status: 429,
                body: envelope("429000", "Too Many Requests"),
                headers: vec![("gw-ratelimit-reset", (s * 1000).to_string())],
            },
            VenueRefusal::ClockSkew => Answer::new(400, envelope("400002", "KC-API-TIMESTAMP Invalid: time differs from server time by more than 5 seconds")),
            VenueRefusal::Unmapped => Answer::new(200, envelope("999999", "something new")),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            VenueRefusal::PositionModeMismatch => Answer::new(400, envelope("330011", "Before initiating any Futures trades, please use the Switch Position Mode endpoint")),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_passes_on_spot() {
    let r = run_catalog(&Script { m: spot() }).await;
    println!("spot ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    assert!(r.ran.len() >= 25 && r.ran.contains(&"E2D session check"), "{:?}", r.ran);
}

/// The catalog on futures.
#[tokio::test]
async fn the_trading_catalog_passes_on_futures() {
    let r = run_catalog(&Script { m: futures() }).await;
    assert!(r.ran.contains(&"E2D session check") && r.ran.contains(&"T16") && r.ran.contains(&"position mode") && r.ran.contains(&"T10"), "{:?}", r.ran);
}

// ── Through the trader, what the catalog cannot mount ──

mod trader {
    use super::*;
    use otw_connector_engine::trading::{Account, TicketOptions, TradeError, Trader};
    use otw_connector_testkit::wiremock::matchers::{body_string_contains, method, path, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn ok(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_string(body.to_string())
    }

    fn driver(m: Kucoin, server: &MockServer) -> otw_connector_sdk::Driver<Kucoin> {
        otw_connector_testkit::driver_against(m, server, otw_connector_testkit::ManualClock::at(otw_connector_testkit::ts(CATALOG_NOW_MS / 1000)))
    }

    fn reads(server_reqs: &[Request], p: &str) -> usize {
        server_reqs.iter().filter(|r| r.url.path() == p).count()
    }

    fn limit_on(m: &Kucoin, key: &str, side: Side) -> OrderRequest {
        order(m, "XBTUSDTM", key, side, OrderType::Limit, "0.01", Some("30000"))
    }

    /// The position mode is read at account level (even flat) and the margin mode per symbol,
    /// once each, and every order carries them; a refusal naming the mode or the margin reads
    /// that one again before the next order, and the refused order is never sent again.
    #[tokio::test]
    async fn futures_read_mode_and_margin_once_and_send_them() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v1/contracts/active")).respond_with(ok(FUT_CONTRACTS)).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v2/position/getPositionMode")).respond_with(ok(FUT_MODE)).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v2/position/getMarginMode")).and(query_param("symbol", "XBTUSDTM")).respond_with(ok(&with(FUT_MARGIN, |v| v["data"]["marginMode"] = json!("CROSS")))).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v1/orders")).and(body_string_contains("\"m3\"")).respond_with(ResponseTemplate::new(400).set_body_string(envelope("330011", "switch position mode"))).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v1/orders")).and(body_string_contains("\"m5\"")).respond_with(ResponseTemplate::new(400).set_body_string(envelope("330005", "Please switch your margin mode to cross margin mode and try again"))).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v1/orders")).respond_with(ok(FUT_ADD)).mount(&server).await;
        let s = settings();
        let signer = KucoinSigner::from_secrets(&s).unwrap();
        let a = Account { settings: &s, signer: &signer, scope: "kc-mode" };
        let dv = driver(futures(), &server);
        let t: Trader<Kucoin> = Trader::new();
        let m = futures();
        t.place(&dv, &a, &[limit_on(&m, "m1", Side::Buy)], &TicketOptions::default()).await.unwrap();
        t.place(&dv, &a, &[limit_on(&m, "m2", Side::Sell)], &TicketOptions::default()).await.unwrap();
        let reqs = server.received_requests().await.unwrap();
        assert_eq!((reads(&reqs, "/api/v2/position/getPositionMode"), reads(&reqs, "/api/v2/position/getMarginMode")), (1, 1), "read once, held");
        let bodies: Vec<Value> = reqs.iter().filter(|r| r.url.path() == "/api/v1/orders").map(|r| serde_json::from_slice(&r.body).unwrap()).collect();
        // Hedge mode: a buy opens the long, a sell the short; the symbol's margin as read.
        assert_eq!(
            bodies.iter().map(|b| (b["positionSide"].as_str().unwrap(), b["marginMode"].as_str().unwrap(), b["qty"].as_str().unwrap())).collect::<Vec<_>>(),
            vec![("LONG", "CROSS", "0.01"), ("SHORT", "CROSS", "0.01")]
        );
        // `330011`: refused, not resent, the mode read again before the next order.
        let out = t.place(&dv, &a, &[limit_on(&m, "m3", Side::Buy)], &TicketOptions::default()).await;
        assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("330011")), "{out:?}");
        t.place(&dv, &a, &[limit_on(&m, "m4", Side::Buy)], &TicketOptions::default()).await.unwrap();
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reads(&reqs, "/api/v2/position/getPositionMode"), 2);
        // `330005`: the margin read again.
        let out = t.place(&dv, &a, &[limit_on(&m, "m5", Side::Buy)], &TicketOptions::default()).await;
        assert!(matches!(out, Err(TradeError::Refused(_))));
        t.place(&dv, &a, &[limit_on(&m, "m6", Side::Buy)], &TicketOptions::default()).await.unwrap();
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reads(&reqs, "/api/v2/position/getMarginMode"), 2);
        let sent: Vec<String> = reqs.iter().filter(|r| r.url.path() == "/api/v1/orders").map(|r| String::from_utf8_lossy(&r.body).to_string()).collect();
        assert_eq!(sent.iter().filter(|b| b.contains("\"m3\"")).count(), 1, "never resent");
        // Leverage and margin, read only, as the chart shows them.
        Mock::given(method("GET")).and(path("/api/v2/position")).respond_with(ok(FUT_POS_ISOLATED)).mount(&server).await;
        let lev = t.leverage(&dv, &a, "XBTUSDTM").await.unwrap().unwrap();
        assert_eq!(lev.0, Some(d("6.01")));
    }

    /// T10 on futures: the position's size read now, closed at the market reduce-only on the
    /// other side, whole or in part; more than held is refused before I/O.
    #[tokio::test]
    async fn t10_closes_futures_at_the_market_reduce_only_on_the_other_side() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v1/contracts/active")).respond_with(ok(FUT_CONTRACTS)).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v2/position/getPositionMode")).respond_with(ok(&with(FUT_MODE, |v| v["data"]["positionMode"] = json!(0)))).mount(&server).await;
        let short = with(FUT_POS_CROSS, |v| {
            v["data"][0]["currentQty"] = json!(-20);
            v["data"][0]["positionSide"] = json!("BOTH");
        });
        Mock::given(method("GET")).and(path("/api/v2/position")).respond_with(ok(&short)).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v1/orders")).respond_with(ok(FUT_ADD)).mount(&server).await;
        let s = settings();
        let signer = KucoinSigner::from_secrets(&s).unwrap();
        let a = Account { settings: &s, signer: &signer, scope: "kc-close" };
        let dv = driver(futures(), &server);
        let t: Trader<Kucoin> = Trader::new();
        t.close_position(&dv, &a, "XBTUSDTM", None, None, "c1").await.unwrap();
        t.close_position(&dv, &a, "XBTUSDTM", None, Some(d("0.005")), "c2").await.unwrap();
        let out = t.close_position(&dv, &a, "XBTUSDTM", None, Some(d("0.05")), "c3").await;
        assert!(matches!(out, Err(TradeError::Refused(_))));
        let bodies: Vec<Value> =
            server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/api/v1/orders").map(|r| serde_json::from_slice(&r.body).unwrap()).collect();
        let got: Vec<(&str, &str, &str, bool, &str, &str)> = bodies
            .iter()
            .map(|b| (b["side"].as_str().unwrap(), b["type"].as_str().unwrap(), b["qty"].as_str().unwrap(), b["reduceOnly"].as_bool().unwrap(), b["positionSide"].as_str().unwrap(), b["marginMode"].as_str().unwrap()))
            .collect();
        assert_eq!(got, vec![("buy", "market", "0.02", true, "BOTH", "CROSS"), ("buy", "market", "0.005", true, "BOTH", "CROSS")]);
    }

    /// Spot lists working orders per symbol only: the whole book names its symbols first, then
    /// walks them page by page; the kill switch cancels each symbol's orders in one call.
    #[tokio::test]
    async fn spot_walks_the_symbols_with_working_orders_and_cancels_each() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v1/hf/orders/active/symbols")).respond_with(ok(r#"{"code":"200000","data":{"symbols":["BTC-USDT","ETH-USDT"]}}"#)).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v1/hf/orders/active/symbols")).respond_with(ok(r#"{"code":"200000","data":{"symbols":[]}}"#)).mount(&server).await;
        let page = |sym: &str, id: &str, cur: i64, total: i64| {
            with(SPOT_OPEN, |v| {
                v["data"]["currentPage"] = json!(cur);
                v["data"]["totalPage"] = json!(total);
                v["data"]["items"][0]["id"] = json!(id);
                v["data"]["items"][0]["symbol"] = json!(sym);
            })
        };
        let open = "/api/v1/hf/orders/active/page";
        Mock::given(method("GET")).and(path(open)).and(query_param("symbol", "BTC-USDT")).and(query_param("pageNum", "1")).respond_with(ok(&page("BTC-USDT", "b1", 1, 2))).mount(&server).await;
        Mock::given(method("GET")).and(path(open)).and(query_param("symbol", "BTC-USDT")).and(query_param("pageNum", "2")).respond_with(ok(&page("BTC-USDT", "b2", 2, 2))).mount(&server).await;
        Mock::given(method("GET")).and(path(open)).and(query_param("symbol", "ETH-USDT")).respond_with(ok(&page("ETH-USDT", "e1", 1, 1))).mount(&server).await;
        Mock::given(method("DELETE")).and(path("/api/v1/hf/orders")).respond_with(ok(SPOT_CANCEL_ALL)).mount(&server).await;
        // One stop order working on BTC-USDT, gone after the cancel all.
        let stop = r#"{"code":"200000","data":{"currentPage":1,"pageSize":500,"totalNum":1,"totalPage":1,"items":[{"id":"st1","symbol":"BTC-USDT","status":"NEW","type":"limit","side":"buy","price":"50000","size":"0.00001","createdAt":1740626554884,"stop":"loss","stopPrice":"60000"}]}}"#;
        let no_stop = r#"{"code":"200000","data":{"currentPage":1,"pageSize":500,"totalNum":0,"totalPage":0,"items":[]}}"#;
        Mock::given(method("GET")).and(path("/api/v1/stop-order")).respond_with(ok(stop)).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v1/stop-order")).respond_with(ok(no_stop)).mount(&server).await;
        Mock::given(method("DELETE")).and(path("/api/v1/stop-order/cancel")).respond_with(ok(r#"{"code":"200000","data":{"cancelledOrderIds":["st1"]}}"#)).mount(&server).await;
        let s = settings();
        let signer = KucoinSigner::from_secrets(&s).unwrap();
        let a = Account { settings: &s, signer: &signer, scope: "kc-walk" };
        let dv = driver(spot(), &server);
        let t: Trader<Kucoin> = Trader::new();
        let n = t.cancel_all(&dv, &a).await.unwrap();
        assert_eq!(n, 4, "b1, b2, e1 and the stop order st1");
        let reqs = server.received_requests().await.unwrap();
        let symbols = |p: &str| {
            let mut v: Vec<String> = reqs.iter().filter(|r| r.method.as_str() == "DELETE" && r.url.path() == p).filter_map(|r| r.url.query_pairs().find(|(k, _)| k == "symbol").map(|(_, v)| v.to_string())).collect();
            v.sort();
            v
        };
        assert_eq!(symbols("/api/v1/hf/orders"), vec!["BTC-USDT", "ETH-USDT"]);
        assert_eq!(symbols("/api/v1/stop-order/cancel"), vec!["BTC-USDT", "ETH-USDT"], "the stop orders cancelled with the rest");
        assert!(reqs.iter().all(|r| r.url.path() != "/api/v1/stop-order/st1"), "nothing left to cancel one by one");
        assert!(the_walk_left_nothing(&dv), "the walk leaves nothing behind");
    }

    fn the_walk_left_nothing(d: &otw_connector_sdk::Driver<Kucoin>) -> bool {
        d.mapper().walks.lock().map(|w| w.is_empty()).unwrap_or(false)
    }

    /// Fills over a long period are read in the venue's 7-day windows, every page of each.
    #[tokio::test]
    async fn futures_fills_walk_seven_day_windows_and_their_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v1/contracts/active")).respond_with(ok(FUT_CONTRACTS)).mount(&server).await;
        let fills = with(FUT_FILLS, |v| v["data"]["items"][0]["symbol"] = json!("XBTUSDTM"));
        Mock::given(method("GET")).and(path("/api/v1/fills")).respond_with(ok(&fills)).mount(&server).await;
        let s = settings();
        let signer = KucoinSigner::from_secrets(&s).unwrap();
        let a = Account { settings: &s, signer: &signer, scope: "kc-fills" };
        let dv = driver(futures(), &server);
        let t: Trader<Kucoin> = Trader::new();
        let got = t.fills_since(&dv, &a, "XBTUSDTM", CATALOG_NOW_MS - 20 * 86_400_000).await.unwrap();
        assert_eq!(got.len(), 1, "the same execution in every window, kept once");
        let windows: Vec<(i64, i64)> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/api/v1/fills")
            .map(|r| {
                let q = |k: &str| r.url.query_pairs().find(|(n, _)| n == k).unwrap().1.parse::<i64>().unwrap();
                (q("startAt"), q("endAt"))
            })
            .collect();
        assert_eq!(windows.len(), 3, "{windows:?}");
        assert!(windows.iter().all(|(a, b)| b - a <= 7 * 86_400_000));
    }

    /// A contract missing from the active list is asked by name: a paused one is refused as not
    /// trading, never as unknown.
    #[tokio::test]
    async fn a_listed_contract_not_trading_is_refused_as_such() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v1/contracts/active")).respond_with(ok(r#"{"code":"200000","data":[]}"#)).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v1/contracts/XBTUSDTM")).respond_with(ok(&with(FUT_CONTRACT, |v| v["data"]["status"] = json!("Paused")))).mount(&server).await;
        let s = settings();
        let signer = KucoinSigner::from_secrets(&s).unwrap();
        let a = Account { settings: &s, signer: &signer, scope: "kc-paused" };
        let dv = driver(futures(), &server);
        let t: Trader<Kucoin> = Trader::new();
        let out = t.place(&dv, &a, &[limit_on(&futures(), "p1", Side::Buy)], &TicketOptions::default()).await;
        assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("Paused")), "{out:?}");
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live instrument lists decode to sound rules for every symbol, and an order call signed
/// with a key KuCoin never issued, or not signed at all, is answered with the auth error the
/// table expects (and nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers kucoin::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    for (m, symbol, qty, price) in [(spot(), "BTC-USDT", "0.0001", "1000"), (futures(), "XBTUSDTM", "0.001", "1000")] {
        let label = m.capabilities().label.clone();
        let provider = m.capabilities().provider.clone();
        let d = Driver::new(m.clone(), DriverConfig::new(&provider), reqwest::Client::new());
        let junk = settings();
        let signer = KucoinSigner::from_secrets(&junk).unwrap();
        let a = Account { settings: &junk, signer: &signer, scope: "net" };
        let trader: Trader<Kucoin> = Trader::new();
        let rules = trader.all_rules(&d, &a).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(rules.len() > 10, "{label}: {} instruments", rules.len());
        for r in &rules {
            r.sound().unwrap_or_else(|e| panic!("{label}: {e}"));
            assert!(r.tick.is_some() && r.lot_step.is_some(), "{label}: {} states no tick or step", r.instrument.native);
        }
        let btc = rules.iter().find(|r| r.instrument.native == symbol).unwrap_or_else(|| panic!("{label}: no {symbol}"));
        println!("{label}: {} instruments, {symbol} {btc:?}", rules.len());
        let o = order(&m, symbol, "otw-net-probe", Side::Buy, OrderType::Limit, qty, Some(price));
        let p = Placing { margin: Some(MarginMode::isolated()), contract_size: btc.contract_size, ..Placing::new("otw-net-probe", PositionSide::Net, CorrId(1)) };
        let req = m.place(&junk, &o, &p).unwrap();
        for (what, signed) in [("junk key", true), ("unsigned", false)] {
            let mut r = req.clone();
            r.signed = signed;
            let frame = d.send_as(r, Some(&signer)).await.unwrap_or_else(|e| panic!("{label} {what}: {e}"));
            let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label} {what}: KuCoin accepted an order with no valid key"));
            println!("{label} {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
            assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{label} {what}: {e:?}");
        }
    }
}
