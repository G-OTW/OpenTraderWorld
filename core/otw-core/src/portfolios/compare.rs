//! Two books on one normalized curve.
//!
//! Both time-weighted indexes are rebased to 100 on the first day both curves have, and
//! only the days both have are kept: a portfolio started in June compared from January
//! would read as flat for five months it did not exist.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Pair {
    pub dates: Vec<String>,
    pub a: Vec<f64>,
    pub b: Vec<f64>,
}

/// Rebase two equity indexes (`dates`, `equity`) to 100 on their first common day.
pub fn normalize(a_dates: &[String], a_eq: &[f64], b_dates: &[String], b_eq: &[f64]) -> Pair {
    let b: BTreeMap<&str, f64> = b_dates.iter().map(|d| d.as_str()).zip(b_eq.iter().copied()).collect();
    let mut out = Pair::default();
    let mut base: Option<(f64, f64)> = None;
    for (d, va) in a_dates.iter().zip(a_eq.iter().copied()) {
        let Some(vb) = b.get(d.as_str()).copied() else { continue };
        if base.is_none() {
            if va.abs() < 1e-12 || vb.abs() < 1e-12 {
                continue;
            }
            base = Some((va, vb));
        }
        let (ba, bb) = base.expect("set above");
        out.dates.push(d.clone());
        out.a.push(va / ba * 100.0);
        out.b.push(vb / bb * 100.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn both_curves_start_at_100_on_the_common_day() {
        let p = normalize(
            &s(&["2026-01-01", "2026-01-02", "2026-01-03", "2026-01-04"]),
            &[1.0, 1.1, 1.21, 1.1],
            &s(&["2026-01-02", "2026-01-03", "2026-01-04", "2026-01-05"]),
            &[2.0, 2.2, 2.0, 1.0],
        );
        assert_eq!(p.dates, s(&["2026-01-02", "2026-01-03", "2026-01-04"]));
        assert!((p.a[0] - 100.0).abs() < 1e-9 && (p.b[0] - 100.0).abs() < 1e-9);
        assert!((p.a[1] - 110.0).abs() < 1e-9);
        assert!((p.b[2] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn no_common_day_is_an_empty_pair() {
        let p = normalize(&s(&["2026-01-01"]), &[1.0], &s(&["2026-02-01"]), &[1.0]);
        assert!(p.dates.is_empty());
    }
}
