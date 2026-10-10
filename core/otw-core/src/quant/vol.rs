//! Volatility from OHLC bars: range-based estimators, volatility cones and a GARCH(1,1)
//! forecast.
//!
//! Estimators (Sinclair, "Volatility Trading", ch. 2), each annualized by √ppy:
//! - close-to-close: sample stdev of ln(C_t / C_{t-1})
//! - Parkinson: √( Σ ln(H/L)² / (4 n ln 2) )
//! - Garman-Klass: √( mean( ½ ln(H/L)² − (2 ln 2 − 1) ln(C/O)² ) )
//! - Rogers-Satchell: √( mean( ln(H/C) ln(H/O) + ln(L/C) ln(L/O) ) )
//! - Yang-Zhang: √( σ²_overnight + k σ²_open-close + (1 − k) σ²_RS ), k = 0.34 / (1.34 + (n+1)/(n−1))
//!
//! GARCH(1,1) with a constant mean and normal errors is fitted by maximum likelihood on
//! returns in percent, with the same exponential backcast `arch` uses to start the recursion.

use serde::Serialize;

use super::linalg;
use super::{mean, quantile_sorted, stddev};

#[derive(Debug, Clone, Copy)]
pub struct Bar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

fn valid(b: &Bar) -> bool {
    b.open > 0.0 && b.high > 0.0 && b.low > 0.0 && b.close > 0.0 && b.high >= b.low
}

/// Per-bar variance terms over a window of bars (index 0 needs no previous close except for
/// close-to-close and Yang-Zhang, which use `prev_close`).
fn estimators_window(bars: &[Bar], prev_close: Option<f64>) -> Estimates {
    let n = bars.len();
    let mut cc = Vec::with_capacity(n);
    let mut on = Vec::with_capacity(n);
    let mut oc = Vec::with_capacity(n);
    let (mut pk, mut gk, mut rs) = (0.0, 0.0, 0.0);
    let mut prev = prev_close;
    for b in bars {
        let hl = (b.high / b.low).ln();
        let co = (b.close / b.open).ln();
        pk += hl * hl;
        gk += 0.5 * hl * hl - (2.0 * std::f64::consts::LN_2 - 1.0) * co * co;
        rs += (b.high / b.close).ln() * (b.high / b.open).ln() + (b.low / b.close).ln() * (b.low / b.open).ln();
        if let Some(p) = prev {
            cc.push((b.close / p).ln());
            on.push((b.open / p).ln());
        }
        oc.push(co);
        prev = Some(b.close);
    }
    let nf = n as f64;
    let yz = if on.len() >= 2 && n >= 2 {
        let k = 0.34 / (1.34 + (nf + 1.0) / (nf - 1.0));
        let so = stddev(&on).powi(2);
        let sc = stddev(&oc).powi(2);
        Some((so + k * sc + (1.0 - k) * rs / nf).max(0.0).sqrt())
    } else {
        None
    };
    Estimates {
        close_to_close: (cc.len() >= 2).then(|| stddev(&cc)),
        parkinson: Some((pk / (4.0 * nf * std::f64::consts::LN_2)).sqrt()),
        garman_klass: Some((gk / nf).max(0.0).sqrt()),
        rogers_satchell: Some((rs / nf).max(0.0).sqrt()),
        yang_zhang: yz,
    }
}

/// Per-period volatility by each estimator (not annualized).
#[derive(Debug, Clone, Serialize)]
pub struct Estimates {
    pub close_to_close: Option<f64>,
    pub parkinson: Option<f64>,
    pub garman_klass: Option<f64>,
    pub rogers_satchell: Option<f64>,
    pub yang_zhang: Option<f64>,
}

impl Estimates {
    fn scaled(&self, f: f64) -> Estimates {
        Estimates {
            close_to_close: self.close_to_close.map(|v| v * f),
            parkinson: self.parkinson.map(|v| v * f),
            garman_klass: self.garman_klass.map(|v| v * f),
            rogers_satchell: self.rogers_satchell.map(|v| v * f),
            yang_zhang: self.yang_zhang.map(|v| v * f),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RollingPoint {
    pub ts: String,
    pub cc: Option<f64>,
    pub parkinson: Option<f64>,
    pub yang_zhang: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Cone {
    /// Window length in bars.
    pub window: usize,
    pub min: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub max: f64,
    /// The most recent window's value, to read against the cone.
    pub current: f64,
    /// Percentile rank of `current` within the window's history (0..1).
    pub current_rank: f64,
    pub samples: usize,
}

#[derive(Debug, Serialize)]
pub struct VolReport {
    pub bars: usize,
    pub periods_per_year: f64,
    /// Whole-window annualized volatility by estimator.
    pub annual: Estimates,
    pub window: usize,
    /// Rolling annualized volatility (thinned for charting).
    pub rolling: Vec<RollingPoint>,
    /// Close-to-close volatility cones, annualized.
    pub cones: Vec<Cone>,
    pub garch: Option<Garch>,
}

/// Every estimator over the whole sample plus rolling series and cones.
pub fn vol_report(bars: &[Bar], ts: &[String], ppy: f64, window: usize, horizon: usize) -> Option<VolReport> {
    let keep: Vec<usize> = (0..bars.len()).filter(|&i| valid(&bars[i])).collect();
    if keep.len() < 30 {
        return None;
    }
    let bars: Vec<Bar> = keep.iter().map(|&i| bars[i]).collect();
    let ts: Vec<String> = keep.iter().map(|&i| ts.get(i).cloned().unwrap_or_default()).collect();
    let ann = ppy.sqrt();
    let annual = estimators_window(&bars, None).scaled(ann);
    let window = window.clamp(5, bars.len() / 2);

    // Rolling window, recomputed per bar: O(n·w) but n ≤ 200k and w small, and exact.
    let stride = (bars.len() / 1500).max(1);
    let mut rolling = Vec::new();
    let mut i = window;
    while i < bars.len() {
        let e = estimators_window(&bars[i + 1 - window..=i], Some(bars[i - window].close));
        rolling.push(RollingPoint {
            ts: ts[i].clone(),
            cc: e.close_to_close.map(|v| v * ann),
            parkinson: e.parkinson.map(|v| v * ann),
            yang_zhang: e.yang_zhang.map(|v| v * ann),
        });
        i += stride;
    }

    // Cones on close-to-close log returns.
    let lr: Vec<f64> = bars.windows(2).map(|w| (w[1].close / w[0].close).ln()).collect();
    let cones = [5usize, 10, 21, 63, 126, 252]
        .into_iter()
        .filter(|&w| w * 3 <= lr.len())
        .map(|w| {
            let mut vals: Vec<f64> = lr.windows(w).map(|x| stddev(x) * ann).collect();
            let current = *vals.last().unwrap_or(&0.0);
            let samples = vals.len();
            vals.sort_by(f64::total_cmp);
            let below = vals.partition_point(|v| *v < current);
            Cone {
                window: w,
                min: vals[0],
                p10: quantile_sorted(&vals, 0.10),
                p25: quantile_sorted(&vals, 0.25),
                median: quantile_sorted(&vals, 0.50),
                p75: quantile_sorted(&vals, 0.75),
                p90: quantile_sorted(&vals, 0.90),
                max: vals[vals.len() - 1],
                current,
                current_rank: below as f64 / samples as f64,
                samples,
            }
        })
        .collect();

    let pct: Vec<f64> = bars.windows(2).map(|w| (w[1].close / w[0].close - 1.0) * 100.0).collect();
    let garch = garch11(&pct, ppy, horizon);
    Some(VolReport { bars: bars.len(), periods_per_year: ppy, annual, window, rolling, cones, garch })
}

// ── GARCH(1,1) ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Garch {
    /// Constant mean of the percent returns.
    pub mu: f64,
    pub omega: f64,
    pub alpha: f64,
    pub beta: f64,
    pub persistence: f64,
    pub loglik: f64,
    /// Long-run annualized volatility implied by the fit, as a fraction.
    pub long_run_vol: Option<f64>,
    /// Periods for a variance shock to halve.
    pub half_life: Option<f64>,
    /// Annualized conditional volatility over the sample (thinned), fraction.
    pub conditional: Vec<f64>,
    /// Annualized volatility forecast for horizons 1..=h, fraction.
    pub forecast: Vec<f64>,
}

fn garch_sigma2(eps: &[f64], omega: f64, alpha: f64, beta: f64, backcast: f64, out: &mut Vec<f64>) {
    out.clear();
    let mut prev_e2 = backcast;
    let mut prev_s2 = backcast;
    for e in eps {
        let s2 = omega + alpha * prev_e2 + beta * prev_s2;
        out.push(s2);
        prev_e2 = e * e;
        prev_s2 = s2;
    }
}

fn garch_nll(r: &[f64], p: &[f64], backcast: f64, buf: &mut Vec<f64>) -> f64 {
    let (mu, omega, alpha, beta) = (p[0], p[1], p[2], p[3]);
    if omega <= 0.0 || alpha < 0.0 || beta < 0.0 || alpha + beta >= 1.0 {
        return f64::INFINITY;
    }
    let eps: Vec<f64> = r.iter().map(|x| x - mu).collect();
    garch_sigma2(&eps, omega, alpha, beta, backcast, buf);
    let mut ll = 0.0;
    for (e, s2) in eps.iter().zip(buf.iter()) {
        if *s2 <= 0.0 {
            return f64::INFINITY;
        }
        ll += (2.0 * std::f64::consts::PI).ln() + s2.ln() + e * e / s2;
    }
    0.5 * ll
}

/// Fit GARCH(1,1) to returns given in PERCENT and forecast `horizon` periods ahead.
pub fn garch11(r: &[f64], ppy: f64, horizon: usize) -> Option<Garch> {
    if r.len() < 100 {
        return None;
    }
    let m = mean(r);
    let var = stddev(r).powi(2);
    let dem: Vec<f64> = r.iter().map(|x| x - m).collect();
    let tau = dem.len().min(75);
    let w: Vec<f64> = (0..tau).map(|i| 0.94f64.powi(i as i32)).collect();
    let ws: f64 = w.iter().sum();
    let backcast: f64 = (0..tau).map(|i| dem[i] * dem[i] * w[i] / ws).sum();

    let mut buf = Vec::with_capacity(r.len());
    let mut f = |p: &[f64]| garch_nll(r, p, backcast, &mut buf);
    // A few starts: the likelihood is flat along α + β, so seed across persistence levels and
    // keep the best, then polish.
    let mut best: Option<(Vec<f64>, f64)> = None;
    for (a0, b0) in [(0.05, 0.90), (0.10, 0.85), (0.10, 0.80), (0.03, 0.95), (0.2, 0.7)] {
        let x0 = [m, var * (1.0 - a0 - b0), a0, b0];
        let (x, v) = linalg::nelder_mead(&mut f, &x0, &[0.05 * var.sqrt(), 0.3 * x0[1], 0.02, 0.02], 6000, 1e-14);
        if best.as_ref().is_none_or(|(_, bv)| v < *bv) {
            best = Some((x, v));
        }
    }
    let (x, _) = best?;
    let (x, v) = linalg::nelder_mead(&mut f, &x, &[0.01 * var.sqrt(), 0.05 * x[1].abs().max(1e-6), 0.005, 0.005], 6000, 1e-15);
    if !v.is_finite() {
        return None;
    }
    let (mu, omega, alpha, beta) = (x[0], x[1], x[2], x[3]);
    let eps: Vec<f64> = r.iter().map(|v| v - mu).collect();
    let mut s2 = Vec::new();
    garch_sigma2(&eps, omega, alpha, beta, backcast, &mut s2);
    let persistence = alpha + beta;
    let ann = |var_pct: f64| var_pct.max(0.0).sqrt() / 100.0 * ppy.sqrt();
    let lr_var = (persistence < 1.0).then(|| omega / (1.0 - persistence));
    // h-step forecast: σ²_{T+1} from the last observation, then mean reversion to the long run.
    let last_e = *eps.last()?;
    let last_s2 = *s2.last()?;
    let mut fc = Vec::with_capacity(horizon);
    let mut next = omega + alpha * last_e * last_e + beta * last_s2;
    for _ in 0..horizon.max(1) {
        fc.push(ann(next));
        next = omega + persistence * next;
    }
    let stride = (s2.len() / 1500).max(1);
    Some(Garch {
        mu,
        omega,
        alpha,
        beta,
        persistence,
        loglik: -v,
        long_run_vol: lr_var.map(ann),
        half_life: (persistence > 0.0 && persistence < 1.0).then(|| (0.5f64).ln() / persistence.ln()),
        conditional: s2.iter().step_by(stride).map(|v| ann(*v)).collect(),
        forecast: fc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_with_no_range_has_no_range_vol() {
        let bars = vec![Bar { open: 10.0, high: 10.0, low: 10.0, close: 10.0 }; 40];
        let e = estimators_window(&bars, None);
        assert_eq!(e.parkinson, Some(0.0));
        assert_eq!(e.garman_klass, Some(0.0));
    }

    #[test]
    fn parkinson_matches_its_closed_form() {
        // Constant 2% high-low range: σ = ln(1.02) / (2 √ln 2).
        let bars = vec![Bar { open: 100.0, high: 101.0, low: 101.0 / 1.02, close: 100.0 }; 50];
        let e = estimators_window(&bars, None);
        let expect = (1.02f64).ln() / (2.0 * std::f64::consts::LN_2.sqrt());
        assert!((e.parkinson.unwrap() - expect).abs() < 1e-12);
    }
}
