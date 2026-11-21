//! The columns a chain row derives from its own quote, the underlying's price, a rate and a
//! dividend yield. Pure: nothing here reads a provider.
//!
//! Every column is `None` when an input it needs is missing, never a stand-in value. The
//! model columns (implied volatilities, theoretical price, greeks) read the contract's own
//! exercise style and dividends through [`super::pricing`]; an expired or same-day contract
//! gets its intrinsic value only.

use serde::Serialize;
use time::{Date, OffsetDateTime};

use super::pricing::Model;

/// The market a row is read against.
#[derive(Debug, Clone, Copy)]
pub struct Market<'a> {
    pub spot: Option<f64>,
    /// The day of the read, on the New York calendar.
    pub today: Date,
    pub model: Model<'a>,
}

/// What one contract quotes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quote {
    pub strike: f64,
    pub call: bool,
    /// Calendar days to expiry.
    pub dte: i64,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    /// The implied volatility the provider published, the theoretical price's fallback.
    pub provider_iv: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct Derived {
    pub mid: Option<f64>,
    pub spread_pct: Option<f64>,
    pub theo: Option<f64>,
    pub distance: Option<f64>,
    pub rel_distance: Option<f64>,
    pub bid_pct: Option<f64>,
    pub ask_pct: Option<f64>,
    pub ann_bid_pct: Option<f64>,
    pub ann_ask_pct: Option<f64>,
    pub intrinsic: Option<f64>,
    pub time_value: Option<f64>,
    pub iv_bid: Option<f64>,
    pub iv_ask: Option<f64>,
    pub iv_mid: Option<f64>,
    pub iv_spread: Option<f64>,
    pub breakeven: Option<f64>,
    pub to_be_pct: Option<f64>,
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    /// Per vol point.
    pub vega: Option<f64>,
    /// Per calendar day.
    pub theta: Option<f64>,
    /// Per rate point.
    pub rho: Option<f64>,
}

/// Calendar days from `as_of` to `expiry`, counted on the New York calendar: US options
/// expire on their date there, and an evening read in UTC is already the next day.
pub fn dte(as_of: OffsetDateTime, expiry: Date) -> i64 {
    (expiry - ny_date(as_of)).whole_days()
}

pub fn ny_date(at: OffsetDateTime) -> Date {
    let ms = (at.unix_timestamp_nanos() / 1_000_000) as i64;
    otw_script::stdlib::time::zone("America/New_York")
        .and_then(|z| otw_script::stdlib::time::local(ms, z))
        .map(|t| t.date())
        .unwrap_or_else(|| at.date())
}

/// Intrinsic value at `spot`.
pub fn intrinsic(call: bool, strike: f64, spot: f64) -> f64 {
    if call { (spot - strike).max(0.0) } else { (strike - spot).max(0.0) }
}

/// A price IB or Massive writes for an empty side (-1, NaN) is no price.
fn price(v: Option<f64>) -> Option<f64> {
    v.filter(|p| p.is_finite() && *p >= 0.0)
}

pub fn derive(m: Market<'_>, q: &Quote) -> Derived {
    let spot = m.spot.filter(|s| s.is_finite() && *s > 0.0);
    let (bid, ask) = (price(q.bid), price(q.ask).filter(|a| *a > 0.0));
    // A crossed market is no market: its midpoint prices nothing.
    let two_sided = match (bid, ask) {
        (Some(b), Some(a)) if a >= b => Some((b, a)),
        _ => None,
    };
    let mid = two_sided.map(|(b, a)| (b + a) / 2.0);
    let k = q.strike;
    let mut d = Derived {
        mid,
        spread_pct: two_sided.zip(mid).filter(|(_, m)| *m > 0.0).map(|((b, a), m)| (a - b) / m),
        breakeven: mid.map(|m| if q.call { k + m } else { k - m }),
        ..Derived::default()
    };
    let Some(s) = spot else { return d };
    d.distance = Some(k - s);
    d.rel_distance = Some((k - s) / s);
    d.bid_pct = bid.map(|b| b / s);
    d.ask_pct = ask.map(|a| a / s);
    if q.dte > 0 {
        let ann = 365.0 / q.dte as f64;
        d.ann_bid_pct = d.bid_pct.map(|p| p * ann);
        d.ann_ask_pct = d.ask_pct.map(|p| p * ann);
    }
    let intr = intrinsic(q.call, k, s);
    d.intrinsic = Some(intr);
    d.time_value = mid.map(|m| m - intr);
    d.to_be_pct = d.breakeven.map(|be| (be - s) / s);
    if q.dte <= 0 {
        return d;
    }
    let expiry = m.today + time::Duration::days(q.dte);
    let iv = |p: Option<f64>| p.and_then(|p| m.model.implied_vol(q.call, p, s, k, m.today, expiry));
    d.iv_bid = iv(bid);
    d.iv_ask = iv(ask);
    d.iv_mid = iv(mid);
    d.iv_spread = d.iv_ask.zip(d.iv_bid).map(|(a, b)| a - b);
    let theo_vol = d.iv_mid.or(q.provider_iv.filter(|v| v.is_finite() && *v > 0.0));
    d.theo = theo_vol.and_then(|v| m.model.value(q.call, s, k, m.today, expiry, v));
    if let Some(g) = d.iv_mid.and_then(|v| m.model.greeks(q.call, s, k, m.today, expiry, v)) {
        d.delta = Some(g.delta);
        d.gamma = Some(g.gamma);
        d.vega = Some(g.vega / 100.0);
        d.theta = Some(g.theta / 365.0);
        d.rho = Some(g.rho / 100.0);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::pricing::Style;
    use crate::quant::calc::bsm;
    use time::macros::{date, datetime};

    const MODEL: Model<'static> = Model { style: Style::European, rate: 0.04, dividend: 0.01, dividends: &[] };
    const M: Market<'static> = Market { spot: Some(100.0), today: date!(2026 - 11 - 02), model: MODEL };

    fn quote(call: bool, strike: f64, dte: i64, bid: f64, ask: f64) -> Quote {
        Quote { strike, call, dte, bid: Some(bid), ask: Some(ask), provider_iv: None }
    }

    #[test]
    fn an_american_quote_is_read_with_its_early_exercise_premium() {
        let american = Market { model: Model { style: Style::American, ..MODEL }, ..M };
        let (call, k, dte) = (false, 115.0, 200);
        let expiry = american.today + time::Duration::days(dte);
        let p = american.model.value(call, 100.0, k, american.today, expiry, 0.3).unwrap();
        let am = derive(american, &quote(call, k, dte, p - 0.05, p + 0.05));
        assert!((am.iv_mid.unwrap() - 0.3).abs() < 1e-3, "{:?}", am.iv_mid);
        // The same quote read as European needs a higher volatility to reach that price.
        let eu = derive(M, &quote(call, k, dte, p - 0.05, p + 0.05));
        assert!(eu.iv_mid.unwrap() > am.iv_mid.unwrap() + 0.005, "{:?} {:?}", eu.iv_mid, am.iv_mid);
    }

    #[test]
    fn the_mid_iv_recovers_the_vol_that_priced_the_quote() {
        let t = 45.0 / 365.0;
        for (call, k) in [(true, 105.0), (false, 95.0), (true, 100.0)] {
            let p = bsm(call, 100.0, k, t, MODEL.rate, MODEL.dividend, 0.27).unwrap().price;
            let d = derive(M, &quote(call, k, 45, p - 0.05, p + 0.05));
            assert!((d.iv_mid.unwrap() - 0.27).abs() < 1e-6, "{call} {k}: {:?}", d.iv_mid);
            assert!(d.iv_bid.unwrap() < 0.27 && d.iv_ask.unwrap() > 0.27);
            assert!((d.iv_spread.unwrap() - (d.iv_ask.unwrap() - d.iv_bid.unwrap())).abs() < 1e-12);
            assert!((d.theo.unwrap() - p).abs() < 1e-6);
        }
    }

    #[test]
    fn greeks_come_in_trader_units() {
        let t = 30.0 / 365.0;
        let p = bsm(true, 100.0, 100.0, t, MODEL.rate, MODEL.dividend, 0.2).unwrap();
        let d = derive(M, &quote(true, 100.0, 30, p.price - 0.02, p.price + 0.02));
        let g = bsm(true, 100.0, 100.0, t, MODEL.rate, MODEL.dividend, d.iv_mid.unwrap()).unwrap();
        assert!((d.vega.unwrap() - g.vega / 100.0).abs() < 1e-12);
        assert!((d.theta.unwrap() - g.theta / 365.0).abs() < 1e-12);
        assert!((d.rho.unwrap() - g.rho / 100.0).abs() < 1e-12);
        assert!((d.delta.unwrap() - g.delta).abs() < 1e-12);
        // An ATM month: about a dime per vol point, a few cents a day.
        assert!((0.05..0.2).contains(&d.vega.unwrap()));
        assert!((-0.1..0.0).contains(&d.theta.unwrap()));
    }

    #[test]
    fn intrinsic_and_time_value_split_the_mid() {
        // An ITM call at 7 and a half over a 95 strike with spot 100: 5 intrinsic, 2.5 time.
        let d = derive(M, &quote(true, 95.0, 20, 7.4, 7.6));
        assert_eq!(d.intrinsic, Some(5.0));
        assert!((d.time_value.unwrap() - 2.5).abs() < 1e-12);
        assert_eq!(d.breakeven, Some(102.5));
        assert!((d.to_be_pct.unwrap() - 0.025).abs() < 1e-12);
        assert_eq!(d.distance, Some(-5.0));
        let put = derive(M, &quote(false, 95.0, 20, 0.9, 1.1));
        assert_eq!(put.intrinsic, Some(0.0));
        assert!((put.time_value.unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(put.breakeven, Some(94.0));
        assert!((put.spread_pct.unwrap() - 0.2).abs() < 1e-12);
        assert!((put.ann_bid_pct.unwrap() - 0.009 * 365.0 / 20.0).abs() < 1e-12);
    }

    #[test]
    fn a_missing_input_leaves_its_columns_empty() {
        let no_spot = derive(Market { spot: None, ..M }, &quote(true, 100.0, 30, 1.0, 1.2));
        assert!(no_spot.mid.is_some() && no_spot.intrinsic.is_none() && no_spot.iv_mid.is_none());
        let one_sided = derive(M, &Quote { bid: None, ..quote(true, 100.0, 30, 0.0, 1.2) });
        assert!(one_sided.mid.is_none() && one_sided.iv_mid.is_none() && one_sided.delta.is_none());
        assert!(one_sided.iv_ask.is_some(), "the ask alone still has its own vol");
        let crossed = derive(M, &quote(true, 100.0, 30, 1.3, 1.2));
        assert!(crossed.mid.is_none());
        // IB's -1 for an empty side.
        let empty = derive(M, &quote(true, 100.0, 30, -1.0, -1.0));
        assert!(empty.bid_pct.is_none() && empty.ask_pct.is_none());
    }

    #[test]
    fn an_expiring_contract_is_intrinsic_only() {
        let d = derive(M, &Quote { provider_iv: Some(0.3), ..quote(false, 110.0, 0, 9.9, 10.1) });
        assert_eq!(d.intrinsic, Some(10.0));
        assert!(d.iv_mid.is_none() && d.theo.is_none() && d.delta.is_none() && d.ann_bid_pct.is_none());
    }

    #[test]
    fn the_provider_vol_prices_the_theo_only_when_ours_is_missing() {
        let d = derive(M, &Quote { bid: None, provider_iv: Some(0.25), ..quote(true, 100.0, 30, 0.0, 3.0) });
        let want = bsm(true, 100.0, 100.0, 30.0 / 365.0, MODEL.rate, MODEL.dividend, 0.25).unwrap().price;
        assert!((d.theo.unwrap() - want).abs() < 1e-12);
        assert!(d.delta.is_none(), "greeks are ours or none");
    }

    #[test]
    fn days_to_expiry_count_on_the_new_york_calendar() {
        // 02:00 UTC on the 16th is still the evening of the 15th in New York.
        assert_eq!(dte(datetime!(2026-10-16 02:00 UTC), date!(2026 - 10 - 16)), 1);
        assert_eq!(dte(datetime!(2026-10-16 15:00 UTC), date!(2026 - 10 - 16)), 0);
        assert_eq!(dte(datetime!(2026-10-16 15:00 UTC), date!(2026 - 11 - 20)), 35);
    }
}
