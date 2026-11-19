//! Row transforms for the agent's `chart` tool: reshape the source before it is drawn.
//!
//! The source rarely has the shape of the chart the user asked for (trades, not "gains vs
//! losses"; bars, not "average return per weekday"). The model describes the reshaping as a
//! short list of structured steps, run server-side over every row, so the rows never reach
//! it. Deliberately not an expression language: each step is a fixed operation with typed
//! fields, checked when the call is parsed, so nothing in a stored chart is code.
//!
//! Rows are JSON objects; a field is a key, or a dot-path into the row.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Map, Value};

/// Most steps one chart takes.
pub const MAX_STEPS: usize = 12;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Keep the rows where `field` compares true against `value`.
    Filter { field: String, cmp: Cmp, value: Value },
    /// Write `fn(field)` into a new field `as`.
    Derive {
        field: String,
        #[serde(rename = "fn")]
        func: DeriveFn,
        /// Constant operand (scale, bucket width, add/sub/mul/div).
        #[serde(default)]
        arg: Option<f64>,
        /// Field operand for add/sub/mul/div, instead of `arg`.
        #[serde(default)]
        other: Option<String>,
        /// Rename outputs, e.g. {"positive": "gain", "negative": "loss"}.
        #[serde(default)]
        labels: Option<Map<String, Value>>,
        #[serde(rename = "as")]
        out: String,
    },
    /// One row per group (or one row overall without `groupby`), sorted by group key.
    Aggregate {
        #[serde(default)]
        groupby: Vec<String>,
        ops: Vec<AggOp>,
    },
    Sort {
        field: String,
        #[serde(default)]
        order: Order,
    },
    Limit { n: usize },
    /// Running total of `field` into `as` (e.g. trade PnL into an equity curve).
    Cumsum {
        field: String,
        #[serde(rename = "as")]
        out: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cmp {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    Contains,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeriveFn {
    /// "positive" | "negative" | "zero".
    Sign,
    Abs,
    Neg,
    /// value × arg.
    Scale,
    Add,
    Sub,
    Mul,
    Div,
    /// floor(value / arg) × arg: numeric buckets for a bar chart.
    Bucket,
    /// "YYYY".
    Year,
    /// "YYYY-MM".
    Month,
    /// "01 Jan" … "12 Dec": seasonality across years.
    MonthOfYear,
    /// "YYYY-MM-DD".
    Day,
    /// "1 Mon" … "7 Sun" (sorts in week order).
    Weekday,
    /// 0 … 23, UTC.
    Hour,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggOp {
    #[serde(rename = "fn")]
    pub func: AggFn,
    /// Not needed for count.
    #[serde(default)]
    pub field: Option<String>,
    #[serde(rename = "as")]
    pub out: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AggFn {
    Sum,
    Mean,
    Count,
    Min,
    Max,
    Median,
    First,
    Last,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    #[default]
    Asc,
    Desc,
}

/// Parse the `transform` argument. `Err` names the step and what was wrong with it.
pub fn parse(v: Option<&Value>) -> Result<Vec<Step>, String> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let Some(items) = v.as_array() else {
        return Err("transform must be an array of steps".into());
    };
    if items.len() > MAX_STEPS {
        return Err(format!("at most {MAX_STEPS} transform steps ({} given)", items.len()));
    }
    let steps = items
        .iter()
        .enumerate()
        .map(|(i, s)| {
            serde_json::from_value::<Step>(s.clone()).map_err(|e| format!("transform step {}: {e}", i + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (i, s) in steps.iter().enumerate() {
        match s {
            Step::Derive { func, arg, other, .. } => {
                let binary = matches!(func, DeriveFn::Add | DeriveFn::Sub | DeriveFn::Mul | DeriveFn::Div);
                let needs_arg = matches!(func, DeriveFn::Scale | DeriveFn::Bucket);
                if binary && arg.is_none() && other.is_none() {
                    return Err(format!("transform step {}: {func:?} needs \"other\" (a field) or \"arg\" (a number)", i + 1));
                }
                if needs_arg && arg.is_none() {
                    return Err(format!("transform step {}: {func:?} needs \"arg\"", i + 1));
                }
                if *func == DeriveFn::Bucket && arg.is_some_and(|a| a <= 0.0) {
                    return Err(format!("transform step {}: bucket width must be positive", i + 1));
                }
            }
            Step::Aggregate { ops, .. } => {
                if ops.is_empty() {
                    return Err(format!("transform step {}: aggregate needs at least one op", i + 1));
                }
                if let Some(op) = ops.iter().find(|o| o.func != AggFn::Count && o.field.is_none()) {
                    return Err(format!("transform step {}: {:?} needs a \"field\"", i + 1, op.func));
                }
            }
            Step::Filter { cmp: Cmp::In, value, .. } if !value.is_array() => {
                return Err(format!("transform step {}: \"in\" takes an array value", i + 1));
            }
            _ => {}
        }
    }
    Ok(steps)
}

/// A field of a row: an exact key first (keys may contain dots), else a dot-path.
pub fn field(row: &Value, f: &str) -> Option<Value> {
    if let Some(v) = row.get(f) {
        return Some(v.clone());
    }
    let segs: Vec<&str> = f.split('.').collect();
    crate::mcp::project_path(row, &segs)
}

/// The field names of a row, for error messages.
pub fn field_names(row: Option<&Value>) -> String {
    match row {
        Some(Value::Object(m)) => m.keys().take(20).cloned().collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

/// Run every step in order.
pub fn apply(rows: Vec<Value>, steps: &[Step]) -> Result<Vec<Value>, String> {
    let mut rows = rows;
    for (i, step) in steps.iter().enumerate() {
        let fields = field_names(rows.first());
        rows = run(rows, step).map_err(|e| format!("transform step {}: {e} (row fields: {fields})", i + 1))?;
    }
    Ok(rows)
}

fn missing(rows: &[Value], f: &str) -> bool {
    !rows.is_empty() && rows.iter().all(|r| field(r, f).is_none_or(|v| v.is_null()))
}

fn run(mut rows: Vec<Value>, step: &Step) -> Result<Vec<Value>, String> {
    match step {
        Step::Filter { field: f, cmp, value } => {
            if missing(&rows, f) {
                return Err(format!("no row has \"{f}\""));
            }
            rows.retain(|r| field(r, f).is_some_and(|v| compare(&v, *cmp, value)));
            Ok(rows)
        }
        Step::Derive { field: f, func, arg, other, labels, out } => {
            if missing(&rows, f) {
                return Err(format!("no row has \"{f}\""));
            }
            if let Some(o) = other {
                if missing(&rows, o) {
                    return Err(format!("no row has \"{o}\""));
                }
            }
            for r in rows.iter_mut() {
                let v = field(r, f).unwrap_or(Value::Null);
                let o = other.as_ref().and_then(|o| field(r, o)).as_ref().and_then(super::chart::num).or(*arg);
                let mut d = derive(&v, *func, o);
                if let (Some(map), Some(key)) = (labels, label_key(&d)) {
                    if let Some(l) = map.get(&key) {
                        d = l.clone();
                    }
                }
                if let Some(obj) = r.as_object_mut() {
                    obj.insert(out.clone(), d);
                }
            }
            Ok(rows)
        }
        Step::Aggregate { groupby, ops } => aggregate(&rows, groupby, ops),
        Step::Sort { field: f, order } => {
            if missing(&rows, f) {
                return Err(format!("no row has \"{f}\""));
            }
            rows.sort_by(|a, b| {
                let o = order_values(&field(a, f).unwrap_or(Value::Null), &field(b, f).unwrap_or(Value::Null));
                if *order == Order::Desc { o.reverse() } else { o }
            });
            Ok(rows)
        }
        Step::Limit { n } => {
            rows.truncate(*n);
            Ok(rows)
        }
        Step::Cumsum { field: f, out } => {
            if missing(&rows, f) {
                return Err(format!("no row has \"{f}\""));
            }
            let mut acc = 0.0;
            for r in rows.iter_mut() {
                if let Some(x) = field(r, f).as_ref().and_then(super::chart::num) {
                    acc += x;
                }
                if let Some(obj) = r.as_object_mut() {
                    obj.insert(out.clone(), json!(acc));
                }
            }
            Ok(rows)
        }
    }
}

fn label_key(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn compare(v: &Value, cmp: Cmp, target: &Value) -> bool {
    use super::chart::num;
    match cmp {
        Cmp::Eq => loose_eq(v, target),
        Cmp::Ne => !loose_eq(v, target),
        Cmp::In => target.as_array().is_some_and(|a| a.iter().any(|t| loose_eq(v, t))),
        Cmp::Contains => match (v, target) {
            (Value::String(s), Value::String(t)) => s.to_lowercase().contains(&t.to_lowercase()),
            (Value::Array(a), t) => a.iter().any(|x| loose_eq(x, t)),
            _ => false,
        },
        Cmp::Gt | Cmp::Gte | Cmp::Lt | Cmp::Lte => {
            let o = match (num(v), num(target)) {
                (Some(a), Some(b)) => a.partial_cmp(&b),
                // Dates and other strings compare as text: ISO dates order correctly.
                _ => match (v.as_str(), target.as_str()) {
                    (Some(a), Some(b)) => Some(a.cmp(b)),
                    _ => None,
                },
            };
            match o {
                Some(o) => match cmp {
                    Cmp::Gt => o == Ordering::Greater,
                    Cmp::Gte => o != Ordering::Less,
                    Cmp::Lt => o == Ordering::Less,
                    _ => o != Ordering::Greater,
                },
                None => false,
            }
        }
    }
}

/// Equal as values, or as numbers when both read as one ("12" == 12), or as text ignoring case.
fn loose_eq(a: &Value, b: &Value) -> bool {
    use super::chart::num;
    if a == b {
        return true;
    }
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        return x == y;
    }
    matches!((a, b), (Value::String(x), Value::String(y)) if x.eq_ignore_ascii_case(y))
}

fn derive(v: &Value, func: DeriveFn, operand: Option<f64>) -> Value {
    use super::chart::{num, time_ms};
    use time::OffsetDateTime;
    let n = num(v);
    let date = || time_ms(v).and_then(|ms| OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok());
    let finite = |x: f64| if x.is_finite() { json!(x) } else { Value::Null };
    match func {
        DeriveFn::Sign => match n {
            Some(x) if x > 0.0 => json!("positive"),
            Some(x) if x < 0.0 => json!("negative"),
            Some(_) => json!("zero"),
            None => Value::Null,
        },
        DeriveFn::Abs => n.map_or(Value::Null, |x| finite(x.abs())),
        DeriveFn::Neg => n.map_or(Value::Null, |x| finite(-x)),
        DeriveFn::Scale | DeriveFn::Mul => match (n, operand) {
            (Some(x), Some(k)) => finite(x * k),
            _ => Value::Null,
        },
        DeriveFn::Add => match (n, operand) {
            (Some(x), Some(k)) => finite(x + k),
            _ => Value::Null,
        },
        DeriveFn::Sub => match (n, operand) {
            (Some(x), Some(k)) => finite(x - k),
            _ => Value::Null,
        },
        DeriveFn::Div => match (n, operand) {
            (Some(x), Some(k)) if k != 0.0 => finite(x / k),
            _ => Value::Null,
        },
        DeriveFn::Bucket => match (n, operand) {
            (Some(x), Some(w)) if w > 0.0 => finite((x / w).floor() * w),
            _ => Value::Null,
        },
        DeriveFn::Year => date().map_or(Value::Null, |d| json!(format!("{:04}", d.year()))),
        DeriveFn::Month => date().map_or(Value::Null, |d| json!(format!("{:04}-{:02}", d.year(), u8::from(d.month())))),
        DeriveFn::MonthOfYear => date().map_or(Value::Null, |d| {
            let m = d.month();
            json!(format!("{:02} {}", u8::from(m), &m.to_string()[..3]))
        }),
        DeriveFn::Day => date().map_or(Value::Null, |d| {
            json!(format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()))
        }),
        DeriveFn::Weekday => date().map_or(Value::Null, |d| {
            let w = d.weekday();
            json!(format!("{} {}", w.number_from_monday(), &w.to_string()[..3]))
        }),
        DeriveFn::Hour => date().map_or(Value::Null, |d| json!(d.hour())),
    }
}

/// Order two values: numbers numerically, then text, nulls last.
fn order_values(a: &Value, b: &Value) -> Ordering {
    use super::chart::num;
    match (a.is_null(), b.is_null()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => label_key(a).unwrap_or_default().cmp(&label_key(b).unwrap_or_default()),
    }
}

fn aggregate(rows: &[Value], groupby: &[String], ops: &[AggOp]) -> Result<Vec<Value>, String> {
    for g in groupby {
        if missing(rows, g) {
            return Err(format!("no row has \"{g}\""));
        }
    }
    for op in ops {
        if let Some(f) = &op.field {
            if missing(rows, f) {
                return Err(format!("no row has \"{f}\""));
            }
        }
    }
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<(Vec<Value>, Vec<&Value>)> = Vec::new();
    for r in rows {
        let key: Vec<Value> = groupby.iter().map(|g| field(r, g).unwrap_or(Value::Null)).collect();
        let k = serde_json::to_string(&key).unwrap_or_default();
        let i = *index.entry(k).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[i].1.push(r);
    }
    groups.sort_by(|a, b| {
        a.0.iter().zip(&b.0).map(|(x, y)| order_values(x, y)).find(|o| *o != Ordering::Equal).unwrap_or(Ordering::Equal)
    });
    Ok(groups
        .into_iter()
        .map(|(key, members)| {
            let mut out = Map::new();
            for (g, v) in groupby.iter().zip(key) {
                out.insert(g.clone(), v);
            }
            for op in ops {
                out.insert(op.out.clone(), reduce(&members, op));
            }
            Value::Object(out)
        })
        .collect())
}

fn reduce(members: &[&Value], op: &AggOp) -> Value {
    use super::chart::num;
    if op.func == AggFn::Count {
        return json!(members.len());
    }
    let f = op.field.as_deref().unwrap_or_default();
    let mut xs: Vec<f64> = members.iter().filter_map(|r| field(r, f).as_ref().and_then(num)).collect();
    if xs.is_empty() {
        return Value::Null;
    }
    let v = match op.func {
        AggFn::Sum => xs.iter().sum(),
        AggFn::Mean => xs.iter().sum::<f64>() / xs.len() as f64,
        AggFn::Min => xs.iter().copied().fold(f64::INFINITY, f64::min),
        AggFn::Max => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        AggFn::Median => {
            xs.sort_by(f64::total_cmp);
            let n = xs.len();
            if n % 2 == 1 { xs[n / 2] } else { (xs[n / 2 - 1] + xs[n / 2]) / 2.0 }
        }
        AggFn::First => xs[0],
        AggFn::Last => xs[xs.len() - 1],
        AggFn::Count => unreachable!("handled above"),
    };
    json!(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(v: Value) -> Vec<Step> {
        parse(Some(&v)).expect("valid steps")
    }

    fn trades() -> Vec<Value> {
        vec![
            json!({ "ticker": "BTC", "pnl": 10.0, "exit": "2026-01-05T10:00:00Z" }),
            json!({ "ticker": "BTC", "pnl": -4.0, "exit": "2026-01-06T15:00:00Z" }),
            json!({ "ticker": "ETH", "pnl": 6.0, "exit": "2026-02-02T09:00:00Z" }),
            json!({ "ticker": "ETH", "pnl": -2.0, "exit": "2026-02-03T09:00:00Z" }),
        ]
    }

    #[test]
    fn gains_vs_losses_by_sign() {
        let s = steps(json!([
            { "op": "derive", "field": "pnl", "fn": "sign", "labels": { "positive": "gain", "negative": "loss" }, "as": "side" },
            { "op": "derive", "field": "pnl", "fn": "abs", "as": "size" },
            { "op": "aggregate", "groupby": ["side"], "ops": [{ "fn": "sum", "field": "size", "as": "total" }] }
        ]));
        let out = apply(trades(), &s).unwrap();
        assert_eq!(out, vec![json!({ "side": "gain", "total": 16.0 }), json!({ "side": "loss", "total": 6.0 })]);
    }

    #[test]
    fn monthly_sum_and_weekday_order() {
        let s = steps(json!([
            { "op": "derive", "field": "exit", "fn": "month", "as": "m" },
            { "op": "aggregate", "groupby": ["m"], "ops": [{ "fn": "sum", "field": "pnl", "as": "pnl" }, { "fn": "count", "as": "n" }] }
        ]));
        let out = apply(trades(), &s).unwrap();
        assert_eq!(out[0], json!({ "m": "2026-01", "pnl": 6.0, "n": 2 }));
        let w = steps(json!([{ "op": "derive", "field": "exit", "fn": "weekday", "as": "w" }]));
        assert_eq!(apply(trades(), &w).unwrap()[0]["w"], "1 Mon");
    }

    #[test]
    fn filter_sort_limit_cumsum() {
        let s = steps(json!([
            { "op": "filter", "field": "ticker", "cmp": "eq", "value": "btc" },
            { "op": "cumsum", "field": "pnl", "as": "equity" },
            { "op": "sort", "field": "pnl", "order": "desc" },
            { "op": "limit", "n": 1 }
        ]));
        let out = apply(trades(), &s).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["equity"], 10.0);
    }

    #[test]
    fn unknown_ops_and_fields_are_refused_with_the_fix() {
        assert!(parse(Some(&json!([{ "op": "pivot" }]))).unwrap_err().contains("step 1"));
        assert!(parse(Some(&json!([{ "op": "limit", "n": 3, "typo": 1 }]))).is_err());
        assert!(parse(Some(&json!([{ "op": "derive", "field": "a", "fn": "div", "as": "b" }]))).unwrap_err().contains("other"));
        let s = steps(json!([{ "op": "filter", "field": "nope", "cmp": "eq", "value": 1 }]));
        let err = apply(trades(), &s).unwrap_err();
        assert!(err.contains("nope") && err.contains("ticker"), "{err}");
    }
}
