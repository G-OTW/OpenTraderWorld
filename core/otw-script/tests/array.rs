//! `array.*` (phase 4): reference semantics, methods, `for…in`, errors, collection, sessions.
mod common;
use std::rc::Rc;

use common::*;
use otw_script::diag::RtCode;
use otw_script::session::Session;

const NA: Option<f64> = None;

#[test]
fn var_array_accumulates_across_bars() {
    let body = "var a = array.new_float()\na.push(close)\nplot(a.size())\nplot(array.sum(a))\nplot(a.get(0))\nplot(a.last())";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 1), some(&[1.0, 3.0, 6.0]));
    assert_eq!(plot(&out, 2), some(&[1.0, 1.0, 1.0]));
    assert_eq!(plot(&out, 3), some(&[1.0, 2.0, 3.0]));
}

#[test]
fn plain_array_is_new_each_bar() {
    let out = run_src(&v6("a = array.new_int(0)\narray.push(a, 1)\nplot(array.size(a))"), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 1.0, 1.0]));
}

#[test]
fn assignment_shares_copy_does_not() {
    let body = "a = array.from(1, 2)\nb = a\nc = array.copy(a)\nb.push(3)\nplot(a.size())\nplot(c.size())";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0)[0], plot(&out, 1)[0]), (Some(3.0), Some(2.0)));
}

#[test]
fn generic_new_and_typed_declarations() {
    let body = "array<float> a = array.new<float>(3, 1.5)\nfloat[] b = array.new_float(2, 2)\nplot(array.sum(a) + array.sum(b))";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    assert_eq!(plot(&out, 0), some(&[8.5]));
}

#[test]
fn for_in_items_and_indices() {
    let body = "a = array.from(3.0, 4.0, 5.0)\ns = 0.0\nfor x in a\n    s += x\nt = 0\nfor [i, x] in a\n    t += i\nplot(s)\nplot(t)";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0)[0], plot(&out, 1)[0]), (Some(12.0), Some(3.0)));
}

#[test]
fn stats_skip_na_and_follow_element_type() {
    let body = "a = array.from(4, 1, 3)\nb = array.from(2.0, na, 6.0)\nplot(a.max())\nplot(a.min(1))\nplot(b.avg())\nplot(b.median())\nplot(a.stdev())\nplot(array.percentile_nearest_rank(a, 50))";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    let v: Vec<Option<f64>> = (0..6).map(|i| plot(&out, i)[0]).collect();
    assert_eq!(&v[..4], &[Some(4.0), Some(3.0), Some(4.0), Some(4.0)]);
    assert!((v[4].unwrap() - libm::sqrt(14.0f64 / 9.0)).abs() < 1e-12);
    assert_eq!(v[5], Some(3.0));
}

#[test]
fn sort_reverse_join_and_search() {
    let body = "a = array.from(3, 1, 2)\na.sort()\ns = a.join(\",\")\nd = array.copy(a)\nd.sort(order.descending)\nplot(s == \"1,2,3\" ? 1 : 0)\nplot(d.get(0))\nplot(a.indexof(2))\nplot(a.binary_search(4))\nplot(a.binary_search_leftmost(4))\nplot(a.includes(3) ? 1 : 0)";
    let out = run_src(&v6(body), bars_close(&[1.0]));
    let v: Vec<Option<f64>> = (0..6).map(|i| plot(&out, i)[0]).collect();
    assert_eq!(v, vec![Some(1.0), Some(3.0), Some(1.0), Some(-1.0), Some(2.0), Some(1.0)]);
}

#[test]
fn negative_index_counts_from_the_end() {
    let out = run_src(&v6("a = array.from(1, 2, 3)\nplot(a.get(-1))\nplot(a.remove(-3))\nplot(a.size())"), bars_close(&[1.0]));
    assert_eq!((plot(&out, 0)[0], plot(&out, 1)[0], plot(&out, 2)[0]), (Some(3.0), Some(1.0), Some(2.0)));
}

#[test]
fn out_of_bounds_stops_the_run() {
    let e = run_inputs(&v6("a = array.new_float(2)\nplot(a.get(2))"), bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!((e.code, e.args.clone()), (RtCode::ArrayIndex, vec!["2".to_string(), "2".to_string()]));
    assert_eq!(e.span.line, 4);
}

#[test]
fn pop_of_empty_and_na_array_stop_the_run() {
    let e = run_inputs(&v6("a = array.new_float()\nplot(a.pop())"), bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!(e.code, RtCode::ArrayEmpty);
    let e = run_inputs(&v6("array<float> a = na\nplot(a.size())"), bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!(e.code, RtCode::ArrayNa);
}

#[test]
fn per_bar_arrays_are_collected() {
    // 20 arrays per bar over 20,000 bars: far past the live-object cap unless collected.
    let body = "s = 0.0\nfor i = 0 to 19\n    a = array.new_float(10, close)\n    s += a.sum()\nvar keep = array.new_float()\nkeep.push(s)\nif keep.size() > 5\n    keep.shift()\nplot(keep.sum())";
    let out = run_src(&v6(body), synth(20_000, 3));
    assert!(plot(&out, 0).iter().all(Option::is_some));
}

#[test]
fn session_rollback_restores_arrays() {
    let src = v6("var a = array.new_float()\na.push(close)\nplot(a.size())\nplot(a.sum())");
    let all = synth(50, 5);
    let p = Rc::new(compile(&src));
    let mut s = Session::open(p, input(sub(&all, 0, 49)), false).unwrap();
    let mut forming = sub(&all, 49, 50);
    forming.close[0] *= 1.01;
    forming.high[0] = forming.high[0].max(forming.close[0]);
    s.advance(forming.clone(), true).unwrap();
    s.advance(forming, true).unwrap();
    s.advance(sub(&all, 49, 50), false).unwrap();
    let full = compile(&src).run(&input(all)).unwrap();
    assert_eq!(s.output(), &full);
    assert_eq!(full.plots[0].values[49], Some(50.0));
}

#[test]
fn array_of_lines_draws() {
    let body = "var lines = array.new_line()\nif barstate.islast\n    array.push(lines, line.new(bar_index - 1, low, bar_index, high))\nplot(close)";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0]));
    assert_eq!(out.drawings.lines.len(), 1);
    let _ = NA;
}

#[test]
fn changes_through_an_alias_are_kept() {
    let out = run_src(&v6("var a = array.new_float()\nb = a\nb.push(close)\nplot(a.size())"), bars_close(&[1.0, 2.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0]));
}

#[test]
fn changes_through_a_function_are_kept() {
    let body = "add(arr, x) =>\n    arr.push(x)\n    0\nvar a = array.new_float()\nadd(a, close)\nplot(a.size())";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 3.0]));
}

#[test]
fn a_function_changing_a_global_array_is_kept() {
    let body = "var a = array.new_float()\nrec() =>\n    a.push(1.0)\n    true\nrec()\nplot(a.size())";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0]));
}

#[test]
fn arrays_feeding_drawings_run_with_them() {
    let body = "var levels = array.new_float()\nvar array<line> lines = array.new<line>()\nif ta.change(close) > 0\n    levels.push(close)\n    ln = line.new(bar_index, close, bar_index + 1, close)\n    lines.push(ln)\n    if levels.size() > 3\n        levels.shift()\n        line.delete(lines.shift())\nplot(close)";
    let p = compile(&v6(body));
    assert!(p.warnings().is_empty(), "{:?}", p.warnings());
    let out = p.run(&input(bars_close(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]))).unwrap();
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
    // Five rises, the oldest two lines deleted past three levels.
    assert_eq!(out.drawings.lines.iter().map(|l| l.x1).collect::<Vec<_>>(), vec![3, 4, 5]);
}

/// Many large arrays alive at once stop the run on the memory limit instead of exhausting
/// memory; the same arrays dropped bar after bar are collected and never reach it.
#[test]
fn cell_budget_caps_live_arrays() {
    let kept = v6("var keep = array.new<float>()\nfor i = 0 to 99\n    a = array.new_float(100000, 1.0)\n    keep.push(array.size(a))\nplot(keep.size())");
    let e = run_inputs(&kept, bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!(e.code, RtCode::MemoryLimit, "{e:?}");
    let temp = v6("a = array.new_float(100000, 1.0)\nplot(array.size(a))");
    let out = run_src(&temp, bars_close(&vec![1.0; 200]));
    assert_eq!(out.plots[0].values.last(), Some(&Some(100000.0)));
}

#[test]
fn slice_end_before_start_stops_the_run() {
    let e = run_inputs(&v6("a = array.from(1, 2, 3)\nplot(array.slice(a, 2, 1).size())"), bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!((e.code, e.args.clone()), (RtCode::ArrayIndex, vec!["1".to_string(), "3".to_string()]));
}
