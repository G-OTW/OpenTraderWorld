//! Each `tests/fixtures/ta_ref/<name>.pine` plots a reference implementation then the native
//! builtin (pairs of plots). They must agree bar for bar, exactly.
mod common;
use common::*;

fn check(name: &str) {
    let src = std::fs::read_to_string(format!("{}/tests/fixtures/ta_ref/{name}.pine", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut series = vec![synth(2000, 42), synth(300, 7), bars_close(&[5.0; 40]), bars_close(&(1..=60).map(f64::from).collect::<Vec<_>>())];
    // A gap: three bars of NaN in the middle.
    let mut gap = synth(200, 3);
    for i in 100..103 {
        gap.close[i] = f64::NAN;
    }
    series.push(gap);
    for (k, bars) in series.into_iter().enumerate() {
        let out = run_src(&src, bars);
        for pair in out.plots.chunks(2) {
            assert_eq!(pair[0].values, pair[1].values, "{name}: series {k}, plots {} vs {}", pair[0].index, pair[1].index);
            // The long random series exercises every builtin: an all-na pair compares nothing.
            assert!(k != 0 || pair[1].values.iter().any(Option::is_some), "{name}: plot {} is all na", pair[1].index);
        }
    }
}

#[test]
fn sma() { check("sma") }
#[test]
fn ema() { check("ema") }
#[test]
fn rma() { check("rma") }
#[test]
fn wma() { check("wma") }
#[test]
fn hma() { check("hma") }
#[test]
fn vwma() { check("vwma") }
#[test]
fn stdev() { check("stdev") }
#[test]
fn atr() { check("atr") }
#[test]
fn rsi() { check("rsi") }
#[test]
fn change() { check("change") }
#[test]
fn macd() { check("macd") }
#[test]
fn highest_lowest() { check("highest") }

// ── Phase 4: ta breadth ──
#[test]
fn cum() { check("cum") }
#[test]
fn barssince() { check("barssince") }
#[test]
fn linreg() { check("linreg") }
#[test]
fn stoch() { check("stoch") }
#[test]
fn cross() { check("cross") }
#[test]
fn supertrend() { check("supertrend") }
#[test]
fn highestbars_lowestbars() { check("highestbars") }
#[test]
fn percentrank() { check("percentrank") }
#[test]
fn roc_mom() { check("roc") }
#[test]
fn sar() { check("sar") }
#[test]
fn dev_cci() { check("dev") }
#[test]
fn correlation() { check("correlation") }
#[test]
fn rising_falling() { check("rising") }
#[test]
fn mfi_obv() { check("mfi") }
#[test]
fn bb_variance_range_wpr() { check("bb") }
#[test]
fn dmi() { check("dmi") }
#[test]
fn tsi() { check("tsi") }
