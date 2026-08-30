//! Storage for the Historical Data module ("histdata").
//!
//! Two concerns: the `histdata_datasets` catalog the management page reads, and the
//! `histdata_jobs` download queue drained by the background worker. Bars themselves are
//! bulk-written via [`write_bars`], which also maintains the dataset summary so the
//! catalog never has to scan `histdata_bars`. Provider connectors and their credentials
//! moved to [`crate::connectors`] when they stopped belonging to one module.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

// ── Datasets (catalog) ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Dataset {
    pub id: Uuid,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub range_from: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub range_to: Option<OffsetDateTime>,
    pub bar_count: i64,
    pub size_bytes: i64,
    pub gaps: JsonValue,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub last_updated: OffsetDateTime,
}

const DATASET_COLS: &str = "id, provider, asset_type, ticker, timeframe, range_from, range_to, \
                            bar_count, size_bytes, gaps, status, last_updated";

/// List every dataset, ordered for the management page: asset type → ticker → timeframe.
pub async fn list_datasets(pool: &PgPool) -> anyhow::Result<Vec<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         ORDER BY asset_type, ticker, timeframe"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn get_dataset(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Dataset>> {
    let sql = format!("SELECT {DATASET_COLS} FROM histdata_datasets WHERE id = $1");
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// The stored dataset for these coordinates, if one exists. Read-only counterpart of
/// [`upsert_dataset`] — the chart preview uses it to tell "already in store" from "live
/// look only" without creating anything.
pub async fn find_dataset(
    pool: &PgPool,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
) -> anyhow::Result<Option<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         WHERE provider = $1 AND asset_type = $2 AND ticker = $3 AND timeframe = $4"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(provider)
        .bind(asset_type)
        .bind(ticker)
        .bind(timeframe)
        .fetch_optional(pool)
        .await?)
}

/// Find or create the dataset for these coordinates, returning its id.
pub async fn upsert_dataset(
    pool: &PgPool,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
) -> anyhow::Result<Uuid> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO histdata_datasets (id, provider, asset_type, ticker, timeframe) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (provider, asset_type, ticker, timeframe) \
         DO UPDATE SET last_updated = now() \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(provider)
    .bind(asset_type)
    .bind(ticker)
    .bind(timeframe)
    .fetch_one(pool)
    .await
    .context("upserting dataset")?;
    Ok(row.0)
}

/// Delete a dataset only if it holds no bars (e.g. a download that returned nothing).
/// Won't touch a dataset that already has data from a prior download. Returns true if removed.
pub async fn delete_dataset_if_empty(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histdata_datasets WHERE id = $1 AND bar_count = 0")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_dataset(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    // Bars cascade via FK. Jobs keep their history (dataset_id set NULL).
    let res = sqlx::query("DELETE FROM histdata_datasets WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Chart layouts ──────────────────────────────────────────────────────────────
//
// What the visualization module was showing for a dataset — indicators, drawings, plot
// style — so reopening it lands you where you left off, from any browser. Only a *stored*
// dataset can carry one: the row is keyed by its id and cascades with it.

/// The saved layout for a dataset, or None when it was never customized.
pub async fn get_layout(pool: &PgPool, dataset_id: Uuid) -> anyhow::Result<Option<JsonValue>> {
    let row: Option<(JsonValue,)> =
        sqlx::query_as("SELECT layout FROM histviz_layouts WHERE dataset_id = $1")
            .bind(dataset_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

/// Replace a dataset's layout.
pub async fn set_layout(pool: &PgPool, dataset_id: Uuid, layout: &JsonValue) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO histviz_layouts (dataset_id, layout) VALUES ($1, $2) \
         ON CONFLICT (dataset_id) DO UPDATE SET layout = EXCLUDED.layout, updated_at = now()",
    )
    .bind(dataset_id)
    .bind(layout)
    .execute(pool)
    .await
    .context("saving chart layout")?;
    Ok(())
}

// ── Bars ───────────────────────────────────────────────────────────────────────

/// One OHLCV bar ready to persist. Adjusted fields are None for crypto/fx.
#[derive(Debug, Clone)]
pub struct Bar {
    pub ts: OffsetDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub adj_open: Option<f64>,
    pub adj_high: Option<f64>,
    pub adj_low: Option<f64>,
    pub adj_close: Option<f64>,
}

/// Upsert a batch of bars (idempotent on PK) and refresh the dataset summary in one
/// transaction. Returns the number of newly inserted rows. Re-downloads update in place.
pub async fn write_bars(
    pool: &PgPool,
    dataset_id: Uuid,
    bars: &[Bar],
) -> anyhow::Result<u64> {
    if bars.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let mut inserted = 0u64;
    for b in bars {
        let res = sqlx::query(
            "INSERT INTO histdata_bars \
               (dataset_id, ts, open, high, low, close, volume, adj_open, adj_high, adj_low, adj_close) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (dataset_id, ts) DO UPDATE SET \
               open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, \
               close = EXCLUDED.close, volume = EXCLUDED.volume, \
               adj_open = EXCLUDED.adj_open, adj_high = EXCLUDED.adj_high, \
               adj_low = EXCLUDED.adj_low, adj_close = EXCLUDED.adj_close",
        )
        .bind(dataset_id)
        .bind(b.ts)
        .bind(b.open)
        .bind(b.high)
        .bind(b.low)
        .bind(b.close)
        .bind(b.volume)
        .bind(b.adj_open)
        .bind(b.adj_high)
        .bind(b.adj_low)
        .bind(b.adj_close)
        .execute(&mut *tx)
        .await
        .context("inserting bar")?;
        inserted += res.rows_affected();
    }
    // Recompute the summary from the (now updated) bars. ~96 bytes/row is a rough
    // on-disk estimate good enough for the management page's size column.
    sqlx::query(
        "UPDATE histdata_datasets d SET \
           range_from   = s.lo, \
           range_to     = s.hi, \
           bar_count    = s.n, \
           size_bytes   = s.n * 96, \
           last_updated = now() \
         FROM (SELECT min(ts) AS lo, max(ts) AS hi, count(*) AS n \
               FROM histdata_bars WHERE dataset_id = $1) s \
         WHERE d.id = $1",
    )
    .bind(dataset_id)
    .execute(&mut *tx)
    .await
    .context("refreshing dataset summary")?;
    tx.commit().await?;
    Ok(inserted)
}

/// Stream all bars of a dataset in time order, for CSV export.
pub async fn export_rows(
    pool: &PgPool,
    dataset_id: Uuid,
) -> anyhow::Result<Vec<Bar>> {
    let rows: Vec<(
        OffsetDateTime,
        f64,
        f64,
        f64,
        f64,
        f64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    )> = sqlx::query_as(
        "SELECT ts, open::float8, high::float8, low::float8, close::float8, volume::float8, \
         adj_open::float8, adj_high::float8, adj_low::float8, adj_close::float8 \
         FROM histdata_bars WHERE dataset_id = $1 ORDER BY ts",
    )
    .bind(dataset_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            ts: r.0,
            open: r.1,
            high: r.2,
            low: r.3,
            close: r.4,
            volume: r.5,
            adj_open: r.6,
            adj_high: r.7,
            adj_low: r.8,
            adj_close: r.9,
        })
        .collect())
}

/// Read bars of a dataset for charting, optionally bounded by [from, to] and capped at
/// `limit` rows. When more than `limit` bars match, returns the most recent `limit`
/// (descending then reversed to ascending) so the chart shows the latest window. Adjusted
/// columns are omitted — the visualization uses raw OHLCV.
pub async fn read_bars(
    pool: &PgPool,
    dataset_id: Uuid,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
    limit: i64,
) -> anyhow::Result<Vec<Bar>> {
    let mut rows: Vec<(OffsetDateTime, f64, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT ts, open::float8, high::float8, low::float8, close::float8, volume::float8 \
         FROM histdata_bars \
         WHERE dataset_id = $1 \
           AND ($2::timestamptz IS NULL OR ts >= $2) \
           AND ($3::timestamptz IS NULL OR ts <= $3) \
         ORDER BY ts DESC LIMIT $4",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.reverse(); // back to ascending time order for the chart
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            ts: r.0,
            open: r.1,
            high: r.2,
            low: r.3,
            close: r.4,
            volume: r.5,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        })
        .collect())
}

// ── Jobs (download queue) ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub dataset_id: Option<Uuid>,
    /// Connector whose credentials/quota the worker uses; NULL on pre-connector jobs
    /// (falls back to the provider's default connector).
    pub connector_id: Option<Uuid>,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    #[serde(with = "time::serde::rfc3339")]
    pub range_from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub range_to: OffsetDateTime,
    pub kind: String,
    pub status: String,
    pub chunks_done: i32,
    pub chunks_total: i32,
    pub bars_written: i64,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Next bar ts to fetch. Set after every chunk, so a job that was parked or interrupted
    /// resumes here instead of refetching what it already stored.
    #[serde(with = "time::serde::rfc3339::option")]
    pub chunk_cursor: Option<OffsetDateTime>,
    /// When a parked (`waiting`) job may run again. NULL on every other status.
    #[serde(with = "time::serde::rfc3339::option")]
    pub resume_at: Option<OffsetDateTime>,
    /// Why it is parked: `quota` (the connector's declared cap is spent) or `rate_limit`
    /// (the provider answered 429). Kept after the resume so the page can explain a pause.
    pub wait_reason: Option<String>,
    /// Consecutive parks; drives the backoff when the provider sends no Retry-After.
    pub wait_count: i32,
    /// The submission this job was queued with (batch download). NULL for a lone job.
    pub batch_id: Option<Uuid>,
}

const JOB_COLS: &str = "id, dataset_id, connector_id, provider, asset_type, ticker, timeframe, \
                        range_from, range_to, kind, status, chunks_done, chunks_total, \
                        bars_written, error, created_at, chunk_cursor, resume_at, wait_reason, \
                        wait_count, batch_id";

/// Statuses a job can still leave on its own — anything else is terminal.
const LIVE_STATUSES: &str = "('queued', 'running', 'waiting', 'cancelling')";

/// Parameters for queueing a download.
pub struct NewJob<'a> {
    pub dataset_id: Uuid,
    pub connector_id: Option<Uuid>,
    pub provider: &'a str,
    pub asset_type: &'a str,
    pub ticker: &'a str,
    pub timeframe: &'a str,
    pub range_from: OffsetDateTime,
    pub range_to: OffsetDateTime,
    pub kind: &'a str,
    /// Groups the jobs one submission queued; None for a single download.
    pub batch_id: Option<Uuid>,
}

pub async fn enqueue_job(pool: &PgPool, j: &NewJob<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO histdata_jobs \
           (id, dataset_id, connector_id, provider, asset_type, ticker, timeframe, \
            range_from, range_to, kind, batch_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(j.dataset_id)
    .bind(j.connector_id)
    .bind(j.provider)
    .bind(j.asset_type)
    .bind(j.ticker)
    .bind(j.timeframe)
    .bind(j.range_from)
    .bind(j.range_to)
    .bind(j.kind)
    .bind(j.batch_id)
    .execute(pool)
    .await
    .context("enqueuing job")?;
    Ok(id)
}

/// Recent jobs for the download page (newest first).
pub async fn list_jobs(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Job>> {
    let sql = format!("SELECT {JOB_COLS} FROM histdata_jobs ORDER BY created_at DESC LIMIT $1");
    Ok(sqlx::query_as::<_, Job>(AssertSqlSafe(sql))
        .bind(limit)
        .fetch_all(pool)
        .await?)
}

// ── Worker-side job lifecycle ──────────────────────────────────────────────────

/// Claim the oldest runnable job, flipping it to `running`. Returns None when idle.
/// Runnable = freshly queued, or parked and past its `resume_at`. `FOR UPDATE SKIP LOCKED`
/// keeps this safe even if more than one worker ever runs.
pub async fn claim_next_job(pool: &PgPool) -> anyhow::Result<Option<Job>> {
    let sql = format!(
        "UPDATE histdata_jobs SET status = 'running', started_at = now() \
         WHERE id = (SELECT id FROM histdata_jobs \
                     WHERE status = 'queued' \
                        OR (status = 'waiting' AND resume_at IS NOT NULL AND resume_at <= now()) \
                     ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED) \
         RETURNING {JOB_COLS}"
    );
    Ok(sqlx::query_as::<_, Job>(AssertSqlSafe(sql))
        .fetch_optional(pool)
        .await?)
}

/// Re-queue any job stuck in `running` (e.g. process crash mid-download) so the worker
/// resumes it. A crash mid-cancel honours the user: `cancelling` becomes `cancelled`
/// rather than restarting. Called once at startup.
pub async fn requeue_orphans(pool: &PgPool) -> anyhow::Result<u64> {
    sqlx::query(
        "UPDATE histdata_jobs SET status = 'cancelled', finished_at = now() \
         WHERE status = 'cancelling'",
    )
    .execute(pool)
    .await?;
    let res = sqlx::query("UPDATE histdata_jobs SET status = 'queued' WHERE status = 'running'")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Park a running job until `resume_at`: it keeps its progress and is claimed again once
/// that instant passes. `reason` is `quota` or `rate_limit`.
pub async fn park_job(
    pool: &PgPool,
    job_id: Uuid,
    resume_at: OffsetDateTime,
    reason: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET status = 'waiting', resume_at = $2, wait_reason = $3, \
             wait_count = wait_count + 1 \
         WHERE id = $1",
    )
    .bind(job_id)
    .bind(resume_at)
    .bind(reason)
    .execute(pool)
    .await
    .context("parking job")?;
    Ok(())
}

/// Clear the parking marks after a chunk goes through, so the backoff ladder starts from
/// scratch the next time the provider pushes back.
pub async fn clear_wait(pool: &PgPool, job_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET resume_at = NULL, wait_reason = NULL, wait_count = 0 \
         WHERE id = $1 AND (resume_at IS NOT NULL OR wait_count > 0)",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Current status of one job — the worker polls this between chunks to notice a cancel.
pub async fn job_status(pool: &PgPool, job_id: Uuid) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT status FROM histdata_jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0))
}

/// Ask a job to stop. A job that has not started yet (or is parked) ends immediately as
/// `cancelled`; a running one is marked `cancelling` and the worker finishes it at the next
/// chunk boundary, keeping the bars already written. Returns the new status, or None when
/// the job is already finished (nothing to cancel).
pub async fn cancel_job(pool: &PgPool, job_id: Uuid) -> anyhow::Result<Option<String>> {
    let sql = format!(
        "UPDATE histdata_jobs SET \
             status = CASE WHEN status = 'running' THEN 'cancelling' ELSE 'cancelled' END, \
             finished_at = CASE WHEN status = 'running' THEN finished_at ELSE now() END, \
             resume_at = NULL \
         WHERE id = $1 AND status IN {LIVE_STATUSES} \
         RETURNING status"
    );
    let row: Option<(String,)> = sqlx::query_as(AssertSqlSafe(sql))
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .context("cancelling job")?;
    Ok(row.map(|r| r.0))
}

/// Cancel every job of a batch that has not finished. Returns how many were affected.
pub async fn cancel_batch(pool: &PgPool, batch_id: Uuid) -> anyhow::Result<u64> {
    let sql = format!(
        "UPDATE histdata_jobs SET \
             status = CASE WHEN status = 'running' THEN 'cancelling' ELSE 'cancelled' END, \
             finished_at = CASE WHEN status = 'running' THEN finished_at ELSE now() END, \
             resume_at = NULL \
         WHERE batch_id = $1 AND status IN {LIVE_STATUSES}"
    );
    let res = sqlx::query(AssertSqlSafe(sql))
        .bind(batch_id)
        .execute(pool)
        .await
        .context("cancelling batch")?;
    Ok(res.rows_affected())
}

/// How a batch stands: one (status, count) row per status present.
pub async fn batch_counts(pool: &PgPool, batch_id: Uuid) -> anyhow::Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT status, count(*) FROM histdata_jobs WHERE batch_id = $1 GROUP BY status",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .context("counting batch jobs")?)
}

/// Persist progress mid-download so the page reflects it and a crash can resume.
pub async fn update_job_progress(
    pool: &PgPool,
    job_id: Uuid,
    chunks_done: i32,
    chunks_total: i32,
    bars_written: i64,
    cursor: Option<OffsetDateTime>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET chunks_done = $2, chunks_total = $3, \
         bars_written = $4, chunk_cursor = $5 WHERE id = $1",
    )
    .bind(job_id)
    .bind(chunks_done)
    .bind(chunks_total)
    .bind(bars_written)
    .bind(cursor)
    .execute(pool)
    .await?;
    Ok(())
}

/// Terminal state: `done`, `partial`, or `error` (with message).
pub async fn finish_job(
    pool: &PgPool,
    job_id: Uuid,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET status = $2, error = $3, finished_at = now() WHERE id = $1",
    )
    .bind(job_id)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Newest bar timestamp held for a dataset, or `None` if it has no bars yet. Used by the
/// live worker to seed/backfill only the gap between stored history and now.
pub async fn latest_bar_ts(
    pool: &PgPool,
    dataset_id: Uuid,
) -> anyhow::Result<Option<OffsetDateTime>> {
    // MAX over a possibly-empty set yields one NULL row → Option, fetch_one.
    let row: (Option<OffsetDateTime>,) =
        sqlx::query_as("SELECT max(ts) FROM histdata_bars WHERE dataset_id = $1")
            .bind(dataset_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

/// Mirror the worst job outcome onto the dataset status for the catalog view.
pub async fn set_dataset_status(pool: &PgPool, id: Uuid, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE histdata_datasets SET status = $2, last_updated = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}
