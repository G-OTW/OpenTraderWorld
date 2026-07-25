//! HTTP API for the Agent module — providers, the default agent, conversations/messages, and
//! the streaming run endpoint. Session-authed (mounted behind the auth middleware).
//!
//! Provider API keys are write-only: accepted on POST/PUT, never returned by GET.

use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
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
        .route("/api/agent/agents", get(list_agents).post(create_agent))
        .route(
            "/api/agent/agents/{id}",
            axum::routing::put(update_one_agent).delete(delete_one_agent),
        )
        .route("/api/agent/agents/{id}/reset", post(reset_agent))
        .route("/api/agent/agents/{id}/clone", post(clone_agent))
        .route("/api/agent/shelf/export", get(export_shelf))
        .route("/api/agent/shelf/import", post(import_shelf))
        .route("/api/agent/conversations", get(list_conversations).post(create_conversation))
        .route(
            "/api/agent/conversations/{id}",
            get(get_conversation).patch(update_conversation).delete(delete_conversation),
        )
        .route("/api/agent/conversations/{id}/run", post(run))
        .route("/api/agent/conversations/{id}/confirm", post(confirm_write))
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

/// Every agent: the default chat agent plus the shipped personas and anything the user made.
/// Each row carries the skill shelf it will actually get this run (its allowlist intersected
/// with the enabled catalog), so the UI shows the real thing rather than the declared wish.
async fn list_agents(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let agents = store::list_agents(&state.pool).await?;
    let mut out = Vec::with_capacity(agents.len());
    for agent in agents {
        let shelf = store::skill_catalog(&state.pool, &agent.shelf()).await?;
        let names: Vec<&str> = shelf.iter().map(|(n, _)| n.as_str()).collect();
        out.push(json!({ "agent": agent, "effective_skills": names }));
    }
    Ok(Json(json!({ "agents": out })))
}

/// Body of a persona create/update. `skills` is the shelf as stored: `["*"]` for the whole
/// catalog, otherwise skill ids. Provider/model/params and the token are patched through the
/// same shape as the default agent, so one editor drives both.
#[derive(Deserialize, Default)]
struct AgentBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    skills: Option<Value>,
    #[serde(default)]
    auto_approve_writes: Option<bool>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    provider_id: Option<Option<Uuid>>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    params: Option<Value>,
}

/// Validate the shelf value and the ids in it. An id that names no skill is rejected rather
/// than dropped: silently shipping a shorter shelf than the user picked is how a persona ends
/// up quietly less capable than its editor says.
async fn check_shelf(state: &AppState, skills: &Value) -> Result<(), ApiError> {
    let Some(items) = skills.as_array() else {
        return Err(ApiError::bad_request("skills must be an array"));
    };
    for v in items {
        let Some(s) = v.as_str() else {
            return Err(ApiError::bad_request("skills must be strings"));
        };
        if s == otw_store::agent::SHELF_ALL {
            continue;
        }
        let id: Uuid = s
            .parse()
            .map_err(|_| ApiError::bad_request(&format!("\"{s}\" is not a skill id")))?;
        if store::get_skill(&state.pool, id).await?.is_none() {
            return Err(ApiError::bad_request("that skill no longer exists — reload the list"));
        }
    }
    Ok(())
}

fn check_agent_name(name: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("persona name required"));
    }
    if name.chars().count() > 60 {
        return Err(ApiError::bad_request("persona name too long (max 60 characters)"));
    }
    Ok(())
}

/// Reject a name another persona already uses (case-insensitively, trimmed). The name is the
/// identity in a shelf bundle — an import matches on it — and it is all the conversation
/// header shows, so two "Quant" rows are two things the user cannot tell apart. Cloning
/// already avoids collisions via [`unique_agent_name`]; this closes the manual path.
async fn check_agent_name_free(
    state: &AppState,
    name: &str,
    except: Option<Uuid>,
) -> Result<(), ApiError> {
    let wanted = name.trim().to_lowercase();
    let taken = store::list_agents(&state.pool)
        .await?
        .into_iter()
        .any(|a| Some(a.id) != except && a.name.trim().to_lowercase() == wanted);
    if taken {
        return Err(ApiError::bad_request(&format!(
            "a persona named \"{}\" already exists — pick another name",
            name.trim()
        )));
    }
    Ok(())
}

/// Create a persona. User-owned: never builtin, never the default, no slug.
async fn create_agent(
    State(state): State<AppState>,
    Json(body): Json<AgentBody>,
) -> Result<Json<Value>, ApiError> {
    check_agent_name(&body.name)?;
    check_agent_name_free(&state, &body.name, None).await?;
    // A new persona with no shelf stated gets none. "Everything" is a choice the user makes
    // explicitly, because a persona IS its shelf — one that can load anything is the generic
    // assistant the module already ships.
    let skills = body.skills.clone().unwrap_or_else(|| json!([]));
    check_shelf(&state, &skills).await?;
    let agent =
        store::create_agent(&state.pool, body.name.trim(), body.system_prompt.trim(), &skills)
            .await?;
    // The rest goes through the shared patch path so create and update cannot drift.
    let agent = apply_agent_patch(&state, agent.id, &body).await?;
    Ok(Json(json!({ "agent": agent })))
}

/// Update any agent — a persona or the default. Builtin rows are editable (that is the
/// point); "reset to shipped" is how a user undoes it.
async fn update_one_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AgentBody>,
) -> Result<Json<Value>, ApiError> {
    check_agent_name(&body.name)?;
    if store::get_agent(&state.pool, id).await?.is_none() {
        return Err(ApiError::not_found("agent not found"));
    }
    check_agent_name_free(&state, &body.name, Some(id)).await?;
    if let Some(skills) = &body.skills {
        check_shelf(&state, skills).await?;
    }
    let agent = apply_agent_patch(&state, id, &body).await?;
    Ok(Json(json!({ "agent": agent })))
}

/// The one place a persona's fields are written, shared by create and update.
async fn apply_agent_patch(
    state: &AppState,
    id: Uuid,
    body: &AgentBody,
) -> Result<store::Agent, ApiError> {
    if let Some(Some(pid)) = body.provider_id {
        if store::get_provider(&state.pool, pid).await?.is_none() {
            return Err(ApiError::bad_request("provider not found"));
        }
    }
    let upd = store::AgentUpdate {
        name: Some(body.name.trim().to_string()),
        system_prompt: Some(body.system_prompt.trim().to_string()),
        provider_id: body.provider_id,
        model: body.model.clone(),
        params: body.params.clone(),
        mcp_token_id: None,
        skills: body.skills.clone(),
        auto_approve_writes: body.auto_approve_writes,
    };
    store::update_agent(&state.pool, id, &upd)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))
}

/// Copy a persona into a new user-owned one. The shipped presets are a starting point, and
/// cloning is how a user keeps the original around while gutting the copy.
async fn clone_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let src = store::get_agent(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    let name = unique_agent_name(&state, &format!("{} copy", src.name)).await?;
    let agent = store::create_agent(&state.pool, &name, &src.system_prompt, &src.skills).await?;
    // Carry the model choice over — a clone that silently falls back to another model is not
    // the same persona. The token is deliberately not copied: access is the conversation's.
    let upd = store::AgentUpdate {
        provider_id: Some(src.provider_id),
        model: Some(src.model.clone()),
        params: Some(src.params.clone()),
        auto_approve_writes: Some(src.auto_approve_writes),
        ..Default::default()
    };
    let agent = store::update_agent(&state.pool, agent.id, &upd).await?.unwrap_or(agent);
    Ok(Json(json!({ "agent": agent })))
}

/// "X copy", "X copy 2", … — names are how the user tells two personas apart in the picker.
async fn unique_agent_name(state: &AppState, base: &str) -> Result<String, ApiError> {
    let taken: Vec<String> =
        store::list_agents(&state.pool).await?.into_iter().map(|a| a.name).collect();
    if !taken.iter().any(|n| n == base) {
        return Ok(base.to_string());
    }
    for n in 2..100 {
        let candidate = format!("{base} {n}");
        if !taken.iter().any(|t| *t == candidate) {
            return Ok(candidate);
        }
    }
    Ok(base.to_string())
}

/// Delete a persona, keeping every conversation it held.
///
/// The FK is RESTRICT (migration 0064), so the conversations move to the default agent first
/// and each transcript gets a marker saying so. Deleting a persona is deleting a stance, not
/// deleting the work done under it.
///
/// Builtins are refused: the seeder re-creates them on the next boot, so a "delete" that
/// appeared to work would quietly come back. Reset is the way back to shipped.
async fn delete_one_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let agent = store::get_agent(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    if agent.is_default {
        return Err(ApiError::bad_request(
            "the default assistant cannot be deleted — it is the fallback every conversation \
             lands on",
        ));
    }
    if agent.builtin {
        return Err(ApiError::bad_request(
            "built-in personas cannot be deleted (they would return on the next restart) — \
             reset it to shipped, or disable its skills",
        ));
    }
    let default = store::default_agent(&state.pool).await?;
    let moved = store::conversations_of_agent(&state.pool, id).await?;
    if !moved.is_empty() {
        store::reassign_conversations(&state.pool, id, default.id).await?;
        let content = json!([{
            "type": "text",
            "text": format!(
                "{} was deleted. This conversation continues with {}; everything above was \
                 produced by {}.",
                agent.name, default.name, agent.name
            ),
        }]);
        for conv in &moved {
            store::add_message(&state.pool, *conv, "system", &content, 0, 0).await?;
        }
    }
    if !store::delete_agent(&state.pool, id).await? {
        return Err(ApiError::not_found("agent not found"));
    }
    Ok(Json(json!({ "ok": true, "conversations_moved": moved.len() })))
}

/// Restore a shipped persona's name, prompt, and shelf, plus the bodies of the builtin skills
/// on that shelf. Configuration the user chose (provider, model, params, token) is untouched —
/// resetting content must not silently repoint an agent at a different model.
async fn reset_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let agent = store::get_agent(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("agent not found"))?;
    let persona = crate::agent::builtin::PERSONAS
        .iter()
        .find(|p| p.slug == agent.slug)
        .ok_or_else(|| ApiError::bad_request("not a built-in persona"))?;

    // The shelf is declared by name and stored by id — resolve it the same way the seeder
    // does, or the reset would write names into a column the run path reads as ids.
    let shelf = crate::agent::builtin::seed::shelf_ids(&state.pool, persona.skills).await?;
    store::reset_builtin_agent(&state.pool, agent.id, persona.name, &persona.prompt(), &json!(shelf))
        .await?;

    // Skills are shared, so only the ones on this persona's shelf are restored: resetting the
    // quant must not undo an edit the user made to a skill only the researcher uses.
    let mut restored = Vec::new();
    for skill in store::list_skills(&state.pool).await? {
        if !skill.builtin || !persona.skills.contains(&skill.name.as_str()) {
            continue;
        }
        if let Some(shipped) =
            crate::agent::builtin::SKILLS.iter().find(|s| s.name == skill.name)
        {
            store::reset_builtin_skill(&state.pool, skill.id, shipped.description, shipped.body)
                .await?;
            restored.push(skill.name);
        }
    }
    let agent = store::get_agent(&state.pool, id).await?;
    Ok(Json(json!({ "agent": agent, "skills_restored": restored })))
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
    let out = with_effective_model(&state, conversations).await?;
    Ok(Json(json!({ "conversations": out })))
}

/// Serialize conversations with the provider/model each one will actually run on.
///
/// The chat header needs the *effective* choice, not the override — a conversation with no
/// override of its own still has to show what it inherited. Resolving it here rather than in
/// the browser keeps one implementation of the rule ([`resolve_model`]); a second copy in the
/// UI would drift the moment the fallback order changed.
async fn with_effective_model(
    state: &AppState,
    conversations: Vec<store::Conversation>,
) -> Result<Vec<Value>, ApiError> {
    let agents = store::list_agents(&state.pool).await?;
    let default_agent = store::default_agent(&state.pool).await?;
    let mut out = Vec::with_capacity(conversations.len());
    for conv in conversations {
        let agent = agents.iter().find(|a| a.id == conv.agent_id).unwrap_or(&default_agent);
        let choice = resolve_model(&conv, agent, &default_agent);
        let mut v = serde_json::to_value(&conv).unwrap_or_else(|_| json!({}));
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "effective_provider_id".into(),
                choice.as_ref().map(|c| json!(c.provider_id)).unwrap_or(Value::Null),
            );
            o.insert(
                "effective_model".into(),
                choice.map(|c| json!(c.model)).unwrap_or(Value::Null),
            );
        }
        out.push(v);
    }
    Ok(out)
}

#[derive(Deserialize, Default)]
struct NewConversation {
    /// Persona to open the conversation with. Absent → the default agent, which is what a
    /// client that predates personas sends.
    #[serde(default)]
    agent_id: Option<Uuid>,
}

async fn create_conversation(
    State(state): State<AppState>,
    body: Option<Json<NewConversation>>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.unwrap_or_default();
    let agent = match body.agent_id {
        Some(id) => store::get_agent(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::bad_request("agent not found"))?,
        None => store::default_agent(&state.pool).await?,
    };
    // New conversations start with the agent's default token; switchable per conversation.
    //
    // A persona carries no token of its own — access is one envelope, the user's, the same
    // whichever persona speaks. So when the chosen agent has none, inherit the default
    // agent's: otherwise picking a persona would silently drop every tool and the agent
    // would answer from nothing, which is the one failure mode the whole design is meant to
    // avoid. Falling back keeps the envelope the user actually configured.
    let token = match agent.mcp_token_id {
        Some(t) => Some(t),
        None => store::default_agent(&state.pool).await?.mcp_token_id,
    };
    let conversation = store::create_conversation(&state.pool, agent.id, token).await?;
    let conversation = with_effective_model(&state, vec![conversation]).await?.remove(0);
    Ok(Json(json!({ "conversation": conversation })))
}

async fn get_conversation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let conversation = store::get_conversation(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    let conversation = with_effective_model(&state, vec![conversation]).await?.remove(0);
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
    /// Switch the conversation to another persona. Takes effect from the next message: the
    /// turns already in the transcript were produced under the previous one, and a marker row
    /// records where the handover happened so the history stays attributable.
    agent_id: Option<Uuid>,
    /// Provider for THIS conversation: absent = unchanged, null = back to inheriting,
    /// id = use that provider here only.
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    provider_id: Option<Option<Uuid>>,
    /// Model for this conversation; empty string = back to inheriting.
    model: Option<String>,
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
    if body.provider_id.is_some() || body.model.is_some() {
        // A set provider must exist — a clear 400 rather than an FK 500.
        if let Some(Some(pid)) = body.provider_id {
            if store::get_provider(&state.pool, pid).await?.is_none() {
                return Err(ApiError::bad_request("provider not found"));
            }
        }
        // Switching provider drops a model id that belonged to the old one: the two are a
        // pair, and keeping the id would send an Anthropic model to an OpenAI endpoint.
        let model = match (&body.model, body.provider_id) {
            (Some(m), _) => Some(m.trim().to_string()),
            (None, Some(_)) => Some(String::new()),
            (None, None) => None,
        };
        if !store::set_conversation_model(&state.pool, id, body.provider_id, model.as_deref())
            .await?
        {
            return Err(ApiError::not_found("conversation not found"));
        }
    }
    if let Some(agent_id) = body.agent_id {
        let conv = store::get_conversation(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("conversation not found"))?;
        if conv.agent_id != agent_id {
            let next = store::get_agent(&state.pool, agent_id)
                .await?
                .ok_or_else(|| ApiError::bad_request("agent not found"))?;
            let previous = store::get_agent(&state.pool, conv.agent_id).await?;
            store::set_conversation_agent(&state.pool, id, agent_id).await?;
            // Drop a marker into the transcript. Stored with role "system", which
            // `build_messages` skips, so it is shown to the user and never sent to a
            // provider — a note about the conversation, not a turn in it.
            let from = previous.map(|p| p.name).unwrap_or_else(|| "a removed agent".into());
            let content = json!([{
                "type": "text",
                "text": format!("Switched from {from} to {}. Everything above was produced by {from}.", next.name),
            }]);
            store::add_message(&state.pool, id, "system", &content, 0, 0).await?;
        }
    }
    let conversation = store::get_conversation(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    let conversation = with_effective_model(&state, vec![conversation]).await?.remove(0);
    Ok(Json(json!({ "ok": true, "conversation": conversation })))
}

#[derive(Deserialize)]
struct ConfirmBody {
    /// The `tool_use` id from the `confirm` SSE frame.
    tool_use_id: String,
    approve: bool,
}

/// Answer a pending write (R5). The run is parked inside its tool loop waiting for this; a
/// 404 means the wait already ended — the user took longer than the window, or the run was
/// stopped — and nothing was written either way.
async fn confirm_write(
    State(_state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ConfirmBody>,
) -> Result<Json<Value>, ApiError> {
    if !crate::agent::confirm::resolve(id, &body.tool_use_id, body.approve) {
        return Err(ApiError::not_found(
            "that write is no longer pending — it timed out or the run was stopped, and nothing \
             was changed",
        ));
    }
    Ok(Json(json!({ "ok": true })))
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

    // Resolve the provider + model for this run: the conversation's own choice, then the
    // persona's, then the default agent's.
    let default_agent = store::default_agent(&state.pool).await?;
    let choice = resolve_model(&conv, &agent, &default_agent).ok_or_else(|| {
        ApiError::bad_request("no provider configured — add one in agent settings")
    })?;
    let (provider_id, agent_model, agent_params) = (choice.provider_id, choice.model, choice.params);
    let prow = store::get_provider(&state.pool, provider_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("configured provider was deleted"))?;
    if !prow.enabled {
        return Err(ApiError::bad_request("the configured provider is disabled"));
    }
    let model = if agent_model.trim().is_empty() { prow.default_model.clone() } else { agent_model };
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
                skills: Vec::new(),
                conversation_id,
                agent_id: Some(agent.id),
                auto_approve_writes: agent.auto_approve_writes,
            },
            None => ToolContext::none(), // token was deleted; degrade to chat
        },
        None => ToolContext::none(),
    };
    // Memory is written without a token, so the persona stamp has to be set on every path.
    tools.conversation_id = conversation_id;
    tools.agent_id = Some(agent.id);
    // Connect this conversation's external MCP servers (parallel, best-effort): a server
    // that fails to answer yields a warning frame instead of blocking the run. Demo
    // sandbox: external MCP is an SSRF primitive — only the built-in gateway runs.
    let (external, warnings) = if crate::demo::enabled() {
        (Vec::new(), Vec::new())
    } else {
        connect_external(&state, &conv.server_ids()).await
    };
    tools.external = external;

    // Resolve this agent's skill shelf once: the prompt advertises exactly this catalog and
    // `load_skill` accepts exactly these names, so the two can never disagree.
    let skill_catalog = if crate::demo::enabled() {
        Vec::new()
    } else {
        store::skill_catalog(&state.pool, &agent.shelf()).await?
    };
    tools.skills = skill_catalog.iter().map(|(name, _)| name.clone()).collect();

    // Roll up older turns into a summary when the history grows past the threshold, so long
    // conversations stay cheap (short-term memory). Best-effort: a failure just means we send
    // more history this run.
    if let Err(e) = agent::summary::maybe_summarize(
        &state,
        conversation_id,
        &conv,
        &prow,
        &model,
        &agent_params,
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
    let system =
        build_system_prompt(&state, &agent.system_prompt, &conv.summary, &skill_catalog, &tools)
            .await?;
    let stored = store::list_messages(&state.pool, conversation_id).await?;
    let covers = (conv.summary_covers.max(0) as usize).min(stored.len());
    let messages = build_messages(&stored[covers..]);

    let req = ChatRequest {
        model,
        system,
        messages,
        tools: tools.tool_defs(),
        params: agent_params.clone(),
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

/// Which provider, model and params a run will actually use.
pub(crate) struct ModelChoice {
    pub provider_id: Uuid,
    /// Empty means "the provider's own default model".
    pub model: String,
    pub params: Value,
}

/// Resolve provider + model + params for a conversation: **its own override, then the
/// persona's, then the default agent's**.
///
/// The rule that makes this more than a chain of `or_else`: the model follows whoever
/// supplied the provider. A persona configured for Anthropic carries an Anthropic model id,
/// so a conversation that switches to an OpenAI-compatible provider must NOT keep it — that
/// pairing produces a 404 from the new endpoint and reads to the user as "the switch broke
/// the chat". Overriding only the model, with no provider of its own, is still allowed:
/// that is picking another model from the same vendor, which is the common case.
///
/// Params (max_tokens, temperature…) are generic knobs rather than vendor ids, so they follow
/// the persona and fall back to the default agent — an inherited provider without them would
/// silently truncate replies at a default the user never chose.
pub(crate) fn resolve_model(
    conv: &store::Conversation,
    agent: &store::Agent,
    default_agent: &store::Agent,
) -> Option<ModelChoice> {
    let params = {
        let own_empty = agent.params.as_object().is_none_or(|o| o.is_empty());
        if own_empty { default_agent.params.clone() } else { agent.params.clone() }
    };
    let conv_model = conv.model.trim();

    // The conversation names a provider: it owns the model too. An empty model here means
    // "that provider's default", never the persona's model id for a different vendor.
    if let Some(p) = conv.provider_id {
        return Some(ModelChoice { provider_id: p, model: conv_model.to_string(), params });
    }
    // Otherwise the provider comes from the persona, or from the default agent. The
    // conversation may still override just the model on top of it.
    let (provider_id, inherited_model) = match agent.provider_id {
        Some(p) => (p, agent.model.clone()),
        None => {
            let p = default_agent.provider_id?;
            let m = if agent.model.trim().is_empty() {
                default_agent.model.clone()
            } else {
                agent.model.clone()
            };
            (p, m)
        }
    };
    let model = if conv_model.is_empty() { inherited_model } else { conv_model.to_string() };
    Some(ModelChoice { provider_id, model, params })
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

/// How many memories carry their description in the injected index. Beyond this, the newest-
/// first index lists slug + author only, so a large store's descriptions stop being re-sent on
/// every turn of every run. Every memory stays listed (discoverable) and readable by slug.
const MEMORY_INDEX_DESCRIBED: usize = 40;

/// Compose the effective system prompt: the agent's base prompt, then (when present) the
/// rolling conversation summary, the long-term memory index (slugs + descriptions), and this
/// agent's skill shelf (names + descriptions), resolved by the caller. Bodies are pulled on
/// demand via the memory_read / load_skill tools — only the index is injected, keeping token
/// cost bounded.
async fn build_system_prompt(
    state: &AppState,
    base: &str,
    summary: &str,
    skills: &[(String, String)],
    tools: &ToolContext,
) -> Result<String, ApiError> {
    let mut out = base.trim().to_string();

    // The clock. Nothing else gives the model one: it has no time tool, and "prepare today's
    // session" / "last week's trades" / "remind me tomorrow" all need a date. Without this it
    // either asks, or guesses from whatever endpoint happened to echo a timestamp.
    let now = time::OffsetDateTime::now_utc();
    out.push_str(&format!(
        "\n\n## Now\n{}-{:02}-{:02} {:02}:{:02} UTC ({}). Dates you compute come from this, \
         never from training.\n",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.weekday(),
    ));

    // The module index, verbatim from the catalog the tools serve. Every conversation used to
    // spend its first tool round asking for exactly this, and the answer is short and stable
    // for the whole run — so it rides in the prompt and the round is saved. Drilling into a
    // module (the part that is long) still goes through otw_catalog.
    if tools.has_mcp_tools() {
        out.push_str("\n\n## Data modules you can reach\n");
        out.push_str(&crate::mcp::agent_catalog(&tools.permissions, None));
        out.push('\n');
    }

    if !summary.trim().is_empty() {
        out.push_str("\n\n## Summary of earlier conversation\n");
        out.push_str(summary.trim());
    }

    // Demo mode ships neither section: the tools that act on them are withheld (see
    // `tools::builtin_defs`), so listing the index would only invite calls that fail.
    let memories = if crate::demo::enabled() { Vec::new() } else { store::memory_index(&state.pool).await? };
    if !memories.is_empty() {
        // The writer is part of the index, not decoration: memory is one shared store, so a
        // constraint the day trader recorded about one session must not read to another
        // persona as a standing fact about the whole book.
        out.push_str(
            "\n\n## Long-term memory\nFacts you've saved. Use memory_read(slug) for full content; \
             memory_write to add or update one. The name in brackets is the persona that wrote \
             the entry — treat one written by another persona as its observation, not as an \
             established fact.\n",
        );
        // The whole index rides in every prompt, on every turn of every run. Descriptions are
        // the heavy part, so only the most recent MEMORY_INDEX_DESCRIBED carry theirs; older
        // ones list the slug (and author) alone. Nothing is dropped — every memory stays
        // discoverable and memory_read-able by slug — the token cost of a large store just
        // stops scaling with its descriptions. `memory_index` is ordered newest-first.
        for (i, (slug, desc, author)) in memories.iter().enumerate() {
            let by = author.as_ref().map(|a| format!(" [{a}]")).unwrap_or_default();
            if i < MEMORY_INDEX_DESCRIBED && !desc.trim().is_empty() {
                out.push_str(&format!("- {slug}{by}: {desc}\n"));
            } else {
                out.push_str(&format!("- {slug}{by}\n"));
            }
        }
        if memories.len() > MEMORY_INDEX_DESCRIBED {
            out.push_str(
                "(Descriptions above are shown for the most recent memories only; for the rest, \
                 read the slug with memory_read when it looks relevant.)\n",
            );
        }
    }

    // The shelf was resolved by the caller (agent allowlist ∩ enabled skills).
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
/// First line of the user's message as a short title. Short on purpose: this is a sidebar
/// label, not a summary — 60 characters overflowed the list and pushed the useful part of the
/// title out of view. Cuts on a word boundary when there is a reasonable one, so a title reads
/// as words rather than a chopped fragment.
fn auto_title(text: &str) -> String {
    const MAX: usize = 38;
    let first = text.lines().next().unwrap_or(text).trim();
    if first.chars().count() <= MAX {
        return first.to_string();
    }
    // Cut by characters, not bytes — a byte slice would split a multi-byte character.
    let head: String = first.chars().take(MAX).collect();
    // Back off to the last space, unless that throws away most of the title (one very long
    // word, e.g. a URL), in which case the hard cut is the better answer.
    let cut = match head.rfind(' ') {
        Some(i) if i >= MAX * 2 / 3 => i,
        _ => head.len(),
    };
    format!("{}…", head[..cut].trim_end())
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
    // Editing a memory here clears the persona stamp — once the user has rewritten it, the
    // fact is theirs, whichever persona first wrote it down.
    let kind = if input.kind.trim().is_empty() { "manual" } else { input.kind.trim() };
    let memory =
        store::upsert_memory(&state.pool, &slug, input.description.trim(), &input.content, kind, None)
            .await?;
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

/// Skills, each with how many personas hold it. Editing a body changes it for every one of
/// them and deleting takes it off all their shelves, so the count travels with the row.
async fn list_skills(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let skills = store::list_skills(&state.pool).await?;
    let usage = store::skill_usage(&state.pool).await?;
    let out: Vec<Value> = skills
        .into_iter()
        .map(|s| {
            let used_by = usage.iter().find(|(id, _)| *id == s.id).map(|(_, n)| *n).unwrap_or(0);
            json!({ "skill": s, "used_by": used_by })
        })
        .collect();
    Ok(Json(json!({ "skills": out, "body_warn_chars": store::SKILL_BODY_WARN_CHARS })))
}

/// Shortest body that can plausibly be a procedure. A skill saved near-empty stays on every
/// persona's shelf, is advertised in their prompts, and `load_skill` serves the stub without a
/// word of warning — the agent then works with no instructions where it believes it has some.
const SKILL_BODY_MIN_CHARS: usize = 40;

fn validate_skill(input: &store::SkillInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("skill name required"));
    }
    let len = input.body.trim().chars().count();
    if len < SKILL_BODY_MIN_CHARS {
        return Err(ApiError::bad_request(&format!(
            "skill body too short ({len} characters, min {SKILL_BODY_MIN_CHARS}) — a skill is \
             loaded verbatim as instructions; to take one out of service, disable it instead"
        )));
    }
    if input.body.chars().count() > store::SKILL_BODY_MAX_CHARS {
        return Err(ApiError::bad_request(&format!(
            "skill body too long ({} characters, max {}) — a skill is a procedure, not a manual; \
             split it or point at a document",
            input.body.chars().count(),
            store::SKILL_BODY_MAX_CHARS
        )));
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
    // Take it off every shelf first: a persona holding an id that no longer resolves shows an
    // editor listing a skill it can never load.
    store::purge_skill_from_shelves(&state.pool, id).await?;
    if !store::delete_skill(&state.pool, id).await? {
        return Err(ApiError::not_found("skill not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ── Personas + skills: export / import ────────────────────────────────────────
//
// A self-hosted box, so this is a plain feature: no review gate, no signing. What the
// operator loads into their own agent is their call (spec §9). The one fixed point is that
// none of it is in the MCP catalog — injected content can never make the agent widen its own
// shelf, because the agent has no route that touches personas or skills at all.

/// A skill as it travels: name, description, body, enabled. No ids — an import has to resolve
/// against the target box's own rows, and carrying ids would invite collisions with unrelated
/// skills that happen to share a uuid.
fn skill_export(s: &store::Skill) -> Value {
    json!({
        "name": s.name,
        "description": s.description,
        "body": s.body,
        "enabled": s.enabled,
    })
}

/// A persona plus the bodies of the skills on its shelf, so one file is enough to reproduce
/// it somewhere else. Shelves travel as skill NAMES for the same reason: names are what a
/// human reads in the file, and the importer re-resolves them to local ids.
async fn agent_export(state: &AppState, agent: &store::Agent) -> Result<Value, ApiError> {
    let all = store::list_skills(&state.pool).await?;
    let shelf = agent.shelf();
    let on_shelf: Vec<&store::Skill> = match &shelf {
        otw_store::agent::Shelf::All => all.iter().collect(),
        otw_store::agent::Shelf::Only(ids) => all.iter().filter(|s| ids.contains(&s.id)).collect(),
    };
    Ok(json!({
        "name": agent.name,
        "system_prompt": agent.system_prompt,
        "auto_approve_writes": agent.auto_approve_writes,
        // "*" survives the trip, so exporting the default assistant re-imports as itself.
        "skills": match &shelf {
            otw_store::agent::Shelf::All => json!([otw_store::agent::SHELF_ALL]),
            otw_store::agent::Shelf::Only(_) =>
                json!(on_shelf.iter().map(|s| s.name.clone()).collect::<Vec<_>>()),
        },
        "skill_bodies": on_shelf.iter().map(|s| skill_export(s)).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
struct ExportQuery {
    /// One persona by id. Omit for the whole shelf (every persona + every skill).
    #[serde(default)]
    agent: Option<Uuid>,
    /// One skill by id.
    #[serde(default)]
    skill: Option<Uuid>,
}

/// Export a persona, a skill, or everything, as a JSON bundle with the markdown inline.
async fn export_shelf(
    State(state): State<AppState>,
    Query(q): Query<ExportQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let (payload, name) = if let Some(id) = q.agent {
        let agent = store::get_agent(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("agent not found"))?;
        let file = format!("otw-persona-{}.json", crate::agent::slugify(&agent.name));
        (json!({ "version": 1, "personas": [agent_export(&state, &agent).await?] }), file)
    } else if let Some(id) = q.skill {
        let skill = store::get_skill(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("skill not found"))?;
        let file = format!("otw-skill-{}.json", crate::agent::slugify(&skill.name));
        (json!({ "version": 1, "skills": [skill_export(&skill)] }), file)
    } else {
        let agents = store::list_agents(&state.pool).await?;
        let mut personas = Vec::with_capacity(agents.len());
        for a in &agents {
            personas.push(agent_export(&state, a).await?);
        }
        let skills: Vec<Value> =
            store::list_skills(&state.pool).await?.iter().map(skill_export).collect();
        (json!({ "version": 1, "personas": personas, "skills": skills }), "otw-agent-shelf.json".to_string())
    };
    let body = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/json; charset=utf-8".to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    ))
}

#[derive(Deserialize, Default)]
struct ImportBundle {
    #[serde(default)]
    personas: Vec<Value>,
    #[serde(default)]
    skills: Vec<Value>,
    /// Overwrite a skill body / persona prompt when the name already exists. Off by default:
    /// an import should not silently rewrite work that is already on the box.
    #[serde(default)]
    overwrite: bool,
}

/// Import a bundle produced by [`export_shelf`]. Names are the identity: an existing name is
/// skipped, or replaced when `overwrite` is set. Imported personas are user-owned — never
/// builtin, so the seeder never touches them and "reset to shipped" is not offered.
async fn import_shelf(
    State(state): State<AppState>,
    Json(bundle): Json<ImportBundle>,
) -> Result<Json<Value>, ApiError> {
    let mut skills_added = 0usize;
    let mut skills_updated = 0usize;
    let mut skills_skipped = 0usize;
    let mut personas_added = 0usize;
    let mut personas_updated = 0usize;
    let mut personas_skipped = 0usize;

    // Skills first: a persona's shelf is resolved against what exists after this step, so a
    // bundle carrying both lands complete in one pass.
    let mut loose: Vec<Value> = bundle.skills.clone();
    for p in &bundle.personas {
        if let Some(bodies) = p.get("skill_bodies").and_then(|v| v.as_array()) {
            loose.extend(bodies.iter().cloned());
        }
    }
    for raw in &loose {
        let input = store::SkillInput {
            name: raw.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string(),
            description: raw
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            body: raw.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            enabled: raw.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        };
        if input.name.is_empty() {
            continue;
        }
        validate_skill(&input)?;
        let existing = store::list_skills(&state.pool)
            .await?
            .into_iter()
            .find(|s| s.name == input.name);
        match existing {
            None => {
                store::add_skill(&state.pool, &input).await.map_err(dup_or(&input.name))?;
                skills_added += 1;
            }
            Some(s) if bundle.overwrite => {
                store::update_skill(&state.pool, s.id, &input).await.map_err(dup_or(&input.name))?;
                skills_updated += 1;
            }
            Some(_) => skills_skipped += 1,
        }
    }

    for raw in &bundle.personas {
        let name = raw.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        check_agent_name(&name)?;
        let existing = store::list_agents(&state.pool)
            .await?
            .into_iter()
            .find(|a| a.name.trim().to_lowercase() == name.to_lowercase());
        match (&existing, bundle.overwrite) {
            // Same name, no overwrite: leave what is on the box alone.
            (Some(_), false) => {
                personas_skipped += 1;
                continue;
            }
            // Same name with overwrite: the flag says the bundle wins, for the prompt and the
            // shelf exactly as it does for a skill body. Provider, model and token stay local
            // — they are this install's wiring, not the persona's content.
            _ => {}
        }
        // Shelf by name → local ids. A name that did not come with a body and is not already
        // here simply is not on the shelf: better a narrower persona than one that claims a
        // skill it cannot load.
        let declared: Vec<String> = raw
            .get("skills")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let skills = if declared.iter().any(|s| s == otw_store::agent::SHELF_ALL) {
            json!([otw_store::agent::SHELF_ALL])
        } else {
            let refs: Vec<&str> = declared.iter().map(String::as_str).collect();
            let ids = crate::agent::builtin::seed::shelf_ids(&state.pool, &refs).await?;
            json!(ids)
        };
        let prompt = raw.get("system_prompt").and_then(|v| v.as_str()).unwrap_or("").trim();
        let auto = raw.get("auto_approve_writes").and_then(|v| v.as_bool());
        match existing {
            Some(a) => {
                let upd = store::AgentUpdate {
                    name: Some(name.clone()),
                    system_prompt: Some(prompt.to_string()),
                    skills: Some(skills),
                    auto_approve_writes: auto,
                    ..Default::default()
                };
                store::update_agent(&state.pool, a.id, &upd).await?;
                personas_updated += 1;
            }
            None => {
                let agent = store::create_agent(&state.pool, &name, prompt, &skills).await?;
                if let Some(flag) = auto {
                    let upd =
                        store::AgentUpdate { auto_approve_writes: Some(flag), ..Default::default() };
                    store::update_agent(&state.pool, agent.id, &upd).await?;
                }
                personas_added += 1;
            }
        }
    }

    Ok(Json(json!({
        "personas_added": personas_added,
        "personas_updated": personas_updated,
        "personas_skipped": personas_skipped,
        "skills_added": skills_added,
        "skills_updated": skills_updated,
        "skills_skipped": skills_skipped,
    })))
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

#[cfg(test)]
mod model_resolution_tests {
    use super::*;
    use time::OffsetDateTime;

    fn agent(provider: Option<Uuid>, model: &str, params: Value) -> store::Agent {
        store::Agent {
            id: Uuid::new_v4(),
            name: "a".into(),
            system_prompt: String::new(),
            provider_id: provider,
            model: model.into(),
            params,
            mcp_token_id: None,
            skills: json!(["*"]),
            auto_approve_writes: false,
            slug: String::new(),
            builtin: false,
            is_default: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn conv(provider: Option<Uuid>, model: &str) -> store::Conversation {
        store::Conversation {
            id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            title: String::new(),
            summary: String::new(),
            summary_covers: 0,
            mcp_token_id: None,
            mcp_servers: json!([]),
            backtest_runs: 0,
            provider_id: provider,
            model: model.into(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    /// Nothing overridden anywhere: the default agent supplies everything, exactly as before
    /// per-conversation models existed.
    #[test]
    fn inherits_from_the_default_agent() {
        let p = Uuid::new_v4();
        let def = agent(Some(p), "sonnet", json!({ "max_tokens": 4096 }));
        let c = resolve_model(&conv(None, ""), &agent(None, "", json!({})), &def).unwrap();
        assert_eq!(c.provider_id, p);
        assert_eq!(c.model, "sonnet");
        assert_eq!(c.params, json!({ "max_tokens": 4096 }));
    }

    /// A persona with its own provider wins over the default agent.
    #[test]
    fn the_persona_beats_the_default_agent() {
        let (p1, p2) = (Uuid::new_v4(), Uuid::new_v4());
        let def = agent(Some(p1), "cheap", json!({}));
        let persona = agent(Some(p2), "strong", json!({}));
        let c = resolve_model(&conv(None, ""), &persona, &def).unwrap();
        assert_eq!(c.provider_id, p2);
        assert_eq!(c.model, "strong");
    }

    /// The point of the feature: two chats on the same persona, different models.
    #[test]
    fn the_conversation_beats_the_persona() {
        let (p1, p2) = (Uuid::new_v4(), Uuid::new_v4());
        let def = agent(Some(p1), "cheap", json!({}));
        let persona = agent(Some(p1), "strong", json!({}));
        let here = resolve_model(&conv(Some(p2), "local-llama"), &persona, &def).unwrap();
        assert_eq!(here.provider_id, p2);
        assert_eq!(here.model, "local-llama");
        // …while a sibling conversation with no override still gets the persona's.
        let there = resolve_model(&conv(None, ""), &persona, &def).unwrap();
        assert_eq!((there.provider_id, there.model.as_str()), (p1, "strong"));
    }

    /// The trap: switching provider must drop the old vendor's model id, or the new endpoint
    /// gets asked for a model it has never heard of.
    #[test]
    fn switching_provider_does_not_carry_the_old_model_id() {
        let (anthropic, openai) = (Uuid::new_v4(), Uuid::new_v4());
        let persona = agent(Some(anthropic), "claude-opus-4-8", json!({}));
        let def = agent(Some(anthropic), "claude-opus-4-8", json!({}));
        let c = resolve_model(&conv(Some(openai), ""), &persona, &def).unwrap();
        assert_eq!(c.provider_id, openai);
        assert_eq!(c.model, "", "must fall back to the new provider's own default");
    }

    /// Overriding only the model is picking another model from the same vendor — allowed.
    #[test]
    fn model_only_override_keeps_the_inherited_provider() {
        let p = Uuid::new_v4();
        let persona = agent(Some(p), "strong", json!({}));
        let c = resolve_model(&conv(None, "  fast  "), &persona, &persona).unwrap();
        assert_eq!(c.provider_id, p);
        assert_eq!(c.model, "fast", "trimmed");
    }

    /// Params are generic knobs, so an empty set on the persona falls back rather than
    /// silently sending a provider default the user never chose.
    #[test]
    fn params_fall_back_when_the_persona_sets_none() {
        let p = Uuid::new_v4();
        let def = agent(Some(p), "m", json!({ "max_tokens": 8192 }));
        let persona = agent(Some(p), "m", json!({}));
        assert_eq!(
            resolve_model(&conv(None, ""), &persona, &def).unwrap().params,
            json!({ "max_tokens": 8192 })
        );
        let persona = agent(Some(p), "m", json!({ "max_tokens": 1024 }));
        assert_eq!(
            resolve_model(&conv(None, ""), &persona, &def).unwrap().params,
            json!({ "max_tokens": 1024 })
        );
    }

    /// No provider anywhere is a configuration error, reported rather than guessed at.
    #[test]
    fn no_provider_anywhere_resolves_to_nothing() {
        let none = agent(None, "", json!({}));
        assert!(resolve_model(&conv(None, ""), &none, &none).is_none());
    }
}

#[cfg(test)]
mod title_tests {
    use super::auto_title;

    #[test]
    fn short_titles_are_untouched() {
        assert_eq!(auto_title("Prep my session"), "Prep my session");
    }

    /// The case that prompted this: a long first line must not fill the sidebar.
    #[test]
    fn long_titles_cut_on_a_word_boundary() {
        let t = auto_title("Backtest a 20/50 EMA crossover long-only on my BTCUSDT data with realistic costs");
        assert!(t.chars().count() <= 39, "too long: {t:?} ({})", t.chars().count());
        assert!(t.ends_with('…'));
        // No half-word before the ellipsis.
        assert!(!t.trim_end_matches('…').ends_with("cross"), "cut mid-word: {t:?}");
    }

    /// One unbroken token (a URL) has no usable space — a hard cut beats returning almost
    /// nothing.
    #[test]
    fn unbreakable_titles_still_get_cut() {
        let t = auto_title("https://example.com/a/very/long/path/that/never/breaks/anywhere/at/all");
        assert!(t.chars().count() <= 39);
        assert!(t.ends_with('…'));
    }

    /// Multi-byte characters must not be split (a byte slice would panic or corrupt).
    #[test]
    fn multibyte_titles_are_safe() {
        let t = auto_title("Analyse le marché des crypto­monnaies européennes en détail aujourd'hui");
        assert!(t.chars().count() <= 39);
        let t2 = auto_title("请详细分析一下比特币在过去五年的价格走势以及波动率的变化情况和风险");
        assert!(t2.chars().count() <= 39);
    }

    #[test]
    fn only_the_first_line_is_used() {
        assert_eq!(auto_title("Line one\nLine two"), "Line one");
    }
}
