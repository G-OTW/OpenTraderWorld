//! Single-series statistics: the shape of the return distribution, serial dependence,
//! mean reversion versus trend, stationarity, tail risk and the significance of a Sharpe ratio.
//!
//! Each test follows one published reference implementation to the letter, so a figure here
//! can be checked against it on the same bars:
//! - moments and Jarque-Bera: `scipy.stats`
//! - ACF, PACF (Levinson-Durbin), Ljung-Box, ADF (AIC lag search), KPSS (Hobijn auto lag),
//!   MacKinnon p-values and critical values: `statsmodels`
//! - Hurst R/S (Anis-Lloyd-Peters corrected) and DFA: `nolds`
//! - Lo-MacKinlay variance ratio (overlapping, debiased, heteroskedasticity robust): `arch`
//! - Generalized Pareto tail fit: `scipy.stats.genpareto` maximum likelihood

use serde::Serialize;

use super::linalg::{self, Mat};
use super::special::{chi2_sf, norm_cdf, norm_ppf, t_two_sided};
use super::{mean, quantile_sorted, stddev};

/// Log returns ln(p_t / p_{t-1}); non-positive prices are skipped rather than turned into NaN.
pub fn log_returns(closes: &[f64]) -> Vec<f64> {
    closes
        .windows(2)
        .filter(|w| w[0] > 0.0 && w[1] > 0.0)
        .map(|w| (w[1] / w[0]).ln())
        .collect()
}

fn sorted(xs: &[f64]) -> Vec<f64> {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    s
}

// ── Moments and normality ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Moments {
    pub n: usize,
    pub mean: f64,
    pub stdev: f64,
    /// Sample skewness, bias-corrected (G1, what pandas and Excel report).
    pub skew: f64,
    /// Sample excess kurtosis, bias-corrected (G2).
    pub excess_kurtosis: f64,
    /// Population skewness g1 (scipy's default), the one Jarque-Bera is built on.
    pub skew_biased: f64,
    /// Population excess kurtosis g2.
    pub excess_kurtosis_biased: f64,
    pub min: f64,
    pub max: f64,
    /// Jarque-Bera statistic and its chi-square(2) p-value.
    pub jarque_bera: f64,
    pub jb_pvalue: f64,
}

pub fn moments(xs: &[f64]) -> Option<Moments> {
    let n = xs.len();
    if n < 4 {
        return None;
    }
    let nf = n as f64;
    let m = mean(xs);
    let (mut m2, mut m3, mut m4) = (0.0, 0.0, 0.0);
    for &x in xs {
        let d = x - m;
        let d2 = d * d;
        m2 += d2;
        m3 += d2 * d;
        m4 += d2 * d2;
    }
    m2 /= nf;
    m3 /= nf;
    m4 /= nf;
    if m2 <= 0.0 {
        return None;
    }
    let g1 = m3 / m2.powf(1.5);
    let g2 = m4 / (m2 * m2) - 3.0;
    let big_g1 = g1 * (nf * (nf - 1.0)).sqrt() / (nf - 2.0);
    let big_g2 = ((nf + 1.0) * g2 + 6.0) * (nf - 1.0) / ((nf - 2.0) * (nf - 3.0));
    let jb = nf / 6.0 * (g1 * g1 + g2 * g2 / 4.0);
    Some(Moments {
        n,
        mean: m,
        stdev: stddev(xs),
        skew: big_g1,
        excess_kurtosis: big_g2,
        skew_biased: g1,
        excess_kurtosis_biased: g2,
        min: xs.iter().cloned().fold(f64::INFINITY, f64::min),
        max: xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        jarque_bera: jb,
        jb_pvalue: chi2_sf(jb, 2.0),
    })
}

/// Normal QQ points: (theoretical quantile, standardized sample quantile), thinned to at most
/// `max_points` evenly spaced ranks (both tails always kept).
pub fn qq_points(xs: &[f64], max_points: usize) -> Vec<[f64; 2]> {
    let n = xs.len();
    if n < 3 {
        return Vec::new();
    }
    let (m, s) = (mean(xs), stddev(xs));
    if s <= 0.0 {
        return Vec::new();
    }
    let srt = sorted(xs);
    let step = (n as f64 / max_points.max(2) as f64).max(1.0);
    let mut idx: Vec<usize> = (0..).map(|i| (i as f64 * step) as usize).take_while(|&i| i < n).collect();
    if idx.last() != Some(&(n - 1)) {
        idx.push(n - 1);
    }
    idx.into_iter()
        .map(|i| {
            // Blom plotting position, the convention scipy.stats.probplot uses (Filliben's).
            let p = (i as f64 + 1.0 - 0.375) / (n as f64 + 0.25);
            [norm_ppf(p), (srt[i] - m) / s]
        })
        .collect()
}

// ── Serial dependence ───────────────────────────────────────────────────────────

/// Sample autocorrelations for lags 0..=nlags (statsmodels `acf`, adjusted = false).
pub fn acf(xs: &[f64], nlags: usize) -> Vec<f64> {
    let n = xs.len();
    let m = mean(xs);
    let d: Vec<f64> = xs.iter().map(|x| x - m).collect();
    let c0: f64 = d.iter().map(|v| v * v).sum();
    (0..=nlags.min(n.saturating_sub(1)))
        .map(|k| if c0 > 0.0 { (0..n - k).map(|t| d[t] * d[t + k]).sum::<f64>() / c0 } else { 0.0 })
        .collect()
}

/// Partial autocorrelations for lags 0..=nlags by Levinson-Durbin on the biased ACF
/// (statsmodels `pacf(method="ldb")`).
pub fn pacf(xs: &[f64], nlags: usize) -> Vec<f64> {
    let r = acf(xs, nlags);
    let p = r.len().saturating_sub(1);
    let mut out = vec![1.0];
    let mut phi: Vec<f64> = Vec::new();
    let mut v = 1.0;
    for k in 1..=p {
        let num = r[k] - (1..k).map(|j| phi[j - 1] * r[k - j]).sum::<f64>();
        let a = if v > 0.0 { num / v } else { 0.0 };
        let mut next: Vec<f64> = (1..k).map(|j| phi[j - 1] - a * phi[k - j - 1]).collect();
        next.push(a);
        phi = next;
        v *= 1.0 - a * a;
        out.push(a);
    }
    out
}

#[derive(Debug, Serialize)]
pub struct LjungBox {
    pub lag: usize,
    pub q: f64,
    pub pvalue: f64,
}

/// Ljung-Box Q at each lag in `lags`, from the ACF (statsmodels `acorr_ljungbox`).
pub fn ljung_box(xs: &[f64], lags: &[usize]) -> Vec<LjungBox> {
    let n = xs.len() as f64;
    let maxl = lags.iter().copied().max().unwrap_or(0);
    let r = acf(xs, maxl);
    lags.iter()
        .filter(|&&l| l >= 1 && l < r.len())
        .map(|&l| {
            let q = n * (n + 2.0) * (1..=l).map(|k| r[k] * r[k] / (n - k as f64)).sum::<f64>();
            LjungBox { lag: l, q, pvalue: chi2_sf(q, l as f64) }
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct Correlogram {
    pub acf: Vec<f64>,
    pub pacf: Vec<f64>,
    /// ±band under the white-noise null (1.96 / √n).
    pub band: f64,
    pub ljung_box: Vec<LjungBox>,
}

pub fn correlogram(xs: &[f64], nlags: usize) -> Correlogram {
    let nlags = nlags.min(xs.len() / 2);
    let lb_lags: Vec<usize> = [5, 10, 20, 40].into_iter().filter(|&l| l <= nlags).collect();
    Correlogram {
        acf: acf(xs, nlags),
        pacf: pacf(xs, nlags),
        band: 1.96 / (xs.len() as f64).sqrt(),
        ljung_box: ljung_box(xs, &lb_lags),
    }
}

// ── Hurst exponent ──────────────────────────────────────────────────────────────

/// Window sizes nolds uses by default for R/S: 15 log-spaced sizes over the middle quarter of
/// the log range, deduplicated.
fn logmid_n(max_n: usize) -> Vec<usize> {
    let l = (max_n as f64).ln();
    let span = l * 0.25;
    let start = l * 0.75 * 0.5;
    let mut v: Vec<usize> =
        (0..15).map(|i| (start + i as f64 / 15.0 * span).exp().round() as usize).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Window sizes nolds uses by default for DFA: from 4 to 10% of the sample, growing by 1.2.
fn logarithmic_n(min_n: f64, max_n: f64, factor: f64) -> Vec<usize> {
    let max_i = ((max_n / min_n).ln() / factor.ln()).floor() as i64;
    let mut ns = vec![min_n as usize];
    for i in 0..=max_i.max(0) {
        let n = (min_n * factor.powi(i as i32)).floor() as usize;
        if n > *ns.last().unwrap_or(&0) {
            ns.push(n);
        }
    }
    ns
}

/// Mean rescaled range over non-overlapping windows of size `n`.
fn rescaled_range(xs: &[f64], n: usize) -> Option<f64> {
    let m = xs.len() / n;
    let mut acc = 0.0;
    let mut cnt = 0usize;
    for w in 0..m {
        let seq = &xs[w * n..(w + 1) * n];
        let mu = mean(seq);
        let mut y = 0.0;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for v in seq {
            y += v - mu;
            lo = lo.min(y);
            hi = hi.max(y);
        }
        let r = hi - lo;
        let s = stddev(seq);
        if r != 0.0 && s > 0.0 {
            acc += r / s;
            cnt += 1;
        }
    }
    (cnt > 0).then(|| acc / cnt as f64)
}

/// Anis-Lloyd-Peters expected R/S of white noise.
fn expected_rs(n: usize) -> f64 {
    let nf = n as f64;
    let front = (nf - 0.5) / nf;
    let back: f64 = (1..n).map(|i| ((nf - i as f64) / i as f64).sqrt()).sum();
    let middle = if n <= 340 {
        (super::special::ln_gamma((nf - 1.0) * 0.5) - super::special::ln_gamma(nf * 0.5)).exp()
            / std::f64::consts::PI.sqrt()
    } else {
        1.0 / (nf * std::f64::consts::PI * 0.5).sqrt()
    };
    front * middle * back
}

/// Least-squares line through (x, y): (slope, intercept).
fn polyfit1(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    super::ols(y, x).map(|(alpha, beta, _)| (beta, alpha))
}

#[derive(Debug, Serialize)]
pub struct Hurst {
    /// R/S Hurst exponent with the Anis-Lloyd-Peters small-sample correction.
    pub rs: Option<f64>,
    /// DFA-1 scaling exponent (≈ H for a stationary increment series).
    pub dfa: Option<f64>,
    /// log-log points behind each fit, for the chart.
    pub rs_points: Vec<[f64; 2]>,
    pub dfa_points: Vec<[f64; 2]>,
}

/// Hurst exponent of an increment series (returns), by corrected R/S and by DFA.
/// 0.5 is a random walk; below it mean-reverting, above it persistent.
pub fn hurst(xs: &[f64]) -> Hurst {
    let n = xs.len();
    let mut out = Hurst { rs: None, dfa: None, rs_points: Vec::new(), dfa_points: Vec::new() };
    if n < 100 {
        return out;
    }
    // R/S, corrected: slope of log(R/S) - log(E[R/S]) on log n, plus 0.5.
    let (mut lx, mut ly) = (Vec::new(), Vec::new());
    for w in logmid_n(n) {
        if w < 2 {
            continue;
        }
        if let Some(rs) = rescaled_range(xs, w) {
            lx.push((w as f64).ln());
            ly.push(rs.ln() - expected_rs(w).ln());
            out.rs_points.push([(w as f64).ln(), rs.ln()]);
        }
    }
    if lx.len() >= 2 {
        out.rs = polyfit1(&lx, &ly).map(|(slope, _)| slope + 0.5);
    }
    // DFA-1 with half-overlapping windows over the integrated profile.
    let mu = mean(xs);
    let mut walk = Vec::with_capacity(n);
    let mut acc = 0.0;
    for v in xs {
        acc += v - mu;
        walk.push(acc);
    }
    let (mut dx, mut dy) = (Vec::new(), Vec::new());
    for w in logarithmic_n(4.0, 0.1 * n as f64, 1.2) {
        if w < 2 || w >= n {
            continue;
        }
        let t: Vec<f64> = (0..w).map(|i| i as f64).collect();
        let mut sum = 0.0;
        let mut cnt = 0usize;
        let mut start = 0;
        while start < walk.len() - w {
            let seg = &walk[start..start + w];
            if let Some((a, b, _)) = super::ols(seg, &t) {
                let f: f64 = seg.iter().zip(&t).map(|(v, x)| (v - (a + b * x)).powi(2)).sum::<f64>() / w as f64;
                sum += f;
                cnt += 1;
            }
            start += (w / 2).max(1);
        }
        if cnt > 0 {
            let fnv = (sum / cnt as f64).sqrt();
            if fnv > 0.0 {
                dx.push((w as f64).ln());
                dy.push(fnv.ln());
            }
        }
    }
    if dx.len() >= 2 {
        out.dfa = polyfit1(&dx, &dy).map(|(slope, _)| slope);
        out.dfa_points = dx.iter().zip(&dy).map(|(a, b)| [*a, *b]).collect();
    }
    out
}

// ── Variance ratio ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct VarianceRatio {
    pub lags: usize,
    pub vr: f64,
    /// Heteroskedasticity-robust z statistic.
    pub stat: f64,
    pub pvalue: f64,
}

/// Lo-MacKinlay variance ratio of a LOG PRICE series at horizon `q` (overlapping,
/// debiased, robust), exactly as `arch.unitroot.VarianceRatio` computes it.
pub fn variance_ratio(log_prices: &[f64], q: usize) -> Option<VarianceRatio> {
    let nobs = log_prices.len();
    if q < 2 || nobs < 2 * q + 2 {
        return None;
    }
    let y = log_prices;
    let mu = (y[nobs - 1] - y[0]) / (nobs - 1) as f64;
    let dy: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).collect();
    let nq = dy.len() as f64;
    let mut s1 = dy.iter().map(|d| (d - mu).powi(2)).sum::<f64>() / nq;
    let qf = q as f64;
    let mut sq = (q..nobs).map(|i| (y[i] - y[i - q] - qf * mu).powi(2)).sum::<f64>() / (nq * qf);
    s1 *= nq / (nq - 1.0);
    let m = qf * (nq - qf + 1.0) * (1.0 - qf / nq);
    sq *= nq * qf / m;
    let z2: Vec<f64> = dy.iter().map(|d| (d - mu).powi(2)).collect();
    let scale = z2.iter().sum::<f64>().powi(2);
    let mut theta = 0.0;
    for k in 1..q {
        let delta = nq * (k..z2.len()).map(|i| z2[i] * z2[i - k]).sum::<f64>() / scale;
        theta += 4.0 * (1.0 - k as f64 / qf).powi(2) * delta;
    }
    if s1 <= 0.0 || theta <= 0.0 {
        return None;
    }
    let vr = sq / s1;
    let stat = nq.sqrt() * (vr - 1.0) / theta.sqrt();
    Some(VarianceRatio { lags: q, vr, stat, pvalue: 2.0 - 2.0 * norm_cdf(stat.abs()) })
}

// ── Unit-root tests ─────────────────────────────────────────────────────────────

/// MacKinnon (1994) p-value tables, regression with a constant, N = 1..6 series.
const TAU_STAR_C: [f64; 6] = [-1.61, -2.62, -3.13, -3.47, -3.78, -3.93];
const TAU_MIN_C: [f64; 6] = [-18.83, -18.86, -23.48, -28.07, -25.96, -23.27];
const TAU_MAX_C: [f64; 6] = [2.74, 0.92, 0.55, 0.61, 0.79, 1.0];
const TAU_C_SMALLP: [[f64; 3]; 6] = [
    [2.1659, 1.4412, 3.8269e-2],
    [2.92, 1.5012, 3.9796e-2],
    [3.4699, 1.4856, 3.164e-2],
    [3.9673, 1.4777, 2.6315e-2],
    [4.5509, 1.5338, 2.9545e-2],
    [5.1399, 1.6036, 3.4445e-2],
];
const TAU_C_LARGEP: [[f64; 4]; 6] = [
    [1.7339, 9.3202e-1, -1.2745e-1, -1.0368e-2],
    [2.1945, 6.4695e-1, -2.9198e-1, -4.2377e-2],
    [2.5893, 4.5168e-1, -3.6529e-1, -5.0074e-2],
    [3.0387, 4.5452e-1, -3.3666e-1, -4.1921e-2],
    [3.5049, 5.2098e-1, -2.9158e-1, -3.3468e-2],
    [3.9489, 5.8933e-1, -2.5359e-1, -2.721e-2],
];
/// MacKinnon (2010) critical-value response surfaces, constant, N = 1..6: rows 1%, 5%, 10%;
/// value = b0 + b1/T + b2/T² + b3/T³.
const TAU_C_2010: [[[f64; 4]; 3]; 6] = [
    [[-3.43035, -6.5393, -16.786, -79.433], [-2.86154, -2.8903, -4.234, -40.040], [-2.56677, -1.5384, -2.809, 0.0]],
    [[-3.89644, -10.9519, -33.527, 0.0], [-3.33613, -6.1101, -6.823, 0.0], [-3.04445, -4.2412, -2.720, 0.0]],
    [[-4.29374, -14.4354, -33.195, 47.433], [-3.74066, -8.5632, -10.852, 27.982], [-3.45218, -6.2143, -3.718, 0.0]],
    [[-4.64332, -18.1031, -37.972, 0.0], [-4.09600, -11.2349, -11.175, 0.0], [-3.81020, -8.3931, -4.137, 0.0]],
    [[-4.95756, -21.8883, -45.142, 0.0], [-4.41519, -14.0405, -12.575, 0.0], [-4.13157, -10.7417, -3.784, 0.0]],
    [[-5.24568, -25.6688, -57.737, 88.639], [-4.70693, -16.9178, -17.492, 60.007], [-4.42501, -13.1875, -5.104, 27.877]],
];

/// Approximate p-value of a Dickey-Fuller / Engle-Granger tau statistic (constant case) for
/// `n_series` integrated series (1 for ADF).
pub fn mackinnon_p(stat: f64, n_series: usize) -> f64 {
    let i = n_series.clamp(1, 6) - 1;
    if stat > TAU_MAX_C[i] {
        return 1.0;
    }
    if stat < TAU_MIN_C[i] {
        return 0.0;
    }
    let z = if stat <= TAU_STAR_C[i] {
        let c = TAU_C_SMALLP[i];
        c[0] + c[1] * stat + c[2] * stat * stat
    } else {
        let c = TAU_C_LARGEP[i];
        c[0] + c[1] * stat + c[2] * stat * stat + c[3] * stat.powi(3)
    };
    norm_cdf(z)
}

/// Finite-sample critical values (1%, 5%, 10%), constant case.
pub fn mackinnon_crit(n_series: usize, nobs: usize) -> [f64; 3] {
    let t = &TAU_C_2010[n_series.clamp(1, 6) - 1];
    let x = 1.0 / nobs as f64;
    let f = |b: &[f64; 4]| b[0] + b[1] * x + b[2] * x * x + b[3] * x.powi(3);
    [f(&t[0]), f(&t[1]), f(&t[2])]
}

#[derive(Debug, Serialize)]
pub struct Adf {
    pub stat: f64,
    pub pvalue: f64,
    pub used_lag: usize,
    pub nobs: usize,
    /// Critical values at 1%, 5%, 10%.
    pub crit: [f64; 3],
}

/// Build the ADF design for `lags` lagged differences: rows t = lags..len(dx)-1, columns
/// [level x_t, Δx_{t-1}, …, Δx_{t-lags}] and the regressand Δx_t, all trimmed to the last
/// `nobs` rows so several lag orders can share one sample.
fn adf_design(x: &[f64], lags: usize, nobs: usize, constant: bool) -> (Vec<f64>, Mat) {
    let dx: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let start = dx.len() - nobs;
    let mut rows = Vec::with_capacity(nobs);
    let mut yv = Vec::with_capacity(nobs);
    for t in start..dx.len() {
        let mut r = Vec::with_capacity(lags + 2);
        if constant {
            r.push(1.0);
        }
        r.push(x[t]);
        for j in 1..=lags {
            r.push(dx[t - j]);
        }
        rows.push(r);
        yv.push(dx[t]);
    }
    (yv, rows)
}

/// Augmented Dickey-Fuller with an AIC search over 0..=maxlag lagged differences on a common
/// sample, then a refit at the chosen lag (statsmodels `adfuller(autolag="AIC")`).
/// `constant` = regression "c"; without it the regression is "n" (Engle-Granger residuals).
/// The p-value is reported for `n_series` (1 for a plain ADF).
pub fn adf(x: &[f64], constant: bool, n_series: usize) -> Option<Adf> {
    let n = x.len();
    let ntrend = usize::from(constant);
    if n < 10 {
        return None;
    }
    let maxlag = ((12.0 * (n as f64 / 100.0).powf(0.25)).ceil() as usize).min((n / 2).saturating_sub(ntrend + 1));
    let nobs_common = n - 1 - maxlag;
    // Every candidate lag order is a leading block of the widest design on the same sample,
    // so the cross-products are accumulated once and each fit only inverts its block: the
    // search costs one pass over the data instead of maxlag + 1 regressions.
    let (yv, xm) = adf_design(x, maxlag, nobs_common, constant);
    let k = xm[0].len();
    let mut xtx = vec![vec![0.0; k]; k];
    let mut xty = vec![0.0; k];
    let mut yty = 0.0;
    for (row, &y) in xm.iter().zip(&yv) {
        yty += y * y;
        for a in 0..k {
            xty[a] += row[a] * y;
            for b in a..k {
                xtx[a][b] += row[a] * row[b];
            }
        }
    }
    let nf = nobs_common as f64;
    let mut best: Option<(f64, usize)> = None;
    for lag in 0..=maxlag {
        let kk = ntrend + 1 + lag;
        let block: Mat = (0..kk).map(|a| (0..kk).map(|b| if a <= b { xtx[a][b] } else { xtx[b][a] }).collect()).collect();
        let Some(inv) = linalg::inverse(&block) else { continue };
        let coef = linalg::matvec(&inv, &xty[..kk]);
        let ssr = (yty - coef.iter().zip(&xty[..kk]).map(|(c, v)| c * v).sum::<f64>()).max(1e-300);
        let llf = -nf / 2.0 * ((2.0 * std::f64::consts::PI).ln() + (ssr / nf).ln() + 1.0);
        let aic = -2.0 * llf + 2.0 * kk as f64;
        if best.is_none_or(|(b, _)| aic < b) {
            best = Some((aic, lag));
        }
    }
    let (_, lag) = best?;
    let nobs = n - 1 - lag;
    let (yv, xm) = adf_design(x, lag, nobs, constant);
    let fit = linalg::ols(&yv, &xm)?;
    let stat = fit.t[ntrend];
    Some(Adf { stat, pvalue: mackinnon_p(stat, n_series), used_lag: lag, nobs, crit: mackinnon_crit(n_series, nobs) })
}

#[derive(Debug, Serialize)]
pub struct Kpss {
    pub stat: f64,
    /// Interpolated from the KPSS table, so bounded to [0.01, 0.10].
    pub pvalue: f64,
    pub lags: usize,
    /// Critical values at 10%, 5%, 2.5%, 1%.
    pub crit: [f64; 4],
}

/// KPSS level-stationarity test with the Hobijn et al. (1998) automatic bandwidth
/// (statsmodels `kpss(regression="c", nlags="auto")`).
pub fn kpss(x: &[f64]) -> Option<Kpss> {
    let n = x.len();
    if n < 10 {
        return None;
    }
    let nf = n as f64;
    let m = mean(x);
    let e: Vec<f64> = x.iter().map(|v| v - m).collect();
    let dot = |k: usize| (k..n).map(|i| e[i] * e[i - k]).sum::<f64>();
    // Automatic lag.
    let covlags = nf.powf(2.0 / 9.0) as usize;
    let mut s0 = dot(0) / nf;
    let mut s1 = 0.0;
    for i in 1..=covlags {
        let p = dot(i) / (nf / 2.0);
        s0 += p;
        s1 += i as f64 * p;
    }
    let s_hat = s1 / s0;
    let gamma = 1.1447 * (s_hat * s_hat).powf(1.0 / 3.0);
    let lags = ((gamma * nf.powf(1.0 / 3.0)) as usize).min(n - 1);
    // Long-run variance with Bartlett weights.
    let mut s2 = dot(0);
    for i in 1..=lags {
        s2 += 2.0 * dot(i) * (1.0 - i as f64 / (lags as f64 + 1.0));
    }
    s2 /= nf;
    let mut cum = 0.0;
    let mut eta = 0.0;
    for v in &e {
        cum += v;
        eta += cum * cum;
    }
    eta /= nf * nf;
    let stat = eta / s2;
    let crit = [0.347, 0.463, 0.574, 0.739];
    let pv = [0.10, 0.05, 0.025, 0.01];
    let pvalue = if stat <= crit[0] {
        pv[0]
    } else if stat >= crit[3] {
        pv[3]
    } else {
        let i = (0..3).find(|&i| stat <= crit[i + 1]).unwrap_or(2);
        pv[i] + (stat - crit[i]) / (crit[i + 1] - crit[i]) * (pv[i + 1] - pv[i])
    };
    Some(Kpss { stat, pvalue, lags, crit })
}

#[derive(Debug, Serialize)]
pub struct HalfLife {
    /// AR(1) coefficient of the level on its lag: Δy_t = a + b y_{t-1}; φ = 1 + b.
    pub phi: f64,
    pub b: f64,
    pub b_t: f64,
    /// Periods for a deviation to halve, `None` when the series does not revert (φ ≥ 1).
    pub half_life: Option<f64>,
}

/// Mean-reversion half-life of a level series from an AR(1) fit (−ln 2 / ln φ).
pub fn half_life(y: &[f64]) -> Option<HalfLife> {
    if y.len() < 10 {
        return None;
    }
    let rows: Mat = y[..y.len() - 1].iter().map(|v| vec![1.0, *v]).collect();
    let dy: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).collect();
    let fit = linalg::ols(&dy, &rows)?;
    let b = fit.coef[1];
    let phi = 1.0 + b;
    let half_life = (phi > 0.0 && phi < 1.0).then(|| -std::f64::consts::LN_2 / phi.ln());
    Some(HalfLife { phi, b, b_t: fit.t[1], half_life })
}

// ── Tail risk ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct EvtLevel {
    pub confidence: f64,
    pub var: f64,
    pub es: f64,
}

#[derive(Debug, Serialize)]
pub struct Gpd {
    /// Loss threshold u (positive fraction) above which the tail is modeled.
    pub threshold: f64,
    pub exceedances: usize,
    /// Shape ξ (> 0 is a fat, power-law tail) and scale β of the fitted Generalized Pareto.
    pub shape: f64,
    pub scale: f64,
    pub levels: Vec<EvtLevel>,
}

#[derive(Debug, Serialize)]
pub struct TailRisk {
    pub confidence: f64,
    pub var_historical: f64,
    pub var_normal: f64,
    /// Cornish-Fisher VaR: the normal quantile corrected for skew and kurtosis.
    pub var_cornish_fisher: f64,
    pub cvar_historical: f64,
    /// 95th percentile return over the absolute 5th percentile (> 1: fatter right tail).
    pub tail_ratio: Option<f64>,
    pub gpd: Option<Gpd>,
}

/// Maximum-likelihood Generalized Pareto fit to positive excesses (location fixed at 0).
pub fn fit_gpd(excess: &[f64]) -> Option<(f64, f64)> {
    let n = excess.len();
    if n < 10 {
        return None;
    }
    let m = mean(excess);
    let nll = |p: &[f64]| -> f64 {
        let (xi, beta) = (p[0], p[1].exp());
        let mut s = n as f64 * beta.ln();
        if xi.abs() < 1e-9 {
            s += excess.iter().sum::<f64>() / beta;
        } else {
            for &y in excess {
                let z = 1.0 + xi * y / beta;
                if z <= 0.0 {
                    return f64::INFINITY;
                }
                s += (1.0 + 1.0 / xi) * z.ln();
            }
        }
        s
    };
    let (p, v) = linalg::nelder_mead(nll, &[0.1, m.ln()], &[0.1, 0.3], 4000, 1e-14);
    // Polish from the first optimum: Nelder-Mead restarts shake off a collapsed simplex.
    let (p, v2) = linalg::nelder_mead(nll, &p, &[0.02, 0.05], 4000, 1e-15);
    (v.is_finite() && v2.is_finite()).then(|| (p[0], p[1].exp()))
}

/// Tail-risk panel over simple returns: historical, normal and Cornish-Fisher VaR, CVaR, the
/// tail ratio, and a peaks-over-threshold GPD fit to the worst `tail_frac` of losses.
pub fn tail_risk(rets: &[f64], confidence: f64, tail_frac: f64) -> Option<TailRisk> {
    let mom = moments(rets)?;
    let srt = sorted(rets);
    let n = srt.len();
    let z = norm_ppf(1.0 - confidence);
    let (s, k) = (mom.skew_biased, mom.excess_kurtosis_biased);
    let zcf = z + (z * z - 1.0) * s / 6.0 + (z.powi(3) - 3.0 * z) * k / 24.0
        - (2.0 * z.powi(3) - 5.0 * z) * s * s / 36.0;
    let p95 = quantile_sorted(&srt, 0.95);
    let p05 = quantile_sorted(&srt, 0.05);
    // Losses as positive numbers, worst first.
    let mut losses: Vec<f64> = rets.iter().map(|r| -r).collect();
    losses.sort_by(f64::total_cmp);
    let u = quantile_sorted(&losses, 1.0 - tail_frac.clamp(0.01, 0.5));
    let excess: Vec<f64> = losses.iter().filter(|l| **l > u).map(|l| l - u).collect();
    let gpd = fit_gpd(&excess).map(|(xi, beta)| {
        let nu = excess.len() as f64;
        let levels = [0.99, 0.995, 0.999]
            .into_iter()
            .map(|p| {
                let var = if xi.abs() < 1e-9 {
                    u - beta * ((n as f64 / nu) * (1.0 - p)).ln()
                } else {
                    u + beta / xi * (((n as f64 / nu) * (1.0 - p)).powf(-xi) - 1.0)
                };
                let es = if xi < 1.0 { var / (1.0 - xi) + (beta - xi * u) / (1.0 - xi) } else { f64::NAN };
                EvtLevel { confidence: p, var, es }
            })
            .collect();
        Gpd { threshold: u, exceedances: excess.len(), shape: xi, scale: beta, levels }
    });
    Some(TailRisk {
        confidence,
        var_historical: super::var_historical(rets, confidence),
        var_normal: (-(mom.mean + z * mom.stdev)).max(0.0),
        var_cornish_fisher: (-(mom.mean + zcf * mom.stdev)).max(0.0),
        cvar_historical: super::cvar(rets, confidence),
        tail_ratio: (p05 != 0.0).then(|| p95.abs() / p05.abs()),
        gpd,
    })
}

// ── Sharpe significance ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SharpeInference {
    pub observations: usize,
    pub periods_per_year: f64,
    /// Per-period and annualized (√ppy) Sharpe of the excess returns.
    pub sharpe_period: f64,
    pub sharpe_annual: f64,
    pub skew: f64,
    /// Raw (non-excess) kurtosis, the γ4 of the Bailey-López de Prado formulas.
    pub kurtosis: f64,
    pub t_stat: f64,
    pub p_value: f64,
    /// Standard error of the annualized Sharpe under iid normal returns (Lo 2002)...
    pub se_iid: f64,
    /// ...and with the skew/kurtosis correction (Mertens 2002).
    pub se_nonnormal: f64,
    /// 95% confidence interval of the annualized Sharpe (non-normal SE).
    pub ci_low: f64,
    pub ci_high: f64,
    /// Benchmark Sharpe (annualized) the PSR and MinTRL are measured against.
    pub benchmark: f64,
    /// Probabilistic Sharpe ratio: P(true Sharpe > benchmark).
    pub psr: f64,
    /// Minimum track record length (in periods and years) for the Sharpe to beat the
    /// benchmark at `confidence`; `None` when the observed Sharpe is not above it.
    pub confidence: f64,
    pub min_trl_periods: Option<f64>,
    pub min_trl_years: Option<f64>,
    /// Lo (2002) annualization corrected for serial correlation (q = ppy, capped at 252 lags).
    pub sharpe_annual_lo: Option<f64>,
}

/// Significance of a return series' Sharpe ratio: t-test, Lo and Mertens standard errors,
/// Probabilistic Sharpe Ratio and Minimum Track Record Length (Bailey & López de Prado 2012).
pub fn sharpe_inference(rets: &[f64], ppy: f64, rf_annual: f64, benchmark_annual: f64, confidence: f64) -> Option<SharpeInference> {
    let n = rets.len();
    if n < 10 {
        return None;
    }
    let rf = rf_annual / ppy;
    let ex: Vec<f64> = rets.iter().map(|r| r - rf).collect();
    let mom = moments(&ex)?;
    if mom.stdev <= 0.0 {
        return None;
    }
    let sr = mom.mean / mom.stdev;
    let ann = ppy.sqrt();
    let g3 = mom.skew_biased;
    let g4 = mom.excess_kurtosis_biased + 3.0;
    let nf = n as f64;
    let t_stat = sr * nf.sqrt();
    let se_iid = ((1.0 + 0.5 * sr * sr) / nf).sqrt() * ann;
    let var_nn = 1.0 - g3 * sr + (g4 - 1.0) / 4.0 * sr * sr;
    let se_nn = (var_nn.max(0.0) / nf).sqrt() * ann;
    let bench = benchmark_annual / ann;
    let psr = norm_cdf((sr - bench) * (nf - 1.0).sqrt() / var_nn.max(1e-12).sqrt());
    let za = norm_ppf(confidence);
    let min_trl = (sr > bench).then(|| 1.0 + var_nn * (za / (sr - bench)).powi(2));
    // Lo's serial-correlation adjustment: η(q) = q / sqrt(q + 2 Σ (q-k) ρ_k).
    let q = (ppy.round() as usize).clamp(2, 252).min(n / 2);
    let rho = acf(&ex, q);
    let denom = q as f64 + 2.0 * (1..q).map(|k| (q - k) as f64 * rho[k]).sum::<f64>();
    let lo = (denom > 0.0).then(|| q as f64 / denom.sqrt() * sr * (ppy / q as f64).sqrt());
    Some(SharpeInference {
        observations: n,
        periods_per_year: ppy,
        sharpe_period: sr,
        sharpe_annual: sr * ann,
        skew: g3,
        kurtosis: g4,
        t_stat,
        p_value: t_two_sided(t_stat, nf - 1.0),
        se_iid,
        se_nonnormal: se_nn,
        ci_low: sr * ann - 1.96 * se_nn,
        ci_high: sr * ann + 1.96 * se_nn,
        benchmark: benchmark_annual,
        psr,
        confidence,
        min_trl_periods: min_trl,
        min_trl_years: min_trl.map(|p| p / ppy),
        sharpe_annual_lo: lo,
    })
}

/// The same significance figures from summary numbers alone (a Sharpe someone quotes), for the
/// calculator: annualized Sharpe, observation count, periods per year, skew and raw kurtosis.
pub fn sharpe_from_summary(
    sharpe_annual: f64,
    observations: usize,
    ppy: f64,
    skew: f64,
    kurtosis: f64,
    benchmark_annual: f64,
    confidence: f64,
) -> Option<SharpeInference> {
    if observations < 2 || ppy <= 0.0 {
        return None;
    }
    let ann = ppy.sqrt();
    let sr = sharpe_annual / ann;
    let nf = observations as f64;
    let var_nn = 1.0 - skew * sr + (kurtosis - 1.0) / 4.0 * sr * sr;
    let bench = benchmark_annual / ann;
    let se_nn = (var_nn.max(0.0) / nf).sqrt() * ann;
    let za = norm_ppf(confidence);
    let min_trl = (sr > bench).then(|| 1.0 + var_nn * (za / (sr - bench)).powi(2));
    let t_stat = sr * nf.sqrt();
    Some(SharpeInference {
        observations,
        periods_per_year: ppy,
        sharpe_period: sr,
        sharpe_annual,
        skew,
        kurtosis,
        t_stat,
        p_value: t_two_sided(t_stat, nf - 1.0),
        se_iid: ((1.0 + 0.5 * sr * sr) / nf).sqrt() * ann,
        se_nonnormal: se_nn,
        ci_low: sharpe_annual - 1.96 * se_nn,
        ci_high: sharpe_annual + 1.96 * se_nn,
        benchmark: benchmark_annual,
        psr: norm_cdf((sr - bench) * (nf - 1.0).sqrt() / var_nn.max(1e-12).sqrt()),
        confidence,
        min_trl_periods: min_trl,
        min_trl_years: min_trl.map(|p| p / ppy),
        sharpe_annual_lo: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-normal noise (Box-Muller over an LCG), for shape checks only.
    fn noise(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        let mut u = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        (0..n).map(|_| (-2.0 * u().ln()).sqrt() * (2.0 * std::f64::consts::PI * u()).cos()).collect()
    }

    #[test]
    fn white_noise_looks_like_white_noise() {
        let x = noise(4000, 7);
        let m = moments(&x).unwrap();
        assert!(m.skew.abs() < 0.15 && m.excess_kurtosis.abs() < 0.3);
        assert!(m.jb_pvalue > 0.01);
        let h = hurst(&x);
        assert!((h.rs.unwrap() - 0.5).abs() < 0.1, "{:?}", h.rs);
        assert!((h.dfa.unwrap() - 0.5).abs() < 0.1, "{:?}", h.dfa);
        let vr = variance_ratio(&x.iter().scan(0.0, |a, v| { *a += v; Some(*a) }).collect::<Vec<_>>(), 4).unwrap();
        assert!((vr.vr - 1.0).abs() < 0.1);
        // Noise is stationary; its running sum is not (the walk's p-value is a draw, so only
        // the ordering is asserted).
        let walk: Vec<f64> = x.iter().scan(0.0, |a, v| { *a += v; Some(*a) }).collect();
        assert!(adf(&x, true, 1).unwrap().pvalue < 0.01);
        assert!(adf(&walk, true, 1).unwrap().pvalue > 0.01);
        assert!(kpss(&walk).unwrap().pvalue <= 0.05);
    }

    #[test]
    fn pacf_of_an_ar1_cuts_off_after_lag_one() {
        let e = noise(5000, 3);
        let mut x = vec![0.0];
        for v in &e[1..] {
            let last = *x.last().unwrap();
            x.push(0.6 * last + v);
        }
        let p = pacf(&x, 5);
        assert!((p[1] - 0.6).abs() < 0.05);
        assert!(p[2].abs() < 0.05 && p[3].abs() < 0.05);
        let hl = half_life(&x).unwrap();
        assert!((hl.half_life.unwrap() - (-(2f64.ln()) / 0.6f64.ln())).abs() < 0.3);
    }

    #[test]
    fn mackinnon_matches_statsmodels_points() {
        // statsmodels.tsa.adfvalues.mackinnonp / mackinnoncrit.
        assert!((mackinnon_p(-2.86, 1) - 0.050_201_099_882_003_074).abs() < 1e-9);
        assert!((mackinnon_p(-3.5, 2) - 0.032_395_388_360_173_73).abs() < 1e-9);
        let c = mackinnon_crit(1, 500);
        assert!((c[1] - -2.867_337_86).abs() < 1e-8);
    }
}
