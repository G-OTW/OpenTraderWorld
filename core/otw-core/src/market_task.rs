//! Background tasks over a provider that lists contracts, shared by the quant derivatives
//! tools and the Options module.
//!
//! A chain or a curve is dozens of provider requests, paced by the provider, so a request
//! starts a task and the page polls `GET …/tasks/{id}`. The connector behind one is checked
//! here once: granted to the module asking, and one of the providers that list contracts.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::{Path, Query};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::histdata::chains;
use crate::{ApiError, AppState};
use otw_store::connectors as conn_store;

/// How long a finished task is kept.
const KEEP: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone)]
struct Task {
    status: &'static str,
    done: usize,
    total: usize,
    step: String,
    result: Option<Value>,
    error: Option<String>,
    at: Instant,
}

fn tasks() -> &'static Mutex<HashMap<Uuid, Task>> {
    static T: OnceLock<Mutex<HashMap<Uuid, Task>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_task(id: Uuid, f: impl FnOnce(&mut Task)) {
    if let Ok(mut m) = tasks().lock() {
        if let Some(t) = m.get_mut(&id) {
            f(t);
            t.at = Instant::now();
        }
    }
}

/// Start `work` in the background and hand back the id the page polls.
pub(crate) fn spawn<F>(work: F) -> Json<Value>
where
    F: FnOnce(Uuid) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<Value>> + Send>>,
{
    let id = Uuid::new_v4();
    if let Ok(mut m) = tasks().lock() {
        m.retain(|_, t| t.at.elapsed() < KEEP);
        m.insert(
            id,
            Task { status: "running", done: 0, total: 0, step: String::new(), result: None, error: None, at: Instant::now() },
        );
    }
    let fut = work(id);
    tokio::spawn(async move {
        let out = fut.await;
        set_task(id, |t| match out {
            Ok(v) => {
                t.status = "done";
                t.result = Some(v);
            }
            Err(e) => {
                t.status = "error";
                t.error = Some(format!("{e:#}"));
            }
        });
    });
    Json(json!({ "task": id }))
}

pub(crate) fn progress(id: Uuid) -> impl Fn(usize, usize, &str) + Send + Sync {
    move |done, total, step| {
        let step = step.to_string();
        set_task(id, |t| {
            t.done = done;
            t.total = total;
            t.step = step;
        })
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TaskQuery {
    /// `summary` drops the per-day rows (futures curve, IV history) and keeps everything else.
    #[serde(default)]
    view: Option<String>,
}

pub(crate) async fn task(Path(id): Path<Uuid>, Query(q): Query<TaskQuery>) -> Result<Json<Value>, ApiError> {
    let mut t = tasks()
        .lock()
        .ok()
        .and_then(|m| m.get(&id).cloned())
        .ok_or_else(|| ApiError::not_found("task not found or expired"))?;
    if q.view.as_deref() == Some("summary") {
        if let Some(r) = t.result.as_mut().and_then(|v| v.get_mut("result")).and_then(Value::as_object_mut) {
            if let Some(rows) = r.remove("rows") {
                r.insert("row_count".into(), json!(rows.as_array().map_or(0, Vec::len)));
                if let Some(last) = rows.as_array().and_then(|a| a.last()) {
                    r.insert("last_row".into(), last.clone());
                }
            }
        }
    }
    Ok(Json(json!({
        "status": t.status,
        "done": t.done,
        "total": t.total,
        "step": t.step,
        "result": t.result,
        "error": t.error,
    })))
}

// ── Connector ────────────────────────────────────────────────────────────────────────────

pub(crate) struct Source {
    pub id: Uuid,
    pub provider: String,
    pub secrets: HashMap<String, String>,
}

/// The connector a task reads, granted to `module` (`label` is how the refusal names it).
pub(crate) async fn source(state: &AppState, id: Uuid, module: &str, label: &str) -> Result<Source, ApiError> {
    read_source(state, id, module, label, chains::PROVIDERS, "an Interactive Brokers or a Massive").await
}

/// The connector an Options chain reads, which Alpaca can also serve.
pub(crate) async fn chain_source(state: &AppState, id: Uuid, module: &str, label: &str) -> Result<Source, ApiError> {
    read_source(state, id, module, label, chains::CHAIN_PROVIDERS, "an Interactive Brokers, a Massive or an Alpaca").await
}

async fn read_source(
    state: &AppState,
    id: Uuid,
    module: &str,
    label: &str,
    providers: &[&str],
    named: &str,
) -> Result<Source, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden("the demo does not reach market-data providers"));
    }
    let c = conn_store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("connector not found"))?;
    if !c.allows(module) {
        return Err(ApiError::bad_request(&format!(
            "connector '{}' is not granted to {label}. Grant it from the connector settings",
            c.name
        )));
    }
    if !providers.contains(&c.provider.as_str()) {
        return Err(ApiError::bad_request(&format!(
            "connector '{}' ({}) does not list futures contracts or option chains. Use {named} connector",
            c.name, c.provider
        )));
    }
    let secrets = conn_store::load_creds(&state.pool, &state.cipher, id).await?;
    Ok(Source { id, provider: c.provider, secrets })
}
