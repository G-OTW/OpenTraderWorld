//! `input.*`: the host's value for the input's key, else the script's default. A value of the
//! wrong type, outside `minval`/`maxval` or not among `options` refuses the run (never clamped).

use super::{bar_value, bar_var, Native, NativeFn};
use crate::diag::RtCode;
use crate::input::InputValue;
use crate::output::InputKind;
use crate::vm::value::Value;
use crate::vm::Ctx;

type Fail = (RtCode, Vec<String>);

/// Parameter positions shared by the `input.*` signatures in the registry.
struct Layout {
    min: Option<usize>,
    max: Option<usize>,
    options: Option<usize>,
}

fn layout(kind: InputKind) -> Layout {
    match kind {
        InputKind::Int | InputKind::Float => Layout { min: Some(2), max: Some(3), options: Some(5) },
        InputKind::Str | InputKind::Session => Layout { min: None, max: None, options: Some(2) },
        _ => Layout { min: None, max: None, options: None },
    }
}

fn kind_of(v: &Value) -> InputKind {
    match v {
        Value::Int(_) => InputKind::Int,
        Value::Bool(_) => InputKind::Bool,
        Value::Str(_) => InputKind::Str,
        Value::Color(_) => InputKind::Color,
        _ => InputKind::Float,
    }
}

fn host_value(ctx: &Ctx, kind: InputKind, key: &str, given: &InputValue) -> Result<Value, Fail> {
    let bad = || (RtCode::InputType, vec![key.to_string()]);
    Ok(match (kind, given) {
        (InputKind::Int, InputValue::Int(i)) => Value::Int(*i),
        (InputKind::Float, InputValue::Int(i)) => Value::Float(*i as f64),
        (InputKind::Float, InputValue::Float(f)) => Value::float(*f),
        (InputKind::Bool, InputValue::Bool(b)) => Value::Bool(*b),
        (InputKind::Str | InputKind::Session, InputValue::Str(s)) => Value::str(s),
        (InputKind::Color, InputValue::Color(c)) => Value::Color(*c),
        (InputKind::Source, InputValue::Source(name)) => {
            let var = bar_var(name).filter(|_| matches!(name.as_str(), "open" | "high" | "low" | "close" | "volume" | "hl2" | "hlc3" | "ohlc4" | "hlcc4")).ok_or_else(bad)?;
            bar_value(ctx, var, 0)?
        }
        _ => return Err(bad()),
    })
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
    }
}

/// Shared body: `a` = the call's arguments in registry order, then the key.
fn input(ctx: &mut Ctx, a: &[Value], kind: Option<InputKind>) -> Result<Value, Fail> {
    let key = match a.last() {
        Some(Value::Str(k)) => k.to_string(),
        _ => String::new(),
    };
    let kind = kind.unwrap_or_else(|| kind_of(&a[0]));
    let Some(given) = ctx.input.inputs.get(&key) else { return Ok(a[0].clone()) };
    let v = host_value(ctx, kind, &key, given)?;
    let l = layout(kind);
    let out = || (RtCode::InputOutOfRange, vec![key.clone(), format!("{given:?}")]);
    if let (Some(x), Some(lo)) = (v.num(), l.min.and_then(|i| a.get(i)).and_then(Value::num)) {
        if x < lo {
            return Err(out());
        }
    }
    if let (Some(x), Some(hi)) = (v.num(), l.max.and_then(|i| a.get(i)).and_then(Value::num)) {
        if x > hi {
            return Err(out());
        }
    }
    if let Some(Value::Tuple(opts)) = l.options.and_then(|i| a.get(i)) {
        if !opts.iter().any(|o| same(o, &v)) {
            return Err(out());
        }
    }
    Ok(v)
}

pub fn native(name: &str) -> Option<Native> {
    let s = |f: NativeFn| Some(Native { f, init: None });
    match name {
        "input" => s(|c, a, _| input(c, a, None)),
        "input.int" => s(|c, a, _| input(c, a, Some(InputKind::Int))),
        "input.float" => s(|c, a, _| input(c, a, Some(InputKind::Float))),
        "input.bool" => s(|c, a, _| input(c, a, Some(InputKind::Bool))),
        "input.string" => s(|c, a, _| input(c, a, Some(InputKind::Str))),
        "input.source" => s(|c, a, _| input(c, a, Some(InputKind::Source))),
        "input.color" => s(|c, a, _| input(c, a, Some(InputKind::Color))),
        "input.session" => s(|c, a, _| input(c, a, Some(InputKind::Session))),
        _ => None,
    }
}
