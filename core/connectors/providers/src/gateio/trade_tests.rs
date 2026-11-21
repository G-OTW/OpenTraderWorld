//! Gate.io orders, from the documentation alone. Fixtures under `fixtures/trade/`, every one
//! `synthetic`: the live doc site answers automated readers with a Cloudflare challenge, and
//! its reader could not copy the response examples verbatim, so each answer is built on the
//! documented schema (field names, types and words) of the official SDK's model pages,
//! gateio/gateapi-python at commit `22fdf34ae4af855863ccb01b95e8c996de4cbea3`, and of the
//! WebSocket pages' field tables:
//!
//! | File | Schema |
//! | --- | --- |
//! | `spot_order.json` | `docs/Order.md` (create, get, cancel answers) |
//! | `spot_open_orders.json` | `docs/OpenOrders.md` (`GET /spot/open_orders`) |
//! | `spot_cancel_all.json` | `docs/OrderCancel.md` (`DELETE /spot/orders`) |
//! | `spot_my_trades.json` | `docs/Trade.md` (`GET /spot/my_trades`) |
//! | `spot_pairs.json` | `docs/CurrencyPair.md` (`GET /spot/currency_pairs`) |
//! | `spot_ticker.json` | `docs/Ticker.md` (`GET /spot/tickers`) |
//! | `futures_order.json` | `docs/FuturesOrder.md` |
//! | `futures_my_trades.json` | `docs/MyFuturesTradeTimeRange.md` |
//! | `futures_contracts.json` | `docs/Contract.md`, plus `market_order_size_max` and `enable_decimal` (https://www.gate.com/docs/developers/apiv4/en/futures/) |
//! | `futures_position.json` | `docs/Position.md` |
//! | `futures_account.json` | `docs/FuturesAccount.md` |
//! | `account_detail.json` | `docs/AccountDetail.md` |
//! | `ws_spot_orders.json`, `ws_spot_usertrades.json` | https://www.gate.com/docs/developers/apiv4/ws/en/ (`spot.orders`, `spot.usertrades` field tables) |
//! | `ws_futures_orders.json`, `ws_futures_usertrades.json`, `ws_futures_positions.json` | https://www.gate.com/docs/developers/futures/ws/en/ (`futures.orders`, `futures.usertrades`, `futures.positions` field tables) |
//!
//! Every error body is `synthetic` too (`{"label", "message"}`, the overview's error shape).
//! The scenarios below (onboarding spec 10.3) run through the shared trader and this mapper
//! against a mock venue: the testkit's catalog cannot drive this venue as it stands (its
//! request probes carry no contract size and no quote amount), so they are written here.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, TradeError, Trader};
use otw_connector_model::{
    CorrId, Decimal, ErrorKind, ExecLevel, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce,
};
use otw_connector_sdk::trade::{Canon, CancelAllWay, ContractSizes, DecodeCtx, Placing, PositionMode, PositionSide, PrivateEvent, Refusal, TradeMapper, TradingStatus, VenueOrder};
use otw_connector_sdk::{Driver, HttpRequest, Mapper, Method, Signer};
use otw_connector_testkit::wiremock::{Mock, MockServer, Request, ResponseTemplate};
use serde_json::{json, Value};

use super::super::signer::GateSigner;
use super::super::Gateio;

const SPOT_ORDER: &str = include_str!("fixtures/trade/spot_order.json");
const SPOT_OPEN: &str = include_str!("fixtures/trade/spot_open_orders.json");
const SPOT_CANCEL_ALL: &str = include_str!("fixtures/trade/spot_cancel_all.json");
const SPOT_TRADES: &str = include_str!("fixtures/trade/spot_my_trades.json");
const SPOT_PAIRS: &str = include_str!("fixtures/trade/spot_pairs.json");
const SPOT_TICKER: &str = include_str!("fixtures/trade/spot_ticker.json");
const FUT_ORDER: &str = include_str!("fixtures/trade/futures_order.json");
const FUT_TRADES: &str = include_str!("fixtures/trade/futures_my_trades.json");
const FUT_CONTRACTS: &str = include_str!("fixtures/trade/futures_contracts.json");
const FUT_POSITION: &str = include_str!("fixtures/trade/futures_position.json");
const FUT_ACCOUNT: &str = include_str!("fixtures/trade/futures_account.json");
const ACCOUNT_DETAIL: &str = include_str!("fixtures/trade/account_detail.json");
const WS_SPOT_ORDERS: &str = include_str!("fixtures/trade/ws_spot_orders.json");
const WS_SPOT_TRADES: &str = include_str!("fixtures/trade/ws_spot_usertrades.json");
const WS_FUT_ORDERS: &str = include_str!("fixtures/trade/ws_futures_orders.json");
const WS_FUT_TRADES: &str = include_str!("fixtures/trade/ws_futures_usertrades.json");
const WS_FUT_POSITIONS: &str = include_str!("fixtures/trade/ws_futures_positions.json");

const SYM: &str = "BTC_USDT";
/// The catalog's clock, epoch milliseconds.
const NOW_MS: i64 = 1_767_225_600_000;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("api_key", "key"), ("api_secret", "secret-test")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn order(m: &Gateio, key: &str, side: Side, t: OrderType, qty: &str, limit: Option<&str>) -> OrderRequest {
    let (l, s) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new(m.book().venue(), SYM),
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

fn body(req: &HttpRequest) -> Value {
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

fn placing(m: &Gateio, cid: &str) -> Placing {
    let mut p = Placing::new(m.client_id(cid).unwrap(), PositionSide::Net, CorrId(1));
    if m.counts_contracts() {
        p.contract_size = Some(d("0.0001"));
    }
    p
}

fn sizes() -> ContractSizes {
    let mut s = ContractSizes::new();
    s.insert(SYM, d("0.0001"));
    s
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

fn refusal(label: &str, msg: &str) -> String {
    json!({ "label": label, "message": msg }).to_string()
}

// ── Requests ──

#[test]
fn a_spot_limit_order_is_the_documented_body() {
    let m = Gateio::spot().unwrap();
    let mut o = order(&m, "k1", Side::Buy, OrderType::Limit, "0.01", Some("30000"));
    o.tif = TimeInForce::Fok;
    let req = m.place(&settings(), &o, &placing(&m, "k1")).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.base), (Method::Post, "/api/v4/spot/orders", true, "rest"));
    assert_eq!(body(&req), json!({ "currency_pair": SYM, "side": "buy", "type": "limit", "amount": "0.01", "price": "30000", "time_in_force": "fok", "text": "t-k1" }));
}

#[test]
fn a_spot_market_sell_is_in_the_base_coin_and_a_market_buy_in_the_quote_coin() {
    let m = Gateio::spot().unwrap();
    let sell = m.place(&settings(), &order(&m, "s", Side::Sell, OrderType::Market, "0.01", None), &placing(&m, "s")).unwrap();
    assert_eq!(body(&sell), json!({ "currency_pair": SYM, "side": "sell", "type": "market", "amount": "0.01", "time_in_force": "ioc", "text": "t-s" }));
    let buy = order(&m, "b", Side::Buy, OrderType::Market, "0.01", None);
    assert!(m.market_buy_in_quote(&buy) && !m.market_buy_in_quote(&order(&m, "l", Side::Buy, OrderType::Limit, "1", Some("1"))));
    assert!(m.place(&settings(), &buy, &placing(&m, "b")).is_err(), "no amount to spend: nothing is built");
    let mut p = placing(&m, "b");
    p.quote_amount = Some(d("300.005"));
    let mut fok = buy.clone();
    fok.tif = TimeInForce::Fok;
    assert_eq!(body(&m.place(&settings(), &fok, &p).unwrap())["amount"], json!("300.005"));
    assert_eq!(body(&m.place(&settings(), &fok, &p).unwrap())["time_in_force"], json!("fok"));
    // The ticker read that values it: the best ask, public.
    let t = m.reference_price(&settings(), SYM, Side::Buy, CorrId(1)).unwrap().unwrap();
    assert_eq!((t.path.as_str(), t.signed), ("/api/v4/spot/tickers", false));
    assert_eq!(m.decode_reference_price(SPOT_TICKER.as_bytes(), Side::Buy).unwrap(), d("30000.5"));
    // Spot takes no reduce-only order, and nothing but market and limit.
    let mut ro = order(&m, "r", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    ro.opts.reduce_only = true;
    assert!(m.place(&settings(), &ro, &placing(&m, "r")).is_err());
    assert!(m.place(&settings(), &order(&m, "x", Side::Buy, OrderType::Stop, "0.01", Some("1")), &placing(&m, "x")).is_err());
}

#[test]
fn a_futures_order_goes_in_whole_contracts_signed_by_its_side() {
    let m = Gateio::futures().unwrap();
    let limit = m.place(&settings(), &order(&m, "f1", Side::Sell, OrderType::Limit, "0.01", Some("30000")), &placing(&m, "f1")).unwrap();
    assert_eq!(limit.path, "/api/v4/futures/usdt/orders");
    assert_eq!(body(&limit), json!({ "contract": SYM, "size": -100, "price": "30000", "tif": "gtc", "text": "t-f1" }));
    let mut close = order(&m, "f2", Side::Buy, OrderType::Market, "0.0005", None);
    close.opts.reduce_only = true;
    assert_eq!(body(&m.place(&settings(), &close, &placing(&m, "f2")).unwrap()), json!({ "contract": SYM, "size": 5, "price": "0", "tif": "ioc", "reduce_only": true, "text": "t-f2" }));
    // Not a whole number of contracts: refused, naming the nearest sizes.
    let e = m.place(&settings(), &order(&m, "f3", Side::Buy, OrderType::Limit, "0.00015", Some("30000")), &placing(&m, "f3")).unwrap_err();
    assert!(e.message.contains("0.0001 or 0.0002"), "{}", e.message);
    // No contract size read: nothing is built.
    assert!(m.place(&settings(), &order(&m, "f4", Side::Buy, OrderType::Limit, "0.01", Some("30000")), &Placing::new("t-f4", PositionSide::Net, CorrId(1))).is_err());
    // A market order FOK is refused before I/O; a limit FOK goes.
    let mut fok = order(&m, "f5", Side::Buy, OrderType::Market, "0.01", None);
    fok.tif = TimeInForce::Fok;
    assert_eq!(m.check_order(&fok).unwrap_err().kind, ErrorKind::Unsupported);
    fok.order_type = OrderType::Limit;
    fok.limit = Some(Price::new(d("30000")));
    assert!(m.check_order(&fok).is_ok());
    assert_eq!(body(&m.place(&settings(), &fok, &placing(&m, "f5")).unwrap())["tif"], json!("fok"));
}

#[test]
fn the_client_id_is_the_key_behind_t_dash_within_28_bytes() {
    let m = Gateio::spot().unwrap();
    assert_eq!(m.client_id("abc.DEF_1-2").unwrap(), "t-abc.DEF_1-2");
    assert_eq!(m.otw_id("t-abc.DEF_1-2"), "abc.DEF_1-2");
    assert!(m.client_id(&"a".repeat(28)).is_ok());
    assert!(m.client_id(&"a".repeat(29)).is_err());
    assert!(m.client_id("no space").is_err());
    // An order's `text` that is its source, not a client id, is none.
    let row = with(SPOT_ORDER, |v| v["text"] = json!("apiv4"));
    assert_eq!(m.decode_orders(row.as_bytes()).unwrap().0[0].client_id, None);
}

/// The account read follows the `site` setting too: a testnet account is read on the testnet
/// host, an unknown site is refused.
#[test]
fn the_account_read_goes_to_the_sites_host() {
    use otw_connector_model::{AccountKind, Command};
    use otw_connector_sdk::Mapper;
    let m = Gateio::spot().unwrap();
    let mut s = settings();
    assert_eq!(m.account_selector(&s), None);
    s.insert("site".into(), "testnet".into());
    let site = m.account_selector(&s);
    assert_eq!(site.as_deref(), Some("testnet"));
    let cmd = Command::QueryAccount { req_id: CorrId(7), kind: AccountKind::OpenOrders, symbols: vec![], range: None, cursor: None, account: site };
    let mut pending = HashMap::new();
    let reqs = super::super::account::encode(super::super::Book::Spot, super::super::account::base(Some("testnet")).unwrap(), &cmd, &mut pending).unwrap();
    let otw_connector_sdk::NativeRequest::Http(h) = &reqs[0] else { panic!("{reqs:?}") };
    assert_eq!(h.base, "rest-testnet");
    assert_eq!(super::super::account::base(None).unwrap(), "rest");
    assert!(super::super::account::base(Some("moon")).is_err());
    s.insert("site".into(), "moon".into());
    assert!(m.check_settings(&s).is_err());
}

#[test]
fn testnet_is_the_sandbox_on_its_own_hosts() {
    let m = Gateio::futures().unwrap();
    let mut s = settings();
    assert!(!m.is_sandbox(&s));
    s.insert("site".into(), "testnet".into());
    assert!(m.is_sandbox(&s));
    assert_eq!(m.cancel(&s, SYM, "1", CorrId(1)).unwrap().base, "rest-testnet");
    assert_eq!(m.instrument_rules(&s, Some(SYM), CorrId(1)).unwrap().base, "rest-testnet");
    let ws = m.private_socket(&s).unwrap();
    assert_eq!((ws.base, ws.path.as_str()), ("ws-testnet", "/v4/ws/futures/usdt"));
    let spot = Gateio::spot().unwrap().private_socket(&s).unwrap();
    assert_eq!((spot.base, spot.path.as_str()), ("ws-testnet", "/v4/ws/spot"));
    s.insert("site".into(), "moon".into());
    assert!(m.cancel(&s, SYM, "1", CorrId(1)).is_err());
    let urls: HashMap<_, _> = m.base_urls().iter().copied().collect();
    assert_eq!((urls["rest-testnet"], urls["ws-testnet"]), ("https://api-testnet.gateapi.io", "wss://ws-testnet.gate.com"));
}

#[test]
fn a_change_is_one_price_or_size_on_spot_and_price_or_contracts_on_futures() {
    let spot = Gateio::spot().unwrap();
    let pl = Placing::new("", PositionSide::Net, CorrId(1));
    let cur = spot.decode_orders(SPOT_ORDER.as_bytes()).unwrap().0.remove(0);
    let price = OrderAmend { limit: Some(Price::new(d("5.1"))), ..Default::default() };
    let req = spot.amend(&settings(), &cur, &price, &pl).unwrap();
    assert_eq!((req.method, req.path.as_str()), (Method::Post, "/api/v4/spot/amend_batch_orders"));
    assert_eq!(body(&req), json!([{ "order_id": "12332324", "currency_pair": "ETH_BTC", "price": "5.1" }]));
    let size = OrderAmend { qty: Some(Qty::new(d("2"))), ..Default::default() };
    assert_eq!(body(&spot.amend(&settings(), &cur, &size, &pl).unwrap())[0]["amount"], json!("2"));
    let both = OrderAmend { limit: Some(Price::new(d("5.1"))), qty: Some(Qty::new(d("2"))), ..Default::default() };
    assert!(spot.amend(&settings(), &cur, &both, &pl).is_err());
    // A failed row of the batch answer is the call's refusal.
    let bad = r#"[{"order_id":"12332324","succeeded":false,"label":"ORDER_NOT_FOUND","message":"Order not found"}]"#;
    let e = spot.decode_error(&otw_connector_testkit::http_frame(1, "spot_amend_batch", 200, bad)).unwrap();
    assert_eq!(e.canon, Canon::Rejected(Refusal::NotFound));
    let fut = Gateio::futures().unwrap();
    let cur = fut.decode_orders_in(&DecodeCtx::new("futures_order", &sizes()), FUT_ORDER.as_bytes()).unwrap().0.remove(0);
    let req = fut.amend(&settings(), &cur, &price, &pl).unwrap();
    assert_eq!((req.method, req.path.as_str()), (Method::Put, "/api/v4/futures/usdt/orders/15675394"));
    assert_eq!(body(&req), json!({ "price": "5.1" }));
    // A new size is sent in contracts, signed like the order; without the contract size nothing is sent.
    assert!(fut.amend(&settings(), &cur, &size, &pl).is_err(), "no contract size read");
    let ct = Placing { contract_size: Some(d("0.0001")), ..pl.clone() };
    assert_eq!(body(&fut.amend(&settings(), &cur, &size, &ct).unwrap()), json!({ "size": 20000 }));
    let short = VenueOrder { side: Mapped::Known(Side::Sell), ..cur.clone() };
    assert_eq!(body(&fut.amend(&settings(), &short, &size, &ct).unwrap()), json!({ "size": -20000 }));
}

#[test]
fn cancel_all_covers_the_spot_book_in_one_call_and_futures_per_contract() {
    let spot = Gateio::spot().unwrap();
    assert_eq!(spot.cancel_all_way(), CancelAllWay::Book);
    let all = spot.cancel_all(&settings(), &[], CorrId(1)).unwrap();
    assert_eq!((all.len(), all[0].method, all[0].path.as_str(), all[0].query.is_empty()), (2, Method::Delete, "/api/v4/spot/orders", true));
    // The price-triggered orders too ("Cancel all auto orders").
    assert_eq!((all[1].method, all[1].path.as_str(), all[1].query.is_empty()), (Method::Delete, "/api/v4/spot/price_orders", true));
    let rows = spot.decode_cancel_rows(&DecodeCtx::new("spot_cancel_all", &ContractSizes::new()), SPOT_CANCEL_ALL.as_bytes()).unwrap();
    assert_eq!((rows[0].venue_id.as_str(), rows[0].error.is_none()), ("1852454420", true));
    assert_eq!(rows[1].error.as_ref().map(|e| e.canon), Some(Canon::Rejected(Refusal::NotFound)));
    let fut = Gateio::futures().unwrap();
    assert_eq!(fut.cancel_all_way(), CancelAllWay::BySymbol);
    let per = fut.cancel_all(&settings(), &[SYM.to_string()], CorrId(1)).unwrap();
    assert_eq!((per[0].path.as_str(), per[0].query.clone()), ("/api/v4/futures/usdt/orders", vec![("contract".to_string(), SYM.to_string())]));
    assert_eq!((per[1].method, per[1].path.as_str(), per[1].query.clone()), (Method::Delete, "/api/v4/futures/usdt/price_orders", vec![("contract".to_string(), SYM.to_string())]));
}

#[test]
fn the_working_list_reads_the_price_triggered_orders_too() {
    let fut = Gateio::futures().unwrap();
    let reqs = fut.open_orders(&settings(), Some(SYM), CorrId(1)).unwrap();
    assert_eq!(reqs.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/api/v4/futures/usdt/orders", "/api/v4/futures/usdt/price_orders"]);
    assert!(reqs[1].query.contains(&("contract".to_string(), SYM.to_string())) && reqs[1].query.contains(&("status".to_string(), "open".to_string())));
    let body = r#"[{"initial":{"contract":"BTC_USDT","size":-100,"price":"0","tif":"ioc","is_reduce_only":true},"trigger":{"price":"75000","rule":2},"id":77,"create_time":1792300000.5,"status":"open","order_type":"plan-close-long-position"},
        {"initial":{"contract":"BTC_USDT","size":0,"price":"0","auto_size":"close_short"},"trigger":{"price":"90000","rule":1},"id":78,"create_time":1792300001,"status":"inactive","order_type":"close-short-position"}]"#;
    let (rows, next) = fut.decode_orders_in(&DecodeCtx::new("futures_price_orders", &sizes()), body.as_bytes()).unwrap();
    assert_eq!(next, None);
    assert_eq!(
        (rows[0].venue_id.as_str(), rows[0].side.clone(), rows[0].qty, rows[0].stop, rows[0].limit, rows[0].reduce_only, rows[0].trigger_pending),
        ("cond:77", Mapped::Known(Side::Sell), d("0.01"), Some(d("75000")), None, true, true)
    );
    assert_eq!(rows[0].status, Mapped::Known(OrderStatus::New));
    // A whole-position close states no size; its side closes the short it names.
    assert_eq!((rows[1].side.clone(), rows[1].qty, rows[1].position, rows[1].order_type.as_str()), (Mapped::Known(Side::Buy), Decimal::ZERO, PositionSide::Short, "close-short-position market (whole position)"));
    // Spot: the account's mode first, then the list, `account=unified` for a unified account.
    let spot = Gateio::spot().unwrap();
    let reqs = spot.open_orders(&settings(), None, CorrId(1)).unwrap();
    assert_eq!(reqs.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/api/v4/spot/open_orders", "/api/v4/unified/unified_mode"]);
    let (rows, cur) = spot.decode_orders_in(&DecodeCtx::new(reqs[1].endpoint_id, &ContractSizes::new()), br#"{"mode":"multi_currency"}"#).unwrap();
    assert!(rows.is_empty());
    let list = spot.with_cursor(&reqs[1], cur.as_deref().unwrap());
    assert_eq!((list.path.as_str(), list.endpoint_id, list.signed), ("/api/v4/spot/price_orders", "spot_price_orders", true));
    assert!(list.query.contains(&("account".to_string(), "unified".to_string())) && list.query.contains(&("status".to_string(), "open".to_string())));
    let (_, cur) = spot.decode_orders_in(&DecodeCtx::new(reqs[1].endpoint_id, &ContractSizes::new()), br#"{"mode":"classic"}"#).unwrap();
    assert!(!spot.with_cursor(&reqs[1], cur.as_deref().unwrap()).query.iter().any(|(k, _)| k == "account"));
    // A key refused the mode check is no error: the classic list is read.
    let refused = otw_connector_testkit::http_frame(1, reqs[1].endpoint_id, 403, &refusal("FORBIDDEN", "Forbidden"));
    assert!(spot.decode_error(&refused).is_none());
    let (_, cur) = spot.decode_orders_in(&DecodeCtx::new(reqs[1].endpoint_id, &ContractSizes::new()), refusal("FORBIDDEN", "Forbidden").as_bytes()).unwrap();
    assert_eq!(cur.as_deref(), Some("classic"));
    let auth = otw_connector_testkit::http_frame(1, reqs[1].endpoint_id, 401, &refusal("INVALID_KEY", "Invalid key"));
    assert_eq!(spot.decode_error(&auth).map(|e| e.canon), Some(Canon::Auth));
    let body = r#"[{"trigger":{"price":"60000","rule":"<="},"put":{"type":"limit","side":"sell","price":"59900","amount":"0.2","account":"unified"},"id":1283293,"market":"BTC_USDT","ctime":1792300000,"status":"open"}]"#;
    let (rows, _) = spot.decode_orders_in(&DecodeCtx::new("spot_price_orders", &ContractSizes::new()), body.as_bytes()).unwrap();
    assert_eq!((rows[0].venue_id.as_str(), rows[0].qty, rows[0].limit, rows[0].stop, rows[0].created_ms), ("cond:1283293", d("0.2"), Some(d("59900")), Some(d("60000")), Some(1_792_300_000_000)));
    // On one pair the spot list is the plain orders only.
    assert_eq!(spot.open_orders(&settings(), Some(SYM), CorrId(1)).unwrap().len(), 1);
}

#[test]
fn a_price_triggered_order_is_cancelled_on_its_own_endpoint() {
    let spot = Gateio::spot().unwrap();
    let c = spot.cancel(&settings(), SYM, "cond:1283293", CorrId(1)).unwrap();
    assert_eq!((c.method, c.path.as_str(), c.query.is_empty()), (Method::Delete, "/api/v4/spot/price_orders/1283293", true));
    // Read back on its own endpoint, its state the price order's.
    let q = spot.query_by_venue_id(&settings(), SYM, "cond:1283293", CorrId(1)).unwrap();
    assert_eq!((q.len(), q[0].method, q[0].path.as_str()), (1, Method::Get, "/api/v4/spot/price_orders/1283293"));
    for (word, st) in [("open", OrderStatus::New), ("cancelled", OrderStatus::Cancelled), ("finish", OrderStatus::Filled), ("failed", OrderStatus::Rejected), ("expired", OrderStatus::Expired)] {
        let body = format!(r#"{{"trigger":{{"price":"60000"}},"put":{{"type":"limit","side":"sell","price":"59900","amount":"0.2"}},"id":1283293,"market":"BTC_USDT","status":"{word}"}}"#);
        let (rows, next) = spot.decode_orders_in(&DecodeCtx::new(q[0].endpoint_id, &ContractSizes::new()), body.as_bytes()).unwrap();
        assert_eq!((rows[0].venue_id.as_str(), rows[0].status.clone(), next), ("cond:1283293", Mapped::Known(st), None), "{word}");
    }
    let fq = Gateio::futures().unwrap().query_by_venue_id(&settings(), SYM, "cond:77", CorrId(1)).unwrap();
    assert_eq!(fq[0].path, "/api/v4/futures/usdt/price_orders/77");
    let body = r#"{"initial":{"contract":"BTC_USDT","size":100,"price":"80000"},"trigger":{"price":"79000"},"id":77,"status":"finished","finish_as":"cancelled"}"#;
    let (rows, _) = Gateio::futures().unwrap().decode_orders_in(&DecodeCtx::new(fq[0].endpoint_id, &sizes()), body.as_bytes()).unwrap();
    assert_eq!((rows[0].status.clone(), rows[0].qty, rows[0].limit), (Mapped::Known(OrderStatus::Cancelled), d("0.01"), Some(d("80000"))));
    let fut = Gateio::futures().unwrap();
    let c = fut.cancel(&settings(), SYM, "cond:77", CorrId(1)).unwrap();
    assert_eq!((c.method, c.path.as_str()), (Method::Delete, "/api/v4/futures/usdt/price_orders/77"));
    // A plain id still goes to the plain endpoint.
    assert_eq!(fut.cancel(&settings(), SYM, "77", CorrId(1)).unwrap().path, "/api/v4/futures/usdt/orders/77");
}

#[test]
fn pages_move_by_offset_or_by_page() {
    let fut = Gateio::futures().unwrap();
    let first = fut.instrument_rules(&settings(), None, CorrId(1)).unwrap();
    let next = fut.with_cursor(&first, "next");
    assert!(next.query.contains(&("offset".to_string(), "100".to_string())));
    let spot = Gateio::spot().unwrap();
    let fills = spot.fills_since(&settings(), SYM, 1_000_500, 2_000_500, CorrId(1)).unwrap();
    assert!(fills.query.contains(&("from".to_string(), "1000".to_string())) && fills.query.contains(&("to".to_string(), "2001".to_string())));
    // The page is the documented maximum, so two pages (`limit * (page - 1) <= 1000`) hold 2000.
    assert!(fills.query.contains(&("limit".to_string(), "1000".to_string())));
    assert!(spot.with_cursor(&fills, "next").query.contains(&("page".to_string(), "2".to_string())));
    let page = format!("[{}]", vec![r#"{"id":"1","order_id":"2","currency_pair":"BTC_USDT","side":"buy","amount":"1","price":"1","create_time_ms":"1"}"#; 100].join(","));
    assert_eq!(spot.decode_fills_in(&DecodeCtx::new("spot_my_trades", &ContractSizes::new()), page.as_bytes()).unwrap().1, None, "100 rows is not a full page of 1000");
    // A day per window: rounded out to whole seconds, far inside the 30-day range.
    assert_eq!((spot.fills_window_ms(), fut.fills_window_ms()), (Some(86_400_000), None));
}

// ── Answers ──

#[test]
fn spot_answers_decode() {
    let m = Gateio::spot().unwrap();
    let ack = m.decode_ack(SPOT_ORDER.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("12332324", Some("t-123456")));
    let o = &m.decode_orders_in(&DecodeCtx::new("spot_order", &ContractSizes::new()), SPOT_ORDER.as_bytes()).unwrap().0[0];
    assert_eq!((o.status.clone(), o.qty, o.filled_qty, o.limit, o.avg_price, o.created_ms), (Mapped::Known(OrderStatus::PartiallyFilled), d("1"), d("0.5"), Some(d("5.00032")), Some(d("5.00032")), Some(1_548_000_000_123)));
    let (open, next) = m.decode_orders_in(&DecodeCtx::new("spot_open_orders", &ContractSizes::new()), SPOT_OPEN.as_bytes()).unwrap();
    assert_eq!((open.len(), open[0].status.clone(), next), (1, Mapped::Known(OrderStatus::New), None));
    let (fills, _) = m.decode_fills(SPOT_TRADES.as_bytes()).unwrap();
    let f = &fills[0];
    assert_eq!((f.exec_id.as_str(), f.venue_order_id.as_str(), f.qty, f.price, f.fee, f.fee_currency.as_str(), f.at_ms), ("1232893232", "4128442423", d("0.15"), d("7000"), d("0.0005"), "BTC", 1_548_000_000_123));
    assert_eq!(f.fees(), vec![(d("0.0005"), "BTC"), (d("0.02"), "GT")]);
    // A market buy's size is what filled: its `amount` is the quote coin spent.
    let mkt = with(SPOT_ORDER, |v| {
        v["type"] = json!("market");
        v["amount"] = json!("300");
        v["status"] = json!("closed");
    });
    let o = &m.decode_orders(mkt.as_bytes()).unwrap().0[0];
    assert_eq!((o.qty, o.limit, o.status.clone()), (d("0.5"), None, Mapped::Known(OrderStatus::Filled)));
}

#[test]
fn futures_answers_decode_in_the_base_coin() {
    let m = Gateio::futures().unwrap();
    let s = sizes();
    let ack = m.decode_ack(FUT_ORDER.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("15675394", Some("t-my-custom-id")));
    let o = &m.decode_orders_in(&DecodeCtx::new("futures_order", &s), FUT_ORDER.as_bytes()).unwrap().0[0];
    assert_eq!((o.side.clone(), o.qty, o.filled_qty, o.limit, o.status.clone(), o.created_ms), (Mapped::Known(Side::Buy), d("0.6024"), d("0"), Some(d("3765")), Mapped::Known(OrderStatus::New), Some(1_546_569_968_184)));
    let part = with(FUT_ORDER, |v| v["left"] = json!(24));
    assert_eq!(m.decode_orders_in(&DecodeCtx::new("futures_order", &s), part.as_bytes()).unwrap().0[0].status, Mapped::Known(OrderStatus::PartiallyFilled));
    // An unknown contract's sizes are never guessed.
    assert!(m.decode_orders(FUT_ORDER.as_bytes()).is_err());
    let (fills, _) = m.decode_fills_in(&DecodeCtx::new("futures_my_trades", &s), FUT_TRADES.as_bytes()).unwrap();
    assert_eq!((fills[0].side.clone(), fills[0].qty, fills[0].fee, fills[0].fee_currency.as_str(), fills[0].at_ms, fills[0].position_only), (Mapped::Known(Side::Sell), d("0.01"), d("0.01"), "USDT", 1_514_764_800_123, false));
    assert!(fills[1].position_only, "a deleveraging fill moved the position with no order of the account");
    assert_eq!(fills[1].extra_fees[0].currency, "POINT");
    let pos = m.decode_positions_in(&DecodeCtx::new("futures_position", &s), FUT_POSITION.as_bytes()).unwrap();
    assert_eq!((pos[0].side, pos[0].direction, pos[0].qty, pos[0].leverage, pos[0].margin.as_ref().map(|x| x.word())), (PositionSide::Net, Some(Side::Sell), d("0.944"), Some(d("25")), Some("cross")));
    assert_eq!(m.decode_account_position_mode(FUT_ACCOUNT.as_bytes()).unwrap(), Some(PositionMode::OneWay));
    let dual = with(FUT_ACCOUNT, |v| v["position_mode"] = json!("dual"));
    assert_eq!(m.decode_account_position_mode(dual.as_bytes()).unwrap(), Some(PositionMode::Hedge));
    let split = with(FUT_ACCOUNT, |v| v["position_mode"] = json!("dual_plus"));
    assert!(m.decode_account_position_mode(split.as_bytes()).is_err(), "split mode needs a position id OTW does not send");
    // Gate 3: in dual mode the contract's sides are read by the dual-mode call, a list of both
    // sides; single mode, or no mode held yet, keeps the single-mode call.
    let path = |mode| m.positions_in_mode(&settings(), "BTC_USDT", mode, CorrId(1)).unwrap().unwrap().path;
    assert_eq!(path(Some(PositionMode::Hedge)), "/api/v4/futures/usdt/dual_comp/positions/BTC_USDT");
    assert_eq!(path(Some(PositionMode::OneWay)), "/api/v4/futures/usdt/positions/BTC_USDT");
    assert_eq!(path(None), "/api/v4/futures/usdt/positions/BTC_USDT");
    let sides = with(FUT_POSITION, |v| {
        let mut long = v.clone();
        long["mode"] = json!("dual_long");
        long["size"] = json!(10);
        let mut short = v.clone();
        short["mode"] = json!("dual_short");
        short["size"] = json!(-20);
        *v = json!([long, short]);
    });
    let pos = m.decode_positions_in(&DecodeCtx::new("futures_dual_position", &s), sides.as_bytes()).unwrap();
    assert_eq!(pos.iter().map(|p| (p.side, p.qty)).collect::<Vec<_>>(), vec![(PositionSide::Long, d("0.001")), (PositionSide::Short, d("0.002"))]);
}

#[test]
fn instrument_lists_decode_to_sound_rules() {
    let (spot, next) = Gateio::spot().unwrap().decode_rules(SPOT_PAIRS.as_bytes()).unwrap();
    let r = &spot[0];
    assert_eq!((r.tick, r.lot_step, r.min_qty, r.max_qty, r.min_notional, r.max_notional, r.contract_size), (Some(d("0.1")), Some(d("0.0001")), Some(d("0.0001")), None, Some(d("10")), Some(d("5000000")), None));
    assert_eq!(r.quote_step, Some(d("0.1")), "a market buy's amount is rounded down to the quote precision");
    assert_eq!((r.status.clone(), next), (TradingStatus::Trading, None));
    r.sound().unwrap();
    assert_eq!(spot[1].status, TradingStatus::Closed("untradable".into()));
    let (fut, _) = Gateio::futures().unwrap().decode_rules(FUT_CONTRACTS.as_bytes()).unwrap();
    let r = &fut[0];
    assert_eq!((r.contract_size, r.tick, r.lot_step, r.min_qty, r.max_qty, r.market_max_qty), (Some(d("0.0001")), Some(d("0.1")), Some(d("0.0001")), Some(d("0.0001")), Some(d("100")), Some(d("50"))));
    r.sound().unwrap();
    assert_eq!(fut[1].status, TradingStatus::CloseOnly, "a delisting contract takes reduce-only and close orders only");
    // "`in_delisting=true` and position_size=0 indicates the contract is delisted".
    let gone = with(FUT_CONTRACTS, |v| {
        v[1]["position_size"] = json!(0);
        v[0]["position_size"] = json!(0);
    });
    let (fut, _) = Gateio::futures().unwrap().decode_rules(gone.as_bytes()).unwrap();
    assert_eq!((fut[0].status.clone(), fut[1].status.clone()), (TradingStatus::Trading, TradingStatus::Closed("delisted".into())));
    let held = with(FUT_CONTRACTS, |v| v[1]["position_size"] = json!(12));
    assert_eq!(Gateio::futures().unwrap().decode_rules(held.as_bytes()).unwrap().0[1].status, TradingStatus::CloseOnly, "still in its transition period");
}

#[test]
fn every_status_word_maps_and_an_unknown_one_stays_unknown() {
    let m = Gateio::spot().unwrap();
    for (w, s) in [
        ("open", OrderStatus::New),
        ("closed", OrderStatus::Filled),
        ("filled", OrderStatus::Filled),
        ("cancelled", OrderStatus::Cancelled),
        ("liquidate_cancelled", OrderStatus::Cancelled),
        ("small", OrderStatus::Cancelled),
        ("depth_not_enough", OrderStatus::Cancelled),
        ("trader_not_enough", OrderStatus::Cancelled),
        ("ioc", OrderStatus::Cancelled),
        ("poc", OrderStatus::Cancelled),
        ("fok", OrderStatus::Cancelled),
        ("stp", OrderStatus::Cancelled),
        ("price_protect_cancelled", OrderStatus::Cancelled),
        ("unified_check_failed", OrderStatus::Cancelled),
        ("liquidated", OrderStatus::Cancelled),
        ("auto_deleveraged", OrderStatus::Cancelled),
        ("reduce_only", OrderStatus::Cancelled),
        ("position_closed", OrderStatus::Cancelled),
        ("reduce_out", OrderStatus::Cancelled),
    ] {
        assert_eq!(m.status_of(w), Mapped::Known(s), "{w}");
    }
    for w in ["unknown", "-", "finished", "NoSuchWord"] {
        assert_eq!(m.status_of(w), Mapped::Unknown(w.into()));
    }
}

#[test]
fn the_private_streams_map_the_documented_frames() {
    let spot = Gateio::spot().unwrap();
    let s = spot.private_socket(&settings()).unwrap();
    assert_eq!((s.base, s.path.as_str(), s.ping_every_ms, s.more.len()), ("ws", "/ws/v4/", 10_000, 1));
    let sub: Value = serde_json::from_slice(&s.subscribe).unwrap();
    assert_eq!((sub["channel"].as_str(), sub["event"].as_str(), sub["payload"].clone()), (Some("spot.orders"), Some("subscribe"), json!(["!all"])));
    assert!(spot.private_socket_bootstrap(&settings(), &s, CorrId(1)).unwrap().is_none());
    let ev = spot.decode_private(WS_SPOT_ORDERS.as_bytes()).unwrap();
    let PrivateEvent::Order(o) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((o.status.clone(), o.client_id.as_deref(), o.side.clone(), o.qty), (Mapped::Known(OrderStatus::New), Some("t-testtext"), Mapped::Known(Side::Sell), d("0.0001")));
    let done = with(WS_SPOT_ORDERS, |v| {
        v["result"][0]["event"] = json!("finish");
        v["result"][0]["finish_as"] = json!("filled");
        v["result"][0]["filled_amount"] = json!("0.0001");
    });
    let PrivateEvent::Order(o) = &spot.decode_private(done.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!(o.status, Mapped::Known(OrderStatus::Filled));
    let ev = spot.decode_private(WS_SPOT_TRADES.as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = &ev[0] else { panic!("{ev:?}") };
    assert_eq!((f.exec_id.as_str(), f.qty, f.price, f.at_ms, f.client_id.clone()), ("5736713", d("1"), d("10000"), 1_605_176_741_123, None));
    // Answers: a subscribe accepted or refused, a pong.
    let ok = spot.decode_private(br#"{"time":1,"channel":"spot.orders","event":"subscribe","error":null,"result":{"status":"success"}}"#).unwrap();
    assert_eq!(ok, vec![PrivateEvent::Answer { op: "subscribe".into(), ok: true, message: String::new() }]);
    let no = spot.decode_private(br#"{"time":1,"channel":"spot.orders","event":"subscribe","error":{"code":4,"message":"invalid signature"},"result":{"status":"fail"}}"#).unwrap();
    assert!(matches!(&no[0], PrivateEvent::Answer { ok: false, message, .. } if message == "invalid signature"));
    assert!(matches!(&spot.decode_private(br#"{"time":1,"channel":"spot.pong","event":"","error":null,"result":null}"#).unwrap()[0], PrivateEvent::Answer { ok: true, .. }));
    // The other book's channels are not this one's.
    assert!(spot.decode_private(WS_FUT_ORDERS.as_bytes()).unwrap().is_empty());

    let fut = Gateio::futures().unwrap();
    let s = fut.private_socket(&settings()).unwrap();
    assert_eq!((s.base, s.path.as_str(), s.more.len()), ("fx-ws", "/v4/ws/usdt", 2));
    let boot = fut.private_socket_bootstrap(&settings(), &s, CorrId(1)).unwrap().unwrap();
    assert_eq!((boot.path.as_str(), boot.signed), ("/api/v4/account/detail", true));
    let s = fut.decode_private_socket_bootstrap(s, ACCOUNT_DETAIL.as_bytes()).unwrap();
    for f in s.subscribes() {
        let v: Value = serde_json::from_slice(f).unwrap();
        assert_eq!(v["payload"], json!(["1667201533", "!all"]), "{v}");
    }
    let held = sizes();
    let ctx = DecodeCtx::new("/v4/ws/usdt", &held);
    let PrivateEvent::Order(o) = &fut.decode_private_in(&ctx, WS_FUT_ORDERS.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((o.status.clone(), o.qty, o.filled_qty, o.avg_price), (Mapped::Known(OrderStatus::Filled), d("0.0001"), d("0.0001"), Some(d("40000.4"))));
    let PrivateEvent::Fill(f) = &fut.decode_private_in(&ctx, WS_FUT_TRADES.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((f.exec_id.as_str(), f.qty, f.fee_currency.as_str(), f.side.clone()), ("3335259", d("0.0001"), "USDT", Mapped::Known(Side::Buy)));
    let PrivateEvent::Position(p) = &fut.decode_private_in(&ctx, WS_FUT_POSITIONS.as_bytes()).unwrap()[0] else { panic!() };
    assert_eq!((p.side, p.qty, p.leverage), (PositionSide::Long, d("0.0003"), Some(d("10"))));
}

#[test]
fn errors_map_to_their_kind() {
    let m = Gateio::futures().unwrap();
    let e = |label: &str| m.decode_error(&otw_connector_testkit::http_frame(1, "futures_create", 400, &refusal(label, "m"))).unwrap();
    assert_eq!(e("INSUFFICIENT_AVAILABLE").canon, Canon::Rejected(Refusal::Funds));
    assert_eq!(e("ORDER_NOT_FOUND").canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(e("ORDER_EXISTS").canon, Canon::Unknown, "a duplicate: looked up, never resent");
    assert_eq!(e("REQUEST_EXPIRED").canon, Canon::Auth);
    assert!(!e("BRAND_NEW").mapped);
    assert!(m.invalidates_position_mode(&e("POSITION_DUAL_MODE")) && !m.invalidates_position_mode(&e("ORDER_NOT_FOUND")));
    for label in ["ORDER_PENDING", "POSITION_HOLDING", "POSITION_CROSS_MARGIN", "NO_MERGEABLE_ORDERS", "REQUEST_FORBIDDEN"] {
        assert_eq!((e(label).canon, e(label).mapped), (Canon::Rejected(Refusal::Other), true), "{label}: a clean refusal");
    }
    // A 404 with no label on a call by id is the order the venue does not know: a lookup moves on.
    for endpoint in ["futures_order", "spot_order", "futures_cancel", "spot_price_order", "futures_price_cancel"] {
        let e = m.decode_error(&otw_connector_testkit::http_frame(1, endpoint, 404, "")).unwrap();
        assert_eq!(e.canon, Canon::Rejected(Refusal::NotFound), "{endpoint}");
    }
    assert_eq!(m.decode_error(&otw_connector_testkit::http_frame(1, "futures_create", 404, "")).unwrap().canon, Canon::InvalidRequest);
    let url = m.decode_error(&otw_connector_testkit::http_frame(1, "futures_order", 404, &refusal("NOT_FOUND", "request URL not exists"))).unwrap();
    assert_eq!(url.canon, Canon::InvalidRequest, "a labelled wrong URL stays a bad request");
}

// ── Scenarios through the shared trader (spec 10.3) ──

fn matches(req: &Request, want: &HttpRequest) -> bool {
    let method = match want.method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };
    if req.method.as_str() != method || req.url.path() != want.path {
        return false;
    }
    let got: Vec<(String, String)> = req.url.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    want.query.iter().all(|p| got.contains(p))
}

/// A mock venue for one book, one-way and flat by default.
struct Env {
    m: Gateio,
    server: MockServer,
    driver: Driver<Gateio>,
    trader: Trader<Gateio>,
    settings: HashMap<String, String>,
    signer: Arc<dyn Signer>,
    scope: String,
}

impl Env {
    async fn new(m: Gateio, tag: &str) -> Env {
        let server = MockServer::start().await;
        let driver = otw_connector_testkit::driver_against(m.clone(), &server, otw_connector_testkit::ManualClock::at(otw_connector_testkit::ts(NOW_MS / 1000)));
        let settings = settings();
        let signer: Arc<dyn Signer> = Arc::new(GateSigner::from_secrets(&settings).unwrap());
        let e = Env { m, server, driver, trader: Trader::new(), settings, signer, scope: format!("gate:{tag}") };
        let rules = e.m.instrument_rules(&e.settings, Some(SYM), CorrId(1)).unwrap();
        e.mount(&rules, 200, if e.futures() { FUT_CONTRACTS.to_string() } else { SPOT_PAIRS.to_string() }, 9).await;
        if let Some(acct) = e.m.account_position_mode(&e.settings, CorrId(1)).unwrap() {
            e.mount(&acct, 200, FUT_ACCOUNT.to_string(), 9).await;
        }
        if let Some(pos) = e.m.positions(&e.settings, SYM, CorrId(1)).unwrap() {
            e.mount(&pos, 200, position(0), 9).await;
        }
        e
    }

    fn futures(&self) -> bool {
        self.m.counts_contracts()
    }

    fn account(&self) -> Account<'_> {
        Account { settings: &self.settings, signer: self.signer.as_ref(), scope: &self.scope }
    }

    async fn mount(&self, want: &HttpRequest, status: u16, body: String, priority: u8) {
        self.mount_h(want, status, body, Vec::new(), priority).await;
    }

    async fn mount_h(&self, want: &HttpRequest, status: u16, body: String, headers: Vec<(&'static str, String)>, priority: u8) {
        let want = want.clone();
        let mut t = ResponseTemplate::new(status).set_body_string(body).insert_header("content-type", "application/json");
        for (k, v) in headers {
            t = t.insert_header(k, v.as_str());
        }
        Mock::given(move |r: &Request| matches(r, &want)).respond_with(t).with_priority(priority).mount(&self.server).await;
    }

    async fn mount_body(&self, want: &HttpRequest, needle: &str, status: u16, body: String, priority: u8) {
        let (want, needle) = (want.clone(), needle.to_string());
        Mock::given(move |r: &Request| matches(r, &want) && String::from_utf8_lossy(&r.body).contains(&needle))
            .respond_with(ResponseTemplate::new(status).set_body_string(body).insert_header("content-type", "application/json"))
            .with_priority(priority)
            .mount(&self.server)
            .await;
    }

    async fn bodies(&self, want: &HttpRequest) -> Vec<String> {
        self.server.received_requests().await.unwrap_or_default().iter().filter(|r| matches(r, want)).map(|r| String::from_utf8_lossy(&r.body).to_string()).collect()
    }

    async fn count(&self, want: &HttpRequest) -> usize {
        self.bodies(want).await.len()
    }

    /// The create call (method and path).
    fn create(&self) -> HttpRequest {
        let p = placing(&self.m, "probe");
        let mut r = self.m.place(&self.settings, &order(&self.m, "probe", Side::Sell, OrderType::Limit, "0.01", Some("30000")), &p).unwrap();
        r.query.clear();
        r
    }

    async fn place(&self, orders: &[OrderRequest]) -> Result<Vec<otw_connector_engine::trading::Placed>, TradeError> {
        self.trader.place(&self.driver, &self.account(), orders, &TicketOptions::default()).await
    }

    async fn lookup_mount(&self, key: &str, row: Option<String>) {
        let cid = self.m.client_id(key).unwrap();
        let reqs = self.m.query_by_client_id(&self.settings, SYM, &cid, CorrId(1)).unwrap();
        match row {
            Some(r) => self.mount(&reqs[0], 200, r, 2).await,
            None => {
                self.mount(&reqs[0], 404, refusal("ORDER_NOT_FOUND", "Order not found"), 2).await;
                self.mount(&reqs[1], 200, "[]".into(), 2).await;
            }
        }
    }
}

/// An order row in the book's words: `word` a canonical state, sizes in the base coin.
fn row(m: &Gateio, cid: Option<&str>, vid: &str, side: Side, st: OrderStatus, qty: Decimal, filled: Decimal) -> Value {
    let text = cid.map(|c| format!("t-{c}")).unwrap_or_else(|| "web".into());
    if m.counts_contracts() {
        let c = |x: Decimal| (x / d("0.0001")).normalize();
        let sign = if side == Side::Buy { Decimal::ONE } else { -Decimal::ONE };
        let (status, finish) = match st {
            OrderStatus::Filled => ("finished", "filled"),
            OrderStatus::Cancelled => ("finished", "cancelled"),
            _ => ("open", ""),
        };
        return with(FUT_ORDER, |v| {
            v["id"] = json!(vid);
            v["text"] = json!(text);
            v["contract"] = json!(SYM);
            v["size"] = json!((c(qty) * sign).to_string().parse::<i64>().unwrap());
            v["left"] = json!(c(qty - filled).to_string().parse::<i64>().unwrap());
            v["status"] = json!(status);
            v["finish_as"] = json!(finish);
            v["price"] = json!("30000");
        })
        .parse()
        .unwrap();
    }
    let status = match st {
        OrderStatus::Filled => "closed",
        OrderStatus::Cancelled => "cancelled",
        _ => "open",
    };
    with(SPOT_ORDER, |v| {
        v["id"] = json!(vid);
        v["text"] = json!(text);
        v["currency_pair"] = json!(SYM);
        v["side"] = json!(if side == Side::Buy { "buy" } else { "sell" });
        v["status"] = json!(status);
        v["amount"] = json!(qty.to_string());
        v["filled_amount"] = json!(filled.to_string());
        v["left"] = json!((qty - filled).to_string());
        v["price"] = json!("30000");
    })
    .parse()
    .unwrap()
}

fn one(v: Value) -> String {
    v.to_string()
}

fn list(rows: Vec<Value>) -> String {
    Value::Array(rows).to_string()
}

fn ack(m: &Gateio, cid: &str, vid: &str) -> String {
    one(row(m, Some(cid), vid, Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO))
}

/// The position in base coins (positive long, negative short), one-way.
fn position(base: i64) -> String {
    with(FUT_POSITION, |v| {
        v["contract"] = json!(SYM);
        v["size"] = json!(base * 100);
        v["mode"] = json!("single");
    })
}

fn fill(m: &Gateio, exec: &str, vid: &str, cid: &str, qty: Decimal, px: Decimal, fee: Decimal) -> Value {
    if m.counts_contracts() {
        let mut v: Vec<Value> = serde_json::from_str(FUT_TRADES).unwrap();
        let mut f = v.remove(0);
        f["trade_id"] = json!(exec);
        f["order_id"] = json!(vid);
        f["text"] = json!(format!("t-{cid}"));
        f["size"] = json!((qty / d("0.0001")).normalize().to_string().parse::<i64>().unwrap());
        f["price"] = json!(px.to_string());
        f["fee"] = json!(fee.to_string());
        return f;
    }
    let mut v: Vec<Value> = serde_json::from_str(SPOT_TRADES).unwrap();
    let mut f = v.remove(0);
    f["id"] = json!(exec);
    f["order_id"] = json!(vid);
    f["text"] = json!(format!("t-{cid}"));
    f["currency_pair"] = json!(SYM);
    f["amount"] = json!(qty.to_string());
    f["price"] = json!(px.to_string());
    f["fee"] = json!(fee.to_string());
    f["fee_currency"] = json!("USDT");
    f["gt_fee"] = json!("0");
    f
}

type Outcome = Result<(), String>;

fn check(ok: bool, what: impl Into<String>) -> Outcome {
    if ok { Ok(()) } else { Err(what.into()) }
}

fn lim(m: &Gateio, key: &str) -> OrderRequest {
    order(m, key, Side::Buy, OrderType::Limit, "0.01", Some("30000"))
}

/// T01 place then cancel, sent once.
async fn t01(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t01").await;
    e.mount(&e.create(), 201, ack(m, "t01k", "901"), 3).await;
    let placed = e.place(&[lim(m, "t01k")]).await.map_err(|x| x.to_string())?;
    check(placed[0].venue_id == "901", format!("{placed:?}"))?;
    e.mount(&m.cancel(&e.settings, SYM, "901", CorrId(1)).unwrap(), 200, ack(m, "t01k", "901"), 3).await;
    let c = e.trader.cancel(&e.driver, &e.account(), SYM, "901").await.map_err(|x| x.to_string())?;
    check(c == Cancelled::Sent && e.count(&e.create()).await == 1, format!("{c:?}"))
}

/// T02 lookup by client id finds it filled.
async fn t02(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t02").await;
    e.lookup_mount("t02k", Some(one(row(m, Some("t02k"), "902", Side::Buy, OrderStatus::Filled, d("0.01"), d("0.01"))))).await;
    let got = e.trader.lookup(&e.driver, &e.account(), SYM, "t02k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::Filled) && got.filled_qty == d("0.01"), format!("{got:?}"))
}

/// T03 partial fills add up; T24 a duplicate execution is dropped by its id.
async fn t03(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t03").await;
    e.lookup_mount("t03k", Some(one(row(m, Some("t03k"), "903", Side::Buy, OrderStatus::PartiallyFilled, d("0.01"), d("0.005"))))).await;
    let got = e.trader.lookup(&e.driver, &e.account(), SYM, "t03k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::PartiallyFilled) && got.filled_qty == d("0.005"), format!("{got:?}"))?;
    let mut req = m.fills_since(&e.settings, SYM, 0, 1, CorrId(1)).unwrap();
    req.query.retain(|(k, _)| k != "from" && k != "to");
    let f = |id, px| fill(m, id, "903", "t03k", d("0.005"), d(px), d("0.01"));
    e.mount(&req, 200, list(vec![f("x1", "30000"), f("x2", "30000.2"), f("x2", "30000.2")]), 3).await;
    let fills = e.trader.fills_since(&e.driver, &e.account(), SYM, NOW_MS - 3_600_000).await.map_err(|x| x.to_string())?;
    let qty: Decimal = fills.iter().map(|f| f.qty).sum();
    let fees: Decimal = fills.iter().map(|f| f.fee).sum();
    check(fills.len() == 2 && qty == d("0.01") && fees == d("0.02"), format!("{fills:?}"))
}

/// T04 market buy and sell, and the fills' fee currency. A spot market buy goes out as the
/// amount to spend at the best ask read just before.
async fn t04(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t04").await;
    if let Some(t) = m.reference_price(&e.settings, SYM, Side::Buy, CorrId(1)).unwrap() {
        e.mount(&t, 200, SPOT_TICKER.to_string(), 3).await;
    }
    for (key, side) in [("t04b", Side::Buy), ("t04s", Side::Sell)] {
        e.mount_body(&e.create(), key, 201, ack(m, key, &format!("v-{key}")), 3).await;
        let placed = e.place(&[order(m, key, side, OrderType::Market, "0.01", None)]).await.map_err(|x| format!("{key}: {x}"))?;
        check(placed[0].venue_id == format!("v-{key}"), format!("{placed:?}"))?;
        if side == Side::Buy && !e.futures() {
            // 0.01 x 30000.5 = 300.005, rounded down to the pair's quote precision (1 place).
            check(placed[0].note.as_deref().is_some_and(|n| n.contains("out as 300 in")), format!("the note names the amount: {placed:?}"))?;
        }
    }
    let sent = e.bodies(&e.create()).await;
    if e.futures() {
        check(sent[0].contains(r#""size":100"#) && sent[1].contains(r#""size":-100"#) && sent.iter().all(|b| b.contains(r#""price":"0""#) && b.contains(r#""tif":"ioc""#)), format!("{sent:?}"))?;
    } else {
        check(sent[0].contains(r#""amount":"300""#) && sent[1].contains(r#""amount":"0.01""#), format!("{sent:?}"))?;
    }
    let mut req = m.fills_since(&e.settings, SYM, 0, 1, CorrId(1)).unwrap();
    req.query.retain(|(k, _)| k != "from" && k != "to");
    e.mount(&req, 200, list(vec![fill(m, "x", "v-t04b", "t04b", d("0.01"), d("30000"), Decimal::ONE)]), 3).await;
    let fills = e.trader.fills_since(&e.driver, &e.account(), SYM, NOW_MS - 3_600_000).await.map_err(|x| x.to_string())?;
    check(fills.first().is_some_and(|f| f.fee_currency == "USDT"), format!("{fills:?}"))
}

/// T09 IOC: the remainder ends cancelled with what filled.
async fn t09(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t09").await;
    let mut o = lim(m, "t09k");
    o.tif = TimeInForce::Ioc;
    e.mount(&e.create(), 201, ack(m, "t09k", "909"), 3).await;
    e.place(&[o]).await.map_err(|x| x.to_string())?;
    check(e.bodies(&e.create()).await[0].contains("\"ioc\""), "sent ioc")?;
    e.lookup_mount("t09k", Some(one(row(m, Some("t09k"), "909", Side::Buy, OrderStatus::Cancelled, d("0.01"), d("0.005"))))).await;
    let got = e.trader.lookup(&e.driver, &e.account(), SYM, "t09k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::Cancelled) && got.filled_qty == d("0.005"), format!("{got:?}"))
}

/// T10 close at the market: long, short, partial, reduce-only for the venue's size; more than
/// held is refused. Spot holds no positions: refused, nothing sent.
async fn t10(m: &Gateio) -> Outcome {
    if !m.counts_contracts() {
        let e = Env::new(m.clone(), "t10").await;
        let out = e.trader.close_position(&e.driver, &e.account(), SYM, None, None, "t10").await;
        return check(matches!(out, Err(TradeError::Refused(_))) && e.count(&e.create()).await == 0, format!("{out:?}"));
    }
    for (key, held, part, want) in [("t10l", 2, None, r#""size":-200"#), ("t10s", -2, None, r#""size":200"#), ("t10p", 2, Some(d("0.01")), r#""size":-100"#)] {
        let e = Env::new(m.clone(), key).await;
        e.mount(&m.positions(&e.settings, SYM, CorrId(1)).unwrap().unwrap(), 200, position(held), 1).await;
        e.mount(&e.create(), 201, ack(m, key, &format!("v-{key}")), 3).await;
        let placed = e.trader.close_position(&e.driver, &e.account(), SYM, None, part, key).await.map_err(|x| format!("{key}: {x}"))?;
        let sent = e.bodies(&e.create()).await;
        check(placed.venue_id == format!("v-{key}") && sent.len() == 1 && sent[0].contains(want) && sent[0].contains(r#""reduce_only":true"#) && sent[0].contains(r#""price":"0""#), format!("{key}: {sent:?}"))?;
    }
    let e = Env::new(m.clone(), "t10x").await;
    e.mount(&m.positions(&e.settings, SYM, CorrId(1)).unwrap().unwrap(), 200, position(1), 1).await;
    let out = e.trader.close_position(&e.driver, &e.account(), SYM, None, Some(d("0.02")), "t10x").await;
    check(matches!(out, Err(TradeError::Refused(_))) && e.count(&e.create()).await == 0, format!("more than held: {out:?}"))
}

async fn refused_before_io(e: &Env, orders: &[OrderRequest], what: &str) -> Outcome {
    let before = e.count(&e.create()).await;
    let out = e.place(orders).await;
    check(matches!(out, Err(TradeError::Refused(_))), format!("{what}: {out:?}"))?;
    check(e.count(&e.create()).await == before, format!("{what}: an order left"))
}

/// T11 the venue's rules refuse before I/O, naming the nearest values.
async fn t11(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t11").await;
    let base = lim(m, "t11");
    refused_before_io(&e, &[OrderRequest { limit: Some(Price::new(d("30000.05"))), ..base.clone() }], "off tick").await?;
    refused_before_io(&e, &[OrderRequest { qty: Qty::new(d("0.01005")), ..base.clone() }], "off lot").await?;
    let small = if e.futures() { "0.00005" } else { "0.0003" };
    refused_before_io(&e, &[OrderRequest { qty: Qty::new(d(small)), ..base.clone() }], "under the minimum").await?;
    if e.futures() {
        refused_before_io(&e, &[OrderRequest { qty: Qty::new(d("100.0001")), ..base.clone() }], "over max qty").await?;
    } else {
        refused_before_io(&e, &[OrderRequest { qty: Qty::new(d("200")), ..base.clone() }], "over max value").await?;
    }
    let out = e.place(&[OrderRequest { limit: Some(Price::new(d("30000.05"))), ..base }]).await;
    check(matches!(&out, Err(TradeError::Refused(x)) if x.message.contains("30000 or 30000.1")), format!("names the nearest: {out:?}"))
}

/// T12 every type, time in force and option the book does not declare is refused before I/O.
async fn t12(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t12").await;
    for t in [OrderType::Stop, OrderType::StopLimit, OrderType::Mit, OrderType::Lit] {
        refused_before_io(&e, &[order(m, "t12", Side::Buy, t, "0.01", Some("30000"))], &format!("{t:?}")).await?;
    }
    let mut trail = order(m, "t12t", Side::Buy, OrderType::TrailingStop, "0.01", None);
    trail.opts.trail = Some(otw_connector_model::Trail::Percent(Decimal::ONE));
    refused_before_io(&e, &[trail], "trailing").await?;
    for tif in [TimeInForce::Day, TimeInForce::Gtd(otw_connector_testkit::ts(1_900_000_000))] {
        refused_before_io(&e, &[OrderRequest { tif, ..lim(m, "t12d") }], &format!("{tif:?}")).await?;
    }
    let mut ice = lim(m, "t12i");
    ice.opts.display_qty = Some(Qty::new(d("0.005")));
    refused_before_io(&e, &[ice], "iceberg").await?;
    let mut post = lim(m, "t12p");
    post.opts.post_only = true;
    refused_before_io(&e, &[post], "post-only").await?;
    let mut ro = lim(m, "t12r");
    ro.opts.reduce_only = true;
    if !e.futures() {
        refused_before_io(&e, &[ro], "spot reduce-only").await?;
    }
    if e.futures() {
        let mut fok = order(m, "t12f", Side::Buy, OrderType::Market, "0.01", None);
        fok.tif = TimeInForce::Fok;
        refused_before_io(&e, &[fok], "futures market FOK").await?;
    }
    let entry = lim(m, "t12e");
    let exit = OrderRequest { parent: Some("t12e".into()), side: Side::Sell, ..lim(m, "t12x") };
    refused_before_io(&e, &[entry, exit], "attached exit").await
}

async fn venue_says(m: &Gateio, tag: &str, status: u16, body: String, headers: Vec<(&'static str, String)>) -> (Env, Result<Vec<otw_connector_engine::trading::Placed>, TradeError>) {
    let e = Env::new(m.clone(), tag).await;
    e.mount_h(&e.create(), status, body, headers, 3).await;
    let out = e.place(&[lim(m, &format!("{tag}k"))]).await;
    (e, out)
}

/// T15 funds: a refusal with Gate.io's label and words, sent once.
async fn t15(m: &Gateio) -> Outcome {
    let label = if m.counts_contracts() { "INSUFFICIENT_AVAILABLE" } else { "BALANCE_NOT_ENOUGH" };
    let (e, out) = venue_says(m, "t15", 400, refusal(label, "Balance is not enough"), vec![]).await;
    check(matches!(&out, Err(TradeError::Refused(x)) if x.message == format!("Gate.io refused it: Balance is not enough (code {label})") && x.provider_code.as_deref() == Some(label)), format!("{out:?}"))?;
    check(e.count(&e.create()).await == 1, "sent once")
}

/// T19 lost answer, the order at the venue: adopted, sent once.
async fn t19(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t19").await;
    e.mount(&e.create(), 502, "<html>502 Bad Gateway</html>".into(), 3).await;
    e.lookup_mount("t19k", Some(one(row(m, Some("t19k"), "919", Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO)))).await;
    let placed = e.place(&[lim(m, "t19k")]).await.map_err(|x| format!("adopted expected: {x}"))?;
    check(placed[0].venue_id == "919" && e.count(&e.create()).await == 1, format!("{placed:?}"))
}

/// T20 lost answer, never at the venue: unknown, and the venue answers never seen.
async fn t20(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t20").await;
    e.mount(&e.create(), 504, "".into(), 3).await;
    e.lookup_mount("t20k", None).await;
    let out = e.place(&[lim(m, "t20k")]).await;
    check(matches!(&out, Err(TradeError::Unknown { keys, alert: false, .. }) if keys == &vec!["t20k".to_string()]), format!("{out:?}"))?;
    let seen = e.trader.lookup(&e.driver, &e.account(), SYM, "t20k").await.map_err(|x| x.to_string())?;
    check(seen.is_none() && e.count(&e.create()).await == 1, "never seen, never resent")
}

/// T21 lost answer, the venue unreachable: unknown by its client id, nothing resent. A 202
/// ("accepted, processing not done") is the same.
async fn t21(m: &Gateio) -> Outcome {
    for (tag, status) in [("t21", 502), ("t21a", 202)] {
        let e = Env::new(m.clone(), tag).await;
        e.mount(&e.create(), status, "".into(), 3).await;
        for r in m.query_by_client_id(&e.settings, SYM, &m.client_id(&format!("{tag}k")).unwrap(), CorrId(1)).unwrap() {
            e.mount(&r, 503, "".into(), 2).await;
        }
        let out = e.place(&[lim(m, &format!("{tag}k"))]).await;
        check(matches!(&out, Err(TradeError::Unknown { keys, .. }) if keys == &vec![format!("{tag}k")]), format!("{tag}: {out:?}"))?;
        check(out.as_ref().err().is_some_and(|x| x.message().contains("do not send it again")) && e.count(&e.create()).await == 1, format!("{tag}: never resent"))?;
    }
    Ok(())
}

/// A label nobody mapped: unknown, and it alerts; an unknown status word is never a near match.
async fn unmapped(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "unm").await;
    e.mount(&e.create(), 400, refusal("SOMETHING_NEW", "new"), 3).await;
    e.lookup_mount("unmk", None).await;
    let out = e.place(&[lim(m, "unmk")]).await;
    check(matches!(&out, Err(TradeError::Unknown { alert: true, .. })), format!("{out:?}"))?;
    let mut odd = row(m, Some("unmw"), "9w", Side::Buy, OrderStatus::Filled, d("0.01"), d("0.01"));
    if e.futures() {
        odd["finish_as"] = json!("NoSuchWord");
    } else {
        odd["status"] = json!("NoSuchWord");
    }
    e.lookup_mount("unmw", Some(one(odd))).await;
    let got = e.trader.lookup(&e.driver, &e.account(), SYM, "unmw").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(matches!(got.status, Mapped::Unknown(_)), format!("{:?}", got.status))
}

/// T22 a cancel that raced a fill ends filled, no error.
async fn t22(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t22").await;
    let label = if e.futures() { "ORDER_FINISHED" } else { "ORDER_CLOSED" };
    e.mount(&m.cancel(&e.settings, SYM, "922", CorrId(1)).unwrap(), 400, refusal(label, "Order already finished"), 3).await;
    for r in m.query_by_venue_id(&e.settings, SYM, "922", CorrId(1)).unwrap() {
        e.mount(&r, 200, one(row(m, Some("t22k"), "922", Side::Buy, OrderStatus::Filled, d("0.01"), d("0.01"))), 2).await;
    }
    let out = e.trader.cancel(&e.driver, &e.account(), SYM, "922").await;
    check(out == Ok(Cancelled::AlreadyFinal(OrderStatus::Filled)), format!("{out:?}"))
}

/// T26 orders that worked while the socket was away: the venue's state is read and adopted.
async fn t26(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t26").await;
    e.lookup_mount("t26k", Some(one(row(m, Some("t26k"), "926", Side::Buy, OrderStatus::Filled, d("0.01"), d("0.01"))))).await;
    for r in m.open_orders(&e.settings, Some(SYM), CorrId(1)).unwrap() {
        e.mount(&r, 200, "[]".into(), 3).await;
    }
    let working = e.trader.open_orders(&e.driver, &e.account(), Some(SYM)).await.map_err(|x| x.to_string())?;
    let got = e.trader.lookup(&e.driver, &e.account(), SYM, "t26k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(working.is_empty() && got.status == Mapped::Known(OrderStatus::Filled), format!("{got:?}"))
}

/// T29 a timestamp outside the venue's 60 s: refused as auth, never unknown.
async fn t29(m: &Gateio) -> Outcome {
    let (_, out) = venue_says(m, "t29", 401, refusal("REQUEST_EXPIRED", "gap between request Timestamp and server time exceeds 60"), vec![]).await;
    check(matches!(&out, Err(TradeError::Refused(x)) if x.kind == ErrorKind::Auth), format!("{out:?}"))
}

/// T30 a rate limit refuses with the wait and holds the next order before it leaves.
async fn t30(m: &Gateio) -> Outcome {
    let reset = (NOW_MS / 1000 + 30).to_string();
    let (e, out) = venue_says(m, "t30", 429, refusal("TOO_MANY_REQUESTS", "Request Rate limit Exceeded"), vec![("X-Gate-RateLimit-Reset-Timestamp", reset)]).await;
    check(matches!(&out, Err(TradeError::Refused(x)) if x.kind == ErrorKind::RateLimited), format!("{out:?}"))?;
    let again = e.place(&[lim(m, "t30b")]).await;
    check(again.as_ref().err().is_some_and(|x| x.message().contains("holds new orders")), format!("{again:?}"))?;
    check(e.count(&e.create()).await == 1, "the second order never left")
}

/// T31 an order placed outside OTW is listed and cancelled by the kill switch; T34 every
/// working order is cancelled (spot: one call for the book, futures: one per contract).
async fn t31_34(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t31").await;
    let rows = vec![
        row(m, None, "ext-1", Side::Sell, OrderStatus::New, d("0.01"), Decimal::ZERO),
        row(m, Some("t34a"), "934a", Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO),
        row(m, Some("t34b"), "934b", Side::Buy, OrderStatus::PartiallyFilled, d("0.01"), d("0.005")),
    ];
    let first = if e.futures() { list(rows) } else { json!([{ "currency_pair": SYM, "total": 3, "orders": rows }]).to_string() };
    // A price-triggered order placed outside OTW works beside them.
    let trigger = if e.futures() {
        r#"[{"initial":{"contract":"BTC_USDT","size":-100,"price":"0","tif":"ioc"},"trigger":{"price":"25000","rule":2},"id":931,"status":"open","order_type":"plan-close-long-position"}]"#
    } else {
        r#"[{"trigger":{"price":"25000","rule":"<="},"put":{"type":"limit","side":"sell","price":"24900","amount":"0.01"},"id":931,"market":"BTC_USDT","status":"open"}]"#
    };
    let reqs = m.open_orders(&e.settings, None, CorrId(1)).unwrap();
    let mut lists = vec![(reqs[0].clone(), first)];
    if e.futures() {
        lists.push((reqs[1].clone(), trigger.to_string()));
    } else {
        // Spot asks the account's mode, then reads the list it names.
        e.mount(&reqs[1], 200, r#"{"mode":"classic"}"#.into(), 3).await;
        lists.push((m.with_cursor(&reqs[1], "classic"), trigger.to_string()));
    }
    for (r, body) in lists {
        let want = r.clone();
        Mock::given(move |q: &Request| matches(q, &want)).respond_with(ResponseTemplate::new(200).set_body_string(body)).up_to_n_times(1).with_priority(3).mount(&e.server).await;
        e.mount(&r, 200, "[]".into(), 4).await;
    }
    let symbols = if e.futures() { vec![SYM.to_string()] } else { Vec::new() };
    let cancels = m.cancel_all(&e.settings, &symbols, CorrId(1)).unwrap();
    for c in &cancels {
        let plain = c.path.ends_with("/orders");
        e.mount(c, 200, if e.futures() || !plain { "[]".into() } else { SPOT_CANCEL_ALL.to_string() }, 3).await;
    }
    let n = e.trader.cancel_all(&e.driver, &e.account()).await.map_err(|x| x.to_string())?;
    check(n == 4, format!("four were working, the external ones included: {n}"))?;
    check(e.count(&cancels[0]).await == 1, "one cancel-all call")?;
    check(e.count(&cancels[1]).await == 1, "one cancel-all call for the price-triggered orders")
}

/// T32 after a restart nothing is resent: the stored client id is looked up and adopted.
async fn t32(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "t32").await;
    e.lookup_mount("t32k", Some(one(row(m, Some("t32k"), "932", Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO)))).await;
    let fresh: Trader<Gateio> = Trader::new();
    let got = fresh.lookup(&e.driver, &e.account(), SYM, "t32k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(got.venue_id == "932" && e.count(&e.create()).await == 0, format!("{got:?}"))
}

/// Spec 6.1: a leg refused after another went out cancels it.
async fn atomic(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "atom").await;
    e.mount_body(&e.create(), "atoma", 201, ack(m, "atoma", "9a"), 3).await;
    e.mount_body(&e.create(), "atomb", 400, refusal("BALANCE_NOT_ENOUGH", "Balance is not enough"), 3).await;
    let cancel = m.cancel(&e.settings, SYM, "9a", CorrId(1)).unwrap();
    e.mount(&cancel, 200, ack(m, "atoma", "9a"), 3).await;
    let out = e.place(&[lim(m, "atoma"), lim(m, "atomb")]).await;
    check(matches!(&out, Err(TradeError::Refused(x)) if x.message.contains("atoma: placed, then cancelled")), format!("{out:?}"))?;
    check(e.count(&cancel).await == 1, "the placed leg was cancelled")
}

/// Futures: the position mode is the account's, read once and held; dual mode sends the
/// documented sign; `POSITION_DUAL_MODE` reads it again before the next order.
async fn position_mode(m: &Gateio) -> Outcome {
    if !m.counts_contracts() {
        return Ok(());
    }
    let e = Env::new(m.clone(), "pm").await;
    let acct = m.account_position_mode(&e.settings, CorrId(1)).unwrap().unwrap();
    e.mount(&acct, 200, with(FUT_ACCOUNT, |v| v["position_mode"] = json!("dual")), 1).await;
    for key in ["pm1", "pm2"] {
        e.mount_body(&e.create(), key, 201, ack(m, key, &format!("v-{key}")), 3).await;
        e.place(&[order(m, key, Side::Sell, OrderType::Limit, "0.01", Some("30000"))]).await.map_err(|x| x.to_string())?;
    }
    check(e.count(&acct).await == 1, "the mode is held, not read before each order")?;
    check(e.bodies(&e.create()).await.iter().all(|b| b.contains(r#""size":-100"#)), "a dual-mode sell opens the short by its sign")?;
    e.mount_body(&e.create(), "pm3", 400, refusal("POSITION_DUAL_MODE", "Operation forbidden in dual-mode"), 3).await;
    let out = e.place(&[lim(m, "pm3")]).await;
    check(matches!(out, Err(TradeError::Refused(_))), format!("{out:?}"))?;
    e.mount_body(&e.create(), "pm4", 201, ack(m, "pm4", "v-pm4"), 3).await;
    e.place(&[lim(m, "pm4")]).await.map_err(|x| x.to_string())?;
    check(e.count(&acct).await == 2, "read again once after the refusal")?;
    // Split mode: refused before the order leaves, naming the fix.
    let e = Env::new(m.clone(), "pm5").await;
    e.mount(&acct, 200, with(FUT_ACCOUNT, |v| v["position_mode"] = json!("dual_plus")), 1).await;
    refused_before_io(&e, &[lim(m, "pm5")], "split mode").await
}

/// A change in place: futures the price (`PUT`), spot one field through the batch amend.
async fn amend(m: &Gateio) -> Outcome {
    let e = Env::new(m.clone(), "amd").await;
    for r in m.query_by_venue_id(&e.settings, SYM, "950", CorrId(1)).unwrap() {
        e.mount(&r, 200, one(row(m, Some("amdk"), "950", Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO)), 2).await;
    }
    let cur = VenueOrder { venue_id: "950".into(), symbol: SYM.into(), ..m.decode_orders_in(&DecodeCtx::new("x", &sizes()), one(row(m, None, "950", Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO)).as_bytes()).unwrap().0.remove(0) };
    let change = OrderAmend { limit: Some(Price::new(d("29999.9"))), ..Default::default() };
    let mut call = m.amend(&e.settings, &cur, &change, &Placing::new("", PositionSide::Net, CorrId(1))).unwrap();
    call.query.clear();
    let ok = if e.futures() { one(row(m, Some("amdk"), "950", Side::Buy, OrderStatus::New, d("0.01"), Decimal::ZERO)) } else { r#"[{"order_id":"950","succeeded":true,"label":"","message":""}]"#.to_string() };
    e.mount(&call, 200, ok, 3).await;
    let out = e.trader.modify(&e.driver, &e.account(), SYM, "950", &change).await.map_err(|x| x.to_string())?;
    check(out.is_none() && e.bodies(&call).await.iter().any(|b| b.contains("29999.9")), format!("{out:?}"))?;
    let off = OrderAmend { limit: Some(Price::new(d("29999.95"))), ..Default::default() };
    let out = e.trader.modify(&e.driver, &e.account(), SYM, "950", &off).await;
    check(matches!(out, Err(TradeError::Refused(_))) && e.count(&call).await == 1, format!("off tick refused before I/O: {out:?}"))
}

async fn run(m: Gateio) -> Vec<&'static str> {
    let mut ran = Vec::new();
    let mut failures = Vec::new();
    macro_rules! go {
        ($id:expr, $f:expr) => {
            match $f.await {
                Ok(()) => ran.push($id),
                Err(e) => failures.push(format!("{}: {e}", $id)),
            }
        };
    }
    go!("T01", t01(&m));
    go!("T02", t02(&m));
    go!("T03+T24", t03(&m));
    go!("T04", t04(&m));
    go!("T09", t09(&m));
    go!("T10", t10(&m));
    go!("T11", t11(&m));
    go!("T12", t12(&m));
    go!("T15", t15(&m));
    go!("T19", t19(&m));
    go!("T20", t20(&m));
    go!("T21", t21(&m));
    go!("unmapped", unmapped(&m));
    go!("T22", t22(&m));
    go!("T26", t26(&m));
    go!("T29", t29(&m));
    go!("T30", t30(&m));
    go!("T31+T34", t31_34(&m));
    go!("T32", t32(&m));
    go!("6.1 atomicity", atomic(&m));
    go!("position mode", position_mode(&m));
    go!("amend", amend(&m));
    assert!(failures.is_empty(), "{} scenarios failed:\n{}", m.capabilities().label, failures.join("\n"));
    ran
}

#[tokio::test]
async fn the_scenarios_pass_on_spot() {
    let ran = run(Gateio::spot().unwrap()).await;
    assert_eq!(ran.len(), 22, "{ran:?}");
}

#[tokio::test]
async fn the_scenarios_pass_on_futures() {
    let ran = run(Gateio::futures().unwrap()).await;
    assert_eq!(ran.len(), 22, "{ran:?}");
}

#[test]
fn both_books_declare_e2d_and_only_what_the_doc_states() {
    let spot = Gateio::spot().unwrap();
    let fut = Gateio::futures().unwrap();
    let (s, f) = (&spot.capabilities().orders, &fut.capabilities().orders);
    assert_eq!((s.level, f.level), (ExecLevel::E2D, ExecLevel::E2D));
    assert!(!s.reduce_only && s.position_mode.is_none() && f.reduce_only && f.position_mode.is_some());
    for c in [s, f] {
        assert!(!c.brackets && !c.post_only && !c.oca && !c.iceberg && c.amend && c.client_id && c.cancel_all_native && !c.types.contains(&OrderType::Stop));
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

/// The live lists decode to sound rules for every instrument, and an order call signed with a
/// key Gate.io never issued, or not signed at all, is answered with the error the table
/// expects (nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers gateio::trade::tests::public` (add the test harness flag `nocapture` to see the counts).
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_sdk::DriverConfig;
    if std::env::var("OTW_NET_TEST").as_deref() != Ok("1") {
        return;
    }
    for m in [Gateio::spot().unwrap(), Gateio::futures().unwrap()] {
        let label = m.capabilities().label.clone();
        let d = Driver::new(m.clone(), DriverConfig::new("gateio"), reqwest::Client::new());
        let junk = settings();
        let signer = GateSigner::from_secrets(&junk).unwrap();
        let a = Account { settings: &junk, signer: &signer, scope: "net" };
        let trader: Trader<Gateio> = Trader::new();
        let rules = trader.all_rules(&d, &a).await.unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(rules.len() > 10, "{label}: {} instruments", rules.len());
        for r in &rules {
            r.sound().unwrap_or_else(|e| panic!("{label}: {e}"));
        }
        let btc = rules.iter().find(|r| r.instrument.native == SYM).unwrap_or_else(|| panic!("{label}: no {SYM}"));
        println!("{label}: {} instruments, {SYM} {btc:?}", rules.len());
        assert!(btc.tick.is_some() && btc.lot_step.is_some());
        let mut p = placing(&m, "otw-net-probe");
        p.contract_size = btc.contract_size;
        let req = m.place(&junk, &order(&m, "otw-net-probe", Side::Buy, OrderType::Limit, "0.001", Some("1000")), &p).unwrap();
        for (what, signed) in [("junk key", true), ("unsigned", false)] {
            let mut r = req.clone();
            r.signed = signed;
            let frame = d.send_as(r, Some(&signer)).await.unwrap_or_else(|e| panic!("{label} {what}: {e}"));
            let e = m.decode_error(&frame).unwrap_or_else(|| panic!("{label} {what}: Gate.io accepted an order with no valid key"));
            println!("{label} {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
            assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{label} {what}: {e:?}");
        }
    }
}
