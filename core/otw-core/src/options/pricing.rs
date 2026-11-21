//! The option model behind the chain's columns and the Builder: American exercise on a
//! binomial tree, European under Black-Scholes-Merton, cash dividends on their dates.
//!
//! - **Exercise**: US equity and ETF options are American, broad index options European. An
//!   American price is the European formula plus the early-exercise premium a tree measures
//!   against its own European price (the control variate of Hull and White): the tree's
//!   lattice error cancels, so greeks read by bumping stay smooth.
//! - **Dividends**: a cash dividend is taken out of the spot at its present value (the
//!   escrowed model) and put back on the tree's nodes until it goes ex, so exercising just
//!   before an ex-date is valued against the full stock price. A continuous yield stays for
//!   an index or an ETF nobody has dates for.
//!
//! Dates are calendar days over 365, as everywhere in this module.

use time::Date;

use crate::quant::calc::{bsm, implied_vol, Greeks};

/// Steps of the tree. The control variate makes this enough: see the convergence test.
const STEPS: usize = 128;
/// Implied volatility is found to this much of the price.
const PRICE_TOLERANCE: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    American,
    European,
}

impl Style {
    /// Listed US equity and ETF options are American; index options are read as European.
    pub fn of(asset_type: &str) -> Self {
        if asset_type == "index" { Style::European } else { Style::American }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Model<'a> {
    pub style: Style,
    /// Continuous, as a fraction.
    pub rate: f64,
    /// Continuous dividend yield, as a fraction.
    pub dividend: f64,
    /// Cash dividends: ex-date and amount per share.
    pub dividends: &'a [(Date, f64)],
}

/// Cash dividends going ex strictly after `from` and before `to`, as (years from `from`, amount).
fn inside(all: &[(Date, f64)], from: Date, to: Date) -> Vec<(f64, f64)> {
    all.iter()
        .filter(|(ex, _)| *ex > from && *ex < to)
        .map(|(ex, a)| ((*ex - from).whole_days() as f64 / 365.0, *a))
        .collect()
}

fn present_value(divs: &[(f64, f64)], r: f64, at: f64) -> f64 {
    divs.iter().filter(|(t, _)| *t > at).map(|(t, a)| a * (-r * (t - at)).exp()).sum()
}

fn exercise(call: bool, s: f64, k: f64) -> f64 {
    if call { s - k } else { k - s }
}

struct Inputs {
    call: bool,
    s: f64,
    k: f64,
    t: f64,
    r: f64,
    q: f64,
}

/// One tree's reading of a price, with delta and gamma off its second step's three nodes: far
/// smoother than bumping the spot, which shifts the whole lattice against the strike.
#[derive(Debug, Clone, Copy, Default)]
struct Read {
    price: f64,
    delta: f64,
    gamma: f64,
}

fn read(price: f64, net: &[f64], v: &[f64]) -> Read {
    let (up, down) = ((v[2] - v[1]) / (net[2] - net[1]), (v[1] - v[0]) / (net[1] - net[0]));
    Read { price, delta: (v[2] - v[0]) / (net[2] - net[0]), gamma: (up - down) / (0.5 * (net[2] - net[0])) }
}

/// American and European reading on one tree (Jarrow-Rudd steps, whose probability stays valid
/// at any volatility). The tree runs on the spot net of dividends; a node's stock price
/// carries back those not yet ex, which is what an exercise there receives.
fn tree(i: &Inputs, divs: &[(f64, f64)], v: f64) -> Option<(Read, Read)> {
    let Inputs { call, k, t, r, q, .. } = *i;
    let base = i.s - present_value(divs, r, 0.0);
    if !(base > 0.0 && t > 0.0 && v > 0.0) {
        return None;
    }
    let n = STEPS;
    let dt = t / n as f64;
    let (up, down) = {
        let drift = (r - q - 0.5 * v * v) * dt;
        let step = v * dt.sqrt();
        ((drift + step).exp(), (drift - step).exp())
    };
    let p = (((r - q) * dt).exp() - down) / (up - down);
    if !(0.0..=1.0).contains(&p) {
        return None;
    }
    let disc = (-r * dt).exp();
    // `net[j]`: the net-of-dividends spot at the node with `j` up moves, on the current step.
    let mut net: Vec<f64> = (0..=n).map(|j| base * up.powi(j as i32) * down.powi((n - j) as i32)).collect();
    let mut am: Vec<f64> = net.iter().map(|s| exercise(call, *s, k).max(0.0)).collect();
    let mut eu = am.clone();
    let (mut at_two_am, mut at_two_eu) = (Read::default(), Read::default());
    for step in (0..n).rev() {
        let carried = present_value(divs, r, step as f64 * dt);
        for j in 0..=step {
            net[j] = net[j + 1] / up;
            eu[j] = disc * (p * eu[j + 1] + (1.0 - p) * eu[j]);
            let hold = disc * (p * am[j + 1] + (1.0 - p) * am[j]);
            am[j] = hold.max(exercise(call, net[j] + carried, k));
        }
        if step == 2 {
            at_two_am = read(0.0, &net[..3], &am[..3]);
            at_two_eu = read(0.0, &net[..3], &eu[..3]);
        }
    }
    Some((Read { price: am[0], ..at_two_am }, Read { price: eu[0], ..at_two_eu }))
}

impl Model<'_> {
    fn inputs(&self, call: bool, s: f64, k: f64, t: f64) -> Inputs {
        Inputs { call, s, k, t, r: self.rate, q: self.dividend }
    }

    /// The closed form, on the spot net of the dividends inside the option's life.
    fn european(&self, i: &Inputs, divs: &[(f64, f64)], v: f64) -> Option<Greeks> {
        let net = (i.s - present_value(divs, i.r, 0.0)).max(1e-9);
        bsm(i.call, net, i.k, i.t, i.r, i.q, v)
    }

    /// Whether exercising early can ever pay: a call on a stock paying nothing never does.
    fn early(&self, i: &Inputs, divs: &[(f64, f64)]) -> bool {
        self.style == Style::American && (!i.call || !divs.is_empty() || i.q > 0.0)
    }

    fn value_of(&self, i: &Inputs, divs: &[(f64, f64)], v: f64) -> Option<f64> {
        let eu = self.european(i, divs, v)?.price;
        if !self.early(i, divs) {
            return Some(eu);
        }
        let (am, tree_eu) = tree(i, divs, v)?;
        Some((eu + (am.price - tree_eu.price).max(0.0)).max(exercise(i.call, i.s, i.k)))
    }

    /// What the option is worth on `from`, expiring on `to`, at `spot` and volatility `v`.
    pub fn value(&self, call: bool, s: f64, k: f64, from: Date, to: Date, v: f64) -> Option<f64> {
        let t = (to - from).whole_days() as f64 / 365.0;
        self.value_of(&self.inputs(call, s, k, t), &inside(self.dividends, from, to), v)
    }

    /// Price and greeks, in the units of [`bsm`]: vega and rho per 1.00, theta per year. The
    /// closed form where exercise cannot come early, central bumps otherwise.
    pub fn greeks(&self, call: bool, s: f64, k: f64, from: Date, to: Date, v: f64) -> Option<Greeks> {
        let t = (to - from).whole_days() as f64 / 365.0;
        let divs = inside(self.dividends, from, to);
        let i = self.inputs(call, s, k, t);
        if !self.early(&i, &divs) {
            return self.european(&i, &divs, v);
        }
        let at = |s: f64, t: f64, r: f64, v: f64| {
            let m = Inputs { s, t, r, ..self.inputs(call, s, k, t) };
            self.value_of(&m, &divs, v)
        };
        let r = self.rate;
        let price = at(s, t, r, v)?;
        // The closed form's delta and gamma, corrected by what early exercise adds on the tree.
        let closed = self.european(&i, &divs, v)?;
        let (am, tree_eu) = tree(&i, &divs, v)?;
        let dv = (v * 0.05).min(0.01);
        let dr = 1e-3;
        // One calendar day on, or half of what is left of a contract about to expire.
        let dt = (1.0_f64 / 365.0).min(t / 2.0);
        // A day later the dividends are a day nearer too: leaving them behind shrinks the gap
        // between ex-date and expiry, and the early-exercise premium swallows the time decay.
        let later: Vec<(f64, f64)> = divs.iter().map(|(at, a)| (at - dt, *a)).filter(|(at, _)| *at > 0.0).collect();
        let tomorrow = self.value_of(&self.inputs(call, s, k, t - dt), &later, v)?;
        Some(Greeks {
            price,
            delta: closed.delta + am.delta - tree_eu.delta,
            gamma: closed.gamma + am.gamma - tree_eu.gamma,
            vega: (at(s, t, r, v + dv)? - at(s, t, r, v - dv)?) / (2.0 * dv),
            theta: (tomorrow - price) / dt,
            rho: (at(s, t, r + dr, v)? - at(s, t, r - dr, v)?) / (2.0 * dr),
        })
    }

    /// The volatility that prices `price`. `None` for a price at or under what exercising now
    /// pays, or outside what any volatility reaches: such a quote has no volatility.
    pub fn implied_vol(&self, call: bool, price: f64, s: f64, k: f64, from: Date, to: Date) -> Option<f64> {
        let t = (to - from).whole_days() as f64 / 365.0;
        if !(s > 0.0 && k > 0.0 && t > 0.0 && price > 0.0) {
            return None;
        }
        let divs = inside(self.dividends, from, to);
        let i = self.inputs(call, s, k, t);
        let net = s - present_value(&divs, i.r, 0.0);
        if net <= 0.0 {
            return None;
        }
        // The European answer is the exact one where exercise cannot come early, and a close
        // first guess elsewhere.
        let closed = implied_vol(call, price, net, k, t, i.r, i.q);
        if !self.early(&i, &divs) {
            return closed;
        }
        if price <= exercise(call, s, k).max(0.0) {
            return None;
        }
        let f = |v: f64| self.value_of(&i, &divs, v).map(|p| p - price);
        let (mut lo, mut hi) = (1e-4, 10.0);
        if f(lo)? > 0.0 || f(hi)? < 0.0 {
            return None;
        }
        let mut v = closed.unwrap_or(0.3).clamp(lo * 2.0, hi / 2.0);
        for _ in 0..60 {
            let diff = f(v)?;
            if diff.abs() < PRICE_TOLERANCE * price.max(1.0) {
                return Some(v);
            }
            if diff > 0.0 {
                hi = v;
            } else {
                lo = v;
            }
            let vega = self.european(&i, &divs, v).map_or(0.0, |g| g.vega);
            let next = if vega > 1e-9 { v - diff / vega } else { f64::NAN };
            v = if next.is_finite() && next > lo && next < hi { next } else { 0.5 * (lo + hi) };
            if hi - lo < 1e-7 {
                break;
            }
        }
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quant::calc::binomial;
    use time::macros::date;

    const FROM: Date = date!(2026 - 01 - 01);
    /// 365 days on: one year exactly.
    const TO: Date = date!(2027 - 01 - 01);

    const AMERICAN: Model<'static> = Model { style: Style::American, rate: 0.05, dividend: 0.0, dividends: &[] };
    /// A one-year at-the-money put, r 5%, vol 25%: price, delta, gamma, vega, theta, rho.
    const REF: (f64, f64, f64, f64, f64, f64) = (7.97414, -0.40965, 0.01838, 37.827, -3.0921, -32.795);

    #[test]
    fn index_options_are_european_and_the_rest_american() {
        assert_eq!(Style::of("index"), Style::European);
        assert_eq!(Style::of("equity"), Style::American);
        assert_eq!(Style::of("etf"), Style::American);
    }

    #[test]
    fn an_american_put_matches_a_deep_tree_and_beats_the_european_one() {
        for (s, k, v) in [(100.0, 100.0, 0.2), (100.0, 120.0, 0.3), (100.0, 80.0, 0.5), (50.0, 60.0, 0.15)] {
            let p = AMERICAN.value(false, s, k, FROM, TO, v).unwrap();
            let deep = binomial(false, true, s, k, 1.0, 0.05, 0.0, v, 5000).unwrap();
            assert!((p - deep).abs() < 5e-3, "{s} {k} {v}: {p} vs {deep}");
            assert!(p > bsm(false, s, k, 1.0, 0.05, 0.0, v).unwrap().price);
        }
    }

    #[test]
    fn a_call_without_dividends_is_the_closed_form() {
        let want = bsm(true, 100.0, 105.0, 1.0, 0.05, 0.0, 0.25).unwrap();
        let got = AMERICAN.greeks(true, 100.0, 105.0, FROM, TO, 0.25).unwrap();
        assert_eq!((got.price, got.delta, got.vega), (want.price, want.delta, want.vega));
    }

    #[test]
    fn european_style_is_the_closed_form_on_the_escrowed_spot() {
        let divs = [(date!(2026 - 07 - 02), 2.0)];
        let m = Model { style: Style::European, rate: 0.05, dividend: 0.0, dividends: &divs };
        let net = 100.0 - 2.0 * (-0.05_f64 * 182.0 / 365.0).exp();
        let want = bsm(false, net, 100.0, 1.0, 0.05, 0.0, 0.2).unwrap().price;
        assert!((m.value(false, 100.0, 100.0, FROM, TO, 0.2).unwrap() - want).abs() < 1e-12);
    }

    #[test]
    fn a_dividend_lowers_a_call_and_can_make_early_exercise_pay() {
        let big = [(date!(2026 - 12 - 20), 8.0)];
        let m = Model { dividends: &big, ..AMERICAN };
        let eu = Model { style: Style::European, ..m };
        let (am_v, eu_v) = (m.value(true, 120.0, 100.0, FROM, TO, 0.2).unwrap(), eu.value(true, 120.0, 100.0, FROM, TO, 0.2).unwrap());
        assert!(eu_v < AMERICAN.value(true, 120.0, 100.0, FROM, TO, 0.2).unwrap());
        assert!(am_v > eu_v + 0.1, "exercising before the ex-date has value: {am_v} vs {eu_v}");
    }

    #[test]
    fn a_dividend_already_ex_or_after_expiry_changes_nothing() {
        let out = [(date!(2025 - 06 - 01), 5.0), (FROM, 5.0), (TO, 5.0), (date!(2027 - 06 - 01), 5.0)];
        let m = Model { dividends: &out, ..AMERICAN };
        assert_eq!(m.value(false, 100.0, 100.0, FROM, TO, 0.2), AMERICAN.value(false, 100.0, 100.0, FROM, TO, 0.2));
    }

    #[test]
    fn the_implied_vol_inverts_the_american_price() {
        let divs = [(date!(2026 - 06 - 15), 1.5)];
        for m in [AMERICAN, Model { dividends: &divs, ..AMERICAN }, Model { dividend: 0.02, ..AMERICAN }] {
            for (call, k, v) in [(false, 100.0, 0.3), (false, 120.0, 0.45), (true, 90.0, 0.25)] {
                let p = m.value(call, 100.0, k, FROM, TO, v).unwrap();
                let got = m.implied_vol(call, p, 100.0, k, FROM, TO).unwrap();
                assert!((got - v).abs() < 1e-3, "{call} {k}: {got} vs {v}");
            }
        }
    }

    #[test]
    fn a_quote_at_or_under_intrinsic_has_no_volatility() {
        assert_eq!(AMERICAN.implied_vol(false, 19.5, 100.0, 120.0, FROM, TO), None);
        assert_eq!(AMERICAN.implied_vol(false, 20.0, 100.0, 120.0, FROM, TO), None);
    }

    #[test]
    fn american_greeks_read_like_greeks() {
        let g = AMERICAN.greeks(false, 100.0, 100.0, FROM, TO, 0.25).unwrap();
        // Against a 3000-step tree bumped by hand.
        for (name, got, want, tolerance) in [
            ("price", g.price, REF.0, 0.005),
            ("delta", g.delta, REF.1, 0.005),
            ("gamma", g.gamma, REF.2, 0.001),
            ("vega", g.vega, REF.3, 0.3),
            ("theta", g.theta, REF.4, 0.2),
            ("rho", g.rho, REF.5, 1.0),
        ] {
            assert!((got - want).abs() < tolerance, "{name}: {got} vs {want}");
        }
        // Deep in the money an American put moves with the stock, and exercise caps its value.
        let deep = AMERICAN.greeks(false, 50.0, 100.0, FROM, TO, 0.2).unwrap();
        assert!(deep.delta < -0.95 && (deep.price - 50.0).abs() < 1e-3, "{deep:?}");
    }

    #[test]
    fn the_time_decay_of_a_call_is_not_lost_to_a_dividend_nearing() {
        let to = date!(2026 - 01 - 31);
        let divs = [(date!(2026 - 01 - 28), 0.53)];
        let m = Model { rate: 0.04, dividends: &divs, ..AMERICAN };
        let am = m.greeks(true, 96.0, 96.0, FROM, to, 0.18).unwrap();
        let eu = Model { style: Style::European, ..m }.greeks(true, 96.0, 96.0, FROM, to, 0.18).unwrap();
        assert!((am.theta / eu.theta - 1.0).abs() < 0.25, "{} vs {}", am.theta, eu.theta);
    }

    #[test]
    fn a_contract_about_to_expire_still_has_greeks() {
        let g = AMERICAN.greeks(false, 100.0, 100.0, FROM, date!(2026 - 01 - 02), 0.3).unwrap();
        assert!(g.delta.is_finite() && g.theta.is_finite() && g.price > 0.0);
    }
}
