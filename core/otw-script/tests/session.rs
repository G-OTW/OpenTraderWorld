//! Realtime model: confirmed bars run once, the forming bar re-runs from the last confirmed
//! state on every update (rollback) and is committed on close (spec §4.2).
mod common;
use std::rc::Rc;

use common::*;
use otw_script::diag::RtCode;
use otw_script::input::Bars;
use otw_script::session::Session;

/// Bar `i` of `b` with its close moved by `pct` (high/low widened to keep a candle).
fn altered(b: &Bars, i: usize, pct: f64) -> Bars {
    let mut one = sub(b, i, i + 1);
    one.close[0] *= 1.0 + pct;
    one.high[0] = one.high[0].max(one.close[0]);
    one.low[0] = one.low[0].min(one.close[0]);
    one
}

fn open(src: &str, bars: Bars, forming: bool) -> Session {
    Session::open(Rc::new(compile(src)), input(bars), forming).unwrap_or_else(|e| panic!("open: {e:?}"))
}

#[test]
fn rollback_equals_closed_run() {
    let src = v6(PERF_BODY);
    let all = synth(3000, 7);
    let mut s = open(&src, sub(&all, 0, 2999), false);
    s.advance(altered(&all, 2999, 0.01), true).unwrap();
    s.advance(altered(&all, 2999, -0.01), true).unwrap();
    s.advance(sub(&all, 2999, 3000), false).unwrap();
    let full = compile(&src).run(&input(all)).unwrap();
    assert_eq!(s.output(), &full);
    assert_eq!(s.committed(), 3000);
}

#[test]
fn forming_then_next_bar_commits() {
    let all = synth(3000, 7);
    let mut s = open(&v6(PERF_BODY), sub(&all, 0, 2998), false);
    s.advance(altered(&all, 2998, 0.005), true).unwrap();
    assert_eq!((s.committed(), s.len()), (2998, 2999));
    let mut tail = sub(&all, 2998, 3000);
    tail.close[1] *= 1.001;
    tail.high[1] = tail.high[1].max(tail.close[1]);
    s.advance(tail, true).unwrap();
    assert_eq!((s.committed(), s.len()), (2999, 3000));
    let o = s.output_from(2998);
    assert_eq!(o.plots[0].values.len(), 2);
    assert_eq!(o.plots[0].colors.len(), 2);
}

#[test]
fn barstate_in_session() {
    let src = v6("plot(barstate.isrealtime ? 1 : 0)\nplot(barstate.isnew ? 1 : 0)\nplot(barstate.isconfirmed ? 1 : 0)\nplot(barstate.ishistory ? 1 : 0)");
    let all = synth(12, 3);
    // [isrealtime, isnew, isconfirmed, ishistory] on bar `i`.
    let flags = |s: &Session, i: usize| -> Vec<f64> { (0..4).map(|p| s.output().plots[p].values[i].unwrap()).collect() };
    let mut s = open(&src, sub(&all, 0, 10), false);
    assert_eq!(flags(&s, 9), [0.0, 1.0, 1.0, 1.0], "open bar");
    s.advance(altered(&all, 10, 0.001), true).unwrap();
    assert_eq!(flags(&s, 10), [1.0, 1.0, 0.0, 0.0], "first forming execution");
    s.advance(altered(&all, 10, 0.002), true).unwrap();
    assert_eq!(flags(&s, 10), [1.0, 0.0, 0.0, 0.0], "later forming execution");
    s.advance(sub(&all, 10, 11), false).unwrap();
    assert_eq!(flags(&s, 10), [1.0, 0.0, 1.0, 0.0], "commit of a bar that was forming");
    s.advance(sub(&all, 11, 12), false).unwrap();
    assert_eq!(flags(&s, 11), [1.0, 1.0, 1.0, 0.0], "commit of a bar never seen forming");
}

#[test]
fn advance_error_rolls_back() {
    let src = v6("if close > 1e9\n    runtime.error(\"big\")\nplot(close)");
    let all = synth(50, 5);
    let mut s = open(&src, sub(&all, 0, 48), false);
    s.advance(altered(&all, 48, 0.01), true).unwrap();
    let before = (s.committed(), s.len(), s.output().clone());
    let mut bad = sub(&all, 48, 49);
    bad.close[0] = 2e9;
    bad.high[0] = 2e9;
    let e = s.advance(bad, true).unwrap_err();
    assert_eq!(e.code, RtCode::RuntimeErrorCall);
    assert_eq!((s.committed(), s.len(), s.output().clone()), before);
}

#[test]
fn conditional_output_is_cleared_on_rollback() {
    let src = v6("bgcolor(close > open ? color.red : na)");
    let all = synth(30, 9);
    let mut s = open(&src, sub(&all, 0, 29), false);
    let mut up = sub(&all, 29, 30);
    up.close[0] = up.open[0] * 1.01;
    up.high[0] = up.high[0].max(up.close[0]);
    s.advance(up, true).unwrap();
    assert!(s.output().bgcolor[0].colors[29].is_some());
    let mut down = sub(&all, 29, 30);
    down.close[0] = down.open[0] * 0.99;
    down.low[0] = down.low[0].min(down.close[0]);
    s.advance(down, true).unwrap();
    assert!(s.output().bgcolor[0].colors[29].is_none());
}

#[test]
fn ops_budget_is_per_call() {
    let src = v6("plot(ta.sma(close, 5))");
    let all = synth(200, 11);
    let p = Rc::new(compile(&src));
    // The smallest budget the open fits in: any later bar run on a shared counter would fail.
    let mut i = input(sub(&all, 0, 195));
    let fits = |ops: u64, i: &mut otw_script::input::RunInput| {
        i.limits.max_ops = ops;
        Session::open(p.clone(), i.clone(), false).is_ok()
    };
    let (mut lo, mut budget) = (1u64, 1_000_000u64);
    assert!(fits(budget, &mut i));
    while lo < budget {
        let mid = (lo + budget) / 2;
        if fits(mid, &mut i) {
            budget = mid;
        } else {
            lo = mid + 1;
        }
    }
    i.limits.max_ops = budget;
    let mut s = Session::open(p, i, false).expect("open fits");
    for k in 195..200 {
        s.advance(sub(&all, k, k + 1), false).expect("each advance has its own budget");
    }
}
