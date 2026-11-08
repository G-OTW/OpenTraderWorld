//! Properties and edge cases of the backtest indicator math (TESTING.md §4.2): bounded
//! oscillators stay in their range, an average of a constant is the constant, the output is
//! aligned to the input with `None` exactly over the warm-up, and nothing panics or produces a
//! NaN on finite input, including an empty series, one shorter than the window and window 1.
//!
//! Indicator values are `f64` by nature (not money); a relative 1e-12 absorbs the rolling-sum
//! noise of the running averages.

use super::*;
use proptest::prelude::*;

/// High, low, close of a random but well-formed market (low <= open, close <= high).
#[derive(Debug, Clone)]
struct Hlc {
    h: Vec<f64>,
    l: Vec<f64>,
    c: Vec<f64>,
    v: Vec<f64>,
}

fn hlc(max: usize) -> impl Strategy<Value = Hlc> {
    (1_000i64..1_000_000, prop::collection::vec((-500i32..=500, 0i32..=200, 0i32..=200, 1u32..10_000), 0..max)).prop_map(|(p0, rows)| {
        let mut m = Hlc { h: Vec::new(), l: Vec::new(), c: Vec::new(), v: Vec::new() };
        let mut px = p0 as f64 / 100.0;
        for (mv, up, down, vol) in rows {
            let o = px;
            px *= 1.0 + mv as f64 / 10_000.0;
            m.h.push(o.max(px) * (1.0 + up as f64 / 10_000.0));
            m.l.push(o.min(px) * (1.0 - down as f64 / 10_000.0));
            m.c.push(px);
            m.v.push(vol as f64);
        }
        m
    })
}

fn rel(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * b.abs().max(1.0)
}

/// Aligned to the input, `None` before the first full window, defined (and finite) after it.
fn aligned(name: &str, out: &[Option<f64>], len: usize, first: usize) -> Result<(), TestCaseError> {
    prop_assert_eq!(out.len(), len, "{}: output length", name);
    for (i, v) in out.iter().enumerate() {
        if i < first {
            prop_assert!(v.is_none(), "{name}[{i}] = {v:?} inside the warm-up of {first}");
        } else {
            prop_assert!(v.is_some_and(f64::is_finite), "{name}[{i}] = {v:?} after the warm-up of {first}");
        }
    }
    Ok(())
}

fn in_range(name: &str, out: &[Option<f64>], lo: f64, hi: f64) -> Result<(), TestCaseError> {
    for v in out.iter().flatten() {
        prop_assert!(v.is_finite() && *v >= lo - 1e-9 && *v <= hi + 1e-9, "{name}: {v} outside [{lo}, {hi}]");
    }
    Ok(())
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
    #![proptest_config(config(256))]

    /// The warm-up of every averaging indicator is exactly its window, whatever the length of
    /// the series (empty and shorter than the window included).
    #[test]
    fn output_is_aligned_with_none_over_the_warmup(m in hlc(120), n in 1usize..40) {
        let len = m.c.len();
        aligned("sma", &sma(&m.c, n), len, n - 1)?;
        // Seeded with the first close, as `ta.ema`: defined from bar 0.
        aligned("ema", &ema(&m.c, n), len, 0)?;
        aligned("wma", &wma(&m.c, n), len, n - 1)?;
        aligned("stddev", &stddev(&m.c, n), len, n - 1)?;
        aligned("rsi", &rsi(&m.c, n), len, n)?;
        // Bar 0's range is high - low, as `ta.atr`: the first average lands on bar n - 1.
        aligned("atr", &atr(&m.h, &m.l, &m.c, n), len, n - 1)?;
        aligned("willr", &willr(&m.h, &m.l, &m.c, n), len, n - 1)?;
        aligned("cci", &cci(&m.h, &m.l, &m.c, n), len, n - 1)?;
        aligned("donchian", &donchian(&m.h, &m.l, n, 1), len, n - 1)?;
    }

    /// Bounded oscillators stay in their range: RSI, stochastic and MFI in [0, 100], Williams
    /// %R in [-100, 0]. Dispersions are never negative.
    #[test]
    fn oscillators_stay_in_their_range(m in hlc(200), n in 1usize..30, smooth in 1usize..5) {
        in_range("rsi", &rsi(&m.c, n), 0.0, 100.0)?;
        in_range("stoch_k", &stoch_k(&m.h, &m.l, &m.c, n, smooth), 0.0, 100.0)?;
        in_range("stoch_d", &stoch_d(&m.h, &m.l, &m.c, n, smooth), 0.0, 100.0)?;
        in_range("mfi", &mfi(&m.h, &m.l, &m.c, &m.v, n), 0.0, 100.0)?;
        in_range("willr", &willr(&m.h, &m.l, &m.c, n), -100.0, 0.0)?;
        in_range("atr", &atr(&m.h, &m.l, &m.c, n), 0.0, f64::MAX)?;
        in_range("stddev", &stddev(&m.c, n), 0.0, f64::MAX)?;
    }

    /// The bands are ordered: Bollinger and Donchian upper >= mid >= lower, Keltner upper >= lower.
    #[test]
    fn bands_are_ordered(m in hlc(150), n in 1usize..30, mult in 1u32..40) {
        let mult = mult as f64 / 10.0;
        let (bu, bm, bl) = (bollinger(&m.c, n, mult, 1), bollinger(&m.c, n, mult, 0), bollinger(&m.c, n, mult, -1));
        let (du, dm, dl) = (donchian(&m.h, &m.l, n, 1), donchian(&m.h, &m.l, n, 0), donchian(&m.h, &m.l, n, -1));
        let (ku, kl) = (keltner(&m.h, &m.l, &m.c, n, mult, 1), keltner(&m.h, &m.l, &m.c, n, mult, -1));
        for i in 0..m.c.len() {
            if let (Some(u), Some(mid), Some(l)) = (bu[i], bm[i], bl[i]) {
                prop_assert!(u >= mid - 1e-9 && mid >= l - 1e-9, "bollinger[{i}] {u} {mid} {l}");
            }
            if let (Some(u), Some(mid), Some(l)) = (du[i], dm[i], dl[i]) {
                prop_assert!(u >= mid && mid >= l, "donchian[{i}] {u} {mid} {l}");
            }
            if let (Some(u), Some(l)) = (ku[i], kl[i]) {
                prop_assert!(u >= l - 1e-9, "keltner[{i}] {u} {l}");
            }
        }
    }

    /// An average of a constant series is that constant, and its dispersion is zero.
    #[test]
    fn averages_of_a_constant_are_the_constant(c in 1i64..10_000_000, len in 0usize..80, n in 1usize..30) {
        let x = c as f64 / 100.0;
        let s = vec![x; len];
        for (name, out) in [("sma", sma(&s, n)), ("ema", ema(&s, n)), ("wma", wma(&s, n)), ("dema", dema(&s, n)), ("tema", tema(&s, n)), ("hma", hma(&s, n))] {
            prop_assert_eq!(out.len(), len);
            for v in out.iter().flatten() {
                prop_assert!(rel(*v, x), "{name} of constant {x} = {v}");
            }
        }
        for v in stddev(&s, n).iter().flatten() {
            prop_assert!(v.abs() <= 1e-9 * x, "stddev of a constant = {v}");
        }
    }

    /// Window 1 is the series itself for the averages, and no indicator panics on it.
    #[test]
    fn window_one_is_the_series(m in hlc(60)) {
        for (name, out) in [("sma", sma(&m.c, 1)), ("ema", ema(&m.c, 1)), ("wma", wma(&m.c, 1))] {
            for (v, c) in out.iter().zip(&m.c) {
                prop_assert!(v.is_some_and(|v| rel(v, *c)), "{name}(1) = {v:?} vs {c}");
            }
        }
        let _ = (rsi(&m.c, 1), atr(&m.h, &m.l, &m.c, 1), stoch_k(&m.h, &m.l, &m.c, 1, 1), cci(&m.h, &m.l, &m.c, 1), mfi(&m.h, &m.l, &m.c, &m.v, 1));
        in_range("rsi(1)", &rsi(&m.c, 1), 0.0, 100.0)?;
    }
}

/// A series that never moves has no gain and no loss: no value, never a NaN from 0 / 0, as the
/// script engine's `ta.rsi` (reference otw-script/tests/fixtures/ta_ref/rsi.pine gives 100 only
/// with gains). Gains alone still read 100.
#[test]
fn rsi_of_a_constant_series_has_no_value_like_ta_rsi() {
    let out = rsi(&[50.0; 30], 14);
    assert!(out.iter().all(Option::is_none), "{out:?}");
    let rising: Vec<f64> = (0..30).map(|i| 50.0 + i as f64).collect();
    assert!(rsi(&rising, 14)[14..].iter().all(|v| *v == Some(100.0)));
}

/// A zero range (high == low over the window) has no position inside it: the stochastic
/// reads the middle, Williams %R its middle, CCI zero, never a division by zero.
#[test]
fn a_zero_range_reads_the_middle_not_nan() {
    let flat = [10.0; 20];
    assert!(stoch_k(&flat, &flat, &flat, 5, 1)[4..].iter().all(|v| *v == Some(50.0)));
    assert!(willr(&flat, &flat, &flat, 5)[4..].iter().all(|v| *v == Some(-50.0)));
    assert!(cci(&flat, &flat, &flat, 5)[4..].iter().all(|v| *v == Some(0.0)));
    assert!(atr(&flat, &flat, &flat, 5)[5..].iter().all(|v| *v == Some(0.0)));
}

/// Empty input, a zero window and a series shorter than the window give an aligned output
/// with no value, never a panic.
#[test]
fn empty_short_and_zero_window_give_no_value() {
    let e: [f64; 0] = [];
    let short = [1.0, 2.0, 3.0];
    for n in [0usize, 5] {
        for s in [&e[..], &short[..]] {
            for out in [sma(s, n), wma(s, n), rsi(s, n), stddev(s, n), atr(s, s, s, n), stoch_k(s, s, s, n, 1), willr(s, s, s, n), cci(s, s, s, n), mfi(s, s, s, s, n)] {
                assert_eq!(out.len(), s.len());
                assert!(out.iter().all(Option::is_none), "window {n} over {} bars: {out:?}", s.len());
            }
        }
    }
    // The EMA is seeded with the first close (`ta.ema`): no value only with no bar or no window.
    assert!(ema(&e, 5).is_empty());
    assert!(ema(&short, 0).iter().all(Option::is_none));
    assert_eq!(ema(&short, 5)[0], Some(1.0));
}

/// Known values, by hand. SMA(3) of 1..=5 is 2, 3, 4. RSI(2) of 1, 2, 1, 2: the first average
/// gain and loss are 0.5 and 0.5 (RS 1, RSI 50); the next bar gains 1, so Wilder's smoothing
/// gives gain (0.5 + 1) / 2 = 0.75 and loss 0.5 / 2 = 0.25, RS 3, RSI 75.
#[test]
fn known_values() {
    assert_eq!(sma(&[1.0, 2.0, 3.0, 4.0, 5.0], 3), vec![None, None, Some(2.0), Some(3.0), Some(4.0)]);
    assert_eq!(rsi(&[1.0, 2.0, 1.0, 2.0], 2), vec![None, None, Some(50.0), Some(75.0)]);
}
