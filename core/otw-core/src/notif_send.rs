//! Sending reminder notifications to external channels.
//!
//! Phase 1 channels, all free for the host — the user brings their own account:
//!   - email    : their own SMTP server (secret = password). `lettre`.
//!   - telegram : a BotFather bot (secret = bot token, config.chat_id). HTTP.
//!   - slack    : an Incoming Webhook URL (secret = the URL). HTTP.
//!   - discord  : a channel Webhook URL (secret = the URL). HTTP.
//!
//! A channel is only sent to when the user has explicitly enabled it. Each attempt's
//! outcome is recorded so the module UI can show per-channel status. Failures never
//! interrupt firing: the in-app notification is already written by the time we get here.

use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use otw_store::crypto::SecretCipher;
use otw_store::notif_channels::{self, ChannelWithSecret};
use otw_store::reminders::FiredNotification;
use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;

/// Push one fired notification to every enabled channel. Errors are logged and recorded
/// per channel, never propagated.
pub async fn dispatch(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    notif: &FiredNotification,
) {
    let channels = match notif_channels::load_enabled(pool, cipher).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("loading enabled notif channels: {e:#}");
            return;
        }
    };
    for ch in channels {
        let res = send_one(http, &ch, &notif.name, &notif.details).await;
        let (ok, err) = match &res {
            Ok(()) => (true, None),
            Err(e) => {
                tracing::warn!("notif channel '{}' ({}) failed: {e:#}", ch.name, ch.kind);
                (false, Some(sanitize(&format!("{e:#}"), ch.secret.as_deref())))
            }
        };
        let _ = notif_channels::record_result(pool, ch.id, ok, err.as_deref()).await;
    }
}

/// Send a single message to one channel. Used by dispatch and by the "send test" route.
pub async fn send_one(
    http: &Client,
    ch: &ChannelWithSecret,
    title: &str,
    body: &str,
) -> anyhow::Result<()> {
    match ch.kind.as_str() {
        "email" => send_email(ch, title, body).await,
        "telegram" => send_telegram(http, ch, title, body).await,
        "slack" => send_slack(http, ch, title, body).await,
        "discord" => send_discord(http, ch, title, body).await,
        other => Err(anyhow::anyhow!("unknown channel kind '{other}'")),
    }
}

fn cfg_str<'a>(ch: &'a ChannelWithSecret, key: &str) -> anyhow::Result<&'a str> {
    ch.config
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("channel config missing '{key}'"))
}

fn secret(ch: &ChannelWithSecret) -> anyhow::Result<&str> {
    ch.secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("channel has no secret configured"))
}

/// Combine title + details into a plain-text body.
fn plain(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        title.to_string()
    } else {
        format!("{title}\n\n{body}")
    }
}

async fn send_email(ch: &ChannelWithSecret, title: &str, body: &str) -> anyhow::Result<()> {
    let host = cfg_str(ch, "host")?;
    let port: u16 = ch
        .config
        .get("port")
        .and_then(|v| v.as_u64())
        .map(|p| p as u16)
        .unwrap_or(587);
    let from: Mailbox = cfg_str(ch, "from")?.parse().map_err(|_| anyhow::anyhow!("invalid 'from' address"))?;
    let to: Mailbox = cfg_str(ch, "to")?.parse().map_err(|_| anyhow::anyhow!("invalid 'to' address"))?;
    // Username defaults to the from-address if not given.
    let username = ch
        .config
        .get("username")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| from.email.to_string());
    let password = secret(ch)?.to_string();
    // starttls (default, port 587) vs implicit TLS (port 465).
    let implicit_tls = ch
        .config
        .get("implicit_tls")
        .and_then(|v| v.as_bool())
        .unwrap_or(port == 465);

    let email = Message::builder()
        .from(from)
        .to(to)
        .subject(title.to_string())
        .header(ContentType::TEXT_PLAIN)
        .body(plain(title, body))?;

    let builder = if implicit_tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?
    };
    let mailer = builder
        .port(port)
        .credentials(Credentials::new(username, password))
        .build();

    mailer.send(email).await.map_err(|e| anyhow::anyhow!("smtp send: {e}"))?;
    Ok(())
}

async fn send_telegram(
    http: &Client,
    ch: &ChannelWithSecret,
    title: &str,
    body: &str,
) -> anyhow::Result<()> {
    let token = secret(ch)?;
    let chat_id = cfg_str(ch, "chat_id")?;
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let text = plain(title, body);
    let resp = http
        .post(&url)
        .json(&json!({ "chat_id": chat_id, "text": text }))
        .send()
        .await?;
    ensure_ok(resp).await
}

async fn send_slack(
    http: &Client,
    ch: &ChannelWithSecret,
    title: &str,
    body: &str,
) -> anyhow::Result<()> {
    let webhook = secret(ch)?; // the whole incoming-webhook URL is the secret
    let text = plain(title, body);
    let resp = http.post(webhook).json(&json!({ "text": text })).send().await?;
    ensure_ok(resp).await
}

async fn send_discord(
    http: &Client,
    ch: &ChannelWithSecret,
    title: &str,
    body: &str,
) -> anyhow::Result<()> {
    let webhook = secret(ch)?;
    let text = plain(title, body);
    let resp = http.post(webhook).json(&json!({ "content": text })).send().await?;
    ensure_ok(resp).await
}

/// Turn a non-2xx HTTP response into an error carrying a short body snippet.
async fn ensure_ok(resp: reqwest::Response) -> anyhow::Result<()> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let snippet: String = resp
        .text()
        .await
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    Err(anyhow::anyhow!("HTTP {status}: {snippet}"))
}

/// Redact the secret (webhook URL / token / password) from an error string, so recorded
/// errors never leak credentials, and cap the length.
fn sanitize(msg: &str, secret: Option<&str>) -> String {
    let mut out = msg.to_string();
    if let Some(s) = secret {
        if s.len() >= 4 {
            out = out.replace(s, "***");
        }
    }
    let capped: String = out.chars().take(240).collect();
    capped
}
