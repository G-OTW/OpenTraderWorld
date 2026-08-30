//! The run loop: walk the graph once, record every step.
//!
//! Sequential by design. Parallel branches would halve the wall clock of a rare workflow
//! and cost every user a run trace they cannot read, so nodes execute in topological order
//! with the authoring order as the tie-break: two independent branches always run in the
//! order they were drawn.
//!
//! A node runs when at least one link into it was **taken**. A node whose links were all
//! left untaken is recorded as `skipped`, which is how the far side of a condition shows up
//! in the trace instead of silently missing. Roots (no incoming link) always run, so a
//! workflow that is just "send this notification" is the same shape as one that fans out.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use uuid::Uuid;

use otw_store::automator::{self as store, Run, StepRecord, Workflow};

use super::nodes::{self, NodeCtx};
use super::{expr, Graph, RunMode};
use crate::AppState;

/// Output larger than this is parked in a blob and replaced by a handle, so a downloaded
/// CSV never rides along inside every later node's context.
const INLINE_LIMIT: usize = 256 * 1024;

/// What a finished run looks like to its caller.
pub struct RunResult {
    pub status: String,
    pub error: String,
    pub steps: Vec<StepRecord>,
}

/// Run a workflow to completion, writing the run's steps as it goes and closing the run row.
pub async fn execute(
    state: &AppState,
    workflow: &Workflow,
    graph: &Graph,
    run: &Run,
    mode: RunMode,
) -> RunResult {
    let started = Instant::now();
    let result = drive(state, workflow, graph, run, mode, started).await;
    let duration = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
    if let Err(e) =
        store::finish_run(&state.pool, run.id, &result.status, &result.error, duration).await
    {
        tracing::error!("closing automator run {}: {e:#}", run.id);
    }
    result
}

async fn drive(
    state: &AppState,
    workflow: &Workflow,
    graph: &Graph,
    run: &Run,
    mode: RunMode,
    started: Instant,
) -> RunResult {
    let order = match graph.order() {
        Ok(o) => o,
        Err(e) => return RunResult { status: "failed".into(), error: e, steps: Vec::new() },
    };
    let deadline = started + Duration::from_secs(workflow.max_runtime_secs.max(1) as u64);

    // The workflow's permission envelope. Absent or expired means internal calls fail with
    // a message that says so; the rest of the workflow still runs.
    let (perms, token_name) = load_perms(state, workflow).await;

    let secrets = expr::Secrets::default();
    let resolver = expr::Resolver::new(state, secrets.clone());
    let mut vars = expr::Ctx {
        steps: serde_json::Map::new(),
        run: json!({
            "id": run.id.to_string(),
            "trigger": run.trigger,
            "mode": run.mode,
            "started_at": rfc3339(run.started_at),
        }),
        input: run.input.clone(),
        workflow: json!({ "id": workflow.id.to_string(), "name": workflow.name }),
    };

    let mut steps: Vec<StepRecord> = Vec::new();
    // Links whose source produced the port they carry: the only thing that makes a node run.
    let mut taken: HashSet<usize> = HashSet::new();
    let incoming: HashMap<&str, Vec<usize>> = graph.edges.iter().enumerate().fold(
        HashMap::new(),
        |mut acc, (i, e)| {
            acc.entry(e.to.as_str()).or_default().push(i);
            acc
        },
    );

    let mut seq = 0i32;
    for index in order {
        let node = &graph.nodes[index];
        let links = incoming.get(node.id.as_str());
        let active = match links {
            None => true,
            Some(list) if list.is_empty() => true,
            Some(list) => list.iter().any(|i| taken.contains(i)),
        };
        if !active {
            seq += 1;
            let step = skipped_step(node, seq);
            record(state, run.id, &mut steps, &mut vars, &secrets, step, Value::Null).await;
            continue;
        }

        if Instant::now() >= deadline {
            return finish(
                steps,
                "timeout",
                &format!("the run hit its {} s limit", workflow.max_runtime_secs),
            );
        }
        if store::cancel_requested(&state.pool, run.id).await.unwrap_or(false) {
            return finish(steps, "cancelled", "stopped by the user");
        }

        // A test run does not execute writes, so a block that reads what a write *would*
        // have returned has nothing to read. Reporting it as simulated (and letting that
        // travel to the blocks after it) is the honest answer; failing the whole test on a
        // reference a live run resolves fine is not, and that is the shape of every
        // create-then-read-it-back workflow.
        if mode.is_test() {
            if let Some(source) = simulated_source(node, &vars) {
                seq += 1;
                let output = json!({
                    "simulated": true,
                    "reason": format!("test run: it reads \"{source}\", which was only simulated"),
                });
                let step = StepRecord {
                    node_id: node.id.clone(),
                    node_name: super::label(node).to_string(),
                    kind: node.kind.clone(),
                    seq,
                    status: "simulated".into(),
                    request: json!({}),
                    output: output.clone(),
                    error: String::new(),
                    bytes: 0,
                    duration_ms: 0,
                };
                record(state, run.id, &mut steps, &mut vars, &secrets, step, output).await;
                mark(&mut taken, graph, &node.id, "out");
                continue;
            }
        }

        let ctx = NodeCtx {
            state,
            run_id: run.id,
            mode,
            node,
            vars: &vars,
            resolver: &resolver,
            perms: perms.clone(),
            token_name: token_name.clone(),
            deadline,
        };
        let node_started = Instant::now();
        let outcome = attempt(&ctx, deadline).await;
        let duration = node_started.elapsed().as_millis().min(i32::MAX as u128) as i32;

        match outcome {
            Ok(out) => {
                let (output, bytes) = park_oversized(state, run.id, &node.id, out.output).await;
                let step = StepRecord {
                    node_id: node.id.clone(),
                    node_name: super::label(node).to_string(),
                    kind: node.kind.clone(),
                    seq: {
                        seq += 1;
                        seq
                    },
                    status: if out.simulated { "simulated".into() } else { "ok".into() },
                    request: out.request,
                    output: output.clone(),
                    error: String::new(),
                    bytes: bytes as i32,
                    duration_ms: duration,
                };
                let port = success_port(node, &output);
                record(state, run.id, &mut steps, &mut vars, &secrets, step, output).await;
                mark(&mut taken, graph, &node.id, port);
            }
            Err(message) => {
                seq += 1;
                let step = StepRecord {
                    node_id: node.id.clone(),
                    node_name: super::label(node).to_string(),
                    kind: node.kind.clone(),
                    seq,
                    status: "failed".into(),
                    request: json!({}),
                    output: Value::Null,
                    error: message.clone(),
                    bytes: 0,
                    duration_ms: duration,
                };
                record(state, run.id, &mut steps, &mut vars, &secrets, step, Value::Null).await;
                if node.on_error != "continue" {
                    return finish(
                        steps,
                        "failed",
                        &format!("{}: {message}", super::label(node)),
                    );
                }
                // Continue policy: follow the error port when one is drawn, otherwise carry
                // on down the normal one so a "best effort" block does not end the run.
                let has_error_edge =
                    graph.edges.iter().any(|e| e.from == node.id && e.port == "error");
                mark(&mut taken, graph, &node.id, if has_error_edge { "error" } else { "out" });
            }
        }
    }
    finish(steps, "ok", "")
}

/// Run one node, honouring its retry policy. Only the node's own failure is retried; a
/// cancelled or timed-out run stops immediately.
async fn attempt(ctx: &NodeCtx<'_>, deadline: Instant) -> Result<nodes::Outcome, String> {
    let tries = ctx.node.retry.count.min(5);
    let mut last = String::new();
    for attempt in 0..=tries {
        match nodes::run(ctx).await {
            Ok(out) => return Ok(out),
            Err(e) => {
                last = e;
                if attempt == tries {
                    break;
                }
                let backoff = Duration::from_secs(ctx.node.retry.backoff_secs.min(300) as u64);
                let left = deadline.saturating_duration_since(Instant::now());
                if backoff >= left {
                    break;
                }
                tracing::debug!(
                    "automator block {} failed ({last}), retry {} of {tries}",
                    ctx.node.id,
                    attempt + 1
                );
                tokio::time::sleep(backoff).await;
            }
        }
    }
    Err(last)
}

/// The first block this one reads whose own step was only simulated, if any. Reading just
/// the `status` of such a block is fine: that is what a status is for.
fn simulated_source(node: &super::Node, vars: &expr::Ctx) -> Option<String> {
    let mut paths = Vec::new();
    expr::referenced_paths(&node.config, &mut paths);
    for path in paths {
        let Some(rest) = path.strip_prefix("steps.") else { continue };
        let mut segments = rest.split(['.', '[']);
        let id = segments.next().unwrap_or("");
        if matches!(segments.next(), Some("status") | Some("error") | None) {
            continue;
        }
        let status = vars.steps.get(id).and_then(|s| s.get("status")).and_then(|v| v.as_str());
        if status == Some("simulated") {
            return Some(id.to_string());
        }
    }
    None
}

/// The port a successful node lit up.
fn success_port(node: &super::Node, output: &Value) -> &'static str {
    if node.kind == "if" {
        if output.get("result").and_then(|v| v.as_bool()).unwrap_or(false) {
            "true"
        } else {
            "false"
        }
    } else {
        "out"
    }
}

fn mark(taken: &mut HashSet<usize>, graph: &Graph, from: &str, port: &str) {
    for (i, e) in graph.edges.iter().enumerate() {
        if e.from == from && e.port == port {
            taken.insert(i);
        }
    }
}

fn skipped_step(node: &super::Node, seq: i32) -> StepRecord {
    StepRecord {
        node_id: node.id.clone(),
        node_name: super::label(node).to_string(),
        kind: node.kind.clone(),
        seq,
        status: "skipped".into(),
        request: json!({}),
        output: Value::Null,
        error: String::new(),
        bytes: 0,
        duration_ms: 0,
    }
}

/// Store one step and publish its result to the expression context.
///
/// The stored copy is scrubbed of every secret resolved so far; the live context is not,
/// so a credential a block genuinely needs downstream still works. The trace is what
/// leaves the process, and the trace is what gets cleaned.
async fn record(
    state: &AppState,
    run_id: Uuid,
    steps: &mut Vec<StepRecord>,
    vars: &mut expr::Ctx,
    secrets: &expr::Secrets,
    step: StepRecord,
    live_output: Value,
) {
    let mut stored = step;
    secrets.scrub_value(&mut stored.request);
    secrets.scrub_value(&mut stored.output);
    stored.error = secrets.scrub(&stored.error);
    // The message a later block can read is the scrubbed one: an error carrying a
    // credential must not travel back out through a notification either.
    vars.record(
        &stored.node_id,
        &stored.status,
        live_output,
        stored.bytes as usize,
        &stored.error,
    );
    if let Err(e) = store::add_step(&state.pool, run_id, &stored).await {
        tracing::error!("recording automator step: {e:#}");
    }
    steps.push(stored);
}

/// Park an oversized output in a blob and hand back the handle that replaces it.
async fn park_oversized(
    state: &AppState,
    run_id: Uuid,
    node_id: &str,
    output: Value,
) -> (Value, usize) {
    let text = serde_json::to_string(&output).unwrap_or_default();
    if text.len() <= INLINE_LIMIT {
        return (output, text.len());
    }
    match store::put_blob(&state.pool, run_id, node_id, "application/json", text.as_bytes()).await {
        Ok(id) => (
            json!({
                "_blob": id.to_string(),
                "bytes": text.len(),
                "note": "stored payload, too large to carry inline",
            }),
            text.len(),
        ),
        Err(e) => {
            tracing::error!("storing automator blob: {e:#}");
            (json!({ "_truncated": true, "bytes": text.len() }), text.len())
        }
    }
}

async fn load_perms(state: &AppState, workflow: &Workflow) -> (Value, String) {
    let Some(id) = workflow.mcp_token_id else {
        return (json!({}), String::new());
    };
    match otw_store::mcp::get_token(&state.pool, id).await {
        Ok(Some(token)) if !token.is_expired() => (token.permissions, token.name),
        Ok(Some(token)) => (json!({}), token.name),
        _ => (json!({}), String::new()),
    }
}

fn finish(steps: Vec<StepRecord>, status: &str, error: &str) -> RunResult {
    RunResult { status: status.to_string(), error: error.to_string(), steps }
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// Start a run in the background (manual trigger and the scheduler both use this).
pub fn spawn(state: AppState, workflow: Workflow, graph: Graph, run: Run, mode: RunMode) {
    tokio::spawn(async move {
        let result = execute(&state, &workflow, &graph, &run, mode).await;
        if result.status != "ok" {
            tracing::warn!(
                "automator workflow \"{}\" finished {}: {}",
                workflow.name,
                result.status,
                result.error
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automator::Node;

    fn ctx(status: &str) -> expr::Ctx {
        let mut vars = expr::Ctx::default();
        vars.record("writer", status, json!({ "body": { "id": 7 } }), 0, "");
        vars
    }

    fn reader(template: &str) -> Node {
        Node {
            id: "reader".into(),
            kind: "api".into(),
            name: String::new(),
            config: json!({ "method": "GET", "path": template }),
            pos: Default::default(),
            on_error: "stop".into(),
            timeout_secs: None,
            retry: Default::default(),
        }
    }

    #[test]
    fn a_block_reading_a_simulated_write_is_itself_simulated() {
        let node = reader("/api/todos/{{steps.writer.output.body.id}}");
        assert_eq!(simulated_source(&node, &ctx("simulated")).as_deref(), Some("writer"));
    }

    #[test]
    fn the_same_block_runs_when_the_write_really_happened() {
        let node = reader("/api/todos/{{steps.writer.output.body.id}}");
        assert!(simulated_source(&node, &ctx("ok")).is_none());
    }

    #[test]
    fn reading_only_the_status_of_a_simulated_block_still_runs() {
        let node = reader("/api/todos?note={{steps.writer.status}}");
        assert!(simulated_source(&node, &ctx("simulated")).is_none());
    }
}
