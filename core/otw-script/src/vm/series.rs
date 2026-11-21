//! A value with history: a ring buffer whose slot 0 is the current execution.

use super::value::Value;

#[derive(Debug, Clone)]
pub struct Series {
    buf: Vec<Value>,
    head: usize,
    len: usize,
}

impl Series {
    pub fn new(cap: usize) -> Self {
        Self { buf: vec![Value::Na; cap.max(1)], head: 0, len: 0 }
    }

    /// Keeps no history (only the current execution).
    pub fn is_scalar(&self) -> bool {
        self.buf.len() == 1
    }

    /// Start a new execution with `v` as its value.
    pub fn push(&mut self, v: Value) {
        self.head += 1;
        if self.head == self.buf.len() {
            self.head = 0;
        }
        self.buf[self.head] = v;
        if self.len < self.buf.len() {
            self.len += 1;
        }
    }

    /// Overwrite the current execution's value.
    pub fn set(&mut self, v: Value) {
        if self.len == 0 {
            self.push(v);
        } else {
            self.buf[self.head] = v;
        }
    }

    /// The value `k` executions back (`Na` beyond the kept history).
    pub fn get(&self, k: usize) -> Value {
        if k >= self.len {
            return Value::Na;
        }
        let i = if k <= self.head { self.head - k } else { self.head + self.buf.len() - k };
        self.buf[i].clone()
    }

    /// Every stored value, current and past (the collector's roots).
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.buf.iter()
    }

    pub fn current(&self) -> Value {
        if self.len == 0 {
            Value::Na
        } else {
            self.buf[self.head].clone()
        }
    }
}
