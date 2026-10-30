//! The user's risk limits and halts, checked before an order leaves (onboarding spec 5).
//!
//! - **One gate, every broker.** Run between the route and the adapter, after the trading
//!   grant and lock, for paper accounts as for venues. [`check`] is a pure function over the
//!   ticket, the limits and the facts gathered for it, so every refusal is unit-tested.
//! - **Refused, never resized.** A breach names the limit and the value; the order is not
//!   changed to fit.
//! - **Taking risk off always passes.** A reduce-only order, a close, or an order that only
//!   shrinks what is held goes out through every limit and every halt: a halt must never trap
//!   a position.
//! - **Halts.** The global kill switch halts every account; reaching the loss limit halts its
//!   account, and so does a position drift until the user acknowledges it. A halt ends at the start of the next hour, day or week (the policy, in the user's
//!   timezone), or only when lifted, and the password lifts it sooner when the policy allows.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use otw_connector_model::{Decimal, Side};
use otw_store::broker_risk::{self as store, Halt, Limits, Policy};
use otw_store::brokers::BrokerRow;
use time::{Duration, OffsetDateTime, Time};
use time_tz::{OffsetDateTimeExt, PrimitiveDateTimeExt};

use super::{Broker, OrderLeg, OrderTicket};
use crate::AppState;

/// A refusal of the gate, with the code the chart answers: `trading_halted`, `risk_limit`, or
/// `duplicate_order` (send again with `confirm_duplicate` to go ahead).
#[derive(Debug, Clone, PartialEq)]
pub struct RiskRefusal {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for RiskRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RiskRefusal {}

fn limit(message: String) -> RiskRefusal {
    RiskRefusal { code: "risk_limit", message }
}

/// The loss over the current period, or why it cannot be told.
#[derive(Debug, Clone, PartialEq)]
pub enum Loss {
    /// `start`: the equity the period started with.
    Known { loss: Decimal, start: Decimal, currency: String },
    Unknown(String),
}

/// What the gate needs to know about the account and the instrument, gathered once.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub halts: Vec<Halt>,
    /// Signed size held on the ticket's instrument (long positive); `None` when not read.
    pub held: Option<Decimal>,
    /// Point value of the instrument, when known (1 for anything but a future).
    pub multiplier: Option<Decimal>,
    /// Legs sent from the account in the last minute, and working now.
    pub sent_last_min: i64,
    pub open_legs: usize,
    /// Recent legs on the instrument, as their requests (within the duplicate window).
    pub recent: Vec<serde_json::Value>,
    /// Only read when a loss limit is set.
    pub loss: Option<Loss>,
    /// How old the chart's last price is, when the ticket says when it was seen.
    pub reference_age: Option<Duration>,
}

/// The oldest chart price a collar measures from.
pub const REFERENCE_MAX_AGE: Duration = Duration::seconds(60);

fn signed(side: Side, qty: Decimal) -> Decimal {
    if side == Side::Buy { qty } else { -qty }
}

/// The legs that open or grow exposure: a ticket's entries (its exits only ever reduce).
fn entries(ticket: &OrderTicket) -> impl Iterator<Item = &OrderLeg> {
    ticket.legs.iter().filter(|l| l.parent.as_deref().is_none_or(str::is_empty))
}

/// Whether every entry of the ticket only takes risk off: flagged so, or shrinking what is
/// held without crossing to the other side.
pub fn reduces(ticket: &OrderTicket, held: Option<Decimal>) -> bool {
    let mut any = false;
    for l in entries(ticket) {
        any = true;
        if l.reduce_only || l.close_position {
            continue;
        }
        let Some(h) = held else { return false };
        let after = h + signed(l.side, l.qty);
        let shrinks = !h.is_zero() && after.abs() <= h.abs() && (after.is_zero() || after.is_sign_positive() == h.is_sign_positive());
        if !shrinks {
            return false;
        }
    }
    any
}

fn price_of(l: &OrderLeg, reference: Option<Decimal>) -> Option<Decimal> {
    l.limit.or(l.stop).or(reference)
}

fn same_order(l: &OrderLeg, r: &serde_json::Value) -> bool {
    let Ok(prev) = serde_json::from_value::<OrderLeg>(r.clone()) else { return false };
    prev.side == l.side && prev.order_type == l.order_type && prev.qty == l.qty && prev.limit == l.limit && prev.stop == l.stop
}

fn halted(h: &Halt) -> String {
    let what = match h.reason.as_str() {
        "kill_switch" => "the kill switch is on",
        super::reconcile::DRIFT => "a position moved without fills to explain it",
        _ => "the loss limit was reached",
    };
    let scope = if h.account_id.is_none() { "every account" } else { "this account" };
    let until = match h.until {
        Some(t) => format!("until {}", t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()),
        None if h.reason == super::reconcile::DRIFT => "until you acknowledge it".into(),
        None => "until it is lifted".into(),
    };
    format!("trading is halted on {scope}: {what} ({until}). Only orders that reduce a position go out; lift it in Settings, Brokers.")
}

/// The gate itself: every check in the spec's order, the first breach refused.
pub fn check(l: &Limits, f: &Facts, t: &OrderTicket) -> Result<(), RiskRefusal> {
    let reducing = reduces(t, f.held);
    if reducing {
        return Ok(());
    }
    if let Some(h) = f.halts.first() {
        return Err(RiskRefusal { code: "trading_halted", message: halted(h) });
    }
    // Price collar against the chart's last price.
    if let Some(pct) = l.collar_pct {
        for leg in entries(t) {
            let Some(r) = t.reference.filter(|r| *r > Decimal::ZERO) else {
                return Err(limit(format!("the price collar ({pct}%) needs the chart's last price, and none came with the order: wait for a price")));
            };
            match f.reference_age {
                None => return Err(limit(format!("the price collar ({pct}%) needs the time of the chart's last price, and none came with the order"))),
                Some(age) if age > REFERENCE_MAX_AGE => {
                    return Err(limit(format!(
                        "the chart's last price {r} is {} s old, too old for your price collar ({pct}%): wait for a live price",
                        age.whole_seconds()
                    )))
                }
                Some(_) => {}
            }
            for p in [leg.limit, leg.stop].into_iter().flatten() {
                let off = ((p - r).abs() / r * Decimal::ONE_HUNDRED).round_dp(2);
                if off > pct {
                    return Err(limit(format!("{p} is {off}% from the last price {r}, past your price collar of {pct}%")));
                }
            }
        }
    }
    // Size and value per order.
    for leg in entries(t) {
        if let Some(max) = l.max_order_qty {
            if leg.qty > max {
                return Err(limit(format!("{} is over your size limit of {max} per order", leg.qty)));
            }
        }
        if let Some(max) = l.max_order_value {
            let Some(px) = price_of(leg, t.reference) else {
                return Err(limit(format!("the value limit ({max}) needs a price to value this order, and none is at hand: wait for the chart's price")));
            };
            let Some(m) = f.multiplier else {
                return Err(limit(format!("the value limit ({max}) needs {}'s point value: set it in the instrument settings", t.ticker)));
            };
            let value = (leg.qty * px * m).round_dp(2);
            if value > max {
                return Err(limit(format!("this order is worth {value}, over your value limit of {max} per order")));
            }
        }
    }
    // Position once filled.
    if let Some(max) = l.max_position_qty {
        let Some(h) = f.held else {
            return Err(limit(format!("the position limit ({max}) needs what is held on {}, which could not be read", t.ticker)));
        };
        for leg in entries(t) {
            let after = (h + signed(leg.side, leg.qty)).abs();
            if after > max {
                return Err(limit(format!("{} would be held on {} once filled, over your position limit of {max}", after, t.ticker)));
            }
        }
    }
    if let Some(max) = l.max_position_value {
        let Some(h) = f.held else {
            return Err(limit(format!("the position value limit ({max}) needs what is held on {}, which could not be read", t.ticker)));
        };
        for leg in entries(t) {
            let Some(px) = price_of(leg, t.reference) else {
                return Err(limit(format!("the position value limit ({max}) needs a price to value the position, and none is at hand: wait for the chart's price")));
            };
            let Some(m) = f.multiplier else {
                return Err(limit(format!("the position value limit ({max}) needs {}'s point value: set it in the instrument settings", t.ticker)));
            };
            let value = ((h + signed(leg.side, leg.qty)).abs() * px * m).round_dp(2);
            if value > max {
                return Err(limit(format!("the position on {} would be worth {value} once filled, over your position value limit of {max}", t.ticker)));
            }
        }
    }
    // Flow.
    let n = t.legs.len() as i64;
    if let Some(max) = l.max_orders_per_min {
        if f.sent_last_min + n > i64::from(max) {
            return Err(limit(format!("{} orders were sent in the last minute and your limit is {max}: wait a moment", f.sent_last_min)));
        }
    }
    if let Some(max) = l.max_open_orders {
        if f.open_legs as i64 + n > i64::from(max) {
            return Err(limit(format!("{} orders are working and your limit is {max} open orders", f.open_legs)));
        }
    }
    // Duplicate.
    if l.duplicate_secs.is_some() && !t.confirm_duplicate {
        if let Some(leg) = entries(t).find(|leg| f.recent.iter().any(|r| same_order(leg, r))) {
            let secs = l.duplicate_secs.unwrap_or_default();
            return Err(RiskRefusal {
                code: "duplicate_order",
                message: format!("the same order ({} {} {}) was sent less than {secs} s ago: confirm to send it again", side_word(leg.side), leg.qty, t.ticker),
            });
        }
    }
    // Loss over the period.
    if l.max_loss.is_some() {
        match &f.loss {
            Some(Loss::Known { loss, start, currency }) => {
                if let Some(words) = loss_reached(l, *loss, *start, currency) {
                    return Err(RiskRefusal { code: "trading_halted", message: format!("the loss this period is {} {currency}, at your loss limit of {words}: trading is halted on this account", loss.round_dp(2)) });
                }
            }
            Some(Loss::Unknown(why)) => return Err(limit(format!("your loss limit cannot be checked: {why}"))),
            None => return Err(limit("your loss limit cannot be checked: the account was not read".into())),
        }
    }
    Ok(())
}

/// The loss limit in words when `loss` has reached it, else `None`. A percentage limit is
/// measured against the equity the period started with.
fn loss_reached(l: &Limits, loss: Decimal, start: Decimal, currency: &str) -> Option<String> {
    let max = l.max_loss?;
    let (cap, words) = if l.max_loss_pct {
        let cap = (start * max / Decimal::ONE_HUNDRED).round_dp(2);
        (cap, format!("{max}% ({cap} {currency})"))
    } else {
        (max, max.to_string())
    };
    (loss > Decimal::ZERO && loss >= cap).then_some(words)
}

fn side_word(s: Side) -> &'static str {
    if s == Side::Buy { "buy" } else { "sell" }
}

/// When the current period started, and when the next one starts, for `reset` in `tz` (a
/// manual policy measures loss per day).
pub fn period(reset: &str, tz_name: &str, now: OffsetDateTime) -> (OffsetDateTime, OffsetDateTime) {
    let tz = time_tz::timezones::get_by_name(tz_name).unwrap_or(time_tz::timezones::db::UTC);
    let local = now.to_timezone(tz);
    let (date, hour) = (local.date(), local.hour());
    let start_local = match reset {
        "hour" => date.with_hms(hour, 0, 0).unwrap_or(date.midnight()),
        "week" => {
            let back = i64::from(local.weekday().number_days_from_monday());
            (date - Duration::days(back)).with_time(Time::MIDNIGHT)
        }
        _ => date.with_time(Time::MIDNIGHT),
    };
    let next_local = match reset {
        "hour" => start_local + Duration::hours(1),
        "week" => start_local + Duration::weeks(1),
        _ => start_local + Duration::days(1),
    };
    let at = |p: time::PrimitiveDateTime| p.assume_timezone(tz).take_first().unwrap_or_else(|| p.assume_utc());
    (at(start_local), at(next_local))
}

/// When a halt set now ends on its own; `None` under a manual policy.
pub fn halt_until(p: &Policy, now: OffsetDateTime) -> Option<OffsetDateTime> {
    if p.reset == "manual" {
        return None;
    }
    Some(period(&p.reset, &p.timezone, now).1)
}

/// The account's equity now, in one currency, as the broker states it.
async fn equity(connector: &dyn Broker, settings: &HashMap<String, String>) -> Result<(Decimal, String), String> {
    let rows = connector.account(settings, None).await.map_err(|e| format!("{e:#}"))?;
    let currency = rows.first().map(|r| r.currency.clone()).ok_or("the broker states no account figures")?;
    let mut total = Decimal::ZERO;
    for r in &rows {
        if r.currency != currency {
            return Err(format!("the broker states the account in {currency} and {}", r.currency));
        }
        let e = r.equity.ok_or("the broker states no equity for this account")?;
        total += Decimal::try_from(e).map_err(|_| "the equity the broker states is not a number".to_string())?;
    }
    Ok((total, currency))
}

/// The loss since the current period started, measured from the equity recorded at its
/// start (recorded now, at zero loss, when this is the period's first read).
pub async fn loss(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, now: OffsetDateTime) -> Result<Loss> {
    let p = store::policy(&state.pool).await?;
    let (start, _) = period(&p.reset, &p.timezone, now);
    let (now_eq, currency) = match equity(connector, settings).await {
        Ok(v) => v,
        Err(why) => return Ok(Loss::Unknown(why)),
    };
    match store::equity_mark(&state.pool, account.id).await? {
        Some((at, eq, cur)) if at == start && cur == currency => Ok(Loss::Known { loss: (eq - now_eq).max(Decimal::ZERO), start: eq, currency }),
        _ => {
            store::set_equity_mark(&state.pool, account.id, start, now_eq, &currency).await?;
            Ok(Loss::Known { loss: Decimal::ZERO, start: now_eq, currency })
        }
    }
}

/// Signed size held on the chart's instrument, by the broker's spelling of it.
async fn held(connector: &dyn Broker, settings: &HashMap<String, String>, ticket: &OrderTicket) -> Option<(Decimal, Option<Decimal>)> {
    // A spot account holds the pair's base asset, not a position on it.
    if let Some(q) = connector.held_asset(settings, &ticket.ticker).await.ok()? {
        return Some((q, None));
    }
    let http = super::client().ok()?;
    let rows = connector.positions(&http, settings).await.ok()?;
    let spelled = connector.symbol_for(settings, &ticket.ticker, &ticket.asset_type).await.ok().flatten().unwrap_or_else(|| ticket.ticker.clone());
    let mut total = Decimal::ZERO;
    let mut multiplier = None;
    for p in rows.iter().filter(|p| p.symbol.eq_ignore_ascii_case(&spelled) || p.symbol.eq_ignore_ascii_case(&ticket.ticker)) {
        let q: Decimal = format!("{:.10}", p.qty).parse().unwrap_or_default();
        total += if p.side == "short" { -q.abs() } else { q.abs() };
        multiplier = format!("{:.10}", p.multiplier).parse::<Decimal>().ok().map(|m| m.normalize());
    }
    Some((total, multiplier))
}

/// The instrument's point value: from a position on it, else its instrument settings; 1 for
/// anything but a future.
async fn multiplier(state: &AppState, ticket: &OrderTicket, from_position: Option<Decimal>) -> Option<Decimal> {
    if ticket.asset_type != "future" {
        return Some(Decimal::ONE);
    }
    if let Some(m) = from_position.filter(|m| *m > Decimal::ZERO) {
        return Some(m);
    }
    let provider = ticket.provider.as_deref()?;
    let meta = otw_store::instrument_meta::get(&state.pool, provider, &ticket.asset_type, &ticket.ticker).await.ok()??;
    meta.pointvalue.filter(|v| *v > 0.0).and_then(|v| format!("{v}").parse().ok())
}

/// Run the gate for a ticket on this account. A loss limit reached halts the account first.
pub async fn gate(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>, ticket: &OrderTicket) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let l = store::limits(&state.pool, account.id).await?;
    let halts = store::halts(&state.pool, Some(account.id), now).await?;
    if l.is_empty() && halts.is_empty() {
        return Ok(());
    }
    let reference_age = ticket.reference_at.and_then(|ms| OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()).map(|at| now - at);
    let mut f = Facts { halts, reference_age, ..Default::default() };
    // What is held decides whether the ticket takes risk off, which passes everything.
    let needs_held = !f.halts.is_empty() || l.max_position_qty.is_some() || l.max_position_value.is_some() || l.max_loss.is_some() || !entries(ticket).all(|e| e.reduce_only || e.close_position);
    let (h, from_position) = if needs_held { held(connector, settings, ticket).await.map_or((None, None), |(h, m)| (Some(h), m)) } else { (None, None) };
    f.held = h;
    if reduces(ticket, f.held) {
        return Ok(());
    }
    if l.max_order_value.is_some() || l.max_position_value.is_some() {
        f.multiplier = multiplier(state, ticket, from_position).await;
    }
    if l.max_orders_per_min.is_some() {
        f.sent_last_min = store::sent_since(&state.pool, account.id, now - Duration::minutes(1)).await?;
    }
    if l.max_open_orders.is_some() {
        f.open_legs = otw_store::broker_orders::open(&state.pool, account.id).await?.len();
    }
    if let Some(secs) = l.duplicate_secs {
        f.recent = store::recent_requests(&state.pool, account.id, &ticket.ticker, now - Duration::seconds(i64::from(secs))).await?;
    }
    if l.max_loss.is_some() && f.halts.is_empty() {
        f.loss = Some(loss(state, account, connector, settings, now).await?);
    }
    match check(&l, &f, ticket) {
        Ok(()) => Ok(()),
        Err(r) => {
            if let Some(Loss::Known { loss, start, currency }) = &f.loss {
                if let Some(words) = loss_reached(&l, *loss, *start, currency).filter(|_| f.halts.is_empty()) {
                    halt_for_loss(state, account, *loss, currency, &words).await;
                }
            }
            Err(anyhow::Error::new(r))
        }
    }
}

async fn halt_for_loss(state: &AppState, account: &BrokerRow, loss: Decimal, currency: &str, max: &str) {
    let until = match store::policy(&state.pool).await {
        Ok(p) => halt_until(&p, OffsetDateTime::now_utc()),
        Err(_) => None,
    };
    let detail = format!("loss {} {currency}, limit {max}", loss.round_dp(2));
    match store::add_halt(&state.pool, Some(account.id), "loss_limit", &detail, until).await {
        Ok(_) => super::reconcile::notify_halt(state, account, &detail),
        Err(e) => tracing::error!("risk: could not halt “{}” at its loss limit: {e:#}", account.name),
    }
}

/// The loss limit, checked on the reconciliation cycle too: an account past it halts even
/// without a new order.
pub async fn watch_loss(state: &AppState, account: &BrokerRow, connector: &dyn Broker, settings: &HashMap<String, String>) -> Result<()> {
    let l = store::limits(&state.pool, account.id).await?;
    if l.max_loss.is_none() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc();
    if !store::halts(&state.pool, Some(account.id), now).await?.is_empty() {
        return Ok(());
    }
    if let Loss::Known { loss, start, currency } = loss(state, account, connector, settings, now).await? {
        if let Some(words) = loss_reached(&l, loss, start, &currency) {
            halt_for_loss(state, account, loss, &currency, &words).await;
        }
    }
    Ok(())
}

/// A change to a working order: refused while trading is halted (cancel it instead), and its
/// new size held to the size limit.
pub async fn gate_modify(state: &AppState, account: &BrokerRow, change: &otw_connector_model::OrderAmend) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    if let Some(h) = store::halts(&state.pool, Some(account.id), now).await?.first() {
        return Err(anyhow::Error::new(RiskRefusal { code: "trading_halted", message: format!("{} A working order cannot be changed meanwhile: cancel it instead.", halted(h)) }));
    }
    let l = store::limits(&state.pool, account.id).await?;
    if let (Some(max), Some(q)) = (l.max_order_qty, change.qty) {
        if q.value() > max {
            return Err(anyhow::Error::new(limit(format!("{} is over your size limit of {max} per order", q.value()))));
        }
    }
    Ok(())
}

/// Check limits as the user sets them: positive, a collar under 100%.
pub fn validate(l: &Limits) -> Result<()> {
    let pos = |v: Option<Decimal>, what: &str| match v {
        Some(d) if d <= Decimal::ZERO => Err(anyhow!("the {what} must be more than zero")),
        _ => Ok(()),
    };
    pos(l.max_order_qty, "size limit")?;
    pos(l.max_order_value, "value limit")?;
    pos(l.max_position_qty, "position limit")?;
    pos(l.max_position_value, "position value limit")?;
    pos(l.max_loss, "loss limit")?;
    if l.max_loss_pct && l.max_loss.is_some_and(|m| m >= Decimal::ONE_HUNDRED) {
        return Err(anyhow!("a loss limit in percent is below 100"));
    }
    if let Some(c) = l.collar_pct {
        if c <= Decimal::ZERO || c >= Decimal::ONE_HUNDRED {
            return Err(anyhow!("the price collar is a percentage above 0 and below 100"));
        }
    }
    for (v, what) in [(l.max_orders_per_min, "orders per minute"), (l.max_open_orders, "open orders"), (l.duplicate_secs, "duplicate window")] {
        if v.is_some_and(|n| n <= 0) {
            return Err(anyhow!("the {what} must be more than zero"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::OrderType;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn ticket(legs: Vec<OrderLeg>) -> OrderTicket {
        OrderTicket { ticker: "ES".into(), asset_type: "future".into(), legs, provider: None, connector: None, reference: Some(d("5000")), reference_at: None, confirm_duplicate: false }
    }

    fn buy(qty: &str, limit: Option<&str>) -> OrderLeg {
        OrderLeg { limit: limit.map(d), ..OrderLeg::new("e", Side::Buy, if limit.is_some() { OrderType::Limit } else { OrderType::Market }, d(qty)) }
    }

    fn sell(qty: &str) -> OrderLeg {
        OrderLeg::new("s", Side::Sell, OrderType::Market, d(qty))
    }

    fn halt(account: bool) -> Halt {
        Halt { id: 1, account_id: account.then(uuid::Uuid::nil), reason: "kill_switch".into(), detail: String::new(), created_at: OffsetDateTime::UNIX_EPOCH, until: None }
    }

    fn facts() -> Facts {
        Facts { held: Some(Decimal::ZERO), multiplier: Some(d("50")), ..Default::default() }
    }

    #[test]
    fn no_limit_set_passes_and_a_halt_refuses_new_risk_but_not_a_reduction() {
        // T13
        assert!(check(&Limits::default(), &facts(), &ticket(vec![buy("1", None)])).is_ok());
        let f = Facts { halts: vec![halt(false)], held: Some(d("2")), ..facts() };
        let e = check(&Limits::default(), &f, &ticket(vec![buy("1", None)])).unwrap_err();
        assert_eq!(e.code, "trading_halted");
        assert!(e.message.contains("every account"), "{}", e.message);
        // Selling part of a long takes risk off: it goes out through the halt.
        assert!(check(&Limits::default(), &f, &ticket(vec![sell("2")])).is_ok());
        // Selling past flat opens a short: new risk.
        assert!(check(&Limits::default(), &f, &ticket(vec![sell("3")])).is_err());
        // A reduce-only flag passes even with nothing read.
        let f = Facts { held: None, ..f };
        assert!(check(&Limits::default(), &f, &ticket(vec![OrderLeg { reduce_only: true, ..sell("3") }])).is_ok());
    }

    #[test]
    fn size_value_and_position_limits_name_the_value_and_the_limit() {
        // T13
        let l = Limits { max_order_qty: Some(d("2")), ..Default::default() };
        let e = check(&l, &facts(), &ticket(vec![buy("3", Some("5000"))])).unwrap_err();
        assert_eq!(e.message, "3 is over your size limit of 2 per order");
        let l = Limits { max_order_value: Some(d("200000")), ..Default::default() };
        // 1 ES at 5000 x 50 = 250 000.
        let e = check(&l, &facts(), &ticket(vec![buy("1", Some("5000"))])).unwrap_err();
        assert!(e.message.contains("250000"), "{}", e.message);
        let f = Facts { multiplier: None, ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", Some("5000"))])).unwrap_err().message.contains("point value"));
        let l = Limits { max_position_qty: Some(d("3")), ..Default::default() };
        let f = Facts { held: Some(d("2")), ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", Some("4000"))])).is_ok());
        assert!(check(&l, &f, &ticket(vec![buy("2", Some("4000"))])).unwrap_err().message.contains("4 would be held"));
    }

    #[test]
    fn the_position_value_limit_values_what_is_held_once_filled() {
        // 2 ES held, buy 1 at 5000 x 50: 3 x 250 000 = 750 000.
        let l = Limits { max_position_value: Some(d("750000")), ..Default::default() };
        let f = Facts { held: Some(d("2")), ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", Some("5000"))])).is_ok());
        let e = check(&l, &f, &ticket(vec![buy("2", Some("5000"))])).unwrap_err();
        assert_eq!(e.message, "the position on ES would be worth 1000000 once filled, over your position value limit of 750000");
        // A market order is valued at the chart's last price.
        assert!(check(&l, &f, &ticket(vec![buy("2", None)])).is_err());
        let f = Facts { held: None, ..f };
        assert!(check(&l, &f, &ticket(vec![buy("1", Some("5000"))])).unwrap_err().message.contains("could not be read"));
    }

    #[test]
    fn the_collar_measures_from_the_charts_price_and_needs_one() {
        // T13
        let l = Limits { collar_pct: Some(d("2")), ..Default::default() };
        let fresh = Facts { reference_age: Some(Duration::seconds(2)), ..facts() };
        assert!(check(&l, &fresh, &ticket(vec![buy("1", Some("4950"))])).is_ok());
        let e = check(&l, &fresh, &ticket(vec![buy("1", Some("4800"))])).unwrap_err();
        assert_eq!(e.message, "4800 is 4.00% from the last price 5000, past your price collar of 2%");
        let t = OrderTicket { reference: None, ..ticket(vec![buy("1", None)]) };
        assert!(check(&l, &fresh, &t).unwrap_err().message.contains("needs the chart's last price"));
    }

    #[test]
    fn the_collar_refuses_a_price_too_old_or_of_unknown_age() {
        // T13
        let l = Limits { collar_pct: Some(d("2")), ..Default::default() };
        let t = ticket(vec![buy("1", Some("4950"))]);
        assert!(check(&l, &facts(), &t).unwrap_err().message.contains("needs the time of the chart's last price"));
        let old = Facts { reference_age: Some(Duration::seconds(61)), ..facts() };
        assert_eq!(check(&l, &old, &t).unwrap_err().message, "the chart's last price 5000 is 61 s old, too old for your price collar (2%): wait for a live price");
        let edge = Facts { reference_age: Some(REFERENCE_MAX_AGE), ..facts() };
        assert!(check(&l, &edge, &t).is_ok());
        // Taking risk off never needs a price.
        let close = Facts { held: Some(d("1")), ..old };
        assert!(check(&l, &close, &ticket(vec![sell("1")])).is_ok());
    }

    #[test]
    fn flow_duplicate_and_loss() {
        // T13
        let l = Limits { max_orders_per_min: Some(3), max_open_orders: Some(5), ..Default::default() };
        let f = Facts { sent_last_min: 3, ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", None)])).unwrap_err().message.contains("last minute"));
        let f = Facts { open_legs: 5, ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", None)])).unwrap_err().message.contains("open orders"));
        let l = Limits { duplicate_secs: Some(10), ..Default::default() };
        let prev = serde_json::to_value(buy("1", Some("4990"))).unwrap();
        let f = Facts { recent: vec![prev], ..facts() };
        let e = check(&l, &f, &ticket(vec![buy("1", Some("4990"))])).unwrap_err();
        assert_eq!(e.code, "duplicate_order");
        let t = OrderTicket { confirm_duplicate: true, ..ticket(vec![buy("1", Some("4990"))]) };
        assert!(check(&l, &f, &t).is_ok());
        assert!(check(&l, &f, &ticket(vec![buy("2", Some("4990"))])).is_ok(), "another size is another order");
        let l = Limits { max_loss: Some(d("500")), ..Default::default() };
        let f = Facts { loss: Some(Loss::Known { loss: d("500"), start: d("100000"), currency: "USD".into() }), ..facts() };
        assert_eq!(check(&l, &f, &ticket(vec![buy("1", None)])).unwrap_err().code, "trading_halted");
        let f = Facts { loss: Some(Loss::Known { loss: d("499.99"), start: d("100000"), currency: "USD".into() }), ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", None)])).is_ok());
        // 1% of the 7729.27 the period started with is 77.29.
        let l = Limits { max_loss: Some(d("1")), max_loss_pct: true, ..Default::default() };
        let f = Facts { loss: Some(Loss::Known { loss: d("20.96"), start: d("7729.27"), currency: "USDT".into() }), ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", None)])).is_ok());
        let f = Facts { loss: Some(Loss::Known { loss: d("77.29"), start: d("7729.27"), currency: "USDT".into() }), ..facts() };
        let e = check(&l, &f, &ticket(vec![buy("1", None)])).unwrap_err();
        assert_eq!(e.code, "trading_halted");
        assert!(e.message.contains("1% (77.29 USDT)"), "{}", e.message);
        let f = Facts { loss: Some(Loss::Known { loss: d("0"), start: d("0"), currency: "USDT".into() }), ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", None)])).is_ok(), "no loss never halts, even on an empty account");
        let f = Facts { loss: Some(Loss::Unknown("no equity".into())), ..facts() };
        assert!(check(&l, &f, &ticket(vec![buy("1", None)])).unwrap_err().message.contains("cannot be checked"));
    }

    #[test]
    fn periods_start_on_the_hour_the_day_and_monday_in_the_users_timezone() {
        use time::macros::datetime;
        // Wednesday 2026-10-28 13:40 UTC is 14:40 in Paris (CET, UTC+1).
        let now = datetime!(2026-10-28 13:40 UTC);
        assert_eq!(period("hour", "Europe/Paris", now), (datetime!(2026-10-28 13:00 UTC), datetime!(2026-10-28 14:00 UTC)));
        assert_eq!(period("day", "Europe/Paris", now), (datetime!(2026-10-27 23:00 UTC), datetime!(2026-10-28 23:00 UTC)));
        assert_eq!(period("week", "Europe/Paris", now), (datetime!(2026-10-25 23:00 UTC), datetime!(2026-11-01 23:00 UTC)));
        assert_eq!(period("day", "nowhere", now).0, datetime!(2026-10-28 0:00 UTC), "an unknown zone counts in UTC");
        let manual = Policy { reset: "manual".into(), password_lift: true, timezone: "UTC".into() };
        assert_eq!(halt_until(&manual, now), None);
    }

    #[test]
    fn limits_are_positive_and_the_collar_is_a_percentage() {
        assert!(validate(&Limits { max_order_qty: Some(d("0")), ..Default::default() }).is_err());
        assert!(validate(&Limits { collar_pct: Some(d("100")), ..Default::default() }).is_err());
        assert!(validate(&Limits { max_open_orders: Some(0), ..Default::default() }).is_err());
        assert!(validate(&Limits { collar_pct: Some(d("1.5")), max_loss: Some(d("250")), ..Default::default() }).is_ok());
        assert!(validate(&Limits { max_loss: Some(d("100")), max_loss_pct: true, ..Default::default() }).is_err());
        assert!(validate(&Limits { max_loss: Some(d("2")), max_loss_pct: true, ..Default::default() }).is_ok());
    }
}
