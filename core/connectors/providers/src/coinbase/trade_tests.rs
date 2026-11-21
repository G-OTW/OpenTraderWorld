//! Coinbase Advanced Trade orders, from the documentation alone. Fixtures under
//! `fixtures/trade/`:
//!
//! | File | Source |
//! | --- | --- |
//! | `sandbox_create.json` | Advanced Trade sandbox, `POST https://api-sandbox.coinbase.com/api/v3/brokerage/orders`, captured 2026-11-12 ([Sandbox](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/sandbox): "All responses are static and pre-defined") |
//! | `sandbox_create_insufficient_fund.json` | the same, header `X-Sandbox: PostOrder_insufficient_fund` |
//! | `sandbox_batch_cancel.json` | sandbox `POST /orders/batch_cancel` |
//! | `sandbox_batch_cancel_failure.json` | the same, `X-Sandbox: CancelOrders_failure` |
//! | `sandbox_edit.json` | sandbox `POST /orders/edit` |
//! | `sandbox_edit_failure.json` | the same, `X-Sandbox: EditOrder_failure` |
//! | `sandbox_preview.json` | sandbox `POST /orders/preview` (body without `client_order_id`) |
//! | `sandbox_preview_insufficient_fund.json` | the same, `X-Sandbox: PreviewOrder_insufficient_fund` |
//! | `sandbox_list_orders.json` | sandbox `GET /orders/historical/batch` |
//! | `sandbox_list_orders_open.json` | sandbox `GET /orders/historical/batch?order_status=OPEN` |
//! | `sandbox_get_order.json` | sandbox `GET /orders/historical/ff6e8dad-22c4-422d-bbf2-3c55b70e42d2` |
//! | `sandbox_get_order_invalid_id.json` | sandbox `GET /orders/historical/abc` (HTTP 400) |
//! | `sandbox_fills.json` | sandbox `GET /orders/historical/fills` |
//! | `doc_faq_success_unknown_reason.json` | [FAQ](https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/faq), "Why am I receiving an `UNKNOWN_FAILURE_REASON`", verbatim |
//! | `live_market_products_btc.json` | `GET https://api.coinbase.com/api/v3/brokerage/market/products?product_type=SPOT&product_ids=BTC-USD&product_ids=BTC-USDC` (public, no key), captured 2026-11-12 |
//! | `synthetic_ws_user_update.json` | **synthetic**: the `user` message's fields as the [AsyncAPI](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/websocket/user) lists them ("No examples found") |
//! | `synthetic_ws_heartbeat.json` | **synthetic**: the [heartbeats](https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/websocket/heartbeats) example with its `<string>` placeholders filled in |
//!
//! The sandbox answers are the only answers obtainable without an account: "the same formatted
//! responses as production", canned. The catalog script ([`Script`]) answers with them, only
//! their ids, sizes, prices, symbol and status changed: those answers are `synthetic`, built on
//! the captured shape. The HTTP 401, 429 and 400 bodies are `synthetic` too (the gRPC gateway's
//! `{error, code, message, details}` shape from the OpenAPI `default` response, its words doc
//! silent).

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TimeInForce, Timestamp};
use otw_connector_sdk::trade::{Canon, CancelAllWay, ContractSizes, DecodeCtx, LookupAt, Placing, PositionSide, PrivateEvent, Refusal, TradeMapper, TradingStatus, VenueOrder};
use otw_connector_sdk::{Mapper, Method, Signer};
use otw_connector_testkit::trading::{run_catalog, Answer, FillRow, Row, VenueRefusal, VenueScript};
use secrecy::SecretString;
use serde_json::{json, Value};

use super::super::signer::CoinbaseSigner;
use super::super::Coinbase;

const CREATE: &str = include_str!("fixtures/trade/sandbox_create.json");
const CREATE_FUNDS: &str = include_str!("fixtures/trade/sandbox_create_insufficient_fund.json");
const CANCEL: &str = include_str!("fixtures/trade/sandbox_batch_cancel.json");
const CANCEL_FAILED: &str = include_str!("fixtures/trade/sandbox_batch_cancel_failure.json");
const EDIT_FAILED: &str = include_str!("fixtures/trade/sandbox_edit_failure.json");
const PREVIEW: &str = include_str!("fixtures/trade/sandbox_preview.json");
const LIST: &str = include_str!("fixtures/trade/sandbox_list_orders.json");
const OPEN: &str = include_str!("fixtures/trade/sandbox_list_orders_open.json");
const GET: &str = include_str!("fixtures/trade/sandbox_get_order.json");
const FILLS: &str = include_str!("fixtures/trade/sandbox_fills.json");
const FAQ: &str = include_str!("fixtures/trade/doc_faq_success_unknown_reason.json");
const PRODUCTS: &str = include_str!("fixtures/trade/live_market_products_btc.json");
const WS_USER: &str = include_str!("fixtures/trade/synthetic_ws_user_update.json");
const WS_HEARTBEAT: &str = include_str!("fixtures/trade/synthetic_ws_heartbeat.json");

/// RFC 8032 test vector 1's seed: a key made for tests, nothing else.
const SEED: &str = "nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A=";
const KEY_NAME: &str = "organizations/00000000-0000-0000-0000-000000000000/apiKeys/00000000-0000-0000-0000-000000000000";

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn m() -> Coinbase {
    Coinbase::new().unwrap()
}

fn settings() -> HashMap<String, String> {
    [("key_name", KEY_NAME), ("api_private_key", SEED)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn signer() -> CoinbaseSigner {
    CoinbaseSigner::new(KEY_NAME.into(), SecretString::from(SEED.to_string())).unwrap()
}

fn order(key: &str, side: Side, t: OrderType, qty: &str, price: Option<&str>) -> OrderRequest {
    let (l, s) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("COINBASE", "BTC-USD"),
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

fn body(req: &otw_connector_sdk::HttpRequest) -> Value {
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

fn placing(cid: &str) -> Placing {
    Placing::new(cid, PositionSide::Net, CorrId(1))
}

fn config(o: &OrderRequest) -> Value {
    body(&m().place(&settings(), o, &placing(&o.client_order_id)).unwrap())["order_configuration"].clone()
}

fn with(template: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(template).unwrap();
    f(&mut v);
    v.to_string()
}

fn ctx_free() -> ContractSizes {
    ContractSizes::new()
}

// ── Requests ──

/// The documented create body: `client_order_id`, `product_id`, `side`, one configuration
/// key (the sandbox echoes the same `limit_limit_gtc {base_size, limit_price}` it was sent).
#[test]
fn a_limit_order_is_the_documented_body() {
    let req = m().place(&settings(), &order("otw-1", Side::Sell, OrderType::Limit, "1", Some("70000.44")), &placing("otw-1")).unwrap();
    assert_eq!((req.method, req.path.as_str(), req.signed, req.base), (Method::Post, "/api/v3/brokerage/orders", true, "brokerage"));
    assert_eq!(
        body(&req),
        json!({ "client_order_id": "otw-1", "product_id": "BTC-USD", "side": "SELL", "order_configuration": { "limit_limit_gtc": { "base_size": "1", "limit_price": "70000.44" } } })
    );
    assert_eq!(req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1);
}

#[test]
fn each_time_in_force_picks_its_configuration() {
    let lim = |tif: TimeInForce| OrderRequest { tif, ..order("k", Side::Buy, OrderType::Limit, "0.01", Some("30000")) };
    assert!(config(&lim(TimeInForce::Gtc))["limit_limit_gtc"].is_object());
    assert!(config(&lim(TimeInForce::Ioc))["sor_limit_ioc"].is_object());
    assert!(config(&lim(TimeInForce::Fok))["limit_limit_fok"].is_object());
    let gtd = config(&lim(TimeInForce::Gtd(Timestamp::from_secs(1_900_000_000).unwrap())));
    assert_eq!(gtd["limit_limit_gtd"]["end_time"].as_str(), Some("2030-03-17T17:46:40Z"));
    // A market order, buy or sell, is sized in the base coin and works IOC.
    for side in [Side::Buy, Side::Sell] {
        assert_eq!(config(&order("k", side, OrderType::Market, "0.01", None)), json!({ "market_market_ioc": { "base_size": "0.01" } }));
    }
    // Post-only rides on GTC and GTD limits only.
    let mut post = lim(TimeInForce::Gtc);
    post.opts.post_only = true;
    assert_eq!(config(&post)["limit_limit_gtc"]["post_only"], json!(true));
    let mut post_ioc = lim(TimeInForce::Ioc);
    post_ioc.opts.post_only = true;
    assert!(m().place(&settings(), &post_ioc, &placing("k")).is_err());
}

#[test]
fn a_stop_limit_triggers_in_its_sides_direction_and_a_limit_if_touched_the_other_way() {
    let dir = |side, t| {
        let c = config(&order("k", side, t, "0.01", Some("31000")));
        let (key, f) = c.as_object().unwrap().iter().next().map(|(k, v)| (k.clone(), v.clone())).unwrap();
        assert_eq!(key, "stop_limit_stop_limit_gtc");
        assert_eq!((f["stop_price"].as_str(), f["limit_price"].as_str()), (Some("31000"), Some("31000")));
        f["stop_direction"].as_str().unwrap().to_string()
    };
    assert_eq!(dir(Side::Buy, OrderType::StopLimit), "STOP_DIRECTION_STOP_UP");
    assert_eq!(dir(Side::Sell, OrderType::StopLimit), "STOP_DIRECTION_STOP_DOWN");
    assert_eq!(dir(Side::Buy, OrderType::Lit), "STOP_DIRECTION_STOP_DOWN");
    assert_eq!(dir(Side::Sell, OrderType::Lit), "STOP_DIRECTION_STOP_UP");
    let gtd = OrderRequest { tif: TimeInForce::Gtd(Timestamp::from_secs(1_900_000_000).unwrap()), ..order("k", Side::Buy, OrderType::StopLimit, "0.01", Some("31000")) };
    assert!(config(&gtd)["stop_limit_stop_limit_gtd"]["end_time"].is_string());
}

#[test]
fn what_the_configurations_cannot_say_is_refused_before_io() {
    let mm = m();
    let with_tif = |t, tif| OrderRequest { tif, ..order("k", Side::Buy, t, "0.01", Some("30000")) };
    for (t, tif) in [
        (OrderType::Market, TimeInForce::Fok),
        (OrderType::Market, TimeInForce::Gtd(Timestamp::from_secs(1_900_000_000).unwrap())),
        (OrderType::StopLimit, TimeInForce::Ioc),
        (OrderType::Lit, TimeInForce::Fok),
    ] {
        assert_eq!(mm.check_order(&with_tif(t, tif)).unwrap_err().kind, otw_connector_model::ErrorKind::Unsupported, "{t:?} {tif:?}");
    }
    assert!(mm.check_order(&with_tif(OrderType::Market, TimeInForce::Ioc)).is_ok());
    // Stop-market, trailing, if-touched market and reduce-only have no configuration.
    for t in [OrderType::Stop, OrderType::Mit, OrderType::TrailingStop] {
        assert!(mm.place(&settings(), &order("k", Side::Buy, t, "0.01", Some("30000")), &placing("k")).is_err(), "{t:?}");
    }
    let mut ro = order("k", Side::Sell, OrderType::Limit, "0.01", Some("30000"));
    ro.opts.reduce_only = true;
    assert!(mm.place(&settings(), &ro, &placing("k")).is_err());
    assert!(mm.place(&settings(), &order("k", Side::Buy, OrderType::Limit, "0.01", Some("1")), &placing("")).is_err(), "a create needs its client id");
}

#[test]
fn the_preview_is_the_order_without_its_client_id() {
    let o = order("otw-dry", Side::Buy, OrderType::Limit, "0.01", Some("30000"));
    let req = m().dry_run(&settings(), &o, &placing("")).unwrap().unwrap();
    assert_eq!((req.method, req.path.as_str()), (Method::Post, "/api/v3/brokerage/orders/preview"));
    let b = body(&req);
    assert!(b.get("client_order_id").is_none(), "{b}");
    assert!(b["order_configuration"]["limit_limit_gtc"].is_object());
    assert_eq!(m().decode_dry_run(PREVIEW.as_bytes()).unwrap(), "order total 5.04, commission 0.04, best bid 65769.37, best ask 65769.38");
}

#[test]
fn a_cancel_is_a_batch_of_one_and_the_kill_switch_batches_ids() {
    let mm = m();
    let c = mm.cancel(&settings(), "BTC-USD", "f898eaf4-6ffc-47be-a159-7ff292e5cdcf", CorrId(1)).unwrap();
    assert_eq!((c.path.as_str(), body(&c)), ("/api/v3/brokerage/orders/batch_cancel", json!({ "order_ids": ["f898eaf4-6ffc-47be-a159-7ff292e5cdcf"] })));
    assert!(mm.cancel(&settings(), "BTC-USD", "../accounts", CorrId(1)).is_err(), "an id lands in no path unchecked");
    assert_eq!(mm.cancel_all_way(), CancelAllWay::ByIds { max: 50 });
    let row = |id: &str| VenueOrder { venue_id: id.into(), ..mm.decode_orders(GET.as_bytes()).unwrap().0[0].clone() };
    let batch = mm.cancel_ids(&settings(), &[row("a"), row("b")], CorrId(1)).unwrap();
    assert_eq!(body(&batch), json!({ "order_ids": ["a", "b"] }));
    let many: Vec<VenueOrder> = (0..51).map(|i| row(&format!("o{i}"))).collect();
    assert!(mm.cancel_ids(&settings(), &many, CorrId(1)).is_err());
    let rows = mm.decode_cancel_rows(&DecodeCtx::new("order_cancel_batch", &ctx_free()), CANCEL_FAILED.as_bytes()).unwrap();
    assert_eq!(rows[0].venue_id, "f898eaf4-6ffc-47be-a159-7ff292e5cdcf");
    assert_eq!(rows[0].error.as_ref().map(|e| e.canon), Some(Canon::Unknown));
    let rows = mm.decode_cancel_rows(&DecodeCtx::new("order_cancel_batch", &ctx_free()), CANCEL.as_bytes()).unwrap();
    assert!(rows[0].error.is_none(), "success true with the enum's default reason is done");
}

#[test]
fn a_change_is_an_edit_of_a_limit_order_in_place() {
    let mm = m();
    let cur = mm.decode_orders(GET.as_bytes()).unwrap().0.remove(0);
    let change = OrderAmend { limit: Some(Price::new(d("70001"))), ..Default::default() };
    let req = mm.amend(&settings(), &cur, &change, &placing("")).unwrap();
    assert_eq!((req.path.as_str(), body(&req)), ("/api/v3/brokerage/orders/edit", json!({ "order_id": "ff6e8dad-22c4-422d-bbf2-3c55b70e42d2", "price": "70001", "size": "1" })));
    assert!(!mm.amend_replaces());
    let qty = OrderAmend { qty: Some(Qty::new(d("0.5"))), ..Default::default() };
    assert_eq!(body(&mm.amend(&settings(), &cur, &qty, &placing("")).unwrap())["size"], json!("0.5"));
    // A stop price, a time in force, a stop-limit or an order sized in the quote currency: refused.
    assert!(mm.amend(&settings(), &cur, &OrderAmend { stop: Some(Price::new(d("1"))), ..Default::default() }, &placing("")).is_err());
    let stop = VenueOrder { order_type: "STOP_LIMIT".into(), ..cur.clone() };
    assert!(mm.amend(&settings(), &stop, &change, &placing("")).is_err());
    let quoted = with(GET, |v| {
        v["order"]["size_in_quote"] = json!(true);
        v["order"]["order_configuration"] = json!({ "limit_limit_gtc": { "quote_size": "50", "limit_price": "70000" } });
    });
    let q = mm.decode_orders(quoted.as_bytes()).unwrap().0.remove(0);
    assert!(mm.amend(&settings(), &q, &change, &placing("")).is_err());
    // A limit entry with an attached take profit and stop loss is still a limit order.
    let bracketed = with(GET, |v| v["order"]["attached_order_configuration"] = json!({ "trigger_bracket_gtc": { "limit_price": "99990", "stop_trigger_price": "99800" } }));
    let b = mm.decode_orders(bracketed.as_bytes()).unwrap().0.remove(0);
    assert_eq!(b.order_type, "LIMIT with take profit 99990 and stop loss 99800 attached");
    assert!(mm.amend(&settings(), &b, &change, &placing("")).is_ok());
}

#[test]
fn a_lookup_lists_the_products_orders_from_just_before_the_order() {
    let mm = m();
    let plain = mm.query_by_client_id(&settings(), "BTC-USD", "otw-1", CorrId(1)).unwrap();
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].path, "/api/v3/brokerage/orders/historical/batch");
    assert!(plain[0].query.contains(&("product_ids".into(), "BTC-USD".into())));
    assert!(!plain[0].query.iter().any(|(k, _)| k == "order_status"), "every state: a filled or cancelled order is found too");
    let at = LookupAt { created_ms: Some(1_767_225_600_000), now_ms: 1_767_225_700_000 };
    let timed = mm.query_by_client_id_at(&settings(), "BTC-USD", "otw-1", &at, CorrId(1)).unwrap();
    assert!(timed[0].query.contains(&("start_date".into(), "2025-12-31T23:59:00Z".into())), "{:?}", timed[0].query);
    assert_eq!(mm.lookup_pages(), 5);
    let byid = mm.query_by_venue_id(&settings(), "BTC-USD", "ff6e8dad-22c4-422d-bbf2-3c55b70e42d2", CorrId(1)).unwrap();
    assert_eq!(byid[0].path, "/api/v3/brokerage/orders/historical/ff6e8dad-22c4-422d-bbf2-3c55b70e42d2");
    // Open orders: every working status, once per placement source of the enum, every page.
    let open = mm.open_orders(&settings(), None, CorrId(1)).unwrap();
    let sources: Vec<String> = open.iter().filter_map(|r| r.query.iter().find(|(k, _)| k == "order_placement_source").map(|(_, v)| v.clone())).collect();
    assert_eq!(sources, ["RETAIL_ADVANCED", "RETAIL_SIMPLE", "RETAIL_ADMIN", "RETAIL_RAISE", "UNKNOWN_PLACEMENT_SOURCE"]);
    for r in &open {
        let statuses: Vec<&str> = r.query.iter().filter(|(k, _)| k == "order_status").map(|(_, v)| v.as_str()).collect();
        assert_eq!(statuses, ["OPEN", "PENDING", "QUEUED", "CANCEL_QUEUED", "EDIT_QUEUED"]);
    }
    let next = mm.with_cursor(&open[0], "c2");
    assert!(next.query.contains(&("cursor".into(), "c2".into())));
    // Fills: spot only, the window in RFC 3339.
    let f = mm.fills_since(&settings(), "BTC-USD", 1_767_225_600_000, 1_767_229_200_000, CorrId(1)).unwrap();
    for p in [("product_types", "SPOT"), ("product_ids", "BTC-USD"), ("start_sequence_timestamp", "2026-01-01T00:00:00Z"), ("end_sequence_timestamp", "2026-01-01T01:00:00Z"), ("limit", "100")] {
        assert!(f.query.contains(&(p.0.to_string(), p.1.to_string())), "{p:?} in {:?}", f.query);
    }
    // Rules: the public list, no key.
    let r = mm.instrument_rules(&settings(), Some("BTC-USD"), CorrId(1)).unwrap();
    assert_eq!((r.path.as_str(), r.signed), ("/api/v3/brokerage/market/products", false));
}

// ── Answers ──

#[test]
fn the_sandbox_and_faq_acks_name_the_order() {
    let ack = m().decode_ack(CREATE.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("f898eaf4-6ffc-47be-a159-7ff292e5cdcf", Some("sandbox_success_order")));
    let ack = m().decode_ack(FAQ.as_bytes()).unwrap();
    assert_eq!((ack.venue_id.as_str(), ack.client_id.as_deref()), ("62475e97-7485-45eb-a5ac-594805fa2748", Some("12345678")));
    assert!(m().decode_ack(CREATE_FUNDS.as_bytes()).is_err());
}

#[test]
fn the_sandbox_order_lists_decode() {
    let mm = m();
    let (rows, next) = mm.decode_orders(LIST.as_bytes()).unwrap();
    assert!(next.is_none());
    let by = |id: &str| rows.iter().find(|r| r.venue_id == id).unwrap();
    let open = by("ff6e8dad-22c4-422d-bbf2-3c55b70e42d2");
    assert_eq!(
        (open.client_id.as_deref(), open.side.clone(), open.status.clone(), open.qty, open.filled_qty, open.limit, open.order_type.as_str()),
        (Some("spot_order_2"), Mapped::Known(Side::Sell), Mapped::Known(OrderStatus::New), d("1"), d("0"), Some(d("70000.44")), "LIMIT")
    );
    assert_eq!(open.created_ms, Some(1_719_587_807_696));
    assert_eq!(by("aa934bbb-1261-4d9f-8556-f7273aa0f5b3").status, Mapped::Known(OrderStatus::Cancelled));
    // OPEN with a fill is partly filled.
    let part = by("c6981abf-a8b4-4209-b33f-2aa6ee8cd72f");
    assert_eq!((part.status.clone(), part.filled_qty), (Mapped::Known(OrderStatus::PartiallyFilled), d("0.01")));
    // Its attached take profit and stop loss (`attached_order_configuration`) are said.
    assert_eq!(part.order_type, "LIMIT with take profit 99990 and stop loss 99800 attached");
    // The sandbox's attached TP/SL row: its type word kept verbatim, its stop the bracket's
    // `stop_trigger_price`.
    let tpsl = by("3daff354-c513-4c9d-869e-6ebf223364f4");
    assert_eq!((tpsl.order_type.as_str(), tpsl.status.clone()), ("TAKE_PROFIT_STOP_LOSS", Mapped::Known(OrderStatus::Filled)));
    assert_eq!((tpsl.limit, tpsl.stop), (Some(d("99990")), Some(d("99800"))));
    // A list withheld behind a proof token (EU SCA) is an error, never an empty list.
    let sca = with(OPEN, |v| v["proof_token_required"] = json!(true));
    assert!(mm.decode_orders(sca.as_bytes()).unwrap_err().message.contains("proof token"));
    let sca = with(FILLS, |v| v["proof_token_required"] = json!(true));
    assert!(mm.decode_fills(sca.as_bytes()).unwrap_err().message.contains("proof token"));
    let (one, _) = mm.decode_orders(GET.as_bytes()).unwrap();
    assert_eq!((one.len(), one[0].venue_id.as_str()), (1, "ff6e8dad-22c4-422d-bbf2-3c55b70e42d2"));
    let (open_page, _) = mm.decode_orders(OPEN.as_bytes()).unwrap();
    assert!(open_page.iter().all(|o| o.status == Mapped::Known(OrderStatus::New)));
    let paged = with(OPEN, |v| {
        v["has_next"] = json!(true);
        v["cursor"] = json!("next-1");
    });
    assert_eq!(mm.decode_orders(paged.as_bytes()).unwrap().1.as_deref(), Some("next-1"));
}

#[test]
fn every_status_word_maps_and_an_unknown_one_stays_unknown() {
    let mm = m();
    for (w, s) in [
        ("PENDING", OrderStatus::PendingNew),
        ("QUEUED", OrderStatus::PendingNew),
        ("OPEN", OrderStatus::New),
        ("FILLED", OrderStatus::Filled),
        ("CANCELLED", OrderStatus::Cancelled),
        ("EXPIRED", OrderStatus::Expired),
        ("FAILED", OrderStatus::Rejected),
        ("CANCEL_QUEUED", OrderStatus::PendingCancel),
        ("EDIT_QUEUED", OrderStatus::PendingReplace),
    ] {
        assert_eq!(mm.status_of(w), Mapped::Known(s), "{w}");
    }
    assert_eq!(mm.status_of("UNKNOWN_ORDER_STATUS"), Mapped::Unknown("UNKNOWN_ORDER_STATUS".into()));
    let row = |f: &dyn Fn(&mut Value)| mm.decode_orders(with(GET, |v| f(&mut v["order"])).as_bytes()).unwrap().0.remove(0);
    assert_eq!(row(&|o| o["pending_cancel"] = json!(true)).status, Mapped::Known(OrderStatus::PendingCancel));
    let stop = row(&|o| {
        o["order_type"] = json!("STOP_LIMIT");
        o["trigger_status"] = json!("STOP_PENDING");
        o["order_configuration"] = json!({ "stop_limit_stop_limit_gtc": { "base_size": "1", "limit_price": "60000", "stop_price": "61000", "stop_direction": "STOP_DIRECTION_STOP_DOWN" } });
    });
    assert_eq!((stop.trigger_pending, stop.status.clone(), stop.stop, stop.limit), (true, Mapped::Known(OrderStatus::New), Some(d("61000")), Some(d("60000"))));
}

/// An order placed in the quote currency states no base size until it fills: its size is what
/// it filled, never one computed from the quote amount, and its type says so.
#[test]
fn an_order_sized_in_the_quote_currency_invents_no_base_size() {
    let o = with(GET, |v| {
        v["order"]["size_in_quote"] = json!(true);
        v["order"]["filled_size"] = json!("0.0002");
        v["order"]["order_type"] = json!("MARKET");
        v["order"]["order_configuration"] = json!({ "market_market_ioc": { "quote_size": "50" } });
    });
    let r = m().decode_orders(o.as_bytes()).unwrap().0.remove(0);
    assert_eq!((r.qty, r.filled_qty, r.order_type.as_str()), (d("0.0002"), d("0.0002"), "MARKET (50 in the quote currency)"));
}

#[test]
fn the_sandbox_fills_decode_with_the_fee_as_stated() {
    let mm = m();
    let (fills, next) = mm.decode_fills(FILLS.as_bytes()).unwrap();
    assert!(next.is_none());
    let f = &fills[0];
    assert_eq!(
        (f.exec_id.as_str(), f.venue_order_id.as_str(), f.qty, f.price, f.fee, f.side.clone(), f.at_ms),
        ("f8b3aa2ba0825457bad20056c52cbe51177af5dd2f66dc55445640f95658f1f3", "37f17b89-cd9e-41eb-9187-2df6aa72f895", d("0.00137199"), d("70000.44"), d("0.3841596147024"), Mapped::Known(Side::Sell), 1_719_520_326_568)
    );
    // A rebate written negative stays a rebate: never made a cost.
    let rebate = with(FILLS, |v| v["fills"][0]["commission"] = json!("-0.12"));
    assert_eq!(mm.decode_fills(rebate.as_bytes()).unwrap().0[0].fee, d("-0.12"));
    // An adjusted fill is never read as a fill: it is left out, and the others are kept.
    for kind in ["REVERSAL", "CORRECTION", "SYNTHETIC"] {
        let adj = with(FILLS, |v| v["fills"][1]["trade_type"] = json!(kind));
        let all = mm.decode_fills(FILLS.as_bytes()).unwrap().0;
        let (kept, _) = mm.decode_fills(adj.as_bytes()).unwrap();
        assert_eq!((kept.len(), kept.iter().any(|f| f.exec_id == all[1].exec_id)), (all.len() - 1, false), "{kind}");
    }
    // A size placed in the quote currency, read in the coin as the account read does.
    let quoted = with(FILLS, |v| {
        v["fills"][1]["size_in_quote"] = json!(true);
        v["fills"][1]["size"] = json!("600.0044");
    });
    assert_eq!(mm.decode_fills(quoted.as_bytes()).unwrap().0[1].qty, d("0.01"));
    // A full page goes on by its cursor; a short one ends.
    let full = with(FILLS, |v| {
        let row = v["fills"][0].clone();
        v["fills"] = json!((0..100).map(|i| { let mut r = row.clone(); r["entry_id"] = json!(format!("e{i}")); r }).collect::<Vec<_>>());
        v["cursor"] = json!("c1");
    });
    assert_eq!(mm.decode_fills(full.as_bytes()).unwrap().1.as_deref(), Some("c1"));
}

#[test]
fn the_public_product_list_gives_sound_rules_and_its_states() {
    let (rules, next) = m().decode_rules(PRODUCTS.as_bytes()).unwrap();
    assert!(next.is_none());
    let r = &rules[0];
    assert_eq!(r.instrument.native, "BTC-USD");
    assert_eq!(
        (r.tick, r.lot_step, r.quote_step, r.min_qty, r.max_qty, r.min_notional, r.status.clone()),
        (Some(d("0.01")), Some(d("0.00000001")), Some(d("0.01")), Some(d("0.00000001")), Some(d("3400")), Some(d("1")), TradingStatus::Trading)
    );
    r.sound().unwrap();
    let state = |f: &dyn Fn(&mut Value)| m().decode_rules(with(PRODUCTS, |v| f(&mut v["products"][0])).as_bytes()).unwrap().0.remove(0).status;
    assert_eq!(state(&|p| p["limit_only"] = json!(true)), TradingStatus::LimitOnly("limit_only".into()));
    assert_eq!(state(&|p| p["post_only"] = json!(true)), TradingStatus::PostOnly("post_only".into()));
    assert_eq!(state(&|p| p["cancel_only"] = json!(true)), TradingStatus::Closed("cancel_only".into()));
    assert_eq!(state(&|p| p["trading_disabled"] = json!(true)), TradingStatus::Closed("trading_disabled".into()));
    assert_eq!(state(&|p| p["status"] = json!("delisted")), TradingStatus::Closed("delisted".into()));
    let paged = with(PRODUCTS, |v| v["pagination"] = json!({ "next_cursor": "QlRDLVVTRA==", "has_next": true }));
    assert_eq!(m().decode_rules(paged.as_bytes()).unwrap().1.as_deref(), Some("QlRDLVVTRA=="));
}

#[test]
fn the_user_socket_subscribes_user_then_heartbeats_and_reads_orders_again_by_id() {
    let mm = m();
    let s = mm.private_socket(&settings()).unwrap();
    assert_eq!((s.base, s.path.as_str()), ("ws_user", ""));
    let frames: Vec<Value> = s.subscribes().iter().map(|f| serde_json::from_slice(f).unwrap()).collect();
    // `user` first: a heartbeat (every second once subscribed) is never taken for its answer.
    assert_eq!(frames, vec![json!({ "type": "subscribe", "channel": "user" }), json!({ "type": "subscribe", "channel": "heartbeats" })]);
    assert!(s.ping.is_empty(), "no client heartbeat is documented");
    assert_eq!(mm.base_urls().iter().find(|(k, _)| *k == "ws_user").map(|(_, v)| *v), Some("wss://advanced-trade-ws-user.coinbase.com"));
    // Each signed frame carries its JWT.
    let signed: Value = serde_json::from_slice(&signer().sign_frame(bytes::Bytes::from(s.more[0].clone()), Timestamp::from_secs(1_700_000_000).unwrap()).unwrap()).unwrap();
    assert!(signed["jwt"].as_str().is_some_and(|j| j.split('.').count() == 3));
    let ev = mm.decode_private(WS_HEARTBEAT.as_bytes()).unwrap();
    assert!(matches!(&ev[..], [PrivateEvent::Answer { ok: true, .. }]), "{ev:?}");
    let ev = mm.decode_private(br#"{"channel":"subscriptions","events":[{"subscriptions":{"user":["x"]}}]}"#).unwrap();
    assert!(matches!(&ev[..], [PrivateEvent::Answer { ok: true, .. }]), "{ev:?}");
    // A `subscriptions` frame without `user` answers nothing: the user subscribe waits on.
    assert!(mm.decode_private(br#"{"channel":"subscriptions","events":[{"subscriptions":{"heartbeats":["heartbeats"]}}]}"#).unwrap().is_empty());
    let ev = mm.decode_private(br#"{"type":"error","message":"authentication failure"}"#).unwrap();
    assert!(matches!(&ev[..], [PrivateEvent::Answer { ok: false, message, .. }] if message == "authentication failure"), "{ev:?}");
    let ev = mm.decode_private(WS_USER.as_bytes()).unwrap();
    assert_eq!(ev, vec![PrivateEvent::OrderChanged { symbol: Some("BTC-USD".into()), venue_id: "ff6e8dad-22c4-422d-bbf2-3c55b70e42d2".into() }]);
    // The snapshot answers the user subscribe; its orders are the book already read.
    let snapshot = WS_USER.replace("\"update\"", "\"snapshot\"");
    let ev = mm.decode_private(snapshot.as_bytes()).unwrap();
    assert!(matches!(&ev[..], [PrivateEvent::Answer { ok: true, .. }]), "{ev:?}");
    assert!(mm.decode_private(br#"{"channel":"status","events":[]}"#).unwrap().is_empty());
}

#[test]
fn the_venue_is_never_a_sandbox_and_a_wrong_edit_is_not_found() {
    assert!(!m().is_sandbox(&settings()));
    let e = m().decode_error(&otw_connector_testkit::http_frame(1, "order_edit", 200, EDIT_FAILED)).unwrap();
    assert_eq!(e.canon, Canon::Rejected(Refusal::NotFound));
    assert_eq!(m().error_table().len(), super::super::trade_errors::ERRORS.len());
}

/// A lost answer on a busy product: the order is on the second page of the list, found by
/// following the cursor (the real mapper, the shared trader, a mock venue); a list that runs out
/// of pages before the order is "not found", never a guess.
#[tokio::test]
async fn a_lookup_follows_the_cursor_to_the_order() {
    use otw_connector_engine::trading::{Account, Trader};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let driver = otw_connector_testkit::driver_against(m(), &server, otw_connector_testkit::ManualClock::at(otw_connector_testkit::ts(1_767_225_600)));
    let page = |client: &str, id: &str, has_next: bool, cursor: &str| {
        with(OPEN, |v| {
            v["orders"][0]["client_order_id"] = json!(client);
            v["orders"][0]["order_id"] = json!(id);
            v["has_next"] = json!(has_next);
            v["cursor"] = json!(cursor);
        })
    };
    let list = "/api/v3/brokerage/orders/historical/batch";
    Mock::given(method("GET")).and(path(list)).and(query_param("cursor", "c2")).respond_with(ResponseTemplate::new(200).set_body_string(page("otw-mine", "mine-1", false, ""))).with_priority(1).mount(&server).await;
    Mock::given(method("GET")).and(path(list)).respond_with(ResponseTemplate::new(200).set_body_string(page("someone-else", "other-1", true, "c2"))).with_priority(2).mount(&server).await;
    let s = settings();
    let sig = signer();
    let a = Account { settings: &s, signer: &sig, scope: "lookup" };
    let trader: Trader<Coinbase> = Trader::new();
    let found = trader.lookup(&driver, &a, "BTC-USD", "otw-mine").await.unwrap().unwrap();
    assert_eq!(found.venue_id, "mine-1");
    let none = trader.lookup(&driver, &a, "BTC-USD", "otw-absent").await.unwrap();
    assert!(none.is_none());
    let asked = server.received_requests().await.unwrap();
    assert!(asked.iter().all(|r| r.url.query().unwrap_or("").contains("product_ids=BTC-USD")));
}

// ── The catalog ──

/// The real mapper, but for one thing: the catalog mounts each lookup's answer on the lookup's
/// own request, and Coinbase's lookup by client id is a product's order list that names no client
/// id (the same request for every order). This wrapper adds a `catalog_client` query pair to the
/// lookup, so each scenario's answer lands on its own order and never on the open-order list.
/// Every other call, and every decoder, is the real mapper's.
#[derive(Clone)]
struct Catalog(Coinbase);

type R<T> = Result<T, otw_connector_model::ConnectorError>;

impl Mapper for Catalog {
    type Session = super::super::Session;

    fn capabilities(&self) -> &otw_connector_model::Capabilities {
        self.0.capabilities()
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.0.base_urls()
    }

    fn encode(&self, cmd: &otw_connector_model::Command, s: &mut Self::Session, ctx: &otw_connector_sdk::MapCtx<'_>) -> R<Vec<otw_connector_sdk::NativeRequest>> {
        self.0.encode(cmd, s, ctx)
    }

    fn decode(&self, frame: &otw_connector_sdk::RawFrame, s: &mut Self::Session, ctx: &otw_connector_sdk::MapCtx<'_>) -> R<Vec<otw_connector_model::Envelope>> {
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
    fn client_id_rule(&self) -> otw_connector_sdk::trade::ClientIdRule {
        self.0.client_id_rule()
    }
    fn place(&self, s: &HashMap<String, String>, o: &OrderRequest, p: &Placing) -> R<otw_connector_sdk::HttpRequest> {
        self.0.place(s, o, p)
    }
    fn amend(&self, s: &HashMap<String, String>, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> R<otw_connector_sdk::HttpRequest> {
        self.0.amend(s, cur, change, p)
    }
    fn cancel(&self, s: &HashMap<String, String>, symbol: &str, id: &str, corr: CorrId) -> R<otw_connector_sdk::HttpRequest> {
        self.0.cancel(s, symbol, id, corr)
    }
    fn cancel_all_way(&self) -> CancelAllWay {
        self.0.cancel_all_way()
    }
    fn cancel_ids(&self, s: &HashMap<String, String>, orders: &[VenueOrder], corr: CorrId) -> R<otw_connector_sdk::HttpRequest> {
        self.0.cancel_ids(s, orders, corr)
    }
    fn decode_cancel_rows(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> R<Vec<otw_connector_sdk::trade::CancelRow>> {
        self.0.decode_cancel_rows(ctx, body)
    }
    fn query_by_client_id(&self, s: &HashMap<String, String>, symbol: &str, cid: &str, corr: CorrId) -> R<Vec<otw_connector_sdk::HttpRequest>> {
        Ok(self.0.query_by_client_id(s, symbol, cid, corr)?.into_iter().map(|r| r.query("catalog_client", cid)).collect())
    }
    fn query_by_client_id_at(&self, s: &HashMap<String, String>, symbol: &str, cid: &str, at: &LookupAt, corr: CorrId) -> R<Vec<otw_connector_sdk::HttpRequest>> {
        Ok(self.0.query_by_client_id_at(s, symbol, cid, at, corr)?.into_iter().map(|r| r.query("catalog_client", cid)).collect())
    }
    fn lookup_pages(&self) -> usize {
        self.0.lookup_pages()
    }
    fn query_by_venue_id(&self, s: &HashMap<String, String>, symbol: &str, id: &str, corr: CorrId) -> R<Vec<otw_connector_sdk::HttpRequest>> {
        self.0.query_by_venue_id(s, symbol, id, corr)
    }
    fn open_orders(&self, s: &HashMap<String, String>, symbol: Option<&str>, corr: CorrId) -> R<Vec<otw_connector_sdk::HttpRequest>> {
        self.0.open_orders(s, symbol, corr)
    }
    fn fills_since(&self, s: &HashMap<String, String>, symbol: &str, from: i64, to: i64, corr: CorrId) -> R<otw_connector_sdk::HttpRequest> {
        self.0.fills_since(s, symbol, from, to, corr)
    }
    fn instrument_rules(&self, s: &HashMap<String, String>, symbol: Option<&str>, corr: CorrId) -> R<otw_connector_sdk::HttpRequest> {
        self.0.instrument_rules(s, symbol, corr)
    }
    fn check_order(&self, o: &OrderRequest) -> R<()> {
        self.0.check_order(o)
    }
    fn dry_run(&self, s: &HashMap<String, String>, o: &OrderRequest, p: &Placing) -> R<Option<otw_connector_sdk::HttpRequest>> {
        self.0.dry_run(s, o, p)
    }
    fn decode_dry_run(&self, body: &[u8]) -> R<String> {
        self.0.decode_dry_run(body)
    }
    fn decode_error(&self, frame: &otw_connector_sdk::RawFrame) -> Option<otw_connector_sdk::trade::VenueError> {
        self.0.decode_error(frame)
    }
    fn decode_ack(&self, body: &[u8]) -> R<otw_connector_sdk::trade::Ack> {
        self.0.decode_ack(body)
    }
    fn decode_orders(&self, body: &[u8]) -> R<otw_connector_sdk::trade::Page<VenueOrder>> {
        self.0.decode_orders(body)
    }
    fn decode_fills(&self, body: &[u8]) -> R<otw_connector_sdk::trade::Page<otw_connector_sdk::trade::VenueFill>> {
        self.0.decode_fills(body)
    }
    fn decode_rules(&self, body: &[u8]) -> R<otw_connector_sdk::trade::Page<otw_connector_sdk::trade::VenueRules>> {
        self.0.decode_rules(body)
    }
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        self.0.status_of(word)
    }
    fn error_table(&self) -> &'static [otw_connector_sdk::trade::ErrorRow] {
        self.0.error_table()
    }
}

/// The catalog's answers: the sandbox's with their ids, sizes and states changed (`synthetic`,
/// built on the captured shape).
struct Script;

fn first(template: &str, list: &str) -> Value {
    let v: Value = serde_json::from_str(template).unwrap();
    v[list][0].clone()
}

impl VenueScript for Script {
    type M = Catalog;

    fn mapper(&self) -> Catalog {
        Catalog(m())
    }

    fn settings(&self) -> HashMap<String, String> {
        settings()
    }

    fn signer(&self) -> Arc<dyn Signer> {
        Arc::new(signer())
    }

    fn symbol(&self) -> &'static str {
        "BTC-USD"
    }

    fn qty(&self) -> Decimal {
        d("0.01")
    }

    fn price(&self) -> Decimal {
        d("30000")
    }

    fn rules(&self, closed: Option<&str>) -> String {
        match closed {
            None => PRODUCTS.to_string(),
            Some(w) => with(PRODUCTS, |v| v["products"][0]["status"] = json!(w)),
        }
    }

    fn closed_word(&self) -> &'static str {
        "delisted"
    }

    fn word(&self, s: OrderStatus) -> &'static str {
        match s {
            OrderStatus::PendingNew => "PENDING",
            OrderStatus::Filled => "FILLED",
            OrderStatus::Cancelled => "CANCELLED",
            OrderStatus::Rejected => "FAILED",
            OrderStatus::Expired => "EXPIRED",
            OrderStatus::PendingCancel => "CANCEL_QUEUED",
            OrderStatus::PendingReplace => "EDIT_QUEUED",
            // Partly filled is OPEN with `filled_size` above zero.
            _ => "OPEN",
        }
    }

    fn ack(&self, client_id: &str, venue_id: &str) -> String {
        with(CREATE, |v| {
            v["success_response"]["order_id"] = json!(venue_id);
            v["success_response"]["client_order_id"] = json!(client_id);
        })
    }

    fn orders(&self, rows: &[Row<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut o = first(OPEN, "orders");
                o["order_id"] = json!(r.venue_id);
                o["client_order_id"] = json!(r.client_id.unwrap_or(""));
                o["product_id"] = json!("BTC-USD");
                o["side"] = json!(if r.side == Side::Buy { "BUY" } else { "SELL" });
                o["status"] = json!(r.word);
                o["order_configuration"]["limit_limit_gtc"]["base_size"] = json!(r.qty.to_string());
                o["order_configuration"]["limit_limit_gtc"]["limit_price"] = json!("30000");
                o["filled_size"] = json!(r.filled.to_string());
                o
            })
            .collect();
        with(OPEN, |v| {
            v["orders"] = json!(list);
            v["has_next"] = json!(false);
            v["cursor"] = json!("");
        })
    }

    fn fills(&self, rows: &[FillRow<'_>]) -> String {
        let list: Vec<Value> = rows
            .iter()
            .map(|r| {
                let mut f = first(FILLS, "fills");
                f["entry_id"] = json!(r.exec_id);
                f["order_id"] = json!(r.venue_id);
                f["product_id"] = json!("BTC-USD");
                f["size"] = json!(r.qty.to_string());
                f["price"] = json!(r.price.to_string());
                f["commission"] = json!(r.fee.to_string());
                f
            })
            .collect();
        with(FILLS, |v| {
            v["fills"] = json!(list);
            v["cursor"] = json!("");
        })
    }

    /// No fill states its fee currency (doc silent): the trade path leaves it unsaid.
    fn fee_currency(&self) -> &'static str {
        ""
    }

    fn positions(&self, _rows: &[(PositionSide, Option<Side>, Decimal)]) -> String {
        "{}".into()
    }

    fn ok(&self) -> String {
        CANCEL.to_string()
    }

    fn dry_run_ok(&self) -> String {
        PREVIEW.to_string()
    }

    fn refusal(&self, r: VenueRefusal) -> Answer {
        let gateway = |error: &str, code: i64, message: &str| json!({ "error": error, "code": code, "message": message, "details": [] }).to_string();
        let create_failed = |reason: &str| json!({ "success": false, "error_response": { "new_order_failure_reason": reason, "message": "m", "error_details": "" } }).to_string();
        match r {
            VenueRefusal::Funds => Answer::new(200, CREATE_FUNDS),
            VenueRefusal::ReduceOnly => Answer::new(200, create_failed("REDUCE_ONLY_NOT_ALLOWED_ON_SPOT_PRODUCTS")),
            VenueRefusal::NotFound => Answer::new(200, EDIT_FAILED),
            VenueRefusal::Final => Answer::new(200, json!({ "results": [{ "success": false, "failure_reason": "ORDER_IS_FULLY_FILLED", "order_id": "v-t22" }] }).to_string()),
            VenueRefusal::RateLimited(s) => Answer { status: 429, body: gateway("RESOURCE_EXHAUSTED", 8, "too many requests"), headers: vec![("Retry-After", s.to_string())] },
            VenueRefusal::ClockSkew => Answer::new(401, gateway("UNAUTHENTICATED", 16, "Unauthorized")),
            VenueRefusal::Unmapped => Answer::new(200, create_failed("SOMETHING_NEW")),
            VenueRefusal::Gateway => Answer::new(502, "<html>502 Bad Gateway</html>"),
            VenueRefusal::PositionModeMismatch => Answer::new(400, gateway("INVALID_ARGUMENT", 3, "invalid request")),
        }
    }
}

#[tokio::test]
async fn the_trading_catalog_passes_on_spot() {
    let r = run_catalog(&Script).await;
    println!("coinbase ran {:?}\nelsewhere {:?}", r.ran, r.elsewhere);
    for id in ["T01", "T02", "T03+T24", "T04", "T05", "T09", "T10", "T11", "T12", "T15", "T19", "T20", "T21", "T22", "T26", "T30", "T31+T34", "T32", "E2D session check"] {
        assert!(r.ran.contains(&id), "{id} did not run: {:?}", r.ran);
    }
}

// ── Public conformance (no key): `OTW_NET_TEST=1` ──

fn net() -> bool {
    std::env::var("OTW_NET_TEST").as_deref() == Ok("1")
}

/// The live spot product list decodes to sound rules for every product, and an order call
/// signed with a key Coinbase never issued, or not signed at all, is answered with the error the
/// table expects (and nothing is placed: no account holds that key). Run with:
/// `OTW_NET_TEST=1 cargo test -p otw-connector-providers coinbase::trade::tests::public -- --nocapture`.
#[tokio::test]
async fn public_conformance_against_the_live_venue() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_sdk::{Driver, DriverConfig};
    if !net() {
        return;
    }
    let mm = m();
    let d = Driver::new(mm.clone(), DriverConfig::new("coinbase"), reqwest::Client::new());
    let junk = settings();
    let signer = signer();
    let a = Account { settings: &junk, signer: &signer, scope: "net" };
    let trader: Trader<Coinbase> = Trader::new();
    let rules = trader.all_rules(&d, &a).await.unwrap_or_else(|e| panic!("{e}"));
    assert!(rules.len() > 100, "{} products", rules.len());
    for r in &rules {
        r.sound().unwrap_or_else(|e| panic!("{e}"));
        assert!(r.tick.is_some() && r.lot_step.is_some(), "{} states no tick or step", r.instrument.native);
    }
    let btc = rules.iter().find(|r| r.instrument.native == "BTC-USD").unwrap();
    let closed = rules.iter().filter(|r| !matches!(r.status, TradingStatus::Trading)).count();
    println!("coinbase: {} products ({closed} not plainly trading), BTC-USD {btc:?}", rules.len());
    // An order no account could fill: the venue must stop at the key.
    let o = order("otw-net-probe", Side::Buy, OrderType::Limit, "0.0001", Some("1000"));
    let req = mm.place(&junk, &o, &placing("otw-net-probe")).unwrap();
    // Seen 2026-11-12: both answered HTTP 401 with the plain text body `Unauthorized`.
    for (what, signed) in [("junk key", true), ("unsigned", false)] {
        let mut r = req.clone();
        r.signed = signed;
        let frame = d.send_as(r, Some(&signer)).await.unwrap_or_else(|e| panic!("{what}: {e}"));
        let e = mm.decode_error(&frame).unwrap_or_else(|| panic!("{what}: Coinbase accepted an order with no valid key"));
        println!("coinbase {what}: {:?} {:?} mapped={} {}", e.canon, e.code, e.mapped, e.message);
        assert_eq!((e.canon, e.mapped), (Canon::Auth, true), "{what}: {e:?}");
    }
}

/// `sequence_num` is read from every frame of the socket, the heartbeats' as the user channel's,
/// so a jump across either one is seen.
#[test]
fn every_socket_frame_states_its_sequence_number() {
    let mm = m();
    assert_eq!(mm.private_sequence(WS_HEARTBEAT.as_bytes()), Some(1));
    assert_eq!(mm.private_sequence(WS_USER.as_bytes()), Some(7));
    assert_eq!(mm.private_sequence(br#"{"type":"error","message":"x"}"#), None);
    assert_eq!(mm.private_sequence(b"not json"), None);
}

/// A fill read on the order path names its fee in the product's quote currency, as the account
/// read does: the row names none, the trader takes it from the product list (read once when
/// the list was not held). `synthetic` product list (the documented `/products` shape).
#[tokio::test]
async fn order_path_fills_name_the_fee_in_the_quote_currency() {
    use otw_connector_engine::trading::{Account, Trader};
    use otw_connector_testkit::wiremock::matchers::{method, path};
    use otw_connector_testkit::wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/v3/brokerage/orders/historical/fills")).respond_with(ResponseTemplate::new(200).set_body_string(FILLS)).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/products"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"[{"id":"BTC-USD","base_currency":"BTC","quote_currency":"USD","display_name":"BTC-USD"}]"#))
        .mount(&server)
        .await;
    let d = otw_connector_testkit::driver_against(m(), &server, otw_connector_testkit::ManualClock::at(otw_connector_testkit::ts(1_719_600_000)));
    let s = settings();
    let sg = signer();
    let a = Account { settings: &s, signer: &sg, scope: "cb-fee" };
    let t: Trader<Coinbase> = Trader::new();
    let fills = t.fills_since(&d, &a, "BTC-USD", 1_718_000_000_000).await.unwrap();
    assert!(!fills.is_empty() && fills.iter().all(|f| f.fee_currency == "USD"), "{fills:?}");
    // The list is read once, then held.
    t.fills_since(&d, &a, "BTC-USD", 1_718_000_000_000).await.unwrap();
    let lists = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/products").count();
    assert_eq!(lists, 1);
}
