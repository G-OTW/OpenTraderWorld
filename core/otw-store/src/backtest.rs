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
    /// Full set of datasets a portfolio run spanned (None/empty for legacy single-asset rows).
    pub dataset_ids: Option<Vec<Uuid>>,
    pub ticker: String,
    pub timeframe: String,
    pub settings: JsonValue,
    pub stats: JsonValue,
    /// Engine semantics version the run was produced under (1 for pre-versioning rows).
    pub engine_version: i32,
    /// Provenance link to the strategy this run came from (None for ad-hoc runs / deleted).
    pub strategy_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, name, dataset_id, dataset_ids, ticker, timeframe, settings, stats, \
                    engine_version, strategy_id, created_at";

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
    /// Primary dataset (first of `dataset_ids` for a portfolio run).
    pub dataset_id: Uuid,
    /// Full dataset set for a portfolio run (empty/single for a legacy single-asset run).
    pub dataset_ids: &'a [Uuid],
    pub ticker: &'a str,
    pub timeframe: &'a str,
    pub settings: &'a JsonValue,
    pub stats: &'a JsonValue,
    pub engine_version: i32,
    /// Optional provenance link to the strategy the run came from.
    pub strategy_id: Option<Uuid>,
}

pub async fn save_run(pool: &PgPool, r: &NewRun<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backtest_runs \
           (id, name, dataset_id, dataset_ids, ticker, timeframe, settings, stats, engine_version, strategy_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(r.name)
    .bind(r.dataset_id)
    .bind(r.dataset_ids)
    .bind(r.ticker)
    .bind(r.timeframe)
    .bind(r.settings)
    .bind(r.stats)
    .bind(r.engine_version)
    .bind(r.strategy_id)
    .execute(pool)
    .await
    .context("saving backtest run")?;
    prune_runs(pool).await?;
    Ok(id)
}

/// Keep only the 20 most-recent runs; older rows are dropped so the history pane stays bounded.
const RUN_HISTORY_LIMIT: i64 = 20;

async fn prune_runs(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM backtest_runs WHERE id IN (\
           SELECT id FROM backtest_runs ORDER BY created_at DESC OFFSET $1)",
    )
    .bind(RUN_HISTORY_LIMIT)
    .execute(pool)
    .await
    .context("pruning backtest run history")?;
    Ok(())
}

pub async fn delete_run(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM backtest_runs WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Strategies + custom indicators (expert-mode library) ────────────────────────────────
//
// Both are simple named-JSON libraries: list / get / create / update / duplicate / delete.
// `settings` (strategy) and `definition` (indicator) are opaque JSON here — otw-core owns
// their shape. Names are unique; a create/update with a taken name surfaces a clear error.

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Strategy {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub settings: JsonValue,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const STRAT_COLS: &str = "id, name, description, settings, created_at, updated_at";

pub async fn list_strategies(pool: &PgPool) -> anyhow::Result<Vec<Strategy>> {
    let sql = format!("SELECT {STRAT_COLS} FROM backtest_strategies ORDER BY updated_at DESC");
    Ok(sqlx::query_as::<_, Strategy>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_strategy(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Strategy>> {
    let sql = format!("SELECT {STRAT_COLS} FROM backtest_strategies WHERE id = $1");
    Ok(sqlx::query_as::<_, Strategy>(sqlx::AssertSqlSafe(sql)).bind(id).fetch_optional(pool).await?)
}

pub async fn create_strategy(
    pool: &PgPool,
    name: &str,
    description: &str,
    settings: &JsonValue,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backtest_strategies (id, name, description, settings) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(settings)
    .execute(pool)
    .await
    .context("creating strategy")?;
    Ok(id)
}

pub async fn update_strategy(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    description: &str,
    settings: &JsonValue,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE backtest_strategies SET name = $2, description = $3, settings = $4, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(settings)
    .execute(pool)
    .await
    .context("updating strategy")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_strategy(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM backtest_strategies WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Indicator {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub definition: JsonValue,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const IND_COLS: &str = "id, name, description, definition, created_at, updated_at";

pub async fn list_indicators(pool: &PgPool) -> anyhow::Result<Vec<Indicator>> {
    let sql = format!("SELECT {IND_COLS} FROM backtest_indicators ORDER BY updated_at DESC");
    Ok(sqlx::query_as::<_, Indicator>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_indicator(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Indicator>> {
    let sql = format!("SELECT {IND_COLS} FROM backtest_indicators WHERE id = $1");
    Ok(sqlx::query_as::<_, Indicator>(sqlx::AssertSqlSafe(sql)).bind(id).fetch_optional(pool).await?)
}

pub async fn create_indicator(
    pool: &PgPool,
    name: &str,
    description: &str,
    definition: &JsonValue,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backtest_indicators (id, name, description, definition) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(definition)
    .execute(pool)
    .await
    .context("creating indicator")?;
    Ok(id)
}

pub async fn update_indicator(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    description: &str,
    definition: &JsonValue,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE backtest_indicators SET name = $2, description = $3, definition = $4, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(definition)
    .execute(pool)
    .await
    .context("updating indicator")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_indicator(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM backtest_indicators WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}
