//! Broker accounts: the centralized account broker.
//!
//! A broker account is a user-named instance of a broker (Binance, Kraken…): read-only
//! credentials, non-secret settings, and a set of **module grants**, so several accounts of
//! one broker coexist and each is offered only where the user allowed it.
//!
//! Deliberately not the same list as [`crate::connectors`]. That one buys market data; this
//! one reads a portfolio. The credentials differ (a data key and an account key are issued
//! separately and one is far more sensitive), and so does the question a grant answers.
//!
//! Grants live in `broker_account_modules`, one row per allowed module id, with the
//! wildcard `'*'` meaning "every account module, present and future".

use anyhow::Context;
use serde::Serialize;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::SecretCipher;

/// Grant value that matches every module.
pub const ALL_MODULES: &str = "*";

/// A broker-account row with the modules it is granted to.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BrokerRow {
    pub id: Uuid,
    pub broker: String,
    pub name: String,
    /// Granted module ids, or `["*"]` for every account module.
    pub modules: Vec<String>,
    /// Non-secret settings (a Flex query id, a sub-account label), keyed by the field names
    /// the broker declares. Readable on purpose: an identifier is not a credential.
    pub config: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl BrokerRow {
    /// Whether this account may be used by `module`.
    pub fn allows(&self, module: &str) -> bool {
        self.modules.iter().any(|m| m == ALL_MODULES || m == module)
    }
}

const BROKER_COLS: &str = "a.id, a.broker, a.name, a.config, a.created_at, \
     COALESCE((SELECT array_agg(m.module ORDER BY m.module) FROM broker_account_modules m \
               WHERE m.account_id = a.id), '{}') AS modules";

/// Every account, oldest first per broker.
pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<BrokerRow>> {
    let sql =
        format!("SELECT {BROKER_COLS} FROM broker_accounts a ORDER BY a.broker, a.created_at");
    Ok(sqlx::query_as::<_, BrokerRow>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing broker accounts")?)
}

/// Accounts a module is allowed to use (its own grants plus the wildcard ones).
pub async fn list_for_module(pool: &PgPool, module: &str) -> anyhow::Result<Vec<BrokerRow>> {
    let sql = format!(
        "SELECT {BROKER_COLS} FROM broker_accounts a \
         WHERE EXISTS (SELECT 1 FROM broker_account_modules m \
                       WHERE m.account_id = a.id AND (m.module = $1 OR m.module = '*')) \
         ORDER BY a.broker, a.created_at"
    );
    Ok(sqlx::query_as::<_, BrokerRow>(AssertSqlSafe(sql))
        .bind(module)
        .fetch_all(pool)
        .await
        .context("listing broker accounts for module")?)
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<BrokerRow>> {
    let sql = format!("SELECT {BROKER_COLS} FROM broker_accounts a WHERE a.id = $1");
    Ok(sqlx::query_as::<_, BrokerRow>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Create an account and its module grants (empty grants = usable nowhere yet).
pub async fn create(
    pool: &PgPool,
    broker: &str,
    name: &str,
    modules: &[String],
    config: &serde_json::Value,
) -> anyhow::Result<BrokerRow> {
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO broker_accounts (id, broker, name, config) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(broker)
        .bind(name)
        .bind(config)
        .execute(&mut *tx)
        .await
        .context("creating broker account")?;
    for m in modules {
        sqlx::query(
            "INSERT INTO broker_account_modules (account_id, module) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(m)
        .execute(&mut *tx)
        .await
        .context("granting broker account module")?;
    }
    tx.commit().await?;
    Ok(get(pool, id).await?.expect("just inserted"))
}

pub async fn rename(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE broker_accounts SET name = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .context("renaming broker account")?;
    Ok(res.rows_affected() > 0)
}

/// Replace an account's non-secret settings wholesale (the API validates the field names
/// against the broker's declared list before calling this).
pub async fn set_config(
    pool: &PgPool,
    id: Uuid,
    config: &serde_json::Value,
) -> anyhow::Result<bool> {
    let res =
        sqlx::query("UPDATE broker_accounts SET config = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(config)
            .execute(pool)
            .await
            .context("updating broker account config")?;
    Ok(res.rows_affected() > 0)
}

/// Replace an account's module grants wholesale.
pub async fn set_modules(pool: &PgPool, id: Uuid, modules: &[String]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM broker_account_modules WHERE account_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .context("clearing broker account modules")?;
    for m in modules {
        sqlx::query(
            "INSERT INTO broker_account_modules (account_id, module) VALUES ($1, $2) \
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(m)
        .execute(&mut *tx)
        .await
        .context("granting broker account module")?;
    }
    tx.commit().await?;
    Ok(())
}

/// Delete an account; its credentials and grants cascade. Import batches keep their
/// history (`broker_account_id` goes NULL) so a past import stays revertible.
pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM broker_accounts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Credentials (encrypted) ─────────────────────────────────────────────────────

/// Store (or replace) a named secret. Plaintext is sealed immediately and never persisted
/// or returned. A direct value replaces any vault reference the credential had.
pub async fn set_cred(
    pool: &PgPool,
    cipher: &SecretCipher,
    account_id: Uuid,
    broker: &str,
    name: &str,
    plaintext: &str,
) -> anyhow::Result<()> {
    let (nonce, ciphertext) = cipher.seal(plaintext)?;
    sqlx::query(
        "INSERT INTO broker_account_creds (id, account_id, broker, name, nonce, ciphertext) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (account_id, name) \
         DO UPDATE SET nonce = EXCLUDED.nonce, ciphertext = EXCLUDED.ciphertext, \
                       vault_item_id = NULL, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(broker)
    .bind(name)
    .bind(nonce)
    .bind(ciphertext)
    .execute(pool)
    .await
    .context("storing broker credential")?;
    Ok(())
}

/// Plug a centralized vault item as a credential (no local sealed copy is kept).
pub async fn set_cred_ref(
    pool: &PgPool,
    account_id: Uuid,
    broker: &str,
    name: &str,
    vault_item_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO broker_account_creds (id, account_id, broker, name, nonce, ciphertext, vault_item_id) \
         VALUES ($1, $2, $3, $4, NULL, NULL, $5) \
         ON CONFLICT (account_id, name) \
         DO UPDATE SET nonce = NULL, ciphertext = NULL, vault_item_id = $5, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(broker)
    .bind(name)
    .bind(vault_item_id)
    .execute(pool)
    .await
    .context("storing broker credential reference")?;
    Ok(())
}

pub async fn delete_cred(pool: &PgPool, account_id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM broker_account_creds WHERE account_id = $1 AND name = $2")
        .bind(account_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Names of the secrets set for an account (never the values).
pub async fn list_cred_names(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM broker_account_creds WHERE account_id = $1 ORDER BY name",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Credential names + their vault reference (if plugged), for the account form UI.
pub async fn list_cred_meta(
    pool: &PgPool,
    account_id: Uuid,
) -> anyhow::Result<Vec<(String, Option<Uuid>)>> {
    Ok(sqlx::query_as(
        "SELECT name, vault_item_id FROM broker_account_creds WHERE account_id = $1 ORDER BY name",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}

/// Everything a broker needs to talk to its API, as one name→value map: the non-secret
/// `config` first, then the decrypted credentials over it.
pub async fn load_creds(
    pool: &PgPool,
    cipher: &SecretCipher,
    account_id: Uuid,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
    let config: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT config FROM broker_accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(pool)
            .await?;
    if let Some((serde_json::Value::Object(obj),)) = config {
        for (k, v) in obj {
            let text = match v {
                serde_json::Value::String(s) => s,
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            map.insert(k, text);
        }
    }
    let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>, Option<Uuid>)> = sqlx::query_as(
        "SELECT c.name, COALESCE(vi.nonce, c.nonce), COALESCE(vi.ciphertext, c.ciphertext), \
                c.vault_item_id \
         FROM broker_account_creds c LEFT JOIN vault_items vi ON vi.id = c.vault_item_id \
         WHERE c.account_id = $1",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
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

    fn row(modules: &[&str]) -> BrokerRow {
        BrokerRow {
            id: Uuid::nil(),
            broker: "binance".into(),
            name: "Binance".into(),
            modules: modules.iter().map(|s| s.to_string()).collect(),
            config: serde_json::Value::Null,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn grants_match_module_or_wildcard() {
        assert!(row(&["journal"]).allows("journal"));
        assert!(!row(&["journal"]).allows("portfolios"));
        assert!(row(&["*"]).allows("taxcalc"));
        assert!(!row(&[]).allows("journal"));
    }
}
