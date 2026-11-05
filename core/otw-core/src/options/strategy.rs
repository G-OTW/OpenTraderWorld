//! A multi-leg strategy read across spot: P&L at expiry, at a chosen date and along the way,
//! breakevens, the most it can make and lose, net premium, position greeks, the probability
//! of profit, the margin it ties up and what may be assigned early. Pure: the legs carry their
//! own prices and volatilities.
//!
//! "At expiry" is the nearest option expiry. A leg expiring then is worth its intrinsic
//! value; a later one (calendar, diagonal) is still alive and priced by the model at its own
//! volatility, so the curve is the one a trader closes the position on, and its extremes are
//! estimates.
//!
//! Volatility follows one of the two standard regimes when spot moves: sticky strike (each
//! strike keeps its volatility) or sticky delta (each moneyness does, read off the smile).

use serde::Serialize;
use time::Date;

use super::margin::{self, Margin};
use super::metrics::intrinsic;
use super::pricing::{Model, Style};
use super::MULTIPLIER;
use crate::quant::special::norm_cdf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Call,
    Put,
    Stock,
}

#[derive(Debug, Clone, Copy)]
pub struct Leg {
    pub kind: Kind,
    /// Ignored on a stock leg.
    pub strike: f64,
    /// `None` on a stock leg.
    pub expiry: Option<Date>,
    /// Signed: + long, - short. Contracts for an option, shares for stock.
    pub qty: f64,
    /// Paid per unit (per share for an option, before the multiplier).
    pub price: f64,
    pub iv: Option<f64>,
}

impl Leg {
    fn multiplier(&self) -> f64 {
        if self.kind == Kind::Stock { 1.0 } else { MULTIPLIER }
    }

    /// The volatility this leg is priced at when spot is `spot`: its own plus the shift, moved
    /// along the smile under sticky delta. `None` when it has none.
    fn vol(&self, inp: &Input, spot: f64) -> Option<f64> {
        let v = self.iv? + inp.iv_shift;
        if inp.regime != Regime::StickyDelta || spot <= 0.0 {
            return Some(v);
        }
        let Some(smile) = inp.smiles.iter().find(|m| Some(m.expiry) == self.expiry) else { return Some(v) };
        let (now, then) = (smile.at(self.strike / inp.spot), smile.at(self.strike / spot));
        Some(now.zip(then).map_or(v, |(a, b)| (v + b - a).max(0.01)))
    }

    /// Value per unit at `spot` on `day`: intrinsic once expired, else the model. `None` when a
    /// live option has no volatility to price it with.
    fn value(&self, inp: &Input, spot: f64, day: Date) -> Option<f64> {
        let call = match self.kind {
            Kind::Stock => return Some(spot),
            Kind::Call => true,
            Kind::Put => false,
        };
        let expiry = self.expiry?;
        if (expiry - day).whole_days() <= 0 || spot <= 0.0 {
            return Some(intrinsic(call, self.strike, spot.max(0.0)));
        }
        inp.model.value(call, spot, self.strike, day, expiry, self.vol(inp, spot)?)
    }
}

/// How volatility moves when spot does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regime {
    StickyStrike,
    StickyDelta,
}

/// One expiry's implied volatility by moneyness (strike / spot), sorted.
#[derive(Debug, Clone)]
pub struct Smile {
    pub expiry: Date,
    pub points: Vec<(f64, f64)>,
}

impl Smile {
    /// A smile off raw quotes, smoothed: a median over five strikes drops the odd stale quote,
    /// a mean over three the steps it leaves. Two legs read it at different points, so left raw
    /// its noise comes out as bumps in a spread's curve, past what the spread can ever make.
    pub fn smoothed(expiry: Date, mut raw: Vec<(f64, f64)>) -> Self {
        raw.sort_by(|a, b| a.0.total_cmp(&b.0));
        let around = |i: usize, reach: usize| i.saturating_sub(reach)..(i + reach + 1).min(raw.len());
        let medians: Vec<f64> = (0..raw.len())
            .map(|i| {
                let mut w: Vec<f64> = raw[around(i, 2)].iter().map(|p| p.1).collect();
                w.sort_by(f64::total_cmp);
                w[w.len() / 2]
            })
            .collect();
        let points = (0..raw.len())
            .map(|i| {
                let w = &medians[around(i, 1)];
                (raw[i].0, w.iter().sum::<f64>() / w.len() as f64)
            })
            .collect();
        Smile { expiry, points }
    }

    /// Linear between the points, flat past the ends.
    pub fn at(&self, moneyness: f64) -> Option<f64> {
        let (first, last) = (self.points.first()?, self.points.last()?);
        if moneyness <= first.0 {
            return Some(first.1);
        }
        if moneyness >= last.0 {
            return Some(last.1);
        }
        let i = self.points.partition_point(|p| p.0 < moneyness);
        let ((x1, y1), (x2, y2)) = (self.points[i - 1], self.points[i]);
        Some(y1 + (y2 - y1) * (moneyness - x1) / (x2 - x1))
    }
}

pub struct Input<'a> {
    pub spot: f64,
    pub model: Model<'a>,
    pub legs: &'a [Leg],
    /// The day the position is read from (greeks, POP).
    pub today: Date,
    /// The day of the dashed curve; `None` = the nearest expiry.
    pub at: Option<Date>,
    /// Absolute, added to every leg's volatility (0.05 = five points).
    pub iv_shift: f64,
    /// Volatility for the probability of profit; `None` = the nearest-expiry leg closest to
    /// the money.
    pub atm_iv: Option<f64>,
    pub regime: Regime,
    /// The chain's smiles, by expiry. Empty is fine: sticky delta then reads sticky strike.
    pub smiles: &'a [Smile],
    pub lo: f64,
    pub hi: f64,
    pub steps: usize,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct Point {
    pub spot: f64,
    pub at_expiry: Option<f64>,
    pub at_date: Option<f64>,
}

#[derive(Debug, Serialize, Clone, Copy, Default)]
pub struct PositionGreeks {
    /// In shares: qty × multiplier × per-unit delta.
    pub delta: f64,
    pub gamma: f64,
    /// Per vol point.
    pub vega: f64,
    /// Per calendar day.
    pub theta: f64,
    /// Per rate point.
    pub rho: f64,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    /// The nearest expiry, the day the solid curve is read on.
    pub expiry: Option<String>,
    pub at: Option<String>,
    pub payoff: Vec<Point>,
    pub breakevens: Vec<f64>,
    /// `None` = unbounded.
    pub max_profit: Option<f64>,
    /// Negative or zero; `None` = unbounded.
    pub max_loss: Option<f64>,
    /// Options only, negative = a debit.
    pub net_premium: f64,
    /// `None` when a live leg has no volatility.
    pub greeks: Option<PositionGreeks>,
    pub pop: Option<f64>,
    /// `smile` when each breakeven read its own strike's volatility, `atm` for one volatility.
    pub pop_basis: Option<&'static str>,
    /// Strategy-based margin, an estimate.
    pub margin: Margin,
    /// Curves between today and the nearest expiry, on the same spots as `payoff`.
    pub path: Vec<PathCurve>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Serialize)]
pub struct PathCurve {
    pub date: String,
    pub pnl: Vec<Option<f64>>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Warning {
    /// `early_assignment` (a leg at risk now) or `assignable` (any short American leg).
    pub code: &'static str,
    pub message: String,
}

fn pnl(inp: &Input, spot: f64, day: Date) -> Option<f64> {
    inp.legs.iter().try_fold(0.0, |acc, l| {
        Some(acc + l.qty * l.multiplier() * (l.value(inp, spot, day)? - l.price))
    })
}

fn nearest_expiry(legs: &[Leg]) -> Option<Date> {
    legs.iter().filter_map(|l| l.expiry).min()
}

/// Where a curve sampled at sorted `pts` crosses or touches zero, interpolated linearly
/// between two samples of opposite sign.
pub fn roots(pts: &[(f64, f64)]) -> Vec<f64> {
    let mut out: Vec<f64> = pts.iter().filter(|p| p.1 == 0.0).map(|p| p.0).collect();
    for w in pts.windows(2) {
        let ((x1, y1), (x2, y2)) = (w[0], w[1]);
        if y1 * y2 < 0.0 {
            out.push(x1 + (x2 - x1) * (-y1) / (y2 - y1));
        }
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    out
}

/// The P&L slope as spot runs to infinity: one per share of stock and of call still held
/// then, discounted by the dividend over a call's remaining life. Zero means the side is
/// bounded.
fn upside_slope(inp: &Input, expiry: Date) -> f64 {
    inp.legs
        .iter()
        .map(|l| match l.kind {
            Kind::Stock => l.qty,
            Kind::Call => {
                let left = l.expiry.map_or(0, |e| (e - expiry).whole_days()).max(0) as f64 / 365.0;
                l.qty * MULTIPLIER * (-inp.model.dividend * left).exp()
            }
            Kind::Put => 0.0,
        })
        .sum()
}

pub fn evaluate(inp: &Input) -> Evaluation {
    let expiry = nearest_expiry(inp.legs);
    // A stock-only position has no expiry: its curve is read today.
    let exp_day = expiry.unwrap_or(inp.today);
    let at_day = inp.at.unwrap_or(exp_day);
    let steps = inp.steps.max(2);
    let payoff: Vec<Point> = (0..steps)
        .map(|i| {
            let s = inp.lo + (inp.hi - inp.lo) * i as f64 / (steps - 1) as f64;
            Point { spot: s, at_expiry: pnl(inp, s, exp_day), at_date: pnl(inp, s, at_day) }
        })
        .collect();

    // At expiry the curve bends at the strikes only (exactly so when every leg expires
    // together), so the grid plus the strikes finds its roots and its extremes.
    let strikes: Vec<f64> = inp.legs.iter().filter(|l| l.kind != Kind::Stock).map(|l| l.strike).collect();
    let mut xs: Vec<f64> = payoff.iter().map(|p| p.spot).collect();
    xs.extend(strikes.iter().copied().filter(|k| *k > inp.lo && *k < inp.hi));
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let curve: Option<Vec<(f64, f64)>> = xs.iter().map(|&s| Some((s, pnl(inp, s, exp_day)?))).collect();
    let breakevens = curve.as_deref().map(roots).unwrap_or_default();

    // Extremes over every spot from zero up: zero, each strike, the range; past the highest
    // of them the slope decides.
    let mut probe: Vec<f64> = vec![0.0];
    probe.extend(strikes.iter().copied());
    probe.extend(xs.iter().copied());
    let values: Option<Vec<f64>> = probe.iter().map(|&s| pnl(inp, s, exp_day)).collect();
    let slope = upside_slope(inp, exp_day);
    let (max_profit, max_loss) = match values {
        Some(v) if !v.is_empty() => {
            let hi = v.iter().copied().fold(f64::MIN, f64::max);
            let lo = v.iter().copied().fold(f64::MAX, f64::min);
            ((slope <= 1e-9).then_some(hi), (slope >= -1e-9).then_some(lo))
        }
        _ => (None, None),
    };

    let net_premium: f64 = inp.legs.iter().filter(|l| l.kind != Kind::Stock).map(|l| -l.qty * l.price * MULTIPLIER).sum();
    let pop_read = expiry.and_then(|e| pop(inp, e));
    let path = path(inp, &payoff, expiry);

    Evaluation {
        expiry: expiry.map(|d| d.to_string()),
        at: Some(at_day.to_string()),
        payoff,
        breakevens,
        max_profit,
        max_loss,
        net_premium,
        greeks: greeks(inp),
        pop: pop_read.map(|p| p.0),
        pop_basis: pop_read.map(|p| p.1),
        margin: margin::estimate(inp.legs, inp.spot, inp.model.style == Style::European, net_premium),
        path,
        warnings: warnings(inp),
    }
}

/// The days between today and the nearest expiry worth a curve: today, then a third and two
/// thirds of the way. Fewer on a contract about to expire, none when a live leg has no
/// volatility to be priced with.
fn path_days(today: Date, expiry: Date) -> Vec<Date> {
    let days = (expiry - today).whole_days();
    let mut out = Vec::new();
    for part in [0, 1, 2] {
        let d = today + time::Duration::days(days * part / 3);
        if d < expiry && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

fn path(inp: &Input, payoff: &[Point], expiry: Option<Date>) -> Vec<PathCurve> {
    let Some(e) = expiry else { return Vec::new() };
    path_days(inp.today, e)
        .into_iter()
        .map(|day| PathCurve { date: day.to_string(), pnl: payoff.iter().map(|p| pnl(inp, p.spot, day)).collect() })
        .filter(|c| c.pnl.iter().all(Option::is_some))
        .collect()
}

/// Days to expiry under which an in-the-money short is flagged whatever else is true.
const LAST_DAYS: i64 = 7;

/// Short American legs: the standard early-assignment checks, read on the market price the
/// leg carries. A call is exercised for a dividend worth more than its time value; a put when
/// the interest on the strike is worth more than its time value.
fn warnings(inp: &Input) -> Vec<Warning> {
    if inp.model.style != Style::American {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut any = false;
    for l in inp.legs.iter().filter(|l| l.qty < 0.0 && l.kind != Kind::Stock) {
        let Some(expiry) = l.expiry else { continue };
        any = true;
        let call = l.kind == Kind::Call;
        let days = (expiry - inp.today).whole_days();
        let inside = intrinsic(call, l.strike, inp.spot);
        if inside <= 0.0 || days < 0 {
            continue;
        }
        let name = format!("{} {} {expiry}", if call { "call" } else { "put" }, l.strike);
        let time_value = (l.price - inside).max(0.0);
        let next_ex = inp.model.dividends.iter().filter(|(ex, _)| *ex > inp.today && *ex <= expiry).min_by_key(|(ex, _)| *ex);
        let carry = l.strike * (1.0 - (-inp.model.rate * days as f64 / 365.0).exp());
        let reason = match next_ex {
            Some((ex, amount)) if call && *amount > time_value => Some(format!(
                "its time value ({time_value:.2}) is under the {amount:.2} dividend going ex on {ex}: close it before that date"
            )),
            _ if !call && carry > time_value && days > 0 => {
                Some(format!("it is in the money and its time value ({time_value:.2}) is under the interest on its strike ({carry:.2})"))
            }
            _ if days <= LAST_DAYS => Some(format!("it is in the money with {days} days left")),
            _ => None,
        };
        if let Some(r) = reason {
            out.push(Warning { code: "early_assignment", message: format!("Short {name} may be assigned early: {r}") });
        }
    }
    if any {
        out.push(Warning {
            code: "assignable",
            message: "Short American options can be assigned on any day before expiry: these curves assume they are not".into(),
        });
    }
    out
}

fn greeks(inp: &Input) -> Option<PositionGreeks> {
    let mut g = PositionGreeks::default();
    for l in inp.legs {
        let n = l.qty * l.multiplier();
        let call = match l.kind {
            Kind::Stock => {
                g.delta += n;
                continue;
            }
            Kind::Call => true,
            Kind::Put => false,
        };
        let expiry = l.expiry?;
        if (expiry - inp.today).whole_days() <= 0 {
            continue;
        }
        let u = inp.model.greeks(call, inp.spot, l.strike, inp.today, expiry, l.iv? + inp.iv_shift)?;
        g.delta += n * u.delta;
        g.gamma += n * u.gamma;
        g.vega += n * u.vega / 100.0;
        g.theta += n * u.theta / 365.0;
        g.rho += n * u.rho / 100.0;
    }
    Some(g)
}

/// Probability that the P&L at `expiry` ends above zero and how it was read: spot lognormal
/// around its forward. Where the chain's smile is known each breakeven reads the volatility of
/// its own strike, so skew counts; otherwise the at-the-money volatility does for all. A
/// model figure, not a forecast.
fn pop(inp: &Input, expiry: Date) -> Option<(f64, &'static str)> {
    let t = (expiry - inp.today).whole_days() as f64 / 365.0;
    let sigma = inp.atm_iv.or_else(|| {
        inp.legs
            .iter()
            .filter(|l| l.expiry == Some(expiry) && l.iv.is_some())
            .min_by(|a, b| (a.strike - inp.spot).abs().total_cmp(&(b.strike - inp.spot).abs()))
            .and_then(|l| l.iv)
    })?;
    if !(t > 0.0 && sigma > 0.0 && inp.spot > 0.0) {
        return None;
    }
    let (r, q) = (inp.model.rate, inp.model.dividend);
    let paid: f64 = inp
        .model
        .dividends
        .iter()
        .filter(|(ex, _)| *ex > inp.today && *ex < expiry)
        .map(|(ex, a)| a * (-r * (*ex - inp.today).whole_days() as f64 / 365.0).exp())
        .sum();
    let base = (inp.spot - paid).max(inp.spot * 0.01);
    let smile = inp.smiles.iter().find(|m| m.expiry == expiry && !m.points.is_empty());
    let moments = |s: f64| (base.ln() + (r - q - 0.5 * s * s) * t, s * t.sqrt());
    let (m, sd) = moments(sigma);
    let cdf = |x: f64| {
        if x <= 0.0 {
            return 0.0;
        }
        let v = smile.and_then(|c| c.at(x / inp.spot)).filter(|v| *v > 0.0).unwrap_or(sigma);
        let (m, sd) = moments(v);
        norm_cdf((x.ln() - m) / sd)
    };
    // Eight standard deviations either side hold all the mass that counts.
    let (a, b) = ((m - 8.0 * sd).exp(), (m + 8.0 * sd).exp());
    let n = 2000;
    let mut xs: Vec<f64> = (0..=n).map(|i| a * (b / a).powf(i as f64 / n as f64)).collect();
    xs.extend(inp.legs.iter().filter(|l| l.kind != Kind::Stock && l.strike > a && l.strike < b).map(|l| l.strike));
    xs.sort_by(f64::total_cmp);
    let pts: Option<Vec<(f64, f64)>> = xs.iter().map(|&s| Some((s, pnl(inp, s, expiry)?))).collect();
    let pts = pts?;
    let mut edges = vec![0.0];
    edges.extend(roots(&pts));
    edges.push(f64::INFINITY);
    let mut p = 0.0;
    for w in edges.windows(2) {
        let probe = if w[1].is_finite() { 0.5 * (w[0] + w[1]) } else { w[0].max(b) * 2.0 };
        let probe = if probe <= 0.0 { a * 0.5 } else { probe };
        if pnl(inp, probe, expiry)? > 0.0 {
            let hi = if w[1].is_finite() { cdf(w[1]) } else { 1.0 };
            p += (hi - cdf(w[0])).max(0.0);
        }
    }
    Some((p.clamp(0.0, 1.0), if smile.is_some() { "smile" } else { "atm" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quant::calc::bsm;
    use time::macros::date;

    const EUROPEAN: Model<'static> = Model { style: Style::European, rate: 0.03, dividend: 0.0, dividends: &[] };

    const TODAY: Date = date!(2026 - 11 - 02);
    const EXP: Date = date!(2026 - 12 - 18);

    fn opt(kind: Kind, strike: f64, qty: f64, price: f64) -> Leg {
        Leg { kind, strike, expiry: Some(EXP), qty, price, iv: Some(0.2) }
    }

    fn run(legs: &[Leg]) -> Evaluation {
        evaluate(&input(legs, EUROPEAN, None, 121))
    }

    fn input<'a>(legs: &'a [Leg], model: Model<'a>, at: Option<Date>, steps: usize) -> Input<'a> {
        Input {
            spot: 100.0,
            model,
            legs,
            today: TODAY,
            at,
            iv_shift: 0.0,
            atm_iv: None,
            regime: Regime::StickyStrike,
            smiles: &[],
            lo: 70.0,
            hi: 130.0,
            steps,
        }
    }

    #[test]
    fn a_long_call_breaks_even_at_strike_plus_premium() {
        let e = run(&[opt(Kind::Call, 100.0, 1.0, 3.0)]);
        assert_eq!(e.breakevens.len(), 1);
        assert!((e.breakevens[0] - 103.0).abs() < 1e-9, "{:?}", e.breakevens);
        assert_eq!(e.net_premium, -300.0, "a debit is negative");
        assert_eq!(e.max_profit, None, "unbounded upside");
        assert_eq!(e.max_loss, Some(-300.0));
    }

    #[test]
    fn a_short_call_has_unbounded_loss_even_when_its_strike_is_off_the_chart() {
        let e = run(&[opt(Kind::Call, 150.0, -1.0, 0.4)]);
        assert_eq!(e.max_loss, None);
        assert_eq!(e.max_profit, Some(40.0));
        assert_eq!(e.net_premium, 40.0, "a credit is positive");
        // Covered: the stock cancels the call's upside.
        let covered = run(&[opt(Kind::Call, 105.0, -1.0, 2.0), Leg { kind: Kind::Stock, strike: 0.0, expiry: None, qty: 100.0, price: 100.0, iv: None }]);
        assert!(covered.max_profit.is_some_and(|p| (p - 700.0).abs() < 1e-9), "{:?}", covered.max_profit);
        assert!(covered.max_loss.is_some_and(|l| (l + 9800.0).abs() < 1e-9), "{:?}", covered.max_loss);
    }

    #[test]
    fn an_iron_condor_has_two_breakevens_and_both_sides_bounded() {
        let e = run(&[
            opt(Kind::Put, 90.0, 1.0, 0.5),
            opt(Kind::Put, 95.0, -1.0, 1.2),
            opt(Kind::Call, 105.0, -1.0, 1.1),
            opt(Kind::Call, 110.0, 1.0, 0.4),
        ]);
        let credit = 1.2 + 1.1 - 0.5 - 0.4;
        assert!((e.net_premium - credit * 100.0).abs() < 1e-9);
        assert_eq!(e.breakevens.len(), 2);
        assert!((e.breakevens[0] - (95.0 - credit)).abs() < 1e-9);
        assert!((e.breakevens[1] - (105.0 + credit)).abs() < 1e-9);
        assert!((e.max_profit.unwrap() - credit * 100.0).abs() < 1e-9);
        assert!((e.max_loss.unwrap() - (credit - 5.0) * 100.0).abs() < 1e-9);
        let pop = e.pop.unwrap();
        assert!(pop > 0.2 && pop < 0.8, "{pop}");
    }

    #[test]
    fn a_long_put_profits_down_to_zero_spot_only() {
        // The most it makes is at a spot of zero, far off the chart's range.
        let e = run(&[opt(Kind::Put, 100.0, 1.0, 2.5)]);
        assert_eq!(e.max_profit, Some(9750.0));
        assert_eq!(e.max_loss, Some(-250.0));
        assert!((e.breakevens[0] - 97.5).abs() < 1e-9);
    }

    #[test]
    fn an_atm_long_call_wins_less_often_than_not() {
        let t = (EXP - TODAY).whole_days() as f64 / 365.0;
        let price = bsm(true, 100.0, 100.0, t, 0.03, 0.0, 0.2).unwrap().price;
        let e = run(&[opt(Kind::Call, 100.0, 1.0, price)]);
        let pop = e.pop.unwrap();
        // It has to end above strike + premium.
        let sd = 0.2 * t.sqrt();
        let want = 1.0 - norm_cdf((((100.0 + price) / 100.0).ln() - (0.03 - 0.02) * t) / sd);
        assert!(pop < 0.5 && pop > 0.3, "{pop}");
        assert!((pop - want).abs() < 1e-3, "{pop} vs {want}");
    }

    #[test]
    fn the_dated_curve_sits_above_expiry_for_a_long_option() {
        let mut inp_legs = [opt(Kind::Call, 100.0, 1.0, 3.0)];
        inp_legs[0].iv = Some(0.25);
        let e = evaluate(&input(&inp_legs, EUROPEAN, Some(TODAY), 61));
        assert!(e.payoff.iter().all(|p| p.at_date.unwrap() >= p.at_expiry.unwrap() - 1e-9));
        let g = e.greeks.unwrap();
        assert!(g.delta > 40.0 && g.delta < 70.0, "about half of 100 shares: {}", g.delta);
        assert!(g.theta < 0.0 && g.vega > 0.0);
    }

    #[test]
    fn a_live_leg_without_a_volatility_leaves_the_dated_curve_empty() {
        let mut legs = [opt(Kind::Call, 100.0, 1.0, 3.0)];
        legs[0].iv = None;
        let e = evaluate(&Input { lo: 90.0, hi: 110.0, ..input(&legs, Model { rate: 0.0, ..EUROPEAN }, Some(TODAY), 5) });
        assert!(e.payoff.iter().all(|p| p.at_date.is_none() && p.at_expiry.is_some()));
        assert!(e.greeks.is_none() && e.pop.is_none());
        assert!(e.path.is_empty(), "no volatility, no curve between today and expiry");
    }

    #[test]
    fn the_path_runs_from_today_towards_expiry_and_loses_time_value() {
        let legs = [opt(Kind::Call, 100.0, 1.0, 3.0)];
        let e = run(&legs);
        assert_eq!(e.path.len(), 3);
        assert_eq!(e.path[0].date, TODAY.to_string());
        assert!(e.path.windows(2).all(|w| w[0].date < w[1].date));
        let atm = e.payoff.iter().position(|p| (p.spot - 100.0).abs() < 1e-9).unwrap();
        let v: Vec<f64> = e.path.iter().map(|c| c.pnl[atm].unwrap()).collect();
        assert!(v[0] > v[1] && v[1] > v[2] && v[2] > e.payoff[atm].at_expiry.unwrap(), "{v:?}");
        assert!(path_days(TODAY, TODAY + time::Duration::days(1)).len() == 1, "one day left: today only");
        assert!(path_days(TODAY, TODAY).is_empty());
    }

    #[test]
    fn a_calendar_reads_at_the_front_expiry_with_the_back_leg_still_priced() {
        let back = Leg { expiry: Some(date!(2027 - 01 - 15)), ..opt(Kind::Call, 100.0, 1.0, 4.0) };
        let legs = [opt(Kind::Call, 100.0, -1.0, 2.0), back];
        let e = run(&legs);
        assert_eq!(e.expiry.as_deref(), Some("2026-12-18"));
        let at = |s: f64| e.payoff.iter().min_by(|a, b| (a.spot - s).abs().total_cmp(&(b.spot - s).abs())).unwrap().at_expiry.unwrap();
        assert!(at(100.0) > at(80.0) && at(100.0) > at(125.0), "the tent peaks at the strike");
    }

    #[test]
    fn the_american_model_prices_a_later_put_above_the_european_one() {
        let put = |model| {
            let legs = [Leg { strike: 120.0, ..opt(Kind::Put, 120.0, 1.0, 21.0) }];
            evaluate(&input(&legs, model, Some(TODAY), 61)).payoff[30].at_date.unwrap()
        };
        assert!(put(Model { style: Style::American, rate: 0.08, ..EUROPEAN }) > put(Model { rate: 0.08, ..EUROPEAN }));
    }

    #[test]
    fn sticky_delta_moves_a_legs_volatility_along_the_smile() {
        let legs = [opt(Kind::Put, 100.0, 1.0, 3.0)];
        // A downward skew: lower moneyness, higher volatility.
        let smiles = [Smile { expiry: EXP, points: vec![(0.8, 0.30), (1.0, 0.20), (1.2, 0.15)] }];
        let inp = input(&legs, EUROPEAN, None, 5);
        let sticky = Input { regime: Regime::StickyDelta, smiles: &smiles, ..input(&legs, EUROPEAN, None, 5) };
        assert_eq!(legs[0].vol(&inp, 120.0), Some(0.2), "sticky strike keeps it");
        assert!((legs[0].vol(&sticky, 100.0).unwrap() - 0.2).abs() < 1e-12, "today it is the leg's own");
        // At spot 120 the strike sits at 0.833 of spot, up the skew.
        let up = legs[0].vol(&sticky, 120.0).unwrap();
        assert!(up > 0.25 && up < 0.3, "{up}");
        // No smile for that expiry: sticky strike.
        let none = Input { regime: Regime::StickyDelta, ..input(&legs, EUROPEAN, None, 5) };
        assert_eq!(legs[0].vol(&none, 120.0), Some(0.2));
    }

    #[test]
    fn a_smile_interpolates_and_stays_flat_past_its_ends() {
        let s = Smile { expiry: EXP, points: vec![(0.9, 0.3), (1.1, 0.2)] };
        assert_eq!(s.at(0.5), Some(0.3));
        assert_eq!(s.at(1.5), Some(0.2));
        assert!((s.at(1.0).unwrap() - 0.25).abs() < 1e-12);
        assert_eq!(Smile { expiry: EXP, points: vec![] }.at(1.0), None);
    }

    #[test]
    fn a_stale_quote_does_not_survive_in_the_smile() {
        let raw = vec![(0.9, 0.30), (0.95, 0.28), (1.0, 0.90), (1.05, 0.24), (1.1, 0.22), (1.15, 0.20)];
        let s = Smile::smoothed(EXP, raw);
        assert!(s.at(1.0).unwrap() < 0.3, "{:?}", s.points);
        assert!(s.points.windows(2).all(|w| w[1].1 <= w[0].1 + 1e-12), "still falling: {:?}", s.points);
        assert!(Smile::smoothed(EXP, vec![]).points.is_empty());
        assert_eq!(Smile::smoothed(EXP, vec![(1.0, 0.2)]).points, vec![(1.0, 0.2)]);
    }

    #[test]
    fn skew_changes_the_probability_of_profit() {
        let legs = [opt(Kind::Put, 85.0, -1.0, 0.5)];
        let flat = run(&legs);
        assert_eq!(flat.pop_basis, Some("atm"));
        // Fat left tail: the breakeven strike is priced at a higher volatility.
        let smiles = [Smile { expiry: EXP, points: vec![(0.7, 0.5), (0.85, 0.4), (1.0, 0.2)] }];
        let skew = evaluate(&Input { smiles: &smiles, ..input(&legs, EUROPEAN, None, 121) });
        assert_eq!(skew.pop_basis, Some("smile"));
        assert!(skew.pop.unwrap() < flat.pop.unwrap(), "{:?} {:?}", skew.pop, flat.pop);
    }

    #[test]
    fn a_dividend_lowers_the_forward_the_probability_reads() {
        let legs = [opt(Kind::Call, 100.0, 1.0, 3.0)];
        let divs = [(date!(2026 - 12 - 01), 3.0)];
        let paying = evaluate(&input(&legs, Model { dividends: &divs, ..EUROPEAN }, None, 121));
        assert!(paying.pop.unwrap() < run(&legs).pop.unwrap());
    }

    #[test]
    fn the_margin_is_part_of_the_evaluation() {
        assert_eq!(run(&[opt(Kind::Call, 100.0, 1.0, 3.0)]).margin.buying_power, 300.0);
        let index = evaluate(&input(&[opt(Kind::Call, 100.0, -1.0, 3.0)], EUROPEAN, None, 5));
        assert_eq!(index.margin.requirement, 1800.0, "European style is read as a broad index");
    }

    fn american(legs: &[Leg], divs: &[(Date, f64)]) -> Vec<Warning> {
        warnings(&input(legs, Model { style: Style::American, dividends: divs, ..EUROPEAN }, None, 5))
    }

    fn codes(w: &[Warning]) -> Vec<&'static str> {
        w.iter().map(|x| x.code).collect()
    }

    #[test]
    fn a_short_call_is_flagged_when_the_dividend_beats_its_time_value() {
        let legs = [opt(Kind::Call, 90.0, -1.0, 10.3)];
        let divs = [(date!(2026 - 12 - 01), 0.5)];
        let w = american(&legs, &divs);
        assert_eq!(codes(&w), ["early_assignment", "assignable"]);
        assert!(w[0].message.contains("2026-12-01"), "{}", w[0].message);
        // More time value than dividend: nothing.
        assert_eq!(codes(&american(&[opt(Kind::Call, 90.0, -1.0, 11.0)], &divs)), ["assignable"]);
        // A dividend after expiry is no reason.
        assert_eq!(codes(&american(&legs, &[(date!(2027 - 01 - 05), 0.5)])), ["assignable"]);
    }

    #[test]
    fn a_short_put_is_flagged_when_the_interest_on_its_strike_beats_its_time_value() {
        let w = american(&[opt(Kind::Put, 120.0, -1.0, 20.2)], &[]);
        assert_eq!(codes(&w), ["early_assignment", "assignable"]);
        assert_eq!(codes(&american(&[opt(Kind::Put, 120.0, -1.0, 23.0)], &[])), ["assignable"]);
    }

    #[test]
    fn an_in_the_money_short_is_flagged_in_its_last_week() {
        let soon = Leg { expiry: Some(TODAY + time::Duration::days(3)), ..opt(Kind::Call, 90.0, -1.0, 12.0) };
        assert_eq!(codes(&american(&[soon], &[])), ["early_assignment", "assignable"]);
    }

    #[test]
    fn nothing_warns_on_european_style_or_without_a_short() {
        assert!(run(&[opt(Kind::Call, 90.0, -1.0, 10.1)]).warnings.is_empty());
        assert!(american(&[opt(Kind::Call, 90.0, 1.0, 10.1)], &[]).is_empty());
        assert_eq!(codes(&american(&[opt(Kind::Call, 120.0, -1.0, 0.5)], &[])), ["assignable"], "out of the money");
    }

    #[test]
    fn roots_interpolate_and_keep_exact_zeros() {
        assert_eq!(roots(&[(0.0, -1.0), (1.0, 1.0)]), vec![0.5]);
        assert_eq!(roots(&[(0.0, -1.0), (1.0, 0.0), (2.0, 0.0), (3.0, 1.0)]), vec![1.0, 2.0]);
        assert_eq!(roots(&[(0.0, 1.0), (1.0, 0.0)]), vec![1.0], "a zero on the last sample");
        assert!(roots(&[(0.0, 1.0), (1.0, 2.0)]).is_empty());
    }
}
