//! `{{ ... }}` resolution: how one node reads what the nodes before it produced.
//!
//! Deliberately **not** an expression language: no arithmetic, no calls, no eval. A
//! template is a path into a read-only context plus an optional chain of fixed filters.
//! Anything a workflow needs beyond that is a `transform` node, which is inspectable in the
//! run trace. That keeps a stored workflow from ever becoming code that runs itself.
//!
//! Paths:
//!   `steps.<node_id>.output…`  what an earlier node returned (dot path, `[i]` indexing)
//!   `steps.<node_id>.status`   `ok` | `simulated` | `failed` | `skipped`
//!   `steps.<node_id>.error`    the failure message, empty unless the block failed
//!   `run.id` `run.started_at` `run.trigger`, `workflow.name`, `input.<key>`
//!   `vault.<vault>.<item>`     a secret, and only in fields declared secret-bearing
//!
//! Two rules make the trace safe to store: a resolved vault value is remembered by the
//! [`Secrets`] set and scrubbed out of every recorded request/output/error, and a whole
//! string that is exactly one expression resolves to the **JSON value**, not to its text,
//! so an object stays an object instead of being flattened into a debug string.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use serde_json::{Map, Value};

use crate::AppState;

/// Cap on one rendered string. A node that needs more than this is moving a payload, and
/// payloads travel by blob handle.
pub const MAX_RENDERED: usize = 256 * 1024;

/// The read-only context a template resolves against.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// node id -> `{ "output": …, "status": "ok", "error": "", "bytes": N }`
    pub steps: Map<String, Value>,
    pub run: Value,
    pub input: Value,
    pub workflow: Value,
}

impl Ctx {
    pub fn record(
        &mut self,
        node_id: &str,
        status: &str,
        output: Value,
        bytes: usize,
        error: &str,
    ) {
        self.steps.insert(
            node_id.to_string(),
            serde_json::json!({
                "output": output,
                "status": status,
                // The failure message, so the block on the error port can say what broke
                // instead of only that something did. Empty on success.
                "error": error,
                "bytes": bytes,
            }),
        );
    }
}

/// Every secret value that was resolved during a run, so the recorder can scrub them.
#[derive(Clone, Default)]
pub struct Secrets(Arc<Mutex<Vec<String>>>);

impl Secrets {
    pub fn add(&self, value: &str) {
        if value.len() < 4 {
            // Too short to scrub without mangling ordinary text; such a "secret" is not one.
            return;
        }
        let mut guard = self.0.lock().expect("secrets lock");
        if !guard.iter().any(|s| s == value) {
            guard.push(value.to_string());
        }
    }

    /// Replace every known secret with `***` inside a string.
    pub fn scrub(&self, text: &str) -> String {
        let guard = self.0.lock().expect("secrets lock");
        let mut out = text.to_string();
        for s in guard.iter() {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), "***");
            }
        }
        out
    }

    /// Scrub every string leaf of a JSON value in place.
    pub fn scrub_value(&self, v: &mut Value) {
        match v {
            Value::String(s) => {
                let scrubbed = self.scrub(s);
                if &scrubbed != s {
                    *s = scrubbed;
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| self.scrub_value(x)),
            Value::Object(o) => o.values_mut().for_each(|x| self.scrub_value(x)),
            _ => {}
        }
    }
}

/// Resolves templates for one run.
pub struct Resolver<'a> {
    state: &'a AppState,
    pub secrets: Secrets,
    /// Vault lookups are cached per run: the same credential is usually read by several
    /// nodes, and each miss is a decryption plus a quota bump.
    cache: Mutex<HashMap<String, String>>,
}

impl<'a> Resolver<'a> {
    pub fn new(state: &'a AppState, secrets: Secrets) -> Self {
        Self { state, secrets, cache: Mutex::new(HashMap::new()) }
    }

    /// Render one template string. `allow_secrets` gates `vault.*` refs: a URL or a label
    /// may not carry a credential, a header value or a request body may.
    pub async fn render(
        &self,
        template: &str,
        ctx: &Ctx,
        allow_secrets: bool,
    ) -> Result<Value, String> {
        if template.len() > MAX_RENDERED {
            return Err("template too long".into());
        }
        let parts = split(template)?;
        // A string that is exactly one expression keeps its JSON type.
        if let [Piece::Expr(e)] = parts.as_slice() {
            return self.eval(e, ctx, allow_secrets).await;
        }
        let mut out = String::with_capacity(template.len());
        for part in &parts {
            match part {
                Piece::Text(t) => out.push_str(t),
                Piece::Expr(e) => {
                    let v = self.eval(e, ctx, allow_secrets).await?;
                    out.push_str(&display(&v));
                }
            }
            if out.len() > MAX_RENDERED {
                return Err("rendered value exceeds 256 KB; pass a blob handle instead".into());
            }
        }
        Ok(Value::String(out))
    }

    /// Render a template as text (the common case: a URL, a title, a header value).
    pub async fn render_str(
        &self,
        template: &str,
        ctx: &Ctx,
        allow_secrets: bool,
    ) -> Result<String, String> {
        Ok(display(&self.render(template, ctx, allow_secrets).await?))
    }

    /// Walk a JSON document and render every string leaf, keys included.
    pub fn render_value<'b>(
        &'b self,
        value: &'b Value,
        ctx: &'b Ctx,
        allow_secrets: bool,
    ) -> BoxFuture<'b, Result<Value, String>> {
        Box::pin(async move {
            match value {
                Value::String(s) => self.render(s, ctx, allow_secrets).await,
                Value::Array(a) => {
                    let mut out = Vec::with_capacity(a.len());
                    for item in a {
                        out.push(self.render_value(item, ctx, allow_secrets).await?);
                    }
                    Ok(Value::Array(out))
                }
                Value::Object(o) => {
                    let mut out = Map::new();
                    for (k, v) in o {
                        let key = display(&self.render(k, ctx, allow_secrets).await?);
                        out.insert(key, self.render_value(v, ctx, allow_secrets).await?);
                    }
                    Ok(Value::Object(out))
                }
                other => Ok(other.clone()),
            }
        })
    }

    async fn eval(&self, raw: &str, ctx: &Ctx, allow_secrets: bool) -> Result<Value, String> {
        let (path, filters) = parse_filters(raw)?;
        let path = path.trim();
        if path.is_empty() {
            return Err("empty expression".into());
        }
        let mut value = if let Some(rest) = path.strip_prefix("vault.") {
            if !allow_secrets {
                return Err(format!(
                    "vault reference \"{path}\" is not allowed in this field (only headers, \
                     auth fields and request bodies can carry a secret)"
                ));
            }
            Some(Value::String(self.vault(rest).await?))
        } else {
            lookup(path, ctx)
        };

        if value.is_none() {
            // A `default` filter is the only way a missing path is not an error: it makes
            // the intent explicit at the call site instead of silently producing "".
            if let Some(d) = filters.iter().find(|f| f.name == "default") {
                value = Some(Value::String(d.arg.clone().unwrap_or_default()));
            } else {
                return Err(format!("{path} is not available at this point in the run"));
            }
        }
        let mut value = value.expect("checked above");
        for f in &filters {
            value = apply_filter(&f.name, f.arg.as_deref(), value)?;
        }
        Ok(value)
    }

    async fn vault(&self, rest: &str) -> Result<String, String> {
        let Some((vault, item)) = rest.split_once('.') else {
            return Err(format!("vault reference must be vault.<vault>.<item>, got \"{rest}\""));
        };
        if let Some(hit) = self.cache.lock().expect("vault cache").get(rest) {
            return Ok(hit.clone());
        }
        let found =
            otw_store::vault::open_by_names(&self.state.pool, &self.state.cipher, vault, item)
                .await
                .map_err(|e| format!("vault lookup failed: {e}"))?;
        let Some(secret) = found else {
            return Err(format!("vault item \"{vault}/{item}\" not found"));
        };
        self.secrets.add(&secret);
        self.cache.lock().expect("vault cache").insert(rest.to_string(), secret.clone());
        Ok(secret)
    }
}

// ── Parsing ──────────────────────────────────────────────────────────────────

enum Piece<'a> {
    Text(&'a str),
    Expr(&'a str),
}

/// Split a template into literal text and `{{ … }}` expressions.
fn split(template: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        if open > 0 {
            out.push(Piece::Text(&rest[..open]));
        }
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            return Err("unclosed {{ in template".into());
        };
        out.push(Piece::Expr(after[..close].trim()));
        rest = &after[close + 2..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    Ok(out)
}

struct Filter {
    name: String,
    arg: Option<String>,
}

/// `path | filter | filter:"arg"`. Quoted arguments may contain a pipe.
fn parse_filters(raw: &str) -> Result<(&str, Vec<Filter>), String> {
    let mut parts: Vec<&str> = Vec::new();
    let (mut start, mut in_quotes) = (0usize, false);
    for (i, c) in raw.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '|' if !in_quotes => {
                parts.push(&raw[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&raw[start..]);
    let path = parts.remove(0);
    let mut filters = Vec::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let (name, arg) = match p.split_once(':') {
            Some((n, a)) => (n.trim(), Some(a.trim().trim_matches('"').to_string())),
            None => (p, None),
        };
        filters.push(Filter { name: name.to_string(), arg });
    }
    Ok((path, filters))
}

/// Resolve a dotted path with optional `[i]` indexing against the context roots.
pub fn lookup(path: &str, ctx: &Ctx) -> Option<Value> {
    let mut segments = path.split('.');
    let root = segments.next()?;
    let mut current = match root {
        "steps" => Value::Object(ctx.steps.clone()),
        "run" => ctx.run.clone(),
        "input" => ctx.input.clone(),
        "workflow" => ctx.workflow.clone(),
        _ => return None,
    };
    for seg in segments {
        current = step_into(current, seg)?;
    }
    Some(current)
}

/// One path segment: a key, possibly followed by `[i]` indexes.
fn step_into(value: Value, segment: &str) -> Option<Value> {
    let (key, rest) = match segment.find('[') {
        Some(i) => (&segment[..i], &segment[i..]),
        None => (segment, ""),
    };
    let mut current = if key.is_empty() {
        value
    } else {
        match value {
            Value::Object(mut o) => o.remove(key)?,
            _ => return None,
        }
    };
    let mut rest = rest;
    while let Some(close) = rest.find(']') {
        let idx: usize = rest[1..close].parse().ok()?;
        current = match current {
            Value::Array(mut a) => {
                if idx >= a.len() {
                    return None;
                }
                a.swap_remove(idx)
            }
            _ => return None,
        };
        rest = &rest[close + 1..];
    }
    Some(current)
}

fn apply_filter(name: &str, arg: Option<&str>, value: Value) -> Result<Value, String> {
    let out = match name {
        "json" => Value::String(serde_json::to_string(&value).unwrap_or_default()),
        "upper" => Value::String(display(&value).to_uppercase()),
        "lower" => Value::String(display(&value).to_lowercase()),
        "trim" => Value::String(display(&value).trim().to_string()),
        // Already applied when the path was missing; a present value passes through.
        "default" => value,
        "round" => {
            let digits: usize = arg.and_then(|a| a.parse().ok()).unwrap_or(2).min(10);
            match as_number(&value) {
                Some(n) => Value::String(format!("{n:.digits$}")),
                None => return Err("round expects a number".into()),
            }
        }
        "date" => Value::String(format_date(&display(&value), arg.unwrap_or("YYYY-MM-DD"))?),
        other => return Err(format!("unknown filter \"{other}\"")),
    };
    Ok(out)
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Reformat an RFC 3339 timestamp with a small token vocabulary. Anything unparseable is
/// an error rather than a silently wrong date.
fn format_date(input: &str, pattern: &str) -> Result<String, String> {
    let parsed = time::OffsetDateTime::parse(
        input.trim(),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| format!("date filter expects an RFC 3339 timestamp, got \"{input}\""))?;
    let mut out = pattern.to_string();
    for (token, value) in [
        ("YYYY", format!("{:04}", parsed.year())),
        ("MM", format!("{:02}", parsed.month() as u8)),
        ("DD", format!("{:02}", parsed.day())),
        ("HH", format!("{:02}", parsed.hour())),
        ("mm", format!("{:02}", parsed.minute())),
        ("ss", format!("{:02}", parsed.second())),
    ] {
        out = out.replace(token, &value);
    }
    Ok(out)
}

/// A JSON value as text: strings stay bare (no quotes), everything else serializes.
pub fn display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Truthiness, used by the `if` node and the `is_true` operator.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => {
            let t = s.trim();
            !t.is_empty() && !t.eq_ignore_ascii_case("false") && t != "0"
        }
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Every `{{ … }}` path a graph references, for save-time validation.
pub fn referenced_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if let Ok(parts) = split(s) {
                for p in parts {
                    if let Piece::Expr(e) = p {
                        let (path, _) = parse_filters(e).unwrap_or((e, Vec::new()));
                        out.push(path.trim().to_string());
                    }
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|v| referenced_paths(v, out)),
        Value::Object(o) => o.iter().for_each(|(k, v)| {
            referenced_paths(&Value::String(k.clone()), out);
            referenced_paths(v, out);
        }),
        _ => {}
    }
}
