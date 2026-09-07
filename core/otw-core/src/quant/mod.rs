//! Quant tools — pure risk/return math over price series.
//!
//! Everything here is stateless and dependency-free: callers pass aligned close-price
//! series (or simple period returns) and get back metrics ready to serialize. The API
//! layer loads bars from histdata and aligns multi-asset series through
//! [`crate::align`] before calling in.
//!
//! Conventions:
//! - "returns" are simple period returns r_t = p_t / p_{t-1} - 1.
//! - Volatility/VaR are annualized using `periods_per_year` inferred from the dataset
//!   timeframe (see [`periods_per_year`]).
//! - VaR/CVaR are reported as positive loss fractions (0.05 = a 5% loss).

use serde::Serialize;

/// Trading periods per year for a histdata timeframe string (e.g. "1d", "1h", "15m").
/// Falls back to daily (252) for anything unrecognized.
///
/// This is the **nominal** factor, a convention table: it cannot know that a daily crypto
/// series holds 365 periods a year and a daily equity series 252, and it is meaningless for
/// a merged multi-asset clock. Multi-asset callers annualize with
/// [`crate::align::observed_ppy`], which measures the clock, and pass this only as the
/// fallback for a sample too short to measure.
pub fn periods_per_year(timeframe: &str) -> f64 {
    if let Ok(tf) = crate::timeframe::Timeframe::parse(timeframe) {
        return tf.periods_per_year();
    }
    // Unrecognized: keep the historical behaviour of reading the leading count, if any, and
    // treating the rest as days.
    let tf = timeframe.trim().to_lowercase();
    let split = tf.find(|c: char| c.is_alphabetic()).unwrap_or(tf.len());
    let n: f64 = tf[..split].parse().unwrap_or(1.0);
    if n <= 0.0 {
        return 252.0;
    }
    (252.0 / n).max(1.0)
}

/// Simple period returns from a close series. Length is `closes.len() - 1`.
pub fn returns(closes: &[f64]) -> Vec<f64> {
    closes
        .windows(2)
        .map(|w| if w[0] != 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
        .collect()
}

pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample standard deviation (n-1). Returns 0 for fewer than 2 points.
pub fn stddev(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() as f64 - 1.0);
    var.sqrt()
}

// ── Multiple-testing correction (probabilistic / deflated Sharpe) ────────────────────────
//
// The best of N backtests is a maximum, and a maximum is biased upward even when every
// strategy in the grid is worthless. Bailey & López de Prado's deflated Sharpe asks the only
// question that matters after a sweep: is the winner better than the best you would EXPECT to
// see from N tries of nothing?
//
// This lives next to the sweep because the trial count is what makes it computable — a single
// run has no N, and a Sharpe reported without one is unfalsifiable.

/// Standard normal CDF, via the Abramowitz & Stegun 7.1.26 error-function approximation
/// (|error| < 1.5e-7 — far below the precision of any Sharpe estimated from a backtest).
fn norm_cdf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let z = x.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-z * z).exp();
    0.5 * (1.0 + sign * y)
}

/// The spread of a set of trial results — the thing a sweep must report instead of its max.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Spread {
    pub n: usize,
    pub min: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
}

/// Percentile of a sorted slice (nearest-rank).
fn pct_of(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p * (sorted.len() - 1) as f64).round() as usize).min(sorted.len() - 1);
    sorted[idx]
}

pub fn spread(xs: &[f64]) -> Option<Spread> {
    if xs.is_empty() {
        return None;
    }
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(Spread {
        n: s.len(),
        min: s[0],
        p25: pct_of(&s, 0.25),
        median: pct_of(&s, 0.5),
        p75: pct_of(&s, 0.75),
        max: s[s.len() - 1],
        mean: mean(&s),
        stddev: stddev(&s),
    })
}

/// The multiple-testing verdict on a sweep.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeflatedSharpe {
    /// Trials the sweep ran — the N the whole correction is about.
    pub trials: usize,
    /// Observations behind each trial's Sharpe (bars simulated).
    pub observations: usize,
    /// Best trial Sharpe, annualized, as reported by the engine.
    pub best_sharpe: f64,
    /// The Sharpe you would expect the BEST of N worthless strategies to show, given how much
    /// the trials varied. The bar the winner has to clear.
    pub expected_max_sharpe: f64,
    /// P(true Sharpe > 0) for the best trial, ignoring selection — the optimistic reading.
    pub probabilistic_sharpe: f64,
    /// P(true Sharpe > expected_max) for the best trial — the honest one after N tries.
    pub deflated_sharpe: f64,
    /// True when the best trial does not clear the selection bar.
    pub selection_explains_it: bool,
    pub note: String,
}

/// Deflated Sharpe for a set of trial Sharpes (annualized), given the number of observations
/// each was computed over and the periods-per-year those Sharpes were annualized with
/// (see [`periods_per_year`]) — the latter is what de-annualizes them back to the estimate's
/// own scale, so passing the wrong one silently flattens PSR against the sample size.
///
/// Simplifying assumptions, stated because they change the number: returns are treated as
/// normal (no skew/kurtosis adjustment — the engine does not carry the moments of the equity
/// curve), and the trials are treated as if they varied independently. Both make this a
/// *floor* on how much to discount the winner, never a ceiling.
pub fn deflated_sharpe(
    trial_sharpes: &[f64],
    observations: usize,
    periods_per_year: f64,
) -> Option<DeflatedSharpe> {
    let n_trials = trial_sharpes.len();
    if n_trials == 0 || observations < 3 {
        return None;
    }
    let best = trial_sharpes.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if !best.is_finite() {
        return None;
    }
    let sd_trials = stddev(trial_sharpes);
    // Expected maximum of N draws (Gumbel approximation from the extreme-value literature).
    // With one trial there is no selection to correct for, and with identical trials there is
    // no spread to extrapolate — both leave the bar at zero, which is the honest default.
    const EULER: f64 = 0.577_215_664_901_532_9;
    let expected_max = if n_trials < 2 || sd_trials <= 0.0 {
        0.0
    } else {
        let n = n_trials as f64;
        sd_trials
            * ((1.0 - EULER) * norm_ppf(1.0 - 1.0 / n)
                + EULER * norm_ppf(1.0 - 1.0 / (n * std::f64::consts::E)))
    };
    // PSR(threshold): P(true SR > threshold) under normality, with the Sharpe expressed
    // per-observation (the engine annualizes; the test is on the estimate's own scale).
    //
    // De-annualize by `sqrt(periods_per_year)` — the exact factor `risk_ratios` multiplied by —
    // NOT by `sqrt(observations)`. Those coincide only for a one-year run, and using the wrong
    // one cancels the `sqrt(t - 1)` term below, which flattens PSR against sample size and
    // destroys the whole point of the statistic (a 10-year record must beat a 6-month one).
    let t = observations as f64;
    let ann = periods_per_year.max(1.0).sqrt();
    // A per-observation Sharpe at or above 1 is outside the model (the estimator's variance
    // term `1 - sr^2` goes non-positive). Refuse the whole verdict rather than clamping it into
    // a confident-looking number.
    if (best / ann).abs() >= 1.0 {
        return None;
    }
    let psr = |threshold: f64| -> f64 {
        let sr = best / ann;
        let thr = threshold / ann;
        let denom = (1.0 - sr * sr).sqrt();
        norm_cdf((sr - thr) * (t - 1.0).sqrt() / denom)
    };
    let dsr = psr(expected_max);
    let selection_explains_it = best <= expected_max;
    let note = if selection_explains_it {
        format!(
            "The best of these {n_trials} trials (Sharpe {best:.2}) does not beat what {n_trials} \
             tries of a worthless strategy would be expected to produce ({expected_max:.2}). \
             Report this as no evidence of an edge, not as a result."
        )
    } else {
        format!(
            "Best of {n_trials} trials: Sharpe {best:.2} against a selection bar of \
             {expected_max:.2}. Report the trial spread alongside it — the headline number is \
             the maximum of {n_trials} draws, not an expectation."
        )
    };
    Some(DeflatedSharpe {
        trials: n_trials,
        observations,
        best_sharpe: best,
        expected_max_sharpe: expected_max,
        probabilistic_sharpe: psr(0.0),
        deflated_sharpe: dsr,
        selection_explains_it,
        note,
    })
}

fn covariance(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let (ma, mb) = (mean(&a[..n]), mean(&b[..n]));
    let s: f64 = (0..n).map(|i| (a[i] - ma) * (b[i] - mb)).sum();
    s / (n as f64 - 1.0)
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let (sa, sb) = (stddev(a), stddev(b));
    if sa == 0.0 || sb == 0.0 {
        return 0.0;
    }
    covariance(a, b) / (sa * sb)
}

// ── Single-asset metrics ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DrawdownPoint {
    pub ts: String,
    /// Drawdown from running peak, as a negative fraction (-0.1 = 10% below peak).
    pub dd: f64,
}

#[derive(Debug, Serialize)]
pub struct SingleResult {
    pub periods: usize,
    pub periods_per_year: f64,
    /// Annualized historical volatility (stddev of returns × √periods/yr).
    pub hv_annual: f64,
    /// Per-period volatility (raw stddev of returns).
    pub hv_period: f64,
    /// Worst peak-to-trough drop as a positive fraction (0.42 = -42%).
    pub max_drawdown: f64,
    /// Historical VaR at the chosen confidence, as a positive loss fraction.
    pub var_hist: f64,
    /// Parametric (normal) VaR at the chosen confidence.
    pub var_param: f64,
    /// Conditional VaR (expected shortfall) beyond the historical VaR.
    pub cvar: f64,
    pub confidence: f64,
    pub mean_return: f64,
    /// Drawdown curve for charting.
    pub drawdown_curve: Vec<DrawdownPoint>,
    /// Return distribution as histogram bins for charting.
    pub histogram: Histogram,
}

#[derive(Debug, Serialize)]
pub struct Histogram {
    /// Bin edges (len = bins + 1).
    pub edges: Vec<f64>,
    /// Count in each bin (len = bins).
    pub counts: Vec<u64>,
}

fn histogram(xs: &[f64], bins: usize) -> Histogram {
    let bins = bins.max(1);
    if xs.is_empty() {
        return Histogram { edges: vec![0.0, 0.0], counts: vec![0] };
    }
    let lo = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = if hi > lo { hi - lo } else { 1.0 };
    let edges: Vec<f64> = (0..=bins).map(|i| lo + span * i as f64 / bins as f64).collect();
    let mut counts = vec![0u64; bins];
    for &x in xs {
        let mut idx = ((x - lo) / span * bins as f64) as usize;
        if idx >= bins {
            idx = bins - 1;
        }
        counts[idx] += 1;
    }
    Histogram { edges, counts }
}

/// Max drawdown and the per-period drawdown curve from a close series.
pub fn drawdown(closes: &[f64], ts: &[String]) -> (f64, Vec<DrawdownPoint>) {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    let mut curve = Vec::with_capacity(closes.len());
    for (i, &p) in closes.iter().enumerate() {
        if p > peak {
            peak = p;
        }
        let dd = if peak > 0.0 { p / peak - 1.0 } else { 0.0 };
        if -dd > max_dd {
            max_dd = -dd;
        }
        curve.push(DrawdownPoint { ts: ts.get(i).cloned().unwrap_or_default(), dd });
    }
    (max_dd, curve)
}

// ── Portfolio measures (shared with the portfolio analytics registry) ───────────
//
// These live here rather than in the portfolio module for one reason: they are pure
// functions of a return series, and the backtest, the journal and the tracker all have one.
// Anything that needs a portfolio's ledger stays in the portfolio module.

/// Compound a series of period returns into a 1.0-based equity index of length `rets.len() + 1`.
///
/// This is where a deposit-adjusted (time-weighted) curve becomes measurable: the caller
/// removes the external flow when it computes each return, so the index below reflects what
/// the *investments* did and not how much money was added to them.
pub fn compound(rets: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(rets.len() + 1);
    let mut v = 1.0;
    out.push(v);
    for r in rets {
        v *= 1.0 + r;
        out.push(v);
    }
    out
}

/// Annualized growth rate of a total return realized over `years`.
///
/// Returns `None` under a fifth of a year rather than a number: annualizing three weeks
/// produces a figure that is arithmetically correct and tells the reader nothing true.
pub fn cagr(total_return: f64, years: f64) -> Option<f64> {
    if years < 0.2 || total_return <= -1.0 {
        return None;
    }
    Some((1.0 + total_return).powf(1.0 / years) - 1.0)
}

/// Deviation of the returns that fell below `target`, annualized by the caller's `ppy`.
///
/// The shortfall is measured over **every** period, not only the losing ones: dividing the
/// downside sum by the count of losses would make a strategy that rarely loses look more
/// volatile than one that loses constantly.
pub fn downside_deviation(rets: &[f64], target: f64, ppy: f64) -> f64 {
    if rets.len() < 2 {
        return 0.0;
    }
    let sum: f64 = rets.iter().map(|r| (r - target).min(0.0).powi(2)).sum();
    (sum / rets.len() as f64).sqrt() * ppy.sqrt()
}

/// Excess annual return per unit of annual volatility. `None` when there is no volatility to
/// divide by, which is a flat series, not a perfect strategy.
pub fn sharpe(ann_return: f64, ann_vol: f64, rf: f64) -> Option<f64> {
    (ann_vol > 1e-9).then(|| (ann_return - rf) / ann_vol)
}

/// Excess annual return per unit of downside deviation.
pub fn sortino(ann_return: f64, downside: f64, rf: f64) -> Option<f64> {
    (downside > 1e-9).then(|| (ann_return - rf) / downside)
}

/// Annual return per unit of worst drawdown. `max_dd` is a positive loss fraction.
pub fn calmar(ann_return: f64, max_dd: f64) -> Option<f64> {
    (max_dd > 1e-9).then(|| ann_return / max_dd)
}

/// One peak-to-trough episode of an equity curve.
#[derive(Debug, Clone, Serialize)]
pub struct Episode {
    /// Positive loss fraction at the trough (0.18 = down 18%).
    pub depth: f64,
    /// Index of the peak the fall started from.
    pub peak_at: String,
    pub trough_at: String,
    /// When the curve got back to the peak, or `None` while it is still under water.
    pub recovered_at: Option<String>,
    /// Periods from peak to trough.
    pub to_trough: usize,
    /// Periods from trough back to the peak, `None` while still under water.
    pub to_recover: Option<usize>,
    /// Periods spent below the peak, so far.
    pub under_water: usize,
}

/// Every drawdown deeper than `floor`, worst first, plus the one still open.
///
/// "Recovery time" is this table and not a scalar: a book with one 40% hole that took two
/// years to fill and a book with twelve 5% dips that filled in a week have the same average
/// and nothing else in common.
pub fn underwater(equity: &[f64], ts: &[String], floor: f64) -> Vec<Episode> {
    let label = |i: usize| ts.get(i).cloned().unwrap_or_else(|| i.to_string());
    let mut out: Vec<Episode> = Vec::new();
    let mut peak = f64::NEG_INFINITY;
    let mut peak_i = 0usize;
    // The episode currently open, if the curve is below its peak.
    let mut open: Option<(usize, usize, f64)> = None; // (peak_i, trough_i, trough value)

    for (i, &v) in equity.iter().enumerate() {
        if v >= peak {
            // Back to the high: close whatever was open, recovered on this bar.
            if let Some((p, t, tv)) = open.take() {
                let depth = if equity[p] > 0.0 { 1.0 - tv / equity[p] } else { 0.0 };
                if depth >= floor {
                    out.push(Episode {
                        depth,
                        peak_at: label(p),
                        trough_at: label(t),
                        recovered_at: Some(label(i)),
                        to_trough: t - p,
                        to_recover: Some(i - t),
                        under_water: i - p,
                    });
                }
            }
            peak = v;
            peak_i = i;
            continue;
        }
        match open.as_mut() {
            Some((_, t, tv)) if v < *tv => {
                *tv = v;
                *t = i;
            }
            Some(_) => {}
            None => open = Some((peak_i, i, v)),
        }
    }
    // Still under water at the end: reported open, never quietly dropped.
    if let Some((p, t, tv)) = open {
        let depth = if equity[p] > 0.0 { 1.0 - tv / equity[p] } else { 0.0 };
        if depth >= floor {
            let last = equity.len().saturating_sub(1);
            out.push(Episode {
                depth,
                peak_at: label(p),
                trough_at: label(t),
                recovered_at: None,
                to_trough: t - p,
                to_recover: None,
                under_water: last - p,
            });
        }
    }
    out.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Internal rate of return of dated cash flows, annualized, by bisection.
///
/// Bisection rather than Newton: a flow series with several sign changes has more than one
/// root, and a derivative method walks off to whichever one it starts nearest. The bracket is
/// deliberately wide and the answer is `None` when the sign does not change inside it, which
/// is the honest reading of "this book has no internal rate of return".
pub fn irr(flows: &[(f64, f64)]) -> Option<f64> {
    if flows.len() < 2 {
        return None;
    }
    let npv = |r: f64| -> f64 {
        flows.iter().map(|(years, amount)| amount / (1.0 + r).powf(*years)).sum()
    };
    let (mut lo, mut hi) = (-0.9999, 10.0);
    let (mut f_lo, f_hi) = (npv(lo), npv(hi));
    if f_lo.is_nan() || f_hi.is_nan() || f_lo * f_hi > 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let f_mid = npv(mid);
        if f_mid.abs() < 1e-9 {
            return Some(mid);
        }
        if f_lo * f_mid <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// Ordinary least squares of `y` on `x`: (alpha, beta, r_squared).
///
/// `None` when the sample is too short or `x` does not move: a beta fitted to a constant is
/// a division by zero wearing a decimal point.
pub fn ols(y: &[f64], x: &[f64]) -> Option<(f64, f64, f64)> {
    let n = y.len().min(x.len());
    if n < 3 {
        return None;
    }
    let (y, x) = (&y[..n], &x[..n]);
    let (my, mx) = (mean(y), mean(x));
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    let mut syy = 0.0;
    for i in 0..n {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    if sxx < 1e-18 {
        return None;
    }
    let beta = sxy / sxx;
    let alpha = my - beta * mx;
    let r2 = if syy > 1e-18 { (sxy * sxy) / (sxx * syy) } else { 0.0 };
    Some((alpha, beta, r2.clamp(0.0, 1.0)))
}

/// Historical VaR: the `confidence` quantile of losses. Returns a positive loss fraction.
fn var_historical(rets: &[f64], confidence: f64) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let mut sorted = rets.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Lower tail at (1 - confidence).
    let idx = ((1.0 - confidence) * sorted.len() as f64).floor() as usize;
    let idx = idx.min(sorted.len() - 1);
    (-sorted[idx]).max(0.0)
}

/// CVaR / expected shortfall: mean loss in the tail at or below the VaR threshold.
fn cvar(rets: &[f64], confidence: f64) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let mut sorted = rets.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let cut = ((1.0 - confidence) * sorted.len() as f64).ceil() as usize;
    let cut = cut.max(1).min(sorted.len());
    let tail = &sorted[..cut];
    (-mean(tail)).max(0.0)
}

/// Standard-normal quantile (inverse CDF), Acklam's rational approximation.
fn norm_ppf(p: f64) -> f64 {
    let p = p.clamp(1e-9, 1.0 - 1e-9);
    let a = [
        -3.969683028665376e+01, 2.209460984245205e+02, -2.759285104469687e+02,
        1.383577518672690e+02, -3.066479806614716e+01, 2.506628277459239e+00,
    ];
    let b = [
        -5.447609879822406e+01, 1.615858368580409e+02, -1.556989798598866e+02,
        6.680131188771972e+01, -1.328068155288572e+01,
    ];
    let c = [
        -7.784894002430293e-03, -3.223964580411365e-01, -2.400758277161838e+00,
        -2.549732539343734e+00, 4.374664141464968e+00, 2.938163982698783e+00,
    ];
    let d = [
        7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    let plow = 0.02425;
    let phigh = 1.0 - plow;
    if p < plow {
        let q = (-2.0 * p.ln()).sqrt();
        (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    } else if p <= phigh {
        let q = p - 0.5;
        let r = q * q;
        (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
            / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    }
}

/// Full single-asset analysis.
pub fn analyze_single(
    closes: &[f64],
    ts: &[String],
    timeframe: &str,
    confidence: f64,
) -> SingleResult {
    let ppy = periods_per_year(timeframe);
    let rets = returns(closes);
    let sd = stddev(&rets);
    let m = mean(&rets);
    let (max_dd, curve) = drawdown(closes, ts);
    // Parametric VaR: -(μ + z·σ) where z is the lower-tail quantile.
    let z = norm_ppf(1.0 - confidence);
    let var_param = (-(m + z * sd)).max(0.0);
    SingleResult {
        periods: rets.len(),
        periods_per_year: ppy,
        hv_annual: sd * ppy.sqrt(),
        hv_period: sd,
        max_drawdown: max_dd,
        var_hist: var_historical(&rets, confidence),
        var_param,
        cvar: cvar(&rets, confidence),
        confidence,
        mean_return: m,
        drawdown_curve: curve,
        histogram: histogram(&rets, 40),
    }
}

// ── Kelly (manual inputs) ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct KellyResult {
    /// Full-Kelly fraction of capital (can exceed 1 or go negative).
    pub kelly: f64,
    /// Clamped to [0, 1] for a sane sizing suggestion.
    pub kelly_clamped: f64,
    pub half_kelly: f64,
    pub quarter_kelly: f64,
    /// Reward/risk ratio b = avg_win / avg_loss.
    pub payoff: f64,
}

/// Kelly from win rate and average win/loss (both entered as positive magnitudes).
/// f* = p - (1 - p) / b, with b = avg_win / avg_loss.
pub fn kelly(win_rate: f64, avg_win: f64, avg_loss: f64) -> KellyResult {
    let p = win_rate.clamp(0.0, 1.0);
    let b = if avg_loss > 0.0 { avg_win / avg_loss } else { 0.0 };
    let f = if b > 0.0 { p - (1.0 - p) / b } else { 0.0 };
    let clamped = f.clamp(0.0, 1.0);
    KellyResult {
        kelly: f,
        kelly_clamped: clamped,
        half_kelly: clamped * 0.5,
        quarter_kelly: clamped * 0.25,
        payoff: b,
    }
}

// ── Position sizing ─────────────────────────────────────────────────────────────

/// Direction of the trade — determines which side of entry the stop sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Long,
    Short,
}

#[derive(Debug, Serialize)]
pub struct PositionSizeResult {
    /// Currency amount put at risk (risk_pct × stack, or the fixed amount).
    pub risk_amount: f64,
    /// Distance from entry to stop, in price units.
    pub stop_distance: f64,
    /// Per-unit loss if the stop is hit (== stop_distance for a 1× multiplier).
    pub risk_per_unit: f64,
    /// Suggested quantity (units/contracts). 0 when inputs are incoherent.
    pub quantity: f64,
    /// Position notional = quantity × entry × multiplier.
    pub notional: f64,
    /// Risk as a fraction of stack (risk_amount / stack).
    pub risk_fraction: f64,
    /// Notional as a fraction of stack (a leverage read-through).
    pub exposure_fraction: f64,
    /// Margin required at the chosen leverage (notional / leverage). None when no leverage given.
    pub margin_required: Option<f64>,
    /// Optional take-profit read-through.
    pub target_distance: Option<f64>,
    pub reward_amount: Option<f64>,
    /// Reward-to-risk ratio (target_distance / stop_distance).
    pub reward_risk: Option<f64>,
    /// True when notional exceeds what the stack supports at the given leverage (1× if none).
    pub over_leveraged: bool,
    /// Human-readable cautions (empty stop, risk > stack, over-leverage, …).
    pub warnings: Vec<String>,
}

/// Risk-based position sizing.
///
/// `risk_amount` is the currency at risk (caller resolves %-of-stack vs. fixed before calling).
/// `multiplier` scales price moves to P&L per unit (contract size / lot multiplier; 1.0 for spot).
/// `leverage` (>1) reports required margin and drives the over-leverage check; `None`/≤1 means cash.
/// `target` is an optional take-profit price for the reward read-through.
#[allow(clippy::too_many_arguments)]
pub fn position_size(
    stack: f64,
    risk_amount: f64,
    entry: f64,
    stop: f64,
    side: Side,
    multiplier: f64,
    leverage: Option<f64>,
    target: Option<f64>,
) -> PositionSizeResult {
    let mult = if multiplier > 0.0 { multiplier } else { 1.0 };
    let stop_distance = (entry - stop).abs();
    let risk_per_unit = stop_distance * mult;
    let mut warnings = Vec::new();

    // A stop on the wrong side of entry can't cap the loss it claims to.
    let stop_ok = match side {
        Side::Long => stop < entry,
        Side::Short => stop > entry,
    };
    if stop_distance > 0.0 && !stop_ok {
        warnings.push("Stop is on the wrong side of entry for this direction.".into());
    }

    let quantity = if risk_per_unit > 0.0 { risk_amount / risk_per_unit } else { 0.0 };
    let notional = quantity * entry * mult;

    let lev = leverage.filter(|l| *l > 0.0);
    let margin_required = lev.map(|l| notional / l);
    // Buying power = stack × leverage (or stack itself when unlevered).
    let buying_power = stack * lev.unwrap_or(1.0);
    let over_leveraged = stack > 0.0 && notional > buying_power + 1e-9;

    let risk_fraction = if stack > 0.0 { risk_amount / stack } else { 0.0 };
    let exposure_fraction = if stack > 0.0 { notional / stack } else { 0.0 };

    if stop_distance == 0.0 {
        warnings.push("Entry and stop are equal — no stop distance to size against.".into());
    }
    if stack > 0.0 && risk_amount > stack {
        warnings.push("Risk amount exceeds the whole stack.".into());
    }
    if over_leveraged {
        warnings.push("Position notional exceeds available buying power.".into());
    }

    let (target_distance, reward_amount, reward_risk) = match target {
        Some(tp) => {
            let td = (tp - entry).abs();
            let rew = td * mult * quantity;
            let rr = if stop_distance > 0.0 { Some(td / stop_distance) } else { None };
            (Some(td), Some(rew), rr)
        }
        None => (None, None, None),
    };

    PositionSizeResult {
        risk_amount,
        stop_distance,
        risk_per_unit,
        quantity,
        notional,
        risk_fraction,
        exposure_fraction,
        margin_required,
        target_distance,
        reward_amount,
        reward_risk,
        over_leveraged,
        warnings,
    }
}

// ── Asset-derived stop signals ──────────────────────────────────────────────────

/// Average True Range over the last `period` bars (Wilder true range from OHLC).
/// Falls back to close-to-close range when highs/lows are absent (all zero).
fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> f64 {
    let n = closes.len();
    if n < 2 {
        return 0.0;
    }
    let mut trs = Vec::with_capacity(n - 1);
    for i in 1..n {
        let h = highs.get(i).copied().unwrap_or(closes[i]);
        let l = lows.get(i).copied().unwrap_or(closes[i]);
        let pc = closes[i - 1];
        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());
        trs.push(tr);
    }
    let take = period.min(trs.len()).max(1);
    let tail = &trs[trs.len() - take..];
    mean(tail)
}

/// One suggested stop, expressed both as a price distance and a placed stop price.
#[derive(Debug, Serialize)]
pub struct StopSuggestion {
    /// Stable key for the client (e.g. "hv_2sigma", "atr_2", "swing").
    pub key: String,
    pub label: String,
    /// Stop distance in price units.
    pub distance: f64,
    /// Stop price placed on the correct side of `entry` for the trade direction.
    pub stop_price: f64,
    /// Distance as a fraction of entry (for display).
    pub distance_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct AssetSignals {
    pub periods: usize,
    pub periods_per_year: f64,
    /// Per-period volatility (stddev of returns).
    pub hv_period: f64,
    pub hv_annual: f64,
    /// ATR over the window (last `atr_period` bars).
    pub atr: f64,
    /// Latest close in the window (a convenient default entry).
    pub last_close: f64,
    pub suggestions: Vec<StopSuggestion>,
}

/// Derive candidate stop distances for an asset from its OHLC series and a chosen `entry`.
///
/// Each signal is measured as a **fraction of the asset's own price** (σ of returns, ATR / last
/// close, swing depth / last close) and then applied to the user's `entry`. This keeps stops
/// coherent even when `entry` is a placeholder (e.g. 100) rather than the live asset price —
/// otherwise an absolute ATR of ~390 on BTC would be subtracted from a 100 entry into nonsense.
///
/// Produces vol-based (1σ/2σ of per-period returns), ATR-based (1×/2× ATR) and a recent swing
/// stop (lowest low for longs / highest high for shorts over the lookback).
pub fn asset_signals(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    ts: &[String],
    timeframe: &str,
    entry: f64,
    side: Side,
    atr_period: usize,
    swing_lookback: usize,
) -> AssetSignals {
    let _ = ts;
    let ppy = periods_per_year(timeframe);
    let rets = returns(closes);
    let sd = stddev(&rets);
    let a = atr(highs, lows, closes, atr_period.max(1));
    let last_close = closes.last().copied().unwrap_or(entry);
    let e = if entry > 0.0 { entry } else { last_close };
    // Reference price the ATR/swing were measured against (guarded for degenerate data).
    let ref_px = if last_close > 0.0 { last_close } else { e };

    let place = |dist: f64| match side {
        Side::Long => e - dist,
        Side::Short => e + dist,
    };
    // `frac` is the stop depth as a fraction of price; the distance in the user's entry units is
    // frac × entry, so the suggestion scales to whatever entry the user typed.
    let mk = |key: &str, label: &str, frac: f64| {
        let dist = frac.max(0.0) * e;
        StopSuggestion {
            key: key.into(),
            label: label.into(),
            distance: dist,
            stop_price: place(dist),
            distance_pct: frac.max(0.0),
        }
    };

    let atr_frac = if ref_px > 0.0 { a / ref_px } else { 0.0 };
    let mut suggestions = vec![
        mk("hv_1sigma", "1σ move (per-period vol)", sd),
        mk("hv_2sigma", "2σ move (per-period vol)", 2.0 * sd),
        mk("atr_1", "1× ATR", atr_frac),
        mk("atr_2", "2× ATR", 2.0 * atr_frac),
    ];

    // Recent swing stop: depth from the reference price to the window extreme, as a fraction.
    let look = swing_lookback.max(1).min(closes.len());
    if look >= 1 {
        let tail_lo = &lows[lows.len().saturating_sub(look)..];
        let tail_hi = &highs[highs.len().saturating_sub(look)..];
        let swing_frac = match side {
            Side::Long => {
                let lo = tail_lo.iter().cloned().fold(f64::INFINITY, f64::min);
                if ref_px > 0.0 { (ref_px - lo).max(0.0) / ref_px } else { 0.0 }
            }
            Side::Short => {
                let hi = tail_hi.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                if ref_px > 0.0 { (hi - ref_px).max(0.0) / ref_px } else { 0.0 }
            }
        };
        if swing_frac.is_finite() && swing_frac > 0.0 {
            suggestions.push(mk("swing", &format!("Recent swing ({look} bars)"), swing_frac));
        }
    }

    AssetSignals {
        periods: rets.len(),
        periods_per_year: ppy,
        hv_period: sd,
        hv_annual: sd * ppy.sqrt(),
        atr: a,
        last_close,
        suggestions,
    }
}

// ── Multi-asset: correlation, frontier, risk parity ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct CorrelationMatrix {
    pub labels: Vec<String>,
    /// Row-major NxN correlation matrix.
    pub matrix: Vec<Vec<f64>>,
}

/// Pairwise correlation matrix of aligned return series.
pub fn correlation_matrix(labels: &[String], rets: &[Vec<f64>]) -> CorrelationMatrix {
    let n = rets.len();
    let mut matrix = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            matrix[i][j] = if i == j { 1.0 } else { correlation(&rets[i], &rets[j]) };
        }
    }
    CorrelationMatrix { labels: labels.to_vec(), matrix }
}

#[derive(Debug, Serialize)]
pub struct FrontierPoint {
    pub ret: f64,
    pub vol: f64,
    pub sharpe: f64,
    pub weights: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct FrontierResult {
    pub labels: Vec<String>,
    /// Monte-Carlo random portfolios (annualized).
    pub cloud: Vec<FrontierPoint>,
    pub min_vol: FrontierPoint,
    pub max_sharpe: FrontierPoint,
}

/// xorshift64 — deterministic, dependency-free RNG for reproducible clouds.
struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        // 53-bit mantissa → [0, 1).
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn annualized_mean(rets: &[Vec<f64>], ppy: f64) -> Vec<f64> {
    rets.iter().map(|r| mean(r) * ppy).collect()
}

fn cov_matrix(rets: &[Vec<f64>], ppy: f64) -> Vec<Vec<f64>> {
    let n = rets.len();
    let mut c = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            c[i][j] = covariance(&rets[i], &rets[j]) * ppy;
        }
    }
    c
}

fn port_return(mu: &[f64], w: &[f64]) -> f64 {
    mu.iter().zip(w).map(|(m, x)| m * x).sum()
}

fn port_vol(cov: &[Vec<f64>], w: &[f64]) -> f64 {
    let n = w.len();
    let mut v = 0.0;
    for i in 0..n {
        for j in 0..n {
            v += w[i] * w[j] * cov[i][j];
        }
    }
    v.max(0.0).sqrt()
}

fn point(mu: &[f64], cov: &[Vec<f64>], w: Vec<f64>, rf: f64) -> FrontierPoint {
    let ret = port_return(mu, &w);
    let vol = port_vol(cov, &w);
    let sharpe = if vol > 0.0 { (ret - rf) / vol } else { 0.0 };
    FrontierPoint { ret, vol, sharpe, weights: w }
}

/// Monte-Carlo efficient frontier over long-only fully-invested portfolios.
pub fn efficient_frontier(
    labels: &[String],
    rets: &[Vec<f64>],
    ppy: f64,
    samples: usize,
    rf: f64,
) -> FrontierResult {
    let n = rets.len();
    let mu = annualized_mean(rets, ppy);
    let cov = cov_matrix(rets, ppy);
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut cloud = Vec::with_capacity(samples);
    let mut best_sharpe = f64::NEG_INFINITY;
    let mut min_vol_val = f64::INFINITY;
    let mut max_sharpe_w = vec![1.0 / n as f64; n];
    let mut min_vol_w = vec![1.0 / n as f64; n];
    for _ in 0..samples {
        // Dirichlet-ish: uniform weights normalized.
        let raw: Vec<f64> = (0..n).map(|_| rng.next_f64() + 1e-9).collect();
        let s: f64 = raw.iter().sum();
        let w: Vec<f64> = raw.iter().map(|x| x / s).collect();
        let p = point(&mu, &cov, w.clone(), rf);
        if p.sharpe > best_sharpe {
            best_sharpe = p.sharpe;
            max_sharpe_w = w.clone();
        }
        if p.vol < min_vol_val {
            min_vol_val = p.vol;
            min_vol_w = w.clone();
        }
        cloud.push(p);
    }
    FrontierResult {
        labels: labels.to_vec(),
        cloud,
        min_vol: point(&mu, &cov, min_vol_w, rf),
        max_sharpe: point(&mu, &cov, max_sharpe_w, rf),
    }
}

#[derive(Debug, Serialize)]
pub struct RiskParityResult {
    pub labels: Vec<String>,
    pub weights: Vec<f64>,
    /// Each asset's share of total portfolio risk (≈ equal at convergence).
    pub risk_contribution: Vec<f64>,
    /// Annualized portfolio volatility at these weights.
    pub portfolio_vol: f64,
}

/// Risk parity via iterative inverse-vol-of-marginal-contribution updates.
/// Long-only, fully invested. Converges to equal risk contributions for typical inputs.
pub fn risk_parity(labels: &[String], rets: &[Vec<f64>], ppy: f64) -> RiskParityResult {
    let n = rets.len();
    let cov = cov_matrix(rets, ppy);
    // Seed with inverse-volatility weights.
    let vols: Vec<f64> = (0..n).map(|i| cov[i][i].max(1e-12).sqrt()).collect();
    let mut w: Vec<f64> = vols.iter().map(|v| 1.0 / v).collect();
    let s: f64 = w.iter().sum();
    for x in &mut w {
        *x /= s;
    }
    let target = 1.0 / n as f64;
    for _ in 0..500 {
        // Marginal risk contribution: (Σw)_i. Total risk = w' Σ w.
        let mut sigma_w = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                sigma_w[i] += cov[i][j] * w[j];
            }
        }
        let total: f64 = (0..n).map(|i| w[i] * sigma_w[i]).sum();
        if total <= 0.0 {
            break;
        }
        // Each asset's risk-contribution share; nudge weights toward equal shares.
        let mut new_w = w.clone();
        for i in 0..n {
            let rc = w[i] * sigma_w[i] / total;
            if rc > 0.0 {
                new_w[i] = w[i] * (target / rc).powf(0.5);
            }
        }
        let ns: f64 = new_w.iter().sum();
        for x in &mut new_w {
            *x /= ns;
        }
        let delta: f64 = (0..n).map(|i| (new_w[i] - w[i]).abs()).sum();
        w = new_w;
        if delta < 1e-9 {
            break;
        }
    }
    // Final risk contributions.
    let mut sigma_w = vec![0.0; n];
    for i in 0..n {
        for j in 0..n {
            sigma_w[i] += cov[i][j] * w[j];
        }
    }
    let total: f64 = (0..n).map(|i| w[i] * sigma_w[i]).sum();
    let rc: Vec<f64> = (0..n)
        .map(|i| if total > 0.0 { w[i] * sigma_w[i] / total } else { 0.0 })
        .collect();
    RiskParityResult {
        labels: labels.to_vec(),
        weights: w.clone(),
        risk_contribution: rc,
        portfolio_vol: total.sqrt(),
    }
}

// ── Monte-Carlo trade-sequence resampling ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Percentiles {
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
}

/// One step of the equity fan chart: the percentile band across all simulated paths at
/// trade index `step` (0 = starting capital).
#[derive(Debug, Serialize)]
pub struct FanPoint {
    pub step: usize,
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
}

#[derive(Debug, Serialize)]
pub struct MonteCarloResult {
    /// Number of source trades resampled from.
    pub source_trades: usize,
    /// Trades per simulated path (the horizon).
    pub horizon: usize,
    pub iterations: usize,
    pub start_capital: f64,
    /// "bootstrap" (IID with replacement) or "block" (fixed-length blocks, streak-preserving).
    pub method: String,
    /// Per-step equity percentile bands for the fan chart (len = horizon + 1).
    pub fan: Vec<FanPoint>,
    /// Distribution of final equity across paths.
    pub final_equity: Percentiles,
    /// Distribution of max drawdown (positive fraction) across paths.
    pub max_drawdown: Percentiles,
    /// Fraction of paths whose equity ever fell to/below the ruin threshold.
    pub risk_of_ruin: f64,
    /// The ruin threshold in currency (fraction × start_capital) the ruin test used.
    pub ruin_level: f64,
    /// Fraction of paths ending below the starting capital.
    pub prob_loss: f64,
    /// Median final equity's total return fraction vs. start ((p50 / start) - 1).
    pub median_return: f64,
    /// Histogram of final equity for charting.
    pub final_histogram: Histogram,
    /// Histogram of max drawdown for charting.
    pub drawdown_histogram: Histogram,
    /// The actual (un-resampled) realized equity curve, for overlay/reference.
    pub actual_curve: Vec<f64>,
}

fn percentiles(sorted: &[f64]) -> Percentiles {
    let q = |p: f64| quantile_sorted(sorted, p);
    Percentiles { p5: q(0.05), p25: q(0.25), p50: q(0.50), p75: q(0.75), p95: q(0.95) }
}

/// Linear-interpolated quantile of an already-sorted slice.
fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = p.clamp(0.0, 1.0) * (sorted.len() as f64 - 1.0);
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Monte-Carlo over a realized per-trade P&L sequence (account-currency deltas).
///
/// Each of `iterations` paths draws `horizon` trades from `pnls` — either IID with replacement
/// ("bootstrap") or in contiguous blocks of `block_len` ("block", preserving win/loss streaks and
/// autocorrelation) — and walks an equity curve from `start_capital`. We record every path's
/// final equity, its worst peak-to-trough drawdown, and whether it ever breached the ruin level
/// (`ruin_pct` × start_capital). Percentile bands over the paths give the fan chart and the
/// final-equity / max-drawdown distributions; the breach rate is the risk of ruin.
pub fn monte_carlo(
    pnls: &[f64],
    start_capital: f64,
    iterations: usize,
    horizon: usize,
    block_len: usize,
    ruin_pct: f64,
    seed: u64,
) -> MonteCarloResult {
    let iterations = iterations.clamp(100, 100_000);
    let horizon = horizon.clamp(1, 20_000);
    let block = block_len.clamp(1, pnls.len().max(1));
    let use_block = block > 1;
    let ruin_level = start_capital * ruin_pct.clamp(0.0, 1.0);
    let mut rng = Rng(seed | 1);

    // The fan chart only needs ~a few hundred x positions, so sample the steps: every
    // `stride`-th step plus the last. Bounds memory (iterations × sampled steps, not
    // iterations × horizon) and keeps the JSON payload flat regardless of horizon.
    let stride = horizon.div_ceil(256).max(1);
    let fan_steps: Vec<usize> =
        (0..=horizon).filter(|s| s % stride == 0 || *s == horizon).collect();

    // Column-major accumulation: for each sampled step, collect equity across all paths so we
    // can take per-step percentiles for the fan without holding every full path.
    let mut step_equity: Vec<Vec<f64>> = vec![Vec::with_capacity(iterations); fan_steps.len()];
    let mut finals = Vec::with_capacity(iterations);
    let mut dds = Vec::with_capacity(iterations);
    let mut ruined = 0usize;
    let mut losses = 0usize;

    let n = pnls.len();
    for _ in 0..iterations {
        let mut equity = start_capital;
        let mut peak = start_capital;
        let mut max_dd = 0.0_f64;
        let mut breached = false;
        step_equity[0].push(equity);
        let mut fan_i = 1usize; // next fan_steps slot to fill

        let mut src = 0usize; // running index within the current block draw
        for step in 0..horizon {
            let pnl = if use_block {
                if step % block == 0 {
                    // Start a new block at a random offset.
                    src = (rng.next_f64() * n as f64) as usize % n.max(1);
                } else {
                    src = (src + 1) % n.max(1);
                }
                pnls[src]
            } else {
                let i = (rng.next_f64() * n as f64) as usize % n.max(1);
                pnls[i]
            };
            equity += pnl;
            if equity > peak {
                peak = equity;
            }
            if peak > 0.0 {
                max_dd = max_dd.max((peak - equity) / peak);
            }
            if !breached && equity <= ruin_level {
                breached = true;
            }
            if fan_i < fan_steps.len() && fan_steps[fan_i] == step + 1 {
                step_equity[fan_i].push(equity);
                fan_i += 1;
            }
        }

        if breached {
            ruined += 1;
        }
        if equity < start_capital {
            losses += 1;
        }
        finals.push(equity);
        dds.push(max_dd);
    }

    // Per-step percentile bands.
    let fan: Vec<FanPoint> = step_equity
        .iter_mut()
        .enumerate()
        .map(|(i, col)| {
            col.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            FanPoint {
                step: fan_steps[i],
                p5: quantile_sorted(col, 0.05),
                p25: quantile_sorted(col, 0.25),
                p50: quantile_sorted(col, 0.50),
                p75: quantile_sorted(col, 0.75),
                p95: quantile_sorted(col, 0.95),
            }
        })
        .collect();

    finals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    dds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let final_pct = percentiles(&finals);
    let median_return =
        if start_capital > 0.0 { final_pct.p50 / start_capital - 1.0 } else { 0.0 };

    // The actual realized equity curve (source order, no resampling), for overlay.
    let mut actual_curve = Vec::with_capacity(n + 1);
    let mut eq = start_capital;
    actual_curve.push(eq);
    for &p in pnls {
        eq += p;
        actual_curve.push(eq);
    }

    MonteCarloResult {
        source_trades: n,
        horizon,
        iterations,
        start_capital,
        method: if use_block { "block".into() } else { "bootstrap".into() },
        fan,
        final_equity: final_pct,
        max_drawdown: percentiles(&dds),
        risk_of_ruin: ruined as f64 / iterations as f64,
        ruin_level,
        prob_loss: losses as f64 / iterations as f64,
        median_return,
        final_histogram: histogram(&finals, 40),
        drawdown_histogram: histogram(&dds, 30),
        actual_curve,
    }
}

// ── Seasonality (month / weekday / hour buckets) ─────────────────────────────────

/// One bucket of a seasonal grouping (a month, weekday, or hour).
#[derive(Debug, Serialize)]
pub struct SeasonBucket {
    /// 0-based bucket key (0=Jan / 0=Mon / 0=00h depending on the axis).
    pub key: usize,
    /// Number of return observations in the bucket.
    pub count: usize,
    /// Mean of the chosen metric over the bucket (mean return, or per-period vol).
    pub value: f64,
    /// Fraction of positive returns in the bucket (win rate). Meaningful for the return metric.
    pub win_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct SeasonalityResult {
    pub periods: usize,
    /// "return" (mean period return) or "volatility" (stddev of period returns).
    pub metric: String,
    /// 12 month-of-year buckets (Jan..Dec).
    pub month: Vec<SeasonBucket>,
    /// 7 weekday buckets (Mon..Sun).
    pub weekday: Vec<SeasonBucket>,
    /// 24 hour-of-day buckets — omitted (empty) for daily+ timeframes where hour is degenerate.
    pub hour: Vec<SeasonBucket>,
    /// month × weekday matrix of the metric (rows = 12 months, cols = 7 weekdays); NaN→null.
    pub month_weekday: Vec<Vec<Option<f64>>>,
    /// True when the timeframe is intraday, so the hour axis is meaningful.
    pub has_hour: bool,
}

/// Aggregate a slice of (bucket-index, return) samples into `n` buckets.
fn agg_buckets(samples: &[(usize, f64)], n: usize, vol: bool) -> Vec<SeasonBucket> {
    let mut groups: Vec<Vec<f64>> = vec![Vec::new(); n];
    for &(b, r) in samples {
        if b < n {
            groups[b].push(r);
        }
    }
    groups
        .into_iter()
        .enumerate()
        .map(|(key, xs)| {
            let count = xs.len();
            let value = if vol { stddev(&xs) } else { mean(&xs) };
            let wins = xs.iter().filter(|x| **x > 0.0).count();
            let win_rate = if count > 0 { wins as f64 / count as f64 } else { 0.0 };
            SeasonBucket { key, count, value, win_rate }
        })
        .collect()
}

/// Seasonality of per-period returns bucketed by month, weekday, and (intraday only) hour.
///
/// `months`/`weekdays`/`hours` are the calendar components of each **bar** (0-based: month 0=Jan,
/// weekday 0=Mon, hour 0..23); they are aligned to `closes` (one component per bar). Returns are
/// bar-to-bar, so each return `r_t` is attributed to bar `t`'s calendar bucket. `metric` selects
/// mean return vs. per-bucket volatility. `has_hour` gates the hour axis for intraday data.
#[allow(clippy::too_many_arguments)]
pub fn seasonality(
    closes: &[f64],
    months: &[u8],
    weekdays: &[u8],
    hours: &[u8],
    metric_vol: bool,
    has_hour: bool,
) -> SeasonalityResult {
    let rets = returns(closes);
    // Attribute return r[i] (from close i→i+1) to bar i+1's calendar bucket.
    let mut m_s = Vec::with_capacity(rets.len());
    let mut w_s = Vec::with_capacity(rets.len());
    let mut h_s = Vec::with_capacity(rets.len());
    // month×weekday accumulation.
    let mut mw: Vec<Vec<Vec<f64>>> = vec![vec![Vec::new(); 7]; 12];
    for (i, &r) in rets.iter().enumerate() {
        let bar = i + 1;
        let mo = *months.get(bar).unwrap_or(&0) as usize;
        let wd = *weekdays.get(bar).unwrap_or(&0) as usize;
        let hr = *hours.get(bar).unwrap_or(&0) as usize;
        if mo < 12 {
            m_s.push((mo, r));
        }
        if wd < 7 {
            w_s.push((wd, r));
        }
        if has_hour && hr < 24 {
            h_s.push((hr, r));
        }
        if mo < 12 && wd < 7 {
            mw[mo][wd].push(r);
        }
    }

    let month_weekday: Vec<Vec<Option<f64>>> = mw
        .iter()
        .map(|row| {
            row.iter()
                .map(|xs| {
                    if xs.is_empty() {
                        None
                    } else {
                        Some(if metric_vol { stddev(xs) } else { mean(xs) })
                    }
                })
                .collect()
        })
        .collect();

    SeasonalityResult {
        periods: rets.len(),
        metric: if metric_vol { "volatility".into() } else { "return".into() },
        month: agg_buckets(&m_s, 12, metric_vol),
        weekday: agg_buckets(&w_s, 7, metric_vol),
        hour: if has_hour { agg_buckets(&h_s, 24, metric_vol) } else { Vec::new() },
        month_weekday,
        has_hour,
    }
}

#[cfg(test)]
mod deflated_tests {
    use super::*;

    #[test]
    fn norm_cdf_matches_known_points() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((norm_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((norm_cdf(-1.96) - 0.025).abs() < 1e-3);
        // Symmetry, at the precision the approximation promises.
        for x in [0.3_f64, 1.0, 2.5, 4.0] {
            assert!((norm_cdf(x) + norm_cdf(-x) - 1.0).abs() < 1e-6, "asymmetric at {x}");
        }
    }

    /// The point of the whole thing: more trials must raise the bar the winner has to clear.
    #[test]
    fn the_selection_bar_rises_with_the_trial_count() {
        let few: Vec<f64> = (0..4).map(|i| 0.5 + i as f64 * 0.1).collect();
        let many: Vec<f64> = (0..40).map(|i| 0.5 + (i % 4) as f64 * 0.1).collect();
        let a = deflated_sharpe(&few, 2000, 252.0).unwrap();
        let b = deflated_sharpe(&many, 2000, 252.0).unwrap();
        assert!(
            b.expected_max_sharpe > a.expected_max_sharpe,
            "40 trials must set a higher bar than 4: {} vs {}",
            b.expected_max_sharpe,
            a.expected_max_sharpe
        );
    }

    /// A grid of indistinguishable junk: the best of it is selection, and the verdict has to
    /// say so rather than reporting the maximum.
    #[test]
    fn a_grid_of_noise_is_explained_by_selection() {
        // Trials scattered around zero — the "best" is just the luckiest.
        let noise: Vec<f64> = (0..32).map(|i| ((i % 7) as f64 - 3.0) * 0.15).collect();
        let d = deflated_sharpe(&noise, 5000, 252.0).unwrap();
        assert!(d.selection_explains_it, "expected a selection verdict, got {d:?}");
        assert!(d.note.contains("no evidence"), "the note must say it plainly: {}", d.note);
    }

    /// One clear standout against tight, low trials should survive the correction.
    #[test]
    fn a_standout_survives_the_correction() {
        let mut trials: Vec<f64> = vec![0.1, 0.12, 0.09, 0.11, 0.1, 0.08];
        trials.push(2.5);
        let d = deflated_sharpe(&trials, 5000, 252.0).unwrap();
        assert!(!d.selection_explains_it, "a 2.5 against ~0.1 noise should clear the bar: {d:?}");
        assert!(d.deflated_sharpe > 0.5);
    }

    /// Degenerate inputs return None rather than a confident-looking zero.
    #[test]
    fn refuses_to_answer_without_data() {
        assert!(deflated_sharpe(&[], 1000, 252.0).is_none());
        assert!(deflated_sharpe(&[1.0], 2, 252.0).is_none());
        // A single trial has no selection to correct for: the bar stays at zero.
        let one = deflated_sharpe(&[1.5], 1000, 252.0).unwrap();
        assert_eq!(one.expected_max_sharpe, 0.0);
        assert_eq!(one.trials, 1);
    }

    #[test]
    fn spread_reports_the_distribution_not_the_max() {
        let s = spread(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(s.n, 5);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 5.0);
        assert_eq!(s.median, 3.0);
        assert!(spread(&[]).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_size_risk_math() {
        // $10k stack, risk $100, long 100 → 95 stop (5 wide) → 20 units, $2000 notional.
        let r = position_size(10_000.0, 100.0, 100.0, 95.0, Side::Long, 1.0, None, None);
        assert!((r.quantity - 20.0).abs() < 1e-9);
        assert!((r.notional - 2000.0).abs() < 1e-9);
        assert!((r.risk_fraction - 0.01).abs() < 1e-9);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn position_size_over_leverage_flag() {
        // Notional $20k on a $10k stack at 1× is over buying power; at 2.5× it is not.
        let a = position_size(10_000.0, 200.0, 100.0, 99.0, Side::Long, 1.0, Some(1.0), None);
        assert!(a.over_leveraged);
        let b = position_size(10_000.0, 200.0, 100.0, 99.0, Side::Long, 1.0, Some(2.5), None);
        assert!(!b.over_leveraged);
        assert!((b.margin_required.unwrap() - b.notional / 2.5).abs() < 1e-6);
    }

    #[test]
    fn asset_signals_scale_to_entry_not_asset_price() {
        // Asset trades near 50_000 (so ATR is in the hundreds); entry is a placeholder 100.
        // Every suggested stop must land just under entry (fraction-scaled), never negative.
        let closes: Vec<f64> = (0..100).map(|i| 50_000.0 + (i as f64) * 10.0).collect();
        let highs: Vec<f64> = closes.iter().map(|c| c + 200.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 200.0).collect();
        let ts: Vec<String> = vec![String::new(); closes.len()];
        let sig = asset_signals(&highs, &lows, &closes, &ts, "1h", 100.0, Side::Long, 14, 20);
        for s in &sig.suggestions {
            assert!(s.stop_price > 0.0, "{}: stop went non-positive", s.key);
            assert!(s.stop_price < 100.0, "{}: stop not below long entry", s.key);
            // distance_pct is a fraction of price and should be small/sane, not asset-scaled.
            assert!(s.distance_pct >= 0.0 && s.distance_pct < 1.0, "{}: bad pct", s.key);
        }
    }

    #[test]
    fn monte_carlo_all_wins_never_ruins() {
        // Every trade is +100 → equity only rises → no path can breach a 50% ruin level,
        // final equity == start + horizon*100, and max drawdown is 0.
        let pnls = vec![100.0; 20];
        let mc = monte_carlo(&pnls, 10_000.0, 2_000, 30, 1, 0.5, 12345);
        assert_eq!(mc.risk_of_ruin, 0.0);
        assert_eq!(mc.prob_loss, 0.0);
        assert!((mc.final_equity.p50 - (10_000.0 + 30.0 * 100.0)).abs() < 1e-6);
        assert!(mc.max_drawdown.p95 < 1e-9);
        assert_eq!(mc.fan.len(), 31);
        assert!((mc.fan[0].p50 - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn monte_carlo_reproducible_and_ruin_detected() {
        // Big losers on a thin stack → some paths must breach the ruin level; and a fixed seed
        // reproduces the exact risk-of-ruin.
        let pnls = vec![50.0, 50.0, -400.0, 50.0];
        let a = monte_carlo(&pnls, 1_000.0, 5_000, 20, 1, 0.5, 999);
        let b = monte_carlo(&pnls, 1_000.0, 5_000, 20, 1, 0.5, 999);
        assert_eq!(a.risk_of_ruin, b.risk_of_ruin);
        assert!(a.risk_of_ruin > 0.0, "expected some ruined paths");
        assert!(a.max_drawdown.p50 > 0.0);
    }

    #[test]
    fn seasonality_buckets_align_and_count() {
        // 4 bars → 3 returns attributed to bars 1..3. Give distinct months so buckets separate.
        let closes = vec![100.0, 110.0, 99.0, 108.9]; // +10%, -10%, +10%
        let months = vec![0u8, 0, 1, 1]; // bar1→Jan, bar2→Feb, bar3→Feb
        let weekdays = vec![0u8, 0, 1, 2];
        let hours = vec![0u8; 4];
        let s = seasonality(&closes, &months, &weekdays, &hours, false, false);
        assert_eq!(s.periods, 3);
        assert_eq!(s.month.len(), 12);
        assert_eq!(s.month[0].count, 1); // one return in January (bar 1)
        assert_eq!(s.month[1].count, 2); // two returns in February (bars 2,3)
        assert!(s.hour.is_empty()); // has_hour=false
        assert_eq!(s.month_weekday.len(), 12);
        assert_eq!(s.month_weekday[0].len(), 7);
    }
}

/// Audit round 3 (2026-08-20): the multiple-testing correction's arithmetic.
#[cfg(test)]
mod verify_dsr {
    use super::*;

    /// norm_cdf must match known standard-normal values.
    #[test]
    fn vq_norm_cdf_known_values() {
        let cases = [(0.0, 0.5), (1.0, 0.841_344_75), (-1.0, 0.158_655_25), (1.96, 0.975_002_1)];
        for (x, want) in cases {
            let got = norm_cdf(x);
            assert!((got - want).abs() < 2e-7, "norm_cdf({x}): want {want}, got {got}");
        }
    }

    /// norm_ppf must invert norm_cdf.
    #[test]
    fn vq_norm_ppf_inverts_cdf() {
        for p in [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99] {
            let x = norm_ppf(p);
            let back = norm_cdf(x);
            assert!((back - p).abs() < 1e-6, "ppf/cdf roundtrip at {p}: got {back}");
        }
    }

    /// The probabilistic Sharpe ratio must grow with the length of the track record: the same
    /// annualized Sharpe observed over 10 years is far stronger evidence than over 6 months.
    /// This is the entire purpose of PSR, so it must not be flat in `observations`.
    #[test]
    fn vq_psr_increases_with_track_record_length() {
        // One trial, so expected_max = 0 and deflated_sharpe == probabilistic_sharpe:
        // P(true Sharpe > 0) for an annualized Sharpe of 1.5.
        let short = deflated_sharpe(&[1.5], 126, 252.0).expect("6 months");
        let long = deflated_sharpe(&[1.5], 2520, 252.0).expect("10 years");
        assert!(
            long.probabilistic_sharpe > short.probabilistic_sharpe + 0.05,
            "PSR must rise with sample size: 126 bars ⇒ {:.4}, 2520 bars ⇒ {:.4}",
            short.probabilistic_sharpe,
            long.probabilistic_sharpe
        );
    }

    /// A Sharpe of 1.5 sustained over ten years of daily bars is overwhelming evidence of a
    /// positive true Sharpe: PSR should be very close to 1.
    #[test]
    fn vq_psr_long_record_is_near_certain() {
        let r = deflated_sharpe(&[1.5], 2520, 252.0).expect("10 years");
        assert!(
            r.probabilistic_sharpe > 0.99,
            "Sharpe 1.5 over 2520 daily bars ⇒ PSR should be ~0.999, got {:.4}",
            r.probabilistic_sharpe
        );
    }

    /// Expected-max selection bar must grow with the number of trials: the best of 10_000 tries
    /// is a higher bar than the best of 10.
    #[test]
    fn vq_expected_max_grows_with_trials() {
        let sharpes_10: Vec<f64> = (0..10).map(|i| i as f64 * 0.1).collect();
        let sharpes_10k: Vec<f64> = (0..10_000).map(|i| (i % 10) as f64 * 0.1).collect();
        let a = deflated_sharpe(&sharpes_10, 252, 252.0).expect("10");
        let b = deflated_sharpe(&sharpes_10k, 252, 252.0).expect("10k");
        assert!(
            b.expected_max_sharpe > a.expected_max_sharpe,
            "selection bar must rise with N: 10 ⇒ {:.3}, 10k ⇒ {:.3}",
            a.expected_max_sharpe,
            b.expected_max_sharpe
        );
    }

    /// A grid of pure noise must be flagged: the best trial should not clear the selection bar.
    #[test]
    fn vq_noise_grid_is_flagged() {
        // Symmetric spread centred on zero — no edge anywhere in the grid.
        let sharpes: Vec<f64> = (0..1000).map(|i| ((i as f64 / 999.0) - 0.5) * 2.0).collect();
        let r = deflated_sharpe(&sharpes, 252, 252.0).expect("noise grid");
        assert!(
            r.selection_explains_it,
            "a zero-centred noise grid must be flagged: best {:.3} vs bar {:.3}",
            r.best_sharpe, r.expected_max_sharpe
        );
    }

    /// A Sharpe at or above 1 per-observation is outside the model (`1 - sr^2` non-positive).
    /// It must refuse to answer, not clamp into a confident-looking probability.
    #[test]
    fn vq_extreme_sharpe_probability_is_bounded() {
        assert!(
            deflated_sharpe(&[500.0], 10, 252.0).is_none(),
            "an out-of-model Sharpe must yield None, not a clamped probability"
        );
        // An ordinary Sharpe still answers, and stays a probability.
        let r = deflated_sharpe(&[1.2], 500, 252.0).expect("ordinary");
        assert!((0.0..=1.0).contains(&r.probabilistic_sharpe), "psr {}", r.probabilistic_sharpe);
        assert!((0.0..=1.0).contains(&r.deflated_sharpe), "dsr {}", r.deflated_sharpe);
    }
}

#[cfg(test)]
mod verify_dsr_fixed {
    use super::*;

    /// After the BUG-8 fix, PSR must track sample size the way the statistic is defined:
    /// the same annualized Sharpe over a longer record is stronger evidence.
    #[test]
    fn vqf_psr_monotone_in_sample_size() {
        let mut prev = 0.0;
        for obs in [126usize, 252, 504, 1260, 2520] {
            let r = deflated_sharpe(&[1.5], obs, 252.0).expect("psr");
            assert!(
                r.probabilistic_sharpe > prev,
                "PSR must increase with observations: {obs} bars gave {:.4} after {prev:.4}",
                r.probabilistic_sharpe
            );
            prev = r.probabilistic_sharpe;
        }
    }

    /// Spot values against the closed form, computed independently in Python:
    /// `ann = sqrt(252); sr = 1.5/ann; PSR(0) = Phi(sr * sqrt(t-1) / sqrt(1 - sr^2))`.
    #[test]
    fn vqf_psr_matches_closed_form() {
        for (obs, want) in [(252usize, 0.933_677_f64), (2520, 0.999_999)] {
            let r = deflated_sharpe(&[1.5], obs, 252.0).unwrap();
            assert!(
                (r.probabilistic_sharpe - want).abs() < 5e-4,
                "{obs} bars: want {want}, got {}",
                r.probabilistic_sharpe
            );
        }
    }

    /// The same annualized Sharpe over the same NUMBER of bars is weaker evidence when those
    /// bars are hourly: 1000 hourly bars are ~3 months, 1000 daily bars are ~4 years, so the
    /// per-bar Sharpe the estimate rests on is smaller. Before the fix `periods_per_year` was
    /// not consulted at all and both answers were identical.
    #[test]
    fn vqf_timeframe_changes_the_verdict() {
        let daily = deflated_sharpe(&[1.5], 1000, 252.0).unwrap();
        let hourly = deflated_sharpe(&[1.5], 1000, 252.0 * 6.5).unwrap();
        assert!(
            hourly.probabilistic_sharpe < daily.probabilistic_sharpe,
            "1000 hourly bars is far less evidence than 1000 daily bars; \
             daily {:.4} vs hourly {:.4}",
            daily.probabilistic_sharpe,
            hourly.probabilistic_sharpe
        );
    }
}

#[cfg(test)]
mod portfolio_measure_tests {
    use super::*;

    fn ts(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2026-01-{:02}", i + 1)).collect()
    }

    #[test]
    fn compounding_a_flat_series_stays_flat() {
        assert_eq!(compound(&[0.0, 0.0, 0.0]), vec![1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn cagr_refuses_to_annualize_three_weeks() {
        assert_eq!(cagr(0.05, 0.06), None);
        let one = cagr(0.21, 3.0).unwrap();
        assert!((one - 0.065_6).abs() < 1e-3, "{one}");
    }

    /// A series that only ever rises has no downside, so Sortino has no denominator and
    /// must say so rather than report infinity.
    #[test]
    fn sortino_has_no_answer_without_a_downside() {
        let rets = vec![0.01; 30];
        let d = downside_deviation(&rets, 0.0, 252.0);
        assert_eq!(d, 0.0);
        assert_eq!(sortino(0.12, d, 0.0), None);
    }

    /// The shortfall divides by every period, not by the losing ones: two books with the
    /// same single loss must not rank differently because one traded more often.
    #[test]
    fn downside_counts_every_period() {
        // The same single loss, spread over more quiet periods, is less downside.
        let rare = downside_deviation(&[0.01, 0.01, 0.01, -0.10], 0.0, 1.0);
        let dense = downside_deviation(&[0.01, -0.10], 0.0, 1.0);
        assert!(rare < dense, "{rare} !< {dense}");
    }

    #[test]
    fn underwater_finds_the_episode_and_its_recovery() {
        // 1.0 → 0.8 (trough at index 2) → back to 1.0 at index 4, then a new high.
        let eq = vec![1.0, 0.9, 0.8, 0.9, 1.0, 1.1];
        let eps = underwater(&eq, &ts(eq.len()), 0.01);
        assert_eq!(eps.len(), 1);
        let e = &eps[0];
        assert!((e.depth - 0.2).abs() < 1e-9);
        assert_eq!(e.to_trough, 2);
        assert_eq!(e.to_recover, Some(2));
        assert_eq!(e.recovered_at.as_deref(), Some("2026-01-05"));
    }

    /// A drawdown still open at the end is reported open, with no recovery date, rather
    /// than dropped because it has not finished.
    #[test]
    fn an_open_drawdown_is_still_reported() {
        let eq = vec![1.0, 1.2, 1.0, 0.9];
        let eps = underwater(&eq, &ts(eq.len()), 0.01);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].recovered_at, None);
        assert_eq!(eps[0].to_recover, None);
        assert!((eps[0].depth - 0.25).abs() < 1e-9);
    }

    #[test]
    fn underwater_sorts_worst_first_and_honours_the_floor() {
        let eq = vec![1.0, 0.99, 1.0, 0.5, 1.0];
        let all = underwater(&eq, &ts(eq.len()), 0.0);
        assert_eq!(all.len(), 2);
        assert!(all[0].depth > all[1].depth);
        assert_eq!(underwater(&eq, &ts(eq.len()), 0.1).len(), 1);
    }

    #[test]
    fn irr_of_a_doubling_over_a_year_is_a_hundred_percent() {
        let r = irr(&[(0.0, -100.0), (1.0, 200.0)]).unwrap();
        assert!((r - 1.0).abs() < 1e-6, "{r}");
    }

    /// Flows that never change sign have no rate of return, and inventing one would be
    /// worse than reporting none.
    #[test]
    fn irr_declines_when_there_is_no_root() {
        assert_eq!(irr(&[(0.0, 100.0), (1.0, 200.0)]), None);
        assert_eq!(irr(&[(0.0, -100.0)]), None);
    }

    #[test]
    fn ols_recovers_a_planted_beta() {
        let x: Vec<f64> = (0..40).map(|i| (i as f64 * 0.7).sin() * 0.02).collect();
        let y: Vec<f64> = x.iter().map(|v| 0.001 + 1.5 * v).collect();
        let (alpha, beta, r2) = ols(&y, &x).unwrap();
        assert!((beta - 1.5).abs() < 1e-9, "{beta}");
        assert!((alpha - 0.001).abs() < 1e-9);
        assert!((r2 - 1.0).abs() < 1e-9);
    }

    /// A factor that does not move gives no beta. Returning 1.0 there is exactly the
    /// fabricated number the stress engine must never report.
    #[test]
    fn ols_refuses_a_constant_factor() {
        assert_eq!(ols(&[0.01, 0.02, 0.03], &[0.0, 0.0, 0.0]), None);
        assert_eq!(ols(&[0.01, 0.02], &[0.01, 0.02]), None);
    }
}
