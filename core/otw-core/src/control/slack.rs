//! Slack transport: Socket Mode.
//!
//! Socket Mode is Slack's answer to the same problem Telegram's long poll solves: the app
//! opens the connection, so no public URL, no request-signing scheme and nothing listening
//! on the host. `apps.connections.open` hands out a short-lived WebSocket URL, and events
//! arrive as envelopes that must be acknowledged by id.
//!
//! Slack needs **two** tokens and they do different jobs: an app-level token (`xapp-`)
//! opens the socket, a bot token (`xoxb-`) posts the replies. Both go in the binding's one
//! credential field and are told apart by their prefix, which is part of Slack's published
//! format. The alternative, a second sealed field on the binding, would leak Slack's shape
//! into the schema for every other platform.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use super::{Chat, Inbound, Listener};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Return from `recv` after this long with nothing to report, so the worker can record
/// health and persist state instead of parking forever inside one call.
const IDLE: Duration = Duration::from_secs(30);

/// Slack renders long messages badly well before its API limit; this is the block cap,
/// which is the size a reply should be split at anyway.
const MESSAGE_LIMIT: usize = 3000;

pub fn build(secret: &str) -> anyhow::Result<(Box<dyn Listener>, Arc<dyn Chat>)> {
    let (app_token, bot_token) = split_tokens(secret)?;
    Ok((
        Box::new(Socketeer { app_token, ws: None }),
        Arc::new(Api { bot_token }),
    ))
}

/// Pull the app-level and bot tokens out of one field. Order does not matter; the prefixes
/// do, and a missing one is named rather than discovered later as a 401.
fn split_tokens(secret: &str) -> anyhow::Result<(String, String)> {
    let mut app = None;
    let mut bot = None;
    for part in secret.split([' ', '\n', '\r', '\t', ',', ';']) {
        let p = part.trim();
        if p.starts_with("xapp-") {
            app = Some(p.to_string());
        } else if p.starts_with("xoxb-") {
            bot = Some(p.to_string());
        }
    }
    match (app, bot) {
        (Some(app), Some(bot)) => Ok((app, bot)),
        (None, _) => Err(anyhow::anyhow!(
            "no app-level token: Slack needs an 'xapp-' token (Socket Mode) as well as the \
             'xoxb-' bot token"
        )),
        (_, None) => Err(anyhow::anyhow!(
            "no bot token: Slack needs an 'xoxb-' token to post replies as well as the \
             'xapp-' app-level token"
        )),
    }
}

struct Socketeer {
    app_token: String,
    ws: Option<Socket>,
}

impl Socketeer {
    /// Ask Slack for a socket URL and connect. Called on the first receive and after every
    /// disconnect: the URL is single-use and short-lived by design.
    async fn connect(&mut self) -> anyhow::Result<()> {
        let resp: Value = super::http()
            .post("https://slack.com/api/apps.connections.open")
            .bearer_auth(&self.app_token)
            .timeout(Duration::from_secs(20))
            .send()
            .await?
            .json()
            .await?;
        if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let why = resp.get("error").and_then(|v| v.as_str()).unwrap_or("connection refused");
            return Err(anyhow::anyhow!("slack: {why}"));
        }
        let url = resp
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("slack returned no socket url"))?;
        let (ws, _) = connect_async(url).await?;
        self.ws = Some(ws);
        Ok(())
    }
}

#[async_trait]
impl Listener for Socketeer {
    async fn recv(&mut self) -> anyhow::Result<Vec<Inbound>> {
        if self.ws.is_none() {
            self.connect().await?;
        }
        let ws = self.ws.as_mut().expect("just connected");
        let deadline = tokio::time::Instant::now() + IDLE;
        let mut out = Vec::new();
        loop {
            let frame = match tokio::time::timeout_at(deadline, ws.next()).await {
                Err(_) => return Ok(out), // idle: healthy, nothing to say
                Ok(None) | Ok(Some(Ok(Message::Close(_)))) => {
                    self.ws = None;
                    return Err(anyhow::anyhow!("slack closed the socket"));
                }
                Ok(Some(Err(e))) => {
                    self.ws = None;
                    return Err(anyhow::anyhow!("slack socket: {e}"));
                }
                Ok(Some(Ok(f))) => f,
            };
            let text = match frame {
                Message::Text(t) => t.to_string(),
                Message::Ping(p) => {
                    let _ = ws.send(Message::Pong(p)).await;
                    continue;
                }
                _ => continue,
            };
            let Ok(env) = serde_json::from_str::<Value>(&text) else { continue };
            // Acknowledge first: an unacked envelope is redelivered, and a redelivered
            // instruction is an instruction obeyed twice.
            if let Some(id) = env.get("envelope_id").and_then(|v| v.as_str()) {
                let _ = ws.send(Message::Text(json!({ "envelope_id": id }).to_string().into())).await;
            }
            match env.get("type").and_then(|v| v.as_str()) {
                // Slack rotates sockets on its own schedule; a reconnect is routine.
                Some("disconnect") => {
                    self.ws = None;
                    return Ok(out);
                }
                Some("events_api") => {
                    if let Some(m) = parse_event(env.get("payload")) {
                        out.push(m);
                    }
                }
                _ => {}
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }

    /// Socket Mode has no resumable position: Slack redelivers what was never acked.
    fn cursor(&self) -> String {
        String::new()
    }
    fn set_cursor(&mut self, _cursor: &str) {}
}

/// One events_api payload, when it is a person typing in a DM.
fn parse_event(payload: Option<&Value>) -> Option<Inbound> {
    let event = payload?.get("event")?;
    if event.get("type").and_then(|v| v.as_str()) != Some("message") {
        return None;
    }
    // Edits, deletions, joins and the bot's own posts all arrive as "message".
    if event.get("subtype").is_some() || event.get("bot_id").is_some() {
        return None;
    }
    let text = event.get("text").and_then(|v| v.as_str())?.to_string();
    let sender_id = event.get("user").and_then(|v| v.as_str())?.to_string();
    let chat_id = event.get("channel").and_then(|v| v.as_str())?.to_string();
    let direct = event.get("channel_type").and_then(|v| v.as_str()) == Some("im");
    let sender_label = payload
        .and_then(|p| p.get("authorizations"))
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.get("team_id"))
        .and_then(|v| v.as_str())
        .map(|team| format!("{team}/{sender_id}"))
        .unwrap_or_else(|| sender_id.clone());
    Some(Inbound { chat_id, sender_id, sender_label, text, direct })
}

struct Api {
    bot_token: String,
}

impl Api {
    async fn call(&self, method: &str, body: Value) -> anyhow::Result<Value> {
        let resp: Value = super::http()
            .post(format!("https://slack.com/api/{method}"))
            .bearer_auth(&self.bot_token)
            .timeout(Duration::from_secs(20))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(resp);
        }
        let why = resp.get("error").and_then(|v| v.as_str()).unwrap_or("slack rejected the call");
        Err(anyhow::anyhow!("slack {method}: {why}"))
    }
}

#[async_trait]
impl Chat for Api {
    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<String> {
        let r = self
            .call("chat.postMessage", json!({ "channel": chat_id, "text": text }))
            .await?;
        // `ts` is both the message id and its timestamp; chat.update takes it back.
        Ok(r.get("ts").and_then(|v| v.as_str()).unwrap_or_default().to_string())
    }

    async fn edit(&self, chat_id: &str, message_id: &str, text: &str) -> anyhow::Result<()> {
        self.call(
            "chat.update",
            json!({ "channel": chat_id, "ts": message_id, "text": text }),
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
    fn tokens_are_told_apart_by_prefix() {
        let (app, bot) = split_tokens("xoxb-222 xapp-111").expect("both");
        assert_eq!(app, "xapp-111");
        assert_eq!(bot, "xoxb-222");
        let (app, bot) = split_tokens("xapp-1\nxoxb-2").expect("both");
        assert_eq!((app.as_str(), bot.as_str()), ("xapp-1", "xoxb-2"));
    }

    /// A missing half must name which half, or the user retries the same paste.
    #[test]
    fn a_missing_token_says_which_one() {
        let e = split_tokens("xoxb-only").unwrap_err().to_string();
        assert!(e.contains("app-level"), "{e}");
        let e = split_tokens("xapp-only").unwrap_err().to_string();
        assert!(e.contains("bot token"), "{e}");
    }

    #[test]
    fn reads_a_direct_message() {
        let payload = json!({
            "event": {
                "type": "message",
                "channel": "D123",
                "channel_type": "im",
                "user": "U99",
                "text": "hello"
            },
            "authorizations": [{ "team_id": "T1" }]
        });
        let m = parse_event(Some(&payload)).expect("a message");
        assert_eq!(m.chat_id, "D123");
        assert_eq!(m.sender_id, "U99");
        assert_eq!(m.sender_label, "T1/U99");
        assert!(m.direct);
    }

    /// Slack reports edits, joins and the bot's own posts as messages too.
    #[test]
    fn echoes_and_edits_are_not_messages() {
        for e in [
            json!({ "event": { "type": "message", "channel": "D1", "channel_type": "im", "user": "U1", "text": "x", "bot_id": "B1" } }),
            json!({ "event": { "type": "message", "subtype": "message_changed", "channel": "D1", "channel_type": "im", "user": "U1", "text": "x" } }),
            json!({ "event": { "type": "reaction_added", "user": "U1" } }),
        ] {
            assert!(parse_event(Some(&e)).is_none(), "{e}");
        }
        let channel = json!({ "event": { "type": "message", "channel": "C1", "channel_type": "channel", "user": "U1", "text": "x" } });
        assert!(!parse_event(Some(&channel)).expect("parsed").direct);
    }
}
