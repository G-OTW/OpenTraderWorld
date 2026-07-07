//! External notification channels for the RemindMe module.
//!
//! Each fired reminder writes an in-app notification; in addition it is pushed to every
//! *enabled* external channel the user has configured (email/telegram/slack/discord).
//! Integration is free for the host — the user brings their own account and credentials.
//!
//! Persistence only: this module stores channel config and the single sealed secret
//! (SMTP password / bot token / webhook URL), sealed with the same XChaCha20-Poly1305
//! scheme as feed secrets. Actual sending lives in otw-core (it owns the HTTP/SMTP
//! clients). Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::SecretCipher;

/// Channel kinds we support in phase 1. All are free for the host to integrate.
pub const CHANNEL_KINDS: &[&str] = &["email", "telegram", "slack", "discord"];

/// A configured external channel. The secret itself is never serialized; the API only
/// exposes whether one is set via `has_secret`.
#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub config: Value,
    /// True when a secret is stored (bot token / webhook URL / SMTP password).
    pub has_secret: bool,
    pub enabled: bool,
    pub last_ok: Option<bool>,
    pub last_error: Option<String>,
    #[serde(with = "ts_opt")]
    pub last_sent_at: Option<OffsetDateTime>,
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

/// Full channel record including the decrypted secret — used only internally by the
/// sender (otw-core). Never returned over the API.
#[derive(Debug, Clone)]
pub struct ChannelWithSecret {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub config: Value,
    pub secret: Option<String>,
}

/// Input for creating/updating a channel. The secret is optional on update: `None`
/// leaves any existing secret untouched, `Some("")` clears it, `Some(v)` replaces it.
#[derive(Debug, Deserialize)]
pub struct ChannelInput {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "empty_obj")]
    pub config: Value,
    #[serde(default)]
    pub enabled: bool,
    /// When present, (re)seal this secret. Absent → keep existing secret.
    #[serde(default)]
    pub secret: Option<String>,
}

fn empty_obj() -> Value {
    serde_json::json!({})
}

#[derive(sqlx::FromRow)]
struct ChannelRow {
    id: Uuid,
    kind: String,
    name: String,
    config: Value,
    secret_nonce: Option<Vec<u8>>,
    enabled: bool,
    last_ok: Option<bool>,
    last_error: Option<String>,
    last_sent_at: Option<OffsetDateTime>,
}

fn row_to_channel(r: ChannelRow) -> Channel {
    Channel {
        id: r.id,
        kind: r.kind,
        name: r.name,
        config: r.config,
        has_secret: r.secret_nonce.is_some(),
        enabled: r.enabled,
        last_ok: r.last_ok,
        last_error: r.last_error,
        last_sent_at: r.last_sent_at,
    }
}

const COLUMNS: &str = "id, kind, name, config, secret_nonce, enabled, last_ok, last_error, last_sent_at";

pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Channel>> {
    let sql = format!("SELECT {COLUMNS} FROM notif_channels ORDER BY created_at");
    let rows = sqlx::query_as::<_, ChannelRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing notif channels")?;
    Ok(rows.into_iter().map(row_to_channel).collect())
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Channel>> {
    let sql = format!("SELECT {COLUMNS} FROM notif_channels WHERE id = $1");
    let row = sqlx::query_as::<_, ChannelRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading notif channel")?;
    Ok(row.map(row_to_channel))
}

pub async fn add(
    pool: &PgPool,
    cipher: &SecretCipher,
    input: &ChannelInput,
) -> anyhow::Result<Channel> {
    let id = Uuid::new_v4();
    let (nonce, ct) = match input.secret.as_deref().filter(|s| !s.is_empty()) {
        Some(plain) => {
            let (n, c) = cipher.seal(plain)?;
            (Some(n), Some(c))
        }
        None => (None, None),
    };
    let sql = format!(
        "INSERT INTO notif_channels (id, kind, name, config, secret_nonce, secret_cipher, enabled) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING {COLUMNS}"
    );
    let row = sqlx::query_as::<_, ChannelRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&input.kind)
        .bind(&input.name)
        .bind(&input.config)
        .bind(nonce)
        .bind(ct)
        .bind(input.enabled)
        .fetch_one(pool)
        .await
        .context("inserting notif channel")?;
    Ok(row_to_channel(row))
}

/// Update a channel. If `input.secret` is `None` the stored secret is left untouched;
/// `Some("")` clears it; `Some(v)` reseals it.
pub async fn update(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &ChannelInput,
) -> anyhow::Result<bool> {
    match &input.secret {
        // Leave the existing secret in place; update everything else.
        None => {
            let n = sqlx::query(
                "UPDATE notif_channels SET kind=$2, name=$3, config=$4, enabled=$5, updated_at=now() \
                 WHERE id=$1",
            )
            .bind(id)
            .bind(&input.kind)
            .bind(&input.name)
            .bind(&input.config)
            .bind(input.enabled)
            .execute(pool)
            .await
            .context("updating notif channel")?
            .rows_affected();
            Ok(n > 0)
        }
        Some(secret) => {
            let (nonce, ct) = if secret.is_empty() {
                (None, None)
            } else {
                let (n, c) = cipher.seal(secret)?;
                (Some(n), Some(c))
            };
            let n = sqlx::query(
                "UPDATE notif_channels SET kind=$2, name=$3, config=$4, enabled=$5, \
                 secret_nonce=$6, secret_cipher=$7, updated_at=now() WHERE id=$1",
            )
            .bind(id)
            .bind(&input.kind)
            .bind(&input.name)
            .bind(&input.config)
            .bind(input.enabled)
            .bind(nonce)
            .bind(ct)
            .execute(pool)
            .await
            .context("updating notif channel + secret")?
            .rows_affected();
            Ok(n > 0)
        }
    }
}

pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM notif_channels WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting notif channel")?
        .rows_affected();
    Ok(n > 0)
}

/// Load every enabled channel with its secret decrypted, for dispatch. A channel whose
/// secret fails to decrypt is skipped (logged by the caller via the returned error slot).
pub async fn load_enabled(
    pool: &PgPool,
    cipher: &SecretCipher,
) -> anyhow::Result<Vec<ChannelWithSecret>> {
    let sql = format!("SELECT {COLUMNS}, secret_cipher FROM notif_channels WHERE enabled");
    let rows = sqlx::query_as::<_, ChannelSecretRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("loading enabled notif channels")?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let secret = match (r.secret_nonce, r.secret_cipher) {
            (Some(n), Some(c)) => Some(cipher.open(&n, &c)?),
            _ => None,
        };
        out.push(ChannelWithSecret {
            id: r.id,
            kind: r.kind,
            name: r.name,
            config: r.config,
            secret,
        });
    }
    Ok(out)
}

/// Load a single channel with its secret decrypted (used by "send test message").
pub async fn load_with_secret(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
) -> anyhow::Result<Option<ChannelWithSecret>> {
    let sql = format!("SELECT {COLUMNS}, secret_cipher FROM notif_channels WHERE id = $1");
    let row = sqlx::query_as::<_, ChannelSecretRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading notif channel with secret")?;
    match row {
        None => Ok(None),
        Some(r) => {
            let secret = match (r.secret_nonce, r.secret_cipher) {
                (Some(n), Some(c)) => Some(cipher.open(&n, &c)?),
                _ => None,
            };
            Ok(Some(ChannelWithSecret {
                id: r.id,
                kind: r.kind,
                name: r.name,
                config: r.config,
                secret,
            }))
        }
    }
}

#[derive(sqlx::FromRow)]
struct ChannelSecretRow {
    id: Uuid,
    kind: String,
    name: String,
    config: Value,
    secret_nonce: Option<Vec<u8>>,
    secret_cipher: Option<Vec<u8>>,
}

/// Record the outcome of a send attempt so the UI can surface per-channel status.
pub async fn record_result(
    pool: &PgPool,
    id: Uuid,
    ok: bool,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE notif_channels SET last_ok=$2, last_error=$3, last_sent_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(ok)
    .bind(error)
    .execute(pool)
    .await
    .context("recording notif channel result")?;
    Ok(())
}
