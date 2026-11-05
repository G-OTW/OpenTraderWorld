//! User-defined types and methods (phase 4).
mod common;
use common::*;
use otw_script::diag::RtCode;

#[test]
fn new_fields_defaults_and_assignment() {
    let body = "type Pivot\n    float price\n    int bar = -1\n    string tag = \"p\"\np = Pivot.new(close)\np.bar := bar_index\nq = Pivot.new(tag = \"q\", price = 2.0)\nplot(p.price)\nplot(p.bar)\nplot(q.bar)\nplot(q.tag == \"q\" ? 1 : 0)";
    let out = run_src(&v6(body), bars_close(&[5.0, 6.0]));
    assert_eq!(plot(&out, 0), some(&[5.0, 6.0]));
    assert_eq!(plot(&out, 1), some(&[0.0, 1.0]));
    assert_eq!(plot(&out, 2), some(&[-1.0, -1.0]));
    assert_eq!(plot(&out, 3), some(&[1.0, 1.0]));
}

#[test]
fn var_object_persists_and_copy_is_shallow() {
    let body = "type Acc\n    float sum = 0.0\n    array<float> seen\nvar a = Acc.new(0.0, array.new_float())\na.sum += close\na.seen.push(close)\nb = Acc.copy(a)\nb.sum := -1\nplot(a.sum)\nplot(a.seen.size())\nplot(b.seen.size())";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 3.0, 6.0]));
    assert_eq!(plot(&out, 1), some(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 2), some(&[1.0, 2.0, 3.0]));
}

#[test]
fn methods_on_types_and_arrays() {
    let body = "type Box2\n    float lo\n    float hi\nmethod width(Box2 this) =>\n    this.hi - this.lo\nmethod grow(Box2 this, float d) =>\n    this.hi += d\n    this\nmethod total(array<float> self) =>\n    s = 0.0\n    for x in self\n        s += x\n    s\nb = Box2.new(1.0, 3.0)\nb.grow(close)\na = array.from(1.0, 2.0)\nplot(b.width())\nplot(a.total())\nplot(width(b))";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0]));
    assert_eq!(plot(&out, 0), some(&[3.0, 4.0]));
    assert_eq!(plot(&out, 1), some(&[3.0, 3.0]));
    assert_eq!(plot(&out, 2), some(&[3.0, 4.0]));
}

#[test]
fn objects_in_arrays() {
    let body = "type P\n    float v\nvar ps = array.new<P>()\nps.push(P.new(close))\nif ps.size() > 2\n    ps.shift()\ns = 0.0\nfor p in ps\n    s += p.v\nplot(s)\nplot(ps.last().v)";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0, 4.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 3.0, 5.0, 7.0]));
    assert_eq!(plot(&out, 1), some(&[1.0, 2.0, 3.0, 4.0]));
}

#[test]
fn field_of_na_object_stops_the_run() {
    let e = run_inputs(&v6("type P\n    float v\nP p = na\nplot(p.v)"), bars_close(&[1.0]), &[]).unwrap_err();
    assert_eq!((e.code, e.args.clone()), (RtCode::ObjectNa, vec!["v".to_string()]));
}

#[test]
fn unknown_field_is_refused() {
    let r = otw_script::compile(&v6("type P\n    float v\np = P.new(1.0)\nplot(p.w)"));
    let d = r.unwrap_err();
    assert!(d.iter().any(|d| d.code == otw_script::diag::DiagCode::UnknownName && d.args == vec!["P.w".to_string()]), "{d:?}");
}

#[test]
fn a_method_changing_its_object_is_kept() {
    let body = "type C\n    int n = 0\nmethod bump(C this) =>\n    this.n += 1\nvar c = C.new()\nc.bump()\nplot(c.n)";
    let out = run_src(&v6(body), bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(plot(&out, 0), some(&[1.0, 2.0, 3.0]));
}

/// Writing a field through a value read out of an array changes the array's object: the write
/// is kept although the local is never read again.
#[test]
fn field_write_through_array_element_is_kept() {
    let src = v6("type P\n    float v = 0\nvar arr = array.from(P.new(), P.new())\nitem = array.get(arr, 0)\nitem.v := 5\nfor p in arr\n    p.v += 1\nplot(array.get(arr, 0).v)\nplot(array.get(arr, 1).v)");
    let out = run_src(&src, bars_close(&[1.0]));
    assert_eq!(out.plots[0].values, vec![Some(6.0)]);
    assert_eq!(out.plots[1].values, vec![Some(1.0)]);
}

/// An array held in a field changes through the field path and through a local holding it.
#[test]
fn push_through_field_array_is_kept() {
    let src = v6("type Bag\n    array<float> items\nvar b = Bag.new(array.new_float())\narray.push(b.items, close)\ninner = b.items\ninner.push(1.0)\nplot(b.items.size())");
    let out = run_src(&src, bars_close(&[1.0, 2.0]));
    assert_eq!(out.plots[0].values, vec![Some(2.0), Some(4.0)]);
}

/// A field written through `c ? a : b` reaches whichever object was picked.
#[test]
fn field_write_through_ternary_is_kept() {
    let src = v6("type P\n    float v = 0\nvar a = P.new()\nvar b = P.new()\np = close > 1 ? a : b\np.v += 1\nplot(a.v)\nplot(b.v)");
    let out = run_src(&src, bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(out.plots[0].values, vec![Some(0.0), Some(1.0), Some(2.0)]);
    assert_eq!(out.plots[1].values, vec![Some(1.0), Some(1.0), Some(1.0)]);
}

/// A field written through the object an `if` or a `switch` picked reaches that object.
#[test]
fn field_write_through_if_and_switch_is_kept() {
    let src = v6("type P\n    float v = 0\nvar a = P.new()\nvar b = P.new()\np = if close > 1\n    a\nelse\n    b\np.v += 1\nq = switch\n    close > 2 => a\n    => b\nq.v += 10\nplot(a.v)\nplot(b.v)");
    let out = run_src(&src, bars_close(&[1.0, 2.0, 3.0]));
    assert_eq!(out.plots[0].values, vec![Some(0.0), Some(1.0), Some(12.0)]);
    assert_eq!(out.plots[1].values, vec![Some(11.0), Some(21.0), Some(21.0)]);
}

/// A user method keeps its type: an array's method of the same name stays the builtin.
#[test]
fn user_method_does_not_shadow_array_method() {
    let src = v6("type P\n    float v = 0\n    int[] xs\nmethod set(P this, float x) =>\n    this.v := x\nvar p = P.new(0, array.new<int>(2, 0))\np.set(close)\np.xs.set(0, 7)\nxs = p.xs\nxs.set(1, 9)\nplot(p.v)\nplot(p.xs.get(0) + p.xs.get(1))");
    let out = run_src(&src, bars_close(&[3.0]));
    assert_eq!(out.plots[0].values, vec![Some(3.0)]);
    assert_eq!(out.plots[1].values, vec![Some(16.0)]);
}
