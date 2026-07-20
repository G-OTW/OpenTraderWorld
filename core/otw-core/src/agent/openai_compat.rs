//! OpenAI-compatible `/chat/completions` adapter (SSE streaming).
//!
//! One adapter for OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Together, Mistral, and any
//! provider exposing the `/chat/completions` shape (including Gemini's OpenAI-compat endpoint).
//! The provider row supplies `base_url` + key + model; nothing is vendor-defaulted here.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::provider::{
    param_u32, Block, ChatRequest, Msg, Provider, ProviderEvent, Role, StopReason, ToolDef,
    Usage,
};
use super::sse::SseDecoder;

pub struct OpenAiCompat {
    pub base_url: String,
    pub api_key: String,
}

/// Translate normalized tool defs to the OpenAI `tools` array.
fn wire_tools(tools: &[ToolDef]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                },
            })
        })
        .collect()
}

/// Build the `messages` array in OpenAI shape from the normalized model. Tool blocks map to
/// the OpenAI convention: an assistant message carries `tool_calls`, and each tool result is
/// its own `{ role: "tool", tool_call_id, content }` message.
fn wire_messages(system: &str, msgs: &[Msg]) -> Vec<Value> {
    let mut out = Vec::new();
    if !system.is_empty() {
        out.push(json!({ "role": "system", "content": system }));
    }
    for m in msgs {
        let text: String = m
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let tool_calls: Vec<Value> = m
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, input } => Some(json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": input.to_string() },
                })),
                _ => None,
            })
            .collect();
        let tool_results: Vec<(&str, &str)> = m
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::ToolResult { tool_use_id, content, .. } => Some((tool_use_id.as_str(), content.as_str())),
                _ => None,
            })
            .collect();

        // Tool results become standalone tool-role messages (order preserved).
        if !tool_results.is_empty() {
            for (id, content) in tool_results {
                out.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
            }
            continue;
        }

        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let mut msg = json!({ "role": role, "content": text });
        if !tool_calls.is_empty() {
            msg["tool_calls"] = json!(tool_calls);
            // OpenAI accepts null content alongside tool_calls.
            if text.is_empty() {
                msg["content"] = Value::Null;
            }
        }
        out.push(msg);
    }
    out
}

#[async_trait]
impl Provider for OpenAiCompat {
    /// `GET /models` — the standard OpenAI-compatible model list (OpenRouter, OpenAI,
    /// DeepSeek, Groq, … all implement it; proxies that don't simply error).
    async fn models(&self) -> anyhow::Result<Vec<String>> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let resp = crate::rate::send(
            &super::host_of(&self.base_url),
            super::http().get(&url).bearer_auth(&self.api_key),
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
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut body = json!({
            "model": req.model,
            "messages": wire_messages(&req.system, &req.messages),
            "stream": true,
            "stream_options": { "include_usage": true },
            "max_tokens": param_u32(&req.params, "max_tokens", 2048),
        });
        if !req.tools.is_empty() {
            body["tools"] = json!(wire_tools(&req.tools));
        }
        // User params ride verbatim; null removes a key — e.g. newer OpenAI models take
        // {"max_completion_tokens": N, "max_tokens": null}, and providers that reject
        // stream_options take {"stream_options": null}.
        super::apply_params(&mut body, &req.params);

        let resp = crate::rate::send(
            &super::host_of(&self.base_url),
            super::http().post(&url).bearer_auth(&self.api_key).json(&body),
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
            let mut decoder = SseDecoder::new();
            let mut usage = Usage::default();
            let mut stop = StopReason::EndTurn;
            // Streamed tool calls arrive as fragments keyed by index: accumulate
            // (id, name, arguments-string) and emit each as a ToolUse once the stream ends.
            let mut tool_calls: Vec<(String, String, String)> = Vec::new();
            'outer: while let Some(chunk) = byte_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(ProviderEvent::Error(format!("stream error: {e}"))).await;
                        break;
                    }
                };
                for data in decoder.push(&bytes) {
                    if data == "[DONE]" {
                        break 'outer;
                    }
                    let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
                    if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
                        usage.input_tokens =
                            u.get("prompt_tokens").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                        usage.output_tokens =
                            u.get("completion_tokens").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                    }
                    let Some(choice) = v.get("choices").and_then(|c| c.get(0)) else { continue };
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            if !content.is_empty()
                                && tx.send(ProviderEvent::TextDelta(content.to_string())).await.is_err()
                            {
                                return; // client gone
                            }
                        }
                        // DeepSeek R1 & friends expose reasoning here.
                        if let Some(reason) = delta
                            .get("reasoning_content")
                            .or_else(|| delta.get("reasoning"))
                            .and_then(|c| c.as_str())
                        {
                            if !reason.is_empty()
                                && tx.send(ProviderEvent::ThinkingDelta(reason.to_string())).await.is_err()
                            {
                                return;
                            }
                        }
                        // Accumulate streamed tool-call fragments by index.
                        if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                            for call in calls {
                                let idx = call.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                while tool_calls.len() <= idx {
                                    tool_calls.push((String::new(), String::new(), String::new()));
                                }
                                let slot = &mut tool_calls[idx];
                                let has_id = call.get("id").and_then(|x| x.as_str()).is_some();
                                if let Some(id) = call.get("id").and_then(|x| x.as_str()) {
                                    slot.0 = id.to_string();
                                }
                                if let Some(f) = call.get("function") {
                                    if let Some(name) = f.get("name").and_then(|x| x.as_str()) {
                                        // A fragment carrying the call `id` (re)announces the
                                        // call with its full name — replace, don't append
                                        // (some providers resend it every fragment, which
                                        // would concatenate into "foofoo"). Id-less fragments
                                        // are true streaming deltas and still append.
                                        if has_id {
                                            slot.1 = name.to_string();
                                        } else {
                                            slot.1.push_str(name);
                                        }
                                    }
                                    if let Some(args) = f.get("arguments").and_then(|x| x.as_str()) {
                                        slot.2.push_str(args);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                        stop = match fr {
                            "stop" => StopReason::EndTurn,
                            "length" => StopReason::MaxTokens,
                            "content_filter" => StopReason::Refusal,
                            "tool_calls" => StopReason::ToolUse,
                            _ => StopReason::Other,
                        };
                    }
                }
            }
            // Emit accumulated tool calls (arguments string → JSON; empty → {}).
            for (id, name, args) in tool_calls {
                if name.is_empty() {
                    continue;
                }
                let input = if args.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&args).unwrap_or_else(|_| json!({ "_raw": args }))
                };
                if tx.send(ProviderEvent::ToolUse { id, name, input }).await.is_err() {
                    return;
                }
            }
            let _ = tx.send(ProviderEvent::Done { stop, usage }).await;
        });

        Ok(ReceiverStream::new(rx).boxed())
    }
}
