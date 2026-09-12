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

/// Marker inside the provider error that means "this model cannot call tools at all"
/// (see `friendly_http_error`). Matched as a substring: the adapters wrap it in context.
const NO_TOOL_SUPPORT: &str = "does not support tool calling";

/// Said once when the turn is replayed with the tools withdrawn.
const TOOLLESS_NOTICE: &str = "This model cannot call tools; answering without them. Memory, \
     skills and your data are unavailable in this conversation until you switch model.";

/// Shown to the user when the tool budget runs out — the run is not failing, it is landing.
const TOOL_LIMIT_NOTICE: &str =
    "Tool-call limit reached for this message; wrapping up with what was gathered.";

/// Handed to the model with the tools withdrawn, so the last turn is a conclusion rather
/// than a truncated exploration.
const WRAP_UP: &str = "You have reached the tool-call limit for this message: no further tool \
     calls are possible. Answer now with what you already gathered. Report ONLY what a tool \
     actually returned in this conversation — mark everything else \"not checked\" rather than \
     inferring it, list what is missing, and say what you would do next.";

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
    ///
    /// The guard is built AFTER the lock is released, and never inside the same expression:
    /// `then_some(Self(id))` would construct one eagerly and drop it while the `MutexGuard`
    /// is still alive, so `Drop` would re-lock the same non-reentrant mutex on the same
    /// thread — a self-deadlock that holds the lock forever and takes the whole API down.
    pub fn acquire(conversation_id: Uuid) -> Option<Self> {
        let claimed = {
            let mut runs = active_runs().lock().unwrap();
            runs.insert(conversation_id)
        };
        claimed.then(|| Self(conversation_id))
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

    let mut iter = 0usize;
    // One-shot: a model that cannot take tools gets the turn replayed without them.
    let mut retried_toolless = false;
    loop {
        // Every round re-sends the whole conversation, so a tool payload read once is paid for
        // again on each later turn. Older ones are trimmed to their head before the turn goes
        // out — the model has already taken what it needed, and it can call the tool again.
        trim_old_tool_results(&mut req.messages);
        let mut inner = match provider.stream(req.clone()).await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("{e:#}");
                // A model with no tool support refuses the whole request, so the chat is dead
                // even with no data tools attached — the memory and skill tools are always
                // offered. Drop them and answer anyway: a plain conversation is what the user
                // asked for, and losing the tools is worth saying once, not failing over.
                if !retried_toolless && !req.tools.is_empty() && msg.contains(NO_TOOL_SUPPORT) {
                    retried_toolless = true;
                    req.tools.clear();
                    send(tx, sse_error(TOOLLESS_NOTICE)).await?;
                    continue;
                }
                send(tx, sse_error(&msg)).await?;
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
        // This model refuses tool definitions outright: replay the turn without them.
        let mut toolless_retry = false;

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
                    // The adapters deliver an HTTP failure as an event on a successful
                    // stream, so this — not the `Err` arm above — is where a model that
                    // cannot take tools shows up. Swallow the frame and replay the turn
                    // without them; the user gets one notice instead of a dead chat.
                    if !retried_toolless && !req.tools.is_empty() && msg.contains(NO_TOOL_SUPPORT)
                    {
                        toolless_retry = true;
                        errored = true;
                        break;
                    }
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
        if toolless_retry {
            retried_toolless = true;
            req.tools.clear();
            send(tx, sse_error(TOOLLESS_NOTICE)).await?;
            continue;
        }
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
        let tools_offered = !req.tools.is_empty();
        let will_dispatch =
            wants_tools && !errored && tools.has_tools() && tools_offered && iter < MAX_TOOL_ITERS;
        if !will_dispatch {
            let cancelled = cancelled_blocks(&tool_uses);
            if !cancelled.is_empty() {
                persist(&pool, conversation_id, "tool", &cancelled, 0, 0).await;
            }
            // Cap reached with the model still calling tools: cutting here leaves the user a
            // half-written answer. Spend one more turn with the tools withdrawn so the run
            // ends on a conclusion drawn from what was actually gathered.
            if wants_tools && !errored && tools_offered && iter >= MAX_TOOL_ITERS {
                send(tx, sse_error(TOOL_LIMIT_NOTICE)).await?;
                req.tools.clear();
                let mut blocks = cancelled;
                blocks.push(Block::Text { text: WRAP_UP.into() });
                req.messages.push(Msg { role: Role::User, blocks });
                iter += 1;
                continue;
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
            // R5: a write waits for the user. The chip carries the exact call; nothing runs
            // until it is approved, and a decline is reported to the model as a refusal
            // rather than an error it should route around.
            let (output, is_error) = match tools.confirm_required(name, input) {
                Some(intent) => {
                    let pending = super::confirm::register(conversation_id, id);
                    if send(tx, sse_json("confirm", intent.to_json(id))).await.is_err() {
                        // Client gone: the user can no longer answer, so the write does not
                        // happen. Persist a paired result and stop.
                        aborted = true;
                        (
                            super::confirm::declined_message(
                                &intent,
                                super::confirm::Decision::TimedOut,
                            ),
                            true,
                        )
                    } else {
                        match pending.wait().await {
                            super::confirm::Decision::Approved => {
                                tools.call(&state, name, input).await
                            }
                            // A refusal and a timeout are told apart: reported as one, the
                            // model reads a decision as a failed request and retries it.
                            d => (super::confirm::declined_message(&intent, d), true),
                        }
                    }
                }
                None => tools.call(&state, name, input).await,
            };
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
        iter += 1;
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

/// Messages left untouched at the tail of the history (roughly the last two rounds): the
/// results the model is actually working from right now.
const VERBATIM_TAIL: usize = 4;
/// How much of an older tool result survives. A module catalog is ~11 000 characters and a
/// bars page is longer; keeping the head is enough to remember what the call returned.
const OLD_TOOL_RESULT_CAP: usize = 1500;

/// Trim tool payloads outside the recent tail, in place. Only what goes over the wire is
/// affected — the persisted transcript keeps every result in full, so the UI and the export
/// still show what the tool really said.
fn trim_old_tool_results(msgs: &mut [Msg]) {
    let cut = msgs.len().saturating_sub(VERBATIM_TAIL);
    for m in msgs[..cut].iter_mut() {
        for b in m.blocks.iter_mut() {
            let Block::ToolResult { content, .. } = b else { continue };
            if content.len() <= OLD_TOOL_RESULT_CAP {
                continue;
            }
            let dropped = content.len() - OLD_TOOL_RESULT_CAP;
            *content = format!(
                "{}\n[trimmed: {dropped} more characters of an earlier tool result. Call the \
                 tool again if you need the rest.]",
                super::truncate(content, OLD_TOOL_RESULT_CAP)
            );
        }
    }
}

/// Errored results for tool calls that were never dispatched, so the history stays
/// well-formed (every tool_use paired with a tool_result — providers reject it otherwise).
fn cancelled_blocks(tool_uses: &[(String, String, serde_json::Value)]) -> Vec<Block> {
    tool_uses
        .iter()
        .map(|(id, ..)| Block::ToolResult {
            tool_use_id: id.clone(),
            content: "[tool call was not run: the run ended first]".into(),
            is_error: true,
        })
        .collect()
}

/// Persist those cancelled results. No-op when there are none.
async fn persist_cancelled_tools(
    pool: &PgPool,
    conversation_id: Uuid,
    tool_uses: &[(String, String, serde_json::Value)],
) {
    if tool_uses.is_empty() {
        return;
    }
    persist(pool, conversation_id, "tool", &cancelled_blocks(tool_uses), 0, 0).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Two concurrent claims on one conversation: the second must simply say no. It used to
    /// self-deadlock instead (a guard built eagerly inside the locked expression, dropped
    /// while the lock was still held), which parked the thread holding the mutex forever and
    /// took every later request down with it. If this test ever hangs, that is back.
    #[test]
    fn second_claim_is_refused_without_deadlocking() {
        let conv = Uuid::new_v4();
        let first = RunGuard::acquire(conv).expect("first claim");
        assert!(RunGuard::acquire(conv).is_none(), "a second run must be refused");
        assert!(RunGuard::acquire(conv).is_none(), "and stay refused");
        drop(first);
        // Released: the conversation is claimable again.
        assert!(RunGuard::acquire(conv).is_some());
    }

    /// Only the tail keeps its payloads; older ones shrink to a head plus a note saying how to
    /// get the rest. The transcript is untouched — this is a wire-format economy, not history
    /// rewriting.
    #[test]
    fn old_tool_results_are_trimmed_but_the_tail_is_not() {
        let big = "x".repeat(20_000);
        let result = |s: &str| Msg {
            role: Role::User,
            blocks: vec![Block::ToolResult {
                tool_use_id: "t".into(),
                content: s.to_string(),
                is_error: false,
            }],
        };
        let mut msgs: Vec<Msg> = (0..6).map(|_| result(&big)).collect();
        trim_old_tool_results(&mut msgs);
        let len = |m: &Msg| match &m.blocks[0] {
            Block::ToolResult { content, .. } => content.len(),
            _ => unreachable!(),
        };
        assert!(len(&msgs[0]) < OLD_TOOL_RESULT_CAP + 200, "old result not trimmed");
        assert!(len(&msgs[1]) < OLD_TOOL_RESULT_CAP + 200);
        for m in &msgs[2..] {
            assert_eq!(len(m), big.len(), "the recent tail must stay verbatim");
        }
    }
}
