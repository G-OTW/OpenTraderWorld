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
/// How long the `chart` tool waits for its source endpoint.
const CHART_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Cap on a third-party tool description. Every advertised tool's description is re-sent on
/// every turn, and nothing bounds what an external server writes there.
const EXT_DESC_CAP: usize = 600;

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
                // A server's own description is arbitrary text we re-send on every turn of
                // every run. Some ship a manual page per tool; cap it at something a model
                // can act on and let the schema carry the rest.
                let desc = crate::agent::truncate(desc.trim(), EXT_DESC_CAP);
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
    /// Names of the skills this agent may load — the same list the system prompt advertises.
    /// `load_skill` refuses anything outside it, so a persona cannot reach another persona's
    /// shelf by guessing a name it saw elsewhere.
    pub skills: Vec<String>,
    /// The conversation this run belongs to — the unit the simulation budget is charged to,
    /// and the key a pending write is answered under.
    pub conversation_id: uuid::Uuid,
    /// The persona speaking. Stamped onto anything it writes to memory, so a fact recorded by
    /// one persona is never read by another as a global truth.
    pub agent_id: Option<uuid::Uuid>,
    /// Skip the confirm step on non-destructive writes (the agent's `auto_approve_writes`).
    /// Deletes confirm regardless — see [`crate::agent::confirm`].
    pub auto_approve_writes: bool,
    /// Long-term memory is on for this chat. Off: the memory tools are neither offered nor
    /// accepted (the prompt also leaves the index out).
    pub memory: bool,
    /// The run's output can show a chart (the in-app chat). Off on chat channels, which only
    /// carry text: the `chart` tool is neither offered nor accepted there.
    pub charts: bool,
}

impl ToolContext {
    /// A context with no token — no OTW data tools (memory/skills tools still apply).
    pub fn none() -> Self {
        Self {
            permissions: json!({}),
            token_name: String::new(),
            external: Vec::new(),
            skills: Vec::new(),
            conversation_id: uuid::Uuid::nil(),
            agent_id: None,
            auto_approve_writes: false,
            memory: true,
            charts: true,
        }
    }

    /// The write this call would make, when it needs the user's consent first. `None` means
    /// "just run it": every non-write tool, and a plain write when the user has opted into
    /// auto-approval.
    ///
    /// Malformed calls return `None` too — they fail in `call` with a message that teaches the
    /// fix, and asking the user to approve a call that cannot run is noise.
    pub fn confirm_required(&self, name: &str, args: &Value) -> Option<super::confirm::WriteIntent> {
        // Deleting a memory destroys data, so it confirms like any delete — the pseudo-path
        // `memory:<slug>` is only for the confirm frame's display. Never auto-approved
        // (deletes never are). A malformed call returns None and fails in `call` with a hint.
        if name == "memory_delete" {
            let slug = crate::agent::slugify(args.get("slug").and_then(|v| v.as_str()).unwrap_or(""));
            if slug.is_empty() {
                return None;
            }
            return Some(super::confirm::WriteIntent {
                method: "DELETE".into(),
                path: format!("memory:{slug}"),
                body: None,
                destructive: true,
            });
        }
        if name != "otw_write" {
            return None;
        }
        // A validate_only call is checked and never dispatched: there is nothing to approve.
        if is_validate_only(args) {
            return None;
        }
        let method = args.get("method").and_then(|v| v.as_str())?.to_ascii_uppercase();
        let path = args.get("path").and_then(|v| v.as_str())?;
        let destructive = method == "DELETE";
        if !destructive && self.auto_approve_writes {
            return None;
        }
        // Compute endpoints answer a question instead of storing something (`/quant/*`,
        // `/backtest/run`…). They ride on POST because they take a body, not because they
        // change anything — confirming them would bury the writes that matter under a stream
        // of prompts for arithmetic.
        let head = path.split('?').next().unwrap_or(path);
        if !destructive
            && crate::mcp::catalog::lookup(&method, head).is_some_and(|e| e.compute)
        {
            return None;
        }
        Some(super::confirm::WriteIntent {
            method,
            path: path.to_string(),
            body: args.get("body").cloned(),
            destructive,
        })
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
                    module's endpoints, or an \"endpoint\" (or several in \"endpoints\") to get its \
                    description, a working example and its body fields before writing it; add \
                    \"full\": true with one endpoint for its raw JSON Schema, only when the \
                    compact field table did not answer. Call this before otw_read/otw_write."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "module": { "type": "string", "description": "List this module's endpoints (e.g. \"journal\"). Omit for the module index." },
                        "endpoint": { "type": "string", "description": "One endpoint as the listing prints it, e.g. \"POST /api/backtest/run\": returns its description, example and body fields. Takes precedence over module." },
                        "endpoints": { "type": "array", "items": { "type": "string" }, "description": "Several endpoints at once, same form as endpoint: one block each." },
                        "full": { "type": "boolean", "description": "With one endpoint: return its raw request-body JSON Schema instead of the field table, cut at 24000 characters. Refused with several endpoints; ask it only when the compact field table did not answer." }
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
                        "pick": { "type": "array", "items": { "type": "string" }, "description": "Dot-paths to extract from the response, e.g. [\"result.var_hist\"]. Arrays map the remaining path over their elements. WARNING on series endpoints that return parallel column arrays (e.g. /api/histdata/.../bars, which returns ts/o/h/l/c/v side by side): picking one column drops the timestamps and every date you then infer by counting positions will be wrong — pick \"ts\" alongside it, or do not pick at all." },
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
                        "body": { "description": "Required for every POST, PUT and PATCH: the JSON object goes here, never in path. GET and DELETE take none." },
                        "pick": { "type": "array", "items": { "type": "string" }, "description": "Dot-paths to extract from the response (useful on compute endpoints like /api/quant/*)." },
                        "head": { "type": "integer", "minimum": 1, "description": "Truncate every array in the (picked) response to its first N elements; totals are reported." },
                        "validate_only": { "type": "boolean", "description": "Check the body against the endpoint's type and schema and answer ok or every problem, without running or storing anything (no confirmation, no simulation charged)." }
                    }
                }),
            });
        }
        if self.charts {
            defs.push(chart_def(&self.permissions));
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
        let mut defs = vec![
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
                name: "memory_search".into(),
                description: "Find stored memories whose slug, description or body contains a \
                    word. Use it when the memory index in your context does not show what you \
                    are looking for — older memories are listed without their description, and \
                    past a point not listed at all."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": { "type": "string", "description": "A word or fragment to look for." }
                    }
                }),
            },
            ToolDef {
                name: "memory_delete".into(),
                description: "Delete a stored memory by its slug. Destructive: the user is always \
                    asked to confirm first. Use it to prune when the store is full, or when the \
                    user asks to forget something — never on your own initiative otherwise."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["slug"],
                    "properties": { "slug": { "type": "string" } }
                }),
            },
        ];
        if !self.memory {
            defs.retain(|d| !d.name.starts_with("memory_"));
        }
        // Only offer load_skill when this agent actually has a shelf — an agent whose
        // allowlist matches nothing enabled would otherwise be handed a tool with nothing
        // to load.
        if !self.skills.is_empty() {
            defs.push(ToolDef {
                name: "load_skill".into(),
                description: "Load the full instructions for one of the skills listed in your \
                    context (by name). Call this before performing a task the skill covers."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                }),
            });
        }
        defs
    }

    /// Dispatch a tool call by name; returns `(result_text, is_error)`.
    pub async fn call(&self, state: &AppState, name: &str, args: &Value) -> (String, bool) {
        // Both adapters park unparseable tool arguments under `_raw` rather than dropping the
        // call. That happens when the provider cuts the reply mid-arguments — a max_tokens
        // truncation lands as a half-written JSON object. Say so: otherwise the call falls
        // through to whichever field looks missing and the model is told "method must be
        // POST…" when the real problem is that its arguments never arrived intact.
        if let Some(raw) = args.get("_raw").and_then(|v| v.as_str()) {
            // A retry is worth it, so make the failure legible and actionable.
            if serde_json::from_str::<Value>(raw).is_err() {
                return (
                    format!(
                        "the arguments for `{name}` were not valid JSON, so the call could not \
                         be made. This usually means the reply was cut off by the token limit \
                         partway through writing them ({} characters arrived). Send the call \
                         again — shorter, or split into several calls.",
                        raw.len()
                    ),
                    true,
                );
            }
        }
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
        if crate::demo::enabled()
            && matches!(name, "memory_write" | "memory_read" | "memory_delete" | "load_skill")
        {
            return ("this tool is disabled in the public demo".into(), true);
        }
        if !self.memory && name.starts_with("memory_") {
            return ("memory is turned off for this conversation".into(), true);
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
                match otw_store::agent::get_memory(&state.pool, &slug).await {
                    // A memory the user wrote by hand is theirs. The agent may not overwrite it
                    // silently — the whole confirm design says the model asking is not the user
                    // agreeing, and memory_write has no confirm step. Refuse and teach the fix.
                    Ok(Some(m)) if m.kind == "manual" => {
                        return (
                            format!(
                                "\"{slug}\" is a memory the user wrote by hand — you may not \
                                 overwrite it. Save your version under a different slug, or ask \
                                 the user to change theirs."
                            ),
                            true,
                        );
                    }
                    // New slug: enforce the count cap. (Updating an agent-written memory does
                    // not grow the store, so it is exempt.)
                    Ok(None) => {
                        if otw_store::agent::count_memories(&state.pool).await.unwrap_or(0)
                            >= otw_store::agent::MEMORY_MAX_COUNT
                        {
                            // A bare "it is full" leaves the model guessing at 200 slugs.
                            // Name the entries nobody has read since they were written: the
                            // user gets a shortlist to confirm instead of a dead end.
                            let stale = otw_store::agent::stale_memories(&state.pool, 5)
                                .await
                                .unwrap_or_default();
                            let suggestion = if stale.is_empty() {
                                String::new()
                            } else {
                                format!(
                                    " Least recently read, so likeliest to be droppable: {}.",
                                    stale.join(", ")
                                )
                            };
                            return (
                                format!(
                                    "memory is full (limit reached). Ask the user which stored \
                                     memories to remove, then use memory_delete (they confirm \
                                     each deletion) before saving this one.{suggestion}"
                                ),
                                true,
                            );
                        }
                    }
                    Err(e) => return (format!("memory lookup failed: {e}"), true),
                    // An existing agent-written memory: fine to update in place.
                    Ok(Some(_)) => {}
                }
                // kind "agent" marks provenance (the UI badges agent-written memories);
                // agent_id records WHICH persona, which the shared store needs.
                match otw_store::agent::upsert_memory(&state.pool, &slug, &description, content, "agent", self.agent_id).await {
                    Ok(m) => (format!("saved memory \"{}\"", m.slug), false),
                    Err(e) => (format!("memory_write failed: {e}"), true),
                }
            }
            "memory_read" => {
                let slug = args.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                match otw_store::agent::get_memory(&state.pool, slug).await {
                    Ok(Some(m)) => {
                        // Stamp the read so a full store can name what nobody recalls any
                        // more. Best-effort: failing to record it must not fail the read.
                        let _ = otw_store::agent::touch_memory(&state.pool, &m.slug).await;
                        (m.content, false)
                    }
                    Ok(None) => (format!("no memory with slug \"{slug}\""), true),
                    Err(e) => (format!("memory_read failed: {e}"), true),
                }
            }
            "memory_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").trim();
                if query.is_empty() {
                    return ("memory_search requires a non-empty \"query\"".into(), true);
                }
                match otw_store::agent::search_memories(&state.pool, query, 20).await {
                    Ok(hits) if hits.is_empty() => {
                        (format!("no memory matches \"{query}\""), false)
                    }
                    Ok(hits) => {
                        let mut out = format!("{} match(es) — read one with memory_read:\n", hits.len());
                        for (slug, desc, author) in hits {
                            let by = author.map(|a| format!(" [{a}]")).unwrap_or_default();
                            out.push_str(&format!("- {slug}{by}: {desc}\n"));
                        }
                        (out, false)
                    }
                    Err(e) => (format!("memory_search failed: {e}"), true),
                }
            }
            "memory_delete" => {
                // The confirm gate ran before this (destructive → always asked); reaching here
                // means the user approved. Slug the input the same way memory_write does so the
                // model can pass the slug exactly as the index shows it.
                let slug = crate::agent::slugify(args.get("slug").and_then(|v| v.as_str()).unwrap_or(""));
                if slug.is_empty() {
                    return ("memory_delete requires a non-empty \"slug\"".into(), true);
                }
                match otw_store::agent::delete_memory(&state.pool, &slug).await {
                    Ok(true) => (format!("deleted memory \"{slug}\""), false),
                    Ok(false) => (format!("no memory with slug \"{slug}\""), true),
                    Err(e) => (format!("memory_delete failed: {e}"), true),
                }
            }
            "load_skill" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                // Enforce the shelf: only what the system prompt advertised is loadable.
                if !self.skills.iter().any(|s| s == name) {
                    return (
                        format!(
                            "no skill named \"{name}\" is available to you. Yours: {}",
                            self.skills.join(", ")
                        ),
                        true,
                    );
                }
                match otw_store::agent::skill_body(&state.pool, name).await {
                    Ok(Some(body)) => (body, false),
                    Ok(None) => (format!("no enabled skill named \"{name}\""), true),
                    Err(e) => (format!("load_skill failed: {e}"), true),
                }
            }
            "otw_catalog" => {
                let (mut text, route, known) = crate::mcp::agent_catalog_for(&self.permissions, args);
                crate::mcp::log_agent_catalog(state, &self.token_name, &route, known);
                if self.charts {
                    text.push_str(&crate::mcp::chart_hints_for(&self.permissions, args));
                }
                (text, !known)
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
                    false,
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
                let validate_only = is_validate_only(args);
                if !validate_only {
                    if let Err(msg) = self.charge_simulations(state, path, args).await {
                        return (msg, true);
                    }
                }
                crate::mcp::agent_call(
                    state,
                    &self.permissions,
                    &self.token_name,
                    &method,
                    path,
                    args.get("body").cloned(),
                    shape,
                    validate_only,
                )
                .await
            }
            "chart" => self.chart(state, args).await,
            other => (format!("unknown tool: {other}"), true),
        }
    }

    /// Read the source series in-process, reduce it, store it and answer with its summary.
    /// The series itself never enters the model's context.
    async fn chart(&self, state: &AppState, args: &Value) -> (String, bool) {
        if !self.charts {
            return ("charts cannot be shown on this channel: describe the numbers instead".into(), true);
        }
        let spec = match super::chart::parse_spec(args) {
            Ok(s) => s,
            Err(msg) => return (msg, true),
        };
        // Inline rows are figures the model already read: nothing to fetch.
        let (body, untrusted) = match spec.path() {
            None => (Value::Null, false),
            Some(path) => {
                let fetched = tokio::time::timeout(
                    CHART_FETCH_TIMEOUT,
                    crate::mcp::agent_fetch_json(state, &self.permissions, &self.token_name, path),
                )
                .await;
                match fetched {
                    Ok(Ok(v)) => v,
                    Ok(Err(msg)) => return (msg, true),
                    Err(_) => {
                        return (
                            format!("{path} did not answer within {}s: narrow the range", CHART_FETCH_TIMEOUT.as_secs()),
                            true,
                        )
                    }
                }
            }
        };
        // Sorting and reducing up to the raw cap is CPU work: keep it off the async workers.
        let job = spec.clone();
        let built = match tokio::task::spawn_blocking(move || super::chart::build(&job, body, untrusted)).await {
            Ok(Ok(b)) => b,
            Ok(Err(msg)) => return (msg, true),
            Err(e) => return (format!("chart failed: {e}"), true),
        };
        let id = match otw_store::agent::insert_chart(
            &state.pool,
            self.conversation_id,
            &spec.to_json(args),
            &built.data,
            built.points_raw.min(i32::MAX as usize) as i32,
            built.points_shown.min(i32::MAX as usize) as i32,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => return (format!("chart could not be stored: {e}"), true),
        };
        let mut out = built.summary;
        out["chart_id"] = json!(id);
        out["points_raw"] = json!(built.points_raw);
        out["shown_to_user"] = json!(
            "the chart is displayed in the conversation: comment on it, do not repeat its data as a table"
        );
        (out.to_string(), false)
    }

    /// Charge a simulating endpoint against this conversation's budget (R11), before it runs.
    ///
    /// The endpoints that cost real CPU are the ones a grid search repeats: `/backtest/run`,
    /// one charge per call, and `/backtest/sweep`, one per trial it is about to execute. Both
    /// are the p-hacking surface — an agent told to "optimise this" will otherwise keep going
    /// until it finds a number it likes, and the honest answer needs the trial count anyway.
    ///
    /// The refusal names the budget and the total, so the model reports a bounded search
    /// instead of quietly reformulating the same sweep.
    async fn charge_simulations(
        &self,
        state: &AppState,
        path: &str,
        args: &Value,
    ) -> Result<(), String> {
        let head = path.split('?').next().unwrap_or(path).trim_end_matches('/');
        let cost = match head {
            // A dry run validates and counts bars: it simulates nothing, so it costs nothing.
            "/api/backtest/run" if is_dry_run(args.get("body")) => return Ok(()),
            "/api/backtest/run" => 1,
            "/api/backtest/sweep" => crate::backtest_api::sweep_trial_count(args.get("body"))
                .unwrap_or(1)
                .clamp(1, i32::MAX as usize) as i32,
            _ => return Ok(()),
        };
        if self.conversation_id.is_nil() {
            return Ok(()); // no conversation to charge (in-process call outside a run)
        }
        let budget = otw_store::agent::BACKTEST_RUN_BUDGET;
        match otw_store::agent::charge_backtest_runs(&state.pool, self.conversation_id, cost).await {
            Ok(Ok(_)) => Ok(()),
            // Refused: nothing was charged, so say what still fits. The model can resize the
            // search in one step instead of shrinking it blindly against a budget that keeps
            // moving away from it.
            Ok(Err(remaining)) if remaining == 0 => Err(format!(
                "simulation budget exhausted for this conversation ({budget} of {budget} used). \
                 Stop searching and report what you have — the trials you already ran, their \
                 spread, and how many you would still need. Start a new conversation if the \
                 search is genuinely unfinished."
            )),
            Ok(Err(remaining)) => Err(format!(
                "this call asks for {cost} simulations but only {remaining} of the {budget} for \
                 this conversation are left, so NOTHING was run and nothing was charged. Resize \
                 the search to {remaining} trials or fewer and call again — or report what you \
                 have and say how many trials a complete search would need."
            )),
            // A bookkeeping failure must not block the work; log and let it through.
            Err(e) => {
                tracing::warn!("agent: charging the simulation budget failed: {e:#}");
                Ok(())
            }
        }
    }
}

/// The `chart` tool: offered with the OTW data tools, since its source is an OTW endpoint.
/// The ready calls of the series endpoints this token can read are listed in the description,
/// so the common charts take one call and no probing.
fn chart_def(perms: &Value) -> ToolDef {
    let mut description = String::from(
        "Draw a chart in the conversation. The server reads the data, reshapes and reduces it \
         (any size) and shows the chart to the user; you get back only a short summary (first, \
         last, min, max, mean per series, or slices, or distribution stats).\n\
         Source, one of:\n\
         - path: a GET endpoint (same access as otw_read). Name x and y by dot-path: arrays map \
         the rest of the path over their elements (\"trades.pnl\" reads every trade's pnl), \
         parallel column arrays (ts/c) are named directly, for [[a, b]] pairs use \
         \"observations.0\" and \"observations.1\". Or set rows (the array whose elements are \
         the rows, e.g. \"trades\") and name x and y as fields of a row. Or fold single values \
         (stats) into {key, value} rows: fold [\"run.stats.all.gross_profit\", \
         \"run.stats.all.gross_loss\"] draws them as slices or bars with no x/y needed.\n\
         - data: up to 100 rows written in the call, only for figures you read from a tool \
         result in this conversation. Never type a series by hand: read it with path.\n\
         transform reshapes the rows before drawing, steps in order: filter {field, cmp: \
         eq|ne|gt|gte|lt|lte|in|contains, value}; derive {field, fn, as} with fn sign (labels \
         {\"positive\":\"gain\",\"negative\":\"loss\"} renames), abs, neg, scale/bucket (arg), \
         add/sub/mul/div (other field or arg), year, month, month_of_year, day, weekday, hour; \
         aggregate {groupby: [fields], ops: [{fn: sum|mean|count|min|max|median|first|last, \
         field, as}]}; sort {field, order}; limit {n}; cumsum {field, as}. After an aggregate, x \
         and y name its output fields. When unsure of an endpoint's fields, otw_read it with \
         head first.",
    );
    let hints = crate::mcp::chart_hints(perms);
    if !hints.is_empty() {
        description.push_str("\nReady calls (substitute {id}, adjust the query):");
        for (_, args) in hints {
            description.push_str("\n- ");
            description.push_str(args);
        }
    }
    ToolDef {
        name: "chart".into(),
        description,
        input_schema: json!({
            "type": "object",
            "required": ["type"],
            "properties": {
                "type": { "type": "string", "enum": ["line", "area", "bar", "scatter", "donut", "histogram"], "description": "line/area: a series over time or order. bar: up to 500 categories or periods. scatter: y against x. donut: shares summed by label (non-negative). histogram: distribution of one value." },
                "title": { "type": "string", "description": "Short chart title." },
                "path": { "type": "string", "description": "GET endpoint with its query string, e.g. /api/journal/trades?limit=5000. Give path or data." },
                "data": { "type": "array", "items": { "type": "object" }, "maxItems": 100, "description": "Rows written in the call, e.g. [{\"label\":\"gross profit\",\"value\":39510.8}]. Only figures read from a tool result." },
                "rows": { "type": "string", "description": "Dot-path of the array whose elements are the rows (path source). x, y and transform fields are then fields of a row." },
                "fold": { "type": "array", "items": { "type": "string" }, "maxItems": 20, "description": "Single values turned into {key, value} rows: dot-paths, each optionally followed by \" as <label>\", e.g. \"run.stats.all.gross_profit as gross profit\"." },
                "transform": { "type": "array", "items": { "type": "object" }, "maxItems": 12, "description": "Steps run over the rows before drawing, each with an \"op\" (see the tool description)." },
                "x": { "type": "string", "description": "Field or dot-path of the x values (dates, numbers or labels); for a donut, the label of each slice. Optional for line/area (position), ignored by histogram, defaults to key with fold." },
                "y": { "type": "array", "items": { "type": "string" }, "description": "Fields or dot-paths of the values, one per series (max 8). Exactly one for donut and histogram. Defaults to value with fold." },
                "names": { "type": "array", "items": { "type": "string" }, "description": "Display name of each series, same order as y." },
                "x_type": { "type": "string", "enum": ["auto", "time", "number", "category"], "description": "How to read x. Default auto: date strings are time, numbers are numbers. Set time for epoch timestamps (seconds or milliseconds)." },
                "bins": { "type": "integer", "minimum": 5, "maximum": 100, "description": "Histogram bin count; default scales with the number of values." },
                "unit": { "type": "string", "description": "Unit shown on the value axis, e.g. \"USD\" or \"%\"." }
            }
        }),
    }
}

/// A `/api/backtest/run` body with `dry_run: true` (the body may arrive as a JSON string).
fn is_dry_run(body: Option<&Value>) -> bool {
    let parsed;
    let body = match body {
        Some(Value::String(s)) => {
            parsed = serde_json::from_str::<Value>(s).ok();
            parsed.as_ref()
        }
        b => b,
    };
    body.and_then(|b| b.get("dry_run")).and_then(Value::as_bool) == Some(true)
}

/// `validate_only: true` on an otw_write call: checked, never dispatched.
fn is_validate_only(args: &Value) -> bool {
    args.get("validate_only").and_then(Value::as_bool) == Some(true)
}

#[cfg(test)]
mod consent_tests {
    use super::*;

    fn ctx(auto: bool) -> ToolContext {
        ToolContext { auto_approve_writes: auto, ..ToolContext::none() }
    }

    fn write(method: &str, path: &str) -> Value {
        json!({ "method": method, "path": path })
    }

    /// A plain write asks, unless the user opted out of being asked.
    #[test]
    fn writes_confirm_by_default() {
        let intent = ctx(false).confirm_required("otw_write", &write("POST", "/api/journal/trades"));
        let intent = intent.expect("POST must confirm");
        assert_eq!(intent.method, "POST");
        assert!(!intent.destructive);
        assert!(ctx(true).confirm_required("otw_write", &write("PUT", "/api/journal/trades")).is_none());
    }

    /// The rule the flag must never override: auto-approve was a decision about routine
    /// writes, not permission to erase the user's data.
    #[test]
    fn deletes_confirm_even_with_auto_approve_on() {
        for c in [ctx(false), ctx(true)] {
            let intent = c
                .confirm_required("otw_write", &write("delete", "/api/journal/trades/1"))
                .expect("DELETE must always confirm");
            assert!(intent.destructive);
            assert_eq!(intent.method, "DELETE", "method is normalized for display");
        }
    }

    /// Reads and the non-destructive built-in tools never stop to ask.
    #[test]
    fn reads_and_builtins_never_confirm() {
        let c = ctx(false);
        assert!(c.confirm_required("otw_read", &json!({ "path": "/api/journal/trades" })).is_none());
        assert!(c.confirm_required("memory_write", &json!({ "slug": "x" })).is_none());
        assert!(c.confirm_required("load_skill", &json!({ "name": "backtest-run" })).is_none());
    }

    /// Deleting a memory destroys data, so it confirms like any delete — even with
    /// auto-approve on, and the pseudo-path carries the slug for the confirm frame.
    #[test]
    fn memory_delete_always_confirms() {
        for c in [ctx(false), ctx(true)] {
            let intent = c
                .confirm_required("memory_delete", &json!({ "slug": "preferred-currency" }))
                .expect("memory_delete must confirm");
            assert!(intent.destructive);
            assert_eq!(intent.method, "DELETE");
            assert_eq!(intent.path, "memory:preferred-currency");
        }
        // Malformed (no slug) is not worth asking about — it fails in `call` with a hint.
        assert!(ctx(false).confirm_required("memory_delete", &json!({})).is_none());
    }

    /// Compute POSTs must not prompt: a backtest changes nothing the user would miss, and a
    /// confirm dialog on every simulation is how people learn to click Approve without
    /// reading it.
    #[test]
    fn dry_runs_are_free_and_validate_only_never_confirms() {
        assert!(is_dry_run(Some(&json!({ "dry_run": true, "settings": {} }))));
        assert!(is_dry_run(Some(&json!("{\"dry_run\": true}"))));
        assert!(!is_dry_run(Some(&json!({ "dry_run": false }))));
        assert!(!is_dry_run(Some(&json!({ "dry_run": "true" }))));
        assert!(!is_dry_run(None));
        let c = ctx(false);
        let mut del = write("DELETE", "/api/journal/trades/1");
        del["validate_only"] = json!(true);
        assert!(c.confirm_required("otw_write", &del).is_none(), "nothing runs, nothing to approve");
        let mut post = write("POST", "/api/journal/trades");
        post["validate_only"] = json!(false);
        assert!(c.confirm_required("otw_write", &post).is_some());
    }

    #[test]
    fn compute_posts_do_not_confirm() {
        let c = ctx(false);
        for path in [
            "/api/backtest/run",
            "/api/backtest/sweep",
            "/api/quant/single",
            "/api/backtest/runs/8b7b1f9e-0000-0000-0000-000000000000/montecarlo",
            "/api/backtest/run?x=1",
        ] {
            assert!(
                c.confirm_required("otw_write", &write("POST", path)).is_none(),
                "{path} should not need confirmation"
            );
        }
        // …while a POST that stores something still does.
        assert!(c.confirm_required("otw_write", &write("POST", "/api/backtest/strategies")).is_some());
        // A DELETE on a compute module is still a delete.
        assert!(c
            .confirm_required("otw_write", &write("DELETE", "/api/backtest/runs"))
            .is_some_and(|i| i.destructive));
    }

    /// A malformed call fails in `call` with a message that teaches the fix — asking the user
    /// to approve a call that cannot run is noise.
    #[test]
    fn malformed_writes_are_not_worth_asking_about() {
        let c = ctx(false);
        assert!(c.confirm_required("otw_write", &json!({ "path": "/api/journal/trades" })).is_none());
        assert!(c.confirm_required("otw_write", &json!({ "method": "POST" })).is_none());
    }
}
