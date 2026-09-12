//! Mailbox module — connected IMAP accounts, the senders seen in them, the messages kept,
//! their attachments, and the curated store of newsletter links.
//!
//! Nothing here talks to a mail server: the IMAP side lives in `otw-core::mailbox`. This
//! module owns persistence only, including the one rule that matters for privacy — an
//! account's password is never stored in `mailbox_accounts`, only a `vault_item_id`
//! pointing at the central vault (see [`crate::vault`]).

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::time::OffsetDateTime;
use sqlx::PgPool;
use uuid::Uuid;

// ── Accounts ─────────────────────────────────────────────────────────────────

/// A connected mailbox. Deliberately carries no secret: the credential is the vault item.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub preset: String,
    pub host: String,
    pub port: i32,
    pub security: String,
    pub username: String,
    /// Password accounts only; OAuth accounts carry `refresh_vault_item` instead.
    pub vault_item_id: Option<Uuid>,
    /// `password` | `oauth`.
    pub auth_kind: String,
    pub oauth_provider: String,
    pub oauth_client_id: String,
    pub oauth_tenant: String,
    pub refresh_vault_item: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub token_refreshed_at: Option<OffsetDateTime>,
    /// The stored credential was revoked; only the user can fix it.
    pub needs_reauth: bool,
    pub folder: String,
    pub interval_secs: i32,
    pub enabled: bool,
    pub uid_validity: i64,
    pub last_uid: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_poll_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_success_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub fail_count: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const ACCOUNT_COLS: &str = "id, name, email, preset, host, port, security, username, \
                            vault_item_id, auth_kind, oauth_provider, oauth_client_id, \
                            oauth_tenant, refresh_vault_item, token_refreshed_at, needs_reauth, \
                            folder, interval_secs, enabled, uid_validity, last_uid, \
                            last_poll_at, last_success_at, last_error, fail_count, created_at";

/// Everything an account needs to connect; the password rides as a vault reference.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default = "generic_preset")]
    pub preset: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: i32,
    #[serde(default = "default_security")]
    pub security: String,
    pub username: String,
    /// Required for `password` accounts, absent for `oauth` ones.
    #[serde(default)]
    pub vault_item_id: Option<Uuid>,
    #[serde(default = "password_auth")]
    pub auth_kind: String,
    #[serde(default)]
    pub oauth_provider: String,
    #[serde(default)]
    pub oauth_client_id: String,
    #[serde(default = "common_tenant")]
    pub oauth_tenant: String,
    #[serde(default = "default_folder")]
    pub folder: String,
    #[serde(default = "default_interval")]
    pub interval_secs: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn generic_preset() -> String {
    "generic".into()
}
fn password_auth() -> String {
    "password".into()
}
fn common_tenant() -> String {
    "common".into()
}
fn default_port() -> i32 {
    993
}
fn default_security() -> String {
    "ssl".into()
}
fn default_folder() -> String {
    "INBOX".into()
}
fn default_interval() -> i32 {
    900
}
fn default_true() -> bool {
    true
}

pub async fn list_accounts(pool: &PgPool) -> anyhow::Result<Vec<Account>> {
    let sql = format!("SELECT {ACCOUNT_COLS} FROM mailbox_accounts ORDER BY created_at");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing mailbox accounts")
}

pub async fn get_account(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Account>> {
    let sql = format!("SELECT {ACCOUNT_COLS} FROM mailbox_accounts WHERE id = $1");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching mailbox account")
}

pub async fn create_account(pool: &PgPool, input: &AccountInput) -> anyhow::Result<Account> {
    let sql = format!(
        "INSERT INTO mailbox_accounts \
         (id, name, email, preset, host, port, security, username, vault_item_id, folder, \
          interval_secs, enabled, auth_kind, oauth_provider, oauth_client_id, oauth_tenant, \
          needs_reauth) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) \
         RETURNING {ACCOUNT_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(input.name.trim())
        .bind(input.email.trim().to_lowercase())
        .bind(&input.preset)
        .bind(input.host.trim())
        .bind(input.port)
        .bind(&input.security)
        .bind(input.username.trim())
        .bind(input.vault_item_id)
        .bind(input.folder.trim())
        .bind(input.interval_secs.max(60))
        .bind(input.enabled)
        .bind(&input.auth_kind)
        .bind(&input.oauth_provider)
        .bind(input.oauth_client_id.trim())
        .bind(input.oauth_tenant.trim())
        // An OAuth account is born unauthenticated: the sign-in happens right after.
        .bind(input.auth_kind == "oauth")
        .fetch_one(pool)
        .await
        .context("creating mailbox account")
}

/// Sparse edit. Changing host/username/folder resets the sync watermark: UIDs from the
/// previous mailbox mean nothing in the new one.
#[derive(Debug, Default, Deserialize)]
pub struct AccountPatch {
    pub name: Option<String>,
    pub email: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub security: Option<String>,
    pub username: Option<String>,
    pub vault_item_id: Option<Uuid>,
    pub folder: Option<String>,
    pub interval_secs: Option<i32>,
    pub enabled: Option<bool>,
}

pub async fn update_account(
    pool: &PgPool,
    id: Uuid,
    patch: &AccountPatch,
) -> anyhow::Result<Option<Account>> {
    let resync = patch.host.is_some() || patch.username.is_some() || patch.folder.is_some();
    let sql = format!(
        "UPDATE mailbox_accounts SET \
           name = COALESCE($2, name), \
           email = COALESCE($3, email), \
           host = COALESCE($4, host), \
           port = COALESCE($5, port), \
           security = COALESCE($6, security), \
           username = COALESCE($7, username), \
           vault_item_id = COALESCE($8, vault_item_id), \
           folder = COALESCE($9, folder), \
           interval_secs = COALESCE($10, interval_secs), \
           enabled = COALESCE($11, enabled), \
           uid_validity = CASE WHEN $12 THEN 0 ELSE uid_validity END, \
           last_uid = CASE WHEN $12 THEN 0 ELSE last_uid END, \
           fail_count = 0, \
           next_run_at = now(), \
           updated_at = now() \
         WHERE id = $1 RETURNING {ACCOUNT_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(patch.name.as_deref().map(str::trim))
        .bind(patch.email.as_deref().map(|s| s.trim().to_lowercase()))
        .bind(patch.host.as_deref().map(str::trim))
        .bind(patch.port)
        .bind(patch.security.as_deref())
        .bind(patch.username.as_deref().map(str::trim))
        .bind(patch.vault_item_id)
        .bind(patch.folder.as_deref().map(str::trim))
        .bind(patch.interval_secs.map(|v| v.max(60)))
        .bind(patch.enabled)
        .bind(resync)
        .fetch_optional(pool)
        .await
        .context("updating mailbox account")
}

pub async fn delete_account(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM mailbox_accounts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting mailbox account")?
        .rows_affected();
    Ok(n > 0)
}

/// Enabled accounts whose next poll is due. Accounts waiting for a re-sign-in are left
/// out: retrying a revoked credential every minute achieves nothing but noise.
pub async fn due_accounts(pool: &PgPool) -> anyhow::Result<Vec<Account>> {
    let sql = format!(
        "SELECT {ACCOUNT_COLS} FROM mailbox_accounts \
         WHERE enabled AND NOT needs_reauth AND next_run_at <= now() ORDER BY next_run_at"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing due mailbox accounts")
}

/// Record a successful poll: clear the error, advance the watermark and the next run.
///
/// The watermark only ever moves forward *within one UIDVALIDITY*. When the server
/// reports a new one the mailbox was recreated and its UIDs restarted from 1, so the old
/// high-water mark would silently hide every new message — take the reported UID instead.
pub async fn mark_poll_ok(
    pool: &PgPool,
    id: Uuid,
    uid_validity: i64,
    last_uid: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE mailbox_accounts SET \
           last_uid = CASE WHEN uid_validity = $2 THEN GREATEST(last_uid, $3) ELSE $3 END, \
           uid_validity = $2, \
           last_poll_at = now(), last_success_at = now(), last_error = NULL, fail_count = 0, \
           next_run_at = now() + make_interval(secs => interval_secs), updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(uid_validity)
    .bind(last_uid)
    .execute(pool)
    .await
    .context("recording mailbox poll success")?;
    Ok(())
}

/// Record a failed poll. Back-off grows with the failure count (capped at ~1 h) so a wrong
/// password does not hammer the provider — the account stays enabled and visibly in error.
pub async fn mark_poll_error(pool: &PgPool, id: Uuid, error: &str) -> anyhow::Result<()> {
    let msg: String = error.chars().take(500).collect();
    sqlx::query(
        "UPDATE mailbox_accounts SET \
           last_poll_at = now(), last_error = $2, fail_count = fail_count + 1, \
           next_run_at = now() + make_interval(secs => LEAST(3600, interval_secs * (fail_count + 1))), \
           updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(msg)
    .execute(pool)
    .await
    .context("recording mailbox poll error")?;
    Ok(())
}

// ── OAuth credentials ────────────────────────────────────────────────────────

/// Attach a freshly minted refresh token to an account and clear the re-auth flag.
/// The token itself is already sealed in the vault; only its item id lands here.
pub async fn set_oauth_credential(
    pool: &PgPool,
    id: Uuid,
    refresh_vault_item: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE mailbox_accounts SET refresh_vault_item = $2, token_refreshed_at = now(), \
           needs_reauth = FALSE, last_error = NULL, fail_count = 0, next_run_at = now(), \
           updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(refresh_vault_item)
    .execute(pool)
    .await
    .context("storing mailbox oauth credential")?;
    Ok(())
}

/// Record a successful token exchange (the 90-day inactivity clock restarts here).
pub async fn touch_token_refresh(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE mailbox_accounts SET token_refreshed_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("recording mailbox token refresh")?;
    Ok(())
}

/// The credential is gone for good (revoked, expired, consent withdrawn). Polling stops
/// until the user signs in again — this is a state the UI shows, not a transient error.
pub async fn mark_needs_reauth(pool: &PgPool, id: Uuid, error: &str) -> anyhow::Result<()> {
    let msg: String = error.chars().take(500).collect();
    sqlx::query(
        "UPDATE mailbox_accounts SET needs_reauth = TRUE, last_error = $2, last_poll_at = now(), \
           updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(msg)
    .execute(pool)
    .await
    .context("flagging mailbox re-auth")?;
    Ok(())
}

/// OAuth accounts that are enabled and still hold a credential — refreshed once at boot so
/// a revocation that happened while the app was down surfaces immediately, not at the next
/// scheduled poll.
pub async fn oauth_accounts(pool: &PgPool) -> anyhow::Result<Vec<Account>> {
    let sql = format!(
        "SELECT {ACCOUNT_COLS} FROM mailbox_accounts \
         WHERE auth_kind = 'oauth' AND enabled AND NOT needs_reauth \
           AND refresh_vault_item IS NOT NULL ORDER BY created_at"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing oauth mailbox accounts")
}

/// OAuth accounts whose refresh token has not been exchanged for `days` — the early
/// warning before Microsoft's 90-day inactivity window closes on a paused mailbox.
pub async fn stale_oauth_accounts(pool: &PgPool, days: i32) -> anyhow::Result<Vec<Account>> {
    let sql = format!(
        "SELECT {ACCOUNT_COLS} FROM mailbox_accounts \
         WHERE auth_kind = 'oauth' AND NOT needs_reauth AND refresh_vault_item IS NOT NULL \
           AND token_refreshed_at < now() - make_interval(days => $1) ORDER BY token_refreshed_at"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(days)
        .fetch_all(pool)
        .await
        .context("listing stale oauth mailbox accounts")
}

/// Come back sooner than the configured interval (a poll left a backlog behind).
pub async fn schedule_soon(pool: &PgPool, id: Uuid, secs: i32) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE mailbox_accounts SET next_run_at = now() + make_interval(secs => $2) WHERE id = $1",
    )
    .bind(id)
    .bind(secs.max(5))
    .execute(pool)
    .await
    .context("rescheduling mailbox poll")?;
    Ok(())
}

// ── Senders ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Sender {
    pub id: Uuid,
    pub from_addr: String,
    pub domain: String,
    pub name: String,
    pub description: String,
    pub site_url: String,
    pub category: String,
    pub status: String,
    pub notify: bool,
    pub list_unsubscribe: String,
    pub last_subject: String,
    pub seen_count: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub last_seen_at: OffsetDateTime,
    /// Stored messages (0 for pending/ignored senders).
    pub message_count: i64,
    /// Stored messages still unread.
    pub unread_count: i64,
}

const SENDER_COLS: &str = "s.id, s.from_addr, s.domain, s.name, s.description, s.site_url, \
                           s.category, s.status, s.notify, s.list_unsubscribe, s.last_subject, \
                           s.seen_count, s.last_seen_at, \
                           (SELECT count(*) FROM mailbox_messages m WHERE m.sender_id = s.id) AS message_count, \
                           (SELECT count(*) FROM mailbox_messages m WHERE m.sender_id = s.id AND NOT m.read AND NOT m.archived) AS unread_count";

pub async fn list_senders(pool: &PgPool) -> anyhow::Result<Vec<Sender>> {
    let sql = format!(
        "SELECT {SENDER_COLS} FROM mailbox_senders s \
         ORDER BY s.domain, lower(COALESCE(NULLIF(s.name, ''), s.from_addr))"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing mailbox senders")
}

pub async fn get_sender(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Sender>> {
    let sql = format!("SELECT {SENDER_COLS} FROM mailbox_senders s WHERE s.id = $1");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching mailbox sender")
}

/// Record one sighting of a sender, creating it on first contact. `default_status` /
/// `default_category` apply to new rows only: once the user has filed a sender, ingest
/// never overrides that choice.
#[allow(clippy::too_many_arguments)]
pub async fn touch_sender(
    pool: &PgPool,
    from_addr: &str,
    display_name: &str,
    subject: &str,
    list_unsubscribe: &str,
    default_status: &str,
    default_category: &str,
) -> anyhow::Result<Sender> {
    let addr = from_addr.trim().to_lowercase();
    let domain = addr.rsplit('@').next().unwrap_or("").to_string();
    let subject: String = subject.chars().take(300).collect();
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO mailbox_senders \
           (id, from_addr, domain, name, category, status, list_unsubscribe, last_subject, seen_count) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1) \
         ON CONFLICT (from_addr) DO UPDATE SET \
           name = CASE WHEN mailbox_senders.name = '' THEN EXCLUDED.name ELSE mailbox_senders.name END, \
           list_unsubscribe = CASE WHEN EXCLUDED.list_unsubscribe <> '' \
                                   THEN EXCLUDED.list_unsubscribe ELSE mailbox_senders.list_unsubscribe END, \
           last_subject = EXCLUDED.last_subject, \
           seen_count = mailbox_senders.seen_count + 1, \
           last_seen_at = now() \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(&addr)
    .bind(&domain)
    .bind(display_name.trim())
    .bind(default_category)
    .bind(default_status)
    .bind(list_unsubscribe)
    .bind(&subject)
    .fetch_one(pool)
    .await
    .context("recording mailbox sender")?;

    get_sender(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sender vanished after upsert"))
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SenderPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub site_url: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub notify: Option<bool>,
}

pub async fn update_sender(
    pool: &PgPool,
    id: Uuid,
    patch: &SenderPatch,
) -> anyhow::Result<Option<Sender>> {
    let n = sqlx::query(
        "UPDATE mailbox_senders SET \
           name = COALESCE($2, name), \
           description = COALESCE($3, description), \
           site_url = COALESCE($4, site_url), \
           category = COALESCE($5, category), \
           status = COALESCE($6, status), \
           notify = COALESCE($7, notify) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.description.as_deref().map(str::trim))
    .bind(patch.site_url.as_deref().map(str::trim))
    .bind(patch.category.as_deref())
    .bind(patch.status.as_deref())
    .bind(patch.notify)
    .execute(pool)
    .await
    .context("updating mailbox sender")?
    .rows_affected();
    if n == 0 {
        return Ok(None);
    }
    get_sender(pool, id).await
}

/// Delete a sender and every message stored for it.
pub async fn delete_sender(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM mailbox_senders WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting mailbox sender")?
        .rows_affected();
    Ok(n > 0)
}

// ── Messages ─────────────────────────────────────────────────────────────────

/// A row of the message list: everything the list needs, no bodies.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MessageRow {
    pub id: Uuid,
    pub account_id: Uuid,
    pub sender_id: Uuid,
    pub sender_name: String,
    pub sender_addr: String,
    pub sender_category: String,
    pub subject: String,
    pub snippet: String,
    pub attachment_count: i32,
    pub has_remote_images: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub received_at: OffsetDateTime,
    pub read: bool,
    pub starred: bool,
    pub archived: bool,
}

const ROW_COLS: &str = "m.id, m.account_id, m.sender_id, \
                        COALESCE(NULLIF(s.name, ''), s.from_addr) AS sender_name, \
                        s.from_addr AS sender_addr, s.category AS sender_category, \
                        m.subject, m.snippet, m.attachment_count, m.has_remote_images, \
                        m.received_at, m.read, m.starred, m.archived";

/// Full message, bodies included.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MessageFull {
    pub id: Uuid,
    pub account_id: Uuid,
    pub sender_id: Uuid,
    pub sender_name: String,
    pub sender_addr: String,
    pub sender_category: String,
    pub subject: String,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub list_unsubscribe: String,
    pub list_unsub_post: bool,
    pub has_remote_images: bool,
    pub attachment_count: i32,
    pub size_bytes: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub received_at: OffsetDateTime,
    pub read: bool,
    pub starred: bool,
    pub archived: bool,
}

const FULL_COLS: &str = "m.id, m.account_id, m.sender_id, \
                         COALESCE(NULLIF(s.name, ''), s.from_addr) AS sender_name, \
                         s.from_addr AS sender_addr, s.category AS sender_category, \
                         m.subject, m.body_html, m.body_text, m.list_unsubscribe, \
                         m.list_unsub_post, m.has_remote_images, m.attachment_count, \
                         m.size_bytes, m.received_at, m.read, m.starred, m.archived";

/// Everything ingest extracted from one mail, ready to store.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub account_id: Uuid,
    pub sender_id: Uuid,
    pub uid: i64,
    pub uid_validity: i64,
    pub message_id: String,
    pub subject: String,
    pub snippet: String,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub list_unsubscribe: String,
    pub list_unsub_post: bool,
    pub has_remote_images: bool,
    pub size_bytes: i32,
    pub received_at: OffsetDateTime,
}

/// Insert one message; `Ok(None)` when it is already stored (same UID, or same Message-ID
/// arriving twice — a mailbox that re-delivers should not duplicate the reading list).
pub async fn insert_message(pool: &PgPool, msg: &NewMessage) -> anyhow::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO mailbox_messages \
           (id, account_id, sender_id, uid, uid_validity, message_id, subject, snippet, \
            body_html, body_text, list_unsubscribe, list_unsub_post, has_remote_images, \
            size_bytes, received_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) \
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(msg.account_id)
    .bind(msg.sender_id)
    .bind(msg.uid)
    .bind(msg.uid_validity)
    .bind(&msg.message_id)
    .bind(&msg.subject)
    .bind(&msg.snippet)
    .bind(&msg.body_html)
    .bind(&msg.body_text)
    .bind(&msg.list_unsubscribe)
    .bind(msg.list_unsub_post)
    .bind(msg.has_remote_images)
    .bind(msg.size_bytes)
    .bind(msg.received_at)
    .fetch_optional(pool)
    .await
    .context("storing mail message")?;
    Ok(row.map(|r| r.0))
}

/// List filter. All fields are optional; `archived` defaults to "not archived".
#[derive(Debug, Default, Deserialize)]
pub struct MessageQuery {
    pub sender_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub category: Option<String>,
    pub q: Option<String>,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    pub archived: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_messages(pool: &PgPool, q: &MessageQuery) -> anyhow::Result<Vec<MessageRow>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let like = q.q.as_ref().map(|s| format!("%{}%", s.trim()));
    let sql = format!(
        "SELECT {ROW_COLS} FROM mailbox_messages m JOIN mailbox_senders s ON s.id = m.sender_id \
         WHERE ($1::uuid IS NULL OR m.sender_id = $1) \
           AND ($2::uuid IS NULL OR m.account_id = $2) \
           AND ($3::text IS NULL OR s.category = $3) \
           AND ($4::text IS NULL OR m.subject ILIKE $4 OR m.snippet ILIKE $4 \
                OR s.from_addr ILIKE $4 OR s.name ILIKE $4) \
           AND ($5::bool IS NULL OR m.read = NOT $5) \
           AND ($6::bool IS NULL OR m.starred = $6) \
           AND m.archived = COALESCE($7::bool, false) \
         ORDER BY m.received_at DESC LIMIT $8 OFFSET $9"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(q.sender_id)
        .bind(q.account_id)
        .bind(q.category.as_deref().filter(|c| *c != "all"))
        .bind(like)
        .bind(q.unread)
        .bind(q.starred)
        .bind(q.archived)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("listing mail messages")
}

pub async fn get_message(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<MessageFull>> {
    let sql = format!(
        "SELECT {FULL_COLS} FROM mailbox_messages m JOIN mailbox_senders s ON s.id = m.sender_id \
         WHERE m.id = $1"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching mail message")
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct MessagePatch {
    pub read: Option<bool>,
    pub starred: Option<bool>,
    pub archived: Option<bool>,
}

pub async fn update_message(pool: &PgPool, id: Uuid, patch: &MessagePatch) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "UPDATE mailbox_messages SET read = COALESCE($2, read), \
           starred = COALESCE($3, starred), archived = COALESCE($4, archived) WHERE id = $1",
    )
    .bind(id)
    .bind(patch.read)
    .bind(patch.starred)
    .bind(patch.archived)
    .execute(pool)
    .await
    .context("updating mail message")?
    .rows_affected();
    Ok(n > 0)
}

pub async fn delete_message(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM mailbox_messages WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting mail message")?
        .rows_affected();
    Ok(n > 0)
}

/// Mark every non-archived message read, optionally scoped to one sender.
pub async fn mark_all_read(pool: &PgPool, sender_id: Option<Uuid>) -> anyhow::Result<u64> {
    let n = sqlx::query(
        "UPDATE mailbox_messages SET read = TRUE \
         WHERE NOT read AND NOT archived AND ($1::uuid IS NULL OR sender_id = $1)",
    )
    .bind(sender_id)
    .execute(pool)
    .await
    .context("marking mail read")?
    .rows_affected();
    Ok(n)
}

/// Unread, total and pending-sender counts for the module header.
pub async fn counts(pool: &PgPool) -> anyhow::Result<(i64, i64, i64)> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM mailbox_messages WHERE NOT read AND NOT archived), \
                (SELECT count(*) FROM mailbox_messages), \
                (SELECT count(*) FROM mailbox_senders WHERE status = 'pending')",
    )
    .fetch_one(pool)
    .await
    .context("counting mail")?;
    Ok(row)
}

/// Drop messages older than `days` (0 = keep everything). Starred mail is never trimmed.
pub async fn trim_older_than(pool: &PgPool, days: i32) -> anyhow::Result<u64> {
    if days <= 0 {
        return Ok(0);
    }
    let n = sqlx::query(
        "DELETE FROM mailbox_messages \
         WHERE NOT starred AND received_at < now() - make_interval(days => $1)",
    )
    .bind(days)
    .execute(pool)
    .await
    .context("trimming old mail")?
    .rows_affected();
    Ok(n)
}

// ── Attachments ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AttachmentMeta {
    pub id: Uuid,
    pub message_id: Uuid,
    pub filename: String,
    pub mime: String,
    pub size_bytes: i32,
}

pub async fn insert_attachment(
    pool: &PgPool,
    message_id: Uuid,
    filename: &str,
    mime: &str,
    content: &[u8],
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO mailbox_attachments (id, message_id, filename, mime, size_bytes, content) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(message_id)
    .bind(filename)
    .bind(mime)
    .bind(content.len() as i32)
    .bind(content)
    .execute(pool)
    .await
    .context("storing mail attachment")?;
    sqlx::query("UPDATE mailbox_messages SET attachment_count = attachment_count + 1 WHERE id = $1")
        .bind(message_id)
        .execute(pool)
        .await
        .context("counting mail attachment")?;
    Ok(())
}

pub async fn list_attachments(
    pool: &PgPool,
    message_id: Uuid,
) -> anyhow::Result<Vec<AttachmentMeta>> {
    sqlx::query_as(
        "SELECT id, message_id, filename, mime, size_bytes FROM mailbox_attachments \
         WHERE message_id = $1 ORDER BY filename",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await
    .context("listing mail attachments")
}

/// One attachment's bytes, with the metadata needed for the download response.
pub async fn open_attachment(
    pool: &PgPool,
    id: Uuid,
) -> anyhow::Result<Option<(String, String, Vec<u8>)>> {
    let row: Option<(String, String, Vec<u8>)> =
        sqlx::query_as("SELECT filename, mime, content FROM mailbox_attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("fetching mail attachment")?;
    Ok(row)
}

// ── Store links ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StoreLink {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub domain: String,
    pub description: String,
    pub topic: String,
    pub subscribed: bool,
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const LINK_COLS: &str =
    "id, name, url, domain, description, topic, subscribed, position, created_at";

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct StoreLinkInput {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub description: String,
    /// mindset | finance | trading | geopolitics | economics | other
    #[serde(default = "other_topic")]
    pub topic: String,
    #[serde(default)]
    pub subscribed: bool,
}

fn other_topic() -> String {
    "other".into()
}

/// Host of a URL, without scheme or `www.` — the Store groups on it.
pub fn domain_of(url: &str) -> String {
    let s = url.trim();
    let s = s.split_once("://").map(|(_, rest)| rest).unwrap_or(s);
    let s = s.split('/').next().unwrap_or(s);
    let s = s.split('@').next_back().unwrap_or(s);
    let s = s.split(':').next().unwrap_or(s);
    s.trim_start_matches("www.").to_lowercase()
}

pub async fn list_links(pool: &PgPool) -> anyhow::Result<Vec<StoreLink>> {
    let sql = format!(
        "SELECT {LINK_COLS} FROM mailbox_store_links ORDER BY domain, position, lower(name)"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing store links")
}

pub async fn create_link(pool: &PgPool, input: &StoreLinkInput) -> anyhow::Result<StoreLink> {
    let sql = format!(
        "INSERT INTO mailbox_store_links (id, name, url, domain, description, topic, subscribed) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {LINK_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(input.name.trim())
        .bind(input.url.trim())
        .bind(domain_of(&input.url))
        .bind(input.description.trim())
        .bind(&input.topic)
        .bind(input.subscribed)
        .fetch_one(pool)
        .await
        .context("creating store link")
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct StoreLinkPatch {
    pub name: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub topic: Option<String>,
    pub subscribed: Option<bool>,
    pub position: Option<i32>,
}

pub async fn update_link(
    pool: &PgPool,
    id: Uuid,
    patch: &StoreLinkPatch,
) -> anyhow::Result<Option<StoreLink>> {
    let sql = format!(
        "UPDATE mailbox_store_links SET \
           name = COALESCE($2, name), \
           url = COALESCE($3, url), \
           domain = COALESCE($4, domain), \
           description = COALESCE($5, description), \
           topic = COALESCE($6, topic), \
           subscribed = COALESCE($7, subscribed), \
           position = COALESCE($8, position), \
           updated_at = now() \
         WHERE id = $1 RETURNING {LINK_COLS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(patch.name.as_deref().map(str::trim))
        .bind(patch.url.as_deref().map(str::trim))
        .bind(patch.url.as_deref().map(|u| domain_of(u)))
        .bind(patch.description.as_deref().map(str::trim))
        .bind(patch.topic.as_deref())
        .bind(patch.subscribed)
        .bind(patch.position)
        .fetch_optional(pool)
        .await
        .context("updating store link")
}

pub async fn delete_link(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM mailbox_store_links WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting store link")?
        .rows_affected();
    Ok(n > 0)
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Settings {
    pub default_interval_secs: i32,
    pub keep_attachments: bool,
    pub load_remote_images: bool,
    pub notify_on_new: bool,
    pub retention_days: i32,
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<Settings> {
    sqlx::query_as(
        "SELECT default_interval_secs, keep_attachments, \
                load_remote_images, notify_on_new, retention_days \
         FROM mailbox_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .context("loading mailbox settings")
}

pub async fn put_settings(pool: &PgPool, s: &Settings) -> anyhow::Result<Settings> {
    sqlx::query(
        "UPDATE mailbox_settings SET default_interval_secs = $1, keep_attachments = $2, \
           load_remote_images = $3, notify_on_new = $4, retention_days = $5, \
           updated_at = now() WHERE id = 1",
    )
    .bind(s.default_interval_secs.max(60))
    .bind(s.keep_attachments)
    .bind(s.load_remote_images)
    .bind(s.notify_on_new)
    .bind(s.retention_days.max(0))
    .execute(pool)
    .await
    .context("saving mailbox settings")?;
    get_settings(pool).await
}

#[cfg(test)]
mod tests {
    use super::domain_of;

    #[test]
    fn domain_of_strips_scheme_path_and_www() {
        assert_eq!(domain_of("https://www.stratechery.com/feed/"), "stratechery.com");
        assert_eq!(domain_of("http://example.org:8080/x"), "example.org");
        assert_eq!(domain_of("newsletter.substack.com"), "newsletter.substack.com");
        assert_eq!(domain_of(""), "");
    }
}
