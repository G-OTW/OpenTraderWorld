//! MEXC orders, from the documentation alone. Fixtures under `fixtures/trade/`, each the
//! example of the page named, read 2026-11-12 (`S` = https://www.mexc.com/api-docs/spot-v3/,
//! `F` = https://www.mexc.com/api-docs/futures/, `FT` = `F/account-and-trading-endpoints/`,
//! `FW` = `F/websocket-api/`, `GH-S` = the docs' GitHub mirror, branch `spot_v3_en` 349c691c,
//! `GH-C` = branch `contract_v1_en` deae62dc). Live pages printed in segments are joined back
//! with their values unchanged.
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_spot_new_order.json` | S/spot-account-trade/new-order |
//! | `doc_spot_query_order.json` | GH-S "Query Order" (its `Qty` field verbatim) |
//! | `doc_spot_open_orders.json` | GH-S "Current Open Orders" |
//! | `doc_spot_cancel.json` | GH-S "Cancel Order" |
//! | `doc_spot_my_trades.json` | GH-S "Account Trade List" |
//! | `doc_spot_exchange_info_symbol.json` | GH-S "Exchange Information" (one symbol object, as printed) |
//! | `rec_spot_exchange_info.json` | recorded: `GET https://api.mexc.com/api/v3/exchangeInfo?symbol=BTCUSDT`, 2026-11-12, public |
//! | `doc_spot_listen_key.json` | GH-S "Listen Key" |
//! | `doc_fut_place.json` | FT/place-order |
//! | `doc_fut_order_external.json` | FT/get-order-by-external-id |
//! | `doc_fut_open_orders.json` | FT/get-current-orders |
//! | `doc_fut_cancel.json` | FT/cancel-orders |
//! | `doc_fut_cancel_all.json` | FT/cancel-all-orders-under-a-contract |
//! | `doc_fut_deals_v3.json` | FT/get-historical-order-deal-details |
//! | `doc_fut_position_mode.json` | FT/get-user-position-mode |
//! | `doc_fut_leverage.json` | FT/get-position-leverage-multipliers |
//! | `doc_fut_open_positions.json` | GH-C "Get the user's current holding position" |
//! | `rec_fut_contract_btc.json` | recorded: `GET https://api.mexc.com/api/v1/contract/detail/country?symbol=BTC_USDT`, 2026-11-12, public |
//! | `doc_ws_fut_order.json` | FW/order |
//! | `doc_ws_fut_deal.json` | FW/fill-details |
//! | `doc_ws_fut_position.json` | FW/position |
//! | `doc_ws_fut_position_mode.json` | FW/position-mode-change |
//!
//! `synthetic`: the spot protobuf frames (built field by field from the values of the GH-S
//! "Spot Account Orders" and "Spot Account Deals" examples, which print them as text); every
//! error body (the docs list codes, not bodies; the envelope is the documented one); the
//! catalog's answers (the examples above with ids, sizes, prices, symbol and states changed).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, TradeError, Trader};
use otw_connector_model::{Capabilities, Command, CorrId, Decimal, Envelope, InstrumentId, Mapped, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce};
use otw_connector_sdk::trade::{
    Ack, Canon, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, MarginMode, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Refusal, Settings, TradeMapper,
    TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript, CATALOG_NOW_MS};
use otw_connector_testkit::wiremock::matchers::{body_string_contains, method, path, query_param};
use otw_connector_testkit::wiremock::{Mock, MockServer, ResponseTemplate};
use otw_connector_testkit::{http_frame, ts};
use serde_json::{json, Value};

use super::super::signer::{self, ContractSigner};
use super::super::{proto, Mexc, Session};

const SPOT_NEW: &str = include_str!("fixtures/trade/doc_spot_new_order.json");
const SPOT_QUERY: &str = include_str!("fixtures/trade/doc_spot_query_order.json");
const SPOT_OPEN: &str = include_str!("fixtures/trade/doc_spot_open_orders.json");
const SPOT_CANCEL: &str = include_str!("fixtures/trade/doc_spot_cancel.json");
const SPOT_TRADES: &str = include_str!("fixtures/trade/doc_spot_my_trades.json");
const SPOT_INFO_DOC: &str = include_str!("fixtures/trade/doc_spot_exchange_info_symbol.json");
const SPOT_INFO: &str = include_str!("fixtures/trade/rec_spot_exchange_info.json");
const SPOT_LISTEN_KEY: &str = include_str!("fixtures/trade/doc_spot_listen_key.json");
const FUT_PLACE: &str = include_str!("fixtures/trade/doc_fut_place.json");
const FUT_EXTERNAL: &str = include_str!("fixtures/trade/doc_fut_order_external.json");
const FUT_OPEN: &str = include_str!("fixtures/trade/doc_fut_open_orders.json");
const FUT_CANCEL: &str = include_str!("fixtures/trade/doc_fut_cancel.json");
const FUT_CANCEL_ALL: &str = include_str!("fixtures/trade/doc_fut_cancel_all.json");
const FUT_DEALS: &str = include_str!("fixtures/trade/doc_fut_deals_v3.json");
const FUT_MODE: &str = include_str!("fixtures/trade/doc_fut_position_mode.json");
const FUT_LEVERAGE: &str = include_str!("fixtures/trade/doc_fut_leverage.json");
const FUT_POSITIONS: &str = include_str!("fixtures/trade/doc_fut_open_positions.json");
const FUT_CONTRACT: &str = include_str!("fixtures/trade/rec_fut_contract_btc.json");
const WS_FUT_ORDER: &str = include_str!("fixtures/trade/doc_ws_fut_order.json");
const WS_FUT_DEAL: &str = include_str!("fixtures/trade/doc_ws_fut_deal.json");
const WS_FUT_POSITION: &str = include_str!("fixtures/trade/doc_ws_fut_position.json");
const WS_FUT_MODE: &str = include_str!("fixtures/trade/doc_ws_fut_position_mode.json");

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "mx0vglTESTKEY"), ("api_secret", "secret-test")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn spot() -> Mexc {
    Mexc::spot().unwrap()
}

fn futures() -> Mexc {
    Mexc::futures().unwrap()
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

fn order(m: &Mexc, symbol: &str, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>) -> OrderRequest {
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new(m.book().venue(), symbol),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: limit.map(|l| Price::new(d(l))),
        stop: None,
        tif: TimeInForce::Gtc,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts::default(),
    }
}

fn body(h: &HttpRequest) -> String {
    String::from_utf8_lossy(h.body.as_deref().unwrap_or_default()).to_string()
}

/// BTC_USDT: 0.0001 BTC a contract.
fn btc_sizes() -> ContractSizes {
    let mut s = ContractSizes::new();
    s.insert("BTC_USDT", d("0.0001"));
    s.insert("ETH_USDT", d("0.01"));
    s.insert("DOGE_USDT", d("100"));
    s
}

fn hedge(position: PositionSide) -> Placing {
    Placing { margin: Some(MarginMode::isolated()), contract_size: Some(d("0.0001")), leverage: Some(d("20")), ..Placing::new("otw-k1", position, CorrId(1)) }
}

// ── Spot requests ──

/// New Order: the parameters in the body as the raw string the doc asks for, under
/// `Content-Type: application/json`; the type carries the time in force.
#[test]
fn spot_orders_write_the_type_and_the_body_the_doc_asks_for() {
    let m = spot();
    let p = Placing::new("otw-k1", PositionSide::Net, CorrId(1));
    let o = order(&m, "BTCUSDT", "otw-k1", Side::Buy, OrderType::Limit, "0.001", Some("30000"));
    let h = m.place(&settings(), &o, &p).unwrap();
    assert_eq!((h.path.as_str(), h.signed, h.query.is_empty()), ("/api/v3/order", true, true));
    assert_eq!(body(&h), "symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=0.001&price=30000&newClientOrderId=otw-k1");
    assert_eq!(h.headers, vec![("Content-Type".to_string(), "application/json".to_string())]);
    let words = |tif, post| {
        let mut o = o.clone();
        o.tif = tif;
        o.opts.post_only = post;
        m.place(&settings(), &o, &p).map(|h| body(&h))
    };
    assert!(words(TimeInForce::Gtc, true).unwrap().contains("type=LIMIT_MAKER"));
    assert!(words(TimeInForce::Ioc, false).unwrap().contains("type=IMMEDIATE_OR_CANCEL"));
    assert!(words(TimeInForce::Fok, false).unwrap().contains("type=FILL_OR_KILL"));
    assert!(words(TimeInForce::Ioc, true).is_err(), "post-only rests");
    // A market sell is a base size; a market buy the amount to spend the trader valued.
    let sell = order(&m, "BTCUSDT", "s1", Side::Sell, OrderType::Market, "0.001", None);
    assert_eq!(body(&m.place(&settings(), &sell, &p).unwrap()), "symbol=BTCUSDT&side=SELL&type=MARKET&quantity=0.001&newClientOrderId=otw-k1");
    let buy = order(&m, "BTCUSDT", "b1", Side::Buy, OrderType::Market, "0.001", None);
    assert!(m.place(&settings(), &buy, &p).is_err(), "a market buy needs its amount");
    let valued = Placing { quote_amount: Some(d("30.12")), ..p.clone() };
    assert_eq!(body(&m.place(&settings(), &buy, &valued).unwrap()), "symbol=BTCUSDT&side=BUY&type=MARKET&quoteOrderQty=30.12&newClientOrderId=otw-k1");
    assert!(m.market_buy_in_quote(&buy) && !m.market_buy_in_quote(&sell));
    let fok = OrderRequest { tif: TimeInForce::Fok, ..sell.clone() };
    assert!(m.check_order(&fok).is_err(), "a market order is not FOK");
    let mut reduce = sell.clone();
    reduce.opts.reduce_only = true;
    assert!(m.place(&settings(), &reduce, &p).is_err());
    // The order test: the same parameters, no client id.
    let dry = m.dry_run(&settings(), &o, &Placing::new("", PositionSide::Net, CorrId(1))).unwrap().unwrap();
    assert_eq!((dry.path.as_str(), body(&dry)), ("/api/v3/order/test", "symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=0.001&price=30000".to_string()));
    assert!(m.dry_run(&settings(), &buy, &Placing::new("", PositionSide::Net, CorrId(1))).unwrap().is_none(), "a market buy not yet valued is not covered");
    assert_eq!(m.decode_dry_run(b"{}").unwrap(), "accepted by MEXC's order test");
}

#[test]
fn spot_cancels_lookups_and_lists_name_the_pair_and_the_id() {
    let m = spot();
    let s = settings();
    let c = m.cancel(&s, "BTCUSDT", "06a480e6", CorrId(1)).unwrap();
    assert_eq!((c.method, c.path.as_str(), c.query.clone()), (otw_connector_sdk::Method::Delete, "/api/v3/order", vec![("symbol".into(), "BTCUSDT".into()), ("orderId".into(), "06a480e6".into())]));
    let all = m.cancel_all(&s, &["BTCUSDT".into(), "ETHUSDT".into()], CorrId(1)).unwrap();
    assert_eq!(all.iter().map(|h| (h.path.as_str(), h.query[0].1.as_str())).collect::<Vec<_>>(), [("/api/v3/openOrders", "BTCUSDT"), ("/api/v3/openOrders", "ETHUSDT")]);
    let q = m.query_by_client_id(&s, "BTCUSDT", "otw-k1", CorrId(1)).unwrap();
    assert_eq!(q[0].query, vec![("symbol".into(), "BTCUSDT".into()), ("origClientOrderId".into(), "otw-k1".into())]);
    let f = m.fills_since(&s, "BTCUSDT", 1_000, 2_000, CorrId(1)).unwrap();
    assert_eq!(f.path, "/api/v3/myTrades");
    assert_eq!(m.fills_window_ms(), Some(3_600_000));
    assert!(m.positions(&s, "BTCUSDT", CorrId(1)).unwrap().is_none() && m.account_position_mode(&s, CorrId(1)).unwrap().is_none());
}

// ── Spot answers ──

#[test]
fn spot_answers_read_the_documented_examples() {
    let m = spot();
    let ack = m.decode_ack(SPOT_NEW.as_bytes()).unwrap();
    assert_eq!(ack, Ack { venue_id: "06a480e69e604477bfb48dddd5f0b750".into(), client_id: None });
    // Query Order writes `Qty`; the id a number.
    let (rows, next) = m.decode_orders(SPOT_QUERY.as_bytes()).unwrap();
    let o = &rows[0];
    assert_eq!((o.venue_id.as_str(), o.client_id.as_deref(), o.qty, o.status.clone(), o.limit, o.order_type.as_str(), next), ("1", Some("myOrder1"), d("1.0"), Mapped::Known(OrderStatus::New), Some(d("0.1")), "limit", None));
    let (rows, _) = m.decode_orders(SPOT_OPEN.as_bytes()).unwrap();
    assert_eq!((rows.len(), rows[0].side.clone(), rows[0].created_ms), (1, Mapped::Known(Side::Buy), Some(1_499_827_319_559)));
    let (rows, _) = m.decode_orders(SPOT_CANCEL.as_bytes()).unwrap();
    assert_eq!(rows[0].status, Mapped::Known(OrderStatus::Cancelled));
    // Every documented status word, and no other.
    for (w, s) in [("NEW", OrderStatus::New), ("PARTIALLY_FILLED", OrderStatus::PartiallyFilled), ("FILLED", OrderStatus::Filled), ("CANCELED", OrderStatus::Cancelled), ("PARTIALLY_CANCELED", OrderStatus::Cancelled)] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    assert!(matches!(m.status_of("EXPIRED"), Mapped::Unknown(_)));
    // A market buy sized in the quote coin: its size is what filled; the average from the
    // quote amount.
    let mb = with(SPOT_QUERY, |v| {
        v["type"] = json!("MARKET");
        v["Qty"] = json!("0");
        v["executedQty"] = json!("0.5");
        v["cummulativeQuoteQty"] = json!("10");
        v["status"] = json!("FILLED");
    });
    let (rows, _) = m.decode_orders(mb.as_bytes()).unwrap();
    assert_eq!((rows[0].qty, rows[0].avg_price, rows[0].limit), (d("0.5"), Some(d("20")), None));
    // Fills: the commission as a cost, in its asset.
    let (f, next) = m.decode_fills(SPOT_TRADES.as_bytes()).unwrap();
    assert_eq!(
        (f[0].exec_id.as_str(), f[0].venue_order_id.as_str(), f[0].qty, f[0].price, f[0].fee, f[0].fee_currency.as_str(), f[0].side.clone(), next),
        ("fad2af9e942049b6adbda1a271f990c6", "bb41e5663e124046bd9497a3f5692f39", d("12"), d("4.000001"), d("10.1"), "BNB", Mapped::Known(Side::Buy), None)
    );
    // A full page cannot be read further: said, never cut.
    let row: Value = serde_json::from_str::<Value>(SPOT_TRADES).unwrap()[0].clone();
    let full = Value::Array(vec![row; 100]).to_string();
    assert!(m.decode_fills(full.as_bytes()).unwrap_err().message.contains("100 spot fills"));
    assert_eq!(m.decode_reference_price(br#"{"symbol":"BTCUSDT","bidPrice":"30000.1","bidQty":"1","askPrice":"30000.2","askQty":"2"}"#, Side::Buy).unwrap(), d("30000.2"));
}

#[test]
fn spot_rules_read_the_precisions_status_and_side_windows() {
    let m = spot();
    let (rules, _) = m.decode_rules(SPOT_INFO.as_bytes()).unwrap();
    let r = &rules[0];
    assert_eq!((r.tick, r.lot_step, r.quote_step, r.min_qty, r.min_notional, r.max_notional, r.status.clone()), (Some(d("0.01")), Some(d("0.00000001")), Some(d("0.01")), Some(d("0.000001")), Some(d("1")), Some(d("4000000")), TradingStatus::Trading));
    r.sound().unwrap();
    // The doc's own example: `ENABLED`, but no API spot trading.
    let doc = format!(r#"{{"symbols":[{SPOT_INFO_DOC}]}}"#);
    let (rules, _) = m.decode_rules(doc.as_bytes()).unwrap();
    assert_eq!(rules[0].status, TradingStatus::Closed("isSpotTradingAllowed false".into()));
    let side = |t: &str| {
        let b = with(SPOT_INFO, |v| v["symbols"][0]["tradeSideType"] = json!(t));
        m.decode_rules(b.as_bytes()).unwrap().0[0].status.clone()
    };
    assert!(matches!(side("2"), TradingStatus::BuyOnly(_)) && matches!(side("3"), TradingStatus::SellOnly(_)) && matches!(side("4"), TradingStatus::Closed(_)));
    let paused = with(SPOT_INFO, |v| v["symbols"][0]["status"] = json!("2"));
    assert_eq!(m.decode_rules(paused.as_bytes()).unwrap().0[0].status, TradingStatus::Closed("2 (Pause)".into()));
}

// ── Futures requests ──

/// Place Order, hedge mode: the side code by position and direction, the size in contracts,
/// the margin and leverage held, the client id, `positionMode` 1; the body is the JSON signed.
#[test]
fn futures_orders_write_side_codes_contracts_and_the_held_margin() {
    let m = futures();
    let s = settings();
    let o = order(&m, "BTC_USDT", "otw-k1", Side::Buy, OrderType::Limit, "0.0005", Some("30000.5"));
    let h = m.place(&s, &o, &hedge(PositionSide::Long)).unwrap();
    assert_eq!((h.path.as_str(), h.method, h.signed), ("/api/v1/private/order/create", otw_connector_sdk::Method::Post, true));
    assert_eq!(body(&h), r#"{"symbol":"BTC_USDT","price":30000.5,"vol":5,"leverage":20,"side":1,"type":1,"openType":1,"externalOid":"otw-k1","positionMode":1}"#);
    assert!(h.headers.is_empty(), "the signer writes the one Content-Type");
    let code = |position, side, reduce| {
        let mut o = OrderRequest { side, ..o.clone() };
        o.opts.reduce_only = reduce;
        let v: Value = serde_json::from_str(&body(&m.place(&s, &o, &hedge(position)).unwrap())).unwrap();
        v["side"].as_i64().unwrap()
    };
    // "1 open long, 2 close short, 3 open short, 4 close long".
    assert_eq!((code(PositionSide::Long, Side::Buy, false), code(PositionSide::Short, Side::Buy, true), code(PositionSide::Short, Side::Sell, false), code(PositionSide::Long, Side::Sell, true)), (1, 2, 3, 4));
    // Market: type 5, no price; the types carry the time in force.
    let mk = order(&m, "BTC_USDT", "k2", Side::Sell, OrderType::Market, "0.001", None);
    let v: Value = serde_json::from_str(&body(&m.place(&s, &mk, &hedge(PositionSide::Short)).unwrap())).unwrap();
    assert_eq!((v["type"].as_i64(), v.get("price").is_none(), v["vol"].as_i64()), (Some(5), true, Some(10)));
    let ty = |tif, post| {
        let mut o = o.clone();
        o.tif = tif;
        o.opts.post_only = post;
        m.place(&s, &o, &hedge(PositionSide::Long)).map(|h| serde_json::from_str::<Value>(&body(&h)).unwrap()["type"].as_i64().unwrap())
    };
    assert_eq!((ty(TimeInForce::Gtc, true).unwrap(), ty(TimeInForce::Ioc, false).unwrap(), ty(TimeInForce::Fok, false).unwrap()), (2, 3, 4));
    assert!(m.check_order(&OrderRequest { tif: TimeInForce::Fok, ..mk.clone() }).is_err());
    // No leverage held: none sent, the venue answers. Cross margin: openType 2.
    let p = Placing { leverage: None, margin: Some(MarginMode::cross()), ..hedge(PositionSide::Long) };
    let v: Value = serde_json::from_str(&body(&m.place(&s, &o, &p).unwrap())).unwrap();
    assert_eq!((v.get("leverage").is_none(), v["openType"].as_i64()), (true, Some(2)));
    // One-way: refused before I/O, naming the fix. No margin read: refused. Not whole contracts: refused.
    let e = m.place(&s, &o, &hedge(PositionSide::Net)).unwrap_err();
    assert!(e.message.contains("hedge mode only") && e.message.contains("Switch"), "{}", e.message);
    assert!(m.place(&s, &o, &Placing { margin: None, ..hedge(PositionSide::Long) }).is_err());
    let odd = order(&m, "BTC_USDT", "k3", Side::Buy, OrderType::Limit, "0.00015", Some("30000"));
    assert!(m.place(&s, &odd, &hedge(PositionSide::Long)).unwrap_err().message.contains("0.0001 or 0.0002"));
    assert!(m.dry_run(&s, &o, &hedge(PositionSide::Long)).unwrap().is_none());
}

#[test]
fn futures_cancels_lookups_and_reads_go_to_the_documented_paths() {
    let m = futures();
    let s = settings();
    let c = m.cancel(&s, "BTC_USDT", "739113577038255616", CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/api/v1/private/order/cancel", "[739113577038255616]".to_string()));
    assert!(m.cancel(&s, "BTC_USDT", "not-a-number", CorrId(1)).is_err());
    let all = m.cancel_all(&s, &["BTC_USDT".into()], CorrId(1)).unwrap();
    assert_eq!((all[0].path.as_str(), body(&all[0])), ("/api/v1/private/order/cancel_all", r#"{"symbol":"BTC_USDT"}"#.to_string()));
    assert_eq!(m.query_by_client_id(&s, "BTC_USDT", "otw-k1", CorrId(1)).unwrap()[0].path, "/api/v1/private/order/external/BTC_USDT/otw-k1");
    assert_eq!(m.query_by_venue_id(&s, "BTC_USDT", "7391", CorrId(1)).unwrap()[0].path, "/api/v1/private/order/get/7391");
    let open = m.open_orders(&s, None, CorrId(1)).unwrap();
    assert_eq!((open[0].path.as_str(), open[0].query.clone()), ("/api/v1/private/order/list/open_orders", vec![("page_num".into(), "1".into()), ("page_size".into(), "100".into())]));
    assert_eq!(m.with_cursor(&open[0], "next").query, vec![("page_size".into(), "100".into()), ("page_num".into(), "2".into())]);
    assert_eq!(m.fills_since(&s, "BTC_USDT", 1, 2, CorrId(1)).unwrap().path, "/api/v1/private/order/list/order_deals/v3");
    assert_eq!(m.instrument_rules(&s, Some("BTC_USDT"), CorrId(1)).unwrap().path, "/api/v1/contract/detail/country");
    assert_eq!(m.account_position_mode(&s, CorrId(1)).unwrap().unwrap().path, "/api/v1/private/position/position_mode");
    assert_eq!(m.symbol_leverage(&s, "BTC_USDT", CorrId(1)).unwrap().unwrap().query, vec![("symbol".into(), "BTC_USDT".into())]);
    assert!(m.margin_per_side() && m.counts_contracts());
}

/// The working conditional lists: plan by page (its required window is the signer's), the
/// current TP/SL orders on their own endpoint (no page, no window), trailing once per state
/// (`states` is a `List<int>` whose GET form is not stated). A lookup by id reads every state.
#[test]
fn futures_conditional_lists_and_lookups_cover_what_the_doc_requires() {
    let m = futures();
    let s = settings();
    let qs = |h: &HttpRequest| h.query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let open = m.open_orders(&s, None, CorrId(1)).unwrap();
    let got: Vec<(&str, String)> = open[1..].iter().map(|h| (h.path.as_str(), qs(h))).collect();
    assert_eq!(
        got,
        vec![
            ("/api/v1/private/planorder/list/orders", "states=1&page_num=1&page_size=100".to_string()),
            ("/api/v1/private/stoporder/open_orders", String::new()),
            ("/api/v1/private/trackorder/list/orders", "states=0&pageIndex=1&pageSize=100".to_string()),
            ("/api/v1/private/trackorder/list/orders", "states=1&pageIndex=1&pageSize=100".to_string()),
        ]
    );
    let plan = m.query_by_venue_id(&s, "BTC_USDT", "cond:plan:7001", CorrId(1)).unwrap();
    assert_eq!((plan.len(), plan[0].path.as_str(), qs(&plan[0])), (1, "/api/v1/private/planorder/list/orders", "page_num=1&page_size=100".to_string()));
    let stop = m.query_by_venue_id(&s, "BTC_USDT", "cond:stop:7002", CorrId(1)).unwrap();
    assert_eq!(stop.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(), ["/api/v1/private/stoporder/open_orders", "/api/v1/private/stoporder/list/orders"]);
    assert!(qs(&stop[1]).find("is_finished").is_none(), "every state");
    let track = m.query_by_venue_id(&s, "BTC_USDT", "cond:track:7003", CorrId(1)).unwrap();
    assert_eq!(track.iter().map(|h| h.query[0].1.clone()).collect::<Vec<_>>(), ["0", "1", "2", "3", "4"]);
    // A cancelled plan order is still found by its id.
    let gone = r#"{"success":true,"code":0,"data":[{"id":"7001","symbol":"BTC_USDT","side":1,"triggerPrice":95000,"price":95100,"vol":10,"state":2,"orderType":1}]}"#;
    let (rows, next) = m.decode_orders_in(&DecodeCtx::new(plan[0].endpoint_id, &btc_sizes()), gone.as_bytes()).unwrap();
    assert_eq!((rows[0].venue_id.as_str(), rows[0].status.clone(), next), ("cond:plan:7001", Mapped::Known(OrderStatus::Cancelled), None));
}

/// One working conditional list the venue refuses reads as empty (OKX's algo list does the
/// same): the plain orders and the other lists are still read. A rate limit, a key refusal, or
/// the same code on the plain list or on a lookup still fails.
#[test]
fn a_refused_conditional_list_reads_as_empty() {
    let m = futures();
    let s = settings();
    let refused = r#"{"success":false,"code":600,"message":"Parameter error"}"#;
    for h in &m.open_orders(&s, None, CorrId(1)).unwrap()[1..] {
        assert!(m.decode_error(&http_frame(1, h.endpoint_id, 200, refused)).is_none(), "{}", h.endpoint_id);
        let (rows, next) = m.decode_orders_in(&DecodeCtx::new(h.endpoint_id, &btc_sizes()), refused.as_bytes()).unwrap();
        assert!(rows.is_empty() && next.is_none());
        let limited = r#"{"success":false,"code":510,"message":"Requests are too frequent"}"#;
        assert_eq!(m.decode_error(&http_frame(1, h.endpoint_id, 200, limited)).unwrap().canon, Canon::RateLimited);
        let key = r#"{"success":false,"code":401,"message":"Not logged in or login has expired"}"#;
        assert_eq!(m.decode_error(&http_frame(1, h.endpoint_id, 200, key)).unwrap().canon, Canon::Auth);
    }
    assert!(m.decode_error(&http_frame(1, "fut_open_orders", 200, refused)).is_some());
    let lookup = m.query_by_venue_id(&s, "BTC_USDT", "cond:plan:7001", CorrId(1)).unwrap();
    assert!(m.decode_error(&http_frame(1, lookup[0].endpoint_id, 200, refused)).is_some());
}

/// A row in one-way mode (`positionMode` 2) acts on the net position: its direction is the
/// code's, never a hedge side.
#[test]
fn a_one_way_order_row_reads_on_the_net_position() {
    let m = futures();
    let one_way = with(FUT_EXTERNAL, |v| {
        v["data"]["positionMode"] = json!(2);
        v["data"]["side"] = json!(4);
    });
    let (rows, _) = m.decode_orders_in(&DecodeCtx::new("fut_order_external", &btc_sizes()), one_way.as_bytes()).unwrap();
    assert_eq!((rows[0].position, rows[0].side.clone(), rows[0].reduce_only), (PositionSide::Net, Mapped::Known(Side::Sell), true));
    let hedge = with(FUT_EXTERNAL, |v| v["data"]["positionMode"] = json!(1));
    let (rows, _) = m.decode_orders_in(&DecodeCtx::new("fut_order_external", &btc_sizes()), hedge.as_bytes()).unwrap();
    assert_eq!(rows[0].position, PositionSide::Long);
    let plan = r#"{"success":true,"code":0,"data":[{"id":"7001","symbol":"BTC_USDT","side":3,"positionMode":2,"triggerPrice":95000,"price":95100,"vol":10,"state":1,"orderType":1}]}"#;
    let (rows, _) = m.decode_orders_in(&DecodeCtx::new("fut_plan_open", &btc_sizes()), plan.as_bytes()).unwrap();
    assert_eq!((rows[0].position, rows[0].side.clone()), (PositionSide::Net, Mapped::Known(Side::Sell)));
}

// ── Futures answers ──

#[test]
fn futures_answers_read_the_documented_examples_in_the_base_coin() {
    let m = futures();
    let sizes = btc_sizes();
    let ctx = DecodeCtx::new("fut_order_external", &sizes);
    assert_eq!(m.decode_ack(FUT_PLACE.as_bytes()).unwrap(), Ack { venue_id: "739113577038255616".into(), client_id: None });
    // The mirror writes the id bare in `data`.
    assert_eq!(m.decode_ack(br#"{"success":true,"code":0,"data":102057569836905984}"#).unwrap().venue_id, "102057569836905984");
    let (rows, next) = m.decode_orders_in(&ctx, FUT_EXTERNAL.as_bytes()).unwrap();
    let o = &rows[0];
    assert_eq!(
        (o.venue_id.as_str(), o.client_id.as_deref(), o.qty, o.filled_qty, o.status.clone(), o.position, o.side.clone(), o.order_type.as_str(), o.limit, o.avg_price, next),
        ("739106551624717312", Some("_m_903a9f81cc1148be99ac9ece15981374"), d("0.0005"), d("0.0005"), Mapped::Known(OrderStatus::Filled), PositionSide::Long, Mapped::Known(Side::Buy), "market", None, Some(d("109777.5")), None)
    );
    let (rows, next) = m.decode_orders_in(&DecodeCtx::new("fut_open_orders", &sizes), FUT_OPEN.as_bytes()).unwrap();
    assert_eq!((rows.len(), rows[0].qty, rows[1].qty, rows[0].status.clone(), rows[0].limit, next), (2, d("0.1"), d("0.001"), Mapped::Known(OrderStatus::New), Some(d("2500")), None));
    // A size the list does not give is never guessed.
    assert!(m.decode_orders_in(&DecodeCtx::new("x", &ContractSizes::new()), FUT_OPEN.as_bytes()).is_err());
    for (w, s) in [("1", OrderStatus::New), ("2", OrderStatus::New), ("3", OrderStatus::Filled), ("4", OrderStatus::Cancelled), ("5", OrderStatus::Rejected)] {
        assert_eq!(m.status_of(w), Mapped::Known(s));
    }
    assert!(matches!(m.status_of("6"), Mapped::Unknown(_)));
    // Fills, both shapes the page shows (a list, a page with `resultList`).
    let (f, next) = m.decode_fills_in(&ctx, FUT_DEALS.as_bytes()).unwrap();
    assert_eq!((f.len(), f[0].qty, f[0].side.clone(), f[0].fee_currency.as_str(), f[0].at_ms, f[0].position_only, next), (2, d("100"), Mapped::Known(Side::Sell), "USDT", 1_761_912_240_000, false, None));
    let page = with(FUT_DEALS, |v| {
        let rows = v["data"].clone();
        v["data"] = json!({ "pageSize": 20, "totalCount": 2, "totalPage": 1, "currentPage": 1, "resultList": rows });
    });
    assert_eq!(m.decode_fills_in(&ctx, page.as_bytes()).unwrap().0.len(), 2);
    let adl = with(FUT_DEALS, |v| v["data"][0]["category"] = json!(4));
    assert!(m.decode_fills_in(&ctx, adl.as_bytes()).unwrap().0[0].position_only, "an ADL reduction moved a position");
    // Positions sided in either mode; margin from `openType`.
    let pos = m.decode_positions_in(&ctx, FUT_POSITIONS.as_bytes()).unwrap();
    assert_eq!(pos, vec![VenuePosition { symbol: "ETH_USDT".into(), side: PositionSide::Long, direction: Some(Side::Buy), qty: d("0.01"), leverage: None, margin: Some(MarginMode::isolated()) }]);
    // Account mode: "1 Hedge Mode; 2 One-way Mode".
    assert_eq!(m.decode_account_position_mode(FUT_MODE.as_bytes()).unwrap(), Some(PositionMode::Hedge));
    assert_eq!(m.decode_account_position_mode(br#"{"success":true,"code":0,"data":2}"#).unwrap(), Some(PositionMode::OneWay));
    // Rules: contracts in the base coin, the limit and market caps apart.
    let (rules, _) = m.decode_rules(FUT_CONTRACT.as_bytes()).unwrap();
    let r = &rules[0];
    assert_eq!(
        (r.contract_size, r.tick, r.lot_step, r.min_qty, r.max_qty, r.market_max_qty, r.status.clone()),
        (Some(d("0.0001")), Some(d("0.1")), Some(d("0.0001")), Some(d("0.0001")), Some(d("250")), Some(d("40")), TradingStatus::Trading)
    );
    let off = with(FUT_CONTRACT, |v| v["data"]["apiAllowed"] = json!(false));
    assert!(matches!(m.decode_rules(off.as_bytes()).unwrap().0[0].status, TradingStatus::Closed(_)));
    let paused = with(FUT_CONTRACT, |v| v["data"]["state"] = json!(4));
    assert_eq!(m.decode_rules(paused.as_bytes()).unwrap().0[0].status, TradingStatus::Closed("4 (paused)".into()));
    let inverse = with(FUT_CONTRACT, |v| v["data"]["settleCoin"] = json!("BTC"));
    assert!(m.decode_rules(inverse.as_bytes()).unwrap().0.is_empty(), "coin-settled contracts are not served");
}

/// `position/leverage` answers a row per side: the margin per side, and the leverage only
/// when both sides hold the same (the trader keeps one per instrument).
#[test]
fn futures_margin_is_read_per_side_and_leverage_only_when_both_sides_agree() {
    let m = futures();
    assert_eq!(m.decode_symbol_margin_on("BTC_USDT", PositionSide::Long, FUT_LEVERAGE.as_bytes()).unwrap(), Some(MarginMode::isolated()));
    let mixed = with(FUT_LEVERAGE, |v| {
        v["data"][1]["openType"] = json!(2);
        v["data"][1]["leverage"] = json!(10);
    });
    assert_eq!(m.decode_symbol_margin_on("BTC_USDT", PositionSide::Short, mixed.as_bytes()).unwrap(), Some(MarginMode::cross()));
    assert_eq!(m.decode_symbol_margin_on("BTC_USDT", PositionSide::Net, mixed.as_bytes()).unwrap(), None);
    assert_eq!(m.decode_symbol_leverage("BTC_USDT", PositionSide::Long, FUT_LEVERAGE.as_bytes()).unwrap(), Some(d("2")));
    assert_eq!(m.decode_symbol_leverage("BTC_USDT", PositionSide::Long, mixed.as_bytes()).unwrap(), None, "never one side's leverage for the other");
}

// ── Errors ──

fn fut_err(code: &str) -> Option<VenueError> {
    futures().decode_error(&http_frame(1, "fut_create", 200, &json!({ "success": false, "code": code.parse::<i64>().unwrap(), "message": "m" }).to_string()))
}

fn spot_err(status: u16, code: &str) -> Option<VenueError> {
    spot().decode_error(&http_frame(1, "spot_order", status, &json!({ "code": code.parse::<i64>().unwrap(), "msg": "m" }).to_string()))
}

#[test]
fn lost_answers_are_unknown_and_6008_is_an_acceptance() {
    for c in ["500", "501", "603", "604", "801", "2042", "3016", "3017", "3018", "3019", "9999", "11444"] {
        assert_eq!(fut_err(c).unwrap().canon, Canon::Unknown, "futures {c}");
    }
    for c in ["500", "503", "504", "20002", "11444"] {
        assert_eq!(spot_err(400, c).unwrap().canon, Canon::Unknown, "spot {c}");
    }
    assert_eq!(spot().decode_error(&http_frame(1, "spot_order", 502, "<html>bad gateway</html>")).unwrap().canon, Canon::Unknown);
    assert_eq!(futures().decode_error(&http_frame(1, "fut_create", 503, "")).unwrap().canon, Canon::Unknown);
    let timeout = RawFrame::Timeout { corr_id: CorrId(1), endpoint_id: "fut_create" };
    assert_eq!(futures().decode_error(&timeout).unwrap().canon, Canon::Unknown);
    // "Order placed successfully": not an error; the ack names no id, found by client id.
    assert!(fut_err("6008").is_none());
    assert!(!futures().decode_ack(br#"{"success":false,"code":6008,"message":"Order placed successfully"}"#).unwrap().has_id());
}

#[test]
fn refusals_name_the_code_and_the_fix() {
    let e = spot_err(400, "70011").unwrap();
    assert_eq!(e.canon, Canon::Permission);
    assert!(e.message.contains("MEXC refused it: m (code 70011)") && e.message.contains("trading pairs"), "{}", e.message);
    assert!(spot_err(400, "30020").unwrap().message.contains("trading pairs"));
    assert!(fut_err("702").unwrap().message.contains("Order Placing"));
    assert_eq!(spot_err(400, "10101").unwrap().canon, Canon::Rejected(Refusal::Funds));
    assert_eq!(spot_err(400, "-2011").unwrap().canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(fut_err("2005").unwrap().canon, Canon::Rejected(Refusal::Funds));
    assert_eq!(fut_err("2008").unwrap().canon, Canon::Rejected(Refusal::ReduceOnly));
    let e = spot_err(400, "999999").unwrap();
    assert!(!e.mapped && e.canon == Canon::Unknown && e.message.contains("MEXC answered code 999999 (m), which OpenTraderWorld does not map"), "{e:?}");
    // A 2xx success body is no error; a spot rate limit carries its wait.
    assert!(spot().decode_error(&http_frame(1, "spot_order", 200, SPOT_NEW)).is_none());
    assert!(futures().decode_error(&http_frame(1, "fut_create", 200, FUT_PLACE)).is_none());
    let mut f = http_frame(1, "spot_order", 429, r#"{"code":429,"msg":"Too Many Requests"}"#);
    if let RawFrame::Http { headers, .. } = &mut f {
        headers.push(("retry-after".into(), "7".into()));
    }
    let e = spot().decode_error(&f).unwrap();
    assert_eq!((e.canon, e.retry_after_ms), (Canon::RateLimited, Some(7_000)));
    // A single futures cancel answers a row per id: the row's refusal is the call's.
    let one = |row: Value| futures().decode_error(&http_frame(1, super::FUT_CANCEL, 200, &json!({ "success": true, "code": 0, "data": [row] }).to_string()));
    let rows: Value = serde_json::from_str::<Value>(FUT_CANCEL).unwrap()["data"].clone();
    assert_eq!(one(rows[0].clone()).unwrap().canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(one(rows[1].clone()).unwrap().canon, Canon::Rejected(Refusal::Final));
    assert!(one(rows[2].clone()).is_none());
    let m = futures();
    assert!(m.invalidates_position_mode(&fut_err("7002").unwrap()) && m.invalidates_margin_mode(&fut_err("2027").unwrap()));
}

/// PROVIDER.md's error tables list the same codes with the same kinds.
#[test]
fn the_tables_match_provider_md() {
    let doc = include_str!("PROVIDER.md");
    let word = |c: &Canon| match c {
        Canon::Rejected(Refusal::VenueRule) => "Rejected(venue_rule)",
        Canon::Rejected(Refusal::Funds) => "Rejected(funds)",
        Canon::Rejected(Refusal::Price) => "Rejected(price)",
        Canon::Rejected(Refusal::MarketClosed) => "Rejected(market closed)",
        Canon::Rejected(Refusal::NotFound) => "Rejected(not found)",
        Canon::Rejected(Refusal::Final) => "Rejected(final)",
        Canon::Rejected(Refusal::ReduceOnly) => "Rejected(reduce only)",
        Canon::Rejected(Refusal::PostOnly) => "Rejected(post only)",
        Canon::Rejected(Refusal::Other) => "Rejected(other)",
        Canon::Unknown => "Unknown",
        Canon::RateLimited => "RateLimited",
        Canon::Auth => "Auth",
        Canon::Permission => "Permission",
        Canon::InvalidRequest => "InvalidRequest",
    };
    for (head, end, table) in [("#### Spot errors", "#### Futures errors", super::super::trade_errors::SPOT_ERRORS), ("#### Futures errors", "### Doc gaps", super::super::trade_errors::FUT_ERRORS)] {
        let start = doc.find(head).unwrap();
        let section = &doc[start..start + doc[start..].find(end).unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in section.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            for c in cells[1].split('`').skip(1).step_by(2) {
                seen.push((c.to_string(), cells[2].to_string()));
            }
        }
        let mut want: Vec<(String, String)> = table.iter().map(|(c, k, _)| (c.to_string(), word(k).to_string())).collect();
        seen.sort();
        want.sort();
        assert_eq!(seen, want, "{head}");
        let mut codes: Vec<&str> = table.iter().map(|r| r.0).collect();
        let n = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), n, "{head}: a code listed twice");
    }
}

// ── Private streams ──

#[test]
fn the_futures_stream_reads_the_documented_pushes_in_the_base_coin() {
    let m = futures();
    let sizes = btc_sizes();
    let ctx = DecodeCtx::new("/edge", &sizes);
    let s = m.private_socket(&settings()).unwrap();
    assert_eq!((s.base, s.path.as_str(), s.subscribes().len(), s.ping.as_slice(), s.ping_every_ms), ("contract-ws", "/edge", 0, br#"{"method":"ping"}"#.as_slice(), 15_000));
    let ev = m.decode_private_in(&ctx, WS_FUT_ORDER.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.venue_id.as_str(), o.client_id.as_deref(), o.qty, o.filled_qty, o.status.clone(), o.position), ("123456789", Some("ext_001"), d("0.001"), d("0.0005"), Mapped::Known(OrderStatus::PartiallyFilled), PositionSide::Long));
    let ev = m.decode_private_in(&ctx, WS_FUT_DEAL.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.exec_id.as_str(), f.qty, f.fee, f.client_id.as_deref()), ("987654321", d("0.001"), d("0.225"), Some("ext_001")));
    let ev = m.decode_private_in(&ctx, WS_FUT_POSITION.as_bytes()).unwrap();
    assert!(matches!(&ev[0], PrivateEvent::Position(p) if p.side == PositionSide::Long && p.qty == d("0.001") && p.leverage.is_none()));
    assert_eq!(m.decode_private_in(&ctx, WS_FUT_MODE.as_bytes()).unwrap(), vec![PrivateEvent::PositionMode { symbol: None, mode: Some(PositionMode::OneWay) }]);
    let login = m.decode_private_in(&ctx, br#"{ "channel": "rs.login", "data": "success", "ts": "1587442022003" }"#).unwrap();
    assert!(matches!(&login[0], PrivateEvent::Answer { op, ok: true, .. } if op == "auth"));
    let refused = m.decode_private_in(&ctx, br#"{"channel":"rs.error","data":"invalid signature"}"#).unwrap();
    assert!(matches!(&refused[0], PrivateEvent::Answer { ok: false, .. }));
    assert!(matches!(&m.decode_private_in(&ctx, br#"{ "channel": "pong", "data": 1587453241453 }"#).unwrap()[0], PrivateEvent::Answer { op, .. } if op == "pong"));
    assert!(m.decode_private_in(&ctx, br#"{"channel":"push.personal.asset","data":{}}"#).unwrap().is_empty());
    assert!(m.private_socket_bootstrap(&settings(), &s, CorrId(1)).unwrap().is_none() && m.private_socket_keepalive(&settings(), &s, CorrId(1)).unwrap().is_none());
}

/// The spot stream: a listen key before each connection, extended every 30 minutes; orders
/// and deals in protobuf frames, answers in JSON text.
#[test]
fn the_spot_stream_reads_protobuf_frames_behind_a_listen_key() {
    use proto::write;
    let m = spot();
    let s = m.private_socket(&settings()).unwrap();
    assert_eq!(
        s.subscribes(),
        vec![br#"{"method":"SUBSCRIPTION","params":["spot@private.orders.v3.api.pb"]}"#.as_slice(), br#"{"method":"SUBSCRIPTION","params":["spot@private.deals.v3.api.pb"]}"#.as_slice()]
    );
    let boot = m.private_socket_bootstrap(&settings(), &s, CorrId(1)).unwrap().unwrap();
    assert_eq!((boot.method, boot.path.as_str(), boot.signed), (otw_connector_sdk::Method::Post, "/api/v3/userDataStream", true));
    let s = m.decode_private_socket_bootstrap(s, SPOT_LISTEN_KEY.as_bytes()).unwrap();
    assert_eq!(s.path, "/ws?listenKey=pqia91ma19a5s61cv6a81va65sdf19v8a65a1a5s61cv6a81va65sdf19v8a65a1");
    let (keep, every) = m.private_socket_keepalive(&settings(), &s, CorrId(1)).unwrap().unwrap();
    assert_eq!((keep.method, keep.query.clone(), every), (otw_connector_sdk::Method::Put, vec![("listenKey".into(), "pqia91ma19a5s61cv6a81va65sdf19v8a65a1a5s61cv6a81va65sdf19v8a65a1".into())], 1_800_000));
    // Answers.
    assert!(matches!(&m.decode_private(br#"{"id":0,"code":0,"msg":"spot@private.orders.v3.api.pb"}"#).unwrap()[0], PrivateEvent::Answer { op, ok: true, .. } if op == "subscribe"));
    assert!(matches!(&m.decode_private(br#"{"id":0,"code":0,"msg":"PONG"}"#).unwrap()[0], PrivateEvent::Answer { op, .. } if op == "pong"));
    // An order frame (GH-S "Spot Account Orders" values): read again by its id.
    let mut o = Vec::new();
    write::text(&mut o, 1, "C02__505979017439002624115");
    write::text(&mut o, 3, "3.5121");
    write::text(&mut o, 4, "1");
    write::int(&mut o, 7, 5);
    write::int(&mut o, 8, 2);
    write::int(&mut o, 15, 2);
    write::int(&mut o, 16, 1_736_417_034_259);
    let mut frame = Vec::new();
    write::text(&mut frame, 1, "spot@private.orders.v3.api.pb");
    write::text(&mut frame, 3, "MXUSDT");
    write::int(&mut frame, 6, 1_736_417_034_281);
    write::bytes(&mut frame, 304, &o);
    assert_eq!(m.decode_private(&frame).unwrap(), vec![PrivateEvent::OrderChanged { symbol: Some("MXUSDT".into()), venue_id: "C02__505979017439002624115".into() }]);
    // `synthetic`: the same frame without the order id (field 1), with its `clientId` (field 2):
    // read again by the client id; with neither, not read (the book is read again).
    let by_client = |cid: Option<&str>| {
        let mut o = Vec::new();
        if let Some(c) = cid {
            write::text(&mut o, 2, c);
        }
        write::int(&mut o, 15, 2);
        let mut frame = Vec::new();
        write::text(&mut frame, 1, "spot@private.orders.v3.api.pb");
        write::text(&mut frame, 3, "MXUSDT");
        write::bytes(&mut frame, 304, &o);
        frame
    };
    assert_eq!(m.decode_private(&by_client(Some("otw-k1"))).unwrap(), vec![PrivateEvent::OrderChangedByClient { symbol: Some("MXUSDT".into()), client_id: "otw-k1".into() }]);
    assert!(m.decode_private(&by_client(None)).is_err());
    // A deal frame (GH-S "Spot Account Deals" values).
    let mut dl = Vec::new();
    write::text(&mut dl, 1, "3.6962");
    write::text(&mut dl, 2, "1");
    write::text(&mut dl, 3, "3.6962");
    write::int(&mut dl, 4, 2);
    write::text(&mut dl, 7, "505979017439002624X1");
    write::text(&mut dl, 9, "C02__505979017439002624115");
    write::text(&mut dl, 10, "0.0003998377369698171");
    write::text(&mut dl, 11, "MX");
    write::int(&mut dl, 12, 1_736_417_034_280);
    let mut frame = Vec::new();
    write::text(&mut frame, 1, "spot@private.deals.v3.api.pb");
    write::text(&mut frame, 3, "MXUSDT");
    write::bytes(&mut frame, 306, &dl);
    let ev = m.decode_private(&frame).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!(
        (f.exec_id.as_str(), f.venue_order_id.as_str(), f.symbol.as_str(), f.side.clone(), f.qty, f.price, f.fee, f.fee_currency.as_str(), f.at_ms),
        ("505979017439002624X1", "C02__505979017439002624115", "MXUSDT", Mapped::Known(Side::Sell), d("1"), d("3.6962"), d("0.0003998377369698171"), "MX", 1_736_417_034_280)
    );
    assert!(m.decode_private(&frame[..frame.len() - 2]).is_err(), "a cut frame is refused");
}

#[test]
fn the_client_id_rule_takes_the_32_characters_externaloid_allows() {
    let m = futures();
    assert_eq!(m.client_id_rule().max_len, 32);
    assert!(m.client_id("otw-0123456789abcdef_0123456789a").is_ok());
    assert!(m.client_id(&"a".repeat(33)).is_err());
    assert!(m.client_id("a:b").is_err());
}

// ── The catalog, through the generic trader (spot) ──

/// The spot mapper with what the catalog's own requests cannot carry completed: a market buy's
/// amount to spend (the trader values it at the price read before sending, tested below).
#[derive(Clone)]
struct Catalog(Mexc);

impl Catalog {
    fn complete(&self, o: &OrderRequest, p: &Placing) -> Placing {
        let mut p = p.clone();
        if o.order_type == OrderType::Market && o.side == Side::Buy {
            p.quote_amount = p.quote_amount.or(Some(o.qty.value() * d("30000")));
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
        self.0.place(s, o, &self.complete(o, p))
    }
    fn dry_run(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<Option<HttpRequest>> {
        self.0.dry_run(s, o, &self.complete(o, p))
    }
    fn decode_dry_run(&self, body: &[u8]) -> R<String> {
        self.0.decode_dry_run(body)
    }
    fn cancel(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<HttpRequest> {
        self.0.cancel(s, symbol, id, corr)
    }
    fn cancel_all(&self, s: &Settings, symbols: &[String], corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.cancel_all(s, symbols, corr)
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
    fn fills_window_ms(&self) -> Option<i64> {
        self.0.fills_window_ms()
    }
    fn instrument_rules(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<HttpRequest> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn check_order(&self, o: &OrderRequest) -> R<()> {
        self.0.check_order(o)
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
    fn decode_fills(&self, body: &[u8]) -> R<Page<VenueFill>> {
        self.0.decode_fills(body)
    }
    fn decode_rules(&self, body: &[u8]) -> R<Page<VenueRules>> {
        self.0.decode_rules(body)
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

struct Script;

fn spot_refusal(status: u16, code: i64, msg: &str) -> Answer {
    Answer::new(status, json!({ "code": code, "msg": msg }).to_string())
}

impl VenueScript for Script {
    type M = Catalog;

    fn mapper(&self) -> Catalog {
        Catalog(spot())
    }

    fn settings(&self) -> HashMap<String, String> {
        settings()
    }

    fn signer(&self) -> Arc<dyn Signer> {
        Arc::new(signer::spot(&settings()).unwrap())
    }

    fn symbol(&self) -> &'static str {
        "BTCUSDT"
    }

    fn qty(&self) -> Decimal {
        d("0.001")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        match closed {
            None => SPOT_INFO.to_string(),
            Some(w) => with(SPOT_INFO, |v| v["symbols"][0]["status"] = json!(w)),
        }
    }

    fn closed_word(&self) -> &'static str {
        "2"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::PartiallyFilled => "PARTIALLY_FILLED",
            OrderStatus::Filled => "FILLED",
            OrderStatus::Cancelled => "CANCELED",
            _ => "NEW",
        }
    }

    fn ack(&self, _client_id: &str, venue_id: &str) -> String {
        with(SPOT_NEW, |v| {
            v["orderId"] = json!(venue_id);
            v["symbol"] = json!("BTCUSDT");
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut o: Value = serde_json::from_str::<Value>(SPOT_OPEN).unwrap()[0].clone();
                o["orderId"] = json!(r.venue_id);
                o["clientOrderId"] = json!(r.client_id);
                o["symbol"] = json!("BTCUSDT");
                o["side"] = json!(if r.side == Side::Buy { "BUY" } else { "SELL" });
                o["price"] = json!("30000");
                o["origQty"] = json!(r.qty.to_string());
                o["executedQty"] = json!(r.filled.to_string());
                o["status"] = json!(r.word);
                o
            })
            .collect();
        Value::Array(items).to_string()
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut f: Value = serde_json::from_str::<Value>(SPOT_TRADES).unwrap()[0].clone();
                f["symbol"] = json!("BTCUSDT");
                f["id"] = json!(r.exec_id);
                f["orderId"] = json!(r.venue_id);
                f["qty"] = json!(r.qty.to_string());
                f["price"] = json!(r.price.to_string());
                f["commission"] = json!(r.fee.to_string());
                f["commissionAsset"] = json!("USDT");
                f
            })
            .collect();
        Value::Array(items).to_string()
    }

    fn fee_currency(&self) -> &'static str {
        "USDT"
    }

    fn positions(&self, _rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        "[]".into()
    }

    fn ok(&self) -> String {
        SPOT_CANCEL.to_string()
    }

    fn dry_run_ok(&self) -> String {
        "{}".into()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        match r {
            VenueRefusal::Funds => spot_refusal(400, 10101, "Insufficient balance"),
            VenueRefusal::ReduceOnly => spot_refusal(400, 30004, "Insufficient position"),
            VenueRefusal::NotFound | VenueRefusal::Final => spot_refusal(400, -2011, "Unknown order sent"),
            VenueRefusal::RateLimited(s) => Answer { status: 429, body: json!({ "code": 429, "msg": "Too Many Requests" }).to_string(), headers: vec![("retry-after", s.to_string())] },
            VenueRefusal::ClockSkew => spot_refusal(400, 700003, "Timestamp for this request is outside of the recvWindow"),
            VenueRefusal::Unmapped => spot_refusal(400, 999_999, "something new"),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            VenueRefusal::PositionModeMismatch => spot_refusal(400, 26, "operation not allowed"),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_passes_on_spot() {
    let r = run_catalog(&Script).await;
    println!("spot ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    assert!(r.ran.len() >= 25 && r.ran.contains(&"E2D session check") && r.ran.contains(&"T31+T34"), "{:?}", r.ran);
}

// ── Through the trader, what the catalog cannot judge (futures side codes) ──

fn ok(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(body.to_string())
}

fn fut_refusal(code: i64, msg: &str) -> ResponseTemplate {
    ok(&json!({ "success": false, "code": code, "message": msg }).to_string())
}

fn driver(m: Mexc, server: &MockServer) -> otw_connector_sdk::Driver<Mexc> {
    otw_connector_testkit::driver_against(m, server, otw_connector_testkit::ManualClock::at(ts(CATALOG_NOW_MS / 1000)))
}

/// The futures venue's reads: the contract list (every contract, or one), the account's mode,
/// the margin and leverage per side.
async fn fut_reads(server: &MockServer, mode: &str) {
    let list = with(FUT_CONTRACT, |v| v["data"] = Value::Array(vec![v["data"].clone()]));
    Mock::given(method("GET")).and(path("/api/v1/contract/detail/country")).respond_with(ok(&list)).mount(server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/position/position_mode")).respond_with(ok(mode)).mount(server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/position/leverage")).and(query_param("symbol", "BTC_USDT")).respond_with(ok(FUT_LEVERAGE)).mount(server).await;
}

fn sent(reqs: &[otw_connector_testkit::wiremock::Request], p: &str) -> Vec<Value> {
    reqs.iter().filter(|r| r.url.path() == p).map(|r| serde_json::from_slice(&r.body).unwrap()).collect()
}

fn reads(reqs: &[otw_connector_testkit::wiremock::Request], p: &str) -> usize {
    reqs.iter().filter(|r| r.url.path() == p).count()
}

/// T01, T09, T15, T16, T30 on futures: the account's mode read once (hedge), each side's
/// margin and leverage from `position/leverage`, the side code by direction and reduce-only,
/// sizes in contracts, the body signed; refusals refused, never resent; a rate limit holds the
/// next order.
#[tokio::test]
async fn futures_place_in_hedge_mode_with_the_venues_margin_and_leverage() {
    let server = MockServer::start().await;
    fut_reads(&server, FUT_MODE).await;
    let create = "/api/v1/private/order/create";
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"t15\"")).respond_with(fut_refusal(2005, "Balance insufficient")).mount(&server).await;
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"t16\"")).respond_with(fut_refusal(2008, "Insufficient number of positions can be closed")).mount(&server).await;
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"t30\"")).respond_with(fut_refusal(510, "Requests are too frequent, please try again later")).mount(&server).await;
    Mock::given(method("POST")).and(path(create)).respond_with(ok(FUT_PLACE)).mount(&server).await;
    let s = settings();
    let signer = ContractSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-hedge" };
    let dv = driver(futures(), &server);
    let t: Trader<Mexc> = Trader::new();
    let m = futures();
    let lim = |k: &str, side| order(&m, "BTC_USDT", k, side, OrderType::Limit, "0.0005", Some("30000"));
    let placed = t.place(&dv, &a, &[lim("t01", Side::Buy)], &TicketOptions::default()).await.unwrap();
    assert_eq!(placed[0].venue_id, "739113577038255616");
    t.place(&dv, &a, &[lim("t01s", Side::Sell)], &TicketOptions::default()).await.unwrap();
    let mut ioc = lim("t09", Side::Buy);
    ioc.tif = TimeInForce::Ioc;
    t.place(&dv, &a, &[ioc], &TicketOptions::default()).await.unwrap();
    let out = t.place(&dv, &a, &[lim("t15", Side::Buy)], &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("code 2005")), "{out:?}");
    let mut reduce = lim("t16", Side::Sell);
    reduce.opts.reduce_only = true;
    let out = t.place(&dv, &a, &[reduce], &TicketOptions::default()).await;
    assert!(matches!(out, Err(TradeError::Refused(_))), "{out:?}");
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reads(&reqs, "/api/v1/private/position/position_mode"), 1, "the account's mode, read once");
    let bodies = sent(&reqs, create);
    let got: Vec<(i64, i64, i64, i64, i64, i64)> = bodies
        .iter()
        .map(|b| (b["side"].as_i64().unwrap(), b["type"].as_i64().unwrap(), b["vol"].as_i64().unwrap(), b["openType"].as_i64().unwrap(), b["leverage"].as_i64().unwrap(), b["positionMode"].as_i64().unwrap()))
        .collect();
    // Open long, open short, IOC open long, refused open long, reduce-only sell closes the long.
    assert_eq!(got, vec![(1, 1, 5, 1, 2, 1), (3, 1, 5, 1, 2, 1), (1, 3, 5, 1, 2, 1), (1, 1, 5, 1, 2, 1), (4, 1, 5, 1, 2, 1)]);
    assert_eq!(bodies.iter().filter(|b| b["externalOid"] == "t15").count(), 1, "never resent");
    let signed = reqs.iter().filter(|r| r.url.path() == create).all(|r| r.headers.contains_key("signature") && r.headers.get_all("content-type").iter().count() == 1);
    assert!(signed, "every order signed, one Content-Type");
    // T30: the venue's rate limit refuses; the code states no wait, so the next order goes.
    let out = t.place(&dv, &a, &[lim("t30", Side::Buy)], &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.kind == otw_connector_model::ErrorKind::RateLimited), "{out:?}");
}

/// A one-way account: refused before any order call, naming the fix. A mode pushed by the
/// stream is held account wide.
#[tokio::test]
async fn a_one_way_futures_account_is_refused_before_io() {
    let server = MockServer::start().await;
    fut_reads(&server, r#"{"success":true,"code":0,"data":2}"#).await;
    Mock::given(method("POST")).and(path("/api/v1/private/order/create")).respond_with(ok(FUT_PLACE)).mount(&server).await;
    let s = settings();
    let signer = ContractSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-oneway" };
    let dv = driver(futures(), &server);
    let t: Trader<Mexc> = Trader::new();
    let m = futures();
    let out = t.place(&dv, &a, &[order(&m, "BTC_USDT", "ow1", Side::Buy, OrderType::Limit, "0.0005", Some("30000"))], &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("hedge mode only")), "{out:?}");
    assert_eq!(reads(&server.received_requests().await.unwrap(), "/api/v1/private/order/create"), 0);
}

/// A one-way account's close is refused too: no page says which `side` code a one-way order
/// takes, nor how `reduceOnly` combines with it, so nothing is sent.
#[tokio::test]
async fn a_one_way_futures_position_close_is_refused_before_io() {
    let server = MockServer::start().await;
    fut_reads(&server, r#"{"success":true,"code":0,"data":2}"#).await;
    let long = with(FUT_POSITIONS, |v| {
        v["data"][0]["symbol"] = json!("BTC_USDT");
        v["data"][0]["holdVol"] = json!(20);
    });
    Mock::given(method("GET")).and(path("/api/v1/private/position/open_positions")).respond_with(ok(&long)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/private/order/create")).respond_with(ok(FUT_PLACE)).mount(&server).await;
    let s = settings();
    let signer = ContractSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-oneway-close" };
    let dv = driver(futures(), &server);
    let t: Trader<Mexc> = Trader::new();
    let out = t.close_position(&dv, &a, "BTC_USDT", None, None, "ow-close").await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("hedge mode only")), "{out:?}");
    assert_eq!(reads(&server.received_requests().await.unwrap(), "/api/v1/private/order/create"), 0);
}

/// 2021 and 7004 (the order's leverage is not the position's): the held leverage is dropped and
/// read again before the next order.
#[tokio::test]
async fn a_leverage_refusal_reads_the_leverage_again() {
    let server = MockServer::start().await;
    fut_reads(&server, FUT_MODE).await;
    let create = "/api/v1/private/order/create";
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"lv1\"")).respond_with(fut_refusal(2021, "Order leverage is inconsistent with the existing position leverage")).mount(&server).await;
    Mock::given(method("POST")).and(path(create)).respond_with(ok(FUT_PLACE)).mount(&server).await;
    let s = settings();
    let signer = ContractSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-lev" };
    let dv = driver(futures(), &server);
    let t: Trader<Mexc> = Trader::new();
    let m = futures();
    assert!(m.invalidates_leverage(&fut_err("2021").unwrap()) && m.invalidates_leverage(&fut_err("7004").unwrap()) && !m.invalidates_leverage(&fut_err("2005").unwrap()));
    let lim = |k: &str| order(&m, "BTC_USDT", k, Side::Buy, OrderType::Limit, "0.0005", Some("30000"));
    t.place(&dv, &a, &[lim("lv0")], &TicketOptions::default()).await.unwrap();
    let before = reads(&server.received_requests().await.unwrap(), "/api/v1/private/position/leverage");
    assert!(t.place(&dv, &a, &[lim("lv1")], &TicketOptions::default()).await.is_err());
    t.place(&dv, &a, &[lim("lv2")], &TicketOptions::default()).await.unwrap();
    let after = reads(&server.received_requests().await.unwrap(), "/api/v1/private/position/leverage");
    assert!(after > before, "read again after the refusal ({before} then {after})");
}

/// T19, T20, T21 and 6008 on futures: a lost answer is looked up by `externalOid`, adopted when
/// found, unknown when not, never sent again; an acceptance with no id is found the same way.
#[tokio::test]
async fn futures_lost_answers_are_looked_up_by_external_oid() {
    let server = MockServer::start().await;
    fut_reads(&server, FUT_MODE).await;
    let create = "/api/v1/private/order/create";
    let found = |k: &str, id: &str| {
        with(FUT_EXTERNAL, |v| {
            v["data"]["externalOid"] = json!(k);
            v["data"]["orderId"] = json!(id);
            v["data"]["state"] = json!(2);
            v["data"]["dealVol"] = json!(0);
        })
    };
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"t19\"")).respond_with(ResponseTemplate::new(502)).mount(&server).await;
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"t20\"")).respond_with(fut_refusal(3016, "BTC_USDT order timed out")).mount(&server).await;
    Mock::given(method("POST")).and(path(create)).and(body_string_contains("\"p6008\"")).respond_with(fut_refusal(6008, "Order placed successfully")).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/order/external/BTC_USDT/t19")).respond_with(ok(&found("t19", "91"))).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/order/external/BTC_USDT/p6008")).respond_with(ok(&found("p6008", "92"))).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/order/external/BTC_USDT/t20")).respond_with(fut_refusal(2040, "Order does not exist")).mount(&server).await;
    let s = settings();
    let signer = ContractSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-lost" };
    let dv = driver(futures(), &server);
    let t: Trader<Mexc> = Trader::new();
    let m = futures();
    let lim = |k: &str| order(&m, "BTC_USDT", k, Side::Buy, OrderType::Limit, "0.0005", Some("30000"));
    assert_eq!(t.place(&dv, &a, &[lim("t19")], &TicketOptions::default()).await.unwrap()[0].venue_id, "91");
    assert_eq!(t.place(&dv, &a, &[lim("p6008")], &TicketOptions::default()).await.unwrap()[0].venue_id, "92");
    let out = t.place(&dv, &a, &[lim("t20")], &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Unknown { keys, alert: false, .. }) if keys == &vec!["t20".to_string()]), "{out:?}");
    let bodies = sent(&server.received_requests().await.unwrap(), create);
    for k in ["t19", "t20", "p6008"] {
        assert_eq!(bodies.iter().filter(|b| b["externalOid"] == k).count(), 1, "{k} never resent");
    }
}

/// T10 on futures (hedge): the position read now, closed at the market on its side's close
/// code, whole or in part; more than held is refused before I/O. T22: a cancel the venue
/// answers 2041 is read back and ends as the state found. T34: cancel all per contract.
#[tokio::test]
async fn futures_close_cancel_race_and_cancel_all() {
    let server = MockServer::start().await;
    fut_reads(&server, FUT_MODE).await;
    let long = with(FUT_POSITIONS, |v| {
        v["data"][0]["symbol"] = json!("BTC_USDT");
        v["data"][0]["holdVol"] = json!(20);
    });
    Mock::given(method("GET")).and(path("/api/v1/private/position/open_positions")).respond_with(ok(&long)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/private/order/create")).respond_with(ok(FUT_PLACE)).mount(&server).await;
    let s = settings();
    let signer = ContractSigner::from_secrets(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-close" };
    let dv = driver(futures(), &server);
    let t: Trader<Mexc> = Trader::new();
    t.close_position(&dv, &a, "BTC_USDT", None, None, "c1").await.unwrap();
    t.close_position(&dv, &a, "BTC_USDT", None, Some(d("0.0005")), "c2").await.unwrap();
    assert!(matches!(t.close_position(&dv, &a, "BTC_USDT", None, Some(d("0.01")), "c3").await, Err(TradeError::Refused(_))));
    let bodies = sent(&server.received_requests().await.unwrap(), "/api/v1/private/order/create");
    let got: Vec<(i64, i64, i64, bool)> = bodies.iter().map(|b| (b["side"].as_i64().unwrap(), b["type"].as_i64().unwrap(), b["vol"].as_i64().unwrap(), b.get("price").is_none())).collect();
    assert_eq!(got, vec![(4, 5, 20, true), (4, 5, 5, true)], "close long, at the market, in contracts");
    // T22: the cancel raced a fill.
    let rows: Value = serde_json::from_str::<Value>(FUT_CANCEL).unwrap()["data"].clone();
    Mock::given(method("POST")).and(path("/api/v1/private/order/cancel")).and(body_string_contains("108885377779302912")).respond_with(ok(&json!({ "success": true, "code": 0, "data": [rows[1]] }).to_string())).mount(&server).await;
    let filled = with(FUT_EXTERNAL, |v| v["data"]["orderId"] = json!("108885377779302912"));
    Mock::given(method("GET")).and(path("/api/v1/private/order/get/108885377779302912")).respond_with(ok(&filled)).mount(&server).await;
    let out = t.cancel(&dv, &a, "BTC_USDT", "108885377779302912").await;
    assert_eq!(out, Ok(Cancelled::AlreadyFinal(OrderStatus::Filled)));
    // T34: every working order listed, cancelled per contract.
    let open = with(FUT_OPEN, |v| {
        v["data"][0]["symbol"] = json!("BTC_USDT");
    });
    Mock::given(method("GET")).and(path("/api/v1/private/order/list/open_orders")).respond_with(ok(&open)).up_to_n_times(1).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/order/list/open_orders")).respond_with(ok(r#"{"success":true,"code":0,"data":[]}"#)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v1/private/order/cancel_all")).respond_with(ok(FUT_CANCEL_ALL)).mount(&server).await;
    // The conditional orders placed elsewhere: a plan order and a trailing one, listed once
    // (then gone); plan and stop orders cancelled per contract, the trailing one on its own.
    let plan = r#"{"success":true,"code":0,"data":[{"id":"7001","symbol":"BTC_USDT","side":1,"triggerPrice":95000,"price":95100,"vol":10,"state":1,"orderType":1}]}"#;
    let track = r#"{"success":true,"code":0,"data":[{"id":7003,"symbol":"BTC_USDT","side":4,"vol":5,"activePrice":99000,"state":0}]}"#;
    let empty = r#"{"success":true,"code":0,"data":[]}"#;
    Mock::given(method("GET")).and(path("/api/v1/private/planorder/list/orders")).respond_with(ok(plan)).up_to_n_times(1).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/planorder/list/orders")).respond_with(ok(empty)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/stoporder/open_orders")).respond_with(ok(empty)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/trackorder/list/orders")).and(query_param("states", "0")).respond_with(ok(track)).up_to_n_times(2).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v1/private/trackorder/list/orders")).respond_with(ok(empty)).mount(&server).await;
    for p in ["planorder/cancel_all", "stoporder/cancel_all", "trackorder/cancel"] {
        Mock::given(method("POST")).and(path(format!("/api/v1/private/{p}"))).respond_with(ok(r#"{"success":true,"code":0}"#)).mount(&server).await;
    }
    assert_eq!(t.cancel_all(&dv, &a).await.unwrap(), 4);
    let reqs = server.received_requests().await.unwrap();
    for p in ["order/cancel_all", "planorder/cancel_all", "stoporder/cancel_all"] {
        assert_eq!(sent(&reqs, &format!("/api/v1/private/{p}")), vec![json!({ "symbol": "BTC_USDT" })], "{p}");
    }
    assert_eq!(sent(&reqs, "/api/v1/private/trackorder/cancel"), vec![json!({ "symbol": "BTC_USDT", "trackOrderId": 7003 })]);
}

/// A conditional order placed elsewhere cancels on its kind's own endpoint.
#[test]
fn futures_conditional_orders_cancel_on_their_own_endpoints() {
    let m = futures();
    let s = Settings::default();
    let plan = m.cancel(&s, "BTC_USDT", "cond:plan:7001", CorrId(1)).unwrap();
    let stop = m.cancel(&s, "BTC_USDT", "cond:stop:7002", CorrId(1)).unwrap();
    let body = |h: &HttpRequest| serde_json::from_slice::<Value>(h.body.as_deref().unwrap()).unwrap();
    assert_eq!((plan.path.as_str(), body(&plan)), ("/api/v1/private/planorder/cancel", json!([{ "symbol": "BTC_USDT", "orderId": "7001" }])));
    assert_eq!((stop.path.as_str(), body(&stop)), ("/api/v1/private/stoporder/cancel", json!([{ "stopPlanOrderId": 7002 }])));
    assert!(m.cancel(&s, "BTC_USDT", "cond:plan:x", CorrId(1)).is_err());
}

/// Spot through the trader: a market buy valued at the best ask read just before sending,
/// rounded down to the quote step; the order test once per session.
#[tokio::test]
async fn spot_market_buy_is_valued_at_the_ask_and_rounded_down() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v3/exchangeInfo")).respond_with(ok(SPOT_INFO)).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/v3/ticker/bookTicker")).respond_with(ok(r#"{"symbol":"BTCUSDT","bidPrice":"30000.1","bidQty":"1","askPrice":"30001.237","askQty":"2"}"#)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/v3/order")).respond_with(ok(SPOT_NEW)).mount(&server).await;
    let s = settings();
    let signer = signer::spot(&s).unwrap();
    let a = Account { settings: &s, signer: &signer, scope: "mx-spot" };
    let dv = driver(spot(), &server);
    let t: Trader<Mexc> = Trader::new();
    let m = spot();
    let placed = t.place(&dv, &a, &[order(&m, "BTCUSDT", "mb1", Side::Buy, OrderType::Market, "0.001", None)], &TicketOptions::default()).await.unwrap();
    assert!(placed[0].note.as_deref().is_some_and(|n| n.contains("30")), "{placed:?}");
    let reqs = server.received_requests().await.unwrap();
    let b: Vec<String> = reqs.iter().filter(|r| r.url.path() == "/api/v3/order").map(|r| String::from_utf8_lossy(&r.body).to_string()).collect();
    // 0.001 at 30001.237 is 30.001237, down to the 0.01 step.
    assert_eq!(b, vec!["symbol=BTCUSDT&side=BUY&type=MARKET&quoteOrderQty=30&newClientOrderId=mb1".to_string()]);
    let r = reqs.iter().find(|r| r.url.path() == "/api/v3/order").unwrap();
    assert!(r.url.query().unwrap().contains("signature=") && r.headers.get("content-type").unwrap() == "application/json");
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live instrument lists decode to sound rules for every instrument, and an order call
/// signed with a key MEXC never issued, or not signed at all, is answered with the auth error
/// the table expects (nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers mexc::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    for (m, symbol, qty, price) in [(spot(), "BTCUSDT", "0.0001", "1000"), (futures(), "BTC_USDT", "0.0001", "1000")] {
        let label = m.capabilities().label.clone();
        let provider = m.capabilities().provider.clone();
        let d = Driver::new(m.clone(), DriverConfig::new(&provider), reqwest::Client::new());
        let junk = settings();
        let signer: Arc<dyn Signer> = if m.book() == super::super::Book::Futures { Arc::new(ContractSigner::from_secrets(&junk).unwrap()) } else { Arc::new(signer::spot(&junk).unwrap()) };
        let a = Account { settings: &junk, signer: signer.as_ref(), scope: "net" };
        let trader: Trader<Mexc> = Trader::new();
        let rules = trader.all_rules(&d, &a).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(rules.len() > 10, "{label}: {} instruments", rules.len());
        for r in &rules {
            r.sound().unwrap_or_else(|e| panic!("{label}: {e}"));
            assert!(r.tick.is_some() && r.lot_step.is_some(), "{label}: {} states no tick or step", r.instrument.native);
        }
        let btc = rules.iter().find(|r| r.instrument.native == symbol).unwrap_or_else(|| panic!("{label}: no {symbol}"));
        println!("{label}: {} instruments, {symbol} {btc:?}", rules.len());
        let o = order(&m, symbol, "otw-net-probe", Side::Buy, OrderType::Limit, qty, Some(price));
        let p = Placing { margin: Some(MarginMode::isolated()), contract_size: btc.contract_size, ..Placing::new("otw-net-probe", PositionSide::Long, CorrId(1)) };
        let req = m.place(&junk, &o, &p).unwrap();
        for (what, signed) in [("junk key", true), ("unsigned", false)] {
            let mut r = req.clone();
            r.signed = signed;
            let frame = d.send_as(r, Some(signer.as_ref())).await.unwrap_or_else(|e| panic!("{label} {what}: {e}"));
            let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label} {what}: MEXC accepted an order with no valid key"));
            println!("{label} {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
            assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{label} {what}: {e:?}");
        }
    }
}
