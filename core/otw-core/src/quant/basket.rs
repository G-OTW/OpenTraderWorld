//! A basket of aligned series: its principal components, how its members cluster, a
//! hierarchical risk parity allocation, a relative-strength league table, how a weighting
//! would have lived through past crises, and a factor regression of one asset on others.
//!
//! References the figures are checked against, on the same aligned returns:
//! - PCA: `sklearn.decomposition.PCA` on standardized returns
//! - linkage and leaf order: `scipy.cluster.hierarchy.linkage` / `to_tree().pre_order()`
//! - HRP: `pypfopt.HRPOpt` (López de Prado 2016)
//! - regression: `statsmodels.OLS`
//! - relative strength and stress windows: pandas

use serde::Serialize;

use super::linalg::{self, Mat};
use super::{correlation, covariance, mean, stddev};

// ── PCA ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Component {
    pub eigenvalue: f64,
    pub explained: f64,
    pub cumulative: f64,
    /// Loading of each asset, sign fixed so the largest absolute loading is positive.
    pub loadings: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct Pca {
    pub labels: Vec<String>,
    pub components: Vec<Component>,
}

/// Principal components of the correlation matrix of the returns.
pub fn pca(labels: &[String], rets: &[Vec<f64>]) -> Pca {
    let n = rets.len();
    let corr: Mat = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { correlation(&rets[i], &rets[j]) }).collect()).collect();
    let (vals, vecs) = linalg::sym_eigen(&corr);
    let total: f64 = vals.iter().map(|v| v.max(0.0)).sum();
    let mut cum = 0.0;
    let components = (0..n)
        .map(|c| {
            let mut l: Vec<f64> = (0..n).map(|r| vecs[r][c]).collect();
            let big = l.iter().cloned().fold(0.0_f64, |m, v| if v.abs() > m.abs() { v } else { m });
            if big < 0.0 {
                for v in &mut l {
                    *v = -*v;
                }
            }
            let ex = vals[c].max(0.0) / total;
            cum += ex;
            Component { eigenvalue: vals[c], explained: ex, cumulative: cum, loadings: l }
        })
        .collect();
    Pca { labels: labels.to_vec(), components }
}

// ── Hierarchical clustering ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Linkage {
    Single,
    Complete,
    Average,
    Ward,
}

/// One merge, in scipy's linkage-matrix layout: clusters `a` < `b` (ids ≥ n are earlier
/// merges), the distance they joined at, and the size of the new cluster.
#[derive(Debug, Clone, Serialize)]
pub struct Merge {
    pub a: usize,
    pub b: usize,
    pub distance: f64,
    pub size: usize,
}

#[derive(Debug, Serialize)]
pub struct Clustering {
    pub labels: Vec<String>,
    pub method: String,
    pub merges: Vec<Merge>,
    /// Leaf order of the dendrogram (left to right).
    pub order: Vec<usize>,
    /// Correlation distance √(½(1 − ρ)) the tree was built on.
    pub distance: Vec<Vec<f64>>,
}

/// Correlation distance matrix √(½(1 − ρ)), clipped to [0, 1].
pub fn corr_distance(rets: &[Vec<f64>]) -> Mat {
    let n = rets.len();
    (0..n)
        .map(|i| {
            (0..n)
                .map(|j| if i == j { 0.0 } else { ((1.0 - correlation(&rets[i], &rets[j])) / 2.0).clamp(0.0, 1.0).sqrt() })
                .collect()
        })
        .collect()
}

/// Agglomerative clustering by Lance-Williams updates, relabeled into scipy's layout (merges
/// sorted by distance, cluster ids assigned in merge order).
pub fn linkage(dist: &Mat, method: Linkage) -> Vec<Merge> {
    let n = dist.len();
    if n < 2 {
        return Vec::new();
    }
    // Working distances between active clusters, keyed by their member lists.
    let mut active: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut d: Mat = dist.clone();
    let mut raw: Vec<(Vec<usize>, Vec<usize>, f64)> = Vec::new();
    while active.len() > 1 {
        let m = active.len();
        let (mut bi, mut bj, mut bd) = (0, 1, f64::INFINITY);
        for i in 0..m {
            for j in i + 1..m {
                if d[i][j] < bd {
                    (bi, bj, bd) = (i, j, d[i][j]);
                }
            }
        }
        let (ni, nj) = (active[bi].len() as f64, active[bj].len() as f64);
        let mut new_row = Vec::with_capacity(m);
        for k in 0..m {
            if k == bi || k == bj {
                continue;
            }
            let nk = active[k].len() as f64;
            let (dik, djk) = (d[bi][k], d[bj][k]);
            let v = match method {
                Linkage::Single => dik.min(djk),
                Linkage::Complete => dik.max(djk),
                Linkage::Average => (ni * dik + nj * djk) / (ni + nj),
                Linkage::Ward => {
                    let t = ni + nj + nk;
                    (((ni + nk) * dik * dik + (nj + nk) * djk * djk - nk * bd * bd) / t).max(0.0).sqrt()
                }
            };
            new_row.push(v);
        }
        let mut merged = active[bi].clone();
        merged.extend(active[bj].iter().copied());
        raw.push((active[bi].clone(), active[bj].clone(), bd));
        // Rebuild the working matrix without bi, bj and with the merged cluster last.
        let keep: Vec<usize> = (0..m).filter(|&k| k != bi && k != bj).collect();
        let mut nd: Mat = keep.iter().map(|&a| keep.iter().map(|&b| d[a][b]).collect()).collect();
        for (r, row) in nd.iter_mut().enumerate() {
            row.push(new_row[r]);
        }
        let mut last = new_row.clone();
        last.push(0.0);
        nd.push(last);
        let mut na: Vec<Vec<usize>> = keep.iter().map(|&k| active[k].clone()).collect();
        na.push(merged);
        active = na;
        d = nd;
    }
    // Relabel: sort by distance (stable), then give each merge the next id; a merge's parts
    // are named by the cluster that currently holds their first member.
    raw.sort_by(|a, b| a.2.total_cmp(&b.2));
    let mut owner: Vec<usize> = (0..n).collect();
    let mut sizes: Vec<usize> = vec![1; n];
    let mut out = Vec::with_capacity(n - 1);
    for (step, (left, right, dd)) in raw.into_iter().enumerate() {
        let (a, b) = (owner[left[0]], owner[right[0]]);
        let (a, b) = (a.min(b), a.max(b));
        let size = sizes[a] + sizes[b];
        let id = n + step;
        for &leaf in left.iter().chain(&right) {
            owner[leaf] = id;
        }
        sizes.push(size);
        out.push(Merge { a, b, distance: dd, size });
    }
    out
}

/// Leaves in dendrogram order: pre-order walk from the root, left child first.
pub fn leaf_order(merges: &[Merge], n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    if merges.is_empty() {
        return (0..n).collect();
    }
    let mut out = Vec::with_capacity(n);
    let mut stack = vec![n + merges.len() - 1];
    while let Some(c) = stack.pop() {
        if c < n {
            out.push(c);
        } else {
            let m = &merges[c - n];
            stack.push(m.b);
            stack.push(m.a);
        }
    }
    out
}

pub fn clustering(labels: &[String], rets: &[Vec<f64>], method: Linkage) -> Clustering {
    let dist = corr_distance(rets);
    let merges = linkage(&dist, method);
    let order = leaf_order(&merges, rets.len());
    Clustering { labels: labels.to_vec(), method: format!("{method:?}").to_lowercase(), merges, order, distance: dist }
}

// ── Hierarchical risk parity ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Hrp {
    pub labels: Vec<String>,
    pub weights: Vec<f64>,
    /// The quasi-diagonal order the bisection walked.
    pub order: Vec<usize>,
    /// Annualized volatility of the HRP portfolio.
    pub vol: f64,
}

fn cluster_var(cov: &Mat, items: &[usize]) -> f64 {
    let ivp: Vec<f64> = items.iter().map(|&i| 1.0 / cov[i][i]).collect();
    let s: f64 = ivp.iter().sum();
    let w: Vec<f64> = ivp.iter().map(|v| v / s).collect();
    let mut v = 0.0;
    for (a, &i) in items.iter().enumerate() {
        for (b, &j) in items.iter().enumerate() {
            v += w[a] * w[b] * cov[i][j];
        }
    }
    v
}

/// Hierarchical risk parity (López de Prado): single-linkage tree on correlation distance,
/// quasi-diagonal order, recursive bisection with inverse-variance cluster weights.
pub fn hrp(labels: &[String], rets: &[Vec<f64>], ppy: f64, method: Linkage) -> Hrp {
    let n = rets.len();
    let cov: Mat = (0..n).map(|i| (0..n).map(|j| covariance(&rets[i], &rets[j])).collect()).collect();
    let merges = linkage(&corr_distance(rets), method);
    let order = leaf_order(&merges, n);
    let mut w = vec![1.0; n];
    let mut clusters: Vec<Vec<usize>> = vec![order.clone()];
    while !clusters.is_empty() {
        clusters = clusters
            .iter()
            .filter(|c| c.len() > 1)
            .flat_map(|c| {
                let h = c.len() / 2;
                [c[..h].to_vec(), c[h..].to_vec()]
            })
            .collect();
        for pair in clusters.chunks(2) {
            let (v1, v2) = (cluster_var(&cov, &pair[0]), cluster_var(&cov, &pair[1]));
            let alpha = 1.0 - v1 / (v1 + v2);
            for &i in &pair[0] {
                w[i] *= alpha;
            }
            for &i in &pair[1] {
                w[i] *= 1.0 - alpha;
            }
        }
    }
    let mut var = 0.0;
    for i in 0..n {
        for j in 0..n {
            var += w[i] * w[j] * cov[i][j];
        }
    }
    Hrp { labels: labels.to_vec(), weights: w, order, vol: (var * ppy).max(0.0).sqrt() }
}

// ── Relative strength ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StrengthRow {
    pub label: String,
    /// Total return over each lookback (same order as `lookbacks`), None when too short.
    pub returns: Vec<Option<f64>>,
    /// 12-month return skipping the most recent month (classic momentum).
    pub momentum_12_1: Option<f64>,
    /// Annualized volatility over the longest lookback.
    pub vol: Option<f64>,
    /// Longest-lookback return per unit of volatility.
    pub risk_adjusted: Option<f64>,
    /// Average rank across the lookbacks (1 = strongest).
    pub score: f64,
    pub rank: usize,
}

#[derive(Debug, Serialize)]
pub struct Strength {
    /// Lookbacks in bars (≈ 1, 3, 6, 12 months at the measured clock).
    pub lookbacks: Vec<usize>,
    pub rows: Vec<StrengthRow>,
}

pub fn relative_strength(labels: &[String], closes: &[Vec<f64>], ppy: f64) -> Strength {
    let lookbacks: Vec<usize> = [12.0, 4.0, 2.0, 1.0].iter().map(|d| ((ppy / d).round() as usize).max(1)).collect();
    let month = lookbacks[0];
    let ret_over = |c: &[f64], k: usize, skip: usize| -> Option<f64> {
        let n = c.len();
        (n > k + skip && c[n - 1 - skip - k] > 0.0).then(|| c[n - 1 - skip] / c[n - 1 - skip - k] - 1.0)
    };
    let mut rows: Vec<StrengthRow> = labels
        .iter()
        .zip(closes)
        .map(|(l, c)| {
            let returns: Vec<Option<f64>> = lookbacks.iter().map(|&k| ret_over(c, k, 0)).collect();
            let long = *lookbacks.last().unwrap_or(&1);
            let vol = (c.len() > long + 1).then(|| {
                let r = super::returns(&c[c.len() - 1 - long..]);
                stddev(&r) * ppy.sqrt()
            });
            let risk_adjusted = match (returns.last().copied().flatten(), vol) {
                (Some(r), Some(v)) if v > 0.0 => Some(r / v),
                _ => None,
            };
            StrengthRow {
                label: l.clone(),
                returns,
                momentum_12_1: ret_over(c, long - month.min(long - 1), month),
                vol,
                risk_adjusted,
                score: 0.0,
                rank: 0,
            }
        })
        .collect();
    // Rank per lookback (1 = best return); missing values rank last.
    let m = rows.len();
    let mut scores = vec![0.0; m];
    for li in 0..lookbacks.len() {
        let mut idx: Vec<usize> = (0..m).collect();
        idx.sort_by(|&a, &b| {
            let (x, y) = (rows[a].returns[li].unwrap_or(f64::NEG_INFINITY), rows[b].returns[li].unwrap_or(f64::NEG_INFINITY));
            y.total_cmp(&x)
        });
        for (pos, &i) in idx.iter().enumerate() {
            scores[i] += (pos + 1) as f64 / lookbacks.len() as f64;
        }
    }
    for (r, s) in rows.iter_mut().zip(&scores) {
        r.score = *s;
    }
    let mut idx: Vec<usize> = (0..m).collect();
    idx.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]));
    for (pos, &i) in idx.iter().enumerate() {
        rows[i].rank = pos + 1;
    }
    rows.sort_by_key(|r| r.rank);
    Strength { lookbacks, rows }
}

// ── Historical stress windows ───────────────────────────────────────────────────

/// Named crisis windows (first and last day, inclusive), peak to trough of the S&P 500.
pub const CRISES: &[(&str, &str, &str)] = &[
    ("gfc_2008", "2007-10-09", "2009-03-09"),
    ("flash_2010", "2010-04-23", "2010-07-02"),
    ("downgrade_2011", "2011-04-29", "2011-10-03"),
    ("china_2015", "2015-07-20", "2016-02-11"),
    ("volmageddon_2018", "2018-01-26", "2018-02-08"),
    ("q4_2018", "2018-09-20", "2018-12-24"),
    ("covid_2020", "2020-02-19", "2020-03-23"),
    ("bear_2022", "2022-01-03", "2022-10-12"),
];

#[derive(Debug, Serialize)]
pub struct StressWindow {
    pub key: String,
    pub from: String,
    pub to: String,
    /// Bars of the aligned clock inside the window.
    pub bars: usize,
    /// Buy-and-hold portfolio return over the window.
    pub ret: f64,
    /// Worst peak-to-trough inside the window.
    pub max_drawdown: f64,
    /// Each asset's own return over the window.
    pub asset_returns: Vec<f64>,
    /// Each asset's contribution to the portfolio return (weight × return).
    pub contributions: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct Stress {
    pub labels: Vec<String>,
    pub weights: Vec<f64>,
    pub windows: Vec<StressWindow>,
    /// Worst single periods of the rebalanced portfolio: (stamp, return).
    pub worst_periods: Vec<(String, f64)>,
}

/// Replay a weighting (bought at the window's first bar, held) through each crisis the aligned
/// history covers, plus the portfolio's worst single periods (weights rebalanced every bar).
pub fn stress(labels: &[String], clock: &[String], closes: &[Vec<f64>], weights: &[f64]) -> Stress {
    let n = closes.len();
    let day = |s: &str| s.get(..10).unwrap_or(s).to_string();
    let first = clock.first().map(|s| day(s)).unwrap_or_default();
    let last = clock.last().map(|s| day(s)).unwrap_or_default();
    let windows = CRISES
        .iter()
        .filter(|(_, from, to)| *from >= first.as_str() && *to <= last.as_str())
        .filter_map(|(key, from, to)| {
            let idx: Vec<usize> = (0..clock.len()).filter(|&i| {
                let d = day(&clock[i]);
                d.as_str() >= *from && d.as_str() <= *to
            }).collect();
            if idx.len() < 2 {
                return None;
            }
            let (i0, i1) = (idx[0], *idx.last()?);
            let value = |t: usize| (0..n).map(|a| weights[a] * closes[a][t] / closes[a][i0]).sum::<f64>();
            let mut peak = f64::NEG_INFINITY;
            let mut mdd = 0.0_f64;
            for &t in &idx {
                let v = value(t);
                peak = peak.max(v);
                mdd = mdd.max(1.0 - v / peak);
            }
            let asset_returns: Vec<f64> = (0..n).map(|a| closes[a][i1] / closes[a][i0] - 1.0).collect();
            let contributions = asset_returns.iter().zip(weights).map(|(r, w)| r * w).collect();
            Some(StressWindow {
                key: key.to_string(),
                from: clock[i0].clone(),
                to: clock[i1].clone(),
                bars: idx.len(),
                ret: value(i1) / value(i0) - 1.0,
                max_drawdown: mdd,
                asset_returns,
                contributions,
            })
        })
        .collect();
    let rets: Vec<Vec<f64>> = closes.iter().map(|c| super::returns(c)).collect();
    let mut port: Vec<(String, f64)> = (0..rets.first().map_or(0, |r| r.len()))
        .map(|t| (clock[t + 1].clone(), (0..n).map(|a| weights[a] * rets[a][t]).sum()))
        .collect();
    port.sort_by(|a, b| a.1.total_cmp(&b.1));
    port.truncate(10);
    Stress { labels: labels.to_vec(), weights: weights.to_vec(), windows, worst_periods: port }
}

// ── Factor regression ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FactorBeta {
    pub label: String,
    pub beta: f64,
    pub t: f64,
    pub p: f64,
}

#[derive(Debug, Serialize)]
pub struct Regression {
    pub asset: String,
    pub observations: usize,
    /// Intercept per period and annualized (× ppy), with its t and p.
    pub alpha: f64,
    pub alpha_annual: f64,
    pub alpha_t: f64,
    pub alpha_p: f64,
    pub betas: Vec<FactorBeta>,
    pub r2: f64,
    pub adj_r2: f64,
    /// Annualized stdev of the residuals.
    pub tracking_error: f64,
    /// Annualized alpha per unit of tracking error.
    pub information_ratio: Option<f64>,
    /// Against the first factor: mean asset return in its up periods over its mean up return,
    /// and the same for down periods.
    pub up_capture: Option<f64>,
    pub down_capture: Option<f64>,
    pub correlation: f64,
}

/// OLS of the asset's excess returns on the factors' excess returns (all per period).
pub fn regression(asset: &str, y: &[f64], factors: &[(String, Vec<f64>)], ppy: f64, rf_annual: f64) -> Option<Regression> {
    let n = y.len();
    if factors.is_empty() || n < factors.len() + 10 {
        return None;
    }
    let rf = rf_annual / ppy;
    let ye: Vec<f64> = y.iter().map(|v| v - rf).collect();
    let rows: Mat = (0..n).map(|t| {
        let mut r = vec![1.0];
        r.extend(factors.iter().map(|(_, f)| f[t] - rf));
        r
    }).collect();
    let fit = linalg::ols(&ye, &rows)?;
    let te = stddev(&fit.resid) * ppy.sqrt();
    let alpha_annual = fit.coef[0] * ppy;
    let f0 = &factors[0].1;
    let capture = |up: bool| -> Option<f64> {
        let idx: Vec<usize> = (0..n).filter(|&t| if up { f0[t] > 0.0 } else { f0[t] < 0.0 }).collect();
        let fm = mean(&idx.iter().map(|&t| f0[t]).collect::<Vec<_>>());
        (!idx.is_empty() && fm != 0.0).then(|| mean(&idx.iter().map(|&t| y[t]).collect::<Vec<_>>()) / fm)
    };
    Some(Regression {
        asset: asset.to_string(),
        observations: n,
        alpha: fit.coef[0],
        alpha_annual,
        alpha_t: fit.t[0],
        alpha_p: fit.p[0],
        betas: factors
            .iter()
            .enumerate()
            .map(|(i, (l, _))| FactorBeta { label: l.clone(), beta: fit.coef[i + 1], t: fit.t[i + 1], p: fit.p[i + 1] })
            .collect(),
        r2: fit.r2,
        adj_r2: fit.adj_r2,
        tracking_error: te,
        information_ratio: (te > 0.0).then(|| alpha_annual / te),
        up_capture: capture(true),
        down_capture: capture(false),
        correlation: correlation(y, f0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linkage_matches_a_hand_worked_example() {
        // Points on a line at 0, 1, 5, 6: single linkage joins (0,1) and (2,3) at 1, then 4.
        let p: [f64; 4] = [0.0, 1.0, 5.0, 6.0];
        let d: Mat = p.iter().map(|a| p.iter().map(|b| (a - b).abs()).collect()).collect();
        let m = linkage(&d, Linkage::Single);
        assert_eq!((m[0].a, m[0].b, m[0].distance), (0, 1, 1.0));
        assert_eq!((m[1].a, m[1].b, m[1].distance), (2, 3, 1.0));
        assert_eq!((m[2].a, m[2].b, m[2].distance, m[2].size), (4, 5, 4.0, 4));
        assert_eq!(leaf_order(&m, 4), vec![0, 1, 2, 3]);
        let c = linkage(&d, Linkage::Complete);
        assert_eq!(c[2].distance, 6.0);
        let a = linkage(&d, Linkage::Average);
        assert_eq!(a[2].distance, 5.0);
    }

    #[test]
    fn hrp_of_two_uncorrelated_assets_is_inverse_variance() {
        let a: Vec<f64> = (0..200).map(|i| if i % 2 == 0 { 0.01 } else { -0.01 }).collect();
        let b: Vec<f64> = (0..200).map(|i| if i % 4 < 2 { 0.02 } else { -0.02 }).collect();
        let h = hrp(&["a".into(), "b".into()], &[a, b], 252.0, Linkage::Single);
        // Variances 1e-4 and 4e-4: weights 0.8 / 0.2.
        assert!((h.weights[0] - 0.8).abs() < 1e-3 && (h.weights[1] - 0.2).abs() < 1e-3);
    }
}
