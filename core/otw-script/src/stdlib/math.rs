//! `math.*`, `na`/`nz`/`fixnan`, casts. Transcendental functions go through `libm` so native
//! and WASM agree bit for bit.

use std::collections::VecDeque;

use super::{Native, NativeFn, NResult, State};
use crate::diag::RtCode;
use crate::vm::value::Value;

fn num(v: &Value) -> Option<f64> {
    v.num()
}

fn unary(a: &[Value], f: fn(f64) -> f64) -> NResult {
    Ok(match num(&a[0]) {
        Some(x) => Value::float(f(x)),
        None => Value::Na,
    })
}

/// Half away from zero, like Pine's `math.round`.
fn round(x: f64) -> f64 {
    libm::round(x)
}

fn extreme(a: &[Value], max: bool) -> NResult {
    if a.iter().any(|v| v.num().is_none()) {
        return Ok(Value::Na);
    }
    let all_int = a.iter().all(|v| matches!(v, Value::Int(_)));
    let it = a.iter().filter_map(Value::num);
    let r = if max { it.fold(f64::NEG_INFINITY, f64::max) } else { it.fold(f64::INFINITY, f64::min) };
    Ok(if all_int { Value::Int(r as i64) } else { Value::float(r) })
}

/// Rolling window of the last `n` values of a call site (`math.sum`).
#[derive(Clone)]
struct Window {
    buf: VecDeque<Option<f64>>,
}

fn window(st: Option<&mut State>) -> &mut Window {
    super::state_mut::<Window>(st)
}

fn sum(ctx_len: Option<i64>, st: Option<&mut State>, v: Option<f64>) -> Value {
    let w = window(st);
    let Some(n) = ctx_len.filter(|n| *n > 0) else { return Value::Na };
    w.buf.push_back(v);
    while w.buf.len() > n as usize {
        w.buf.pop_front();
    }
    if w.buf.len() < n as usize || w.buf.iter().any(Option::is_none) {
        return Value::Na;
    }
    Value::float(w.buf.iter().map(|x| x.unwrap_or(0.0)).sum())
}

#[derive(Clone)]
struct LastValid(Value);

pub fn native(name: &str) -> Option<Native> {
    let s = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "na" => s(|_, a, _| Ok(Value::Bool(a[0].is_na()))),
        "nz" => s(|_, a, _| Ok(if a[0].is_na() { a[1].clone() } else { a[0].clone() })),
        "fixnan" => Some(Native {
            f: |_, a, st| {
                let last = super::state_mut::<LastValid>(st);
                if !a[0].is_na() {
                    last.0 = a[0].clone();
                }
                Ok(last.0.clone())
            },
            init: Some(|| Box::new(LastValid(Value::Na))),
        }),
        "int" => s(|_, a, _| Ok(a[0].int().map(Value::Int).unwrap_or(Value::Na))),
        "float" => s(|_, a, _| Ok(num(&a[0]).map(Value::float).unwrap_or(Value::Na))),
        "bool" => s(|_, a, _| Ok(Value::Bool(a[0].truthy()))),
        "color" | "string" => s(|_, a, _| Ok(a[0].clone())),
        "math.abs" => s(|_, a, _| {
            Ok(match &a[0] {
                Value::Int(i) => Value::Int(i.wrapping_abs()),
                v => v.num().map(|x| Value::float(x.abs())).unwrap_or(Value::Na),
            })
        }),
        "math.acos" => s(|_, a, _| unary(a, libm::acos)),
        "math.asin" => s(|_, a, _| unary(a, libm::asin)),
        "math.atan" => s(|_, a, _| unary(a, libm::atan)),
        "math.cos" => s(|_, a, _| unary(a, libm::cos)),
        "math.sin" => s(|_, a, _| unary(a, libm::sin)),
        "math.tan" => s(|_, a, _| unary(a, libm::tan)),
        "math.exp" => s(|_, a, _| unary(a, libm::exp)),
        "math.log" => s(|_, a, _| unary(a, libm::log)),
        "math.log10" => s(|_, a, _| unary(a, libm::log10)),
        "math.sqrt" => s(|_, a, _| unary(a, libm::sqrt)),
        "math.todegrees" => s(|_, a, _| unary(a, |x| x * 180.0 / std::f64::consts::PI)),
        "math.toradians" => s(|_, a, _| unary(a, |x| x * std::f64::consts::PI / 180.0)),
        "math.sign" => s(|_, a, _| {
            unary(a, |x| {
                if x > 0.0 {
                    1.0
                } else if x < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            })
        }),
        "math.pow" => s(|_, a, _| {
            Ok(match (num(&a[0]), num(&a[1])) {
                (Some(b), Some(e)) => Value::float(libm::pow(b, e)),
                _ => Value::Na,
            })
        }),
        "math.ceil" => s(|_, a, _| Ok(num(&a[0]).map(|x| Value::Int(libm::ceil(x) as i64)).unwrap_or(Value::Na))),
        "math.floor" => s(|_, a, _| Ok(num(&a[0]).map(|x| Value::Int(libm::floor(x) as i64)).unwrap_or(Value::Na))),
        "math.round" => s(|_, a, _| {
            let Some(x) = num(&a[0]) else { return Ok(Value::Na) };
            Ok(match a[1].int() {
                Some(p) => {
                    let m = libm::pow(10.0, p as f64);
                    Value::float(round(x * m) / m)
                }
                None => Value::Int(round(x) as i64),
            })
        }),
        "math.round_to_mintick" => s(|ctx, a, _| {
            let Some(tick) = ctx.input.syminfo.mintick else { return Err((RtCode::SyminfoUnknown, vec!["mintick".into()])) };
            Ok(match num(&a[0]) {
                Some(x) if tick > 0.0 => Value::float(round(x / tick) * tick),
                _ => Value::Na,
            })
        }),
        "math.max" => s(|_, a, _| extreme(a, true)),
        "math.min" => s(|_, a, _| extreme(a, false)),
        "math.avg" => s(|_, a, _| {
            if a.iter().any(|v| v.num().is_none()) {
                return Ok(Value::Na);
            }
            Ok(Value::float(a.iter().filter_map(Value::num).sum::<f64>() / a.len() as f64))
        }),
        "math.sum" => Some(Native {
            f: |ctx, a, st| {
                if a[1].int().is_some_and(|n| n > 0 && n as usize > ctx.max_history) {
                    return Err((RtCode::HistoryTooDeep, vec![a[1].int().unwrap_or(0).to_string()]));
                }
                Ok(sum(a[1].int(), st, a[0].num()))
            },
            init: Some(|| Box::new(Window { buf: VecDeque::new() })),
        }),
        "runtime.error" => s(|_, a, _| {
            let msg = match &a[0] {
                Value::Str(s) => s.to_string(),
                _ => String::new(),
            };
            Err((RtCode::RuntimeErrorCall, vec![msg]))
        }),
        _ => None,
    }
}

pub fn constant(name: &str) -> Option<Value> {
    Some(Value::Float(match name {
        "math.pi" => std::f64::consts::PI,
        "math.e" => std::f64::consts::E,
        "math.phi" => std::f64::consts::GOLDEN_RATIO,
        "math.rphi" => 0.618_033_988_749_895,
        _ => return None,
    }))
}
