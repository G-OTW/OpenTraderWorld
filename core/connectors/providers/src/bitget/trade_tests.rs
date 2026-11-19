//! Bitget orders, from the documentation alone. Fixtures under `fixtures/trade/`, each the
//! doc's "Example Responses" (REST) or "Push Data" (socket) block, copied verbatim (read
//! 2026-11-11). Base `https://www.bitget.com/docs/catalog/`:
//!
//! | File | Source |
//! | --- | --- |
//! | `doc_spot_place_order.json` | `classic-spot-trade/classic-spot-trade` (Place Order) |
//! | `doc_spot_cancel_order.json` | `classic-spot-trade/classic-spot-trade` (Cancel Order) |
//! | `doc_spot_cancel_symbol.json` | `classic-spot-trade/classic-spot-trade` (Cancel Order by Symbol) |
//! | `doc_spot_order_info.json` | `classic-spot-trade/classic-spot-trade` (Get Order Info) |
//! | `doc_spot_unfilled_orders.json` | `classic-spot-trade/classic-spot-trade` (Get Current Orders) |
//! | `doc_spot_history_orders.json` | `classic-spot-trade/classic-spot-trade` (Get History Orders) |
//! | `doc_spot_fills.json` | `classic-spot-trade/classic-spot-trade` (Get Fills) |
//! | `doc_spot_plan_place.json` | `classic-spot-plan/classic-spot-plan` (Place Plan Order) |
//! | `doc_spot_plan_cancel.json` | `classic-spot-plan/classic-spot-plan` (Cancel Plan Order) |
//! | `doc_spot_plan_cancel_batch.json` | `classic-spot-plan/classic-spot-plan` (Cancel Plan Orders in Batch) |
//! | `doc_spot_plan_current.json` | `classic-spot-plan/classic-spot-plan` (Get Current Plan Orders) |
//! | `doc_spot_plan_history.json` | `classic-spot-plan/classic-spot-plan` (Get History Plan Orders) |
//! | `doc_spot_symbols.json` | `classic-spot-market/classic-spot-market` (Get Symbol Info) |
//! | `doc_spot_tickers.json` | `classic-spot-market/classic-spot-market` (Get Ticker Information) |
//! | `doc_mix_place_order.json` | `classic-contract-trade/classic-contract-trade` (Place Order) |
//! | `doc_mix_cancel_order.json` | `classic-contract-trade/classic-contract-trade` (Cancel Order) |
//! | `doc_mix_detail.json` | `classic-contract-trade/classic-contract-trade` (Get Order Detail) |
//! | `doc_mix_orders_pending.json` | `classic-contract-trade/classic-contract-trade` (Get Pending Orders) |
//! | `doc_mix_fills.json` | `classic-contract-trade/classic-contract-trade` (Get Order Fill Details) |
//! | `doc_mix_cancel_all.json` | `classic-contract-trade/classic-contract-trade` (Cancel All Orders) |
//! | `doc_mix_plan_place.json` | `classic-contract-plan/classic-contract-plan` (Place Trigger Order) |
//! | `doc_mix_plan_cancel.json` | `classic-contract-plan/classic-contract-plan` (Cancel Trigger Order) |
//! | `doc_mix_plan_pending.json` | `classic-contract-plan/classic-contract-plan` (Get Pending Trigger Order) |
//! | `doc_mix_plan_history.json` | `classic-contract-plan/classic-contract-plan` (Get History Trigger Order) |
//! | `doc_mix_contracts.json` | `classic-contract-market/classic-contract-market` (Get Contract Config) |
//! | `doc_mix_single_position.json` | `classic-contract-position/classic-contract-position` (Get Single Position) |
//! | `doc_mix_account.json` | `classic-contract-account/classic-contract-account` (Get Single Account) |
//! | `doc_ws_spot_orders.json` | https://www.bitget.com/docs/classic/websocket/spot/private/Order-Channel |
//! | `doc_ws_spot_fill.json` | https://www.bitget.com/docs/classic/websocket/spot/private/Fill-Channel |
//! | `doc_ws_mix_orders.json` | https://www.bitget.com/docs/classic/websocket/contract/private/Order-Channel |
//! | `doc_ws_mix_fill.json` | https://www.bitget.com/docs/classic/websocket/contract/private/Fill-Channel |
//! | `doc_ws_mix_positions.json` | https://www.bitget.com/docs/classic/websocket/contract/private/Positions-Channel |
//!
//! The catalog script ([`Script`]) answers with these examples with only their ids, sizes,
//! prices, symbol, sides and status changed: those answers are `synthetic`, built on the
//! documented shape. Every error body is `synthetic` too: the doc lists codes, not bodies; the
//! envelope is the one every documented answer carries.

#![allow(clippy::unwrap_used, clippy::panic, clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{Capabilities, Command, ConnectorError, CorrId, Decimal, Envelope, InstrumentId, Mapped, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce, TriggerKind};
use otw_connector_sdk::trade::{
    Ack, Canon, CancelAllWay, CancelRow, ClientIdRule, DecodeCtx, ErrorRow, MarginKind, MarginMode, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Refusal, Settings,
    TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, RawFrame, SignableRequest, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript};
use serde_json::{json, Value};

use super::super::signer::BitgetSigner;
use super::super::Bitget;

const SPOT_PLACE: &str = include_str!("fixtures/trade/doc_spot_place_order.json");
const SPOT_CANCEL: &str = include_str!("fixtures/trade/doc_spot_cancel_order.json");
const SPOT_CANCEL_SYMBOL: &str = include_str!("fixtures/trade/doc_spot_cancel_symbol.json");
const SPOT_ORDER_INFO: &str = include_str!("fixtures/trade/doc_spot_order_info.json");
const SPOT_UNFILLED: &str = include_str!("fixtures/trade/doc_spot_unfilled_orders.json");
const SPOT_HISTORY: &str = include_str!("fixtures/trade/doc_spot_history_orders.json");
const SPOT_FILLS: &str = include_str!("fixtures/trade/doc_spot_fills.json");
const SPOT_PLAN_PLACE: &str = include_str!("fixtures/trade/doc_spot_plan_place.json");
const SPOT_PLAN_CANCEL: &str = include_str!("fixtures/trade/doc_spot_plan_cancel.json");
const SPOT_PLAN_BATCH: &str = include_str!("fixtures/trade/doc_spot_plan_cancel_batch.json");
const SPOT_PLAN_CURRENT: &str = include_str!("fixtures/trade/doc_spot_plan_current.json");
const SPOT_PLAN_HISTORY: &str = include_str!("fixtures/trade/doc_spot_plan_history.json");
const SPOT_SYMBOLS: &str = include_str!("fixtures/trade/doc_spot_symbols.json");
const SPOT_TICKERS: &str = include_str!("fixtures/trade/doc_spot_tickers.json");
const MIX_PLACE: &str = include_str!("fixtures/trade/doc_mix_place_order.json");
const MIX_CANCEL: &str = include_str!("fixtures/trade/doc_mix_cancel_order.json");
const MIX_DETAIL: &str = include_str!("fixtures/trade/doc_mix_detail.json");
const MIX_PENDING: &str = include_str!("fixtures/trade/doc_mix_orders_pending.json");
const MIX_FILLS: &str = include_str!("fixtures/trade/doc_mix_fills.json");
const MIX_CANCEL_ALL: &str = include_str!("fixtures/trade/doc_mix_cancel_all.json");
const MIX_PLAN_PLACE: &str = include_str!("fixtures/trade/doc_mix_plan_place.json");
const MIX_PLAN_CANCEL: &str = include_str!("fixtures/trade/doc_mix_plan_cancel.json");
const MIX_PLAN_PENDING: &str = include_str!("fixtures/trade/doc_mix_plan_pending.json");
const MIX_PLAN_HISTORY: &str = include_str!("fixtures/trade/doc_mix_plan_history.json");
const MIX_CONTRACTS: &str = include_str!("fixtures/trade/doc_mix_contracts.json");
const MIX_POSITION: &str = include_str!("fixtures/trade/doc_mix_single_position.json");
const MIX_ACCOUNT: &str = include_str!("fixtures/trade/doc_mix_account.json");
const WS_SPOT_ORDERS: &str = include_str!("fixtures/trade/doc_ws_spot_orders.json");
const WS_SPOT_FILL: &str = include_str!("fixtures/trade/doc_ws_spot_fill.json");
const WS_MIX_ORDERS: &str = include_str!("fixtures/trade/doc_ws_mix_orders.json");
const WS_MIX_FILL: &str = include_str!("fixtures/trade/doc_ws_mix_fill.json");
const WS_MIX_POSITIONS: &str = include_str!("fixtures/trade/doc_ws_mix_positions.json");

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "k-test"), ("api_secret", "secret-test"), ("api_passphrase", "p-test")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn demo() -> HashMap<String, String> {
    let mut s = settings();
    s.insert("demo".into(), "1".into());
    s
}

fn signer() -> BitgetSigner {
    let s = |v: &str| secrecy::SecretString::from(v.to_string());
    BitgetSigner { key: s("k-test"), secret: s("secret-test"), passphrase: s("p-test") }
}

fn order(m: &Bitget, key: &str, side: Side, t: OrderType, qty: &str, px: Option<&str>) -> OrderRequest {
    let (l, s) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new(m.book().venue(), "BTCUSDT"),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: if l { px.map(|p| Price::new(d(p))) } else { None },
        stop: if s { px.map(|p| Price::new(d(p))) } else { None },
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

fn crossed() -> Option<MarginMode> {
    Some(MarginMode { kind: MarginKind::Cross, coin: Some("USDT".into()) })
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

fn frame(endpoint: &'static str, status: u16, body: &str) -> RawFrame {
    otw_connector_testkit::http_frame(1, endpoint, status, body)
}

// ── Requests ──

#[test]
fn a_spot_limit_order_is_the_documented_body() {
    let m = Bitget::spot().unwrap();
    let req = m.place(&settings(), &order(&m, "k1", Side::Buy, OrderType::Limit, "0.01", Some("30000")), &placing("k1", PositionSide::Net)).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.endpoint_id), (Method::Post, "/api/v2/spot/trade/place-order", true, "spot_place"));
    assert_eq!(body(&req), json!({ "symbol": "BTCUSDT", "side": "buy", "orderType": "limit", "force": "gtc", "price": "30000", "size": "0.01", "clientOid": "k1" }));
    assert!(req.headers.iter().all(|(k, _)| k != "paptrading"), "a live account sends no demo header");
    // Post-only through `force`.
    let mut po = order(&m, "k2", Side::Sell, OrderType::Limit, "0.01", Some("31000"));
    po.opts.post_only = true;
    assert_eq!(body(&m.place(&settings(), &po, &placing("k2", PositionSide::Net)).unwrap())["force"], json!("post_only"));
}

#[test]
fn a_spot_market_buy_spends_the_valued_quote_amount_and_a_sell_its_base_size() {
    let m = Bitget::spot().unwrap();
    let buy = order(&m, "mb", Side::Buy, OrderType::Market, "0.01", None);
    assert!(m.market_buy_in_quote(&buy));
    assert!(m.place(&settings(), &buy, &placing("mb", PositionSide::Net)).is_err(), "never sent unvalued");
    let p = Placing { quote_amount: Some(d("300.005")), ..placing("mb", PositionSide::Net) };
    let b = body(&m.place(&settings(), &buy, &p).unwrap());
    assert_eq!((b["size"].as_str(), b["orderType"].as_str(), b.get("price").is_none()), (Some("300.005"), Some("market"), true));
    let sell = order(&m, "ms", Side::Sell, OrderType::Market, "0.01", None);
    assert!(!m.market_buy_in_quote(&sell));
    assert_eq!(body(&m.place(&settings(), &sell, &placing("ms", PositionSide::Net)).unwrap())["size"], json!("0.01"));
    // Futures market orders are sized in the base coin.
    let f = Bitget::usdt_futures().unwrap();
    assert!(!f.market_buy_in_quote(&order(&f, "fm", Side::Buy, OrderType::Market, "0.01", None)));
}

#[test]
fn a_spot_stop_is_a_plan_order_in_the_base_coin_and_spot_takes_no_reduce_only() {
    let m = Bitget::spot().unwrap();
    let req = m.place(&settings(), &order(&m, "s1", Side::Buy, OrderType::Stop, "0.01", Some("31000")), &placing("s1", PositionSide::Net)).unwrap();
    assert_eq!((req.path.as_str(), req.endpoint_id), ("/api/v2/spot/trade/place-plan-order", "spot_plan_place"));
    assert_eq!(body(&req), json!({ "symbol": "BTCUSDT", "side": "buy", "orderType": "market", "triggerPrice": "31000", "triggerType": "fill_price", "planType": "amount", "size": "0.01", "clientOid": "s1" }));
    assert!(m.place(&settings(), &order(&m, "x", Side::Buy, OrderType::StopLimit, "0.01", Some("31000")), &placing("x", PositionSide::Net)).is_err());
    let mut ro = order(&m, "r", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    ro.opts.reduce_only = true;
    assert!(m.place(&settings(), &ro, &placing("r", PositionSide::Net)).is_err());
}

#[test]
fn a_futures_order_carries_its_margin_and_its_position_in_either_mode() {
    let m = Bitget::usdt_futures().unwrap();
    let p = |pos| Placing { margin: crossed(), ..placing("f1", pos) };
    // One-way: the trade's side; reduce-only spelt `YES`.
    let mut close = order(&m, "f1", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    close.opts.reduce_only = true;
    let b = body(&m.place(&settings(), &close, &p(PositionSide::Net)).unwrap());
    assert_eq!(
        b,
        json!({ "symbol": "BTCUSDT", "productType": "USDT-FUTURES", "marginMode": "crossed", "marginCoin": "USDT", "size": "0.01", "orderType": "limit", "price": "30000", "force": "gtc", "side": "sell", "reduceOnly": "YES", "clientOid": "f1" })
    );
    // Hedge: "Close long: side=buy, tradeSide=close"; "Open short: side=sell, tradeSide=open".
    let b = body(&m.place(&settings(), &close, &p(PositionSide::Long)).unwrap());
    assert_eq!((b["side"].as_str(), b["tradeSide"].as_str(), b.get("reduceOnly").is_none()), (Some("buy"), Some("close"), true));
    let open = order(&m, "f2", Side::Sell, OrderType::Market, "0.01", None);
    let b = body(&m.place(&settings(), &open, &p(PositionSide::Short)).unwrap());
    assert_eq!((b["side"].as_str(), b["tradeSide"].as_str(), b.get("force").is_none(), b.get("price").is_none()), (Some("sell"), Some("open"), true, true));
    // Isolated, as the account says.
    let iso = Placing { margin: Some(MarginMode { kind: MarginKind::Isolated, coin: Some("USDT".into()) }), ..placing("f3", PositionSide::Net) };
    assert_eq!(body(&m.place(&settings(), &open, &iso).unwrap())["marginMode"], json!("isolated"));
}

#[test]
fn a_futures_stop_is_a_trigger_order_on_the_price_asked() {
    let m = Bitget::usdt_futures().unwrap();
    let mut sl = order(&m, "t1", Side::Sell, OrderType::Stop, "0.01", Some("29000"));
    sl.opts.reduce_only = true;
    let req = m.place(&settings(), &sl, &Placing { margin: crossed(), ..placing("t1", PositionSide::Net) }).unwrap();
    assert_eq!((req.path.as_str(), req.endpoint_id), ("/api/v2/mix/order/place-plan-order", "mix_plan_place"));
    assert_eq!(
        body(&req),
        json!({ "symbol": "BTCUSDT", "productType": "USDT-FUTURES", "marginMode": "crossed", "marginCoin": "USDT", "size": "0.01", "orderType": "market", "planType": "normal_plan", "triggerPrice": "29000", "triggerType": "fill_price", "side": "sell", "reduceOnly": "yes", "clientOid": "t1" })
    );
    let mut sll = OrderRequest { order_type: OrderType::StopLimit, limit: Some(Price::new(d("28900"))), ..sl.clone() };
    sll.opts.trigger = Some(TriggerKind::Mark);
    let b = body(&m.place(&settings(), &sll, &placing("t2", PositionSide::Long)).unwrap());
    assert_eq!((b["orderType"].as_str(), b["price"].as_str(), b["triggerType"].as_str(), b["tradeSide"].as_str()), (Some("limit"), Some("28900"), Some("mark_price"), Some("close")));
}

#[test]
fn a_demo_account_sends_the_paptrading_header_and_is_the_sandbox() {
    let m = Bitget::usdt_futures().unwrap();
    assert!(m.is_sandbox(&demo()) && !m.is_sandbox(&settings()));
    let req = m.cancel(&demo(), "BTCUSDT", "1", CorrId(1)).unwrap();
    assert!(req.headers.contains(&("paptrading".to_string(), "1".to_string())));
    assert!(m.instrument_rules(&demo(), None, CorrId(1)).unwrap().headers.contains(&("paptrading".to_string(), "1".to_string())));
    assert_eq!(m.private_socket(&demo()).unwrap().base, "ws-demo");
    assert_eq!(m.private_socket(&settings()).unwrap().base, "ws");
}

#[test]
fn a_cancel_goes_to_the_space_its_id_names() {
    let s = Bitget::spot().unwrap();
    let plain = s.cancel(&settings(), "BTCUSDT", "77", CorrId(1)).unwrap();
    assert_eq!((plain.path.as_str(), body(&plain)), ("/api/v2/spot/trade/cancel-order", json!({ "symbol": "BTCUSDT", "orderId": "77" })));
    let cond = s.cancel(&settings(), "BTCUSDT", "cond:77", CorrId(1)).unwrap();
    assert_eq!((cond.path.as_str(), body(&cond)), ("/api/v2/spot/trade/cancel-plan-order", json!({ "orderId": "77" })));
    let f = Bitget::usdt_futures().unwrap();
    let cond = f.cancel(&settings(), "BTCUSDT", "cond:9", CorrId(1)).unwrap();
    assert_eq!(
        (cond.path.as_str(), body(&cond)),
        ("/api/v2/mix/order/cancel-plan-order", json!({ "orderIdList": [{ "orderId": "9" }], "symbol": "BTCUSDT", "productType": "USDT-FUTURES", "marginCoin": "USDT", "planType": "normal_plan" }))
    );
    assert_eq!(f.cancel(&settings(), "BTCUSDT", "9", CorrId(1)).unwrap().path, "/api/v2/mix/order/cancel-order");
    // Looked up in its space too.
    assert_eq!(f.query_by_venue_id(&settings(), "BTCUSDT", "cond:9", CorrId(1)).unwrap()[0].path, "/api/v2/mix/order/orders-plan-pending");
    assert_eq!(s.query_by_venue_id(&settings(), "BTCUSDT", "77", CorrId(1)).unwrap()[0].path, "/api/v2/spot/trade/orderInfo");
}

#[test]
fn cancel_all_covers_plain_and_plan_orders_per_book() {
    let s = Bitget::spot().unwrap();
    assert_eq!(s.cancel_all_way(), CancelAllWay::BySymbol);
    let reqs = s.cancel_all(&settings(), &["BTCUSDT".into(), "ETHUSDT".into()], CorrId(1)).unwrap();
    let paths: Vec<&str> = reqs.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, vec!["/api/v2/spot/trade/cancel-symbol-order", "/api/v2/spot/trade/cancel-symbol-order", "/api/v2/spot/trade/batch-cancel-plan-order"]);
    assert_eq!(body(&reqs[2]), json!({ "symbolList": ["BTCUSDT", "ETHUSDT"] }));
    let f = Bitget::usdt_futures().unwrap();
    assert_eq!(f.cancel_all_way(), CancelAllWay::Book);
    let reqs = f.cancel_all(&settings(), &[], CorrId(1)).unwrap();
    assert_eq!(body(&reqs[0]), json!({ "productType": "USDT-FUTURES", "marginCoin": "USDT" }));
    assert_eq!(body(&reqs[1]), json!({ "productType": "USDT-FUTURES", "marginCoin": "USDT", "planType": "normal_plan" }));
    // Every plan type the Cancel Trigger Order enum lists, but the position's own exits.
    let plans: Vec<Value> = reqs[1..].iter().map(|r| body(r)["planType"].clone()).collect();
    assert_eq!(plans, vec![json!("normal_plan"), json!("profit_plan"), json!("loss_plan"), json!("moving_plan")]);
    assert!(reqs[1..].iter().all(|r| r.path == "/api/v2/mix/order/cancel-plan-order"));
    let rows = f.decode_cancel_rows(&DecodeCtx::new(reqs[2].endpoint_id, &Default::default()), MIX_PLAN_CANCEL.as_bytes()).unwrap();
    assert_eq!(rows[0].venue_id, "cond:profit_plan:3", "a row left names its plan type, as the lists do");
    // The rows a cancel-all left, each with its venue's words.
    let rows = f.decode_cancel_rows(&DecodeCtx::new("mix_cancel_all", &Default::default()), MIX_CANCEL_ALL.as_bytes()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].venue_id, "232");
    let rows = f.decode_cancel_rows(&DecodeCtx::new("mix_plan_cancel_all", &Default::default()), MIX_PLAN_CANCEL.as_bytes()).unwrap();
    assert_eq!(rows.iter().map(|r| r.venue_id.as_str()).collect::<Vec<_>>(), vec!["cond:3", "cond:21221"]);
    let rows = s.decode_cancel_rows(&DecodeCtx::new("spot_plan_cancel_batch", &Default::default()), SPOT_PLAN_BATCH.as_bytes()).unwrap();
    assert_eq!(rows[0].venue_id, "cond:121211212122");
    assert!(s.decode_cancel_rows(&DecodeCtx::new("spot_cancel_symbol", &Default::default()), SPOT_CANCEL_SYMBOL.as_bytes()).unwrap().is_empty());
}

/// One id per plan order across the account read, the order lists, the cancel and the look-up:
/// `cond:<planType>:<orderId>` for a futures plan type other than `normal_plan`.
#[test]
fn a_futures_plan_order_has_one_id_everywhere() {
    let f = Bitget::usdt_futures().unwrap();
    let sizes = Default::default();
    let ctx = |e| DecodeCtx::new(e, &sizes);
    let typed = |t: &str| with(MIX_PLAN_PENDING, |v| v["data"]["entrustedList"][0]["planType"] = json!(t));
    for (t, want) in [("normal_plan", "cond:123"), ("track_plan", "cond:track_plan:123"), ("loss_plan", "cond:loss_plan:123"), ("moving_plan", "cond:moving_plan:123")] {
        let (rows, _) = f.decode_orders_in(&ctx("mix_plan_pending"), typed(t).as_bytes()).unwrap();
        assert_eq!(rows[0].venue_id, want, "{t}");
        let (rows, _) = f.decode_orders_in(&ctx("mix_plan_history"), with(MIX_PLAN_HISTORY, |v| v["data"]["entrustedList"][0]["planType"] = json!(t)).as_bytes()).unwrap();
        assert!(rows[0].venue_id.starts_with(&want[..want.len() - 3]), "{t}: {}", rows[0].venue_id);
    }
    // The look-up asks the list that holds it, by its own id, and the row it reads matches.
    let q = f.query_by_venue_id(&settings(), "ETHUSDT", "cond:loss_plan:123", CorrId(1)).unwrap();
    assert!(q[0].query.contains(&("planType".into(), "profit_loss".into())) && q[0].query.contains(&("orderId".into(), "123".into())));
    let (rows, _) = f.decode_orders_in(&ctx(q[0].endpoint_id), typed("loss_plan").as_bytes()).unwrap();
    assert!(rows.iter().any(|o| o.venue_id == "cond:loss_plan:123"));
    // Its cancel names the plan type.
    let c = f.cancel(&settings(), "ETHUSDT", "cond:loss_plan:123", CorrId(1)).unwrap();
    assert_eq!((body(&c)["planType"].clone(), body(&c)["orderIdList"][0]["orderId"].clone()), (json!("loss_plan"), json!("123")));
    // The open-orders read of the take profit and stop loss list leaves the position's own.
    let open = f.open_orders(&settings(), None, CorrId(1)).unwrap();
    let exits = open.iter().find(|r| r.query.contains(&("planType".into(), "profit_loss".into()))).unwrap();
    assert!(f.decode_orders_in(&ctx(exits.endpoint_id), typed("pos_loss").as_bytes()).unwrap().0.is_empty());
    assert_eq!(f.decode_orders_in(&ctx(exits.endpoint_id), typed("loss_plan").as_bytes()).unwrap().0.len(), 1);
    // A position exit read for a change is named by its venue id alone in the change.
    let (rows, _) = f.decode_orders_in(&ctx("mix_plan_tpsl"), typed("pos_loss").as_bytes()).unwrap();
    assert_eq!(rows[0].venue_id, "cond:pos_loss:123");
    let x = otw_connector_sdk::trade::ExitChange {
        symbol: "ETHUSDT",
        position: PositionSide::Long,
        entry_side: Side::Buy,
        kind: otw_connector_sdk::trade::ExitKind::StopLoss,
        price: Some(d("1800")),
        current: Some(&rows[0]),
    };
    assert_eq!(body(&f.change_exit(&settings(), &x, CorrId(1)).unwrap())["orderId"], json!("123"));
}

/// The spot `orders` channel's table names the order type `ordType` (its example
/// `orderType`): either is read.
#[test]
fn the_spot_order_push_reads_ord_type_too() {
    let m = Bitget::spot().unwrap();
    let renamed = with(WS_SPOT_ORDERS, |v| {
        let row = v["data"][0].as_object_mut().unwrap();
        let t = row.remove("orderType").unwrap();
        row.insert("ordType".into(), t);
    });
    let a = m.decode_private(WS_SPOT_ORDERS.as_bytes()).unwrap();
    let b = m.decode_private(renamed.as_bytes()).unwrap();
    let ty = |e: &[PrivateEvent]| match &e[0] {
        PrivateEvent::Order(o) => o.order_type.clone(),
        _ => panic!(),
    };
    assert!(!ty(&a).is_empty());
    assert_eq!(ty(&a), ty(&b));
}

#[test]
fn lookups_and_lists_name_the_documented_calls() {
    let s = Bitget::spot().unwrap();
    let q = s.query_by_client_id(&settings(), "BTCUSDT", "k1", CorrId(1)).unwrap();
    assert_eq!(q.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), vec!["/api/v2/spot/trade/orderInfo", "/api/v2/spot/trade/current-plan-order", "/api/v2/spot/trade/history-plan-order"]);
    assert!(q[0].query.contains(&("clientOid".into(), "k1".into())));
    let f = Bitget::usdt_futures().unwrap();
    let q = f.query_by_client_id(&settings(), "BTCUSDT", "k1", CorrId(1)).unwrap();
    assert_eq!(q.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), vec!["/api/v2/mix/order/detail", "/api/v2/mix/order/orders-plan-pending", "/api/v2/mix/order/orders-plan-history"]);
    assert!(q.iter().all(|r| r.query.contains(&("clientOid".into(), "k1".into())) && r.query.contains(&("productType".into(), "USDT-FUTURES".into()))));
    let open = f.open_orders(&settings(), None, CorrId(1)).unwrap();
    assert_eq!(open.len(), 5);
    assert!(open[1].query.contains(&("status".into(), "partially_filled".into())));
    let plans: Vec<_> = open[2..].iter().map(|r| r.query.iter().find(|(k, _)| k == "planType").unwrap().1.clone()).collect();
    assert_eq!(plans, vec!["normal_plan", "track_plan", "profit_loss"]);
    let fills = f.fills_since(&settings(), "BTCUSDT", 1, 2, CorrId(1)).unwrap();
    assert_eq!(fills.path, "/api/v2/mix/order/fills");
    assert_eq!((s.fills_window_ms(), f.fills_window_ms()), (Some(90 * 86_400_000), Some(89 * 86_400_000)));
    let next = f.with_cursor(&fills, "123");
    assert!(next.query.contains(&("idLessThan".into(), "123".into())));
}

#[test]
fn the_client_id_rule_is_the_faqs() {
    let m = Bitget::spot().unwrap();
    assert_eq!(m.client_id("otw-1_a+b#c").unwrap(), "otw-1_a+b#c");
    assert!(m.client_id("a:b").is_err());
    assert!(m.client_id(&"a".repeat(50)).is_err());
    assert!(m.client_id(&"a".repeat(49)).is_ok());
}

#[test]
fn a_market_fok_and_a_stop_with_a_time_in_force_are_refused_before_io() {
    for m in [Bitget::spot().unwrap(), Bitget::usdt_futures().unwrap()] {
        let mut o = order(&m, "f", Side::Buy, OrderType::Market, "0.01", None);
        o.tif = TimeInForce::Fok;
        assert_eq!(m.check_order(&o).unwrap_err().kind, otw_connector_model::ErrorKind::Unsupported);
        o.tif = TimeInForce::Ioc;
        assert!(m.check_order(&o).is_ok());
        let mut stop = order(&m, "s", Side::Buy, OrderType::Stop, "0.01", Some("31000"));
        stop.tif = TimeInForce::Ioc;
        assert!(m.check_order(&stop).is_err());
        let mut limit = order(&m, "l", Side::Buy, OrderType::Limit, "0.01", Some("31000"));
        limit.tif = TimeInForce::Fok;
        assert!(m.check_order(&limit).is_ok());
    }
}

/// The REST signature over the exact bytes an order sends: `timestamp + METHOD + path + body`
/// (Signature page; its worked example gives the plain text only, checked in `signer.rs`),
/// signed by an independent HMAC-SHA256 (Python's `hmac` module, computed once and pinned).
#[test]
fn an_order_body_signs_like_an_independent_implementation() {
    let m = Bitget::usdt_futures().unwrap();
    let mut o = order(&m, "f1", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    o.opts.reduce_only = true;
    let req = m.place(&settings(), &o, &Placing { margin: crossed(), ..placing("f1", PositionSide::Net) }).unwrap();
    let sent = String::from_utf8(req.body.clone().unwrap().to_vec()).unwrap();
    assert_eq!(sent, SIGNED_BODY);
    let mut s = SignableRequest { method: Method::Post, path: req.path.clone(), query: vec![], headers: vec![], body: req.body.clone() };
    signer().sign(&mut s, otw_connector_model::Timestamp::from_millis(1_695_806_875_837).unwrap()).unwrap();
    let h = |k: &str| s.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(h("ACCESS-SIGN").as_deref(), Some(SIGNED_BODY_HMAC));
    assert_eq!(h("ACCESS-TIMESTAMP").as_deref(), Some("1695806875837"));
}

const SIGNED_BODY: &str = r#"{"clientOid":"f1","force":"gtc","marginCoin":"USDT","marginMode":"crossed","orderType":"limit","price":"30000","productType":"USDT-FUTURES","reduceOnly":"YES","side":"sell","size":"0.01","symbol":"BTCUSDT"}"#;
const SIGNED_BODY_HMAC: &str = "DSyaj2YR/UvU7/lK+Bx9V1S09qnJL5c8m/PoHbhXW9g=";

/// The socket login: base64 HMAC-SHA256 of `timestamp + "GET" + "/user/verify"` (websocket
/// intro, "Login"). The page's example signature comes with no secret, so it cannot be redone:
/// checked against an independent HMAC (Python), as the REST one.
#[test]
fn the_socket_login_signs_like_an_independent_implementation() {
    let frame = signer().ws_login(otw_connector_model::Timestamp::from_millis(1_538_054_050_000).unwrap()).unwrap().unwrap();
    let v: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(v["op"], json!("login"));
    let a = &v["args"][0];
    assert_eq!((a["apiKey"].as_str(), a["passphrase"].as_str(), a["timestamp"].as_str()), (Some("k-test"), Some("p-test"), Some("1538054050000")));
    assert_eq!(a["sign"].as_str(), Some(WS_LOGIN_HMAC));
}

const WS_LOGIN_HMAC: &str = "6oieX5tTTiTAXYS4c70tK8J6rQ9gifD4olFAIGX1OYk=";

// ── Answers ──

#[test]
fn documented_acks_decode_and_a_plan_ack_is_tagged() {
    let s = Bitget::spot().unwrap();
    let ack = s.decode_ack(SPOT_PLACE.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("1001", Some("121211212122")));
    let sizes = Default::default();
    let plan = s.decode_ack_in(&DecodeCtx::new("spot_plan_place", &sizes), SPOT_PLAN_PLACE.as_bytes()).unwrap();
    assert_eq!(plan.venue_id, "cond:121211212122");
    let f = Bitget::usdt_futures().unwrap();
    assert_eq!(f.decode_ack(MIX_PLACE.as_bytes()).unwrap().venue_id, "121211212122");
    assert_eq!(f.decode_ack_in(&DecodeCtx::new("mix_plan_place", &sizes), MIX_PLAN_PLACE.as_bytes()).unwrap().venue_id, "cond:121212121212");
    // A one-way reduce-only order that made Bitget cancel older ones: no orderId (synthetic).
    let bare = with(MIX_PLACE, |v| v["data"]["orderId"] = Value::Null);
    let ack = f.decode_ack(bare.as_bytes()).unwrap();
    assert!(!ack.has_id());
    assert_eq!(ack.client_id.as_deref(), Some("121211212122"));
    for ok in [SPOT_CANCEL, SPOT_CANCEL_SYMBOL, SPOT_PLAN_CANCEL, MIX_CANCEL] {
        assert!(f.decode_error(&frame("x", 200, ok)).is_none());
    }
}

#[test]
fn documented_spot_orders_decode() {
    let m = Bitget::spot().unwrap();
    let sizes = otw_connector_sdk::trade::ContractSizes::new();
    let ctx = |e| DecodeCtx::new(e, &sizes);
    // A market buy: its `size` is quote spent, its quantity the base it bought.
    let (info, next) = m.decode_orders_in(&ctx("spot_order_info"), SPOT_ORDER_INFO.as_bytes()).unwrap();
    let o = &info[0];
    assert_eq!(
        (o.venue_id.as_str(), o.client_id.as_deref(), o.status.clone(), o.qty, o.filled_qty, o.limit, o.avg_price, o.side.clone()),
        ("121211212122", Some("121211212122"), Mapped::Known(OrderStatus::Filled), d("0.0007"), d("0.0007"), None, Some(d("13000")), Mapped::Known(Side::Buy))
    );
    assert!(next.is_none());
    // Get Current Orders: `priceAvg` is the order's price; its example writes `new`.
    let (open, _) = m.decode_orders_in(&ctx("spot_unfilled"), SPOT_UNFILLED.as_bytes()).unwrap();
    let o = &open[0];
    assert_eq!((o.symbol.as_str(), o.status.clone(), o.qty, o.limit, o.avg_price), ("BTCUSDT", Mapped::Known(OrderStatus::New), d("1"), Some(d("34829.12")), None));
    let (hist, _) = m.decode_orders_in(&ctx("spot_history"), SPOT_HISTORY.as_bytes()).unwrap();
    assert_eq!((hist[0].status.clone(), hist[0].filled_qty), (Mapped::Known(OrderStatus::Filled), d("0.0125")));
    // Plan orders: tagged, waiting for their trigger.
    let (plans, _) = m.decode_orders_in(&ctx("spot_plan_current"), SPOT_PLAN_CURRENT.as_bytes()).unwrap();
    let p = &plans[0];
    assert_eq!(
        (p.venue_id.as_str(), p.status.clone(), p.trigger_pending, p.stop, p.limit, p.qty),
        ("cond:121211212122", Mapped::Known(OrderStatus::New), true, Some(d("0.041572")), Some(d("0.041572")), d("151"))
    );
    let executed = with(SPOT_PLAN_HISTORY, |v| v["data"]["orderList"][0]["status"] = json!("executed"));
    let (h, _) = m.decode_orders_in(&ctx("spot_plan_history"), executed.as_bytes()).unwrap();
    assert_eq!((h[0].status.clone(), h[0].filled_qty, h[0].trigger_pending), (Mapped::Known(OrderStatus::Filled), d("151"), false));
}

#[test]
fn documented_futures_orders_decode() {
    let m = Bitget::usdt_futures().unwrap();
    let sizes = Default::default();
    let ctx = |e| DecodeCtx::new(e, &sizes);
    // The detail is one object.
    let (det, _) = m.decode_orders_in(&ctx("mix_detail"), MIX_DETAIL.as_bytes()).unwrap();
    let o = &det[0];
    assert_eq!(
        (o.venue_id.as_str(), o.client_id.as_deref(), o.symbol.as_str(), o.status.clone(), o.qty, o.filled_qty, o.limit, o.reduce_only),
        ("123456", Some("77777"), "ETHUSDT", Mapped::Known(OrderStatus::Filled), d("2"), d("2"), Some(d("1900")), true)
    );
    // Hedge mode: `posSide` long opened.
    let (pend, next) = m.decode_orders_in(&ctx("mix_pending"), MIX_PENDING.as_bytes()).unwrap();
    let o = &pend[0];
    assert_eq!((o.status.clone(), o.position, o.side.clone(), o.filled_qty, o.reduce_only), (Mapped::Known(OrderStatus::PartiallyFilled), PositionSide::Long, Mapped::Known(Side::Buy), d("12.1"), false));
    assert!(next.is_none(), "one row, under a full page");
    // A hedge close of the long is a sell (synthetic: the same row closing).
    let closing = with(MIX_PENDING, |v| v["data"]["entrustedList"][0]["tradeSide"] = json!("close"));
    let o = &m.decode_orders_in(&ctx("mix_pending"), closing.as_bytes()).unwrap().0[0];
    assert_eq!((o.side.clone(), o.reduce_only), (Mapped::Known(Side::Sell), true));
    let (plans, _) = m.decode_orders_in(&ctx("mix_plan_pending"), MIX_PLAN_PENDING.as_bytes()).unwrap();
    let p = &plans[0];
    assert_eq!((p.venue_id.as_str(), p.trigger_pending, p.stop, p.limit, p.status.clone()), ("cond:123", true, Some(d("1901")), Some(d("1900")), Mapped::Known(OrderStatus::New)));
    // Executed with all of it traded: filled. With none traded yet: its order still works.
    let (h, _) = m.decode_orders_in(&ctx("mix_plan_history"), MIX_PLAN_HISTORY.as_bytes()).unwrap();
    assert_eq!((h[0].status.clone(), h[0].filled_qty, h[0].avg_price), (Mapped::Known(OrderStatus::Filled), d("1"), Some(d("1999.2"))));
    let resting = with(MIX_PLAN_HISTORY, |v| v["data"]["entrustedList"][0]["baseVolume"] = json!("0"));
    assert_eq!(m.decode_orders_in(&ctx("mix_plan_history"), resting.as_bytes()).unwrap().0[0].status, Mapped::Known(OrderStatus::New));
    // `data: null` (an order the detail does not know) is no order.
    assert!(m.decode_orders_in(&ctx("mix_detail"), br#"{"code":"00000","data":null}"#).unwrap().0.is_empty());
}

#[test]
fn documented_fills_decode_with_their_fee_as_a_cost() {
    let s = Bitget::spot().unwrap();
    let (f, _) = s.decode_fills(SPOT_FILLS.as_bytes()).unwrap();
    assert_eq!(
        (f[0].exec_id.as_str(), f[0].venue_order_id.as_str(), f[0].qty, f[0].price, f[0].fee, f[0].fee_currency.as_str(), f[0].at_ms),
        ("12345678910", "12345678910", d("0.0007"), d("13000"), d("0.0000007"), "BTC", 1_695_865_232_579)
    );
    let m = Bitget::usdt_futures().unwrap();
    let (f, _) = m.decode_fills(MIX_FILLS.as_bytes()).unwrap();
    assert_eq!((f[0].exec_id.as_str(), f[0].qty, f[0].price, f[0].fee, f[0].fee_currency.as_str(), f[0].symbol.as_str()), ("123", d("1"), d("1900"), d("0.017118519726"), "BGB", "ETHUSDT"));
    assert!(!f[0].position_only);
    // A liquidation moves the position with no order of the account's behind it.
    let burst = with(MIX_FILLS, |v| v["data"]["fillList"][0]["tradeSide"] = json!("burst_close_long"));
    assert!(m.decode_fills(burst.as_bytes()).unwrap().0[0].position_only);
}

#[test]
fn documented_instrument_lists_decode_to_sound_rules() {
    let (spot, _) = Bitget::spot().unwrap().decode_rules(SPOT_SYMBOLS.as_bytes()).unwrap();
    let r = &spot[0];
    assert_eq!(
        (r.tick, r.lot_step, r.quote_step, r.min_notional, r.max_notional, r.min_qty, r.status.clone()),
        (Some(d("0.01")), Some(d("0.000001")), Some(d("0.00000001")), Some(d("1")), Some(d("500000")), None, TradingStatus::Trading)
    );
    r.sound().unwrap();
    let (fut, _) = Bitget::usdt_futures().unwrap().decode_rules(MIX_CONTRACTS.as_bytes()).unwrap();
    let r = &fut[0];
    assert_eq!(
        (r.tick, r.lot_step, r.min_qty, r.max_qty, r.market_max_qty, r.min_notional, r.status.clone()),
        (Some(d("0.1")), Some(d("0.01")), Some(d("0.01")), Some(d("1200")), Some(d("220")), Some(d("5")), TradingStatus::Trading)
    );
    r.sound().unwrap();
    // `priceEndStep` counts in the last decimal place: 5 at one place is a 0.5 tick.
    let five = with(MIX_CONTRACTS, |v| v["data"][0]["priceEndStep"] = json!("5"));
    assert_eq!(Bitget::usdt_futures().unwrap().decode_rules(five.as_bytes()).unwrap().0[0].tick, Some(d("0.5")));
    // Status words.
    let st = |w: &str| Bitget::usdt_futures().unwrap().decode_rules(with(MIX_CONTRACTS, |v| v["data"][0]["symbolStatus"] = json!(w)).as_bytes()).unwrap().0[0].status.clone();
    assert_eq!(st("limit_open"), TradingStatus::CloseOnly);
    assert_eq!(st("maintain"), TradingStatus::Closed("maintain".into()));
    let gray = Bitget::spot().unwrap().decode_rules(with(SPOT_SYMBOLS, |v| v["data"][0]["status"] = json!("gray")).as_bytes()).unwrap();
    assert_eq!(gray.0[0].status, TradingStatus::Closed("gray".into()));
    // A pair quoted in another coin: the USDT bounds do not apply.
    let eth = Bitget::spot().unwrap().decode_rules(with(SPOT_SYMBOLS, |v| v["data"][0]["quoteCoin"] = json!("BTC")).as_bytes()).unwrap();
    assert_eq!((eth.0[0].min_notional, eth.0[0].max_notional), (None, None));
}

#[test]
fn positions_margin_and_mode_decode_from_the_documented_reads() {
    let m = Bitget::usdt_futures().unwrap();
    let p = m.decode_positions(MIX_POSITION.as_bytes()).unwrap();
    assert_eq!((p[0].side, p[0].direction, p[0].qty, p[0].leverage), (PositionSide::Short, Some(Side::Sell), d("0.005"), Some(d("14"))));
    assert_eq!(p[0].margin, crossed());
    let one_way = with(MIX_POSITION, |v| v["data"][0]["posMode"] = json!("one_way_mode"));
    let p = m.decode_positions(one_way.as_bytes()).unwrap();
    assert_eq!((p[0].side, p[0].direction), (PositionSide::Net, Some(Side::Sell)));
    assert_eq!(PositionMode::of(&p), Some(PositionMode::OneWay));
    assert_eq!(m.decode_account_position_mode(MIX_ACCOUNT.as_bytes()).unwrap(), Some(PositionMode::Hedge));
    assert_eq!(m.decode_symbol_margin("BTCUSDT", MIX_ACCOUNT.as_bytes()).unwrap(), crossed());
    let none = with(MIX_ACCOUNT, |v| v["data"]["marginMode"] = json!(""));
    assert!(m.decode_symbol_margin("BTCUSDT", none.as_bytes()).is_err(), "never a guessed margin mode");
    let req = m.account_position_mode(&settings(), CorrId(1)).unwrap().unwrap();
    assert_eq!(req.path, "/api/v2/mix/account/account");
    assert!(Bitget::spot().unwrap().account_position_mode(&settings(), CorrId(1)).unwrap().is_none());
}

#[test]
fn the_reference_price_is_the_best_ask_of_the_ticker() {
    let m = Bitget::spot().unwrap();
    let req = m.reference_price(&settings(), "BTCUSDT", Side::Buy, CorrId(1)).unwrap().unwrap();
    assert_eq!((req.path.as_str(), req.signed), ("/api/v2/spot/market/tickers", false));
    // The documented example's ask is "0": the trader refuses a price that is not above zero.
    assert_eq!(m.decode_reference_price(SPOT_TICKERS.as_bytes(), Side::Buy).unwrap(), Decimal::ZERO);
    let quoted = with(SPOT_TICKERS, |v| v["data"][0]["askPr"] = json!("34413.5"));
    assert_eq!(m.decode_reference_price(quoted.as_bytes(), Side::Buy).unwrap(), d("34413.5"));
}

#[test]
fn every_status_word_maps_and_an_unknown_one_stays_unknown() {
    let m = Bitget::spot().unwrap();
    for (w, s) in [
        ("live", OrderStatus::New),
        ("new", OrderStatus::New),
        ("not_trigger", OrderStatus::New),
        ("executing", OrderStatus::New),
        ("partially_filled", OrderStatus::PartiallyFilled),
        ("filled", OrderStatus::Filled),
        ("executed", OrderStatus::Filled),
        ("cancelled", OrderStatus::Cancelled),
        ("canceled", OrderStatus::Cancelled),
        ("fail_execute", OrderStatus::Cancelled),
    ] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    assert_eq!(m.status_of("rejected"), Mapped::Unknown("rejected".into()));
}

#[test]
fn the_private_stream_maps_the_documented_frames() {
    let s = Bitget::spot().unwrap();
    let ev = s.decode_private(WS_SPOT_ORDERS.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    // A market buy: what it bought so far is its quantity.
    assert_eq!((o.venue_id.as_str(), o.status.clone(), o.filled_qty, o.qty), ("1", Mapped::Known(OrderStatus::PartiallyFilled), d("0.0003"), d("0.0003")));
    let ev = s.decode_private(WS_SPOT_FILL.as_bytes()).unwrap();
    assert_eq!(ev.len(), 2);
    let PrivateEvent::Fill(f) = &ev[1] else { panic!("{ev:?}") };
    assert_eq!((f.exec_id.as_str(), f.qty, f.price, f.fee, f.fee_currency.as_str()), ("222", d("0.0006"), d("42741.46"), d("0.01538693"), "USDT"));
    // Futures frames are not the spot book's.
    assert!(s.decode_private(WS_MIX_ORDERS.as_bytes()).unwrap().is_empty());
    let m = Bitget::usdt_futures().unwrap();
    let ev = m.decode_private(WS_MIX_ORDERS.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.client_id.as_deref(), o.position, o.side.clone(), o.qty, o.limit, o.status.clone()), (Some("12354678990111"), PositionSide::Long, Mapped::Known(Side::Buy), d("0.4"), Some(d("3000")), Mapped::Known(OrderStatus::New)));
    let ev = m.decode_private(WS_MIX_FILL.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.client_id.as_deref(), f.qty, f.price, f.fee), (Some("111"), d("0.01"), d("51000.5"), d("0.183717")));
    let ev = m.decode_private(WS_MIX_POSITIONS.as_bytes()).unwrap();
    let PrivateEvent::Position(p) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((p.symbol.as_str(), p.side, p.qty, p.leverage), ("ETHUSDT", PositionSide::Short, d("0.1"), Some(d("20"))));
    // Answers: login, subscribe, an error, and the `pong` string.
    let answer = |t: &str| match &m.decode_private(t.as_bytes()).unwrap()[0] {
        PrivateEvent::Answer { op, ok, .. } => (op.clone(), *ok),
        other => panic!("{other:?}"),
    };
    assert_eq!(answer(r#"{"event":"login","code":"0","msg":""}"#), ("auth".into(), true));
    assert_eq!(answer(r#"{"event":"error","code":"30005","msg":"error"}"#), ("auth".into(), false));
    assert_eq!(answer(r#"{"event":"subscribe","arg":{"instType":"USDT-FUTURES","channel":"orders","instId":"default"}}"#), ("subscribe".into(), true));
    assert_eq!(answer("pong"), ("ping".into(), true));
    // One channel per subscribe frame.
    let sock = m.private_socket(&settings()).unwrap();
    assert_eq!((sock.path.as_str(), sock.subscribes().len(), sock.ping.as_slice(), sock.ping_every_ms), ("/v2/ws/private", 3, b"ping".as_slice(), 30_000));
    assert_eq!(Bitget::spot().unwrap().private_socket(&settings()).unwrap().subscribes().len(), 2);
}

#[test]
fn errors_map_to_their_kind() {
    let m = Bitget::usdt_futures().unwrap();
    let e = |code: &str| m.decode_error(&frame("mix_place", 400, &format!(r#"{{"code":"{code}","msg":"m"}}"#))).unwrap();
    assert_eq!(e("40754").canon, Canon::Rejected(Refusal::Funds));
    assert_eq!(e("43001").canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(e("45034").canon, Canon::Unknown, "a duplicate clientOid: looked up, never resent");
    assert_eq!(e("40008").canon, Canon::Auth);
    assert!(!e("12345").mapped);
    assert!(m.invalidates_position_mode(&e("43075")) && !m.invalidates_position_mode(&e("40754")));
    assert!(m.invalidates_margin_mode(&e("40716")));
    assert!(!Bitget::spot().unwrap().invalidates_position_mode(&e("43075")));
}

// ── The catalog ──

/// The real mapper, with the two reads the catalog's mock venue does not serve answered from
/// what it does: the account-level position mode and the symbol's margin are left to the
/// position rows (both read from the account are tested on their own below), and the price a
/// spot market buy is valued at is read from the instrument list call (the catalog mounts no
/// ticker) at the script's price. Everything else is [`Bitget`]'s own.
#[derive(Clone)]
struct Catalog(Bitget);

impl Mapper for Catalog {
    type Session = <Bitget as Mapper>::Session;

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
    fn client_id_rule(&self) -> ClientIdRule {
        self.0.client_id_rule()
    }
    fn is_sandbox(&self, s: &Settings) -> bool {
        self.0.is_sandbox(s)
    }
    /// The catalog builds its request matchers by calling `place` with a bare [`Placing`]: a
    /// spot market buy is then valued here, so the matcher names the call. The trader's own
    /// calls arrive valued (`quote_amount` set), and the real mapper's refusal of an unvalued
    /// one is tested on its own.
    fn place(&self, s: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if self.0.market_buy_in_quote(o) && p.quote_amount.is_none() {
            return self.0.place(s, o, &Placing { quote_amount: Some(o.qty.value() * d("30000")), ..p.clone() });
        }
        self.0.place(s, o, p)
    }
    fn cancel(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.0.cancel(s, symbol, id, corr)
    }
    fn cancel_all(&self, s: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.cancel_all(s, symbols, corr)
    }
    fn cancel_all_way(&self) -> CancelAllWay {
        self.0.cancel_all_way()
    }
    fn decode_cancel_rows(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        self.0.decode_cancel_rows(ctx, body)
    }
    fn query_by_client_id(&self, s: &Settings, symbol: &str, cid: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.query_by_client_id(s, symbol, cid, corr)
    }
    fn query_by_venue_id(&self, s: &Settings, symbol: &str, id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.query_by_venue_id(s, symbol, id, corr)
    }
    fn open_orders(&self, s: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.open_orders(s, symbol, corr)
    }
    fn fills_since(&self, s: &Settings, symbol: &str, from: i64, to: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn fills_window_ms(&self) -> Option<i64> {
        self.0.fills_window_ms()
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
    fn all_positions(&self, s: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.0.all_positions(s, corr)
    }
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.0.invalidates_position_mode(e)
    }
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.0.invalidates_margin_mode(e)
    }
    fn market_buy_in_quote(&self, o: &OrderRequest) -> bool {
        self.0.market_buy_in_quote(o)
    }
    fn reference_price(&self, s: &Settings, symbol: &str, _side: Side, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.0.reference_price(s, symbol, Side::Buy, corr)?.map(|_| self.0.instrument_rules(s, Some(symbol), corr)).transpose()
    }
    fn decode_reference_price(&self, _body: &[u8], _side: Side) -> Result<Decimal, ConnectorError> {
        Ok(d("30000"))
    }
    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        self.0.decode_error(frame)
    }
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        self.0.decode_ack(body)
    }
    fn decode_ack_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Ack, ConnectorError> {
        self.0.decode_ack_in(ctx, body)
    }
    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.0.decode_orders(body)
    }
    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.0.decode_orders_in(ctx, body)
    }
    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.0.decode_fills(body)
    }
    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        self.0.decode_rules(body)
    }
    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.0.decode_positions(body)
    }
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        self.0.status_of(word)
    }
    fn private_socket(&self, s: &Settings) -> Option<PrivateSocket> {
        self.0.private_socket(s)
    }
    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.0.decode_private(payload)
    }
    fn error_table(&self) -> &'static [ErrorRow] {
        self.0.error_table()
    }
}

/// The catalog's answers: the documented examples with their ids, sizes and states changed
/// (`synthetic`, built on the documented shape).
struct Script {
    m: Bitget,
}

impl Script {
    fn futures(&self) -> bool {
        self.m.book() == super::super::Book::UsdtFutures
    }
}

fn envelope(code: &str, msg: &str) -> String {
    json!({ "code": code, "msg": msg, "requestTime": 1_695_808_949_356_i64, "data": null }).to_string()
}

fn first(template: &str, list: Option<&str>) -> Value {
    let v: Value = serde_json::from_str(template).unwrap();
    match list {
        Some(k) => v["data"][k][0].clone(),
        None => v["data"][0].clone(),
    }
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
        Arc::new(signer())
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
        let (src, field) = if self.futures() { (MIX_CONTRACTS, "symbolStatus") } else { (SPOT_SYMBOLS, "status") };
        match closed {
            None => src.to_string(),
            Some(w) => with(src, |v| v["data"][0][field] = json!(w)),
        }
    }

    fn closed_word(&self) -> &'static str {
        if self.futures() {
            "maintain"
        } else {
            "halt"
        }
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::PartiallyFilled => "partially_filled",
            OrderStatus::Filled => "filled",
            OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired if self.futures() => "canceled",
            OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired => "cancelled",
            _ => "live",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        with(if self.futures() { MIX_PLACE } else { SPOT_PLACE }, |v| {
            v["data"]["orderId"] = json!(venue_id);
            v["data"]["clientOid"] = json!(client_id);
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut o = if self.futures() { first(MIX_PENDING, Some("entrustedList")) } else { first(SPOT_ORDER_INFO, None) };
                o["orderId"] = json!(r.venue_id);
                o["clientOid"] = json!(r.client_id.unwrap_or(""));
                o["symbol"] = json!("BTCUSDT");
                o["side"] = json!(if r.side == Side::Buy { "buy" } else { "sell" });
                o["orderType"] = json!("limit");
                o["price"] = json!("30000");
                o["size"] = json!(r.qty.to_string());
                o["baseVolume"] = json!(r.filled.to_string());
                if self.futures() {
                    o["status"] = json!(r.word);
                    o["posSide"] = json!("net");
                    o["tradeSide"] = json!("");
                    o["posMode"] = json!("one_way_mode");
                } else {
                    o["status"] = json!(r.word);
                }
                o
            })
            .collect();
        if self.futures() {
            with(MIX_PENDING, |v| {
                v["data"]["entrustedList"] = json!(list);
                v["data"]["endId"] = json!("");
            })
        } else {
            with(SPOT_ORDER_INFO, |v| v["data"] = json!(list))
        }
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut f = if self.futures() { first(MIX_FILLS, Some("fillList")) } else { first(SPOT_FILLS, None) };
                f["tradeId"] = json!(r.exec_id);
                f["orderId"] = json!(r.venue_id);
                f["symbol"] = json!("BTCUSDT");
                let fee = json!({ "deduction": "no", "feeCoin": "USDT", "totalDeductionFee": "0", "totalFee": format!("-{}", r.fee) });
                if self.futures() {
                    f["price"] = json!(r.price.to_string());
                    f["baseVolume"] = json!(r.qty.to_string());
                    f["feeDetail"] = json!([fee]);
                    f["tradeSide"] = json!("open");
                } else {
                    f["priceAvg"] = json!(r.price.to_string());
                    f["size"] = json!(r.qty.to_string());
                    f["feeDetail"] = fee;
                }
                f
            })
            .collect();
        if self.futures() {
            with(MIX_FILLS, |v| {
                v["data"]["fillList"] = json!(list);
                v["data"]["endId"] = json!("");
            })
        } else {
            with(SPOT_FILLS, |v| v["data"] = json!(list))
        }
    }

    fn fee_currency(&self) -> &'static str {
        "USDT"
    }

    /// Rows without `marginMode`: the catalog's orders go out with no margin read (see
    /// [`Catalog`]), so every body it compares is built the same way.
    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|(side, dir, qty)| {
                let mut p = first(MIX_POSITION, None);
                p["symbol"] = json!("BTCUSDT");
                p["posMode"] = json!(if *side == PositionSide::Net { "one_way_mode" } else { "hedge_mode" });
                p["holdSide"] = json!(match (side, dir) {
                    (PositionSide::Short, _) | (PositionSide::Net, Some(Side::Sell)) => "short",
                    _ => "long",
                });
                p["total"] = json!(qty.to_string());
                p.as_object_mut().unwrap().remove("marginMode");
                p
            })
            .collect();
        with(MIX_POSITION, |v| v["data"] = json!(list))
    }

    fn ok(&self) -> String {
        if self.futures() { MIX_CANCEL } else { SPOT_CANCEL }.to_string()
    }

    fn dry_run_ok(&self) -> String {
        self.ok()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        let f = self.futures();
        match r {
            VenueRefusal::Funds => Answer::new(400, if f { envelope("40754", "balance not enough") } else { envelope("43012", "Insufficient balance") }),
            VenueRefusal::ReduceOnly => Answer::new(400, envelope("22002", "No position to close")),
            VenueRefusal::NotFound => Answer::new(400, envelope("43001", "The order does not exist")),
            VenueRefusal::Final => Answer::new(400, if f { envelope("45031", "The order is finalized") } else { envelope("43004", "There is no order to cancel") }),
            VenueRefusal::RateLimited(_) => Answer::new(429, envelope("429", "Too many requests")),
            VenueRefusal::ClockSkew => Answer::new(400, envelope("40008", "Request timestamp expired")),
            VenueRefusal::Unmapped => Answer::new(400, envelope("99999", "something new")),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            VenueRefusal::PositionModeMismatch => Answer::new(400, envelope("43075", "Position pattern mismatch")),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_passes_on_spot() {
    let r = run_catalog(&Script { m: Bitget::spot().unwrap() }).await;
    println!("spot ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    assert!(r.ran.len() >= 25, "{:?}", r.ran);
}

#[tokio::test]
async fn the_trading_catalog_passes_on_usdt_futures() {
    let r = run_catalog(&Script { m: Bitget::usdt_futures().unwrap() }).await;
    println!("usdt-futures ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    assert!(r.ran.contains(&"T10"), "{:?}", r.ran);
}

// ── Through the trader, the reads the catalog does not serve ──

mod through_the_trader {
    use super::*;
    use otw_connector_engine::trading::{Account, TicketOptions, TradeError, Trader};
    use otw_connector_testkit::wiremock::matchers::{method, path, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn ok(body: String) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_string(body).insert_header("content-type", "application/json")
    }

    async fn bodies(server: &MockServer, p: &str) -> Vec<Value> {
        server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == p).map(|r| serde_json::from_slice(&r.body).unwrap()).collect()
    }

    async fn count(server: &MockServer, p: &str) -> usize {
        server.received_requests().await.unwrap().iter().filter(|r: &&Request| r.url.path() == p).count()
    }

    fn driver(m: Bitget, server: &MockServer) -> otw_connector_sdk::Driver<Bitget> {
        otw_connector_testkit::driver_against(m, server, otw_connector_testkit::ManualClock::at(otw_connector_testkit::ts(1_767_225_600)))
    }

    /// Get Single Account names the product type's position mode (hedge in the documented
    /// example) and the symbol's margin: both read once, held, and sent with every order.
    #[tokio::test]
    async fn margin_and_position_mode_are_read_from_the_account_once_and_go_with_the_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v2/mix/market/contracts")).respond_with(ok(MIX_CONTRACTS.into())).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v2/mix/account/account")).respond_with(ok(MIX_ACCOUNT.into())).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v2/mix/order/place-order")).respond_with(ok(MIX_PLACE.into())).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v2/mix/position/single-position")).respond_with(ok(MIX_POSITION.into())).mount(&server).await;
        let m = Bitget::usdt_futures().unwrap();
        let drv = driver(m.clone(), &server);
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "acct" };
        let t: Trader<Bitget> = Trader::new();
        for key in ["a1", "a2"] {
            t.place(&drv, &a, &[order(&m, key, Side::Buy, OrderType::Limit, "0.01", Some("30000"))], &TicketOptions::default()).await.unwrap();
        }
        let sent = bodies(&server, "/api/v2/mix/order/place-order").await;
        assert_eq!(sent.len(), 2);
        for b in &sent {
            assert_eq!((b["marginMode"].as_str(), b["marginCoin"].as_str(), b["side"].as_str(), b["tradeSide"].as_str()), (Some("crossed"), Some("USDT"), Some("buy"), Some("open")));
        }
        assert_eq!(count(&server, "/api/v2/mix/account/account").await, 2, "the mode and the margin, once each");
        assert_eq!(count(&server, "/api/v2/mix/position/single-position").await, 0, "no position read on the order path");
        // The leverage and margin shown read only, from what is held.
        let lev = t.leverage(&drv, &a, "BTCUSDT").await;
        assert!(matches!(lev, Ok(Some((_, Some(ref w)))) if w == "cross"), "{lev:?}");
    }

    /// A position's take profit and stop loss, through the path attached exits change by: set
    /// with none working (Place Position TP/SL), moved by its id when one works (Modify the TP/SL
    /// order, `size` empty), removed by its plan type (Cancel Trigger Order, `pos_profit`).
    #[tokio::test]
    async fn a_position_take_profit_is_set_moved_and_removed_on_the_position() {
        use otw_connector_engine::trading::AttachedExit;
        use otw_connector_sdk::trade::ExitKind;
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v2/mix/market/contracts")).respond_with(ok(MIX_CONTRACTS.into())).mount(&server).await;
        Mock::given(method("GET")).and(path("/api/v2/mix/account/account")).respond_with(ok(MIX_ACCOUNT.into())).mount(&server).await;
        let none = with(MIX_PLAN_PENDING, |v| v["data"]["entrustedList"] = json!([]));
        let empty = Mock::given(method("GET")).and(path("/api/v2/mix/order/orders-plan-pending")).and(query_param("planType", "profit_loss")).respond_with(ok(none)).mount_as_scoped(&server).await;
        Mock::given(method("POST")).and(path("/api/v2/mix/order/place-pos-tpsl")).respond_with(ok(r#"{"code":"00000","msg":"success","requestTime":1,"data":[{"orderId":"t1","stopSurplusClientOid":"","stopLossClientOid":""}]}"#.into())).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v2/mix/order/modify-tpsl-order")).respond_with(ok(r#"{"code":"00000","msg":"success","requestTime":1,"data":{"orderId":"t1","clientOid":""}}"#.into())).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/v2/mix/order/cancel-plan-order")).respond_with(ok(r#"{"code":"00000","msg":"success","requestTime":1,"data":{"successList":[{"orderId":"t1","clientOid":""}],"failureList":[]}}"#.into())).mount(&server).await;
        let m = Bitget::usdt_futures().unwrap();
        let drv = driver(m.clone(), &server);
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "tpsl" };
        let t: Trader<Bitget> = Trader::new();
        // The account read's id for a long position's take profit (`tpsl::tp:buy`).
        let x = AttachedExit::parse("BTCUSDT", "tpsl::tp:buy").unwrap();
        assert_eq!((x.kind, x.entry_side), (ExitKind::TakeProfit, Side::Buy));
        t.change_exit(&drv, &a, &x, Some(d("32000"))).await.unwrap();
        let set = bodies(&server, "/api/v2/mix/order/place-pos-tpsl").await;
        assert_eq!(
            (set[0]["holdSide"].as_str(), set[0]["stopSurplusTriggerPrice"].as_str(), set[0]["stopSurplusTriggerType"].as_str(), set[0].get("stopLossTriggerPrice").is_none()),
            (Some("long"), Some("32000"), Some("fill_price"), true)
        );
        drop(empty);
        let working = with(MIX_PLAN_PENDING, |v| {
            let r = &mut v["data"]["entrustedList"][0];
            r["planType"] = json!("pos_profit");
            r["symbol"] = json!("BTCUSDT");
            r["orderId"] = json!("t1");
            r["size"] = json!("");
        });
        Mock::given(method("GET")).and(path("/api/v2/mix/order/orders-plan-pending")).and(query_param("planType", "profit_loss")).respond_with(ok(working)).mount(&server).await;
        t.change_exit(&drv, &a, &x, Some(d("33000"))).await.unwrap();
        let moved = bodies(&server, "/api/v2/mix/order/modify-tpsl-order").await;
        assert_eq!((moved[0]["orderId"].as_str(), moved[0]["triggerPrice"].as_str(), moved[0]["size"].as_str()), (Some("t1"), Some("33000"), Some("")));
        t.change_exit(&drv, &a, &x, None).await.unwrap();
        let off = bodies(&server, "/api/v2/mix/order/cancel-plan-order").await;
        assert_eq!((off[0]["orderIdList"][0]["orderId"].as_str(), off[0]["planType"].as_str()), (Some("t1"), Some("pos_profit")));
        assert_eq!(count(&server, "/api/v2/mix/order/place-pos-tpsl").await, 1);
        // A stop loss with none working has nothing to remove: refused, nothing sent.
        let sl = AttachedExit::parse("BTCUSDT", "tpsl::sl:buy").unwrap();
        assert!(matches!(t.change_exit(&drv, &a, &sl, None).await, Err(TradeError::Refused(_))));
        assert_eq!(count(&server, "/api/v2/mix/order/cancel-plan-order").await, 1);
    }

    /// A plan type other than `normal_plan` is named on its cancel (`cond:<planType>:<id>`).
    #[test]
    fn a_plan_order_is_cancelled_under_its_own_plan_type() {
        let m = Bitget::usdt_futures().unwrap();
        let b = |id: &str| serde_json::from_slice::<Value>(m.cancel(&settings(), "BTCUSDT", id, CorrId(1)).unwrap().body.as_deref().unwrap()).unwrap();
        assert_eq!(b("cond:track_plan:9")["planType"], json!("track_plan"));
        assert_eq!(b("cond:track_plan:9")["orderIdList"][0]["orderId"], json!("9"));
        assert_eq!(b("cond:9")["planType"], json!("normal_plan"));
    }

    /// T10 through the trader: the size the venue holds, at the market, reduce-only on the other
    /// side; in one-way mode Bitget may answer without an `orderId` ("the response for the latest
    /// reduce-only order request will not include an orderId"): found by its client id.
    #[tokio::test]
    async fn a_position_closes_at_the_market_reduce_only_and_an_ack_without_id_is_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v2/mix/market/contracts")).respond_with(ok(MIX_CONTRACTS.into())).mount(&server).await;
        let long = with(MIX_POSITION, |v| {
            let p = &mut v["data"][0];
            p["holdSide"] = json!("long");
            p["posMode"] = json!("one_way_mode");
            p["total"] = json!("0.02");
        });
        Mock::given(method("GET")).and(path("/api/v2/mix/position/single-position")).respond_with(ok(long)).mount(&server).await;
        let bare = with(MIX_PLACE, |v| {
            v["data"]["orderId"] = Value::Null;
            v["data"]["clientOid"] = json!("c-close");
        });
        Mock::given(method("POST")).and(path("/api/v2/mix/order/place-order")).respond_with(ok(bare)).mount(&server).await;
        let found = with(MIX_DETAIL, |v| {
            v["data"]["clientOid"] = json!("c-close");
            v["data"]["orderId"] = json!("777");
        });
        Mock::given(method("GET")).and(path("/api/v2/mix/order/detail")).and(query_param("clientOid", "c-close")).respond_with(ok(found)).mount(&server).await;
        let m = Bitget::usdt_futures().unwrap();
        let drv = driver(m.clone(), &server);
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "close" };
        let t: Trader<Bitget> = Trader::new();
        let placed = t.close_position(&drv, &a, "BTCUSDT", None, None, "c-close").await.unwrap();
        assert_eq!(placed.venue_id, "777");
        let sent = bodies(&server, "/api/v2/mix/order/place-order").await;
        assert_eq!(sent.len(), 1, "never sent twice");
        assert_eq!(
            (sent[0]["side"].as_str(), sent[0]["reduceOnly"].as_str(), sent[0]["size"].as_str(), sent[0]["orderType"].as_str(), sent[0]["marginMode"].as_str()),
            (Some("sell"), Some("YES"), Some("0.02"), Some("market"), Some("crossed"))
        );
        // Part of it, and never more than held.
        let part = t.close_position(&drv, &a, "BTCUSDT", None, Some(d("0.01")), "c-close").await.unwrap();
        assert_eq!(part.venue_id, "777");
        assert_eq!(bodies(&server, "/api/v2/mix/order/place-order").await[1]["size"], json!("0.01"));
        let more = t.close_position(&drv, &a, "BTCUSDT", None, Some(d("0.03")), "c-more").await;
        assert!(matches!(more, Err(TradeError::Refused(_))), "{more:?}");
        assert_eq!(count(&server, "/api/v2/mix/order/place-order").await, 2);
    }

    /// The doc's ticker example answers an ask of "0": refused, nothing sent. At a real ask the
    /// buy goes out as the amount to spend, rounded down to the quote precision.
    #[tokio::test]
    async fn a_spot_market_buy_is_valued_at_the_best_ask_read_just_before_sending() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/v2/spot/public/symbols")).respond_with(ok(SPOT_SYMBOLS.into())).mount(&server).await;
        let zero = Mock::given(method("GET")).and(path("/api/v2/spot/market/tickers")).respond_with(ok(SPOT_TICKERS.into())).mount_as_scoped(&server).await;
        Mock::given(method("POST")).and(path("/api/v2/spot/trade/place-order")).respond_with(ok(SPOT_PLACE.into())).mount(&server).await;
        let m = Bitget::spot().unwrap();
        let drv = driver(m.clone(), &server);
        let (s, sg) = (settings(), signer());
        let a = Account { settings: &s, signer: &sg, scope: "quote" };
        let t: Trader<Bitget> = Trader::new();
        let buy = order(&m, "q1", Side::Buy, OrderType::Market, "0.01", None);
        let out = t.place(&drv, &a, std::slice::from_ref(&buy), &TicketOptions::default()).await;
        assert!(matches!(out, Err(TradeError::Refused(_))), "{out:?}");
        assert_eq!(count(&server, "/api/v2/spot/trade/place-order").await, 0);
        drop(zero);
        let quoted = with(SPOT_TICKERS, |v| v["data"][0]["askPr"] = json!("34413.123456789"));
        Mock::given(method("GET")).and(path("/api/v2/spot/market/tickers")).respond_with(ok(quoted)).mount(&server).await;
        let placed = t.place(&drv, &a, &[buy], &TicketOptions::default()).await.unwrap();
        assert!(placed[0].note.as_deref().is_some_and(|n| n.contains("amount to spend")), "{placed:?}");
        let sent = bodies(&server, "/api/v2/spot/trade/place-order").await;
        assert_eq!(sent[0]["size"], json!("344.13123456"));
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live instrument lists decode to sound rules for every symbol, and an order call signed
/// with a key Bitget never issued, or not signed at all, is answered with the auth error the
/// table expects (nothing can be placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers bitget::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    for m in [Bitget::spot().unwrap(), Bitget::usdt_futures().unwrap()] {
        let label = m.capabilities().label.clone();
        let d = Driver::new(m.clone(), DriverConfig::new("bitget"), reqwest::Client::new());
        let junk = settings();
        let sg = signer();
        let a = Account { settings: &junk, signer: &sg, scope: "net" };
        let trader: Trader<Bitget> = Trader::new();
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
        let req = m.place(&junk, &o, &Placing { margin: crossed(), ..placing("otw-net-probe", PositionSide::Net) }).unwrap();
        for (what, signed) in [("junk key", true), ("unsigned", false)] {
            let mut r = req.clone();
            r.signed = signed;
            let frame = d.send_as(r, Some(&sg)).await.unwrap_or_else(|e| panic!("{label} {what}: {e}"));
            let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label} {what}: Bitget accepted an order with no valid key"));
            println!("{label} {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
            assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{label} {what}: {e:?}");
        }
    }
}
