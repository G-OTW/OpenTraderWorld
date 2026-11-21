//! Options: saved strategies (migration 0178).
//!
//! What a strategy is worth and how its legs are read lives in `otw-core` (`options/`). This
//! module only stores the rows the builder saves.

use anyhow::Context;
use serde::Serialize;
use serde_json::Value as JsonValue;
use sqlx::{AssertSqlSafe, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Strategy {
    pub id: Uuid,
    pub name: String,
    pub underlying: String,
    pub asset_type: String,
    pub provider: Option<String>,
    pub legs: JsonValue,
    pub notes: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const COLS: &str = "id, name, underlying, asset_type, provider, legs, notes, created_at, updated_at";

pub struct StrategyInput<'a> {
    pub name: &'a str,
    pub underlying: &'a str,
    pub asset_type: &'a str,
    pub provider: Option<&'a str>,
    pub legs: &'a JsonValue,
    pub notes: &'a str,
}

pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Strategy>> {
    let sql = format!("SELECT {COLS} FROM options_strategies ORDER BY updated_at DESC");
    Ok(sqlx::query_as::<_, Strategy>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn create(pool: &PgPool, s: &StrategyInput<'_>) -> anyhow::Result<Strategy> {
    let sql = format!(
        "INSERT INTO options_strategies (name, underlying, asset_type, provider, legs, notes) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {COLS}"
    );
    sqlx::query_as::<_, Strategy>(AssertSqlSafe(sql))
        .bind(s.name)
        .bind(s.underlying)
        .bind(s.asset_type)
        .bind(s.provider)
        .bind(s.legs)
        .bind(s.notes)
        .fetch_one(pool)
        .await
        .context("creating options strategy")
}

pub async fn update(pool: &PgPool, id: Uuid, s: &StrategyInput<'_>) -> anyhow::Result<Option<Strategy>> {
    let sql = format!(
        "UPDATE options_strategies SET name = $2, underlying = $3, asset_type = $4, provider = $5, \
         legs = $6, notes = $7, updated_at = now() WHERE id = $1 RETURNING {COLS}"
    );
    sqlx::query_as::<_, Strategy>(AssertSqlSafe(sql))
        .bind(id)
        .bind(s.name)
        .bind(s.underlying)
        .bind(s.asset_type)
        .bind(s.provider)
        .bind(s.legs)
        .bind(s.notes)
        .fetch_optional(pool)
        .await
        .context("saving options strategy")
}

pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM options_strategies WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}
