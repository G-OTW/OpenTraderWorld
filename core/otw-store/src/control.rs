//! External control bindings — the return path of a notification channel.
//!
//! A [`notif_channels::Channel`](crate::notif_channels) is a destination: the app pushes to
//! it. A binding is what lets the same channel push back. It carries the three things the
//! outbound half has no use for:
//!
//! - a **bot credential that can receive**. Slack and Discord channels are usually a webhook
//!   URL, which is write-only; listening needs a real app/bot token. It is sealed here, next
//!   to the binding, rather than as a second secret on the channel.
//! - the **agent** that answers.
//! - the **permission envelope**, which is an `mcp_tokens` row and nothing new. The token's
//!   per-module levels apply as-is, the way the in-app agent already uses them; the only
//!   addition is the `external` flag, which says a token may be reached from outside at all.
//!
//! One binding per channel and one token per binding, enforced by the schema: sharing a
//! token across transports would make revoking one revoke the other, and would erase which
//! way a call came in.
//!
//! Persistence only. Transports, pairing flow and dispatch live in otw-core.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::SecretCipher;

/// Channel kinds that can carry control. Email is absent on purpose: a mailbox is a
/// different trust model (anyone can forge a From line) and a different latency.
pub const CONTROL_KINDS: &[&str] = &["telegram", "slack", "discord"];

/// App setting holding the pairing-code lifetime, in minutes.
pub const PAIR_TTL_SETTING: &str = "control_pair_ttl_minutes";

/// Default lifetime of a pairing code. Long enough to walk to a phone, find the chat and
/// type six digits without the code dying on the way; the user can widen or narrow it in
/// Settings → External control.
pub const PAIR_TTL_DEFAULT: i64 = 60;

/// Bounds on the stored value. A code that never expires is a standing invitation, and one
/// that expires in seconds cannot be typed.
pub const PAIR_TTL_MIN: i64 = 5;
pub const PAIR_TTL_MAX: i64 = 1440;

/// The configured pairing-code lifetime, clamped to the supported range.
pub async fn pair_ttl_minutes(pool: &PgPool) -> anyhow::Result<i64> {
    let raw = crate::settings::get_or(pool, PAIR_TTL_SETTING, "").await?;
    let mins = raw.trim().parse::<i64>().unwrap_or(PAIR_TTL_DEFAULT);
    Ok(mins.clamp(PAIR_TTL_MIN, PAIR_TTL_MAX))
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Sender {
    pub sender_id: String,
    pub label: String,
    #[serde(with = "time::serde::rfc3339")]
    pub paired_at: OffsetDateTime,
}

/// A binding as the API returns it: no secret, no live pairing code.
#[derive(Debug, Clone, Serialize)]
pub struct Binding {
    pub id: Uuid,
    pub name: String,
    pub channel_id: Uuid,
    /// Denormalised for the UI, which lists bindings before it lists channels.
    pub channel_kind: String,
    pub channel_name: String,
    pub agent_id: Uuid,
    /// Provider override for this binding. NULL = inherit from the agent.
    pub provider_id: Option<Uuid>,
    /// Model override for this binding. Empty = the provider's/agent's own.
    pub model: String,
    pub token_id: Uuid,
    pub enabled: bool,
    /// True when an inbound bot credential is stored.
    pub has_secret: bool,
    pub secret_vault_item: Option<Uuid>,
    pub config: Value,
    pub senders: Vec<Sender>,
    /// Whether a pairing code is currently outstanding (the code itself is returned only
    /// by [`mint_pair_code`], at the moment it is minted).
    pub pairing: bool,
    pub last_ok: Option<bool>,
    pub last_error: Option<String>,
    #[serde(with = "ts_opt")]
    pub last_seen_at: Option<OffsetDateTime>,
}

mod ts_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

/// A binding as a transport worker needs it: enabled, credential opened, channel kind
/// known. Never returned over the API.
#[derive(Debug, Clone)]
pub struct ActiveBinding {
    pub id: Uuid,
    pub name: String,
    pub channel_id: Uuid,
    pub channel_kind: String,
    pub agent_id: Uuid,
    pub provider_id: Option<Uuid>,
    pub model: String,
    pub token_id: Uuid,
    pub config: Value,
    /// The inbound bot credential. A binding without one cannot listen.
    pub secret: Option<String>,
}

/// Create/update input. `secret` follows the channel convention: absent leaves the stored
/// credential untouched, `Some("")` clears it, `Some(v)` reseals it.
#[derive(Debug, Deserialize)]
pub struct BindingInput {
    #[serde(default)]
    pub name: String,
    pub channel_id: Uuid,
    pub agent_id: Uuid,
    /// Absent leaves the stored override alone; `Some(None)` clears it back to inherit.
    #[serde(default, deserialize_with = "crate::agent::double_option")]
    pub provider_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub model: Option<String>,
    pub token_id: Uuid,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub config: Option<Value>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub secret_vault_item: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct BindingRow {
    id: Uuid,
    name: String,
    channel_id: Uuid,
    channel_kind: String,
    channel_name: String,
    agent_id: Uuid,
    provider_id: Option<Uuid>,
    model: String,
    token_id: Uuid,
    enabled: bool,
    secret_nonce: Option<Vec<u8>>,
    secret_vault_item: Option<Uuid>,
    config: Value,
    pair_expires_at: Option<OffsetDateTime>,
    last_ok: Option<bool>,
    last_error: Option<String>,
    last_seen_at: Option<OffsetDateTime>,
}

const COLUMNS: &str = "cb.id, cb.name, cb.channel_id, nc.kind AS channel_kind, \
     nc.name AS channel_name, cb.agent_id, cb.provider_id, cb.model, cb.token_id, \
     cb.enabled, cb.secret_nonce, \
     cb.secret_vault_item, cb.config, cb.pair_expires_at, cb.last_ok, cb.last_error, \
     cb.last_seen_at \
     FROM control_bindings cb JOIN notif_channels nc ON nc.id = cb.channel_id";

async fn hydrate(pool: &PgPool, r: BindingRow) -> anyhow::Result<Binding> {
    let senders = list_senders(pool, r.id).await?;
    Ok(Binding {
        id: r.id,
        name: r.name,
        channel_id: r.channel_id,
        channel_kind: r.channel_kind,
        channel_name: r.channel_name,
        agent_id: r.agent_id,
        provider_id: r.provider_id,
        model: r.model,
        token_id: r.token_id,
        enabled: r.enabled,
        has_secret: r.secret_nonce.is_some() || r.secret_vault_item.is_some(),
        secret_vault_item: r.secret_vault_item,
        config: r.config,
        senders,
        pairing: r.pair_expires_at.is_some_and(|at| at > OffsetDateTime::now_utc()),
        last_ok: r.last_ok,
        last_error: r.last_error,
        last_seen_at: r.last_seen_at,
    })
}

pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Binding>> {
    let sql = format!("SELECT {COLUMNS} ORDER BY cb.created_at");
    let rows = sqlx::query_as::<_, BindingRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing control bindings")?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(hydrate(pool, r).await?);
    }
    Ok(out)
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Binding>> {
    let sql = format!("SELECT {COLUMNS} WHERE cb.id = $1");
    let row = sqlx::query_as::<_, BindingRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading control binding")?;
    match row {
        Some(r) => Ok(Some(hydrate(pool, r).await?)),
        None => Ok(None),
    }
}

pub async fn add(
    pool: &PgPool,
    cipher: &SecretCipher,
    input: &BindingInput,
) -> anyhow::Result<Binding> {
    let id = Uuid::new_v4();
    // A vault reference wins over a pasted credential: never keep both.
    let (nonce, ct) = match input.secret.as_deref().filter(|s| !s.is_empty()) {
        Some(plain) if input.secret_vault_item.is_none() => {
            let (n, c) = cipher.seal(plain)?;
            (Some(n), Some(c))
        }
        _ => (None, None),
    };
    sqlx::query(
        "INSERT INTO control_bindings \
         (id, name, channel_id, agent_id, provider_id, model, token_id, enabled, \
          secret_nonce, secret_cipher, secret_vault_item, config) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.channel_id)
    .bind(input.agent_id)
    .bind(input.provider_id.flatten())
    .bind(input.model.clone().unwrap_or_default())
    .bind(input.token_id)
    .bind(input.enabled)
    .bind(nonce)
    .bind(ct)
    .bind(input.secret_vault_item)
    .bind(input.config.clone().unwrap_or_else(|| serde_json::json!({})))
    .execute(pool)
    .await
    .context("inserting control binding")?;
    Ok(get(pool, id).await?.expect("just inserted"))
}

/// Update a binding. The credential follows the same three-way rule as a channel's:
/// a vault reference replaces it, `Some("")` clears it, `None` leaves it alone.
pub async fn update(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &BindingInput,
) -> anyhow::Result<bool> {
    let config = input.config.clone();
    let n = match (input.secret_vault_item, &input.secret) {
        (Some(item), _) => {
            sqlx::query(
                "UPDATE control_bindings SET name=$2, channel_id=$3, agent_id=$4, token_id=$5, \
                 enabled=$6, config=COALESCE($7, config), \
                 provider_id = CASE WHEN $9 THEN $10 ELSE provider_id END, \
                 model = COALESCE($11, model), secret_nonce=NULL, secret_cipher=NULL, \
                 secret_vault_item=$8, updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(&input.name)
            .bind(input.channel_id)
            .bind(input.agent_id)
            .bind(input.token_id)
            .bind(input.enabled)
            .bind(config)
            .bind(item)
            .bind(input.provider_id.is_some())
            .bind(input.provider_id.flatten())
            .bind(input.model.as_deref())
            .execute(pool)
            .await
            .context("updating control binding + vault reference")?
            .rows_affected()
        }
        (None, None) => {
            sqlx::query(
                "UPDATE control_bindings SET name=$2, channel_id=$3, agent_id=$4, token_id=$5, \
                 enabled=$6, config=COALESCE($7, config), \
                 provider_id = CASE WHEN $8 THEN $9 ELSE provider_id END, \
                 model = COALESCE($10, model), updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(&input.name)
            .bind(input.channel_id)
            .bind(input.agent_id)
            .bind(input.token_id)
            .bind(input.enabled)
            .bind(config)
            .bind(input.provider_id.is_some())
            .bind(input.provider_id.flatten())
            .bind(input.model.as_deref())
            .execute(pool)
            .await
            .context("updating control binding")?
            .rows_affected()
        }
        (None, Some(secret)) => {
            let (nonce, ct) = if secret.is_empty() {
                (None, None)
            } else {
                let (n, c) = cipher.seal(secret)?;
                (Some(n), Some(c))
            };
            sqlx::query(
                "UPDATE control_bindings SET name=$2, channel_id=$3, agent_id=$4, token_id=$5, \
                 enabled=$6, config=COALESCE($7, config), \
                 provider_id = CASE WHEN $10 THEN $11 ELSE provider_id END, \
                 model = COALESCE($12, model), secret_nonce=$8, secret_cipher=$9, \
                 secret_vault_item=NULL, updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(&input.name)
            .bind(input.channel_id)
            .bind(input.agent_id)
            .bind(input.token_id)
            .bind(input.enabled)
            .bind(config)
            .bind(nonce)
            .bind(ct)
            .bind(input.provider_id.is_some())
            .bind(input.provider_id.flatten())
            .bind(input.model.as_deref())
            .execute(pool)
            .await
            .context("updating control binding + credential")?
            .rows_affected()
        }
    };
    Ok(n > 0)
}

pub async fn set_enabled(pool: &PgPool, id: Uuid, enabled: bool) -> anyhow::Result<bool> {
    let n = sqlx::query("UPDATE control_bindings SET enabled=$2, updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(enabled)
        .execute(pool)
        .await
        .context("toggling control binding")?
        .rows_affected();
    Ok(n > 0)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM control_bindings WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting control binding")?
        .rows_affected();
    Ok(n > 0)
}

/// Record how the transport is doing, so the UI can show a binding that is connected
/// apart from one whose token the platform rejected.
pub async fn record_result(
    pool: &PgPool,
    id: Uuid,
    ok: bool,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE control_bindings SET last_ok=$2, last_error=$3, last_seen_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(ok)
    .bind(error)
    .execute(pool)
    .await
    .context("recording control binding status")?;
    Ok(())
}

// ── Transport side ───────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ActiveRow {
    id: Uuid,
    name: String,
    channel_id: Uuid,
    channel_kind: String,
    agent_id: Uuid,
    provider_id: Option<Uuid>,
    model: String,
    token_id: Uuid,
    config: Value,
    secret_nonce: Option<Vec<u8>>,
    secret_cipher: Option<Vec<u8>>,
    secret_vault_item: Option<Uuid>,
}

/// Every binding a worker should be listening on: enabled, on a channel kind that can
/// receive, and pointing at a token that is allowed to be reached from outside and has not
/// expired. The `external` flag is filtered in SQL rather than checked after loading, so a
/// token that lost the flag stops the transport instead of merely refusing its calls.
pub async fn load_active(
    pool: &PgPool,
    cipher: &SecretCipher,
) -> anyhow::Result<Vec<ActiveBinding>> {
    let rows = sqlx::query_as::<_, ActiveRow>(
        "SELECT cb.id, cb.name, cb.channel_id, nc.kind AS channel_kind, cb.agent_id, \
                cb.provider_id, cb.model, cb.token_id, cb.config, \
                COALESCE(vi.nonce, cb.secret_nonce) AS secret_nonce, \
                COALESCE(vi.ciphertext, cb.secret_cipher) AS secret_cipher, \
                cb.secret_vault_item \
         FROM control_bindings cb \
         JOIN notif_channels nc ON nc.id = cb.channel_id \
         JOIN mcp_tokens t ON t.id = cb.token_id \
         LEFT JOIN vault_items vi ON vi.id = cb.secret_vault_item \
         WHERE cb.enabled AND t.external \
           AND (t.expires_at IS NULL OR t.expires_at > now()) \
         ORDER BY cb.created_at",
    )
    .fetch_all(pool)
    .await
    .context("loading active control bindings")?;

    let mut out = Vec::with_capacity(rows.len());
    let mut vault_items = Vec::new();
    for r in rows {
        let secret = match (r.secret_nonce, r.secret_cipher) {
            (Some(n), Some(c)) => {
                if let Some(item) = r.secret_vault_item {
                    vault_items.push(item);
                }
                Some(cipher.open(&n, &c)?)
            }
            _ => None,
        };
        out.push(ActiveBinding {
            id: r.id,
            name: r.name,
            channel_id: r.channel_id,
            channel_kind: r.channel_kind,
            agent_id: r.agent_id,
            provider_id: r.provider_id,
            model: r.model,
            token_id: r.token_id,
            config: r.config,
            secret,
        });
    }
    crate::vault::bump_vaults_for_items(pool, &vault_items).await?;
    Ok(out)
}

// ── Senders ──────────────────────────────────────────────────────────────────

pub async fn list_senders(pool: &PgPool, binding_id: Uuid) -> anyhow::Result<Vec<Sender>> {
    sqlx::query_as::<_, Sender>(
        "SELECT sender_id, label, paired_at FROM control_senders \
         WHERE binding_id = $1 ORDER BY paired_at",
    )
    .bind(binding_id)
    .fetch_all(pool)
    .await
    .context("listing control senders")
}

/// Whether this platform user may drive this binding. The one authority check every
/// inbound message passes through.
pub async fn allows(pool: &PgPool, binding_id: Uuid, sender_id: &str) -> anyhow::Result<bool> {
    let found: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM control_senders WHERE binding_id = $1 AND sender_id = $2",
    )
    .bind(binding_id)
    .bind(sender_id)
    .fetch_optional(pool)
    .await
    .context("checking control sender")?;
    Ok(found.is_some())
}

pub async fn add_sender(
    pool: &PgPool,
    binding_id: Uuid,
    sender_id: &str,
    label: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO control_senders (binding_id, sender_id, label) VALUES ($1,$2,$3) \
         ON CONFLICT (binding_id, sender_id) DO UPDATE SET label = EXCLUDED.label",
    )
    .bind(binding_id)
    .bind(sender_id)
    .bind(label)
    .execute(pool)
    .await
    .context("adding control sender")?;
    Ok(())
}

pub async fn remove_sender(
    pool: &PgPool,
    binding_id: Uuid,
    sender_id: &str,
) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM control_senders WHERE binding_id=$1 AND sender_id=$2")
        .bind(binding_id)
        .bind(sender_id)
        .execute(pool)
        .await
        .context("removing control sender")?
        .rows_affected();
    Ok(n > 0)
}

/// Point a binding at a provider and/or a model. `provider_id` absent leaves the stored
/// one alone, `Some(None)` clears it back to inheriting the agent's; `model` follows the
/// same shape with the empty string as "inherit". Used by the in-chat `/provider` and
/// `/model` commands, which change the binding rather than one conversation: the person
/// typing them is choosing how this chat answers from now on.
pub async fn set_model(
    pool: &PgPool,
    id: Uuid,
    provider_id: Option<Option<Uuid>>,
    model: Option<&str>,
) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "UPDATE control_bindings SET \
         provider_id = CASE WHEN $2 THEN $3 ELSE provider_id END, \
         model = COALESCE($4, model), updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(provider_id.is_some())
    .bind(provider_id.flatten())
    .bind(model)
    .execute(pool)
    .await
    .context("setting control binding model")?
    .rows_affected();
    Ok(n > 0)
}

// ── Pairing ──────────────────────────────────────────────────────────────────

/// Mint a short code the user reads in Settings and types into the chat, binding whoever
/// sends it as an allowed sender.
///
/// Kept in clear, unlike every other credential here, because it exists to be read off a
/// screen. What makes it safe is not entropy but lifetime and single use: it expires after
/// [`pair_ttl_minutes`] and is spent the moment it is redeemed.
pub async fn mint_pair_code(pool: &PgPool, binding_id: Uuid) -> anyhow::Result<Option<String>> {
    let ttl = pair_ttl_minutes(pool).await?;
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    // Digits only: it gets typed on a phone keyboard, in a hurry.
    let code = format!("{:06}", u32::from_be_bytes(bytes) % 1_000_000);
    let n = sqlx::query(
        "UPDATE control_bindings SET pair_code=$2, \
         pair_expires_at = now() + make_interval(mins => $3), updated_at=now() WHERE id=$1",
    )
    .bind(binding_id)
    .bind(&code)
    .bind(ttl as i32)
    .execute(pool)
    .await
    .context("minting pairing code")?
    .rows_affected();
    Ok((n > 0).then_some(code))
}

/// Spend a pairing code: on a match the sender is added to the allowlist and the code is
/// cleared in the same statement, so two people racing on the same code cannot both pair.
pub async fn redeem_pair_code(
    pool: &PgPool,
    binding_id: Uuid,
    code: &str,
    sender_id: &str,
    label: &str,
) -> anyhow::Result<bool> {
    let claimed = sqlx::query(
        "UPDATE control_bindings SET pair_code=NULL, pair_expires_at=NULL, updated_at=now() \
         WHERE id=$1 AND pair_code=$2 AND pair_expires_at > now()",
    )
    .bind(binding_id)
    .bind(code.trim())
    .execute(pool)
    .await
    .context("redeeming pairing code")?
    .rows_affected()
        > 0;
    if claimed {
        add_sender(pool, binding_id, sender_id, label).await?;
    }
    Ok(claimed)
}

/// Whether a pairing code is outstanding right now (the transport only looks for a code in
/// a message when one was actually minted).
pub async fn pairing_open(pool: &PgPool, binding_id: Uuid) -> anyhow::Result<bool> {
    // COALESCE, because a row whose expiry is NULL would decode as a NULL bool rather
    // than as "not open".
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT COALESCE(pair_expires_at > now(), false) FROM control_bindings \
         WHERE id=$1 AND pair_code IS NOT NULL",
    )
    .bind(binding_id)
    .fetch_optional(pool)
    .await
    .context("checking pairing state")?;
    Ok(row.is_some_and(|(open,)| open))
}

// ── Chat ↔ conversation ──────────────────────────────────────────────────────

/// The agent conversation backing one chat, if it still exists. A conversation deleted
/// from the app leaves no row (ON DELETE CASCADE), so the next message starts a fresh one.
pub async fn conversation_for(
    pool: &PgPool,
    binding_id: Uuid,
    chat_id: &str,
) -> anyhow::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT conversation_id FROM control_chats WHERE binding_id=$1 AND chat_id=$2",
    )
    .bind(binding_id)
    .bind(chat_id)
    .fetch_optional(pool)
    .await
    .context("loading chat conversation")?;
    Ok(row.map(|(id,)| id))
}

pub async fn bind_conversation(
    pool: &PgPool,
    binding_id: Uuid,
    chat_id: &str,
    conversation_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO control_chats (binding_id, chat_id, conversation_id) VALUES ($1,$2,$3) \
         ON CONFLICT (binding_id, chat_id) DO UPDATE \
         SET conversation_id = EXCLUDED.conversation_id, updated_at = now()",
    )
    .bind(binding_id)
    .bind(chat_id)
    .bind(conversation_id)
    .execute(pool)
    .await
    .context("binding chat conversation")?;
    Ok(())
}

/// Forget a chat's conversation, so the next message starts from nothing ("/new").
pub async fn clear_conversation(
    pool: &PgPool,
    binding_id: Uuid,
    chat_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM control_chats WHERE binding_id=$1 AND chat_id=$2")
        .bind(binding_id)
        .bind(chat_id)
        .execute(pool)
        .await
        .context("clearing chat conversation")?;
    Ok(())
}

// ── Transport cursor ─────────────────────────────────────────────────────────

pub async fn get_cursor(pool: &PgPool, binding_id: Uuid) -> anyhow::Result<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT cursor FROM control_cursors WHERE binding_id=$1")
            .bind(binding_id)
            .fetch_optional(pool)
            .await
            .context("loading control cursor")?;
    Ok(row.map(|(c,)| c).unwrap_or_default())
}

/// Persist how far the transport has read. Written after a message is answered, so a crash
/// re-delivers at most the message in flight instead of the whole backlog.
pub async fn set_cursor(pool: &PgPool, binding_id: Uuid, cursor: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO control_cursors (binding_id, cursor) VALUES ($1,$2) \
         ON CONFLICT (binding_id) DO UPDATE SET cursor = EXCLUDED.cursor, updated_at = now()",
    )
    .bind(binding_id)
    .bind(cursor)
    .execute(pool)
    .await
    .context("saving control cursor")?;
    Ok(())
}
