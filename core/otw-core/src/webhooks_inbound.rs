//! Inbound webhook receiver — `POST /api/hooks/{token}` (public route).
//!
//! Sender-agnostic: any service that can POST a small text/JSON body to a URL works
//! (alerting platforms, monitors, scripts…). The lowest common denominator of those
//! senders shapes the design:
//!   - many cannot send custom headers, so the credential is the 256-bit token in the
//!     URL path (SHA-256 at rest, like MCP tokens; failed lookups are throttled);
//!   - most expect a fast 2xx (a few seconds budget), so dispatch happens inline and
//!     stays cheap — a DB insert plus fire-and-forget channel pushes;
//!   - the body is whatever the sender emits — plain text or JSON, so [`parse_payload`]
//!     accepts both and is liberal about field names.
//!
//! Each endpoint redirects its payloads to a module via the [`TARGETS`] registry. v1
//! ships `remindme` (payload → in-app notification + enabled external channels); adding
//! a target is one registry entry plus one `dispatch` arm — nothing else changes.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{notif_send, AppState};

/// Inbound body cap. Webhook payloads are small messages; anything bigger is abuse.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Failed-token throttle: after this many failures per window, reject early.
const AUTH_FAIL_LIMIT: u32 = 20;
const AUTH_FAIL_WINDOW: Duration = Duration::from_secs(60);

/// A redirect target: the module a webhook's payloads are handed to.
pub struct Target {
    pub id: &'static str,
    pub label: &'static str,
}

/// Registry of modules a webhook can redirect to. To add one: add its entry here and a
/// matching arm in [`dispatch`]; the management API and the frontend picker follow.
pub const TARGETS: &[Target] = &[Target {
    id: "remindme",
    label: "RemindMe — in-app notification, pushed to enabled channels",
}];

// ── Failed-token throttle (same pattern as the MCP endpoint) ─────────────────

fn auth_fails() -> &'static Mutex<Option<(Instant, u32)>> {
    static FAILS: OnceLock<Mutex<Option<(Instant, u32)>>> = OnceLock::new();
    FAILS.get_or_init(|| Mutex::new(None))
}

fn auth_throttled() -> bool {
    let guard = auth_fails().lock().unwrap();
    matches!(*guard, Some((start, n)) if n >= AUTH_FAIL_LIMIT && start.elapsed() < AUTH_FAIL_WINDOW)
}

fn record_auth_failure() {
    let mut guard = auth_fails().lock().unwrap();
    *guard = match *guard {
        Some((start, n)) if start.elapsed() < AUTH_FAIL_WINDOW => Some((start, n + 1)),
        _ => Some((Instant::now(), 1)),
    };
}

// ── Receiver ─────────────────────────────────────────────────────────────────

fn http_json(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// `POST /api/hooks/{token}` — authenticate by token, log the delivery, redirect the
/// payload to the endpoint's target. Always answers quickly; the sender only needs 2xx.
pub async fn handle(
    State(state): State<AppState>,
    Path(token): Path<String>,
    body: String,
) -> Response {
    if auth_throttled() {
        return http_json(
            StatusCode::TOO_MANY_REQUESTS,
            json!({ "error": "too many failed attempts" }),
        );
    }
    let endpoint = match otw_store::webhooks::find_by_token(&state.pool, token.trim()).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            record_auth_failure();
            // Uniform 404 — no oracle on whether a token exists.
            return http_json(StatusCode::NOT_FOUND, json!({ "error": "unknown webhook" }));
        }
        Err(e) => {
            tracing::error!("webhook token lookup failed: {e:#}");
            return http_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "internal error" }),
            );
        }
    };

    if !endpoint.enabled {
        let _ = otw_store::webhooks::record_delivery(
            &state.pool, endpoint.id, "ignored", "endpoint disabled", &body,
        )
        .await;
        return http_json(StatusCode::NOT_FOUND, json!({ "error": "unknown webhook" }));
    }

    let (status, detail) = match dispatch(&state, &endpoint, &body).await {
        Ok(detail) => ("ok", detail),
        Err(e) => {
            tracing::warn!("webhook '{}' dispatch failed: {e:#}", endpoint.name);
            ("error", format!("{e:#}"))
        }
    };
    if let Err(e) = otw_store::webhooks::record_delivery(
        &state.pool, endpoint.id, status, &detail, &body,
    )
    .await
    {
        tracing::error!("recording webhook delivery: {e:#}");
    }

    if status == "ok" {
        tracing::info!("webhook '{}' → {}: {detail}", endpoint.name, endpoint.target);
        http_json(StatusCode::OK, json!({ "ok": true }))
    } else {
        http_json(StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": "dispatch failed" }))
    }
}

/// Redirect one payload to the endpoint's target module. Returns a short human detail
/// for the delivery log. New targets: add a [`TARGETS`] entry and an arm here.
async fn dispatch(
    state: &AppState,
    endpoint: &otw_store::webhooks::WebhookEndpoint,
    body: &str,
) -> anyhow::Result<String> {
    match endpoint.target.as_str() {
        "remindme" => {
            let (title, details) = parse_payload(body, &endpoint.name);
            let notif =
                otw_store::reminders::add_notification(&state.pool, &title, &details).await?;
            // External pushes must not eat the sender's 2xx budget: detach them.
            let (pool, cipher, http) =
                (state.pool.clone(), state.cipher.clone(), state.http.clone());
            tokio::spawn(async move {
                notif_send::dispatch(&pool, &cipher, &http, &notif).await;
            });
            Ok(format!("notification \"{title}\""))
        }
        other => anyhow::bail!("unknown target '{other}'"),
    }
}

// ── Payload parsing ──────────────────────────────────────────────────────────

/// JSON keys accepted as the notification title / body. Senders name these fields
/// however they like, so be liberal.
const TITLE_KEYS: &[&str] = &["title", "name", "subject"];
const BODY_KEYS: &[&str] = &["message", "text", "body", "details", "content", "comment"];
/// A plain-text first line longer than this is a message, not a title.
const TITLE_MAX_CHARS: usize = 120;

/// Split an inbound body into (title, details) for a notification.
///
/// JSON object → title from the first `TITLE_KEYS` hit, details from the first
/// `BODY_KEYS` hit plus any remaining scalar fields as `key: value` lines (so
/// `{{ticker}}`/`{{close}}`-style fields the user added are not lost). Plain text →
/// first line as title when it reads like one, rest as details. Empty or unusable
/// bodies fall back to the endpoint name as title.
pub fn parse_payload(body: &str, fallback_title: &str) -> (String, String) {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return (fallback_title.to_string(), String::new());
    }

    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
        let pick = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| map.get(*k).and_then(|v| v.as_str()))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        let title = pick(TITLE_KEYS).unwrap_or_else(|| fallback_title.to_string());
        let mut details = pick(BODY_KEYS).unwrap_or_default();
        // Extra scalar fields (ticker, price, …) go below the message, one per line.
        let extras: Vec<String> = map
            .iter()
            .filter(|(k, _)| {
                !TITLE_KEYS.contains(&k.as_str()) && !BODY_KEYS.contains(&k.as_str())
            })
            .filter_map(|(k, v)| match v {
                Value::String(s) => Some(format!("{k}: {s}")),
                Value::Number(n) => Some(format!("{k}: {n}")),
                Value::Bool(b) => Some(format!("{k}: {b}")),
                _ => None,
            })
            .collect();
        if !extras.is_empty() {
            if !details.is_empty() {
                details.push('\n');
            }
            details.push_str(&extras.join("\n"));
        }
        return (title, details);
    }

    // Plain text: a short first line with more below is a title; otherwise the whole
    // body is the message under the endpoint's name.
    match trimmed.split_once('\n') {
        Some((first, rest)) if first.trim().chars().count() <= TITLE_MAX_CHARS => {
            (first.trim().to_string(), rest.trim().to_string())
        }
        None if trimmed.chars().count() <= TITLE_MAX_CHARS => {
            (trimmed.to_string(), String::new())
        }
        _ => (fallback_title.to_string(), trimmed.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_payload_maps_title_and_message() {
        let (t, d) = parse_payload(r#"{"title":"BTC alert","message":"crossed 100k"}"#, "wh");
        assert_eq!(t, "BTC alert");
        assert_eq!(d, "crossed 100k");
    }

    #[test]
    fn json_extras_are_appended_as_lines() {
        let (t, d) = parse_payload(
            r#"{"message":"breakout","ticker":"AAPL","close":213.5}"#,
            "TV hook",
        );
        assert_eq!(t, "TV hook"); // no title key → endpoint name
        assert!(d.starts_with("breakout"));
        assert!(d.contains("ticker: AAPL"));
        assert!(d.contains("close: 213.5"));
    }

    #[test]
    fn plain_text_single_short_line_is_the_title() {
        let (t, d) = parse_payload("BTCUSD crossed 100000", "wh");
        assert_eq!(t, "BTCUSD crossed 100000");
        assert_eq!(d, "");
    }

    #[test]
    fn plain_text_multiline_splits_title_and_body() {
        let (t, d) = parse_payload("Alert: EURUSD\nPrice hit 1.0950\ncheck the chart", "wh");
        assert_eq!(t, "Alert: EURUSD");
        assert_eq!(d, "Price hit 1.0950\ncheck the chart");
    }

    #[test]
    fn long_or_empty_bodies_fall_back_to_endpoint_name() {
        let long = "x".repeat(500);
        let (t, d) = parse_payload(&long, "My webhook");
        assert_eq!(t, "My webhook");
        assert_eq!(d, long);

        let (t, d) = parse_payload("   ", "My webhook");
        assert_eq!(t, "My webhook");
        assert_eq!(d, "");
    }

    #[test]
    fn non_object_json_is_treated_as_text() {
        let (t, d) = parse_payload(r#"["a","b"]"#, "wh");
        assert_eq!(t, r#"["a","b"]"#);
        assert_eq!(d, "");
    }
}
