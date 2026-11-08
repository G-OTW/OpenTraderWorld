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
//! Each endpoint redirects its payloads to a module via the [`TARGETS`] registry:
//! `remindme` (payload → in-app notification + enabled external channels), `automator`
//! (payload → a run of the workflow the endpoint is bound to) and `histdata` (bars or a tick
//! → the custom source the endpoint is bound to, stored and broadcast live). Adding a target
//! is one registry entry plus one `dispatch` arm, nothing else changes.

use std::time::Duration;

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::automator::{engine, Graph, RunMode};
use crate::{notif_send, security, AppState};

/// Inbound body cap. Webhook payloads are small messages; anything bigger is abuse.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Failed-token throttle, **per source**: after this many unknown tokens from one address
/// inside the window, that address is rejected before the lookup.
///
/// Per source is the whole point. A global counter here would be an own goal: this route is
/// public and its callers are third-party services, so anyone could stop every legitimate
/// delivery by spraying garbage tokens at it for a minute at a time — the attacker's cost
/// becomes the owner's outage. Keyed by address, a spray only silences the sprayer.
const AUTH_FAIL_LIMIT: u32 = 20;
const AUTH_FAIL_WINDOW: Duration = Duration::from_secs(60);

/// A redirect target: the module a webhook's payloads are handed to.
pub struct Target {
    pub id: &'static str,
    pub label: &'static str,
    /// The target needs an object picked at creation, stored in the endpoint's `config`
    /// (`automator` → `workflow_id`). The management API validates it, the form asks for it.
    pub needs: Option<&'static str>,
}

/// Registry of modules a webhook can redirect to. To add one: add its entry here and a
/// matching arm in [`dispatch`]; the management API and the frontend picker follow.
pub const TARGETS: &[Target] = &[
    Target {
        id: "remindme",
        label: "RemindMe — in-app notification, pushed to enabled channels",
        needs: None,
    },
    Target {
        id: "automator",
        label: "Automator — run a workflow, payload readable as {{input.…}}",
        needs: Some("workflow_id"),
    },
    Target {
        id: "histdata",
        label: "Historical data: push bars or ticks into a custom source",
        needs: Some("connector_id"),
    },
];

/// The workflow an `automator` endpoint is bound to, from its stored config.
pub fn config_workflow_id(config: &Value) -> Option<uuid::Uuid> {
    config.get("workflow_id")?.as_str()?.parse().ok()
}

/// The custom source a `histdata` endpoint is bound to, from its stored config.
pub fn config_connector_id(config: &Value) -> Option<uuid::Uuid> {
    config.get("connector_id")?.as_str()?.parse().ok()
}

/// Whether a connector row can receive pushed bars: a custom push source, or a custom HTTP
/// source (a script can fill what its endpoint cannot page back to).
pub fn pushable(provider: &str) -> bool {
    use crate::histdata::kinds;
    kinds::is_instance(provider) && matches!(kinds::kind_of(provider), Some(kinds::push::KIND) | Some(kinds::http::KIND))
}

// ── Failed-token throttle ────────────────────────────────────────────────────

static AUTH_FAILS: security::Attempts = security::Attempts::new(AUTH_FAIL_WINDOW);

fn auth_throttled(ip: &str) -> bool {
    AUTH_FAILS.count(ip) >= AUTH_FAIL_LIMIT
}

fn record_auth_failure(ip: &str) {
    AUTH_FAILS.record(ip);
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
    headers: HeaderMap,
    body: String,
) -> Response {
    let ip = security::client_ip(&headers);
    if auth_throttled(&ip) {
        return http_json(
            StatusCode::TOO_MANY_REQUESTS,
            json!({ "error": "too many failed attempts" }),
        );
    }
    let endpoint = match otw_store::webhooks::find_by_token(&state.pool, token.trim()).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            record_auth_failure(&ip);
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

    let (status, detail, extra) = match dispatch(&state, &endpoint, &body).await {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!("webhook '{}' dispatch failed: {e:#}", endpoint.name);
            ("error", format!("{e:#}"), Value::Null)
        }
    };
    if let Err(e) = otw_store::webhooks::record_delivery(
        &state.pool, endpoint.id, status, &detail, &body,
    )
    .await
    {
        tracing::error!("recording webhook delivery: {e:#}");
    }

    // `ignored` is a deliberate no-op (disabled or already-running workflow), not a
    // failure: the sender gets its 2xx and does not retry. Only `error` is a 422.
    if status != "error" {
        tracing::info!("webhook '{}' → {}: {detail}", endpoint.name, endpoint.target);
        let mut out = json!({ "ok": true, "detail": detail });
        if let (Some(o), Value::Object(more)) = (out.as_object_mut(), extra) {
            o.extend(more);
        }
        http_json(StatusCode::OK, out)
    } else if endpoint.target == "histdata" {
        // A script sending bars needs the reason to fix its payload; the sender already holds
        // the token, so the detail tells it nothing it could not learn otherwise.
        http_json(StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": "dispatch failed", "detail": detail }))
    } else {
        http_json(StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": "dispatch failed" }))
    }
}

/// Redirect one payload to the endpoint's target module. Returns the delivery-log status
/// (`ok`, or `ignored` when the target deliberately did nothing), a short human detail, and
/// fields the answer carries besides (a JSON object, or null).
/// New targets: add a [`TARGETS`] entry and an arm here.
async fn dispatch(
    state: &AppState,
    endpoint: &otw_store::webhooks::WebhookEndpoint,
    body: &str,
) -> anyhow::Result<(&'static str, String, Value)> {
    if endpoint.target == "histdata" {
        return push_ingest(state, &endpoint.config, body).await;
    }
    let (status, detail) = dispatch_plain(state, endpoint, body).await?;
    Ok((status, detail, Value::Null))
}

/// The demo's own refusal, word for word, for a write that reaches past the route gate.
const DEMO_REFUSAL: &str = "this action is disabled in the public demo";

/// A `histdata` delivery: closed bars are written to the dataset of their coordinates (created
/// on the first write, as an import does), the forming one is not, and the newest broadcast to the charts watching them;
/// a tick is broadcast only, folded by each chart's subscription and recorded by the ones
/// that record. Bulk writes, so the demo refuses it and no MCP tool reaches it.
pub(crate) async fn push_ingest(state: &AppState, config: &Value, body: &str) -> anyhow::Result<(&'static str, String, Value)> {
    use crate::histdata::kinds::{self, push};
    use crate::live::hub::Ingest;
    if crate::demo::enabled() {
        anyhow::bail!(DEMO_REFUSAL);
    }
    let id = config_connector_id(config).context("this webhook has no custom source configured")?;
    let row = otw_store::connectors::get(&state.pool, id)
        .await?
        .context("the custom source this webhook points at no longer exists")?;
    let spec = push::spec_for(&row.provider, &kinds::config_map(&row.config))
        .map_err(|e| anyhow::anyhow!("custom source \"{}\": {e:#}", row.name))?;
    let json: Value = serde_json::from_str(body.trim()).map_err(|e| anyhow::anyhow!("the body is not JSON: {e}"))?;
    match push::parse_payload(&spec, &json)? {
        // A value source: observations of one Fundamentals series, created on its first push,
        // upserted by period (latest vintage wins), and appended live on the charts drawing it.
        push::Payload::Observations { series, title, observations, missing } => {
            let meta = crate::fundamentals::custom::meta(&row.provider, &series, title.as_deref())?;
            let series_id = otw_store::fundamentals::ensure_series(&state.pool, &meta).await?;
            let changed = crate::fundamentals::custom::store_observations(&state.pool, &state.live, series_id, &observations).await?;
            let (first, last) = (observations[0].period_start.to_string(), observations[observations.len() - 1].period_start.to_string());
            Ok((
                "ok",
                format!(
                    "{} {series} observations stored ({changed} new or revised), {first} to {last}{}",
                    observations.len(),
                    crate::histdata::kinds::value::missing_note(missing)
                ),
                json!({ "written": observations.len(), "changed": changed, "missing": missing, "first": first, "last": last, "series_id": series_id }),
            ))
        }
        push::Payload::Bars { symbol, asset_type, timeframe, bars } => {
            // Only bars whose period is over are history. The forming one is broadcast to the
            // charts watching it and left out of the dataset: a later push corrects it.
            let (closed, forming_skipped) =
                crate::live::push::closed_bars(&timeframe, bars.clone(), time::OffsetDateTime::now_utc())?;
            let fmt = |t: time::OffsetDateTime| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
            let live = state.live.ingest(row.id, &symbol, &asset_type, Ingest::Bars { timeframe: timeframe.clone(), bars });
            if closed.is_empty() {
                return Ok((
                    "ok",
                    format!("{symbol} {timeframe}: {forming_skipped} forming bar(s) broadcast, none closed to write"),
                    json!({ "written": 0, "forming_skipped": forming_skipped, "live": live }),
                ));
            }
            let dataset_id =
                otw_store::histdata::upsert_dataset(&state.pool, &row.provider, &asset_type, &symbol, &timeframe).await?;
            let written = otw_store::histdata::write_bars(&state.pool, dataset_id, &closed).await?;
            let (first, last) = (fmt(closed[0].ts), fmt(closed[closed.len() - 1].ts));
            Ok((
                "ok",
                format!("{written} {symbol} {timeframe} bars written, {first} to {last}, {forming_skipped} forming skipped"),
                json!({
                    "written": written, "first": first, "last": last, "dataset_id": dataset_id,
                    "forming_skipped": forming_skipped, "live": live,
                }),
            ))
        }
        push::Payload::Tick { symbol, asset_type, price, size, ts } => {
            let live = state.live.ingest(row.id, &symbol, &asset_type, Ingest::Tick { price, size, ts });
            if live == 0 {
                // Not an error: the sender must not retry. A tick is only ever live.
                return Ok((
                    "ignored",
                    format!("no live chart is watching {symbol}: a tick is not stored, push bars to keep history"),
                    json!({ "live": 0 }),
                ));
            }
            Ok(("ok", format!("{symbol} tick at {price} reached {live} live subscription(s)"), json!({ "live": live })))
        }
    }
}

async fn dispatch_plain(
    state: &AppState,
    endpoint: &otw_store::webhooks::WebhookEndpoint,
    body: &str,
) -> anyhow::Result<(&'static str, String)> {
    match endpoint.target.as_str() {
        "remindme" => {
            let (title, details) = parse_payload(body, &endpoint.name);
            let notif =
                otw_store::reminders::add_notification(&state.pool, &title, &details).await?;
            // External pushes must not eat the sender's 2xx budget: detach them. The
            // producing module is `webhooks`, not the target the payload was redirected
            // to — the user grants channels where the endpoint is configured.
            let (pool, cipher, http) =
                (state.pool.clone(), state.cipher.clone(), state.http.clone());
            tokio::spawn(async move {
                notif_send::dispatch(&pool, &cipher, &http, &notif, "webhooks", None).await;
            });
            Ok(("ok", format!("notification \"{title}\"")))
        }
        // A webhook run is an automated trigger like a schedule, so it plays by the
        // scheduler's rules: a disabled workflow stays put, an already-running one drops
        // the occurrence, and the run is spawned detached so the sender keeps its 2xx.
        "automator" => {
            let id = config_workflow_id(&endpoint.config)
                .context("this webhook has no workflow configured")?;
            let workflow = otw_store::automator::get_workflow(&state.pool, id)
                .await?
                .context("the workflow this webhook points at no longer exists")?;
            if !workflow.enabled {
                return Ok((
                    "ignored",
                    format!("workflow \"{}\" is disabled", workflow.name),
                ));
            }
            let graph = Graph::parse(&workflow.graph)
                .and_then(|g| {
                    crate::automator::validate(&g)?;
                    Ok(g)
                })
                .map_err(|e| anyhow::anyhow!("workflow \"{}\" is not runnable: {e}", workflow.name))?;
            if graph.nodes.is_empty() {
                anyhow::bail!("workflow \"{}\" has no blocks yet", workflow.name);
            }
            let input = run_input(body);
            let run = otw_store::automator::start_run(
                &state.pool,
                workflow.id,
                workflow.version_id,
                None,
                "webhook",
                "live",
                &input,
            )
            .await?;
            match run {
                None => Ok((
                    "ignored",
                    format!("\"{}\" is still running, payload dropped", workflow.name),
                )),
                Some(run) => {
                    let detail = format!("run {} of \"{}\"", run.id, workflow.name);
                    engine::spawn(state.clone(), workflow, graph, run, RunMode::Live);
                    Ok(("ok", detail))
                }
            }
        }
        other => anyhow::bail!("unknown target '{other}'"),
    }
}

/// The payload as a workflow reads it in `{{input.…}}`. A JSON object is handed over
/// as-is, so `{{input.ticker}}` is the sender's own field; anything else (array, scalar,
/// plain text) arrives whole as `{{input.text}}`.
pub fn run_input(body: &str) -> Value {
    let trimmed = body.trim();
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => json!({ "text": trimmed }),
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
    fn a_json_object_body_is_the_workflow_input_as_is() {
        let v = run_input(r#"{"ticker":"AAPL","close":213.5}"#);
        assert_eq!(v["ticker"], "AAPL");
        assert_eq!(v["close"], 213.5);
    }

    #[test]
    fn a_non_object_body_arrives_whole_as_input_text() {
        assert_eq!(run_input("  BTC crossed 100000  ")["text"], "BTC crossed 100000");
        assert_eq!(run_input(r#"["a","b"]"#)["text"], r#"["a","b"]"#);
    }

    #[test]
    fn a_workflow_id_is_read_from_the_endpoint_config() {
        let ok = json!({ "workflow_id": "4e4b9d06-1f2a-4a62-9a4e-0d7b6d3f8a11" });
        assert!(config_workflow_id(&ok).is_some());
        assert!(config_workflow_id(&json!({})).is_none());
        assert!(config_workflow_id(&json!({ "workflow_id": "nope" })).is_none());
    }

    #[test]
    fn only_a_custom_source_takes_pushed_bars() {
        let t = TARGETS.iter().find(|t| t.id == "histdata").expect("histdata target");
        assert_eq!(t.needs, Some("connector_id"));
        assert!(pushable("push-1a2b3c4d"));
        assert!(pushable("http-1a2b3c4d"));
        // A kind's template is not a connector, and a provider's own market is not ours to write.
        assert!(!pushable("push"));
        assert!(!pushable("binance"));
        let ok = json!({ "connector_id": "4e4b9d06-1f2a-4a62-9a4e-0d7b6d3f8a11" });
        assert!(config_connector_id(&ok).is_some());
        assert!(config_connector_id(&json!({ "connector_id": 3 })).is_none());
    }

    /// Over a scratch database (`OTW_TEST_DATABASE_URL`, skipped without it): the first bars
    /// pushed for an instrument create its dataset, a second push upserts into the same one,
    /// and a tick nobody watches is answered `ignored`, never stored.
    #[tokio::test]
    async fn pushed_bars_create_their_dataset_on_the_first_write() {
        let Some(state) = crate::brokers::mock_tests::scratch_state().await else { return };
        let provider = crate::histdata::kinds::mint_provider("push");
        let config = json!({ "timeframes": "1m", "columns": "{\"ts\": \"t\", \"close\": \"c\"}", "ts_unit": "ms" });
        let row = otw_store::connectors::create(&state.pool, &provider, &format!("Script {provider}"), &["*".to_string()], &config)
            .await
            .unwrap();
        crate::histdata::kinds::register(&row.provider, &row.name, &row.config);
        let bound = json!({ "connector_id": row.id.to_string() });
        assert!(otw_store::histdata::find_dataset(&state.pool, &provider, "other", "ACME", "1m").await.unwrap().is_none());

        let body = r#"{"symbol": "ACME", "timeframe": "1m", "bars": [{"t": 1704067200000, "c": 1.5}, {"t": 1704067260000, "c": 1.6}]}"#;
        let (status, _, out) = push_ingest(&state, &bound, body).await.unwrap();
        assert_eq!(status, "ok");
        assert_eq!(out["written"], 2);
        assert_eq!(out["first"], "2024-01-01T00:00:00Z");
        assert_eq!(out["last"], "2024-01-01T00:01:00Z");
        let ds = otw_store::histdata::find_dataset(&state.pool, &provider, "other", "ACME", "1m").await.unwrap().expect("created");
        // The same bar again lands on the same row.
        let again = r#"{"symbol": "ACME", "timeframe": "1m", "bars": [{"t": 1704067260000, "c": 1.7}]}"#;
        push_ingest(&state, &bound, again).await.unwrap();
        let (n, _) = otw_store::histdata::count_bars(&state.pool, ds.id, None, None).await.unwrap();
        assert_eq!(n, 2);

        // A bar whose minute is not over yet is broadcast, never written.
        let now_ms = (time::OffsetDateTime::now_utc().unix_timestamp() / 60) * 60_000;
        let forming = format!(r#"{{"symbol": "ACME", "timeframe": "1m", "bars": [{{"t": 1704067320000, "c": 1.8}}, {{"t": {now_ms}, "c": 1.9}}]}}"#);
        let (_, _, out) = push_ingest(&state, &bound, &forming).await.unwrap();
        assert_eq!((out["written"].as_u64(), out["forming_skipped"].as_u64()), (Some(1), Some(1)));
        let (n, _) = otw_store::histdata::count_bars(&state.pool, ds.id, None, None).await.unwrap();
        assert_eq!(n, 3);

        let tick = r#"{"symbol": "ACME", "tick": {"ts": 1704067270000, "price": 1.8}}"#;
        let (status, _, out) = push_ingest(&state, &bound, tick).await.unwrap();
        assert_eq!((status, out["live"].as_u64()), ("ignored", Some(0)));
        // A payload that does not read is an error naming the field.
        let err = push_ingest(&state, &bound, r#"{"symbol": "ACME", "timeframe": "1h", "bars": []}"#).await.unwrap_err();
        assert!(format!("{err:#}").contains("1h"), "{err:#}");
        crate::histdata::kinds::forget(&provider);
        otw_store::connectors::delete(&state.pool, row.id).await.unwrap();
    }

    /// Over a scratch database (`OTW_TEST_DATABASE_URL`, skipped without it): a value-shaped
    /// push source creates its Fundamentals series on the first push and upserts by period.
    #[tokio::test]
    async fn pushed_observations_create_their_series_and_keep_the_latest_vintage() {
        let Some(state) = crate::brokers::mock_tests::scratch_state().await else { return };
        let provider = crate::histdata::kinds::mint_provider("push");
        let config = json!({ "shape": "value", "columns": "{\"period_start\": \"p\", \"value\": \"v\", \"released_at\": \"r\"}", "frequency": "1M", "unit": "%" });
        let row = otw_store::connectors::create(&state.pool, &provider, &format!("Values {provider}"), &["*".to_string()], &config)
            .await
            .unwrap();
        crate::histdata::kinds::register(&row.provider, &row.name, &row.config);
        let bound = json!({ "connector_id": row.id.to_string() });
        let body = r#"{"series": "CPI", "observations": [{"p": "2024-01-01", "v": 3.1, "r": "2024-02-13T13:30:00Z"}, {"p": "2024-02-01", "v": 3.2}]}"#;
        let (status, _, out) = push_ingest(&state, &bound, body).await.unwrap();
        assert_eq!((status, out["written"].as_u64(), out["changed"].as_u64()), ("ok", Some(2), Some(2)));
        let id: uuid::Uuid = out["series_id"].as_str().unwrap().parse().unwrap();
        // A revision of February replaces it; January unchanged is not news.
        let again = r#"{"series": "CPI", "observations": [{"p": "2024-01-01", "v": 3.1}, {"p": "2024-02-01", "v": 3.3}]}"#;
        let (_, _, out) = push_ingest(&state, &bound, again).await.unwrap();
        assert_eq!(out["changed"].as_u64(), Some(1));
        let obs = otw_store::fundamentals::observations_released(&state.pool, id, None, None).await.unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[1].value, 3.3);
        // A missing release time never erases the stored one.
        assert!(obs[0].released_at.is_some());
        otw_store::fundamentals::delete_series(&state.pool, id).await.unwrap();
        crate::histdata::kinds::forget(&provider);
        otw_store::connectors::delete(&state.pool, row.id).await.unwrap();
    }

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
