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
    /// false = auto history entry (capped, sweepable); true = the user named it via Save.
    pub pinned: bool,
    /// Date window the run was simulated over; NULL/NULL = the whole dataset. Persisted
    /// because replays (chart rerun, Monte Carlo) rebuild the run from settings + datasets
    /// alone — without it a windowed run would replay over a different span.
    #[serde(with = "time::serde::rfc3339::option")]
    pub bars_from: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub bars_to: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const COLS: &str = "id, name, dataset_id, dataset_ids, ticker, timeframe, settings, stats, \
                    engine_version, strategy_id, pinned, bars_from, bars_to, created_at";

/// Newest-first history for the list pane. `pinned_only` narrows it to the saved runs.
pub async fn list_runs(pool: &PgPool, pinned_only: bool) -> anyhow::Result<Vec<SavedRun>> {
    let filter = if pinned_only { "WHERE pinned " } else { "" };
    let sql = format!("SELECT {COLS} FROM backtest_runs {filter}ORDER BY created_at DESC");
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
    /// true only when the user explicitly named the run; auto history entries are false.
    pub pinned: bool,
    /// Date window simulated (None/None = whole dataset). Replays read it back.
    pub bars_from: Option<OffsetDateTime>,
    pub bars_to: Option<OffsetDateTime>,
}

pub async fn save_run(pool: &PgPool, r: &NewRun<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backtest_runs \
           (id, name, dataset_id, dataset_ids, ticker, timeframe, settings, stats, engine_version, strategy_id, pinned, bars_from, bars_to) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
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
    .bind(r.pinned)
    .bind(r.bars_from)
    .bind(r.bars_to)
    .execute(pool)
    .await
    .context("saving backtest run")?;
    prune_runs(pool).await?;
    Ok(id)
}

/// Promote an auto history entry to a saved run under a user-chosen name. Returns false when
/// the id is unknown.
pub async fn pin_run(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE backtest_runs SET pinned = true, name = $2 WHERE id = $1")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .context("pinning backtest run")?;
    Ok(res.rows_affected() > 0)
}

/// Cap on *auto* history entries; pinned (saved) runs are never counted nor pruned.
const RUN_HISTORY_LIMIT: i64 = 300;

async fn prune_runs(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM backtest_runs WHERE id IN (\
           SELECT id FROM backtest_runs WHERE NOT pinned ORDER BY created_at DESC OFFSET $1)",
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

/// Clear the auto history in one go. Saved (pinned) runs survive; returns the number dropped.
pub async fn delete_unpinned_runs(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM backtest_runs WHERE NOT pinned")
        .execute(pool)
        .await
        .context("clearing backtest run history")?;
    Ok(res.rows_affected())
}

// ── Strategies + custom indicators (library) ────────────────────────────────────────────
//
// Both are simple named-JSON libraries: list / get / create / update / duplicate / delete.
// `settings` (strategy) and `definition` (indicator) are opaque JSON here — otw-core owns
// their shape. Names are unique; a create/update with a taken name surfaces a clear error.

/// Normalise tags: trim, drop empties, dedup (case-insensitive), preserve order.
fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim();
        if t.is_empty() || out.iter().any(|s| s.eq_ignore_ascii_case(t)) {
            continue;
        }
        out.push(t.to_string());
    }
    out
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Strategy {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    /// Free-form labels the library's search box matches on, alongside the name.
    pub tags: Vec<String>,
    pub settings: JsonValue,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const STRAT_COLS: &str = "id, name, description, tags, settings, created_at, updated_at";

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
    tags: &[String],
    settings: &JsonValue,
) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backtest_strategies (id, name, description, tags, settings) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(clean_tags(tags))
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
    tags: &[String],
    settings: &JsonValue,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE backtest_strategies SET name = $2, description = $3, tags = $4, settings = $5, \
                updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(clean_tags(tags))
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
