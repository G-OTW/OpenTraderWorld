//! Net worth attribution: which categories moved the total over a period, and how.
//!
//! Each asset is valued at both ends the way the chart samples a point. An asset valued at
//! both ends moved by revaluation (`valuation`: a new revision, a market price, drift, FX);
//! one with no value yet at the start was added during the period (`added`); one valued at
//! the start and not at the end was removed (`removed`, its revisions deleted). The three
//! add up to the category's change, and the categories add up to the total change.

use std::collections::BTreeMap;

use serde::Serialize;

use otw_store::wealth::{AssetAt, MARKET_TYPES};

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct CategoryMove {
    pub category: String,
    pub from: f64,
    pub to: f64,
    pub delta: f64,
    pub valuation: f64,
    pub added: f64,
    pub removed: f64,
}

#[derive(Debug, Default, Serialize)]
pub struct Movers {
    /// Largest move first, by absolute size.
    pub categories: Vec<CategoryMove>,
    pub total_from: f64,
    pub total_to: f64,
    pub total_delta: f64,
    /// Revaluation of what a market prices (linked portfolios, broker accounts, wallets): the
    /// part of the change that is not the owner saving or spending.
    pub market_effect: f64,
    /// Change in the assumed drift between the two dates.
    pub drift_effect: f64,
    /// Assets left out at one end or the other for want of an FX rate.
    pub unconverted: usize,
}

/// Attribute the change between two valuations of the same assets (matched by id).
pub fn attribute(from: &[AssetAt], to: &[AssetAt]) -> Movers {
    let start: BTreeMap<_, _> = from.iter().map(|a| (a.id, a)).collect();
    let mut cats: BTreeMap<String, CategoryMove> = BTreeMap::new();
    let mut out = Movers::default();
    for end in to {
        let begin = start.get(&end.id).copied();
        if end.unconverted || begin.is_some_and(|b| b.unconverted) {
            out.unconverted += 1;
            continue;
        }
        let f = begin.and_then(|b| b.value);
        let t = end.value;
        let c = cats.entry(end.category.clone()).or_insert_with(|| CategoryMove {
            category: end.category.clone(),
            ..Default::default()
        });
        match (f, t) {
            (Some(f), Some(t)) => {
                c.valuation += t - f;
                if MARKET_TYPES.contains(&end.asset_type.as_str()) {
                    out.market_effect += t - f;
                }
            }
            (None, Some(t)) => c.added += t,
            (Some(f), None) => c.removed -= f,
            (None, None) => {}
        }
        c.from += f.unwrap_or(0.0);
        c.to += t.unwrap_or(0.0);
        out.drift_effect += end.estimated - begin.map(|b| b.estimated).unwrap_or(0.0);
    }
    for mut c in cats.into_values() {
        c.delta = c.to - c.from;
        out.total_from += c.from;
        out.total_to += c.to;
        if c.delta.abs() > 1e-9 || c.from.abs() > 1e-9 {
            out.categories.push(c);
        }
    }
    out.total_delta = out.total_to - out.total_from;
    out.categories
        .sort_by(|a, b| b.delta.abs().partial_cmp(&a.delta.abs()).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn at(id: u128, cat: &str, kind: &str, v: Option<f64>) -> AssetAt {
        AssetAt {
            id: Uuid::from_u128(id),
            name: String::new(),
            asset_type: kind.into(),
            category: cat.into(),
            owner: String::new(),
            sign: if v.is_some_and(|x| x < 0.0) { -1 } else { 1 },
            value: v,
            estimated: 0.0,
            unconverted: false,
        }
    }

    #[test]
    fn deltas_sum_to_the_total_change() {
        let from = vec![
            at(1, "Cash", "money", Some(10_000.0)),
            at(2, "Investments", "portfolio", Some(50_000.0)),
            at(3, "Debt", "loan", Some(-120_000.0)),
            at(4, "Cash", "money", None),
        ];
        let to = vec![
            at(1, "Cash", "money", Some(12_000.0)),
            at(2, "Investments", "portfolio", Some(47_000.0)),
            at(3, "Debt", "loan", Some(-118_500.0)),
            at(4, "Cash", "money", Some(3_000.0)),
        ];
        let m = attribute(&from, &to);
        let sum: f64 = m.categories.iter().map(|c| c.delta).sum();
        assert!((sum - m.total_delta).abs() < 1e-9);
        assert!((m.total_delta - 3_500.0).abs() < 1e-9);
        assert!((m.total_to - m.total_from - m.total_delta).abs() < 1e-9);
        // Each category's parts add up to its own change.
        for c in &m.categories {
            assert!((c.valuation + c.added + c.removed - c.delta).abs() < 1e-9, "{c:?}");
        }
        let cash = m.categories.iter().find(|c| c.category == "Cash").unwrap();
        assert_eq!(cash.valuation, 2_000.0);
        assert_eq!(cash.added, 3_000.0);
        // Only the portfolio is market effect; paying the loan down is not.
        assert_eq!(m.market_effect, -3_000.0);
        // Largest move first.
        assert_eq!(m.categories[0].category, "Cash");
    }

    #[test]
    fn a_removed_asset_is_counted_as_removed() {
        let m = attribute(&[at(1, "Car", "vehicle", Some(8_000.0))], &[at(1, "Car", "vehicle", None)]);
        assert_eq!(m.categories[0].removed, -8_000.0);
        assert_eq!(m.total_delta, -8_000.0);
    }

    #[test]
    fn an_unconverted_asset_is_left_out_and_counted() {
        let mut bad = at(1, "Cash", "money", None);
        bad.unconverted = true;
        let m = attribute(&[at(1, "Cash", "money", Some(5.0))], &[bad]);
        assert_eq!(m.unconverted, 1);
        assert!(m.categories.is_empty());
    }
}
