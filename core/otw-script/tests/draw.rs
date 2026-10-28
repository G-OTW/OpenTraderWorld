//! Drawing objects (phase 5): labels, lines, boxes, tables, linefills, polylines, chart points.
mod common;
use std::rc::Rc;

use common::*;
use otw_script::diag::{DiagCode, RtCode};
use otw_script::input::Bars;
use otw_script::session::Session;

fn ramp(n: usize) -> Bars {
    bars_close(&(0..n).map(|i| 100.0 + i as f64).collect::<Vec<_>>())
}

#[test]
fn label_per_bar_keeps_the_newest_fifty() {
    let out = run_src(&v6("label.new(bar_index, high, str.tostring(bar_index))\nplot(close)"), ramp(80));
    let l = &out.drawings.labels;
    assert_eq!(l.len(), 50);
    assert_eq!((l[0].x, l[49].x), (30, 79));
    assert_eq!((l[49].y, l[49].text.as_str()), (Some(179.0), "79"));
    assert_eq!((l[0].xloc.as_str(), l[0].yloc.as_str(), l[0].style.as_str(), l[0].size.as_str()), ("bar_index", "price", "label_down", "normal"));
}

#[test]
fn count_limit_comes_from_indicator() {
    let src = "//@version=6\nindicator(\"T\", max_labels_count = 3, max_lines_count = 2)\nlabel.new(bar_index, close)\nline.new(bar_index, close, bar_index + 1, close)\nplot(close)";
    let out = run_src(src, ramp(10));
    assert_eq!((out.drawings.labels.len(), out.drawings.lines.len()), (3, 2));
    let refused = otw_script::compile("//@version=6\nindicator(\"T\", max_labels_count = 501)\nplot(close)").unwrap_err();
    assert_eq!(refused[0].code, DiagCode::OutOfRange);
}

#[test]
fn var_line_moves_with_setters_and_methods() {
    let src = v6("var line l = line.new(0, close, 0, close, color = color.red, style = line.style_dashed)\nline.set_xy2(l, bar_index, close)\nl.set_width(3)\nplot(close)");
    let out = run_src(&src, ramp(5));
    let l = &out.drawings.lines;
    assert_eq!(l.len(), 1);
    assert_eq!((l[0].x1, l[0].y1, l[0].x2, l[0].y2), (0, Some(100.0), 4, Some(104.0)));
    assert_eq!((l[0].style.as_str(), l[0].width, l[0].color.map(|c| c.r)), ("dashed", 3, Some(255)));
}

#[test]
fn method_on_untyped_parameter_picks_the_kind() {
    let src = v6("paint(d, c) =>\n    d.set_color(c)\nvar l = label.new(0, 1)\nvar b = line.new(0, 2, 1, 1)\npaint(l, color.red)\npaint(b, color.green)\nplot(close)");
    let out = run_src(&src, ramp(2));
    assert_eq!(out.drawings.labels[0].color.map(|c| c.r), Some(255));
    assert_eq!(out.drawings.lines[0].color.map(|c| c.g), Some(175));
}

#[test]
fn getters_read_back_into_values() {
    let src = v6("var line l = line.new(0, 0.0, 10, 10.0)\nplot(line.get_price(l, 5))\nplot(l.get_x2())\nvar label lb = label.new(3, 7.5, \"t\")\nplot(lb.get_y())\nplot(str.length(label.get_text(lb)))");
    let out = run_src(&src, ramp(1));
    assert_eq!(out.plots.iter().map(|p| p.values[0]).collect::<Vec<_>>(), vec![Some(5.0), Some(10.0), Some(7.5), Some(1.0)]);
}

#[test]
fn delete_all_and_linefill_follow_their_lines() {
    let src = v6("a = line.new(bar_index, 1.0, bar_index + 1, 1.0)\nb = line.new(bar_index, 2.0, bar_index + 1, 2.0)\nlinefill.new(a, b, color.blue)\nif bar_index == 2\n    line.delete(a)\nfor lb in label.all\n    label.delete(lb)\nlabel.new(bar_index, close)\nplot(array.size(line.all))");
    let out = run_src(&src, ramp(3));
    assert_eq!(out.drawings.lines.len(), 5);
    assert_eq!(out.drawings.linefills.len(), 2, "the bar-2 pair lost its line");
    assert_eq!(out.drawings.labels.len(), 1);
    assert_eq!(out.plots[0].values, vec![Some(2.0), Some(4.0), Some(5.0)]);
}

#[test]
fn table_cells_and_out_of_range() {
    let src = v6("var t = table.new(position.top_right, 2, 2, bgcolor = color.black)\nif barstate.islast\n    table.cell(t, 0, 0, \"A\")\n    table.cell(t, 1, 1, str.tostring(close), text_color = color.white)\n    t.cell_set_text(0, 0, \"B\")\nplot(close)");
    let out = run_src(&src, ramp(3));
    let t = &out.drawings.tables[0];
    assert_eq!((t.position.as_str(), t.columns, t.rows, t.cells.len()), ("top_right", 2, 2, 2));
    assert_eq!((t.cells[0].text.as_str(), t.cells[1].text.as_str()), ("B", "102"));
    let e = run_inputs(&v6("var t = table.new(position.top_right, 1, 1)\ntable.cell(t, 1, 0, \"x\")\nplot(close)"), ramp(1), &[]).unwrap_err();
    assert_eq!((e.code, e.args.clone()), (RtCode::TableCell, vec!["1".to_string(), "0".to_string(), "1".to_string(), "1".to_string()]));
}

#[test]
fn chart_points_label_and_polyline() {
    let src = v6("p = chart.point.from_index(bar_index, high)\nlabel.new(p, \"hi\")\nvar pts = array.new<chart.point>()\npts.push(chart.point.now(low))\nif barstate.islast\n    polyline.new(pts, closed = true)\nplot(p.price)");
    let out = run_src(&src, ramp(3));
    let l = out.drawings.labels.last().unwrap();
    assert_eq!((l.x, l.y, l.text.as_str()), (2, Some(102.0), "hi"));
    let pl = &out.drawings.polylines[0];
    assert_eq!(pl.points, vec![(0, 100.0), (1, 101.0), (2, 102.0)]);
    assert!(pl.closed);
}

#[test]
fn bar_time_coordinates() {
    let out = run_src(&v6("label.new(time, close, xloc = xloc.bar_time)\nplot(close)"), ramp(2));
    assert_eq!((out.drawings.labels[1].x, out.drawings.labels[1].xloc.as_str()), (3_600_000, "bar_time"));
}

#[test]
fn drawings_run_without_a_plot_reading_them() {
    let out = run_src(&v6("if close > 101\n    label.new(bar_index, close)\nplot(close)"), ramp(4));
    assert_eq!(out.drawings.labels.len(), 2);
}

#[test]
fn a_drawing_needing_an_unsupported_feature_is_skipped() {
    let src = v6("h = request.security(syminfo.tickerid, \"D\", high)\nlabel.new(bar_index, h)\nplot(close)");
    let p = compile(&src);
    assert!(p.warnings().iter().any(|w| w.code == DiagCode::SkippedVisual && w.args[0] == "request.security"), "{:?}", p.warnings());
    let out = p.run(&input(ramp(3))).unwrap();
    assert!(out.drawings.labels.is_empty());
    assert_eq!(out.plots[0].values.len(), 3);
}

#[test]
fn drawings_survive_collection_when_no_variable_holds_them() {
    let src = v6("a = array.new_float(10, close)\nlabel.new(bar_index, array.sum(a))\nplot(close)");
    let out = run_src(&src, ramp(10_000));
    assert_eq!(out.drawings.labels.len(), 50);
    assert_eq!(out.drawings.labels[49].y, Some(10.0 * 10_099.0));
}

#[test]
fn forming_bar_drawings_roll_back() {
    let src = v6("label.new(bar_index, close)\nplot(close)");
    let p = Rc::new(compile(&src));
    let mut s = Session::open(p, input(ramp(3)), true).unwrap();
    assert_eq!(s.output().drawings.labels.len(), 3);
    // The forming bar updates twice: still one label for it, with its last price.
    let mut tail = ramp(3);
    tail = Bars { time: vec![tail.time[2]], time_close: vec![tail.time_close[2]], open: vec![150.0], high: vec![150.0], low: vec![150.0], close: vec![150.0], volume: vec![1.0] };
    s.advance(tail.clone(), true).unwrap();
    s.advance(tail, true).unwrap();
    let l = &s.output().drawings.labels;
    assert_eq!(l.len(), 3);
    assert_eq!(l[2].y, Some(150.0));
}

#[test]
fn typed_na_casts() {
    let src = v6("var a = array.new<label>(1, label(na))\nvar l = line(na)\nif na(l)\n    l := line.new(0, 1, 1, 1)\nplot(na(array.get(a, 0)) ? 1 : 0)");
    let out = run_src(&src, ramp(2));
    assert_eq!(out.plots[0].values, vec![Some(1.0), Some(1.0)]);
    assert_eq!(out.drawings.lines.len(), 1);
}

/// A drawing changed under a condition that reads the future taints what reads it back.
#[test]
fn readback_of_a_drawing_set_on_the_last_bar_is_tainted() {
    let p = compile(&v6("var line ln = line.new(0, 0.0, 1, 0.0)\nif barstate.islast\n    ln.set_y2(close)\nplot(ln.get_y2(), \"y\")\nplot(close, \"c\")"));
    assert!(!p.meta().plots[0].causal);
    assert!(p.meta().plots[1].causal);
    let q = compile(&v6("var line ln = line.new(0, 0.0, 1, 0.0)\nif barstate.islast\n    line.set_y2(ln, close)\nplot(line.get_y2(ln), \"y\")"));
    assert!(!q.meta().plots[0].causal);
}

/// The server cuts a run's warmup off before answering: per-bar columns lose the first bars
/// and drawings placed by bar index move with them; those placed by time stay.
#[test]
fn cutting_the_warmup_moves_bar_index_drawings_only() {
    let src = v6("label.new(bar_index, close)\nline.new(time, close, time + 1, close, xloc = xloc.bar_time)\nplot(close)");
    let out = run_src(&src, ramp(5)).cut(2);
    assert_eq!(out.plots[0].values, vec![Some(102.0), Some(103.0), Some(104.0)]);
    assert_eq!(out.drawings.labels.iter().map(|l| l.x).collect::<Vec<_>>(), vec![-2, -1, 0, 1, 2]);
    assert_eq!(out.drawings.lines[0].x1, 0, "a bar_time line keeps its time");
}
