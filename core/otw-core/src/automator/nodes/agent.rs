//! `agent`: one model turn inside a workflow.
//!
//! Not a conversation: no history is stored, no tools are offered, and nothing is written
//! to the agent module. A workflow turn is a function call (a prompt in, a value out), so
//! it talks to the provider adapter directly and the whole exchange lives in the run trace.
//!
//! Two things make the output usable downstream:
//! - `output: "json"` asks for a JSON object, parses it, and retries once with a repair
//!   instruction before failing. A block that follows this one needs fields, not prose.
//! - Tools are deliberately empty. An agent that could call the gateway from inside a
//!   workflow that itself calls the gateway is a permission puzzle with no product behind it.

use futures::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

use super::{NodeCtx, Outcome};
use crate::agent::provider::{Block, ChatRequest, Msg, ProviderEvent, Role, StopReason};

const DEFAULT_TIMEOUT: u64 = 120;
/// Cap on the prompt a workflow may send, so a runaway upstream payload fails here rather
/// than at the provider, with a message that names the block.
const MAX_PROMPT: usize = 200_000;

pub fn validate(config: &Value) -> Result<(), String> {
    super::field(config, "input")?;
    let output = super::opt(config, "output", "text");
    if output != "text" && output != "json" {
        return Err("output must be text or json".into());
    }
    for key in ["agent_id", "provider_id", "prompt_id"] {
        if let Some(raw) = config.get(key).and_then(|v| v.as_str()) {
            if !raw.trim().is_empty() && raw.parse::<Uuid>().is_err() {
                return Err(format!("{key} is not a valid id"));
            }
        }
    }
    Ok(())
}

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let config = &ctx.node.config;
    let pool = &ctx.state.pool;

    // Persona: a named agent, or the default one. Its provider, model and params are the
    // defaults; the block may override each of them.
    let agent = match uuid_of(config, "agent_id") {
        Some(id) => otw_store::agent::get_agent(pool, id)
            .await
            .map_err(|e| format!("loading the agent failed: {e}"))?
            .ok_or("the agent this block points at no longer exists")?,
        None => otw_store::agent::default_agent(pool)
            .await
            .map_err(|e| format!("loading the default agent failed: {e}"))?,
    };

    let provider_id = uuid_of(config, "provider_id")
        .or(agent.provider_id)
        .ok_or("no AI provider is configured: set one in the Agent module first")?;
    let provider_row = otw_store::agent::get_provider(pool, provider_id)
        .await
        .map_err(|e| format!("loading the provider failed: {e}"))?
        .ok_or("the AI provider this block points at no longer exists")?;
    if !provider_row.enabled {
        return Err(format!("the provider \"{}\" is disabled", provider_row.label));
    }
    let model = {
        let override_model = super::opt(config, "model", "");
        if !override_model.is_empty() {
            override_model.to_string()
        } else if !agent.model.is_empty() {
            agent.model.clone()
        } else {
            provider_row.default_model.clone()
        }
    };
    if model.is_empty() {
        return Err("no model is selected for this block".into());
    }

    // System prompt: a prompt-store entry, a custom text, or the persona's own.
    let system = match uuid_of(config, "prompt_id") {
        Some(id) => otw_store::prompts::get_prompt(pool, id)
            .await
            .map_err(|e| format!("loading the prompt failed: {e}"))?
            .ok_or("the stored prompt this block points at no longer exists")?
            .body,
        None => {
            let custom = super::opt(config, "system", "");
            if custom.is_empty() {
                agent.system_prompt.clone()
            } else {
                ctx.resolver.render_str(custom, ctx.vars, false).await?
            }
        }
    };

    let input = ctx.resolver.render_str(super::field(config, "input")?, ctx.vars, false).await?;
    if input.len() > MAX_PROMPT {
        return Err("the prompt is larger than 200 KB; pass a summary or a blob handle".into());
    }
    let wants_json = super::opt(config, "output", "text") == "json";
    let schema = config.get("schema").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

    let request = json!({
        "agent": agent.name,
        "provider": provider_row.label,
        "model": model,
        "output": if wants_json { "json" } else { "text" },
        "input": crate::agent::truncate_plain(&input, 2000),
    });

    let key = otw_store::agent::provider_key(pool, &ctx.state.cipher, provider_id)
        .await
        .map_err(|e| format!("reading the provider key failed: {e}"))?
        .unwrap_or_default();
    if key.is_empty() {
        return Err(format!("the provider \"{}\" has no API key", provider_row.label));
    }
    // The key is a secret the trace must never carry, even if a provider echoes it back.
    ctx.resolver.secrets.add(&key);

    let system = if wants_json {
        format!("{system}\n\n{}", json_instruction(&schema)).trim().to_string()
    } else {
        system
    };

    let mut messages = vec![Msg { role: Role::User, blocks: vec![Block::Text { text: input }] }];
    let budget = ctx.budget(DEFAULT_TIMEOUT);

    let mut attempt = 0;
    loop {
        let provider = crate::agent::build_provider(
            &provider_row.kind,
            &provider_row.base_url,
            key.clone(),
        )
        .ok_or("unsupported provider type")?;
        let req = ChatRequest {
            model: model.clone(),
            // A workflow block is one shot: nothing in the prompt is volatile, so the whole
            // system prompt is the stable half and the cache breakpoint sits after it. Two
            // runs of the same block on the same day read the prefix back instead of paying
            // for it twice.
            system: system.clone(),
            system_tail: String::new(),
            messages: messages.clone(),
            tools: Vec::new(),
            params: agent.params.clone(),
        };
        let turn = tokio::time::timeout(budget, one_turn(provider, req))
            .await
            .map_err(|_| "the model did not answer in time".to_string())??;

        if !wants_json {
            return Ok(Outcome::new(
                request,
                json!({ "text": turn.text, "tokens": { "in": turn.input_tokens, "out": turn.output_tokens } }),
            ));
        }
        match parse_json(&turn.text) {
            Ok(value) => {
                return Ok(Outcome::new(
                    request,
                    json!({ "data": value, "text": turn.text,
                            "tokens": { "in": turn.input_tokens, "out": turn.output_tokens } }),
                ))
            }
            Err(why) if attempt == 0 => {
                // One repair round. A second failure means the model cannot hold the
                // contract, and pretending otherwise would hand the next block garbage.
                attempt += 1;
                messages.push(Msg {
                    role: Role::Assistant,
                    blocks: vec![Block::Text { text: turn.text }],
                });
                messages.push(Msg {
                    role: Role::User,
                    blocks: vec![Block::Text {
                        text: format!(
                            "That was not valid JSON ({why}). Answer again with the JSON object \
                             alone: no prose, no code fence."
                        ),
                    }],
                });
            }
            Err(why) => return Err(format!("the model did not return valid JSON ({why})")),
        }
    }
}

struct Turn {
    text: String,
    input_tokens: i32,
    output_tokens: i32,
}

/// Drain one provider stream into a finished answer. The streaming run loop in
/// `agent::run` exists to feed an SSE client; a workflow only wants the result.
async fn one_turn(
    provider: Box<dyn crate::agent::provider::Provider>,
    req: ChatRequest,
) -> Result<Turn, String> {
    let mut stream = provider.stream(req).await.map_err(|e| format!("provider call failed: {e}"))?;
    let mut text = String::new();
    let mut usage = (0, 0);
    let mut stop = StopReason::EndTurn;
    while let Some(event) = stream.next().await {
        match event {
            ProviderEvent::TextDelta(d) => text.push_str(&d),
            ProviderEvent::Done { stop: s, usage: u } => {
                stop = s;
                usage = (u.input_tokens, u.output_tokens);
            }
            ProviderEvent::Error(e) => return Err(e),
            // Thinking is not an answer, and a workflow has no tools to run.
            ProviderEvent::ThinkingDelta(_) | ProviderEvent::ToolUse { .. } => {}
        }
    }
    match stop {
        StopReason::Refusal => return Err("the model declined to answer".into()),
        StopReason::MaxTokens if text.trim().is_empty() => {
            return Err("the answer hit the token limit before any text".into())
        }
        _ => {}
    }
    if text.trim().is_empty() {
        return Err("the model returned an empty answer".into());
    }
    Ok(Turn { text, input_tokens: usage.0, output_tokens: usage.1 })
}

fn json_instruction(schema: &str) -> String {
    let mut out = String::from(
        "Answer with a single JSON object and nothing else: no explanation, no code fence.",
    );
    if !schema.is_empty() {
        out.push_str("\nUse exactly this shape:\n");
        out.push_str(schema);
    }
    out
}

/// Parse the model's answer as JSON, tolerating the code fence models keep adding.
fn parse_json(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|rest| rest.trim_start().trim_end_matches("```").trim())
        .unwrap_or(trimmed);
    serde_json::from_str(body).map_err(|e| e.to_string())
}

fn uuid_of(config: &Value, key: &str) -> Option<Uuid> {
    config.get(key)?.as_str()?.trim().parse().ok()
}
