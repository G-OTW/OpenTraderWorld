//! When a paper order fills, and at what: the rules a venue's matching engine would apply,
//! on the prices the chart's own feed prints. Pure, so every rule is tested without a feed.
//!
//! A buy deals on the ask and a sell on the bid when the feed publishes a book and the account
//! fills on it; otherwise both deal on the last price. Market and stop fills pay the slippage,
//! a resting limit never does and never fills worse than its price.
//!
//! Every price is a [`Decimal`]: a fill at a limit is the limit to the last digit.

#![deny(clippy::float_arithmetic)]

use otw_connector_model::Decimal;

/// What the market did since the order was last looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    /// Lowest and highest price traded since the last look (the last price alone on a quote).
    pub lo: Decimal,
    pub hi: Decimal,
    pub last: Decimal,
    /// The book, when the feed publishes one, it is fresh, and the account fills on it.
    pub book: Option<(Decimal, Decimal)>,
}

impl Tick {
    /// The price a buy (or sell) deals at right now.
    fn deal(&self, buy: bool) -> Decimal {
        match self.book {
            Some((bid, ask)) => {
                if buy {
                    ask
                } else {
                    bid
                }
            }
            None => self.last,
        }
    }

    /// Whether a buy (or sell) reached `px`: the ask came down to it (the bid up to it), or
    /// with no book, the market traded there.
    fn reached(&self, buy: bool, px: Decimal) -> bool {
        match self.book {
            Some((bid, ask)) => {
                if buy {
                    ask <= px
                } else {
                    bid >= px
                }
            }
            None => {
                if buy {
                    self.lo <= px
                } else {
                    self.hi >= px
                }
            }
        }
    }

    /// Whether a stop to buy (or sell) at `px` was hit: the market came up (down) to it.
    fn stopped(&self, buy: bool, px: Decimal) -> bool {
        match self.book {
            Some((bid, ask)) => {
                if buy {
                    ask >= px
                } else {
                    bid <= px
                }
            }
            None => {
                if buy {
                    self.hi >= px
                } else {
                    self.lo <= px
                }
            }
        }
    }
}

/// How far a trailing stop follows the market.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trail {
    Amount(Decimal),
    /// Percent of the price (1.5 = 1.5%).
    Percent(Decimal),
}

impl Trail {
    /// The stop `best` puts it at: below the best price for a sell, above it for a buy.
    fn level(self, buy: bool, best: Decimal) -> Decimal {
        let d = match self {
            Trail::Amount(a) => a,
            Trail::Percent(p) => best * p / Decimal::ONE_HUNDRED,
        };
        if buy {
            best + d
        } else {
            best - d
        }
    }
}

/// The order, as matching sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resting<'a> {
    pub buy: bool,
    /// market | limit | stop | stop_limit | trailing_stop | mit | lit
    pub kind: &'a str,
    pub limit: Option<Decimal>,
    /// The trigger: a stop's, an if-touched order's, or the price a trailing stop starts at.
    pub stop: Option<Decimal>,
    /// A stop-limit or limit-if-touched past its trigger, or a trailing stop started.
    pub triggered: bool,
    pub trail: Option<Trail>,
    /// A trailing stop's best price since it started: the highest behind a sell, the lowest
    /// behind a buy.
    pub best: Option<Decimal>,
}

/// What a tick does to an order.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Decision {
    /// Its trigger was reached on this tick: a stop-limit or limit-if-touched rests as its
    /// limit from now on, a trailing stop starts trailing.
    pub trigger: bool,
    /// A trailing stop's new best price, to keep.
    pub best: Option<Decimal>,
    /// The price it fills at, slippage included.
    pub fill: Option<Decimal>,
}

/// `px` made worse by `bps` basis points for the side that pays it.
pub fn slip(px: Decimal, buy: bool, bps: Decimal) -> Decimal {
    let d = px * bps.max(Decimal::ZERO) / Decimal::from(10_000);
    if buy {
        px + d
    } else {
        px - d
    }
}

/// The limit a resting order fills at once the market reached it: its own price, or better
/// when the market is already through it.
fn at_limit(t: &Tick, buy: bool, limit: Decimal) -> Decimal {
    let deal = t.deal(buy);
    if buy {
        deal.min(limit)
    } else {
        deal.max(limit)
    }
}

/// A stop's fill once hit: where the market is through a gap, never at a price that did not
/// trade; touched and back, at the stop.
fn stop_fill(t: &Tick, buy: bool, s: Decimal, slippage_bps: Decimal) -> Decimal {
    let deal = t.deal(buy);
    let base = if buy { deal.max(s) } else { deal.min(s) };
    slip(base, buy, slippage_bps)
}

pub fn decide(o: &Resting, t: &Tick, slippage_bps: Decimal) -> Decision {
    let buy = o.buy;
    match o.kind {
        "market" => Decision { fill: Some(slip(t.deal(buy), buy, slippage_bps)), ..Default::default() },
        "limit" => {
            let Some(l) = o.limit else { return Decision::default() };
            Decision { fill: t.reached(buy, l).then(|| at_limit(t, buy, l)), ..Default::default() }
        }
        "stop" => {
            let Some(s) = o.stop else { return Decision::default() };
            if !t.stopped(buy, s) {
                return Decision::default();
            }
            Decision { fill: Some(stop_fill(t, buy, s, slippage_bps)), ..Default::default() }
        }
        "stop_limit" => {
            let (Some(s), Some(l)) = (o.stop, o.limit) else { return Decision::default() };
            let trigger = !o.triggered && t.stopped(buy, s);
            if !(o.triggered || trigger) {
                return Decision::default();
            }
            Decision { trigger, fill: t.reached(buy, l).then(|| at_limit(t, buy, l)), ..Default::default() }
        }
        // If touched: the market came to the trigger from the side a limit waits on, then a
        // market order (MIT) or a limit (LIT).
        "mit" => {
            let Some(s) = o.stop else { return Decision::default() };
            if !t.reached(buy, s) {
                return Decision::default();
            }
            Decision { fill: Some(slip(t.deal(buy), buy, slippage_bps)), ..Default::default() }
        }
        "lit" => {
            let (Some(s), Some(l)) = (o.stop, o.limit) else { return Decision::default() };
            let trigger = !o.triggered && t.reached(buy, s);
            if !(o.triggered || trigger) {
                return Decision::default();
            }
            Decision { trigger, fill: t.reached(buy, l).then(|| at_limit(t, buy, l)), ..Default::default() }
        }
        "trailing_stop" => trailing(o, t, slippage_bps),
        _ => Decision::default(),
    }
}

/// A trailing stop. Not started (a start price not reached yet), it waits for the market to
/// come to that price from the side the stop protects. Started, its stop sits the trail away
/// from the best price since: hit against the stop the last best set, then the best moves on
/// with this tick's extreme and the stop is checked again on the close, since which came first
/// inside the tick is not known.
fn trailing(o: &Resting, t: &Tick, slippage_bps: Decimal) -> Decision {
    let buy = o.buy;
    let Some(trail) = o.trail else { return Decision::default() };
    let mut d = Decision::default();
    let started = o.triggered || o.stop.is_none();
    if !started {
        // A sell starts once the market rose to it, a buy once it fell to it.
        let Some(s) = o.stop else { return d };
        if !t.reached(buy, s) {
            return d;
        }
        d.trigger = true;
    }
    let extreme = if !started {
        o.stop.unwrap_or(t.last)
    } else if buy {
        t.lo
    } else {
        t.hi
    };
    if let Some(best) = o.best.filter(|_| started) {
        let level = trail.level(buy, best);
        if t.stopped(buy, level) {
            d.fill = Some(stop_fill(t, buy, level, slippage_bps));
            return d;
        }
    }
    let best = match o.best.filter(|_| started) {
        Some(b) if buy => b.min(extreme),
        Some(b) => b.max(extreme),
        None => extreme,
    };
    d.best = Some(best);
    let level = trail.level(buy, best);
    let close = Tick { lo: t.last, hi: t.last, ..*t };
    if close.stopped(buy, level) {
        d.fill = Some(stop_fill(&close, buy, level, slippage_bps));
    }
    d
}

/// Commission settings, instrument currency.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Commission {
    pub per_order: Decimal,
    pub per_unit: Decimal,
    /// Percent of the notional (0.1 = 0.1%).
    pub pct: Decimal,
    pub min: Decimal,
}

impl Commission {
    /// What one fill of `qty` at `price` costs, `multiplier` points per unit.
    pub fn fee(&self, qty: Decimal, price: Decimal, multiplier: Decimal) -> Decimal {
        let notional = qty * price * multiplier;
        let fee = self.per_order + self.per_unit * qty + self.pct * notional / Decimal::ONE_HUNDRED;
        if fee > Decimal::ZERO || self.min > Decimal::ZERO {
            fee.max(self.min)
        } else {
            Decimal::ZERO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(v: &str) -> Decimal {
        v.parse().unwrap()
    }

    fn bars(lo: Decimal, hi: Decimal, last: Decimal) -> Tick {
        Tick { lo, hi, last, book: None }
    }
    fn book(bid: Decimal, ask: Decimal) -> Tick {
        let mid = (bid + ask) / Decimal::TWO;
        Tick { lo: mid, hi: mid, last: mid, book: Some((bid, ask)) }
    }
    fn order(buy: bool, kind: &str, limit: Option<Decimal>, stop: Option<Decimal>) -> Resting<'_> {
        Resting { buy, kind, limit, stop, triggered: false, trail: None, best: None }
    }

    #[test]
    fn a_market_order_deals_on_the_side_that_pays_with_slippage() {
        let buy = order(true, "market", None, None);
        assert_eq!(decide(&buy, &book(num("99"), num("101")), num("0")).fill, Some(num("101")));
        assert_eq!(decide(&order(false, "market", None, None), &book(num("99"), num("101")), num("0")).fill, Some(num("99")));
        // 10 bps on the last price when there is no book.
        let px = decide(&buy, &bars(num("99"), num("101"), num("100")), num("10")).fill.unwrap_or_default();
        assert_eq!(px, num("100.1"));
    }

    #[test]
    fn a_limit_waits_for_the_market_and_never_fills_worse_than_its_price() {
        let buy = order(true, "limit", Some(num("95")), None);
        assert_eq!(decide(&buy, &bars(num("96"), num("99"), num("98")), num("50")).fill, None);
        // Traded down to it and back up: filled at the limit, no slippage.
        assert_eq!(decide(&buy, &bars(num("94.5"), num("99"), num("98")), num("50")).fill, Some(num("95")));
        // Already through it: the better price.
        assert_eq!(decide(&buy, &book(num("93"), num("94")), num("50")).fill, Some(num("94")));
        // With a book, a print at the limit is not enough: the ask must come down to it.
        let t = Tick { lo: num("95"), hi: num("96"), last: num("95.5"), book: Some((num("95.4"), num("95.6"))) };
        assert_eq!(decide(&buy, &t, num("0")).fill, None);
        let sell = order(false, "limit", Some(num("105")), None);
        assert_eq!(decide(&sell, &bars(num("100"), num("106"), num("101")), num("0")).fill, Some(num("105")));
    }

    #[test]
    fn a_stop_fills_at_the_market_through_a_gap_and_at_its_price_when_touched() {
        let sell = order(false, "stop", None, Some(num("95")));
        assert_eq!(decide(&sell, &bars(num("96"), num("99"), num("97")), num("0")).fill, None);
        assert_eq!(decide(&sell, &bars(num("94"), num("99"), num("96")), num("0")).fill, Some(num("95")));
        // Gapped down to 90: no one sold at 95.
        assert_eq!(decide(&sell, &bars(num("89"), num("91"), num("90")), num("0")).fill, Some(num("90")));
        let buy = order(true, "stop", None, Some(num("105")));
        let px = decide(&buy, &book(num("105"), num("105.5")), num("100")).fill.unwrap_or_default();
        assert_eq!(px, num("106.555"));
    }

    #[test]
    fn a_stop_limit_becomes_a_limit_once_its_stop_is_reached() {
        let mut o = order(true, "stop_limit", Some(num("106")), Some(num("105")));
        assert_eq!(decide(&o, &bars(num("100"), num("104"), num("103")), num("0")), Decision::default());
        // Stop hit and the limit in reach: triggered and filled on the same tick.
        assert_eq!(decide(&o, &bars(num("104"), num("105.5"), num("105.5")), num("0")), Decision { trigger: true, fill: Some(num("105.5")), best: None });
        // Ran away above the limit: triggered, resting.
        assert_eq!(decide(&o, &book(num("107"), num("107.5")), num("0")), Decision { trigger: true, fill: None, best: None });
        o.triggered = true;
        assert_eq!(decide(&o, &book(num("105"), num("105.8")), num("0")), Decision { trigger: false, fill: Some(num("105.8")), best: None });
    }

    #[test]
    fn commission_adds_its_parts_and_respects_its_minimum() {
        let c = Commission { per_order: num("1"), per_unit: num("0.005"), pct: num("0.1"), min: num("0") };
        // 1 + 100 x num("0.005") + num("0.1")% of 10_000.
        assert_eq!(c.fee(num("100"), num("100"), num("1")), num("11.5"));
        let m = Commission { per_unit: num("0.005"), min: num("1"), ..Default::default() };
        assert_eq!(m.fee(num("10"), num("50"), num("1")), num("1"));
        assert_eq!(Commission::default().fee(num("10"), num("50"), num("1")), num("0"));
    }

    #[test]
    fn if_touched_orders_trigger_from_the_limit_side() {
        // A buy MIT below the market: untouched, nothing; touched, a market fill with slippage.
        let mit = order(true, "mit", None, Some(num("95")));
        assert_eq!(decide(&mit, &bars(num("96"), num("99"), num("98")), num("0")).fill, None);
        let px = decide(&mit, &bars(num("94.5"), num("99"), num("96")), num("100")).fill.unwrap_or_default();
        assert_eq!(px, num("96.96"));
        // A sell LIT above: touched, it rests as its limit and fills once reached.
        let mut lit = order(false, "lit", Some(num("104")), Some(num("105")));
        assert_eq!(decide(&lit, &bars(num("100"), num("104.5"), num("104")), num("0")), Decision::default());
        assert_eq!(decide(&lit, &book(num("105"), num("105.2")), num("0")), Decision { trigger: true, fill: Some(num("105")), best: None });
        lit.triggered = true;
        assert_eq!(decide(&lit, &book(num("103"), num("103.2")), num("0")).fill, None);
    }

    #[test]
    fn a_trailing_stop_follows_the_best_price_and_never_backs_off() {
        let mut o = Resting { trail: Some(Trail::Amount(num("2"))), ..order(false, "trailing_stop", None, None) };
        // Started at once: the first tick sets the best.
        let d = decide(&o, &bars(num("100"), num("100"), num("100")), num("0"));
        assert_eq!((d.best, d.fill), (Some(num("100")), None));
        o.best = d.best;
        // Up to 105: the stop climbs to 103; a dip to 104 does not reach it.
        let d = decide(&o, &bars(num("100.5"), num("105"), num("104")), num("0"));
        assert_eq!((d.best, d.fill), (Some(num("105")), None));
        o.best = d.best;
        let d = decide(&o, &bars(num("103.5"), num("104.5"), num("104")), num("0"));
        assert_eq!((d.best, d.fill), (Some(num("105")), None), "the best never falls back");
        // Down through 103: filled like a stop, at the stop or where the market is past it.
        assert_eq!(decide(&o, &bars(num("102.5"), num("104"), num("103.5")), num("0")).fill, Some(num("103")));
        assert_eq!(decide(&o, &bars(num("102.5"), num("104"), num("102.8")), num("0")).fill, Some(num("102.8")));
        assert_eq!(decide(&o, &book(num("99"), num("99.5")), num("0")).fill, Some(num("99")));
        // A buy trails above the lowest price, by a percentage.
        let b = Resting { trail: Some(Trail::Percent(num("1"))), best: Some(num("100")), ..order(true, "trailing_stop", None, None) };
        assert_eq!(decide(&b, &bars(num("99"), num("99.5"), num("99.5")), num("0")).best, Some(num("99")));
        assert_eq!(decide(&b, &bars(num("100.5"), num("101.5"), num("100.8")), num("0")).fill, Some(num("101")));
    }

    #[test]
    fn a_trailing_stop_with_a_start_price_waits_for_it() {
        let o = Resting { trail: Some(Trail::Amount(num("2"))), ..order(false, "trailing_stop", None, Some(num("110"))) };
        assert_eq!(decide(&o, &bars(num("100"), num("109"), num("108")), num("0")), Decision::default());
        let d = decide(&o, &bars(num("108"), num("110.5"), num("110")), num("0"));
        assert!(d.trigger && d.fill.is_none());
        assert_eq!(d.best, Some(num("110")));
    }
}
