//! Wealth-level targets: the share each category should hold, with a tolerance band.
//!
//! The math is the Portfolio Tracker's (`portfolios/analytics/drift.rs`, `Drift::run`), copied
//! rather than shared because that file belongs to the Portfolio module: current share =
//! value / net worth, deviation = share - target, out of band when |deviation| > band, and
//! the money that would close the gap = (target - share) × net worth. Display only.

use std::collections::BTreeMap;

use serde::Serialize;

use otw_store::wealth::Target;

#[derive(Debug, Serialize, PartialEq)]
pub struct DriftRow {
    pub category: String,
    pub value: f64,
    pub current_pct: f64,
    pub target_pct: f64,
    pub deviation_pct: f64,
    pub band_pct: f64,
    pub out_of_band: bool,
    /// Money to add (positive) or take out (negative) to sit on target.
    pub delta_value: f64,
}

#[derive(Debug, Serialize)]
pub struct Unallocated {
    pub category: String,
    pub value: f64,
    pub current_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct DriftReport {
    pub net_worth: f64,
    pub rows: Vec<DriftRow>,
    /// Held but named by no target: a hole in the plan, never a silent zero target.
    pub unallocated: Vec<Unallocated>,
    pub worst_deviation_pct: f64,
    pub in_band: bool,
    pub target_total_pct: f64,
}

/// Why there is no drift to show: `no_targets`, `empty_book`, `negative_equity`.
pub fn drift(by_category: &BTreeMap<String, f64>, targets: &[Target]) -> Result<DriftReport, &'static str> {
    if targets.is_empty() {
        // No target is not a drift of zero.
        return Err("no_targets");
    }
    let base: f64 = by_category.values().sum();
    if base.abs() < 1e-9 {
        return Err("empty_book");
    }
    // A negative base flips every share at once and the plan would say sell what is held.
    if base < 0.0 {
        return Err("negative_equity");
    }
    let mut current = by_category.clone();
    let mut rows = Vec::new();
    let mut worst = 0.0_f64;
    for t in targets {
        let value = current.remove(&t.category).unwrap_or(0.0);
        let pct = value / base * 100.0;
        let deviation = pct - t.target_pct;
        worst = worst.max(deviation.abs());
        rows.push(DriftRow {
            category: t.category.clone(),
            value,
            current_pct: pct,
            target_pct: t.target_pct,
            deviation_pct: deviation,
            band_pct: t.band_pct,
            out_of_band: deviation.abs() > t.band_pct,
            delta_value: (t.target_pct - pct) / 100.0 * base,
        });
    }
    let unallocated = current
        .into_iter()
        .filter(|(_, v)| v.abs() > 1e-9)
        .map(|(category, value)| Unallocated { category, value, current_pct: value / base * 100.0 })
        .collect();
    let in_band = rows.iter().all(|r| !r.out_of_band);
    Ok(DriftReport {
        net_worth: base,
        rows,
        unallocated,
        worst_deviation_pct: worst,
        in_band,
        target_total_pct: targets.iter().map(|t| t.target_pct).sum(),
    })
}

/// Check a target set before it is stored.
pub fn validate(targets: &[Target]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for t in targets {
        if !(0.0..=100.0).contains(&t.target_pct) {
            return Err(format!("target for “{}” must be 0 to 100 %", t.category));
        }
        if !(t.band_pct >= 0.0 && t.band_pct.is_finite()) {
            return Err(format!("band for “{}” cannot be negative", t.category));
        }
        if !seen.insert(t.category.as_str()) {
            return Err(format!("“{}” has two targets", t.category));
        }
    }
    let total: f64 = targets.iter().map(|t| t.target_pct).sum();
    if total > 100.0 + 1e-9 {
        return Err(format!("targets add up to {total:.1} %, more than 100 %"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(c: &str, target: f64, band: f64) -> Target {
        Target { category: c.into(), target_pct: target, band_pct: band }
    }

    fn book() -> BTreeMap<String, f64> {
        BTreeMap::from([("Cash".to_string(), 30_000.0), ("Property".to_string(), 60_000.0), ("Crypto".to_string(), 10_000.0)])
    }

    #[test]
    fn shares_deviation_and_band_follow_the_portfolio_math() {
        let r = drift(&book(), &[t("Cash", 20.0, 5.0), t("Property", 60.0, 5.0)]).unwrap();
        assert_eq!(r.net_worth, 100_000.0);
        let cash = &r.rows[0];
        assert!((cash.current_pct - 30.0).abs() < 1e-9);
        assert!((cash.deviation_pct - 10.0).abs() < 1e-9);
        assert!(cash.out_of_band);
        assert!((cash.delta_value + 10_000.0).abs() < 1e-6);
        assert!(!r.rows[1].out_of_band);
        assert!(!r.in_band);
        assert!((r.worst_deviation_pct - 10.0).abs() < 1e-9);
        // What no target names is reported, not folded into a zero target.
        assert_eq!(r.unallocated.len(), 1);
        assert_eq!(r.unallocated[0].category, "Crypto");
    }

    #[test]
    fn a_target_on_an_empty_category_reads_as_a_buy() {
        let r = drift(&book(), &[t("Bonds", 10.0, 2.0)]).unwrap();
        assert_eq!(r.rows[0].value, 0.0);
        assert!((r.rows[0].delta_value - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn no_plan_an_empty_book_or_negative_equity_is_unavailable() {
        assert_eq!(drift(&book(), &[]).unwrap_err(), "no_targets");
        assert_eq!(drift(&BTreeMap::new(), &[t("Cash", 10.0, 1.0)]).unwrap_err(), "empty_book");
        let debt = BTreeMap::from([("Cash".to_string(), 10.0), ("Loan".to_string(), -50.0)]);
        assert_eq!(drift(&debt, &[t("Cash", 10.0, 1.0)]).unwrap_err(), "negative_equity");
    }

    #[test]
    fn target_sets_are_checked() {
        assert!(validate(&[t("Cash", 60.0, 5.0), t("Property", 50.0, 5.0)]).is_err());
        assert!(validate(&[t("Cash", 60.0, -1.0)]).is_err());
        assert!(validate(&[t("Cash", 10.0, 1.0), t("Cash", 10.0, 1.0)]).is_err());
        assert!(validate(&[t("Cash", 40.0, 5.0), t("", 60.0, 5.0)]).is_ok());
    }
}
