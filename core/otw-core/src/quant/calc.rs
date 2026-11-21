//! Calculators: option pricing and greeks, implied volatility, futures basis and carry,
//! compounding and drawdown recovery, volatility-targeted sizing.

use serde::Serialize;

use super::special::{norm_cdf, norm_pdf};

// ── Black-Scholes-Merton ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone, Copy)]
pub struct Greeks {
    pub price: f64,
    pub delta: f64,
    pub gamma: f64,
    /// Per 1.00 of volatility (divide by 100 for one vol point).
    pub vega: f64,
    /// Per year (divide by 365 for one calendar day).
    pub theta: f64,
    /// Per 1.00 of rate (divide by 100 for one point).
    pub rho: f64,
}

/// European option under Black-Scholes-Merton: spot `s`, strike `k`, time `t` in years, rate
/// `r` and dividend (or carry) yield `q`, both continuous, volatility `v`.
pub fn bsm(call: bool, s: f64, k: f64, t: f64, r: f64, q: f64, v: f64) -> Option<Greeks> {
    if !(s > 0.0 && k > 0.0 && t > 0.0 && v > 0.0) {
        return None;
    }
    let st = t.sqrt();
    let d1 = ((s / k).ln() + (r - q + 0.5 * v * v) * t) / (v * st);
    let d2 = d1 - v * st;
    let (dq, dr) = ((-q * t).exp(), (-r * t).exp());
    let pdf = norm_pdf(d1);
    let gamma = dq * pdf / (s * v * st);
    let vega = s * dq * pdf * st;
    Some(if call {
        Greeks {
            price: s * dq * norm_cdf(d1) - k * dr * norm_cdf(d2),
            delta: dq * norm_cdf(d1),
            gamma,
            vega,
            theta: -s * dq * pdf * v / (2.0 * st) - r * k * dr * norm_cdf(d2) + q * s * dq * norm_cdf(d1),
            rho: k * t * dr * norm_cdf(d2),
        }
    } else {
        Greeks {
            price: k * dr * norm_cdf(-d2) - s * dq * norm_cdf(-d1),
            delta: -dq * norm_cdf(-d1),
            gamma,
            vega,
            theta: -s * dq * pdf * v / (2.0 * st) + r * k * dr * norm_cdf(-d2) - q * s * dq * norm_cdf(-d1),
            rho: -k * t * dr * norm_cdf(-d2),
        }
    })
}

/// Cox-Ross-Rubinstein binomial tree, European or American exercise.
#[allow(clippy::too_many_arguments)]
pub fn binomial(call: bool, american: bool, s: f64, k: f64, t: f64, r: f64, q: f64, v: f64, steps: usize) -> Option<f64> {
    if !(s > 0.0 && k > 0.0 && t > 0.0 && v > 0.0) {
        return None;
    }
    let n = steps.clamp(1, 5000);
    let dt = t / n as f64;
    let u = (v * dt.sqrt()).exp();
    let d = 1.0 / u;
    let disc = (-r * dt).exp();
    let p = (((r - q) * dt).exp() - d) / (u - d);
    if !(0.0..=1.0).contains(&p) {
        return None;
    }
    let payoff = |x: f64| if call { (x - k).max(0.0) } else { (k - x).max(0.0) };
    let mut vals: Vec<f64> = (0..=n).map(|j| payoff(s * u.powi(j as i32) * d.powi((n - j) as i32))).collect();
    for i in (0..n).rev() {
        for j in 0..=i {
            let cont = disc * (p * vals[j + 1] + (1.0 - p) * vals[j]);
            vals[j] = if american { cont.max(payoff(s * u.powi(j as i32) * d.powi((i - j) as i32))) } else { cont };
        }
    }
    Some(vals[0])
}

/// Implied volatility of a European price: Newton on vega from a Brenner-Subrahmanyam start,
/// falling back to bisection whenever a step leaves the bracket. None when the price is
/// outside the no-arbitrage bounds.
pub fn implied_vol(call: bool, price: f64, s: f64, k: f64, t: f64, r: f64, q: f64) -> Option<f64> {
    if !(s > 0.0 && k > 0.0 && t > 0.0 && price > 0.0) {
        return None;
    }
    let fwd_s = s * (-q * t).exp();
    let pv_k = k * (-r * t).exp();
    let (lo_b, hi_b) = if call { ((fwd_s - pv_k).max(0.0), fwd_s) } else { ((pv_k - fwd_s).max(0.0), pv_k) };
    if price <= lo_b || price >= hi_b {
        return None;
    }
    let f = |v: f64| bsm(call, s, k, t, r, q, v).map(|g| g.price - price);
    let (mut lo, mut hi) = (1e-6, 10.0);
    if f(hi)? < 0.0 {
        return None;
    }
    let mut v = ((2.0 * std::f64::consts::PI / t).sqrt() * price / s).clamp(0.01, 5.0);
    for _ in 0..200 {
        let g = bsm(call, s, k, t, r, q, v)?;
        let diff = g.price - price;
        if diff.abs() < 1e-13 * price.max(1.0) {
            return Some(v);
        }
        if diff > 0.0 {
            hi = v;
        } else {
            lo = v;
        }
        let next = if g.vega > 1e-12 { v - diff / g.vega } else { f64::NAN };
        v = if next.is_finite() && next > lo && next < hi { next } else { 0.5 * (lo + hi) };
        if hi - lo < 1e-15 {
            return Some(v);
        }
    }
    Some(v)
}

// ── Futures basis and carry ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Basis {
    /// F − S.
    pub basis: f64,
    /// F / S − 1.
    pub basis_pct: f64,
    /// (F / S − 1) · 365 / days.
    pub carry_simple: f64,
    /// ln(F / S) · 365 / days, the continuously compounded carry the futures price implies.
    pub carry_continuous: f64,
    /// Fair value S · exp((r − q) · T) and the gap to the quoted future.
    pub fair_value: Option<f64>,
    pub mispricing: Option<f64>,
    /// Financing rate the future implies once the yield q is added back: ln(F/S)/T + q.
    pub implied_repo: f64,
    /// Annualized roll yield between this contract and the next, ln(F1/F2) · 365 / (d2 − d1):
    /// positive in backwardation (rolling earns), negative in contango.
    pub roll_yield: Option<f64>,
    pub contango: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn basis(spot: f64, fut: f64, days: f64, rate: Option<f64>, yield_q: f64, next: Option<(f64, f64)>) -> Option<Basis> {
    if !(spot > 0.0 && fut > 0.0 && days > 0.0) {
        return None;
    }
    let t = days / 365.0;
    let ln = (fut / spot).ln();
    let fair = rate.map(|r| spot * ((r - yield_q) * t).exp());
    Some(Basis {
        basis: fut - spot,
        basis_pct: fut / spot - 1.0,
        carry_simple: (fut / spot - 1.0) / t,
        carry_continuous: ln / t,
        fair_value: fair,
        mispricing: fair.map(|f| fut - f),
        implied_repo: ln / t + yield_q,
        roll_yield: next.filter(|(f2, d2)| *f2 > 0.0 && *d2 > days).map(|(f2, d2)| (fut / f2).ln() * 365.0 / (d2 - days)),
        contango: fut > spot,
    })
}

// ── Compounding, required return, drawdown recovery ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Compounding {
    /// Value after `periods` at `rate`, with `contribution` added at the end of each period.
    pub future_value: f64,
    pub contributed: f64,
    pub growth: f64,
    /// Rate per period that reaches the target in `periods`.
    pub required_rate: Option<f64>,
    /// Periods needed at `rate` to reach the target.
    pub periods_to_target: Option<f64>,
    /// Value at the end of each period, for the chart.
    pub path: Vec<f64>,
    /// (drawdown, gain needed to recover, periods to recover at `rate`).
    pub recovery: Vec<(f64, f64, Option<f64>)>,
}

fn fv(pv: f64, r: f64, n: f64, c: f64) -> f64 {
    if r.abs() < 1e-14 { pv + c * n } else { pv * (1.0 + r).powf(n) + c * ((1.0 + r).powf(n) - 1.0) / r }
}

pub fn compounding(pv: f64, rate: f64, periods: usize, contribution: f64, target: Option<f64>) -> Compounding {
    let n = periods.min(10_000);
    let mut path = Vec::with_capacity(n + 1);
    let mut v = pv;
    path.push(v);
    for _ in 0..n {
        v = v * (1.0 + rate) + contribution;
        path.push(v);
    }
    let fv_n = fv(pv, rate, n as f64, contribution);
    let required_rate = target.filter(|t| *t > 0.0 && n > 0).and_then(|t| {
        if contribution == 0.0 {
            return (pv > 0.0).then(|| (t / pv).powf(1.0 / n as f64) - 1.0);
        }
        // fv is increasing in r for non-negative flows: bisection.
        let (mut lo, mut hi) = (-0.99, 10.0);
        if fv(pv, lo, n as f64, contribution) > t || fv(pv, hi, n as f64, contribution) < t {
            return None;
        }
        for _ in 0..300 {
            let mid = 0.5 * (lo + hi);
            if fv(pv, mid, n as f64, contribution) < t { lo = mid } else { hi = mid }
        }
        Some(0.5 * (lo + hi))
    });
    let periods_to_target = target.filter(|t| *t > 0.0).and_then(|t| {
        if rate.abs() < 1e-14 {
            return (contribution > 0.0 && t >= pv).then(|| (t - pv) / contribution);
        }
        // (1+r)^n = (t·r + c) / (pv·r + c)
        let num = t * rate + contribution;
        let den = pv * rate + contribution;
        let x = num / den;
        (x > 0.0 && den != 0.0 && rate > -1.0).then(|| x.ln() / (1.0 + rate).ln()).filter(|v| v.is_finite() && *v >= 0.0)
    });
    let recovery = [0.05f64, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.75, 0.9]
        .iter()
        .map(|&d| {
            let g = 1.0 / (1.0 - d) - 1.0;
            (d, g, (rate > 0.0).then(|| (1.0 / (1.0 - d)).ln() / (1.0 + rate).ln()))
        })
        .collect();
    Compounding {
        future_value: fv_n,
        contributed: pv + contribution * n as f64,
        growth: fv_n - pv - contribution * n as f64,
        required_rate,
        periods_to_target,
        path,
        recovery,
    }
}

// ── Volatility targeting ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VolEstimator {
    /// Rolling standard deviation of returns over `window` bars.
    Rolling,
    /// RiskMetrics exponentially weighted variance, λ = 1 − 2 / (window + 1).
    Ewma,
}

#[derive(Debug, Serialize)]
pub struct VolTarget {
    pub target: f64,
    pub max_leverage: f64,
    /// Latest volatility estimate (annualized) and the exposure it implies for the next bar.
    pub current_vol: f64,
    pub current_weight: f64,
    /// Units to hold for `equity` at the last close, and their notional.
    pub units: Option<f64>,
    pub notional: Option<f64>,
    pub observations: usize,
    /// Realized annualized vol, return, Sharpe and max drawdown: unscaled asset vs scaled.
    pub asset_vol: f64,
    pub scaled_vol: f64,
    pub asset_return: f64,
    pub scaled_return: f64,
    pub asset_sharpe: Option<f64>,
    pub scaled_sharpe: Option<f64>,
    pub asset_max_dd: f64,
    pub scaled_max_dd: f64,
    pub avg_weight: f64,
    /// (ts, weight, estimated vol, asset equity, scaled equity), thinned for charting.
    pub points: Vec<(String, f64, f64, f64, f64)>,
}

fn max_dd(eq: &[f64]) -> f64 {
    let mut peak = f64::MIN;
    let mut dd = 0.0f64;
    for &e in eq {
        peak = peak.max(e);
        dd = dd.max(1.0 - e / peak);
    }
    dd
}

/// Scale exposure so the position runs at `target` annualized vol: weight for bar t is
/// min(max_leverage, target / σ̂) with σ̂ estimated on returns up to t − 1 (no look-ahead).
#[allow(clippy::too_many_arguments)]
pub fn vol_target(
    ts: &[String],
    closes: &[f64],
    ppy: f64,
    target: f64,
    window: usize,
    estimator: VolEstimator,
    max_leverage: f64,
    equity: Option<f64>,
) -> Option<VolTarget> {
    let r: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let w = window.max(2);
    if r.len() < w + 10 || target <= 0.0 {
        return None;
    }
    let ann = ppy.sqrt();
    // est[i] = vol known after return i (annualized).
    let mut est = vec![f64::NAN; r.len()];
    match estimator {
        VolEstimator::Rolling => {
            for i in w - 1..r.len() {
                est[i] = super::stddev(&r[i + 1 - w..=i]) * ann;
            }
        }
        VolEstimator::Ewma => {
            let lambda = 1.0 - 2.0 / (w as f64 + 1.0);
            // Seed with the sample variance of the first window, then recurse.
            let m0 = super::mean(&r[..w]);
            let mut var = r[..w].iter().map(|x| (x - m0).powi(2)).sum::<f64>() / (w as f64 - 1.0);
            est[w - 1] = var.sqrt() * ann;
            for i in w..r.len() {
                var = lambda * var + (1.0 - lambda) * r[i] * r[i];
                est[i] = var.sqrt() * ann;
            }
        }
    }
    let lev = max_leverage.max(0.0);
    let weight = |v: f64| if v > 0.0 { (target / v).min(lev) } else { lev };
    // Trading starts at return index w (first bar with an estimate from the bar before).
    let start = w;
    let mut asset_r = Vec::new();
    let mut scaled_r = Vec::new();
    let mut weights = Vec::new();
    for i in start..r.len() {
        let wt = weight(est[i - 1]);
        weights.push(wt);
        asset_r.push(r[i]);
        scaled_r.push(wt * r[i]);
    }
    let curve = |rs: &[f64]| {
        let mut e = 1.0;
        let mut out = vec![1.0];
        for x in rs {
            e *= 1.0 + x;
            out.push(e);
        }
        out
    };
    let (ae, se) = (curve(&asset_r), curve(&scaled_r));
    let stats = |rs: &[f64]| {
        let mu = super::mean(rs) * ppy;
        let v = super::stddev(rs) * ann;
        (mu, v, (v > 0.0).then(|| mu / v))
    };
    let (am, av, ash) = stats(&asset_r);
    let (sm, sv, ssh) = stats(&scaled_r);
    let cur_vol = est[r.len() - 1];
    let cur_w = weight(cur_vol);
    let last = *closes.last()?;
    let n = asset_r.len();
    let step = n.div_ceil(1500).max(1);
    let points = (0..n)
        .filter(|i| i % step == 0 || *i == n - 1)
        .map(|i| (ts[start + i + 1].clone(), weights[i], est[start + i - 1], ae[i + 1], se[i + 1]))
        .collect();
    Some(VolTarget {
        target,
        max_leverage: lev,
        current_vol: cur_vol,
        current_weight: cur_w,
        units: equity.map(|e| e * cur_w / last),
        notional: equity.map(|e| e * cur_w),
        observations: n,
        asset_vol: av,
        scaled_vol: sv,
        asset_return: am,
        scaled_return: sm,
        asset_sharpe: ash,
        scaled_sharpe: ssh,
        asset_max_dd: max_dd(&ae),
        scaled_max_dd: max_dd(&se),
        avg_weight: super::mean(&weights),
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_call_parity_holds() {
        let c = bsm(true, 100.0, 95.0, 0.5, 0.03, 0.01, 0.25).unwrap();
        let p = bsm(false, 100.0, 95.0, 0.5, 0.03, 0.01, 0.25).unwrap();
        let lhs = c.price - p.price;
        let rhs = 100.0 * (-0.01f64 * 0.5).exp() - 95.0 * (-0.03f64 * 0.5).exp();
        assert!((lhs - rhs).abs() < 1e-12);
    }

    #[test]
    fn implied_vol_round_trips() {
        for &(call, k, v) in &[(true, 80.0, 0.1), (false, 120.0, 0.6), (true, 100.0, 1.5), (false, 60.0, 0.3)] {
            let px = bsm(call, 100.0, k, 0.75, 0.02, 0.0, v).unwrap().price;
            let iv = implied_vol(call, px, 100.0, k, 0.75, 0.02, 0.0).unwrap();
            assert!((iv - v).abs() < 1e-9, "{call} {k} {v}: {iv}");
        }
    }

    #[test]
    fn binomial_converges_to_black_scholes() {
        let bs = bsm(true, 100.0, 100.0, 1.0, 0.05, 0.0, 0.2).unwrap().price;
        let tree = binomial(true, false, 100.0, 100.0, 1.0, 0.05, 0.0, 0.2, 2000).unwrap();
        assert!((bs - tree).abs() < 2e-3);
        // An American call without dividends is worth the European one.
        let am = binomial(true, true, 100.0, 100.0, 1.0, 0.05, 0.0, 0.2, 2000).unwrap();
        assert!((am - tree).abs() < 1e-9);
    }

    #[test]
    fn a_half_drawdown_needs_a_double() {
        let c = compounding(1000.0, 0.1, 2, 0.0, Some(1210.0));
        assert!((c.future_value - 1210.0).abs() < 1e-9);
        assert!((c.required_rate.unwrap() - 0.1).abs() < 1e-12);
        assert!((c.periods_to_target.unwrap() - 2.0).abs() < 1e-9);
        let half = c.recovery.iter().find(|r| r.0 == 0.5).unwrap();
        assert!((half.1 - 1.0).abs() < 1e-12);
    }
}
