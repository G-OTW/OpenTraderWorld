//! Market regimes from a Gaussian hidden Markov model over returns.
//!
//! Baum-Welch (EM with scaled forward-backward) fits K Gaussian states and the transition
//! matrix; Viterbi decodes the most likely state path. States are reported low-volatility
//! first, so "state 0" means the same thing on every run. Fitting happens on returns in
//! percent to keep the variances well away from underflow, the same way the reference
//! (`hmmlearn.GaussianHMM`, diagonal covariance) is checked.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RegimeState {
    /// Mean return per period (fraction) and its annualized value.
    pub mean: f64,
    pub mean_annual: f64,
    /// Volatility per period (fraction) and annualized.
    pub vol: f64,
    pub vol_annual: f64,
    /// Share of the sample decoded in this state.
    pub occupancy: f64,
    /// Expected stay in periods, 1 / (1 − p_ii).
    pub expected_duration: f64,
}

#[derive(Debug, Serialize)]
pub struct Segment {
    pub from: String,
    pub to: String,
    pub state: usize,
    pub bars: usize,
    /// Compounded return over the segment.
    pub ret: f64,
}

#[derive(Debug, Serialize)]
pub struct Regimes {
    pub k: usize,
    pub iterations: usize,
    pub loglik: f64,
    pub converged: bool,
    pub states: Vec<RegimeState>,
    /// Row-stochastic transition matrix (from row to column).
    pub transition: Vec<Vec<f64>>,
    /// Filtered probability of each state at the last bar.
    pub current: Vec<f64>,
    /// Viterbi path compressed into runs.
    pub segments: Vec<Segment>,
}

struct Params {
    pi: Vec<f64>,
    a: Vec<Vec<f64>>,
    mu: Vec<f64>,
    var: Vec<f64>,
}

fn emission(x: f64, mu: f64, var: f64) -> f64 {
    ((-(x - mu).powi(2) / (2.0 * var)).exp() / (2.0 * std::f64::consts::PI * var).sqrt()).max(1e-300)
}

/// Scaled forward-backward; returns (loglik, gamma, xi sum, last filtered distribution).
fn forward_backward(x: &[f64], p: &Params) -> (f64, Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<f64>) {
    let (n, k) = (x.len(), p.mu.len());
    let b: Vec<Vec<f64>> = x.iter().map(|&v| (0..k).map(|j| emission(v, p.mu[j], p.var[j])).collect()).collect();
    let mut alpha = vec![vec![0.0; k]; n];
    let mut c = vec![0.0; n];
    for j in 0..k {
        alpha[0][j] = p.pi[j] * b[0][j];
    }
    c[0] = alpha[0].iter().sum::<f64>().max(1e-300);
    for j in 0..k {
        alpha[0][j] /= c[0];
    }
    for t in 1..n {
        for j in 0..k {
            let s: f64 = (0..k).map(|i| alpha[t - 1][i] * p.a[i][j]).sum();
            alpha[t][j] = s * b[t][j];
        }
        c[t] = alpha[t].iter().sum::<f64>().max(1e-300);
        for j in 0..k {
            alpha[t][j] /= c[t];
        }
    }
    let mut beta = vec![vec![1.0; k]; n];
    for t in (0..n - 1).rev() {
        for i in 0..k {
            beta[t][i] = (0..k).map(|j| p.a[i][j] * b[t + 1][j] * beta[t + 1][j]).sum::<f64>() / c[t + 1];
        }
    }
    let gamma: Vec<Vec<f64>> = (0..n)
        .map(|t| {
            let g: Vec<f64> = (0..k).map(|j| alpha[t][j] * beta[t][j]).collect();
            let s: f64 = g.iter().sum::<f64>().max(1e-300);
            g.iter().map(|v| v / s).collect()
        })
        .collect();
    let mut xi = vec![vec![0.0; k]; k];
    for t in 0..n - 1 {
        for i in 0..k {
            for j in 0..k {
                xi[i][j] += alpha[t][i] * p.a[i][j] * b[t + 1][j] * beta[t + 1][j] / c[t + 1];
            }
        }
    }
    let loglik = c.iter().map(|v| v.ln()).sum();
    let last = alpha[n - 1].clone();
    (loglik, gamma, xi, last)
}

fn viterbi(x: &[f64], p: &Params) -> Vec<usize> {
    let (n, k) = (x.len(), p.mu.len());
    let la: Vec<Vec<f64>> = p.a.iter().map(|r| r.iter().map(|v| v.max(1e-300).ln()).collect()).collect();
    let mut d = vec![vec![0.0; k]; n];
    let mut psi = vec![vec![0usize; k]; n];
    for j in 0..k {
        d[0][j] = p.pi[j].max(1e-300).ln() + emission(x[0], p.mu[j], p.var[j]).ln();
    }
    for t in 1..n {
        for j in 0..k {
            let (bi, bv) = (0..k)
                .map(|i| (i, d[t - 1][i] + la[i][j]))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((0, f64::NEG_INFINITY));
            d[t][j] = bv + emission(x[t], p.mu[j], p.var[j]).ln();
            psi[t][j] = bi;
        }
    }
    let mut path = vec![0usize; n];
    path[n - 1] = (0..k).max_by(|&a, &b| d[n - 1][a].total_cmp(&d[n - 1][b])).unwrap_or(0);
    for t in (0..n - 1).rev() {
        path[t] = psi[t + 1][path[t + 1]];
    }
    path
}

/// Deterministic starting point: equal-count quantile groups of the sorted sample, a sticky
/// transition matrix, uniform start.
pub fn initial_params(x: &[f64], k: usize) -> (Vec<f64>, Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
    let mut s = x.to_vec();
    s.sort_by(f64::total_cmp);
    let mut mu = Vec::with_capacity(k);
    let mut var = Vec::with_capacity(k);
    for j in 0..k {
        let g = &s[j * s.len() / k..(j + 1) * s.len() / k];
        let m = super::mean(g);
        mu.push(m);
        // Floor at a tenth of the overall variance so no group starts degenerate.
        var.push(super::stddev(g).powi(2).max(super::stddev(x).powi(2) * 0.1));
    }
    let off = 0.1 / (k - 1).max(1) as f64;
    let a = (0..k).map(|i| (0..k).map(|j| if i == j { 0.9 } else { off }).collect()).collect();
    (vec![1.0 / k as f64; k], a, mu, var)
}

/// Fit a K-state Gaussian HMM to percent returns `x` and decode it. `rets` are the matching
/// fractional returns (for segment compounding), `ts` the bar stamps of each return.
pub fn fit(x: &[f64], ts: &[String], ppy: f64, k: usize, max_iter: usize) -> Option<Regimes> {
    let k = k.clamp(2, 4);
    if x.len() < 50 * k {
        return None;
    }
    let (pi, a, mu, var) = initial_params(x, k);
    let mut p = Params { pi, a, mu, var };
    let mut prev = f64::NEG_INFINITY;
    let mut iterations = 0;
    let mut converged = false;
    let mut last_ll = prev;
    for it in 0..max_iter {
        iterations = it + 1;
        let (ll, gamma, xi, _) = forward_backward(x, &p);
        last_ll = ll;
        if (ll - prev).abs() < 1e-8 {
            converged = true;
            break;
        }
        prev = ll;
        // M step.
        p.pi = gamma[0].clone();
        for i in 0..k {
            let row: f64 = xi[i].iter().sum::<f64>().max(1e-300);
            for j in 0..k {
                p.a[i][j] = xi[i][j] / row;
            }
        }
        for j in 0..k {
            let w: f64 = gamma.iter().map(|g| g[j]).sum::<f64>().max(1e-300);
            let m = gamma.iter().zip(x).map(|(g, v)| g[j] * v).sum::<f64>() / w;
            let v = gamma.iter().zip(x).map(|(g, v)| g[j] * (v - m).powi(2)).sum::<f64>() / w;
            p.mu[j] = m;
            p.var[j] = v.max(1e-12);
        }
    }
    // Report order: calmest state first.
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|&i, &j| p.var[i].total_cmp(&p.var[j]));
    let rank: Vec<usize> = {
        let mut r = vec![0; k];
        for (new, &old) in order.iter().enumerate() {
            r[old] = new;
        }
        r
    };
    let path: Vec<usize> = viterbi(x, &p).into_iter().map(|s| rank[s]).collect();
    let (_, _, _, last) = forward_backward(x, &p);
    let n = x.len() as f64;
    let states = order
        .iter()
        .enumerate()
        .map(|(new, &old)| {
            let occ = path.iter().filter(|&&s| s == new).count() as f64 / n;
            let pii = p.a[old][old];
            RegimeState {
                mean: p.mu[old] / 100.0,
                mean_annual: p.mu[old] / 100.0 * ppy,
                vol: p.var[old].sqrt() / 100.0,
                vol_annual: p.var[old].sqrt() / 100.0 * ppy.sqrt(),
                occupancy: occ,
                expected_duration: if pii < 1.0 { 1.0 / (1.0 - pii) } else { f64::INFINITY },
            }
        })
        .collect();
    let transition = order.iter().map(|&i| order.iter().map(|&j| p.a[i][j]).collect()).collect();
    let current = order.iter().map(|&i| last[i]).collect();
    let mut segments: Vec<Segment> = Vec::new();
    let mut start = 0usize;
    let mut growth = 1.0;
    for t in 0..path.len() {
        growth *= 1.0 + x[t] / 100.0;
        if t + 1 == path.len() || path[t + 1] != path[t] {
            segments.push(Segment {
                from: ts.get(start).cloned().unwrap_or_default(),
                to: ts.get(t).cloned().unwrap_or_default(),
                state: path[t],
                bars: t + 1 - start,
                ret: growth - 1.0,
            });
            start = t + 1;
            growth = 1.0;
        }
    }
    Some(Regimes { k, iterations, loglik: last_ll, converged, states, transition, current, segments })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_a_calm_and_a_wild_regime() {
        // 300 calm bars, 300 wild bars, 300 calm: Gaussian noise at σ 0.5 and σ 3.
        let mut seed = 11u64;
        let mut u = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let x: Vec<f64> = (0..900)
            .map(|i| {
                let z = (-2.0 * u().ln()).sqrt() * (2.0 * std::f64::consts::PI * u()).cos();
                z * if (300..600).contains(&i) { 3.0 } else { 0.5 }
            })
            .collect();
        let ts: Vec<String> = (0..900).map(|i| i.to_string()).collect();
        let r = fit(&x, &ts, 252.0, 2, 300).unwrap();
        assert!(r.states[0].vol < r.states[1].vol);
        assert!((r.states[1].occupancy - 1.0 / 3.0).abs() < 0.05);
        assert!(r.segments.len() <= 9, "{}", r.segments.len());
    }
}
