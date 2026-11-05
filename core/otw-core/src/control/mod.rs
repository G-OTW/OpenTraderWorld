//! External control — driving OTW from a chat channel.
//!
//! The notification broker pushes out; this listens back. A control binding
//! ([`otw_store::control`]) names a channel, an agent and an `mcp_tokens` row, and a worker
//! keeps a connection open to the platform so a message the owner sends becomes an agent
//! run whose tools are exactly that token's.
//!
//! **Outbound connections only.** Every transport here dials the platform (Telegram long
//! poll, and the socket gateways that will join it) rather than exposing an inbound route.
//! A self-hosted OTW listens on `127.0.0.1` by default, and requiring a public URL to drive
//! your own server would mean opening one just for the chat leg. Dialling out also removes
//! the whole class of forged-inbound attacks: there is no endpoint to find, no signature
//! scheme to get wrong, and the transport is authenticated by TLS to the platform plus the
//! bot token.
//!
//! What gates a message, in order: the global switch, the binding being enabled, the token
//! still carrying `external`, a direct chat, an allow-listed sender, and the per-binding
//! rate limit. None of it is new authorization: the permission ceiling is the token's own
//! module levels, applied by the same catalog the in-app agent goes through.

pub mod discord;
pub mod session;
pub mod slack;
pub mod telegram;

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use otw_store::control::ActiveBinding;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::AppState;

/// App setting that turns the whole feature on. Off unless the user says otherwise, like
/// the MCP endpoint's own switch: a chat channel that can drive the app is not something
/// to acquire by upgrading.
pub const ENABLED_SETTING: &str = "control_enabled";

/// How often the supervisor reconciles workers against the stored bindings. Short enough
/// that disabling a binding stops it while the user is still looking at the screen.
const RECONCILE: Duration = Duration::from_secs(15);

/// Messages one binding may send per minute before the rest are dropped. A chat is a
/// person typing: this is a runaway guard, not a throughput dial.
const RATE_PER_MINUTE: usize = 12;

/// Queue depth per chat. Beyond it the sender is answering faster than the agent can,
/// and dropping is better than growing an unbounded backlog of stale questions.
const CHAT_QUEUE: usize = 8;

/// Shared client for every transport: connection reuse plus a connect timeout, and
/// deliberately no overall request timeout — a long poll is *supposed* to hang for
/// half a minute.
pub(crate) fn http() -> &'static reqwest::Client {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("building control http client")
    })
}

/// One message arriving from a platform, in the only shape the rest of the module knows.
#[derive(Debug, Clone)]
pub struct Inbound {
    /// The conversation on the platform (a Telegram chat, a Slack channel, a DM).
    pub chat_id: String,
    /// The platform's id for the person who sent it. Authority is bound to this and to
    /// nothing else: a chat is a place, not an identity.
    pub sender_id: String,
    /// Display name at the time of sending, for the settings list. Never trusted.
    pub sender_label: String,
    pub text: String,
    /// A one-to-one chat. Group messages are ignored in v1: a group is several people
    /// writing into the prompt of an agent that may hold a write grant.
    pub direct: bool,
}

/// The receiving half of a transport. Implementations dial out and stay connected.
#[async_trait]
pub trait Listener: Send {
    /// Wait for the next batch. Returns an empty vec on an idle period — a heartbeat, not
    /// an error, so the caller can tell a quiet channel from a broken one.
    async fn recv(&mut self) -> anyhow::Result<Vec<Inbound>>;
    /// Where the transport has got to in its own stream, for persistence across restarts.
    fn cursor(&self) -> String;
    fn set_cursor(&mut self, cursor: &str);
}

/// The sending half. Separate from [`Listener`] because the chat tasks hold it while the
/// worker's own loop is blocked receiving.
#[async_trait]
pub trait Chat: Send + Sync {
    /// Post a message; returns the platform's id for it.
    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<String>;
    /// Replace the text of a message already sent. This is the only streaming any chat
    /// platform offers, and it is rate limited, so the caller flushes in blocks.
    async fn edit(&self, chat_id: &str, message_id: &str, text: &str) -> anyhow::Result<()>;
    /// Best-effort "typing…" hint. A platform without one does nothing.
    async fn typing(&self, _chat_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
    /// Hard cap on one message's length, in characters.
    fn limit(&self) -> usize;
}

/// Start the supervisor. One task for the whole feature; it owns every worker.
pub fn spawn(state: AppState) {
    tokio::spawn(async move { supervise(state).await });
}

/// What makes a running worker the same worker: change any of it and the transport has to
/// be rebuilt, so the fingerprint is compared rather than the fields.
fn fingerprint(b: &ActiveBinding) -> String {
    let secret = b.secret.as_deref().unwrap_or_default();
    format!(
        "{}|{}|{}|{}|{}",
        b.channel_kind,
        b.agent_id,
        b.token_id,
        b.config,
        // Never hold a bot token in a long-lived map; its hash answers "did it change".
        otw_store::mcp::hash_token(secret),
    )
}

async fn supervise(state: AppState) {
    // Let migrations and the rest of startup settle before dialling anything out.
    tokio::time::sleep(Duration::from_secs(8)).await;
    let mut running: HashMap<Uuid, (String, JoinHandle<()>)> = HashMap::new();
    let mut tick = tokio::time::interval(RECONCILE);
    loop {
        tick.tick().await;
        let wanted = match desired(&state).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("control: loading bindings failed: {e:#}");
                continue;
            }
        };
        let mut keep: HashMap<Uuid, String> = HashMap::new();
        for b in &wanted {
            keep.insert(b.id, fingerprint(b));
        }
        // Stop what is gone, changed, or finished on its own.
        running.retain(|id, (fp, handle)| {
            let live = keep.get(id).is_some_and(|want| want == fp) && !handle.is_finished();
            if !live {
                handle.abort();
            }
            live
        });
        for b in wanted {
            if running.contains_key(&b.id) {
                continue;
            }
            let fp = fingerprint(&b);
            let id = b.id;
            let name = b.name.clone();
            let st = state.clone();
            tracing::info!("control: starting binding '{name}' ({})", b.channel_kind);
            running.insert(id, (fp, tokio::spawn(async move { worker(st, b).await })));
        }
    }
}

/// The bindings that should be listening right now. The switch and the demo sandbox are
/// checked here rather than inside each worker, so turning the feature off drops every
/// transport at the next reconcile instead of leaving them connected but mute.
async fn desired(state: &AppState) -> anyhow::Result<Vec<ActiveBinding>> {
    if crate::demo::enabled() {
        return Ok(Vec::new());
    }
    let on = otw_store::settings::get_or(&state.pool, ENABLED_SETTING, "false").await? == "true";
    if !on {
        return Ok(Vec::new());
    }
    otw_store::control::load_active(&state.pool, &state.cipher).await
}

/// Build the transport pair for a binding, or say why it cannot listen.
fn build(b: &ActiveBinding) -> anyhow::Result<(Box<dyn Listener>, std::sync::Arc<dyn Chat>)> {
    let secret = b
        .secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("no bot credential set for this binding"))?;
    match b.channel_kind.as_str() {
        "telegram" => Ok(telegram::build(secret)),
        "slack" => slack::build(secret),
        "discord" => Ok(discord::build(secret)),
        other => Err(anyhow::anyhow!(
            "a '{other}' channel cannot receive — this binding has nothing to listen on"
        )),
    }
}

/// One binding's transport: receive, route each message to its chat's task, remember how
/// far the stream was read.
async fn worker(state: AppState, b: ActiveBinding) {
    let (mut listener, chat) = match build(&b) {
        Ok(pair) => pair,
        Err(e) => {
            let msg = format!("{e:#}");
            tracing::warn!("control: binding '{}' cannot start: {msg}", b.name);
            let _ = otw_store::control::record_result(&state.pool, b.id, false, Some(&msg)).await;
            return;
        }
    };
    match otw_store::control::get_cursor(&state.pool, b.id).await {
        Ok(c) if !c.is_empty() => listener.set_cursor(&c),
        Ok(_) => {}
        Err(e) => tracing::warn!("control: cursor load failed for '{}': {e:#}", b.name),
    }

    let mut chats: HashMap<String, mpsc::Sender<Inbound>> = HashMap::new();
    let mut rate = Window::new(RATE_PER_MINUTE, Duration::from_secs(60));
    let mut backoff = Duration::from_secs(2);
    let mut healthy = false;
    loop {
        let batch = match listener.recv().await {
            Ok(batch) => {
                if !healthy {
                    healthy = true;
                    let _ = otw_store::control::record_result(&state.pool, b.id, true, None).await;
                }
                backoff = Duration::from_secs(2);
                batch
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::warn!("control: '{}' receive failed: {msg}", b.name);
                let _ =
                    otw_store::control::record_result(&state.pool, b.id, false, Some(&msg)).await;
                healthy = false;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        if batch.is_empty() {
            continue;
        }
        for msg in batch {
            // A group chat is several people writing into one agent's prompt. Ignored
            // before anything else looks at the text.
            if !msg.direct || msg.text.trim().is_empty() {
                continue;
            }
            if !rate.allow() {
                tracing::warn!("control: '{}' rate limit hit, dropping message", b.name);
                continue;
            }
            let tx = chats.entry(msg.chat_id.clone()).or_insert_with(|| {
                let (tx, rx) = mpsc::channel(CHAT_QUEUE);
                let cx = session::ChatCx {
                    state: state.clone(),
                    binding_id: b.id,
                    binding_name: b.name.clone(),
                    agent_id: b.agent_id,
                    token_id: b.token_id,
                    chat: chat.clone(),
                };
                let chat_id = msg.chat_id.clone();
                tokio::spawn(async move { session::chat_loop(cx, chat_id, rx).await });
                tx
            });
            // A full queue means the person is typing faster than the agent answers;
            // dropping the newest keeps the ones already being worked on.
            if tx.try_send(msg).is_err() {
                tracing::warn!("control: '{}' chat queue full, dropping message", b.name);
            }
        }
        chats.retain(|_, tx| !tx.is_closed());
        let cursor = listener.cursor();
        if !cursor.is_empty() {
            if let Err(e) = otw_store::control::set_cursor(&state.pool, b.id, &cursor).await {
                tracing::warn!("control: cursor save failed for '{}': {e:#}", b.name);
            }
        }
    }
}

/// Fixed-window counter. Coarse on purpose: it exists to stop a loop, not to shape traffic.
struct Window {
    max: usize,
    span: Duration,
    start: Instant,
    used: usize,
}

impl Window {
    fn new(max: usize, span: Duration) -> Self {
        Window { max, span, start: Instant::now(), used: 0 }
    }
    fn allow(&mut self) -> bool {
        if self.start.elapsed() >= self.span {
            self.start = Instant::now();
            self.used = 0;
        }
        if self.used >= self.max {
            return false;
        }
        self.used += 1;
        true
    }
}

/// Whether the token behind a binding may still be reached from outside. Checked again at
/// message time, not only when the worker started: clearing the flag has to stop the next
/// message, not the next reconcile.
pub(crate) async fn token_is_external(pool: &PgPool, token_id: Uuid) -> bool {
    match otw_store::mcp::get_token(pool, token_id).await {
        Ok(Some(t)) => t.external && !t.is_expired(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_caps_then_reopens() {
        let mut w = Window::new(2, Duration::from_millis(30));
        assert!(w.allow());
        assert!(w.allow());
        assert!(!w.allow());
        std::thread::sleep(Duration::from_millis(40));
        assert!(w.allow());
    }
}
