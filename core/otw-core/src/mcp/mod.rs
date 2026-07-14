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
            passing concrete paths like /api/journal/trades?limit=20. All bodies are JSON; dates \
            are YYYY-MM-DD unless an endpoint says otherwise.",
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
            "description": "GET an allowlisted endpoint. Returns the JSON response body.",
            "inputSchema": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "API path with optional query string, e.g. /api/journal/trades?limit=20" }
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
                    "body": { "description": "JSON request body, when the endpoint expects one." }
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
            run_endpoint(state, auth, id, "GET", path, None).await
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
            run_endpoint(state, auth, id, &method, path, args.get("body").cloned()).await
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

/// Permission-check a concrete request against the allowlist, then serve it in-process.
async fn run_endpoint(
    state: &AppState,
    auth: &otw_store::mcp::McpToken,
    id: Value,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Response {
    let bare = path.split('?').next().unwrap_or("");
    if !bare.starts_with("/api/") || path.contains("..") {
        return tool_text(id, format!("invalid path: {path}"), true);
    }
    let Some(endpoint) = catalog::lookup(method, bare) else {
        return tool_text(
            id,
            format!("{method} {bare} is not an MCP-accessible endpoint; call otw_catalog to see what is."),
            true,
        );
    };
    let allowed = match method {
        "GET" => can_read(&auth.permissions, endpoint.module),
        "DELETE" => can_delete(&auth.permissions, endpoint.module),
        _ => can_write(&auth.permissions, endpoint.module),
    };
    if !allowed {
        let need = match method {
            "GET" => "read",
            "DELETE" => "delete",
            _ => "write",
        };
        return tool_text(
            id,
            format!("token \"{}\" lacks {} access to module \"{}\"",
                auth.name, need, endpoint.module),
            true,
        );
    }

    tracing::info!("mcp[{}]: {} {}", auth.name, method, bare);
    match dispatch(state, method, path, body).await {
        Ok((status, text)) => {
            if status.is_success() {
                tool_text(id, if text.is_empty() { format!("HTTP {status}") } else { text }, false)
            } else {
                tool_text(id, format!("HTTP {status}: {text}"), true)
            }
        }
        Err(e) => {
            tracing::error!("mcp dispatch failed: {e:#}");
            tool_text(id, "internal dispatch error".into(), true)
        }
    }
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
        // pass it through raw instead of double-encoding it.
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
    let bytes = axum::body::to_bytes(resp.into_body(), MAX_RESULT_BYTES)
        .await
        .unwrap_or_else(|_| Bytes::from_static(b"[response too large; narrow the query with filters/limit]"));
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
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
    fn module_catalog_hides_forbidden_and_unknown_modules() {
        let perms = json!({ "backtest": "rw" });
        assert!(render_catalog(&perms, Some("wealth")).contains("No accessible endpoints"));
        assert!(render_catalog(&perms, Some("nope")).contains("Unknown module"));
    }
}
