//! The trading catalog (onboarding spec 10.3, T01 to T34) run against a scripted mock venue,
//! through the shared trader and a venue's real mapper.
//!
//! A venue writes a [`VenueScript`]: its answers, built from its documented examples, for the
//! calls the scenarios need. The runner asks the mapper which calls those are (method, path,
//! query), mounts the answers there, and drives the trader. What the venue does not declare is
//! asserted refused before any order call leaves; what it declares is asserted working.
//!
//! Scenarios that live above the trader (the trading grant, the lock, the risk gate, drift and
//! its acknowledgement) are listed as covered elsewhere, with where.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_engine::trading::{Account, Cancelled, TicketOptions, TradeError, Trader};
use otw_connector_model::{
    Decimal, DryRunCap, ErrorKind, InstrumentId, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus, OrderType, Price, Qty, Side, TifKind, TimeInForce,
};
use otw_connector_sdk::trade::{PositionSide, Settings, TradeMapper, VenueRules};
use otw_connector_sdk::{Driver, HttpRequest, Mapper, Method, Signer};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::{driver_against, ManualClock};

/// A refusal the script answers in the venue's own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueRefusal {
    /// Balance or margin (T15).
    Funds,
    /// A reduce-only order that would add (T16).
    ReduceOnly,
    /// The venue does not know the order.
    NotFound,
    /// The order is already final (a cancel that raced a fill, T22).
    Final,
    /// A rate limit, with the venue's wait in seconds (T30).
    RateLimited(u64),
    /// The request's time is outside the venue's window (T29).
    ClockSkew,
    /// A code the venue's table does not list.
    Unmapped,
    /// The gateway in front of the venue fell over (HTTP 5xx).
    Gateway,
    /// An order sent with the other position mode's index (the account switched mode).
    PositionModeMismatch,
}

/// One order row as a script writes it.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub client_id: Option<&'a str>,
    pub venue_id: &'a str,
    pub word: &'a str,
    pub side: Side,
    pub qty: Decimal,
    pub filled: Decimal,
}

/// One fill row as a script writes it.
#[derive(Debug, Clone)]
pub struct FillRow<'a> {
    pub exec_id: &'a str,
    pub venue_id: &'a str,
    pub client_id: &'a str,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
}

/// A venue's answers for the catalog, from its documented examples.
pub trait VenueScript: Send + Sync {
    type M: TradeMapper + Clone;
    fn mapper(&self) -> Self::M;
    /// An account's settings (non-secret ones and credentials alike).
    fn settings(&self) -> Settings;
    fn signer(&self) -> Arc<dyn Signer>;
    fn symbol(&self) -> &'static str;
    /// A size and a limit price the rules of [`VenueScript::rules`] accept.
    fn qty(&self) -> Decimal;
    fn price(&self) -> Decimal;
    /// The instrument list answer for [`VenueScript::symbol`], trading or in the venue's
    /// `closed` word.
    fn rules(&self, closed: Option<&str>) -> String;
    /// A status word the venue lists an instrument with when it does not trade it.
    fn closed_word(&self) -> &'static str;
    /// The venue's word for a canonical state.
    fn word(&self, s: OrderStatus) -> &'static str;
    fn ack(&self, client_id: &str, venue_id: &str) -> String;
    fn orders(&self, rows: &[Row<'_>]) -> String;
    fn fills(&self, rows: &[FillRow<'_>]) -> String;
    /// The fee currency the fills answer carries (T04).
    fn fee_currency(&self) -> &'static str;
    /// The positions answer: `(side, direction, size)` per row.
    fn positions(&self, rows: &[(PositionSide, Option<Side>, Decimal)]) -> String;
    /// A success with nothing to read (a cancel's answer).
    fn ok(&self) -> String;
    /// The venue's check passing.
    fn dry_run_ok(&self) -> String;
    fn refusal(&self, r: VenueRefusal) -> Answer;
}

/// A venue answer: status, body, headers.
#[derive(Debug, Clone)]
pub struct Answer {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(&'static str, String)>,
}

impl Answer {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Answer { status, body: body.into(), headers: Vec::new() }
    }
}

/// The venue clock every catalog scenario runs at, epoch milliseconds (a rate-limit answer
/// that states when its window resets states it against this).
pub const CATALOG_NOW_MS: i64 = 1_767_225_600_000;

/// What a catalog run covered.
#[derive(Debug, Default)]
pub struct Report {
    /// Scenarios run, by id.
    pub ran: Vec<&'static str>,
    /// Scenarios not meaningful at this level, with where they are covered or why not.
    pub elsewhere: Vec<(&'static str, &'static str)>,
    pub failures: Vec<String>,
}

struct Env<S: VenueScript> {
    server: MockServer,
    driver: Driver<S::M>,
    trader: Trader<S::M>,
    settings: Settings,
    signer: Arc<dyn Signer>,
    scope: String,
    rules: VenueRules,
}

/// The request matches a mapper-built one: method, path, and every query pair it set
/// (a query pair it sets to the account's own value counts too).
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

fn body_has(req: &Request, needle: &str) -> bool {
    String::from_utf8_lossy(&req.body).contains(needle)
}

impl<S: VenueScript> Env<S> {
    async fn new(s: &S, tag: &str) -> Self {
        let server = MockServer::start().await;
        let clock = ManualClock::at(crate::ts(CATALOG_NOW_MS / 1000));
        let driver = driver_against(s.mapper(), &server, clock);
        let settings = s.settings();
        let m = s.mapper();
        let rules_req = m.instrument_rules(&settings, Some(s.symbol()), otw_connector_model::CorrId(1)).unwrap();
        let (rows, _) = m.decode_rules(s.rules(None).as_bytes()).unwrap();
        let rules = rows.into_iter().find(|r| r.instrument.native == s.symbol()).expect("the rules answer lists the script's symbol");
        let env = Env { server, driver, trader: Trader::new(), settings, signer: s.signer(), scope: format!("catalog:{tag}"), rules };
        env.mount(&rules_req, 200, s.rules(None), 4).await;
        // One-way and flat by default, where the venue holds positions.
        if let Some(p) = m.positions(&env.settings, s.symbol(), otw_connector_model::CorrId(1)).unwrap() {
            env.mount(&p, 200, s.positions(&[(PositionSide::Net, None, Decimal::ZERO)]), 5).await;
        }
        env
    }

    fn account(&self) -> Account<'_> {
        Account { settings: &self.settings, signer: self.signer.as_ref(), scope: &self.scope }
    }

    /// Answer `want` with `body`, before any mount of a higher `priority` number.
    async fn mount(&self, want: &HttpRequest, status: u16, body: String, priority: u8) {
        self.mount_answer(want, Answer::new(status, body), priority).await;
    }

    async fn mount_answer(&self, want: &HttpRequest, a: Answer, priority: u8) {
        let want = want.clone();
        let mut t = ResponseTemplate::new(a.status).set_body_string(a.body).insert_header("content-type", "application/json");
        for (k, v) in a.headers {
            t = t.insert_header(k, v.as_str());
        }
        Mock::given(move |r: &Request| matches(r, &want)).respond_with(t).with_priority(priority).mount(&self.server).await;
    }

    /// Answer `want` with `body` only when the request's body carries `needle`.
    async fn mount_body(&self, want: &HttpRequest, needle: &str, status: u16, body: String, priority: u8) {
        let (want, needle) = (want.clone(), needle.to_string());
        Mock::given(move |r: &Request| matches(r, &want) && body_has(r, &needle))
            .respond_with(ResponseTemplate::new(status).set_body_string(body).insert_header("content-type", "application/json"))
            .with_priority(priority)
            .mount(&self.server)
            .await;
    }

    async fn count(&self, want: &HttpRequest) -> usize {
        self.server.received_requests().await.unwrap_or_default().iter().filter(|r| matches(r, want)).count()
    }

    async fn bodies(&self, want: &HttpRequest) -> Vec<String> {
        self.server.received_requests().await.unwrap_or_default().iter().filter(|r| matches(r, want)).map(|r| String::from_utf8_lossy(&r.body).to_string()).collect()
    }
}

fn order(s: &impl VenueScript, key: &str, side: Side, t: OrderType) -> OrderRequest {
    let (limit, stop) = t.prices();
    OrderRequest {
        client_order_id: key.into(),
        instrument: InstrumentId::new("CATALOG", s.symbol()),
        side,
        order_type: t,
        qty: Qty::new(s.qty()),
        limit: limit.then(|| Price::new(s.price())),
        stop: stop.then(|| Price::new(s.price())),
        tif: TimeInForce::Gtc,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts::default(),
    }
}

/// Fix the instrument id to the venue's own (the trader compares rules by native symbol only).
fn on(rules: &VenueRules, mut o: OrderRequest) -> OrderRequest {
    o.instrument = rules.instrument.clone();
    o
}

type Outcome = Result<(), String>;

fn check(ok: bool, what: impl Into<String>) -> Outcome {
    if ok { Ok(()) } else { Err(what.into()) }
}

/// Fills are asked from an hour before the catalog's clock.
const SINCE_MS: i64 = CATALOG_NOW_MS - 3_600_000;

/// The fills call the trader makes for the last hour (one window), matched on what it names
/// besides the window.
fn fills_req<S: VenueScript>(s: &S, e: &Env<S>) -> HttpRequest {
    let mut r = s.mapper().fills_since(&e.settings, s.symbol(), SINCE_MS, CATALOG_NOW_MS, otw_connector_model::CorrId(1)).unwrap();
    r.query.retain(|(k, _)| !k.to_ascii_lowercase().contains("time"));
    r
}

fn place_req<S: VenueScript>(s: &S, e: &Env<S>, o: &OrderRequest) -> HttpRequest {
    let p = otw_connector_sdk::trade::Placing::new(o.client_order_id.clone(), PositionSide::Net, otw_connector_model::CorrId(1));
    let mut r = s.mapper().place(&e.settings, &on(&e.rules, o.clone()), &p).unwrap();
    // Matched on method and path: the body says the rest.
    r.query.clear();
    r
}

/// Any order call at all (the path of a plain limit place): what "refused before I/O" counts.
async fn order_calls<S: VenueScript>(s: &S, e: &Env<S>) -> usize {
    let probe = place_req(s, e, &order(s, "probe", Side::Buy, OrderType::Limit));
    e.count(&probe).await
}

async fn refused_before_io<S: VenueScript>(s: &S, e: &Env<S>, orders: &[OrderRequest], what: &str) -> Outcome {
    let orders: Vec<OrderRequest> = orders.iter().cloned().map(|o| on(&e.rules, o)).collect();
    let before = order_calls(s, e).await;
    let out = e.trader.place(&e.driver, &e.account(), &orders, &TicketOptions::default()).await;
    check(matches!(out, Err(TradeError::Refused(_))), format!("{what}: refused expected, got {out:?}"))?;
    check(order_calls(s, e).await == before, format!("{what}: an order call left"))
}

async fn lookup_mount<S: VenueScript>(s: &S, e: &Env<S>, key: &str, body: String) {
    let m = s.mapper();
    let reqs = m.query_by_client_id(&e.settings, s.symbol(), key, otw_connector_model::CorrId(1)).unwrap();
    for r in reqs {
        e.mount(&r, 200, body.clone(), 2).await;
    }
}

async fn by_id_mount<S: VenueScript>(s: &S, e: &Env<S>, venue_id: &str, body: String) {
    let m = s.mapper();
    for r in m.query_by_venue_id(&e.settings, s.symbol(), venue_id, otw_connector_model::CorrId(1)).unwrap() {
        e.mount(&r, 200, body.clone(), 2).await;
    }
}

fn row<'a>(s: &impl VenueScript, cid: &'a str, vid: &'a str, st: OrderStatus, qty: Decimal, filled: Decimal) -> Row<'a> {
    Row { client_id: Some(cid), venue_id: vid, word: s.word(st), side: Side::Buy, qty, filled }
}

// ── Scenarios ──

async fn t01<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t01").await;
    let o = order(s, "t01k", Side::Buy, OrderType::Limit);
    e.mount(&place_req(s, &e, &o), 200, s.ack("t01k", "v-t01"), 3).await;
    let placed = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o.clone())], &TicketOptions::default()).await.map_err(|x| format!("place: {x}"))?;
    check(placed.len() == 1 && placed[0].venue_id == "v-t01", format!("placed {placed:?}"))?;
    let cancel = s.mapper().cancel(&e.settings, s.symbol(), "v-t01", otw_connector_model::CorrId(1)).unwrap();
    e.mount(&cancel, 200, s.ok(), 3).await;
    let c = e.trader.cancel(&e.driver, &e.account(), s.symbol(), "v-t01").await.map_err(|x| format!("cancel: {x}"))?;
    check(c == Cancelled::Sent, format!("cancel {c:?}"))?;
    check(e.count(&place_req(s, &e, &o)).await == 1, "sent once")
}

async fn t02<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t02").await;
    let o = order(s, "t02k", Side::Buy, OrderType::Limit);
    e.mount(&place_req(s, &e, &o), 200, s.ack("t02k", "v-t02"), 3).await;
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await.map_err(|x| format!("place: {x}"))?;
    lookup_mount(s, &e, "t02k", s.orders(&[row(s, "t02k", "v-t02", OrderStatus::Filled, s.qty(), s.qty())])).await;
    let got = e.trader.lookup(&e.driver, &e.account(), s.symbol(), "t02k").await.map_err(|x| format!("lookup: {x}"))?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::Filled) && got.filled_qty == s.qty(), format!("{got:?}"))
}

/// T03 partial fills add up; T24 a duplicate execution is dropped by its id.
async fn t03<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t03").await;
    let half = s.qty() / Decimal::TWO;
    lookup_mount(s, &e, "t03k", s.orders(&[row(s, "t03k", "v-t03", OrderStatus::PartiallyFilled, s.qty(), half)])).await;
    let got = e.trader.lookup(&e.driver, &e.account(), s.symbol(), "t03k").await.map_err(|x| format!("lookup: {x}"))?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::PartiallyFilled) && got.filled_qty == half, format!("{got:?}"))?;
    let p1 = s.price();
    let p2 = s.price() + e.rules.tick.unwrap_or(Decimal::ONE);
    let f = |id, px| FillRow { exec_id: id, venue_id: "v-t03", client_id: "t03k", qty: half, price: px, fee: Decimal::new(1, 2) };
    let req = fills_req(s, &e);
    e.mount(&req, 200, s.fills(&[f("x1", p1), f("x2", p2), f("x2", p2)]), 3).await;
    let fills = e.trader.fills_since(&e.driver, &e.account(), s.symbol(), SINCE_MS).await.map_err(|x| format!("fills: {x}"))?;
    check(fills.len() == 2, format!("T24: a duplicate execution is dropped by id: {} fills", fills.len()))?;
    let qty: Decimal = fills.iter().map(|f| f.qty).sum();
    let value: Decimal = fills.iter().map(|f| f.qty * f.price).sum();
    let fees: Decimal = fills.iter().map(|f| f.fee).sum();
    check(qty == s.qty() && value / qty == (p1 + p2) / Decimal::TWO && fees == Decimal::new(2, 2), "average price and fees add up")
}

async fn t04<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t04").await;
    for (key, side) in [("t04b", Side::Buy), ("t04s", Side::Sell)] {
        let o = order(s, key, side, OrderType::Market);
        e.mount_body(&place_req(s, &e, &o), key, 200, s.ack(key, &format!("v-{key}")), 3).await;
        let placed = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await.map_err(|x| format!("{key}: {x}"))?;
        check(placed[0].venue_id == format!("v-{key}"), format!("{placed:?}"))?;
    }
    let req = fills_req(s, &e);
    e.mount(&req, 200, s.fills(&[FillRow { exec_id: "x", venue_id: "v-t04b", client_id: "t04b", qty: s.qty(), price: s.price(), fee: Decimal::ONE }]), 3).await;
    let fills = e.trader.fills_since(&e.driver, &e.account(), s.symbol(), SINCE_MS).await.map_err(|x| format!("fills: {x}"))?;
    check(fills.first().is_some_and(|f| f.fee_currency == s.fee_currency()), format!("fee currency: {fills:?}"))
}

/// T05 stop and stop-limit go out where declared, with their stop price; refused before I/O
/// where not.
async fn t05<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t05").await;
    let caps = s.mapper().capabilities().orders.clone();
    for t in [OrderType::Stop, OrderType::StopLimit] {
        let key = if t == OrderType::Stop { "t05s" } else { "t05l" };
        let o = order(s, key, Side::Buy, t);
        if !caps.types.contains(&t) {
            refused_before_io(s, &e, &[o], &format!("undeclared {t:?}")).await?;
            continue;
        }
        e.mount_body(&place_req(s, &e, &o), key, 200, s.ack(key, &format!("v-{key}")), 3).await;
        e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o.clone())], &TicketOptions::default()).await.map_err(|x| format!("{t:?}: {x}"))?;
        let sent = e.bodies(&place_req(s, &e, &o)).await;
        let stop = s.price().normalize().to_string();
        check(sent.iter().any(|b| b.contains(key) && b.contains(&stop)), format!("{t:?} carries its stop price: {sent:?}"))?;
    }
    Ok(())
}

/// T06 (amend), T07 (brackets), T08 (OCA), T17 (post-only): refused before I/O where undeclared.
async fn t06_07_08_17<S: VenueScript>(s: &S, r: &mut Report) -> Outcome {
    let e = Env::new(s, "t06").await;
    let caps = s.mapper().capabilities().orders.clone();
    if !caps.amend {
        let out = e.trader.modify(&e.driver, &e.account(), s.symbol(), "v1", &OrderAmend { limit: Some(Price::new(s.price())), ..Default::default() }).await;
        check(matches!(out, Err(TradeError::Refused(ref x)) if x.kind == ErrorKind::Unsupported), format!("T06 amend: {out:?}"))?;
        check(e.server.received_requests().await.unwrap_or_default().is_empty(), "T06: nothing sent")?;
        r.ran.push("T06");
    } else {
        r.elsewhere.push(("T06", "amend declared: the venue's own tests cover it"));
    }
    if !caps.brackets {
        let entry = order(s, "t07e", Side::Buy, OrderType::Limit);
        let exit = OrderRequest { parent: Some("t07e".into()), side: Side::Sell, ..order(s, "t07x", Side::Sell, OrderType::Limit) };
        refused_before_io(s, &e, &[entry, exit], "T07 bracket").await?;
        r.ran.push("T07");
    } else {
        r.elsewhere.push(("T07", "brackets declared: the venue's own tests cover them"));
    }
    if !caps.oca {
        let a = OrderRequest { oca_group: Some("g".into()), ..order(s, "t08a", Side::Sell, OrderType::Limit) };
        let b = OrderRequest { oca_group: Some("g".into()), ..order(s, "t08b", Side::Sell, OrderType::Limit) };
        refused_before_io(s, &e, &[a, b], "T08 OCA").await?;
        r.ran.push("T08");
    } else {
        r.elsewhere.push(("T08", "OCA declared: the venue's own tests cover it"));
    }
    if !caps.post_only {
        let mut o = order(s, "t17", Side::Buy, OrderType::Limit);
        o.opts.post_only = true;
        refused_before_io(s, &e, &[o], "T17 post-only").await?;
        r.ran.push("T17");
    } else {
        r.elsewhere.push(("T17", "post-only declared: the venue's own tests cover it"));
    }
    Ok(())
}

async fn t09<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t09").await;
    let mut o = order(s, "t09k", Side::Buy, OrderType::Limit);
    o.tif = TimeInForce::Ioc;
    if !s.mapper().capabilities().orders.tif.contains(&TifKind::Ioc) {
        return refused_before_io(s, &e, &[o], "undeclared IOC").await;
    }
    e.mount(&place_req(s, &e, &o), 200, s.ack("t09k", "v-t09"), 3).await;
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await.map_err(|x| format!("place: {x}"))?;
    let part = s.qty() / Decimal::TWO;
    lookup_mount(s, &e, "t09k", s.orders(&[row(s, "t09k", "v-t09", OrderStatus::Cancelled, s.qty(), part)])).await;
    let got = e.trader.lookup(&e.driver, &e.account(), s.symbol(), "t09k").await.map_err(|x| format!("lookup: {x}"))?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::Cancelled) && got.filled_qty == part, format!("the IOC remainder is cancelled: {got:?}"))
}

/// T10 close at the market: long, short, partial; each reduce-only on the side held. A book
/// with no positions refuses before I/O.
async fn t10<S: VenueScript>(s: &S) -> Outcome {
    let m = s.mapper();
    let e = Env::new(s, "t10").await;
    let Some(pos) = m.positions(&e.settings, s.symbol(), otw_connector_model::CorrId(1)).unwrap() else {
        let out = e.trader.close_position(&e.driver, &e.account(), s.symbol(), None, None, "t10").await;
        return check(matches!(out, Err(TradeError::Refused(_))) && order_calls(s, &e).await == 0, format!("a book with no positions: {out:?}"));
    };
    let q = s.qty() * Decimal::TWO;
    for (key, dir, part, want_side) in [("t10l", Side::Buy, None, "Sell"), ("t10s", Side::Sell, None, "Buy"), ("t10p", Side::Buy, Some(s.qty()), "Sell")] {
        let e = Env::new(s, key).await;
        e.mount(&pos, 200, s.positions(&[(PositionSide::Net, Some(dir), q)]), 1).await;
        let probe = place_req(s, &e, &order(s, key, Side::Buy, OrderType::Market));
        e.mount(&probe, 200, s.ack(key, &format!("v-{key}")), 3).await;
        let placed = e.trader.close_position(&e.driver, &e.account(), s.symbol(), None, part, key).await.map_err(|x| format!("{key}: {x}"))?;
        check(placed.venue_id == format!("v-{key}"), format!("{placed:?}"))?;
        let sent = e.bodies(&probe).await;
        let size = part.unwrap_or(q).normalize().to_string();
        // A venue that counts contracts says the size in contracts; side words in any case.
        let contracts = e.rules.contract_size.filter(|c| !c.is_zero()).map(|c| (part.unwrap_or(q) / c).normalize().to_string());
        let body = sent.first().map(|b| b.to_ascii_lowercase()).unwrap_or_default();
        let sized = body.contains(&size) || contracts.as_ref().is_some_and(|c| body.contains(c.as_str()));
        check(sent.len() == 1 && body.contains(&want_side.to_ascii_lowercase()) && sized && body.contains("reduce"), format!("{key}: closes {size} on the other side: {sent:?}"))?;
    }
    // More than held is refused, nothing sent.
    let e = Env::new(s, "t10x").await;
    e.mount(&pos, 200, s.positions(&[(PositionSide::Net, Some(Side::Buy), s.qty())]), 1).await;
    let out = e.trader.close_position(&e.driver, &e.account(), s.symbol(), None, Some(q), "t10x").await;
    check(matches!(out, Err(TradeError::Refused(_))) && order_calls(s, &e).await == 0, format!("closing more than held: {out:?}"))
}

/// T11 the venue's rules refuse before I/O, naming the nearest values.
async fn t11<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t11").await;
    let r = e.rules.clone();
    let base = order(s, "t11", Side::Buy, OrderType::Limit);
    if let Some(tick) = r.tick {
        let o = OrderRequest { limit: Some(Price::new(s.price() + tick / Decimal::TWO)), ..base.clone() };
        refused_before_io(s, &e, &[o], "off tick").await?;
    }
    if let Some(step) = r.lot_step {
        let o = OrderRequest { qty: Qty::new(s.qty() + step / Decimal::TWO), ..base.clone() };
        refused_before_io(s, &e, &[o], "off lot").await?;
    }
    if let Some(min) = r.min_qty.filter(|m| *m > r.lot_step.unwrap_or_default()) {
        let o = OrderRequest { qty: Qty::new(min - r.lot_step.unwrap_or(min / Decimal::TWO)), ..base.clone() };
        refused_before_io(s, &e, &[o], "under min qty").await?;
    }
    if let Some(min) = r.min_notional {
        let px = s.price();
        let step = r.lot_step.unwrap_or(Decimal::new(1, 8));
        let q = ((min / px) / step).floor() * step;
        let q = if q * px >= min { q - step } else { q };
        if q > Decimal::ZERO && r.min_qty.is_none_or(|m| q >= m) {
            let o = OrderRequest { qty: Qty::new(q), ..base.clone() };
            refused_before_io(s, &e, &[o], "under min notional").await?;
        }
    }
    if let Some(max) = r.max_qty {
        let o = OrderRequest { qty: Qty::new(max + r.lot_step.unwrap_or(Decimal::ONE)), ..base.clone() };
        refused_before_io(s, &e, &[o], "over max qty").await?;
    }
    Ok(())
}

/// T12 every type and time in force the venue does not declare is refused before I/O.
async fn t12<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t12").await;
    let caps = s.mapper().capabilities().orders.clone();
    for t in [OrderType::Market, OrderType::Limit, OrderType::Stop, OrderType::StopLimit, OrderType::Mit, OrderType::Lit] {
        if !caps.types.contains(&t) {
            refused_before_io(s, &e, &[order(s, "t12", Side::Buy, t)], &format!("type {t:?}")).await?;
        }
    }
    let mut trail = order(s, "t12t", Side::Buy, OrderType::TrailingStop);
    trail.opts.trail = Some(otw_connector_model::Trail::Percent(Decimal::ONE));
    if !caps.types.contains(&OrderType::TrailingStop) {
        refused_before_io(s, &e, &[trail], "trailing stop").await?;
    }
    for (k, tif) in [(TifKind::Day, TimeInForce::Day), (TifKind::Ioc, TimeInForce::Ioc), (TifKind::Fok, TimeInForce::Fok), (TifKind::Gtc, TimeInForce::Gtc)] {
        if !caps.tif.contains(&k) {
            let o = OrderRequest { tif, ..order(s, "t12d", Side::Buy, OrderType::Limit) };
            refused_before_io(s, &e, &[o], &format!("tif {k:?}")).await?;
        }
    }
    if !caps.tif.contains(&TifKind::Gtd) {
        let o = OrderRequest { tif: TimeInForce::Gtd(crate::ts(1_900_000_000)), ..order(s, "t12g", Side::Buy, OrderType::Limit) };
        refused_before_io(s, &e, &[o], "tif Gtd").await?;
    }
    let mut ice = order(s, "t12i", Side::Buy, OrderType::Limit);
    ice.opts.display_qty = Some(Qty::new(s.qty() / Decimal::TWO));
    if !caps.iceberg {
        refused_before_io(s, &e, &[ice], "iceberg").await?;
    }
    Ok(())
}

async fn venue_refuses<S: VenueScript>(s: &S, tag: &str, o: OrderRequest, r: VenueRefusal) -> Result<(Env<S>, Result<Vec<otw_connector_engine::trading::Placed>, TradeError>), String> {
    let e = Env::new(s, tag).await;
    e.mount_answer(&place_req(s, &e, &o), s.refusal(r), 3).await;
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await;
    Ok((e, out))
}

async fn t15<S: VenueScript>(s: &S) -> Outcome {
    let (e, out) = venue_refuses(s, "t15", order(s, "t15k", Side::Buy, OrderType::Limit), VenueRefusal::Funds).await?;
    check(matches!(out, Err(TradeError::Refused(_))), format!("funds is a refusal, not unknown: {out:?}"))?;
    check(order_calls(s, &e).await == 1, "sent once")
}

async fn t16<S: VenueScript>(s: &S) -> Outcome {
    let mut o = order(s, "t16k", Side::Buy, OrderType::Limit);
    o.opts.reduce_only = true;
    if !s.mapper().capabilities().orders.reduce_only {
        let e = Env::new(s, "t16").await;
        return refused_before_io(s, &e, &[o], "undeclared reduce-only").await;
    }
    let (e, out) = venue_refuses(s, "t16", o.clone(), VenueRefusal::ReduceOnly).await?;
    check(matches!(out, Err(TradeError::Refused(_))), format!("a reduce-only that would add is refused: {out:?}"))?;
    let sent = e.bodies(&place_req(s, &e, &o)).await;
    check(sent.len() == 1 && sent[0].to_ascii_lowercase().contains("reduce"), format!("sent reduce-only: {sent:?}"))
}

async fn t18<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t18").await;
    let req = s.mapper().instrument_rules(&e.settings, Some(s.symbol()), otw_connector_model::CorrId(1)).unwrap();
    e.mount(&req, 200, s.rules(Some(s.closed_word())), 1).await;
    refused_before_io(s, &e, &[order(s, "t18", Side::Buy, OrderType::Limit)], "closed instrument").await
}

/// T19 lost answer, order at the venue: adopted, sent once.
async fn t19<S: VenueScript>(s: &S) -> Outcome {
    let (e, out) = {
        let e = Env::new(s, "t19").await;
        let o = order(s, "t19k", Side::Buy, OrderType::Limit);
        e.mount_answer(&place_req(s, &e, &o), s.refusal(VenueRefusal::Gateway), 3).await;
        lookup_mount(s, &e, "t19k", s.orders(&[row(s, "t19k", "v-t19", OrderStatus::New, s.qty(), Decimal::ZERO)])).await;
        let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await;
        (e, out)
    };
    let placed = out.map_err(|x| format!("adopted expected: {x}"))?;
    check(placed[0].venue_id == "v-t19", format!("{placed:?}"))?;
    check(order_calls(s, &e).await == 1, "never sent twice")
}

/// T20 lost answer, never at the venue: unknown now (the reconciliation cycle settles it as
/// not received after two "never seen" past the ack window), and the venue answers never seen.
async fn t20<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t20").await;
    let o = order(s, "t20k", Side::Buy, OrderType::Limit);
    e.mount_answer(&place_req(s, &e, &o), s.refusal(VenueRefusal::Gateway), 3).await;
    lookup_mount(s, &e, "t20k", s.orders(&[])).await;
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await;
    check(matches!(&out, Err(TradeError::Unknown { keys, alert: false, .. }) if keys == &vec!["t20k".to_string()]), format!("{out:?}"))?;
    let seen = e.trader.lookup(&e.driver, &e.account(), s.symbol(), "t20k").await.map_err(|x| x.to_string())?;
    check(seen.is_none(), "the venue answers never seen")?;
    check(order_calls(s, &e).await == 1, "never sent twice")
}

/// T21 lost answer, venue unreachable: unknown, named by its client id, nothing resent.
async fn t21<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t21").await;
    let o = order(s, "t21k", Side::Buy, OrderType::Limit);
    e.mount_answer(&place_req(s, &e, &o), s.refusal(VenueRefusal::Gateway), 3).await;
    for r in s.mapper().query_by_client_id(&e.settings, s.symbol(), "t21k", otw_connector_model::CorrId(1)).unwrap() {
        e.mount_answer(&r, s.refusal(VenueRefusal::Gateway), 2).await;
    }
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await;
    check(matches!(&out, Err(TradeError::Unknown { keys, .. }) if keys == &vec!["t21k".to_string()]), format!("{out:?}"))?;
    check(out.as_ref().err().is_some_and(|x| x.message().contains("do not send it again")), "the message says not to send it again")?;
    check(order_calls(s, &e).await == 1, "never sent twice")
}

/// An answer the venue's table does not map: unknown, and it alerts.
async fn unmapped<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "unm").await;
    let o = order(s, "unmk", Side::Buy, OrderType::Limit);
    e.mount_answer(&place_req(s, &e, &o), s.refusal(VenueRefusal::Unmapped), 3).await;
    lookup_mount(s, &e, "unmk", s.orders(&[])).await;
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], &TicketOptions::default()).await;
    check(matches!(&out, Err(TradeError::Unknown { alert: true, .. })), format!("unmapped answer: {out:?}"))?;
    // An unknown status word is never a near match.
    lookup_mount(s, &e, "unmw", s.orders(&[Row { word: "NoSuchWord", ..row(s, "unmw", "v-w", OrderStatus::New, s.qty(), Decimal::ZERO) }])).await;
    let got = e.trader.lookup(&e.driver, &e.account(), s.symbol(), "unmw").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(matches!(got.status, Mapped::Unknown(_)), format!("unknown word: {:?}", got.status))
}

/// T22 a cancel that raced a fill ends `Filled`, no error.
async fn t22<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t22").await;
    let cancel = s.mapper().cancel(&e.settings, s.symbol(), "v-t22", otw_connector_model::CorrId(1)).unwrap();
    e.mount_answer(&cancel, s.refusal(VenueRefusal::Final), 3).await;
    by_id_mount(s, &e, "v-t22", s.orders(&[row(s, "t22k", "v-t22", OrderStatus::Filled, s.qty(), s.qty())])).await;
    let out = e.trader.cancel(&e.driver, &e.account(), s.symbol(), "v-t22").await;
    check(out == Ok(Cancelled::AlreadyFinal(OrderStatus::Filled)), format!("{out:?}"))
}

/// T26 orders working while the socket was away: the venue's state is read and adopted.
async fn t26<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t26").await;
    lookup_mount(s, &e, "t26k", s.orders(&[row(s, "t26k", "v-t26", OrderStatus::Filled, s.qty(), s.qty())])).await;
    let open = s.mapper().open_orders(&e.settings, Some(s.symbol()), otw_connector_model::CorrId(1)).unwrap();
    for r in &open {
        e.mount(r, 200, s.orders(&[]), 3).await;
    }
    let working = e.trader.open_orders(&e.driver, &e.account(), Some(s.symbol())).await.map_err(|x| x.to_string())?;
    check(working.is_empty(), "nothing works any more")?;
    let got = e.trader.lookup(&e.driver, &e.account(), s.symbol(), "t26k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(got.status == Mapped::Known(OrderStatus::Filled), "the fill made while away is found")
}

async fn t29<S: VenueScript>(s: &S) -> Outcome {
    let (_, out) = venue_refuses(s, "t29", order(s, "t29k", Side::Buy, OrderType::Limit), VenueRefusal::ClockSkew).await?;
    check(matches!(out, Err(TradeError::Refused(ref x)) if x.kind == ErrorKind::Auth), format!("clock skew is refused, never unknown: {out:?}"))
}

/// T30 a rate limit refuses with the wait, and holds the next order before it leaves.
async fn t30<S: VenueScript>(s: &S) -> Outcome {
    let (e, out) = venue_refuses(s, "t30", order(s, "t30a", Side::Buy, OrderType::Limit), VenueRefusal::RateLimited(30)).await?;
    check(matches!(out, Err(TradeError::Refused(ref x)) if x.kind == ErrorKind::RateLimited), format!("{out:?}"))?;
    let again = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, order(s, "t30b", Side::Buy, OrderType::Limit))], &TicketOptions::default()).await;
    check(again.as_ref().err().is_some_and(|x| x.message().contains("holds new orders")), format!("held: {again:?}"))?;
    check(order_calls(s, &e).await == 1, "the second order never left")
}

/// T31 an order placed outside OTW is listed and the kill switch cancels it; T34 the kill
/// switch cancels every working order.
async fn t31_34<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t31").await;
    let m = s.mapper();
    let rows = [
        Row { client_id: None, venue_id: "ext-1", word: s.word(OrderStatus::New), side: Side::Sell, qty: s.qty(), filled: Decimal::ZERO },
        row(s, "t34a", "v-a", OrderStatus::New, s.qty(), Decimal::ZERO),
        row(s, "t34b", "v-b", OrderStatus::PartiallyFilled, s.qty(), s.qty() / Decimal::TWO),
    ];
    let open = m.open_orders(&e.settings, None, otw_connector_model::CorrId(1)).unwrap();
    // The first read lists them; the read after the cancel finds none (the mount is used up).
    for (i, r) in open.iter().enumerate() {
        let body = if i == 0 { s.orders(&rows) } else { s.orders(&[]) };
        let want = r.clone();
        Mock::given(move |q: &Request| matches(q, &want))
            .respond_with(ResponseTemplate::new(200).set_body_string(body.clone()))
            .up_to_n_times(1)
            .with_priority(3)
            .mount(&e.server)
            .await;
        e.mount(r, 200, s.orders(&[]), 4).await;
    }
    let mut cancels: Vec<HttpRequest> = Vec::new();
    if m.capabilities().orders.cancel_all_native {
        cancels.extend(m.cancel_all(&e.settings, &[s.symbol().to_string()], otw_connector_model::CorrId(1)).unwrap());
    } else {
        for v in ["ext-1", "v-a", "v-b"] {
            cancels.push(m.cancel(&e.settings, s.symbol(), v, otw_connector_model::CorrId(1)).unwrap());
        }
    }
    for c in &cancels {
        let mut c = c.clone();
        c.query.clear();
        e.mount(&c, 200, s.ok(), 3).await;
    }
    let n = e.trader.cancel_all(&e.driver, &e.account()).await.map_err(|x| x.to_string())?;
    check(n == 3, format!("three were working, the external one included: {n}"))?;
    let mut probe = cancels[0].clone();
    probe.query.clear();
    check(e.count(&probe).await >= 1, "the cancel went out")
}

/// T32 after a restart nothing is resent: the stored client id is looked up and adopted.
async fn t32<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "t32").await;
    lookup_mount(s, &e, "t32k", s.orders(&[row(s, "t32k", "v-t32", OrderStatus::New, s.qty(), Decimal::ZERO)])).await;
    let fresh: Trader<S::M> = Trader::new();
    let got = fresh.lookup(&e.driver, &e.account(), s.symbol(), "t32k").await.map_err(|x| x.to_string())?.ok_or("not found")?;
    check(got.venue_id == "v-t32" && got.status == Mapped::Known(OrderStatus::New), format!("{got:?}"))?;
    check(order_calls(s, &e).await == 0, "nothing resent")
}

/// Spec 6.1: a leg refused after another went out cancels it, and the answer names each leg.
async fn atomic<S: VenueScript>(s: &S) -> Outcome {
    let e = Env::new(s, "atom").await;
    let a = order(s, "atoma", Side::Buy, OrderType::Limit);
    let b = order(s, "atomb", Side::Buy, OrderType::Limit);
    e.mount_body(&place_req(s, &e, &a), "atoma", 200, s.ack("atoma", "v-atom"), 3).await;
    let no = s.refusal(VenueRefusal::Funds);
    e.mount_body(&place_req(s, &e, &b), "atomb", no.status, no.body, 3).await;
    let cancel = s.mapper().cancel(&e.settings, s.symbol(), "v-atom", otw_connector_model::CorrId(1)).unwrap();
    e.mount(&cancel, 200, s.ok(), 3).await;
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, a), on(&e.rules, b)], &TicketOptions::default()).await;
    check(matches!(out, Err(TradeError::Refused(ref x)) if x.message.contains("atoma: placed, then cancelled")), format!("{out:?}"))?;
    check(e.count(&cancel).await == 1, "the placed leg was cancelled")
}

/// One-way or hedge is read from the venue and decides the position each order names; a
/// mode the venue does not tell is refused before the order leaves.
async fn position_mode<S: VenueScript>(s: &S) -> Outcome {
    let m = s.mapper();
    let e = Env::new(s, "pm").await;
    let Some(pos) = m.positions(&e.settings, s.symbol(), otw_connector_model::CorrId(1)).unwrap() else { return Ok(()) };
    e.mount(&pos, 200, s.positions(&[]), 1).await;
    refused_before_io(s, &e, &[order(s, "pm0", Side::Buy, OrderType::Limit)], "an unknown position mode").await?;
    let e = Env::new(s, "pm2").await;
    e.mount(&pos, 200, s.positions(&[(PositionSide::Long, None, Decimal::ZERO), (PositionSide::Short, None, Decimal::ZERO)]), 1).await;
    let o = order(s, "pmh", Side::Sell, OrderType::Limit);
    e.mount_body(&place_req(s, &e, &o), "\"pmh\"", 200, s.ack("pmh", "v-pmh"), 3).await;
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o.clone())], &TicketOptions::default()).await.map_err(|x| x.to_string())?;
    let want = m.place(&e.settings, &on(&e.rules, o.clone()), &otw_connector_sdk::trade::Placing::new("pmh", PositionSide::Short, otw_connector_model::CorrId(1))).unwrap();
    let want_body = String::from_utf8_lossy(want.body.as_deref().unwrap_or_default()).to_string();
    let sent = e.bodies(&place_req(s, &e, &o)).await;
    check(sent == vec![want_body], format!("a hedge-mode sell opens the short: {sent:?}"))?;
    // Held, not read again on the order path: a second order reads no position.
    let reads = e.count(&pos).await;
    let o2 = order(s, "pmh2", Side::Sell, OrderType::Limit);
    e.mount_body(&place_req(s, &e, &o2), "pmh2", 200, s.ack("pmh2", "v-pmh2"), 3).await;
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o2)], &TicketOptions::default()).await.map_err(|x| x.to_string())?;
    check(e.count(&pos).await == reads, "the mode is held, not read before each order")?;
    // A refusal that may mean the mode changed: that order stays refused, nothing is resent,
    // and the next order reads the mode again.
    let o3 = order(s, "pmh3", Side::Sell, OrderType::Limit);
    let no = s.refusal(VenueRefusal::PositionModeMismatch);
    e.mount_body(&place_req(s, &e, &o3), "pmh3", no.status, no.body, 3).await;
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o3.clone())], &TicketOptions::default()).await;
    check(matches!(out, Err(TradeError::Refused(_))), format!("refused: {out:?}"))?;
    check(e.bodies(&place_req(s, &e, &o3)).await.iter().filter(|b| b.contains("pmh3")).count() == 1, "never resent")?;
    let o4 = order(s, "pmh4", Side::Sell, OrderType::Limit);
    e.mount_body(&place_req(s, &e, &o4), "pmh4", 200, s.ack("pmh4", "v-pmh4"), 3).await;
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o4)], &TicketOptions::default()).await.map_err(|x| x.to_string())?;
    check(e.count(&pos).await == reads + 1, "the mode is read again once after the refusal")
}

/// E2D: the venue's own check before the first order of the session, once; a refusal there
/// places nothing; an order the check does not cover is refused.
async fn session_check<S: VenueScript>(s: &S) -> Outcome {
    let m = s.mapper();
    if m.capabilities().orders.dry_run == DryRunCap::None {
        return Err("no dry run declared".into());
    }
    let e = Env::new(s, "dry").await;
    let a = order(s, "drya", Side::Buy, OrderType::Limit);
    let p = otw_connector_sdk::trade::Placing::new("drya", PositionSide::Net, otw_connector_model::CorrId(1));
    let mut check_req = m.dry_run(&e.settings, &on(&e.rules, a.clone()), &p).unwrap().ok_or("the venue's check covers a plain limit")?;
    check_req.query.clear();
    e.mount(&check_req, 200, s.dry_run_ok(), 3).await;
    e.mount_body(&place_req(s, &e, &a), "drya", 200, s.ack("drya", "v-drya"), 3).await;
    let opt = TicketOptions { reference: None, dry_run_first: true };
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, a)], &opt).await.map_err(|x| x.to_string())?;
    let b = order(s, "dryb", Side::Buy, OrderType::Limit);
    e.mount_body(&place_req(s, &e, &b), "dryb", 200, s.ack("dryb", "v-dryb"), 3).await;
    e.trader.place(&e.driver, &e.account(), &[on(&e.rules, b)], &opt).await.map_err(|x| x.to_string())?;
    check(e.count(&check_req).await == 1, "checked once per session")?;
    check(e.bodies(&check_req).await.iter().all(|b| !b.contains("drya")), "the check never carries the live order's client id")?;
    // Refused by the check: nothing placed.
    let e = Env::new(s, "dryno").await;
    e.mount_answer(&check_req, s.refusal(VenueRefusal::Funds), 3).await;
    refused_with(s, &e, order(s, "dryn", Side::Buy, OrderType::Limit), &opt, "a check that refuses").await?;
    // An order the check does not cover goes nowhere first.
    if let Some(t) = [OrderType::Stop, OrderType::StopLimit].into_iter().find(|t| m.capabilities().orders.types.contains(t)) {
        let e = Env::new(s, "drystop").await;
        refused_with(s, &e, order(s, "drys", Side::Buy, t), &opt, "an order the check does not cover").await?;
    }
    Ok(())
}

async fn refused_with<S: VenueScript>(s: &S, e: &Env<S>, o: OrderRequest, opt: &TicketOptions, what: &str) -> Outcome {
    let out = e.trader.place(&e.driver, &e.account(), &[on(&e.rules, o)], opt).await;
    check(matches!(out, Err(TradeError::Refused(_))), format!("{what}: {out:?}"))?;
    check(order_calls(s, e).await == 0, format!("{what}: nothing placed"))
}

/// Run the catalog for one venue. Panics naming every scenario that failed.
pub async fn run_catalog<S: VenueScript>(s: &S) -> Report {
    let mut r = Report::default();
    macro_rules! run {
        ($id:expr, $f:expr) => {{
            match $f.await {
                Ok(()) => r.ran.push($id),
                Err(e) => r.failures.push(format!("{}: {e}", $id)),
            }
        }};
    }
    run!("T01", t01(s));
    run!("T02", t02(s));
    run!("T03+T24", t03(s));
    run!("T04", t04(s));
    run!("T05", t05(s));
    let mut sub = Report::default();
    match t06_07_08_17(s, &mut sub).await {
        Ok(()) => {
            r.ran.extend(sub.ran);
            r.elsewhere.extend(sub.elsewhere);
        }
        Err(e) => r.failures.push(format!("T06-T08,T17: {e}")),
    }
    run!("T09", t09(s));
    run!("T10", t10(s));
    run!("T11", t11(s));
    run!("T12", t12(s));
    run!("T15", t15(s));
    run!("T16", t16(s));
    run!("T18", t18(s));
    run!("T19", t19(s));
    run!("T20", t20(s));
    run!("T21", t21(s));
    run!("unmapped", unmapped(s));
    run!("T22", t22(s));
    run!("T26", t26(s));
    run!("T29", t29(s));
    run!("T30", t30(s));
    run!("T31+T34", t31_34(s));
    run!("T32", t32(s));
    run!("6.1 atomicity", atomic(s));
    run!("position mode", position_mode(s));
    if s.mapper().capabilities().orders.dry_run == DryRunCap::None {
        r.elsewhere.push(("E2D session check", "the venue declares no dry run for this book"));
    } else {
        run!("E2D session check", session_check(s));
    }
    r.elsewhere.extend([
        ("T13", "risk gate: otw-core brokers/risk.rs, every broker alike"),
        ("T14", "grant, lock, demo: otw-core routes and dispatch, every broker alike"),
        ("T23", "amend races a fill: no amend declared at this level"),
        ("T25", "fill correction or bust: not documented by the venue (doc gap)"),
        ("T27", "listen key or session renewal: no private socket wired at this level"),
        ("T28", "cancel on disconnect: not declared"),
        ("T33", "drift and acknowledgement: otw-core reconcile and live (E2D downgrade)"),
    ]);
    if !r.failures.is_empty() {
        panic!("trading catalog failures:\n{}", r.failures.join("\n"));
    }
    r
}

/// Settings from pairs, for scripts.
pub fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}
