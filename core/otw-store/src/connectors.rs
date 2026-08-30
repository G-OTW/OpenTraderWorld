//! Market-data connectors — the centralized data broker.
//!
//! A connector is a user-named instance of a provider (Binance, EODHD…): credentials,
//! an optional request quota (scope `histconn:<id>`, see [`crate::api_quota`]) and a set
//! of **module grants** attach to it, so several accounts/keys of one provider can
//! coexist and each is offered only where the user allowed it.
//!
//! Grants live in `connector_modules`, one row per allowed module id, with the wildcard
//! `'*'` meaning "every data module, present and future". Before 0067 the same idea was
//! a single owning `scope` column, which forced the user to re-create — key included —
//! one connector per module.
//!
//! Tables are still named `histdata_*` (that is where they were born); only the reach
//! changed.

use anyhow::Context;
use serde::Serialize;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::SecretCipher;

/// Grant value that matches every module.
pub const ALL_MODULES: &str = "*";

/// A connector row with the modules it is granted to.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConnectorRow {
    pub id: Uuid,
    pub provider: String,
    pub name: String,
    /// Granted module ids, or `["*"]` for every data module.
    pub modules: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl ConnectorRow {
    /// Whether this connector may be used by `module`.
    pub fn allows(&self, module: &str) -> bool {
        self.modules.iter().any(|m| m == ALL_MODULES || m == module)
    }
}

const CONNECTOR_COLS: &str = "c.id, c.provider, c.name, c.created_at, \
     COALESCE((SELECT array_agg(m.module ORDER BY m.module) FROM connector_modules m \
               WHERE m.connector_id = c.id), '{}') AS modules";

/// Every connector, oldest first per provider (the settings page and the broker admin).
pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<ConnectorRow>> {
    let sql = format!(
        "SELECT {CONNECTOR_COLS} FROM histdata_connectors c ORDER BY c.provider, c.created_at"
    );
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing connectors")?)
}

/// Connectors a module is allowed to use (its own grants plus the wildcard ones).
pub async fn list_for_module(pool: &PgPool, module: &str) -> anyhow::Result<Vec<ConnectorRow>> {
    let sql = format!(
        "SELECT {CONNECTOR_COLS} FROM histdata_connectors c \
         WHERE EXISTS (SELECT 1 FROM connector_modules m \
                       WHERE m.connector_id = c.id AND (m.module = $1 OR m.module = '*')) \
         ORDER BY c.provider, c.created_at"
    );
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .bind(module)
        .fetch_all(pool)
        .await
        .context("listing connectors for module")?)
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<ConnectorRow>> {
    let sql = format!("SELECT {CONNECTOR_COLS} FROM histdata_connectors c WHERE c.id = $1");
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Create a connector and its module grants (empty grants = usable nowhere yet).
pub async fn create(
    pool: &PgPool,
    provider: &str,
    name: &str,
    modules: &[String],
) -> anyhow::Result<ConnectorRow> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO histdata_connectors (id, provider, name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(provider)
        .bind(name)
        .execute(&mut *tx)
        .await
        .context("creating connector")?;
    for m in modules {
        sqlx::query(
            "INSERT INTO connector_modules (connector_id, module) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(m)
        .execute(&mut *tx)
        .await
        .context("granting connector module")?;
    }
    tx.commit().await?;
    Ok(get(pool, id).await?.expect("just inserted"))
}

pub async fn rename(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res =
        sqlx::query("UPDATE histdata_connectors SET name = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(name)
            .execute(pool)
            .await
            .context("renaming connector")?;
    Ok(res.rows_affected() > 0)
}

/// Replace a connector's module grants wholesale.
pub async fn set_modules(pool: &PgPool, id: Uuid, modules: &[String]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM connector_modules WHERE connector_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("clearing connector modules")?;
    for m in modules {
        sqlx::query(
            "INSERT INTO connector_modules (connector_id, module) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(m)
        .execute(&mut *tx)
        .await
        .context("granting connector module")?;
    }
    tx.commit().await?;
    Ok(())
}

/// Delete a connector; its credentials and grants cascade, jobs keep history
/// (connector_id NULL).
pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histdata_connectors WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Fallback connector for a provider (oldest = the seeded default), for provider-addressed
/// calls that name no connector: queued jobs, live seeds, MCP downloads.
pub async fn default_for(pool: &PgPool, provider: &str) -> anyhow::Result<Option<ConnectorRow>> {
    let sql = format!(
        "SELECT {CONNECTOR_COLS} FROM histdata_connectors c \
         WHERE c.provider = $1 ORDER BY c.created_at LIMIT 1"
    );
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .bind(provider)
        .fetch_optional(pool)
        .await?)
}

// ── Credentials (encrypted) ─────────────────────────────────────────────────────

/// Store (or replace) a named secret for a connector. Plaintext is sealed immediately
/// and never persisted or returned. `provider` is denormalized alongside for legibility.
/// A direct value replaces any vault reference the credential had.
pub async fn set_cred(
    pool: &PgPool,
    cipher: &SecretCipher,
    connector_id: Uuid,
    provider: &str,
    name: &str,
    plaintext: &str,
) -> anyhow::Result<()> {
    let (nonce, ciphertext) = cipher.seal(plaintext)?;
    sqlx::query(
        "INSERT INTO histdata_provider_creds (id, connector_id, provider, name, nonce, ciphertext) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (connector_id, name) \
         DO UPDATE SET nonce = EXCLUDED.nonce, ciphertext = EXCLUDED.ciphertext, \
                       vault_item_id = NULL, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(connector_id)
    .bind(provider)
    .bind(name)
    .bind(nonce)
    .bind(ciphertext)
    .execute(pool)
    .await
    .context("storing provider credential")?;
    Ok(())
}

/// Plug a centralized vault item as a connector credential (no local sealed copy is kept).
pub async fn set_cred_ref(
    pool: &PgPool,
    connector_id: Uuid,
    provider: &str,
    name: &str,
    vault_item_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO histdata_provider_creds (id, connector_id, provider, name, nonce, ciphertext, vault_item_id) \
         VALUES ($1, $2, $3, $4, NULL, NULL, $5) \
         ON CONFLICT (connector_id, name) \
         DO UPDATE SET nonce = NULL, ciphertext = NULL, vault_item_id = $5, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(connector_id)
    .bind(provider)
    .bind(name)
    .bind(vault_item_id)
    .execute(pool)
    .await
    .context("storing provider credential reference")?;
    Ok(())
}

pub async fn delete_cred(pool: &PgPool, connector_id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res =
        sqlx::query("DELETE FROM histdata_provider_creds WHERE connector_id = $1 AND name = $2")
            .bind(connector_id)
            .bind(name)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() > 0)
}

/// Names of the secrets set for a connector (never the values).
pub async fn list_cred_names(pool: &PgPool, connector_id: Uuid) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM histdata_provider_creds WHERE connector_id = $1 ORDER BY name",
    )
    .bind(connector_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Credential names + their vault reference (if plugged), for the connector form UI.
pub async fn list_cred_meta(
    pool: &PgPool,
    connector_id: Uuid,
) -> anyhow::Result<Vec<(String, Option<Uuid>)>> {
    Ok(sqlx::query_as(
        "SELECT name, vault_item_id FROM histdata_provider_creds WHERE connector_id = $1 ORDER BY name",
    )
    .bind(connector_id)
    .fetch_all(pool)
    .await?)
}

/// Decrypt all secrets for a connector into a name→plaintext map (worker use only).
/// Vault-plugged credentials resolve from the vault; each distinct vault used is counted
/// once against its vault-wide quota (one job run = one logical request).
pub async fn load_creds(
    pool: &PgPool,
    cipher: &SecretCipher,
    connector_id: Uuid,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>, Option<Uuid>)> = sqlx::query_as(
        "SELECT c.name, COALESCE(vi.nonce, c.nonce), COALESCE(vi.ciphertext, c.ciphertext), \
                c.vault_item_id \
         FROM histdata_provider_creds c LEFT JOIN vault_items vi ON vi.id = c.vault_item_id \
         WHERE c.connector_id = $1",
    )
    .bind(connector_id)
    .fetch_all(pool)
    .await?;
    let mut map = std::collections::HashMap::new();
    let mut vault_items = Vec::new();
    for (name, nonce, ct, ref_id) in rows {
        if let (Some(nonce), Some(ct)) = (nonce, ct) {
            map.insert(name, cipher.open(&nonce, &ct)?);
            if let Some(id) = ref_id {
                vault_items.push(id);
            }
        }
    }
    crate::vault::bump_vaults_for_items(pool, &vault_items).await?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(modules: &[&str]) -> ConnectorRow {
        ConnectorRow {
            id: Uuid::nil(),
            provider: "binance".into(),
            name: "Binance".into(),
            modules: modules.iter().map(|s| s.to_string()).collect(),
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn grants_match_module_or_wildcard() {
        assert!(row(&["histdata"]).allows("histdata"));
        assert!(!row(&["histdata"]).allows("watchlists"));
        // The wildcard covers modules that did not exist when the grant was written.
        assert!(row(&["*"]).allows("watchlists"));
        assert!(row(&["*"]).allows("some-future-module"));
        // No grant at all = usable nowhere.
        assert!(!row(&[]).allows("histdata"));
    }
}
