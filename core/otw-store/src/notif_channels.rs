//! External notification channels — one list shared by every module that notifies.
//!
//! A fired reminder, a triggered price alert, new bulk mail or an inbound webhook each
//! write an in-app notification; in addition it is pushed to the *enabled* external
//! channels the user has configured (email/telegram/slack/discord) **and granted to that
//! module**. Integration is free for the host — the user brings their own account and
//! credentials.
//!
//! Grants live in `channel_modules`, one row per allowed module id, with the wildcard
//! `'*'` meaning "every notifying module, present and future" — the same shape the data
//! broker uses for connectors. Before 0088 a channel was global: `enabled` was the only
//! dial, so the user could not route price alerts to Telegram and keep email for the rest.
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

/// Grant value that matches every notifying module.
pub const ALL_MODULES: &str = "*";

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
    /// Vault item the secret resolves from, when plugged from the centralized vault.
    pub secret_vault_item: Option<Uuid>,
    pub enabled: bool,
    /// Granted module ids, or `["*"]` for every notifying module.
    pub modules: Vec<String>,
    pub last_ok: Option<bool>,
    pub last_error: Option<String>,
    #[serde(with = "ts_opt")]
    pub last_sent_at: Option<OffsetDateTime>,
}

impl Channel {
    /// Whether this channel may be pushed to on behalf of `module`.
    pub fn allows(&self, module: &str) -> bool {
        self.modules.iter().any(|m| m == ALL_MODULES || m == module)
    }
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
    /// Reference a centralized vault item instead of pasting the secret. Takes
    /// precedence over `secret`; setting a direct secret clears the reference.
    #[serde(default)]
    pub secret_vault_item: Option<Uuid>,
    /// Module ids allowed to push to this channel, or `["*"]` for all. Absent on update
    /// leaves the grants untouched — toggling `enabled` must not silently re-grant.
    #[serde(default)]
    pub modules: Option<Vec<String>>,
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
    secret_vault_item: Option<Uuid>,
    enabled: bool,
    modules: Vec<String>,
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
        has_secret: r.secret_nonce.is_some() || r.secret_vault_item.is_some(),
        secret_vault_item: r.secret_vault_item,
        enabled: r.enabled,
        modules: r.modules,
        last_ok: r.last_ok,
        last_error: r.last_error,
        last_sent_at: r.last_sent_at,
    }
}

const COLUMNS: &str = "nc.id, nc.kind, nc.name, nc.config, nc.secret_nonce, nc.secret_vault_item, \
     nc.enabled, nc.last_ok, nc.last_error, nc.last_sent_at, \
     COALESCE((SELECT array_agg(m.module ORDER BY m.module) FROM channel_modules m \
               WHERE m.channel_id = nc.id), '{}') AS modules";

/// Every channel, oldest first (the settings admin and every module's picker).
pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Channel>> {
    let sql = format!("SELECT {COLUMNS} FROM notif_channels nc ORDER BY nc.created_at");
    let rows = sqlx::query_as::<_, ChannelRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing notif channels")?;
    Ok(rows.into_iter().map(row_to_channel).collect())
}

/// Channels a module may push to (its own grants plus the wildcard ones). Enabled or
/// not — the admin still has to show a granted-but-off channel.
pub async fn list_for_module(pool: &PgPool, module: &str) -> anyhow::Result<Vec<Channel>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM notif_channels nc \
         WHERE EXISTS (SELECT 1 FROM channel_modules m \
                       WHERE m.channel_id = nc.id AND (m.module = $1 OR m.module = '*')) \
         ORDER BY nc.created_at"
    );
    let rows = sqlx::query_as::<_, ChannelRow>(sqlx::AssertSqlSafe(sql))
        .bind(module)
        .fetch_all(pool)
        .await
        .context("listing notif channels for module")?;
    Ok(rows.into_iter().map(row_to_channel).collect())
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Channel>> {
    let sql = format!("SELECT {COLUMNS} FROM notif_channels nc WHERE nc.id = $1");
    let row = sqlx::query_as::<_, ChannelRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading notif channel")?;
    Ok(row.map(row_to_channel))
}

/// Replace a channel's module grants wholesale.
pub async fn set_modules(pool: &PgPool, id: Uuid, modules: &[String]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM channel_modules WHERE channel_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("clearing channel modules")?;
    for m in modules {
        sqlx::query(
            "INSERT INTO channel_modules (channel_id, module) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(m)
        .execute(&mut *tx)
        .await
        .context("granting channel module")?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn add(
    pool: &PgPool,
    cipher: &SecretCipher,
    input: &ChannelInput,
) -> anyhow::Result<Channel> {
    let id = Uuid::new_v4();
    // A vault reference wins over a pasted secret: never keep both.
    let (nonce, ct) = match input.secret.as_deref().filter(|s| !s.is_empty()) {
        Some(plain) if input.secret_vault_item.is_none() => {
            let (n, c) = cipher.seal(plain)?;
            (Some(n), Some(c))
        }
        _ => (None, None),
    };
    // Absent grants on create = every module: a channel plugged in from Settings with no
    // opinion is expected to receive everything, which is also the pre-0088 behaviour.
    let modules = input
        .modules
        .clone()
        .unwrap_or_else(|| vec![ALL_MODULES.to_string()]);
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO notif_channels (id, kind, name, config, secret_nonce, secret_cipher, enabled, secret_vault_item) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(&input.kind)
    .bind(&input.name)
    .bind(&input.config)
    .bind(nonce)
    .bind(ct)
    .bind(input.enabled)
    .bind(input.secret_vault_item)
    .execute(&mut *tx)
    .await
    .context("inserting notif channel")?;
    for m in &modules {
        sqlx::query(
            "INSERT INTO channel_modules (channel_id, module) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(m)
        .execute(&mut *tx)
        .await
        .context("granting channel module")?;
    }
    tx.commit().await?;
    Ok(get(pool, id).await?.expect("just inserted"))
}

/// Update a channel. If `input.secret_vault_item` is set the channel is plugged to that
/// vault item (clearing any locally sealed secret). Otherwise `input.secret` applies:
/// `None` leaves the stored secret untouched; `Some("")` clears it (and any vault
/// reference); `Some(v)` reseals it locally (and clears any vault reference).
///
/// `input.modules`, when present, replaces the grants; absent leaves them alone.
pub async fn update(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &ChannelInput,
) -> anyhow::Result<bool> {
    let found = update_row(pool, cipher, id, input).await?;
    if found {
        if let Some(modules) = &input.modules {
            set_modules(pool, id, modules).await?;
        }
    }
    Ok(found)
}

async fn update_row(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
    input: &ChannelInput,
) -> anyhow::Result<bool> {
    match (input.secret_vault_item, &input.secret) {
        // Plug to a vault item; drop the local sealed copy.
        (Some(item), _) => {
            let n = sqlx::query(
                "UPDATE notif_channels SET kind=$2, name=$3, config=$4, enabled=$5, \
                 secret_nonce=NULL, secret_cipher=NULL, secret_vault_item=$6, updated_at=now() \
                 WHERE id=$1",
            )
            .bind(id)
            .bind(&input.kind)
            .bind(&input.name)
            .bind(&input.config)
            .bind(input.enabled)
            .bind(item)
            .execute(pool)
            .await
            .context("updating notif channel + vault reference")?
            .rows_affected();
            Ok(n > 0)
        }
        // Leave the existing secret in place; update everything else.
        (None, None) => {
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
        (None, Some(secret)) => {
            let (nonce, ct) = if secret.is_empty() {
                (None, None)
            } else {
                let (n, c) = cipher.seal(secret)?;
                (Some(n), Some(c))
            };
            let n = sqlx::query(
                "UPDATE notif_channels SET kind=$2, name=$3, config=$4, enabled=$5, \
                 secret_nonce=$6, secret_cipher=$7, secret_vault_item=NULL, updated_at=now() \
                 WHERE id=$1",
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

/// Dispatch-side columns: the sealed secret resolves from the vault when the channel is
/// plugged to a vault item, otherwise from the local copy. Qualified — vault_items shares
/// `id`/`name` with notif_channels.
const SECRET_SELECT: &str = "SELECT nc.id, nc.kind, nc.name, nc.config, \
     COALESCE(vi.nonce, nc.secret_nonce) AS secret_nonce, \
     COALESCE(vi.ciphertext, nc.secret_cipher) AS secret_cipher, \
     nc.secret_vault_item \
     FROM notif_channels nc LEFT JOIN vault_items vi ON vi.id = nc.secret_vault_item";

/// Load every enabled channel `module` is granted, with its secret decrypted, for
/// dispatch. Filtering in SQL keeps us from decrypting secrets we have no business
/// using. Each distinct vault used is counted once against its vault-wide quota.
pub async fn load_enabled(
    pool: &PgPool,
    cipher: &SecretCipher,
    module: &str,
) -> anyhow::Result<Vec<ChannelWithSecret>> {
    let sql = format!(
        "{SECRET_SELECT} WHERE nc.enabled \
         AND EXISTS (SELECT 1 FROM channel_modules m \
                     WHERE m.channel_id = nc.id AND (m.module = $1 OR m.module = '*'))"
    );
    let rows = sqlx::query_as::<_, ChannelSecretRow>(sqlx::AssertSqlSafe(sql))
        .bind(module)
        .fetch_all(pool)
        .await
        .context("loading enabled notif channels")?;
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
        out.push(ChannelWithSecret {
            id: r.id,
            kind: r.kind,
            name: r.name,
            config: r.config,
            secret,
        });
    }
    crate::vault::bump_vaults_for_items(pool, &vault_items).await?;
    Ok(out)
}

/// Load a single channel with its secret decrypted (used by "send test message").
pub async fn load_with_secret(
    pool: &PgPool,
    cipher: &SecretCipher,
    id: Uuid,
) -> anyhow::Result<Option<ChannelWithSecret>> {
    let sql = format!("{SECRET_SELECT} WHERE nc.id = $1");
    let row = sqlx::query_as::<_, ChannelSecretRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("loading notif channel with secret")?;
    match row {
        None => Ok(None),
        Some(r) => {
            let secret = match (r.secret_nonce, r.secret_cipher) {
                (Some(n), Some(c)) => {
                    if let Some(item) = r.secret_vault_item {
                        crate::vault::bump_vaults_for_items(pool, &[item]).await?;
                    }
                    Some(cipher.open(&n, &c)?)
                }
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
    secret_vault_item: Option<Uuid>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(modules: &[&str]) -> Channel {
        Channel {
            id: Uuid::nil(),
            kind: "telegram".into(),
            name: "Telegram".into(),
            config: empty_obj(),
            has_secret: true,
            secret_vault_item: None,
            enabled: true,
            modules: modules.iter().map(|s| s.to_string()).collect(),
            last_ok: None,
            last_error: None,
            last_sent_at: None,
        }
    }

    #[test]
    fn grants_match_module_or_wildcard() {
        assert!(ch(&["watchlists"]).allows("watchlists"));
        assert!(!ch(&["watchlists"]).allows("remindme"));
        // The wildcard covers modules that did not exist when the grant was written.
        assert!(ch(&["*"]).allows("remindme"));
        assert!(ch(&["*"]).allows("some-future-module"));
        // No grant at all = a channel nothing can push to, even while enabled.
        assert!(!ch(&[]).allows("remindme"));
    }
}
