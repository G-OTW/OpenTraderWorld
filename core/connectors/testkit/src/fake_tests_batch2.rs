//! The batch 2 hooks of the shared trading layer, each proven through the trader against the
//! fake venue ([`crate::fake`]): every answer here is `synthetic`, in the fake's own dialect.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, TradeError, Trader};
use otw_connector_model::{Decimal, InstrumentId, Mapped, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, SizeUnit, TimeInForce};
use otw_connector_sdk::trade::{ClientRef, PositionMode, Settings};
use otw_connector_sdk::{Driver, HttpRequest, Signer};
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::fake::{FakeSigner, FakeVenue, Knobs, SerialSigner};
use crate::{driver_against, ManualClock};

const NOW_MS: i64 = 1_767_225_600_000;

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
    scope: &'static str,
}

impl Env {
    async fn new(knobs: Knobs, rules: Value, scope: &'static str) -> Self {
        let server = MockServer::start().await;
        let driver = driver_against(FakeVenue::new(knobs), &server, ManualClock::at(crate::ts(NOW_MS / 1000)));
        Mock::given(method("GET")).and(path("/rules")).respond_with(ok(json!({ "rows": [rules] }))).with_priority(10).mount(&server).await;
        Mock::given(method("GET")).and(path("/positions")).respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "net", "qty": "0" }] }))).with_priority(10).mount(&server).await;
        Env { server, driver, trader: Trader::new(), settings: Settings::new(), scope }
    }

    fn account(&self) -> Account<'_> {
        Account { settings: &self.settings, signer: &FakeSigner, scope: self.scope }
    }

    async fn bodies(&self, p: &str) -> Vec<Value> {
        self.server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == p).map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null)).collect()
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

fn in_quote(mut o: OrderRequest) -> OrderRequest {
    o.opts.size_unit = SizeUnit::Quote;
    o
}

fn row(id: &str, cid: &str, status: &str, qty: &str) -> Value {
    json!({ "id": id, "cid": cid, "symbol": "BTC", "side": "buy", "status": status, "qty": qty, "filled": "0" })
}

/// The leverage the venue holds goes with the order: read once, then held (no read in front
/// of each order); none when the venue holds none (the order goes without one), and the chart
/// is told so; without a read of the venue's own, a position row's leverage goes.
#[tokio::test]
async fn the_leverage_the_venue_holds_goes_with_the_order_and_none_is_never_made_up() {
    let e = Env::new(Knobs { leverage: true, ..Knobs::default() }, rules(), "lev").await;
    Mock::given(method("GET")).and(path("/leverage")).respond_with(ok(json!({ "leverage": "5" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    e.place(order("l1", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    e.place(order("l2", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    let sent = e.bodies("/order").await;
    assert_eq!((sent[0]["lev"].as_str(), sent[1]["lev"].as_str()), (Some("5"), Some("5")), "{sent:?}");
    assert_eq!(e.count("/leverage").await, 1, "read once, then held");
    // The venue holds none: the order goes without one, and none is shown.
    let e = Env::new(Knobs { leverage: true, ..Knobs::default() }, rules(), "lev-none").await;
    Mock::given(method("GET")).and(path("/leverage")).respond_with(ok(json!({ "leverage": null }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    e.place(order("l2", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.bodies("/order").await[0].get("lev"), None);
    let shown = e.trader.leverage(&e.driver, &e.account(), "BTC").await.unwrap();
    assert_eq!(shown.map(|l| l.0), Some(None), "no leverage held: none shown, never a number made up");
    // No read of the venue's own: the position row's leverage.
    let e = Env::new(Knobs::default(), rules(), "lev-row").await;
    Mock::given(method("GET")).and(path("/positions")).respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "net", "dir": "buy", "qty": "1", "lev": "3" }] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    e.place(order("l3", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.bodies("/order").await[0]["lev"].as_str(), Some("3"));
}

/// A book sized in the quote currency: a size in it goes out rounded down to the step; a size
/// in the coin is converted at the price read just before, then rounded down; never up. A size
/// that rounds under the smallest is refused naming it, nothing sent. A book sized in the coin
/// refuses a size in the quote currency. A close goes out as the venue holds it.
#[tokio::test]
async fn a_book_sized_in_the_quote_currency_rounds_down_never_up() {
    let r = json!({ "symbol": "BTC", "tick": "0.5", "step": "10", "unit": "quote" });
    let e = Env::new(Knobs::default(), r, "quote").await;
    Mock::given(method("GET")).and(path("/ticker")).respond_with(ok(json!({ "ask": "50000" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    let usd = e.place(in_quote(order("u1", Side::Buy, OrderType::Limit, "25"))).await.unwrap();
    assert!(usd[0].note.as_deref().is_some_and(|n| n.contains("rounded down")), "{:?}", usd[0].note);
    let coin = e.place(order("u2", Side::Buy, OrderType::Limit, "0.0013")).await.unwrap();
    assert!(coin[0].note.as_deref().is_some_and(|n| n.contains("50000")), "{:?}", coin[0].note);
    let exact = e.place(in_quote(order("u3", Side::Sell, OrderType::Market, "30"))).await.unwrap();
    assert!(exact[0].note.is_none(), "a size on the step goes as typed");
    let sent = e.bodies("/order").await;
    // 25 USD -> 20; 0.0013 BTC x 50000 = 65 USD -> 60; 30 -> 30.
    assert_eq!(sent.iter().map(|b| b["quote"].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["20", "60", "30"]);
    let small = e.place(in_quote(order("u4", Side::Buy, OrderType::Limit, "5"))).await;
    assert!(matches!(&small, Err(TradeError::Refused(x)) if x.message.contains("smallest size it takes, 10")), "{small:?}");
    let tiny = e.place(order("u5", Side::Buy, OrderType::Limit, "0.0001")).await;
    assert!(matches!(&tiny, Err(TradeError::Refused(x)) if x.message.contains("0.0002 in the coin")), "{tiny:?}");
    assert_eq!(e.count("/order").await, 3, "nothing sent for a size that rounds under the smallest");
    // The close: the size as held, in the venue's unit.
    Mock::given(method("GET")).and(path("/positions")).respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "net", "dir": "buy", "qty": "100" }] }))).mount(&e.server).await;
    e.trader.close_position(&e.driver, &e.account(), "BTC", None, None, "u6").await.unwrap();
    assert_eq!(e.bodies("/order").await[3]["quote"].as_str(), Some("100"));
    // A book sized in the coin takes no size in the quote currency.
    let e = Env::new(Knobs::default(), rules(), "quote-base").await;
    let out = e.place(in_quote(order("b1", Side::Buy, OrderType::Limit, "100"))).await;
    assert!(matches!(&out, Err(TradeError::Refused(x)) if x.message.contains("in the base coin")), "{out:?}");
    assert_eq!(e.count("/order").await, 0);
}

/// Where the venue states the mode account wide, the position rows (sided in either mode)
/// never set it; a pushed mode is held, and a push that says it changed reads it again.
#[tokio::test]
async fn an_account_wide_mode_is_never_read_from_sided_rows() {
    let e = Env::new(Knobs { account_mode: true, ..Knobs::default() }, rules(), "acct").await;
    Mock::given(method("GET")).and(path("/mode")).respond_with(ok(json!({ "mode": "one_way" }))).mount(&e.server).await;
    Mock::given(method("GET"))
        .and(path("/positions"))
        .respond_with(ok(json!({ "rows": [{ "symbol": "BTC", "side": "long", "qty": "1" }, { "symbol": "BTC", "side": "short", "qty": "0" }] })))
        .mount(&e.server)
        .await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    assert_eq!(e.trader.prefill_modes(&e.driver, &e.account()).await.unwrap(), 0, "no mode taken from the rows");
    let sided = otw_connector_sdk::trade::VenuePosition { symbol: "BTC".into(), side: otw_connector_sdk::trade::PositionSide::Long, direction: None, qty: d("1"), leverage: None, margin: None };
    e.trader.set_position(&e.driver, &e.account(), &sided);
    e.place(order("a1", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.bodies("/order").await[0]["pos"].as_str(), Some("net"), "one-way as the account says");
    e.trader.mode_pushed(&e.account(), None, Some(PositionMode::Hedge));
    e.place(order("a2", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.bodies("/order").await[1]["pos"].as_str(), Some("short"), "the pushed mode is held");
    let reads = e.count("/mode").await;
    e.trader.mode_pushed(&e.account(), None, None);
    e.place(order("a3", Side::Sell, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.count("/mode").await, reads + 1, "a push that says it changed reads it again");
}

/// A minted client id: minted before I/O and strictly after the one before, the order goes
/// under it, a lost answer is looked up with its time, the stream's echo maps back to OTW's
/// key, a cancel names it; after a restart it is known again from what was stored, and a
/// minted id not held is never guessed.
#[tokio::test]
async fn a_minted_client_id_goes_out_is_looked_up_with_its_time_and_maps_back() {
    let e = Env::new(Knobs { minted: true, ..Knobs::default() }, rules(), "mint").await;
    let first = e.trader.mint(&e.driver, &e.account(), "m1").unwrap().unwrap();
    let again = e.trader.mint(&e.driver, &e.account(), "m1").unwrap().unwrap();
    assert_eq!(first, again, "minted once per key");
    let second = e.trader.mint(&e.driver, &e.account(), "m2").unwrap().unwrap();
    assert!(second.at_ms > first.at_ms && second.id.parse::<i64>().unwrap() > first.id.parse::<i64>().unwrap());
    Mock::given(method("POST")).and(path("/order")).and(body_string_contains(first.id.as_str())).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/order")).and(body_string_contains(second.id.as_str())).respond_with(ResponseTemplate::new(502)).mount(&e.server).await;
    let since = second.at_ms.unwrap().to_string();
    Mock::given(method("GET"))
        .and(path("/order"))
        .and(query_param("cid", second.id.as_str()))
        .and(query_param("since", since.as_str()))
        .respond_with(ok(json!({ "rows": [row("v2", &second.id, "new", "0.01")] })))
        .mount(&e.server)
        .await;
    Mock::given(method("POST")).and(path("/cancel")).respond_with(ok(json!({}))).mount(&e.server).await;
    let placed = e.place(order("m1", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(placed[0].venue_id, "v1");
    assert_eq!(e.bodies("/order").await[0]["cid"].as_str(), Some(first.id.as_str()));
    let lost = e.place(order("m2", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(lost[0].venue_id, "v2", "found by its minted id and time");
    assert_eq!(e.trader.otw_key(&e.driver, &e.account(), &second.id).as_deref(), Some("m2"));
    assert_eq!(e.trader.otw_key(&e.driver, &e.account(), "999"), None, "a minted id OTW does not hold is no key");
    e.trader.cancel(&e.driver, &e.account(), "BTC", "v1").await.unwrap();
    let c = e.bodies("/cancel").await;
    assert_eq!((c[0]["cid"].as_str(), c[0]["cid_at"].as_i64()), (Some(first.id.as_str()), first.at_ms));
    // After a restart: unknown until held again from the store.
    let fresh: Trader<FakeVenue> = Trader::new();
    assert!(matches!(fresh.lookup(&e.driver, &e.account(), "BTC", "m2").await, Err(TradeError::Refused(_))), "never guessed");
    fresh.hold_client(&e.account(), "m2", ClientRef { id: second.id.clone(), at_ms: second.at_ms }, Some("v2"));
    assert_eq!(fresh.lookup(&e.driver, &e.account(), "BTC", "m2").await.unwrap().unwrap().venue_id, "v2");
}

/// A trigger that fired as a new order is followed to it: the lookup answers the new order,
/// and a cancel that finds the trigger ended cancels the order it became.
#[tokio::test]
async fn a_trigger_that_fired_is_followed_to_its_new_order() {
    let e = Env::new(Knobs::default(), rules(), "succ").await;
    let mut fired = row("T1", "t1", "cancelled", "0.01");
    fired["next"] = json!("N1");
    Mock::given(method("GET")).and(path("/order")).and(query_param("cid", "t1")).respond_with(ok(json!({ "rows": [fired.clone()] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("id", "T1")).respond_with(ok(json!({ "rows": [fired] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("id", "N1")).respond_with(ok(json!({ "rows": [row("N1", "t1", "partial", "0.01")] }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cancel")).and(body_string_contains("\"T1\"")).respond_with(ok(json!({ "code": 3, "msg": "triggered" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cancel")).and(body_string_contains("\"N1\"")).respond_with(ok(json!({}))).mount(&e.server).await;
    let o = e.trader.lookup(&e.driver, &e.account(), "BTC", "t1").await.unwrap().unwrap();
    assert_eq!((o.venue_id.as_str(), o.status.clone()), ("N1", Mapped::Known(OrderStatus::PartiallyFilled)));
    let c = e.trader.cancel(&e.driver, &e.account(), "BTC", "T1").await.unwrap();
    assert_eq!(c, Cancelled::Sent);
    let ids: Vec<String> = e.bodies("/cancel").await.iter().map(|b| b["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, vec!["T1", "N1"]);
}

/// A lookup walks the pages the venue offers, following the cursor up to the mapper's cap.
#[tokio::test]
async fn a_lookup_walks_pages_up_to_the_venues_cap() {
    for (pages, found) in [(3, true), (2, false)] {
        let e = Env::new(Knobs { lookup_pages: pages, ..Knobs::default() }, rules(), "pages").await;
        Mock::given(method("GET")).and(path("/order")).and(query_param("cursor", "c3")).respond_with(ok(json!({ "rows": [row("v3", "p1", "new", "0.01")] }))).with_priority(1).mount(&e.server).await;
        Mock::given(method("GET")).and(path("/order")).and(query_param("cursor", "c2")).respond_with(ok(json!({ "rows": [row("x", "other", "new", "1")], "next": "c3" }))).with_priority(1).mount(&e.server).await;
        Mock::given(method("GET")).and(path("/order")).respond_with(ok(json!({ "rows": [], "next": "c2" }))).with_priority(5).mount(&e.server).await;
        let got = e.trader.lookup(&e.driver, &e.account(), "BTC", "p1").await.unwrap();
        assert_eq!(got.is_some(), found, "{pages} pages");
        assert_eq!(e.count("/order").await, pages);
    }
}

/// Fills older than the venue's recent store come from both stores, merged, each once.
#[tokio::test]
async fn fills_come_from_both_stores_by_age() {
    let e = Env::new(Knobs { two_stores: true, ..Knobs::default() }, rules(), "fills").await;
    let f = |x: &str| json!({ "exec": x, "id": "v1", "cid": "k1", "symbol": "BTC", "qty": "1", "price": "30000", "fee": "0.1", "fee_coin": "USDT" });
    Mock::given(method("GET")).and(path("/fills")).respond_with(ok(json!({ "rows": [f("x1"), f("x2")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/fills-old")).respond_with(ok(json!({ "rows": [f("x0"), f("x1")] }))).mount(&e.server).await;
    let recent = e.trader.fills_since(&e.driver, &e.account(), "BTC", NOW_MS - 3_600_000).await.unwrap();
    assert_eq!((recent.len(), e.count("/fills-old").await), (2, 0), "a recent window reads the recent store only");
    let all = e.trader.fills_since(&e.driver, &e.account(), "BTC", NOW_MS - 2 * 86_400_000).await.unwrap();
    assert_eq!(all.iter().map(|f| f.exec_id.as_str()).collect::<Vec<_>>(), vec!["x1", "x2", "x0"]);
}

/// The venue's other names for an instrument reach the decoders: a row named by an alias is
/// read under the instrument's own symbol; a name the list does not give is never guessed.
#[tokio::test]
async fn the_name_table_reads_every_name_of_an_instrument() {
    let r = json!({ "symbol": "BTC", "tick": "0.5", "step": "0.0001", "aliases": ["XBTUSD", "XXBTZUSD"] });
    let e = Env::new(Knobs { names: true, ..Knobs::default() }, r, "names").await;
    let mut alias = row("v1", "n1", "new", "0.01");
    alias["symbol"] = json!("xxbtzusd");
    let mut stranger = row("v2", "n2", "new", "0.01");
    stranger["symbol"] = json!("DOGEUSD");
    Mock::given(method("GET")).and(path("/order")).and(query_param("cid", "n1")).respond_with(ok(json!({ "rows": [alias] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/order")).and(query_param("cid", "n2")).respond_with(ok(json!({ "rows": [stranger] }))).mount(&e.server).await;
    let o = e.trader.lookup(&e.driver, &e.account(), "BTC", "n1").await.unwrap().unwrap();
    assert_eq!(o.symbol, "BTC");
    let out = e.trader.lookup(&e.driver, &e.account(), "BTC", "n2").await;
    assert!(matches!(&out, Err(TradeError::Unknown { alert: true, .. })), "{out:?}");
    assert_eq!(e.trader.symbol_names(&e.driver, &e.account()).await.unwrap().native("XBTUSD"), Some("BTC"));
}

/// The signed calls of a key whose nonce must rise leave one at a time, each answered before
/// the next is signed; any other key's go out together.
#[tokio::test]
async fn signed_calls_of_a_nonce_key_leave_one_at_a_time() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/slow")).respond_with(ok(json!({})).set_delay(Duration::from_millis(300))).mount(&server).await;
    let driver = driver_against(FakeVenue::new(Knobs::default()), &server, ManualClock::at(crate::ts(NOW_MS / 1000)));
    let call = |s: &'static dyn Signer, i: u64| {
        let driver = &driver;
        async move { driver.send_as(HttpRequest::get("slow", "rest", "/slow", otw_connector_model::CorrId(i)).signed(), Some(s)).await.unwrap() }
    };
    static SERIAL: SerialSigner = SerialSigner("fake-nonce-key");
    static FREE: FakeSigner = FakeSigner;
    let t = Instant::now();
    tokio::join!(call(&SERIAL, 1), call(&SERIAL, 2), call(&SERIAL, 3));
    assert!(t.elapsed() >= Duration::from_millis(850), "one at a time: {:?}", t.elapsed());
    let t = Instant::now();
    tokio::join!(call(&FREE, 4), call(&FREE, 5), call(&FREE, 6));
    assert!(t.elapsed() < Duration::from_millis(850), "in parallel: {:?}", t.elapsed());
}

/// An order sent under another of the instrument's names finds its rules by that alias, and
/// the placed order carries the name the venue's answers use, so a read or a stream frame of
/// it is never a second order under another spelling.
#[tokio::test]
async fn an_order_under_an_alias_is_placed_under_the_name_the_answers_use() {
    let r = json!({ "symbol": "BTC", "tick": "0.5", "step": "0.0001", "aliases": ["XBTUSD"] });
    let e = Env::new(Knobs { names: true, ..Knobs::default() }, r, "alias-place").await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    let rules = e.trader.rules(&e.driver, &e.account(), "XBTUSD").await.unwrap();
    assert_eq!(rules.instrument.native, "BTC", "found by its alias");
    let mut o = order("a1", Side::Buy, OrderType::Limit, "0.01");
    o.instrument = InstrumentId::new("FAKE", "XBTUSD");
    let placed = e.place(o).await.unwrap();
    assert_eq!((placed[0].symbol.as_str(), placed[0].venue_id.as_str()), ("BTC", "v1"));
}

/// An order listed twice (two requests, or a page that repeats it) is one working order.
#[tokio::test]
async fn an_order_listed_twice_is_one_working_order() {
    let e = Env::new(Knobs::default(), rules(), "dedupe").await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [row("v1", "c1", "new", "0.01"), row("v1", "c1", "new", "0.01"), row("v2", "c2", "new", "0.01")] }))).mount(&e.server).await;
    let open = e.trader.open_orders(&e.driver, &e.account(), None).await.unwrap();
    assert_eq!(open.iter().map(|o| o.venue_id.as_str()).collect::<Vec<_>>(), vec!["v1", "v2"]);
}

/// The rate wait comes before the signature: a call that waited on the limiter is signed after
/// its wait, so its nonce never goes out older than one signed later.
#[tokio::test]
async fn a_signed_call_is_signed_after_its_rate_wait() {
    use std::sync::{Arc, Mutex};
    struct Log(Arc<Mutex<Vec<&'static str>>>);
    #[async_trait::async_trait]
    impl otw_connector_sdk::Limiter for Log {
        async fn acquire(&self, _endpoint_id: &str, _weight: u32) {
            self.0.lock().unwrap().push("wait");
        }
    }
    impl Signer for Log {
        fn sign(&self, _req: &mut otw_connector_sdk::SignableRequest, _now: otw_connector_model::Timestamp) -> Result<(), otw_connector_model::ConnectorError> {
            self.0.lock().unwrap().push("sign");
            Ok(())
        }
        fn serial_key(&self) -> Option<String> {
            Some("fake-order-key".into())
        }
    }
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/x")).respond_with(ok(json!({}))).mount(&server).await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let driver = driver_against(FakeVenue::new(Knobs::default()), &server, ManualClock::at(crate::ts(NOW_MS / 1000))).with_limiter(Arc::new(Log(log.clone())));
    let signer = Log(log.clone());
    driver.send_as(HttpRequest::get("x", "rest", "/x", otw_connector_model::CorrId(1)).signed(), Some(&signer)).await.unwrap();
    assert_eq!(*log.lock().unwrap(), vec!["wait", "sign"]);
}

/// A conditional list the venue refuses leaves the rest of the book read (and cancelled by the
/// kill switch); a refusal of the first list, or a failure that is not a refusal, still fails.
#[tokio::test]
async fn a_refused_conditional_list_leaves_the_rest_of_the_book_read() {
    let e = Env::new(Knobs { conditional: true, ..Knobs::default() }, rules(), "cond-refused").await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "rows": [row("v1", "c1", "new", "0.01")] }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/cond/open")).respond_with(ok(json!({ "code": 4, "msg": "window required" }))).mount(&e.server).await;
    let open = e.trader.open_orders(&e.driver, &e.account(), None).await.unwrap();
    assert_eq!(open.iter().map(|o| o.venue_id.as_str()).collect::<Vec<_>>(), vec!["v1"]);
    let e = Env::new(Knobs { conditional: true, ..Knobs::default() }, rules(), "first-refused").await;
    Mock::given(method("GET")).and(path("/open")).respond_with(ok(json!({ "code": 4, "msg": "no" }))).mount(&e.server).await;
    Mock::given(method("GET")).and(path("/cond/open")).respond_with(ok(json!({ "rows": [] }))).mount(&e.server).await;
    assert!(e.trader.open_orders(&e.driver, &e.account(), None).await.is_err());
}

/// While the venue has maintenance announced (pushed by its private stream), a new order and a
/// close are refused at once with the reason, nothing sent; a cancel still goes out. Its end
/// lets orders go again. Nothing waits or retries.
#[tokio::test]
async fn announced_maintenance_refuses_new_orders_and_lets_cancels_go() {
    let e = Env::new(Knobs::default(), rules(), "maint").await;
    Mock::given(method("POST")).and(path("/order")).respond_with(ok(json!({ "id": "v1" }))).mount(&e.server).await;
    Mock::given(method("POST")).and(path("/cancel")).respond_with(ok(json!({}))).mount(&e.server).await;
    let a = e.account();
    e.trader.set_maintenance(&a, Some("code 20060 Maintenance".into()));
    let started = Instant::now();
    match e.place(order("m1", Side::Buy, OrderType::Limit, "0.01")).await {
        Err(TradeError::Refused(c)) => {
            assert_eq!(c.kind, otw_connector_model::ErrorKind::ProviderDown);
            assert!(c.message.contains("announced maintenance (code 20060 Maintenance)") && c.message.contains("nothing was sent"), "{}", c.message);
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(1), "refused at once, never held");
    let close = e.trader.close_position(&e.driver, &a, "BTC", None, None, "m-close").await;
    assert!(matches!(&close, Err(TradeError::Refused(c)) if c.message.contains("maintenance")), "{close:?}");
    assert_eq!(e.count("/order").await, 0, "nothing sent");
    assert_eq!(e.trader.cancel(&e.driver, &a, "BTC", "v0").await.unwrap(), Cancelled::Sent);
    e.trader.set_maintenance(&a, None);
    assert!(e.trader.maintenance(&a).is_none());
    e.place(order("m2", Side::Buy, OrderType::Limit, "0.01")).await.unwrap();
    assert_eq!(e.count("/order").await, 1);
}

/// A lookup by venue id walks the pages the venue offers, following the cursor up to the
/// mapper's cap, as a lookup by client id does; a page's cursor may come in a header.
#[tokio::test]
async fn a_lookup_by_venue_id_walks_pages_and_reads_a_header_cursor() {
    for (pages, found) in [(3, true), (2, false)] {
        let e = Env::new(Knobs { lookup_pages: pages, ..Knobs::default() }, rules(), "vpages").await;
        Mock::given(method("GET")).and(path("/order")).and(query_param("cursor", "c3")).respond_with(ok(json!({ "rows": [row("v9", "p1", "new", "0.01")] }))).with_priority(1).mount(&e.server).await;
        // The second page hands its cursor in the `x-next` header only.
        Mock::given(method("GET"))
            .and(path("/order"))
            .and(query_param("cursor", "c2"))
            .respond_with(ok(json!({ "rows": [row("x", "other", "new", "1")] })).insert_header("x-next", "c3"))
            .with_priority(1)
            .mount(&e.server)
            .await;
        Mock::given(method("GET")).and(path("/order")).respond_with(ok(json!({ "rows": [], "next": "c2" }))).with_priority(5).mount(&e.server).await;
        let got = e.trader.by_venue_id(&e.driver, &e.account(), "BTC", "v9").await.unwrap();
        assert_eq!(got.map(|o| o.venue_id), found.then(|| "v9".to_string()), "{pages} pages");
        assert_eq!(e.count("/order").await, pages);
    }
}

/// A change whose answer is lost: the order is read again once by its venue id, and its state
/// answered. Working with the value asked: changed. Working at the old one: refused, unchanged.
/// Ended: refused naming the state it ended in. Not found: unknown, as before.
#[tokio::test]
async fn a_change_with_no_clear_answer_reads_the_order_once_and_answers_its_state() {
    let change = otw_connector_model::OrderAmend { limit: Some(Price::new(d("31000"))), ..Default::default() };
    let working = |px: &str, status: &str| json!({ "rows": [{ "id": "v1", "cid": "k1", "symbol": "BTC", "side": "buy", "status": status, "qty": "0.01", "filled": "0", "price": px }] });
    for (after, want) in [(Some(working("31000", "new")), "changed"), (Some(working("30000", "new")), "unchanged"), (Some(working("30000", "cancelled")), "ended"), (None, "unknown")] {
        let e = Env::new(Knobs::default(), rules(), "amend-lost").await;
        Mock::given(method("GET")).and(path("/order")).respond_with(ok(working("30000", "new"))).up_to_n_times(1).with_priority(1).mount(&e.server).await;
        let later = match after {
            Some(v) => ok(v),
            None => ok(json!({ "rows": [] })),
        };
        Mock::given(method("GET")).and(path("/order")).respond_with(later).with_priority(5).mount(&e.server).await;
        Mock::given(method("POST")).and(path("/amend")).respond_with(ResponseTemplate::new(502)).mount(&e.server).await;
        let got = e.trader.modify(&e.driver, &e.account(), "BTC", "v1", &change).await;
        match (want, &got) {
            ("changed", Ok(None)) => {}
            ("unchanged", Err(TradeError::Refused(c))) if c.message.contains("still works unchanged") => {}
            ("ended", Err(TradeError::Refused(c))) if c.message.contains("has ended (Cancelled") => {}
            ("unknown", Err(TradeError::Unknown { alert: false, message, .. })) if message.contains("read the order again") => {}
            _ => panic!("{want}: {got:?}"),
        }
        assert_eq!((e.count("/order").await, e.count("/amend").await), (2, 1), "{want}: read before, once after, never sent again");
    }
}
