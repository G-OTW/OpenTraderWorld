//! Runtime values. NaN and ±inf never survive: they become `Na`.

use std::rc::Rc;

pub use crate::output::Rgba;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Na,
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(Rgba),
    Str(Rc<str>),
    Tuple(Rc<[Value]>),
    PlotRef(usize),
    HLineRef(usize),
    /// An object on the run's heap (`vm::heap`): an array.
    Ref(u32),
}

impl Value {
    /// A float, `Na` when not finite.
    pub fn float(v: f64) -> Value {
        if v.is_finite() {
            Value::Float(v)
        } else {
            Value::Na
        }
    }

    pub fn is_na(&self) -> bool {
        matches!(self, Value::Na)
    }

    /// Numeric view: ints and floats (bools are not numbers here).
    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.is_finite() => Some(f.trunc() as i64),
            _ => None,
        }
    }

    /// Condition value: `na` is false; v5 numbers are true when non-zero.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            _ => false,
        }
    }

    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }
}
