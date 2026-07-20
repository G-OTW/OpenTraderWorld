//! Provider abstraction — one trait, two wire-format adapters.
//!
//! The rest of the agent (loop, storage, API) speaks a single **normalized** message model
//! (`role` + content blocks). Each adapter translates that model to and from its provider's
//! wire format, so adding a future native adapter (Gemini, Bedrock…) is one new file.
//!
//! No vendor is privileged here: nothing defaults to a model or a provider. The two adapters
//! exist only because Anthropic's Messages API and the OpenAI-compatible /chat/completions
//! endpoint are different wire formats.

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

/// A content block in the normalized model. Stored as JSONB, streamed to the client, and
/// translated by adapters in both directions. `tool_use`/`tool_result` are carried now so
/// Phase 2 (MCP tools) is purely additive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text { text: String },
    /// Extended-thinking / reasoning text (Anthropic thinking, some OpenAI-compat reasoning).
    Thinking { text: String },
    /// A tool call requested by the model (Phase 2).
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// The result of a tool call, fed back to the model (Phase 2).
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// A normalized message: a role plus its content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub role: Role,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A tool definition passed to the provider (name + JSON schema). Empty in Phases 0–1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// One request to a provider. Params (max_tokens, reasoning knobs…) ride in `params` so no
/// provider-specific field is ever hardcoded — some models reject `temperature`, etc.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<Msg>,
    /// Tool definitions advertised to the provider (empty = pure chat).
    pub tools: Vec<ToolDef>,
    pub params: serde_json::Value,
}

/// Token usage reported by the provider at the end of a turn.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// Why the provider stopped generating this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished its turn (no tool call pending).
    EndTurn,
    /// The model wants one or more tools run before continuing (Phase 2).
    ToolUse,
    /// Hit the max token limit mid-generation.
    MaxTokens,
    /// The model declined (content filter / refusal).
    Refusal,
    Other,
}

/// A single streamed event from a provider adapter.
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    /// Incremental visible-text delta.
    TextDelta(String),
    /// Incremental thinking/reasoning delta.
    ThinkingDelta(String),
    /// A completed tool-use block (adapters accumulate partial JSON, then emit once).
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Terminal event: how the turn ended and how many tokens it cost.
    Done { stop: StopReason, usage: Usage },
    /// A provider-side error surfaced mid-stream (HTTP or protocol).
    Error(String),
}

/// The wire-format adapter contract.
#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream(
        &self,
        req: ChatRequest,
    ) -> anyhow::Result<BoxStream<'static, ProviderEvent>>;

    /// List the model ids the provider exposes (drives the in-chat model picker).
    /// Best-effort: Err when the provider doesn't implement a model-list endpoint.
    async fn models(&self) -> anyhow::Result<Vec<String>>;
}

/// A `param` accessor: reads a number from `params`, falling back to `default`.
pub fn param_u32(params: &serde_json::Value, key: &str, default: u32) -> u32 {
    params.get(key).and_then(|v| v.as_u64()).map(|n| n as u32).unwrap_or(default)
}
