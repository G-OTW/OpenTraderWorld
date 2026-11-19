//! `array.*`. Arrays are reference objects on the run's heap (`vm::heap`): `b = a` shares the
//! array, `array.copy` makes a new one. Aggregates skip `na` elements; an index outside the
//! array, an element asked of an empty one or an `na` array stops the run, as in Pine. v6
//! negative indices count from the end.

use super::{Native, NativeFn, NResult};
use crate::diag::RtCode;
use crate::vm::heap::{weight, Array, Elem, Obj, MAX_ARRAY_LEN};
use crate::vm::value::Value;
use crate::vm::Ctx;

type E = (RtCode, Vec<String>);

fn na_array() -> E {
    (RtCode::ArrayNa, Vec::new())
}

fn get<'c>(ctx: &'c Ctx, v: &Value) -> Result<&'c Array, E> {
    match v {
        Value::Ref(id) => ctx.heap.array(*id).ok_or_else(na_array),
        _ => Err(na_array()),
    }
}

fn get_mut<'c>(ctx: &'c mut Ctx, v: &Value) -> Result<&'c mut Array, E> {
    match v {
        Value::Ref(id) => ctx.heap.array_mut(*id).ok_or_else(na_array),
        _ => Err(na_array()),
    }
}

fn too_long() -> E {
    (RtCode::MemoryLimit, vec![MAX_ARRAY_LEN.to_string()])
}

/// Room for `v` (or `n` elements) in the heap's cell budget.
fn room(ctx: &mut Ctx, n: usize) -> Result<(), E> {
    if ctx.heap.grow(n) {
        Ok(())
    } else {
        Err((RtCode::MemoryLimit, vec![crate::vm::heap::MAX_CELLS.to_string()]))
    }
}

/// A new array on the heap.
pub fn alloc(ctx: &mut Ctx, elem: Elem, items: Vec<Value>) -> NResult {
    if items.len() > MAX_ARRAY_LEN {
        return Err(too_long());
    }
    ctx.heap.alloc(Obj::Array(Array { elem, items })).ok_or_else(|| (RtCode::MemoryLimit, vec![crate::vm::heap::MAX_OBJECTS.to_string()]))
}

/// The value as the array stores it: ints widen in a float array.
fn coerce(elem: Elem, v: &Value) -> Value {
    match (elem, v) {
        (Elem::Float, Value::Int(i)) => Value::Float(*i as f64),
        _ => v.clone(),
    }
}

/// Index `i` of an array of `n` (negative from the end), or the out-of-bounds error.
fn index(i: &Value, n: usize, allow_end: bool) -> Result<usize, E> {
    let Some(k) = i.int() else { return Err((RtCode::ArrayIndex, vec!["na".into(), n.to_string()])) };
    let r = if k < 0 { k + n as i64 } else { k };
    let limit = if allow_end { n as i64 } else { n as i64 - 1 };
    if r < 0 || r > limit {
        return Err((RtCode::ArrayIndex, vec![k.to_string(), n.to_string()]));
    }
    Ok(r as usize)
}

fn empty(f: &str) -> E {
    (RtCode::ArrayEmpty, vec![f.to_string()])
}

fn elem_of(name: &str) -> Elem {
    match name {
        "int" => Elem::Int,
        "float" => Elem::Float,
        "bool" => Elem::Bool,
        "string" => Elem::Str,
        "color" => Elem::Color,
        _ => Elem::Any,
    }
}

fn elem_of_value(v: &Value) -> Option<Elem> {
    Some(match v {
        Value::Int(_) => Elem::Int,
        Value::Float(_) => Elem::Float,
        Value::Bool(_) => Elem::Bool,
        Value::Str(_) => Elem::Str,
        Value::Color(_) => Elem::Color,
        Value::Na => return None,
        _ => Elem::Any,
    })
}

/// Numbers of the array, `na` skipped.
fn nums(a: &Array) -> Vec<f64> {
    a.items.iter().filter_map(Value::num).collect()
}

/// A number typed like the array's elements (int arrays give ints).
fn typed(a: &Array, x: f64) -> Value {
    if a.elem == Elem::Int && x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::Int(x as i64)
    } else {
        Value::float(x)
    }
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn variance(v: &[f64], biased: bool) -> Option<f64> {
    let m = mean(v)?;
    let d = if biased { v.len() as f64 } else { v.len() as f64 - 1.0 };
    let r = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / d;
    r.is_finite().then_some(r)
}

/// Total order for sorting: numbers, then strings, `na` last.
fn order(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (Value::Na, Value::Na) => Equal,
        (Value::Na, _) => Greater,
        (_, Value::Na) => Less,
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => Equal,
        },
    }
}

fn descending(v: &Value) -> bool {
    matches!(v, Value::Str(s) if &**s == "descending")
}

/// Pine's `str.tostring` default format (`#.##########`) for an element.
pub fn to_text(v: &Value) -> String {
    match v {
        Value::Na => "NaN".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            let s = format!("{f:.10}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s == "-0" { "0".into() } else { s.to_string() }
        }
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

fn percentile(v: &mut [f64], p: f64, linear: bool) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let p = p.clamp(0.0, 100.0) / 100.0;
    if linear {
        let pos = p * (v.len() - 1) as f64;
        let (lo, hi) = (pos.floor() as usize, (pos.ceil() as usize).min(v.len() - 1));
        Some(v[lo] + (pos - lo as f64) * (v[hi] - v[lo]))
    } else {
        let rank = (p * v.len() as f64).ceil() as usize;
        Some(v[rank.clamp(1, v.len()) - 1])
    }
}

/// `nth` largest (`max`) or smallest number, `na` past the count.
fn nth(a: &Array, n: &Value, max: bool) -> Value {
    let mut v = nums(a);
    v.sort_by(|x, y| if max { y.total_cmp(x) } else { x.total_cmp(y) });
    let k = n.int().unwrap_or(0).max(0) as usize;
    v.get(k).map(|x| typed(a, *x)).unwrap_or(Value::Na)
}

fn binary_search(a: &Array, x: &Value) -> (usize, usize) {
    let lo = a.items.partition_point(|v| order(v, x) == std::cmp::Ordering::Less);
    let hi = a.items.partition_point(|v| order(v, x) != std::cmp::Ordering::Greater);
    (lo, hi)
}

fn new_typed(elem: Elem) -> NativeFn {
    match elem {
        Elem::Int => |ctx, a, _| make(ctx, Elem::Int, a),
        Elem::Float => |ctx, a, _| make(ctx, Elem::Float, a),
        Elem::Bool => |ctx, a, _| make(ctx, Elem::Bool, a),
        Elem::Str => |ctx, a, _| make(ctx, Elem::Str, a),
        Elem::Color => |ctx, a, _| make(ctx, Elem::Color, a),
        // `array.new<T>`: the compiler appends T's name.
        Elem::Any => |ctx, a, _| make(ctx, elem_of(match a.get(2) { Some(Value::Str(s)) => s, _ => "" }), a),
    }
}

/// `array.new_*(size, initial_value)`.
fn make(ctx: &mut Ctx, elem: Elem, a: &[Value]) -> NResult {
    let n = a[0].int().unwrap_or(0).max(0) as usize;
    if n > MAX_ARRAY_LEN {
        return Err(too_long());
    }
    let init = coerce(elem, &a[1]);
    alloc(ctx, elem, vec![init; n])
}

pub fn native(name: &str) -> Option<Native> {
    let s = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "array.new_float" => s(new_typed(Elem::Float)),
        "array.new_int" => s(new_typed(Elem::Int)),
        "array.new_bool" => s(new_typed(Elem::Bool)),
        "array.new_string" => s(new_typed(Elem::Str)),
        "array.new_color" => s(new_typed(Elem::Color)),
        "array.new" => s(new_typed(Elem::Any)),
        "array.new_line" | "array.new_label" | "array.new_box" | "array.new_table" | "array.new_linefill" => s(|ctx, a, _| make(ctx, Elem::Any, a)),
        "array.from" => s(|ctx, a, _| {
            let mut elem = a.iter().find_map(elem_of_value).unwrap_or(Elem::Any);
            if elem == Elem::Int && a.iter().any(|v| matches!(v, Value::Float(_))) {
                elem = Elem::Float;
            }
            let items = a.iter().map(|v| coerce(elem, v)).collect();
            alloc(ctx, elem, items)
        }),
        "array.size" => s(|ctx, a, _| Ok(Value::Int(get(ctx, &a[0])?.items.len() as i64))),
        "array.get" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            Ok(arr.items[index(&a[1], arr.items.len(), false)?].clone())
        }),
        "array.set" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let i = index(&a[1], arr.items.len(), false)?;
            let v = coerce(arr.elem, &a[2]);
            room(ctx, weight(&v))?;
            get_mut(ctx, &a[0])?.items[i] = v;
            Ok(Value::Na)
        }),
        "array.first" => s(|ctx, a, _| get(ctx, &a[0])?.items.first().cloned().ok_or_else(|| empty("array.first"))),
        "array.last" => s(|ctx, a, _| get(ctx, &a[0])?.items.last().cloned().ok_or_else(|| empty("array.last"))),
        "array.push" => s(|ctx, a, _| {
            let arr = get_mut(ctx, &a[0])?;
            if arr.items.len() >= MAX_ARRAY_LEN {
                return Err(too_long());
            }
            let v = coerce(arr.elem, &a[1]);
            room(ctx, weight(&v))?;
            get_mut(ctx, &a[0])?.items.push(v);
            Ok(Value::Na)
        }),
        "array.unshift" => s(|ctx, a, _| {
            let arr = get_mut(ctx, &a[0])?;
            if arr.items.len() >= MAX_ARRAY_LEN {
                return Err(too_long());
            }
            let v = coerce(arr.elem, &a[1]);
            room(ctx, weight(&v))?;
            get_mut(ctx, &a[0])?.items.insert(0, v);
            Ok(Value::Na)
        }),
        "array.insert" => s(|ctx, a, _| {
            let arr = get_mut(ctx, &a[0])?;
            if arr.items.len() >= MAX_ARRAY_LEN {
                return Err(too_long());
            }
            let i = index(&a[1], arr.items.len(), true)?;
            let v = coerce(arr.elem, &a[2]);
            room(ctx, weight(&v))?;
            get_mut(ctx, &a[0])?.items.insert(i, v);
            Ok(Value::Na)
        }),
        "array.pop" => s(|ctx, a, _| get_mut(ctx, &a[0])?.items.pop().ok_or_else(|| empty("array.pop"))),
        "array.shift" => s(|ctx, a, _| {
            let arr = get_mut(ctx, &a[0])?;
            if arr.items.is_empty() {
                return Err(empty("array.shift"));
            }
            Ok(arr.items.remove(0))
        }),
        "array.remove" => s(|ctx, a, _| {
            let arr = get_mut(ctx, &a[0])?;
            let i = index(&a[1], arr.items.len(), false)?;
            Ok(arr.items.remove(i))
        }),
        "array.clear" => s(|ctx, a, _| {
            get_mut(ctx, &a[0])?.items.clear();
            Ok(Value::Na)
        }),
        "array.fill" => s(|ctx, a, _| {
            let arr = get_mut(ctx, &a[0])?;
            let n = arr.items.len() as i64;
            let from = a[2].int().unwrap_or(0).clamp(0, n) as usize;
            let to = a[3].int().unwrap_or(n).clamp(0, n) as usize;
            let v = coerce(arr.elem, &a[1]);
            for x in arr.items.iter_mut().take(to).skip(from) {
                *x = v.clone();
            }
            Ok(Value::Na)
        }),
        "array.reverse" => s(|ctx, a, _| {
            get_mut(ctx, &a[0])?.items.reverse();
            Ok(Value::Na)
        }),
        "array.sort" => s(|ctx, a, _| {
            let desc = descending(&a[1]);
            let arr = get_mut(ctx, &a[0])?;
            arr.items.sort_by(order);
            if desc {
                // `na` stays last.
                let k = arr.items.iter().take_while(|v| !v.is_na()).count();
                arr.items[..k].reverse();
            }
            Ok(Value::Na)
        }),
        "array.sort_indices" => s(|ctx, a, _| {
            let desc = descending(&a[1]);
            let arr = get(ctx, &a[0])?;
            let mut idx: Vec<usize> = (0..arr.items.len()).collect();
            idx.sort_by(|&i, &j| {
                let o = order(&arr.items[i], &arr.items[j]);
                if desc && !arr.items[i].is_na() && !arr.items[j].is_na() { o.reverse() } else { o }
            });
            let items = idx.into_iter().map(|i| Value::Int(i as i64)).collect();
            alloc(ctx, Elem::Int, items)
        }),
        "array.copy" => s(|ctx, a, _| {
            // `o.copy()` on an object of a user-defined type reaches here too.
            if let Value::Ref(id) = &a[0] {
                if let Some(o) = ctx.heap.object(*id).cloned() {
                    return ctx.heap.alloc(Obj::Object(o)).ok_or_else(|| (RtCode::MemoryLimit, vec![crate::vm::heap::MAX_OBJECTS.to_string()]));
                }
            }
            let arr = get(ctx, &a[0])?.clone();
            alloc(ctx, arr.elem, arr.items)
        }),
        "array.slice" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let n = arr.items.len();
            let from = index(&a[1], n, true)?;
            let to = index(&a[2], n, true)?;
            if to < from {
                return Err((RtCode::ArrayIndex, vec![to.to_string(), n.to_string()]));
            }
            let items = arr.items[from..to].to_vec();
            let elem = arr.elem;
            alloc(ctx, elem, items)
        }),
        "array.concat" => s(|ctx, a, _| {
            let add = get(ctx, &a[1])?.items.clone();
            if get(ctx, &a[0])?.items.len() + add.len() > MAX_ARRAY_LEN {
                return Err(too_long());
            }
            room(ctx, add.iter().map(weight).sum())?;
            let arr = get_mut(ctx, &a[0])?;
            let elem = arr.elem;
            arr.items.extend(add.iter().map(|v| coerce(elem, v)));
            Ok(a[0].clone())
        }),
        "array.abs" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let elem = arr.elem;
            let items = arr
                .items
                .iter()
                .map(|v| match v {
                    Value::Int(i) => Value::Int(i.wrapping_abs()),
                    v => v.num().map(|x| Value::float(x.abs())).unwrap_or(Value::Na),
                })
                .collect();
            alloc(ctx, elem, items)
        }),
        "array.includes" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            Ok(Value::Bool(arr.items.iter().any(|v| crate::vm::binary(crate::ast::BinOp::Eq, v, &a[1], false).truthy())))
        }),
        "array.indexof" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let i = arr.items.iter().position(|v| crate::vm::binary(crate::ast::BinOp::Eq, v, &a[1], false).truthy());
            Ok(Value::Int(i.map(|i| i as i64).unwrap_or(-1)))
        }),
        "array.lastindexof" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let i = arr.items.iter().rposition(|v| crate::vm::binary(crate::ast::BinOp::Eq, v, &a[1], false).truthy());
            Ok(Value::Int(i.map(|i| i as i64).unwrap_or(-1)))
        }),
        "array.binary_search" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let (lo, hi) = binary_search(arr, &a[1]);
            Ok(Value::Int(if lo < hi { lo as i64 } else { -1 }))
        }),
        "array.binary_search_leftmost" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let (lo, hi) = binary_search(arr, &a[1]);
            Ok(Value::Int(if lo < hi { lo as i64 } else { lo as i64 - 1 }))
        }),
        "array.binary_search_rightmost" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let (lo, hi) = binary_search(arr, &a[1]);
            Ok(Value::Int(if lo < hi { hi as i64 - 1 } else { hi as i64 }))
        }),
        "array.sum" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let v = nums(arr);
            Ok(if v.is_empty() { Value::Na } else { typed(arr, v.iter().sum()) })
        }),
        "array.avg" => s(|ctx, a, _| Ok(mean(&nums(get(ctx, &a[0])?)).map(Value::float).unwrap_or(Value::Na))),
        "array.max" => s(|ctx, a, _| Ok(nth(get(ctx, &a[0])?, &a[1], true))),
        "array.min" => s(|ctx, a, _| Ok(nth(get(ctx, &a[0])?, &a[1], false))),
        "array.range" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let v = nums(arr);
            let (lo, hi) = (v.iter().copied().fold(f64::INFINITY, f64::min), v.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            Ok(if v.is_empty() { Value::Na } else { typed(arr, hi - lo) })
        }),
        "array.median" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let mut v = nums(arr);
            v.sort_by(f64::total_cmp);
            let m = v.len();
            Ok(match m {
                0 => Value::Na,
                _ if m % 2 == 1 => typed(arr, v[m / 2]),
                _ => Value::float((v[m / 2 - 1] + v[m / 2]) / 2.0),
            })
        }),
        "array.mode" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let mut v = nums(arr);
            v.sort_by(f64::total_cmp);
            // Most frequent; the smallest on a tie.
            let mut best: Option<(f64, usize)> = None;
            let mut i = 0;
            while i < v.len() {
                let j = v[i..].iter().take_while(|x| **x == v[i]).count();
                if best.is_none_or(|(_, c)| j > c) {
                    best = Some((v[i], j));
                }
                i += j;
            }
            Ok(best.map(|(x, _)| typed(arr, x)).unwrap_or(Value::Na))
        }),
        "array.stdev" => s(|ctx, a, _| {
            let biased = !matches!(a[1], Value::Bool(false));
            Ok(variance(&nums(get(ctx, &a[0])?), biased).map(|v| Value::float(libm::sqrt(v))).unwrap_or(Value::Na))
        }),
        "array.variance" => s(|ctx, a, _| {
            let biased = !matches!(a[1], Value::Bool(false));
            Ok(variance(&nums(get(ctx, &a[0])?), biased).map(Value::float).unwrap_or(Value::Na))
        }),
        "array.covariance" => s(|ctx, a, _| {
            let (x, y) = (get(ctx, &a[0])?, get(ctx, &a[1])?);
            let pairs: Vec<(f64, f64)> = x.items.iter().zip(&y.items).filter_map(|(p, q)| Some((p.num()?, q.num()?))).collect();
            let biased = !matches!(a[2], Value::Bool(false));
            if pairs.is_empty() {
                return Ok(Value::Na);
            }
            let n = pairs.len() as f64;
            let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
            let s: f64 = pairs.iter().map(|(p, q)| (p - mx) * (q - my)).sum();
            Ok(Value::float(s / if biased { n } else { n - 1.0 }))
        }),
        "array.standardize" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let v = nums(arr);
            let (m, sd) = (mean(&v), variance(&v, true).map(libm::sqrt));
            let items = arr
                .items
                .iter()
                .map(|x| match (x.num(), m, sd) {
                    (Some(x), Some(m), Some(sd)) => Value::float((x - m) / sd),
                    _ => Value::Na,
                })
                .collect();
            alloc(ctx, Elem::Float, items)
        }),
        "array.percentile_linear_interpolation" | "array.percentile_nearest_rank" => {
            let f: NativeFn = if name.ends_with("interpolation") {
                |ctx, a, _| {
                    let p = a[1].num().unwrap_or(f64::NAN);
                    Ok(percentile(&mut nums(get(ctx, &a[0])?), p, true).map(Value::float).unwrap_or(Value::Na))
                }
            } else {
                |ctx, a, _| {
                    let p = a[1].num().unwrap_or(f64::NAN);
                    Ok(percentile(&mut nums(get(ctx, &a[0])?), p, false).map(Value::float).unwrap_or(Value::Na))
                }
            };
            s(f)
        }
        "array.percentrank" => s(|ctx, a, _| {
            let arr = get(ctx, &a[0])?;
            let i = index(&a[1], arr.items.len(), false)?;
            let Some(x) = arr.items[i].num() else { return Ok(Value::Na) };
            let others = arr.items.len().saturating_sub(1);
            let le = arr.items.iter().enumerate().filter(|(j, v)| *j != i && v.num().is_some_and(|v| v <= x)).count();
            Ok(if others == 0 { Value::Na } else { Value::float(100.0 * le as f64 / others as f64) })
        }),
        "array.join" => s(|ctx, a, _| {
            let sep = match &a[1] {
                Value::Str(s) => s.to_string(),
                _ => String::new(),
            };
            let arr = get(ctx, &a[0])?;
            Ok(Value::str(&arr.items.iter().map(to_text).collect::<Vec<_>>().join(&sep)))
        }),
        "array.every" => s(|ctx, a, _| Ok(Value::Bool(get(ctx, &a[0])?.items.iter().all(Value::truthy)))),
        "array.some" => s(|ctx, a, _| Ok(Value::Bool(get(ctx, &a[0])?.items.iter().any(Value::truthy)))),
        _ => None,
    }
}
