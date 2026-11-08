mod common;
use common::*;

use otw_script::diag::RtCode;

const NA: Option<f64> = None;

#[test]
fn highest_window() {
    let out = run_src(&v6("plot(ta.highest(close, 3))\nplot(ta.lowest(close, 3))"), bars_close(&[1.0, 5.0, 2.0, 3.0, 0.5]));
    assert_eq!(plot(&out, 0), vec![NA, NA, Some(5.0), Some(5.0), Some(3.0)]);
    assert_eq!(plot(&out, 1), vec![NA, NA, Some(1.0), Some(2.0), Some(0.5)]);
}

#[test]
fn crossover_equality_not_cross() {
    // a vs b = 2: a goes 1, 2 (equal), 3. Equality on the previous bar is not a cross.
    let out = run_src(&v6("plot(ta.crossover(close, 2) ? 1 : 0)\nplot(ta.crossunder(close, 2) ? 1 : 0)"), bars_close(&[1.0, 2.0, 3.0, 1.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[0.0, 0.0, 0.0, 0.0, 1.0]));
    assert_eq!(plot(&out, 1), some(&[0.0, 0.0, 0.0, 1.0, 0.0]));
}

#[test]
fn pivothigh_emitted_right_bars_late() {
    // Peak 9 at bar 2; left = right = 2: reported on bar 4 with the peak's value.
    let out = run_src(&v6("plot(ta.pivothigh(close, 2, 2))\nplot(ta.pivotlow(close, 1, 1))"), bars_close(&[1.0, 2.0, 9.0, 3.0, 2.0, 4.0]));
    assert_eq!(plot(&out, 0), vec![NA, NA, NA, NA, Some(9.0), NA]);
    assert_eq!(plot(&out, 1), vec![NA, NA, NA, NA, NA, Some(2.0)]);
}

#[test]
fn pivothigh_ties_not_pivot() {
    let out = run_src(&v6("plot(ta.pivothigh(close, 1, 1))"), bars_close(&[1.0, 5.0, 5.0, 1.0]));
    assert_eq!(plot(&out, 0), vec![NA; 4]);
}

#[test]
fn valuewhen_occurrences() {
    let body = "c = close > 2\nplot(ta.valuewhen(c, close, 0))\nplot(ta.valuewhen(c, bar_index, 1))";
    let out = run_src(&v6(body), bars_close(&[3.0, 1.0, 4.0, 1.0, 5.0]));
    assert_eq!(plot(&out, 0), some(&[3.0, 3.0, 4.0, 4.0, 5.0]));
    assert_eq!(plot(&out, 1), vec![NA, NA, Some(0.0), Some(0.0), Some(2.0)]);
}

#[test]
fn macd_tuple() {
    let out = run_src(&v6("[m, s, h] = ta.macd(close, 2, 3, 2)\nplot(m - s - h)"), synth(50, 1));
    assert!(plot(&out, 0).iter().flatten().all(|x| x.abs() < 1e-12));
}

#[test]
fn tr_first_bar() {
    let mut bars = bars_close(&[10.0, 11.0]);
    bars.high = vec![12.0, 13.0];
    bars.low = vec![9.0, 10.5];
    let out = run_src(&v6("plot(ta.tr)\nplot(ta.tr(true))"), bars);
    assert_eq!(plot(&out, 0), vec![NA, Some(3.0)]);
    assert_eq!(plot(&out, 1), some(&[3.0, 3.0]));
}

#[test]
fn rsi_extremes() {
    let out = run_src(&v6("plot(ta.rsi(close, 2))"), bars_close(&[1.0, 2.0, 3.0, 4.0]));
    assert_eq!(plot(&out, 0)[3], Some(100.0), "no losses: 100");
    let out = run_src(&v6("plot(ta.rsi(close, 2))"), bars_close(&[4.0, 3.0, 2.0, 1.0]));
    assert_eq!(plot(&out, 0)[3], Some(0.0), "no gains: 0");
}

#[test]
fn vwap_resets_daily_in_tz() {
    // 2024-01-01 22:00Z .. 2024-01-02 01:00Z; closes 1, 3, 10, 20; volume 1.
    let t = 1_704_146_400_000;
    let mut bars = bars_close(&[1.0, 3.0, 10.0, 20.0]);
    bars.time = (0..4).map(|i| t + i * 3_600_000).collect();
    let out = run_src(&v6("plot(ta.vwap(close))"), bars.clone());
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 10.0, 15.0]));
    // In New York the day turns at 05:00Z: no reset inside these bars.
    let mut i = input(bars);
    i.syminfo.timezone = Some("America/New_York".into());
    let out = run_with(&v6("plot(ta.vwap(close))"), i);
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 14.0 / 3.0, 8.5]));
}

#[test]
fn vwap_anchor_and_bands() {
    let out = run_src(&v6("[v, u, l] = ta.vwap(close, bar_index % 2 == 0, 1)\nplot(v)\nplot(u - v)"), bars_close(&[1.0, 3.0, 5.0, 9.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 5.0, 7.0]));
    assert_eq!(plot(&out, 1), some(&[0.0, 1.0, 0.0, 2.0]));
}

#[test]
fn vwap_unknown_tz_refused() {
    let mut i = input(bars_close(&[1.0]));
    i.syminfo.timezone = None;
    let err = compile(&v6("plot(ta.vwap(close))")).run(&i).expect_err("refused");
    assert_eq!((err.code, err.args.clone()), (RtCode::SyminfoUnknown, vec!["timezone".to_string()]));
}

#[test]
fn zero_volume_na() {
    let mut bars = bars_close(&[1.0, 2.0, 3.0]);
    bars.volume = vec![0.0; 3];
    let out = run_src(&v6("plot(ta.vwma(close, 2))\nplot(ta.vwap(close))"), bars);
    assert_eq!(plot(&out, 0), vec![NA; 3]);
    assert_eq!(plot(&out, 1), vec![NA; 3]);
}

#[test]
fn nan_bar_values_na() {
    let out = run_src(&v6("plot(ta.sma(close, 2))\nplot(ta.ema(close, 2))\nplot(ta.highest(close, 2))"), bars_close(&[1.0, f64::NAN, 3.0, 4.0, 5.0]));
    assert_eq!(plot(&out, 0), vec![NA, NA, NA, Some(3.5), Some(4.5)]);
    assert_eq!(plot(&out, 1)[1], NA);
    assert!(plot(&out, 1)[4].is_some(), "ema recovers after the gap");
    assert_eq!(plot(&out, 2), vec![NA, Some(1.0), Some(3.0), Some(4.0), Some(5.0)]);
}

#[test]
fn sma_call_site_state_and_short_series() {
    let out = run_src(&v6("f(x) => ta.sma(x, 2)\na = f(close)\nb = f(close * 10)\nplot(a)\nplot(b)"), bars_close(&[1.0, 3.0]));
    assert_eq!(plot(&out, 0), vec![NA, Some(2.0)]);
    assert_eq!(plot(&out, 1), vec![NA, Some(20.0)]);
    let out = run_src(&v6("x = bar_index % 2 == 0 ? ta.sma(close, 2) : na\nplot(x)"), bars_close(&[1.0, 2.0, 3.0, 4.0, 5.0]));
    assert_eq!(plot(&out, 0), vec![NA, NA, Some(2.0), NA, Some(4.0)], "the SMA of the even bars only");
    assert_eq!(plot(&run_src(&v6("plot(ta.sma(close, 200))"), bars_close(&[1.0; 3])), 0), vec![NA; 3]);
}

#[test]
fn median_odd_even_window() {
    let out = run_src(&v6("plot(ta.median(close, 3))\nplot(ta.median(close, 4))"), bars_close(&[5.0, 1.0, 3.0, 9.0, 2.0]));
    assert_eq!(plot(&out, 0), vec![NA, NA, Some(3.0), Some(3.0), Some(3.0)]);
    assert_eq!(plot(&out, 1), vec![NA, NA, NA, Some(4.0), Some(2.5)]);
}

#[test]
fn percentiles() {
    // Window 1..=5: nearest rank of 40% = 2nd value; linear 40% = 1 + 0.4 * 4 = 2.6.
    let body = "plot(ta.percentile_nearest_rank(close, 5, 40))\nplot(ta.percentile_linear_interpolation(close, 5, 40))\nplot(ta.percentile_nearest_rank(close, 5, 100))";
    let out = run_src(&v6(body), bars_close(&[3.0, 1.0, 5.0, 2.0, 4.0]));
    assert_eq!(plot(&out, 0)[4], Some(2.0));
    assert!((plot(&out, 1)[4].unwrap() - 2.6).abs() < 1e-12);
    assert_eq!(plot(&out, 2)[4], Some(5.0));
    assert_eq!(plot(&out, 0)[3], NA);
}

#[test]
fn obv_starts_at_zero() {
    let out = run_src(&v6("plot(ta.obv)"), bars_close(&[1.0, 2.0, 2.0, 1.0]));
    assert_eq!(plot(&out, 0), some(&[0.0, 1.0, 1.0, 0.0]));
}
