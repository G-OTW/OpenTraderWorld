//! Global app settings — a flat key/value store in `app_settings`.
//!
//! Single-user: one row per key. Values are TEXT; callers interpret them. Known keys:
//! `default_currency`, `default_timezone`, `log_level`. Modules read these later for
//! their own defaults.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

/// All settings as a key→value map.
pub async fn all(pool: &PgPool) -> anyhow::Result<BTreeMap<String, String>> {
    let rows = sqlx::query_as::<_, Setting>("SELECT key, value FROM app_settings")
        .fetch_all(pool)
        .await
        .context("listing settings")?;
    Ok(rows.into_iter().map(|s| (s.key, s.value)).collect())
}

/// Read one setting, or `None` if unset.
pub async fn get(pool: &PgPool, key: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM app_settings WHERE key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await
            .context("reading setting")?;
    Ok(row.map(|r| r.0))
}

/// Read one setting or fall back to a default.
pub async fn get_or(pool: &PgPool, key: &str, default: &str) -> anyhow::Result<String> {
    Ok(get(pool, key).await?.unwrap_or_else(|| default.to_string()))
}

/// Upsert one setting.
pub async fn set(pool: &PgPool, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_at) VALUES ($1, $2, now()) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .context("writing setting")?;
    Ok(())
}
