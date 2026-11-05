//! Market sessions: named wall-clock windows in a timezone, shared by every module that needs
//! to say "during the US session". Three are created by the migration; the user edits them and
//! adds their own from Settings. What a session *means* at a given instant (which occurrence is
//! running) is computed in `otw-core`, where the timezone database lives.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub timezone: String,
    /// Minutes after local midnight.
    pub start_minute: i32,
    /// Minutes after local midnight; `<= start_minute` means the next day.
    pub end_minute: i32,
    /// Days the window opens on, Monday = bit 0.
    pub weekdays: i32,
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub struct SessionInput<'a> {
    pub name: &'a str,
    pub timezone: &'a str,
    pub start_minute: i32,
    pub end_minute: i32,
    pub weekdays: i32,
}

const COLS: &str =
    "id, name, timezone, start_minute, end_minute, weekdays, position, created_at, updated_at";

pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Session>> {
    let sql = format!("SELECT {COLS} FROM market_sessions ORDER BY position, created_at");
    Ok(sqlx::query_as::<_, Session>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Session>> {
    let sql = format!("SELECT {COLS} FROM market_sessions WHERE id = $1");
    Ok(sqlx::query_as::<_, Session>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn create(pool: &PgPool, s: SessionInput<'_>) -> anyhow::Result<Session> {
    let sql = format!(
        "INSERT INTO market_sessions (name, timezone, start_minute, end_minute, weekdays, position) \
         VALUES ($1,$2,$3,$4,$5, (SELECT COALESCE(MAX(position) + 1, 0) FROM market_sessions)) \
         RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(sqlx::AssertSqlSafe(sql))
        .bind(s.name)
        .bind(s.timezone)
        .bind(s.start_minute)
        .bind(s.end_minute)
        .bind(s.weekdays)
        .fetch_one(pool)
        .await
        .context("creating market session")?)
}

pub async fn update(pool: &PgPool, id: Uuid, s: SessionInput<'_>) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE market_sessions SET name = $2, timezone = $3, start_minute = $4, \
           end_minute = $5, weekdays = $6, updated_at = now() \
         WHERE id = $1 RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(s.name)
        .bind(s.timezone)
        .bind(s.start_minute)
        .bind(s.end_minute)
        .bind(s.weekdays)
        .fetch_optional(pool)
        .await
        .context("saving market session")?)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM market_sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
