//! Orders over the TWS socket (execution, v2): canonical [`OrderRequest`]s in, IB's answers
//! out as [`OrderAck`]s and refusals.
//!
//! The translation is pure ([`ib_order`], [`transmits`], [`status_of`], [`off_tick`]); the
//! calls below it ([`place`], [`amend`], [`cancel`]) only send and wait. Three rules hold
//! throughout:
//!
//! - **Sent once.** An order call is never retried: a second attempt after a lost answer
//!   could place the order twice. A submission whose answer never came is looked up by its
//!   client id (`orderRef`, [`lookup`]); still not settled, it is reported unknown
//!   ([`PlaceError::Unknown`]) with the legs it names, never refused.
//! - **Paper until E4.** Live execution needs the order path at level E4 (spec 9); below it,
//!   a gateway signed in to anything but paper accounts (IB numbers them `D…`) is refused,
//!   except for the one account a maintainer names for the live canary ([`LIVE_CANARY`]).
//!   On a live account, the first order of a connection is sent as a what-if first
//!   ([`super::margin::dry_run`]).
//! - **On the tick, on the size grid.** A price off the contract's increment is refused with
//!   the two valid prices around it, where IB would answer a bare "Invalid Price"; a size under
//!   IB's minimum or off its size increment with the valid sizes around it ([`Rules`]).
//! - **One id, one order.** A client order id already sent to the gateway is refused, so a
//!   resent ticket cannot place the same order twice.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use ibapi::contracts::{Contract, ContractDetails, MarketRule, SecurityType};
use ibapi::contracts::TagValue;
use ibapi::orders::conditions::TriggerMethod;
use ibapi::orders::{Action, CancelOrder, Execution, ExecutionFilter, Executions, OcaType, Order, OrderStatus as IbStatus, Orders, PlaceOrder, TimeInForce as IbTif};
use ibapi::subscriptions::Subscription;
use otw_connector_model::{
    Capabilities, ConnectorError, Decimal, ErrorKind, ExecLevel, Mapped, OrderAck, OrderAmend, OrderRequest, OrderStatus, OrderType, Price, Side, TimeInForce,
    Trail, TriggerKind, Urgency,
};

use super::session::Session;

/// How long IB has to accept or refuse an order call.
pub const ORDER_WAIT: Duration = Duration::from_secs(10);
/// How long IB's reason is waited for once it answered an order `Inactive` (turned down, by
/// its margin check for one): the reason follows the status on the order's own stream.
const REASON_WAIT: Duration = Duration::from_secs(2);

/// Client order ids sent to each gateway by this process, seeded once from the orders IB
/// lists as working when the first order goes out.
static SENT: LazyLock<Mutex<HashMap<String, HashSet<String>>>> = LazyLock::new(Default::default);
/// Contract rules read from IB, by gateway and contract id, with when they were read.
type RulesCache = HashMap<(String, i32), (Instant, Rules)>;
static RULES: LazyLock<Mutex<RulesCache>> = LazyLock::new(Default::default);
/// How long a contract's rules are kept before IB is asked again (spec 4.2: daily).
const RULES_FOR: Duration = Duration::from_secs(24 * 3600);

fn refused(msg: impl Into<String>) -> ConnectorError {
    ConnectorError::new(ErrorKind::InvalidRequest, msg)
}

/// Why a placement answered no id for every order.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaceError {
    /// Nothing of the ticket works: refused before sending, or by IB (the legs already sent
    /// were cancelled).
    Refused(ConnectorError),
    /// Sent, and IB's answer was lost: the legs named (client ids) may be working. They are
    /// looked up by client id ([`lookup`]), never sent again.
    /// `unmapped`: IB answered with a code the adapter does not map ([`UNMAPPED`]), which the
    /// user is told about whatever the lookup finds.
    Unknown { keys: Vec<String>, message: String, unmapped: bool },
}

/// How the answer to an order call names an IB code the adapter does not map yet.
pub const UNMAPPED: &str = "a code OpenTraderWorld does not map yet";

impl From<ConnectorError> for PlaceError {
    fn from(e: ConnectorError) -> Self {
        PlaceError::Refused(e)
    }
}

impl std::fmt::Display for PlaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceError::Refused(e) => e.fmt(f),
            PlaceError::Unknown { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for PlaceError {}

/// How one order call ended when IB did not accept it.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// IB said no: nothing works from it.
    Refused(ConnectorError),
    /// No word IB stands by (the socket went, the answer never came, a duplicate id): the
    /// order may be working. The text says what happened.
    Lost(String),
}

/// The refusals IB documents for an order call (PROVIDER.md error table, quirks): 110 off the
/// tick, 201 rejected with a reason, 321 a request IB could not validate (the API in read-only
/// mode, among others), 10318 a fractional size.
const REFUSALS: [i32; 4] = [110, 201, 321, 10318];

/// IB's numbered answer to an order call, by the table of spec section 8. A duplicate order id
/// (103) leaves the order's fate open: unknown, never sent again. A message rate exceeded
/// (100) is a refusal to retry in a second; not connected (504, 1100) a transient refusal;
/// a documented refusal ([`REFUSALS`]) IB's refusal with its words. Every refusal carries
/// IB's code. Any other code is one no table maps: the order's fate is unknown (looked up by
/// client id, never refused on a guess), and it is logged as an error so the code gets mapped.
pub fn order_error(what: &str, code: i32, msg: &str) -> Outcome {
    let msg = msg.replace("<br>", "");
    let text = format!("Interactive Brokers refused {what}: {msg} ({code})");
    let e = match code {
        103 => return Outcome::Lost(format!("Interactive Brokers answered {what} with a duplicate order id ({msg}, 103)")),
        100 => ConnectorError::new(ErrorKind::RateLimited, format!("{text}. IB takes about 50 API messages a second: send it again in a second")).with_retry_after_ms(1000),
        504 | 1100 => ConnectorError::new(ErrorKind::Transient, format!("{text}. Nothing was placed: try again once the gateway is connected")),
        c if REFUSALS.contains(&c) => refused(text),
        _ => {
            tracing::error!("ibkr: unmapped order error {code} on {what}: {msg}");
            return Outcome::Lost(format!("Interactive Brokers answered {what} with code {code} ({msg}), {UNMAPPED}"));
        }
    };
    Outcome::Refused(e.with_code(code.to_string()))
}

fn f(p: Decimal) -> Result<f64, ConnectorError> {
    p.to_string().parse::<f64>().map_err(|_| ConnectorError::invalid(format!("{p} cannot be sent to Interactive Brokers")))
}

/// IB's word for an order's state, in the canonical vocabulary. `PreSubmitted` is an order IB
/// holds until its trigger (a stop, an attached exit): working, so `New`.
pub fn status_of(s: &str) -> Mapped<OrderStatus> {
    Mapped::Known(match s {
        "ApiPending" | "PendingSubmit" => OrderStatus::PendingNew,
        "PreSubmitted" | "Submitted" => OrderStatus::New,
        "PendingCancel" => OrderStatus::PendingCancel,
        "ApiCancelled" | "Cancelled" => OrderStatus::Cancelled,
        "Filled" => OrderStatus::Filled,
        "Inactive" => OrderStatus::Rejected,
        other => {
            tracing::warn!("ibkr: unknown order status {other:?}");
            return Mapped::Unknown(other.to_string());
        }
    })
}

fn dead(st: &IbStatus) -> bool {
    matches!(status_of(&st.status), Mapped::Known(s) if s.is_final() && s != OrderStatus::Filled)
}

/// A notice that is information, not a refusal: IB's warning band, its "will not be placed
/// until" holds, and the cancel confirmation.
fn benign(code: i32) -> bool {
    ibapi::messages::WARNING_CODE_RANGE.contains(&code) || code == 399 || code == ibapi::messages::ORDER_CANCELLED_CODE || code == 10349
}

/// IB's time in force, with the expiry a `GTD` order carries (UTC).
fn tif_of(t: TimeInForce) -> Result<(IbTif, String), ConnectorError> {
    Ok(match t {
        TimeInForce::Day => (IbTif::Day, String::new()),
        TimeInForce::Gtc => (IbTif::GoodTilCanceled, String::new()),
        TimeInForce::Ioc => (IbTif::ImmediateOrCancel, String::new()),
        TimeInForce::Fok => (IbTif::FillOrKill, String::new()),
        TimeInForce::Gtd(t) => {
            let at = t.to_offset_datetime().ok_or_else(|| ConnectorError::invalid("expiry out of range"))?;
            let fmt = time::macros::format_description!("[year][month][day]-[hour]:[minute]:[second]");
            (IbTif::GoodTilDate, at.format(&fmt).map_err(|e| ConnectorError::invalid(e.to_string()))?)
        }
    })
}

/// One canonical order as an IB order, ids aside. A trailing stop's distance rides in the
/// stop field as an amount, or as IB's trailing percentage; MIT and LIT trigger on `stop`.
pub fn ib_order(o: &OrderRequest) -> Result<Order, ConnectorError> {
    let price = |p: Option<Price>| p.map(|p| f(p.value())).transpose();
    let order_type = match o.order_type {
        OrderType::Market => "MKT",
        OrderType::Limit => "LMT",
        OrderType::Stop => "STP",
        OrderType::StopLimit => "STP LMT",
        OrderType::TrailingStop => "TRAIL",
        OrderType::Mit => "MIT",
        OrderType::Lit => "LIT",
    };
    let (tif, good_till_date) = tif_of(o.tif)?;
    let x = &o.opts;
    let (aux_price, trailing_percent) = match (o.order_type, x.trail) {
        (OrderType::TrailingStop, Some(Trail::Amount(a))) => (Some(f(a.value())?), None),
        (OrderType::TrailingStop, Some(Trail::Percent(p))) => (None, Some(f(p)?)),
        (OrderType::TrailingStop, None) => return Err(ConnectorError::invalid("a trailing stop needs its distance")),
        _ => (price(o.stop)?, None),
    };
    let display_size = match x.display_qty {
        None => None,
        Some(d) => {
            let v = d.value();
            if !v.fract().is_zero() {
                return Err(ConnectorError::invalid("Interactive Brokers shows an iceberg in whole units"));
            }
            Some(i32::try_from(v.trunc().mantissa()).map_err(|_| ConnectorError::invalid("the shown size is too large"))?)
        }
    };
    let trigger_method = match x.trigger {
        None => TriggerMethod::Default,
        Some(TriggerKind::Last) => TriggerMethod::Last,
        Some(TriggerKind::BidAsk) => TriggerMethod::BidAsk,
        Some(TriggerKind::Mid) => TriggerMethod::Midpoint,
        Some(TriggerKind::Mark) => return Err(ConnectorError::unsupported("Interactive Brokers triggers on no mark price")),
    };
    let (algo_strategy, algo_params) = match x.adaptive {
        None => (String::new(), Vec::new()),
        Some(u) => {
            let priority = match u {
                Urgency::Patient => "Patient",
                Urgency::Normal => "Normal",
                Urgency::Urgent => "Urgent",
            };
            ("Adaptive".to_string(), vec![TagValue { tag: "adaptivePriority".into(), value: priority.into() }])
        }
    };
    Ok(Order {
        action: match o.side {
            Side::Buy => Action::Buy,
            Side::Sell => Action::Sell,
        },
        order_type: order_type.into(),
        total_quantity: f(o.qty.value())?,
        limit_price: price(o.limit)?,
        aux_price,
        trailing_percent,
        tif,
        good_till_date,
        display_size,
        trigger_method,
        outside_rth: x.outside_rth,
        algo_strategy,
        algo_params,
        order_ref: o.client_order_id.clone(),
        account: o.account.clone().unwrap_or_default(),
        ..Order::default()
    })
}

/// Whether order `i` transmits on its own. An entry with exits waits for its last exit, which
/// releases the whole bracket at once: sent piecemeal, the entry could fill before its stop
/// exists.
pub fn transmits(orders: &[OrderRequest], i: usize) -> bool {
    let id = orders[i].client_order_id.as_str();
    if orders.iter().any(|o| o.parent.as_deref() == Some(id)) {
        return false;
    }
    match &orders[i].parent {
        Some(p) => !orders[i + 1..].iter().any(|o| o.parent.as_deref() == Some(p.as_str())),
        None => true,
    }
}

/// A price off the contract's increment, named with the two prices around it.
pub fn off_tick(price: Price, tick: Decimal, name: &str) -> Option<String> {
    if tick <= Decimal::ZERO {
        return None;
    }
    let p = price.value();
    if (p % tick).is_zero() {
        return None;
    }
    let below = (p / tick).floor() * tick;
    Some(format!("{} is not on {name}'s {} tick: use {} or {}", p.normalize(), tick.normalize(), below.normalize(), (below + tick).normalize()))
}

/// A size under the contract's minimum or off its size increment, named with the step and the
/// valid sizes around it (spec 4.2: refused, never adjusted). Valid sizes are the multiples of
/// the increment from the minimum up.
pub fn off_size(qty: Decimal, min: Option<Decimal>, step: Option<Decimal>, name: &str) -> Option<String> {
    let min = min.filter(|m| *m > Decimal::ZERO);
    let step = step.filter(|s| *s > Decimal::ZERO);
    let steps = step.map(|s| format!(" (sizes go in steps of {})", s.normalize())).unwrap_or_default();
    if let Some(m) = min.filter(|m| qty < *m) {
        let first = step.map_or(m, |s| (m / s).ceil() * s);
        return Some(format!("{} is under {name}'s minimum size of {}{steps}: the smallest order is {}", qty.normalize(), m.normalize(), first.normalize()));
    }
    let s = step?;
    if (qty % s).is_zero() {
        return None;
    }
    let below = (qty / s).floor() * s;
    let above = below + s;
    let sizes = if below > Decimal::ZERO && min.is_none_or(|m| below >= m) {
        format!("{} or {}", below.normalize(), above.normalize())
    } else {
        above.normalize().to_string()
    };
    Some(format!("{} is not a size {name} takes{steps}: use {sizes}", qty.normalize()))
}

/// The ticker a contract is charted under, for messages.
pub(super) fn name_of(c: &Contract) -> String {
    if c.local_symbol.is_empty() { c.symbol.0.clone() } else { c.local_symbol.clone() }
}

/// What IB's definition of a contract says an order on it must respect: the price increment
/// (`minTick`, or the market rule's ladder), the smallest size (`minSize`) and the size
/// increment (`sizeIncrement`). A field IB leaves unset (zero, or its `f64::MAX` sentinel) is
/// not checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    /// `minTick`: the smallest tick of the contract, whatever its price.
    pub tick: Option<Decimal>,
    pub min_size: Option<Decimal>,
    pub size_step: Option<Decimal>,
    /// The market rule of the contract's exchange (spec 4.2): `(from_price, tick)`, sorted by
    /// price. Empty when IB answered none: `tick` is checked instead.
    pub ladder: Vec<(Decimal, Decimal)>,
}

impl Rules {
    /// The rules one contract's details state (the ladder comes from its market rule, read
    /// apart: [`ladder_of`]).
    pub fn from_details(d: &ContractDetails) -> Result<Rules, ConnectorError> {
        let set = |v: f64| (v.is_finite() && v > 0.0 && v < 1e300).then(|| super::mapper::dec(v)).transpose();
        Ok(Rules { tick: set(d.min_tick)?, min_size: set(d.min_size)?, size_step: set(d.size_increment)?, ladder: Vec::new() })
    }

    /// The band of the ladder a price falls in, as `(from_price, tick)`. A negative price (a
    /// spread) takes the band of its size.
    fn band(&self, p: Decimal) -> Option<(Decimal, Decimal)> {
        self.ladder.iter().rev().find(|(from, _)| *from <= p.abs()).copied()
    }

    /// The tick a price has to sit on: its band's, else `minTick`.
    pub fn tick_at(&self, p: Decimal) -> Option<Decimal> {
        self.band(p).map(|(_, t)| t).or(self.tick)
    }

    /// A price off the tick of its band, named with the two valid prices around it, which may
    /// sit in the next band down or up. With no ladder, [`off_tick`] on `minTick`.
    pub fn off_tick(&self, price: Price, name: &str) -> Option<String> {
        let p = price.value();
        let Some((from, t)) = self.band(p).filter(|_| self.ladder.len() > 1 && p >= Decimal::ZERO) else {
            return self.tick_at(p).and_then(|t| off_tick(price, t, name));
        };
        if (p % t).is_zero() {
            return None;
        }
        let mut below = (p / t).floor() * t;
        if below < from {
            // No price of this band under it: the highest of the band below.
            if let Some((_, t0)) = self.ladder.iter().rev().find(|(f, _)| *f < from) {
                below = ((from / t0).ceil() - Decimal::ONE) * t0;
            }
        }
        let mut above = (p / t).ceil() * t;
        if let Some((next, t2)) = self.ladder.iter().find(|(f, _)| *f > from).filter(|(f, _)| above >= *f) {
            // No price of this band over it: the lowest of the band above.
            above = (next / t2).ceil() * t2;
        }
        Some(format!(
            "{} is not on {name}'s {} tick for prices from {}: use {} or {}",
            p.normalize(),
            t.normalize(),
            from.normalize(),
            below.normalize(),
            above.normalize()
        ))
    }

    /// The first price or size of these orders the contract does not take, said as IB's rule
    /// with the valid values around it.
    pub fn refusal(&self, orders: &[OrderRequest], name: &str) -> Option<String> {
        orders.iter().find_map(|o| {
            [o.limit, o.stop].into_iter().flatten().find_map(|p| self.off_tick(p, name)).or_else(|| off_size(o.qty.value(), self.min_size, self.size_step, name))
        })
    }
}

/// IB's ladder for one market rule, as `(from_price, tick)` sorted by price; increments IB
/// leaves unset are dropped.
pub fn ladder_of(rule: &MarketRule) -> Result<Vec<(Decimal, Decimal)>, ConnectorError> {
    let mut out = Vec::new();
    for b in &rule.price_increments {
        if b.increment.is_finite() && b.increment > 0.0 && b.low_edge.is_finite() && b.low_edge >= 0.0 && b.low_edge < 1e300 {
            out.push((super::mapper::dec(b.low_edge)?, super::mapper::dec(b.increment)?));
        }
    }
    out.sort_by_key(|(from, _)| *from);
    out.dedup_by_key(|(from, _)| *from);
    Ok(out)
}

/// The market rule of the exchange an order on `c` is routed to: IB lists the rule ids in the
/// order of `validExchanges`. An exchange not listed takes the one rule all exchanges share,
/// else none (`minTick` is checked).
pub fn rule_id(d: &ContractDetails, exchange: &str) -> Option<i32> {
    let ids: Vec<i32> = d.market_rule_ids.iter().map(|s| s.trim().parse::<i32>().ok()).collect::<Option<_>>()?;
    if let Some(i) = d.valid_exchanges.iter().position(|e| e.trim().eq_ignore_ascii_case(exchange.trim())) {
        if ids.len() == d.valid_exchanges.len() {
            return ids.get(i).copied();
        }
    }
    let first = *ids.first()?;
    ids.iter().all(|i| *i == first).then_some(first)
}

/// The contract's rules ([`Rules`]), from IB's own definition of it and the market rule of its
/// exchange: read once a day per contract, and again after IB refused a price off its tick
/// (110).
pub async fn rules_of(s: &Session, c: &Contract) -> Result<Rules, ConnectorError> {
    let key = (s.addr().to_string(), c.contract_id);
    if c.contract_id != 0 {
        if let Some((at, r)) = RULES.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            if at.elapsed() < RULES_FOR {
                return Ok(r.clone());
            }
        }
    }
    let cc = c.clone();
    let details = s
        .ask("contract details", move |client| {
            let cc = cc.clone();
            async move { client.contract_details(&cc).await }
        })
        .await?;
    let mut rules = details.first().map(Rules::from_details).transpose()?.unwrap_or_default();
    let exchange = if c.exchange.0.is_empty() { details.first().map(|d| d.contract.exchange.0.clone()).unwrap_or_default() } else { c.exchange.0.clone() };
    if let Some(id) = details.first().and_then(|d| rule_id(d, &exchange)) {
        match market_rule(s, id).await {
            Ok(ladder) => rules.ladder = ladder,
            Err(e) => tracing::warn!("ibkr: market rule {id} of {} at {}: {e}; prices checked on minTick", name_of(c), s.addr()),
        }
    }
    if c.contract_id != 0 {
        RULES.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), rules.clone()));
    }
    Ok(rules)
}

/// One market rule's ladder ([`ladder_of`]). IB answers market rules on one shared channel
/// that names the rule, so they are asked one at a time and the answer's id is checked. Not
/// through [`Session::ask`]: a rule IB keeps silent about is no reason to drop the connection.
async fn market_rule(s: &Session, id: i32) -> Result<Vec<(Decimal, Decimal)>, ConnectorError> {
    static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _turn = ONE_AT_A_TIME.lock().await;
    let (client, _) = s.client().await?;
    let rule = tokio::time::timeout(ORDER_WAIT, client.market_rule(id))
        .await
        .map_err(|_| ConnectorError::new(ErrorKind::Transient, "no answer in time"))?
        .map_err(|e| ConnectorError::new(ErrorKind::Transient, e.to_string()))?;
    if rule.market_rule_id != id {
        return Err(ConnectorError::new(ErrorKind::Decode, format!("IB answered market rule {} instead", rule.market_rule_id)));
    }
    ladder_of(&rule)
}

/// An order's prices on the contract's tick ladder and its size on the size grid, as a place
/// checks them, refused with the valid values around the one given.
pub fn on_grid(rules: &Rules, o: &OrderRequest, name: &str) -> Result<(), ConnectorError> {
    let price = [o.limit, o.stop].into_iter().flatten().find_map(|p| rules.off_tick(p, name));
    match price.or_else(|| off_size(o.qty.value(), rules.min_size, rules.size_step, name)) {
        Some(msg) => Err(ConnectorError::invalid(msg)),
        None => Ok(()),
    }
}

/// Forget a contract's rules, so the next order reads them again.
fn forget_rules(s: &Session, c: &Contract) {
    RULES.lock().unwrap_or_else(|e| e.into_inner()).remove(&(s.addr().to_string(), c.contract_id));
}

/// The setting the core adds to the one account a maintainer runs the live canary on (spec 9,
/// E3 to E4). Never a user setting: the account form only stores the fields a broker declares.
pub const LIVE_CANARY: &str = "live_canary";

/// Below E4, only paper accounts take orders (spec 9). IB numbers its paper accounts `D…`
/// (`DU`, `DF`); a gateway signed in to any other account is refused before anything is sent.
pub async fn check_execution(s: &Session, caps: &Capabilities, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
    // 1100: IB lost its servers. Nothing new goes out until it says the link is back (spec 7.3).
    if let Some(why) = s.link().borrow().lost.clone() {
        return Err(ConnectorError::new(ErrorKind::Transient, format!("{why}. No order is sent meanwhile")).with_code("1100"));
    }
    if caps.orders.level >= ExecLevel::E4 || settings.get(LIVE_CANARY).is_some_and(|v| v == "1") {
        return Ok(());
    }
    let accounts = accounts(s).await?;
    let live: Vec<&String> = accounts.iter().filter(|a| !paper(a)).collect();
    if accounts.is_empty() || !live.is_empty() {
        return Err(ConnectorError::new(
            ErrorKind::Permission,
            format!(
                "{} at {} is signed in to a live account ({}). Orders go to paper accounts only until its order path is rated E4 (it is {:?}). Sign the gateway in to paper trading.",
                caps.label,
                s.addr(),
                live.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(", "),
                caps.orders.level
            ),
        ));
    }
    Ok(())
}

/// The accounts the gateway is signed in to.
pub async fn accounts(s: &Session) -> Result<Vec<String>, ConnectorError> {
    let (client, _) = s.client().await?;
    tokio::time::timeout(ORDER_WAIT, client.managed_accounts())
        .await
        .map_err(|_| ConnectorError::new(ErrorKind::Transient, "Interactive Brokers did not name its accounts in time"))?
        .map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("Interactive Brokers accounts: {e}")))
}

/// IB numbers its paper accounts `D…` (`DU`, `DF`).
pub fn paper(account: &str) -> bool {
    account.trim().starts_with('D')
}

/// Whether an order has to pass a dry run first (spec 9, E4): on a gateway signed in to a live
/// account (or naming none), until one passed on this connection. Paper accounts skip it.
pub fn dry_run_due(accounts: &[String], passed: bool) -> bool {
    !passed && (accounts.is_empty() || accounts.iter().any(|a| !paper(a)))
}

/// Refuse a client order id this gateway was already sent (spec 2.1), and note the ones about
/// to go out. The first order to a gateway seeds the list with the working orders' ids.
async fn claim(s: &Session, orders: &[OrderRequest]) -> Result<(), ConnectorError> {
    let seeded = SENT.lock().unwrap_or_else(|e| e.into_inner()).contains_key(s.addr());
    if !seeded {
        let working: HashSet<String> = open_orders(s).await?.into_iter().map(|(_, o, _)| o.order_ref).filter(|r| !r.is_empty()).collect();
        SENT.lock().unwrap_or_else(|e| e.into_inner()).entry(s.addr().to_string()).or_default().extend(working);
    }
    let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
    let known = sent.entry(s.addr().to_string()).or_default();
    let mut ticket = HashSet::new();
    for o in orders {
        let id = o.client_order_id.as_str();
        if known.contains(id) || !ticket.insert(id) {
            return Err(refused(format!("client order id {id} was already sent to Interactive Brokers: a new order needs a new id, so nothing is placed twice")));
        }
    }
    known.extend(ticket.into_iter().map(String::from));
    Ok(())
}

/// Wait for IB's word on one placed order.
async fn accepted(sub: &mut Subscription<PlaceOrder>, what: &str) -> Result<Mapped<OrderStatus>, Outcome> {
    let mut said = String::new();
    while let Some(item) = sub.next().await {
        match item {
            Ok(PlaceOrder::OpenOrder(d)) if d.order_state.status == "Inactive" => return turned_down(sub, what).await.map_err(Outcome::Refused),
            Ok(PlaceOrder::OpenOrder(d)) => return Ok(status_of(&d.order_state.status)),
            Ok(PlaceOrder::OrderStatus(st)) if dead(&st) => {
                return Err(Outcome::Refused(refused(format!("Interactive Brokers refused {what}{}", if said.is_empty() { String::new() } else { format!(": {said}") }))))
            }
            Ok(PlaceOrder::OrderStatus(st)) => return Ok(status_of(&st.status)),
            Ok(PlaceOrder::ExecutionData(_)) => return Ok(Mapped::Known(OrderStatus::PartiallyFilled)),
            Ok(PlaceOrder::Message(n)) if benign(n.code) => {}
            Ok(PlaceOrder::Message(n)) => return Err(order_error(what, n.code, &n.message)),
            Ok(PlaceOrder::CommissionReport(_)) => {}
            Err(ibapi::Error::Message(code, msg)) if !benign(code) => return Err(order_error(what, code, &msg)),
            Err(ibapi::Error::Message(_, msg)) => said = msg,
            // The socket went with the order on it: sent or not, IB alone knows.
            Err(e) => return Err(Outcome::Lost(format!("{what}: {e}"))),
        }
    }
    Err(Outcome::Lost(format!("{what}: the connection to Interactive Brokers closed before it answered")))
}

/// The word on an order whose answer was lost, once IB was asked for it by client id
/// ([`lookup`]): listed and working (or filled) is accepted; listed and dead (cancelled,
/// rejected) is IB's refusal; not listed, or IB could not be asked, stays unknown (`Lost`), for
/// the caller to look up again later (spec 6.3). Never a refusal on silence alone: an order
/// IB has not listed yet may still be on its way.
pub fn after_silence(what: &str, why: &str, looked: Result<Option<Seen>, ConnectorError>) -> Result<Mapped<OrderStatus>, Outcome> {
    match looked {
        Ok(Some(seen)) if seen.status.is_final() && seen.status != OrderStatus::Filled => {
            Err(Outcome::Refused(refused(format!("Interactive Brokers refused {what}: it lists the order {}", serde_label(seen.status)))))
        }
        Ok(Some(seen)) => Ok(Mapped::Known(seen.status)),
        Ok(None) => Err(Outcome::Lost(format!("{why}, and Interactive Brokers does not list {what} yet"))),
        Err(e) => Err(Outcome::Lost(format!("{why}, and Interactive Brokers could not be asked about {what}: {}", e.message))),
    }
}

/// A status as the book writes it (`cancelled`, `rejected`).
fn serde_label(s: OrderStatus) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| format!("{s:?}").to_lowercase())
}

/// IB answered `Inactive`: the order is not working, and IB's reason (insufficient margin, a
/// permission) comes next on this stream. Said as a refusal with that reason; with no reason
/// in time, the status stands as IB gave it.
async fn turned_down(sub: &mut Subscription<PlaceOrder>, what: &str) -> Result<Mapped<OrderStatus>, ConnectorError> {
    let reason = async {
        while let Some(item) = sub.next().await {
            match item {
                Ok(PlaceOrder::Message(n)) if !benign(n.code) => return Some(format!("{} ({})", n.message, n.code)),
                Err(ibapi::Error::Message(code, msg)) if !benign(code) => return Some(format!("{msg} ({code})")),
                _ => {}
            }
        }
        None
    };
    match tokio::time::timeout(REASON_WAIT, reason).await {
        Ok(Some(why)) => Err(refused(format!("Interactive Brokers refused {what}: {}", why.replace("<br>", "")))),
        _ => Ok(status_of("Inactive")),
    }
}

/// IB's open orders, as (contract, order, status).
async fn open_orders(s: &Session) -> Result<Vec<(Contract, Order, String)>, ConnectorError> {
    s.ask("open orders", |client| async move {
        let mut sub = client.all_open_orders().await?;
        let mut out = Vec::new();
        while let Some(item) = sub.next().await {
            if let Orders::OrderData(d) = item? {
                out.push((d.contract, d.order, d.order_state.status));
            }
        }
        Ok(out)
    })
    .await
}

/// One order as IB lists it: working (`reqAllOpenOrders`) or done today
/// (`reqCompletedOrders`, which names no order id).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listed {
    pub order_id: Option<i32>,
    pub order_ref: String,
    pub status: String,
    /// What has filled, when IB said it.
    pub filled: Option<f64>,
}

/// What IB says of an order found by its client id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// IB's order id; `None` for an order IB lists only among the day's completed ones,
    /// without a fill to name it.
    pub order_id: Option<i32>,
    pub status: OrderStatus,
    pub filled: Decimal,
}

/// The executions that stand, in the order IB sent them: a repeat of an execution id is
/// dropped, and a correction (IB's convention: the same id but for the digits after its last
/// period, `.01` corrected by `.02`) replaces the fill it corrects (spec 2.3).
pub fn standing<T>(execs: impl IntoIterator<Item = T>, id: impl Fn(&T) -> &str) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for e in execs {
        let (base, rev) = id(&e).rsplit_once('.').map_or((id(&e).to_string(), String::new()), |(b, r)| (b.to_string(), r.to_string()));
        match out.iter().position(|x| id(x).rsplit_once('.').map_or(id(x), |(b, _)| b) == base) {
            Some(i) => {
                let held = id(&out[i]).rsplit_once('.').map(|(_, r)| r.to_string()).unwrap_or_default();
                // The same or an older revision: a repeat, dropped.
                if (rev.len(), rev.as_str()) > (held.len(), held.as_str()) {
                    out[i] = e;
                }
            }
            None => out.push(e),
        }
    }
    out
}

/// Settle a client id against what IB lists (spec 6.3): working orders first, then the day's
/// completed ones; the fills under that `orderRef` count, each once. Not listed anywhere and
/// no fill: IB has never seen it (`None`). Fills with no order to say its state, or a state no
/// table knows: an error, the order stays unknown.
pub fn settle(key: &str, open: &[Listed], done: &[Listed], execs: &[Execution]) -> Result<Option<Seen>, ConnectorError> {
    let mine = standing(execs.iter().filter(|e| e.order_reference == key), |e| e.execution_id.as_str());
    let mut fills = Decimal::ZERO;
    for e in &mine {
        fills += super::mapper::dec(e.shares)?;
    }
    let exec_id = mine.first().map(|e| e.order_id).filter(|id| *id != 0);
    let Some(hit) = open.iter().find(|l| l.order_ref == key).or_else(|| done.iter().find(|l| l.order_ref == key)) else {
        return match exec_id {
            Some(id) => Err(ConnectorError::new(ErrorKind::Transient, format!("Interactive Brokers lists fills of order {id} ({key}) but not its state yet"))),
            None => Ok(None),
        };
    };
    let status = match status_of(&hit.status) {
        Mapped::Known(st) => st,
        Mapped::Unknown(raw) => return Err(ConnectorError::new(ErrorKind::Unknown, format!("Interactive Brokers lists order {key} as {raw:?}, a state OpenTraderWorld does not read"))),
    };
    // What IB counts and what the fills add up to: the larger, a filled quantity never shrinks.
    let filled = hit.filled.filter(|f| f.is_finite() && *f > 0.0 && *f < 1e300).map(super::mapper::dec).transpose()?.unwrap_or(Decimal::ZERO).max(fills);
    let status = if status == OrderStatus::New && filled > Decimal::ZERO { OrderStatus::PartiallyFilled } else { status };
    Ok(Some(Seen { order_id: hit.order_id.or(exec_id), status, filled }))
}

/// Find the order sent under client id `key` (its `orderRef`) among everything IB lists today:
/// the working orders of every API client and of TWS itself, the completed ones, and the
/// fills. `None` only when IB answered all three and named it nowhere; an error when it could
/// not be asked.
pub async fn lookup(s: &Session, key: &str) -> Result<Option<Seen>, ConnectorError> {
    let (open, done, execs) = s
        .ask("order lookup", |client| async move {
            let read = |mut sub: Subscription<Orders>| async move {
                let mut out: Vec<Listed> = Vec::new();
                let mut filled: HashMap<i32, f64> = HashMap::new();
                while let Some(item) = sub.next().await {
                    match item? {
                        Orders::OrderData(d) => out.push(Listed {
                            order_id: (d.order_id != 0).then_some(d.order_id),
                            order_ref: d.order.order_ref,
                            status: d.order_state.status,
                            filled: Some(d.order.filled_quantity),
                        }),
                        Orders::OrderStatus(st) => {
                            filled.insert(st.order_id, st.filled);
                        }
                        Orders::Notice(_) => {}
                    }
                }
                for l in &mut out {
                    if let Some(f) = l.order_id.and_then(|id| filled.get(&id)) {
                        l.filled = Some(*f);
                    }
                }
                Ok::<_, ibapi::Error>(out)
            };
            let open = read(client.all_open_orders().await?).await?;
            let done = read(client.completed_orders(false).await?).await?;
            let mut sub = client.executions(ExecutionFilter::default()).await?;
            let mut execs = Vec::new();
            while let Some(item) = sub.next().await {
                if let Executions::ExecutionData(d) = item? {
                    execs.push(d.execution);
                }
            }
            Ok((open, done, execs))
        })
        .await?;
    settle(key, &open, &done, &execs)
}

/// Send orders on one contract (already checked against the capabilities and its [`Rules`])
/// and report each once IB accepted it. One refused after others went out cancels them all:
/// half a bracket is a risk nobody chose. An answer lost (no word within [`ORDER_WAIT`], the
/// socket gone, a duplicate id) is looked up by client id; still unsettled, the ticket is
/// [`PlaceError::Unknown`], naming every leg that went out. A lone order is then left as IB
/// has it; the legs of a ticket are cancelled, as for a refusal, and stay unknown until a
/// lookup reads how they ended.
pub async fn place(s: &Session, contract: &Contract, orders: &[OrderRequest], rules: &Rules) -> Result<Vec<OrderAck>, PlaceError> {
    let ib: Vec<Order> = orders.iter().map(ib_order).collect::<Result<_, _>>()?;
    if let Some(msg) = rules.refusal(orders, &name_of(contract)) {
        return Err(ConnectorError::invalid(msg).into());
    }
    claim(s, orders).await?;
    let (client, _) = s.client().await?;
    let ids: Vec<i32> = orders.iter().map(|_| client.next_order_id()).collect();
    let id_of = |cid: &str| orders.iter().position(|o| o.client_order_id == cid).map(|i| ids[i]).unwrap_or(0);
    let mut sent = Vec::new();
    // Legs that went out (or may have, on a socket that died while sending).
    let mut out: Vec<usize> = Vec::new();
    let mut failure: Option<Outcome> = None;
    for (i, (o, mut order)) in orders.iter().zip(ib).enumerate() {
        order.order_id = ids[i];
        order.transmit = transmits(orders, i);
        match &o.parent {
            // IB links a bracket's exits itself: the first to fill cancels the other.
            Some(p) => order.parent_id = id_of(p),
            None => {
                if let Some(g) = o.oca_group.as_deref().filter(|g| !g.is_empty()) {
                    order.oca_group = format!("OTW-{g}");
                    order.oca_type = OcaType::CancelWithBlock;
                }
            }
        }
        match client.place_order(ids[i], contract, &order).await {
            Ok(sub) => {
                out.push(i);
                sent.push((i, sub));
            }
            // A socket that died while writing may have carried the order.
            Err(e) if super::session::dropped(&e) => {
                out.push(i);
                failure = Some(Outcome::Lost(format!("sending order {}: {e}", i + 1)));
                break;
            }
            Err(e) => {
                failure = Some(Outcome::Refused(ConnectorError::new(ErrorKind::Transient, format!("sending order {}: {e}", i + 1))));
                break;
            }
        }
    }
    let mut acks = Vec::new();
    if failure.is_none() {
        for (i, mut sub) in sent {
            let o = &orders[i];
            let what = format!("the {:?} {:?} order", o.side, o.order_type).to_lowercase();
            let why = match tokio::time::timeout(ORDER_WAIT, accepted(&mut sub, &what)).await {
                Ok(Ok(status)) => Ok(status),
                Ok(Err(Outcome::Lost(why))) => Err(why),
                Ok(Err(refusal)) => {
                    failure = Some(refusal);
                    break;
                }
                Err(_) => Err(format!("Interactive Brokers did not confirm {what} within {}s", ORDER_WAIT.as_secs())),
            };
            // No answer is not "not placed": ask IB by client id before saying anything.
            let answer = match why {
                Ok(status) => Ok(status),
                Err(why) => after_silence(&what, &why, lookup(s, &o.client_order_id).await),
            };
            match answer {
                Ok(status) => acks.push(OrderAck { client_order_id: o.client_order_id.clone(), order_id: ids[i].to_string(), status }),
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
    }
    match failure {
        None => {
            tracing::info!("ibkr: placed {} order(s) on {} at {}", acks.len(), name_of(contract), s.addr());
            Ok(acks)
        }
        Some(Outcome::Refused(e)) => {
            for id in &ids {
                let _ = client.cancel_order(*id, "").await;
            }
            if e.provider_code.as_deref() == Some("110") {
                forget_rules(s, contract);
            }
            Err(e.into())
        }
        Some(Outcome::Lost(why)) => {
            if orders.len() > 1 {
                for i in &out {
                    let _ = client.cancel_order(ids[*i], "").await;
                }
            }
            let keys: Vec<String> = out.iter().map(|i| orders[*i].client_order_id.clone()).collect();
            tracing::warn!("ibkr: outcome unknown for {keys:?} on {} at {}: {why}", name_of(contract), s.addr());
            let then = if orders.len() > 1 { "Its legs were cancelled; how each ended" } else { "Whether it works" };
            Err(PlaceError::Unknown {
                message: format!("{why}. {then} is read from Interactive Brokers by client id ({}): do not send it again", keys.join(", ")),
                keys,
                unmapped: why.contains(UNMAPPED),
            })
        }
    }
}

fn order_id(id: &str) -> Result<i32, ConnectorError> {
    id.trim().parse::<i32>().map_err(|_| ConnectorError::invalid(format!("{id:?} is not an Interactive Brokers order id")))
}

/// Change a working order in place (prices, quantity, time in force), and wait for IB to echo
/// it: its first answer is often the order as it stood, and a refused change arrives after it.
pub async fn amend(s: &Session, id: &str, change: OrderAmend) -> Result<(), ConnectorError> {
    let n = order_id(id)?;
    let (contract, mut order, _) = open_orders(s)
        .await?
        .into_iter()
        .find(|(_, o, _)| o.order_id == n)
        .ok_or_else(|| ConnectorError::not_found(format!("order {n} is not working any more: it was filled or cancelled")))?;
    let limit = change.limit.map(|p| f(p.value())).transpose()?;
    let stop = change.stop.map(|p| f(p.value())).transpose()?;
    if limit.is_some() && order.limit_price.is_none() {
        return Err(ConnectorError::invalid(format!("order {n} carries no limit price to move")));
    }
    if stop.is_some() && order.aux_price.is_none() {
        return Err(ConnectorError::invalid(format!("order {n} carries no stop price to move")));
    }
    let rules = rules_of(s, &contract).await?;
    let name = name_of(&contract);
    let price = |p: Price| rules.off_tick(p, &name);
    let size = |q: otw_connector_model::Qty| off_size(q.value(), rules.min_size, rules.size_step, &name);
    if let Some(msg) = [change.limit, change.stop].into_iter().flatten().find_map(price).or_else(|| change.qty.and_then(size)) {
        return Err(ConnectorError::invalid(msg));
    }
    if limit.is_some() {
        order.limit_price = limit;
    }
    if stop.is_some() {
        order.aux_price = stop;
    }
    if let Some(q) = change.qty {
        order.total_quantity = f(q.value())?;
    }
    if let Some(t) = change.tif {
        (order.tif, order.good_till_date) = tif_of(t)?;
    }
    order.transmit = true;
    let (client, _) = s.client().await?;
    let mut sub = client
        .place_order(n, &contract, &order)
        .await
        .map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("moving order {n}: {e}")))?;
    // Compared as decimals: IB echoes the double it was sent, read back exactly.
    let same = |echo: Option<f64>, want: Option<Price>| want.is_none_or(|w| echo.and_then(|v| super::mapper::dec(v).ok()) == Some(w.value()));
    let wait = async {
        while let Some(item) = sub.next().await {
            match item {
                Ok(PlaceOrder::OpenOrder(d))
                    if same(d.order.limit_price, change.limit)
                        && same(d.order.aux_price, change.stop)
                        && change.qty.is_none_or(|q| super::mapper::dec(d.order.total_quantity).ok() == Some(q.value())) =>
                {
                    return Ok(())
                }
                Ok(PlaceOrder::OrderStatus(st)) if dead(&st) => return Err(ConnectorError::not_found(format!("order {n} is no longer working"))),
                Ok(PlaceOrder::Message(m)) if !benign(m.code) => return Err(refused(format!("Interactive Brokers refused to move order {n}: {} ({})", m.message, m.code))),
                Err(ibapi::Error::Message(code, msg)) if !benign(code) => return Err(refused(format!("Interactive Brokers refused to move order {n}: {msg} ({code})"))),
                Err(e) if super::session::dropped(&e) => return Err(ConnectorError::new(ErrorKind::Transient, format!("moving order {n}: {e}"))),
                _ => {}
            }
        }
        Err(ConnectorError::new(ErrorKind::Transient, format!("moving order {n}: the connection to Interactive Brokers closed before it answered")))
    };
    tokio::time::timeout(ORDER_WAIT, wait)
        .await
        .map_err(|_| ConnectorError::new(ErrorKind::Transient, format!("Interactive Brokers did not confirm the move of order {n} within {}s. Check TWS / IB Gateway", ORDER_WAIT.as_secs())))?
}

/// IB's answer to cancelling an order that already stopped working (`10148`): the state it
/// names is the outcome. Final, filled included, is no error to the user: the cancel raced
/// the order's end, and the book shows how it ended (spec 6.2).
fn already_final(code: i32, msg: &str) -> Option<Result<(), ConnectorError>> {
    if code != 10148 {
        return None;
    }
    let state = msg.rsplit_once("state:").map(|(_, s)| s.trim().trim_end_matches('.').trim())?;
    match status_of(state) {
        Mapped::Known(s) if s.is_final() => Some(Ok(())),
        _ => None,
    }
}

/// IB says an order cannot be cancelled in its state (161, or a 10148 naming no final state):
/// read it from IB's list instead (spec 6.2). No longer working, it ended on its own (filled,
/// expired): done. Still working, the refusal stands.
fn not_cancellable(n: i32, code: i32, msg: &str, working: Option<&str>) -> Result<(), ConnectorError> {
    match working.map(status_of) {
        None => Ok(()),
        Some(Mapped::Known(st)) if st.is_final() => Ok(()),
        Some(_) => Err(refused(format!("Interactive Brokers refused to cancel order {n}: {msg} ({code})"))),
    }
}

/// Cancel a working order (IB cancels the exits attached to it). An order that ended before
/// the cancel landed (filled, expired) answers done: its state is on the book.
pub async fn cancel(s: &Session, id: &str) -> Result<(), ConnectorError> {
    let n = order_id(id)?;
    let (client, _) = s.client().await?;
    let mut sub = client
        .cancel_order(n, "")
        .await
        .map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("cancelling order {n}: {e}")))?;
    // `Err((code, msg))`: IB says not cancellable, to be read from its list.
    let wait = async {
        while let Some(item) = sub.next().await {
            match item {
                Ok(CancelOrder::OrderStatus(st)) if dead(&st) || st.status == "PendingCancel" => return Ok(Ok(())),
                Ok(CancelOrder::OrderStatus(st)) if st.status == "Filled" => {
                    tracing::info!("ibkr: order {n} filled before its cancel landed");
                    return Ok(Ok(()));
                }
                Ok(CancelOrder::OrderStatus(_)) => {}
                Ok(CancelOrder::Notice(m)) if m.code == ibapi::messages::ORDER_CANCELLED_CODE => return Ok(Ok(())),
                Ok(CancelOrder::Notice(m)) => match already_final(m.code, &m.message) {
                    Some(r) => return Ok(r),
                    None if m.code == 161 || m.code == 10148 => return Err((m.code, m.message)),
                    None if benign(m.code) => {}
                    None => return Ok(Err(refused(format!("Interactive Brokers refused to cancel order {n}: {} ({})", m.message, m.code)))),
                },
                Err(ibapi::Error::Message(code, _)) if code == ibapi::messages::ORDER_CANCELLED_CODE => return Ok(Ok(())),
                Err(ibapi::Error::Message(code, msg)) => match already_final(code, &msg) {
                    Some(r) => return Ok(r),
                    None if code == 161 || code == 10148 => return Err((code, msg)),
                    None => return Ok(Err(refused(format!("Interactive Brokers refused to cancel order {n}: {msg} ({code})")))),
                },
                Err(e) => return Ok(Err(ConnectorError::new(ErrorKind::Transient, format!("cancelling order {n}: {e}")))),
            }
        }
        Ok(Err(ConnectorError::new(ErrorKind::Transient, format!("cancelling order {n}: the connection to Interactive Brokers closed before it answered"))))
    };
    let answer = tokio::time::timeout(ORDER_WAIT, wait)
        .await
        .map_err(|_| ConnectorError::new(ErrorKind::Transient, format!("Interactive Brokers did not confirm the cancel of order {n} within {}s", ORDER_WAIT.as_secs())))?;
    match answer {
        Ok(r) => r,
        Err((code, msg)) => {
            let list = open_orders(s).await?;
            not_cancellable(n, code, &msg, list.iter().find(|(_, o, _)| o.order_id == n).map(|(_, _, st)| st.as_str()))
        }
    }
}

/// The kill switch: cancel every working order of the gateway's accounts (IB's global cancel,
/// which also reaches orders placed in TWS by hand), then wait until IB lists none still
/// working. Answers how many were working.
pub async fn cancel_all(s: &Session) -> Result<usize, ConnectorError> {
    let working = |list: &[(Contract, Order, String)]| {
        list.iter().filter(|(_, _, st)| !matches!(status_of(st), Mapped::Known(x) if x.is_final() || x == OrderStatus::PendingCancel)).count()
    };
    let before = working(&open_orders(s).await?);
    let (client, _) = s.client().await?;
    client.global_cancel().await.map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("cancelling every order: {e}")))?;
    let deadline = tokio::time::Instant::now() + ORDER_WAIT;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let left = working(&open_orders(s).await?);
        if left == 0 {
            tracing::warn!("ibkr: cancelled every order at {} ({before} working)", s.addr());
            return Ok(before);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ConnectorError::new(
                ErrorKind::Transient,
                format!("{left} order(s) still working {}s after cancelling everything. Check TWS / IB Gateway", ORDER_WAIT.as_secs()),
            ));
        }
    }
}

/// Close a position: a market order for what the account holds of this contract, on the
/// other side, on the contract IB itself reports the position under (its id), in the account
/// holding it. What is held is read again here, so a position that already moved is closed
/// for what it is now. Read from the account feed ([`super::events::held`]): a positions
/// request of its own would cancel the feed's when it ends.
pub async fn close(
    s: &Arc<Session>,
    caps: &Capabilities,
    settings: &HashMap<String, String>,
    contract_id: i32,
    account: &str,
    client_order_id: &str,
    now: otw_connector_model::Timestamp,
) -> Result<OrderAck, PlaceError> {
    check_execution(s, caps, settings).await?;
    let held = super::events::held(s)
        .await?
        .into_iter()
        .find(|p| p.contract.contract_id == contract_id && (account.is_empty() || p.account == account) && p.position != 0.0)
        .ok_or_else(|| ConnectorError::not_found(format!("no position in contract {contract_id} to close: it is already flat")))?;
    let probe = Contract { contract_id, exchange: "".into(), currency: "".into(), ..Default::default() };
    let details = s
        .ask("contract details", move |client| {
            let probe = probe.clone();
            async move { client.contract_details(&probe).await }
        })
        .await?;
    let contract = details
        .into_iter()
        .next()
        .map(|d| d.contract)
        .ok_or_else(|| ConnectorError::not_found(format!("Interactive Brokers does not know contract {contract_id}")))?;
    let contract = tradable(contract, &name_of(&held.contract))?;
    let qty = super::mapper::dec(held.position)?;
    let req = OrderRequest {
        client_order_id: client_order_id.to_string(),
        instrument: otw_connector_model::InstrumentId::new("IBKR", &contract_id.to_string()),
        side: if qty.is_sign_negative() { Side::Buy } else { Side::Sell },
        order_type: OrderType::Market,
        qty: otw_connector_model::Qty::new(qty.abs()),
        limit: None,
        stop: None,
        tif: TimeInForce::Day,
        parent: None,
        oca_group: None,
        account: Some(held.account.clone()).filter(|a| !a.is_empty()),
        opts: Default::default(),
    };
    otw_connector_model::check_orders(caps, std::slice::from_ref(&req), now)?;
    super::margin::dry_run(s, &contract, std::slice::from_ref(&req)).await?;
    place(s, &contract, std::slice::from_ref(&req), &Rules::default())
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ConnectorError::new(ErrorKind::Unknown, "Interactive Brokers accepted no order").into())
}

/// The contract an instrument names, ready for an order: resolved on this session, and never
/// the continuous futures series, which IB does not trade.
pub fn tradable(mut c: Contract, ticker: &str) -> Result<Contract, ConnectorError> {
    if c.security_type == SecurityType::ContinuousFuture {
        return Err(ConnectorError::invalid(format!(
            "{ticker} is the continuous futures series, which Interactive Brokers does not trade. Open the contract month (for example {ticker}Z6) and trade that chart"
        )));
    }
    // Set so a chart can read an expired contract's history; an order is for a live one.
    c.include_expired = false;
    Ok(c)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::{InstrumentId, OrderOpts, Qty, Timestamp};
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn order(id: &str, t: OrderType, parent: Option<&str>) -> OrderRequest {
        let (l, s) = t.prices();
        OrderRequest {
            client_order_id: id.into(),
            instrument: InstrumentId::new("CME", "MNQ"),
            side: Side::Buy,
            order_type: t,
            qty: Qty::new(d("2")),
            limit: l.then(|| Price::new(d("100.5"))),
            stop: s.then(|| Price::new(d("99"))),
            tif: TimeInForce::Day,
            parent: parent.map(Into::into),
            oca_group: None,
            account: None,
            opts: Default::default(),
        }
    }

    #[test]
    fn a_cancel_of_an_order_already_stopped_reads_its_final_state() {
        let msg = |st: &str| format!("OrderId 22 that needs to be cancelled cannot be cancelled, state: {st}.");
        assert!(matches!(already_final(10148, &msg("Cancelled")), Some(Ok(()))));
        assert!(matches!(already_final(10148, &msg("Inactive")), Some(Ok(()))));
        // T22: the cancel raced a fill. Filled is the outcome, not an error to the user.
        assert!(matches!(already_final(10148, &msg("Filled")), Some(Ok(()))));
        assert!(already_final(10148, &msg("Submitted")).is_none());
        assert!(already_final(10148, "no state named").is_none());
        assert!(already_final(201, &msg("Cancelled")).is_none());
    }

    #[test]
    fn an_order_ib_cannot_cancel_is_read_from_its_list() {
        let m = "Cancel attempted when order is not in a cancellable state";
        assert!(not_cancellable(22, 161, m, None).is_ok(), "no longer listed: it ended on its own");
        assert!(not_cancellable(22, 161, m, Some("Filled")).is_ok());
        let still = not_cancellable(22, 161, m, Some("PendingSubmit")).unwrap_err();
        assert!(still.message.contains("(161)"), "{}", still.message);
    }

    #[test]
    fn a_size_off_the_contract_s_grid_names_the_step_and_the_valid_sizes() {
        let one = Some(d("1"));
        // T11b: 0.5 MNQ, refused before it is sent, where IB answers 10318 after.
        assert_eq!(off_size(d("0.5"), one, one, "MNQZ6").unwrap(), "0.5 is under MNQZ6's minimum size of 1 (sizes go in steps of 1): the smallest order is 1");
        assert_eq!(off_size(d("2.5"), one, one, "MNQZ6").unwrap(), "2.5 is not a size MNQZ6 takes (sizes go in steps of 1): use 2 or 3");
        assert_eq!(off_size(d("2"), one, one, "MNQZ6"), None);
        // A minimum off the step: the first valid size is the step's multiple above it.
        assert_eq!(off_size(d("0.001"), Some(d("0.0025")), Some(d("0.001")), "X").unwrap(), "0.001 is under X's minimum size of 0.0025 (sizes go in steps of 0.001): the smallest order is 0.003");
        // Below the step with no minimum: only the size above is valid.
        assert_eq!(off_size(d("0.5"), None, one, "X").unwrap(), "0.5 is not a size X takes (sizes go in steps of 1): use 1");
        assert_eq!(off_size(d("3"), Some(d("100")), None, "X").unwrap(), "3 is under X's minimum size of 100: the smallest order is 100");
        assert_eq!(off_size(d("0.5"), None, None, "X"), None, "IB stated no rule: nothing checked");
        assert_eq!(off_size(d("0.5"), Some(Decimal::ZERO), Some(Decimal::ZERO), "X"), None);
    }

    #[test]
    fn the_rules_come_from_contract_details_and_refuse_before_sending() {
        let det = ContractDetails { min_tick: 0.25, min_size: 1.0, size_increment: 1.0, ..Default::default() };
        let r = Rules::from_details(&det).unwrap();
        assert_eq!(r, Rules { tick: Some(d("0.25")), min_size: Some(d("1")), size_step: Some(d("1")), ladder: vec![] });
        // IB's unset sentinels are no rule.
        let unset = ContractDetails { min_tick: 0.0, min_size: f64::MAX, size_increment: f64::NAN, ..Default::default() };
        assert_eq!(Rules::from_details(&unset).unwrap(), Rules::default());
        let mut half = order("h", OrderType::Limit, None);
        half.qty = Qty::new(d("0.5"));
        assert!(r.refusal(&[order("a", OrderType::Limit, None), half], "MNQZ6").is_some_and(|m| m.contains("minimum size of 1")));
        let mut off = order("o", OrderType::Limit, None);
        off.limit = Some(Price::new(d("100.3")));
        assert!(r.refusal(&[off], "MNQZ6").is_some_and(|m| m.contains("tick")));
        assert_eq!(r.refusal(&[order("a", OrderType::StopLimit, None)], "MNQZ6"), None);
    }

    fn two_bands() -> Rules {
        // 0.01 below 1, 0.05 from 1 (IB lists the ladder as low edge and increment).
        let rule = MarketRule {
            market_rule_id: 26,
            price_increments: vec![
                ibapi::contracts::PriceIncrement { low_edge: 1.0, increment: 0.05 },
                ibapi::contracts::PriceIncrement { low_edge: 0.0, increment: 0.01 },
                ibapi::contracts::PriceIncrement { low_edge: 5.0, increment: 0.0 },
            ],
        };
        Rules { tick: Some(d("0.01")), ladder: ladder_of(&rule).unwrap(), ..Default::default() }
    }

    #[test]
    fn a_market_rule_is_a_ladder_sorted_by_price() {
        let r = two_bands();
        assert_eq!(r.ladder, vec![(d("0"), d("0.01")), (d("1"), d("0.05"))], "sorted, the unset increment dropped");
        assert_eq!(r.tick_at(d("0.5")), Some(d("0.01")));
        assert_eq!(r.tick_at(d("1")), Some(d("0.05")), "the low edge belongs to its band");
        assert_eq!(r.tick_at(d("250")), Some(d("0.05")));
        assert_eq!(r.tick_at(d("-2")), Some(d("0.05")), "a negative price takes the band of its size");
        assert_eq!(Rules { tick: Some(d("0.25")), ..Default::default() }.tick_at(d("7")), Some(d("0.25")), "no rule answered: minTick");
    }

    #[test]
    fn a_price_is_checked_on_the_tick_of_its_band() {
        let r = two_bands();
        let p = |s: &str| Price::new(d(s));
        assert_eq!(r.off_tick(p("0.37"), "X"), None);
        assert_eq!(r.off_tick(p("1.15"), "X"), None);
        // On minTick, off the band's tick: refused before I/O, where minTick alone lets it go.
        assert_eq!(r.off_tick(p("1.02"), "X").unwrap(), "1.02 is not on X's 0.05 tick for prices from 1: use 1 or 1.05");
        assert_eq!(Rules { tick: Some(d("0.01")), ..Default::default() }.off_tick(p("1.02"), "X"), None);
        assert_eq!(r.off_tick(p("0.375"), "X").unwrap(), "0.375 is not on X's 0.01 tick for prices from 0: use 0.37 or 0.38");
        // Across the edge: the valid price above sits in the next band.
        assert_eq!(r.off_tick(p("0.995"), "X").unwrap(), "0.995 is not on X's 0.01 tick for prices from 0: use 0.99 or 1");
        let mut o = order("a", OrderType::Limit, None);
        o.limit = Some(p("2.33"));
        assert!(r.refusal(&[o], "X").is_some_and(|m| m.contains("use 2.3 or 2.35")));
    }

    #[test]
    fn a_band_edge_off_its_own_tick_names_the_prices_of_both_bands() {
        // 0.1 below 10.05, 0.25 from 10.05: the band above starts at 10.25, the one below ends
        // at 10.
        let r = Rules { tick: Some(d("0.1")), ladder: vec![(d("0"), d("0.1")), (d("10.05"), d("0.25"))], ..Default::default() };
        assert_eq!(r.off_tick(Price::new(d("10.1")), "X").unwrap(), "10.1 is not on X's 0.25 tick for prices from 10.05: use 10 or 10.25");
        assert_eq!(r.off_tick(Price::new(d("10.03")), "X").unwrap(), "10.03 is not on X's 0.1 tick for prices from 0: use 10 or 10.25");
    }

    #[test]
    fn the_market_rule_is_the_one_of_the_contract_s_exchange() {
        let det = |ex: &[&str], ids: &[&str]| ContractDetails {
            valid_exchanges: ex.iter().map(|s| s.to_string()).collect(),
            market_rule_ids: ids.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let d1 = det(&["SMART", "AMEX", "NYSE"], &["26", "26", "239"]);
        assert_eq!(rule_id(&d1, "SMART"), Some(26));
        assert_eq!(rule_id(&d1, "nyse"), Some(239));
        assert_eq!(rule_id(&d1, "ARCA"), None, "not listed and the rules differ: minTick");
        assert_eq!(rule_id(&det(&["CME"], &["67"]), "GLOBEX"), Some(67), "one rule for every exchange");
        assert_eq!(rule_id(&det(&[], &[]), "CME"), None);
        assert_eq!(rule_id(&det(&["CME"], &["x"]), "CME"), None);
    }

    #[test]
    fn ib_order_codes_map_to_the_spec_table() {
        let refused_as = |code: i32| match order_error("the buy limit order", code, "text") {
            Outcome::Refused(e) => Some((e.kind, e.provider_code.unwrap_or_default())),
            Outcome::Lost(_) => None,
        };
        // 103 duplicate id: unknown, never sent again.
        assert!(matches!(order_error("x", 103, "Duplicate order id"), Outcome::Lost(m) if m.contains("103")));
        // T30: IB's message pacing is a refusal to retry, with when.
        let paced = order_error("x", 100, "Max rate of messages per second has been exceeded");
        assert!(matches!(paced, Outcome::Refused(e) if e.kind == ErrorKind::RateLimited && e.retry_after_ms == Some(1000)));
        assert_eq!(refused_as(504), Some((ErrorKind::Transient, "504".into())));
        assert_eq!(refused_as(1100), Some((ErrorKind::Transient, "1100".into())));
        assert_eq!(refused_as(110), Some((ErrorKind::InvalidRequest, "110".into())));
        assert_eq!(refused_as(201), Some((ErrorKind::InvalidRequest, "201".into())));
        assert_eq!(refused_as(321), Some((ErrorKind::InvalidRequest, "321".into())));
        assert_eq!(refused_as(10318), Some((ErrorKind::InvalidRequest, "10318".into())));
        assert!(matches!(order_error("x", 201, "Order rejected<br>funds"), Outcome::Refused(e) if e.message.contains("Order rejectedfunds (201)")));
    }

    #[test]
    fn an_unmapped_order_code_is_an_unknown_outcome_naming_the_code() {
        // Spec 8: a code no table maps is never a refusal on a guess. The order is looked up
        // by client id, and the message says which code to map.
        for code in [135, 200, 383, 10147, 12345] {
            let out = order_error("the buy limit order", code, "Something<br>new");
            assert!(
                matches!(&out, Outcome::Lost(m) if m.contains(&format!("code {code} (Somethingnew)")) && m.contains("does not map")),
                "{code}: {out:?}"
            );
        }
        // A lost answer the lookup cannot settle stays unknown, carrying the code onward.
        let why = match order_error("the order", 12345, "new") {
            Outcome::Lost(m) => m,
            Outcome::Refused(e) => e.message,
        };
        assert!(matches!(after_silence("the order", &why, Ok(None)), Err(Outcome::Lost(m)) if m.contains("code 12345")));
    }

    fn listed(id: Option<i32>, key: &str, status: &str, filled: Option<f64>) -> Listed {
        Listed { order_id: id, order_ref: key.into(), status: status.into(), filled }
    }

    fn exec(id: &str, key: &str, order_id: i32, shares: f64) -> Execution {
        Execution { execution_id: id.into(), order_reference: key.into(), order_id, shares, ..Default::default() }
    }

    #[test]
    fn a_lost_answer_is_settled_by_client_id() {
        let open = [listed(Some(41), "other", "Submitted", None), listed(Some(42), "k1", "Submitted", Some(0.0))];
        // T19: IB has it working.
        assert_eq!(settle("k1", &open, &[], &[]).unwrap(), Some(Seen { order_id: Some(42), status: OrderStatus::New, filled: Decimal::ZERO }));
        // T20: listed nowhere, no fill: IB never saw it.
        assert_eq!(settle("k9", &open, &[], &[]).unwrap(), None);
        // Done today: the completed list names no id, the fill does.
        let done = [listed(None, "k2", "Filled", Some(2.0))];
        let fills = [exec("0001.aa.01.01", "k2", 43, 1.0), exec("0001.ab.01.01", "k2", 43, 1.0)];
        assert_eq!(settle("k2", &open, &done, &fills).unwrap(), Some(Seen { order_id: Some(43), status: OrderStatus::Filled, filled: d("2") }));
        // Working with a fill: partially filled, the fills counting when IB's count lags.
        let fill = [exec("0001.ac.01.01", "k1", 42, 1.0)];
        assert_eq!(settle("k1", &open, &[], &fill).unwrap().map(|s| (s.status, s.filled)), Some((OrderStatus::PartiallyFilled, d("1"))));
        // Fills but no state: unknown still, never a guess.
        assert!(settle("k3", &open, &[], &[exec("0001.ad.01.01", "k3", 44, 1.0)]).is_err());
        assert!(settle("k1", &[listed(Some(42), "k1", "Parked", None)], &[], &[]).is_err());
    }

    #[test]
    fn a_silence_is_unknown_until_ib_lists_the_order() {
        let seen = |st: OrderStatus| Ok(Some(Seen { order_id: Some(7), status: st, filled: Decimal::ZERO }));
        assert_eq!(after_silence("the order", "no answer", seen(OrderStatus::New)), Ok(Mapped::Known(OrderStatus::New)));
        assert_eq!(after_silence("the order", "no answer", seen(OrderStatus::Filled)), Ok(Mapped::Known(OrderStatus::Filled)));
        assert!(matches!(after_silence("the order", "no answer", seen(OrderStatus::Rejected)), Err(Outcome::Refused(e)) if e.message.contains("rejected")));
        // T20: not listed yet is unknown, not refused: the lookup settles it later.
        assert!(matches!(after_silence("the order", "no answer within 10s", Ok(None)), Err(Outcome::Lost(m)) if m.contains("does not list")));
        // T21: IB unreachable: unknown.
        let down = Err(ConnectorError::new(ErrorKind::Transient, "connection refused"));
        assert!(matches!(after_silence("the order", "socket closed", down), Err(Outcome::Lost(m)) if m.contains("connection refused")));
    }

    #[test]
    fn an_unknown_outcome_names_its_legs() {
        let e = PlaceError::Unknown { keys: vec!["k1".into(), "k2".into()], message: "no answer".into(), unmapped: false };
        assert_eq!(e.to_string(), "no answer");
        let r: PlaceError = ConnectorError::invalid("off tick").into();
        assert!(matches!(r, PlaceError::Refused(e) if e.kind == ErrorKind::InvalidRequest));
    }

    #[test]
    fn an_execution_counts_once_and_a_correction_replaces_its_fill() {
        // T24: the same execution twice (a replay, a reconnect).
        let twice = [exec("0001.aa.01.01", "k", 1, 1.0), exec("0001.aa.01.01", "k", 1, 1.0), exec("0001.ab.01.01", "k", 1, 2.0)];
        let kept = standing(twice.iter(), |e| e.execution_id.as_str());
        assert_eq!(kept.iter().map(|e| e.shares).collect::<Vec<_>>(), [1.0, 2.0]);
        // T25: `.02` corrects `.01`, in place; an older revision after it changes nothing.
        let corrected = [exec("0001.aa.01.01", "k", 1, 3.0), exec("0001.aa.01.02", "k", 1, 2.0), exec("0001.aa.01.01", "k", 1, 3.0)];
        let kept = standing(corrected.iter(), |e| e.execution_id.as_str());
        assert_eq!(kept.iter().map(|e| (e.execution_id.as_str(), e.shares)).collect::<Vec<_>>(), [("0001.aa.01.02", 2.0)]);
        assert_eq!(settle("k", &[], &[listed(None, "k", "Filled", None)], &corrected).unwrap().map(|s| s.filled), Some(d("2")));
    }

    #[test]
    fn an_order_becomes_the_ib_order_its_type_names() {
        let o = ib_order(&order("a", OrderType::StopLimit, None)).unwrap();
        assert_eq!((o.order_type.as_str(), o.limit_price, o.aux_price), ("STP LMT", Some(100.5), Some(99.0)));
        let o = ib_order(&order("a", OrderType::Stop, None)).unwrap();
        assert_eq!((o.order_type.as_str(), o.limit_price, o.aux_price), ("STP", None, Some(99.0)));
        let o = ib_order(&order("a", OrderType::Market, None)).unwrap();
        assert_eq!((o.order_type.as_str(), o.action, o.tif.clone(), o.total_quantity), ("MKT", Action::Buy, IbTif::Day, 2.0));
        // The client id rides as orderRef: what a lost answer is looked up by.
        assert_eq!(o.order_ref, "a");
        let mut g = order("a", OrderType::Limit, None);
        g.tif = TimeInForce::Gtd(Timestamp::from_secs(1_792_598_400).unwrap());
        let o = ib_order(&g).unwrap();
        assert_eq!((o.tif, o.good_till_date.as_str()), (IbTif::GoodTilDate, "20261021-16:00:00"));
    }

    #[test]
    fn trailing_if_touched_and_the_extras_become_ib_fields() {
        let mut t = order("t", OrderType::TrailingStop, None);
        t.opts.trail = Some(Trail::Amount(Price::new(d("2.5"))));
        let o = ib_order(&t).unwrap();
        assert_eq!((o.order_type.as_str(), o.aux_price, o.trailing_percent), ("TRAIL", Some(2.5), None));
        t.opts.trail = Some(Trail::Percent(d("1.5")));
        let o = ib_order(&t).unwrap();
        assert_eq!((o.aux_price, o.trailing_percent), (None, Some(1.5)));
        let o = ib_order(&order("m", OrderType::Mit, None)).unwrap();
        assert_eq!((o.order_type.as_str(), o.aux_price), ("MIT", Some(99.0)));
        let o = ib_order(&order("l", OrderType::Lit, None)).unwrap();
        assert_eq!((o.order_type.as_str(), o.limit_price, o.aux_price), ("LIT", Some(100.5), Some(99.0)));
        let mut x = order("x", OrderType::Limit, None);
        x.qty = Qty::new(d("10"));
        x.tif = TimeInForce::Fok;
        x.opts = OrderOpts { display_qty: Some(Qty::new(d("2"))), outside_rth: true, adaptive: Some(Urgency::Patient), ..Default::default() };
        let o = ib_order(&x).unwrap();
        assert_eq!((o.display_size, o.outside_rth, o.algo_strategy.as_str(), o.tif), (Some(2), true, "Adaptive", IbTif::FillOrKill));
        assert_eq!(o.algo_params.first().map(|t| (t.tag.as_str(), t.value.as_str())), Some(("adaptivePriority", "Patient")));
        let mut s = order("s", OrderType::Stop, None);
        s.opts.trigger = Some(TriggerKind::Mid);
        assert_eq!(ib_order(&s).unwrap().trigger_method, TriggerMethod::Midpoint);
        s.opts.trigger = Some(TriggerKind::Mark);
        assert!(ib_order(&s).is_err());
        x.opts.display_qty = Some(Qty::new(d("2.5")));
        assert!(ib_order(&x).is_err(), "an iceberg shows whole units");
    }

    #[test]
    fn a_bracket_goes_out_on_its_last_exit() {
        let b = [order("e", OrderType::Limit, None), order("tp", OrderType::Limit, Some("e")), order("sl", OrderType::Stop, Some("e"))];
        assert_eq!((0..3).map(|i| transmits(&b, i)).collect::<Vec<_>>(), [false, false, true]);
        assert!(transmits(&[order("e", OrderType::Limit, None)], 0));
        let oco = [order("a", OrderType::Limit, None), order("b", OrderType::Stop, None)];
        assert!(transmits(&oco, 0) && transmits(&oco, 1));
    }

    #[test]
    fn every_ib_status_has_a_canonical_word() {
        for (ib, want) in [
            ("ApiPending", OrderStatus::PendingNew),
            ("PendingSubmit", OrderStatus::PendingNew),
            ("PreSubmitted", OrderStatus::New),
            ("Submitted", OrderStatus::New),
            ("PendingCancel", OrderStatus::PendingCancel),
            ("ApiCancelled", OrderStatus::Cancelled),
            ("Cancelled", OrderStatus::Cancelled),
            ("Filled", OrderStatus::Filled),
            ("Inactive", OrderStatus::Rejected),
        ] {
            assert_eq!(status_of(ib), Mapped::Known(want), "{ib}");
        }
        assert_eq!(status_of("Parked"), Mapped::Unknown("Parked".into()));
    }

    #[test]
    fn a_price_off_the_tick_names_the_two_around_it() {
        let p = |s: &str| Price::new(d(s));
        assert_eq!(off_tick(p("29904.25"), d("0.25"), "MNQZ6"), None);
        assert_eq!(off_tick(p("1.2345"), d("0.00005"), "EUR"), None);
        assert_eq!(off_tick(p("29904.44"), d("0.25"), "MNQZ6").unwrap(), "29904.44 is not on MNQZ6's 0.25 tick: use 29904.25 or 29904.5");
        assert_eq!(off_tick(p("29904.44"), Decimal::ZERO, "X"), None);
    }

    #[test]
    fn a_live_account_s_first_order_is_dry_run_and_paper_skips_it() {
        let accts = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Spec 9 E4: live, first order of the connection: dry run; later ones go straight out.
        assert!(dry_run_due(&accts(&["U1234567"]), false));
        assert!(!dry_run_due(&accts(&["U1234567"]), true));
        // One live account among paper ones is live.
        assert!(dry_run_due(&accts(&["DU111", "U222"]), false));
        // Paper accounts skip it, first order or not.
        assert!(!dry_run_due(&accts(&["DU1234567", " DF77"]), false));
        assert!(!dry_run_due(&accts(&["DU1234567"]), true));
        // A gateway that names no account cannot be told paper: dry run.
        assert!(dry_run_due(&[], false));
    }

    #[test]
    fn a_continuous_future_is_not_traded() {
        let c = Contract { security_type: SecurityType::ContinuousFuture, ..Default::default() };
        assert!(tradable(c, "MNQ").is_err());
        let c = Contract { security_type: SecurityType::Future, include_expired: true, ..Default::default() };
        assert!(!tradable(c, "MNQZ6").unwrap().include_expired);
    }
}
