//! What a host hands a compiled script to run.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::output::Rgba;

/// OHLCV aligned by index. Times are epoch milliseconds UTC (bar open / bar close).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Bars {
    pub time: Vec<i64>,
    pub time_close: Vec<i64>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

impl Bars {
    pub fn len(&self) -> usize {
        self.close.len()
    }
    pub fn is_empty(&self) -> bool {
        self.close.is_empty()
    }
}

/// Instrument facts. `None` = unknown: a script reading it is refused, never defaulted.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SymInfo {
    pub ticker: String,
    pub tickerid: String,
    pub mintick: Option<f64>,
    pub pointvalue: Option<f64>,
    pub timezone: Option<String>,
    pub session: Option<String>,
    pub kind: Option<String>,
    pub currency: Option<String>,
    pub basecurrency: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TfUnit {
    Second,
    Minute,
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeframe {
    pub multiplier: u32,
    pub unit: TfUnit,
}

impl Timeframe {
    /// An OTW timeframe string: `30s`, `1m`, `4h`, `1d`, `1w`, `1M` or `1mo`. Hours become
    /// minutes (`4h` = 240 minutes), as Pine counts them. `m` is minutes, `M` months.
    pub fn parse(s: &str) -> Option<Timeframe> {
        let split = s.find(|c: char| !c.is_ascii_digit())?;
        let (num, unit) = s.split_at(split);
        let n: u32 = num.parse().ok().filter(|n| *n > 0)?;
        let (multiplier, unit) = match unit {
            "s" => (n, TfUnit::Second),
            "m" => (n, TfUnit::Minute),
            "h" => (n.checked_mul(60)?, TfUnit::Minute),
            "d" => (n, TfUnit::Day),
            "w" => (n, TfUnit::Week),
            "M" | "mo" => (n, TfUnit::Month),
            _ => return None,
        };
        Some(Timeframe { multiplier, unit })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// A price series name: open high low close volume hl2 hlc3 ohlc4 hlcc4.
    Source(String),
    Color(Rgba),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_ops: u64,
    pub max_loop_iters_per_bar: u32,
    pub max_history: usize,
    pub max_string_len: usize,
    /// Native only; the WASM host relies on `max_ops`.
    pub timeout_ms: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_ops: 500_000_000,
            max_loop_iters_per_bar: 100_000,
            max_history: 5000,
            max_string_len: 4096,
            timeout_ms: Some(10_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunInput {
    pub bars: Bars,
    pub syminfo: SymInfo,
    pub timeframe: Timeframe,
    /// Values by `InputMeta::key`; missing keys take the script's default.
    pub inputs: BTreeMap<String, InputValue>,
    pub limits: Limits,
}
