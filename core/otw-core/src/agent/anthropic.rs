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
    param_u32, Block, ChatRequest, Msg, Provider, ProviderEvent, Role, StopReason, ToolDef,
    Usage,
};
use super::sse::SseTypedDecoder;

/// The Messages API version pin. Not a model or vendor default — a required protocol header.
const API_VERSION: &str = "2023-06-01";
const DEFAULT_BASE: &str = "https://api.anthropic.com";

pub struct Anthropic {
    pub base_url: String,
    pub api_key: String,
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
        let mut body = json!({
            "model": req.model,
            "max_tokens": param_u32(&req.params, "max_tokens", 2048),
            "stream": true,
            "messages": wire_messages(&req.messages),
        });
        if !req.system.is_empty() {
            body["system"] = json!(req.system);
        }
        if !req.tools.is_empty() {
            body["tools"] = json!(wire_tools(&req.tools));
        }
        // User params ride verbatim (temperature, top_p, thinking…); null removes a key.
        super::apply_params(&mut body, &req.params);

        let resp = crate::rate::send(
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
                            if let Some(u) = v.pointer("/message/usage") {
                                usage.input_tokens =
                                    u.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
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
