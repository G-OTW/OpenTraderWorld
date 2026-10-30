//! The ticket a trade on alert sends, written before the price is known and materialized into
//! an order at the price that fires the alert.
//!
//! Pure functions over decimals: no I/O, every refusal one sentence naming the fix.
//!
//! - **Prices** are distances from the trigger price: `abs` (a price amount), `pct` (percent of
//!   the trigger), `ticks` (the instrument's tick, as the broker states it) or `atr` (the
//!   instrument's ATR 14 over its stored bars). An entry's limit and stop are signed (above
//!   the trigger is positive); a take profit and a stop loss are positive distances from the
//!   entry, each on its own side. Every computed price is put on the tick grid, to the nearest
//!   tick.
//! - **Sizes** are `units`, a `quote_amount` (money, over the entry price and the point value)
//!   or a `risk_amount` (money lost at the stop loss). A size worked out is snapped down to the
//!   venue's size step and refused under its minimum, never rounded up; a size typed in units
//!   is refused off the step, never changed.

use otw_connector_model::{Decimal, OrderType, Side, TifKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::brokers::{Grid, OrderLeg, OrderTicket};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeKind {
    Units,
    QuoteAmount,
    RiskAmount,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub kind: SizeKind,
    pub value: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OffsetKind {
    Abs,
    Pct,
    Ticks,
    Atr,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offset {
    pub kind: OffsetKind,
    pub value: Decimal,
}

/// The entry types a trade on alert sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    Market,
    Limit,
    Stop,
    StopLimit,
}

impl EntryType {
    pub fn order_type(self) -> OrderType {
        match self {
            EntryType::Market => OrderType::Market,
            EntryType::Limit => OrderType::Limit,
            EntryType::Stop => OrderType::Stop,
            EntryType::StopLimit => OrderType::StopLimit,
        }
    }
}

/// The ticket as armed. An `exit` trade carries none of it: it closes what is held.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketTemplate {
    #[serde(default)]
    pub side: Option<Side>,
    #[serde(default)]
    pub order_type: Option<EntryType>,
    #[serde(default)]
    pub tif: Option<TifKind>,
    #[serde(default)]
    pub reduce_only: bool,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub limit_offset: Option<Offset>,
    #[serde(default)]
    pub stop_offset: Option<Offset>,
    #[serde(default)]
    pub take_profit: Option<Offset>,
    #[serde(default)]
    pub stop_loss: Option<Offset>,
}

/// Where a trade on alert goes: into a position, or out of the one held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Enter,
    Exit,
}

impl Intent {
    pub fn parse(s: &str) -> Result<Intent, String> {
        match s {
            "enter" => Ok(Intent::Enter),
            "exit" => Ok(Intent::Exit),
            _ => Err("the intent must be enter or exit".into()),
        }
    }
}

pub const FALLBACKS: &[&str] = &["none", "cancel_symbol", "close_symbol", "flatten_account"];

/// Asset types traded in whole units when the broker states no size step (the chart's rule).
fn whole_units(asset_type: &str) -> bool {
    matches!(asset_type.to_ascii_lowercase().as_str(), "future" | "futures" | "option" | "options" | "stock" | "etf" | "fund")
}

/// Asset types whose size counts contracts, worth a point value each.
pub fn contracts(asset_type: &str) -> bool {
    matches!(asset_type.to_ascii_lowercase().as_str(), "future" | "futures" | "option" | "options")
}

impl TicketTemplate {
    /// Check the template on its own, for `intent`. An exit template is emptied: it closes
    /// what is held, whatever was typed.
    pub fn validate(mut self, intent: Intent) -> Result<TicketTemplate, String> {
        if intent == Intent::Exit {
            return Ok(TicketTemplate::default());
        }
        if self.side.is_none() {
            return Err("pick a side, buy or sell".into());
        }
        let Some(ty) = self.order_type else { return Err("pick an order type".into()) };
        let Some(size) = self.size else { return Err("give the order a size".into()) };
        if size.value <= Decimal::ZERO {
            return Err("the size must be more than zero".into());
        }
        if size.kind == SizeKind::RiskAmount && self.stop_loss.is_none() {
            return Err("a size from the money at risk needs a stop loss to measure it from".into());
        }
        let (needs_limit, needs_stop) = match ty {
            EntryType::Market => (false, false),
            EntryType::Limit => (true, false),
            EntryType::Stop => (false, true),
            EntryType::StopLimit => (true, true),
        };
        if needs_limit != self.limit_offset.is_some() {
            return Err(if needs_limit { "a limit order needs its limit price as a distance from the trigger" } else { "this order type takes no limit price" }.into());
        }
        if needs_stop != self.stop_offset.is_some() {
            return Err(if needs_stop { "a stop order needs its stop price as a distance from the trigger" } else { "this order type takes no stop price" }.into());
        }
        if self.tif == Some(TifKind::Gtd) {
            return Err("good till date needs a date, which a trade armed for later cannot know: use day or good till cancelled".into());
        }
        for (o, what, signed) in [
            (self.limit_offset, "limit", true),
            (self.stop_offset, "stop", true),
            (self.take_profit, "take profit", false),
            (self.stop_loss, "stop loss", false),
        ] {
            let Some(o) = o else { continue };
            if !signed && o.value <= Decimal::ZERO {
                return Err(format!("the {what} distance must be more than zero"));
            }
            if o.kind == OffsetKind::Pct && o.value.abs() >= Decimal::ONE_HUNDRED {
                return Err(format!("the {what} distance must be under 100%"));
            }
            if o.kind == OffsetKind::Ticks && !o.value.fract().is_zero() {
                return Err(format!("the {what} distance in ticks must be a whole number"));
            }
        }
        if self.tif.is_none() {
            self.tif = Some(if ty == EntryType::Market { TifKind::Day } else { TifKind::Gtc });
        }
        Ok(self)
    }

    /// Whether materializing it reads the instrument's ATR.
    pub fn uses_atr(&self) -> bool {
        [self.limit_offset, self.stop_offset, self.take_profit, self.stop_loss].iter().flatten().any(|o| o.kind == OffsetKind::Atr)
    }

    /// Whether materializing it needs the broker's grid: a price or a size worked out.
    pub fn needs_grid(&self) -> bool {
        [self.limit_offset, self.stop_offset, self.take_profit, self.stop_loss].iter().any(Option::is_some)
            || self.size.is_some_and(|s| s.kind != SizeKind::Units)
    }

    /// Whether the size reads the point value (a size from money on contracts).
    pub fn uses_point_value(&self, asset_type: &str) -> bool {
        contracts(asset_type) && self.size.is_some_and(|s| s.kind != SizeKind::Units)
    }
}

/// What materializing needs besides the template: the price that fired, the instrument, and
/// what the broker states about it.
pub struct Cx<'a> {
    pub ticker: &'a str,
    pub asset_type: &'a str,
    pub provider: Option<String>,
    pub connector: Option<Uuid>,
    /// The price that fired the alert, and when, epoch ms.
    pub trigger: Decimal,
    pub at_ms: i64,
    /// The broker's grid; `None` when it states none.
    pub grid: Option<&'a Grid>,
    /// Units per contract, when known.
    pub point_value: Option<Decimal>,
    /// ATR 14 over the stored bars, or why it could not be read.
    pub atr: Result<Decimal, String>,
    /// The broker takes reduce-only exits.
    pub reduce_only_ok: bool,
    /// The time in force the exits rest under.
    pub exit_tif: TifKind,
    /// The client id the entry goes under; its exits add `t` and `s`.
    pub key: &'a str,
}

/// A template made an order: the ticket, and the figures the notification and the preview say.
#[derive(Debug, Clone, Serialize)]
pub struct Materialized {
    pub ticket: OrderTicket,
    pub side: Side,
    pub qty: Decimal,
    pub entry: Decimal,
    pub limit: Option<Decimal>,
    pub stop: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub stop_loss: Option<Decimal>,
}

/// Down to a multiple of `step` (never up).
fn floor_to(v: Decimal, step: Decimal) -> Decimal {
    ((v / step).floor() * step).normalize()
}

/// To the nearest multiple of `step`, half away from zero.
fn nearest(v: Decimal, step: Decimal) -> Decimal {
    let n = v / step;
    let r = if n.is_sign_negative() { -((-n) + Decimal::new(5, 1)).floor() } else { (n + Decimal::new(5, 1)).floor() };
    (r * step).normalize()
}

/// Eight decimals, down: what a size or a price comes to when the broker states no grid.
fn eight(v: Decimal) -> Decimal {
    floor_to(v, Decimal::new(1, 8))
}

fn dist(o: Offset, base: Decimal, cx: &Cx<'_>, what: &str) -> Result<Decimal, String> {
    Ok(match o.kind {
        OffsetKind::Abs => o.value,
        OffsetKind::Pct => base * o.value / Decimal::ONE_HUNDRED,
        OffsetKind::Ticks => {
            let tick = cx.grid.and_then(|g| g.tick_at(base)).ok_or_else(|| {
                format!("the {what} is in ticks, and the broker states no tick for {}: give it as a price amount or a percentage", cx.ticker)
            })?;
            o.value * tick
        }
        OffsetKind::Atr => o.value * cx.atr.clone()?,
    })
}

/// A computed price on the tick grid, refused at or under zero.
fn on_tick(p: Decimal, cx: &Cx<'_>, what: &str) -> Result<Decimal, String> {
    let p = match cx.grid.and_then(|g| g.tick_at(p)) {
        Some(t) => nearest(p, t),
        None => nearest(p, Decimal::new(1, 8)),
    };
    if p <= Decimal::ZERO {
        return Err(format!("the {what} worked out from the trigger {} is {p}, not a price: move it closer", cx.trigger.normalize()));
    }
    Ok(p)
}

/// The template as an order at `cx.trigger`.
pub fn materialize(t: &TicketTemplate, cx: &Cx<'_>) -> Result<Materialized, String> {
    let side = t.side.ok_or("pick a side, buy or sell")?;
    let ty = t.order_type.ok_or("pick an order type")?;
    let size = t.size.ok_or("give the order a size")?;
    if cx.trigger <= Decimal::ZERO {
        return Err("the price that fired the alert is not a price, so nothing was sent".into());
    }
    let limit = t.limit_offset.map(|o| dist(o, cx.trigger, cx, "limit price").and_then(|d| on_tick(cx.trigger + d, cx, "limit price"))).transpose()?;
    let stop = t.stop_offset.map(|o| dist(o, cx.trigger, cx, "stop price").and_then(|d| on_tick(cx.trigger + d, cx, "stop price"))).transpose()?;
    let entry = match ty {
        EntryType::Market => cx.trigger,
        EntryType::Limit | EntryType::StopLimit => limit.ok_or("a limit order needs its limit price")?,
        EntryType::Stop => stop.ok_or("a stop order needs its stop price")?,
    };
    let dir = if side == Side::Buy { Decimal::ONE } else { -Decimal::ONE };
    let take_profit = t.take_profit.map(|o| dist(o, entry, cx, "take profit").and_then(|d| on_tick(entry + d * dir, cx, "take profit"))).transpose()?;
    let stop_loss = t.stop_loss.map(|o| dist(o, entry, cx, "stop loss").and_then(|d| on_tick(entry - d * dir, cx, "stop loss"))).transpose()?;

    let market = ty == EntryType::Market;
    let grid = cx.grid.cloned().unwrap_or_default();
    let step = if market { grid.market_step.or(grid.lot_step) } else { grid.lot_step };
    let min = if market { grid.market_min_qty.or(grid.min_qty) } else { grid.min_qty };
    let whole = whole_units(cx.asset_type);
    let qty = match size.kind {
        SizeKind::Units => {
            let q = size.value;
            if let Some(s) = step {
                if !(q % s).is_zero() {
                    return Err(format!("{} is not a multiple of the size step {} on {}: change the size", q.normalize(), s.normalize(), cx.ticker));
                }
            } else if whole && !q.fract().is_zero() {
                return Err(format!("{} trades in whole units: the size cannot be a fraction", cx.ticker));
            }
            q.normalize()
        }
        SizeKind::QuoteAmount | SizeKind::RiskAmount => {
            let pv = match cx.point_value {
                Some(v) if v > Decimal::ZERO => v,
                _ if contracts(cx.asset_type) => {
                    return Err(format!("a size from an amount needs {}'s point value: set it in the chart's instrument info", cx.ticker));
                }
                _ => Decimal::ONE,
            };
            let per = match size.kind {
                SizeKind::QuoteAmount => entry * pv,
                _ => {
                    let sl = stop_loss.ok_or("a size from the money at risk needs a stop loss to measure it from")?;
                    (entry - sl).abs() * pv
                }
            };
            if per <= Decimal::ZERO {
                return Err("the size cannot be worked out: the stop loss sits on the entry".into());
            }
            let raw = size.value / per;
            let q = match step {
                Some(s) => floor_to(raw, s),
                None if whole => raw.floor(),
                None => eight(raw),
            };
            if q <= Decimal::ZERO {
                return Err(format!(
                    "{} at {} buys less than one size step of {}: raise the amount",
                    size.value.normalize(),
                    entry.normalize(),
                    cx.ticker
                ));
            }
            q.normalize()
        }
    };
    if let Some(m) = min {
        if qty < m {
            return Err(format!("the size {} is under the minimum {} of {}: raise the amount", qty, m.normalize(), cx.ticker));
        }
    }

    let tif = t.tif.unwrap_or(if market { TifKind::Day } else { TifKind::Gtc });
    let mut entry_leg = OrderLeg::new(cx.key, side, ty.order_type(), qty);
    entry_leg.limit = limit.filter(|_| matches!(ty, EntryType::Limit | EntryType::StopLimit));
    entry_leg.stop = stop.filter(|_| matches!(ty, EntryType::Stop | EntryType::StopLimit));
    entry_leg.tif = Some(tif);
    entry_leg.reduce_only = t.reduce_only;
    let mut legs = vec![entry_leg];
    let group = (take_profit.is_some() && stop_loss.is_some()).then(|| format!("{}g", cx.key));
    let exit_side = if side == Side::Buy { Side::Sell } else { Side::Buy };
    let exit = |suffix: &str, ty: OrderType| {
        let mut l = OrderLeg::new(format!("{}{suffix}", cx.key), exit_side, ty, qty);
        l.parent = Some(cx.key.to_string());
        l.group = group.clone();
        l.tif = Some(cx.exit_tif);
        l.reduce_only = cx.reduce_only_ok;
        l
    };
    if let Some(p) = take_profit {
        let mut l = exit("t", OrderType::Limit);
        l.limit = Some(p);
        legs.push(l);
    }
    if let Some(p) = stop_loss {
        let mut l = exit("s", OrderType::Stop);
        l.stop = Some(p);
        legs.push(l);
    }
    let ticket = OrderTicket {
        ticker: cx.ticker.to_string(),
        asset_type: cx.asset_type.to_string(),
        legs,
        provider: cx.provider.clone(),
        connector: cx.connector,
        reference: Some(cx.trigger),
        reference_at: Some(cx.at_ms),
        confirm_duplicate: false,
    };
    Ok(Materialized { ticket, side, qty, entry: entry.normalize(), limit: entry_leg_price(limit, ty, true), stop: entry_leg_price(stop, ty, false), take_profit, stop_loss })
}

fn entry_leg_price(p: Option<Decimal>, ty: EntryType, limit: bool) -> Option<Decimal> {
    let carries = if limit { matches!(ty, EntryType::Limit | EntryType::StopLimit) } else { matches!(ty, EntryType::Stop | EntryType::StopLimit) };
    p.filter(|_| carries)
}

/// A one-line account of an order as sent: `buy 0.002 BTCUSDT limit 64000, take profit 65000`.
pub fn describe(m: &Materialized, ticker: &str, ty: Option<EntryType>) -> String {
    let side = if m.side == Side::Buy { "buy" } else { "sell" };
    let mut s = format!("{side} {} {ticker}", m.qty);
    if ty.is_none_or(|t| t == EntryType::Market) {
        s.push_str(" at the market");
    }
    if let Some(p) = m.stop {
        s.push_str(&format!(" stop {}", p.normalize()));
    }
    if let Some(p) = m.limit {
        s.push_str(&format!(" limit {}", p.normalize()));
    }
    if let Some(p) = m.take_profit {
        s.push_str(&format!(", take profit {}", p.normalize()));
    }
    if let Some(p) = m.stop_loss {
        s.push_str(&format!(", stop loss {}", p.normalize()));
    }
    s
}

/// The fallback that may run after an outcome: `Unknown` never adds exposure next to an order
/// that may be working, so whatever was chosen, it only cancels.
pub fn fallback_for(chosen: &str, unknown: bool) -> &str {
    match chosen {
        "none" => "none",
        _ if unknown => "cancel_symbol",
        other => other,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn off(kind: OffsetKind, v: &str) -> Option<Offset> {
        Some(Offset { kind, value: d(v) })
    }

    fn tpl(ty: EntryType, size: SizeKind, v: &str) -> TicketTemplate {
        TicketTemplate { side: Some(Side::Buy), order_type: Some(ty), size: Some(Size { kind: size, value: d(v) }), ..Default::default() }
    }

    fn grid() -> Grid {
        Grid { tick: Some(d("0.01")), lot_step: Some(d("0.00001")), min_qty: Some(d("0.0001")), ..Default::default() }
    }

    fn cx<'a>(g: Option<&'a Grid>, asset: &'a str) -> Cx<'a> {
        Cx {
            ticker: "BTCUSDT",
            asset_type: asset,
            provider: Some("binance".into()),
            connector: None,
            trigger: d("64000"),
            at_ms: 1,
            grid: g,
            point_value: None,
            atr: Err("ATR offsets read the stored bars of BTCUSDT 1h, and none are stored: download its history, or use another distance".into()),
            reduce_only_ok: false,
            exit_tif: TifKind::Gtc,
            key: "atk",
        }
    }

    #[test]
    fn a_market_order_in_units_goes_out_as_typed_with_the_price_that_fired() {
        let g = grid();
        let m = materialize(&tpl(EntryType::Market, SizeKind::Units, "0.002").validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!(m.qty, d("0.002"));
        assert_eq!(m.ticket.legs.len(), 1);
        assert_eq!(m.ticket.legs[0].order_type, OrderType::Market);
        assert_eq!(m.ticket.legs[0].tif, Some(TifKind::Day));
        assert_eq!((m.ticket.reference, m.ticket.reference_at), (Some(d("64000")), Some(1)));
    }

    #[test]
    fn each_offset_kind_measures_from_the_trigger_and_lands_on_the_tick() {
        let g = grid();
        let mut c = cx(Some(&g), "crypto");
        c.atr = Ok(d("150.123"));
        for (kind, v, want) in [
            (OffsetKind::Abs, "-100", "63900"),
            (OffsetKind::Pct, "-0.5", "63680"),
            (OffsetKind::Ticks, "-25", "63999.75"),
            (OffsetKind::Atr, "-2", "63699.75"),
        ] {
            let mut t = tpl(EntryType::Limit, SizeKind::Units, "0.001");
            t.limit_offset = off(kind, v);
            let m = materialize(&t.validate(Intent::Enter).unwrap(), &c).unwrap();
            assert_eq!(m.limit, Some(d(want)), "{kind:?}");
            assert_eq!(m.ticket.legs[0].limit, Some(d(want)));
        }
    }

    #[test]
    fn exits_sit_on_their_own_side_of_the_entry() {
        let g = grid();
        let mut t = tpl(EntryType::Limit, SizeKind::Units, "0.001");
        t.limit_offset = off(OffsetKind::Abs, "-1000");
        t.take_profit = off(OffsetKind::Pct, "2");
        t.stop_loss = off(OffsetKind::Abs, "500");
        let m = materialize(&t.clone().validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!((m.entry, m.take_profit, m.stop_loss), (d("63000"), Some(d("64260")), Some(d("62500"))));
        let legs = &m.ticket.legs;
        assert_eq!(legs.len(), 3);
        assert_eq!((legs[1].side, legs[1].parent.as_deref(), legs[1].group.as_deref()), (Side::Sell, Some("atk"), Some("atkg")));
        assert_eq!((legs[1].key.as_str(), legs[2].key.as_str()), ("atkt", "atks"));
        // A sell's exits are mirrored.
        t.side = Some(Side::Sell);
        t.limit_offset = off(OffsetKind::Abs, "1000");
        let m = materialize(&t.validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!((m.entry, m.take_profit, m.stop_loss), (d("65000"), Some(d("63700")), Some(d("65500"))));
    }

    #[test]
    fn a_size_from_money_is_snapped_down_to_the_step_never_up() {
        let g = grid();
        // 100 / 64000 = 0.0015625 -> 0.00156 on a 0.00001 step.
        let m = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "100").validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!(m.qty, d("0.00156"));
        // Risk: 50 lost over a 500 stop is 0.1.
        let mut t = tpl(EntryType::Market, SizeKind::RiskAmount, "50");
        t.stop_loss = off(OffsetKind::Abs, "500");
        let m = materialize(&t.validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!(m.qty, d("0.1"));
    }

    #[test]
    fn a_size_under_the_minimum_is_refused_not_raised() {
        let g = grid();
        // 5 / 64000 = 0.000078 -> 0.00007, under the 0.0001 minimum.
        let e = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "5").validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap_err();
        assert!(e.contains("under the minimum 0.0001"), "{e}");
        // Less than one step at all.
        let e = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "0.1").validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap_err();
        assert!(e.contains("less than one size step"), "{e}");
        // Typed units off the step are refused, never changed.
        let e = materialize(&tpl(EntryType::Market, SizeKind::Units, "0.000015").validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap_err();
        assert!(e.contains("not a multiple of the size step"), "{e}");
    }

    #[test]
    fn a_market_order_reads_the_market_size_grid_where_the_venue_has_one() {
        let g = Grid { market_step: Some(d("0.001")), market_min_qty: Some(d("0.001")), ..grid() };
        let m = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "100").validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!(m.qty, d("0.001"));
        let mut limit = tpl(EntryType::Limit, SizeKind::QuoteAmount, "100");
        limit.limit_offset = off(OffsetKind::Abs, "0");
        let m = materialize(&limit.validate(Intent::Enter).unwrap(), &cx(Some(&g), "crypto")).unwrap();
        assert_eq!(m.qty, d("0.00156"));
    }

    #[test]
    fn contracts_need_their_point_value_and_trade_whole() {
        let mut c = cx(None, "future");
        let e = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "100000").validate(Intent::Enter).unwrap(), &c).unwrap_err();
        assert!(e.contains("point value"), "{e}");
        c.point_value = Some(d("5"));
        // 100000 / (64000 * 5) = 0.3125 -> 0 whole contracts.
        let e = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "100000").validate(Intent::Enter).unwrap(), &c).unwrap_err();
        assert!(e.contains("less than one size step"), "{e}");
        let m = materialize(&tpl(EntryType::Market, SizeKind::QuoteAmount, "700000").validate(Intent::Enter).unwrap(), &c).unwrap();
        assert_eq!(m.qty, d("2"));
        let e = materialize(&tpl(EntryType::Market, SizeKind::Units, "1.5").validate(Intent::Enter).unwrap(), &c).unwrap_err();
        assert!(e.contains("whole units"), "{e}");
    }

    #[test]
    fn what_cannot_be_measured_is_refused_never_guessed() {
        // Ticks with no tick stated.
        let mut t = tpl(EntryType::Limit, SizeKind::Units, "1");
        t.limit_offset = off(OffsetKind::Ticks, "-3");
        let e = materialize(&t.validate(Intent::Enter).unwrap(), &cx(None, "crypto")).unwrap_err();
        assert!(e.contains("states no tick"), "{e}");
        // ATR with no stored bars.
        let mut t = tpl(EntryType::Market, SizeKind::Units, "1");
        t.stop_loss = off(OffsetKind::Atr, "1");
        let e = materialize(&t.validate(Intent::Enter).unwrap(), &cx(None, "crypto")).unwrap_err();
        assert!(e.contains("stored bars"), "{e}");
        // A price pushed under zero.
        let mut t = tpl(EntryType::Limit, SizeKind::Units, "1");
        t.limit_offset = off(OffsetKind::Abs, "-70000");
        let e = materialize(&t.validate(Intent::Enter).unwrap(), &cx(None, "crypto")).unwrap_err();
        assert!(e.contains("not a price"), "{e}");
    }

    #[test]
    fn a_template_is_checked_before_it_is_stored() {
        let bad = |f: &dyn Fn(&mut TicketTemplate)| {
            let mut t = tpl(EntryType::Limit, SizeKind::Units, "1");
            t.limit_offset = off(OffsetKind::Abs, "-1");
            f(&mut t);
            t.validate(Intent::Enter).is_err()
        };
        assert!(!bad(&|_| {}));
        assert!(bad(&|t| t.side = None));
        assert!(bad(&|t| t.limit_offset = None));
        assert!(bad(&|t| t.stop_offset = off(OffsetKind::Abs, "1")));
        assert!(bad(&|t| t.size = Some(Size { kind: SizeKind::Units, value: Decimal::ZERO })));
        assert!(bad(&|t| t.size = Some(Size { kind: SizeKind::RiskAmount, value: Decimal::ONE })));
        assert!(bad(&|t| t.tif = Some(TifKind::Gtd)));
        assert!(bad(&|t| t.take_profit = off(OffsetKind::Abs, "-5")));
        assert!(bad(&|t| t.stop_loss = off(OffsetKind::Pct, "100")));
        assert!(bad(&|t| t.limit_offset = off(OffsetKind::Ticks, "1.5")));
        // An exit carries nothing.
        let t = tpl(EntryType::Limit, SizeKind::Units, "1").validate(Intent::Exit).unwrap();
        assert_eq!(t, TicketTemplate::default());
        // Unknown fields are refused rather than dropped.
        assert!(serde_json::from_str::<TicketTemplate>(r#"{"side":"buy","qty":"1"}"#).is_err());
    }

    #[test]
    fn an_unknown_outcome_only_ever_cancels() {
        for chosen in FALLBACKS {
            let after_unknown = fallback_for(chosen, true);
            assert!(after_unknown == "none" || after_unknown == "cancel_symbol", "{chosen} -> {after_unknown}");
            assert_eq!(fallback_for(chosen, false), *chosen);
        }
        assert_eq!(fallback_for("flatten_account", true), "cancel_symbol");
        assert_eq!(fallback_for("none", true), "none");
    }

    #[test]
    fn a_ladder_picks_the_tick_of_the_price_band() {
        let g = Grid { tick: Some(d("0.01")), ladder: vec![(d("0"), d("0.01")), (d("1000"), d("0.25"))], ..Default::default() };
        assert_eq!(g.tick_at(d("999")), Some(d("0.01")));
        assert_eq!(g.tick_at(d("64000")), Some(d("0.25")));
        assert_eq!(nearest(d("64000.13"), d("0.25")), d("64000.25"));
        assert_eq!(nearest(d("64000.12"), d("0.25")), d("64000"));
    }
}
