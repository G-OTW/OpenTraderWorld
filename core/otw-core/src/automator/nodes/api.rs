//! `api`: call OpenTraderWorld's own API from a workflow.
//!
//! Reuses the MCP security model wholesale rather than inventing a second one:
//! the endpoint must be in `mcp::catalog` (so settings, network, secrets, wipe, files,
//! streams and the bulk importers are unreachable, exactly as they are for an agent), and
//! the workflow's `mcp_tokens` row must grant the endpoint's module at the right level.
//! A workflow with no token attached cannot make an internal call at all.
//!
//! The call runs in-process against the real router (`crate::internal_call`), so a
//! workflow and a browser session get identical behavior from the same handler.

use serde_json::{json, Value};

use super::{NodeCtx, Outcome};
use crate::mcp::catalog;

pub fn validate(config: &Value) -> Result<(), String> {
    let method = super::opt(config, "method", "GET").to_ascii_uppercase();
    let path = super::field(config, "path")?;
    if !path.starts_with("/api/") {
        return Err("the path must start with /api/".into());
    }
    if path.contains("..") {
        return Err("the path cannot contain \"..\"".into());
    }
    // A path with a template in it can only be checked at run time; a literal one is
    // checked now, which is the common case and the useful one.
    if !path.contains("{{") {
        let bare = path.split('?').next().unwrap_or("");
        if catalog::lookup(&method, bare).is_none() {
            let others = catalog::methods_for(bare);
            return if others.is_empty() {
                Err(format!("{method} {bare} is not an endpoint a workflow can call"))
            } else {
                Err(format!("{bare} accepts {} here, not {method}", others.join(", ")))
            };
        }
    }
    Ok(())
}

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let config = &ctx.node.config;
    let method = super::opt(config, "method", "GET").to_ascii_uppercase();
    let path = ctx.resolver.render_str(super::field(config, "path")?, ctx.vars, false).await?;
    let body = match config.get("body") {
        Some(Value::Null) | None => None,
        Some(raw) => {
            let rendered = ctx.resolver.render_value(raw, ctx.vars, true).await?;
            match &rendered {
                Value::Object(o) if o.is_empty() => None,
                _ => Some(rendered),
            }
        }
    };

    let bare = path.split('?').next().unwrap_or("");
    if !bare.starts_with("/api/") || path.contains("..") {
        return Err(format!("invalid path: {path}"));
    }
    let Some(endpoint) = catalog::lookup(&method, bare) else {
        return Err(format!("{method} {bare} is not an endpoint a workflow can call"));
    };

    if ctx.perms.as_object().is_none_or(|m| m.is_empty()) {
        return Err(
            "this workflow has no access token: attach one in its settings before it can call \
             the app's own API"
                .into(),
        );
    }
    let allowed = match method.as_str() {
        "GET" => crate::mcp::can_read(&ctx.perms, endpoint.module),
        "DELETE" => crate::mcp::can_delete(&ctx.perms, endpoint.module),
        _ => crate::mcp::can_write(&ctx.perms, endpoint.module),
    };
    if !allowed {
        let need = match method.as_str() {
            "GET" => "read",
            "DELETE" => "delete",
            _ => "write",
        };
        return Err(format!(
            "the token \"{}\" does not grant {need} access to \"{}\"",
            ctx.token_name, endpoint.module
        ));
    }

    let request = json!({
        "method": method,
        "path": path,
        "module": endpoint.module,
        "body": body.clone().unwrap_or(Value::Null),
    });

    // A test run answers questions but never changes anything: reads and the endpoints the
    // catalog marks as compute run, everything else reports the exact call it would make.
    if ctx.mode.is_test() && method != "GET" && !endpoint.compute {
        return Ok(Outcome::simulated(request, "test run: writes are not executed"));
    }

    let (status, text) = crate::internal_call::call(ctx.state, &method, &path, body)
        .await
        .map_err(|e| {
            tracing::error!("automator internal call failed: {e:#}");
            "internal dispatch error".to_string()
        })?;

    let parsed: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()));
    if !status.is_success() {
        let detail = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| crate::agent::truncate_plain(&text, 300));
        return Err(format!("{method} {bare} answered {status}: {detail}"));
    }
    Ok(Outcome::new(
        request,
        json!({ "status": status.as_u16(), "body": parsed }),
    ))
}
