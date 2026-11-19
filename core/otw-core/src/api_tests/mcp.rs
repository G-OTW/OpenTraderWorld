//! The MCP gateway over JSON-RPC, through `POST /api/mcp` as a client reaches it: the tool
//! list and the catalog they front, every tool with a valid call, invalid arguments and a
//! resource that does not exist, the protocol errors, and the allowlist's exclusions.
//!
//! The gateway's rendering and validation have unit tests in `mcp::tests`. Only one test
//! here dispatches into the API (`valid_calls_reach_the_api_and_answer_its_result`): the
//! in-process dispatch router is a process-wide `OnceLock` that `main` sets once, so that
//! test sets it over its own database and every other test stays clear of dispatching.

use axum::http::StatusCode;
use axum::Router;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::{app, request, send, state_over};
use crate::mcp::catalog::{self, CATALOG};

/// Every module at full access.
fn all_modules() -> Value {
    Value::Object(catalog::MODULES.iter().map(|(m, _)| (m.to_string(), json!("rwd"))).collect())
}

struct Gateway {
    app: Router,
    token: String,
    pool: PgPool,
}

impl Gateway {
    /// The gateway switched on, with an admin and one token holding `perms`.
    async fn new(pool: PgPool, perms: Value) -> Self {
        otw_store::settings::set(&pool, "mcp_enabled", "true").await.unwrap();
        if otw_store::find_admin(&pool).await.unwrap().is_none() {
            otw_store::create_admin(&pool, "owner", "not-a-real-hash", false).await.unwrap();
        }
        let (_, token) = otw_store::mcp::create_token(&pool, "tests", &perms, None, false).await.unwrap();
        let app = app(state_over(pool.clone()).await);
        Gateway { app, token, pool }
    }

    async fn post(&self, body: &str) -> (StatusCode, Value) {
        let bearer = format!("Bearer {}", self.token);
        send(&self.app, request("POST", "/api/mcp", Some(body), None, &[("authorization", &bearer)])).await
    }

    async fn rpc(&self, method: &str, params: Value) -> Value {
        let (status, body) = self.post(&json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params }).to_string()).await;
        assert_eq!(status, StatusCode::OK, "{method}: {body}");
        assert_eq!((&body["jsonrpc"], &body["id"]), (&json!("2.0"), &json!(7)), "{body}");
        body
    }

    /// A tool call's text and whether it was flagged as an error.
    async fn call(&self, tool: &str, args: Value) -> (String, bool) {
        let body = self.rpc("tools/call", json!({ "name": tool, "arguments": args })).await;
        let result = &body["result"];
        assert!(body.get("error").is_none(), "{tool} answered a protocol error: {body}");
        let text = result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{tool}: {body}")).to_string();
        (text, result["isError"].as_bool().expect("isError is always set"))
    }

    async fn todo_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM todos").fetch_one(&self.pool).await.unwrap()
    }
}

// ── The catalog (no database) ───────────────────────────────────────────────

/// One line per endpoint: method, path, module, `compute`, and the body's top-level fields
/// (`*` = required). A route added, dropped, moved to another module, or a body field
/// renamed shows up here for review.
#[test]
fn catalog_snapshot() {
    let mut lines: Vec<String> = CATALOG
        .iter()
        .map(|e| {
            let mut line = format!("{} {} [{}]", e.method, e.path, e.module);
            if e.compute {
                line.push_str(" compute");
            }
            if let Some(body) = e.body {
                let schema = body();
                let required: Vec<&str> = schema["required"].as_array().map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                let mut fields: Vec<String> = schema["properties"]
                    .as_object()
                    .map(|p| p.keys().map(|k| if required.contains(&k.as_str()) { format!("{k}*") } else { k.clone() }).collect())
                    .unwrap_or_default();
                fields.sort();
                line.push_str(&format!(" body{{{}}}", fields.join(",")));
            }
            line
        })
        .collect();
    lines.sort();
    insta::assert_snapshot!("catalog", lines.join("\n"));
}

/// Nothing that mints a credential, reads or changes a secret, rewires the network, wipes
/// or moves a whole store, serves files or streams is reachable by an agent token.
#[test]
fn no_secret_minting_bulk_write_file_or_stream_route_is_on_the_catalog() {
    const FORBIDDEN_PREFIXES: &[&str] = &[
        // Credentials and secrets: minted, stored, or revealed.
        "/api/mcp",
        "/api/vault",
        "/api/webhooks",
        "/api/hooks",
        "/api/social-login",
        "/api/agent/providers",
        "/api/mailbox/oauth",
        "/api/notif-channels",
        "/api/control",
        "/api/brokers",
        // Account, session, network and the whole-store moves.
        "/api/login",
        "/api/setup",
        "/api/network",
        "/api/settings/account",
        "/api/settings/data",
        "/api/settings/sessions",
        "/api/settings/totp",
        "/api/settings/reauth",
        "/api/settings/logs",
        "/api/settings/logout",
        "/api/settings/modules",
        "/api/settings/trading-lock",
        "/api/settings/social",
        "/api/data-transfer",
        // Bulk imports.
        "/api/journal/import",
        "/api/portfolios/import",
        "/api/histdata/import",
        // Raw files.
        "/api/files",
    ];
    let under = |path: &str, prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
    let bad: Vec<String> = CATALOG
        .iter()
        .filter(|e| FORBIDDEN_PREFIXES.iter().any(|p| under(e.path, p)))
        .map(|e| format!("{} {}", e.method, e.path))
        .collect();
    assert!(bad.is_empty(), "on the MCP allowlist but must not be: {bad:?}");

    // Credential writes anywhere (a connector's or a broker's key).
    let creds: Vec<String> = CATALOG
        .iter()
        .filter(|e| e.method != "GET" && (e.path.contains("/cred") || e.path.contains("/secret")))
        .map(|e| format!("{} {}", e.method, e.path))
        .collect();
    assert!(creds.is_empty(), "credential writes on the allowlist: {creds:?}");

    // A stream holds the connection open forever: the dispatcher reads a body to its end.
    let streams: Vec<String> = CATALOG
        .iter()
        .filter(|e| e.path.ends_with("/stream") || e.path.ends_with("/streams") || e.path.contains("/stream/"))
        .map(|e| format!("{} {}", e.method, e.path))
        .collect();
    assert!(streams.is_empty(), "server-sent event routes on the allowlist: {streams:?}");
}

// ── The protocol ────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn protocol_errors_are_clean_json_rpc_errors(pool: PgPool) {
    let gw = Gateway::new(pool, json!({ "todos": "r" })).await;

    let body = gw.rpc("no/such/method", json!({})).await;
    assert_eq!(body["error"]["code"], -32601, "{body}");

    let body = gw.rpc("tools/call", json!({ "name": "rm_rf", "arguments": {} })).await;
    assert_eq!(body["error"]["code"], -32602, "an unknown tool is invalid params: {body}");

    let (status, body) = gw.post("{not json").await;
    assert_eq!((status, &body["error"]["code"], &body["id"]), (StatusCode::OK, &json!(-32700), &Value::Null), "{body}");

    let (status, body) = gw.post(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#).await;
    assert_eq!((status, &body["error"]["code"]), (StatusCode::OK, &json!(-32600)), "no batching: {body}");

    let (status, _) = gw.post(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await;
    assert_eq!(status, StatusCode::ACCEPTED, "a notification has no answer");

    let body = gw.rpc("initialize", json!({ "protocolVersion": "2025-06-18" })).await;
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    let body = gw.rpc("ping", json!({})).await;
    assert_eq!(body["result"]["resultType"], "complete");
}

/// The gateway is shut until switched on, and a bearer it does not know is refused with
/// a 401 before any method runs.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn the_gateway_is_shut_until_enabled_and_needs_a_known_bearer(pool: PgPool) {
    let gw = Gateway::new(pool.clone(), json!({ "todos": "r" })).await;
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let unknown = format!("Bearer otw_mcp_{}", "0".repeat(64));
    let (status, _) = send(&gw.app, request("POST", "/api/mcp", Some(ping), None, &[("authorization", &unknown)])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    otw_store::settings::set(&pool, "mcp_enabled", "false").await.unwrap();
    let (status, body) = gw.post(ping).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

// ── tools/list ──────────────────────────────────────────────────────────────

/// The four tools and their input schemas, for a token that may do everything. A token that
/// may only read is offered the two read tools and nothing that writes.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn tools_list_snapshot(pool: PgPool) {
    let gw = Gateway::new(pool.clone(), all_modules()).await;
    let body = gw.rpc("tools/list", json!({})).await;
    insta::assert_yaml_snapshot!("tools_list", body["result"], {
        // The server's version rides here; it changes with every release.
        "._meta" => "[server meta]",
    });

    let (_, read_only) = otw_store::mcp::create_token(&pool, "ro", &json!({ "journal": "r", "todos": "r" }), None, false).await.unwrap();
    let bearer = format!("Bearer {read_only}");
    let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let (_, body) = send(&gw.app, request("POST", "/api/mcp", Some(list), None, &[("authorization", &bearer)])).await;
    let names: Vec<&str> = body["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, vec!["otw_catalog", "otw_read"]);
}

// ── Invalid arguments and unknown resources ─────────────────────────────────

/// A malformed call is a tool result the model can read and fix (`isError`), never a
/// protocol error, and it writes nothing.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn invalid_arguments_are_an_error_result_per_tool(pool: PgPool) {
    let gw = Gateway::new(pool, all_modules()).await;
    let cases: &[(&str, Value, &str)] = &[
        ("otw_catalog", json!({ "module": "no-such-module" }), "no-such-module"),
        ("otw_catalog", json!({ "endpoints": [] }), "at least one"),
        ("otw_read", json!({}), "requires a \"path\" string"),
        ("otw_read", json!({ "path": "/api/todos", "head": 0 }), "head"),
        ("otw_read", json!({ "path": "/api/todos", "pick": "name" }), "pick"),
        ("otw_compute", json!({}), "requires a \"path\" string"),
        ("otw_compute", json!({ "path": "/api/todos", "body": {} }), "otw_write"),
        ("otw_compute", json!({ "path": "/api/backtest/run", "validate_only": "yes" }), "validate_only"),
        ("otw_write", json!({ "method": "GET", "path": "/api/todos" }), "POST, PUT, PATCH or DELETE"),
        ("otw_write", json!({ "method": "POST" }), "requires a \"path\" string"),
        ("otw_write", json!({ "method": "POST", "path": "/api/todos", "body": { "name": "x" }, "idempotency_key": "" }), "idempotency_key"),
        ("otw_write", json!({ "method": "POST", "path": "/api/todos", "body": { "name": 42 } }), "invalid body"),
        ("otw_write", json!({ "method": "POST", "path": "/api/todos" }), "needs a JSON request body"),
    ];
    for (tool, args, says) in cases {
        let (text, is_error) = gw.call(tool, args.clone()).await;
        assert!(is_error, "{tool} {args} was accepted: {text}");
        assert!(text.contains(says), "{tool} {args}: expected {says:?} in {text}");
    }
    assert_eq!(gw.todo_count().await, 0, "no refused call wrote anything");
}

/// A path the API does not serve, one it serves but keeps off the allowlist, and one in a
/// module the token holds no grant on are each refused by name, without being dispatched.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn an_unknown_or_unlisted_resource_is_refused_by_name(pool: PgPool) {
    let gw = Gateway::new(pool.clone(), json!({ "todos": "rwd" })).await;
    let tokens_before: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_tokens").fetch_one(&pool).await.unwrap();
    let cases: &[(&str, Value)] = &[
        ("otw_read", json!({ "path": "/api/no-such-thing" })),
        ("otw_read", json!({ "path": "/api/todoz" })),
        ("otw_read", json!({ "path": "/api/../api/todos" })),
        ("otw_read", json!({ "path": "/api/settings/data/export" })),
        ("otw_read", json!({ "path": "/api/journal/trades" })),
        ("otw_write", json!({ "method": "POST", "path": "/api/mcp/tokens", "body": { "name": "evil", "permissions": { "todos": "rwd" } } })),
        ("otw_write", json!({ "method": "POST", "path": "/api/settings/data/import", "body": {} })),
        ("otw_write", json!({ "method": "DELETE", "path": "/api/vault/items/00000000-0000-0000-0000-000000000001" })),
    ];
    for (tool, args) in cases {
        let (text, is_error) = gw.call(tool, args.clone()).await;
        assert!(is_error, "{tool} {args} was served: {text}");
    }
    let (text, _) = gw.call("otw_read", json!({ "path": "/api/todoz" })).await;
    assert!(text.contains("/api/todos"), "a near miss names the real route: {text}");
    let tokens_after: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_tokens").fetch_one(&pool).await.unwrap();
    assert_eq!(tokens_after, tokens_before, "no token was minted through the gateway");
}

// ── Valid calls ─────────────────────────────────────────────────────────────

/// One valid call per tool, end to end. The only test that dispatches into the API (see the
/// module doc): it sets the in-process router over its own database.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn valid_calls_reach_the_api_and_answer_its_result(pool: PgPool) {
    let gw = Gateway::new(pool.clone(), json!({ "todos": "rwd", "backtest": "rw" })).await;
    let state = state_over(pool.clone()).await;
    crate::mcp::init_dispatch(super::api_routes().with_state(state));

    let (text, is_error) = gw.call("otw_catalog", json!({ "module": "todos" })).await;
    assert!(!is_error && text.contains("POST /api/todos"), "{text}");

    let (text, is_error) = gw.call("otw_write", json!({ "method": "POST", "path": "/api/todos", "body": { "name": "from an agent" } })).await;
    assert!(!is_error, "{text}");
    assert_eq!(gw.todo_count().await, 1);

    let (text, is_error) = gw.call("otw_read", json!({ "path": "/api/todos", "pick": ["todos.name"] })).await;
    assert!(!is_error && text.contains("from an agent"), "{text}");

    // A resource that does not exist: the API's 404, as an error result.
    let missing = uuid::Uuid::new_v4();
    let (text, is_error) = gw.call("otw_read", json!({ "path": format!("/api/todos/{missing}") })).await;
    assert!(is_error && text.contains("404"), "{text}");

    // The write is replayed, not repeated, under the same idempotency key.
    let again = json!({ "method": "POST", "path": "/api/todos", "body": { "name": "once" }, "idempotency_key": "k-1" });
    let (_, e1) = gw.call("otw_write", again.clone()).await;
    let (text, e2) = gw.call("otw_write", again).await;
    assert!(!e1 && !e2 && text.contains("replayed"), "{text}");
    assert_eq!(gw.todo_count().await, 2);

    // Compute: a body checked against the endpoint's own type, nothing run.
    let run = catalog::lookup("POST", "/api/backtest/run").and_then(|e| e.example).map(|f| f()).unwrap();
    let (text, is_error) = gw.call("otw_compute", json!({ "path": "/api/backtest/run", "body": run, "validate_only": true })).await;
    assert!(!is_error && text.starts_with("ok: body is valid"), "{text}");
}
