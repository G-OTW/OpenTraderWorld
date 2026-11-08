//! Return attribution: how much of the window's return each holding brought.
//!
//! Each day a holding contributes its opening weight times its own price return; the days
//! are linked geometrically (each day's contribution scaled by the growth before it), so
//! the contributions add up to the compounded return of the reconstructed book exactly.
//! Price returns, not value changes: a buy is money moved in, not a gain.

use serde::Serialize;

/// One line's daily inputs over the window.
pub struct Line {
    pub label: String,
    /// Units held at each day's close.
    pub qty: Vec<f64>,
    /// Close of each day, `None` when the series has no bar that day.
    pub close: Vec<Option<f64>>,
    /// Multiplier from the bar currency to the portfolio currency.
    pub fx: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Contribution {
    pub label: String,
    /// Average opening weight over the days measured, percent.
    pub avg_weight_pct: f64,
    /// The holding's own compounded price return over the days it was held, percent.
    pub return_pct: f64,
    /// Linked contribution to the book's return, percentage points.
    pub contribution_pct: f64,
    /// Contribution over the max drawdown span, percentage points.
    pub drawdown_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attribution {
    pub rows: Vec<Contribution>,
    /// Compounded return of the reconstructed book, percent. The rows add up to it.
    pub total_pct: f64,
    /// Days measured and days skipped for want of a close.
    pub days: usize,
    pub skipped: usize,
    pub drawdown_from: Option<usize>,
    pub drawdown_to: Option<usize>,
}

/// `nav[t]` is the book's net worth at day `t` (positions plus cash); the cash share is
/// whatever the lines do not hold, and it returns nothing.
pub fn attribute(lines: &[Line], nav: &[f64]) -> Option<Attribution> {
    let n = nav.len();
    if n < 2 {
        return None;
    }
    let k = lines.len();
    // Daily contributions per line, plus the cash line, for the days that could be measured.
    let mut daily: Vec<Vec<f64>> = vec![Vec::new(); k + 1];
    let mut weights: Vec<Vec<f64>> = vec![Vec::new(); k + 1];
    let mut port: Vec<f64> = Vec::new();
    let mut skipped = 0;
    for t in 1..n {
        let prev_nav = nav[t - 1];
        if prev_nav <= 1e-9 {
            skipped += 1;
            continue;
        }
        let mut ok = true;
        let mut c = vec![0.0; k + 1];
        let mut w = vec![0.0; k + 1];
        let mut held = 0.0;
        for (i, l) in lines.iter().enumerate() {
            let q = l.qty[t - 1];
            if q.abs() < 1e-12 {
                continue;
            }
            let (Some(p0), Some(p1)) = (l.close[t - 1], l.close[t]) else {
                ok = false;
                break;
            };
            if p0 <= 0.0 {
                ok = false;
                break;
            }
            let wi = q * p0 * l.fx / prev_nav;
            w[i] = wi;
            c[i] = wi * (p1 / p0 - 1.0);
            held += wi;
        }
        if !ok {
            skipped += 1;
            continue;
        }
        w[k] = 1.0 - held;
        port.push(c.iter().sum());
        for i in 0..=k {
            daily[i].push(c[i]);
            weights[i].push(w[i]);
        }
    }
    let m = port.len();
    if m == 0 {
        return None;
    }
    // Growth before each day, for the linking.
    let mut growth = Vec::with_capacity(m);
    let mut g = 1.0;
    for r in &port {
        growth.push(g);
        g *= 1.0 + r;
    }
    let total = g - 1.0;
    // Max drawdown span on the reconstructed index.
    let (mut peak, mut peak_i, mut worst, mut span) = (1.0, 0usize, 0.0, None);
    let mut idx = 1.0;
    for (t, r) in port.iter().enumerate() {
        idx *= 1.0 + r;
        if idx > peak {
            peak = idx;
            peak_i = t + 1;
        }
        let dd = idx / peak - 1.0;
        if dd < worst {
            worst = dd;
            span = Some((peak_i, t + 1));
        }
    }
    let mut rows = Vec::with_capacity(k + 1);
    for i in 0..=k {
        let contribution: f64 = (0..m).map(|t| daily[i][t] * growth[t]).sum();
        let drawdown = match span {
            Some((a, b)) => (a..b).map(|t| daily[i][t] * growth[t] / growth[a]).sum(),
            None => 0.0,
        };
        let held: Vec<usize> = (0..m).filter(|t| weights[i][*t].abs() > 1e-12).collect();
        let ret = if i < k {
            held.iter().fold(1.0, |acc, t| {
                let w = weights[i][*t];
                acc * (1.0 + daily[i][*t] / w)
            }) - 1.0
        } else {
            0.0
        };
        let avg_w = weights[i].iter().sum::<f64>() / m as f64;
        rows.push(Contribution {
            label: if i < k { lines[i].label.clone() } else { "cash".into() },
            avg_weight_pct: avg_w * 100.0 + 0.0,
            return_pct: ret * 100.0 + 0.0,
            contribution_pct: contribution * 100.0 + 0.0,
            drawdown_pct: drawdown * 100.0 + 0.0,
        });
    }
    rows.sort_by(|a, b| b.contribution_pct.partial_cmp(&a.contribution_pct).unwrap_or(std::cmp::Ordering::Equal));
    Some(Attribution {
        rows,
        total_pct: total * 100.0,
        days: m,
        skipped,
        drawdown_from: span.map(|s| s.0),
        drawdown_to: span.map(|s| s.1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two assets and some cash: the contributions add up to the book's compounded return.
    #[test]
    fn contributions_sum_to_the_return() {
        let a = Line { label: "A".into(), qty: vec![10.0; 5], close: [10.0, 11.0, 10.5, 12.0, 12.5].map(Some).to_vec(), fx: 1.0 };
        let b = Line { label: "B".into(), qty: vec![5.0; 5], close: [20.0, 19.0, 18.0, 19.5, 21.0].map(Some).to_vec(), fx: 1.0 };
        let cash = 50.0;
        let nav: Vec<f64> = (0..5)
            .map(|t| a.qty[t] * a.close[t].unwrap() + b.qty[t] * b.close[t].unwrap() + cash)
            .collect();
        let out = attribute(&[a, b], &nav).unwrap();
        let sum: f64 = out.rows.iter().map(|r| r.contribution_pct).sum();
        let book = (nav[4] / nav[0] - 1.0) * 100.0;
        assert!((out.total_pct - book).abs() < 1e-9, "{} vs {book}", out.total_pct);
        assert!((sum - out.total_pct).abs() < 1e-9, "{sum} vs {}", out.total_pct);
        let a_row = out.rows.iter().find(|r| r.label == "A").unwrap();
        assert!((a_row.return_pct - 25.0).abs() < 1e-9);
        assert!(out.rows.iter().any(|r| r.label == "cash" && r.contribution_pct.abs() < 1e-12));
        assert!(out.drawdown_from.is_some());
    }

    #[test]
    fn a_day_without_a_close_is_skipped_not_guessed() {
        let a = Line { label: "A".into(), qty: vec![1.0; 4], close: vec![Some(10.0), None, Some(12.0), Some(13.2)], fx: 1.0 };
        let out = attribute(&[a], &[10.0, 11.0, 12.0, 13.2]).unwrap();
        assert_eq!(out.days, 1);
        assert_eq!(out.skipped, 2);
        let gap = Line { label: "A".into(), qty: vec![1.0; 2], close: vec![Some(10.0), None], fx: 1.0 };
        assert!(attribute(&[gap], &[10.0, 10.0]).is_none());
    }
}
