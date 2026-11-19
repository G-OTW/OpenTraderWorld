//! Deribit orders, from the documentation alone. Fixtures under `fixtures/trade/`, copied
//! verbatim from the response examples of the OpenAPI file every method page embeds
//! (https://docs.deribit.com/specifications/deribit_openapi.json, version 2.1.1, read
//! 2026-11-12, sha256 `d06d82d9a42dd9f2...`); each page is
//! `https://docs.deribit.com/api-reference/<group>/<method>`:
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_buy.json` | trading/private-buy (Response example: a filled market buy, ETH-PERPETUAL) |
//! | `doc_sell.json` | trading/private-sell (Response example: an untriggered stop limit) |
//! | `doc_edit.json` | trading/private-edit (Response example) |
//! | `doc_cancel.json` | trading/private-cancel (Response example: an untriggered stop market) |
//! | `doc_cancel_all_by_instrument.json` | trading/private-cancel_all_by_instrument (Response example) |
//! | `doc_order_state.json` | trading/private-get_order_state (Response example) |
//! | `doc_order_state_by_label.json` | trading/private-get_order_state_by_label (Response example) |
//! | `doc_order_history_by_instrument.json` | trading/private-get_order_history_by_instrument (Response example) |
//! | `doc_open_orders_by_instrument.json` | trading/private-get_open_orders_by_instrument (Response example) |
//! | `doc_trigger_order_history.json` | trading/private-get_trigger_order_history (Response example: `trigger:order`) |
//! | `doc_user_trades_by_instrument_and_time.json` | trading/private-get_user_trades_by_instrument_and_time (Response example) |
//! | `doc_position.json` | account-management/private-get_position (Response example) |
//! | `doc_positions.json` | account-management/private-get_positions (Response example) |
//! | `doc_instrument.json` | market-data/public-get_instrument (Response example: an option) |
//! | `doc_instruments.json` | market-data/public-get_instruments (Response example: BTC-29SEP23, BTC-PERPETUAL) |
//! | `doc_ticker.json` | market-data/public-ticker (Response example) |
//! | `doc_set_heartbeat.json`, `doc_test.json`, `doc_subscribe.json` | session-management/public-set_heartbeat, supporting/public-test, subscription-management/private-subscribe (Response examples) |
//! | `synthetic_user_changes.json` | **synthetic**: the `user.changes.(kind).(currency).(interval)` page gives the payload schema only (no example). Built from that schema in the documented notification envelope (JSON-RPC overview), its rows copied from the trigger history, order and trade examples above: a stop that fired, the order it became, its fill, the flat position |
//!
//! The linear contract the catalog runs on (`BTC_USDC-PERPETUAL`) has no documented example: its
//! row is the one recorded from the live public list on 2026-10-19 (`tests.rs`). The catalog
//! script ([`Script`]) answers with the documented row shapes, ids, sizes, prices and states
//! changed: those answers are synthetic. Every error body is synthetic too (the doc lists codes
//! and the envelope, not bodies per call), as are the reduce-only refusal (the doc names no
//! reduce-only code: `11030 other_reject` is used) and the `Retry-After` header of the rate limit
//! answer (Deribit documents no wait).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, TradeError, Trader};
use otw_connector_model::{CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, SizeUnit, TimeInForce, TriggerKind};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, Canon, ClientIdRule, DecodeCtx, ErrorRow, Page, Placing, PositionSide, PrivateEvent, PrivateSocket, Refusal, Settings, TradeMapper, TradingStatus,
    VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript, CATALOG_NOW_MS};
use otw_connector_testkit::{driver_against, http_frame, ManualClock};
use serde_json::{json, Value};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::super::{Deribit, Session};
use super::{CHANNEL, ENV};
use crate::deribit::DeribitSigner;

const BUY: &str = include_str!("fixtures/trade/doc_buy.json");
const SELL: &str = include_str!("fixtures/trade/doc_sell.json");
const EDIT: &str = include_str!("fixtures/trade/doc_edit.json");
const CANCEL: &str = include_str!("fixtures/trade/doc_cancel.json");
const CANCEL_ALL: &str = include_str!("fixtures/trade/doc_cancel_all_by_instrument.json");
const ORDER_STATE: &str = include_str!("fixtures/trade/doc_order_state.json");
const BY_LABEL: &str = include_str!("fixtures/trade/doc_order_state_by_label.json");
const HISTORY: &str = include_str!("fixtures/trade/doc_order_history_by_instrument.json");
const OPEN: &str = include_str!("fixtures/trade/doc_open_orders_by_instrument.json");
const TRIGGERS: &str = include_str!("fixtures/trade/doc_trigger_order_history.json");
const TRADES: &str = include_str!("fixtures/trade/doc_user_trades_by_instrument_and_time.json");
const POSITION: &str = include_str!("fixtures/trade/doc_position.json");
const POSITIONS: &str = include_str!("fixtures/trade/doc_positions.json");
const INSTRUMENT: &str = include_str!("fixtures/trade/doc_instrument.json");
const INSTRUMENTS: &str = include_str!("fixtures/trade/doc_instruments.json");
const TICKER: &str = include_str!("fixtures/trade/doc_ticker.json");
const HEARTBEAT: &str = include_str!("fixtures/trade/doc_set_heartbeat.json");
const TEST: &str = include_str!("fixtures/trade/doc_test.json");
const SUBSCRIBE: &str = include_str!("fixtures/trade/doc_subscribe.json");
const CHANGES: &str = include_str!("fixtures/trade/synthetic_user_changes.json");
/// Recorded from the live public list, 2026-10-19 (`tests.rs`, `the_list_reads_every_kind_and_leaves_combos_out`).
const LINEAR_ROW: &str = r#"{"instrument_name":"BTC_USDC-PERPETUAL","kind":"future","instrument_type":"linear","base_currency":"BTC","quote_currency":"USDC","settlement_currency":"USDC","settlement_period":"perpetual","tick_size":0.1,"min_trade_amount":0.0001,"contract_size":0.0001,"expiration_timestamp":32503708800000,"is_active":true,"state":"open","creation_timestamp":1646824342000}"#;
const LINEAR: &str = "BTC_USDC-PERPETUAL";
const INVERSE: &str = "BTC-PERPETUAL";

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "AMANDA"), ("api_secret", "AMANDASECRECT")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn test_env() -> HashMap<String, String> {
    let mut s = settings();
    s.insert(ENV.into(), "test".into());
    s
}

fn signer(s: &HashMap<String, String>) -> DeribitSigner {
    DeribitSigner::from_secrets(s).unwrap()
}

fn order(symbol: &str, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>, stop: Option<&str>) -> OrderRequest {
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("DERIBIT", symbol),
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
    Placing::new(cid, PositionSide::Net, CorrId(7))
}

fn body(req: &HttpRequest) -> Value {
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

fn q(req: &HttpRequest, k: &str) -> Option<String> {
    req.query.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone())
}

fn ctx(endpoint: &str) -> DecodeCtx<'_> {
    static SIZES: std::sync::OnceLock<otw_connector_sdk::trade::ContractSizes> = std::sync::OnceLock::new();
    DecodeCtx::new(endpoint, SIZES.get_or_init(Default::default))
}

// ── Requests ──

#[test]
fn a_linear_limit_is_a_signed_json_rpc_post_with_one_content_type() {
    let m = Deribit::new().unwrap();
    let mut o = order(LINEAR, "otwk1", Side::Buy, OrderType::Limit, "0.0123", Some("80000.5"), None);
    o.opts.reduce_only = true;
    let r = m.place(&settings(), &o, &placing("otwk1")).unwrap();
    assert_eq!((r.method, r.base, r.path.as_str(), r.signed, r.endpoint_id), (Method::Post, "rest", "/api/v2/private/buy", true, "me:buy"));
    assert_eq!(r.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1);
    assert_eq!(
        body(&r),
        json!({"jsonrpc":"2.0","id":"otwk1","method":"private/buy","params":{"instrument_name":LINEAR,"amount":0.0123,"type":"limit","label":"otwk1","price":80000.5,"time_in_force":"good_til_cancelled","reduce_only":true}})
    );
    // Numbers leave as JSON numbers, written from the exact decimal.
    let raw = String::from_utf8(r.body.clone().unwrap().to_vec()).unwrap();
    assert!(raw.contains(r#""amount":0.0123"#) && raw.contains(r#""price":80000.5"#), "{raw}");
    // A sell is the other method.
    let s = m.place(&settings(), &OrderRequest { side: Side::Sell, ..o }, &placing("otwk1")).unwrap();
    assert_eq!((s.path.as_str(), body(&s)["method"].as_str()), ("/api/v2/private/sell", Some("private/sell")));
    // Signed over the body (the signer's prehash covers it), with Deribit's header.
    let mut signable = otw_connector_sdk::SignableRequest { method: r.method, path: r.path.clone(), query: r.query.clone(), headers: r.headers.clone(), body: r.body.clone() };
    signer(&settings()).sign_with(&mut signable, otw_connector_model::Timestamp::from_millis(1_576_074_319_000).unwrap(), "1iqt2wls").unwrap();
    assert!(signable.headers.iter().any(|(k, v)| k == "Authorization" && v.starts_with("deri-hmac-sha256 id=AMANDA,ts=1576074319000,sig=")));
}

/// private/buy's JSON-RPC request example: `{"instrument_name":"ETH-PERPETUAL","amount":40,
/// "type":"market","label":"market0000234"}`, an inverse market buy of 40 USD.
#[test]
fn the_documented_market_buy_is_the_body_sent_and_an_inverse_order_goes_out_in_usd_only() {
    let m = Deribit::new().unwrap();
    let o = order("ETH-PERPETUAL", "market0000234", Side::Buy, OrderType::Market, "0.02", None, None);
    let p = Placing { quote_amount: Some(Decimal::from(40)), ..placing("market0000234") };
    let r = m.place(&settings(), &o, &p).unwrap();
    assert_eq!(body(&r)["params"], json!({"instrument_name":"ETH-PERPETUAL","amount":40,"type":"market","label":"market0000234"}));
    // Never the coin size: an inverse order the trader did not size in USD goes nowhere.
    let e = m.place(&settings(), &o, &placing("market0000234")).unwrap_err();
    assert!(e.message.contains("sizes ETH-PERPETUAL in USD"), "{e:?}");
    // A size in the quote currency on a linear book is refused.
    let mut usd = order(LINEAR, "k", Side::Buy, OrderType::Limit, "100", Some("80000"), None);
    usd.opts.size_unit = SizeUnit::Quote;
    assert!(m.place(&settings(), &usd, &placing("k")).is_err());
}

#[test]
fn stops_and_if_touched_orders_carry_the_trigger_the_user_picked_and_there_is_no_default() {
    let m = Deribit::new().unwrap();
    for (t, word, limit) in [(OrderType::Stop, "stop_market", None), (OrderType::StopLimit, "stop_limit", Some("79000")), (OrderType::Mit, "take_market", None), (OrderType::Lit, "take_limit", Some("81000"))] {
        let mut o = order(LINEAR, "k", Side::Sell, t, "0.01", limit, Some("80000"));
        o.opts.reduce_only = true;
        assert!(m.check_order(&o).unwrap_err().message.contains("no default"), "{t:?}");
        for (kind, w) in [(TriggerKind::Last, "last_price"), (TriggerKind::Mark, "mark_price"), (TriggerKind::Index, "index_price")] {
            o.opts.trigger = Some(kind);
            m.check_order(&o).unwrap();
            let b = body(&m.place(&settings(), &o, &placing("k")).unwrap());
            assert_eq!((b["params"]["type"].as_str(), b["params"]["trigger"].as_str(), b["params"]["trigger_price"].as_f64()), (Some(word), Some(w), Some(80000.0)), "{t:?}");
            assert_eq!(b["params"]["price"].as_str().is_none() && b["params"].get("price").is_some(), limit.is_some());
        }
        o.opts.trigger = Some(TriggerKind::BidAsk);
        assert!(m.check_order(&o).is_err());
    }
    let mut limit = order(LINEAR, "k", Side::Buy, OrderType::Limit, "0.01", Some("80000"), None);
    limit.opts.trigger = Some(TriggerKind::Mark);
    assert!(m.check_order(&limit).is_err(), "a trigger kind on a plain limit says nothing");
}

#[test]
fn time_in_force_post_only_and_market_orders_go_out_as_documented() {
    let m = Deribit::new().unwrap();
    let base = order(LINEAR, "k", Side::Buy, OrderType::Limit, "0.01", Some("80000"), None);
    for (tif, w) in [(TimeInForce::Gtc, "good_til_cancelled"), (TimeInForce::Day, "good_til_day"), (TimeInForce::Ioc, "immediate_or_cancel"), (TimeInForce::Fok, "fill_or_kill")] {
        let o = OrderRequest { tif, ..base.clone() };
        assert_eq!(body(&m.place(&settings(), &o, &placing("k")).unwrap())["params"]["time_in_force"].as_str(), Some(w));
    }
    // Post-only always with reject_post_only: alone, the venue moves the price.
    let mut post = base.clone();
    post.opts.post_only = true;
    m.check_order(&post).unwrap();
    let b = body(&m.place(&settings(), &post, &placing("k")).unwrap());
    assert_eq!((b["params"]["post_only"].as_bool(), b["params"]["reject_post_only"].as_bool()), (Some(true), Some(true)));
    for o in [OrderRequest { tif: TimeInForce::Ioc, ..post.clone() }, OrderRequest { order_type: OrderType::Market, limit: None, ..post.clone() }] {
        assert!(m.check_order(&o).is_err(), "post-only only on a GTC or day limit");
    }
    // A market order: no time in force unless IOC or FOK is asked; a day market is refused.
    let mkt = order(LINEAR, "k", Side::Buy, OrderType::Market, "0.01", None, None);
    assert!(body(&m.place(&settings(), &mkt, &placing("k")).unwrap())["params"].get("time_in_force").is_none());
    let ioc = OrderRequest { tif: TimeInForce::Ioc, ..mkt.clone() };
    assert_eq!(body(&m.place(&settings(), &ioc, &placing("k")).unwrap())["params"]["time_in_force"].as_str(), Some("immediate_or_cancel"));
    assert!(m.check_order(&OrderRequest { tif: TimeInForce::Day, ..mkt }).is_err());
}

#[test]
fn options_combos_and_spot_are_refused_by_name_before_anything_leaves() {
    let m = Deribit::new().unwrap();
    for sym in ["BTC-27DEC26-100000-C", "SOL_USDC-27DEC26-150-P", "BTC_USDT", "BTC-FS-20OCT26_PERP"] {
        let o = order(sym, "k", Side::Buy, OrderType::Limit, "1", Some("1"), None);
        let e = m.check_order(&o).unwrap_err();
        assert!(e.message.contains("futures and perpetuals only") && e.message.contains(sym), "{e:?}");
        assert!(m.instrument_rules(&settings(), Some(sym), CorrId(1)).is_err());
        assert!(m.place(&settings(), &o, &placing("k")).is_err());
    }
    for sym in [LINEAR, INVERSE, "BTC-27DEC26", "SOL_USDC-27DEC26"] {
        m.check_order(&order(sym, "k", Side::Buy, OrderType::Limit, "1", Some("1"), None)).unwrap();
    }
}

#[test]
fn an_edit_resends_the_amount_reduce_only_and_the_post_only_flags() {
    let m = Deribit::new().unwrap();
    // The documented row is an option: the same row on a perpetual.
    let open = OPEN.replace("ETH-22FEB19-120-C", "ETH-PERPETUAL");
    let rows = m.decode_orders_in(&ctx("openOrdersByInstrument"), open.as_bytes()).unwrap().0;
    let mut cur = rows[0].clone();
    // A change of price keeps the amount.
    let r = m.amend(&settings(), &cur, &OrderAmend { limit: Some(Price::new(d("0.14"))), ..Default::default() }, &placing("")).unwrap();
    assert_eq!((r.path.as_str(), r.method), ("/api/v2/private/edit", Method::Post));
    assert_eq!(body(&r)["params"], json!({"order_id":"ETH-252017","amount":10,"price":0.14,"reduce_only":false}));
    // A post-only reduce-only order keeps both: the flags and reduce_only go out again.
    cur.order_type = "limit post-only".into();
    cur.reduce_only = true;
    let r = m.amend(&settings(), &cur, &OrderAmend { qty: Some(Qty::new(d("20"))), ..Default::default() }, &placing("")).unwrap();
    assert_eq!(body(&r)["params"], json!({"order_id":"ETH-252017","amount":20,"price":0.135,"post_only":true,"reject_post_only":true,"reduce_only":true}));
    // What an edit cannot do is refused: the time in force, a stop on a limit order.
    assert!(m.amend(&settings(), &cur, &OrderAmend { tif: Some(TimeInForce::Ioc), ..Default::default() }, &placing("")).is_err());
    assert!(m.amend(&settings(), &cur, &OrderAmend { stop: Some(Price::new(d("1"))), ..Default::default() }, &placing("")).unwrap_err().message.contains("does not become a stop"));
    // A stop's trigger price changes; its amount is resent.
    let stop = m.decode_ack(SELL.as_bytes()).unwrap();
    assert_eq!(stop.venue_id, "ETH-SLTS-28");
    let cur = m.decode_orders_in(&ctx("orderState"), &serde_json::to_vec(&json!({"result": serde_json::from_str::<Value>(SELL).unwrap()["result"]["order"]})).unwrap()).unwrap().0.remove(0);
    let r = m.amend(&settings(), &cur, &OrderAmend { stop: Some(Price::new(d("146"))), ..Default::default() }, &placing("")).unwrap();
    assert_eq!(body(&r)["params"], json!({"order_id":"ETH-SLTS-28","amount":123,"price":145.61,"trigger_price":146,"reduce_only":false}));
}

#[test]
fn cancel_cancel_all_lookups_and_reads_are_the_documented_calls() {
    let m = Deribit::new().unwrap();
    let s = settings();
    let c = m.cancel(&s, INVERSE, "ETH-SLIS-12", CorrId(3)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)["params"].clone()), ("/api/v2/private/cancel", json!({"order_id":"ETH-SLIS-12"})));
    let all = m.cancel_all(&s, &[INVERSE.to_string(), LINEAR.to_string()], CorrId(3)).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!((all[0].path.as_str(), body(&all[0])["params"].clone()), ("/api/v2/private/cancel_all_by_instrument", json!({"instrument_name":INVERSE,"type":"all"})));
    assert_eq!(m.cancel_all_way(), CancelAllWay::BySymbol);
    m.decode_cancel_all(CANCEL_ALL.as_bytes()).unwrap();
    assert!(m.decode_cancel_all(br#"{"jsonrpc":"2.0","result":[]}"#).is_err());
    // By label: working orders (type all: trigger orders too), the last 30 minutes under the
    // label (both currencies a linear contract may be filed under), the history.
    let l = m.query_by_client_id(&s, LINEAR, "otwk1", CorrId(3)).unwrap();
    let paths: Vec<(&str, Option<String>)> = l.iter().map(|r| (r.path.as_str(), q(r, "currency"))).collect();
    assert_eq!(
        paths,
        [
            ("/api/v2/private/get_open_orders_by_instrument", None),
            ("/api/v2/private/get_order_state_by_label", Some("USDC".into())),
            ("/api/v2/private/get_order_state_by_label", Some("BTC".into())),
            ("/api/v2/private/get_order_history_by_instrument", None),
        ]
    );
    assert!(l.iter().all(|r| r.signed && r.method == Method::Get));
    assert_eq!((q(&l[0], "type").as_deref(), q(&l[1], "label").as_deref()), (Some("all"), Some("otwk1")));
    assert_eq!((q(&l[3], "historical").as_deref(), q(&l[3], "include_unfilled").as_deref(), q(&l[3], "include_old").as_deref(), q(&l[3], "count").as_deref()), (Some("true"), Some("true"), Some("true"), Some("1000")));
    assert_eq!(m.query_by_client_id(&s, INVERSE, "otwk1", CorrId(3)).unwrap().iter().filter_map(|r| q(r, "currency")).collect::<Vec<_>>(), ["BTC"]);
    // By id: the order's state, the working orders, the trigger history, the history.
    let v = m.query_by_venue_id(&s, INVERSE, "SLIS-103", CorrId(3)).unwrap();
    let paths: Vec<&str> = v.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["/api/v2/private/get_order_state", "/api/v2/private/get_open_orders_by_instrument", "/api/v2/private/get_trigger_order_history", "/api/v2/private/get_order_history_by_instrument"]);
    assert_eq!((q(&v[2], "currency").as_deref(), q(&v[2], "instrument_name").as_deref()), (Some("BTC"), Some(INVERSE)));
    // Working orders: one instrument, or every kind of the account.
    let o = m.open_orders(&s, Some(LINEAR), CorrId(3)).unwrap();
    assert_eq!((o[0].path.as_str(), q(&o[0], "type")), ("/api/v2/private/get_open_orders_by_instrument", None));
    assert_eq!(m.open_orders(&s, None, CorrId(3)).unwrap()[0].path, "/api/v2/private/get_open_orders");
    // Positions, rules, the price a coin size is converted at.
    let p = m.positions(&s, INVERSE, CorrId(3)).unwrap().unwrap();
    assert_eq!((p.path.as_str(), q(&p, "instrument_name").as_deref()), ("/api/v2/private/get_position", Some(INVERSE)));
    let r = m.instrument_rules(&s, Some(LINEAR), CorrId(3)).unwrap();
    assert_eq!((r.path.as_str(), r.endpoint_id, q(&r, "currency").as_deref(), q(&r, "kind").as_deref(), r.signed), ("/api/v2/public/get_instruments", "instruments", Some("USDC"), Some("future"), false));
    assert_eq!(q(&m.instrument_rules(&s, Some(INVERSE), CorrId(3)).unwrap(), "currency").as_deref(), Some("BTC"));
    assert_eq!(q(&m.instrument_rules(&s, None, CorrId(3)).unwrap(), "currency").as_deref(), Some("any"));
    assert_eq!(m.instrument_rules_any_status(&s, INVERSE, CorrId(3)).unwrap()[0].path, "/api/v2/public/get_instrument");
    assert_eq!(m.reference_price(&s, INVERSE, Side::Buy, CorrId(3)).unwrap().unwrap().path, "/api/v2/public/ticker");
    assert_eq!(m.decode_reference_price(TICKER.as_bytes(), Side::Buy).unwrap(), d("36289.5"));
}

#[test]
fn the_test_environment_moves_every_call_and_the_socket_and_a_bad_one_is_refused() {
    let m = Deribit::new().unwrap();
    let t = test_env();
    assert!(m.is_sandbox(&t) && !m.is_sandbox(&settings()));
    let o = order(LINEAR, "k", Side::Buy, OrderType::Limit, "0.01", Some("80000"), None);
    assert_eq!(m.place(&t, &o, &placing("k")).unwrap().base, "rest-test");
    assert_eq!(m.cancel(&t, LINEAR, "1", CorrId(1)).unwrap().base, "rest-test");
    assert!(m.query_by_client_id(&t, LINEAR, "k", CorrId(1)).unwrap().iter().all(|r| r.base == "rest-test"));
    assert_eq!(m.instrument_rules(&t, None, CorrId(1)).unwrap().base, "rest-test");
    assert_eq!(m.private_socket(&t).unwrap().base, "ws-test");
    assert_eq!(m.private_socket(&settings()).unwrap().base, "ws");
    assert!(m.base_urls().contains(&("rest-test", "https://test.deribit.com")) && m.base_urls().contains(&("ws-test", "wss://test.deribit.com")));
    let mut bad = settings();
    bad.insert(ENV.into(), "demo".into());
    assert!(m.check_settings(&bad).unwrap_err().message.contains("live") && m.place(&bad, &o, &placing("k")).is_err());
    assert!(m.check_settings(&t).is_ok() && m.check_settings(&settings()).is_ok());
    // The account read goes to the test host too.
    let cmd = otw_connector_model::Command::QueryAccount { req_id: CorrId(9), kind: otw_connector_model::AccountKind::Positions, symbols: vec![], range: None, cursor: None, account: Some("test".into()) };
    let reqs = otw_connector_testkit::encode_once(&m, &mut Session::default(), &otw_connector_sdk::InstrumentCache::new(), &cmd, otw_connector_testkit::ts(1_792_400_000)).unwrap();
    assert!(matches!(&reqs[0], NativeRequest::Http(h) if h.base == "rest-test"));
    assert_eq!(m.account_selector(&t).as_deref(), Some("test"));
}

#[test]
fn the_label_is_letters_digits_dash_underscore_up_to_64() {
    let m = Deribit::new().unwrap();
    assert!(m.client_id(&"a".repeat(64)).is_ok());
    assert!(m.client_id(&"a".repeat(65)).is_err());
    assert!(m.client_id("otw:1").is_err());
}

// ── Answers ──

#[test]
fn documented_answers_decode() {
    let m = Deribit::new().unwrap();
    // A filled inverse market buy: its id and label; sizes in USD.
    assert_eq!(m.decode_ack(BUY.as_bytes()).unwrap(), Ack { venue_id: "ETH-584849853".into(), client_id: Some("market0000234".into()) });
    let edit = m.decode_ack(EDIT.as_bytes()).unwrap();
    assert_eq!((edit.venue_id.as_str(), edit.client_id), ("438994", None));
    // The cancel's answer: an untriggered stop market, its trigger pending, no limit price.
    let c = m.decode_orders_in(&ctx("orderState"), CANCEL.as_bytes()).unwrap().0;
    assert_eq!((c[0].venue_id.as_str(), c[0].status.clone(), c[0].trigger_pending, c[0].limit, c[0].stop), ("ETH-SLIS-12", Mapped::Known(OrderStatus::New), true, None, Some(d("144.73"))));
    let s = m.decode_orders_in(&ctx("orderState"), ORDER_STATE.as_bytes()).unwrap().0;
    assert_eq!((s[0].status.clone(), s[0].filled_qty, s[0].qty, s[0].avg_price, s[0].side.clone()), (Mapped::Known(OrderStatus::Filled), d("37"), d("37"), Some(d("118.94")), Mapped::Known(Side::Sell)));
    let l = m.decode_orders_in(&ctx("orderStateByLabel"), BY_LABEL.as_bytes()).unwrap().0;
    assert_eq!(l[0].client_id.as_deref(), Some("fooBar"));
    let (h, next) = m.decode_orders_in(&ctx("orderHistory"), HISTORY.as_bytes()).unwrap();
    assert_eq!((h[0].venue_id.as_str(), h[0].limit, next), ("146475", Some(d("3886.5")), None));
    let o = m.decode_orders_in(&ctx("openOrdersByInstrument"), OPEN.as_bytes()).unwrap().0;
    assert_eq!((o[0].status.clone(), o[0].order_type.as_str(), o[0].client_id.clone()), (Mapped::Known(OrderStatus::New), "limit", None));
    // The trigger history: the trigger, naming the order it became.
    let t = m.decode_orders_in(&ctx("triggerHistory"), TRIGGERS.as_bytes()).unwrap().0;
    assert_eq!((t[0].venue_id.as_str(), t[0].successor_id.as_deref(), t[0].symbol.as_str()), ("SLIS-103", Some("671473"), INVERSE));
    // Fills, in the order's unit (USD here), the fee as stated, the next page from the last one.
    let (f, next) = m.decode_fills(TRADES.as_bytes()).unwrap();
    assert_eq!((f.len(), f[0].exec_id.as_str(), f[0].venue_order_id.as_str(), f[0].qty, f[0].fee, f[0].fee_currency.as_str()), (2, "48078944", "4008716877", d("4490"), d("0.00037465"), "BTC"));
    assert_eq!((f[1].fee, next.as_deref()), (d("0.00001503"), Some("1590480866924")));
    assert!(matches!(f[0].side, Mapped::Known(Side::Sell)) && !f[0].position_only);
    // Positions: an inverse one in USD (the doc's 50), the coin worth in `size_currency`.
    let p = m.decode_positions(POSITIONS.as_bytes()).unwrap();
    assert_eq!((p[0].qty, p[0].direction, p[0].side, p[0].leverage), (d("50"), Some(Side::Buy), PositionSide::Net, Some(d("35.639422"))));
    let p = m.decode_positions(POSITION.as_bytes()).unwrap();
    assert_eq!((p[0].qty, p[0].direction), (Decimal::ZERO, Some(Side::Buy)), "get_position's example: size 0");
    let lin = format!(r#"{{"jsonrpc":"2.0","result":{{"instrument_name":"{LINEAR}","direction":"sell","size":-8212.55,"size_currency":-0.1,"kind":"future"}}}}"#);
    assert_eq!(m.decode_positions(lin.as_bytes()).unwrap()[0].qty, d("0.1"), "a linear position counts the coin");
    // Rules: an inverse contract is sized in USD in steps of its minimum; an option is no rule.
    let (r, _) = m.decode_rules(INSTRUMENTS.as_bytes()).unwrap();
    let perp = r.iter().find(|r| r.instrument.native == INVERSE).unwrap();
    assert_eq!((perp.tick, perp.lot_step, perp.min_qty, perp.size_unit, perp.status.clone()), (Some(d("0.5")), Some(d("10")), Some(d("10")), SizeUnit::Quote, TradingStatus::Trading));
    perp.sound().unwrap();
    assert!(m.decode_rules(INSTRUMENT.as_bytes()).unwrap().0.is_empty(), "the documented row is an option");
    let row = format!(r#"{{"jsonrpc":"2.0","result":[{LINEAR_ROW}]}}"#);
    let lin = &m.decode_rules(row.as_bytes()).unwrap().0[0];
    assert_eq!((lin.size_unit, lin.lot_step, lin.contract_size), (SizeUnit::Base, Some(d("0.0001")), None));
    let closed = row.replace(r#""state":"open""#, r#""state":"settlement""#);
    assert_eq!(m.decode_rules(closed.as_bytes()).unwrap().0[0].status, TradingStatus::Closed("settlement".into()));
}

#[test]
fn every_state_word_maps_and_an_unknown_one_stays_unknown() {
    let m = Deribit::new().unwrap();
    for (w, s) in [("open", OrderStatus::New), ("untriggered", OrderStatus::New), ("filled", OrderStatus::Filled), ("rejected", OrderStatus::Rejected), ("cancelled", OrderStatus::Cancelled)] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    // A fired trigger's own word says nothing of the order it became; `archive` (trade rows).
    for w in ["triggered", "archive", "Open", "new"] {
        assert!(matches!(m.status_of(w), Mapped::Unknown(_)), "{w}");
    }
    // `open` with a fill is partly filled.
    let row = OPEN.replace(r#""filled_amount": 0"#, r#""filled_amount": 4"#);
    assert_eq!(m.decode_orders_in(&ctx("openOrdersByInstrument"), row.as_bytes()).unwrap().0[0].status, Mapped::Known(OrderStatus::PartiallyFilled));
}

/// One label, two rows: the stop that fired (`triggered`) and the order it became
/// (`trigger_order_id`, the same label carried over, per get_order_state_by_label).
fn fired_pair() -> String {
    let v: Value = serde_json::from_str(CHANGES).unwrap();
    json!({"jsonrpc":"2.0","result": v["params"]["data"]["orders"]}).to_string()
}

#[test]
fn a_trigger_that_fired_is_followed_to_the_order_it_became() {
    let m = Deribit::new().unwrap();
    let rows = m.decode_orders_in(&ctx("orderStateByLabel"), fired_pair().as_bytes()).unwrap().0;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].venue_id.as_str(), rows[0].status.clone(), rows[0].client_id.as_deref()), ("671473", Mapped::Known(OrderStatus::Filled), Some("otwk1")), "the spawned order first");
    assert_eq!((rows[1].venue_id.as_str(), rows[1].successor_id.as_deref()), ("SLIS-103", Some("671473")));
    // Read by id, a fired trigger whose successor the answer does not hold is passed over, so
    // the trigger history is read next.
    let v: Value = serde_json::from_str(CHANGES).unwrap();
    let alone = json!({"jsonrpc":"2.0","result": v["params"]["data"]["orders"][0]}).to_string();
    assert!(m.decode_orders_in(&ctx("orderState"), alone.as_bytes()).unwrap().0.is_empty());
    assert_eq!(m.decode_orders_in(&ctx("orderStateByLabel"), alone.as_bytes()).unwrap().0.len(), 1, "a lookup by label keeps it: never read as never seen");
}

#[test]
fn fills_are_read_from_the_store_the_window_reaches() {
    let m = Deribit::new().unwrap();
    let s = settings();
    let now = 1_800_000_000_000;
    let hist = |r: &HttpRequest| (q(r, "historical").unwrap(), q(r, "start_timestamp").unwrap().parse::<i64>().unwrap(), q(r, "end_timestamp").unwrap().parse::<i64>().unwrap());
    // The last hour: the recent store only.
    let r = m.fills_requests(&s, LINEAR, now - 3_600_000, now, now, CorrId(1)).unwrap();
    assert_eq!(r.iter().map(hist).collect::<Vec<_>>(), [("false".to_string(), now - 3_600_000, now)]);
    assert_eq!(m.fills_since(&s, LINEAR, now - 3_600_000, now, CorrId(1)).unwrap(), r[0]);
    // A week ago: the historical store only.
    let r = m.fills_requests(&s, LINEAR, now - 8 * 86_400_000, now - 7 * 86_400_000, now, CorrId(1)).unwrap();
    assert_eq!(r.iter().map(|x| q(x, "historical").unwrap()).collect::<Vec<_>>(), ["true"]);
    // Three days to now: both, the recent one from the 24-hour edge.
    let r = m.fills_requests(&s, LINEAR, now - 3 * 86_400_000, now, now, CorrId(1)).unwrap();
    assert_eq!(r.iter().map(hist).collect::<Vec<_>>(), [("true".to_string(), now - 3 * 86_400_000, now), ("false".to_string(), now - 86_400_000, now)]);
    let first = &r[0];
    assert_eq!((first.path.as_str(), q(first, "count").as_deref(), q(first, "sorting").as_deref(), q(first, "instrument_name").as_deref()), ("/api/v2/private/get_user_trades_by_instrument_and_time", Some("1000"), Some("asc"), Some(LINEAR)));
    // The next page from the last fill's millisecond; the history by offset.
    let next = m.with_cursor(first, "1590480866924");
    assert_eq!(q(&next, "start_timestamp").as_deref(), Some("1590480866924"));
    let h = m.query_by_client_id(&s, LINEAR, "k", CorrId(1)).unwrap().pop().unwrap();
    let h2 = m.with_cursor(&h, "1000");
    assert_eq!(q(&h2, "offset").as_deref(), Some("1000"));
    assert_eq!(q(&m.with_cursor(&h2, "1000"), "offset").as_deref(), Some("2000"));
}

#[test]
fn a_liquidation_moved_the_position_without_an_order_of_the_account() {
    let m = Deribit::new().unwrap();
    let t = TRADES.replacen(r#""order_type": "limit""#, r#""order_type": "liquidation", "liquidation": "M""#, 1);
    let (f, _) = m.decode_fills(t.as_bytes()).unwrap();
    assert!(f[0].position_only && !f[1].position_only);
    // `liquidation` names the side under liquidation, `liquidity` the account's (taker here):
    // the account's own side moved its position, the maker's is a counterparty's.
    for (side, own) in [("T", true), ("MT", true), ("M", false)] {
        let t = TRADES.replacen(r#""order_type": "limit""#, &format!(r#""order_type": "limit", "liquidation": "{side}""#), 1);
        let (f, _) = m.decode_fills(t.as_bytes()).unwrap();
        assert_eq!(f[0].position_only, own, "liquidation {side}");
    }
}

// ── Private stream ──

#[test]
fn the_socket_logs_in_subscribes_sets_the_heartbeat_and_pings_with_public_test() {
    let m = Deribit::new().unwrap();
    let s = m.private_socket(&settings()).unwrap();
    assert_eq!((s.base, s.path.as_str(), s.ping_every_ms), ("ws", "/ws/api/v2", 15_000));
    let frames: Vec<Value> = s.subscribes().iter().map(|f| serde_json::from_slice(f).unwrap()).collect();
    assert_eq!(frames[0], json!({"jsonrpc":"2.0","id":2,"method":"private/subscribe","params":{"channels":[CHANNEL]}}));
    assert_eq!(frames[1], json!({"jsonrpc":"2.0","id":3,"method":"public/set_heartbeat","params":{"interval":30}}));
    assert_eq!(serde_json::from_slice::<Value>(&s.ping).unwrap(), json!({"jsonrpc":"2.0","id":4,"method":"public/test","params":{}}));
    // The documented answers, under OTW's ids.
    let with_id = |doc: &str, id: u64| {
        let mut v: Value = serde_json::from_str(doc).unwrap();
        v["id"] = json!(id);
        v.to_string()
    };
    let auth = r#"{"jsonrpc":"2.0","id":1,"result":{"access_token":"t","expires_in":31536000,"refresh_token":"r","scope":"connection mainaccount","token_type":"bearer"}}"#;
    assert_eq!(m.decode_private(auth.as_bytes()).unwrap(), vec![PrivateEvent::Answer { op: "auth".into(), ok: true, message: String::new() }]);
    let refused = r#"{"jsonrpc":"2.0","id":1,"error":{"code":13004,"message":"invalid_credentials"}}"#;
    assert!(matches!(&m.decode_private(refused.as_bytes()).unwrap()[0], PrivateEvent::Answer { op, ok: false, message } if op == "auth" && message.contains("13004")));
    // The doc's subscribe answer names another channel: not ours, so not subscribed.
    assert!(matches!(&m.decode_private(with_id(SUBSCRIBE, 2).as_bytes()).unwrap()[0], PrivateEvent::Answer { ok: false, .. }));
    let ours = json!({"jsonrpc":"2.0","id":2,"result":[CHANNEL]}).to_string();
    assert!(matches!(&m.decode_private(ours.as_bytes()).unwrap()[0], PrivateEvent::Answer { op, ok: true, .. } if op == "subscribe"));
    assert!(matches!(&m.decode_private(with_id(HEARTBEAT, 3).as_bytes()).unwrap()[0], PrivateEvent::Answer { op, ok: true, .. } if op == "subscribe"));
    assert!(matches!(&m.decode_private(with_id(TEST, 4).as_bytes()).unwrap()[0], PrivateEvent::Answer { op, ok: true, .. } if op == "ping"));
    // The venue's heartbeat asks for a `public/test`: the next ping is one.
    assert!(m.decode_private(br#"{"jsonrpc":"2.0","method":"heartbeat","params":{"type":"test_request"}}"#).unwrap().is_empty());
}

#[test]
fn a_user_changes_frame_is_orders_fills_and_positions_and_a_fired_stop_is_its_new_order() {
    let m = Deribit::new().unwrap();
    let ev = m.decode_private(CHANGES.as_bytes()).unwrap();
    assert_eq!(ev.len(), 3, "{ev:?}");
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.venue_id.as_str(), o.client_id.as_deref(), o.status.clone(), o.qty), ("671473", Some("otwk1"), Mapped::Known(OrderStatus::Filled), d("10")));
    let PrivateEvent::Fill(f) = &ev[1] else { panic!() };
    assert_eq!((f.exec_id.as_str(), f.venue_order_id.as_str(), f.qty, f.fee_currency.as_str()), ("48078944", "671473", d("10"), "BTC"));
    let PrivateEvent::Position(p) = &ev[2] else { panic!() };
    assert_eq!((p.symbol.as_str(), p.qty, p.direction, p.leverage), (INVERSE, Decimal::ZERO, None, Some(d("50"))));
    // The stop alone (the new order in a later frame): read again by its id, then followed.
    let mut v: Value = serde_json::from_str(CHANGES).unwrap();
    v["params"]["data"]["orders"].as_array_mut().unwrap().truncate(1);
    v["params"]["data"]["trades"] = json!([]);
    v["params"]["data"]["positions"] = json!([]);
    assert_eq!(m.decode_private(v.to_string().as_bytes()).unwrap(), vec![PrivateEvent::OrderChanged { symbol: Some(INVERSE.into()), venue_id: "SLIS-103".into() }]);
    // Another channel says nothing here.
    assert!(m.decode_private(br#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"quote.BTC-PERPETUAL","data":{}}}"#).unwrap().is_empty());
}

// ── Through the trader, against a mock venue ──

struct Mock2 {
    server: MockServer,
    driver: otw_connector_sdk::Driver<Deribit>,
    trader: Trader<Deribit>,
    settings: Settings,
    signer: Arc<dyn Signer>,
}

impl Mock2 {
    async fn new() -> Self {
        let server = MockServer::start().await;
        let clock = ManualClock::at(otw_connector_testkit::ts(CATALOG_NOW_MS / 1000));
        let driver = driver_against(Deribit::new().unwrap(), &server, clock);
        let settings = settings();
        let signer: Arc<dyn Signer> = Arc::new(signer(&settings));
        let m = Mock2 { server, driver, trader: Trader::new(), settings, signer };
        let rules = m.driver.mapper().instrument_rules(&m.settings, Some(INVERSE), CorrId(1)).unwrap();
        m.mount(&rules, 200, INSTRUMENTS, 5).await;
        let pos = m.driver.mapper().positions(&m.settings, INVERSE, CorrId(1)).unwrap().unwrap();
        m.mount(&pos, 200, &POSITION.replace(r#""direction": "buy""#, r#""direction": "zero""#), 5).await;
        m
    }

    fn account(&self) -> Account<'_> {
        Account { settings: &self.settings, signer: self.signer.as_ref(), scope: "deribit-test" }
    }

    async fn mount(&self, want: &HttpRequest, status: u16, body: &str, priority: u8) {
        self.mount_if(want, None, status, body, priority).await;
    }

    async fn mount_if(&self, want: &HttpRequest, needle: Option<&str>, status: u16, body: &str, priority: u8) {
        let (want, needle) = (want.clone(), needle.map(str::to_string));
        Mock::given(move |r: &Request| {
            r.url.path() == want.path
                && want.query.iter().all(|p| r.url.query_pairs().any(|(k, v)| k == p.0 && v == p.1))
                && needle.as_ref().is_none_or(|n| String::from_utf8_lossy(&r.body).contains(n.as_str()))
        })
        .respond_with(ResponseTemplate::new(status).set_body_string(body.to_string()).insert_header("content-type", "application/json"))
        .with_priority(priority)
        .mount(&self.server)
        .await;
    }

    async fn sent(&self, path: &str) -> Vec<Value> {
        self.server.received_requests().await.unwrap_or_default().iter().filter(|r| r.url.path() == path).map(|r| serde_json::from_slice(&r.body).unwrap()).collect()
    }
}

fn ack(id: &str, label: &str, amount: &str) -> String {
    let mut v: Value = serde_json::from_str(BUY).unwrap();
    v["result"]["order"]["order_id"] = json!(id);
    v["result"]["order"]["label"] = json!(label);
    v["result"]["order"]["instrument_name"] = json!(INVERSE);
    v["result"]["order"]["amount"] = serde_json::from_str(amount).unwrap();
    v.to_string()
}

fn inverse(key: &str, qty: &str, unit: SizeUnit) -> OrderRequest {
    let mut o = order(INVERSE, key, Side::Buy, OrderType::Limit, qty, Some("36000"), None);
    o.instrument = InstrumentId::new("DERIBIT", INVERSE);
    o.opts.size_unit = unit;
    o
}

#[tokio::test]
async fn an_inverse_order_given_in_usd_goes_out_rounded_down_to_the_contract_step() {
    let v = Mock2::new().await;
    let place = v.driver.mapper().place(&v.settings, &inverse("k1", "39", SizeUnit::Quote), &Placing { quote_amount: Some(Decimal::ONE), ..placing("k1") }).unwrap();
    v.mount(&place, 200, &ack("BTC-1", "k1", "30"), 3).await;
    let placed = v.trader.place(&v.driver, &v.account(), &[inverse("k1", "39", SizeUnit::Quote)], &TicketOptions::default()).await.unwrap();
    assert_eq!(placed[0].venue_id, "BTC-1");
    assert!(placed[0].note.as_deref().unwrap().contains("39 went out as 30"), "{:?}", placed[0].note);
    let sent = v.sent("/api/v2/private/buy").await;
    assert_eq!(sent[0]["params"]["amount"], json!(30));
    // Under the smallest size: refused, naming it, nothing sent.
    let out = v.trader.place(&v.driver, &v.account(), &[inverse("k2", "9", SizeUnit::Quote)], &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("smallest size it takes, 10")), "{out:?}");
    assert_eq!(v.sent("/api/v2/private/buy").await.len(), 1);
}

#[tokio::test]
async fn an_inverse_order_given_in_the_coin_is_converted_at_the_last_price_then_rounded_down() {
    let v = Mock2::new().await;
    let ticker = v.driver.mapper().reference_price(&v.settings, INVERSE, Side::Buy, CorrId(1)).unwrap().unwrap();
    v.mount(&ticker, 200, TICKER, 3).await;
    let place = v.driver.mapper().place(&v.settings, &inverse("k1", "0.001", SizeUnit::Base), &Placing { quote_amount: Some(Decimal::ONE), ..placing("k1") }).unwrap();
    v.mount(&place, 200, &ack("BTC-2", "k1", "30"), 3).await;
    // 0.001 BTC at the 36289.5 last price is 36.2895 USD: 30 in steps of 10.
    let placed = v.trader.place(&v.driver, &v.account(), &[inverse("k1", "0.001", SizeUnit::Base)], &TicketOptions::default()).await.unwrap();
    assert!(placed[0].note.as_deref().unwrap().contains("0.001 in the coin at 36289.5 went out as 30"), "{:?}", placed[0].note);
    assert_eq!(v.sent("/api/v2/private/buy").await[0]["params"]["amount"], json!(30));
    // Worth under one step: refused with the coin the smallest size is worth.
    let out = v.trader.place(&v.driver, &v.account(), &[inverse("k2", "0.0001", SizeUnit::Base)], &TicketOptions::default()).await;
    assert!(matches!(&out, Err(TradeError::Refused(e)) if e.message.contains("smallest size it takes, 10") && e.message.contains("in the coin")), "{out:?}");
}

#[tokio::test]
async fn an_inverse_position_is_closed_in_usd_reduce_only_on_the_other_side() {
    let v = Mock2::new().await;
    let pos = v.driver.mapper().positions(&v.settings, INVERSE, CorrId(1)).unwrap().unwrap();
    let short = POSITION.replace(r#""direction": "buy""#, r#""direction": "sell""#).replace(r#""size": 0"#, r#""size": -50"#);
    v.mount(&pos, 200, &short, 1).await;
    let probe = v.driver.mapper().place(&v.settings, &inverse("c1", "1", SizeUnit::Quote), &Placing { quote_amount: Some(Decimal::ONE), ..placing("c1") }).unwrap();
    v.mount(&probe, 200, &ack("BTC-3", "c1", "50"), 3).await;
    let placed = v.trader.close_position(&v.driver, &v.account(), INVERSE, None, None, "c1").await.unwrap();
    assert_eq!(placed.venue_id, "BTC-3");
    let sent = v.sent("/api/v2/private/buy").await;
    assert_eq!(sent[0]["params"], json!({"instrument_name":INVERSE,"amount":50,"type":"market","label":"c1","time_in_force":"immediate_or_cancel","reduce_only":true}));
}

#[tokio::test]
async fn a_cancel_of_a_stop_that_fired_cancels_the_order_it_became() {
    let v = Mock2::new().await;
    let m = v.driver.mapper();
    let cancel = m.cancel(&v.settings, INVERSE, "SLIS-103", CorrId(1)).unwrap();
    v.mount_if(&cancel, Some("SLIS-103"), 400, r#"{"jsonrpc":"2.0","error":{"code":10010,"message":"already_closed"}}"#, 2).await;
    v.mount_if(&cancel, Some("671473"), 200, CANCEL, 2).await;
    // Read by id: the stop alone, fired (passed over); not working; the trigger history names
    // the order it became, which is working.
    let pair: Value = serde_json::from_str(&fired_pair()).unwrap();
    let stop = json!({"jsonrpc":"2.0","result": pair["result"][0]}).to_string();
    let mut child = pair["result"][1].clone();
    child["order_state"] = json!("open");
    child["filled_amount"] = json!(0);
    child["order_type"] = json!("limit");
    let reqs = m.query_by_venue_id(&v.settings, INVERSE, "SLIS-103", CorrId(1)).unwrap();
    v.mount(&reqs[0], 200, &stop, 3).await;
    v.mount(&reqs[1], 200, r#"{"jsonrpc":"2.0","result":[]}"#, 3).await;
    v.mount(&reqs[2], 200, TRIGGERS, 3).await;
    let state_of_child = m.query_by_venue_id(&v.settings, INVERSE, "671473", CorrId(1)).unwrap();
    v.mount(&state_of_child[0], 200, &json!({"jsonrpc":"2.0","result": child}).to_string(), 2).await;
    let out = v.trader.cancel(&v.driver, &v.account(), INVERSE, "SLIS-103").await.unwrap();
    assert_eq!(out, Cancelled::Sent);
    let sent: Vec<String> = v.sent("/api/v2/private/cancel").await.iter().map(|b| b["params"]["order_id"].as_str().unwrap().to_string()).collect();
    assert_eq!(sent, ["SLIS-103", "671473"]);
}

#[tokio::test]
async fn a_lost_answer_finds_the_order_a_fired_stop_became_by_its_label() {
    let v = Mock2::new().await;
    let m = v.driver.mapper();
    let mut o = order(INVERSE, "otwk1", Side::Buy, OrderType::Stop, "10", None, Some("5285"));
    o.opts = OrderOpts { trigger: Some(TriggerKind::Index), reduce_only: true, size_unit: SizeUnit::Quote, ..Default::default() };
    let place = m.place(&v.settings, &o, &Placing { quote_amount: Some(Decimal::TEN), ..placing("otwk1") }).unwrap();
    v.mount(&place, 502, "<html>bad gateway</html>", 3).await;
    let reqs = m.query_by_client_id(&v.settings, INVERSE, "otwk1", CorrId(1)).unwrap();
    v.mount(&reqs[0], 200, r#"{"jsonrpc":"2.0","result":[]}"#, 3).await;
    v.mount(&reqs[1], 200, &fired_pair(), 3).await;
    let placed = v.trader.place(&v.driver, &v.account(), &[o], &TicketOptions::default()).await.unwrap();
    assert_eq!(placed[0].venue_id, "671473", "the order the stop became, never sent twice");
    assert_eq!(v.sent("/api/v2/private/buy").await.len(), 1);
}

#[tokio::test]
async fn fills_over_both_stores_are_merged_each_once() {
    let v = Mock2::new().await;
    let m = v.driver.mapper();
    let since = CATALOG_NOW_MS - 3 * 86_400_000;
    let reqs = m.fills_requests(&v.settings, INVERSE, since, CATALOG_NOW_MS, CATALOG_NOW_MS, CorrId(1)).unwrap();
    // The historical answer holds both documented fills; the recent one repeats the second.
    let page: Value = serde_json::from_str(TRADES).unwrap();
    let older = json!({"jsonrpc":"2.0","result":{"trades": page["result"]["trades"], "has_more": false}}).to_string();
    let recent = json!({"jsonrpc":"2.0","result":{"trades": [page["result"]["trades"][1]], "has_more": false}}).to_string();
    let strip = |r: &HttpRequest| {
        let mut r = r.clone();
        r.query.retain(|(k, _)| k == "historical" || k == "instrument_name");
        r
    };
    v.mount(&strip(&reqs[0]), 200, &older, 3).await;
    v.mount(&strip(&reqs[1]), 200, &recent, 3).await;
    let fills = v.trader.fills_since(&v.driver, &v.account(), INVERSE, since).await.unwrap();
    assert_eq!(fills.iter().map(|f| f.exec_id.as_str()).collect::<Vec<_>>(), ["48078944", "48078945"]);
}

#[tokio::test]
async fn an_edit_through_the_trader_reads_the_order_and_resends_what_it_holds() {
    let v = Mock2::new().await;
    let m = v.driver.mapper();
    let mut row: Value = serde_json::from_str::<Value>(OPEN).unwrap()["result"][0].clone();
    row["instrument_name"] = json!(INVERSE);
    row["price"] = json!(36000);
    row["amount"] = json!(100);
    row["post_only"] = json!(true);
    row["reduce_only"] = json!(true);
    let reqs = m.query_by_venue_id(&v.settings, INVERSE, "ETH-252017", CorrId(1)).unwrap();
    v.mount(&reqs[0], 200, &json!({"jsonrpc":"2.0","result": row}).to_string(), 3).await;
    let edit = m.cancel(&v.settings, INVERSE, "x", CorrId(1)).unwrap();
    let mut edit = edit;
    edit.path = "/api/v2/private/edit".into();
    v.mount(&edit, 200, EDIT, 3).await;
    v.trader.modify(&v.driver, &v.account(), INVERSE, "ETH-252017", &OrderAmend { limit: Some(Price::new(d("36000.5"))), ..Default::default() }).await.unwrap();
    let sent = v.sent("/api/v2/private/edit").await;
    assert_eq!(sent[0]["params"], json!({"order_id":"ETH-252017","amount":100,"price":36000.5,"post_only":true,"reject_post_only":true,"reduce_only":true}));
    // Off the tick: refused before it leaves.
    let out = v.trader.modify(&v.driver, &v.account(), INVERSE, "ETH-252017", &OrderAmend { limit: Some(Price::new(d("36000.2"))), ..Default::default() }).await;
    assert!(matches!(out, Err(TradeError::Refused(_))));
    assert_eq!(v.sent("/api/v2/private/edit").await.len(), 1);
}

// ── The catalog ──

fn envelope(code: i64, msg: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":{code},"message":"{msg}"}},"testnet":false}}"#)
}

/// A row in the documented order shape (get_open_orders_by_instrument's example).
fn order_row(sym: &str, cid: Option<&str>, vid: &str, word: &str, side: Side, qty: Decimal, filled: Decimal, price: Decimal) -> Value {
    json!({
        "time_in_force":"good_til_cancelled","reduce_only":false,"price":price,"post_only":false,"order_type":"limit",
        "order_state":word,"order_id":vid,"max_show":qty,"last_update_timestamp":CATALOG_NOW_MS,"label":cid.unwrap_or(""),
        "is_rebalance":false,"is_liquidation":false,"instrument_name":sym,"filled_amount":filled,
        "direction": if side == Side::Buy { "buy" } else { "sell" },"creation_timestamp":CATALOG_NOW_MS - 1000,"average_price":0,"api":true,"amount":qty
    })
}

#[derive(Clone)]
struct Script<M: TradeMapper + Clone> {
    m: M,
}

impl<M: TradeMapper + Clone + 'static> VenueScript for Script<M> {
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
        LINEAR
    }

    fn qty(&self) -> Decimal {
        d("0.02")
    }

    fn price(&self) -> Decimal {
        d("80000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        let row = match closed {
            Some(w) => LINEAR_ROW.replace(r#""state":"open""#, &format!(r#""state":"{w}""#)),
            None => LINEAR_ROW.to_string(),
        };
        format!(r#"{{"jsonrpc":"2.0","id":1,"result":[{row}]}}"#)
    }

    fn closed_word(&self) -> &'static str {
        "halted"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::Filled => "filled",
            OrderStatus::Cancelled | OrderStatus::Expired => "cancelled",
            OrderStatus::Rejected => "rejected",
            _ => "open",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        let o = order_row(LINEAR, Some(client_id), venue_id, "open", Side::Buy, self.qty(), Decimal::ZERO, self.price());
        json!({"jsonrpc":"2.0","id":client_id,"result":{"trades":[],"order":o}}).to_string()
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let rows: Vec<Value> = rows.iter().map(|r| order_row(LINEAR, r.client_id, r.venue_id, r.word, r.side, r.qty, r.filled, self.price())).collect();
        json!({"jsonrpc":"2.0","id":1,"result":rows}).to_string()
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let rows: Vec<Value> = rows
            .iter()
            .map(|f| {
                json!({"trade_seq":1,"trade_id":f.exec_id,"timestamp":CATALOG_NOW_MS - 60_000,"tick_direction":2,"state":"filled","reduce_only":false,"price":f.price,
                       "post_only":false,"order_type":"limit","order_id":f.venue_id,"label":f.client_id,"matching_id":null,"mark_price":f.price,"liquidity":"M",
                       "instrument_name":LINEAR,"index_price":f.price,"fee_currency":"USDC","fee":f.fee,"direction":"buy","amount":f.qty})
            })
            .collect();
        json!({"jsonrpc":"2.0","id":1,"result":{"trades":rows,"has_more":false}}).to_string()
    }

    fn fee_currency(&self) -> &'static str {
        "USDC"
    }

    /// get_position's shape; several rows (or none) as get_positions lists them. Deribit holds
    /// one position per instrument: a long and a short row are two flat ones.
    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        let row = |dir: Option<Side>, q: Decimal| {
            let (w, sign) = match dir {
                Some(Side::Buy) => ("buy", Decimal::ONE),
                Some(Side::Sell) => ("sell", -Decimal::ONE),
                None => ("zero", Decimal::ZERO),
            };
            json!({"average_price":self.price(),"direction":w,"instrument_name":LINEAR,"kind":"future","leverage":50,"mark_price":self.price(),"size":sign * q * self.price(),"size_currency":sign * q})
        };
        let result = match rows {
            [(_, dir, q)] => row(*dir, *q),
            many => Value::Array(many.iter().map(|(_, dir, q)| row(*dir, *q)).collect()),
        };
        json!({"jsonrpc":"2.0","id":1,"result":result}).to_string()
    }

    fn ok(&self) -> String {
        r#"{"jsonrpc":"2.0","id":1,"result":1}"#.into()
    }

    fn dry_run_ok(&self) -> String {
        self.ok()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        match r {
            VenueRefusal::Funds => Answer::new(400, envelope(10009, "not_enough_funds")),
            VenueRefusal::ReduceOnly => Answer::new(400, envelope(11030, "other_reject reduce_only")),
            VenueRefusal::NotFound => Answer::new(400, envelope(10004, "order_not_found")),
            VenueRefusal::Final => Answer::new(400, envelope(10010, "already_closed")),
            VenueRefusal::RateLimited(s) => Answer { headers: vec![("Retry-After", s.to_string())], ..Answer::new(400, envelope(10028, "too_many_requests")) },
            VenueRefusal::ClockSkew => Answer::new(400, r#"{"jsonrpc":"2.0","error":{"code":13004,"data":{"invalid":"timestamp"},"message":"invalid_credentials"}}"#),
            VenueRefusal::Unmapped => Answer::new(400, envelope(11004, "order_not_found")),
            VenueRefusal::Gateway => Answer::new(502, "<html>bad gateway</html>"),
            VenueRefusal::PositionModeMismatch => Answer::new(400, envelope(11030, "other_reject")),
        }
    }
}

/// Runs the catalog; only the checks written for another venue's words may fail.
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

/// The catalog on the linear book. Two of its checks are written for another venue's words, and
/// each is run on Deribit's own terms above:
/// - T05 sends a stop with no trigger kind; Deribit has no default and requires one, so the
///   order is refused before I/O (`stops_and_if_touched_orders_carry_the_trigger...`).
/// - "position mode" needs a hedge mode and a refusal that means the mode changed; Deribit holds
///   one position per instrument and documents neither.
///
/// The catalog mounts one place path for both sides, and Deribit names the direction in the path
/// (`private/buy`, `private/sell`): the real mapper runs behind [`CatalogDeribit`], which sends a
/// sell to the buy path (its body still says `private/sell`). Every other call and decoder is the
/// mapper's own.
#[tokio::test]
async fn the_trading_catalog_passes_on_the_linear_book() {
    catalog(Script { m: CatalogDeribit(Deribit::new().unwrap()) }, &["T05", "position mode"]).await;
}

#[derive(Clone)]
struct CatalogDeribit(Deribit);

impl Mapper for CatalogDeribit {
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

impl TradeMapper for CatalogDeribit {
    fn venue_name(&self) -> &str {
        self.0.venue_name()
    }
    fn client_id_rule(&self) -> ClientIdRule {
        self.0.client_id_rule()
    }
    fn is_sandbox(&self, s: &Settings) -> bool {
        self.0.is_sandbox(s)
    }
    fn check_order(&self, o: &OrderRequest) -> R<()> {
        self.0.check_order(o)
    }
    fn place(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> R<HttpRequest> {
        let mut r = self.0.place(s, o, p)?;
        r.path = "/api/v2/private/buy".into();
        Ok(r)
    }
    fn amend(&self, s: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> R<HttpRequest> {
        self.0.amend(s, cur, change, p)
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
    fn decode_cancel_all(&self, body: &[u8]) -> R<()> {
        self.0.decode_cancel_all(body)
    }
    fn query_by_client_id(&self, s: &Settings, symbol: &str, cid: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_client_id(s, symbol, cid, corr)
    }
    fn query_by_venue_id(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.query_by_venue_id(s, symbol, id, corr)
    }
    fn lookup_pages(&self) -> usize {
        self.0.lookup_pages()
    }
    fn open_orders(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.open_orders(s, symbol, corr)
    }
    fn fills_since(&self, s: &Settings, symbol: &str, from: i64, to: i64, corr: CorrId) -> R<HttpRequest> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn fills_requests(&self, s: &Settings, symbol: &str, from: i64, to: i64, now: i64, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.fills_requests(s, symbol, from, to, now, corr)
    }
    fn instrument_rules(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> R<HttpRequest> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn instrument_rules_any_status(&self, s: &Settings, symbol: &str, corr: CorrId) -> R<Vec<HttpRequest>> {
        self.0.instrument_rules_any_status(s, symbol, corr)
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
    fn reference_price(&self, s: &Settings, symbol: &str, side: Side, corr: CorrId) -> R<Option<HttpRequest>> {
        self.0.reference_price(s, symbol, side, corr)
    }
    fn decode_reference_price(&self, body: &[u8], side: Side) -> R<Decimal> {
        self.0.decode_reference_price(body, side)
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
    fn decode_rules(&self, body: &[u8]) -> R<Page<VenueRules>> {
        self.0.decode_rules(body)
    }
    fn decode_positions(&self, body: &[u8]) -> R<Vec<VenuePosition>> {
        self.0.decode_positions(body)
    }
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        self.0.status_of(word)
    }
    fn private_socket(&self, s: &Settings) -> Option<PrivateSocket> {
        self.0.private_socket(s)
    }
    fn decode_private(&self, payload: &[u8]) -> R<Vec<PrivateEvent>> {
        self.0.decode_private(payload)
    }
    fn error_table(&self) -> &'static [ErrorRow] {
        self.0.error_table()
    }
}

#[test]
fn the_error_table_is_the_order_paths_and_its_frames_decode() {
    let m = Deribit::new().unwrap();
    assert!(m.error_table().len() > 150);
    let e = m.decode_error(&http_frame(1, "buy", 400, &envelope(11054, "post_only_reject"))).unwrap();
    assert_eq!(e.canon, Canon::Rejected(Refusal::PostOnly));
    let e = m.decode_error(&http_frame(1, "buy", 400, &envelope(13888, "timed_out"))).unwrap();
    assert_eq!(e.canon, Canon::Unknown);
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// Every future and perpetual of the live list decodes to sound rules, and an order call with a
/// key Deribit never issued, or with none, is answered with the auth error the table expects
/// (nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers deribit::trade::tests::public` (add
/// cargo's no-capture flag to see what the venue answered).
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    let m = Deribit::new().unwrap();
    let dv = Driver::new(m.clone(), DriverConfig::new("deribit"), reqwest::Client::new());
    let junk: HashMap<String, String> = [("api_key", "otwjunkkey01"), ("api_secret", "otw-not-a-secret")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let sg = signer(&junk);
    let a = Account { settings: &junk, signer: &sg, scope: "net" };
    let trader: Trader<Deribit> = Trader::new();
    let rules = trader.all_rules(&dv, &a).await.unwrap();
    assert!(rules.len() > 20, "{} futures", rules.len());
    for r in &rules {
        r.sound().unwrap_or_else(|e| panic!("{e}"));
        if r.status == TradingStatus::Trading {
            assert!(r.tick.is_some() && r.lot_step.is_some(), "{} states no tick or step", r.instrument.native);
        }
    }
    let quote = rules.iter().filter(|r| r.size_unit == SizeUnit::Quote).count();
    println!("deribit: {} futures and perpetuals, {quote} sized in USD", rules.len());
    let perp = rules.iter().find(|r| r.instrument.native == INVERSE).unwrap();
    let lin = rules.iter().find(|r| r.instrument.native == LINEAR).unwrap();
    println!("{perp:?}\n{lin:?}");
    assert_eq!((perp.size_unit, lin.size_unit), (SizeUnit::Quote, SizeUnit::Base));
    let o = order(LINEAR, "otwnetprobe", Side::Buy, OrderType::Limit, &lin.min_qty.unwrap().to_string(), Some(&lin.tick.unwrap().to_string()), None);
    let req = m.place(&junk, &o, &placing("otwnetprobe")).unwrap();
    for (what, signed) in [("junk key", true), ("unsigned", false)] {
        let mut r = req.clone();
        r.signed = signed;
        let frame = dv.send_as(r, Some(&sg)).await.unwrap_or_else(|e| panic!("{what}: {e}"));
        let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{what}: Deribit accepted an order with no valid key"));
        println!("{what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
        assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{what}: {e:?}");
    }
}

/// Matching-engine calls (`me:` ids: buy, sell, edit, cancel, cancel all) are paced by their
/// own declared bucket, Tier 4's "5 requests/second, 20 requests (burst)": a cancel all over
/// many instruments waits past the burst instead of drawing `10028`.
#[tokio::test]
async fn matching_engine_calls_are_paced_by_their_own_bucket() {
    use otw_connector_sdk::Limiter;
    let m = Deribit::new().unwrap();
    let s = settings();
    let symbols: Vec<String> = (0..21).map(|i| format!("BTC-{i}")).collect();
    let all = m.cancel_all(&s, &symbols, CorrId(1)).unwrap();
    assert!(all.iter().all(|r| r.endpoint_id.starts_with("me:")));
    let o = order(LINEAR, "otwk1", Side::Buy, OrderType::Limit, "0.0123", Some("80000.5"), None);
    assert!(m.place(&s, &o, &placing("otwk1")).unwrap().endpoint_id.starts_with("me:"));
    let limiter = otw_connector_engine::limiter::TokenBucketLimiter::new(&m.capabilities().rate_limits);
    let start = std::time::Instant::now();
    for r in &all[..20] {
        limiter.acquire(r.endpoint_id, r.weight).await;
    }
    assert!(start.elapsed() < std::time::Duration::from_millis(100), "the burst of 20 goes at once");
    limiter.acquire(all[20].endpoint_id, all[20].weight).await;
    assert!(start.elapsed() >= std::time::Duration::from_millis(150), "the 21st waits a fifth of a second");
}
