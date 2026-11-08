mod common;
use common::*;

use otw_script::diag::RtCode;
use otw_script::input::{Bars, InputValue, TfUnit, Timeframe};
use otw_script::output::InputKind;

const NA: Option<f64> = None;

/// Bars at given epoch-second times, close = 1.
fn bars_at(times_s: &[i64]) -> Bars {
    let mut b = bars_close(&vec![1.0; times_s.len()]);
    b.time = times_s.iter().map(|t| t * 1000).collect();
    b.time_close = times_s.iter().map(|t| (t + 3600) * 1000).collect();
    b
}

#[test]
fn math_round_half_away() {
    let out = run_src(&v6("plot(math.round(2.5))\nplot(math.round(-2.5))\nplot(math.round(1.2345, 2))"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0), plot(&out, 1), plot(&out, 2)), (some(&[3.0]), some(&[-3.0]), some(&[1.23])));
}

#[test]
fn math_sum_rolling() {
    assert_eq!(plot(&run_src(&v6("plot(math.sum(close, 2))"), bars_close(&[1.0, 2.0, 3.0])), 0), vec![NA, Some(3.0), Some(5.0)]);
}

#[test]
fn math_misc() {
    let body = "plot(math.max(1, 5, 3))\nplot(math.avg(1, 2, 3))\nplot(math.pow(2, 10))\nplot(math.sqrt(-1))\nplot(math.sign(-3))\nplot(math.abs(-2))\nplot(math.round_to_mintick(1.234))\nplot(math.max(1, na))";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    let got: Vec<Option<f64>> = (0..8).map(|i| plot(&out, i)[0]).collect();
    assert_eq!(got, vec![Some(5.0), Some(2.0), Some(1024.0), NA, Some(-1.0), Some(2.0), Some(1.23), NA]);
}

#[test]
fn color_new_alpha() {
    let out = run_src(&v6("c = color.new(color.red, 50)\nplot(color.r(c))\nplot(color.g(c))\nplot(color.t(c))"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0), plot(&out, 1), plot(&out, 2)), (some(&[255.0]), some(&[82.0]), some(&[50.0])));
}

#[test]
fn from_gradient_mid() {
    let out = run_src(&v6("c = color.from_gradient(50, 0, 100, color.rgb(0, 0, 0), color.rgb(200, 100, 50))\nplot(color.r(c))\nplot(color.g(c))\nplot(color.b(c))"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0), plot(&out, 1), plot(&out, 2)), (some(&[100.0]), some(&[50.0]), some(&[25.0])));
}

#[test]
fn input_default_and_override() {
    let src = v6("len = input.int(3, \"Len\", minval = 1)\nplot(len)");
    let p = compile(&src);
    let m = &p.meta().inputs[0];
    assert_eq!((m.key.as_str(), m.title.as_str(), m.kind, m.default.clone(), m.minval), ("len", "Len", InputKind::Int, InputValue::Int(3), Some(1.0)));
    assert_eq!(plot(&run_src(&src, bars_close(&[1.0])), 0), some(&[3.0]));
    let out = run_inputs(&src, bars_close(&[1.0]), &[("len", InputValue::Int(5))]).unwrap();
    assert_eq!(plot(&out, 0), some(&[5.0]));
}

#[test]
fn input_out_of_range_refused() {
    let err = run_inputs(&v6("n = input.int(5, minval = 1, maxval = 10)\nplot(n)"), bars_close(&[1.0]), &[("n", InputValue::Int(11))]).expect_err("refused");
    assert_eq!((err.code, err.args[0].as_str()), (RtCode::InputOutOfRange, "n"));
    let err = run_inputs(&v6("n = input.int(5)\nplot(n)"), bars_close(&[1.0]), &[("n", InputValue::Str("x".into()))]).expect_err("refused");
    assert_eq!(err.code, RtCode::InputType);
}

#[test]
fn input_options_refused() {
    let src = v6("m = input.string(\"A\", options = [\"A\", \"B\"])\nplot(m == \"B\" ? 1 : 0)");
    assert_eq!(compile(&src).meta().inputs[0].options, vec!["A".to_string(), "B".to_string()]);
    assert_eq!(plot(&run_inputs(&src, bars_close(&[1.0]), &[("m", InputValue::Str("B".into()))]).unwrap(), 0), some(&[1.0]));
    let err = run_inputs(&src, bars_close(&[1.0]), &[("m", InputValue::Str("C".into()))]).expect_err("refused");
    assert_eq!(err.code, RtCode::InputOutOfRange);
}

#[test]
fn input_source_close_vs_hl2() {
    let mut bars = bars_close(&[10.0]);
    bars.high = vec![12.0];
    bars.low = vec![6.0];
    let src = v6("s = input.source(close, \"Source\")\nplot(s)");
    assert_eq!(compile(&src).meta().inputs[0].kind, InputKind::Source);
    assert_eq!(plot(&run_src(&src, bars.clone()), 0), some(&[10.0]));
    assert_eq!(plot(&run_inputs(&src, bars, &[("s", InputValue::Source("hl2".into()))]).unwrap(), 0), some(&[9.0]));
}

#[test]
fn barstate_flags() {
    let out = run_src(&v6("plot(barstate.isfirst ? 1 : 0)\nplot(barstate.islast ? 1 : 0)\nplot(barstate.ishistory and barstate.isconfirmed and not barstate.isrealtime ? 1 : 0)\nplot(last_bar_index)"), bars_close(&[1.0; 3]));
    assert_eq!(plot(&out, 0), some(&[1.0, 0.0, 0.0]));
    assert_eq!(plot(&out, 1), some(&[0.0, 0.0, 1.0]));
    assert_eq!(plot(&out, 2), some(&[1.0, 1.0, 1.0]));
    assert_eq!(plot(&out, 3), some(&[2.0, 2.0, 2.0]));
}

#[test]
fn time_in_exchange_tz() {
    // 2024-03-31 00:30Z is 01:30 CET; 01:30Z is 03:30 CEST (DST starts at 01:00Z).
    let t0 = 1_711_845_000;
    let mut i = input(bars_at(&[t0, t0 + 3600]));
    i.syminfo.timezone = Some("Europe/Paris".into());
    let out = run_with(&v6("plot(hour)\nplot(hour(time, \"UTC\"))\nplot(dayofweek)\nplot(year)"), i);
    assert_eq!(plot(&out, 0), some(&[1.0, 3.0]));
    assert_eq!(plot(&out, 1), some(&[0.0, 1.0]));
    assert_eq!(plot(&out, 2), some(&[1.0, 1.0]), "Sunday = 1");
    assert_eq!(plot(&out, 3), some(&[2024.0, 2024.0]));
}

#[test]
fn time_session_filter() {
    // 2024-01-02 09:00Z and 10:00Z, session 0930-1600 in UTC.
    let t = 1_704_186_000;
    let out = run_src(&v6("plot(na(time(\"60\", \"0930-1600\")) ? 0 : 1)"), bars_at(&[t, t + 3600]));
    assert_eq!(plot(&out, 0), some(&[0.0, 1.0]));
}

#[test]
fn timeframe_period_strings() {
    let cases = [(1, TfUnit::Minute, "1"), (60, TfUnit::Minute, "60"), (1, TfUnit::Day, "D"), (2, TfUnit::Day, "2D"), (1, TfUnit::Week, "W"), (3, TfUnit::Month, "3M"), (30, TfUnit::Second, "30S")];
    for (m, unit, want) in cases {
        let mut i = input(bars_close(&[1.0]));
        i.timeframe = Timeframe { multiplier: m, unit };
        let out = run_with(&v6(&format!("plot(timeframe.period == \"{want}\" ? 1 : 0)")), i);
        assert_eq!(plot(&out, 0), some(&[1.0]), "{want}");
    }
    let out = run_src(&v6("plot(timeframe.isintraday ? 1 : 0)\nplot(timeframe.in_seconds())\nplot(timeframe.in_seconds(\"D\"))"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0), plot(&out, 1), plot(&out, 2)), (some(&[1.0]), some(&[3600.0]), some(&[86400.0])));
}

#[test]
fn timeframe_change_daily() {
    // 2024-01-01 22:00Z, 23:00Z, 2024-01-02 00:00Z, 01:00Z.
    let t = 1_704_146_400;
    let out = run_src(&v6("plot(timeframe.change(\"D\") ? 1 : 0)"), bars_at(&[t, t + 3600, t + 7200, t + 10800]));
    assert_eq!(plot(&out, 0), some(&[1.0, 0.0, 1.0, 0.0]));
}

#[test]
fn syminfo_unknown_refused() {
    let mut i = input(bars_close(&[1.0]));
    i.syminfo.mintick = None;
    let err = compile(&v6("plot(syminfo.mintick)")).run(&i).expect_err("refused");
    assert_eq!((err.code, err.args.clone()), (RtCode::SyminfoUnknown, vec!["mintick".to_string()]));
    let out = run_src(&v6("plot(syminfo.pointvalue)\nplot(syminfo.prefix == \"X\" ? 1 : 0)"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0), plot(&out, 1)), (some(&[1.0]), some(&[1.0])));
}

#[test]
fn nz_fixnan() {
    let out = run_src(&v6("x = bar_index == 1 ? na : close\nplot(nz(x, 5))\nplot(fixnan(x))"), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 5.0, 3.0]));
    assert_eq!(plot(&out, 1), some(&[1.0, 1.0, 3.0]));
}

#[test]
fn casts_truncate() {
    let out = run_src(&v6("plot(int(-2.7))\nplot(float(3))\nplot(bool(2) ? 1 : 0)"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0), plot(&out, 1), plot(&out, 2)), (some(&[-2.0]), some(&[3.0]), some(&[1.0])));
}

#[test]
fn runtime_error_call() {
    let err = compile(&v6("if bar_index == 1\n    runtime.error(\"bad\")\nplot(close)")).run(&input(bars_close(&[1.0; 3]))).expect_err("error");
    assert_eq!((err.code, err.args.clone(), err.bar), (RtCode::RuntimeErrorCall, vec!["bad".to_string()], 1));
}

#[test]
fn short_series_all_na() {
    let src = v6("plot(math.sum(close, 200))");
    assert_eq!(plot(&run_src(&src, bars_close(&[1.0; 3])), 0), vec![NA; 3]);
    assert_eq!(plot(&run_src(&src, bars_close(&[1.0])), 0), vec![NA]);
    assert_eq!(plot(&run_src(&src, bars_close(&[])), 0), Vec::<Option<f64>>::new());
}

// ── Outputs (Task 12) ──

use otw_script::output::{FillRef, Rgba};

const RED: Rgba = Rgba { r: 0xFF, g: 0x52, b: 0x52, a: 255 };
const GREEN: Rgba = Rgba { r: 0x4C, g: 0xAF, b: 0x50, a: 255 };

#[test]
fn plot_static_and_dynamic_color() {
    let out = run_src(&v6("plot(close, color = color.red, linewidth = 2)\nplot(close, color = close > 1 ? color.green : na)"), bars_close(&[1.0, 2.0]));
    assert_eq!(out.plots[0].colors, vec![Some(RED), Some(RED)]);
    assert_eq!(out.plots[0].linewidth, 2);
    assert_eq!(out.plots[1].colors, vec![None, Some(GREEN)]);
}

#[test]
fn plot_offset_kept_raw() {
    let out = run_src(&v6("plot(close, offset = 2, histbase = 1.5)"), bars_close(&[1.0, 2.0]));
    assert_eq!((plot(&out, 0), out.plots[0].offset, out.plots[0].histbase), (some(&[1.0, 2.0]), 2, 1.5));
}

#[test]
fn display_none_hidden() {
    let out = run_src(&v6("plot(close, display = display.none)\nplot(close)"), bars_close(&[1.0]));
    assert!(out.plots[0].hidden);
    assert!(!out.plots[1].hidden);
    assert_eq!(plot(&out, 0), some(&[1.0]));
}

#[test]
fn hline_and_fill() {
    let out = run_src(&v6("a = hline(70, \"Up\", color = color.red, linestyle = hline.style_dashed)\nb = hline(30)\nfill(a, b, color = color.new(color.red, 90))"), bars_close(&[1.0, 2.0]));
    assert_eq!(out.hlines.len(), 2);
    assert_eq!((out.hlines[0].price, out.hlines[0].title.as_str(), out.hlines[0].style.as_str(), out.hlines[0].color), (70.0, "Up", "style_dashed", Some(RED)));
    assert_eq!((out.fills[0].a, out.fills[0].b), (FillRef::HLine(0), FillRef::HLine(1)));
    assert_eq!(out.fills[0].colors[1].map(|c| c.a), Some(26));
}

#[test]
fn fill_between_plots() {
    let out = run_src(&v6("p1 = plot(high)\np2 = plot(low)\nfill(p1, p2, color = close > 1 ? color.green : color.red)"), bars_close(&[1.0, 2.0]));
    assert_eq!((out.fills[0].a, out.fills[0].b), (FillRef::Plot(0), FillRef::Plot(1)));
    assert_eq!(out.fills[0].colors, vec![Some(RED), Some(GREEN)]);
}

#[test]
fn bgcolor_na_transparent() {
    let out = run_src(&v6("bgcolor(close > 1 ? color.green : na)\nbarcolor(color.red)\nplot(close)"), bars_close(&[1.0, 2.0]));
    assert_eq!(out.bgcolor[0].colors, vec![None, Some(GREEN)]);
    assert_eq!(out.barcolor[0].colors, vec![Some(RED), Some(RED)]);
}

#[test]
fn plotshape_when_series() {
    let out = run_src(&v6("plotshape(close > 1, \"Up\", shape.triangleup, location.belowbar, color.green, text = \"B\")\nplotchar(close > 1, char = \"x\")\nplotarrow(close - 1.5)"), bars_close(&[1.0, 2.0]));
    let s = &out.shapes[0];
    assert_eq!((s.kind.as_str(), s.style.as_str(), s.location.as_str(), s.text.as_str()), ("plotshape", "triangleup", "belowbar", "B"));
    assert_eq!(s.when, vec![Some(0.0), Some(1.0)]);
    assert_eq!(out.shapes[1].style, "x");
    assert_eq!(out.shapes[2].when, vec![Some(-0.5), Some(0.5)]);
}

#[test]
fn plotcandle_series() {
    let out = run_src(&v6("plotcandle(open, high, low, close * 2, color = color.green)\nplotbar(open, high, low, close)"), bars_close(&[1.0, 2.0]));
    assert_eq!((out.candles[0].kind.as_str(), out.candles[0].c.clone()), ("plotcandle", some(&[2.0, 4.0])));
    assert_eq!(out.candles[0].colors, vec![Some(GREEN), Some(GREEN)]);
    assert_eq!(out.candles[1].kind, "plotbar");
}

#[test]
fn plot_in_local_scope_refused() {
    let e = otw_script::compile(&v6("if close > 1\n    plot(close)")).expect_err("refused");
    assert!(e.iter().any(|d| d.code == otw_script::diag::DiagCode::Syntax), "{e:?}");
    let e = otw_script::compile(&v6("f() =>\n    plot(close)\nf()")).expect_err("refused");
    assert!(e.iter().any(|d| d.code == otw_script::diag::DiagCode::Syntax), "{e:?}");
}

#[test]
fn indicator_meta() {
    let p = compile("//@version=6\nindicator(\"My RSI\", shorttitle = \"RSI\", overlay = true, precision = 2)\nplot(close)");
    let m = p.meta();
    assert_eq!((m.title.as_str(), m.shorttitle.as_deref(), m.overlay, m.precision), ("My RSI", Some("RSI"), true, Some(2)));
}
