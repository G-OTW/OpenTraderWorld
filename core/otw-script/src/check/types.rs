//! Types as the checker sees them, argument binding, and branch unification. The checker stays
//! permissive (qualifiers are not enforced): it refuses only what Pine refuses and would change
//! a value or crash a run.

use crate::ast::Arg;
use crate::diag::DiagCode;
use crate::registry::{Builtin, Elt, Param, RTy};

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Color,
    Str,
    Na,
    Tuple(Vec<Ty>),
    Void,
    PlotRef,
    HLineRef,
    /// A drawing (`label`, `line`, `box`, `table`, `linefill`, `polyline`).
    Drawing(String),
    /// `array<T>` (`Unknown` element when not known statically).
    Array(Box<Ty>),
    /// An instance of a user-defined type, by type name.
    Object(String),
    /// Not known statically (user function results, control blocks): compatible with anything.
    Unknown,
}

/// Common type of two branches, `None` when they cannot meet (`1` vs `"a"`).
pub fn unify(a: &Ty, b: &Ty) -> Option<Ty> {
    use Ty::*;
    Some(match (a, b) {
        (x, y) if x == y => x.clone(),
        (Na, x) | (x, Na) => x.clone(),
        (Unknown, _) | (_, Unknown) => Unknown,
        (Int, Float) | (Float, Int) => Float,
        (Tuple(x), Tuple(y)) if x.len() == y.len() => Tuple(x.iter().zip(y).map(|(a, b)| unify(a, b)).collect::<Option<Vec<_>>>()?),
        (Array(x), Array(y)) => Array(Box::new(unify(x, y)?)),
        _ => return None,
    })
}

/// Can a value of type `value` be stored in a variable declared `declared` (v6 when `v6`)?
pub fn assignable(declared: &Ty, value: &Ty, v6: bool) -> bool {
    use Ty::*;
    match (declared, value) {
        (_, Unknown) | (Unknown, _) => true,
        (Bool, Na) => !v6,
        (_, Na) => true,
        // v5 casts numbers to bool implicitly; v6 does not.
        (Bool, Int | Float) => !v6,
        (Float, Int) => true,
        (Array(d), Array(v)) => assignable(d, v, v6) || **v == Na,
        (d, v) => d == v,
    }
}

/// Named Pine types to checker types (`None` = not a basic type).
pub fn named(t: &str) -> Option<Ty> {
    Some(match t {
        "int" => Ty::Int,
        "float" => Ty::Float,
        "bool" => Ty::Bool,
        "color" => Ty::Color,
        "string" => Ty::Str,
        _ => return None,
    })
}

/// Which argument fills each parameter. `slots[i]` = index in `args` for `params[i]`;
/// `extra` = trailing positional arguments of a variadic builtin.
#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    pub slots: Vec<Option<usize>>,
    pub extra: Vec<usize>,
}

/// Bind call arguments to parameters. Positional arguments fill the required parameters in
/// order when there are no more of them than required parameters (`ta.highest(10)` = length),
/// otherwise every parameter in declaration order. Named arguments fill by name.
pub fn bind_args(params: &[Param], variadic: bool, args: &[Arg]) -> Result<Bound, (DiagCode, String)> {
    let mut slots = vec![None; params.len()];
    let mut extra = Vec::new();
    let positional: Vec<usize> = (0..args.len()).filter(|&i| args[i].name.is_none()).collect();
    let required: Vec<usize> = (0..params.len()).filter(|&i| params[i].default.is_none()).collect();
    let order: Vec<usize> = if positional.len() <= required.len() { required.clone() } else { (0..params.len()).collect() };
    for (k, &ai) in positional.iter().enumerate() {
        match order.get(k) {
            Some(&pi) => slots[pi] = Some(ai),
            None if variadic => extra.push(ai),
            None => return Err((DiagCode::ArgCount, format!("{} > {}", positional.len(), params.len()))),
        }
    }
    for (ai, a) in args.iter().enumerate() {
        if let Some(n) = &a.name {
            match params.iter().position(|p| p.name == n) {
                Some(pi) => slots[pi] = Some(ai),
                None => return Err((DiagCode::ArgName, n.clone())),
            }
        }
    }
    if let Some(&pi) = required.iter().find(|&&pi| slots[pi].is_none()) {
        return Err((DiagCode::ArgCount, params[pi].name.to_string()));
    }
    Ok(Bound { slots, extra })
}

/// The result type of a builtin call given the types of its bound arguments.
pub fn ret_of(b: &Builtin, bound: &Bound, arg_tys: &[Ty]) -> Ty {
    let arg = |pi: usize| bound.slots.get(pi).copied().flatten().map(|ai| arg_tys[ai].clone());
    match b.ret {
        RTy::Float => Ty::Float,
        RTy::Int => Ty::Int,
        RTy::Bool => Ty::Bool,
        RTy::Color => Ty::Color,
        RTy::Str => Ty::Str,
        RTy::Void => Ty::Void,
        RTy::Tuple3 => Ty::Tuple(vec![Ty::Float; 3]),
        RTy::FloatInt => Ty::Tuple(vec![Ty::Float, Ty::Int]),
        RTy::Array(elt) => Ty::Array(Box::new(match elt {
            Elt::Int => Ty::Int,
            Elt::Float => Ty::Float,
            Elt::Bool => Ty::Bool,
            Elt::Str => Ty::Str,
            Elt::Color => Ty::Color,
            Elt::Arg0 => match arg(0) {
                Some(Ty::Na) | None => Ty::Unknown,
                Some(t) => t,
            },
            Elt::Same => return arg(0).filter(|t| matches!(t, Ty::Array(_))).unwrap_or(Ty::Array(Box::new(Ty::Unknown))),
            Elt::Generic => Ty::Unknown,
            Elt::Draw(k) => Ty::Drawing(k.to_string()),
        })),
        RTy::Draw("chart.point") => Ty::Object("chart.point".to_string()),
        RTy::Draw(k) => Ty::Drawing(k.to_string()),
        RTy::Elem => match arg(0) {
            Some(Ty::Array(e)) => *e,
            _ => Ty::Unknown,
        },
        RTy::PlotRef => Ty::PlotRef,
        RTy::HLineRef => Ty::HLineRef,
        RTy::Arg0 => match arg(0) {
            Some(Ty::Na) | None => Ty::Unknown,
            Some(t) => t,
        },
        RTy::Arg1 => match arg(1) {
            Some(Ty::Na) | None => Ty::Unknown,
            Some(t) => t,
        },
        RTy::Vwap => {
            if arg(2).is_some() {
                Ty::Tuple(vec![Ty::Float; 3])
            } else {
                Ty::Float
            }
        }
        RTy::Round => {
            if arg(1).is_some() {
                Ty::Float
            } else {
                Ty::Int
            }
        }
    }
}
