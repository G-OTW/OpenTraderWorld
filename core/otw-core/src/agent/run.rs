//! The agent run loop — assemble the prompt, stream provider turns to the client, dispatch
//! any tool calls, and persist each message.
//!
//! A run is a bounded loop: stream one provider turn; if it ends asking for tools and the
//! agent has a tool context, dispatch them in-process (OTW MCP), feed the results back, and
//! stream the next turn. Caps: at most [`MAX_TOOL_ITERS`] tool rounds. A client abort (SSE
//! disconnect) drops the channel and cancels the run.
//!
//! SSE protocol (one event per frame): `delta` (text), `thinking`, `tool` (a tool_use, with
//! its result once run), `done` (stop reason + tokens), `error`.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use axum::response::sse::Event;
use futures::stream::StreamExt;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use super::provider::{Block, ChatRequest, Msg, Provider, ProviderEvent, Role, StopReason};
use super::tools::ToolContext;
use crate::AppState;
use otw_store::agent;

/// Hard cap on tool-dispatch rounds per run (matches the network MCP posture).
const MAX_TOOL_ITERS: usize = 15;

/// Abort a turn when the provider sends nothing for this long (a hung upstream would
/// otherwise stall the run forever — SSE keep-alives keep the client connection open).
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Conversations with a run in flight. One run at a time per conversation: two concurrent
/// runs would interleave persisted messages (mispairing tool_use/tool_result, which
/// providers then reject on every later turn) and race the summarizer's watermark.
static ACTIVE_RUNS: OnceLock<Mutex<HashSet<Uuid>>> = OnceLock::new();

fn active_runs() -> &'static Mutex<HashSet<Uuid>> {
    ACTIVE_RUNS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Exclusive claim on a conversation for the duration of one run. Released on drop, so an
/// aborted client / crashed task can never leave the conversation locked.
pub struct RunGuard(Uuid);

impl RunGuard {
    /// Claim `conversation_id`; `None` when a run is already in flight for it.
    pub fn acquire(conversation_id: Uuid) -> Option<Self> {
        active_runs()
            .lock()
            .unwrap()
            .insert(conversation_id)
            .then_some(Self(conversation_id))
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        active_runs().lock().unwrap().remove(&self.0);
    }
}

/// Everything one run needs. `req.messages` is the assembled history; the loop mutates a
/// local copy as it appends assistant/tool turns.
pub struct RunConfig {
    pub state: AppState,
    pub conversation_id: Uuid,
    pub provider: Box<dyn Provider>,
    pub req: ChatRequest,
    pub tools: ToolContext,
    /// Non-fatal setup notices (e.g. an external MCP server that didn't answer) —
    /// surfaced as error frames before the first turn, run continues without them.
    pub warnings: Vec<String>,
    /// Exclusive claim on the conversation — held until this run's task ends.
    pub guard: RunGuard,
}

/// Drive the run to completion, streaming SSE events. Returns a stream for `Sse::new(...)`.
pub fn run(cfg: RunConfig) -> ReceiverStream<Result<Event, std::convert::Infallible>> {
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(64);
    tokio::spawn(async move {
        if let Err(()) = drive(cfg, &tx).await {
            // `Err(())` only means the client hung up; nothing more to send.
        }
    });
    ReceiverStream::new(rx)
}

type Tx = mpsc::Sender<Result<Event, std::convert::Infallible>>;

/// Returns `Err(())` if the client disconnected mid-run (caller just stops).
async fn drive(cfg: RunConfig, tx: &Tx) -> Result<(), ()> {
    let RunConfig { state, conversation_id, provider, mut req, tools, warnings, guard } = cfg;
    // Hold the conversation claim for the whole run; released on drop (any exit path).
    let _guard = guard;
    let pool = state.pool.clone();

    for w in &warnings {
        send(tx, sse_error(w)).await?;
    }

    let mut total_in = 0i32;
    let mut total_out = 0i32;
    let mut final_stop = StopReason::EndTurn;

    for iter in 0..=MAX_TOOL_ITERS {
        let mut inner = match provider.stream(req.clone()).await {
            Ok(s) => s,
            Err(e) => {
                send(tx, sse_error(&format!("{e:#}"))).await?;
                break;
            }
        };

        let mut text = String::new();
        let mut thinking = String::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut stop = StopReason::EndTurn;
        // Per-turn usage; totals accumulate across tool rounds for the final `done` frame.
        let mut turn_in = 0i32;
        let mut turn_out = 0i32;
        let mut errored = false;
        // Client hung up (Stop button / closed tab): finish persisting what streamed, then
        // stop quietly — no further frames can be delivered.
        let mut client_gone = false;

        loop {
            let ev = match tokio::time::timeout(IDLE_TIMEOUT, inner.next()).await {
                Err(_) => {
                    let _ = send(
                        tx,
                        sse_error(&format!(
                            "Provider sent nothing for {}s; giving up on this turn.",
                            IDLE_TIMEOUT.as_secs()
                        )),
                    )
                    .await;
                    errored = true;
                    break;
                }
                Ok(None) => break,
                Ok(Some(ev)) => ev,
            };
            match ev {
                ProviderEvent::TextDelta(d) => {
                    text.push_str(&d);
                    if send(tx, sse_json("delta", json!({ "text": d }))).await.is_err() {
                        client_gone = true;
                        break;
                    }
                }
                ProviderEvent::ThinkingDelta(d) => {
                    thinking.push_str(&d);
                    if send(tx, sse_json("thinking", json!({ "text": d }))).await.is_err() {
                        client_gone = true;
                        break;
                    }
                }
                ProviderEvent::ToolUse { id, name, input } => {
                    tool_uses.push((id, name, input));
                }
                ProviderEvent::Done { stop: s, usage } => {
                    turn_in += usage.input_tokens;
                    turn_out += usage.output_tokens;
                    stop = s;
                }
                ProviderEvent::Error(msg) => {
                    if send(tx, sse_error(&msg)).await.is_err() {
                        client_gone = true;
                        break;
                    }
                    errored = true;
                }
            }
        }
        // Close the upstream connection promptly (matters on abort/timeout).
        drop(inner);
        total_in += turn_in;
        total_out += turn_out;
        final_stop = stop;

        let had_text = !text.is_empty();
        // Build + persist this assistant message (thinking, text, tool_use blocks) — even a
        // partial one when the client aborted, so a stopped reply isn't lost on reload.
        let mut blocks: Vec<Block> = Vec::new();
        if !thinking.is_empty() {
            blocks.push(Block::Thinking { text: thinking });
        }
        if !text.is_empty() {
            blocks.push(Block::Text { text });
        }
        for (id, name, input) in &tool_uses {
            blocks.push(Block::ToolUse { id: id.clone(), name: name.clone(), input: input.clone() });
        }
        if !blocks.is_empty() {
            persist(&pool, conversation_id, "assistant", &blocks, turn_in, turn_out).await;
            // Append to the working history so the next turn sees this turn's tool_use.
            req.messages.push(Msg { role: Role::Assistant, blocks: blocks.clone() });
        }
        if client_gone {
            // A persisted tool_use must never dangle without its tool_result (providers
            // reject the history on the next run) — record the calls as cancelled.
            persist_cancelled_tools(&pool, conversation_id, &tool_uses).await;
            return Err(());
        }

        // Surface a refusal or a mid-generation cutoff that produced nothing, so the user isn't
        // left staring at an empty turn.
        if !errored {
            if matches!(stop, StopReason::Refusal) {
                send(tx, sse_error("The model declined to answer (content filter / refusal).")).await?;
            } else if matches!(stop, StopReason::MaxTokens) && !had_text && tool_uses.is_empty() {
                send(tx, sse_error("The reply hit the max-tokens limit before any output. Raise max tokens in agent settings.")).await?;
            }
        }

        // Decide whether to run tools and loop, or finish.
        let wants_tools = matches!(stop, StopReason::ToolUse) || !tool_uses.is_empty();
        let will_dispatch = wants_tools && !errored && tools.has_tools() && iter < MAX_TOOL_ITERS;
        if !will_dispatch {
            persist_cancelled_tools(&pool, conversation_id, &tool_uses).await;
            if wants_tools && !errored && iter == MAX_TOOL_ITERS {
                send(tx, sse_error("tool iteration limit reached; stopping.")).await?;
            }
            break;
        }

        // Dispatch each tool call, stream the chip (with result), collect tool_result blocks.
        // On a client abort mid-dispatch, stop running further tools but still persist a
        // result row for every tool_use (pending ones as cancelled) so history stays paired.
        let mut result_blocks: Vec<Block> = Vec::new();
        let mut aborted = false;
        for (id, name, input) in &tool_uses {
            if aborted {
                result_blocks.push(Block::ToolResult {
                    tool_use_id: id.clone(),
                    content: "[tool call was not run: the run ended first]".into(),
                    is_error: true,
                });
                continue;
            }
            let (output, is_error) = tools.call(&state, name, input).await;
            if send(
                tx,
                sse_json(
                    "tool",
                    json!({
                        "id": id,
                        "name": name,
                        "input": input,
                        "result": truncate_for_ui(&output),
                        "is_error": is_error,
                    }),
                ),
            )
            .await
            .is_err()
            {
                aborted = true;
            }
            result_blocks.push(Block::ToolResult {
                tool_use_id: id.clone(),
                content: output,
                is_error,
            });
        }
        // Tool results are delivered on a user-role turn (both wire formats expect this).
        persist(&pool, conversation_id, "tool", &result_blocks, 0, 0).await;
        if aborted {
            return Err(());
        }
        req.messages.push(Msg { role: Role::User, blocks: result_blocks });
        // …then loop for the next assistant turn.
    }

    send(
        tx,
        sse_json(
            "done",
            json!({
                "stop": stop_label(final_stop),
                "input_tokens": total_in,
                "output_tokens": total_out,
            }),
        ),
    )
    .await?;
    Ok(())
}

/// Record never-dispatched tool calls as errored results so the persisted history stays
/// well-formed (every tool_use paired with a tool_result). No-op when there are none.
async fn persist_cancelled_tools(
    pool: &PgPool,
    conversation_id: Uuid,
    tool_uses: &[(String, String, serde_json::Value)],
) {
    if tool_uses.is_empty() {
        return;
    }
    let results: Vec<Block> = tool_uses
        .iter()
        .map(|(id, ..)| Block::ToolResult {
            tool_use_id: id.clone(),
            content: "[tool call was not run: the run ended first]".into(),
            is_error: true,
        })
        .collect();
    persist(pool, conversation_id, "tool", &results, 0, 0).await;
}

async fn persist(
    pool: &PgPool,
    conversation_id: Uuid,
    role: &str,
    blocks: &[Block],
    input_tokens: i32,
    output_tokens: i32,
) {
    let content = serde_json::to_value(blocks).unwrap_or_else(|_| json!([]));
    if let Err(e) =
        agent::add_message(pool, conversation_id, role, &content, input_tokens, output_tokens).await
    {
        tracing::error!("agent: persisting {role} message failed: {e:#}");
    }
}

/// Send one SSE frame; `Err(())` means the client hung up.
async fn send(tx: &Tx, ev: Event) -> Result<(), ()> {
    tx.send(Ok(ev)).await.map_err(|_| ())
}

fn sse_json(event: &str, data: serde_json::Value) -> Event {
    Event::default().event(event).data(data.to_string())
}

fn sse_error(msg: &str) -> Event {
    sse_json("error", json!({ "message": msg }))
}

fn stop_label(stop: StopReason) -> &'static str {
    match stop {
        StopReason::EndTurn => "end_turn",
        StopReason::ToolUse => "tool_use",
        StopReason::MaxTokens => "max_tokens",
        StopReason::Refusal => "refusal",
        StopReason::Other => "other",
    }
}

/// Keep tool-result payloads sent to the UI bounded (the full result still goes to the model).
fn truncate_for_ui(s: &str) -> String {
    const MAX: usize = 4000;
    super::truncate(s, MAX)
}
