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
use crate::mcp::{catalog, validate};

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

    // The contract the catalog publishes, checked as the MCP gateway checks an agent's call:
    // a param the handler never reads (`ignored`) or a value it will refuse (`invalid`) is
    // written into the trace, so a workflow does not walk into a silent empty result.
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let findings = contract_findings(endpoint, query, body.as_ref());

    let mut request = json!({
        "method": method,
        "path": path,
        "module": endpoint.module,
        "body": body.clone().unwrap_or(Value::Null),
    });
    if let Some(v) = &findings {
        request["validation"] = v.clone();
    }

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
        let mut msg = format!("{method} {bare} answered {status}: {detail}");
        if let Some(v) = &findings {
            msg.push_str(&format!("\nrequest check: {}", findings_line(v)));
        }
        return Err(msg);
    }
    let mut output = json!({ "status": status.as_u16(), "body": parsed });
    if let Some(v) = findings {
        output["validation"] = v;
    }
    Ok(Outcome::new(request, output))
}

/// What `mcp::validate` finds in this call against the endpoint's published schemas, as
/// `{invalid: [...], ignored: [...]}`, or `None` when the call is clean.
fn contract_findings(endpoint: &catalog::Endpoint, query: &str, body: Option<&Value>) -> Option<Value> {
    let query_schema = endpoint.query.map(|q| q());
    let mut f = validate::check_query(query_schema.as_ref(), query);
    match (endpoint.body, body) {
        (Some(schema), Some(b)) => f.extend(validate::check_body(&schema(), b)),
        (None, Some(b)) if !b.is_null() => {
            f.ignored.push("body (this endpoint takes no request body)".into());
        }
        (Some(_), None) => f.invalid.push("body: this endpoint needs a JSON request body".into()),
        _ => {}
    }
    if f.invalid.is_empty() && f.ignored.is_empty() {
        return None;
    }
    Some(json!({ "invalid": f.invalid, "ignored": f.ignored }))
}

/// The findings on one line, for an error message.
fn findings_line(v: &Value) -> String {
    let list = |k: &str| -> Vec<String> {
        v[k].as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(|s| format!("{k}: {s}")).collect())
            .unwrap_or_default()
    };
    [list("invalid"), list("ignored")].concat().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_body_key_is_an_ignored_finding() {
        let ep = catalog::lookup("POST", "/api/quant/seasonality").expect("catalogued");
        let id = "00000000-0000-0000-0000-000000000001";
        let v = contract_findings(ep, "", Some(&json!({ "dataset_id": id, "metrik": "return" })))
            .expect("a finding");
        let ignored = v["ignored"].to_string();
        assert!(ignored.contains("metrik"), "{v}");
        assert!(findings_line(&v).starts_with("ignored: "), "{v}");
        assert!(contract_findings(ep, "", Some(&json!({ "dataset_id": id }))).is_none());
    }

    #[test]
    fn a_bad_query_value_is_an_invalid_finding() {
        let ep = catalog::lookup("GET", "/api/automator/runs").expect("catalogued");
        let v = contract_findings(ep, "status=success", None).expect("a finding");
        assert!(v["invalid"].to_string().contains("success"), "{v}");
        let v = contract_findings(ep, "statuss=ok", None).expect("a finding");
        assert!(v["ignored"].to_string().contains("statuss"), "{v}");
        assert!(contract_findings(ep, "status=ok", None).is_none());
    }
}
