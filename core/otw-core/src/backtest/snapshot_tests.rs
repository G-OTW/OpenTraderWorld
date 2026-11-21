//! Snapshots of complete runs on the frozen market fixtures (`fixtures/market/`, generated with
//! a fixed seed, see its README): the trade list, the equity curve sampled every 10 bars and the
//! headline metrics, for one strategy per engine (signals long, signals both ways, DCA).
//!
//! A changed snapshot means the engine's numbers changed. Review it with `cargo insta review`
//! and justify the change; never accept one blind. Numbers are rounded to 6 decimals in the
//! snapshot so the last bits of a transcendental function (which differ between libm builds)
//! cannot make Linux and macOS disagree.

use super::*;
use serde_json::{json, Value};

/// A fixture file loaded into the shape the engine borrows.
fn load(name: &str, ticker: &str) -> OwnedBars {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/market").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut b = OwnedBars {
        ticker: ticker.into(),
        timeframe: "1d".into(),
        ts: Vec::new(),
        open: Vec::new(),
        high: Vec::new(),
        low: Vec::new(),
        close: Vec::new(),
        volume: Vec::new(),
        quotes: None,
        sub: None,
        script: None,
    };
    for (i, line) in text.lines().enumerate().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        assert_eq!(f.len(), 6, "{name} line {}: {line}", i + 1);
        let num = |k: usize| f[k].parse::<f64>().unwrap_or_else(|e| panic!("{name} line {}: {e}", i + 1));
        b.ts.push(f[0].to_string());
        b.open.push(num(1));
        b.high.push(num(2));
        b.low.push(num(3));
        b.close.push(num(4));
        b.volume.push(num(5));
    }
    b
}

fn settings(v: Value) -> Settings {
    let s: Settings = serde_json::from_value(v).expect("settings parse");
    assert_eq!(s.validate(), None);
    s
}

/// Every float rounded to 6 decimals, recursively.
fn rounded(v: Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => {
            let x = n.as_f64().unwrap();
            json!((x * 1e6).round() / 1e6)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(rounded).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, rounded(v))).collect()),
        other => other,
    }
}

/// What a reviewer reads: the trades, the curve every 10 bars plus the last point, the metrics.
fn view(r: &RunResult) -> Value {
    let mut equity: Vec<&EquityPoint> = r.equity.iter().step_by(10).collect();
    if let Some(last) = r.equity.last() {
        if r.equity.len() % 10 != 1 {
            equity.push(last);
        }
    }
    let s = &r.stats;
    rounded(json!({
        "warmup_bars": r.warmup_bars,
        "trading_start_ts": r.trading_start_ts,
        "trades": r.trades,
        "open_positions": r.open_positions,
        "equity_every_10": equity,
        "stats": {
            "engine_version": s.engine_version,
            "trades": s.trades,
            "wins": s.wins,
            "losses": s.losses,
            "net_pnl": s.net_pnl,
            "return_pct": s.return_pct,
            "total_fees": s.total_fees,
            "max_drawdown": s.max_drawdown,
            "max_drawdown_pct": s.max_drawdown_pct,
            "profit_factor": s.profit_factor,
            "final_equity": s.final_equity,
            "buy_hold_return_pct": s.buy_hold_return_pct,
            "sharpe": s.sharpe,
            "sortino": s.sortino,
            "exit_reasons": s.exit_reasons,
        },
        "dca": r.dca.as_ref().map(|d| json!({
            "contributed": d.contributed,
            "invested": d.invested,
            "fees": d.fees,
            "cash": d.cash,
            "final_value": d.final_value,
            "total_return_pct": d.total_return_pct,
            "twr_pct": d.twr_pct,
            "irr_pct": d.irr_pct,
            "lump_sum_value": d.lump_sum_value,
            "buys": d.buys,
            "sells": d.sells,
            "assets": d.assets,
        })),
        "notes": r.notes,
    }))
}

/// Run twice: the snapshot is only worth something if the run is reproducible.
fn run_twice(s: &Settings, inputs: &[&OwnedBars]) -> Value {
    let go = || {
        let bars: Vec<Bars> = inputs.iter().map(|b| b.as_bars()).collect();
        let refs: Vec<&Bars> = bars.iter().collect();
        view(&run_portfolio(s, &refs))
    };
    let first = go();
    assert_eq!(first, go(), "same input, same settings, different result");
    first
}

fn sma(period: usize) -> Value {
    json!({"kind": "indicator", "indicator": "sma", "period": period})
}

/// SMA 10/30 crossover, long only, 50% of equity, 0.1% fees, 5% stop, exit on the cross back.
#[test]
fn sma_cross_long_on_synth_a() {
    let a = load("synth_daily_a_seed42.csv", "A");
    let s = settings(json!({
        "kind": "signals",
        "mode": "long",
        "long": {
            "entry": {"logic": "all", "conditions": [{"left": sma(10), "op": "crosses_above", "right": sma(30)}]},
            "exit": {"logic": "all", "conditions": [{"left": sma(10), "op": "crosses_below", "right": sma(30)}]},
            "stop_loss": {"kind": "pct", "value": 0.05},
        },
        "sizing": {"mode": "percent_equity", "percent": 50.0},
        "starting_capital": 10_000.0,
        "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.1},
    }));
    insta::assert_yaml_snapshot!(run_twice(&s, &[&a]));
}

/// RSI(14) mean reversion both ways on two assets, fixed quantity, spread and fixed fees.
#[test]
fn rsi_reversion_both_ways_on_synth_a_and_b() {
    let (a, b) = (load("synth_daily_a_seed42.csv", "A"), load("synth_daily_b_seed7.csv", "B"));
    let rsi = json!({"kind": "indicator", "indicator": "rsi", "period": 14});
    let side = |enter: &str, level: f64, leave: &str, back: f64| {
        json!({
            "entry": {"logic": "all", "conditions": [{"left": rsi, "op": enter, "right": {"kind": "const", "value": level}}]},
            "exit": {"logic": "all", "conditions": [{"left": rsi, "op": leave, "right": {"kind": "const", "value": back}}]},
            "take_profit": {"kind": "pct", "value": 0.06},
        })
    };
    let s = settings(json!({
        "kind": "signals",
        "mode": "both",
        "long": side("crosses_below", 40.0, "crosses_above", 55.0),
        "short": side("crosses_above", 60.0, "crosses_below", 45.0),
        "sizing": {"mode": "fixed_qty", "qty": 10.0},
        "starting_capital": 20_000.0,
        "spread_pct": 0.001,
        "fees": {"amount_kind": "fixed", "per": "trade", "amount": 1.0},
    }));
    insta::assert_yaml_snapshot!(run_twice(&s, &[&a, &b]));
}

/// A two-asset savings plan: 1 000 up front, 200 a week, 60/40, a dip buy and a take-profit.
#[test]
fn dca_weekly_plan_on_synth_a_and_b() {
    let (a, b) = (load("synth_daily_a_seed42.csv", "A"), load("synth_daily_b_seed7.csv", "B"));
    let s = settings(json!({
        "kind": "dca",
        "mode": "long",
        "sizing": {"mode": "fixed_qty", "qty": 1.0},
        "starting_capital": 1_000.0,
        "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.1},
        "dca": {
            "weights": [{"ticker": "A", "weight": 60.0}, {"ticker": "B", "weight": 40.0}],
            "contribution": {"amount": 200.0, "period": "week", "every": 1, "invest": true},
            "buys": [{"amount_kind": "fixed", "amount": 300.0, "per_asset": false, "cooldown_bars": 10,
                      "condition": {"logic": "all", "conditions": [
                          {"left": {"kind": "metric", "metric": "dd_from_high", "period": 0}, "op": "above",
                           "right": {"kind": "const", "value": 5.0}}]}}],
            "sells": [{"amount_kind": "pct_position", "amount": 25.0, "per_asset": true, "target_gain_pct": 8.0,
                       "condition": {"logic": "all", "conditions": []}, "cooldown_bars": 15}],
        },
    }));
    insta::assert_yaml_snapshot!(run_twice(&s, &[&a, &b]));
}

/// Reference backtest timing (TESTING.md §4.14): the SMA crossover of the first snapshot over
/// 200 000 bars, the frozen fixture's moves replayed end to end. Local only, never a CI gate
/// (otw-core is a binary crate, so a `benches/` target cannot reach the engine):
/// `cargo test -p otw-core --release --bin otw-core backtest::snapshot_tests::bench -- --ignored --nocapture`.
#[test]
#[ignore = "timing: run with --release --ignored, see the doc comment"]
fn bench_reference_backtest_200k_bars() {
    let a = load("synth_daily_a_seed42.csv", "A");
    let n = 200_000;
    let mut long = OwnedBars { ticker: "A".into(), timeframe: "1h".into(), ts: Vec::with_capacity(n), open: Vec::new(), high: Vec::new(), low: Vec::new(), close: Vec::new(), volume: Vec::new(), quotes: None, sub: None, script: None };
    let mut level = 1.0;
    for i in 0..n {
        let k = i % a.close.len();
        if k == 0 && i > 0 {
            level *= a.close[a.close.len() - 1] / a.open[0];
        }
        let t = time::OffsetDateTime::from_unix_timestamp(1_704_067_200 + i as i64 * 3600).unwrap();
        long.ts.push(t.format(&time::format_description::well_known::Rfc3339).unwrap());
        long.open.push(a.open[k] * level);
        long.high.push(a.high[k] * level);
        long.low.push(a.low[k] * level);
        long.close.push(a.close[k] * level);
        long.volume.push(a.volume[k]);
    }
    let s = settings(json!({
        "kind": "signals",
        "mode": "long",
        "long": {
            "entry": {"logic": "all", "conditions": [{"left": sma(10), "op": "crosses_above", "right": sma(30)}]},
            "exit": {"logic": "all", "conditions": [{"left": sma(10), "op": "crosses_below", "right": sma(30)}]},
            "stop_loss": {"kind": "pct", "value": 0.05},
        },
        "sizing": {"mode": "percent_equity", "percent": 50.0},
        "starting_capital": 10_000.0,
        "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.1},
    }));
    let t = std::time::Instant::now();
    let r = run(&s, &long.as_bars());
    let ms = t.elapsed().as_millis();
    eprintln!("bench_reference_backtest_200k_bars: {ms} ms, {} trades", r.trades.len());
    assert!(r.trades.len() > 100, "{} trades", r.trades.len());
    assert!(r.stats.final_equity.is_finite());
}
