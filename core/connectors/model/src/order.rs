//! Execution (v2): what an OTW module asks a venue to work, and what the venue answers.
//!
//! An order is always about one instrument and carries the caller's own id
//! (`client_order_id`), the key a submission whose answer never came is looked up by instead
//! of being sent a second time. Prices and quantities are exact decimals; a venue's tick and
//! lot rules are checked against them before anything leaves.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::caps::{Capabilities, OrderCaps};
use crate::error::ConnectorError;
use crate::event::Side;
use crate::id::InstrumentId;
use crate::instrument::Mapped;
use crate::num::{Price, Qty, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    StopLimit,
    /// A stop that follows the market by a distance ([`Trail`]): up behind a sell, down
    /// behind a buy, never back. `stop`, when given, is the price that starts it trailing.
    TrailingStop,
    /// Market if touched: a market order once the price comes to `stop` from the other side
    /// than a stop's (a buy below the market, a sell above).
    Mit,
    /// Limit if touched: the same trigger, then a limit at `limit`.
    Lit,
}

impl OrderType {
    /// Which prices this type must carry: (limit, stop). A trailing stop's start price is
    /// optional and checked on its own.
    pub fn prices(self) -> (bool, bool) {
        match self {
            OrderType::Market | OrderType::TrailingStop => (false, false),
            OrderType::Limit => (true, false),
            OrderType::Stop | OrderType::Mit => (false, true),
            OrderType::StopLimit | OrderType::Lit => (true, true),
        }
    }

    /// Whether the order waits for a trigger before it works.
    pub fn triggered(self) -> bool {
        matches!(self, OrderType::Stop | OrderType::StopLimit | OrderType::TrailingStop | OrderType::Mit | OrderType::Lit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TifKind {
    Day,
    Gtc,
    Gtd,
    Ioc,
    Fok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "until")]
pub enum TimeInForce {
    Day,
    Gtc,
    /// Good until this moment.
    Gtd(Timestamp),
    Ioc,
    /// Fill or kill: all of it at once, or nothing.
    Fok,
}

impl TimeInForce {
    pub fn kind(self) -> TifKind {
        match self {
            TimeInForce::Day => TifKind::Day,
            TimeInForce::Gtc => TifKind::Gtc,
            TimeInForce::Gtd(_) => TifKind::Gtd,
            TimeInForce::Ioc => TifKind::Ioc,
            TimeInForce::Fok => TifKind::Fok,
        }
    }

    /// Whether the order lives only for the moment it is sent.
    pub fn immediate(self) -> bool {
        matches!(self, TimeInForce::Ioc | TimeInForce::Fok)
    }
}

/// How far a trailing stop follows the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Trail {
    /// A price distance.
    Amount(Price),
    /// A percentage of the price (`1.5` is 1.5%).
    Percent(Decimal),
}

impl Trail {
    pub fn kind(self) -> TrailKind {
        match self {
            Trail::Amount(_) => TrailKind::Amount,
            Trail::Percent(_) => TrailKind::Percent,
        }
    }

    fn value(self) -> Decimal {
        match self {
            Trail::Amount(p) => p.value(),
            Trail::Percent(d) => d,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailKind {
    Amount,
    Percent,
}

/// The price a conditional order watches for its trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    /// The last trade.
    Last,
    /// The venue's mark price (futures).
    Mark,
    /// The bid for a sell, the ask for a buy.
    BidAsk,
    /// The middle of the bid and the ask.
    Mid,
}

/// How hard an adaptive algo works the order: patient waits for a better price, urgent fills
/// sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Patient,
    Normal,
    Urgent,
}

/// What an order asks beyond its type, prices and time in force. Each is declared per venue
/// ([`crate::caps::OrderCaps`]) and refused before anything leaves where it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderOpts {
    /// Only shrink the position, never open or turn one.
    #[serde(default)]
    pub reduce_only: bool,
    /// A limit that only rests: refused rather than filled at once against the book.
    #[serde(default)]
    pub post_only: bool,
    /// A trailing stop's distance; what that type needs and only it.
    #[serde(default)]
    pub trail: Option<Trail>,
    /// Iceberg: the size shown on the book, the rest hidden behind it.
    #[serde(default)]
    pub display_qty: Option<Qty>,
    /// Also trigger and fill outside regular trading hours.
    #[serde(default)]
    pub outside_rth: bool,
    /// The price a conditional order triggers on; the venue's default when absent.
    #[serde(default)]
    pub trigger: Option<TriggerKind>,
    /// On its trigger, close whatever position is open instead of a set quantity.
    #[serde(default)]
    pub close_position: bool,
    /// Worked by the venue's adaptive algo at this urgency.
    #[serde(default)]
    pub adaptive: Option<Urgency>,
}

impl OrderOpts {
    pub fn is_plain(&self) -> bool {
        *self == OrderOpts::default()
    }
}

/// The canonical order states (spec 5.8). A venue's own word is mapped onto these, and a word
/// the mapping does not know stays `Unknown(raw)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    PendingNew,
    New,
    PartiallyFilled,
    Filled,
    PendingCancel,
    Cancelled,
    PendingReplace,
    Rejected,
    Expired,
}

impl OrderStatus {
    /// No longer working, whatever happens next.
    pub fn is_final(self) -> bool {
        matches!(self, OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired)
    }
}

/// One order to place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderRequest {
    /// The caller's id for this order, unique per account.
    pub client_order_id: String,
    pub instrument: InstrumentId,
    pub side: Side,
    pub order_type: OrderType,
    pub qty: Qty,
    pub limit: Option<Price>,
    pub stop: Option<Price>,
    pub tif: TimeInForce,
    /// The `client_order_id` of the entry this exit is attached to: it works only once that
    /// one fills, and the venue links the exits of one entry so the first to fill cancels
    /// the others.
    pub parent: Option<String>,
    /// One-cancels-other group, for orders that are not exits of one entry.
    pub oca_group: Option<String>,
    /// Which account of a login that holds several; `None`: the venue's default.
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub opts: OrderOpts,
}

/// A change to a working order; `None` leaves that field as it is. Side and type are not
/// changed in place: that is a cancel and a new order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderAmend {
    #[serde(default)]
    pub limit: Option<Price>,
    #[serde(default)]
    pub stop: Option<Price>,
    #[serde(default)]
    pub qty: Option<Qty>,
    #[serde(default)]
    pub tif: Option<TimeInForce>,
}

impl OrderAmend {
    pub fn is_empty(&self) -> bool {
        self.limit.is_none() && self.stop.is_none() && self.qty.is_none() && self.tif.is_none()
    }
}

/// Check a change before it is sent: something to change, prices and quantity above zero, a
/// time in force the venue declares, an expiry in the future.
pub fn check_amend(caps: &Capabilities, a: &OrderAmend, now: Timestamp) -> Result<(), ConnectorError> {
    if !caps.orders.amend {
        return Err(ConnectorError::unsupported(format!("{} orders cannot be changed in place", caps.label)));
    }
    if a.is_empty() {
        return Err(ConnectorError::invalid("nothing to change"));
    }
    let above = |d: Decimal| d > Decimal::ZERO;
    if a.limit.is_some_and(|p| !above(p.value())) || a.stop.is_some_and(|p| !above(p.value())) {
        return Err(ConnectorError::invalid("a price must be above zero"));
    }
    if a.qty.is_some_and(|q| !above(q.value())) {
        return Err(ConnectorError::invalid("quantity must be above zero"));
    }
    if let Some(t) = a.tif {
        if !caps.orders.tif.contains(&t.kind()) {
            return Err(ConnectorError::unsupported(format!("{} takes no {:?} time in force", caps.label, t.kind())));
        }
        if let TimeInForce::Gtd(until) = t {
            if until <= now {
                return Err(ConnectorError::invalid("a good-till-date order needs an expiry in the future"));
            }
        }
    }
    Ok(())
}

/// A placed order: the caller's id and the venue's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderAck {
    pub client_order_id: String,
    pub order_id: String,
    pub status: Mapped<OrderStatus>,
}

/// An order's options against what the venue declares and what its type can carry.
fn check_opts(label: &str, oc: &OrderCaps, o: &OrderRequest) -> Result<(), ConnectorError> {
    let x = &o.opts;
    let no = |what: &str| Err(ConnectorError::unsupported(format!("{label} takes no {what}")));
    if x.reduce_only && !oc.reduce_only {
        return no("reduce-only order");
    }
    if x.post_only {
        if !oc.post_only {
            return no("post-only order");
        }
        if o.order_type != OrderType::Limit || o.tif.immediate() {
            return Err(ConnectorError::invalid("post-only is a limit order that rests: not a market, stop or immediate one"));
        }
    }
    match (o.order_type, x.trail) {
        (OrderType::TrailingStop, None) => return Err(ConnectorError::invalid("a trailing stop needs its distance (an amount or a percentage)")),
        (OrderType::TrailingStop, Some(t)) => {
            if !oc.trail.contains(&t.kind()) {
                return no(match t.kind() {
                    TrailKind::Amount => "trailing distance as an amount: give it as a percentage",
                    TrailKind::Percent => "trailing distance as a percentage: give it as an amount",
                });
            }
            if t.value() <= Decimal::ZERO {
                return Err(ConnectorError::invalid("a trailing distance must be above zero"));
            }
            if matches!(t, Trail::Percent(p) if p >= Decimal::ONE_HUNDRED) {
                return Err(ConnectorError::invalid("a trailing percentage must be below 100"));
            }
            if o.stop.is_some() && !oc.trail_start {
                return no("start price on a trailing stop: it trails from the moment it is placed");
            }
        }
        (_, Some(_)) => return Err(ConnectorError::invalid("only a trailing stop carries a trailing distance")),
        _ => {}
    }
    if let Some(d) = x.display_qty {
        if !oc.iceberg {
            return no("iceberg order");
        }
        if o.limit.is_none() {
            return Err(ConnectorError::invalid("an iceberg is a limit order: only a resting limit shows part of its size"));
        }
        if d.is_zero() || d.is_sign_negative() || d.value() >= o.qty.value() {
            return Err(ConnectorError::invalid("the shown size must be above zero and below the order's size"));
        }
    }
    if x.outside_rth && !oc.outside_rth {
        return no("order outside regular trading hours");
    }
    if let Some(t) = x.trigger {
        if !oc.triggers.contains(&t) {
            return no(&format!("{t:?} trigger").to_lowercase());
        }
        if !o.order_type.triggered() {
            return Err(ConnectorError::invalid("only a conditional order (stop, trailing, if touched) has a trigger price"));
        }
    }
    if x.close_position {
        if !oc.close_position {
            return no("close-position order");
        }
        if !matches!(o.order_type, OrderType::Stop | OrderType::Mit) || x.reduce_only || o.parent.is_some() {
            return Err(ConnectorError::invalid("close-position is a stop or market-if-touched order of its own, not reduce-only nor an attached exit"));
        }
    }
    if x.adaptive.is_some() {
        if !oc.adaptive {
            return no("adaptive algo");
        }
        if !matches!(o.order_type, OrderType::Market | OrderType::Limit) {
            return Err(ConnectorError::invalid("the adaptive algo works market and limit orders only"));
        }
    }
    Ok(())
}

/// Check orders sent together before anything leaves: each well formed, each type and time
/// in force one the venue declares, every exit after the entry it hangs on. A refusal names
/// the field to fix; nothing of a refused set is sent.
pub fn check_orders(caps: &Capabilities, orders: &[OrderRequest], now: Timestamp) -> Result<(), ConnectorError> {
    check_orders_for(&caps.label, &caps.orders, orders, now)
}

/// [`check_orders`] against order capabilities a broker states itself (one run by OTW, with
/// no venue file), `label` naming it in refusals.
pub fn check_orders_for(label: &str, oc: &OrderCaps, orders: &[OrderRequest], now: Timestamp) -> Result<(), ConnectorError> {
    if orders.is_empty() {
        return Err(ConnectorError::invalid("no order to send"));
    }
    for (i, o) in orders.iter().enumerate() {
        if o.client_order_id.trim().is_empty() || orders[..i].iter().any(|p| p.client_order_id == o.client_order_id) {
            return Err(ConnectorError::invalid("every order needs its own client order id"));
        }
        if i > 0 && o.instrument != orders[0].instrument {
            return Err(ConnectorError::invalid("orders sent together are on one instrument"));
        }
        if !oc.types.contains(&o.order_type) {
            return Err(ConnectorError::unsupported(format!("{} takes no {:?} order", label, o.order_type)));
        }
        if !oc.tif.contains(&o.tif.kind()) {
            return Err(ConnectorError::unsupported(format!("{} takes no {:?} time in force", label, o.tif.kind())));
        }
        if o.qty.is_zero() || o.qty.is_sign_negative() {
            return Err(ConnectorError::invalid("quantity must be above zero"));
        }
        let (limit, stop) = o.order_type.prices();
        let positive = |p: Option<Price>| p.is_some_and(|p| !p.is_zero() && !p.is_sign_negative());
        if limit != o.limit.is_some() || (limit && !positive(o.limit)) {
            return Err(ConnectorError::invalid(if limit { "this order needs a limit price above zero" } else { "this order carries no limit price" }));
        }
        // A trailing stop may name the price it starts trailing at.
        let stop_ok = stop || (o.order_type == OrderType::TrailingStop && o.stop.is_some());
        if stop_ok != o.stop.is_some() || (stop_ok && !positive(o.stop)) {
            return Err(ConnectorError::invalid(if stop { "this order needs a stop price above zero" } else { "this order carries no stop price" }));
        }
        if let TimeInForce::Gtd(until) = o.tif {
            if until <= now {
                return Err(ConnectorError::invalid("a good-till-date order needs an expiry in the future"));
            }
        }
        check_opts(label, oc, o)?;
        if let Some(p) = &o.parent {
            if !oc.brackets {
                return Err(ConnectorError::unsupported(format!("{} takes no attached exits", label)));
            }
            match orders[..i].iter().find(|e| &e.client_order_id == p) {
                Some(entry) if entry.parent.is_none() => {}
                Some(_) => return Err(ConnectorError::invalid("an exit cannot carry exits of its own")),
                None => return Err(ConnectorError::invalid("an exit must follow the entry it is attached to")),
            }
        }
        if o.oca_group.is_some() && !oc.oca {
            return Err(ConnectorError::unsupported(format!("{} takes no one-cancels-other group", label)));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::caps::{Maturity, OrderCaps};

    fn caps(orders: OrderCaps) -> Capabilities {
        let mut c = Capabilities::from_toml(
            "provider = \"t\"\nlabel = \"T\"\nvenue = \"T\"\napi_version = \"1\"\nmaturity = \"M2\"\nasset_classes = [\"future\"]\ninstrument_discovery = \"none\"\nauth = \"none\"\n",
        )
        .unwrap();
        c.orders = orders;
        assert_eq!(c.maturity, Maturity::M2);
        c
    }

    fn all() -> OrderCaps {
        OrderCaps {
            types: [OrderType::Market, OrderType::Limit, OrderType::Stop, OrderType::StopLimit, OrderType::TrailingStop, OrderType::Mit, OrderType::Lit].into(),
            tif: [TifKind::Day, TifKind::Gtc, TifKind::Gtd, TifKind::Ioc, TifKind::Fok].into(),
            amend: true,
            brackets: true,
            oca: true,
            reduce_only: true,
            post_only: true,
            trail: [TrailKind::Amount, TrailKind::Percent].into(),
            trail_start: true,
            iceberg: true,
            outside_rth: true,
            triggers: [TriggerKind::Last, TriggerKind::Mark].into(),
            close_position: true,
            adaptive: true,
            client_id: true,
            ..Default::default()
        }
    }

    fn px(v: i64) -> Option<Price> {
        Some(Price::new(Decimal::from(v)))
    }

    fn order(id: &str, t: OrderType, parent: Option<&str>) -> OrderRequest {
        let (l, s) = t.prices();
        OrderRequest {
            client_order_id: id.into(),
            instrument: InstrumentId::new("IBKR", "MNQ"),
            side: Side::Buy,
            order_type: t,
            qty: Qty::new(Decimal::ONE),
            limit: if l { px(10) } else { None },
            stop: if s { px(9) } else { None },
            tif: TimeInForce::Day,
            parent: parent.map(Into::into),
            oca_group: None,
            account: None,
            opts: OrderOpts::default(),
        }
    }

    #[test]
    fn a_bracket_passes_and_a_malformed_set_is_refused_whole() {
        let c = caps(all());
        let now = Timestamp::from_secs(1_000).unwrap();
        let ok = [order("e", OrderType::Limit, None), order("tp", OrderType::Limit, Some("e")), order("sl", OrderType::Stop, Some("e"))];
        assert!(check_orders(&c, &ok, now).is_ok());
        assert!(check_orders(&c, &[], now).is_err());
        assert!(check_orders(&c, &[order("tp", OrderType::Limit, Some("e")), order("e", OrderType::Limit, None)], now).is_err());
        assert!(check_orders(&c, &[order("e", OrderType::Limit, None), order("e", OrderType::Stop, None)], now).is_err());
        let mut no_price = order("e", OrderType::Limit, None);
        no_price.limit = None;
        assert!(check_orders(&c, &[no_price], now).is_err());
        let mut stray = order("e", OrderType::Market, None);
        stray.limit = px(10);
        assert!(check_orders(&c, &[stray], now).is_err());
        let mut zero = order("e", OrderType::Market, None);
        zero.qty = Qty::ZERO;
        assert!(check_orders(&c, &[zero], now).is_err());
        let mut gtd = order("e", OrderType::Limit, None);
        gtd.tif = TimeInForce::Gtd(Timestamp::from_secs(500).unwrap());
        assert!(check_orders(&c, &[gtd], now).is_err());
    }

    #[test]
    fn a_change_needs_something_to_change_and_sane_values() {
        let c = caps(all());
        let now = Timestamp::from_secs(1_000).unwrap();
        assert!(check_amend(&c, &OrderAmend::default(), now).is_err());
        assert!(check_amend(&c, &OrderAmend { qty: Some(Qty::new(Decimal::from(3))), ..Default::default() }, now).is_ok());
        assert!(check_amend(&c, &OrderAmend { qty: Some(Qty::ZERO), ..Default::default() }, now).is_err());
        assert!(check_amend(&c, &OrderAmend { limit: px(0), ..Default::default() }, now).is_err());
        let past = OrderAmend { tif: Some(TimeInForce::Gtd(Timestamp::from_secs(10).unwrap())), ..Default::default() };
        assert!(check_amend(&c, &past, now).is_err());
        let mut no = all();
        no.amend = false;
        assert!(check_amend(&caps(no), &OrderAmend { limit: px(10), ..Default::default() }, now).is_err());
    }

    #[test]
    fn what_the_venue_does_not_declare_is_unsupported() {
        let now = Timestamp::from_secs(1_000).unwrap();
        let mut oc = all();
        oc.types.remove(&OrderType::StopLimit);
        oc.brackets = false;
        let c = caps(oc);
        let e = check_orders(&c, &[order("e", OrderType::StopLimit, None)], now).unwrap_err();
        assert_eq!(e.kind, crate::ErrorKind::Unsupported);
        let e = check_orders(&c, &[order("e", OrderType::Limit, None), order("tp", OrderType::Limit, Some("e"))], now).unwrap_err();
        assert_eq!(e.kind, crate::ErrorKind::Unsupported);
        // Declaring nothing takes nothing.
        assert!(check_orders(&caps(OrderCaps::default()), &[order("e", OrderType::Market, None)], now).is_err());
    }

    #[test]
    fn each_option_is_checked_against_the_venue_and_the_type() {
        let now = Timestamp::from_secs(1_000).unwrap();
        let c = caps(all());
        let with = |t: OrderType, f: &dyn Fn(&mut OrderRequest)| {
            let mut o = order("e", t, None);
            f(&mut o);
            o
        };
        // A trailing stop needs its distance, may carry a start price, and nothing else does.
        assert!(check_orders(&c, &[order("e", OrderType::TrailingStop, None)], now).is_err());
        let trail = |o: &mut OrderRequest| o.opts.trail = Some(Trail::Percent(Decimal::ONE));
        assert!(check_orders(&c, &[with(OrderType::TrailingStop, &trail)], now).is_ok());
        let started = |o: &mut OrderRequest| {
            trail(o);
            o.stop = px(9);
        };
        assert!(check_orders(&c, &[with(OrderType::TrailingStop, &started)], now).is_ok());
        assert!(check_orders(&c, &[with(OrderType::Limit, &trail)], now).is_err());
        let wide = |o: &mut OrderRequest| o.opts.trail = Some(Trail::Percent(Decimal::ONE_HUNDRED));
        assert!(check_orders(&c, &[with(OrderType::TrailingStop, &wide)], now).is_err());
        // If touched: a trigger, and a limit for LIT.
        assert!(check_orders(&c, &[order("e", OrderType::Mit, None), order("f", OrderType::Lit, None)], now).is_ok());
        // Post-only rests: a limit, never immediate.
        let post = |o: &mut OrderRequest| o.opts.post_only = true;
        assert!(check_orders(&c, &[with(OrderType::Limit, &post)], now).is_ok());
        assert!(check_orders(&c, &[with(OrderType::Market, &post)], now).is_err());
        let post_fok = |o: &mut OrderRequest| {
            post(o);
            o.tif = TimeInForce::Fok;
        };
        assert!(check_orders(&c, &[with(OrderType::Limit, &post_fok)], now).is_err());
        // Iceberg: a limit showing less than its size.
        let shown = |q: i64| move |o: &mut OrderRequest| o.opts.display_qty = Some(Qty::new(Decimal::from(q)));
        assert!(check_orders(&c, &[with(OrderType::Limit, &|o| { o.qty = Qty::new(Decimal::from(10)); shown(2)(o) })], now).is_ok());
        assert!(check_orders(&c, &[with(OrderType::Limit, &shown(1))], now).is_err());
        assert!(check_orders(&c, &[with(OrderType::Market, &|o| { o.qty = Qty::new(Decimal::from(10)); shown(2)(o) })], now).is_err());
        // A trigger source only on a conditional order, and one the venue names.
        let mark = |o: &mut OrderRequest| o.opts.trigger = Some(TriggerKind::Mark);
        assert!(check_orders(&c, &[with(OrderType::Stop, &mark)], now).is_ok());
        assert!(check_orders(&c, &[with(OrderType::Limit, &mark)], now).is_err());
        let mid = |o: &mut OrderRequest| o.opts.trigger = Some(TriggerKind::Mid);
        assert!(check_orders(&c, &[with(OrderType::Stop, &mid)], now).is_err());
        // Close-position: a stop of its own.
        let close = |o: &mut OrderRequest| o.opts.close_position = true;
        assert!(check_orders(&c, &[with(OrderType::Stop, &close)], now).is_ok());
        assert!(check_orders(&c, &[with(OrderType::Limit, &close)], now).is_err());
        let close_reduce = |o: &mut OrderRequest| {
            close(o);
            o.opts.reduce_only = true;
        };
        assert!(check_orders(&c, &[with(OrderType::Stop, &close_reduce)], now).is_err());
        // Adaptive: market and limit.
        let adaptive = |o: &mut OrderRequest| o.opts.adaptive = Some(Urgency::Normal);
        assert!(check_orders(&c, &[with(OrderType::Market, &adaptive)], now).is_ok());
        assert!(check_orders(&c, &[with(OrderType::Stop, &adaptive)], now).is_err());
        // A venue that declares none of them refuses each, as unsupported.
        let mut bare = all();
        bare.post_only = false;
        bare.reduce_only = false;
        bare.outside_rth = false;
        bare.trail = [TrailKind::Amount].into();
        let b = caps(bare);
        for f in [&post as &dyn Fn(&mut OrderRequest), &|o: &mut OrderRequest| o.opts.reduce_only = true, &|o: &mut OrderRequest| o.opts.outside_rth = true] {
            let e = check_orders(&b, &[with(OrderType::Limit, f)], now).unwrap_err();
            assert_eq!(e.kind, crate::ErrorKind::Unsupported);
        }
        let e = check_orders(&b, &[with(OrderType::TrailingStop, &trail)], now).unwrap_err();
        assert_eq!(e.kind, crate::ErrorKind::Unsupported);
    }

    #[test]
    fn options_travel_as_json_and_default_to_none() {
        let o: OrderOpts = serde_json::from_str(r#"{"trail":{"kind":"amount","value":"2.5"},"post_only":true}"#).unwrap();
        assert_eq!(o.trail, Some(Trail::Amount(Price::new(Decimal::new(25, 1)))));
        assert!(o.post_only && !o.reduce_only);
        assert!(serde_json::from_str::<OrderOpts>("{}").unwrap().is_plain());
    }
}
