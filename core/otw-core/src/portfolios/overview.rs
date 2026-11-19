//! Views across books: every portfolio summed, symbol overlap, currency exposure, and the
//! sector and country groupings of one book.
//!
//! Pure: the API hands over positions already valued in one currency.

use std::collections::BTreeMap;

use serde::Serialize;

/// One holding of one portfolio, valued in the overview currency.
#[derive(Debug, Clone)]
pub struct Held {
    pub portfolio: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub value: f64,
}

/// The same symbol across books.
#[derive(Debug, Clone, Serialize)]
pub struct Overlap {
    pub symbol: String,
    pub name: String,
    pub portfolios: Vec<String>,
    pub value: f64,
    /// Share of everything held, percent.
    pub weight_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Share {
    pub key: String,
    pub value: f64,
    pub weight_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Aggregate {
    /// Every symbol, biggest first. `portfolios.len() > 1` is an overlap.
    pub symbols: Vec<Overlap>,
    pub overlaps: Vec<Overlap>,
    pub currencies: Vec<Share>,
    /// The five largest symbols across every book.
    pub top: Vec<Overlap>,
    pub total: f64,
}

/// Symbols are matched upper case and trimmed: `vt` and `VT` are the same listing. Nothing
/// fuzzier: two tickers that merely look alike are two instruments.
fn key(symbol: &str) -> String {
    symbol.trim().to_uppercase()
}

pub fn aggregate(held: &[Held]) -> Aggregate {
    let total: f64 = held.iter().map(|h| h.value.max(0.0)).sum();
    let pct = |v: f64| if total > 0.0 { v / total * 100.0 } else { 0.0 };

    let mut by_symbol: BTreeMap<String, Overlap> = BTreeMap::new();
    let mut by_ccy: BTreeMap<String, f64> = BTreeMap::new();
    for h in held.iter().filter(|h| h.value > 0.0) {
        let e = by_symbol.entry(key(&h.symbol)).or_insert_with(|| Overlap {
            symbol: key(&h.symbol),
            name: h.name.clone(),
            portfolios: Vec::new(),
            value: 0.0,
            weight_pct: 0.0,
        });
        e.value += h.value;
        if !e.portfolios.contains(&h.portfolio) {
            e.portfolios.push(h.portfolio.clone());
        }
        *by_ccy.entry(h.currency.clone()).or_default() += h.value;
    }
    let mut symbols: Vec<Overlap> = by_symbol
        .into_values()
        .map(|mut o| {
            o.weight_pct = pct(o.value);
            o
        })
        .collect();
    symbols.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    let overlaps = symbols.iter().filter(|o| o.portfolios.len() > 1).cloned().collect();
    let top = symbols.iter().take(5).cloned().collect();
    let mut currencies: Vec<Share> =
        by_ccy.into_iter().map(|(k, v)| Share { key: k, value: v, weight_pct: pct(v) }).collect();
    currencies.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    Aggregate { symbols, overlaps, currencies, top, total }
}

/// Group values by a label. An empty label is `unclassified`, named as a bucket of its own
/// so the share nobody classified is seen rather than spread over the others.
pub fn group(items: &[(String, f64)]) -> Vec<Share> {
    let mut m: BTreeMap<String, f64> = BTreeMap::new();
    for (label, v) in items.iter().filter(|(_, v)| *v > 0.0) {
        let k = if label.trim().is_empty() { "unclassified".to_string() } else { label.trim().to_string() };
        *m.entry(k).or_default() += v;
    }
    let total: f64 = m.values().sum();
    let mut out: Vec<Share> = m
        .into_iter()
        .map(|(key, value)| Share { key, value, weight_pct: if total > 0.0 { value / total * 100.0 } else { 0.0 } })
        .collect();
    out.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(_pf: u8, name: &str, sym: &str, ccy: &str, v: f64) -> Held {
        Held {
            portfolio: name.into(),
            symbol: sym.into(),
            name: sym.into(),
            currency: ccy.into(),
            value: v,
        }
    }

    #[test]
    fn the_same_symbol_in_two_books_merges() {
        let a = aggregate(&[
            h(1, "Main", "VT", "USD", 600.0),
            h(2, "Kids", "vt ", "USD", 200.0),
            h(2, "Kids", "CW8", "EUR", 200.0),
        ]);
        assert_eq!(a.overlaps.len(), 1);
        let vt = &a.overlaps[0];
        assert_eq!(vt.symbol, "VT");
        assert_eq!(vt.portfolios, vec!["Main".to_string(), "Kids".to_string()]);
        assert!((vt.value - 800.0).abs() < 1e-9);
        assert!((vt.weight_pct - 80.0).abs() < 1e-9);
        assert!((a.total - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn currencies_group_by_trade_currency() {
        let a = aggregate(&[h(1, "A", "X", "USD", 300.0), h(1, "A", "Y", "EUR", 100.0), h(2, "B", "Z", "USD", 600.0)]);
        assert_eq!(a.currencies[0].key, "USD");
        assert!((a.currencies[0].weight_pct - 90.0).abs() < 1e-9);
        assert_eq!(a.currencies[1].key, "EUR");
    }

    #[test]
    fn groupings_sum_to_the_whole_and_name_the_unclassified() {
        let g = group(&[
            ("Technology".into(), 50.0),
            ("".into(), 30.0),
            ("Technology".into(), 10.0),
            ("Energy".into(), 10.0),
        ]);
        let sum: f64 = g.iter().map(|s| s.weight_pct).sum();
        assert!((sum - 100.0).abs() < 1e-9);
        assert_eq!(g[0].key, "Technology");
        assert!(g.iter().any(|s| s.key == "unclassified" && (s.value - 30.0).abs() < 1e-9));
    }
}
