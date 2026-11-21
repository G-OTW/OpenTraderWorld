//! Bitfinex orders, from the documentation alone. Fixtures under `fixtures/trade/`, each the
//! doc's example (read 2026-11-12) with only its `//` field comments and its `[...]` / `...`
//! placeholders taken out (they are not JSON), or a measured public answer:
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_submit_order.json` | https://docs.bitfinex.com/reference/rest-auth-submit-order |
//! | `doc_update_order.json` | https://docs.bitfinex.com/reference/rest-auth-update-order |
//! | `doc_cancel_order.json` | https://docs.bitfinex.com/reference/rest-auth-cancel-order |
//! | `doc_cancel_multi.json` | https://docs.bitfinex.com/reference/rest-auth-cancel-orders-multiple |
//! | `doc_retrieve_orders.json` | https://docs.bitfinex.com/reference/rest-auth-retrieve-orders |
//! | `doc_orders_history.json` | https://docs.bitfinex.com/reference/rest-auth-orders-history |
//! | `doc_trades_by_symbol.json` | https://docs.bitfinex.com/reference/rest-auth-trades-by-symbol |
//! | `doc_positions.json` | https://docs.bitfinex.com/reference/rest-auth-positions |
//! | `doc_ws_order_cancel.json` | https://docs.bitfinex.com/reference/ws-auth-orders (`oc`; its `{meta_object}` written `null`) |
//! | `doc_ws_position_update.json` | https://docs.bitfinex.com/reference/ws-auth-positions (`pu`) |
//! | `doc_ws_trade_executed.json` | https://docs.bitfinex.com/reference/ws-auth-trades (`te`) |
//! | `doc_ws_trade_update.json` | https://docs.bitfinex.com/reference/ws-auth-trades (`tu`) |
//! | `measured_info_pair.json` | `GET https://api-pub.bitfinex.com/v2/conf/pub:info:pair`, 2026-11-12, four pairs kept |
//! | `measured_info_pair_futures.json` | `GET https://api-pub.bitfinex.com/v2/conf/pub:info:pair:futures`, 2026-11-12, three kept |
//!
//! The catalog script ([`Script`]) and the derivatives rows answer with these examples with
//! only their ids, sizes, prices, symbol and status changed: those answers are `synthetic`,
//! built on the documented shape. Every refusal body is `synthetic` too: the doc lists codes and
//! the notification layout, not refusal bodies (the `["error", ...]` array is the collateral
//! page's own example shape).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use otw_connector_engine::trading::{Account, TicketOptions, TradeError, Trader};
use otw_connector_model::{
    Capabilities, Command, ConnectorError, CorrId, Decimal, Envelope, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce,
};
use otw_connector_sdk::trade::{
    check_rules, Ack, CancelAllWay, Canon, ClientIdRule, ClientRef, ContractSizes, DecodeCtx, ErrorRow, LookupAt, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Refusal,
    Settings, TradeMapper, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, RawFrame, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript, CATALOG_NOW_MS};
use otw_connector_testkit::{driver_against, http_frame, ManualClock};
use serde_json::{json, Value};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::signer::BitfinexSigner;
use super::trade::{cid_dates, date_of, price_step, status_head, FILLS_LIMIT, INT45_MAX};
use super::Bitfinex;

const SUBMIT: &str = include_str!("fixtures/trade/doc_submit_order.json");
const UPDATE: &str = include_str!("fixtures/trade/doc_update_order.json");
const CANCEL: &str = include_str!("fixtures/trade/doc_cancel_order.json");
const CANCEL_MULTI: &str = include_str!("fixtures/trade/doc_cancel_multi.json");
const RETRIEVE: &str = include_str!("fixtures/trade/doc_retrieve_orders.json");
const HISTORY: &str = include_str!("fixtures/trade/doc_orders_history.json");
const TRADES: &str = include_str!("fixtures/trade/doc_trades_by_symbol.json");
const POSITIONS: &str = include_str!("fixtures/trade/doc_positions.json");
const WS_OC: &str = include_str!("fixtures/trade/doc_ws_order_cancel.json");
const WS_PU: &str = include_str!("fixtures/trade/doc_ws_position_update.json");
const WS_TE: &str = include_str!("fixtures/trade/doc_ws_trade_executed.json");
const WS_TU: &str = include_str!("fixtures/trade/doc_ws_trade_update.json");
const INFO_PAIR: &str = include_str!("fixtures/trade/measured_info_pair.json");
const INFO_FUTURES: &str = include_str!("fixtures/trade/measured_info_pair_futures.json");

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn spot() -> Bitfinex {
    Bitfinex::spot().unwrap()
}

fn deriv() -> Bitfinex {
    Bitfinex::derivatives().unwrap()
}

fn settings() -> Settings {
    HashMap::from([("api_key".to_string(), "bfx-key".to_string()), ("api_secret".to_string(), "bfx-test-secret".to_string())])
}

fn signer() -> BitfinexSigner {
    BitfinexSigner::from_secrets(&settings()).unwrap()
}

fn body(r: &HttpRequest) -> Value {
    serde_json::from_slice(r.body.as_deref().unwrap()).unwrap()
}

fn with(src: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(src).unwrap();
    f(&mut v);
    v.to_string()
}

fn order(m: &Bitfinex, sym: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>, stop: Option<&str>) -> OrderRequest {
    OrderRequest {
        client_order_id: "k1".into(),
        instrument: InstrumentId::new(m.book().venue(), sym),
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

const BTC: &str = "tBTCUSD";
const PERP: &str = "tBTCF0:USTF0";
const STAMP: i64 = 1_792_400_000_000;

// ── Requests ──

/// Submit Order: the type carries the book (`EXCHANGE` on spot), the amount its sign, the
/// price its role; IOC and FOK are types; good-till-date is `tif`; post-only and reduce-only
/// are flags; the minted client id goes as `cid`.
#[test]
fn orders_go_out_in_the_venues_words() {
    let m = spot();
    let s = settings();
    let p = placing(&STAMP.to_string());
    let lim = m.place(&s, &order(&m, BTC, Side::Buy, OrderType::Limit, "0.1", Some("10000"), None), &p).unwrap();
    assert_eq!((lim.path.as_str(), lim.endpoint_id, lim.signed), ("/v2/auth/w/order/submit", "orderSubmit", true));
    assert_eq!(body(&lim), json!({"type":"EXCHANGE LIMIT","symbol":"tBTCUSD","amount":"0.1","price":"10000","cid":STAMP}));
    // A market sell: negative amount, no price ("Not required for market orders").
    let mkt = m.place(&s, &order(&m, BTC, Side::Sell, OrderType::Market, "0.25", None, None), &p).unwrap();
    assert_eq!(body(&mkt), json!({"type":"EXCHANGE MARKET","symbol":"tBTCUSD","amount":"-0.25","cid":STAMP}));
    // A market order as immediate-or-cancel is the same order.
    let mut ioc_mkt = order(&m, BTC, Side::Sell, OrderType::Market, "0.25", None, None);
    ioc_mkt.tif = TimeInForce::Ioc;
    assert_eq!(body(&m.place(&s, &ioc_mkt, &p).unwrap())["type"], "EXCHANGE MARKET");
    let stop = m.place(&s, &order(&m, BTC, Side::Sell, OrderType::Stop, "0.1", None, Some("9000")), &p).unwrap();
    assert_eq!(body(&stop), json!({"type":"EXCHANGE STOP","symbol":"tBTCUSD","amount":"-0.1","price":"9000","cid":STAMP}));
    let sl = m.place(&s, &order(&m, BTC, Side::Buy, OrderType::StopLimit, "0.1", Some("11010"), Some("11000")), &p).unwrap();
    assert_eq!(body(&sl), json!({"type":"EXCHANGE STOP LIMIT","symbol":"tBTCUSD","amount":"0.1","price":"11000","price_aux_limit":"11010","cid":STAMP}));
    for (tif, word) in [(TimeInForce::Ioc, "EXCHANGE IOC"), (TimeInForce::Fok, "EXCHANGE FOK")] {
        let mut o = order(&m, BTC, Side::Buy, OrderType::Limit, "0.1", Some("10000"), None);
        o.tif = tif;
        assert_eq!(body(&m.place(&s, &o, &p).unwrap())["type"], word);
    }
    let mut gtd = order(&m, BTC, Side::Buy, OrderType::Limit, "0.1", Some("10000"), None);
    gtd.tif = TimeInForce::Gtd(otw_connector_testkit::ts(1_579_085_123));
    gtd.opts.post_only = true;
    let b = body(&m.place(&s, &gtd, &p).unwrap());
    assert_eq!((b["tif"].as_str(), b["flags"].as_i64(), b["type"].as_str()), (Some("2020-01-15 10:45:23"), Some(4096), Some("EXCHANGE LIMIT")));
    // A check carries no client id.
    assert!(body(&m.place(&s, &order(&m, BTC, Side::Buy, OrderType::Limit, "0.1", Some("10000"), None), &placing("")).unwrap()).get("cid").is_none());
}

/// Derivatives: unprefixed types, reduce-only flag, the held leverage as `lev` when it is one
/// an order can carry (a whole number from 1 to 100); none when flat or when the venue states a
/// fraction.
#[test]
fn a_derivatives_order_carries_the_held_leverage_or_none() {
    let m = deriv();
    let s = settings();
    let mut o = order(&m, PERP, Side::Sell, OrderType::Market, "0.5", None, None);
    o.opts.reduce_only = true;
    let held = Placing { leverage: Some(d("5")), ..placing(&STAMP.to_string()) };
    assert_eq!(body(&m.place(&s, &o, &held).unwrap()), json!({"type":"MARKET","symbol":"tBTCF0:USTF0","amount":"-0.5","flags":1024,"lev":5,"cid":STAMP}));
    let flat = placing(&STAMP.to_string());
    assert!(body(&m.place(&s, &o, &flat).unwrap()).get("lev").is_none(), "none held: none sent");
    let ratio = Placing { leverage: Some(d("1.409288545397275")), ..placing(&STAMP.to_string()) };
    assert!(body(&m.place(&s, &o, &ratio).unwrap()).get("lev").is_none(), "a fraction is never rounded into a leverage");
    for (l, sent) in [("10.0", Some(10)), ("100", Some(100)), ("101", None), ("0.5", None)] {
        let p = Placing { leverage: Some(d(l)), ..placing(&STAMP.to_string()) };
        assert_eq!(body(&m.place(&s, &o, &p).unwrap())["lev"].as_i64(), sent, "{l}");
    }
    // Spot takes no reduce-only and no leverage.
    let sp = spot();
    let mut ro = order(&sp, BTC, Side::Sell, OrderType::Market, "0.5", None, None);
    ro.opts.reduce_only = true;
    assert!(sp.place(&s, &ro, &placing(&STAMP.to_string())).is_err());
    let lev = Placing { leverage: Some(d("5")), ..placing(&STAMP.to_string()) };
    assert!(body(&sp.place(&s, &order(&sp, BTC, Side::Buy, OrderType::Market, "0.5", None, None), &lev).unwrap()).get("lev").is_none());
    // Each book refuses the other's symbols.
    assert!(m.place(&s, &order(&m, BTC, Side::Buy, OrderType::Market, "1", None, None), &flat).is_err());
    assert!(sp.place(&s, &order(&sp, PERP, Side::Buy, OrderType::Market, "1", None, None), &flat).is_err());
}

/// "All pairs on Bitfinex use up to 5 significant digits and up to 8 decimals ... Prices submit
/// with a precision larger than 5 will be cut by the API": refused before I/O with the two
/// nearest; amounts of more than 8 decimals refused by the rules' step.
#[test]
fn prices_and_amounts_the_venue_would_cut_are_refused_with_the_nearest() {
    let m = spot();
    for (p, step) in [("1.2345", "0.0001"), ("123.45", "0.01"), ("1234.5", "0.1"), ("0.00012345", "0.00000001"), ("98765", "1"), ("123456", "10"), ("100000", "10")] {
        assert_eq!(price_step(d(p)), d(step), "{p}");
    }
    for ok in ["1.2345", "123.45", "1234.5", "0.00012345", "12345", "123400", "0.1"] {
        assert!(m.check_order(&order(&m, BTC, Side::Buy, OrderType::Limit, "1", Some(ok), None)).is_ok(), "{ok}");
    }
    let e = m.check_order(&order(&m, BTC, Side::Buy, OrderType::Limit, "1", Some("123456"), None)).unwrap_err();
    assert!(e.message.contains("123450 or 123460") && e.provider_code.as_deref() == Some("venue_rule"), "{}", e.message);
    let e = m.check_order(&order(&m, BTC, Side::Buy, OrderType::Stop, "1", None, Some("30000.5"))).unwrap_err();
    assert!(e.message.contains("stop price 30000.5") && e.message.contains("30000 or 30001"), "{}", e.message);
    assert!(m.check_order(&order(&m, BTC, Side::Buy, OrderType::Limit, "1", Some("0.000123456"), None)).is_err());
    // The amount: 8 decimals at most, through the instrument's rules.
    let (rules, _) = m.decode_rules(INFO_PAIR.as_bytes()).unwrap();
    let r = rules.iter().find(|r| r.instrument.native == BTC).unwrap();
    let e = check_rules("Bitfinex", r, &order(&m, BTC, Side::Buy, OrderType::Limit, "0.123456789", Some("30000"), None), None).unwrap_err();
    assert!(e.message.contains("0.12345678 or 0.12345679"), "{}", e.message);
    assert!(check_rules("Bitfinex", r, &order(&m, BTC, Side::Buy, OrderType::Limit, "0.00003", Some("30000"), None), None).unwrap_err().message.contains("at least 0.00004"));
}

#[test]
fn orders_the_types_cannot_say_are_refused_with_the_fix() {
    let m = spot();
    let mut fok_stop = order(&m, BTC, Side::Buy, OrderType::Stop, "1", None, Some("30000"));
    fok_stop.tif = TimeInForce::Fok;
    assert!(m.check_order(&fok_stop).unwrap_err().message.contains("limit orders there"));
    let mut fok_mkt = order(&m, BTC, Side::Buy, OrderType::Market, "1", None, None);
    fok_mkt.tif = TimeInForce::Fok;
    assert!(m.check_order(&fok_mkt).is_err());
    let mut post_ioc = order(&m, BTC, Side::Buy, OrderType::Limit, "1", Some("30000"), None);
    post_ioc.tif = TimeInForce::Ioc;
    post_ioc.opts.post_only = true;
    assert!(m.check_order(&post_ioc).unwrap_err().message.contains("post-only"));
    let mut sub_second = order(&m, BTC, Side::Buy, OrderType::Limit, "1", Some("30000"), None);
    sub_second.tif = TimeInForce::Gtd(otw_connector_model::Timestamp::from_millis(1_579_085_123_500).unwrap());
    assert!(m.check_order(&sub_second).unwrap_err().message.contains("1579085123 or 1579085124"));
}

/// A minted client id is the milliseconds it was made at, below 2^45; its date is read from it,
/// with the next or previous day when it was made within five minutes of midnight UTC.
#[test]
fn a_client_id_is_minted_with_its_date() {
    let m = spot();
    assert!(m.mints_client_ids());
    assert_eq!(m.mint_client_id(1_678_987_199_446).unwrap(), "1678987199446");
    assert!(m.mint_client_id(INT45_MAX + 1).is_err() && m.mint_client_id(0).is_err());
    assert_eq!(date_of(1_678_987_199_446).unwrap(), "2023-03-16");
    let midnight = 1_678_924_800_000; // 2023-03-16 00:00:00 UTC
    assert_eq!(cid_dates(midnight + 3_600_000).unwrap(), vec!["2023-03-16"]);
    assert_eq!(cid_dates(midnight + 60_000).unwrap(), vec!["2023-03-16", "2023-03-15"]);
    assert_eq!(cid_dates(midnight - 60_000).unwrap(), vec!["2023-03-15", "2023-03-16"]);
}

/// A lost answer is looked up by the client id and its date(s) among the working orders, then
/// among the ended ones created around the mint time (no client id filter there); by the venue
/// id on the symbol, working then ended.
#[test]
fn lookups_name_the_client_id_its_date_and_the_window() {
    let m = spot();
    let s = settings();
    let at = LookupAt { created_ms: Some(STAMP), now_ms: STAMP + 1_000 };
    let reqs = m.query_by_client_id_at(&s, BTC, &STAMP.to_string(), &at, CorrId(1)).unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!((reqs[0].path.as_str(), body(&reqs[0])), ("/v2/auth/r/orders", json!({"cid": STAMP, "cid_date": date_of(STAMP).unwrap()})));
    assert_eq!(
        (reqs[1].path.as_str(), reqs[1].endpoint_id, body(&reqs[1])),
        ("/v2/auth/r/orders/tBTCUSD/hist", "ordersHist", json!({"start": STAMP - 300_000, "end": STAMP + 3_600_000, "limit": 2500}))
    );
    // Near midnight both days are asked.
    let edge = 1_678_924_800_000 - 1_000;
    let near = m.query_by_client_id_at(&s, BTC, &edge.to_string(), &LookupAt { created_ms: Some(edge), now_ms: edge }, CorrId(1)).unwrap();
    assert_eq!(near.iter().filter(|r| r.path == "/v2/auth/r/orders").count(), 2);
    // Without the order's time held, the minted id is its own.
    assert_eq!(m.query_by_client_id(&s, BTC, &STAMP.to_string(), CorrId(1)).unwrap()[0].body, reqs[0].body);
    assert!(m.query_by_client_id(&s, BTC, "otw-key", CorrId(1)).is_err(), "only a minted id is looked up");
    let by_id = m.query_by_venue_id(&s, BTC, "1747566428", CorrId(1)).unwrap();
    assert_eq!((by_id[0].path.as_str(), body(&by_id[0])), ("/v2/auth/r/orders/tBTCUSD", json!({"id": [1747566428_i64]})));
    assert_eq!((by_id[1].path.as_str(), body(&by_id[1])), ("/v2/auth/r/orders/tBTCUSD/hist", json!({"id": [1747566428_i64]})));
    assert_eq!(body(&m.open_orders(&s, None, CorrId(1)).unwrap()[0]), json!({}));
    assert_eq!(m.open_orders(&s, Some(BTC), CorrId(1)).unwrap()[0].path, "/v2/auth/r/orders/tBTCUSD");
}

/// Orders History states no sort and keeps 2500 rows a page: a full page names the next one by
/// the bound the page reached, newest first (`end`) or oldest first (`start`); a page short of
/// full, or a working-orders list, names none.
#[test]
fn a_full_page_of_ended_orders_names_the_next() {
    let m = spot();
    assert_eq!(m.lookup_pages(), 4);
    let at = LookupAt { created_ms: Some(STAMP), now_ms: STAMP + 1_000 };
    let hist = m.query_by_client_id_at(&settings(), BTC, &STAMP.to_string(), &at, CorrId(1)).unwrap().pop().unwrap();
    let row: Value = serde_json::from_str::<Value>(HISTORY).unwrap()[0].clone();
    let page = |first: i64, step: i64| -> String {
        Value::Array((0..2500).map(|i| with(&row.to_string(), |r| r[4] = json!(first + step * i))).map(|s| serde_json::from_str(&s).unwrap()).collect()).to_string()
    };
    let sizes = ContractSizes::new();
    let ctx = |e| DecodeCtx::new(e, &sizes);
    let (rows, next) = m.decode_orders_in(&ctx("ordersHist"), page(STAMP + 10_000, -2).as_bytes()).unwrap();
    assert_eq!((rows.len(), next.as_deref()), (2500, Some(format!("end:{}", STAMP + 10_000 - 2 * 2499).as_str())));
    assert_eq!(body(&m.with_cursor(&hist, next.as_deref().unwrap())), json!({"start": STAMP - 300_000, "end": STAMP + 10_000 - 2 * 2499, "limit": 2500}));
    let (_, next) = m.decode_orders_in(&ctx("ordersHist"), page(STAMP - 10_000, 2).as_bytes()).unwrap();
    assert_eq!(next.as_deref(), Some(format!("start:{}", STAMP - 10_000 + 2 * 2499).as_str()));
    assert_eq!(body(&m.with_cursor(&hist, next.as_deref().unwrap())), json!({"start": STAMP - 10_000 + 2 * 2499, "end": STAMP + 3_600_000, "limit": 2500}));
    assert_eq!(m.decode_orders_in(&ctx("ordersHist"), HISTORY.as_bytes()).unwrap().1, None, "not full");
    assert_eq!(m.decode_orders_in(&ctx("ordersSymbol"), page(STAMP, 1).as_bytes()).unwrap().1, None, "working orders are one list");
}

/// Cancel by id; by client id and its date(s) in one batch call when no id is held; the kill
/// switch names its ids (never `all: 1`, which cancels other books and programs).
#[test]
fn cancels_name_the_order_never_everything() {
    let m = spot();
    let s = settings();
    let c = m.cancel(&s, BTC, "1747566428", CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/v2/auth/w/order/cancel", json!({"id": 1747566428_i64})));
    assert!(m.cancel(&s, BTC, "v-1", CorrId(1)).is_err(), "only a venue id leaves");
    let edge = 1_678_924_800_000 + 1_000;
    let by_cid = m.cancel_order(&s, BTC, "", Some(&ClientRef { id: edge.to_string(), at_ms: Some(edge) }), CorrId(1)).unwrap();
    assert_eq!((by_cid.path.as_str(), body(&by_cid)), ("/v2/auth/w/order/cancel/multi", json!({"cid": [[edge, "2023-03-16"], [edge, "2023-03-15"]]})));
    assert_eq!(m.cancel_order(&s, BTC, "1747566428", Some(&ClientRef { id: edge.to_string(), at_ms: None }), CorrId(1)).unwrap().path, "/v2/auth/w/order/cancel");
    assert_eq!(m.cancel_all_way(), CancelAllWay::ByIds { max: 75 });
    let (rows, _) = m.decode_orders(RETRIEVE.as_bytes()).unwrap();
    let batch = m.cancel_ids(&s, &rows, CorrId(1)).unwrap();
    assert_eq!((batch.path.as_str(), body(&batch)), ("/v2/auth/w/order/cancel/multi", json!({"id": [33950998275_i64]})));
    assert!(!String::from_utf8_lossy(batch.body.as_deref().unwrap()).contains("all"));
    // The doc's answers decode as successes.
    for ok in [CANCEL, CANCEL_MULTI] {
        assert!(m.decode_error(&http_frame(1, "orderCancel", 200, ok)).is_none());
    }
}

/// Update Order, in place, prices only; a derivatives change carries the held leverage, and is
/// refused when none an order can carry is held.
#[test]
fn a_change_moves_prices_in_place() {
    let m = spot();
    let s = settings();
    let (rows, _) = m.decode_orders(HISTORY.replace("CANCELED", "ACTIVE").as_bytes()).unwrap();
    let cur = &rows[0];
    let p = placing("");
    let a = m.amend(&s, cur, &OrderAmend { limit: Some(Price::new(d("16"))), ..Default::default() }, &p).unwrap();
    assert_eq!((a.path.as_str(), body(&a)), ("/v2/auth/w/order/update", json!({"id": 33961681942_i64, "price": "16"})));
    assert!(m.amend(&s, cur, &OrderAmend { qty: Some(Qty::new(d("1"))), ..Default::default() }, &p).unwrap_err().message.contains("not its size"));
    assert!(m.amend(&s, cur, &OrderAmend { stop: Some(Price::new(d("16"))), ..Default::default() }, &p).is_err());
    assert!(m.amend(&s, cur, &OrderAmend { limit: Some(Price::new(d("16.00001"))), ..Default::default() }, &p).is_err(), "6 significant digits");
    let sl = VenueOrder { order_type: "EXCHANGE STOP LIMIT".into(), ..cur.clone() };
    let a = m.amend(&s, &sl, &OrderAmend { stop: Some(Price::new(d("15"))), limit: Some(Price::new(d("14"))), ..Default::default() }, &p).unwrap();
    assert_eq!(body(&a), json!({"id": 33961681942_i64, "price": "15", "price_aux_limit": "14"}));
    let dm = deriv();
    let perp = VenueOrder { symbol: PERP.into(), order_type: "LIMIT".into(), ..cur.clone() };
    let change = OrderAmend { limit: Some(Price::new(d("30000"))), ..Default::default() };
    assert!(dm.amend(&s, &perp, &change, &p).unwrap_err().message.contains("leverage"));
    let held = Placing { leverage: Some(d("3")), ..p.clone() };
    assert_eq!(body(&dm.amend(&s, &perp, &change, &held).unwrap()), json!({"id": 33961681942_i64, "price": "30000", "lev": 3}));
    // The order's own leverage (`META.$F33` of its read) goes first. (A row id no other test uses.)
    let own = with(RETRIEVE, |v| {
        v[0][0] = json!(990_000_000_001_i64);
        v[0][3] = json!(PERP);
        v[0][31] = json!({"$F33": 7});
    });
    let (rows, _) = dm.decode_orders(own.as_bytes()).unwrap();
    assert_eq!(body(&dm.amend(&s, &rows[0], &change, &held).unwrap()), json!({"id": 990_000_000_001_i64, "price": "30000", "lev": 7}));
    assert_eq!(body(&dm.amend(&s, &rows[0], &change, &p).unwrap())["lev"], json!(7));
    // The doc's answer decodes as a success.
    assert!(m.decode_error(&http_frame(1, "orderUpdate", 200, UPDATE)).is_none());
}

// ── Answers ──

#[test]
fn the_submit_answer_names_the_order() {
    let m = spot();
    assert!(m.decode_error(&http_frame(1, "orderSubmit", 200, SUBMIT)).is_none());
    assert_eq!(m.decode_ack(SUBMIT.as_bytes()).unwrap(), Ack { venue_id: "1747566428".into(), client_id: Some("1678987199446".into()) });
    let empty = with(SUBMIT, |v| v[4] = json!([]));
    assert!(!m.decode_ack(empty.as_bytes()).unwrap().has_id(), "no order named: looked up by its client id");
}

/// The glossary's statuses, read on their head word; `RSN_DUST` says nothing of a fill.
#[test]
fn every_status_word_maps_or_stays_unknown() {
    let m = spot();
    let st = |w: &str| m.status_of(w);
    assert_eq!(st("ACTIVE"), Mapped::Known(OrderStatus::New));
    assert_eq!(st("PARTIALLY FILLED @ 107.6(-0.1)"), Mapped::Known(OrderStatus::PartiallyFilled));
    assert_eq!(st("EXECUTED @ 107.6(-0.2)"), Mapped::Known(OrderStatus::Filled));
    assert_eq!(st("EXECUTED @ 8565.0(0.0195) was ACTIVE (note:POSCLOSE)"), Mapped::Known(OrderStatus::Filled));
    assert_eq!(st("FORCED EXECUTED"), Mapped::Known(OrderStatus::Filled));
    for w in [
        "CANCELED",
        "CANCELED was: PARTIALLY FILLED @ 107.6(-0.1)",
        "POSTONLY CANCELED",
        "FILLORKILL CANCELED",
        "IOC CANCELED",
        "INSUFFICIENT BALANCE (G1)",
        "INSUFFICIENT BALANCE (U1)",
        "INSUFFICIENT MARGIN was: PARTIALLY FILLED @ 107.6(-0.1)",
        "RSN_POS_REDUCE_INCR",
        "RSN_POS_REDUCE_FLIP",
        "RSN_POS_NOTFOUND",
        "RSN_BOOK_SLIP was: PARTIALLY FILLED @ 107.6(-0.1)",
        "RSN_PAUSE",
    ] {
        assert_eq!(st(w), Mapped::Known(OrderStatus::Cancelled), "{w}");
    }
    assert!(matches!(st("RSN_DUST was: ACTIVE (note: POSCLOSE)"), Mapped::Unknown(_)));
    assert!(matches!(st("NEW SOMETHING"), Mapped::Unknown(_)));
    assert_eq!(status_head("INSUFFICIENT MARGIN was: PARTIALLY FILLED @ 1(2)"), "INSUFFICIENT MARGIN");
}

/// Order rows: what is left is `AMOUNT`, the size `AMOUNT_ORIG`; an active order with a fill is
/// partly filled; on the history a partly filled order is a cancelled remainder; a type the doc
/// does not list is not claimed; every book's rows come in one list and each book reads its own.
#[test]
fn order_rows_read_size_fill_prices_and_book() {
    let m = spot();
    let (rows, next) = m.decode_orders(RETRIEVE.as_bytes()).unwrap();
    let r = &rows[0];
    assert_eq!(next, None);
    assert_eq!((r.venue_id.as_str(), r.client_id.as_deref(), r.symbol.as_str(), r.side.clone(), r.qty, r.filled_qty), ("33950998275", Some("1573476747887"), "tETHUSD", Mapped::Known(Side::Sell), d("0.5"), d("0")));
    assert_eq!((r.limit, r.stop, r.status.clone(), r.created_ms), (Some(d("220")), None, Mapped::Known(OrderStatus::New), Some(1_573_476_748_000)));
    // History: GID and FLAGS written as strings; a cancelled order.
    let (h, _) = m.decode_orders_in(&DecodeCtx::new("ordersHist", &ContractSizes::new()), HISTORY.as_bytes()).unwrap();
    assert_eq!((h[0].status.clone(), h[0].client_id.as_deref(), h[0].reduce_only), (Mapped::Known(OrderStatus::Cancelled), Some("1337"), false));
    let part = with(HISTORY, |v| {
        v[0][6] = json!(0.0004);
        v[0][13] = json!("PARTIALLY FILLED @ 15.0(0.0006)");
    });
    let (h, _) = m.decode_orders_in(&DecodeCtx::new("ordersHist", &ContractSizes::new()), part.as_bytes()).unwrap();
    assert_eq!((h[0].status.clone(), h[0].filled_qty), (Mapped::Known(OrderStatus::Cancelled), d("0.0006")), "the remainder was cancelled");
    let (w, _) = m.decode_orders_in(&DecodeCtx::new("ordersSymbol", &ContractSizes::new()), part.as_bytes()).unwrap();
    assert_eq!(w[0].status, Mapped::Known(OrderStatus::PartiallyFilled));
    let active_part = with(RETRIEVE, |v| v[0][6] = json!(-0.2));
    assert_eq!(m.decode_orders(active_part.as_bytes()).unwrap().0[0].status, Mapped::Known(OrderStatus::PartiallyFilled));
    // A stop limit's limit is PRICE_AUX_LIMIT; reduce-only is flag 1024.
    let stop_limit = with(RETRIEVE, |v| {
        v[0][8] = json!("STOP LIMIT");
        v[0][16] = json!(230);
        v[0][19] = json!(229);
        v[0][12] = json!(1024);
    });
    let sl = &m.decode_orders(stop_limit.as_bytes()).unwrap().0[0];
    assert_eq!((sl.limit, sl.stop, sl.reduce_only), (Some(d("229")), Some(d("230")), true));
    let odd = with(RETRIEVE, |v| v[0][8] = json!("EXCHANGE SOMETHING NEW"));
    assert!(matches!(m.decode_orders(odd.as_bytes()).unwrap().0[0].status, Mapped::Unknown(_)), "an unknown type is not claimed");
    // One list, two books: each reads its own.
    let mixed = with(RETRIEVE, |v| {
        let mut perp = v[0].clone();
        perp[0] = json!(1);
        perp[3] = json!(PERP);
        v.as_array_mut().unwrap().push(perp);
    });
    assert_eq!(m.decode_orders(mixed.as_bytes()).unwrap().0.iter().map(|o| o.venue_id.clone()).collect::<Vec<_>>(), vec!["33950998275"]);
    assert_eq!(deriv().decode_orders(mixed.as_bytes()).unwrap().0.iter().map(|o| o.venue_id.clone()).collect::<Vec<_>>(), vec!["1"]);
}

/// Trades: the fee is negative when paid (every example), so its cost is its negation; a full
/// page hands back where the next starts.
#[test]
fn fills_read_fee_side_and_the_next_page() {
    let m = spot();
    let req = m.fills_since(&settings(), "tETHUST", 1, 2, CorrId(1)).unwrap();
    assert_eq!((req.path.as_str(), body(&req)), ("/v2/auth/r/trades/tETHUST/hist", json!({"start": 1, "end": 2, "limit": 2500, "sort": 1})));
    let (f, next) = m.decode_fills(TRADES.as_bytes()).unwrap();
    assert_eq!(next, None);
    let x = &f[0];
    assert_eq!((x.exec_id.as_str(), x.venue_order_id.as_str(), x.client_id.as_deref(), x.side.clone()), ("402088407", "34938060782", Some("1234"), Mapped::Known(Side::Sell)));
    assert_eq!((x.qty, x.price, x.fee, x.fee_currency.as_str(), x.at_ms, x.fee_pending), (d("0.2"), d("153.57"), d("0.061668"), "USD", 1_574_963_975_602, false));
    let row: Value = serde_json::from_str::<Value>(TRADES).unwrap()[0].clone();
    let full: Vec<Value> = (0..FILLS_LIMIT).map(|i| with(&row.to_string(), |r| r[0] = json!(i))).map(|s| serde_json::from_str(&s).unwrap()).collect();
    let (_, next) = m.decode_fills(Value::Array(full).to_string().as_bytes()).unwrap();
    assert_eq!(next.as_deref(), Some("1574963975602"));
    let again = m.with_cursor(&req, "1574963975602");
    assert_eq!(body(&again), json!({"start": 1574963975602_i64, "end": 2, "limit": 2500, "sort": 1}));
    assert!(deriv().decode_fills(TRADES.as_bytes()).unwrap().0.is_empty(), "a spot pair's fill is not the derivatives book's");
}

/// `synthetic`: the doc's working order on the perpetual, one row per leverage its `META` states
/// (`$F33`, "Leverage (int)"), ids no other test uses.
fn perp_orders(levs: &[Option<i64>]) -> String {
    let row: Value = serde_json::from_str::<Value>(RETRIEVE).unwrap()[0].clone();
    let list: Vec<Value> = levs
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mut r = row.clone();
            r[0] = json!(980_000_000_000_i64 + i64::try_from(i).unwrap());
            r[3] = json!(PERP);
            r[31] = l.map(|l| json!({"$F33": l})).unwrap_or(Value::Null);
            r
        })
        .collect();
    Value::Array(list).to_string()
}

fn perp_positions() -> String {
    // `synthetic`: the doc's row on two perpetuals, a margin row left as is.
    with(POSITIONS, |v| {
        let mut btc = v[0].clone();
        btc[0] = json!(PERP);
        btc[2] = json!(0.5);
        btc[9] = json!(5);
        btc[15] = json!(1);
        let mut eth = v[0].clone();
        eth[0] = json!("tETHF0:USTF0");
        eth[2] = json!(-2);
        eth[9] = json!(3.0673001895895604);
        eth[15] = json!(1);
        v.as_array_mut().unwrap().extend([btc, eth]);
    })
}

/// Retrieve Positions answers every position of the account: a read for one instrument keeps
/// that one; the mode is one net position per instrument, whatever the rows. A position's
/// `LEVERAGE` (a measured ratio) is not read: the leverage an order carries is the one the
/// working orders on the instrument state (`META.$F33`), none when they state none or differ.
#[test]
fn positions_are_net_per_instrument_and_read_for_the_one_asked() {
    let m = deriv();
    let s = settings();
    assert!(spot().positions(&s, BTC, CorrId(1)).unwrap().is_none(), "spot holds balances");
    assert!(spot().account_position_mode(&s, CorrId(1)).unwrap().is_none());
    let req = m.positions(&s, PERP, CorrId(1)).unwrap().unwrap();
    assert_eq!((req.path.as_str(), req.endpoint_id, body(&req)), ("/v2/auth/r/positions", "positions:tBTCF0:USTF0", json!({})));
    let rows = m.decode_positions_in(&DecodeCtx::new(req.endpoint_id, &ContractSizes::new()), perp_positions().as_bytes()).unwrap();
    assert_eq!(rows, vec![VenuePosition { symbol: PERP.into(), side: PositionSide::Net, direction: Some(Side::Buy), qty: d("0.5"), leverage: None, margin: None }]);
    let all = m.decode_positions(perp_positions().as_bytes()).unwrap();
    assert_eq!(all.len(), 2, "the margin position on tBTCUSD is not this book's");
    assert_eq!((all[1].direction, all[1].qty), (Some(Side::Sell), d("2")));
    assert_eq!(m.decode_account_position_mode(b"[]").unwrap(), Some(PositionMode::OneWay));
    let lev = m.symbol_leverage(&s, PERP, CorrId(1)).unwrap().unwrap();
    assert_eq!((lev.path.as_str(), lev.endpoint_id, body(&lev)), ("/v2/auth/r/orders/tBTCF0:USTF0", "ordersSymbol", json!({})));
    assert!(spot().symbol_leverage(&s, BTC, CorrId(1)).unwrap().is_none());
    assert_eq!(m.decode_symbol_leverage(PERP, PositionSide::Net, perp_orders(&[Some(5), Some(5), None]).as_bytes()).unwrap(), Some(d("5")));
    assert_eq!(m.decode_symbol_leverage(PERP, PositionSide::Net, perp_orders(&[Some(5), Some(10)]).as_bytes()).unwrap(), None, "they differ: none chosen");
    assert_eq!(m.decode_symbol_leverage(PERP, PositionSide::Net, perp_orders(&[None]).as_bytes()).unwrap(), None);
    assert_eq!(m.decode_symbol_leverage(PERP, PositionSide::Net, b"[]").unwrap(), None, "no working order: none");
    assert_eq!(m.positions(&s, PERP, CorrId(2)).unwrap().unwrap().endpoint_id, req.endpoint_id, "one id per instrument");
}

/// `pub:info:pair` and its derivatives twin, measured: each book its own pairs, the paper pairs
/// left out, sizes from MIN/MAX_ORDER_SIZE, an 8-decimal step, no tick (5 significant digits
/// are checked apart).
#[test]
fn rules_come_from_the_pair_info() {
    let (spot_rules, _) = spot().decode_rules(INFO_PAIR.as_bytes()).unwrap();
    assert_eq!(spot_rules.iter().map(|r| r.instrument.native.as_str()).collect::<Vec<_>>(), vec!["tAVAX:USD", "tBTCUSD", "tETHUSD"]);
    let b = spot_rules.iter().find(|r| r.instrument.native == BTC).unwrap();
    assert_eq!((b.instrument.venue.0.as_str(), b.min_qty, b.max_qty, b.lot_step, b.tick), ("BITFINEX", Some(d("0.00004")), Some(d("2000")), Some(d("0.00000001")), None));
    let (perp_rules, _) = deriv().decode_rules(INFO_FUTURES.as_bytes()).unwrap();
    assert_eq!(perp_rules.iter().map(|r| r.instrument.native.as_str()).collect::<Vec<_>>(), vec!["tBTCF0:USTF0", "tETHF0:USTF0"]);
    assert_eq!(perp_rules[0].instrument.venue.0, "BITFINEX_PERP");
    for r in spot_rules.iter().chain(&perp_rules) {
        r.sound().unwrap();
    }
    let req = deriv().instrument_rules(&settings(), Some(PERP), CorrId(1)).unwrap();
    assert_eq!((req.path.as_str(), req.signed), ("/v2/conf/pub:info:pair:futures", false));
}

/// The account socket: the login subscribes it (no subscribe frame), a ping as often as the
/// server's heartbeat; orders, positions and trades of the book read, every other frame quiet;
/// a server restart asks to reconnect.
#[test]
fn the_account_socket_pushes_orders_positions_and_trades() {
    let m = spot();
    let sock: PrivateSocket = m.private_socket(&settings()).unwrap();
    assert_eq!((sock.base, sock.path.as_str(), sock.subscribes().len(), sock.ping_every_ms), ("wsauth", "/ws/2", 0, 15_000));
    assert_eq!(serde_json::from_slice::<Value>(&sock.ping).unwrap(), json!({"event":"ping","cid":1}));
    let ok = br#"{"event":"auth","status":"OK","chanId":0,"userId":269312,"auth_id":"a26236f1-ef44-4671-be32-197ce190348f","caps":"{}"}"#;
    assert_eq!(m.decode_private(ok).unwrap(), vec![PrivateEvent::Answer { op: "auth".into(), ok: true, message: String::new() }]);
    let fail = br#"{"event":"auth","status":"FAIL","chanId":0,"code":10100,"msg":"apikey: invalid"}"#;
    assert!(matches!(&m.decode_private(fail).unwrap()[0], PrivateEvent::Answer { ok: false, .. }));
    assert!(m.decode_private(br#"{"event":"info","version":2,"platform":{"status":1}}"#).unwrap().is_empty());
    assert!(matches!(&m.decode_private(br#"{"event":"info","code":20051,"msg":"Stopping"}"#).unwrap()[0], PrivateEvent::Answer { ok: false, .. }));
    // 20060 announces maintenance (new orders refused until it ends); 20061 ends it, then the
    // stream reconnects.
    assert_eq!(m.decode_private(br#"{"event":"info","code":20060,"msg":"Maintenance"}"#).unwrap(), vec![PrivateEvent::Maintenance { on: true, message: "code 20060 Maintenance".into() }]);
    let over = m.decode_private(br#"{"event":"info","code":20061,"msg":"Maintenance ended"}"#).unwrap();
    assert!(matches!(&over[..], [PrivateEvent::Maintenance { on: false, .. }, PrivateEvent::Answer { ok: false, .. }]), "{over:?}");
    assert!(m.decode_private(br#"{"event":"pong","ts":1511545528111,"cid":1}"#).unwrap().is_empty());
    assert!(m.decode_private(br#"[0,"hb"]"#).unwrap().is_empty());
    assert!(m.decode_private(br#"[0,"wu",["exchange","BTC",1,0,1,null,null]]"#).unwrap().is_empty());
    let ev = m.decode_private(WS_OC.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.venue_id.as_str(), o.client_id.as_deref(), o.status.clone()), ("34930659963", Some("1574955083558"), Mapped::Known(OrderStatus::Cancelled)));
    assert!(deriv().decode_private(WS_OC.as_bytes()).unwrap().is_empty(), "the other book's order");
    let snapshot = format!("[0,\"os\",[{}]]", serde_json::from_str::<Value>(WS_OC).unwrap()[2]);
    assert_eq!(m.decode_private(snapshot.as_bytes()).unwrap().len(), 1);
    // Trades: `te` carries no fee yet, `tu` does.
    let PrivateEvent::Fill(te) = &m.decode_private(WS_TE.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((te.fee_pending, te.client_id.as_deref()), (true, None));
    let PrivateEvent::Fill(tu) = &m.decode_private(WS_TU.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((tu.fee, tu.fee_pending, tu.client_id.as_deref()), (d("0.061668"), false, Some("1714466193700")));
    // Positions: the doc's update is a spot pair (margin): not the derivatives book's.
    assert!(deriv().decode_private(WS_PU.as_bytes()).unwrap().is_empty());
    let pu = with(WS_PU, |v| v[2][0] = json!(PERP));
    let PrivateEvent::Position(p) = &deriv().decode_private(pu.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((p.qty, p.direction, p.leverage), (d("0.2"), Some(Side::Buy), None), "LEVERAGE is a measured ratio");
    // An update that says the position is closed, or holds nothing, is the instrument flat.
    for (status, amount) in [("CLOSED", json!(0.2)), ("ACTIVE", json!(0))] {
        for kind in ["pn", "pu"] {
            let f = with(&pu, |v| {
                v[1] = json!(kind);
                v[2][1] = json!(status);
                v[2][2] = amount.clone();
            });
            let ev = deriv().decode_private(f.as_bytes()).unwrap();
            let [PrivateEvent::Position(z)] = ev.as_slice() else { panic!("{kind} {status}: {ev:?}") };
            assert_eq!((z.symbol.as_str(), z.qty, z.direction), (PERP, Decimal::ZERO, None), "{kind} {status}");
        }
    }
    assert!(deriv().decode_private(with(&pu, |v| v[2][1] = json!("ODD")).as_bytes()).unwrap().is_empty(), "a status the doc does not list");
    let pc = with(&pu, |v| {
        v[1] = json!("pc");
        v[2][1] = json!("CLOSED");
    });
    let PrivateEvent::Position(c) = &deriv().decode_private(pc.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((c.qty, c.direction), (Decimal::ZERO, None));
}

// ── Through the trader (mock venue) ──

struct Env<M: TradeMapper + Clone> {
    server: MockServer,
    driver: otw_connector_sdk::Driver<M>,
    trader: Trader<M>,
    settings: Settings,
    signer: BitfinexSigner,
}

fn notify(kind: &str, data: Value, status: &str, text: &str) -> String {
    json!([CATALOG_NOW_MS, kind, null, null, data, null, status, text]).to_string()
}

impl<M: TradeMapper + Clone> Env<M> {
    async fn new(m: M, rules: &str) -> Self {
        let server = MockServer::start().await;
        let driver = driver_against(m, &server, ManualClock::at(otw_connector_testkit::ts(CATALOG_NOW_MS / 1000)));
        Mock::given(wiremock::matchers::path_regex("^/v2/conf/")).respond_with(ResponseTemplate::new(200).set_body_string(rules.to_string())).mount(&server).await;
        Env { server, driver, trader: Trader::new(), settings: settings(), signer: signer() }
    }

    fn account(&self) -> Account<'_> {
        Account { settings: &self.settings, signer: &self.signer, scope: "bfx" }
    }

    async fn answer(&self, p: &str, body: String) {
        Mock::given(wiremock::matchers::path(p)).respond_with(ResponseTemplate::new(200).set_body_string(body)).mount(&self.server).await;
    }

    async fn answer_n(&self, p: &str, body: String, n: u64) {
        Mock::given(wiremock::matchers::path(p)).respond_with(ResponseTemplate::new(200).set_body_string(body)).up_to_n_times(n).with_priority(1).mount(&self.server).await;
    }

    async fn bodies(&self, p: &str) -> Vec<Value> {
        self.server.received_requests().await.unwrap().iter().filter(|r: &&Request| r.url.path() == p).map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null)).collect()
    }
}

fn submitted(id: i64, sym: &str) -> String {
    let row = with(&serde_json::from_str::<Value>(SUBMIT).unwrap()[4][0].to_string(), |r| {
        r[0] = json!(id);
        r[3] = json!(sym);
    });
    notify("on-req", json!([serde_json::from_str::<Value>(&row).unwrap()]), "SUCCESS", "Submitting")
}

/// The minted client id goes out with the order and, when the answer is lost, the order is
/// found by it and its date among the working orders: adopted, sent once.
#[tokio::test]
async fn a_lost_answer_is_found_by_the_minted_id_and_its_date() {
    let e = Env::new(spot(), INFO_PAIR).await;
    let minted = e.trader.mint(&e.driver, &e.account(), "otwkey1").unwrap().unwrap();
    assert_eq!(minted.at_ms.map(|t| t.to_string()), Some(minted.id.clone()), "the id is its mint time");
    Mock::given(wiremock::matchers::path("/v2/auth/w/order/submit")).respond_with(ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>")).mount(&e.server).await;
    let row = with(RETRIEVE, |v| {
        v[0][2] = json!(minted.id.parse::<i64>().unwrap());
        v[0][3] = json!(BTC);
        v[0][8] = json!("EXCHANGE LIMIT");
    });
    e.answer("/v2/auth/r/orders", row).await;
    let mut o = order(&spot(), BTC, Side::Buy, OrderType::Limit, "0.5", Some("220"), None);
    o.client_order_id = "otwkey1".into();
    let placed = e.trader.place(&e.driver, &e.account(), &[o], &TicketOptions::default()).await.unwrap();
    assert_eq!(placed[0].venue_id, "33950998275");
    let sent = e.bodies("/v2/auth/w/order/submit").await;
    assert_eq!((sent.len(), sent[0]["cid"].to_string()), (1, minted.id.clone()));
    let looked = e.bodies("/v2/auth/r/orders").await;
    assert_eq!(looked[0], json!({"cid": minted.id.parse::<i64>().unwrap(), "cid_date": date_of(minted.at_ms.unwrap()).unwrap()}));
    // A stream order maps back to OTW's key through the minted id; one placed elsewhere does not.
    assert_eq!(e.trader.otw_key(&e.driver, &e.account(), &minted.id).as_deref(), Some("otwkey1"));
    assert_eq!(e.trader.otw_key(&e.driver, &e.account(), "1573476747887"), None);
}

/// Derivatives through the trader: one net position per instrument whatever the rows (no
/// position open: still one-way); an order goes with the leverage the working orders on the
/// instrument state, none when they state none; a close is a reduce-only market order of the
/// held size on the instrument asked, with that leverage too.
#[tokio::test]
async fn derivatives_open_and_close_with_the_held_leverage() {
    let e = Env::new(deriv(), INFO_FUTURES).await;
    e.answer("/v2/auth/r/positions", perp_positions()).await;
    e.answer("/v2/auth/r/orders/tBTCF0:USTF0", perp_orders(&[Some(5)])).await;
    e.answer("/v2/auth/r/orders/tETHF0:USTF0", "[]".into()).await;
    e.answer("/v2/auth/w/order/submit", submitted(77, PERP)).await;
    let mut buy = order(&deriv(), PERP, Side::Buy, OrderType::Market, "0.1", None, None);
    buy.client_order_id = "open1".into();
    e.trader.place(&e.driver, &e.account(), &[buy], &TicketOptions::default()).await.unwrap();
    let p = e.trader.close_position(&e.driver, &e.account(), PERP, None, None, "close1").await.unwrap();
    assert_eq!(p.venue_id, "77");
    let mut eth = order(&deriv(), "tETHF0:USTF0", Side::Buy, OrderType::Market, "1", None, None);
    eth.client_order_id = "eth1".into();
    e.trader.place(&e.driver, &e.account(), &[eth], &TicketOptions::default()).await.unwrap();
    let sent = e.bodies("/v2/auth/w/order/submit").await;
    assert_eq!((sent[0]["type"].as_str(), sent[0]["amount"].as_str(), sent[0]["lev"].as_i64()), (Some("MARKET"), Some("0.1"), Some(5)));
    assert_eq!((sent[1]["amount"].as_str(), sent[1]["flags"].as_i64(), sent[1]["lev"].as_i64(), sent[1]["symbol"].as_str()), (Some("-0.5"), Some(1024), Some(5), Some(PERP)));
    assert!(sent[2].get("lev").is_none(), "no working order on it states one (its position's ratio is not one)");
    // Flat: none sent.
    let flat = Env::new(deriv(), INFO_FUTURES).await;
    flat.answer("/v2/auth/r/positions", "[]".into()).await;
    flat.answer("/v2/auth/r/orders/tBTCF0:USTF0", "[]".into()).await;
    flat.answer("/v2/auth/w/order/submit", submitted(78, PERP)).await;
    let mut o = order(&deriv(), PERP, Side::Sell, OrderType::Limit, "0.1", Some("90000"), None);
    o.client_order_id = "flat1".into();
    flat.trader.place(&flat.driver, &flat.account(), &[o], &TicketOptions::default()).await.unwrap();
    assert!(flat.bodies("/v2/auth/w/order/submit").await[0].get("lev").is_none());
    let out = flat.trader.close_position(&flat.driver, &flat.account(), PERP, None, None, "close2").await;
    assert!(matches!(out, Err(TradeError::Refused(_))), "nothing held: {out:?}");
    // The leverage the chart shows is the venue's, read only.
    let lev = e.trader.leverage(&e.driver, &e.account(), PERP).await.unwrap();
    assert_eq!(lev.and_then(|(l, _)| l), Some(d("5")));
}

/// A position the stream says is closed, or a positions read that no longer lists it, leaves no
/// leverage held: the next order reads what the working orders state again.
#[tokio::test]
async fn a_closed_position_leaves_no_leverage_held() {
    let e = Env::new(deriv(), INFO_FUTURES).await;
    e.answer_n("/v2/auth/r/orders/tBTCF0:USTF0", perp_orders(&[Some(5)]), 2).await;
    e.answer("/v2/auth/r/orders/tBTCF0:USTF0", "[]".into()).await;
    e.answer("/v2/auth/r/positions", "[]".into()).await;
    e.answer("/v2/auth/w/order/submit", submitted(79, PERP)).await;
    let place = |k: &'static str| {
        let mut o = order(&deriv(), PERP, Side::Buy, OrderType::Limit, "0.1", Some("90000"), None);
        o.client_order_id = k.into();
        o
    };
    e.trader.place(&e.driver, &e.account(), &[place("a1")], &TicketOptions::default()).await.unwrap();
    e.trader.place(&e.driver, &e.account(), &[place("a2")], &TicketOptions::default()).await.unwrap();
    let pc = with(&with(WS_PU, |v| v[2][0] = json!(PERP)), |v| {
        v[1] = json!("pc");
        v[2][1] = json!("CLOSED");
    });
    let ev = deriv().decode_private(pc.as_bytes()).unwrap();
    let PrivateEvent::Position(flat) = &ev[0] else { panic!("{ev:?}") };
    e.trader.set_position(&e.driver, &e.account(), flat);
    e.trader.place(&e.driver, &e.account(), &[place("a3")], &TicketOptions::default()).await.unwrap();
    e.trader.positions(&e.driver, &e.account(), PERP).await.unwrap();
    e.trader.place(&e.driver, &e.account(), &[place("a4")], &TicketOptions::default()).await.unwrap();
    let levs: Vec<Option<i64>> = e.bodies("/v2/auth/w/order/submit").await.iter().map(|b| b["lev"].as_i64()).collect();
    assert_eq!(levs, vec![Some(5), Some(5), Some(5), None]);
    assert_eq!(e.bodies("/v2/auth/r/orders/tBTCF0:USTF0").await.len(), 3, "read once, again after the close, again after the read");
}

/// The kill switch: every working order of the book, by id, in batches; the other book's orders
/// in the same answer are left to it.
#[tokio::test]
async fn the_kill_switch_cancels_the_books_orders_by_id() {
    let e = Env::new(deriv(), INFO_FUTURES).await;
    let list = with(RETRIEVE, |v| {
        let mut a = v[0].clone();
        a[0] = json!(11);
        a[3] = json!(PERP);
        let mut b = a.clone();
        b[0] = json!(12);
        v.as_array_mut().unwrap().extend([a, b]);
    });
    e.answer_n("/v2/auth/r/orders", list, 1).await;
    e.answer("/v2/auth/r/orders", "[]".into()).await;
    e.answer("/v2/auth/w/order/cancel/multi", notify("oc_multi-req", json!([]), "SUCCESS", "Submitting 2 order cancellations.")).await;
    let n = e.trader.cancel_all(&e.driver, &e.account()).await.unwrap();
    assert_eq!(n, 2);
    assert_eq!(e.bodies("/v2/auth/w/order/cancel/multi").await, vec![json!({"id": [11, 12]})]);
}

/// A cancel the venue refuses (`ERROR` notification) is reconciled: the order read by its id is
/// already final, so no error.
#[tokio::test]
async fn a_refused_cancel_of_a_filled_order_is_no_error() {
    let e = Env::new(spot(), INFO_PAIR).await;
    e.answer("/v2/auth/w/order/cancel", notify("oc-req", Value::Null, "ERROR", "Order not found.")).await;
    e.answer("/v2/auth/r/orders/tBTCUSD", "[]".into()).await;
    let filled = with(HISTORY, |v| {
        v[0][6] = json!(0);
        v[0][13] = json!("EXECUTED @ 15.0(0.001)");
    });
    e.answer("/v2/auth/r/orders/tBTCUSD/hist", filled).await;
    let out = e.trader.cancel(&e.driver, &e.account(), BTC, "33961681942").await;
    assert_eq!(out, Ok(otw_connector_engine::trading::Cancelled::AlreadyFinal(OrderStatus::Filled)));
}

// ── The catalog (spot book) ──

/// Catalog keys (`t01k`) and venue ids (`v-t01`) as the integers Bitfinex takes, both ways.
fn ids() -> &'static Mutex<Vec<String>> {
    static IDS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    IDS.get_or_init(Default::default)
}

const ID_BASE: i64 = 1_700_000_000_000;

fn num(name: &str) -> i64 {
    if let Ok(n) = name.parse::<i64>() {
        return n;
    }
    let mut t = ids().lock().unwrap();
    let i = t.iter().position(|n| n == name).unwrap_or_else(|| {
        t.push(name.to_string());
        t.len() - 1
    });
    ID_BASE + i64::try_from(i).unwrap()
}

fn name(n: &str) -> String {
    let t = ids().lock().unwrap();
    n.parse::<i64>().ok().and_then(|v| usize::try_from(v - ID_BASE).ok()).and_then(|i| t.get(i).cloned()).unwrap_or_else(|| n.to_string())
}

/// The real spot mapper, with the catalog's string keys and venue ids spelled as the integers
/// Bitfinex takes (and read back), the key carried in the body so the catalog's mock venue can
/// tell orders apart (Bitfinex's own body carries only the integer), and the kill switch walked
/// one order at a time (the batch cancel by ids is tested on its own above). Everything else is
/// [`Bitfinex`]'s own.
#[derive(Clone)]
struct Catalog(Bitfinex);

impl Catalog {
    fn back(&self, mut o: VenueOrder) -> VenueOrder {
        o.venue_id = name(&o.venue_id);
        o
    }
}

impl Mapper for Catalog {
    type Session = <Bitfinex as Mapper>::Session;

    fn capabilities(&self) -> &Capabilities {
        self.0.capabilities()
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.0.base_urls()
    }

    fn encode(&self, cmd: &Command, s: &mut Self::Session, ctx: &MapCtx<'_>) -> Result<Vec<otw_connector_sdk::NativeRequest>, ConnectorError> {
        self.0.encode(cmd, s, ctx)
    }

    fn decode(&self, frame: &RawFrame, s: &mut Self::Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        self.0.decode(frame, s, ctx)
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        self.0.instrument_id(symbol)
    }
}

impl TradeMapper for Catalog {
    fn venue_name(&self) -> &str {
        self.0.venue_name()
    }
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(36)
    }
    fn client_id(&self, otw_id: &str) -> Result<String, ConnectorError> {
        Ok(num(otw_id).to_string())
    }
    fn otw_id(&self, venue_client_id: &str) -> String {
        name(venue_client_id)
    }
    fn place(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let cid = if p.client_id.is_empty() { String::new() } else { num(&p.client_id).to_string() };
        let mut req = self.0.place(s, o, &Placing { client_id: cid.clone(), ..p.clone() })?;
        let mut b = body(&req);
        b["meta"] = json!({ "catalog_key": name(&cid) });
        req.body = Some(b.to_string().into());
        Ok(req)
    }
    fn amend(&self, s: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        self.0.amend(s, &VenueOrder { venue_id: num(&cur.venue_id).to_string(), ..cur.clone() }, change, p)
    }
    fn cancel(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.0.cancel(s, symbol, &num(id).to_string(), corr)
    }
    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::BySymbol
    }
    /// Bitfinex names the client id in the body; the catalog's mock venue tells lookups apart by
    /// their query, so the id is also written there (test only).
    fn query_by_client_id(&self, s: &Settings, symbol: &str, cid: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let n = num(cid).to_string();
        Ok(self.0.query_by_client_id(s, symbol, &n, corr)?.into_iter().map(|r| r.query("catalog_cid", &n)).collect())
    }
    fn query_by_client_id_at(&self, s: &Settings, symbol: &str, cid: &str, _at: &LookupAt, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.query_by_client_id(s, symbol, cid, corr)
    }
    fn query_by_venue_id(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.query_by_venue_id(s, symbol, &num(id).to_string(), corr)
    }
    fn open_orders(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.open_orders(s, symbol, corr)
    }
    fn fills_since(&self, s: &Settings, symbol: &str, from: i64, to: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn instrument_rules(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        self.0.check_order(o)
    }
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        self.0.with_cursor(req, cursor)
    }
    fn positions(&self, s: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.0.positions(s, symbol, corr)
    }
    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        self.0.decode_error(frame)
    }
    fn decode_ack(&self, b: &[u8]) -> Result<Ack, ConnectorError> {
        let a = self.0.decode_ack(b)?;
        Ok(Ack { venue_id: if a.has_id() { name(&a.venue_id) } else { a.venue_id }, client_id: a.client_id })
    }
    fn decode_orders(&self, b: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let (rows, next) = self.0.decode_orders(b)?;
        Ok((rows.into_iter().map(|o| self.back(o)).collect(), next))
    }
    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, b: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let (rows, next) = self.0.decode_orders_in(ctx, b)?;
        Ok((rows.into_iter().map(|o| self.back(o)).collect(), next))
    }
    fn decode_fills(&self, b: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.0.decode_fills(b)
    }
    fn decode_rules(&self, b: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        self.0.decode_rules(b)
    }
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        self.0.status_of(word)
    }
    fn error_table(&self) -> &'static [ErrorRow] {
        self.0.error_table()
    }
}

/// The catalog's answers: the documented examples with their ids, sizes and states changed
/// (`synthetic`, built on the documented shape).
struct Script;

fn order_row(cid: Option<&str>, vid: &str, word: &str, side: Side, qty: Decimal, filled: Decimal) -> Value {
    let mut r = serde_json::from_str::<Value>(RETRIEVE).unwrap()[0].clone();
    let sign = |v: Decimal| if side == Side::Buy { v } else { -v };
    r[0] = json!(num(vid));
    r[2] = cid.map(|c| json!(num(c))).unwrap_or(Value::Null);
    r[3] = json!(BTC);
    r[6] = json!(sign(qty - filled).normalize().to_string().parse::<f64>().unwrap());
    r[7] = json!(sign(qty).normalize().to_string().parse::<f64>().unwrap());
    r[8] = json!("EXCHANGE LIMIT");
    r[13] = json!(word);
    r[16] = json!(30000);
    r
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
        Arc::new(signer())
    }

    fn symbol(&self) -> &'static str {
        BTC
    }

    fn qty(&self) -> Decimal {
        d("0.01")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    /// The list states no status: an instrument that does not trade is not listed.
    fn rules(&self, closed: Option<&str>) -> String {
        match closed {
            None => INFO_PAIR.to_string(),
            Some(_) => with(INFO_PAIR, |v| v[0].as_array_mut().unwrap().retain(|p| p[0] != "BTCUSD")),
        }
    }

    fn closed_word(&self) -> &'static str {
        "delisted"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::Filled => "EXECUTED @ 30000.0(0.01)",
            OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired => "CANCELED",
            // An active order with a fill reads as partly filled.
            _ => "ACTIVE",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        let row = order_row(Some(client_id), venue_id, "ACTIVE", Side::Buy, d("0.01"), Decimal::ZERO);
        notify("on-req", json!([row]), "SUCCESS", "Submitting exchange limit buy order for 0.01 BTC.")
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        Value::Array(rows.iter().map(|r| order_row(r.client_id, r.venue_id, r.word, r.side, r.qty, r.filled)).collect()).to_string()
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let template = serde_json::from_str::<Value>(TRADES).unwrap()[0].clone();
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut f = template.clone();
                f[0] = json!(num(r.exec_id));
                f[1] = json!(BTC);
                f[2] = json!(CATALOG_NOW_MS - 60_000);
                f[3] = json!(num(r.venue_id));
                f[4] = json!(r.qty.normalize().to_string().parse::<f64>().unwrap());
                f[5] = json!(r.price.normalize().to_string().parse::<f64>().unwrap());
                f[9] = json!((-r.fee).normalize().to_string().parse::<f64>().unwrap());
                f[10] = json!("USD");
                f[11] = json!(num(r.client_id));
                f
            })
            .collect();
        Value::Array(list).to_string()
    }

    fn fee_currency(&self) -> &'static str {
        "USD"
    }

    fn positions(&self, _rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        "[]".into()
    }

    fn ok(&self) -> String {
        CANCEL.to_string()
    }

    fn dry_run_ok(&self) -> String {
        "[]".into()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        match r {
            VenueRefusal::Funds | VenueRefusal::ReduceOnly | VenueRefusal::PositionModeMismatch => {
                Answer::new(200, notify("on-req", json!([]), "ERROR", "Invalid order: not enough exchange balance for 0.01 BTCUSD at 30000"))
            }
            VenueRefusal::NotFound | VenueRefusal::Final => Answer::new(200, notify("oc-req", Value::Null, "ERROR", "Order not found.")),
            VenueRefusal::RateLimited(_) => Answer::new(429, r#"{"error":"ERR_RATE_LIMIT"}"#),
            VenueRefusal::ClockSkew => Answer::new(500, r#"["error",10114,"nonce: small"]"#),
            VenueRefusal::Unmapped => Answer::new(500, r#"["error",99999,"something new"]"#),
            VenueRefusal::Gateway => Answer::new(502, "<html><body>502 Bad Gateway</body></html>"),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_runs_on_the_spot_book() {
    let r = run_catalog(&Script).await;
    for id in ["T01", "T02", "T03+T24", "T04", "T05", "T09", "T10", "T11", "T12", "T15", "T19", "T20", "T21", "T22", "T26", "T30", "T31+T34", "T32"] {
        assert!(r.ran.contains(&id), "{id} did not run: {:?}", r.ran);
    }
    println!("ran {:?}", r.ran);
}

#[test]
fn the_error_table_is_the_trade_errors() {
    assert_eq!(spot().error_table().len(), super::trade_errors::ERRORS.len());
    let e = spot().decode_error(&http_frame(1, "orderSubmit", 500, r#"["error",10020,"price: invalid"]"#)).unwrap();
    assert_eq!(e.canon, Canon::InvalidRequest);
    let refused = spot().decode_error(&http_frame(1, "orderSubmit", 200, &notify("on-req", json!([]), "ERROR", "x"))).unwrap();
    assert_eq!(refused.canon, Canon::Rejected(Refusal::Other));
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live pair lists decode to sound rules for every pair of each book, and an order call
/// signed with a key Bitfinex never issued is answered with the auth error the table expects
/// (no account holds that key, so nothing is placed). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers bitfinex::trade_tests::public`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    for (m, sym, qty, px) in [(spot(), BTC, "0.0001", "1000"), (deriv(), PERP, "0.0001", "1000")] {
        let label = m.venue_name().to_string();
        let dv = Driver::new(m.clone(), DriverConfig::new("bitfinex"), reqwest::Client::new());
        let junk = settings();
        let sg = signer();
        let a = Account { settings: &junk, signer: &sg, scope: "net" };
        let trader: Trader<Bitfinex> = Trader::new();
        let rules = trader.all_rules(&dv, &a).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(rules.len() > 5, "{label}: {} instruments", rules.len());
        for r in &rules {
            r.sound().unwrap_or_else(|e| panic!("{label}: {e}"));
            assert!(r.min_qty.is_some() && r.max_qty.is_some(), "{label}: {} states no size", r.instrument.native);
        }
        println!("{label}: {} instruments", rules.len());
        let btc = rules.iter().find(|r| r.instrument.native == sym).unwrap_or_else(|| panic!("{label}: no {sym}"));
        println!("{label}: {btc:?}");
        let o = order(&m, sym, Side::Buy, OrderType::Limit, qty, Some(px), None);
        let req = m.place(&junk, &o, &placing(&STAMP.to_string())).unwrap();
        let frame = dv.send_as(req, Some(&sg)).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        if let RawFrame::Http { status, body, .. } = &frame {
            println!("{label} junk key: HTTP {status} {}", String::from_utf8_lossy(body));
        }
        let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label}: Bitfinex accepted an order with a made-up key"));
        println!("{label} junk key: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
        assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{label}: {e:?}");
    }
}
