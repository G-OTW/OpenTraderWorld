//! Telegram transport: long polling with a BotFather token.
//!
//! `getUpdates` is a request OTW makes, held open by Telegram until something arrives.
//! That is the whole reason this is the first transport: it needs no public URL, no
//! `setWebhook`, and nothing listening on the host, so it works unchanged on the default
//! localhost-only deployment and behind any NAT.
//!
//! The bot token is in the URL path, which is how the API is designed. Everything that can
//! surface an error string therefore scrubs it first: a failed request would otherwise put
//! the credential in a log line and in the binding's status field.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{Chat, Inbound, Listener};

/// Seconds Telegram holds the poll open with nothing to say. Under the request timeout
/// below, so a normal quiet period ends as an empty batch rather than as a failure.
const POLL_SECONDS: u64 = 25;

/// Ceiling on one poll request, including the long-poll wait. Cuts a connection that
/// silently stopped delivering, which a bare long poll cannot detect.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(POLL_SECONDS + 15);

/// Telegram's hard cap on one message.
const MESSAGE_LIMIT: usize = 4096;

pub fn build(token: &str) -> (Box<dyn Listener>, Arc<dyn Chat>) {
    let api = Api { token: token.to_string() };
    (
        Box::new(Poller { api: api.clone(), offset: None, primed: false }),
        Arc::new(api),
    )
}

#[derive(Clone)]
struct Api {
    token: String,
}

impl Api {
    fn url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.token)
    }

    /// Replace the bot token wherever it appears. Applied to every error that escapes
    /// this module, because reqwest puts the request URL in its own Display output.
    fn scrub(&self, s: String) -> String {
        s.replace(&self.token, "***")
    }

    async fn call(&self, method: &str, body: Value, timeout: Duration) -> anyhow::Result<Value> {
        let resp = super::http()
            .post(self.url(method))
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("{}", self.scrub(format!("{e}"))))?;
        let status = resp.status();
        let payload: Value = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("{}", self.scrub(format!("{e}"))))?;
        if payload.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(payload.get("result").cloned().unwrap_or(Value::Null));
        }
        // Telegram explains itself in `description`; keep it, it names the fix (a revoked
        // token, a bot that was never started by this user, a chat it was removed from).
        let why = payload
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("telegram rejected the request");
        Err(anyhow::anyhow!("{} ({status})", self.scrub(why.to_string())))
    }
}

struct Poller {
    api: Api,
    /// Next update id to ask for. `None` until the first poll has fixed a starting point.
    offset: Option<i64>,
    /// Whether the backlog has been skipped.
    primed: bool,
}

#[async_trait]
impl Listener for Poller {
    async fn recv(&mut self) -> anyhow::Result<Vec<Inbound>> {
        // First poll of a fresh binding: ask for the last update only and answer none of
        // them. Telegram keeps undelivered updates for 24 hours, and replaying a day of
        // instructions the moment a binding is switched on is not a recovery, it is a
        // surprise — the user is not there to see it happen.
        if !self.primed && self.offset.is_none() {
            let result = self
                .api
                .call("getUpdates", json!({ "offset": -1, "timeout": 0 }), REQUEST_TIMEOUT)
                .await?;
            if let Some(last) = result.as_array().and_then(|a| a.last()) {
                if let Some(id) = last.get("update_id").and_then(|v| v.as_i64()) {
                    self.offset = Some(id + 1);
                }
            }
            self.primed = true;
            return Ok(Vec::new());
        }
        self.primed = true;

        let mut body = json!({
            "timeout": POLL_SECONDS,
            "allowed_updates": ["message"],
        });
        if let Some(offset) = self.offset {
            body["offset"] = json!(offset);
        }
        let result = self.api.call("getUpdates", body, REQUEST_TIMEOUT).await?;
        let updates = result.as_array().cloned().unwrap_or_default();
        let mut out = Vec::new();
        for u in &updates {
            if let Some(id) = u.get("update_id").and_then(|v| v.as_i64()) {
                // Advance past every update read, answered or not: an update we chose to
                // ignore must not come back on the next poll.
                self.offset = Some(self.offset.map_or(id + 1, |o| o.max(id + 1)));
            }
            if let Some(m) = parse_message(u.get("message")) {
                out.push(m);
            }
        }
        Ok(out)
    }

    fn cursor(&self) -> String {
        self.offset.map(|o| o.to_string()).unwrap_or_default()
    }

    fn set_cursor(&mut self, cursor: &str) {
        if let Ok(offset) = cursor.trim().parse::<i64>() {
            self.offset = Some(offset);
            // A stored cursor IS the starting point; there is no backlog to skip.
            self.primed = true;
        }
    }
}

/// Normalise one Telegram message, or `None` when it is not something a person typed.
fn parse_message(m: Option<&Value>) -> Option<Inbound> {
    let m = m?;
    let text = m.get("text").and_then(|v| v.as_str())?.to_string();
    let from = m.get("from")?;
    if from.get("is_bot").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let sender_id = from.get("id").and_then(|v| v.as_i64())?.to_string();
    let sender_label = from
        .get("username")
        .and_then(|v| v.as_str())
        .map(|u| format!("@{u}"))
        .or_else(|| from.get("first_name").and_then(|v| v.as_str()).map(str::to_string))
        .unwrap_or_default();
    let chat = m.get("chat")?;
    let chat_id = chat.get("id").and_then(|v| v.as_i64())?.to_string();
    let direct = chat.get("type").and_then(|v| v.as_str()) == Some("private");
    Some(Inbound { chat_id, sender_id, sender_label, text, direct })
}

#[async_trait]
impl Chat for Api {
    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<String> {
        let result = self
            .call(
                "sendMessage",
                json!({
                    "chat_id": chat_id,
                    "text": text,
                    "disable_web_page_preview": true,
                }),
                Duration::from_secs(20),
            )
            .await?;
        Ok(result
            .get("message_id")
            .and_then(|v| v.as_i64())
            .map(|id| id.to_string())
            .unwrap_or_default())
    }

    async fn edit(&self, chat_id: &str, message_id: &str, text: &str) -> anyhow::Result<()> {
        let id: i64 = message_id.parse().unwrap_or_default();
        self.call(
            "editMessageText",
            json!({
                "chat_id": chat_id,
                "message_id": id,
                "text": text,
                "disable_web_page_preview": true,
            }),
            Duration::from_secs(20),
        )
        .await?;
        Ok(())
    }

    async fn typing(&self, chat_id: &str) -> anyhow::Result<()> {
        self.call(
            "sendChatAction",
            json!({ "chat_id": chat_id, "action": "typing" }),
            Duration::from_secs(10),
        )
        .await?;
        Ok(())
    }

    fn limit(&self) -> usize {
        MESSAGE_LIMIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_private_message() {
        let u = json!({
            "update_id": 7,
            "message": {
                "message_id": 3,
                "from": { "id": 42, "username": "someone", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "text": "hello"
            }
        });
        let m = parse_message(u.get("message")).expect("a message");
        assert_eq!(m.sender_id, "42");
        assert_eq!(m.chat_id, "42");
        assert_eq!(m.sender_label, "@someone");
        assert_eq!(m.text, "hello");
        assert!(m.direct);
    }

    /// A group is flagged, not dropped here: the decision is the session's, one place.
    #[test]
    fn a_group_message_is_not_direct() {
        let u = json!({
            "message": {
                "from": { "id": 1, "first_name": "Ann", "is_bot": false },
                "chat": { "id": -100, "type": "supergroup" },
                "text": "hi"
            }
        });
        let m = parse_message(u.get("message")).expect("a message");
        assert!(!m.direct);
        assert_eq!(m.sender_label, "Ann");
    }

    #[test]
    fn bots_photos_and_joins_are_not_messages() {
        let bot = json!({ "message": { "from": { "id": 9, "is_bot": true }, "chat": { "id": 9, "type": "private" }, "text": "x" } });
        assert!(parse_message(bot.get("message")).is_none());
        let photo = json!({ "message": { "from": { "id": 9, "is_bot": false }, "chat": { "id": 9, "type": "private" }, "photo": [] } });
        assert!(parse_message(photo.get("message")).is_none());
        assert!(parse_message(None).is_none());
    }

    /// The token lives in the URL, so nothing that can reach a log may carry it.
    #[test]
    fn errors_never_carry_the_token() {
        let api = Api { token: "123:SECRET".into() };
        let scrubbed = api.scrub(format!("failed to POST {}", api.url("getUpdates")));
        assert!(!scrubbed.contains("SECRET"), "{scrubbed}");
        assert!(scrubbed.contains("***"));
    }
}
