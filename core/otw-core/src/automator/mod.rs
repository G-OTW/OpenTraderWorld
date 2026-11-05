//! Automator: scheduled and manual workflows over the app's own API and the outside world.
//!
//! A workflow is a DAG of nodes stored as JSON. The engine walks it one node at a time,
//! recording a step per node, so a failed run always says which node failed and with what
//! payload. Everything a node needs from the nodes before it travels through `{{ … }}`
//! expressions (see [`expr`]), and a payload too large to inline travels as a blob handle.
//!
//! Three boundaries are load-bearing:
//! - **Internal calls** go through the MCP catalog allowlist and the workflow's `mcp_tokens`
//!   envelope. No token means no internal call runs at all (fail closed).
//! - **External calls** refuse private network targets unless the node says otherwise, and
//!   the resolved IP is what is checked, not the hostname.
//! - **Secrets** only resolve in fields declared secret-bearing, and every resolved secret
//!   is scrubbed from the recorded trace.
//!
//! The runner lives here rather than in `otw-scheduler` because it dispatches into the real
//! Axum router (`crate::internal_call`), which is defined in this crate.

pub mod engine;
pub mod expr;
pub mod nodes;
pub mod schedule;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Hard caps on a stored graph. A workflow is an automation, not a program.
pub const MAX_NODES: usize = 60;
pub const MAX_EDGES: usize = 200;
pub const MAX_EDGES_PER_PORT: usize = 8;

/// Every node kind the engine can run. The palette is generated from this list plus the
/// MCP catalog (for `api`), so adding a kind is one entry here and one module in `nodes/`.
pub const KINDS: &[&str] = &["api", "http", "agent", "notify", "if", "transform", "delay"];

/// Output ports. `out` is the normal path, `true`/`false` belong to an `if` node, and
/// `error` is only followed when the node's `on_error` is `continue`.
pub const PORTS: &[&str] = &["out", "true", "false", "error"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Pos {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

/// Retry policy for one node. Applied to transient node failures only (an HTTP call, an
/// internal call, an agent turn); a validation error is never retried.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retry {
    #[serde(default)]
    pub count: u32,
    #[serde(default = "default_backoff")]
    pub backoff_secs: u32,
}

impl Default for Retry {
    fn default() -> Self {
        Self { count: 0, backoff_secs: default_backoff() }
    }
}

fn default_backoff() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub pos: Pos,
    /// `stop` (default) fails the run; `continue` follows the `error` port instead.
    #[serde(default = "default_on_error")]
    pub on_error: String,
    /// Per-node deadline; falls back to the node kind's own default.
    #[serde(default)]
    pub timeout_secs: Option<u32>,
    #[serde(default)]
    pub retry: Retry,
}

fn default_on_error() -> String {
    "stop".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default = "default_port")]
    pub port: String,
}

fn default_port() -> String {
    "out".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Graph {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

impl Graph {
    pub fn parse(value: &Value) -> Result<Self, String> {
        serde_json::from_value(value.clone()).map_err(|e| format!("invalid graph: {e}"))
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Nodes in execution order: a topological sort that keeps the authoring order as the
    /// tie-break, so two independent branches always run in the order they were drawn.
    pub fn order(&self) -> Result<Vec<usize>, String> {
        let mut indegree = vec![0usize; self.nodes.len()];
        let index: std::collections::HashMap<&str, usize> =
            self.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
        for e in &self.edges {
            if let Some(&to) = index.get(e.to.as_str()) {
                indegree[to] += 1;
            }
        }
        let mut ready: Vec<usize> =
            (0..self.nodes.len()).filter(|i| indegree[*i] == 0).collect();
        ready.sort_unstable();
        let mut out = Vec::with_capacity(self.nodes.len());
        while let Some(i) = ready.first().copied() {
            ready.remove(0);
            out.push(i);
            let id = self.nodes[i].id.as_str();
            for e in self.edges.iter().filter(|e| e.from == id) {
                if let Some(&to) = index.get(e.to.as_str()) {
                    indegree[to] -= 1;
                    if indegree[to] == 0 {
                        ready.push(to);
                        ready.sort_unstable();
                    }
                }
            }
        }
        if out.len() != self.nodes.len() {
            return Err("the workflow has a cycle: nodes must flow forward".into());
        }
        Ok(out)
    }

    /// Ids of the nodes whose output this node can read (its ancestors), for the editor's
    /// reference helper and for save-time validation of `{{steps.…}}`.
    pub fn ancestors(&self, id: &str) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut stack = vec![id.to_string()];
        while let Some(current) = stack.pop() {
            for e in self.edges.iter().filter(|e| e.to == current) {
                if seen.insert(e.from.clone()) {
                    stack.push(e.from.clone());
                }
            }
        }
        let mut out: Vec<String> = seen.into_iter().collect();
        out.sort();
        out
    }
}

/// Validate a graph the way the API must: every failure is a message a user can act on.
pub fn validate(graph: &Graph) -> Result<(), String> {
    if graph.nodes.len() > MAX_NODES {
        return Err(format!("a workflow holds at most {MAX_NODES} blocks"));
    }
    if graph.edges.len() > MAX_EDGES {
        return Err(format!("a workflow holds at most {MAX_EDGES} links"));
    }
    let mut ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for n in &graph.nodes {
        if n.id.is_empty()
            || n.id.len() > 40
            || !n.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!("invalid block id \"{}\"", n.id));
        }
        if !ids.insert(n.id.as_str()) {
            return Err(format!("duplicate block id \"{}\"", n.id));
        }
        if !KINDS.contains(&n.kind.as_str()) {
            return Err(format!("unknown block type \"{}\"", n.kind));
        }
        if n.on_error != "stop" && n.on_error != "continue" {
            return Err(format!("block \"{}\": on_error must be stop or continue", label(n)));
        }
        if n.retry.count > 5 {
            return Err(format!("block \"{}\": at most 5 retries", label(n)));
        }
        if n.retry.backoff_secs > 300 {
            return Err(format!("block \"{}\": retry backoff caps at 300 s", label(n)));
        }
        if n.timeout_secs.is_some_and(|t| t == 0 || t > 600) {
            return Err(format!("block \"{}\": timeout must be between 1 and 600 s", label(n)));
        }
        nodes::validate_config(&n.kind, &n.config)
            .map_err(|e| format!("block \"{}\": {e}", label(n)))?;
    }

    let mut per_port: std::collections::HashMap<(&str, &str), usize> =
        std::collections::HashMap::new();
    for e in &graph.edges {
        if !ids.contains(e.from.as_str()) || !ids.contains(e.to.as_str()) {
            return Err(format!("a link points at a missing block ({} -> {})", e.from, e.to));
        }
        if e.from == e.to {
            return Err("a block cannot link to itself".into());
        }
        if !PORTS.contains(&e.port.as_str()) {
            return Err(format!("unknown link port \"{}\"", e.port));
        }
        let from = graph.node(&e.from).expect("checked above");
        let branching = from.kind == "if";
        if branching && e.port == "out" {
            return Err(format!(
                "block \"{}\" is a condition: link it from its Yes or No port",
                label(from)
            ));
        }
        if !branching && (e.port == "true" || e.port == "false") {
            return Err(format!(
                "block \"{}\" has no Yes/No ports, only a condition does",
                label(from)
            ));
        }
        if e.port == "error" && from.on_error != "continue" {
            return Err(format!(
                "block \"{}\": the error port only exists when its failure policy is Continue",
                label(from)
            ));
        }
        let counter = per_port.entry((e.from.as_str(), e.port.as_str())).or_insert(0);
        *counter += 1;
        if *counter > MAX_EDGES_PER_PORT {
            return Err(format!(
                "block \"{}\": at most {MAX_EDGES_PER_PORT} links leave one port",
                label(from)
            ));
        }
    }
    graph.order()?;

    // Every `{{steps.X…}}` must name a block that actually runs before this one. Catching
    // it here is the difference between a broken link and a run that dies at 3 a.m.
    for n in &graph.nodes {
        let allowed = graph.ancestors(&n.id);
        let mut paths = Vec::new();
        expr::referenced_paths(&n.config, &mut paths);
        for path in paths {
            let Some(rest) = path.strip_prefix("steps.") else {
                let root = path.split('.').next().unwrap_or("");
                if !["run", "input", "workflow", "vault"].contains(&root) {
                    return Err(format!(
                        "block \"{}\": unknown reference \"{{{{{path}}}}}\"",
                        label(n)
                    ));
                }
                continue;
            };
            let target = rest.split(['.', '[']).next().unwrap_or("");
            if !ids.contains(target) {
                return Err(format!(
                    "block \"{}\": \"{{{{{path}}}}}\" names a block that does not exist",
                    label(n)
                ));
            }
            if !allowed.iter().any(|a| a == target) {
                return Err(format!(
                    "block \"{}\": \"{{{{{path}}}}}\" reads a block that does not run before it",
                    label(n)
                ));
            }
        }
    }
    Ok(())
}

/// A node's display label: its name if it has one, else its id.
pub fn label(node: &Node) -> &str {
    if node.name.trim().is_empty() {
        &node.id
    } else {
        &node.name
    }
}

/// How a run treats the actions that change something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Everything runs for real.
    Live,
    /// Reads and computations run; writes, sends and non-GET external calls are recorded
    /// as `simulated` with the exact request they would have made.
    Test { send_notifications: bool, call_external: bool },
}

impl RunMode {
    pub fn is_test(&self) -> bool {
        matches!(self, RunMode::Test { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph(value: serde_json::Value) -> Graph {
        Graph::parse(&value).expect("graph parses")
    }

    fn notify(id: &str) -> serde_json::Value {
        json!({ "id": id, "kind": "notify", "config": { "title": "hi" } })
    }

    #[test]
    fn a_cycle_is_refused() {
        let g = graph(json!({
            "nodes": [notify("a"), notify("b")],
            "edges": [{ "from": "a", "to": "b" }, { "from": "b", "to": "a" }],
        }));
        assert!(validate(&g).unwrap_err().contains("cycle"));
    }

    #[test]
    fn order_follows_the_links_then_the_drawing_order() {
        let g = graph(json!({
            "nodes": [notify("c"), notify("a"), notify("b")],
            "edges": [{ "from": "a", "to": "b" }],
        }));
        let ids: Vec<&str> = g.order().unwrap().iter().map(|i| g.nodes[*i].id.as_str()).collect();
        // "c" and "a" are both roots, so they keep their authoring order; "b" waits for "a".
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn a_reference_to_a_later_block_is_refused() {
        let g = graph(json!({
            "nodes": [
                notify("a"),
                { "id": "b", "kind": "notify",
                  "config": { "title": "{{steps.c.output.body}}" } },
                notify("c"),
            ],
            "edges": [{ "from": "a", "to": "b" }],
        }));
        assert!(validate(&g).unwrap_err().contains("does not run before it"));
    }

    #[test]
    fn a_reference_to_an_upstream_block_is_accepted() {
        let g = graph(json!({
            "nodes": [
                notify("a"),
                { "id": "b", "kind": "notify",
                  "config": { "title": "{{steps.a.output.channels}}" } },
            ],
            "edges": [{ "from": "a", "to": "b" }],
        }));
        validate(&g).expect("upstream reference is fine");
    }

    #[test]
    fn only_a_condition_has_yes_no_ports() {
        let g = graph(json!({
            "nodes": [notify("a"), notify("b")],
            "edges": [{ "from": "a", "to": "b", "port": "true" }],
        }));
        assert!(validate(&g).unwrap_err().contains("Yes/No"));
    }

    #[test]
    fn the_error_port_needs_the_continue_policy() {
        let g = graph(json!({
            "nodes": [notify("a"), notify("b")],
            "edges": [{ "from": "a", "to": "b", "port": "error" }],
        }));
        assert!(validate(&g).unwrap_err().contains("Continue"));
    }

    #[test]
    fn an_unknown_filter_is_caught_at_save_time() {
        let g = graph(json!({
            "nodes": [{ "id": "a", "kind": "notify",
                        "config": { "title": "{{run.id}}", "body": "" } }],
            "edges": [],
        }));
        validate(&g).expect("known root");
        let bad = graph(json!({
            "nodes": [{ "id": "a", "kind": "notify",
                        "config": { "title": "{{nope.id}}" } }],
            "edges": [],
        }));
        assert!(validate(&bad).unwrap_err().contains("unknown reference"));
    }
}
