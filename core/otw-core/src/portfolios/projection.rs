//! Goal projection: progress, the monthly contribution the target needs, and a Monte Carlo
//! cone built from the book's own measured return and volatility.
//!
//! Measured, never assumed: the drift and the volatility come from the stored curve, and a
//! curve too short to annualize is refused rather than extrapolated. The paths are drawn
//! from a fixed seed so the same book gives the same cone on every reload.

use serde::Serialize;

/// Paths drawn for the cone.
pub const PATHS: usize = 1000;
/// Fixed seed: a cone that moves on reload reads as news.
const SEED: u64 = 0x5EED_0F_0B7E;

/// Return and volatility as measured on the curve, both annual fractions.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Measured {
    pub annual_return: f64,
    pub annual_vol: f64,
    pub years: f64,
}

/// Required monthly contribution to reach `target` from `value` in `months` at the annual
/// rate `r`, contributions at month end. Zero when the book gets there on its own.
pub fn required_contribution(value: f64, target: f64, months: u32, r: f64) -> f64 {
    if months == 0 {
        return (target - value).max(0.0);
    }
    let i = (1.0 + r).powf(1.0 / 12.0) - 1.0;
    let n = months as f64;
    let grown = value * (1.0 + i).powf(n);
    let gap = target - grown;
    if gap <= 0.0 {
        return 0.0;
    }
    if i.abs() < 1e-12 {
        gap / n
    } else {
        gap * i / ((1.0 + i).powf(n) - 1.0)
    }
}

/// One month of the cone.
#[derive(Debug, Clone, Serialize)]
pub struct ConePoint {
    pub month: u32,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
}

/// SplitMix64: small, fast, and the same numbers on every platform.
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn uniform(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    /// Standard normal by Box-Muller.
    fn normal(&mut self) -> f64 {
        let (u1, u2) = (self.uniform(), self.uniform());
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// Monthly lognormal paths from today's value with a monthly contribution.
pub fn cone(value: f64, monthly: f64, months: u32, m: Measured, paths: usize) -> Vec<ConePoint> {
    let mu = (1.0 + m.annual_return).max(1e-9).ln() / 12.0;
    let sigma = m.annual_vol / 12f64.sqrt();
    let drift = mu - 0.5 * sigma * sigma;
    let mut rng = Rng(SEED);
    let mut v = vec![value; paths];
    let mut out = Vec::with_capacity(months as usize + 1);
    out.push(ConePoint { month: 0, p10: value, p50: value, p90: value });
    let mut sorted = vec![0.0; paths];
    for month in 1..=months {
        for x in v.iter_mut() {
            *x = *x * (drift + sigma * rng.normal()).exp() + monthly;
        }
        sorted.copy_from_slice(&v);
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let q = |p: f64| sorted[((paths as f64 - 1.0) * p).round() as usize];
        out.push(ConePoint { month, p10: q(0.1), p50: q(0.5), p90: q(0.9) });
    }
    out
}

/// Whole months from `today` to `target`, zero when the date is past.
pub fn months_between(today: time::Date, target: time::Date) -> u32 {
    if target <= today {
        return 0;
    }
    let m = (target.year() - today.year()) * 12 + (target.month() as i32 - today.month() as i32);
    let m = if target.day() < today.day() { m - 1 } else { m };
    m.max(0) as u32
}

/// Under a fifth of a year, an annual rate is an extrapolation (same floor as performance).
pub const MIN_YEARS: f64 = 0.2;

pub fn measurable(m: &Measured) -> bool {
    m.years >= MIN_YEARS && m.annual_vol.is_finite() && m.annual_return.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Measured = Measured { annual_return: 0.07, annual_vol: 0.15, years: 3.0 };

    #[test]
    fn the_cone_widens_with_the_horizon() {
        let c = cone(10_000.0, 200.0, 120, M, PATHS);
        assert_eq!(c.len(), 121);
        let mut last_spread = 0.0;
        for p in c.iter().step_by(12) {
            assert!(p.p10 <= p.p50 && p.p50 <= p.p90);
            let spread = p.p90 - p.p10;
            assert!(spread >= last_spread, "month {}: {spread} < {last_spread}", p.month);
            last_spread = spread;
        }
        // The same seed draws the same cone.
        let again = cone(10_000.0, 200.0, 120, M, PATHS);
        assert_eq!(c.last().unwrap().p50, again.last().unwrap().p50);
    }

    #[test]
    fn the_required_contribution_closes_the_gap() {
        let c = required_contribution(10_000.0, 50_000.0, 120, 0.05);
        // Grow it back with the same rate: it lands on the target.
        let i = 1.05f64.powf(1.0 / 12.0) - 1.0;
        let mut v = 10_000.0;
        for _ in 0..120 {
            v = v * (1.0 + i) + c;
        }
        assert!((v - 50_000.0).abs() < 1e-6, "{v}");
        assert_eq!(required_contribution(10_000.0, 5_000.0, 12, 0.05), 0.0);
        assert!((required_contribution(0.0, 1_200.0, 12, 0.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_short_history_is_refused() {
        assert!(!measurable(&Measured { years: 0.1, ..M }));
        assert!(measurable(&M));
    }

    #[test]
    fn months_count_whole_months() {
        use time::{Date, Month};
        let d = |y, m, day| Date::from_calendar_date(y, m, day).unwrap();
        assert_eq!(months_between(d(2026, Month::January, 15), d(2027, Month::January, 15)), 12);
        assert_eq!(months_between(d(2026, Month::January, 15), d(2026, Month::March, 14)), 1);
        assert_eq!(months_between(d(2026, Month::March, 1), d(2026, Month::January, 1)), 0);
    }
}
