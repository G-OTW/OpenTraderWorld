//! The demo sandbox gate in front of the real router. Its route table has unit tests of its
//! own (`demo::tests`); these check what a visitor actually gets back: a refusal that never
//! reaches a handler, and reads that do. No database: a refused request must not need one.

use axum::http::StatusCode;

use super::{demo_app, offline_state, request, send};

/// Downloads, outbound sends and bulk writes, each on a route the app really serves.
const DENIED: &[(&str, &str, &str)] = &[
    // Downloads: the data leaves the sandbox.
    ("download", "GET", "/api/histdata/datasets/00000000-0000-0000-0000-000000000001/export"),
    ("download", "GET", "/api/settings/data/export"),
    ("download", "GET", "/api/mailbox/attachments/00000000-0000-0000-0000-000000000001"),
    // Sends: a message or a request leaves the app.
    ("send", "POST", "/api/notif-channels/00000000-0000-0000-0000-000000000001/test"),
    ("send", "POST", "/api/community-docs/submit"),
    ("send", "POST", "/api/mailbox/messages/00000000-0000-0000-0000-000000000001/unsubscribe"),
    ("send", "POST", "/api/mcp"),
    // Bulk writes: a whole store replaced or loaded from outside.
    ("bulk write", "POST", "/api/settings/data/import"),
    ("bulk write", "POST", "/api/journal/import/broker"),
    ("bulk write", "POST", "/api/portfolios/00000000-0000-0000-0000-000000000001/import/broker"),
    ("bulk write", "POST", "/api/histdata/jobs"),
    ("bulk write", "POST", "/api/findb/install"),
];

#[tokio::test]
async fn the_demo_refuses_downloads_sends_and_bulk_writes_before_any_handler() {
    let app = demo_app(offline_state().await);
    let mut first = None;
    for (kind, method, path) in DENIED {
        let (status, body) = send(&app, request(method, path, Some("{}"), None, &[("x-forwarded-for", "203.0.113.7")])).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{kind} {method} {path}: {body}");
        assert_eq!(body["error"], "demo_disabled", "{kind} {method} {path}: {body}");
        first.get_or_insert(body);
    }
    insta::assert_yaml_snapshot!("demo_refusal_body", first.unwrap());
}

/// What the gate lets through reaches the app's own guards: a read is answered by the
/// session check, not by the gate.
#[tokio::test]
async fn the_demo_lets_reads_through_to_the_app() {
    let app = demo_app(offline_state().await);
    let ip = [("x-forwarded-for", "203.0.113.8")];
    for path in ["/api/todos", "/api/journal/trades", "/api/backtest/strategies"] {
        let (status, body) = send(&app, request("GET", path, None, None, &ip)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path} passed the gate to the session check: {body}");
        assert_eq!(body["error"], "not signed in");
    }
    let (status, body) = send(&app, request("GET", "/api/demo", None, None, &ip)).await;
    assert_eq!((status, body), (StatusCode::OK, serde_json::json!({ "demo": false })), "this process is not the demo");
}

/// A write under a read-only prefix is refused even though the same path reads.
#[tokio::test]
async fn the_demo_reads_a_read_only_module_but_never_writes_it() {
    let app = demo_app(offline_state().await);
    let ip = [("x-forwarded-for", "203.0.113.9")];
    let (status, _) = send(&app, request("GET", "/api/mcp/tokens", None, None, &ip)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the read passes the gate");
    let (status, body) = send(&app, request("POST", "/api/mcp/tokens", Some("{}"), None, &ip)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "demo_disabled");
}
