//! Mailbox ingest — turn what IMAP hands back into stored, safe-to-display mail.
//!
//! The pipeline per mail: parse → classify the sender → (only if the user keeps that
//! sender) sanitise, store, attach, notify. Senders that are not bulk mail are recorded
//! as `pending` and nothing of their content is written: connecting a personal mailbox
//! must never copy private conversations into the database behind the user's back.

pub mod imap;
pub mod oauth;
pub mod parse;
pub mod presets;
pub mod sanitize;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use otw_store::crypto::SecretCipher;
use otw_store::mailbox::{self, Account, NewMessage, Settings};
use reqwest::Client;
use sqlx::types::time::OffsetDateTime;
use sqlx::PgPool;
use uuid::Uuid;

/// Mails downloaded in one poll. A backlog is drained over consecutive polls rather than
/// in one long connection — a poll that never ends is a poll that never reports an error.
const MAX_PER_POLL: usize = 40;
/// Whole-poll ceiling: a wedged server must not hold the loop.
const POLL_TIMEOUT: Duration = Duration::from_secs(180);
/// When a poll leaves a backlog, come back this soon instead of waiting a full interval.
const BACKLOG_DELAY_SECS: i32 = 30;
/// Characters of the one-line preview kept in the list.
const SNIPPET_CHARS: usize = 220;
/// Bodies are truncated at this size. A newsletter is text; anything past this is padding.
const MAX_BODY_KB: usize = 512;
/// Attachments bigger than this are skipped (the mail is still stored) — one sender must
/// not be able to fill the database with 40 MB of PDFs.
const MAX_ATTACHMENT_KB: usize = 10_240;

/// Build a connection config, resolving whichever credential the account uses. The
/// plaintext exists only inside the returned struct, for the duration of one session.
pub async fn config_for(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    account: &Account,
) -> anyhow::Result<imap::ImapConfig> {
    let auth = if account.auth_kind == "oauth" {
        imap::Auth::XOAuth2(access_token(pool, cipher, http, account).await?)
    } else {
        let item = account
            .vault_item_id
            .ok_or_else(|| anyhow!("this mailbox has no password attached"))?;
        imap::Auth::Password(
            otw_store::vault::open_item(pool, cipher, item)
                .await?
                .ok_or_else(|| anyhow!("the vault item holding this password no longer exists"))?,
        )
    };
    Ok(imap::ImapConfig {
        host: account.host.clone(),
        port: account.port as u16,
        security: account.security.clone(),
        username: account.username.clone(),
        auth,
    })
}

/// A usable access token for an OAuth account: the cached one while it lasts, otherwise a
/// fresh exchange against the refresh token.
///
/// Two rules make an unattended refresh safe. The rotated refresh token is persisted
/// **before** the access token is handed out, so a crash mid-poll can never leave the
/// account holding a token Microsoft has already retired. And a revoked grant is recorded
/// as `needs_reauth` rather than as an error, which stops the poll loop from retrying a
/// credential that no amount of retrying will fix.
async fn access_token(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    account: &Account,
) -> anyhow::Result<String> {
    if let Some(token) = oauth::cached_token(account.id) {
        return Ok(token);
    }
    let item = account.refresh_vault_item.ok_or_else(|| {
        anyhow!("this mailbox has not been signed in yet — use “Reconnect”")
    })?;
    let refresh_token = otw_store::vault::open_item(pool, cipher, item)
        .await?
        .ok_or_else(|| anyhow!("the vault item holding this sign-in no longer exists"))?;

    match oauth::refresh(http, &account.oauth_tenant, &account.oauth_client_id, &refresh_token)
        .await
    {
        Ok(fresh) => {
            if let Some(rotated) = fresh.refresh_token.as_deref() {
                if rotated != refresh_token {
                    store_refresh_token(pool, cipher, account, rotated).await?;
                }
            }
            mailbox::touch_token_refresh(pool, account.id).await?;
            oauth::cache_token(account.id, &fresh.access_token, fresh.expires_in);
            Ok(fresh.access_token)
        }
        Err(oauth::RefreshError::Revoked(msg)) => {
            oauth::forget_token(account.id);
            mailbox::mark_needs_reauth(pool, account.id, &msg).await?;
            Err(anyhow!("sign-in expired: {msg}"))
        }
        Err(oauth::RefreshError::Transient(e)) => Err(e),
    }
}

/// Seal a refresh token into the module's vault, creating the vault (and the item) on
/// first use. Returns the item id, which the account row then points at.
pub async fn store_refresh_token(
    pool: &PgPool,
    cipher: &SecretCipher,
    account: &Account,
    refresh_token: &str,
) -> anyhow::Result<Uuid> {
    let vault = match otw_store::vault::find_by_name(pool, OAUTH_VAULT).await? {
        Some(v) => v,
        None => otw_store::vault::create(pool, OAUTH_VAULT).await?,
    };
    let name = if account.email.is_empty() {
        format!("mailbox-{}", account.id)
    } else {
        account.email.clone()
    };
    let item = otw_store::vault::set_item(pool, cipher, vault.id, &name, refresh_token).await?;
    Ok(item.id)
}

/// Vault that holds mailbox sign-ins. One vault, one item per mailbox.
pub const OAUTH_VAULT: &str = "Mailbox sign-ins";

/// Test one account's credentials. Stores nothing, flags nothing (beyond the re-auth
/// state a dead OAuth grant sets on its own).
pub async fn test_account(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    account: &Account,
) -> anyhow::Result<imap::ProbeReport> {
    let cfg = config_for(pool, cipher, http, account).await?;
    tokio::time::timeout(POLL_TIMEOUT, imap::probe(&cfg, &account.folder))
        .await
        .map_err(|_| anyhow!("the server did not answer in time"))?
}

/// Poll one account and store what it kept. Returns the number of new messages stored.
/// Success and failure are both recorded on the account row, so the UI can explain a
/// silent mailbox without the user reading logs.
pub async fn poll_account(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    account: &Account,
) -> anyhow::Result<usize> {
    let settings = mailbox::get_settings(pool).await?;
    let cfg = match config_for(pool, cipher, http, account).await {
        Ok(c) => c,
        Err(e) => {
            mailbox::mark_poll_error(pool, account.id, &format!("{e:#}")).await?;
            return Err(e);
        }
    };

    let max_bytes = (MAX_BODY_KB as u32)
        .saturating_add(if settings.keep_attachments {
            MAX_ATTACHMENT_KB as u32
        } else {
            0
        })
        .saturating_mul(1024)
        .saturating_add(256 * 1024);

    let stored = Arc::new(AtomicUsize::new(0));
    let result = {
        let stored = stored.clone();
        let sink = move |uid_validity: u32, mails: Vec<imap::FetchedMail>| {
            // Everything the batch needs is owned by the future: the closure returns a
            // self-contained task, which is what lets `poll` stay generic and simple.
            let pool = pool.clone();
            let cipher = cipher.clone();
            let http = http.clone();
            let settings = settings.clone();
            let account_id = account.id;
            let stored = stored.clone();
            async move {
                let n = ingest_batch(
                    &pool,
                    &cipher,
                    &http,
                    &settings,
                    account_id,
                    uid_validity,
                    mails,
                )
                .await?;
                stored.fetch_add(n, Ordering::Relaxed);
                Ok(())
            }
        };
        tokio::time::timeout(
            POLL_TIMEOUT,
            imap::poll(
                &cfg,
                &account.folder,
                account.uid_validity as u32,
                account.last_uid as u32,
                MAX_PER_POLL,
                max_bytes,
                sink,
            ),
        )
        .await
        .unwrap_or_else(|_| Err(anyhow!("the mailbox did not answer in time")))
    };

    match result {
        Ok(report) => {
            mailbox::mark_poll_ok(
                pool,
                account.id,
                report.uid_validity as i64,
                report.highest_uid as i64,
            )
            .await?;
            if report.remaining > 0 {
                mailbox::schedule_soon(pool, account.id, BACKLOG_DELAY_SECS).await?;
            }
            let n = stored.load(Ordering::Relaxed);
            if n > 0 || report.remaining > 0 {
                tracing::info!(
                    "mailbox {}: stored {n} message(s), {} left for the next poll",
                    account.name,
                    report.remaining
                );
            }
            Ok(n)
        }
        Err(e) => {
            mailbox::mark_poll_error(pool, account.id, &format!("{e:#}")).await?;
            Err(e)
        }
    }
}

/// Poll every account that is due. Errors are per-account: one broken mailbox never
/// stops the others.
pub async fn poll_due(pool: &PgPool, cipher: &SecretCipher, http: &Client) -> usize {
    let accounts = match mailbox::due_accounts(pool).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("mailbox: listing due accounts failed: {e:#}");
            return 0;
        }
    };
    let mut total = 0;
    for account in accounts {
        match poll_account(pool, cipher, http, &account).await {
            Ok(n) => total += n,
            Err(e) => tracing::warn!("mailbox {}: poll failed: {e:#}", account.name),
        }
    }
    total
}

/// Store one downloaded batch.
async fn ingest_batch(
    pool: &PgPool,
    cipher: &SecretCipher,
    http: &Client,
    settings: &Settings,
    account_id: Uuid,
    uid_validity: u32,
    mails: Vec<imap::FetchedMail>,
) -> anyhow::Result<usize> {
    let mut stored = 0usize;
    for mail in mails {
        let Some(parsed) = parse::parse(&mail.raw) else {
            continue;
        };
        if parsed.from_addr.is_empty() {
            continue;
        }

        // First contact decides the default filing; the user's later choice always wins.
        let (default_status, default_category) =
            if parsed.is_bulk { ("kept", "newsletter") } else { ("pending", "other") };
        let sender = mailbox::touch_sender(
            pool,
            &parsed.from_addr,
            &parsed.from_name,
            &parsed.subject,
            &parsed.list_unsubscribe,
            default_status,
            default_category,
        )
        .await?;
        if sender.status != "kept" {
            continue;
        }

        let (body_html, has_remote_images) = match parsed.html.as_deref() {
            Some(html) => {
                let (clean, remote) = sanitize::clean(html);
                (Some(clean), remote)
            }
            None => (None, false),
        };
        let preview_source = match (&parsed.text, &body_html) {
            (Some(t), _) => t.clone(),
            (None, Some(h)) => sanitize::to_text(h),
            _ => String::new(),
        };

        let received_at = parsed
            .date
            .or(mail.internal_date)
            .unwrap_or_else(OffsetDateTime::now_utc);

        let new = NewMessage {
            account_id,
            sender_id: sender.id,
            uid: mail.uid as i64,
            uid_validity: uid_validity as i64,
            message_id: parsed.message_id.chars().take(400).collect(),
            subject: parsed.subject.chars().take(500).collect(),
            snippet: sanitize::snippet(&preview_source, SNIPPET_CHARS),
            body_html: body_html.map(|h| truncate(h, MAX_BODY_KB * 1024)),
            body_text: parsed.text.map(|t| truncate(t, MAX_BODY_KB * 1024)),
            list_unsubscribe: parsed.list_unsubscribe.chars().take(1000).collect(),
            list_unsub_post: parsed.list_unsub_post,
            has_remote_images,
            size_bytes: mail.size as i32,
            received_at,
        };

        let Some(message_id) = mailbox::insert_message(pool, &new).await? else {
            continue; // already stored (same UID, or the same Message-ID re-delivered)
        };
        stored += 1;

        if settings.keep_attachments {
            let cap = MAX_ATTACHMENT_KB * 1024;
            for att in parsed.attachments.iter().filter(|a| a.bytes.len() <= cap) {
                mailbox::insert_attachment(
                    pool,
                    message_id,
                    &att.filename,
                    &att.mime,
                    &att.bytes,
                )
                .await?;
            }
        }

        if settings.notify_on_new && sender.notify {
            let title = if new.subject.is_empty() { "New mail" } else { new.subject.as_str() };
            let name = format!("{}: {title}", sender_label(&sender));
            match otw_store::reminders::add_notification(pool, &name, &new.snippet).await {
                Ok(notif) => {
                    crate::notif_send::dispatch(pool, cipher, http, &notif, "mailbox", None).await;
                }
                Err(e) => tracing::warn!("mailbox: notification failed: {e:#}"),
            }
        }
    }
    Ok(stored)
}

fn sender_label(sender: &mailbox::Sender) -> &str {
    if sender.name.is_empty() {
        &sender.from_addr
    } else {
        &sender.name
    }
}

/// Cut a body to a byte budget without splitting a character.
fn truncate(mut s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str("\n…");
    s
}

/// One-click unsubscribe (RFC 8058) — POST the magic body to the sender's https endpoint.
/// Only ever called from an explicit user action.
pub async fn one_click_unsubscribe(http: &Client, url: &str) -> anyhow::Result<()> {
    let res = http
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("List-Unsubscribe=One-Click")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context("sending the unsubscribe request")?;
    if !res.status().is_success() {
        return Err(anyhow!("the sender answered {}", res.status()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_respects_char_boundaries() {
        let s = "héllo wörld".to_string();
        let cut = truncate(s, 3);
        assert!(cut.starts_with("hé") || cut.starts_with("h"));
        assert!(cut.ends_with('…'));
    }
}
