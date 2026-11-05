//! Broker imports the user sent to the background.
//!
//! A pull can take minutes (a year of weekly slices, a Flex statement generated on
//! demand). The import modals ask for an estimate first and, past a few seconds, offer to
//! let the pull run here while the user does something else. The task keeps its answer
//! until the modal that asked for it takes it back, and a notification says when it is
//! ready, in the bell and on the channels granted to the module.
//!
//! One task per scope (`journal:<category>`, `taxcalc`, `portfolios:<portfolio>`): a
//! second pull for the same book while one is running would race it. Held in memory on
//! purpose. A restart loses a running pull, which the user asks for again; nothing is
//! half-written, since an import is one revertible batch.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use tokio::task::AbortHandle;
use uuid::Uuid;

use crate::AppState;

/// A finished task nobody came back for is dropped after a day.
const KEEP: time::Duration = time::Duration::hours(24);

#[derive(Clone, Serialize)]
pub struct TaskView {
    pub id: Uuid,
    pub scope: String,
    pub label: String,
    /// Where the result is read: the page whose modal takes it back.
    pub link: String,
    /// running | done | failed
    pub status: &'static str,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    pub estimate_secs: f64,
    pub summary: Option<String>,
    pub error: Option<String>,
    /// The answer the modal would have had, had it waited. Only sent when it is taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

struct Task {
    view: TaskView,
    result: Option<Value>,
    abort: Option<AbortHandle>,
}

fn tasks() -> &'static Mutex<HashMap<Uuid, Task>> {
    static T: OnceLock<Mutex<HashMap<Uuid, Task>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune(map: &mut HashMap<Uuid, Task>) {
    let cutoff = OffsetDateTime::now_utc() - KEEP;
    map.retain(|_, t| t.view.finished_at.is_none_or(|f| f > cutoff));
}

/// What the work hands back: the answer, and one line for the notification.
pub struct Done {
    pub result: Value,
    pub summary: String,
}

/// Run `work` in the background under `scope`. Refused while another task holds the scope.
#[allow(clippy::too_many_arguments)]
pub fn spawn<F>(
    state: &AppState,
    scope: String,
    label: String,
    link: String,
    module: &'static str,
    estimate_secs: f64,
    work: F,
) -> Result<TaskView>
where
    F: Future<Output = Result<Done>> + Send + 'static,
{
    let id = Uuid::new_v4();
    let view = TaskView {
        id,
        scope: scope.clone(),
        label: label.clone(),
        link: link.clone(),
        status: "running",
        started_at: OffsetDateTime::now_utc(),
        finished_at: None,
        estimate_secs,
        summary: None,
        error: None,
        result: None,
    };
    {
        let mut map = tasks().lock().map_err(|_| anyhow!("task registry poisoned"))?;
        prune(&mut map);
        if map.values().any(|t| t.view.scope == scope && t.view.status == "running") {
            return Err(anyhow!("a pull is already running in the background for this one"));
        }
        // A new pull replaces the answer the last one left waiting.
        map.retain(|_, t| t.view.scope != scope);
        map.insert(id, Task { view: view.clone(), result: None, abort: None });
    }

    let state = state.clone();
    let handle = tokio::spawn(async move {
        let outcome = work.await;
        let (title, details) = match &outcome {
            Ok(d) => (format!("{label}: ready"), d.summary.clone()),
            Err(e) => (format!("{label}: failed"), format!("{e:#}")),
        };
        if let Ok(mut map) = tasks().lock() {
            if let Some(t) = map.get_mut(&id) {
                t.view.finished_at = Some(OffsetDateTime::now_utc());
                t.abort = None;
                match outcome {
                    Ok(d) => {
                        t.view.status = "done";
                        t.view.summary = Some(d.summary);
                        t.result = Some(d.result);
                    }
                    Err(e) => {
                        t.view.status = "failed";
                        t.view.error = Some(format!("{e:#}"));
                    }
                }
            }
        }
        notify(&state, module, &title, &details, &link).await;
    });
    if let Ok(mut map) = tasks().lock() {
        if let Some(t) = map.get_mut(&id) {
            // Already finished if the work was instant: nothing left to abort.
            if t.view.status == "running" {
                t.abort = Some(handle.abort_handle());
            }
        }
    }
    Ok(view)
}

async fn notify(state: &AppState, module: &str, title: &str, details: &str, link: &str) {
    match otw_store::reminders::add_linked_notification(&state.pool, "task", title, details, link)
        .await
    {
        Ok(mut n) => {
            // The link is a path inside the app: it means nothing in an email or a chat.
            n.url.clear();
            crate::notif_send::dispatch(&state.pool, &state.cipher, &state.http, &n, module, None)
                .await;
        }
        Err(e) => tracing::warn!("task notification: {e:#}"),
    }
}

/// Every task, or the ones under a scope. Results stay out: they are taken one by one.
pub fn list(scope: Option<&str>) -> Vec<TaskView> {
    let Ok(mut map) = tasks().lock() else {
        return vec![];
    };
    prune(&mut map);
    let mut out: Vec<TaskView> = map
        .values()
        .filter(|t| scope.is_none_or(|s| t.view.scope == s))
        .map(|t| t.view.clone())
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.started_at));
    out
}

/// Hand a finished task's answer back, once, and forget the task.
pub fn take(id: Uuid) -> Result<TaskView> {
    let mut map = tasks().lock().map_err(|_| anyhow!("task registry poisoned"))?;
    match map.get(&id) {
        None => Err(anyhow!("this background pull is gone. Run it again")),
        Some(t) if t.view.status == "running" => Err(anyhow!("this pull is still running")),
        Some(_) => {
            let t = map.remove(&id).expect("checked above");
            Ok(TaskView { result: t.result, ..t.view })
        }
    }
}

/// Stop a running task, or dismiss a finished one.
pub fn cancel(id: Uuid) -> bool {
    let Ok(mut map) = tasks().lock() else {
        return false;
    };
    match map.remove(&id) {
        Some(t) => {
            if let Some(a) = t.abort {
                a.abort();
            }
            true
        }
        None => false,
    }
}

/// `?background=true` on an import route: run it here and answer with the task.
#[derive(Debug, Default, serde::Deserialize)]
pub struct Background {
    #[serde(default)]
    pub background: bool,
}
