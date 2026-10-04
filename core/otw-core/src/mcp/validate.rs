//! Checks a request against the contract the catalog publishes, before it is dispatched.
//!
//! Two kinds of finding:
//! - `invalid`: the request breaks the schema (missing required field, wrong type, value
//!   outside an enum). The handler rejects these on its own, but serde stops at the first
//!   one and names it in its own vocabulary; this lists all of them with their path.
//! - `ignored`: a field or query param the handler does not read. Serde drops those
//!   without a word, so the call succeeds and the agent believes it filtered or set
//!   something it did not. That is the silent failure this module exists for.
//!
//! Neither kind blocks the call: the handler stays the authority on what it accepts, so a
//! schema that lags behind the code costs a misleading note, never a refused request. The
//! gateway shows `invalid` next to a rejection, `ignored` next to a success, and logs an
//! `invalid` finding on a success as schema drift.

use serde_json::{Map, Value};

/// Cap on findings per request: past this the agent has enough to fix the call.
const MAX_FINDINGS: usize = 12;

#[derive(Default, Debug)]
pub struct Findings {
    pub invalid: Vec<String>,
    pub ignored: Vec<String>,
}

impl Findings {
    fn full(&self) -> bool {
        self.invalid.len() + self.ignored.len() >= MAX_FINDINGS
    }

    pub fn extend(&mut self, other: Findings) {
        self.invalid.extend(other.invalid);
        self.ignored.extend(other.ignored);
    }
}

// ── Body ─────────────────────────────────────────────────────────────────────

/// Check a JSON body against the endpoint's body schema.
pub fn check_body(schema: &Value, body: &Value) -> Findings {
    let mut out = Findings::default();
    check(schema, schema, body, "body", true, &mut out);
    out
}

/// Follow `#/definitions/<name>` references (schemars' only form).
fn resolve<'a>(root: &'a Value, mut s: &'a Value) -> &'a Value {
    for _ in 0..16 {
        let Some(r) = s.get("$ref").and_then(Value::as_str) else { break };
        let Some(name) = r.strip_prefix("#/definitions/") else { break };
        match root.pointer(&format!("/definitions/{name}")) {
            Some(t) => s = t,
            None => break,
        }
    }
    s
}

fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_ok(allowed: &str, v: &Value) -> bool {
    let actual = type_of(v);
    allowed == actual
        || (allowed == "number" && actual == "integer")
        || (allowed == "integer" && v.as_f64().is_some_and(|f| f.fract() == 0.0))
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 60 { format!("{}…", s.chars().take(60).collect::<String>()) } else { s }
}

fn join(base: &str, key: &str) -> String {
    format!("{base}.{key}")
}

/// `strict`: report keys the object schema does not declare. Off under `allOf`, where each
/// branch only knows its own share of the properties.
fn check(root: &Value, schema: &Value, v: &Value, path: &str, strict: bool, out: &mut Findings) {
    if out.full() {
        return;
    }
    let s = resolve(root, schema);
    let Some(obj) = s.as_object() else { return };

    if let Some(Value::Array(branches)) = obj.get("anyOf").or_else(|| obj.get("oneOf")) {
        check_union(root, branches, v, path, strict, out);
        return;
    }
    if let Some(Value::Array(branches)) = obj.get("allOf") {
        for b in branches {
            check(root, b, v, path, false, out);
        }
    }

    if let Some(t) = obj.get("type") {
        let allowed: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !allowed.is_empty() && !allowed.iter().any(|a| type_ok(a, v)) {
            out.invalid
                .push(format!("{path}: expected {}, got {} {}", allowed.join(" or "), type_of(v), short(v)));
            return;
        }
    }
    if let Some(Value::Array(options)) = obj.get("enum") {
        if !options.contains(v) {
            let list: Vec<String> = options.iter().map(|o| o.to_string()).collect();
            out.invalid.push(format!("{path}: {} is not one of [{}]", short(v), list.join(", ")));
            return;
        }
    }
    if let Some(c) = obj.get("const") {
        if c != v {
            out.invalid.push(format!("{path}: must be {c}, got {}", short(v)));
            return;
        }
    }
    if let Some(n) = v.as_f64() {
        if let Some(min) = obj.get("minimum").and_then(Value::as_f64) {
            if n < min {
                out.invalid.push(format!("{path}: {n} is below the minimum {min}"));
            }
        }
        if let Some(max) = obj.get("maximum").and_then(Value::as_f64) {
            if n > max {
                out.invalid.push(format!("{path}: {n} is above the maximum {max}"));
            }
        }
    }

    match v {
        Value::Object(fields) => check_object(root, obj, fields, path, strict, out),
        Value::Array(items) => {
            if let Some(item_schema) = obj.get("items").filter(|i| i.is_object()) {
                for (i, item) in items.iter().enumerate() {
                    check(root, item_schema, item, &format!("{path}[{i}]"), strict, out);
                }
            }
        }
        _ => {}
    }
}

fn check_object(
    root: &Value,
    obj: &Map<String, Value>,
    fields: &Map<String, Value>,
    path: &str,
    strict: bool,
    out: &mut Findings,
) {
    if let Some(Value::Array(req)) = obj.get("required") {
        for r in req.iter().filter_map(Value::as_str) {
            if !fields.contains_key(r) {
                out.invalid.push(format!("{path}: missing required field `{r}`"));
            }
        }
    }
    let props = obj.get("properties").and_then(Value::as_object);
    let extra = obj.get("additionalProperties");
    for (k, fv) in fields {
        if let Some(ps) = props.and_then(|p| p.get(k)) {
            check(root, ps, fv, &join(path, k), strict, out);
            continue;
        }
        match extra {
            Some(Value::Bool(false)) => out.invalid.push(format!(
                "{}: unknown field (accepted: {})",
                join(path, k),
                names(props)
            )),
            Some(es @ Value::Object(_)) => check(root, es, fv, &join(path, k), strict, out),
            _ if strict && props.is_some_and(|p| !p.is_empty()) => out.ignored.push(format!(
                "{} (not a field here; accepted: {})",
                join(path, k),
                names(props)
            )),
            _ => {}
        }
    }
}

fn names(props: Option<&Map<String, Value>>) -> String {
    props.map(|p| p.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

/// The discriminating field of a tagged union: a property every object branch pins with a
/// `const` (serde's `#[serde(tag = "...")]`).
fn tag_of<'a>(root: &'a Value, branches: &'a [Value]) -> Option<(&'a str, Vec<(&'a Value, &'a Value)>)> {
    let first = resolve(root, branches.first()?);
    for (key, _) in first.get("properties")?.as_object()? {
        let mut variants = Vec::new();
        for b in branches {
            let b = resolve(root, b);
            let c = b.get("properties").and_then(|p| p.get(key)).map(|p| resolve(root, p));
            let c = c.and_then(|p| p.get("const").or_else(|| {
                p.get("enum").and_then(Value::as_array).filter(|e| e.len() == 1).and_then(|e| e.first())
            }));
            match c {
                Some(c) => variants.push((c, b)),
                None => break,
            }
        }
        if variants.len() == branches.len() {
            return Some((key.as_str(), variants));
        }
    }
    None
}

fn check_union(root: &Value, branches: &[Value], v: &Value, path: &str, strict: bool, out: &mut Findings) {
    // A branch that fits settles it; keep its `ignored` notes (fewest wins).
    let mut best: Option<Findings> = None;
    let mut tried: Vec<Findings> = Vec::new();
    for b in branches {
        let mut f = Findings::default();
        check(root, b, v, path, strict, &mut f);
        if f.invalid.is_empty() {
            if best.as_ref().is_none_or(|x| f.ignored.len() < x.ignored.len()) {
                best = Some(f);
            }
        } else {
            tried.push(f);
        }
    }
    if let Some(f) = best {
        out.extend(f);
        return;
    }

    // Tagged union: judge the value against the variant its tag names.
    if let (Some(fields), Some((tag, variants))) = (v.as_object(), tag_of(root, branches)) {
        let tags: Vec<String> = variants.iter().map(|(c, _)| c.to_string()).collect();
        match fields.get(tag) {
            Some(given) => match variants.iter().find(|(c, _)| *c == given) {
                Some((_, b)) => check(root, b, v, path, strict, out),
                None => out.invalid.push(format!(
                    "{}: {} is not one of [{}]",
                    join(path, tag),
                    short(given),
                    tags.join(", ")
                )),
            },
            None => out.invalid.push(format!(
                "{path}: missing required field `{tag}` (one of [{}])",
                tags.join(", ")
            )),
        }
        return;
    }

    // A plain enum written as one `const` per variant.
    let consts: Vec<&Value> = branches
        .iter()
        .filter_map(|b| resolve(root, b).get("const"))
        .collect();
    if consts.len() == branches.len() {
        let list: Vec<String> = consts.iter().map(|c| c.to_string()).collect();
        out.invalid.push(format!("{path}: {} is not one of [{}]", short(v), list.join(", ")));
        return;
    }

    // Untagged (typically `Option<T>`): report the closest branch, never the `null` one
    // unless null was sent.
    let is_null_branch = |b: &Value| resolve(root, b).get("type").and_then(Value::as_str) == Some("null");
    let closest = branches
        .iter()
        .zip(tried)
        .filter(|(b, _)| v.is_null() || !is_null_branch(b))
        .map(|(_, f)| f)
        .min_by_key(|f| f.invalid.len());
    match closest {
        Some(f) => out.extend(f),
        None => out.invalid.push(format!("{path}: {} matches none of the accepted shapes", short(v))),
    }
}

// ── Query string ─────────────────────────────────────────────────────────────

/// Check a raw query string (no leading `?`) against the endpoint's query schema, or
/// against "takes no params" when it has none.
pub fn check_query(schema: Option<&Value>, query: &str) -> Findings {
    let mut out = Findings::default();
    let keys: Vec<String> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| decode(p.split('=').next().unwrap_or("")))
        .collect();
    let Some(schema) = schema else {
        for k in keys {
            out.ignored.push(format!("query param `{k}` (this endpoint reads no query params)"));
        }
        return out;
    };
    let props = schema.get("properties").and_then(Value::as_object);
    for k in &keys {
        if !props.is_some_and(|p| p.contains_key(k)) {
            out.ignored.push(format!("query param `{k}` (accepted: {})", names(props)));
        }
    }
    if let Some(Value::Array(req)) = schema.get("required") {
        for r in req.iter().filter_map(Value::as_str) {
            if !keys.iter().any(|k| k == r) {
                out.invalid.push(format!("query: missing required param `{r}`"));
            }
        }
    }
    out
}

/// Percent-decode a query key (`+` is a space in form encoding).
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(x) => {
                        out.push(x);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One line per accepted query param: `name (type, required): description`. Compact on
/// purpose: this is what the module listing prints instead of the raw schema.
pub fn brief_params(schema: &Value) -> String {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return String::new();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    props
        .iter()
        .map(|(k, p)| {
            let mut t = brief_type(schema, p);
            if required.contains(&k.as_str()) {
                t.push_str(", required");
            }
            if let Some(d) = p.get("default").filter(|d| !d.is_null()) {
                t.push_str(&format!(", default {d}"));
            }
            match resolve(schema, p).get("description").and_then(Value::as_str) {
                Some(d) => format!("{k} ({t}): {}", d.lines().next().unwrap_or("").trim()),
                None => format!("{k} ({t})"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn brief_type(root: &Value, p: &Value) -> String {
    let p = resolve(root, p);
    if let Some(Value::Array(e)) = p.get("enum") {
        return e.iter().map(|x| x.as_str().map(str::to_string).unwrap_or(x.to_string())).collect::<Vec<_>>().join("|");
    }
    if let Some(Value::Array(bs)) = p.get("anyOf").or_else(|| p.get("oneOf")) {
        let parts: Vec<String> = bs
            .iter()
            .filter(|b| resolve(root, b).get("type").and_then(Value::as_str) != Some("null"))
            .map(|b| {
                let b = resolve(root, b);
                match b.get("const") {
                    Some(Value::String(c)) => c.clone(),
                    _ => brief_type(root, b),
                }
            })
            .collect();
        return parts.join("|");
    }
    let base = match p.get("type") {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .filter(|t| *t != "null")
            .collect::<Vec<_>>()
            .join("|"),
        _ => "any".into(),
    };
    match p.get("format").and_then(Value::as_str) {
        Some("uuid") => "uuid".into(),
        Some("date") => "date YYYY-MM-DD".into(),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tagged() -> Value {
        json!({
            "type": "object",
            "required": ["sizing"],
            "properties": {
                "name": { "type": ["string", "null"] },
                "sizing": { "$ref": "#/definitions/Sizing" },
                "legs": { "type": "array", "items": { "$ref": "#/definitions/Leg" } }
            },
            "definitions": {
                "Sizing": { "oneOf": [
                    { "type": "object", "required": ["mode", "percent"],
                      "properties": { "mode": { "const": "percent_equity" }, "percent": { "type": "number" } } },
                    { "type": "object", "required": ["mode", "qty"],
                      "properties": { "mode": { "const": "fixed_qty" }, "qty": { "type": "integer", "minimum": 0 } } }
                ]},
                "Leg": { "type": "object", "properties": { "side": { "enum": ["long", "short"] } } }
            }
        })
    }

    #[test]
    fn a_valid_body_has_no_findings() {
        let f = check_body(&tagged(), &json!({ "sizing": { "mode": "fixed_qty", "qty": 3 }, "name": null }));
        assert!(f.invalid.is_empty() && f.ignored.is_empty(), "{f:?}");
    }

    #[test]
    fn a_tagged_union_is_judged_by_the_variant_its_tag_names() {
        let f = check_body(&tagged(), &json!({ "sizing": { "mode": "fixed_qty", "percent": 10 } }));
        assert_eq!(f.invalid, ["body.sizing: missing required field `qty`"]);
        assert!(f.ignored[0].starts_with("body.sizing.percent"), "{f:?}");

        let f = check_body(&tagged(), &json!({ "sizing": { "mode": "pct" } }));
        assert_eq!(f.invalid, [r#"body.sizing.mode: "pct" is not one of ["percent_equity", "fixed_qty"]"#]);

        let f = check_body(&tagged(), &json!({ "sizing": { "percent": 5 } }));
        assert!(f.invalid[0].contains("missing required field `mode`"), "{f:?}");
    }

    #[test]
    fn every_problem_is_reported_at_once_with_its_path() {
        let f = check_body(&tagged(), &json!({ "legs": [{ "side": "up" }], "nme": "x" }));
        assert!(f.invalid.iter().any(|m| m == "body: missing required field `sizing`"), "{f:?}");
        assert!(f.invalid.iter().any(|m| m.starts_with("body.legs[0].side: \"up\"")), "{f:?}");
        assert!(f.ignored.iter().any(|m| m.starts_with("body.nme")), "{f:?}");
    }

    #[test]
    fn an_option_reports_the_inner_type_not_the_null_branch() {
        let s = json!({ "type": "object", "properties": {
            "leg": { "anyOf": [{ "$ref": "#/definitions/Leg" }, { "type": "null" }] }
        }, "definitions": { "Leg": { "type": "object", "properties": { "side": { "enum": ["long"] } } } } });
        let f = check_body(&s, &json!({ "leg": { "side": "short" } }));
        assert_eq!(f.invalid.len(), 1, "{f:?}");
        assert!(f.invalid[0].starts_with("body.leg.side"), "{f:?}");
    }

    #[test]
    fn unknown_query_params_are_ignored_findings_and_required_ones_are_named() {
        let s = json!({ "type": "object", "required": ["q"], "properties": {
            "q": { "type": "string" }, "limit": { "type": ["integer", "null"] }
        } });
        let f = check_query(Some(&s), "limt=5&q=abc");
        assert_eq!(f.ignored.len(), 1);
        assert!(f.ignored[0].contains("`limt`") && f.ignored[0].contains("limit"), "{f:?}");
        assert!(f.invalid.is_empty());

        let f = check_query(Some(&s), "limit=5");
        assert_eq!(f.invalid, ["query: missing required param `q`"]);

        let f = check_query(None, "limit=5");
        assert!(f.ignored[0].contains("reads no query params"));
    }

    #[test]
    fn brief_params_lists_names_types_and_requirements() {
        let s = json!({ "type": "object", "required": ["column"], "properties": {
            "column": { "type": "string" },
            "id": { "type": ["string", "null"], "format": "uuid" },
            "full": { "type": "boolean", "default": false }
        } });
        let b = brief_params(&s);
        assert!(b.contains("column (string, required)"), "{b}");
        assert!(b.contains("id (uuid)"), "{b}");
        assert!(b.contains("full (boolean, default false)"), "{b}");
    }
}
