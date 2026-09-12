//! `if`: the only control flow in a workflow.
//!
//! Conditions compare two resolved values. Comparison is numeric when both sides read as
//! numbers and textual otherwise, which is what makes `{{steps.x.output.count}} > 10` work
//! whether the API returned `10` or `"10"`.

use serde_json::{json, Value};

use super::{NodeCtx, Outcome};
use crate::automator::expr;

pub const OPS: &[&str] = &[
    "eq", "ne", "gt", "gte", "lt", "lte", "contains", "not_contains", "starts_with",
    "ends_with", "empty", "not_empty", "is_true",
];

/// Operators that ignore the right-hand side.
const UNARY: &[&str] = &["empty", "not_empty", "is_true"];

pub fn validate(config: &Value) -> Result<(), String> {
    let mode = super::opt(config, "match", "all");
    if mode != "all" && mode != "any" {
        return Err("match must be all or any".into());
    }
    let conditions = config.get("conditions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if conditions.is_empty() {
        return Err("add at least one condition".into());
    }
    if conditions.len() > 20 {
        return Err("at most 20 conditions".into());
    }
    for c in &conditions {
        let op = super::opt(c, "op", "");
        if !OPS.contains(&op) {
            return Err(format!("unknown operator \"{op}\""));
        }
        if c.get("left").and_then(|v| v.as_str()).unwrap_or("").trim().is_empty() {
            return Err("a condition needs a left side".into());
        }
    }
    Ok(())
}

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let mode = super::opt(&ctx.node.config, "match", "all");
    let conditions =
        ctx.node.config.get("conditions").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut details = Vec::new();
    let mut results = Vec::new();
    for c in &conditions {
        let op = super::opt(c, "op", "eq").to_string();
        let left_tpl = super::opt(c, "left", "");
        let right_tpl = super::opt(c, "right", "");
        let left = ctx.resolver.render(left_tpl, ctx.vars, false).await?;
        let right = if UNARY.contains(&op.as_str()) {
            Value::Null
        } else {
            ctx.resolver.render(right_tpl, ctx.vars, false).await?
        };
        let hit = compare(&op, &left, &right);
        details.push(json!({ "left": left, "op": op, "right": right, "result": hit }));
        results.push(hit);
    }

    let result = if mode == "any" {
        results.iter().any(|r| *r)
    } else {
        results.iter().all(|r| *r)
    };
    Ok(Outcome::new(
        json!({ "match": mode, "conditions": details }),
        json!({ "result": result }),
    ))
}

fn compare(op: &str, left: &Value, right: &Value) -> bool {
    match op {
        "empty" => !expr::truthy(left),
        "not_empty" => expr::truthy(left),
        "is_true" => expr::truthy(left),
        _ => {
            let (l, r) = (expr::display(left), expr::display(right));
            let nums = (num(left), num(right));
            match op {
                "eq" => match nums {
                    (Some(a), Some(b)) => a == b,
                    _ => l == r,
                },
                "ne" => match nums {
                    (Some(a), Some(b)) => a != b,
                    _ => l != r,
                },
                "gt" | "gte" | "lt" | "lte" => match nums {
                    (Some(a), Some(b)) => match op {
                        "gt" => a > b,
                        "gte" => a >= b,
                        "lt" => a < b,
                        _ => a <= b,
                    },
                    // Text comparison is a real answer for dates and versions, not a fallback.
                    _ => match op {
                        "gt" => l > r,
                        "gte" => l >= r,
                        "lt" => l < r,
                        _ => l <= r,
                    },
                },
                "contains" => l.contains(&r),
                "not_contains" => !l.contains(&r),
                "starts_with" => l.starts_with(&r),
                "ends_with" => l.ends_with(&r),
                _ => false,
            }
        }
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}
