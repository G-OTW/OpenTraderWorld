mod common;
use common::*;

use otw_script::diag::RtCode;

const NA: Option<f64> = None;

#[test]
fn arith_na() {
    let out = run_src(&v6("x = close > 2 ? close : na\nplot(x + 1)\nplot(x * 2 - na)"), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), vec![NA, NA, Some(4.0)]);
    assert_eq!(plot(&out, 1), vec![NA, NA, NA]);
}

#[test]
fn compare_na_false() {
    let out = run_src(&v6("x = close > 2 ? close : na\nplot(x > 0 ? 1 : 0)\nplot(x == x ? 1 : 0)"), bars_close(&[1.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[0.0, 1.0]));
    assert_eq!(plot(&out, 1), some(&[0.0, 1.0]));
}

#[test]
fn short_circuit_v6_not_v5() {
    // `f` counts its own executions; on the right of `and` it runs only when the left is true (v6).
    let body = "f() =>\n    var int n = 0\n    n += 1\n    n > 2\nc = bar_index % 2 == 0 and f()\nplot(c ? 1 : 0)";
    let bars = bars_close(&[1.0; 6]);
    assert_eq!(plot(&run_src(&v6(body), bars.clone()), 0), some(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    assert_eq!(plot(&run_src(&v5(body), bars), 0), some(&[0.0, 0.0, 1.0, 0.0, 1.0, 0.0]));
}

#[test]
fn int_div_v5_v6() {
    let bars = bars_close(&[1.0, 1.0]);
    assert_eq!(plot(&run_src(&v5("plot(5 / 2)"), bars.clone()), 0), some(&[2.0, 2.0]));
    assert_eq!(plot(&run_src(&v6("plot(5 / 2)"), bars.clone()), 0), some(&[2.5, 2.5]));
    assert_eq!(plot(&run_src(&v5("plot(bar_index / 2)"), bars.clone()), 0), some(&[0.0, 0.5]));
    assert_eq!(plot(&run_src(&v6("plot(bar_index / 2)"), bars), 0), some(&[0.0, 0.5]));
}

#[test]
fn div_zero_na() {
    assert_eq!(plot(&run_src(&v6("plot(close / 0)\nplot(1 % 0)"), bars_close(&[1.0])), 0), vec![NA]);
}

#[test]
fn history_basic() {
    let out = run_src(&v6("plot(close[2])"), bars_close(&[1.0, 2.0, 3.0, 4.0]));
    assert_eq!(plot(&out, 0), vec![NA, NA, Some(1.0), Some(2.0)]);
}

#[test]
fn history_expr() {
    let out = run_src(&v6("plot((close + 1)[1])\ny = close * 10\nplot(y[1])"), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), vec![NA, Some(2.0), Some(3.0)]);
    assert_eq!(plot(&out, 1), vec![NA, Some(10.0), Some(20.0)]);
}

#[test]
fn var_once() {
    let out = run_src(&v6("var c = 0\nc += 1\nplot(c)"), bars_close(&[1.0, 1.0, 1.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 3.0]));
}

#[test]
fn plain_reruns() {
    let out = run_src(&v6("c = 0\nc += 1\nplot(c)\nplot(c[1])"), bars_close(&[1.0, 1.0, 1.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 1.0, 1.0]));
    assert_eq!(plot(&out, 1), vec![NA, Some(1.0), Some(1.0)]);
}

#[test]
fn history_too_deep() {
    let err = compile(&v6("plot(close[6000])")).run(&input(bars_close(&[1.0]))).unwrap_err();
    assert_eq!(err.code, RtCode::HistoryTooDeep);
    assert_eq!(err.span.line, 3);
}

// ── Control flow, functions, call-site state (Task 10) ──

const COUNTER: &str = "cnt() =>\n    var int n = 0\n    n += 1\n    n\n";

#[test]
fn if_expr_no_else_is_na() {
    let out = run_src(&v6("x = if close > 1\n    close\nplot(x)"), bars_close(&[1.0, 2.0]));
    assert_eq!(plot(&out, 0), vec![NA, Some(2.0)]);
}

#[test]
fn switch_key_and_cond_forms() {
    let body = "k = bar_index % 3\ns = switch k\n    0 => 10\n    1 => 20\n    => 30\nt = switch\n    close > 2 => 1\n    => 2\nplot(s)\nplot(t)";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[10.0, 20.0, 30.0]));
    assert_eq!(plot(&out, 1), some(&[2.0, 2.0, 1.0]));
}

#[test]
fn for_to_by_inclusive() {
    let out = run_src(&v6("s = 0\nfor i = 0 to 4 by 2\n    s += i\nplot(s)"), bars_close(&[1.0]));
    assert_eq!(plot(&out, 0), some(&[6.0]));
}

#[test]
fn for_down() {
    let out = run_src(&v6("s = 0\nlast = -1\nfor i = 4 to 0\n    s += i\n    last := i\nplot(s)\nplot(last)"), bars_close(&[1.0]));
    assert_eq!(plot(&out, 0), some(&[10.0]));
    assert_eq!(plot(&out, 1), some(&[0.0]));
}

#[test]
fn while_break_continue() {
    let body = "i = 0\ns = 0\nwhile i < 10\n    i += 1\n    if i == 3\n        continue\n    if i == 6\n        break\n    s += i\nplot(s)";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    assert_eq!(plot(&out, 0), some(&[1.0 + 2.0 + 4.0 + 5.0]));
}

#[test]
fn tuple_return() {
    let out = run_src(&v6("f() => [1, 2]\n[a, b] = f()\nplot(a * 10 + b)"), bars_close(&[1.0]));
    assert_eq!(plot(&out, 0), some(&[12.0]));
}

#[test]
fn call_site_state() {
    // Two call sites of the same function: two independent counters.
    let out = run_src(&v6(&format!("{COUNTER}a = cnt()\nb = cnt()\nplot(a)\nplot(b)")), bars_close(&[1.0; 3]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 1), some(&[1.0, 2.0, 3.0]));
}

#[test]
fn stateful_call_inside_branch_advances_only_when_run() {
    let out = run_src(&v6(&format!("{COUNTER}x = bar_index % 2 == 0 ? cnt() : na\nplot(x)")), bars_close(&[1.0; 5]));
    assert_eq!(plot(&out, 0), vec![Some(1.0), NA, Some(2.0), NA, Some(3.0)]);
}

#[test]
fn block_history_per_execution() {
    let body = "x = 0.0\nif bar_index % 2 == 0\n    y = close\n    x := y[1]\nplot(x)";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0, 4.0, 5.0]));
    assert_eq!(plot(&out, 0), vec![NA, Some(0.0), Some(1.0), Some(0.0), Some(3.0)]);
}

#[test]
fn function_params_have_history() {
    let out = run_src(&v6("prev(x) => x[1]\nplot(prev(close))"), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), vec![NA, Some(1.0), Some(2.0)]);
}

#[test]
fn function_reads_globals() {
    let out = run_src(&v6("k = 10\nf(x) => x * k\nplot(f(close))"), bars_close(&[1.0, 2.0]));
    assert_eq!(plot(&out, 0), some(&[10.0, 20.0]));
}

// ── Final review fixes ──

#[test]
fn function_returning_switch_keeps_its_arms() {
    let body = "t = input.string(\"SMA\", \"type\", options = [\"SMA\", \"EMA\"])\nma(src, len) =>\n    switch t\n        \"SMA\" => ta.sma(src, len)\n        \"EMA\" => ta.ema(src, len)\nplot(ma(close, 2))";
    assert_eq!(plot(&run_src(&v6(body), bars_close(&[1.0, 3.0])), 0), vec![NA, Some(2.0)]);
}

#[test]
fn nested_if_as_value_keeps_its_arms() {
    let body = "x = if close > 0\n    if close > 1\n        1\n    else\n        2\nplot(x)";
    assert_eq!(plot(&run_src(&v6(body), bars_close(&[1.0, 3.0])), 0), some(&[2.0, 1.0]));
}

#[test]
fn statement_if_with_a_drawing_still_compiles() {
    let body = "x = 0\nif close > 1\n    x := 1\n    label.new(bar_index, close)\nplot(x)";
    assert_eq!(plot(&run_src(&v6(body), bars_close(&[1.0, 3.0])), 0), some(&[0.0, 1.0]));
}

#[test]
fn generic_input_source_is_a_series() {
    let src = v5("src = input(close, \"Source\")\nplot(src)");
    assert_eq!(compile(&src).meta().inputs[0].kind, otw_script::output::InputKind::Source);
    assert_eq!(plot(&run_src(&src, bars_close(&[1.0, 2.0, 3.0])), 0), some(&[1.0, 2.0, 3.0]));
}

#[test]
fn int_overflow_wraps_never_panics() {
    let out = run_src(&v6("x = -9223372036854775807 - 1\nplot(x % -1)\nplot(x / -1)"), bars_close(&[1.0]));
    assert_eq!(plot(&out, 0), some(&[0.0]));
}
