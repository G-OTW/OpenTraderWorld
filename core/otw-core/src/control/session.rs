//! One chat, one task: authority, commands, and the agent run behind them.
//!
//! Each chat gets its own task so the transport's receive loop never blocks on a model
//! that is thinking, and so a write confirmation can wait for the next message *from that
//! chat* without holding up any other.
//!
//! Two rules are enforced here and nowhere else:
//!
//! - **The channel is not an identity.** Anyone can write into a chat a bot is in, so the
//!   check is on the platform's sender id against the binding's allowlist. An unknown
//!   sender gets silence, not an error: a refusal is itself information.
//! - **A chat never inherits auto-approval.** `auto_approve_writes` was ticked in an
//!   authenticated app session; it does not travel to a phone. Every write confirms here,
//!   destructive or not.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use uuid::Uuid;

use super::{Chat, Inbound};
use crate::agent::run::RunGuard;
use crate::AppState;

/// How often the in-progress reply is pushed to the platform. Every chat client rate
/// limits edits to roughly one a second, and a token-by-token stream would spend the
/// whole budget (and the bot's outbound quota) on redrawing a half-written sentence.
const EDIT_INTERVAL: Duration = Duration::from_millis(1500);

/// How long a write confirmation waits for a yes or no. Just under the run loop's own
/// 5-minute wait, so the answer is "you did not reply" rather than a silent expiry.
const CONFIRM_WAIT: Duration = Duration::from_secs(280);

/// Words that approve a pending write, lowercased. Anything else is a refusal: consent
/// has to be given, not inferred from an ambiguous reply.
const APPROVALS: &[&str] = &["yes", "y", "ok", "okay", "approve", "oui", "go"];

const HELP: &str = "Send a message and it goes to the agent with this binding's tools.\n\n\
     /new — start a fresh conversation, back on this binding's default model\n\
     /provider — list providers, /provider <n|name> to switch this chat\n\
     /model — list models, /model <n|id> to switch this chat\n\
     /whoami — show the id this chat is paired as\n\
     /help — this message\n\n\
     Writes always ask first: you will get the exact call and reply yes or no.";

/// Longest model list one message will carry. A gateway like OpenRouter offers hundreds;
/// past this the reply stops being readable on a phone and `/model <text>` is the way in.
const MODEL_PAGE: usize = 40;

pub struct ChatCx {
    pub state: AppState,
    pub binding_id: Uuid,
    pub binding_name: String,
    pub agent_id: Uuid,
    pub token_id: Uuid,
    pub chat: Arc<dyn Chat>,
}

/// Process this chat's messages one at a time, forever (the task ends when the worker
/// drops the sender, which is how a stopped binding stops its chats).
pub async fn chat_loop(cx: ChatCx, chat_id: String, mut rx: mpsc::Receiver<Inbound>) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = handle(&cx, &chat_id, msg, &mut rx).await {
            tracing::warn!("control: '{}' chat {chat_id} failed: {e:#}", cx.binding_name);
        }
    }
}

async fn handle(
    cx: &ChatCx,
    chat_id: &str,
    msg: Inbound,
    rx: &mut mpsc::Receiver<Inbound>,
) -> anyhow::Result<()> {
    let pool = &cx.state.pool;
    if !otw_store::control::allows(pool, cx.binding_id, &msg.sender_id).await? {
        return pair(cx, chat_id, &msg).await;
    }
    let text = msg.text.trim();
    match text.split_whitespace().next().unwrap_or_default() {
        "/start" | "/help" => {
            cx.chat.send(chat_id, HELP).await?;
            return Ok(());
        }
        "/whoami" => {
            let who = format!("{}\nsender id: {}", cx.binding_name, msg.sender_id);
            cx.chat.send(chat_id, &who).await?;
            return Ok(());
        }
        "/new" => {
            otw_store::control::clear_conversation(pool, cx.binding_id, chat_id).await?;
            cx.chat.send(chat_id, "New conversation.").await?;
            return Ok(());
        }
        "/provider" => {
            let arg = text.split_once(char::is_whitespace).map(|(_, r)| r.trim()).unwrap_or("");
            let reply = provider_cmd(cx, chat_id, arg).await?;
            cx.chat.send(chat_id, &reply).await?;
            return Ok(());
        }
        "/model" => {
            let arg = text.split_once(char::is_whitespace).map(|(_, r)| r.trim()).unwrap_or("");
            let reply = model_cmd(cx, chat_id, arg).await?;
            cx.chat.send(chat_id, &reply).await?;
            return Ok(());
        }
        _ => {}
    }
    answer(cx, chat_id, text, rx).await
}

/// First contact: an unknown sender is only ever answered when they present the code the
/// user just minted in Settings, and only to confirm it worked.
async fn pair(cx: &ChatCx, chat_id: &str, msg: &Inbound) -> anyhow::Result<()> {
    let pool = &cx.state.pool;
    if !otw_store::control::pairing_open(pool, cx.binding_id).await? {
        return Ok(());
    }
    let Some(code) = six_digits(&msg.text) else {
        return Ok(());
    };
    let label = if msg.sender_label.is_empty() { msg.sender_id.clone() } else { msg.sender_label.clone() };
    if otw_store::control::redeem_pair_code(pool, cx.binding_id, &code, &msg.sender_id, &label)
        .await?
    {
        // Keyed by the sender, so two pairings in a row are two notices: each one is a
        // new party that can drive the app.
        crate::security_events::alert(
            &cx.state,
            crate::security_events::CONTROL_PAIRED,
            &format!("{}:{}", crate::security_events::CONTROL_PAIRED, msg.sender_id),
            "A new chat sender was paired",
            &format!(
                "Binding: {}\nSender: {label} ({})\n\nThey can now send this instance \
                 instructions through the agent. Remove them in Settings → External control \
                 if that was not you.",
                cx.binding_name, msg.sender_id
            ),
        );
        cx.chat
            .send(chat_id, &format!("Paired with {}.\n\n{HELP}", cx.binding_name))
            .await?;
    }
    Ok(())
}

/// First run of six consecutive digits in a message, so "code 123456" and a bare number
/// both work.
fn six_digits(text: &str) -> Option<String> {
    let digits: Vec<char> = text.chars().collect();
    digits
        .windows(6)
        .find(|w| w.iter().all(char::is_ascii_digit))
        .map(|w| w.iter().collect())
}

/// The agent conversation backing this chat, created on first use with the binding's own
/// agent and token so the envelope is the binding's, not whatever default the app holds.
///
/// The binding's provider/model is the **seed**, written onto the conversation at creation
/// and never re-applied. From then on the conversation owns the choice, exactly as a chat
/// in the browser does: `/provider` and `/model` move this chat and leave the others where
/// they are. `/new` drops the conversation, so the next one starts from the binding's
/// default again.
///
/// Seeded rather than resolved per run because `agent_conversations` is already where the
/// run loop resolves a model from: the chat leg gets the same fallback as the browser
/// without a rule of its own.
async fn conversation(cx: &ChatCx, chat_id: &str) -> anyhow::Result<Uuid> {
    let pool = &cx.state.pool;
    if let Some(id) = otw_store::control::conversation_for(pool, cx.binding_id, chat_id).await? {
        if otw_store::agent::get_conversation(pool, id).await?.is_some() {
            return Ok(id);
        }
    }
    let conv =
        otw_store::agent::create_conversation(pool, cx.agent_id, Some(cx.token_id)).await?;
    let (provider_id, model) = binding_choice(cx).await?;
    if provider_id.is_some() || !model.is_empty() {
        otw_store::agent::set_conversation_model(pool, conv.id, Some(provider_id), Some(&model))
            .await?;
    }
    otw_store::control::bind_conversation(pool, cx.binding_id, chat_id, conv.id).await?;
    Ok(conv.id)
}

// ── /provider and /model ─────────────────────────────────────────────────────
//
// The person driving a binding is on a phone, not in Settings. These two commands are the
// same choice the model picker offers in the app, and they have the same scope: this chat.
// The entry is addressed by position in a freshly listed set rather than by a remembered
// menu, so there is no per-chat listing state to keep in sync and `/provider 2` means the
// same thing whenever it is typed.

/// The binding's default, used to seed a new conversation.
async fn binding_choice(cx: &ChatCx) -> anyhow::Result<(Option<Uuid>, String)> {
    let b = otw_store::control::get(&cx.state.pool, cx.binding_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("binding vanished"))?;
    Ok((b.provider_id, b.model))
}

/// What this chat is set to right now: its conversation's own override, empty while it is
/// still inheriting.
async fn chat_choice(cx: &ChatCx, conv: Uuid) -> anyhow::Result<(Option<Uuid>, String)> {
    let c = otw_store::agent::get_conversation(&cx.state.pool, conv)
        .await?
        .ok_or_else(|| anyhow::anyhow!("conversation vanished"))?;
    Ok((c.provider_id, c.model))
}

/// The provider a message from this chat would actually run on: the conversation's, else
/// the persona's, else the default agent's. `/model` needs it to know whose catalog to
/// list.
async fn effective_provider(cx: &ChatCx, conv: Uuid) -> anyhow::Result<Option<Uuid>> {
    let pool = &cx.state.pool;
    if let (Some(p), _) = chat_choice(cx, conv).await? {
        return Ok(Some(p));
    }
    if let Some(agent) = otw_store::agent::get_agent(pool, cx.agent_id).await? {
        if agent.provider_id.is_some() {
            return Ok(agent.provider_id);
        }
    }
    Ok(otw_store::agent::default_agent(pool).await?.provider_id)
}

/// Write the choice to this chat's conversation, and to nothing else.
async fn apply_choice(
    cx: &ChatCx,
    conv: Uuid,
    provider_id: Option<Option<Uuid>>,
    model: Option<&str>,
) -> anyhow::Result<()> {
    otw_store::agent::set_conversation_model(&cx.state.pool, conv, provider_id, model).await?;
    Ok(())
}

/// `/provider` lists what is configured; `/provider <n|name|kind>` switches this chat to
/// one. The conversation is created here if the person switches before saying anything, so
/// the choice has somewhere to live.
async fn provider_cmd(cx: &ChatCx, chat_id: &str, arg: &str) -> anyhow::Result<String> {
    let all = otw_store::agent::list_providers(&cx.state.pool).await?;
    let usable: Vec<_> = all.into_iter().filter(|p| p.enabled).collect();
    if usable.is_empty() {
        return Ok("No provider is configured. Add one in Settings → AI agents.".into());
    }
    let conv = conversation(cx, chat_id).await?;
    let current = effective_provider(cx, conv).await?;
    if arg.is_empty() {
        let mut out = String::from("Providers:\n");
        for (i, p) in usable.iter().enumerate() {
            let mark = if Some(p.id) == current { " ←" } else { "" };
            out.push_str(&format!("{}. {} ({}){}\n", i + 1, p.label, p.kind, mark));
        }
        out.push_str("\n/provider <number or name> to switch this chat.");
        return Ok(out);
    }
    let Some(pick) = pick_one(arg, usable.len(), |i| {
        let p = &usable[i];
        [p.label.as_str(), p.kind.as_str()]
    }) else {
        return Ok(format!("No provider matches '{arg}'. /provider to see the list."));
    };
    let p = &usable[pick];
    // A model id belongs to one vendor: switching provider clears it back to that
    // provider's default rather than carrying a name the new one has never heard of.
    apply_choice(cx, conv, Some(Some(p.id)), Some("")).await?;
    let tail = if p.default_model.is_empty() {
        "Pick a model with /model.".to_string()
    } else {
        format!("Model: {} (its default). /model to change it.", p.default_model)
    };
    Ok(format!("This chat: {} ({}). {tail}", p.label, p.kind))
}

/// `/model` lists the current provider's catalog; `/model <n|id|text>` switches this chat
/// to one. A text argument that matches exactly one id selects it, several list them back.
async fn model_cmd(cx: &ChatCx, chat_id: &str, arg: &str) -> anyhow::Result<String> {
    let conv = conversation(cx, chat_id).await?;
    let Some(provider_id) = effective_provider(cx, conv).await? else {
        return Ok("No provider is configured. Add one in Settings → AI agents.".into());
    };
    let models = match crate::agent_api::models_of(&cx.state, provider_id).await {
        Ok(m) => m,
        Err(e) => return Ok(format!("Could not list models: {}", e.message())),
    };
    if models.is_empty() {
        return Ok("This provider does not publish a model list. /model <id> to set one.".into());
    }
    let (_, current) = chat_choice(cx, conv).await?;
    if arg.is_empty() {
        return Ok(render_models(
            &models,
            &current,
            "/model <number or text> to switch this chat.",
        ));
    }
    // A number picks from the list as just rendered; the list is sorted, so the same
    // number means the same model whenever it is typed.
    if let Some(i) = index_arg(arg, models.len()) {
        apply_choice(cx, conv, None, Some(&models[i])).await?;
        return Ok(format!("This chat: {}", models[i]));
    }
    let needle = arg.to_lowercase();
    let hits: Vec<&String> = models.iter().filter(|m| m.to_lowercase().contains(&needle)).collect();
    match hits.len() {
        0 => Ok(format!("No model matches '{arg}'. /model to see the list.")),
        1 => {
            apply_choice(cx, conv, None, Some(hits[0])).await?;
            Ok(format!("This chat: {}", hits[0]))
        }
        _ => {
            let narrowed: Vec<String> = hits.into_iter().cloned().collect();
            Ok(render_models(&narrowed, &current, "/model <exact id> to pick one."))
        }
    }
}

/// A numbered list, truncated so one message stays readable on a phone.
fn render_models(models: &[String], current: &str, hint: &str) -> String {
    let mut out = String::from("Models:\n");
    for (i, m) in models.iter().take(MODEL_PAGE).enumerate() {
        let mark = if m == current { " ←" } else { "" };
        out.push_str(&format!("{}. {m}{mark}\n", i + 1));
    }
    if models.len() > MODEL_PAGE {
        out.push_str(&format!("… and {} more.\n", models.len() - MODEL_PAGE));
    }
    out.push('\n');
    out.push_str(hint);
    out
}

/// A 1-based position within `len`, or nothing when the argument is not one.
fn index_arg(arg: &str, len: usize) -> Option<usize> {
    let n: usize = arg.trim().parse().ok()?;
    (1..=len).contains(&n).then(|| n - 1)
}

/// Resolve an argument to one entry: a 1-based number, else a case-insensitive match on
/// any of the entry's names. Ambiguous text picks nothing.
fn pick_one<'a, F, I>(arg: &str, len: usize, names: F) -> Option<usize>
where
    F: Fn(usize) -> I,
    I: IntoIterator<Item = &'a str>,
{
    if let Some(i) = index_arg(arg, len) {
        return Some(i);
    }
    let needle = arg.trim().to_lowercase();
    let mut hit = None;
    for i in 0..len {
        if names(i).into_iter().any(|n| n.to_lowercase().contains(&needle)) {
            if hit.is_some() {
                return None;
            }
            hit = Some(i);
        }
    }
    hit
}

async fn answer(
    cx: &ChatCx,
    chat_id: &str,
    text: &str,
    rx: &mut mpsc::Receiver<Inbound>,
) -> anyhow::Result<()> {
    // The flag can be cleared while a worker is connected; the next message must stop,
    // not the next reconcile.
    if !super::token_is_external(&cx.state.pool, cx.token_id).await {
        cx.chat
            .send(chat_id, "This binding's access token is no longer allowed to be used from outside.")
            .await?;
        return Ok(());
    }
    let conversation_id = conversation(cx, chat_id).await?;
    let Some(guard) = RunGuard::acquire(conversation_id) else {
        cx.chat.send(chat_id, "Still working on the previous message.").await?;
        return Ok(());
    };
    let mut cfg = match crate::agent_api::prepare_run(
        &cx.state,
        conversation_id,
        text,
        None,
        guard,
    )
    .await
    {
        Ok(cfg) => cfg,
        Err(e) => {
            cx.chat.send(chat_id, e.message()).await?;
            return Ok(());
        }
    };
    // Consent given in the app does not reach a phone: every write asks here.
    cfg.tools.auto_approve_writes = false;

    let _ = cx.chat.typing(chat_id).await;
    let mut frames = crate::agent::run::run_frames(cfg);
    let mut out = Reply::new(cx, chat_id);
    while let Some(f) = frames.recv().await {
        match f.event {
            "delta" => {
                if let Some(t) = f.data.get("text").and_then(|v| v.as_str()) {
                    out.push(t);
                    out.maybe_flush().await;
                }
            }
            "tool" => {
                let name = f.data.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                out.status(&format!("· {name}"));
                out.maybe_flush().await;
            }
            "error" => {
                if let Some(m) = f.data.get("message").and_then(|v| v.as_str()) {
                    out.push(&format!("\n\n⚠ {m}\n"));
                }
            }
            "confirm" => {
                out.flush().await;
                let approved = confirm(cx, chat_id, &f.data, rx).await?;
                crate::agent::confirm::resolve(
                    conversation_id,
                    f.data.get("id").and_then(|v| v.as_str()).unwrap_or_default(),
                    approved,
                );
                out.start_new_message();
                let _ = cx.chat.typing(chat_id).await;
            }
            "done" => break,
            _ => {}
        }
    }
    out.finish().await;
    Ok(())
}

/// Put the exact call to the user and wait for a word. The rendered call is the whole
/// point: approving a summary is not approving the call.
async fn confirm(
    cx: &ChatCx,
    chat_id: &str,
    intent: &serde_json::Value,
    rx: &mut mpsc::Receiver<Inbound>,
) -> anyhow::Result<bool> {
    let method = intent.get("method").and_then(|v| v.as_str()).unwrap_or("?");
    let path = intent.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let destructive = intent.get("destructive").and_then(|v| v.as_bool()).unwrap_or(false);
    let body = match intent.get("body") {
        Some(b) if !b.is_null() => {
            let text = serde_json::to_string_pretty(b).unwrap_or_default();
            format!("\n{}", cap(&text, 1200))
        }
        _ => String::new(),
    };
    let head = if destructive { "Approve this DESTRUCTIVE write?" } else { "Approve this write?" };
    let ask = format!("{head}\n\n{method} {path}{body}\n\nReply yes or no.");
    cx.chat.send(chat_id, &ask).await?;

    // Only an allow-listed sender can answer, and only from this chat: the queue this
    // reads is the chat's own, but membership is still checked per message.
    let deadline = tokio::time::Instant::now() + CONFIRM_WAIT;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(msg)) => {
                if !otw_store::control::allows(&cx.state.pool, cx.binding_id, &msg.sender_id)
                    .await?
                {
                    continue;
                }
                let word = msg.text.trim().to_lowercase();
                let approved = APPROVALS.contains(&word.as_str());
                if !approved {
                    cx.chat.send(chat_id, "Not applied.").await?;
                }
                return Ok(approved);
            }
            // The sender is gone (binding stopped) or nobody answered: either way the
            // write does not happen.
            Ok(None) | Err(_) => return Ok(false),
        }
    }
}

/// The reply as it is being written: one platform message, edited in blocks, split into
/// more messages when it outgrows the platform's cap.
struct Reply<'a> {
    cx: &'a ChatCx,
    chat_id: &'a str,
    /// Platform id of the message currently being edited.
    message_id: Option<String>,
    text: String,
    /// What the platform is currently showing, so an unchanged render costs nothing.
    shown: String,
    status: String,
    last: Instant,
}

impl<'a> Reply<'a> {
    fn new(cx: &'a ChatCx, chat_id: &'a str) -> Self {
        Reply {
            cx,
            chat_id,
            message_id: None,
            text: String::new(),
            shown: String::new(),
            status: String::new(),
            last: Instant::now(),
        }
    }

    fn push(&mut self, t: &str) {
        self.text.push_str(t);
        self.status.clear();
    }

    fn status(&mut self, s: &str) {
        self.status = s.to_string();
    }

    /// Everything after a confirmation goes into a fresh message: the question and the
    /// answer stay readable as a sequence instead of overwriting each other.
    fn start_new_message(&mut self) {
        self.message_id = None;
        self.text.clear();
        self.shown.clear();
        self.status.clear();
    }

    fn render(&self) -> String {
        let body = self.text.trim();
        match (body.is_empty(), self.status.is_empty()) {
            (true, true) => "…".to_string(),
            (true, false) => self.status.clone(),
            (false, true) => body.to_string(),
            (false, false) => format!("{body}\n{}", self.status),
        }
    }

    async fn maybe_flush(&mut self) {
        if self.last.elapsed() >= EDIT_INTERVAL {
            self.flush().await;
        }
    }

    /// Push the current state, capped to one message: the overflow is delivered by
    /// [`finish`], because a growing text would otherwise be re-split on every edit.
    async fn flush(&mut self) {
        let full = self.render();
        let head = cap(&full, self.cx.chat.limit());
        if head == self.shown {
            return;
        }
        self.last = Instant::now();
        match &self.message_id {
            Some(id) => {
                if let Err(e) = self.cx.chat.edit(self.chat_id, id, &head).await {
                    tracing::warn!("control: edit failed: {e:#}");
                    return;
                }
            }
            None => match self.cx.chat.send(self.chat_id, &head).await {
                Ok(id) => self.message_id = Some(id),
                Err(e) => {
                    tracing::warn!("control: send failed: {e:#}");
                    return;
                }
            },
        }
        self.shown = head;
    }

    /// Land the reply: the first block replaces what is on screen, the rest follow as
    /// their own messages.
    async fn finish(&mut self) {
        self.status.clear();
        let full = self.render();
        let mut parts = split(&full, self.cx.chat.limit());
        if parts.is_empty() {
            return;
        }
        let head = parts.remove(0);
        if head != self.shown || self.message_id.is_none() {
            self.shown = head.clone();
            match &self.message_id {
                Some(id) => {
                    if let Err(e) = self.cx.chat.edit(self.chat_id, id, &head).await {
                        tracing::warn!("control: final edit failed: {e:#}");
                    }
                }
                None => {
                    if let Err(e) = self.cx.chat.send(self.chat_id, &head).await {
                        tracing::warn!("control: final send failed: {e:#}");
                    }
                }
            }
        }
        for part in parts {
            if let Err(e) = self.cx.chat.send(self.chat_id, &part).await {
                tracing::warn!("control: overflow send failed: {e:#}");
                return;
            }
        }
    }
}

/// Truncate to `max` characters with an ellipsis.
fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Split into platform-sized pieces, preferring a line break so a table or a list is not
/// cut mid-row.
fn split(text: &str, max: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest: Vec<char> = text.chars().collect();
    while !rest.is_empty() {
        if rest.len() <= max {
            parts.push(rest.iter().collect());
            break;
        }
        let window = &rest[..max];
        let cut = window.iter().rposition(|c| *c == '\n').map(|i| i + 1).unwrap_or(max);
        parts.push(rest[..cut].iter().collect());
        rest.drain(..cut);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_pairing_code_anywhere_in_the_line() {
        assert_eq!(six_digits("123456").as_deref(), Some("123456"));
        assert_eq!(six_digits("my code is 004321 thanks").as_deref(), Some("004321"));
        assert_eq!(six_digits("12345").as_deref(), None);
        assert_eq!(six_digits("no digits here").as_deref(), None);
    }

    /// Consent is given, never inferred: only the listed words approve.
    #[test]
    fn only_explicit_words_approve() {
        for w in ["yes", "Y", "ok", "GO"] {
            assert!(APPROVALS.contains(&w.to_lowercase().as_str()), "{w}");
        }
        for w in ["maybe", "sure why not", "no", "later", ""] {
            assert!(!APPROVALS.contains(&w.to_lowercase().as_str()), "{w}");
        }
    }

    /// A number addresses the list as rendered; text addresses it by name. Ambiguous text
    /// picks nothing, because silently taking the first match would switch the model the
    /// person's next question runs on.
    #[test]
    fn a_pick_is_a_position_or_an_unambiguous_name() {
        let names = ["OpenRouter", "Anthropic", "Local Ollama"];
        let by = |i: usize| [names[i]];
        assert_eq!(pick_one("2", names.len(), by), Some(1));
        assert_eq!(pick_one("anthro", names.len(), by), Some(1));
        assert_eq!(pick_one("OLLAMA", names.len(), by), Some(2));
        // "o" is in all three.
        assert_eq!(pick_one("o", names.len(), by), None);
        assert_eq!(pick_one("mistral", names.len(), by), None);
        // Out of range is not a position, and is not a name either.
        assert_eq!(pick_one("4", names.len(), by), None);
        assert_eq!(pick_one("0", names.len(), by), None);
    }

    #[test]
    fn split_prefers_line_breaks_and_keeps_everything() {
        let text = "aaaa\nbbbb\ncccc\n";
        let parts = split(text, 6);
        assert!(parts.len() > 1);
        assert_eq!(parts.concat(), text);
        for p in &parts {
            assert!(p.chars().count() <= 6, "{p:?}");
        }
        // A single run with no break still has to be cut somewhere.
        let solid = "x".repeat(25);
        let parts = split(&solid, 10);
        assert_eq!(parts.concat(), solid);
        assert_eq!(parts.len(), 3);
    }
}
