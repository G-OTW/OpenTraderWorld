//! Download worker for the Historical Data module.
//!
//! A single background task drains the `histdata_jobs` queue serially (kind to provider
//! rate limits). Each job is paged into chunks of the connector's `max_bars_per_req`; after
//! every chunk we persist bars + progress so the download page reflects it and a crash can
//! resume from `chunk_cursor`. Transient errors (429/5xx) get a bounded backoff-retry; a job
//! that wrote some bars before failing terminally ends `partial`, otherwise `error`.

use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;
use time::OffsetDateTime;

use otw_store::histdata as store;

use crate::histdata::{self, Connector};

/// How long to idle when the queue is empty before polling again.
const IDLE_TICK: Duration = Duration::from_secs(5);
/// Per-chunk retry budget for transient failures.
const MAX_RETRIES: u32 = 3;

pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        // Recover jobs that were running when the process died.
        match store::requeue_orphans(&pool).await {
            Ok(n) if n > 0 => tracing::info!("histdata: re-queued {n} interrupted job(s)"),
            Err(e) => tracing::error!("histdata: orphan requeue failed: {e:#}"),
            _ => {}
        }
        loop {
            match store::claim_next_job(&pool).await {
                Ok(Some(job)) => {
                    if let Err(e) = run_job(&pool, &job).await {
                        tracing::error!("histdata job {} crashed: {e:#}", job.id);
                        let _ = store::finish_job(&pool, job.id, "error", Some(&format!("{e:#}"))).await;
                        if let Some(ds) = job.dataset_id {
                            // Drop the dataset if the crash left it empty; else mark it errored.
                            match store::delete_dataset_if_empty(&pool, ds).await {
                                Ok(true) => {}
                                _ => {
                                    let _ = store::set_dataset_status(&pool, ds, "error").await;
                                }
                            }
                        }
                    }
                }
                Ok(None) => tokio::time::sleep(IDLE_TICK).await,
                Err(e) => {
                    tracing::error!("histdata: claim failed: {e:#}");
                    tokio::time::sleep(IDLE_TICK).await;
                }
            }
        }
    });
}

async fn run_job(pool: &PgPool, job: &store::Job) -> Result<()> {
    let connector = histdata::connector_for(&job.provider)?;
    let max_bars = connector.capability().max_bars_per_req;
    let client = histdata::client()?;
    run_with(pool, job, connector.as_ref(), &client, max_bars).await
}

async fn run_with(
    pool: &PgPool,
    job: &store::Job,
    connector: &dyn Connector,
    client: &reqwest::Client,
    max_bars: u32,
) -> Result<()> {
    // A job whose dataset was deleted (FK set null) can't be run; fail it cleanly instead
    // of panicking — an unwinding panic here would kill the whole worker task and stall the
    // queue for every later job.
    let Some(dataset) = job.dataset_id else {
        store::finish_job(pool, job.id, "error", Some("dataset no longer exists")).await?;
        return Ok(());
    };

    // Resolve the connector: the job's own, else the provider's default (pre-connector
    // jobs). Its credentials are used and its api_quota scope billed per request.
    let connector_id = match job.connector_id {
        Some(id) => Some(id),
        None => store::default_connector_for(pool, &job.provider).await?.map(|c| c.id),
    };
    // The worker is spawned with the pool only; the AEAD cipher is published once at startup.
    let secrets = match connector_id {
        Some(id) => store::load_creds(pool, crate::histdata_cipher::get(), id).await?,
        None => Default::default(),
    };
    let quota_scope = connector_id.map(|id| format!("histconn:{id}"));

    let step_secs = histdata::timeframe_secs(&job.timeframe)? * max_bars.max(1) as i64;
    let mut cursor = job.range_from;
    let end = job.range_to;
    // Estimate total chunks for the progress bar.
    let total = (((end - cursor).whole_seconds().max(0) / step_secs.max(1)) + 1) as i32;
    let mut done = 0i32;
    let mut written = 0i64;
    let mut last_err: Option<String> = None;

    while cursor < end {
        let chunk_to = (cursor + time::Duration::seconds(step_secs)).min(end);
        let bars = match fetch_retry(
            pool,
            quota_scope.as_deref(),
            connector,
            client,
            &secrets,
            job,
            cursor,
            chunk_to,
        )
        .await
        {
            Ok(b) => b,
            Err(e) => {
                // Terminal for this chunk after retries; record and stop.
                last_err = Some(format!("{e:#}"));
                break;
            }
        };
        if !bars.is_empty() {
            written += store::write_bars(pool, dataset, &bars).await? as i64;
        }
        done += 1;
        store::update_job_progress(pool, job.id, done, total.max(done), written, Some(chunk_to)).await?;
        cursor = chunk_to;
    }

    match last_err {
        // Completed but the provider returned nothing (e.g. no data in range): don't
        // leave an empty dataset in the Datasets tab — drop it if it holds no bars.
        None if written == 0 => {
            store::finish_job(pool, job.id, "done", None).await?;
            store::delete_dataset_if_empty(pool, dataset).await?;
        }
        None => {
            store::finish_job(pool, job.id, "done", None).await?;
            store::set_dataset_status(pool, dataset, "complete").await?;
        }
        Some(e) if written > 0 => {
            store::finish_job(pool, job.id, "partial", Some(&e)).await?;
            store::set_dataset_status(pool, dataset, "partial").await?;
        }
        // Failed before writing any bars: record the job error, drop the empty dataset.
        Some(e) => {
            store::finish_job(pool, job.id, "error", Some(&e)).await?;
            if !store::delete_dataset_if_empty(pool, dataset).await? {
                store::set_dataset_status(pool, dataset, "error").await?;
            }
        }
    }
    Ok(())
}

/// Fetch one chunk with bounded retry + linear backoff on transient errors.
/// Every attempt (retries included) is a real provider request, so each one is
/// counted against the connector's quota scope before it fires.
#[allow(clippy::too_many_arguments)]
async fn fetch_retry(
    pool: &PgPool,
    quota_scope: Option<&str>,
    connector: &dyn Connector,
    client: &reqwest::Client,
    secrets: &std::collections::HashMap<String, String>,
    job: &store::Job,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Vec<otw_store::histdata::Bar>> {
    let mut attempt = 0u32;
    loop {
        if let Some(scope) = quota_scope {
            // Display-only counter; a failed write must not block the download.
            if let Err(e) = otw_store::api_quota::bump(pool, scope).await {
                tracing::debug!("histdata: quota bump failed: {e:#}");
            }
        }
        match connector
            .fetch_chunk(client, secrets, &job.ticker, &job.asset_type, &job.timeframe, from, to)
            .await
        {
            Ok(chunk) => return Ok(chunk.bars),
            Err(e) => {
                attempt += 1;
                // Only transient errors (429/5xx, timeouts, connection drops) are worth
                // retrying. Permanent failures — 4xx, bad symbol, premium-gated endpoint,
                // bad/missing key — would just fail again and needlessly hold the single
                // worker through the backoff, so fail fast.
                if attempt > MAX_RETRIES || !is_transient(&e) {
                    return Err(e);
                }
                tracing::warn!("histdata {} chunk retry {attempt}: {e:#}", job.provider);
                tokio::time::sleep(Duration::from_secs(2 * attempt as u64)).await;
            }
        }
    }
}

/// Whether a fetch error is worth retrying. Transient = timeouts, connection failures, and
/// HTTP 429/5xx. Everything else (4xx, and connectors' own parsed errors such as a bad
/// symbol or a premium-gated endpoint) is permanent — retrying only wastes the worker.
fn is_transient(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.is_timeout() || re.is_connect() {
                return true;
            }
            if let Some(status) = re.status() {
                return status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status.is_server_error();
            }
        }
    }
    // No HTTP error in the chain → a connector-level parse/validation error. Permanent.
    false
}
