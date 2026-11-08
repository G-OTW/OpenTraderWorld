//! User interface preferences in `ui_prefs`: one JSON value per key.
//!
//! Single-user, so a key names one preference app-wide (`otw-theme`,
//! `otw.privacy.hidden`, ...). The values are opaque here: the frontend owns their shape
//! and the API caps their size.

use anyhow::Context;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeMap;

/// Every stored preference, key to value.
pub async fn all(pool: &PgPool) -> anyhow::Result<BTreeMap<String, Value>> {
    let rows: Vec<(String, Value)> = sqlx::query_as("SELECT key, value FROM ui_prefs")
        .fetch_all(pool)
        .await
        .context("listing ui prefs")?;
    Ok(rows.into_iter().collect())
}

/// One preference, or `None` when it was never stored.
pub async fn get(pool: &PgPool, key: &str) -> anyhow::Result<Option<Value>> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT value FROM ui_prefs WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .context("reading ui pref")?;
    Ok(row.map(|r| r.0))
}

/// Upsert one preference.
pub async fn set(pool: &PgPool, key: &str, value: &Value) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO ui_prefs (key, value, updated_at) VALUES ($1, $2, now()) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .context("writing ui pref")?;
    Ok(())
}

/// Upsert many preferences in one transaction: all of them land, or none.
pub async fn set_many(pool: &PgPool, prefs: &BTreeMap<String, Value>) -> anyhow::Result<()> {
    let mut tx = pool.begin().await.context("opening ui prefs transaction")?;
    for (key, value) in prefs {
        sqlx::query(
            "INSERT INTO ui_prefs (key, value, updated_at) VALUES ($1, $2, now()) \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await
        .context("writing ui pref")?;
    }
    tx.commit().await.context("committing ui prefs")?;
    Ok(())
}

/// Remove one preference. True when a row was there.
pub async fn delete(pool: &PgPool, key: &str) -> anyhow::Result<bool> {
    let done = sqlx::query("DELETE FROM ui_prefs WHERE key = $1")
        .bind(key)
        .execute(pool)
        .await
        .context("deleting ui pref")?;
    Ok(done.rows_affected() > 0)
}
