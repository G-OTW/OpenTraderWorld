//! Two (or a few) aligned series: cointegration, the tradable spread, rolling co-movement and
//! who leads whom.
//!
//! References the figures are checked against, on the same aligned bars:
//! - Engle-Granger: `statsmodels.tsa.stattools.coint` (trend "c", AIC lag search)
//! - Johansen: `statsmodels.tsa.vector_ar.vecm.coint_johansen(det_order=0)`
//! - Granger causality: `statsmodels.tsa.stattools.grangercausalitytests` (ssr F test)
//! - rolling correlation / beta and lagged correlation: pandas

use serde::Serialize;

use super::linalg::{self, Mat};
use super::special::f_sf;
use super::stats;
use super::{mean, stddev};

// ── Engle-Granger ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct EngleGranger {
    /// Regression y = alpha + beta x on the levels.
    pub alpha: f64,
    pub beta: f64,
    /// ADF tau on the residuals (no constant), its MacKinnon p-value for two series.
    pub stat: f64,
    pub pvalue: f64,
    pub used_lag: usize,
    /// Critical values at 1%, 5%, 10%.
    pub crit: [f64; 3],
}

/// Engle-Granger two-step test of `y` on `x` (levels, usually log prices).
pub fn engle_granger(y: &[f64], x: &[f64]) -> Option<EngleGranger> {
    let n = y.len().min(x.len());
    if n < 20 {
        return None;
    }
    let rows: Mat = (0..n).map(|i| vec![x[i], 1.0]).collect();
    let fit = linalg::ols(&y[..n], &rows)?;
    let (beta, alpha) = (fit.coef[0], fit.coef[1]);
    let resid: Vec<f64> = (0..n).map(|i| y[i] - alpha - beta * x[i]).collect();
    let adf = stats::adf(&resid, false, 2)?;
    Some(EngleGranger {
        alpha,
        beta,
        stat: adf.stat,
        pvalue: adf.pvalue,
        used_lag: adf.used_lag,
        crit: stats::mackinnon_crit(2, n - 1),
    })
}

// ── Johansen ────────────────────────────────────────────────────────────────────

/// MacKinnon-Haug-Michelis critical values (90%, 95%, 99%), constant term (det_order 0),
/// indexed by the number of series n − r being tested.
const TRACE_CRIT: [[f64; 3]; 12] = [
    [2.7055, 3.8415, 6.6349],
    [13.4294, 15.4943, 19.9349],
    [27.0669, 29.7961, 35.4628],
    [44.4929, 47.8545, 54.6815],
    [65.8202, 69.8189, 77.8202],
    [91.109, 95.7542, 104.9637],
    [120.3673, 125.6185, 135.9825],
    [153.6341, 159.529, 171.0905],
    [190.8714, 197.3772, 210.0366],
    [232.103, 239.2468, 253.2526],
    [277.374, 285.1402, 300.2821],
    [326.5354, 334.9795, 351.215],
];
const MAXEIG_CRIT: [[f64; 3]; 12] = [
    [2.7055, 3.8415, 6.6349],
    [12.2971, 14.2639, 18.52],
    [18.8928, 21.1314, 25.865],
    [25.1236, 27.5858, 32.7172],
    [31.2379, 33.8777, 39.3693],
    [37.2786, 40.0763, 45.8662],
    [43.2947, 46.2299, 52.3069],
    [49.2855, 52.3622, 58.6634],
    [55.2412, 58.4332, 64.996],
    [61.2041, 64.504, 71.2525],
    [67.1307, 70.5392, 77.4877],
    [73.0563, 76.5734, 83.7105],
];

#[derive(Debug, Serialize)]
pub struct JohansenRank {
    /// Null hypothesis: at most `r` cointegrating relations.
    pub r: usize,
    pub eigenvalue: f64,
    pub trace: f64,
    /// Trace critical values at 90%, 95%, 99%.
    pub trace_crit: [f64; 3],
    pub max_eig: f64,
    pub max_eig_crit: [f64; 3],
}

#[derive(Debug, Serialize)]
pub struct Johansen {
    pub lags: usize,
    pub ranks: Vec<JohansenRank>,
    /// Cointegration rank the trace test accepts at 95%.
    pub rank_95: usize,
    /// Cointegrating vectors (one per row, strongest first), scaled so the first
    /// coefficient is 1.
    pub vectors: Vec<Vec<f64>>,
}

fn demean_cols(m: &Mat) -> Mat {
    if m.is_empty() {
        return Vec::new();
    }
    let k = m[0].len();
    let means: Vec<f64> = (0..k).map(|j| m.iter().map(|r| r[j]).sum::<f64>() / m.len() as f64).collect();
    m.iter().map(|r| r.iter().zip(&means).map(|(v, mu)| v - mu).collect()).collect()
}

/// Residuals of each column of `y` regressed on the columns of `z` (no constant).
fn resid_on(y: &Mat, z: &Mat) -> Option<Mat> {
    if z.is_empty() || z[0].is_empty() {
        return Some(y.clone());
    }
    let zt = linalg::transpose(z);
    let inv = linalg::inverse(&linalg::matmul(&zt, z))?;
    let coef = linalg::matmul(&inv, &linalg::matmul(&zt, y));
    let fitted = linalg::matmul(z, &coef);
    Some(y.iter().zip(&fitted).map(|(a, b)| a.iter().zip(b).map(|(u, v)| u - v).collect()).collect())
}

fn cross(a: &Mat, b: &Mat) -> Mat {
    let t = a.len() as f64;
    linalg::matmul(&linalg::transpose(a), b).into_iter().map(|r| r.into_iter().map(|v| v / t).collect()).collect()
}

/// Johansen trace and maximum-eigenvalue tests with a constant (det_order 0) and `k` lagged
/// differences. `levels` has one row per bar and one column per series.
pub fn johansen(levels: &Mat, k: usize) -> Option<Johansen> {
    let t_all = levels.len();
    let n = levels.first()?.len();
    if !(2..=12).contains(&n) || t_all < 30 + k {
        return None;
    }
    let endog = demean_cols(levels);
    let dx: Mat = (1..t_all).map(|t| (0..n).map(|j| endog[t][j] - endog[t - 1][j]).collect()).collect();
    // Lagged differences: row t holds dx[t-1..=t-k], rows 0..k dropped.
    let z: Mat = (k..dx.len())
        .map(|t| (1..=k).flat_map(|l| dx[t - l].iter().copied()).collect())
        .collect();
    let z = demean_cols(&z);
    let dxk = demean_cols(&dx[k..].to_vec());
    let r0t = resid_on(&dxk, &z)?;
    let lx: Mat = endog[1..t_all - k].to_vec();
    let rkt = resid_on(&demean_cols(&lx), &z)?;
    let skk = cross(&rkt, &rkt);
    let sk0 = cross(&rkt, &r0t);
    let s00 = cross(&r0t, &r0t);
    let sig = linalg::matmul(&sk0, &linalg::matmul(&linalg::inverse(&s00)?, &linalg::transpose(&sk0)));
    // Symmetric form of the generalized problem skk⁻¹ sig: with skk = L Lᵀ, the eigenvalues of
    // L⁻¹ sig L⁻ᵀ are the same, and d = L⁻ᵀ u satisfies dᵀ skk d = I.
    let l = linalg::cholesky(&skk)?;
    let li = linalg::inverse(&l)?;
    let m = linalg::matmul(&li, &linalg::matmul(&sig, &linalg::transpose(&li)));
    let (vals, vecs) = linalg::sym_eigen(&m);
    let d = linalg::matmul(&linalg::transpose(&li), &vecs);
    let t = rkt.len() as f64;
    let mut ranks = Vec::with_capacity(n);
    for i in 0..n {
        let trace = -t * vals[i..].iter().map(|a| (1.0 - a).ln()).sum::<f64>();
        let max_eig = -t * (1.0 - vals[i]).ln();
        ranks.push(JohansenRank {
            r: i,
            eigenvalue: vals[i],
            trace,
            trace_crit: TRACE_CRIT[n - i - 1],
            max_eig,
            max_eig_crit: MAXEIG_CRIT[n - i - 1],
        });
    }
    let rank_95 = ranks.iter().take_while(|r| r.trace > r.trace_crit[1]).count();
    let vectors = (0..n)
        .map(|c| {
            let v: Vec<f64> = (0..n).map(|r| d[r][c]).collect();
            let s = if v[0].abs() > 1e-15 { v[0] } else { 1.0 };
            v.iter().map(|x| x / s).collect()
        })
        .collect();
    Some(Johansen { lags: k, ranks, rank_95, vectors })
}

// ── The spread ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SpreadPoint {
    pub ts: String,
    pub spread: f64,
    /// Full-sample z-score.
    pub z: f64,
    /// z-score against the trailing window's mean and stdev.
    pub z_rolling: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Spread {
    pub alpha: f64,
    pub hedge_ratio: f64,
    pub mean: f64,
    pub stdev: f64,
    pub z_now: f64,
    pub z_rolling_now: Option<f64>,
    pub half_life: Option<stats::HalfLife>,
    pub window: usize,
    /// Share of bars with |z| > 2 (full sample), a quick read of how often it stretches.
    pub beyond_2: f64,
    pub points: Vec<SpreadPoint>,
}

/// Spread y − alpha − beta·x from an OLS hedge ratio, with full-sample and rolling z-scores.
pub fn spread(y: &[f64], x: &[f64], ts: &[String], window: usize) -> Option<Spread> {
    let n = y.len().min(x.len());
    if n < 20 {
        return None;
    }
    let (alpha, beta, _) = super::ols(&y[..n], &x[..n])?;
    let s: Vec<f64> = (0..n).map(|i| y[i] - alpha - beta * x[i]).collect();
    let (m, sd) = (mean(&s), stddev(&s));
    if sd <= 0.0 {
        return None;
    }
    let w = window.clamp(5, n);
    let zr: Vec<Option<f64>> = (0..n)
        .map(|i| {
            if i + 1 < w {
                return None;
            }
            let win = &s[i + 1 - w..=i];
            let sdw = stddev(win);
            (sdw > 0.0).then(|| (s[i] - mean(win)) / sdw)
        })
        .collect();
    let stride = (n / 2000).max(1);
    let mut points: Vec<SpreadPoint> = (0..n)
        .step_by(stride)
        .map(|i| SpreadPoint { ts: ts.get(i).cloned().unwrap_or_default(), spread: s[i], z: (s[i] - m) / sd, z_rolling: zr[i] })
        .collect();
    if (n - 1) % stride != 0 {
        points.push(SpreadPoint { ts: ts.get(n - 1).cloned().unwrap_or_default(), spread: s[n - 1], z: (s[n - 1] - m) / sd, z_rolling: zr[n - 1] });
    }
    Some(Spread {
        alpha,
        hedge_ratio: beta,
        mean: m,
        stdev: sd,
        z_now: (s[n - 1] - m) / sd,
        z_rolling_now: zr[n - 1],
        half_life: stats::half_life(&s),
        window: w,
        beyond_2: s.iter().filter(|v| ((*v - m) / sd).abs() > 2.0).count() as f64 / n as f64,
        points,
    })
}

// ── Co-movement ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RollingPoint {
    pub ts: String,
    pub corr: Option<f64>,
    /// Beta of the first series on the second.
    pub beta: Option<f64>,
}

/// Rolling Pearson correlation and beta (cov(a, b) / var(b)) of two aligned return series.
pub fn rolling(a: &[f64], b: &[f64], ts: &[String], window: usize) -> Vec<RollingPoint> {
    let n = a.len().min(b.len());
    let w = window.clamp(5, n.max(5));
    if n < w {
        return Vec::new();
    }
    let stride = (n / 1500).max(1);
    let mut idx: Vec<usize> = (w - 1..n).step_by(stride).collect();
    if idx.last() != Some(&(n - 1)) {
        idx.push(n - 1);
    }
    idx.into_iter()
        .map(|i| {
            let (x, y) = (&a[i + 1 - w..=i], &b[i + 1 - w..=i]);
            let (mx, my) = (mean(x), mean(y));
            let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
            for j in 0..w {
                sxy += (x[j] - mx) * (y[j] - my);
                sxx += (x[j] - mx).powi(2);
                syy += (y[j] - my).powi(2);
            }
            RollingPoint {
                ts: ts.get(i).cloned().unwrap_or_default(),
                corr: (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt()),
                beta: (syy > 0.0).then(|| sxy / syy),
            }
        })
        .collect()
}

/// Pearson correlation of a_t with b_{t+k} for k = −maxlag..=maxlag on the overlapping bars.
/// A peak at positive k means `a` moves first and `b` follows k bars later.
pub fn lagged_correlation(a: &[f64], b: &[f64], maxlag: usize) -> Vec<(i64, f64)> {
    let n = a.len().min(b.len()) as i64;
    (-(maxlag as i64)..=maxlag as i64)
        .filter_map(|k| {
            let (xa, xb): (Vec<f64>, Vec<f64>) = (0..n)
                .filter(|&t| t + k >= 0 && t + k < n)
                .map(|t| (a[t as usize], b[(t + k) as usize]))
                .unzip();
            (xa.len() > 2).then(|| (k, super::correlation(&xa, &xb)))
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct GrangerLag {
    pub lag: usize,
    pub f: f64,
    pub pvalue: f64,
    pub df_num: usize,
    pub df_den: usize,
}

/// Does `x` Granger-cause `y`? F test of adding x's lags 1..=p to an autoregression of y,
/// for each p in 1..=maxlag.
pub fn granger(y: &[f64], x: &[f64], maxlag: usize) -> Vec<GrangerLag> {
    let n = y.len().min(x.len());
    (1..=maxlag)
        .filter_map(|p| {
            if n < 3 * p + 10 {
                return None;
            }
            let rows = n - p;
            let yv: Vec<f64> = (p..n).map(|t| y[t]).collect();
            let restricted: Mat = (p..n).map(|t| {
                let mut r = vec![1.0];
                r.extend((1..=p).map(|l| y[t - l]));
                r
            }).collect();
            let full: Mat = (p..n).map(|t| {
                let mut r = vec![1.0];
                r.extend((1..=p).map(|l| y[t - l]));
                r.extend((1..=p).map(|l| x[t - l]));
                r
            }).collect();
            let fr = linalg::ols(&yv, &restricted)?;
            let fu = linalg::ols(&yv, &full)?;
            let df_den = rows - (2 * p + 1);
            let f = (fr.ssr - fu.ssr) / fu.ssr / p as f64 * df_den as f64;
            Some(GrangerLag { lag: p, f, pvalue: f_sf(f, p as f64, df_den as f64), df_num: p, df_den })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        let mut u = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        (0..n).map(|_| (-2.0 * u().ln()).sqrt() * (2.0 * std::f64::consts::PI * u()).cos()).collect()
    }

    #[test]
    fn a_planted_pair_is_cointegrated_and_its_hedge_ratio_recovered() {
        let e = noise(3000, 5);
        // AR(1) spread with φ = 0.5: half-life ln 2 / ln 2 = 1 bar.
        let u: Vec<f64> = noise(3000, 9).into_iter().scan(0.0, |a, v| { *a = 0.5 * *a + v; Some(*a) }).collect();
        let x: Vec<f64> = e.iter().scan(0.0, |a, v| { *a += v; Some(*a) }).collect();
        let y: Vec<f64> = x.iter().zip(&u).map(|(a, b)| 2.0 + 1.5 * a + b).collect();
        let eg = engle_granger(&y, &x).unwrap();
        assert!((eg.beta - 1.5).abs() < 0.02 && eg.pvalue < 0.01);
        let lv: Mat = x.iter().zip(&y).map(|(a, b)| vec![*a, *b]).collect();
        let j = johansen(&lv, 1).unwrap();
        assert_eq!(j.rank_95, 1);
        let ts: Vec<String> = (0..3000).map(|i| i.to_string()).collect();
        let sp = spread(&y, &x, &ts, 60).unwrap();
        assert!((sp.half_life.unwrap().half_life.unwrap() - 1.0).abs() < 0.15);
    }

    #[test]
    fn a_leading_series_granger_causes_its_follower() {
        let x = noise(2000, 3);
        let e = noise(2000, 4);
        let y: Vec<f64> = (0..2000).map(|t| if t == 0 { e[0] } else { 0.5 * x[t - 1] + e[t] }).collect();
        assert!(granger(&y, &x, 2)[0].pvalue < 1e-6);
        assert!(granger(&x, &y, 2)[0].pvalue > 0.01);
        let lc = lagged_correlation(&x, &y, 3);
        let best = lc.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(best.0, 1);
    }
}
