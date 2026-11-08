//! Property tests of the DCA savings plan (TESTING.md §5), complementing the randomized
//! engine checks of `fuzz_tests` (lookahead, determinism, cash ledger): the invariants a plan's
//! money must hold whatever the market and the plan are.
//!
//! Money is `f64` as in production; equalities are held to a relative 1e-9 of the money in.

use super::*;
use proptest::prelude::*;
use serde_json::json;

fn daily(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let d = time::Date::from_ordinal_date(2021, 1).unwrap() + time::Duration::days(i as i64);
            format!("{d}T00:00:00Z")
        })
        .collect()
}

fn owned(ticker: &str, ts: &[String], open: &[f64], close: &[f64]) -> OwnedBars {
    let high: Vec<f64> = open.iter().zip(close).map(|(o, c)| o.max(*c)).collect();
    let low: Vec<f64> = open.iter().zip(close).map(|(o, c)| o.min(*c)).collect();
    OwnedBars {
        ticker: ticker.into(),
        timeframe: "1d".into(),
        ts: ts.to_vec(),
        open: open.to_vec(),
        high,
        low,
        close: close.to_vec(),
        volume: vec![1.0; ts.len()],
        quotes: None,
        sub: None,
        script: None,
    }
}

/// A random walk of daily closes in basis points, the next open at the previous close.
fn path() -> impl Strategy<Value = (f64, Vec<i32>)> {
    (2_000i64..50_000, prop::collection::vec(-400i32..=400, 20..160)).prop_map(|(p0, r)| (p0 as f64 / 100.0, r))
}

fn market(ticker: &str, p0: f64, moves: &[i32], scale: f64) -> OwnedBars {
    let ts = daily(moves.len());
    let mut close = Vec::with_capacity(moves.len());
    let mut open = Vec::with_capacity(moves.len());
    let mut px = p0;
    for m in moves {
        open.push(px * scale);
        px *= 1.0 + *m as f64 / 10_000.0;
        close.push(px * scale);
    }
    owned(ticker, &ts, &open, &close)
}

/// One plan: weights over the tickers, a contribution, a drawdown-triggered buy and a
/// take-profit sell, all expressed in percent or money so a split cannot move them.
#[derive(Debug, Clone)]
struct Plan {
    capital: f64,
    weights: Vec<f64>,
    contribution: f64,
    period: &'static str,
    dip_pct: f64,
    dip_amount: f64,
    take_profit_pct: f64,
    fee_pct: f64,
    with_sells: bool,
}

fn plan() -> impl Strategy<Value = Plan> {
    (
        prop::sample::select(vec![0.0, 1_000.0, 10_000.0]),
        prop::collection::vec(1u32..=100, 1..=3),
        (1i64..2_000).prop_map(|c| c as f64),
        prop::sample::select(vec!["day", "week", "month"]),
        (1i64..30).prop_map(|c| c as f64),
        (1i64..1_000).prop_map(|c| c as f64),
        (2i64..40).prop_map(|c| c as f64),
        any::<bool>(),
    )
        .prop_map(|(capital, w, contribution, period, dip_pct, dip_amount, take_profit_pct, with_sells)| Plan {
            capital,
            weights: w.into_iter().map(f64::from).collect(),
            contribution,
            period,
            dip_pct,
            dip_amount,
            take_profit_pct,
            fee_pct: 0.0,
            with_sells,
        })
}

const TICKERS: [&str; 3] = ["AAA", "BBB", "CCC"];

fn settings(p: &Plan) -> Settings {
    let weights: Vec<_> = p.weights.iter().zip(TICKERS).map(|(w, t)| json!({"ticker": t, "weight": w})).collect();
    let sells = if p.with_sells {
        json!([{"amount_kind": "pct_position", "amount": 50.0, "per_asset": true, "target_gain_pct": p.take_profit_pct,
                "condition": {"logic": "all", "conditions": []}, "cooldown_bars": 5}])
    } else {
        json!([])
    };
    let v = json!({
        "kind": "dca",
        "mode": "long",
        "sizing": {"mode": "fixed_qty", "qty": 1.0},
        "starting_capital": p.capital,
        "fees": {"amount_kind": "pct", "per": "trade", "amount": p.fee_pct},
        "dca": {
            "weights": weights,
            "contribution": {"amount": p.contribution, "period": p.period, "every": 1, "invest": true},
            "buys": [{"amount_kind": "fixed", "amount": p.dip_amount, "per_asset": false, "cooldown_bars": 3,
                      "condition": {"logic": "all", "conditions": [
                          {"left": {"kind": "metric", "metric": "dd_from_high", "period": 0}, "op": "above",
                           "right": {"kind": "const", "value": p.dip_pct}}]}}],
            "sells": sells,
        },
    });
    let s: Settings = serde_json::from_value(v).expect("plan settings parse");
    assert!(s.validate().is_none(), "{:?}", s.validate());
    s
}

fn run_on(s: &Settings, markets: &[OwnedBars]) -> DcaStats {
    let bars: Vec<Bars> = markets.iter().map(|m| m.as_bars()).collect();
    let refs: Vec<&Bars> = bars.iter().collect();
    run_dca(s, &refs).dca.expect("a dca run carries its block")
}

fn markets(p: &Plan, base: &(f64, Vec<i32>), scale: f64) -> Vec<OwnedBars> {
    // Each further asset walks the same moves shifted, so the basket is not one asset thrice.
    (0..p.weights.len())
        .map(|a| {
            let mut m = base.1.clone();
            let shift = a * 7 % m.len().max(1);
            m.rotate_left(shift);
            market(TICKERS[a], base.0 * (1.0 + a as f64), &m, scale)
        })
        .collect()
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-9 * scale.abs().max(1.0)
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
    #![proptest_config(config(96))]

    /// A market that never moves is a zero-return period: whatever the plan buys and sells,
    /// without costs the value is the money in, and both returns are zero.
    #[test]
    fn a_flat_market_earns_nothing(p in plan(), px in 100i64..100_000, n in 20usize..200) {
        let flat = (px as f64 / 100.0, vec![0; n]);
        let d = run_on(&settings(&p), &markets(&p, &flat, 1.0));
        prop_assert!(close(d.final_value, d.contributed, d.contributed), "value {} vs money in {}", d.final_value, d.contributed);
        prop_assert!(d.total_return_pct.abs() < 1e-9, "return {}", d.total_return_pct);
        prop_assert!(d.twr_pct.abs() < 1e-9, "twr {}", d.twr_pct);
        prop_assert!(d.realized_pnl.abs() < 1e-9 * d.contributed.max(1.0));
    }

    /// Cash plus the holdings marked at the last price is the final value, the value the
    /// equity curve ends on, and the cash is never below zero (a plan spends what it has).
    #[test]
    fn cash_plus_holdings_is_the_final_value(p in plan(), base in path()) {
        let ms = markets(&p, &base, 1.0);
        let bars: Vec<Bars> = ms.iter().map(|m| m.as_bars()).collect();
        let refs: Vec<&Bars> = bars.iter().collect();
        let r = run_dca(&settings(&p), &refs);
        let d = r.dca.as_ref().unwrap();
        let held: f64 = d.assets.iter().map(|a| a.units * a.last_price).sum();
        let scale = d.contributed;
        prop_assert!(close(d.holdings_value, held, scale), "holdings {} vs units x price {}", d.holdings_value, held);
        prop_assert!(close(d.final_value, d.cash + d.holdings_value, scale));
        prop_assert!(close(r.equity.last().unwrap().equity, d.final_value, scale));
        prop_assert!(d.cash >= -1e-9 * scale.max(1.0), "cash {}", d.cash);
    }

    /// Commissions only take money away: the same plan on the same market with a percent fee
    /// never ends worth more than without, and every fill's fee is non-negative.
    #[test]
    fn fees_never_increase_the_final_value(p in plan(), base in path(), fee in 1i64..200) {
        let p = Plan { with_sells: false, ..p };
        let ms = markets(&p, &base, 1.0);
        let free = run_on(&settings(&p), &ms);
        let paid = run_on(&settings(&Plan { fee_pct: fee as f64 / 100.0, ..p.clone() }), &ms);
        prop_assert!(paid.final_value <= free.final_value + 1e-9 * free.contributed.max(1.0), "{} > {}", paid.final_value, free.final_value);
        prop_assert!(paid.events.iter().all(|e| e.fee >= 0.0));
        prop_assert!(close(paid.contributed, free.contributed, free.contributed));
    }

    /// A split N:1 on every asset (prices / N, N a power of two so the division is exact): the
    /// plan makes the same decisions, holds N times the units at a cost N times lower, and its
    /// money does not move.
    #[test]
    fn a_split_changes_units_not_money(p in plan(), base in path(), k in 1u32..=4) {
        let n = f64::from(1u32 << k);
        let a = run_on(&settings(&p), &markets(&p, &base, 1.0));
        let b = run_on(&settings(&p), &markets(&p, &base, 1.0 / n));
        let scale = a.contributed;
        prop_assert_eq!(a.events_total, b.events_total);
        prop_assert!(close(a.final_value, b.final_value, scale), "{} vs {}", a.final_value, b.final_value);
        prop_assert!(close(a.realized_pnl, b.realized_pnl, scale));
        prop_assert!(close(a.fees, b.fees, scale));
        for (x, y) in a.assets.iter().zip(&b.assets) {
            prop_assert!(close(x.units * n, y.units, y.units), "{}: units {} x {n} vs {}", x.ticker, x.units, y.units);
            prop_assert!(close(x.avg_cost / n, y.avg_cost, x.avg_cost), "{}: cost {} / {n} vs {}", x.ticker, x.avg_cost, y.avg_cost);
        }
    }

    /// The units held are never negative (a savings plan never shorts), and their weighted
    /// average cost lies between the cheapest and the dearest buy that built it.
    #[test]
    fn average_cost_lies_between_the_buy_prices(p in plan(), base in path()) {
        let d = run_on(&settings(&p), &markets(&p, &base, 1.0));
        prop_assert!(d.events_total <= EVENT_CAP, "the fill list was capped, the range check needs every buy");
        for a in &d.assets {
            prop_assert!(a.units >= 0.0, "{}: {} units", a.ticker, a.units);
            let buys: Vec<f64> = d.events.iter().filter(|e| e.ticker == a.ticker && e.action == "buy").map(|e| e.price).collect();
            if a.units <= 1e-12 || buys.is_empty() {
                continue;
            }
            let (lo, hi) = buys.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(*x), h.max(*x)));
            prop_assert!(a.avg_cost >= lo * (1.0 - 1e-9) && a.avg_cost <= hi * (1.0 + 1e-9), "{}: avg {} outside [{lo}, {hi}]", a.ticker, a.avg_cost);
        }
        for e in &d.events {
            prop_assert!(e.units_after >= -1e-12, "{e:?}");
        }
    }
}
