//! Rolling short-term memory — the OpenClaw-style pattern.
//!
//! When the *unsummarized* span of a conversation grows past a token threshold (estimated
//! cheaply as chars/4), fold everything but the most recent messages into
//! `agent_conversations.summary` and advance the `summary_covers` watermark. The run then
//! sends the summary (in the system prompt) plus only the messages after the watermark.
//!
//! The watermark keeps this bounded: each summarize call transcribes only the span between
//! the previous watermark and the keep-recent tail, and once folded that span is neither
//! re-transcribed nor re-sent verbatim. The whole call is best-effort and capped by
//! [`SUMMARY_TIMEOUT`] so a hung provider can't stall the run that triggered it.

use futures::stream::StreamExt;
use serde_json::Value;
use std::time::Duration;

use super::provider::{param_u32, Block, ChatRequest, Msg, ProviderEvent, Role};
use crate::AppState;
use otw_store::agent::{self, Conversation};
use otw_store::agent::Provider as ProviderRow;

/// Trigger once the estimated unsummarized prompt size exceeds this many tokens.
const SUMMARY_TRIGGER_TOKENS: usize = 6000;
/// Keep this many of the most recent messages out of the summary (sent verbatim).
const KEEP_RECENT: usize = 16;
/// Hard cap on the whole summarize call (it runs before the user's turn streams).
const SUMMARY_TIMEOUT: Duration = Duration::from_secs(90);
/// Cheap ~chars-per-token estimate for the trigger heuristic.
fn est_tokens(chars: usize) -> usize {
    chars / 4
}

/// Summarize older turns into `conversation.summary` when the unsummarized span is large.
/// Best-effort; returns Ok(()) even when nothing was summarized.
pub async fn maybe_summarize(
    state: &AppState,
    conversation_id: uuid::Uuid,
    conv: &Conversation,
    prow: &ProviderRow,
    model: &str,
    params: &Value,
) -> anyhow::Result<()> {
    let stored = agent::list_messages(&state.pool, conversation_id).await?;
    let covers = (conv.summary_covers.max(0) as usize).min(stored.len());
    let fresh = &stored[covers..];
    if fresh.len() <= KEEP_RECENT {
        return Ok(());
    }

    // Estimate what this run would send verbatim (existing summary + unsummarized text).
    let total_chars: usize =
        conv.summary.len() + fresh.iter().map(|m| m.content.to_string().len()).sum::<usize>();
    if est_tokens(total_chars) < SUMMARY_TRIGGER_TOKENS {
        return Ok(());
    }

    // Fold the span between the watermark and the keep-recent tail into the summary.
    // Advance the cutoff to the next plain user message so the unsummarized tail always
    // starts on a user turn: a watermark landing mid-exchange would leave assistant/tool
    // rows that are neither in the summary nor safely sendable (see build_messages).
    let mut cutoff = stored.len() - KEEP_RECENT;
    while cutoff < stored.len() && stored[cutoff].role != "user" {
        cutoff += 1;
    }
    if cutoff >= stored.len() {
        return Ok(()); // no clean boundary in the tail — try again next run
    }
    let older = &stored[covers..cutoff];
    let transcript = render_transcript(&conv.summary, older);
    if transcript.trim().is_empty() {
        return Ok(());
    }

    let key = match agent::provider_key(&state.pool, &state.cipher, prow.id).await? {
        Some(k) if !k.is_empty() => k,
        _ => return Ok(()), // no key → skip (the run itself will surface the error)
    };
    let Some(provider) = super::build_provider(&prow.kind, &prow.base_url, key) else {
        return Ok(());
    };

    let sys = "You compress a chat transcript into a concise running summary for the assistant's \
        own future reference. Preserve durable facts, decisions, user preferences, open threads, \
        and any tool results that still matter. Drop pleasantries. Write terse notes, not prose. \
        Output only the summary.";
    let req = ChatRequest {
        model: model.to_string(),
        system: sys.to_string(),
        messages: vec![Msg {
            role: Role::User,
            blocks: vec![Block::Text {
                text: format!("Summarize this conversation so far:\n\n{transcript}"),
            }],
        }],
        tools: Vec::new(),
        // Cap the summary itself; ignore the run's max_tokens which may be large.
        params: summary_params(params),
    };

    let summary = match tokio::time::timeout(SUMMARY_TIMEOUT, async {
        Ok::<_, anyhow::Error>(collect_text(provider.stream(req).await?).await)
    })
    .await
    {
        Ok(res) => res?,
        Err(_) => anyhow::bail!("summarize call timed out after {}s", SUMMARY_TIMEOUT.as_secs()),
    };
    if summary.trim().is_empty() {
        return Ok(());
    }
    agent::set_summary(&state.pool, conversation_id, summary.trim(), cutoff as i32).await?;
    tracing::info!("agent: rolled {} older messages into summary", older.len());
    Ok(())
}

/// A bounded max_tokens for the summary call (keeps it cheap regardless of run params).
fn summary_params(params: &Value) -> Value {
    // Honour a small explicit cap if set, else 800.
    let mt = param_u32(params, "summary_max_tokens", 800);
    serde_json::json!({ "max_tokens": mt })
}

/// Render the prior summary + a slice of messages as a plain-text transcript for summarizing.
fn render_transcript(prior_summary: &str, msgs: &[agent::Message]) -> String {
    let mut out = String::new();
    if !prior_summary.trim().is_empty() {
        out.push_str("[previous summary]\n");
        out.push_str(prior_summary.trim());
        out.push_str("\n\n");
    }
    for m in msgs {
        let blocks: Vec<Block> = serde_json::from_value(m.content.clone()).unwrap_or_default();
        let text: String = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.clone()),
                Block::ToolResult { content, .. } => Some(format!("[tool result] {content}")),
                Block::ToolUse { name, .. } => Some(format!("[called tool {name}]")),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            continue;
        }
        let who = match m.role.as_str() {
            "assistant" => "assistant",
            "tool" => "tool",
            _ => "user",
        };
        out.push_str(&format!("{who}: {text}\n"));
    }
    out
}

/// Drain a provider stream to its concatenated visible text (ignoring thinking/tools).
async fn collect_text(
    mut stream: futures::stream::BoxStream<'static, ProviderEvent>,
) -> String {
    let mut text = String::new();
    while let Some(ev) = stream.next().await {
        match ev {
            ProviderEvent::TextDelta(d) => text.push_str(&d),
            ProviderEvent::Error(_) => break,
            _ => {}
        }
    }
    text
}
