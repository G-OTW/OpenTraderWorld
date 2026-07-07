//! Storage for saved backtest runs.
//!
//! The simulation is stateless and lives in otw-core; this module only persists a *saved*
//! run so it can be rerun and listed in a history. `settings` and `stats` are opaque JSON
//! to the store — the engine owns their shape. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SavedRun {
    pub id: Uuid,
    pub name: String,
    pub dataset_id: Option<Uuid>,
    pub ticker: String,
    pub timeframe: String,
    pub settings: JsonValue,
    pub stats: JsonValue,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, name, dataset_id, ticker, timeframe, settings, stats, created_at";

/// Newest-first history for the list pane.
pub async fn list_runs(pool: &PgPool) -> anyhow::Result<Vec<SavedRun>> {
    let sql = format!("SELECT {COLS} FROM backtest_runs ORDER BY created_at DESC");
    Ok(sqlx::query_as::<_, SavedRun>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn get_run(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<SavedRun>> {
    let sql = format!("SELECT {COLS} FROM backtest_runs WHERE id = $1");
    Ok(sqlx::query_as::<_, SavedRun>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub struct NewRun<'a> {
    pub name: &'a str,
    pub dataset_id: Uuid,
    pub ticker: &'a str,
    pub timeframe: &'a str,
    pub settings: &'a JsonValue,
    pub stats: &'a JsonValue,
}

pub async fn save_run(pool: &PgPool, r: &NewRun<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backtest_runs (id, name, dataset_id, ticker, timeframe, settings, stats) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(r.name)
    .bind(r.dataset_id)
    .bind(r.ticker)
    .bind(r.timeframe)
    .bind(r.settings)
    .bind(r.stats)
    .execute(pool)
    .await
    .context("saving backtest run")?;
    Ok(id)
}

pub async fn delete_run(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM backtest_runs WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
