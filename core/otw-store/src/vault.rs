//! Centralized secrets vault.
//!
//! A vault groups the credentials of one external service (e.g. "Binance"); each item is
//! one named value (`apikey`, `secretkey`, ...) sealed with the shared XChaCha20-Poly1305
//! cipher. Values are write-only: set/replace over the API, decrypted only inside outbound
//! call paths (`open_item` / consumer joins), never serialized back to a client.
//!
//! Request tracking reuses `api_quotas` under a `vault:<uuid>` scope — usage is counted
//! per *vault*, not per item: every resolution of any item bumps the vault's counter once.
//! Like the rest of the quota system it observes and displays; nothing throttles.
//!
//! Consumers (feed_secrets, histdata_provider_creds, agent_providers, agent_mcp_servers,
//! notif_channels) reference an item by id. Deleting an item or a vault that is still
//! referenced is refused (`item_usage` / `vault_usage` pre-checks, FK as backstop).

use anyhow::Context;
use serde::Serialize;
use sqlx::types::time::OffsetDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use crate::api_quota;
use crate::crypto::SecretCipher;

/// A vault row (never carries values).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Vault {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// An item row: name + bookkeeping + how many consumers currently reference it.
/// The sealed value is deliberately absent.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct VaultItem {
    pub id: Uuid,
    pub vault_id: Uuid,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub in_use: i64,
}

/// Sum of references to one item across every consumer table.
const ITEM_USE_SQL: &str = "(SELECT count(*) FROM feed_secrets WHERE vault_item_id = vault_items.id) \
     + (SELECT count(*) FROM histdata_provider_creds WHERE vault_item_id = vault_items.id) \
     + (SELECT count(*) FROM agent_providers WHERE api_key_vault_item = vault_items.id) \
     + (SELECT count(*) FROM agent_mcp_servers WHERE auth_vault_item = vault_items.id) \
     + (SELECT count(*) FROM notif_channels WHERE secret_vault_item = vault_items.id)";

pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Vault>> {
    sqlx::query_as("SELECT id, name, created_at, updated_at FROM vaults ORDER BY lower(name)")
        .fetch_all(pool)
        .await
        .context("listing vaults")
}

/// Every item of every vault, with live reference counts (one round-trip for the UI).
pub async fn list_items(pool: &PgPool) -> anyhow::Result<Vec<VaultItem>> {
    let sql = format!(
        "SELECT id, vault_id, name, updated_at, ({ITEM_USE_SQL}) AS in_use \
         FROM vault_items ORDER BY lower(name)"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing vault items")
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Vault>> {
    sqlx::query_as("SELECT id, name, created_at, updated_at FROM vaults WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching vault")
}

/// Case-insensitive name lookup (uniqueness pre-check for a friendly 409).
pub async fn find_by_name(pool: &PgPool, name: &str) -> anyhow::Result<Option<Vault>> {
    sqlx::query_as("SELECT id, name, created_at, updated_at FROM vaults WHERE lower(name) = lower($1)")
        .bind(name)
        .fetch_optional(pool)
        .await
        .context("looking up vault by name")
}

pub async fn create(pool: &PgPool, name: &str) -> anyhow::Result<Vault> {
    sqlx::query_as(
        "INSERT INTO vaults (id, name) VALUES ($1, $2) RETURNING id, name, created_at, updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .fetch_one(pool)
    .await
    .context("creating vault")
}

pub async fn rename(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<bool> {
    let n = sqlx::query("UPDATE vaults SET name = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .context("renaming vault")?
        .rows_affected();
    Ok(n > 0)
}

/// References to any item of this vault, across every consumer table.
pub async fn vault_usage(pool: &PgPool, id: Uuid) -> anyhow::Result<i64> {
    let sql = format!(
        "SELECT COALESCE(SUM({ITEM_USE_SQL}), 0)::BIGINT FROM vault_items WHERE vault_id = $1"
    );
    let (n,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .context("counting vault usage")?;
    Ok(n)
}

pub async fn item_usage(pool: &PgPool, item_id: Uuid) -> anyhow::Result<i64> {
    let sql = format!("SELECT ({ITEM_USE_SQL})::BIGINT FROM vault_items WHERE id = $1");
    let n: Option<(i64,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(item_id)
        .fetch_optional(pool)
        .await
        .context("counting vault item usage")?;
    Ok(n.map(|r| r.0).unwrap_or(0))
}

/// Delete a vault (items cascade). Caller must have verified `vault_usage == 0`;
/// the FK on consumer tables backstops a race. The quota scope goes with it.
pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM vaults WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting vault")?
        .rows_affected();
    if n > 0 {
        api_quota::remove(pool, &quota_scope(id)).await?;
    }
    Ok(n > 0)
}

/// Create or replace an item's sealed value. Plaintext is sealed immediately and never
/// persisted or returned.
pub async fn set_item(
    pool: &PgPool,
    cipher: &SecretCipher,
    vault_id: Uuid,
    name: &str,
    plaintext: &str,
) -> anyhow::Result<VaultItem> {
    let (nonce, ciphertext) = cipher.seal(plaintext)?;
    let sql = format!(
        "INSERT INTO vault_items (id, vault_id, name, nonce, ciphertext) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (vault_id, name) \
         DO UPDATE SET nonce = EXCLUDED.nonce, ciphertext = EXCLUDED.ciphertext, updated_at = now() \
         RETURNING id, vault_id, name, updated_at, ({ITEM_USE_SQL}) AS in_use"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(vault_id)
        .bind(name)
        .bind(nonce)
        .bind(ciphertext)
        .fetch_one(pool)
        .await
        .context("storing vault item")
}

/// Delete an item. Caller must have verified `item_usage == 0` (FK backstops a race).
pub async fn delete_item(pool: &PgPool, item_id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM vault_items WHERE id = $1")
        .bind(item_id)
        .execute(pool)
        .await
        .context("deleting vault item")?
        .rows_affected();
    Ok(n > 0)
}

/// The `api_quotas` scope for a vault's request tracking.
pub fn quota_scope(vault_id: Uuid) -> String {
    format!("vault:{vault_id}")
}

/// Decrypt one item for an outbound call path and count the use against the vault's
/// quota (vault-wide, per spec — items don't have individual counters). Never expose
/// the returned plaintext over the API.
pub async fn open_item(
    pool: &PgPool,
    cipher: &SecretCipher,
    item_id: Uuid,
) -> anyhow::Result<Option<String>> {
    let row: Option<(Uuid, Vec<u8>, Vec<u8>)> =
        sqlx::query_as("SELECT vault_id, nonce, ciphertext FROM vault_items WHERE id = $1")
            .bind(item_id)
            .fetch_optional(pool)
            .await
            .context("fetching vault item")?;
    let Some((vault_id, nonce, ct)) = row else {
        return Ok(None);
    };
    let plaintext = cipher.open(&nonce, &ct)?;
    api_quota::bump(pool, &quota_scope(vault_id)).await?;
    Ok(Some(plaintext))
}

/// Resolve an item by `vault_name` + `item_name` (case-insensitive), decrypt it, and
/// count the use against the vault's quota. Used by inline `{{vault.item}}` placeholders.
/// Returns `None` if no such vault/item exists. Never expose the plaintext over the API.
pub async fn open_by_names(
    pool: &PgPool,
    cipher: &SecretCipher,
    vault_name: &str,
    item_name: &str,
) -> anyhow::Result<Option<String>> {
    let row: Option<(Uuid, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT v.id, i.nonce, i.ciphertext \
         FROM vault_items i JOIN vaults v ON v.id = i.vault_id \
         WHERE lower(v.name) = lower($1) AND lower(i.name) = lower($2)",
    )
    .bind(vault_name)
    .bind(item_name)
    .fetch_optional(pool)
    .await
    .context("resolving vault item by name")?;
    let Some((vault_id, nonce, ct)) = row else {
        return Ok(None);
    };
    let plaintext = cipher.open(&nonce, &ct)?;
    api_quota::bump(pool, &quota_scope(vault_id)).await?;
    Ok(Some(plaintext))
}

/// Count one use for each distinct vault behind `item_ids` (batch loaders that decrypt
/// via a JOIN call this instead of `open_item`). Duplicate items in one batch still
/// bump their vault once — the batch is one logical request.
pub async fn bump_vaults_for_items(pool: &PgPool, item_ids: &[Uuid]) -> anyhow::Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT DISTINCT vault_id FROM vault_items WHERE id = ANY($1)")
            .bind(item_ids)
            .fetch_all(pool)
            .await
            .context("resolving vaults for quota bump")?;
    for (vault_id,) in rows {
        api_quota::bump(pool, &quota_scope(vault_id)).await?;
    }
    Ok(())
}
