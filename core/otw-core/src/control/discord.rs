//! Discord transport: the bot gateway.
//!
//! Same shape as the other two: OTW dials the gateway, so there is no interactions URL to
//! publish and no Ed25519 signature check to get wrong. The gateway is chattier than the
//! others — it expects a heartbeat on the interval it names, and it will close the socket
//! if one is missed — so the receive loop doubles as the heartbeat timer.
//!
//! Only the `DIRECT_MESSAGES` intent is requested. Reading message content in a server is
//! a privileged intent that has to be justified to Discord; a DM to the bot is not, and a
//! DM is the only thing a binding answers anyway.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use super::{Chat, Inbound, Listener};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const API: &str = "https://discord.com/api/v10";

/// `DIRECT_MESSAGES` (1 << 12). Nothing else is asked for: an intent is a standing request
/// for data, and this binding only ever reads a DM addressed to its own bot.
const INTENTS: u64 = 1 << 12;

/// Return from `recv` after this long with nothing to report. Shorter than the usual
/// heartbeat interval, so the timer below is driven by the heartbeat, not by this.
const IDLE: Duration = Duration::from_secs(20);

/// Discord's hard cap on one message.
const MESSAGE_LIMIT: usize = 2000;

pub fn build(token: &str) -> (Box<dyn Listener>, Arc<dyn Chat>) {
    let api = Api { token: token.to_string() };
    (
        Box::new(Gateway { token: token.to_string(), ws: None, seq: None, beat: None }),
        Arc::new(api),
    )
}

struct Gateway {
    token: String,
    ws: Option<Socket>,
    /// Last sequence number seen, echoed in every heartbeat.
    seq: Option<u64>,
    /// When the next heartbeat is due, and how often after that.
    beat: Option<(tokio::time::Instant, Duration)>,
}

impl Gateway {
    async fn connect(&mut self) -> anyhow::Result<()> {
        let resp: Value = super::http()
            .get(format!("{API}/gateway"))
            .timeout(Duration::from_secs(20))
            .send()
            .await?
            .json()
            .await?;
        let url = resp
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("discord returned no gateway url"))?;
        let (mut ws, _) = connect_async(format!("{url}/?v=10&encoding=json")).await?;
        // Identify straight away: the gateway closes an unidentified socket, and the
        // Hello it sends first only carries the heartbeat interval.
        let identify = json!({
            "op": 2,
            "d": {
                "token": self.token,
                "intents": INTENTS,
                "properties": { "os": "linux", "browser": "opentraderworld", "device": "opentraderworld" },
            }
        });
        ws.send(Message::Text(identify.to_string().into())).await?;
        self.ws = Some(ws);
        self.seq = None;
        self.beat = None;
        Ok(())
    }

    async fn heartbeat(&mut self) -> anyhow::Result<()> {
        let seq = self.seq;
        if let Some(ws) = self.ws.as_mut() {
            ws.send(Message::Text(json!({ "op": 1, "d": seq }).to_string().into())).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl Listener for Gateway {
    async fn recv(&mut self) -> anyhow::Result<Vec<Inbound>> {
        if self.ws.is_none() {
            self.connect().await?;
        }
        let idle_at = tokio::time::Instant::now() + IDLE;
        let mut out = Vec::new();
        loop {
            // Whichever comes first: a frame, the heartbeat falling due, or the idle
            // return. The heartbeat has to win, or the gateway drops us mid-wait.
            let wake = match self.beat {
                Some((at, _)) => at.min(idle_at),
                None => idle_at,
            };
            let frame = {
                let ws = self.ws.as_mut().expect("connected above");
                match tokio::time::timeout_at(wake, ws.next()).await {
                    Err(_) => {
                        // Deadline: heartbeat if that is what fell due, then decide.
                        if let Some((at, every)) = self.beat {
                            if at <= tokio::time::Instant::now() {
                                self.beat = Some((tokio::time::Instant::now() + every, every));
                                self.heartbeat().await?;
                                if tokio::time::Instant::now() < idle_at {
                                    continue;
                                }
                            }
                        }
                        return Ok(out);
                    }
                    Ok(None) | Ok(Some(Ok(Message::Close(_)))) => {
                        self.ws = None;
                        return Err(anyhow::anyhow!("discord closed the gateway"));
                    }
                    Ok(Some(Err(e))) => {
                        self.ws = None;
                        return Err(anyhow::anyhow!("discord gateway: {e}"));
                    }
                    Ok(Some(Ok(f))) => f,
                }
            };
            let text = match frame {
                Message::Text(t) => t.to_string(),
                Message::Ping(p) => {
                    if let Some(ws) = self.ws.as_mut() {
                        let _ = ws.send(Message::Pong(p)).await;
                    }
                    continue;
                }
                _ => continue,
            };
            let Ok(ev) = serde_json::from_str::<Value>(&text) else { continue };
            if let Some(s) = ev.get("s").and_then(|v| v.as_u64()) {
                self.seq = Some(s);
            }
            match ev.get("op").and_then(|v| v.as_u64()) {
                // Hello: start the heartbeat on the interval it names.
                Some(10) => {
                    let ms = ev
                        .get("d")
                        .and_then(|d| d.get("heartbeat_interval"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(41_250);
                    let every = Duration::from_millis(ms);
                    self.beat = Some((tokio::time::Instant::now() + every, every));
                }
                // The gateway asking for one out of band.
                Some(1) => self.heartbeat().await?,
                // Heartbeat ack.
                Some(11) => {}
                // Reconnect / invalid session: drop the socket and dial again.
                Some(7) | Some(9) => {
                    self.ws = None;
                    return Ok(out);
                }
                Some(0) => {
                    if ev.get("t").and_then(|v| v.as_str()) == Some("MESSAGE_CREATE") {
                        if let Some(m) = parse_message(ev.get("d")) {
                            out.push(m);
                        }
                    }
                }
                _ => {}
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
    }

    /// The session is resumable in principle, but a resume needs the session id and the
    /// socket URL the gateway handed out, which do not survive a process restart. A fresh
    /// identify is the honest recovery.
    fn cursor(&self) -> String {
        String::new()
    }
    fn set_cursor(&mut self, _cursor: &str) {}
}

/// One MESSAGE_CREATE, when it is a person writing to the bot directly.
fn parse_message(d: Option<&Value>) -> Option<Inbound> {
    let d = d?;
    let author = d.get("author")?;
    if author.get("bot").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let text = d.get("content").and_then(|v| v.as_str())?.to_string();
    if text.is_empty() {
        return None;
    }
    let sender_id = author.get("id").and_then(|v| v.as_str())?.to_string();
    let sender_label = author
        .get("global_name")
        .and_then(|v| v.as_str())
        .or_else(|| author.get("username").and_then(|v| v.as_str()))
        .unwrap_or(&sender_id)
        .to_string();
    let chat_id = d.get("channel_id").and_then(|v| v.as_str())?.to_string();
    // A DM has no guild: that is the whole test.
    let direct = d.get("guild_id").is_none_or(|v| v.is_null());
    Some(Inbound { chat_id, sender_id, sender_label, text, direct })
}

struct Api {
    token: String,
}

impl Api {
    async fn request(
        &self,
        method: reqwest::Method,
        path: String,
        body: Option<Value>,
    ) -> anyhow::Result<Value> {
        let mut req = super::http()
            .request(method, format!("{API}{path}"))
            // Discord's scheme: the literal word "Bot", then the token.
            .header(reqwest::header::AUTHORIZATION, format!("Bot {}", self.token))
            .timeout(Duration::from_secs(20));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let payload: Value = resp.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(payload);
        }
        let why = payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("discord rejected the call");
        Err(anyhow::anyhow!("discord ({status}): {why}"))
    }
}

#[async_trait]
impl Chat for Api {
    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<String> {
        let r = self
            .request(
                reqwest::Method::POST,
                format!("/channels/{chat_id}/messages"),
                Some(json!({ "content": text })),
            )
            .await?;
        Ok(r.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string())
    }

    async fn edit(&self, chat_id: &str, message_id: &str, text: &str) -> anyhow::Result<()> {
        self.request(
            reqwest::Method::PATCH,
            format!("/channels/{chat_id}/messages/{message_id}"),
            Some(json!({ "content": text })),
        )
        .await?;
        Ok(())
    }

    async fn typing(&self, chat_id: &str) -> anyhow::Result<()> {
        self.request(reqwest::Method::POST, format!("/channels/{chat_id}/typing"), None)
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
    fn reads_a_dm() {
        let d = json!({
            "channel_id": "C1",
            "content": "hello",
            "author": { "id": "U1", "username": "someone", "global_name": "Someone", "bot": false }
        });
        let m = parse_message(Some(&d)).expect("a message");
        assert_eq!(m.chat_id, "C1");
        assert_eq!(m.sender_id, "U1");
        assert_eq!(m.sender_label, "Someone");
        assert!(m.direct);
    }

    /// A guild message is a room full of people; the session drops it.
    #[test]
    fn a_guild_message_is_not_direct() {
        let d = json!({
            "channel_id": "C1",
            "guild_id": "G1",
            "content": "hi",
            "author": { "id": "U1", "username": "someone" }
        });
        assert!(!parse_message(Some(&d)).expect("parsed").direct);
    }

    #[test]
    fn bots_and_empty_posts_are_skipped() {
        let bot = json!({ "channel_id": "C1", "content": "x", "author": { "id": "B", "bot": true } });
        assert!(parse_message(Some(&bot)).is_none());
        let empty = json!({ "channel_id": "C1", "content": "", "author": { "id": "U1" } });
        assert!(parse_message(Some(&empty)).is_none());
        assert!(parse_message(None).is_none());
    }

    /// Only the DM intent, and never the privileged content one.
    #[test]
    fn asks_for_direct_messages_only() {
        assert_eq!(INTENTS, 4096);
    }
}
