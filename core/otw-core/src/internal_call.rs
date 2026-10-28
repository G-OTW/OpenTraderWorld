//! In-process API calls: run the real Axum handlers without an HTTP hop.
//!
//! One router is captured at boot (state applied, no session middleware) and every
//! internal caller drives it with `tower::ServiceExt::oneshot`, injecting the admin user
//! so handlers taking `Extension<User>` see exactly what a browser session would.
//! Behavior therefore always matches the REST API: there is one implementation of every
//! endpoint, not a copy per consumer.
//!
//! Two callers today: the MCP gateway (`crate::mcp`) and the Automator's `api` node
//! (`crate::automator::nodes::api`). Neither of them is a permission check: the caller
//! decides what it may reach (catalog allowlist + token levels) *before* calling here.

use std::sync::OnceLock;

use axum::{
    body::{Body, Bytes},
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::Value;
use tower::ServiceExt;

use crate::AppState;

/// Cap on raw bytes read from one dispatch. Callers shape/truncate afterwards, so this is
/// deliberately generous: the projection must see the whole JSON.
pub const MAX_RAW_BYTES: usize = 16 * 1024 * 1024;

/// Marks a request as coming from an internal caller (the MCP gateway, so an agent, or an
/// Automator `api` block) rather than from someone at the keyboard. Injected on every
/// dispatch below; a browser request can never carry it, since extensions are set
/// server-side and never read off the wire.
///
/// Almost no handler cares: the caller's reach is already decided before it gets here
/// (catalog allowlist + token levels). It exists for the handful of endpoints where the
/// *same* action means something different depending on who asked, the Automator's graph
/// save being the one today: an agent may propose a graph, only a human may make it the
/// one that runs.
#[derive(Clone, Copy)]
pub struct Automated;

/// The full API router used to serve internal calls. Set once from `main`.
static DISPATCH: OnceLock<Router> = OnceLock::new();

pub fn init(router: Router) {
    let _ = DISPATCH.set(router);
}

/// Run one request through the real API router with the admin user injected.
/// Returns the response status and its body as text (lossy UTF-8: binary endpoints are
/// kept out of every internal caller's reach for exactly that reason).
pub async fn call(
    state: &AppState,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<(StatusCode, String)> {
    let router = DISPATCH
        .get()
        .ok_or_else(|| anyhow::anyhow!("internal dispatch router not initialized"))?
        .clone();
    let user = otw_store::find_admin(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no admin user"))?;

    let bytes = match &body {
        // Some MCP clients send the body pre-serialized as a JSON string; pass it through
        // raw instead of double-encoding it. Trade-off: an endpoint whose legitimate body
        // IS a top-level JSON string can never be reached this way, so keep such endpoints
        // out of the catalog (none exist today).
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
    req.extensions_mut().insert(Automated);

    let resp = router.oneshot(req).await?;
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), MAX_RAW_BYTES)
        .await
        .unwrap_or_else(|_| {
            Bytes::from_static(
                b"[response exceeds the raw dispatch cap; narrow with query filters/limit]",
            )
        });
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}
