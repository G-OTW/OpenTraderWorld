//! Small dense linear algebra for the statistical tests: multivariate OLS with standard
//! errors, a symmetric eigensolver, Cholesky, and a Nelder-Mead minimizer for the maximum
//! likelihood fits (GARCH, GPD).
//!
//! Matrices are row-major `Vec<Vec<f64>>`. Every problem here is tiny (a handful of
//! regressors, at most a few dozen assets), so clarity beats cache tricks.

pub type Mat = Vec<Vec<f64>>;

pub fn zeros(r: usize, c: usize) -> Mat {
    vec![vec![0.0; c]; r]
}

pub fn transpose(a: &Mat) -> Mat {
    if a.is_empty() {
        return Vec::new();
    }
    let (r, c) = (a.len(), a[0].len());
    let mut t = zeros(c, r);
    for i in 0..r {
        for j in 0..c {
            t[j][i] = a[i][j];
        }
    }
    t
}

pub fn matmul(a: &Mat, b: &Mat) -> Mat {
    let (n, m, p) = (a.len(), b.len(), b.first().map_or(0, |r| r.len()));
    let mut out = zeros(n, p);
    for i in 0..n {
        for k in 0..m {
            let aik = a[i][k];
            if aik == 0.0 {
                continue;
            }
            for j in 0..p {
                out[i][j] += aik * b[k][j];
            }
        }
    }
    out
}

pub fn matvec(a: &Mat, v: &[f64]) -> Vec<f64> {
    a.iter().map(|row| row.iter().zip(v).map(|(x, y)| x * y).sum()).collect()
}

/// Inverse by Gauss-Jordan with partial pivoting. `None` when singular.
pub fn inverse(a: &Mat) -> Option<Mat> {
    let n = a.len();
    let mut m: Mat = a
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
            r
        })
        .collect();
    let scale = a.iter().flatten().fold(0.0_f64, |s, x| s.max(x.abs())).max(1e-300);
    for col in 0..n {
        let piv = (col..n).max_by(|&x, &y| m[x][col].abs().total_cmp(&m[y][col].abs()))?;
        if m[piv][col].abs() < 1e-13 * scale {
            return None;
        }
        m.swap(col, piv);
        let p = m[col][col];
        for x in m[col].iter_mut() {
            *x /= p;
        }
        for r in 0..n {
            if r != col {
                let f = m[r][col];
                if f != 0.0 {
                    for c in 0..2 * n {
                        m[r][c] -= f * m[col][c];
                    }
                }
            }
        }
    }
    Some(m.into_iter().map(|r| r[n..].to_vec()).collect())
}

/// Lower-triangular Cholesky factor of a symmetric positive-definite matrix.
pub fn cholesky(a: &Mat) -> Option<Mat> {
    let n = a.len();
    let mut l = zeros(n, n);
    for i in 0..n {
        for j in 0..=i {
            let s: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let d = a[i][i] - s;
                if d <= 0.0 {
                    return None;
                }
                l[i][j] = d.sqrt();
            } else {
                l[i][j] = (a[i][j] - s) / l[j][j];
            }
        }
    }
    Some(l)
}

/// Eigen-decomposition of a symmetric matrix (cyclic Jacobi). Returns eigenvalues sorted
/// descending and the matching eigenvectors as the COLUMNS of the second matrix.
pub fn sym_eigen(a: &Mat) -> (Vec<f64>, Mat) {
    let n = a.len();
    let mut m = a.clone();
    let mut v = zeros(n, n);
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _sweep in 0..100 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| m[i][j] * m[i][j])
            .sum();
        if off < 1e-22 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if m[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (mkp, mkq) = (m[k][p], m[k][q]);
                    m[k][p] = c * mkp - s * mkq;
                    m[k][q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = c * mpk - s * mqk;
                    m[q][k] = s * mpk + c * mqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k][p], v[k][q]);
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| m[j][j].total_cmp(&m[i][i]));
    let vals = order.iter().map(|&i| m[i][i]).collect();
    let vecs = (0..n).map(|r| order.iter().map(|&c| v[r][c]).collect()).collect();
    (vals, vecs)
}

/// Result of an OLS fit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Ols {
    pub coef: Vec<f64>,
    pub se: Vec<f64>,
    pub t: Vec<f64>,
    /// Two-sided p-values from the t distribution with n - k degrees of freedom.
    pub p: Vec<f64>,
    pub r2: f64,
    pub adj_r2: f64,
    /// Sum of squared residuals.
    pub ssr: f64,
    pub nobs: usize,
    #[serde(skip)]
    pub resid: Vec<f64>,
    /// Gaussian log-likelihood, as statsmodels reports it (used for AIC lag selection).
    pub llf: f64,
}

/// OLS of `y` on the columns of `x` (rows = observations). Add a column of ones yourself for
/// an intercept. `None` when the design is rank-deficient or has no residual degrees of freedom.
pub fn ols(y: &[f64], x: &Mat) -> Option<Ols> {
    let n = y.len();
    let k = x.first().map_or(0, |r| r.len());
    if n != x.len() || k == 0 || n <= k {
        return None;
    }
    let mut xtx = zeros(k, k);
    let mut xty = vec![0.0; k];
    for (row, &yi) in x.iter().zip(y) {
        for a in 0..k {
            xty[a] += row[a] * yi;
            for b in a..k {
                xtx[a][b] += row[a] * row[b];
            }
        }
    }
    for a in 0..k {
        for b in 0..a {
            xtx[a][b] = xtx[b][a];
        }
    }
    let inv = inverse(&xtx)?;
    let coef = matvec(&inv, &xty);
    let resid: Vec<f64> = x
        .iter()
        .zip(y)
        .map(|(row, &yi)| yi - row.iter().zip(&coef).map(|(a, b)| a * b).sum::<f64>())
        .collect();
    let ssr: f64 = resid.iter().map(|e| e * e).sum();
    let df = (n - k) as f64;
    let s2 = ssr / df;
    let se: Vec<f64> = (0..k).map(|i| (inv[i][i] * s2).max(0.0).sqrt()).collect();
    let t: Vec<f64> = coef.iter().zip(&se).map(|(c, s)| if *s > 0.0 { c / s } else { f64::NAN }).collect();
    let p = t.iter().map(|&tv| super::special::t_two_sided(tv, df)).collect();
    // R² against the mean when there is an intercept column, against zero otherwise (the
    // convention statsmodels follows).
    let has_const = (0..k).any(|c| x.iter().all(|r| r[c] == 1.0));
    let my = y.iter().sum::<f64>() / n as f64;
    let tss: f64 = if has_const {
        y.iter().map(|v| (v - my).powi(2)).sum()
    } else {
        y.iter().map(|v| v * v).sum()
    };
    let r2 = if tss > 0.0 { 1.0 - ssr / tss } else { 0.0 };
    let adj_r2 = if has_const {
        1.0 - (1.0 - r2) * (n as f64 - 1.0) / df
    } else {
        1.0 - (1.0 - r2) * n as f64 / df
    };
    let nf = n as f64;
    let llf = -nf / 2.0 * ((2.0 * std::f64::consts::PI).ln() + (ssr / nf).ln() + 1.0);
    Some(Ols { coef, se, t, p, r2, adj_r2, ssr, nobs: n, resid, llf })
}

/// Nelder-Mead simplex minimizer. `step` sizes the initial simplex around `x0`.
pub fn nelder_mead<F: FnMut(&[f64]) -> f64>(mut f: F, x0: &[f64], step: &[f64], max_iter: usize, tol: f64) -> (Vec<f64>, f64) {
    let n = x0.len();
    let mut simplex: Vec<Vec<f64>> = vec![x0.to_vec()];
    for i in 0..n {
        let mut p = x0.to_vec();
        p[i] += step[i];
        simplex.push(p);
    }
    let mut vals: Vec<f64> = simplex.iter().map(|p| f(p)).collect();
    for _ in 0..max_iter {
        let mut idx: Vec<usize> = (0..=n).collect();
        idx.sort_by(|&a, &b| vals[a].total_cmp(&vals[b]));
        simplex = idx.iter().map(|&i| simplex[i].clone()).collect();
        vals = idx.iter().map(|&i| vals[i]).collect();
        let spread = (vals[n] - vals[0]).abs();
        let size: f64 = simplex[1..]
            .iter()
            .map(|p| p.iter().zip(&simplex[0]).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max))
            .fold(0.0, f64::max);
        if spread <= tol * (1.0 + vals[0].abs()) && size <= tol.sqrt() {
            break;
        }
        let centroid: Vec<f64> =
            (0..n).map(|j| simplex[..n].iter().map(|p| p[j]).sum::<f64>() / n as f64).collect();
        let along = |t: f64| -> Vec<f64> {
            (0..n).map(|j| centroid[j] + t * (simplex[n][j] - centroid[j])).collect()
        };
        let xr = along(-1.0);
        let fr = f(&xr);
        if fr < vals[0] {
            let xe = along(-2.0);
            let fe = f(&xe);
            if fe < fr {
                simplex[n] = xe;
                vals[n] = fe;
            } else {
                simplex[n] = xr;
                vals[n] = fr;
            }
        } else if fr < vals[n - 1] {
            simplex[n] = xr;
            vals[n] = fr;
        } else {
            let (xc, fc) = if fr < vals[n] {
                let x = along(-0.5);
                let v = f(&x);
                (x, v)
            } else {
                let x = along(0.5);
                let v = f(&x);
                (x, v)
            };
            if fc < vals[n].min(fr) {
                simplex[n] = xc;
                vals[n] = fc;
            } else {
                // Shrink toward the best vertex.
                for i in 1..=n {
                    for j in 0..n {
                        simplex[i][j] = simplex[0][j] + 0.5 * (simplex[i][j] - simplex[0][j]);
                    }
                    vals[i] = f(&simplex[i]);
                }
            }
        }
    }
    let best = (0..=n).min_by(|&a, &b| vals[a].total_cmp(&vals[b])).unwrap_or(0);
    (simplex[best].clone(), vals[best])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ols_recovers_known_coefficients() {
        // y = 1 + 2 x1 - 0.5 x2, exactly: residuals vanish.
        let xs: Mat = (0..20).map(|i| vec![1.0, i as f64, ((i * 7) % 5) as f64]).collect();
        let y: Vec<f64> = xs.iter().map(|r| 1.0 + 2.0 * r[1] - 0.5 * r[2]).collect();
        let fit = ols(&y, &xs).unwrap();
        assert!((fit.coef[0] - 1.0).abs() < 1e-9);
        assert!((fit.coef[1] - 2.0).abs() < 1e-9);
        assert!((fit.coef[2] + 0.5).abs() < 1e-9);
        assert!(fit.r2 > 0.999_999);
    }

    #[test]
    fn eigen_of_a_known_matrix() {
        let a = vec![vec![2.0, 1.0], vec![1.0, 2.0]];
        let (vals, vecs) = sym_eigen(&a);
        assert!((vals[0] - 3.0).abs() < 1e-12 && (vals[1] - 1.0).abs() < 1e-12);
        // First eigenvector ∝ (1, 1).
        assert!((vecs[0][0].abs() - vecs[1][0].abs()).abs() < 1e-12);
    }

    #[test]
    fn nelder_mead_finds_the_rosenbrock_minimum() {
        let f = |p: &[f64]| (1.0 - p[0]).powi(2) + 100.0 * (p[1] - p[0] * p[0]).powi(2);
        let (x, _) = nelder_mead(f, &[-1.2, 1.0], &[0.5, 0.5], 5000, 1e-14);
        assert!((x[0] - 1.0).abs() < 1e-5 && (x[1] - 1.0).abs() < 1e-5);
    }
}
