//! The agent's tool registry — its entire action surface.
//!
//! There is **no shell tool by construction**. Three tool families:
//! - always-on memory + skills tools;
//! - OTW data tools, dispatched in-process into the same catalog/permission logic the
//!   network MCP endpoint uses (see [`crate::mcp`]). The permission envelope is a selected
//!   `mcp_tokens` row (by id, no plaintext); its per-module levels (r/rw/rwd, set in
//!   Settings → AI agents) apply DIRECTLY — no agent-side re-validation. The module
//!   allowlist and the settings/secrets/wipe exclusions still apply. `otw_write` is only
//!   offered when the token grants rw/rwd somewhere;
//! - EXTERNAL MCP servers (per-conversation selection, remote Streamable HTTP only — see
//!   [`super::mcp_client`]). Their tools are namespaced `<server-slug>__<tool>` and their
//!   output is untrusted third-party content; the UI warns when combined with write access.

use serde_json::{json, Value};

use super::mcp_client::McpClient;
use super::provider::ToolDef;
use crate::AppState;

/// Separator between the server slug and the tool name in model-visible names. Slugs are
/// [a-z0-9-] only, so a double underscore is unambiguous.
const EXT_SEP: &str = "__";
/// Per-call timeout for external MCP tools.
const EXT_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// One external tool: the name the model sees vs the server's real name.
pub struct ExtTool {
    pub exposed: String,
    pub real: String,
    pub def: ToolDef,
}

/// A connected external MCP server with its listed tools. (The slug is baked into each
/// tool's exposed name at assembly; it isn't needed afterwards, so it isn't stored.)
pub struct ExternalServer {
    pub name: String,
    pub client: McpClient,
    pub tools: Vec<ExtTool>,
}

impl ExternalServer {
    /// Build from a connected client + its tool list, namespacing every tool as
    /// `<slug>__<tool>` (sanitized to the providers' `[A-Za-z0-9_-]{1,64}` rule) and
    /// prefixing descriptions with the server name so provenance stays visible.
    /// Exposed names are deduplicated within the server: two long tool names that truncate
    /// to the same 64-char string would otherwise both dispatch to the first match.
    pub fn assemble(
        name: &str,
        slug: String,
        client: McpClient,
        listed: Vec<(String, String, Value)>,
    ) -> Self {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let tools = listed
            .into_iter()
            .map(|(real, desc, schema)| {
                let clean: String = real
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
                    .collect();
                let budget = 64usize.saturating_sub(slug.len() + EXT_SEP.len()).max(1);
                let mut exposed =
                    format!("{slug}{EXT_SEP}{}", crate::agent::truncate_plain(&clean, budget));
                let mut n = 2;
                while !seen.insert(exposed.clone()) {
                    let suffix = format!("-{n}");
                    let b = budget.saturating_sub(suffix.len()).max(1);
                    exposed = format!(
                        "{slug}{EXT_SEP}{}{suffix}",
                        crate::agent::truncate_plain(&clean, b)
                    );
                    n += 1;
                }
                ExtTool {
                    exposed: exposed.clone(),
                    real,
                    def: ToolDef {
                        name: exposed,
                        description: format!("[{name}] {desc}"),
                        input_schema: schema,
                    },
                }
            })
            .collect();
        Self { name: name.to_string(), client, tools }
    }
}

/// The agent's resolved tool context for one run.
pub struct ToolContext {
    /// The selected token's permission map (module → "r"/"rw"/"rwd"). Empty when no token.
    pub permissions: Value,
    pub token_name: String,
    /// Connected external MCP servers (this conversation's selection).
    pub external: Vec<ExternalServer>,
}

impl ToolContext {
    /// A context with no token — no OTW data tools (memory/skills tools still apply).
    pub fn none() -> Self {
        Self { permissions: json!({}), token_name: String::new(), external: Vec::new() }
    }

    /// True when the token grants write (rw) or full (rwd) on at least one module —
    /// drives whether `otw_write` is offered at all.
    fn can_write_any(&self) -> bool {
        self.permissions
            .as_object()
            .is_some_and(|m| m.values().any(|v| matches!(v.as_str(), Some("rw") | Some("rwd"))))
    }

    /// True when an MCP token is attached (OTW data tools should be offered).
    pub fn has_mcp_tools(&self) -> bool {
        self.permissions.as_object().is_some_and(|m| !m.is_empty())
    }

    /// True when the model should loop for ANY tool this run (MCP, memory, or skills).
    /// Memory + skills tools are always available, so this is currently always true; kept
    /// as a method so the run loop reads intent rather than a literal.
    pub fn has_tools(&self) -> bool {
        true
    }

    /// Build the tool definitions to advertise to the provider this run: the always-on
    /// memory + skills tools, external MCP server tools, plus the OTW data tools when an
    /// MCP token is attached.
    pub fn tool_defs(&self) -> Vec<ToolDef> {
        let mut defs = self.builtin_defs();
        for srv in &self.external {
            defs.extend(srv.tools.iter().map(|t| t.def.clone()));
        }
        if !self.has_mcp_tools() {
            return defs;
        }
        defs.extend([
            ToolDef {
                name: "otw_catalog".into(),
                description: "Discover OpenTraderWorld API endpoints. With no argument, returns a \
                    compact index of the modules you can access. Pass a \"module\" to list that \
                    module's concrete endpoints. Call this before otw_read/otw_write."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "module": { "type": "string", "description": "List this module's endpoints (e.g. \"journal\"). Omit for the module index." }
                    }
                }),
            },
            ToolDef {
                name: "otw_read".into(),
                description: "GET an allowlisted OpenTraderWorld endpoint and return its JSON body. \
                    Use concrete paths with query strings, e.g. /api/journal/trades?limit=20. For \
                    large responses, pass \"pick\" to extract only the fields the user asked for."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string", "description": "API path with optional query string." },
                        "pick": { "type": "array", "items": { "type": "string" }, "description": "Dot-paths to extract from the response, e.g. [\"result.var_hist\"]. Arrays map the remaining path over their elements." },
                        "head": { "type": "integer", "minimum": 1, "description": "Truncate every array in the (picked) response to its first N elements; totals are reported." }
                    }
                }),
            },
        ]);
        if self.can_write_any() {
            defs.push(ToolDef {
                name: "otw_write".into(),
                description: "Mutate through an allowlisted OpenTraderWorld endpoint \
                    (POST/PUT/PATCH/DELETE). Only use after confirming the target with otw_catalog."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["method", "path"],
                    "properties": {
                        "method": { "type": "string", "enum": ["POST", "PUT", "PATCH", "DELETE"] },
                        "path": { "type": "string", "description": "API path, e.g. /api/journal/trades" },
                        "body": { "description": "JSON request body, when the endpoint expects one." },
                        "pick": { "type": "array", "items": { "type": "string" }, "description": "Dot-paths to extract from the response (useful on compute endpoints like /api/quant/*)." },
                        "head": { "type": "integer", "minimum": 1, "description": "Truncate every array in the (picked) response to its first N elements; totals are reported." }
                    }
                }),
            });
        }
        defs
    }

    /// The memory + skills tools. Absent in demo mode: memory and skills are shared
    /// mutable state that every visitor would write into and every later visitor would
    /// read back, and the 15-minute reset makes anything saved vanish mid-conversation.
    /// Not advertising the tools beats offering them and refusing every call.
    fn builtin_defs(&self) -> Vec<ToolDef> {
        if crate::demo::enabled() {
            return Vec::new();
        }
        vec![
            ToolDef {
                name: "memory_write".into(),
                description: "Save a small, durable fact to long-term memory (e.g. a user \
                    preference or a stable detail worth remembering across conversations). \
                    Overwrites any existing memory with the same slug."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["slug", "content"],
                    "properties": {
                        "slug": { "type": "string", "description": "Short kebab-case identifier, e.g. \"preferred-currency\"." },
                        "description": { "type": "string", "description": "One-line summary shown in the memory index." },
                        "content": { "type": "string", "description": "The fact (markdown, kept short)." }
                    }
                }),
            },
            ToolDef {
                name: "memory_read".into(),
                description: "Read the full content of a stored memory by its slug. The memory \
                    index (slugs + descriptions) is already in your context."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["slug"],
                    "properties": { "slug": { "type": "string" } }
                }),
            },
            ToolDef {
                name: "load_skill".into(),
                description: "Load the full instructions for one of the skills listed in your \
                    context (by name). Call this before performing a task the skill covers."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                }),
            },
        ]
    }

    /// Dispatch a tool call by name; returns `(result_text, is_error)`.
    pub async fn call(&self, state: &AppState, name: &str, args: &Value) -> (String, bool) {
        // External MCP tools are namespaced `<server-slug>__<tool>` — route them first.
        if name.contains(EXT_SEP) {
            for srv in &self.external {
                if let Some(t) = srv.tools.iter().find(|t| t.exposed == name) {
                    return match tokio::time::timeout(
                        EXT_CALL_TIMEOUT,
                        srv.client.call_tool(&t.real, args),
                    )
                    .await
                    {
                        Ok(Ok(res)) => res,
                        Ok(Err(e)) => (format!("{} tool failed: {e:#}", srv.name), true),
                        Err(_) => (
                            format!(
                                "{} tool timed out after {}s",
                                srv.name,
                                EXT_CALL_TIMEOUT.as_secs()
                            ),
                            true,
                        ),
                    };
                }
            }
            return (format!("unknown external tool: {name}"), true);
        }
        // Defence in depth: `builtin_defs` stops advertising these in demo mode, but a
        // model can still emit a call for a tool it was never offered (or one it saw
        // earlier in a resumed conversation), so refuse at the dispatch too.
        if crate::demo::enabled() && matches!(name, "memory_write" | "memory_read" | "load_skill") {
            return ("this tool is disabled in the public demo".into(), true);
        }
        match name {
            "memory_write" => {
                let slug = crate::agent::slugify(args.get("slug").and_then(|v| v.as_str()).unwrap_or(""));
                if slug.is_empty() {
                    return ("memory_write requires a non-empty \"slug\"".into(), true);
                }
                if slug.chars().count() > otw_store::agent::MEMORY_MAX_SLUG {
                    return (format!("slug too long (max {} chars)", otw_store::agent::MEMORY_MAX_SLUG), true);
                }
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.chars().count() > otw_store::agent::MEMORY_MAX_LEN {
                    return (format!("memory too long (max {} chars)", otw_store::agent::MEMORY_MAX_LEN), true);
                }
                // Slugs + descriptions ride in every system prompt: keep the description
                // bounded no matter what the model sends.
                let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("").trim();
                let description = crate::agent::truncate(description, otw_store::agent::MEMORY_MAX_DESC);
                // Enforce the count cap on new slugs.
                match otw_store::agent::get_memory(&state.pool, &slug).await {
                    Ok(None) => {
                        if otw_store::agent::count_memories(&state.pool).await.unwrap_or(0)
                            >= otw_store::agent::MEMORY_MAX_COUNT
                        {
                            return ("memory limit reached; ask the user to prune old memories".into(), true);
                        }
                    }
                    Err(e) => return (format!("memory lookup failed: {e}"), true),
                    _ => {}
                }
                // kind "agent" marks provenance (the UI badges agent-written memories).
                match otw_store::agent::upsert_memory(&state.pool, &slug, &description, content, "agent").await {
                    Ok(m) => (format!("saved memory \"{}\"", m.slug), false),
                    Err(e) => (format!("memory_write failed: {e}"), true),
                }
            }
            "memory_read" => {
                let slug = args.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                match otw_store::agent::get_memory(&state.pool, slug).await {
                    Ok(Some(m)) => (m.content, false),
                    Ok(None) => (format!("no memory with slug \"{slug}\""), true),
                    Err(e) => (format!("memory_read failed: {e}"), true),
                }
            }
            "load_skill" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                match otw_store::agent::skill_body(&state.pool, name).await {
                    Ok(Some(body)) => (body, false),
                    Ok(None) => (format!("no enabled skill named \"{name}\""), true),
                    Err(e) => (format!("load_skill failed: {e}"), true),
                }
            }
            "otw_catalog" => {
                let only = args.get("module").and_then(|v| v.as_str());
                (crate::mcp::agent_catalog(&self.permissions, only), false)
            }
            "otw_read" => {
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return ("otw_read requires a \"path\" string".into(), true);
                };
                let shape = match crate::mcp::shape_from(args) {
                    Ok(s) => s,
                    Err(msg) => return (msg, true),
                };
                crate::mcp::agent_call(
                    state,
                    &self.permissions,
                    &self.token_name,
                    "GET",
                    path,
                    None,
                    shape,
                )
                .await
            }
            "otw_write" => {
                let method = args.get("method").and_then(|v| v.as_str()).unwrap_or("").to_ascii_uppercase();
                if !matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
                    return ("otw_write method must be POST, PUT, PATCH or DELETE".into(), true);
                }
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return ("otw_write requires a \"path\" string".into(), true);
                };
                let shape = match crate::mcp::shape_from(args) {
                    Ok(s) => s,
                    Err(msg) => return (msg, true),
                };
                crate::mcp::agent_call(
                    state,
                    &self.permissions,
                    &self.token_name,
                    &method,
                    path,
                    args.get("body").cloned(),
                    shape,
                )
                .await
            }
            other => (format!("unknown tool: {other}"), true),
        }
    }
}
