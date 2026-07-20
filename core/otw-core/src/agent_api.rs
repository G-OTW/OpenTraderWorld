//! HTTP API for the Agent module — providers, the default agent, conversations/messages, and
//! the streaming run endpoint. Session-authed (mounted behind the auth middleware).
//!
//! Provider API keys are write-only: accepted on POST/PUT, never returned by GET.

use std::time::Duration;

use axum::{
    extract::{Path, State},
    response::sse::{KeepAlive, Sse},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::{self, provider::{Block, ChatRequest, Msg, Role}, run::RunConfig, tools::ToolContext};
use crate::{ApiError, AppState};
use otw_store::agent as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/agent/providers", get(list_providers).post(add_provider))
        .route(
            "/api/agent/providers/{id}",
            axum::routing::put(update_provider).delete(delete_provider),
        )
        .route("/api/agent/providers/{id}/models", get(provider_models))
        .route("/api/agent/mcp-servers", get(list_mcp_servers).post(add_mcp_server))
        .route(
            "/api/agent/mcp-servers/{id}",
            axum::routing::put(update_mcp_server).delete(delete_mcp_server),
        )
        .route("/api/agent/mcp-servers/{id}/test", post(test_mcp_server))
        .route("/api/agent/agent", get(get_agent).put(update_agent))
        .route("/api/agent/conversations", get(list_conversations).post(create_conversation))
        .route(
            "/api/agent/conversations/{id}",
            get(get_conversation).patch(update_conversation).delete(delete_conversation),
        )
        .route("/api/agent/conversations/{id}/run", post(run))
        .route("/api/agent/conversations/{id}/export", get(export_conversation))
        .route("/api/agent/memories", get(list_memories).post(upsert_memory))
        .route("/api/agent/memories/{slug}", axum::routing::delete(delete_memory))
        .route("/api/agent/skills", get(list_skills).post(add_skill))
        .route("/api/agent/skills/{id}", axum::routing::put(update_skill).delete(delete_skill))
}

// ── Providers ─────────────────────────────────────────────────────────────────

async fn list_providers(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut providers = store::list_providers(&state.pool).await?;
    // Demo sandbox: the seeded row has an empty `api_key`, so the store's row-derived
    // `has_key` is false — but the run path resolves the host's shared key, so the agent
    // really is usable. Report that, or the UI blocks the composer on a key the visitor
    // can never supply. Still only a boolean: the key itself never leaves the server.
    if crate::demo::shared_llm_key().is_some() {
        for p in &mut providers {
            p.has_key = true;
        }
    }
    Ok(Json(json!({ "providers": providers })))
}

fn validate_provider(input: &store::ProviderInput) -> Result<(), ApiError> {
    if !matches!(input.kind.as_str(), "anthropic" | "openai_compat") {
        return Err(ApiError::bad_request("kind must be 'anthropic' or 'openai_compat'"));
    }
    if input.label.trim().is_empty() {
        return Err(ApiError::bad_request("provider label required"));
    }
    if input.kind == "openai_compat" && input.base_url.trim().is_empty() {
        return Err(ApiError::bad_request("openai_compat providers need a base_url"));
    }
    Ok(())
}

async fn add_provider(
    State(state): State<AppState>,
    Json(input): Json<store::ProviderInput>,
) -> Result<Json<Value>, ApiError> {
    validate_provider(&input)?;
    let provider = store::add_provider(&state.pool, &state.cipher, &input).await?;
    Ok(Json(json!({ "provider": provider })))
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<store::ProviderInput>,
) -> Result<Json<Value>, ApiError> {
    validate_provider(&input)?;
    let provider = store::update_provider(&state.pool, &state.cipher, id, &input)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    Ok(Json(json!({ "provider": provider })))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_provider(&state.pool, id).await? {
        return Err(ApiError::not_found("provider not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Live model list from the provider (for the in-chat picker). Queried server-side so the
/// API key never reaches the browser. Best-effort: 400 with the provider's message when the
/// upstream doesn't support listing.
async fn provider_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let prow = store::get_provider(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let stored_key = store::provider_key(&state.pool, &state.cipher, id)
        .await?
        .filter(|k| !k.is_empty());
    let key = match stored_key {
        Some(k) => k,
        // Demo sandbox: same env fallback as `run` — the seeded provider has no key.
        None if crate::demo::enabled() => crate::demo::shared_llm_key()
            .ok_or_else(|| ApiError::bad_request("demo AI is not configured on this host"))?,
        None => return Err(ApiError::bad_request("provider has no API key set")),
    };
    let provider = agent::build_provider(&prow.kind, &prow.base_url, key)
        .ok_or_else(|| ApiError::bad_request("unknown provider kind"))?;
    let mut models = provider
        .models()
        .await
        .map_err(|e| ApiError::bad_request(&format!("could not list models: {e}")))?;
    models.sort();
    models.dedup();
    // Demo sandbox: the picker only offers what the run endpoint will accept.
    if crate::demo::enabled() {
        models.retain(|m| m.ends_with(":free"));
    }
    Ok(Json(json!({ "models": models })))
}

// ── External MCP servers ──────────────────────────────────────────────────────

async fn list_mcp_servers(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let servers = store::list_mcp_servers(&state.pool).await?;
    Ok(Json(json!({ "servers": servers })))
}

fn validate_mcp_server(input: &store::McpServerInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("server name required"));
    }
    let url = input.url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ApiError::bad_request("server URL must start with http(s)://"));
    }
    Ok(())
}

async fn add_mcp_server(
    State(state): State<AppState>,
    Json(mut input): Json<store::McpServerInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    input.url = input.url.trim().to_string();
    validate_mcp_server(&input)?;
    let server = store::add_mcp_server(&state.pool, &state.cipher, &input)
        .await
        .map_err(dup_server_or(&input.name))?;
    Ok(Json(json!({ "server": server })))
}

async fn update_mcp_server(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<store::McpServerInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    input.url = input.url.trim().to_string();
    validate_mcp_server(&input)?;
    let server = store::update_mcp_server(&state.pool, &state.cipher, id, &input)
        .await
        .map_err(dup_server_or(&input.name))?
        .ok_or_else(|| ApiError::not_found("server not found"))?;
    Ok(Json(json!({ "server": server })))
}

async fn delete_mcp_server(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_mcp_server(&state.pool, id).await? {
        return Err(ApiError::not_found("server not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Connect + list tools, as a validation probe for the UI's Test button.
async fn test_mcp_server(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let server = store::get_mcp_server(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("server not found"))?;
    let (header, value) = store::mcp_server_auth(&state.pool, &state.cipher, id)
        .await?
        .unwrap_or_default();
    let probe = async {
        let client = agent::mcp_client::McpClient::connect(&server.url, &header, &value).await?;
        client.list_tools().await
    };
    let tools = tokio::time::timeout(Duration::from_secs(20), probe)
        .await
        .map_err(|_| ApiError::bad_request("server did not answer within 20s"))?
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let names: Vec<&str> = tools.iter().map(|(n, ..)| n.as_str()).collect();
    Ok(Json(json!({ "ok": true, "tools": names })))
}

/// Map a unique-violation on the server name to a friendly 400, else pass through.
fn dup_server_or(name: &str) -> impl Fn(anyhow::Error) -> ApiError + '_ {
    move |e| {
        if e.to_string().contains("agent_mcp_servers_name_key")
            || e.chain().any(|c| c.to_string().contains("duplicate key"))
        {
            ApiError::bad_request(&format!("a server named \"{name}\" already exists"))
        } else {
            e.into()
        }
    }
}

// ── Agent (single default) ────────────────────────────────────────────────────

async fn get_agent(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let agent = store::default_agent(&state.pool).await?;
    Ok(Json(json!({ "agent": agent })))
}

async fn update_agent(
    State(state): State<AppState>,
    Json(upd): Json<store::AgentUpdate>,
) -> Result<Json<Value>, ApiError> {
    // A set (non-null) provider_id must reference an existing provider — fail as a clear
    // 400 instead of bubbling the FK violation up as a 500.
    if let Some(Some(pid)) = upd.provider_id {
        if store::get_provider(&state.pool, pid).await?.is_none() {
            return Err(ApiError::bad_request("provider not found"));
        }
    }
    let agent = store::default_agent(&state.pool).await?;
    let updated = store::update_agent(&state.pool, agent.id, &upd)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    Ok(Json(json!({ "agent": updated })))
}

// ── Conversations ─────────────────────────────────────────────────────────────

async fn list_conversations(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conversations = store::list_conversations(&state.pool).await?;
    Ok(Json(json!({ "conversations": conversations })))
}

async fn create_conversation(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let agent = store::default_agent(&state.pool).await?;
    // New conversations start with the agent's default token; switchable per conversation.
    let conversation =
        store::create_conversation(&state.pool, agent.id, agent.mcp_token_id).await?;
    Ok(Json(json!({ "conversation": conversation })))
}

async fn get_conversation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let conversation = store::get_conversation(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    let messages = store::list_messages(&state.pool, id).await?;
    let (input_tokens, output_tokens) = store::conversation_tokens(&state.pool, id).await?;
    Ok(Json(json!({
        "conversation": conversation,
        "messages": messages,
        "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens },
    })))
}

#[derive(Deserialize)]
struct ConversationPatch {
    /// New title (absent = unchanged).
    title: Option<String>,
    /// Tools envelope: absent = unchanged, null = chat only, id = switch to that token.
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    mcp_token_id: Option<Option<Uuid>>,
    /// External MCP servers: absent = unchanged, [] = none, ids = replace the selection.
    mcp_servers: Option<Vec<Uuid>>,
}

async fn update_conversation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ConversationPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(title) = &body.title {
        if !store::rename_conversation(&state.pool, id, title.trim()).await? {
            return Err(ApiError::not_found("conversation not found"));
        }
    }
    if let Some(token) = body.mcp_token_id {
        // A set (non-null) token must exist — clear 400 instead of an FK 500.
        if let Some(tid) = token {
            if otw_store::mcp::get_token(&state.pool, tid).await?.is_none() {
                return Err(ApiError::bad_request("MCP token not found"));
            }
        }
        if !store::set_conversation_token(&state.pool, id, token).await? {
            return Err(ApiError::not_found("conversation not found"));
        }
    }
    if let Some(servers) = &body.mcp_servers {
        for sid in servers {
            if store::get_mcp_server(&state.pool, *sid).await?.is_none() {
                return Err(ApiError::bad_request("MCP server not found"));
            }
        }
        if !store::set_conversation_servers(&state.pool, id, servers).await? {
            return Err(ApiError::not_found("conversation not found"));
        }
    }
    let conversation = store::get_conversation(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    Ok(Json(json!({ "ok": true, "conversation": conversation })))
}

async fn delete_conversation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_conversation(&state.pool, id).await? {
        return Err(ApiError::not_found("conversation not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Run (SSE) ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RunBody {
    /// The user's new message text.
    message: String,
}

// Demo sandbox: keep the shared free-tier OpenRouter key alive for everyone. The free
// tier is ~50 requests/day per key and one run can spend several (tool loops), so both a
// burst cap and a daily budget apply. Each is enforced per-IP *and* globally: the global
// half caps total spend on the shared account, the per-IP half stops one visitor from
// eating that budget and locking every other visitor out.
static DEMO_RUN_BURST: crate::demo::WindowQuota =
    crate::demo::WindowQuota::new(8, 3, std::time::Duration::from_secs(600));
static DEMO_RUN_DAILY: crate::demo::WindowQuota =
    crate::demo::WindowQuota::new(40, 10, std::time::Duration::from_secs(24 * 3600));

/// Append the user message, then stream one provider turn over SSE.
async fn run(
    State(state): State<AppState>,
    Path(conversation_id): Path<Uuid>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(body): Json<RunBody>,
) -> Result<impl IntoResponse, ApiError> {
    let text = body.message.trim();
    if text.is_empty() {
        return Err(ApiError::bad_request("message is empty"));
    }
    if crate::demo::enabled() {
        if text.len() > 2000 {
            return Err(ApiError::bad_request("demo: messages are capped at 2000 characters"));
        }
        // Requests that bypassed the gate (in-process MCP dispatch) share one bucket.
        let ip = ip.map(|e| e.0 .0).unwrap_or_else(|| "internal".to_string());
        // Both must be charged, so evaluate eagerly — `||` would skip the daily counter
        // whenever the burst window is already exhausted.
        let burst_ok = DEMO_RUN_BURST.allow(&ip);
        let daily_ok = DEMO_RUN_DAILY.allow(&ip);
        if !burst_ok || !daily_ok {
            return Err(ApiError::too_many(
                "the shared demo AI budget is used up for now — try again later",
            ));
        }
    }

    let conv = store::get_conversation(&state.pool, conversation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    // One run at a time per conversation: claim it before any message is persisted, so a
    // double-send (Stop + quick resend while the old task still drains) can't interleave
    // history. Held through setup and the streaming task; released when the run ends.
    let guard = crate::agent::run::RunGuard::acquire(conversation_id).ok_or_else(|| {
        ApiError::conflict("a reply is already streaming for this conversation — stop it or wait")
    })?;
    let agent = store::get_agent(&state.pool, conv.agent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;

    // Resolve the provider (agent override → provider default model).
    let provider_id = agent
        .provider_id
        .ok_or_else(|| ApiError::bad_request("no provider configured — add one in agent settings"))?;
    let prow = store::get_provider(&state.pool, provider_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("configured provider was deleted"))?;
    if !prow.enabled {
        return Err(ApiError::bad_request("the configured provider is disabled"));
    }
    let model = if agent.model.trim().is_empty() { prow.default_model.clone() } else { agent.model.clone() };
    if model.trim().is_empty() {
        return Err(ApiError::bad_request("no model set — choose one in agent settings"));
    }
    // Demo sandbox: only OpenRouter `:free` models — the shared key holds zero credits,
    // so a paid model id would just 402; refuse it up front with a clear message.
    if crate::demo::enabled() && !model.trim().ends_with(":free") {
        return Err(ApiError::bad_request("demo: only OpenRouter ':free' models are available"));
    }
    let stored_key = store::provider_key(&state.pool, &state.cipher, provider_id)
        .await?
        .filter(|k| !k.is_empty());
    let key = match stored_key {
        Some(k) => k,
        // Demo sandbox: the seeded provider row carries no key (the seed is a public
        // artifact); the shared free-tier key comes from the host environment.
        None if crate::demo::enabled() => crate::demo::shared_llm_key()
            .ok_or_else(|| ApiError::bad_request("demo AI is not configured on this host"))?,
        None => return Err(ApiError::bad_request("provider has no API key set")),
    };
    let dyn_provider = agent::build_provider(&prow.kind, &prow.base_url, key)
        .ok_or_else(|| ApiError::bad_request("unknown provider kind"))?;

    // Persist the user message, auto-title from it if the conversation is untitled.
    let user_content = json!([{ "type": "text", "text": text }]);
    store::add_message(&state.pool, conversation_id, "user", &user_content, 0, 0).await?;
    if conv.title.is_empty() {
        store::set_title_if_empty(&state.pool, conversation_id, &auto_title(text)).await?;
    }

    // Resolve the tool context from THIS CONVERSATION's MCP token — per-conversation,
    // prefilled from the agent default at creation. The token's per-module permissions
    // (set in Settings → AI agents) apply directly; no token → pure chat.
    let mut tools = match conv.mcp_token_id {
        Some(token_id) => match otw_store::mcp::get_token(&state.pool, token_id).await? {
            Some(tok) => ToolContext {
                permissions: tok.permissions,
                token_name: tok.name,
                external: Vec::new(),
            },
            None => ToolContext::none(), // token was deleted; degrade to chat
        },
        None => ToolContext::none(),
    };
    // Connect this conversation's external MCP servers (parallel, best-effort): a server
    // that fails to answer yields a warning frame instead of blocking the run. Demo
    // sandbox: external MCP is an SSRF primitive — only the built-in gateway runs.
    let (external, warnings) = if crate::demo::enabled() {
        (Vec::new(), Vec::new())
    } else {
        connect_external(&state, &conv.server_ids()).await
    };
    tools.external = external;

    // Roll up older turns into a summary when the history grows past the threshold, so long
    // conversations stay cheap (short-term memory). Best-effort: a failure just means we send
    // more history this run.
    if let Err(e) = agent::summary::maybe_summarize(
        &state,
        conversation_id,
        &conv,
        &prow,
        &model,
        &agent.params,
    )
    .await
    {
        tracing::warn!("agent: rolling summary failed: {e:#}");
    }
    // Re-read the conversation to pick up a fresh summary, if one was just written.
    let conv = store::get_conversation(&state.pool, conversation_id)
        .await?
        .unwrap_or(conv);

    // Assemble the prompt: base system prompt + rolling summary + memory index + skill catalog,
    // then the messages the summary does not cover (capped at HISTORY_WINDOW).
    let system = build_system_prompt(&state, &agent.system_prompt, &conv.summary).await?;
    let stored = store::list_messages(&state.pool, conversation_id).await?;
    let covers = (conv.summary_covers.max(0) as usize).min(stored.len());
    let messages = build_messages(&stored[covers..]);

    let req = ChatRequest {
        model,
        system,
        messages,
        tools: tools.tool_defs(),
        params: agent.params.clone(),
    };

    let stream = agent::run::run(RunConfig {
        state: state.clone(),
        conversation_id,
        provider: dyn_provider,
        req,
        tools,
        warnings,
        guard,
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// Per-server connect+list budget (runs before the model turn starts streaming).
const EXT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Connect the conversation's external MCP servers in parallel. Disabled/deleted ids are
/// skipped silently; reachable servers come back ready, failures become user-visible
/// warning strings. Auth values are unsealed here and never leave the process.
async fn connect_external(
    state: &AppState,
    ids: &[Uuid],
) -> (Vec<agent::tools::ExternalServer>, Vec<String>) {
    let mut ready = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_slugs: Vec<String> = Vec::new();

    let futures = ids.iter().map(|id| {
        let state = state.clone();
        let id = *id;
        async move {
            let server = match store::get_mcp_server(&state.pool, id).await {
                Ok(Some(s)) if s.enabled => s,
                _ => return None, // deleted or disabled: skip silently
            };
            let (header, value) =
                match store::mcp_server_auth(&state.pool, &state.cipher, id).await {
                    Ok(Some(a)) => a,
                    Ok(None) => return None,
                    Err(e) => return Some(Err((server.name, format!("{e:#}")))),
                };
            let connect = async {
                let client =
                    agent::mcp_client::McpClient::connect(&server.url, &header, &value).await?;
                let tools = client.list_tools().await?;
                Ok::<_, anyhow::Error>((client, tools))
            };
            match tokio::time::timeout(EXT_CONNECT_TIMEOUT, connect).await {
                Ok(Ok((client, tools))) => Some(Ok((server.name, client, tools))),
                Ok(Err(e)) => Some(Err((server.name, format!("{e:#}")))),
                Err(_) => Some(Err((
                    server.name,
                    format!("no answer within {}s", EXT_CONNECT_TIMEOUT.as_secs()),
                ))),
            }
        }
    });

    for outcome in futures::future::join_all(futures).await.into_iter().flatten() {
        match outcome {
            Ok((name, client, tools)) => {
                // Slugs namespace tool names — keep them unique across servers.
                let mut slug = agent::truncate_plain(&agent::slugify(&name), 24);
                if slug.is_empty() {
                    slug = "srv".into();
                }
                while seen_slugs.contains(&slug) {
                    slug.push('x');
                }
                seen_slugs.push(slug.clone());
                ready.push(agent::tools::ExternalServer::assemble(&name, slug, client, tools));
            }
            Err((name, msg)) => {
                warnings.push(format!("MCP server \"{name}\" unavailable: {msg}"));
            }
        }
    }
    (ready, warnings)
}

/// Compose the effective system prompt: the agent's base prompt, then (when present) the
/// rolling conversation summary, the long-term memory index (slugs + descriptions), and the
/// enabled-skill catalog (names + descriptions). Bodies are pulled on demand via the
/// memory_read / load_skill tools — only the index is injected, keeping token cost bounded.
async fn build_system_prompt(
    state: &AppState,
    base: &str,
    summary: &str,
) -> Result<String, ApiError> {
    let mut out = base.trim().to_string();

    if !summary.trim().is_empty() {
        out.push_str("\n\n## Summary of earlier conversation\n");
        out.push_str(summary.trim());
    }

    // Demo mode ships neither section: the tools that act on them are withheld (see
    // `tools::builtin_defs`), so listing the index would only invite calls that fail.
    let memories = if crate::demo::enabled() { Vec::new() } else { store::memory_index(&state.pool).await? };
    if !memories.is_empty() {
        out.push_str(
            "\n\n## Long-term memory\nFacts you've saved. Use memory_read(slug) for full content; \
             memory_write to add or update one.\n",
        );
        for (slug, desc) in memories {
            if desc.trim().is_empty() {
                out.push_str(&format!("- {slug}\n"));
            } else {
                out.push_str(&format!("- {slug}: {desc}\n"));
            }
        }
    }

    let skills = if crate::demo::enabled() { Vec::new() } else { store::skill_catalog(&state.pool).await? };
    if !skills.is_empty() {
        out.push_str(
            "\n\n## Skills\nSpecialised instructions available on demand. Call load_skill(name) \
             before doing a task a skill covers.\n",
        );
        for (name, desc) in skills {
            if desc.trim().is_empty() {
                out.push_str(&format!("- {name}\n"));
            } else {
                out.push_str(&format!("- {name}: {desc}\n"));
            }
        }
    }

    Ok(out)
}

/// Hard cap on messages sent verbatim. The summarizer normally keeps the unsummarized span
/// well below this; the cap only bites when summarization keeps failing.
const HISTORY_WINDOW: usize = 40;

/// Turn stored messages into the normalized provider model, keeping only the last
/// `HISTORY_WINDOW`. Tool-result rows (stored with role "tool") are delivered on a user-role
/// turn — both wire formats expect tool_result blocks in a user message.
///
/// The window must start on a plain user message: starting on a "tool" row would send a
/// tool_result whose tool_use fell outside the window (both providers 400 on that), and
/// Anthropic wants the first message to be user-role.
fn build_messages(stored: &[store::Message]) -> Vec<Msg> {
    let mut start = stored.len().saturating_sub(HISTORY_WINDOW);
    while start < stored.len() && stored[start].role != "user" {
        start += 1;
    }
    if start >= stored.len() {
        // Degenerate (no user row in the window — shouldn't happen: every run appends one).
        // Send nothing rather than a window starting on a tool row, which providers 400 on.
        return Vec::new();
    }
    stored[start..]
        .iter()
        .filter_map(|m| {
            let role = match m.role.as_str() {
                "assistant" => Role::Assistant,
                "user" | "tool" => Role::User,
                _ => return None,
            };
            let blocks: Vec<Block> = serde_json::from_value(m.content.clone()).unwrap_or_default();
            if blocks.is_empty() {
                return None;
            }
            Some(Msg { role, blocks })
        })
        .collect()
}

/// First line of the user's message, trimmed to a short title.
fn auto_title(text: &str) -> String {
    let first = text.lines().next().unwrap_or(text).trim();
    crate::agent::truncate(first, 60)
}

// ── Export ────────────────────────────────────────────────────────────────────

async fn export_conversation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let conv = store::get_conversation(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    let messages = store::list_messages(&state.pool, id).await?;

    let title = if conv.title.is_empty() { "Conversation" } else { &conv.title };
    let mut md = format!("# {title}\n\n");
    for m in &messages {
        let who = match m.role.as_str() {
            "user" => "You",
            "assistant" => "Assistant",
            other => other,
        };
        let blocks: Vec<Block> = serde_json::from_value(m.content.clone()).unwrap_or_default();
        let text: String = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            continue;
        }
        md.push_str(&format!("**{who}** — {}\n\n{}\n\n", store::rfc3339_string(&m.created_at), text));
    }

    let filename = format!("conversation-{}.md", &id.to_string()[..8]);
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/markdown; charset=utf-8".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        md,
    ))
}

// ── Memories ──────────────────────────────────────────────────────────────────

async fn list_memories(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let memories = store::list_memories(&state.pool).await?;
    Ok(Json(json!({ "memories": memories, "max": store::MEMORY_MAX_COUNT })))
}

#[derive(Deserialize)]
struct MemoryInput {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    kind: String,
}

/// Upsert a memory by slug (create or replace). Enforces the count + size caps — slug and
/// description are capped too, since the whole index rides in every system prompt.
async fn upsert_memory(
    State(state): State<AppState>,
    Json(input): Json<MemoryInput>,
) -> Result<Json<Value>, ApiError> {
    let slug = agent::slugify(&input.slug);
    if slug.is_empty() {
        return Err(ApiError::bad_request("a slug (letters/digits/hyphens) is required"));
    }
    if slug.chars().count() > store::MEMORY_MAX_SLUG {
        return Err(ApiError::bad_request(&format!(
            "slug too long (max {} characters)",
            store::MEMORY_MAX_SLUG
        )));
    }
    if input.description.trim().chars().count() > store::MEMORY_MAX_DESC {
        return Err(ApiError::bad_request(&format!(
            "description too long (max {} characters)",
            store::MEMORY_MAX_DESC
        )));
    }
    if input.content.chars().count() > store::MEMORY_MAX_LEN {
        return Err(ApiError::bad_request(&format!(
            "memory too long (max {} characters)",
            store::MEMORY_MAX_LEN
        )));
    }
    // Count cap applies only to NEW slugs (updates don't grow the store).
    if store::get_memory(&state.pool, &slug).await?.is_none()
        && store::count_memories(&state.pool).await? >= store::MEMORY_MAX_COUNT
    {
        return Err(ApiError::bad_request(&format!(
            "memory limit reached ({}); delete some first",
            store::MEMORY_MAX_COUNT
        )));
    }
    // Provenance: UI writes are "manual"; the agent's memory_write tool stamps "agent".
    let kind = if input.kind.trim().is_empty() { "manual" } else { input.kind.trim() };
    let memory =
        store::upsert_memory(&state.pool, &slug, input.description.trim(), &input.content, kind).await?;
    Ok(Json(json!({ "memory": memory })))
}

async fn delete_memory(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_memory(&state.pool, &slug).await? {
        return Err(ApiError::not_found("memory not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Skills ────────────────────────────────────────────────────────────────────

async fn list_skills(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let skills = store::list_skills(&state.pool).await?;
    Ok(Json(json!({ "skills": skills })))
}

fn validate_skill(input: &store::SkillInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("skill name required"));
    }
    Ok(())
}

async fn add_skill(
    State(state): State<AppState>,
    Json(mut input): Json<store::SkillInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate_skill(&input)?;
    let skill = store::add_skill(&state.pool, &input).await.map_err(dup_or(&input.name))?;
    Ok(Json(json!({ "skill": skill })))
}

async fn update_skill(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<store::SkillInput>,
) -> Result<Json<Value>, ApiError> {
    input.name = input.name.trim().to_string();
    validate_skill(&input)?;
    let skill = store::update_skill(&state.pool, id, &input)
        .await
        .map_err(dup_or(&input.name))?
        .ok_or_else(|| ApiError::not_found("skill not found"))?;
    Ok(Json(json!({ "skill": skill })))
}

async fn delete_skill(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_skill(&state.pool, id).await? {
        return Err(ApiError::not_found("skill not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Map a unique-violation on the skill name to a friendly 400, else pass through.
fn dup_or(name: &str) -> impl Fn(anyhow::Error) -> ApiError + '_ {
    move |e| {
        if e.to_string().contains("agent_skills_name_key") || e.chain().any(|c| c.to_string().contains("duplicate key")) {
            ApiError::bad_request(&format!("a skill named \"{name}\" already exists"))
        } else {
            e.into()
        }
    }
}
