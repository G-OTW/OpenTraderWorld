//! The shared trading layer's hooks, each proven through the trader against the fake venue
//! ([`crate::fake`]): every answer here is `synthetic`, in the fake's own dialect.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_engine::trading::{Account, TicketOptions, TradeError, Trader};
use otw_connector_model::{Decimal, ErrorKind, InstrumentId, OrderAmend, OrderOpts, OrderRequest, OrderType, Price, Qty, Side, TimeInForce};
use otw_connector_sdk::trade::{DecodeCtx, PrivateEvent, Settings, TradeMapper};
use otw_connector_sdk::Driver;
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fake::{FakeSigner, FakeVenue, Knobs};
use crate::{driver_against, ManualClock};

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn ok(v: Value) -> ResponseTemplate {
    let mut v = v;
    if v.get("code").is_none() {
        v["code"] = json!(0);
    }
    ResponseTemplate::new(200).set_body_json(v)
}

struct Env {
    server: MockServer,
    driver: Driver<FakeVenue>,
    trader: Trader<FakeVenue>,
    settings: Settings,
}

impl Env {
    async fn new(knobs: Knobs, rules: Value) -> Self {
        let server = MockServer::start().await;
        let driver = driver_against(FakeVenue::new(knobs), &server, ManualClock::at(crate::ts(1_767_225_600)));
        Mock::given(method("GET")).and(path("/rules")).respond_with(ok(json!({ "rows": [rules] }))).with_priority(10).mount(&server).await;
        // Flat, one way, unless a test says otherwise.
        Mock::given(method("GET")).and(path("/positions")).respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "net", "qty": "0" }] }))).with_priority(10).mount(&server).await;
        Env { server, driver, trader: Trader::new(), settings: Settings::new() }
    }

    fn account(&self) -> Account<'_> {
        Account { settings: &self.settings, signer: &FakeSigner, scope: "fake" }
    }

    async fn bodies(&self, p: &str) -> Vec<Value> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == p)
            .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
            .collect()
    }

    async fn count(&self, p: &str) -> usize {
        self.server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == p).count()
    }

    async fn place(&self, o: OrderRequest) -> Result<Vec<otw_connector_engine::trading::Placed>, TradeError> {
        self.trader.place(&self.driver, &self.account(), &[o], &TicketOptions::default()).await
    }
}

fn rules() -> Value {
    json!({ "symbol": "BTC", "tick": "0.5", "step": "0.0001" })
}

fn order(key: &str, side: Side, t: OrderType, qty: &str) -> OrderRequest {
    let (l, s) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("FAKE", "BTC"),
        side,
        order_type: t,
        qty: Qty::new(d(qty)),
        limit: l.then(|| Price::new(d("30000"))),
        stop: s.then(|| Price::new(d("30000"))),
        tif: TimeInForce::Gtc,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts::default(),
    }
}

fn row(id: &str, cid: &str, status: &str, qty: &str) -> Value {
    json!({ "id": id, "cid": cid, "symbol": "BTC", "side": "buy", "status": status, "qty": qty, "filled": "0" })
}

/// Margin per instrument and the account-wide position mode, read from the venue when flat,
/// once, handed to the order; each read again after a refusal that may mean it changed.
#[tokio::test]
async fn margin_and_account_position_mode_are_read_once_and_go_with_the_order() {
    let e = Env::new(Knobs { symbol_margin: true, account_mode: true, ..Knobs::default() }, rules()).await;
    Mock::given(method("GET")).and(path("/mode")).respond_with(ok(json!({ "mode": "hedge" }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/margin")).respond_with(ok(json!({ "margin": "isolated", "coin": "USDT" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).and(body_string_contains("\"m4\"")).respond_with(ok(json!({ "code": 4, "msg": "mode" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).and(body_string_contains("\"m5\"")).respond_with(ok(json!({ "code": 5, "msg": "margin" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1", "cid": "x" }))).mount(&e.server).await;
    e.place(order("m1", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    e.place(order("m2", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    let sent = e.bodies("/order").await;
    assert_eq!((sent[0]["pos"].as_str(), sent[0]["margin"].as_str()), (Some("short"), Some("isolated")), "{sent:?}");
    assert_eq!((e.count("/mode").await, e.count("/margin").await, e.count("/positions").await), (1, 1, 0), "read once, at account level, flat");
    assert!(e.place(order("m4", Side::Sell, OrderType::Limit, "0.01")).await.is_err());
    e.place(order("m6", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!((e.count("/mode").await, e.count("/margin").await), (2, 1), "the mode is read again after its refusal");
    assert!(e.place(order("m5", Side::Sell, OrderType::Limit, "0.01")).await.is_err());
    e.place(order("m7", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!((e.count("/mode").await, e.count("/margin").await), (2, 2), "the margin is read again after its refusal");
    // The leverage read answers the instrument's own margin.
    let lev = e.trader.leverage(&e.driver, &e.account(), "BTC").await.unwrap();
    assert_eq!(lev.and_then(|l| l.1).as_deref(), Some("isolated"));
}

/// A margin a position row states is held as the instrument's (no margin read of its own).
#[tokio::test]
async fn a_position_rows_margin_is_held() {
    let e = Env::new(Knobs::default(), rules()).await;
    Mock::given(method("GET")).and(path("/positions")).respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "net", "dir": "buy", "qty": "1", "margin": "cross" }] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    e.place(order("p1", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.bodies("/order").await[0]["margin"].as_str(), Some("cross"));
}

/// Conditional orders in their own id space: the id carries the kind, and cancel, lookup and
/// open orders go to its endpoints; open orders cover both spaces.
#[tokio::test]
async fn conditional_orders_route_by_the_kind_their_id_carries() {
    let e = Env::new(Knobs { conditional: true, ..Knobs::default() }, rules()).await;
    Mock::given(method("POST")).and(path("/cond/order")).respond_with(ok(json!({ "id": "77", "cid": "c1" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cond/cancel")).respond_with(ok(json!({}))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [row("a", "k1", "new", "1")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/cond/open")).respond_with(ok(json!({ "rows": [row("77", "c1", "new", "1")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/cond/order")).and(query_param("id", "77")).respond_with(ok(json!({ "rows": [row("77", "c1", "new", "1")] }))).mount(&e.server).await;
    let placed = e.place(order("c1", Side::Buy, OrderType::Stop, "0.01")).await.unwrap();
    assert_eq!(placed[0].venue_id, "cond:77");
    assert_eq!(e.count("/order").await, 0, "a stop goes to the conditional endpoint only");
    let open = e.trader.open_orders(&e.driver, &e.account(), None).await.unwrap();
    let ids: Vec<&str> = open.iter().map(|o| o.venue_id.as_str()).collect();
    assert_eq!(ids, vec!["a", "cond:77"]);
    let found = e.trader.by_venue_id(&e.driver, &e.account(), "BTC", "cond:77").await.unwrap().unwrap();
    assert_eq!(found.client_id.as_deref(), Some("c1"));
    e.trader.cancel(&e.driver, &e.account(), "BTC", "cond:77").await.unwrap();
    assert_eq!(e.bodies("/cond/cancel").await[0]["id"].as_str(), Some("77"));
    assert_eq!(e.count("/cancel").await, 0);
}

/// A book that counts in contracts: the trader reads the contract sizes; the order goes out in
/// contracts; orders, fills and positions are read back in the base coin.
#[tokio::test]
async fn contract_sizes_reach_the_mapper_both_ways() {
    let e = Env::new(Knobs { contracts: true, ..Knobs::default() }, json!({ "symbol": "BTC", "tick": "0.5", "step": "0.01", "contract": "0.01" })).await;
    Mock::given(method("GET")).and(path("/positions")).respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "net", "dir": "buy", "qty": "7" }] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).respond_with(ok(json!({ "rows": [row("v1", "k1", "partial", "5")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/fills")).respond_with(ok(json!({ "rows": [{ "exec": "x1", "id": "v1", "cid": "k1", "symbol": "BTC", "qty": "3", "price": "30000", "fee": "0.1", "fee_coin": "USDT" }] }))).mount(&e.server).await;
    e.place(order("k1", Side::Buy, OrderType::Limit, "0.05")).await.unwrap();
    assert_eq!(e.bodies("/order").await[0]["contracts"].as_str(), Some("5"));
    let o = e.trader.lookup(&e.driver, &e.account(), "BTC", "k1").await.unwrap().unwrap();
    assert_eq!(o.qty, d("0.05"));
    let fills = e.trader.fills_since(&e.driver, &e.account(), "BTC", 0).await.unwrap();
    assert_eq!(fills[0].qty, d("0.03"));
    let pos = e.trader.positions(&e.driver, &e.account(), "BTC").await.unwrap();
    assert_eq!(pos[0].qty, d("0.07"));
    // Off the contract grid: refused before I/O by the rules (step = one contract).
    assert!(e.place(order("k2", Side::Buy, OrderType::Limit, "0.055")).await.is_err());
    assert_eq!(e.count("/order").await, 2, "one place, one lookup: nothing more sent");
    let read_rules = e.count("/rules").await;
    e.place(order("k3", Side::Buy, OrderType::Limit, "0.02")).await.unwrap();
    assert_eq!(e.count("/rules").await, read_rules, "the sizes are held, not read before each order");
}

/// Cancel all as batches by ids, at most `max` per call; a row the answer refuses is
/// cancelled on its own after.
#[tokio::test]
async fn cancel_all_goes_in_batches_by_id_and_walks_a_refused_row() {
    let e = Env::new(Knobs { batch: Some(2), ..Knobs::default() }, rules()).await;
    let three = json!({ "rows": [row("a", "k1", "new", "1"), row("b", "k2", "new", "1"), row("c", "", "new", "1")] });
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(three)).up_to_n_times(1).with_priority(1).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [row("b", "k2", "new", "1")] }))).up_to_n_times(1).with_priority(2).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [] }))).with_priority(3).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cancel-batch")).respond_with(ok(json!({ "rows": [{ "id": "a", "code": "0" }, { "id": "b", "code": "1" }] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cancel")).respond_with(ok(json!({}))).mount(&e.server).await;
    let n = e.trader.cancel_all(&e.driver, &e.account()).await.unwrap();
    assert_eq!(n, 3);
    let batches = e.bodies("/cancel-batch").await;
    assert_eq!(batches.iter().map(|b| b["ids"].as_array().unwrap().len()).collect::<Vec<_>>(), vec![2, 1]);
    assert_eq!(e.bodies("/cancel").await.iter().map(|b| b["id"].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["b".to_string()]);
}

/// Cancel all over the whole book: one call naming no symbol.
#[tokio::test]
async fn cancel_all_over_the_book_names_no_symbol() {
    let e = Env::new(Knobs { book_cancel: true, ..Knobs::default() }, rules()).await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [row("a", "k1", "new", "1")] }))).up_to_n_times(1).with_priority(1).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [] }))).with_priority(2).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cancel-all")).respond_with(ok(json!({}))).mount(&e.server).await;
    assert_eq!(e.trader.cancel_all(&e.driver, &e.account()).await.unwrap(), 1);
    assert_eq!(e.bodies("/cancel-all").await, vec![json!({})]);
}

/// An ack that names no order is resolved by its client id, never resent; not found yet, it is
/// unknown under its key.
#[tokio::test]
async fn an_ack_without_an_id_is_found_by_its_client_id_never_resent() {
    let e = Env::new(Knobs::default(), rules()).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "cid": "x" }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("cid", "n1")).respond_with(ok(json!({ "rows": [row("v9", "n1", "filled", "1")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("cid", "n2")).respond_with(ok(json!({ "rows": [] }))).mount(&e.server).await;
    let placed = e.place(order("n1", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(placed[0].venue_id, "v9");
    let out = e.place(order("n2", Side::Buy, OrderType::Limit, "0.01")).await;
    assert!(matches!(&out, Err(TradeError::Unknown { keys, alert: false, .. }) if keys == &vec!["n2".to_string()]), "{out:?}");
    assert_eq!(e.count("/order").await, 4, "two places, two lookups: nothing resent");
    assert_eq!(e.bodies("/order").await.iter().filter(|b| b["cid"] == "n2").count(), 1);
}

/// A change made as a new order: a new client id minted before I/O, the new venue id
/// answered; a lost answer is looked up by the new id; an order with no client id is refused
/// before I/O; a venue that changes in place answers no replacement.
#[tokio::test]
async fn a_change_as_cancel_and_replace_answers_the_new_order() {
    let e = Env::new(Knobs { replaces: true, ..Knobs::default() }, rules()).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("id", "v1")).respond_with(ok(json!({ "rows": [row("v1", "k1", "new", "0.01")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("id", "v3")).respond_with(ok(json!({ "rows": [row("v3", "k3", "new", "0.01")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("id", "ext")).respond_with(ok(json!({ "rows": [row("ext", "", "new", "0.01")] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/replace")).and(body_string_contains("\"v1\"")).respond_with(ok(json!({ "id": "v2", "cid": "k1-r1" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/replace")).and(body_string_contains("\"v3\"")).respond_with(ResponseTemplate::new(502)).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("cid", "k3-r1")).respond_with(ok(json!({ "rows": [row("v4", "k3-r1", "new", "0.01")] }))).mount(&e.server).await;
    let change = OrderAmend { limit: Some(Price::new(d("30500"))), ..Default::default() };
    let moved = e.trader.modify(&e.driver, &e.account(), "BTC", "v1", &change).await.unwrap().unwrap();
    assert_eq!((moved.key.as_str(), moved.venue_id.as_str()), ("k1-r1", "v2"));
    assert_eq!(e.bodies("/replace").await[0]["new_cid"].as_str(), Some("k1-r1"));
    let lost = e.trader.modify(&e.driver, &e.account(), "BTC", "v3", &change).await.unwrap().unwrap();
    assert_eq!((lost.key.as_str(), lost.venue_id.as_str()), ("k3-r1", "v4"));
    let ext = e.trader.modify(&e.driver, &e.account(), "BTC", "ext", &change).await;
    assert!(matches!(&ext, Err(TradeError::Refused(x)) if x.message.contains("cancel it and place a new one")), "{ext:?}");
    assert_eq!(e.count("/replace").await, 2, "never resent, nothing sent for the external order");
    // In place: no replacement answered.
    let e = Env::new(Knobs::default(), rules()).await;
    Mock::given(method("GET")).and(path("/order")).respond_with(ok(json!({ "rows": [row("v1", "k1", "new", "0.01")] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/amend")).respond_with(ok(json!({}))).mount(&e.server).await;
    assert!(e.trader.modify(&e.driver, &e.account(), "BTC", "v1", &change).await.unwrap().is_none());
}

/// An error code inside an HTTP 200 answer is the venue's refusal, mapped.
#[tokio::test]
async fn an_error_code_inside_a_200_answer_is_a_refusal() {
    let e = Env::new(Knobs::default(), rules()).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "code": 1, "msg": "insufficient" }))).mount(&e.server).await;
    let out = e.place(order("f1", Side::Buy, OrderType::Limit, "0.01")).await;
    assert!(matches!(&out, Err(TradeError::Refused(x)) if x.provider_code.as_deref() == Some("1")), "{out:?}");
}

/// Several fee lines on one fill; a stream fill without a fee completed from the venue's fills.
#[tokio::test]
async fn fee_lines_and_a_fill_without_a_fee_completed_from_the_fills() {
    let e = Env::new(Knobs::default(), rules()).await;
    let rest = json!({ "rows": [{ "exec": "x1", "id": "v1", "cid": "k1", "symbol": "BTC", "qty": "1", "price": "30000", "fee": "0.3", "fee_coin": "USDT",
        "extra": [{ "fee": "0.05", "coin": "EUR", "kind": "WHT" }], "at": 5 }] });
    Mock::given(method("GET")).and(path("/fills")).respond_with(ok(rest)).mount(&e.server).await;
    let m = e.driver.mapper();
    let sizes = Default::default();
    let frame = json!({ "fill": { "exec": "x1", "id": "v1", "cid": "k1", "symbol": "BTC", "qty": "1", "price": "30000", "at": 5 } });
    let ev = m.decode_private_in(&DecodeCtx::new("/ws", &sizes), frame.to_string().as_bytes()).unwrap();
    let PrivateEvent::Fill(f) = ev[0].clone() else { panic!("{ev:?}") };
    assert!(f.fee_pending && f.fee.is_zero());
    let mut fills = vec![f];
    let n = e.trader.complete_fees(&e.driver, &e.account(), "BTC", &mut fills).await.unwrap();
    assert_eq!(n, 1);
    assert!(!fills[0].fee_pending);
    assert_eq!(fills[0].fees(), vec![(d("0.3"), "USDT"), (d("0.05"), "EUR")]);
}

/// A market buy the venue sizes in the quote coin: valued at the venue's reference price read
/// just before sending, rounded down to the quote step, and the answer says the size filled
/// may differ. A sell, and a limit buy, go out sized as asked. No price, nothing sent.
#[tokio::test]
async fn a_quote_sized_market_buy_is_valued_just_before_it_leaves() {
    let e = Env::new(Knobs { quote_buy: true, ..Knobs::default() }, json!({ "symbol": "BTC", "tick": "0.5", "step": "0.0001", "quote_step": "0.01" })).await;
    Mock::given(method("GET")).and(path("/ticker")).respond_with(ok(json!({ "ask": "30000.5" }))).up_to_n_times(1).with_priority(1).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/ticker")).respond_with(ResponseTemplate::new(502)).with_priority(2).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    let placed = e.place(order("q1", Side::Buy, OrderType::Market, "0.0013")).await.unwrap();
    let sent = e.bodies("/order").await;
    // 0.0013 x 30000.5 = 39.00065, rounded down to 0.01.
    assert_eq!((sent[0]["quote"].as_str(), sent[0].get("qty")), (Some("39"), None));
    assert!(placed[0].note.as_deref().is_some_and(|n| n.contains("may differ") && n.contains("30000.5")), "{:?}", placed[0].note);
    e.place(order("q2", Side::Sell, OrderType::Market, "0.0013")).await.unwrap();
    e.place(order("q3", Side::Buy, OrderType::Limit, "0.0013")).await.unwrap();
    let sent = e.bodies("/order").await;
    assert_eq!((sent[1]["qty"].as_str(), sent[2]["qty"].as_str()), (Some("0.0013"), Some("0.0013")));
    assert_eq!(e.count("/ticker").await, 1, "read for the market buy only");
    let out = e.place(order("q4", Side::Buy, OrderType::Market, "0.0013")).await;
    assert!(matches!(&out, Err(TradeError::Refused(x)) if x.kind == ErrorKind::Transient), "{out:?}");
    assert_eq!(e.count("/order").await, 3, "no price, nothing sent");
}

/// The ticket's own reference stands in when the venue offers no price read; with none at
/// hand, or a price of zero, the buy is refused and nothing is sent.
#[tokio::test]
async fn a_quote_sized_buy_falls_back_to_the_tickets_reference() {
    let r = json!({ "symbol": "BTC", "tick": "0.5", "step": "0.0001", "quote_step": "0.01" });
    let e = Env::new(Knobs { quote_buy: true, no_ticker: true, ..Knobs::default() }, r.clone()).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    let opt = TicketOptions { reference: Some(d("20000")), dry_run_first: false };
    e.trader.place(&e.driver, &e.account(), &[order("r1", Side::Buy, OrderType::Market, "0.002")], &opt).await.unwrap();
    assert_eq!(e.bodies("/order").await[0]["quote"].as_str(), Some("40"));
    let out = e.place(order("r2", Side::Buy, OrderType::Market, "0.002")).await;
    assert!(matches!(&out, Err(TradeError::Refused(x)) if x.message.contains("no price was at hand")), "{out:?}");
    let e = Env::new(Knobs { quote_buy: true, ..Knobs::default() }, r).await;
    Mock::given(method("GET")).and(path("/ticker")).respond_with(ok(json!({ "ask": "0" }))).mount(&e.server).await;
    assert!(matches!(e.place(order("z1", Side::Buy, OrderType::Market, "0.0013")).await, Err(TradeError::Refused(_))));
    assert_eq!(e.count("/order").await, 0);
}
