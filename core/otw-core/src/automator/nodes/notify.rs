//! `notify`: the end of most workflows.
//!
//! No channel management lives here: destinations are the shared notification broker's
//! channels, filtered by the `automator` grant server-side. An empty `channels` list means
//! every enabled channel granted to the module, which is the RemindMe behavior; a non-empty
//! one narrows to those, and the grant still applies on top.

use serde_json::{json, Value};
use uuid::Uuid;

use super::{NodeCtx, Outcome};

const MAX_TITLE: usize = 200;
const MAX_BODY: usize = 4000;

pub fn validate(config: &Value) -> Result<(), String> {
    super::field(config, "title")?;
    if let Some(list) = config.get("channels") {
        if !list.is_array() {
            return Err("channels must be a list".into());
        }
        for c in list.as_array().expect("checked above") {
            let raw = c.as_str().unwrap_or("");
            if raw.parse::<Uuid>().is_err() {
                return Err(format!("\"{raw}\" is not a channel"));
            }
        }
    }
    Ok(())
}

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let config = &ctx.node.config;
    let title = truncate(
        &ctx.resolver.render_str(super::field(config, "title")?, ctx.vars, false).await?,
        MAX_TITLE,
    );
    let body = truncate(
        &ctx.resolver
            .render_str(config.get("body").and_then(|v| v.as_str()).unwrap_or(""), ctx.vars, false)
            .await?,
        MAX_BODY,
    );
    let in_app = config.get("in_app").and_then(|v| v.as_bool()).unwrap_or(true);
    let ids: Vec<Uuid> = config
        .get("channels")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()?.parse().ok()).collect())
        .unwrap_or_default();

    let request = json!({
        "title": title,
        "body": body,
        "in_app": in_app,
        "channels": ids.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
    });

    if let crate::automator::RunMode::Test { send_notifications, .. } = ctx.mode {
        if !send_notifications {
            return Ok(Outcome::simulated(request, "test run: nothing was sent"));
        }
    }

    if in_app {
        otw_store::reminders::add_notification(&ctx.state.pool, &title, &body)
            .await
            .map_err(|e| format!("writing the in-app notification failed: {e}"))?;
    }
    let notif = otw_store::reminders::FiredNotification {
        name: title.clone(),
        details: body.clone(),
        url: String::new(),
    };
    let only = if ids.is_empty() { None } else { Some(ids.as_slice()) };
    crate::notif_send::dispatch(
        &ctx.state.pool,
        &ctx.state.cipher,
        &ctx.state.http,
        &notif,
        "automator",
        only,
    )
    .await;

    Ok(Outcome::new(
        request,
        json!({ "in_app": in_app, "channels": ids.len() }),
    ))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}
