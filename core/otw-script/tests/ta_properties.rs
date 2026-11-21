//! Properties and edge cases of the script engine's `ta.*` builtins (TESTING.md §4.2), next to
//! `ta_reference.rs` (which checks each builtin bar for bar against a Pine reference
//! implementation on fixed series). Here the series are random: bounded oscillators stay in
//! their range, averages of a constant are the constant, outputs are aligned to the bars with
//! `na` exactly over the warm-up, and nothing errors on an empty series, one shorter than the
//! window, or window 1.
//!
//! Indicator values are `f64` by nature (not money); a relative 1e-12 absorbs the noise of the
//! running sums.

mod common;
use common::*;

use otw_script::input::Bars;
use proptest::prelude::*;

/// Well-formed OHLCV (low <= open, close <= high) from a start price and per-bar moves.
fn market(min: usize, max: usize) -> impl Strategy<Value = Bars> {
    (1_000i64..1_000_000, prop::collection::vec((-500i32..=500, 0i32..=200, 0i32..=200, 1u32..10_000), min..max)).prop_map(|(p0, rows)| {
        let mut b = Bars::default();
        let mut px = p0 as f64 / 100.0;
        for (i, (mv, up, down, vol)) in rows.into_iter().enumerate() {
            let o = px;
            px *= 1.0 + mv as f64 / 10_000.0;
            b.time.push(i as i64 * 3_600_000);
            b.time_close.push((i as i64 + 1) * 3_600_000);
            b.open.push(o);
            b.high.push(o.max(px) * (1.0 + up as f64 / 10_000.0));
            b.low.push(o.min(px) * (1.0 - down as f64 / 10_000.0));
            b.close.push(px);
            b.volume.push(vol as f64);
        }
        b
    })
}

fn run(body: &str, bars: Bars) -> Vec<Vec<Option<f64>>> {
    let out = run_src(&v6(body), bars);
    out.plots.iter().map(|p| p.values.clone()).collect()
}

fn rel(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * b.abs().max(1.0)
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

    /// One value per bar, `na` over the warm-up, a finite value on every bar after it.
    #[test]
    fn outputs_are_aligned_with_na_over_the_warmup(b in market(0, 120), n in 1usize..40) {
        let len = b.close.len();
        let body = format!("plot(ta.sma(close, {n}))\nplot(ta.ema(close, {n}))\nplot(ta.wma(close, {n}))\nplot(ta.highest(high, {n}))\nplot(ta.rsi(close, {n}))\nplot(ta.atr({n}))");
        let plots = run(&body, b);
        // Warm-ups of the Pine reference implementations (tests/fixtures/ta_ref): sma, wma and
        // highest need `n` bars (first value on bar n - 1); ema is seeded with the first source
        // value (defined from bar 0); atr is the rma of the true range, whose first bar is
        // high - low (bar n - 1); rsi needs one change before its rma (bar n).
        for (k, first) in [n - 1, 0, n - 1, n - 1, n, n - 1].into_iter().enumerate() {
            let p = &plots[k];
            prop_assert_eq!(p.len(), len, "plot {} length", k);
            for (i, v) in p.iter().enumerate() {
                if i < first {
                    prop_assert!(v.is_none(), "plot {k}[{i}] = {v:?} inside the warm-up {first}");
                } else {
                    prop_assert!(v.is_some_and(f64::is_finite), "plot {k}[{i}] = {v:?} after the warm-up {first}");
                }
            }
        }
    }

    /// RSI, stochastic and MFI stay in [0, 100], Williams-style %R from `ta.wpr` in [-100, 0];
    /// standard deviation and ATR are never negative.
    #[test]
    fn oscillators_stay_in_their_range(b in market(1, 200), n in 1usize..30) {
        let body = format!("plot(ta.rsi(close, {n}))\nplot(ta.stoch(close, high, low, {n}))\nplot(ta.mfi(hlc3, {n}))\nplot(ta.wpr({n}))\nplot(ta.stdev(close, {n}))\nplot(ta.atr({n}))");
        let plots = run(&body, b);
        let ranges = [(0.0, 100.0), (0.0, 100.0), (0.0, 100.0), (-100.0, 0.0), (0.0, f64::MAX), (0.0, f64::MAX)];
        for (k, (lo, hi)) in ranges.into_iter().enumerate() {
            for v in plots[k].iter().flatten() {
                prop_assert!(v.is_finite() && *v >= lo - 1e-9 && *v <= hi + 1e-9, "plot {k}: {v} outside [{lo}, {hi}]");
            }
        }
    }

    /// An average of a constant series is the constant; its deviation is zero.
    #[test]
    fn averages_of_a_constant_are_the_constant(c in 1i64..10_000_000, len in 0usize..80, n in 1usize..30) {
        let x = c as f64 / 100.0;
        let body = format!("plot(ta.sma(close, {n}))\nplot(ta.ema(close, {n}))\nplot(ta.rma(close, {n}))\nplot(ta.wma(close, {n}))\nplot(ta.vwma(close, {n}))\nplot(ta.stdev(close, {n}))");
        let plots = run(&body, bars_close(&vec![x; len]));
        for (k, p) in plots[..5].iter().enumerate() {
            prop_assert_eq!(p.len(), len);
            for v in p.iter().flatten() {
                prop_assert!(rel(*v, x), "plot {k} of constant {x} = {v}");
            }
        }
        for v in plots[5].iter().flatten() {
            prop_assert!(v.abs() <= 1e-9 * x, "stdev of a constant = {v}");
        }
    }

    /// Window 1: the averages are the series itself.
    #[test]
    fn window_one_is_the_series(b in market(1, 60)) {
        let close = b.close.clone();
        let plots = run("plot(ta.sma(close, 1))\nplot(ta.ema(close, 1))\nplot(ta.wma(close, 1))\nplot(ta.highest(close, 1))", b);
        for (k, p) in plots.iter().enumerate() {
            for (v, c) in p.iter().zip(&close) {
                prop_assert!(v.is_some_and(|v| rel(v, *c)), "plot {k}: {v:?} vs {c}");
            }
        }
    }
}

/// A series that never moves has no gain and no loss. The engine follows its documented
/// reference (tests/fixtures/ta_ref/rsi.pine: 100 only when there are gains and no loss), so
/// 0 / 0 is `na`, never a NaN value plotted. The backtest engine's own `rsi` returns 100 here:
/// the two conventions differ on this one case.
#[test]
fn rsi_of_a_constant_series_is_na_not_nan() {
    let p = run("plot(ta.rsi(close, 14))", bars_close(&[50.0; 30]));
    assert!(p[0].iter().all(Option::is_none), "{:?}", p[0]);
    // A rising series with no loss is the documented edge: 100.
    let up = run("plot(ta.rsi(close, 14))", bars_close(&(1..=30).map(f64::from).collect::<Vec<_>>()));
    assert!(up[0][14..].iter().all(|v| *v == Some(100.0)), "{:?}", up[0]);
}

/// No bar at all, and fewer bars than the window: the script runs, every plot is aligned to
/// the bars and the windowed builtins hold no value (`ta.ema`, seeded on its first bar like
/// Pine's, is the exception and is checked by the alignment property).
#[test]
fn empty_and_short_series_run_and_hold_no_value() {
    let body = "plot(ta.sma(close, 5))\nplot(ta.rma(close, 5))\nplot(ta.rsi(close, 5))\nplot(ta.atr(5))\nplot(ta.stdev(close, 5))\nplot(ta.highest(high, 5))";
    for len in [0usize, 1, 3, 4] {
        let plots = run(body, bars_close(&(1..=len).map(|i| i as f64).collect::<Vec<_>>()));
        for (k, p) in plots.iter().enumerate() {
            assert_eq!(p.len(), len, "plot {k}");
            assert!(p.iter().all(Option::is_none), "{len} bars, plot {k}: {p:?}");
        }
    }
}
