//! MCP (Model Context Protocol) server — Streamable HTTP transport, stateless.
//!
//! `POST /api/mcp` speaks JSON-RPC 2.0 per the 2025-06-18 MCP revision: single JSON
//! request in, single JSON response out (no SSE stream, no session ids — every request
//! is independently authenticated). Exposes three gateway tools over the allowlist in
//! [`catalog`]: `otw_catalog` (discover), `otw_read` (GET), `otw_write` (mutations).
//!
//! Security layers, in order:
//! 1. Global kill-switch: the `mcp_enabled` app setting (default off).
//! 2. Origin/Host match when an Origin header is present (DNS-rebinding guard).
//! 3. Bearer token → SHA-256 lookup in `mcp_tokens`; failures are rate-limited.
//! 4. Per-token module permissions (`"r"` / `"rw"`), enforced on every tool call.
//! 5. The catalog allowlist: endpoints not listed there are unreachable, whatever
//!    the token's permissions say.
//!
//! Dispatch runs the real Axum handlers in-process (`tower::ServiceExt::oneshot`)
//! with the admin user injected, so MCP behavior always matches the REST API.

pub mod catalog;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::AppState;

const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const LATEST_VERSION: &str = "2025-06-18";
/// Cap on response bytes returned to the agent (context protection).
const MAX_RESULT_BYTES: usize = 512 * 1024;
/// Cap on raw bytes read from an in-process dispatch. Responses are parsed and shaped
/// (pick/head) gateway-side before the agent-facing cap applies, so this is generous.
const MAX_RAW_BYTES: usize = 16 * 1024 * 1024;
/// Failed-auth throttle: after this many failures per window, reject early.
const AUTH_FAIL_LIMIT: u32 = 10;
const AUTH_FAIL_WINDOW: Duration = Duration::from_secs(60);

/// The full API router (state applied, no session middleware) used to serve tool
/// calls in-process. Set once from `main` after the router is built.
static DISPATCH: OnceLock<Router> = OnceLock::new();

pub fn init_dispatch(router: Router) {
    let _ = DISPATCH.set(router);
}

// ── Auth ─────────────────────────────────────────────────────────────────────

static AUTH_FAILS: Mutex<Option<(Instant, u32)>> = Mutex::new(None);

fn auth_throttled() -> bool {
    let guard = AUTH_FAILS.lock().unwrap();
    matches!(*guard, Some((start, n)) if n >= AUTH_FAIL_LIMIT && start.elapsed() < AUTH_FAIL_WINDOW)
}

fn record_auth_failure() {
    let mut guard = AUTH_FAILS.lock().unwrap();
    *guard = match *guard {
        Some((start, n)) if start.elapsed() < AUTH_FAIL_WINDOW => Some((start, n + 1)),
        _ => Some((Instant::now(), 1)),
    };
}

fn http_error(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

/// Origin/Host consistency: browsers always send Origin; a DNS-rebinding page would
/// carry a foreign Origin with our Host. Non-browser MCP clients omit Origin entirely.
fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    origin
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
        .is_some_and(|ohost| ohost.eq_ignore_ascii_case(host))
}

// ── Permissions ──────────────────────────────────────────────────────────────

fn level<'a>(perms: &'a Value, module: &str) -> Option<&'a str> {
    perms.get(module).and_then(|v| v.as_str())
}

fn can_read(perms: &Value, module: &str) -> bool {
    matches!(level(perms, module), Some("r") | Some("rw") | Some("rwd"))
}

fn can_write(perms: &Value, module: &str) -> bool {
    matches!(level(perms, module), Some("rw") | Some("rwd"))
}

fn can_delete(perms: &Value, module: &str) -> bool {
    matches!(level(perms, module), Some("rwd"))
}

fn any_write(perms: &Value) -> bool {
    catalog::MODULES.iter().any(|(m, _)| can_write(perms, m))
}

// ── Endpoint ─────────────────────────────────────────────────────────────────

/// `POST /api/mcp` — authenticate, then answer one JSON-RPC message.
pub async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !origin_ok(&headers) {
        return http_error(StatusCode::FORBIDDEN, "origin not allowed");
    }

    let enabled = otw_store::settings::get_or(&state.pool, "mcp_enabled", "false").await;
    if !matches!(enabled.as_deref(), Ok("true")) {
        return http_error(StatusCode::FORBIDDEN, "MCP access is disabled in Settings");
    }

    if auth_throttled() {
        return http_error(StatusCode::TOO_MANY_REQUESTS, "too many failed auth attempts");
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    let auth = match otw_store::mcp::find_by_token(&state.pool, token).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            record_auth_failure();
            return http_error(StatusCode::UNAUTHORIZED, "invalid or missing bearer token");
        }
        Err(e) => {
            tracing::error!("mcp auth lookup failed: {e:#}");
            return http_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };

    // One message per request; JSON-RPC batching was removed in the 2025-06-18 revision.
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(Value::Object(m)) => Value::Object(m),
        Ok(_) => return rpc_error(Value::Null, -32600, "expected a single JSON-RPC object"),
        Err(e) => return rpc_error(Value::Null, -32700, &format!("parse error: {e}")),
    };
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications get no response body (202 per Streamable HTTP).
    if msg.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    match method {
        "initialize" => rpc_result(id, initialize(&params)),
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, tools_list(&auth.permissions)),
        "tools/call" => tools_call(&state, &auth, id, &params).await,
        _ => rpc_error(id, -32601, &format!("method not found: {method}")),
    }
}

// ── In-process tool calls (trusted internal caller: the Agent module) ──────────
//
// The Agent dispatches into the same catalog/permission logic as the network endpoint,
// authenticated as a selected `mcp_tokens` row's permissions. Layers skipped relative to
// the network path, deliberately:
//   - bearer-hash check — the caller is in-process and already session-authed;
//   - Origin/Host check — meaningless without a network request;
//   - the global `mcp_enabled` setting — that toggle governs the NETWORK endpoint;
//     disabling it must not silently break the in-app assistant (the settings UI states
//     this next to the token picker).
// Per-module permissions and the static catalog allowlist are enforced exactly as for a
// remote client.

/// Render the catalog (module index, or one module's endpoints) for `perms` — the token's
/// levels apply as-is, exactly like for a remote MCP client.
pub fn agent_catalog(perms: &Value, only: Option<&str>) -> String {
    render_catalog(perms, only)
}

/// Run one `otw_read`/`otw_write` call in-process; returns `(text, is_error)`. The token's
/// per-module permissions are the single gate (write needs rw, delete needs rwd) — same
/// checks as the network path, no agent-side overlay.
pub async fn agent_call(
    state: &AppState,
    perms: &Value,
    token_name: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
    shape: Shape,
) -> (String, bool) {
    let method = method.to_ascii_uppercase();
    match run_endpoint_inner(state, perms, token_name, &method, path, body, &shape).await {
        Ok((status, text)) => {
            if status.is_success() {
                (if text.is_empty() { format!("HTTP {status}") } else { text }, false)
            } else {
                (format!("HTTP {status}: {text}"), true)
            }
        }
        Err(msg) => (msg, true),
    }
}

fn rpc_result(id: Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Response {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    }))
    .into_response()
}

/// Tool outcome (including HTTP-level failures) — `isError` flags it for the agent.
fn tool_text(id: Value, text: String, is_error: bool) -> Response {
    rpc_result(
        id,
        json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
    )
}

// ── Methods ──────────────────────────────────────────────────────────────────

fn initialize(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(|v| v.as_str())
        .unwrap_or(LATEST_VERSION);
    let version = if SUPPORTED_VERSIONS.contains(&requested) { requested } else { LATEST_VERSION };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "opentraderworld",
            "title": "OpenTraderWorld",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Gateway to the OpenTraderWorld REST API. Call otw_catalog with no \
            argument to see the modules this token can access, then otw_catalog with a \"module\" \
            to list that module's endpoints. Use otw_read for GET and otw_write for mutations, \
            passing concrete paths like /api/journal/trades?limit=20. Every write endpoint's \
            listing includes the JSON Schema of its request body — follow it exactly (field \
            names and required fields) instead of guessing. Dates are YYYY-MM-DD unless the \
            schema says otherwise. Responses are chart-oriented and can be large: when the \
            user needs specific figures, pass \"pick\" (dot-paths) and/or \"head\" (array cap) \
            to receive only that.",
    })
}

fn tools_list(perms: &Value) -> Value {
    let mut tools = vec![
        json!({
            "name": "otw_catalog",
            "title": "List available endpoints",
            "description": "Discover endpoints. With no argument, returns a compact index of the \
                modules this token can access (label, access level, endpoint count). Pass a \
                \"module\" to list that module's concrete endpoints. Call this before \
                otw_read/otw_write.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "List this module's endpoints (e.g. \"journal\"). Omit to get the module index." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true }
        }),
        json!({
            "name": "otw_read",
            "title": "Read from the API",
            "description": "GET an allowlisted endpoint. Returns the JSON response body. For \
                large responses (portfolio detail, analytics), pass \"pick\" to extract only \
                the fields the user asked for, and/or \"head\" to cap arrays.",
            "inputSchema": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "API path with optional query string, e.g. /api/journal/trades?limit=20" },
                    "pick": { "type": "array", "items": { "type": "string" }, "description": "Dot-paths to extract from the response, e.g. [\"result.var_hist\",\"positions.symbol\"]. Arrays map the remaining path over their elements; a numeric segment indexes. The response becomes {path: value, …}." },
                    "head": { "type": "integer", "minimum": 1, "description": "Truncate every array in the (picked) response to its first N elements; totals are reported." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true }
        }),
    ];
    if any_write(perms) {
        tools.push(json!({
            "name": "otw_write",
            "title": "Write to the API",
            "description": "Mutate through an allowlisted endpoint (POST/PUT/PATCH/DELETE). \
                Requires read+write permission on the endpoint's module.",
            "inputSchema": {
                "type": "object",
                "required": ["method", "path"],
                "properties": {
                    "method": { "type": "string", "enum": ["POST", "PUT", "PATCH", "DELETE"] },
                    "path": { "type": "string", "description": "API path, e.g. /api/journal/trades" },
                    "body": { "description": "JSON request body, when the endpoint expects one." },
                    "pick": { "type": "array", "items": { "type": "string" }, "description": "Dot-paths to extract from the response (useful on compute endpoints like /api/quant/* or /api/backtest/run)." },
                    "head": { "type": "integer", "minimum": 1, "description": "Truncate every array in the (picked) response to its first N elements; totals are reported." }
                },
                "additionalProperties": false
            },
            "annotations": { "destructiveHint": true, "openWorldHint": false }
        }));
    }
    json!({ "tools": tools })
}

async fn tools_call(
    state: &AppState,
    auth: &otw_store::mcp::McpToken,
    id: Value,
    params: &Value,
) -> Response {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let perms = &auth.permissions;

    match name {
        "otw_catalog" => {
            let only = args.get("module").and_then(|v| v.as_str());
            tool_text(id, render_catalog(perms, only), false)
        }
        "otw_read" => {
            let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                return rpc_error(id, -32602, "otw_read requires a \"path\" string");
            };
            let shape = match shape_from(&args) {
                Ok(s) => s,
                Err(msg) => return rpc_error(id, -32602, &msg),
            };
            run_endpoint(state, auth, id, "GET", path, None, shape).await
        }
        "otw_write" => {
            let method = args
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_uppercase();
            if !matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
                return rpc_error(id, -32602, "method must be POST, PUT, PATCH or DELETE");
            }
            let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                return rpc_error(id, -32602, "otw_write requires a \"path\" string");
            };
            let shape = match shape_from(&args) {
                Ok(s) => s,
                Err(msg) => return rpc_error(id, -32602, &msg),
            };
            run_endpoint(state, auth, id, &method, path, args.get("body").cloned(), shape).await
        }
        other => rpc_error(id, -32602, &format!("unknown tool: {other}")),
    }
}

/// Whether `method` on `module` is visible/callable at this permission level:
/// GET needs read, DELETE needs full (rwd), other writes need read+write.
fn can_call(perms: &Value, module: &str, method: &str) -> bool {
    match method {
        "GET" => can_read(perms, module),
        "DELETE" => can_delete(perms, module),
        _ => can_write(perms, module),
    }
}

/// Number of endpoints in `module` visible at this permission level.
fn visible_endpoint_count(perms: &Value, module: &str) -> usize {
    catalog::CATALOG
        .iter()
        .filter(|e| e.module == module && can_call(perms, module, e.method))
        .count()
}

/// Two-level catalog to keep token cost proportional to intent:
/// - `only == None`  → a compact **index** of accessible modules (label, access,
///   endpoint count) and nothing else. The model drills in with a second call.
/// - `only == Some(m)` → the full endpoint list for that one module.
fn render_catalog(perms: &Value, only: Option<&str>) -> String {
    match only {
        None => render_index(perms),
        Some(m) => render_module(perms, m),
    }
}

fn render_index(perms: &Value) -> String {
    let mut out = String::new();
    for (module, label) in catalog::MODULES {
        if !can_read(perms, module) {
            continue;
        }
        let access = access_label(perms, module);
        let n = visible_endpoint_count(perms, module);
        out.push_str(&format!("{module} — {label} ({access}, {n} endpoints)\n"));
    }
    if out.is_empty() {
        no_access_message()
    } else {
        format!(
            "Accessible modules. Call otw_catalog with a \"module\" argument \
             (e.g. {{\"module\":\"journal\"}}) to list that module's endpoints.\n\n{out}"
        )
    }
}

fn render_module(perms: &Value, module: &str) -> String {
    let Some((_, label)) = catalog::MODULES.iter().find(|(m, _)| *m == module) else {
        return format!(
            "Unknown module \"{module}\". Call otw_catalog with no argument to list accessible modules."
        );
    };
    if !can_read(perms, module) {
        return no_access_message();
    }
    let access = access_label(perms, module);
    let mut out = format!("## {module} — {label} ({access})\n");
    for e in catalog::CATALOG.iter().filter(|e| e.module == module) {
        if can_call(perms, module, e.method) {
            out.push_str(&format!("{} {} — {}\n", e.method, e.path, e.desc));
            // The body contract, generated from the exact struct the handler
            // deserializes — send fields exactly as named here.
            if let Some(body) = e.body {
                let schema = serde_json::to_string(&body()).unwrap_or_default();
                out.push_str(&format!("  body schema: {schema}\n"));
            }
        }
    }
    out
}

/// Human label for a token's access on `module`, assuming read is already granted.
fn access_label(perms: &Value, module: &str) -> &'static str {
    if can_delete(perms, module) {
        "full (read+write+delete)"
    } else if can_write(perms, module) {
        "read+write"
    } else {
        "read-only"
    }
}

fn no_access_message() -> String {
    "No accessible endpoints. This token has no permission on the requested module(s); \
     permissions are managed in OpenTraderWorld Settings → MCP."
        .to_string()
}

// ── Response shaping (pick/head) ─────────────────────────────────────────────
//
// REST responses are shaped for the frontend (charts want per-bar arrays; tables want
// every row). An agent usually needs a few scalars to answer the user, so otw_read and
// otw_write accept optional shaping arguments applied gateway-side — handlers and the
// frontend are untouched:
//   - `pick`: dot-paths extracted from the JSON body (arrays map the remaining path
//     over their elements; a numeric segment indexes instead).
//   - `head`: every array truncated to its first N elements, with totals reported.
// The agent-facing size cap applies to the *shaped* output; oversized responses come
// back as a per-key size breakdown so the follow-up call can pick precisely.

/// Parsed shaping arguments from an otw_read/otw_write call.
#[derive(Default)]
pub struct Shape {
    pick: Vec<String>,
    head: Option<usize>,
}

impl Shape {
    fn is_noop(&self) -> bool {
        self.pick.is_empty() && self.head.is_none()
    }
}

/// Extract pick/head from tool-call arguments. `Err` is a caller-facing message.
pub fn shape_from(args: &Value) -> Result<Shape, String> {
    let mut shape = Shape::default();
    match args.get("pick") {
        None | Some(Value::Null) => {}
        Some(Value::Array(paths)) => {
            for p in paths {
                match p.as_str() {
                    Some(s) if !s.trim().is_empty() => shape.pick.push(s.trim().to_string()),
                    _ => return Err("pick must be an array of non-empty strings".into()),
                }
            }
        }
        Some(_) => return Err("pick must be an array of dot-paths".into()),
    }
    match args.get("head") {
        None | Some(Value::Null) => {}
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 => shape.head = Some(n as usize),
            _ => return Err("head must be a positive integer".into()),
        },
    }
    Ok(shape)
}

/// Resolve one dot-path against a JSON value. Objects are traversed by key; arrays map
/// the remaining path over their elements (or index, when the segment is numeric).
fn project_path(v: &Value, segs: &[&str]) -> Option<Value> {
    let Some(seg) = segs.first() else {
        return Some(v.clone());
    };
    match v {
        Value::Object(m) => m.get(*seg).and_then(|c| project_path(c, &segs[1..])),
        Value::Array(a) => {
            if let Ok(i) = seg.parse::<usize>() {
                return a.get(i).and_then(|c| project_path(c, &segs[1..]));
            }
            let hits: Vec<Value> = a.iter().filter_map(|e| project_path(e, segs)).collect();
            if hits.is_empty() { None } else { Some(Value::Array(hits)) }
        }
        _ => None,
    }
}

/// Truncate every array under `v` to its first `n` elements, recording `path: total`.
fn truncate_arrays(v: &mut Value, n: usize, path: &str, notes: &mut Vec<String>) {
    match v {
        Value::Array(a) => {
            if a.len() > n {
                notes.push(format!("{}: {} items, kept first {n}", if path.is_empty() { "(root)" } else { path }, a.len()));
                a.truncate(n);
            }
            for e in a.iter_mut() {
                truncate_arrays(e, n, path, notes);
            }
        }
        Value::Object(m) => {
            for (k, e) in m.iter_mut() {
                let child = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                truncate_arrays(e, n, &child, notes);
            }
        }
        _ => {}
    }
}

/// Apply `shape` to a JSON response body; returns the shaped text. Trailing note lines
/// report truncations and unmatched picks so the agent can self-correct.
fn shape_response(body: &Value, shape: &Shape) -> String {
    let mut notes: Vec<String> = Vec::new();
    let mut out = if shape.pick.is_empty() {
        body.clone()
    } else {
        let mut picked = serde_json::Map::new();
        for path in &shape.pick {
            let segs: Vec<&str> = path.split('.').collect();
            match project_path(body, &segs) {
                Some(v) => {
                    picked.insert(path.clone(), v);
                }
                None => notes.push(format!("pick \"{path}\": no match")),
            }
        }
        if picked.is_empty() {
            notes.push(format!("available top-level keys: {}", top_level_keys(body)));
        }
        Value::Object(picked)
    };
    if let Some(n) = shape.head {
        truncate_arrays(&mut out, n, "", &mut notes);
    }
    let mut text = serde_json::to_string(&out).unwrap_or_default();
    if !notes.is_empty() {
        text.push_str("\nnote: ");
        text.push_str(&notes.join("; "));
    }
    text
}

fn top_level_keys(v: &Value) -> String {
    match v {
        Value::Object(m) => m.keys().cloned().collect::<Vec<_>>().join(", "),
        Value::Array(a) => format!("(array of {} items)", a.len()),
        _ => "(scalar body)".into(),
    }
}

/// Actionable oversize report: per-key serialized sizes so the next call can pick.
fn size_error(body: Option<&Value>, total: usize) -> String {
    let mut msg = format!(
        "response too large for the agent channel ({} KB > {} KB limit).",
        total / 1024,
        MAX_RESULT_BYTES / 1024
    );
    if let Some(Value::Object(m)) = body {
        let mut sizes: Vec<(usize, String)> = m
            .iter()
            .map(|(k, v)| {
                let n = serde_json::to_string(v).map(|s| s.len()).unwrap_or(0);
                (n, k.clone())
            })
            .collect();
        sizes.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        let breakdown: Vec<String> =
            sizes.iter().map(|(n, k)| format!("{k}: {} KB", n.div_ceil(1024))).collect();
        msg.push_str(&format!(" Top-level keys: {}.", breakdown.join(", ")));
    }
    msg.push_str(
        " Re-call with \"pick\" (dot-paths of just the fields the user needs) and/or \
         \"head\" (cap arrays to N items), or narrow with query filters/limit.",
    );
    msg
}

/// Permission-check a concrete request against the allowlist, then serve it in-process.
/// (Network path — wraps the shared [`run_endpoint_inner`] in an MCP tool result.)
async fn run_endpoint(
    state: &AppState,
    auth: &otw_store::mcp::McpToken,
    id: Value,
    method: &str,
    path: &str,
    body: Option<Value>,
    shape: Shape,
) -> Response {
    match run_endpoint_inner(state, &auth.permissions, &auth.name, method, path, body, &shape).await
    {
        Ok((status, text)) => {
            if status.is_success() {
                tool_text(id, if text.is_empty() { format!("HTTP {status}") } else { text }, false)
            } else {
                tool_text(id, format!("HTTP {status}: {text}"), true)
            }
        }
        Err(msg) => tool_text(id, msg, true),
    }
}

/// Shared allowlist + permission check + in-process dispatch, independent of transport.
/// `Ok((status, body))` on a completed request (any HTTP status); `Err(text)` when the
/// request was rejected before dispatch (bad path, not allowlisted, insufficient perms) or
/// the internal dispatch itself errored — `text` is caller-facing.
async fn run_endpoint_inner(
    state: &AppState,
    perms: &Value,
    token_name: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
    shape: &Shape,
) -> Result<(StatusCode, String), String> {
    let bare = path.split('?').next().unwrap_or("");
    if !bare.starts_with("/api/") || path.contains("..") {
        return Err(format!("invalid path: {path}"));
    }
    let Some(endpoint) = catalog::lookup(method, bare) else {
        return Err(format!(
            "{method} {bare} is not an MCP-accessible endpoint; call otw_catalog to see what is."
        ));
    };
    let allowed = match method {
        "GET" => can_read(perms, endpoint.module),
        "DELETE" => can_delete(perms, endpoint.module),
        _ => can_write(perms, endpoint.module),
    };
    if !allowed {
        let need = match method {
            "GET" => "read",
            "DELETE" => "delete",
            _ => "write",
        };
        return Err(format!(
            "token \"{token_name}\" lacks {need} access to module \"{}\"",
            endpoint.module
        ));
    }

    tracing::info!("mcp[{token_name}]: {method} {bare}");
    let (status, text) = match dispatch(state, method, path, body).await {
        Ok(res) => res,
        Err(e) => {
            tracing::error!("mcp dispatch failed: {e:#}");
            return Err("internal dispatch error".into());
        }
    };
    if !status.is_success() {
        return Ok((status, text));
    }
    // Shape the successful JSON body; the agent-facing cap applies to the shaped output.
    let parsed: Option<Value> = serde_json::from_str(&text).ok();
    let out = match (&parsed, shape.is_noop()) {
        (Some(body), false) => shape_response(body, shape),
        _ => text,
    };
    if out.len() <= MAX_RESULT_BYTES {
        return Ok((status, out));
    }
    // Still over budget: keep scalars and mark oversized arrays; if even that fails,
    // report per-key sizes so the follow-up call can pick precisely.
    let shrunk = shrink_oversized(out.as_bytes());
    if shrunk.len() <= MAX_RESULT_BYTES && !shrunk.starts_with("[response too large") {
        return Ok((status, shrunk));
    }
    Err(size_error(parsed.as_ref(), out.len()))
}

/// Run the request through the real API router with the admin user injected
/// (single-user app; handlers that take `Extension<User>` see the same identity
/// a browser session would).
async fn dispatch(
    state: &AppState,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<(StatusCode, String)> {
    let router = DISPATCH
        .get()
        .ok_or_else(|| anyhow::anyhow!("mcp dispatch router not initialized"))?
        .clone();
    let user = otw_store::find_admin(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no admin user"))?;

    let bytes = match &body {
        // Some MCP clients send the body pre-serialized as a JSON string;
        // pass it through raw instead of double-encoding it. Trade-off: an endpoint whose
        // legitimate body IS a top-level JSON string can never receive one through MCP —
        // keep such endpoints out of the catalog (none exist today).
        Some(Value::String(s)) => s.clone().into_bytes(),
        Some(v) => serde_json::to_vec(v)?,
        None => Vec::new(),
    };
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))?;
    req.extensions_mut().insert(user);

    let resp = router.oneshot(req).await?;
    let status = resp.status();
    // Read with headroom and return the body raw: shaping (pick/head) must see the full
    // JSON, so any size reduction happens in the caller *after* projection.
    let bytes = axum::body::to_bytes(resp.into_body(), MAX_RAW_BYTES)
        .await
        .unwrap_or_else(|_| {
            Bytes::from_static(b"[response exceeds the raw dispatch cap; narrow with query filters/limit]")
        });
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}

/// Best-effort reduction of an over-budget JSON body: keep every scalar/small field, replace
/// each oversized array with a `[N items omitted …]` marker. Falls back to a plain notice when
/// the body is not a JSON object or is still too big afterwards.
fn shrink_oversized(bytes: &[u8]) -> String {
    const NOTICE: &str = "[response too large; narrow the query with filters/limit]";
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(bytes) else {
        return NOTICE.to_string();
    };
    // Drop the heaviest fields first, biggest last, until the whole thing fits.
    let mut kept: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut omitted: Vec<(String, usize, usize)> = Vec::new();
    for (k, v) in map {
        let weight = serde_json::to_vec(&v).map(|b| b.len()).unwrap_or(0);
        match &v {
            Value::Array(items) if weight > MAX_RESULT_BYTES / 8 => {
                omitted.push((k, items.len(), weight));
            }
            _ => {
                kept.insert(k, v);
            }
        }
    }
    for (k, n, _) in &omitted {
        kept.insert(
            k.clone(),
            Value::String(format!("[{n} items omitted: response too large]")),
        );
    }
    if !omitted.is_empty() {
        let names: Vec<&str> = omitted.iter().map(|(k, _, _)| k.as_str()).collect();
        kept.insert(
            "_truncated".into(),
            Value::String(format!(
                "Omitted large field(s): {}. Re-call with \"pick\" (dot-paths of just the fields \
                 you need) or \"head\" (cap arrays to N items); or narrow with limit/filter, or \
                 (for a backtest) pass \"view\":\"summary\" and read the full result from its run_id.",
                names.join(", ")
            )),
        );
    }
    let out = serde_json::to_string(&Value::Object(kept)).unwrap_or_else(|_| NOTICE.to_string());
    if out.len() > MAX_RESULT_BYTES {
        return NOTICE.to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bare_catalog_is_a_compact_index_without_endpoint_paths() {
        let perms = json!({ "journal": "r", "backtest": "rw" });
        let out = render_catalog(&perms, None);
        // Index lists accessible modules with counts…
        assert!(out.contains("journal — Trading Journal (read-only,"));
        assert!(out.contains("backtest — Backtest (read+write,"));
        // …but not the concrete endpoint paths (that's the drill-down).
        assert!(!out.contains("/api/journal/trades"));
        assert!(!out.contains("/api/backtest/run"));
        // Modules with no permission are hidden.
        assert!(!out.contains("wealth"));
    }

    #[test]
    fn module_catalog_lists_endpoints_and_respects_write_level() {
        // read-only sees GETs only.
        let ro = render_catalog(&json!({ "backtest": "r" }), Some("backtest"));
        assert!(ro.contains("GET /api/backtest/strategies"));
        assert!(!ro.contains("POST /api/backtest/strategies"));

        // read+write sees the creator/mutation endpoints too.
        let rw = render_catalog(&json!({ "backtest": "rw" }), Some("backtest"));
        assert!(rw.contains("POST /api/backtest/strategies"));
        assert!(rw.contains("POST /api/backtest/indicators"));
    }

    #[test]
    fn delete_endpoints_need_full_level() {
        // rw sees writes but NOT deletes; rwd sees everything.
        let rw = render_catalog(&json!({ "journal": "rw" }), Some("journal"));
        assert!(!rw.contains("DELETE"));
        assert!(rw.contains("read+write"));

        let full = render_catalog(&json!({ "journal": "rwd" }), Some("journal"));
        assert!(full.contains("DELETE"));
        assert!(full.contains("full (read+write+delete)"));

        // Enforcement matches visibility.
        assert!(can_write(&json!({ "journal": "rw" }), "journal"));
        assert!(!can_delete(&json!({ "journal": "rw" }), "journal"));
        assert!(can_delete(&json!({ "journal": "rwd" }), "journal"));
    }

    #[test]
    fn pick_extracts_scalars_and_maps_over_arrays() {
        let body = json!({
            "ticker": "BTCUSDT",
            "result": { "var_hist": 0.031, "cvar": 0.045, "drawdown_curve": [1, 2, 3] },
            "positions": [
                { "symbol": "AAPL", "value": 10.0 },
                { "symbol": "MSFT", "value": 20.0 },
            ],
        });
        let shape = shape_from(&json!({
            "pick": ["result.var_hist", "positions.symbol", "positions.1.value", "nope.x"]
        }))
        .unwrap();
        let out = shape_response(&body, &shape);
        let (json_part, note) = out.split_once("\nnote: ").unwrap();
        let v: Value = serde_json::from_str(json_part).unwrap();
        assert_eq!(v["result.var_hist"], json!(0.031));
        assert_eq!(v["positions.symbol"], json!(["AAPL", "MSFT"]));
        assert_eq!(v["positions.1.value"], json!(20.0));
        assert!(v.get("nope.x").is_none());
        assert!(note.contains("pick \"nope.x\": no match"));
    }

    #[test]
    fn head_truncates_arrays_and_reports_totals() {
        let body = json!({ "items": [1, 2, 3, 4, 5], "meta": { "tags": ["a"] } });
        let shape = shape_from(&json!({ "head": 2 })).unwrap();
        let out = shape_response(&body, &shape);
        let (json_part, note) = out.split_once("\nnote: ").unwrap();
        let v: Value = serde_json::from_str(json_part).unwrap();
        assert_eq!(v["items"], json!([1, 2]));
        assert_eq!(v["meta"]["tags"], json!(["a"]));
        assert!(note.contains("items: 5 items, kept first 2"));
    }

    #[test]
    fn shape_from_rejects_bad_arguments() {
        assert!(shape_from(&json!({ "pick": "result" })).is_err());
        assert!(shape_from(&json!({ "pick": [1] })).is_err());
        assert!(shape_from(&json!({ "head": 0 })).is_err());
        assert!(shape_from(&json!({ "head": -3 })).is_err());
        assert!(shape_from(&json!({})).unwrap().is_noop());
    }

    #[test]
    fn size_error_reports_per_key_sizes() {
        let body = json!({ "stats": { "sharpe": 1.2 }, "equity": vec![0.0f64; 1000] });
        let msg = size_error(Some(&body), 900 * 1024);
        assert!(msg.contains("900 KB"));
        assert!(msg.contains("equity:"));
        assert!(msg.contains("stats:"));
        assert!(msg.contains("pick"));
    }

    #[test]
    fn module_catalog_carries_body_schemas_for_writes() {
        let out = render_catalog(&json!({ "histdata": "rw" }), Some("histdata"));
        println!("{out}");
        // The download endpoint advertises its real contract…
        assert!(out.contains("POST /api/histdata/downloads"));
        let dl = out
            .lines()
            .skip_while(|l| !l.starts_with("POST /api/histdata/downloads"))
            .nth(1)
            .expect("schema line follows the endpoint line");
        assert!(dl.trim_start().starts_with("body schema: "));
        for field in ["\"asset_type\"", "\"ticker\"", "\"timeframe\"", "\"from\"", "\"to\""] {
            assert!(dl.contains(field), "schema line missing {field}");
        }
        // …and GETs stay schema-free.
        assert!(!out.contains("GET /api/histdata/jobs — Download queue/job status.\n  body"));
    }

    #[test]
    fn module_catalog_hides_forbidden_and_unknown_modules() {
        let perms = json!({ "backtest": "rw" });
        assert!(render_catalog(&perms, Some("wealth")).contains("No accessible endpoints"));
        assert!(render_catalog(&perms, Some("nope")).contains("Unknown module"));
    }
}
