//! The block catalog: one module per node kind, one dispatch table.
//!
//! A node is a pure-ish function of its config and the run context: it resolves its
//! `{{ … }}` fields, does one thing, and returns what it did (`request`, recorded for the
//! trace) and what it produced (`output`, readable by the nodes after it). Nothing else is
//! shared, so a new block type is one file plus one arm here.

pub mod agent;
pub mod api;
pub mod branch;
pub mod delay;
pub mod http;
pub mod notify;
pub mod transform;

use std::time::Instant;

use serde_json::{json, Value};
use uuid::Uuid;

use super::{expr, Node, RunMode};
use crate::AppState;

/// Everything one node execution can see.
pub struct NodeCtx<'a> {
    pub state: &'a AppState,
    /// The run this node belongs to. Only the blocks that can wait need it, to notice a
    /// stop request without holding the run hostage until they finish.
    pub run_id: Uuid,
    pub mode: RunMode,
    pub node: &'a Node,
    pub vars: &'a expr::Ctx,
    pub resolver: &'a expr::Resolver<'a>,
    /// The workflow's permission envelope: the `mcp_tokens` levels, and the token's name
    /// for the error message when it is missing one.
    pub perms: Value,
    pub token_name: String,
    /// When the whole run must be over. A node never outlives it.
    pub deadline: Instant,
}

impl NodeCtx<'_> {
    /// The node's own deadline: its timeout, clamped by what is left of the run's.
    pub fn budget(&self, default_secs: u64) -> std::time::Duration {
        let asked = std::time::Duration::from_secs(
            self.node.timeout_secs.map(u64::from).unwrap_or(default_secs),
        );
        let left = self.deadline.saturating_duration_since(Instant::now());
        asked.min(left).max(std::time::Duration::from_millis(1))
    }
}

/// What a node produced.
pub struct Outcome {
    /// The resolved request, as it will be shown in the trace (secrets are scrubbed by the
    /// engine before it is stored).
    pub request: Value,
    pub output: Value,
    /// True when the node deliberately did not act because the run is a test.
    pub simulated: bool,
}

impl Outcome {
    pub fn new(request: Value, output: Value) -> Self {
        Self { request, output, simulated: false }
    }
    pub fn simulated(request: Value, reason: &str) -> Self {
        Self {
            request,
            output: json!({ "simulated": true, "reason": reason }),
            simulated: true,
        }
    }
}

/// Run one node.
pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    match ctx.node.kind.as_str() {
        "api" => api::run(ctx).await,
        "http" => http::run(ctx).await,
        "agent" => agent::run(ctx).await,
        "notify" => notify::run(ctx).await,
        "if" => branch::run(ctx).await,
        "transform" => transform::run(ctx).await,
        "delay" => delay::run(ctx).await,
        other => Err(format!("unknown block type \"{other}\"")),
    }
}

/// Validate a node's config at save time, so a broken workflow is refused at the editor
/// rather than at 3 a.m. in a scheduled run.
pub fn validate_config(kind: &str, config: &Value) -> Result<(), String> {
    if !config.is_object() {
        return Err("settings must be an object".into());
    }
    match kind {
        "api" => api::validate(config),
        "http" => http::validate(config),
        "agent" => agent::validate(config),
        "notify" => notify::validate(config),
        "if" => branch::validate(config),
        "transform" => transform::validate(config),
        "delay" => delay::validate(config),
        other => Err(format!("unknown block type \"{other}\"")),
    }
}

/// A required string field of a node config.
pub fn field<'a>(config: &'a Value, key: &str) -> Result<&'a str, String> {
    config
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

/// An optional string field, defaulted. Trimmed: a path, a URL or a method with a stray
/// space around it is the same field.
pub fn opt<'a>(config: &'a Value, key: &str, default: &'a str) -> &'a str {
    config.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(default)
}

/// An optional string field kept **verbatim**. For a separator or a delimiter the
/// whitespace *is* the value: trimming turns ", " into "," and drops a tab entirely.
pub fn raw<'a>(config: &'a Value, key: &str, default: &'a str) -> &'a str {
    config.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(default)
}
