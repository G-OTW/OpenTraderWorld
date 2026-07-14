//! Storage for the Historical Data module ("histdata").
//!
//! Four concerns: named provider *connectors* (several per provider, each with its own
//! encrypted credentials and optional `api_quota` scope `histconn:<id>`), the
//! `histdata_datasets` catalog the management page reads, and the `histdata_jobs`
//! download queue drained by the background worker. Bars themselves are bulk-written
//! via [`write_bars`], which also maintains the dataset summary so the catalog never
//! has to scan `histdata_bars`.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::SecretCipher;

// ── Connectors (named provider instances) ──────────────────────────────────────

/// A user-named instance of a provider. Credentials and the optional request quota
/// (scope `histconn:<id>`, see [`crate::api_quota`]) attach here, not to the provider,
/// so several accounts/keys of one provider can coexist.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConnectorRow {
    pub id: Uuid,
    pub provider: String,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

const CONNECTOR_COLS: &str = "id, provider, name, created_at";

pub async fn list_connectors(pool: &PgPool) -> anyhow::Result<Vec<ConnectorRow>> {
    let sql = format!(
        "SELECT {CONNECTOR_COLS} FROM histdata_connectors ORDER BY provider, created_at"
    );
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn get_connector(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<ConnectorRow>> {
    let sql = format!("SELECT {CONNECTOR_COLS} FROM histdata_connectors WHERE id = $1");
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn create_connector(
    pool: &PgPool,
    provider: &str,
    name: &str,
) -> anyhow::Result<ConnectorRow> {
    let sql = format!(
        "INSERT INTO histdata_connectors (id, provider, name) VALUES ($1, $2, $3) \
         RETURNING {CONNECTOR_COLS}"
    );
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(provider)
        .bind(name)
        .fetch_one(pool)
        .await
        .context("creating connector")?)
}

pub async fn rename_connector(pool: &PgPool, id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE histdata_connectors SET name = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await
    .context("renaming connector")?;
    Ok(res.rows_affected() > 0)
}

/// Delete a connector; its credentials cascade, jobs keep history (connector_id NULL).
pub async fn delete_connector(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histdata_connectors WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Fallback connector for a provider (oldest = the seeded default), for jobs queued
/// before connectors existed and provider-addressed API calls (MCP).
pub async fn default_connector_for(
    pool: &PgPool,
    provider: &str,
) -> anyhow::Result<Option<ConnectorRow>> {
    let sql = format!(
        "SELECT {CONNECTOR_COLS} FROM histdata_connectors WHERE provider = $1 \
         ORDER BY created_at LIMIT 1"
    );
    Ok(sqlx::query_as::<_, ConnectorRow>(AssertSqlSafe(sql))
        .bind(provider)
        .fetch_optional(pool)
        .await?)
}

// ── Connector credentials (encrypted) ───────────────────────────────────────────

/// Store (or replace) a named secret for a connector. Plaintext is sealed immediately
/// and never persisted or returned. `provider` is denormalized alongside for legibility.
pub async fn set_cred(
    pool: &PgPool,
    cipher: &SecretCipher,
    connector_id: Uuid,
    provider: &str,
    name: &str,
    plaintext: &str,
) -> anyhow::Result<()> {
    let (nonce, ciphertext) = cipher.seal(plaintext)?;
    sqlx::query(
        "INSERT INTO histdata_provider_creds (id, connector_id, provider, name, nonce, ciphertext) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (connector_id, name) \
         DO UPDATE SET nonce = EXCLUDED.nonce, ciphertext = EXCLUDED.ciphertext, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(connector_id)
    .bind(provider)
    .bind(name)
    .bind(nonce)
    .bind(ciphertext)
    .execute(pool)
    .await
    .context("storing provider credential")?;
    Ok(())
}

pub async fn delete_cred(pool: &PgPool, connector_id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res =
        sqlx::query("DELETE FROM histdata_provider_creds WHERE connector_id = $1 AND name = $2")
            .bind(connector_id)
            .bind(name)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() > 0)
}

/// Names of the secrets set for a connector (never the values).
pub async fn list_cred_names(pool: &PgPool, connector_id: Uuid) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM histdata_provider_creds WHERE connector_id = $1 ORDER BY name",
    )
    .bind(connector_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Decrypt all secrets for a connector into a name→plaintext map (worker use only).
pub async fn load_creds(
    pool: &PgPool,
    cipher: &SecretCipher,
    connector_id: Uuid,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT name, nonce, ciphertext FROM histdata_provider_creds WHERE connector_id = $1",
    )
    .bind(connector_id)
    .fetch_all(pool)
    .await?;
    let mut map = std::collections::HashMap::new();
    for (name, nonce, ct) in rows {
        map.insert(name, cipher.open(&nonce, &ct)?);
    }
    Ok(map)
}

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
}

const JOB_COLS: &str = "id, dataset_id, connector_id, provider, asset_type, ticker, timeframe, \
                        range_from, range_to, kind, status, chunks_done, chunks_total, \
                        bars_written, error, created_at";

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
}

pub async fn enqueue_job(pool: &PgPool, j: &NewJob<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO histdata_jobs \
           (id, dataset_id, connector_id, provider, asset_type, ticker, timeframe, \
            range_from, range_to, kind) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
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

/// Claim the oldest queued job, flipping it to `running`. Returns None when idle.
/// `FOR UPDATE SKIP LOCKED` keeps this safe even if more than one worker ever runs.
pub async fn claim_next_job(pool: &PgPool) -> anyhow::Result<Option<Job>> {
    let sql = format!(
        "UPDATE histdata_jobs SET status = 'running', started_at = now() \
         WHERE id = (SELECT id FROM histdata_jobs WHERE status = 'queued' \
                     ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED) \
         RETURNING {JOB_COLS}"
    );
    Ok(sqlx::query_as::<_, Job>(AssertSqlSafe(sql))
        .fetch_optional(pool)
        .await?)
}

/// Re-queue any job stuck in `running` (e.g. process crash mid-download) so the
/// worker resumes it. Called once at startup.
pub async fn requeue_orphans(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("UPDATE histdata_jobs SET status = 'queued' WHERE status = 'running'")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
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

/// Mirror the worst job outcome onto the dataset status for the catalog view.
pub async fn set_dataset_status(pool: &PgPool, id: Uuid, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE histdata_datasets SET status = $2, last_updated = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}
