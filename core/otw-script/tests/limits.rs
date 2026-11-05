mod common;
use common::*;

use otw_script::diag::RtCode;

#[test]
fn loop_limit() {
    let err = compile(&v6("x = 0\nwhile true\n    x += 1\nplot(x)")).run(&input(bars_close(&[1.0]))).expect_err("loop limit");
    assert_eq!(err.code, RtCode::LoopLimit);
    assert_eq!(err.span.line, 4);
}

#[test]
fn ops_budget() {
    let mut i = input(bars_close(&vec![1.0; 1000]));
    i.limits.max_ops = 1000;
    let err = compile(&v6("plot(close[1] + close[2])")).run(&i).expect_err("budget exceeded");
    assert_eq!(err.code, RtCode::OpsBudget);
}

#[test]
fn timeout_native() {
    let mut i = input(bars_close(&vec![1.0; 200]));
    i.limits.timeout_ms = Some(1);
    i.limits.max_loop_iters_per_bar = u32::MAX;
    let err = compile(&v6("s = 0.0\nfor j = 0 to 100000\n    s += j\nplot(s)")).run(&i).expect_err("timed out");
    assert_eq!(err.code, RtCode::Timeout);
}

// ── Final review fixes ──

/// A host worker thread: 2 MB (tokio's default) in release; debug frames are several times
/// larger, so debug runs get 16 MB.
fn on_small_stack<F: FnOnce() + Send + 'static>(f: F) {
    let mb = if cfg!(debug_assertions) { 16 } else { 2 };
    std::thread::Builder::new().stack_size(mb * 1024 * 1024).spawn(f).unwrap().join().unwrap();
}

#[test]
fn deep_nesting_refused_without_stack_overflow() {
    on_small_stack(|| {
        let parens = format!("x = {}1{}\nplot(x)", "(".repeat(5000), ")".repeat(5000));
        assert!(otw_script::compile(&v6(&parens)).is_err());
        let chain = format!("x = 1{}\nplot(x)", " + 1".repeat(10_000));
        assert!(otw_script::compile(&v6(&chain)).is_err());
        let unary = format!("x = {}1\nplot(x)", "-".repeat(100_000));
        assert!(otw_script::compile(&v6(&unary)).is_err());
    });
}

#[test]
fn moderate_nesting_runs_on_small_stack() {
    on_small_stack(|| {
        let parens = format!("x = {}1{}\nplot(x)", "(".repeat(200), ")".repeat(200));
        let chain = format!("x = 1{}\nplot(x)", " + 1".repeat(200));
        for src in [parens, chain] {
            let p = otw_script::compile(&v6(&src)).expect("compiles");
            p.run(&input(bars_close(&[1.0]))).expect("runs");
        }
    });
}

#[test]
fn string_length_capped() {
    let err = compile(&v6("s = \"a\"\nfor i = 0 to 26\n    s := s + s\nplot(s == \"b\" ? 1 : 0)")).run(&input(bars_close(&[1.0]))).expect_err("capped");
    assert_eq!(err.code, RtCode::StringTooLong);
}

#[test]
fn max_bars_back_above_limit_refused() {
    let e = otw_script::compile("//@version=6\nindicator(\"t\", max_bars_back = 200000000)\nplot(close)").expect_err("refused");
    assert!(e.iter().any(|d| d.code == otw_script::diag::DiagCode::OutOfRange), "{e:?}");
}

#[test]
fn window_length_above_history_refused() {
    let err = compile(&v6("plot(ta.sma(close, 1000000000))")).run(&input(bars_close(&[1.0]))).expect_err("refused");
    assert_eq!(err.code, RtCode::HistoryTooDeep);
}

#[test]
fn instance_fan_out_capped() {
    let mut src = String::from("f0(x) => x\n");
    for i in 1..=22 {
        src.push_str(&format!("f{i}(x) => f{}(x) + f{}(x)\n", i - 1, i - 1));
    }
    src.push_str("plot(f22(close))");
    let err = compile(&v6(&src)).run(&input(bars_close(&[1.0]))).expect_err("capped");
    assert_eq!(err.code, RtCode::MemoryLimit);
}
