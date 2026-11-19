//! `str.*` (phase 4).
mod common;
use common::*;
use otw_script::diag::RtCode;
use otw_script::output::RunOutput;

/// Plot 1 when the string expression equals `want`, so a test reads strings through plots.
fn eq(expr: &str, want: &str) -> String {
    format!("plot({expr} == \"{want}\" ? 1 : 0)")
}

fn all_true(out: &RunOutput) {
    for (i, p) in out.plots.iter().enumerate() {
        assert_eq!(p.values[0], Some(1.0), "plot {i}");
    }
}

#[test]
fn tostring_default_and_patterns() {
    let body = [
        eq("str.tostring(1.0 / 3)", "0.3333333333"),
        eq("str.tostring(2.50)", "2.5"),
        eq("str.tostring(42)", "42"),
        eq("str.tostring(-0.0001, \"#.##\")", "0"),
        eq("str.tostring(1234567.891, \"#,##0.00\")", "1,234,567.89"),
        eq("str.tostring(0.256, \"#.#%\")", "25.6%"),
        eq("str.tostring(true)", "true"),
        eq("str.tostring(na(float) ? na : 1.0)", "1"),
        eq("str.tostring(12.3456, format.percent)", "12.35%"),
        eq("str.tostring(1534000, format.volume)", "1.534M"),
        eq("str.tostring(3.14159, format.mintick)", "3.14"),
        eq("str.tostring(array.from(1, 2))", "[1, 2]"),
    ]
    .join("\n");
    all_true(&run_src(&v6(&body), bars_close(&[1.0])));
}

#[test]
fn format_message_and_time() {
    let body = [
        eq("str.format(\"{0} is {1}\", \"x\", 2)", "x is 2"),
        eq("str.format(\"{0,number,#.#}\", 1.25)", "1.2"),
        eq("str.format(\"{0}\", 1234.5678)", "1,234.568"),
        eq("str.format(\"'{0}' {0}\", 1)", "{0} 1"),
        eq("str.format_time(0, \"yyyy-MM-dd HH:mm\", \"UTC\")", "1970-01-01 00:00"),
        eq("str.format_time(time, \"EEE d MMM yy\", \"UTC\")", "Thu 1 Jan 70"),
    ]
    .join("\n");
    all_true(&run_src(&v6(&body), bars_close(&[1.0])));
}

#[test]
fn search_and_edit() {
    let body = [
        "plot(str.length(\"héllo\"))".to_string(),
        "plot(str.pos(\"abcabc\", \"ca\"))".to_string(),
        "plot(str.contains(\"BTCUSDT\", \"USD\") ? 1 : 0)".to_string(),
        eq("str.substring(\"abcdef\", 2, 4)", "cd"),
        eq("str.substring(\"abcdef\", 3)", "def"),
        eq("str.replace(\"a-b-c\", \"-\", \"+\", 1)", "a-b+c"),
        eq("str.replace_all(\"a-b-c\", \"-\", \"\")", "abc"),
        eq("str.upper(\"aB\") + str.lower(\"aB\")", "ABab"),
        eq("str.trim(\"  x \")", "x"),
        eq("str.repeat(\"ab\", 3, \"-\")", "ab-ab-ab"),
        "plot(str.tonumber(\"12.5\") + 1)".to_string(),
        "plot(na(str.tonumber(\"x\")) ? 1 : 0)".to_string(),
        "parts = str.split(\"a,b,,c\", \",\")\nplot(parts.size())".to_string(),
    ]
    .join("\n");
    let out = run_src(&v6(&body), bars_close(&[1.0]));
    let v: Vec<Option<f64>> = out.plots.iter().map(|p| p.values[0]).collect();
    assert_eq!(&v[..3], &[Some(5.0), Some(2.0), Some(1.0)]);
    assert!(v[3..10].iter().all(|x| *x == Some(1.0)), "{v:?}");
    assert_eq!(&v[10..], &[Some(13.5), Some(1.0), Some(4.0)]);
}

#[test]
fn mintick_unknown_is_refused() {
    let mut i = input(bars_close(&[1.0]));
    i.syminfo.mintick = None;
    let e = compile(&v6("plot(str.length(str.tostring(close, format.mintick)))")).run(&i).unwrap_err();
    assert_eq!((e.code, e.args.clone()), (RtCode::SyminfoUnknown, vec!["mintick".to_string()]));
}

/// `log.*` runs without a warning and changes no value (the Pine Logs pane is not shown).
#[test]
fn log_calls_are_supported() {
    let src = v6("log.info(\"close {0}\", close)\nlog.warning(\"w\")\nif close > 0\n    log.error(\"e {0} {1}\", close, bar_index)\nplot(close)");
    let program = otw_script::compile(&src).expect("compiles");
    assert!(program.warnings().is_empty(), "{:?}", program.warnings());
    let out = run_src(&src, bars_close(&[1.0, 2.0]));
    assert_eq!(out.plots[0].values, vec![Some(1.0), Some(2.0)]);
}

/// A replacement that would grow past the string limit stops the run before it is built.
#[test]
fn replace_all_is_capped() {
    let src = v6("s = str.replace_all(str.repeat(\"a\", 4000), \"a\", \"bb\")\nplot(str.length(s))");
    let e = run_inputs(&src, bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!(e.code, RtCode::StringTooLong);
}
