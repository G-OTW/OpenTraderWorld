//! Property tests of the signal engine (TESTING.md §4.3), complementing `fuzz_tests` (which
//! replaces the bars after a cut, compares reruns and reconciles the books on random
//! strategies): here the cut is a plain truncation, the equity curve is checked bar by bar
//! against an independent mark-to-market, and a strategy that never trades must not move.
//!
//! Money is `f64` as in production; equalities are held to a relative 1e-9 of the capital.

use super::*;
use proptest::prelude::*;
use serde_json::json;

#[derive(Debug, Clone)]
struct Market {
    ts: Vec<String>,
    o: Vec<f64>,
    h: Vec<f64>,
    l: Vec<f64>,
    c: Vec<f64>,
    v: Vec<f64>,
}

impl Market {
    fn bars(&self) -> Bars<'_> {
        Bars { ticker: "X", ts: &self.ts, open: &self.o, high: &self.h, low: &self.l, close: &self.c, volume: &self.v, quotes: None, sub: None, script: None }
    }
    fn head(&self, k: usize) -> Market {
        Market {
            ts: self.ts[..k].to_vec(),
            o: self.o[..k].to_vec(),
            h: self.h[..k].to_vec(),
            l: self.l[..k].to_vec(),
            c: self.c[..k].to_vec(),
            v: self.v[..k].to_vec(),
        }
    }
}

/// Daily bars from a start price and per-bar (gap, move, wick) in basis points.
fn market(min: usize, max: usize) -> impl Strategy<Value = Market> {
    (1_000i64..100_000, prop::collection::vec((-50i32..=50, -300i32..=300, 0i32..=150), min..max)).prop_map(|(p0, rows)| {
        let mut m = Market { ts: Vec::new(), o: Vec::new(), h: Vec::new(), l: Vec::new(), c: Vec::new(), v: Vec::new() };
        let mut prev = p0 as f64 / 100.0;
        for (i, (gap, mv, wick)) in rows.into_iter().enumerate() {
            let o = prev * (1.0 + gap as f64 / 10_000.0);
            let c = o * (1.0 + mv as f64 / 10_000.0);
            let w = wick as f64 / 10_000.0;
            let d = time::Date::from_ordinal_date(2022, 1).unwrap() + time::Duration::days(i as i64);
            m.ts.push(format!("{d}T00:00:00Z"));
            m.o.push(o);
            m.h.push(o.max(c) * (1.0 + w));
            m.l.push(o.min(c) * (1.0 - w));
            m.c.push(c);
            m.v.push(1_000.0);
            prev = c;
        }
        m
    })
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-9 * scale.abs().max(1.0)
}

fn sma(period: usize) -> serde_json::Value {
    json!({"kind": "indicator", "indicator": "sma", "period": period})
}

fn parse(v: serde_json::Value) -> Settings {
    let s: Settings = serde_json::from_value(v).expect("settings parse");
    assert_eq!(s.validate(), None);
    s
}

/// A fixed seed by default, so every run explores the same cases (TESTING.md section 0, rule 5);
/// `PROPTEST_RNG_SEED=<n>` explores others. A failure is saved under `proptest-regressions/`
/// and replayed first on the next run: commit that file with the fix.
fn config(cases: u32) -> ProptestConfig {
    let mut c = ProptestConfig::with_cases(cases);
    if c.rng_seed == proptest::test_runner::RngSeed::Random {
        c.rng_seed = proptest::test_runner::RngSeed::Fixed(20_261_107);
    }
    c
}

proptest! {
    #![proptest_config(config(128))]

    /// A strategy whose entry never holds never trades: no trade, no open position, and the
    /// equity is the starting capital on every bar, costs configured or not.
    #[test]
    fn a_strategy_that_never_enters_keeps_equity_flat(
        m in market(5, 120), capital in 1_000i64..1_000_000, fee in 0i64..100, spread in 0i64..50,
    ) {
        let capital = capital as f64;
        let s = parse(json!({
            "kind": "signals",
            "mode": "both",
            "long": {"entry": {"logic": "all", "conditions": [{"left": {"kind": "const", "value": 0.0}, "op": "above", "right": {"kind": "const", "value": 1.0}}]}},
            "short": {"entry": {"logic": "all", "conditions": [{"left": {"kind": "const", "value": 0.0}, "op": "above", "right": {"kind": "const", "value": 1.0}}]}},
            "sizing": {"mode": "percent_equity", "percent": 50.0},
            "starting_capital": capital,
            "spread_pct": spread as f64 / 10_000.0,
            "fees": {"amount_kind": "pct", "per": "trade", "amount": fee as f64 / 100.0},
        }));
        let r = run(&s, &m.bars());
        prop_assert!(r.trades.is_empty() && r.open_positions.is_empty());
        prop_assert!(!r.equity.is_empty());
        for p in &r.equity {
            prop_assert_eq!(p.equity, capital, "{}", p.ts);
        }
        prop_assert_eq!(r.stats.final_equity, capital);
        prop_assert_eq!(r.stats.total_fees, 0.0);
    }

    /// Cash plus the open position marked at the close is the equity, on every bar: an
    /// always-long fixed quantity without costs is worth the capital before its entry, then
    /// the capital plus qty x (close - entry price).
    #[test]
    fn equity_is_cash_plus_the_position_marked_at_the_close(m in market(3, 120), qty in 1i64..1_000) {
        let qty = qty as f64 / 10.0;
        let mut s = super::tests::settings_always_long();
        s.sizing = Sizing::FixedQty { qty };
        s.starting_capital = 1.0e9;
        let r = run(&s, &m.bars());
        // One position, entered once and held: closed at "end" in `trades`, and still listed in
        // `open_positions` (what a paper session keeps watching after the last bar).
        prop_assert!(r.trades.len() <= 1 && r.open_positions.len() <= 1, "{:?} {:?}", r.trades, r.open_positions);
        let (entry_ts, entry) = match (r.trades.first(), r.open_positions.first()) {
            (Some(t), _) => (t.entry_ts.clone(), t.entry_price),
            (None, Some(p)) => (p.entry_ts.clone(), p.avg_price),
            _ => return Err(TestCaseError::fail("an always-true entry never entered")),
        };
        let at = m.ts.iter().position(|t| *t == entry_ts).expect("entry on a bar");
        for p in &r.equity {
            let i = m.ts.iter().position(|t| *t == p.ts).expect("equity on a bar");
            let want = if i < at { 1.0e9 } else { 1.0e9 + qty * (m.c[i] - entry) };
            prop_assert!(close(p.equity, want, 1.0e9), "bar {i}: equity {} vs cash + position {want}", p.equity);
        }
    }

    /// No look-ahead: the same run cut after bar k decides the same things up to k. Every bar
    /// before the cut's last one carries the same equity, every trade closed before it is the
    /// same trade, and a long-only run never holds a short.
    #[test]
    fn truncating_the_future_changes_nothing_before_the_cut(
        m in market(60, 160), fast in 2usize..10, slow in 11usize..30, cut in 0.3f64..0.95, fee in 0i64..50,
    ) {
        let s = parse(json!({
            "kind": "signals",
            "mode": "long",
            "long": {
                "entry": {"logic": "all", "conditions": [{"left": sma(fast), "op": "crosses_above", "right": sma(slow)}]},
                "exit": {"logic": "all", "conditions": [{"left": sma(fast), "op": "crosses_below", "right": sma(slow)}]},
                "stop_loss": {"kind": "pct", "value": 0.04},
            },
            "sizing": {"mode": "percent_equity", "percent": 80.0},
            "starting_capital": 10_000.0,
            "fees": {"amount_kind": "pct", "per": "trade", "amount": fee as f64 / 100.0},
        }));
        let k = ((m.ts.len() as f64 * cut) as usize).max(slow + 3);
        prop_assume!(k < m.ts.len());
        let full = run(&s, &m.bars());
        let head = m.head(k);
        let part = run(&s, &head.bars());
        let last = &m.ts[k - 1];
        let before: Vec<&EquityPoint> = full.equity.iter().filter(|p| p.ts < *last).collect();
        let cut_eq: Vec<&EquityPoint> = part.equity.iter().filter(|p| p.ts < *last).collect();
        prop_assert_eq!(before.len(), cut_eq.len());
        for (a, b) in before.iter().zip(&cut_eq) {
            prop_assert_eq!(&a.ts, &b.ts);
            prop_assert!(close(a.equity, b.equity, 10_000.0), "{}: {} vs {}", a.ts, a.equity, b.equity);
        }
        let closed = |r: &RunResult| -> Vec<(String, String, String)> {
            r.trades.iter().filter(|t| t.exit_ts < *last).map(|t| (t.entry_ts.clone(), t.exit_ts.clone(), t.exit_reason.clone())).collect()
        };
        prop_assert_eq!(closed(&full), closed(&part));
        prop_assert!(full.trades.iter().all(|t| t.direction == "long" && t.qty > 0.0));
        prop_assert!(full.open_positions.iter().all(|p| p.direction == "long" && p.qty > 0.0));
    }
}
