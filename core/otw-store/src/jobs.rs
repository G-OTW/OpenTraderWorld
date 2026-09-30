//! Background jobs: the run log every job writes to, the pause switch of the built-in
//! ones, and the conflicts a scheduled broker sync hands back to the user.
//!
//! A job's own settings stay in its module's table (`journal_broker_syncs`,
//! `portfolio_broker_syncs`, `mailbox_accounts`): this is the shared part only, so the
//! Automator's Jobs tab can list them all without a second copy of their state.

use std::collections::HashMap;

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Runs older than this are dropped when a new one starts.
const KEEP_DAYS: i32 = 90;

// ── Runs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct JobRun {
    pub id: Uuid,
    pub job_id: String,
    /// schedule | manual
    pub trigger: String,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    /// running | ok | warning | failed
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
}

pub async fn start_run(pool: &PgPool, job_id: &str, trigger: &str) -> anyhow::Result<Uuid> {
    sqlx::query("DELETE FROM job_runs WHERE started_at < now() - make_interval(days => $1)")
        .bind(KEEP_DAYS)
        .execute(pool)
        .await
        .context("trimming old job runs")?;
    let (id,): (Uuid,) =
        sqlx::query_as("INSERT INTO job_runs (job_id, trigger) VALUES ($1, $2) RETURNING id")
            .bind(job_id)
            .bind(trigger)
            .fetch_one(pool)
            .await
            .context("starting a job run")?;
    Ok(id)
}

pub async fn finish_run(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    summary: Option<&str>,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE job_runs SET finished_at = now(), status = $2, summary = $3, error = $4 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(summary)
    .bind(error)
    .execute(pool)
    .await
    .context("finishing a job run")?;
    Ok(())
}

/// Newest first. `job_id` narrows to one job.
pub async fn list_runs(
    pool: &PgPool,
    job_id: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<JobRun>> {
    sqlx::query_as::<_, JobRun>(
        "SELECT id, job_id, trigger, started_at, finished_at, status, summary, error \
         FROM job_runs WHERE ($1::text IS NULL OR job_id = $1) \
         ORDER BY started_at DESC LIMIT $2",
    )
    .bind(job_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("listing job runs")
}

/// The latest run of every job that ever ran, keyed by job id.
pub async fn last_runs(pool: &PgPool) -> anyhow::Result<HashMap<String, JobRun>> {
    let rows = sqlx::query_as::<_, JobRun>(
        "SELECT DISTINCT ON (job_id) id, job_id, trigger, started_at, finished_at, status, \
                summary, error \
         FROM job_runs ORDER BY job_id, started_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("reading the last job runs")?;
    Ok(rows.into_iter().map(|r| (r.job_id.clone(), r)).collect())
}

/// A run still marked `running` belongs to a process that died mid-job. Called once at
/// boot so the Jobs tab does not show it spinning forever.
pub async fn close_orphan_runs(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE job_runs SET finished_at = now(), status = 'failed', \
         error = 'interrupted by a restart' WHERE status = 'running'",
    )
    .execute(pool)
    .await
    .context("closing interrupted job runs")?;
    Ok(())
}

// ── Built-in jobs ────────────────────────────────────────────────────────────

/// Whether a built-in job may run. A job never paused has no row and runs.
pub async fn system_enabled(pool: &PgPool, id: &str) -> bool {
    sqlx::query_as::<_, (bool,)>("SELECT enabled FROM system_jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|(e,)| e)
        .unwrap_or(true)
}

pub async fn set_system_enabled(pool: &PgPool, id: &str, enabled: bool) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO system_jobs (id, enabled, updated_at) VALUES ($1, $2, now()) \
         ON CONFLICT (id) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now()",
    )
    .bind(id)
    .bind(enabled)
    .execute(pool)
    .await
    .context("pausing a built-in job")?;
    Ok(())
}

// ── Broker sync conflicts ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Conflict {
    pub id: Uuid,
    pub module: String,
    pub target_id: Uuid,
    pub account_id: Uuid,
    pub symbol: String,
    pub kind: String,
    pub details: JsonValue,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const CONFLICT_COLS: &str =
    "id, module, target_id, account_id, symbol, kind, details, created_at, updated_at";

/// Record (or refresh) what a sync could not decide for one instrument. Returns whether
/// the conflict is new, so a scheduled run notifies once rather than at every pass.
pub async fn upsert_conflict(
    pool: &PgPool,
    module: &str,
    target_id: Uuid,
    account_id: Uuid,
    symbol: &str,
    kind: &str,
    details: &JsonValue,
) -> anyhow::Result<bool> {
    let (inserted,): (bool,) = sqlx::query_as(
        "INSERT INTO broker_sync_conflicts (module, target_id, account_id, symbol, kind, details) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (module, target_id, account_id, symbol) DO UPDATE SET \
           kind = EXCLUDED.kind, details = EXCLUDED.details, updated_at = now() \
         RETURNING (xmax = 0)",
    )
    .bind(module)
    .bind(target_id)
    .bind(account_id)
    .bind(symbol)
    .bind(kind)
    .bind(details)
    .fetch_one(pool)
    .await
    .context("recording a broker sync conflict")?;
    Ok(inserted)
}

/// Conflicts of one module, optionally narrowed to one book.
pub async fn list_conflicts(
    pool: &PgPool,
    module: &str,
    target_id: Option<Uuid>,
) -> anyhow::Result<Vec<Conflict>> {
    sqlx::query_as::<_, Conflict>(sqlx::AssertSqlSafe(format!(
        "SELECT {CONFLICT_COLS} FROM broker_sync_conflicts \
         WHERE module = $1 AND ($2::uuid IS NULL OR target_id = $2) \
         ORDER BY created_at, symbol"
    )))
    .bind(module)
    .bind(target_id)
    .fetch_all(pool)
    .await
    .context("listing broker sync conflicts")
}

pub async fn get_conflict(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Conflict>> {
    sqlx::query_as::<_, Conflict>(sqlx::AssertSqlSafe(format!(
        "SELECT {CONFLICT_COLS} FROM broker_sync_conflicts WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading a broker sync conflict")
}

pub async fn delete_conflict(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM broker_sync_conflicts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting a broker sync conflict")?;
    Ok(())
}

/// Drop the conflicts of `symbols` for one book: the sync looked at them again and found
/// nothing left to ask.
pub async fn clear_conflicts(
    pool: &PgPool,
    module: &str,
    target_id: Uuid,
    account_id: Uuid,
    symbols: &[String],
) -> anyhow::Result<()> {
    if symbols.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "DELETE FROM broker_sync_conflicts \
         WHERE module = $1 AND target_id = $2 AND account_id = $3 AND symbol = ANY($4)",
    )
    .bind(module)
    .bind(target_id)
    .bind(account_id)
    .bind(symbols)
    .execute(pool)
    .await
    .context("clearing broker sync conflicts")?;
    Ok(())
}

pub async fn count_conflicts(pool: &PgPool, module: &str) -> anyhow::Result<i64> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM broker_sync_conflicts WHERE module = $1")
            .bind(module)
            .fetch_one(pool)
            .await
            .context("counting broker sync conflicts")?;
    Ok(n)
}
