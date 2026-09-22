//! Anthropic Messages API adapter (`/v1/messages`, SSE streaming).
//!
//! Anthropic uses content blocks and its own typed SSE events (`content_block_delta`,
//! `message_delta`, …), so it needs a separate adapter from the OpenAI-compatible one. Raw
//! `reqwest`, no SDK. The provider row supplies base_url (default api.anthropic.com) + key +
//! model; nothing is vendor-defaulted in code beyond the required API version header.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::provider::{
    param_u32, Block, ChatRequest, Msg, Provider, ProviderEvent, Role, StopReason, ToolDef, Usage,
    DEFAULT_MAX_TOKENS,
};
use super::sse::SseTypedDecoder;

/// The Messages API version pin. Not a model or vendor default — a required protocol header.
const API_VERSION: &str = "2023-06-01";
const DEFAULT_BASE: &str = "https://api.anthropic.com";

pub struct Anthropic {
    pub base_url: String,
    pub api_key: String,
}

/// The cache marker Anthropic reads. Placed on the last block of a span to say "everything
/// up to here is stable, bill it as a cache read next time".
fn ephemeral() -> Value {
    json!({ "type": "ephemeral" })
}

/// Build the `messages` array in Anthropic shape. Content is a block array: text, plus
/// `tool_use` blocks on assistant turns and `tool_result` blocks on the following user turn.
///
/// Thinking blocks are NOT replayed: Anthropic requires their `signature`, which we don't
/// store. A message left with no wire content (e.g. thinking-only) is dropped entirely —
/// an empty text block is a hard 400, and consecutive same-role messages are merged
/// server-side, so dropping is safe.
fn wire_messages(msgs: &[Msg]) -> Vec<Value> {
    msgs.iter()
        .filter_map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            let mut content: Vec<Value> = Vec::new();
            for b in &m.blocks {
                match b {
                    Block::Text { text } if !text.is_empty() => {
                        content.push(json!({ "type": "text", "text": text }));
                    }
                    Block::ToolUse { id, name, input } => {
                        content.push(json!({ "type": "tool_use", "id": id, "name": name, "input": input }));
                    }
                    Block::ToolResult { tool_use_id, content: c, is_error } => {
                        content.push(json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": c,
                            "is_error": is_error,
                        }));
                    }
                    _ => {}
                }
            }
            if content.is_empty() {
                return None;
            }
            Some(json!({ "role": role, "content": content }))
        })
        .collect()
}

/// Translate normalized tool defs to Anthropic's `tools` array.
fn wire_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect()
}

/// Mark the last content block of the last message as a cache breakpoint.
///
/// This is the one that pays in a tool loop: each round re-sends the whole conversation, so
/// without it the same history is re-processed up to [`MAX_TOOL_ITERS`](crate::agent::run)
/// times in a single user turn. Anthropic caches the prefix up to the marker, so round N+1
/// reads what round N wrote.
fn mark_last_message(msgs: &mut [Value]) {
    let Some(last) = msgs.last_mut() else { return };
    let Some(blocks) = last.get_mut("content").and_then(|c| c.as_array_mut()) else { return };
    if let Some(block) = blocks.last_mut() {
        block["cache_control"] = ephemeral();
    }
}

/// The system prompt as Anthropic blocks, with the stable half marked cacheable.
///
/// Two blocks, never one: the tail carries the clock and the rolling summary, and a single
/// concatenated block would move on every request and cache nothing. The breakpoint also
/// covers the tool schemas, which sit above `system` in Anthropic's cache hierarchy.
fn wire_system(stable: &str, tail: &str) -> Option<Value> {
    let mut blocks: Vec<Value> = Vec::new();
    if !stable.trim().is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": stable,
            "cache_control": ephemeral(),
        }));
    }
    if !tail.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": tail }));
    }
    (!blocks.is_empty()).then(|| json!(blocks))
}

#[async_trait]
impl Provider for Anthropic {
    /// `GET /v1/models` — returns the account's available model ids.
    async fn models(&self) -> anyhow::Result<Vec<String>> {
        let base = if self.base_url.is_empty() { DEFAULT_BASE } else { self.base_url.trim_end_matches('/') };
        let url = format!("{base}/v1/models?limit=100");
        let resp = crate::rate::send(
            &super::host_of(base),
            super::http()
                .get(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!(super::friendly_http_error(status, &text));
        }
        let v: Value = resp.json().await?;
        Ok(v.get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn stream(
        &self,
        req: ChatRequest,
    ) -> anyhow::Result<BoxStream<'static, ProviderEvent>> {
        let base = if self.base_url.is_empty() { DEFAULT_BASE } else { self.base_url.trim_end_matches('/') };
        let url = format!("{base}/v1/messages");
        // Prompt caching is on unless the user turns it off (some compatible gateways reject
        // the field). Two breakpoints: the stable system half (which also covers the tool
        // schemas) and the last message.
        let caching = req.params.get("prompt_cache").and_then(|v| v.as_bool()).unwrap_or(true);
        let mut messages = wire_messages(&req.messages);
        if caching {
            mark_last_message(&mut messages);
        }
        let mut body = json!({
            "model": req.model,
            "max_tokens": param_u32(&req.params, "max_tokens", DEFAULT_MAX_TOKENS),
            "stream": true,
            "messages": messages,
        });
        match (caching, wire_system(&req.system, &req.system_tail)) {
            (true, Some(blocks)) => body["system"] = blocks,
            _ => {
                let text = req.system_text();
                if !text.is_empty() {
                    body["system"] = json!(text);
                }
            }
        }
        if !req.tools.is_empty() {
            body["tools"] = json!(wire_tools(&req.tools));
        }
        // User params ride verbatim (temperature, top_p, thinking…); null removes a key.
        super::apply_params(&mut body, &req.params);

        let resp = super::send_retrying(
            &super::host_of(base),
            super::http()
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .json(&body),
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let retry = super::retry_after(&resp);
            let text = resp.text().await.unwrap_or_default();
            let mut msg = super::friendly_http_error(status.as_u16(), &text);
            if let Some(s) = retry {
                msg.push_str(&format!(" (retry after {s}s)"));
            }
            return Ok(futures::stream::once(async move { ProviderEvent::Error(msg) }).boxed());
        }

        let (tx, rx) = mpsc::channel::<ProviderEvent>(64);
        tokio::spawn(async move {
            let byte_stream = resp.bytes_stream();
            futures::pin_mut!(byte_stream);
            let mut decoder = SseTypedDecoder::new();
            let mut usage = Usage::default();
            let mut stop = StopReason::EndTurn;
            // Current tool_use block being streamed: (id, name, accumulated partial JSON).
            let mut cur_tool: Option<(String, String, String)> = None;
            'read: while let Some(chunk) = byte_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(ProviderEvent::Error(format!("stream error: {e}"))).await;
                        break;
                    }
                };
                for (event, data) in decoder.push(&bytes) {
                    let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
                    match event.as_str() {
                        "message_start" => {
                            // input_tokens land here; output_tokens accrue in message_delta.
                            // The cache pair is reported alongside and counts SEPARATELY from
                            // input_tokens on this API — a cached prompt shows a near-zero
                            // input_tokens, which reads as a bug until the pair is recorded.
                            if let Some(u) = v.pointer("/message/usage") {
                                let n = |k: &str| {
                                    u.get(k).and_then(|x| x.as_i64()).unwrap_or(0) as i32
                                };
                                usage.input_tokens = n("input_tokens");
                                usage.cache_write_tokens = n("cache_creation_input_tokens");
                                usage.cache_read_tokens = n("cache_read_input_tokens");
                            }
                        }
                        "content_block_start" => {
                            // A tool_use block opens here with its id + name.
                            if v.pointer("/content_block/type").and_then(|t| t.as_str()) == Some("tool_use") {
                                let id = v.pointer("/content_block/id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                let name = v.pointer("/content_block/name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                                cur_tool = Some((id, name, String::new()));
                            }
                        }
                        "content_block_delta" => {
                            if let Some(d) = v.get("delta") {
                                match d.get("type").and_then(|t| t.as_str()) {
                                    Some("text_delta") => {
                                        if let Some(t) = d.get("text").and_then(|x| x.as_str()) {
                                            if tx.send(ProviderEvent::TextDelta(t.to_string())).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                    Some("thinking_delta") => {
                                        if let Some(t) = d.get("thinking").and_then(|x| x.as_str()) {
                                            if tx.send(ProviderEvent::ThinkingDelta(t.to_string())).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let (Some((.., buf)), Some(pj)) =
                                            (cur_tool.as_mut(), d.get("partial_json").and_then(|x| x.as_str()))
                                        {
                                            buf.push_str(pj);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "content_block_stop" => {
                            if let Some((id, name, buf)) = cur_tool.take() {
                                let input = if buf.trim().is_empty() {
                                    json!({})
                                } else {
                                    serde_json::from_str(&buf).unwrap_or_else(|_| json!({ "_raw": buf }))
                                };
                                if tx.send(ProviderEvent::ToolUse { id, name, input }).await.is_err() {
                                    return;
                                }
                            }
                        }
                        "message_delta" => {
                            if let Some(sr) = v.pointer("/delta/stop_reason").and_then(|s| s.as_str()) {
                                stop = match sr {
                                    "end_turn" | "stop_sequence" => StopReason::EndTurn,
                                    "max_tokens" => StopReason::MaxTokens,
                                    "tool_use" => StopReason::ToolUse,
                                    "refusal" => StopReason::Refusal,
                                    _ => StopReason::Other,
                                };
                            }
                            if let Some(ot) = v.pointer("/usage/output_tokens").and_then(|x| x.as_i64()) {
                                usage.output_tokens = ot as i32;
                            }
                        }
                        "error" => {
                            let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("provider error");
                            let _ = tx.send(ProviderEvent::Error(msg.to_string())).await;
                            return;
                        }
                        // The message is complete — stop reading the byte stream entirely
                        // (a plain `break` would only exit this per-chunk event loop).
                        "message_stop" => break 'read,
                        _ => {}
                    }
                }
            }
            let _ = tx.send(ProviderEvent::Done { stop, usage }).await;
        });

        Ok(ReceiverStream::new(rx).boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(system: &str, tail: &str, msgs: Vec<Msg>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            system: system.into(),
            system_tail: tail.into(),
            messages: msgs,
            tools: Vec::new(),
            params: json!({}),
        }
    }

    /// The breakpoint goes on the stable half and nowhere else. Marking the tail too would
    /// cache a block that changes with the clock: a write every request, read never.
    #[test]
    fn only_the_stable_half_is_marked_cacheable() {
        let blocks = wire_system("you are an agent", "\n\n## Now\n2026-09-18").expect("system");
        let arr = blocks.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["cache_control"]["type"], "ephemeral");
        assert!(arr[1].get("cache_control").is_none(), "the tail must not be a breakpoint");
    }

    /// A tail with nothing in it must not produce an empty text block (a hard 400).
    #[test]
    fn an_empty_tail_produces_one_block() {
        let blocks = wire_system("you are an agent", "").expect("system");
        assert_eq!(blocks.as_array().expect("array").len(), 1);
        assert!(wire_system("", "").is_none(), "nothing to say means no system field");
    }

    /// The second breakpoint sits on the last block of the last message: that is what makes
    /// round N+1 of a tool loop read what round N wrote.
    #[test]
    fn the_last_message_carries_the_second_breakpoint() {
        let msgs = vec![
            Msg { role: Role::User, blocks: vec![Block::Text { text: "first".into() }] },
            Msg {
                role: Role::User,
                blocks: vec![
                    Block::ToolResult {
                        tool_use_id: "t1".into(),
                        content: "a".into(),
                        is_error: false,
                    },
                    Block::ToolResult {
                        tool_use_id: "t2".into(),
                        content: "b".into(),
                        is_error: false,
                    },
                ],
            },
        ];
        let mut wire = wire_messages(&msgs);
        mark_last_message(&mut wire);
        assert!(wire[0]["content"][0].get("cache_control").is_none());
        assert!(wire[1]["content"][0].get("cache_control").is_none());
        assert_eq!(wire[1]["content"][1]["cache_control"]["type"], "ephemeral");
    }

    /// Turning caching off has to fall back to a plain string system prompt carrying BOTH
    /// halves — a gateway that rejects the block form must not also lose the clock.
    #[test]
    fn the_uncached_fallback_keeps_both_halves() {
        let r = req("stable", "\n\n## Now\ntoday", Vec::new());
        let text = r.system_text();
        assert!(text.starts_with("stable"));
        assert!(text.contains("## Now"));
    }
}
