//! Download worker for the Historical Data module.
//!
//! A single background task drains the `histdata_jobs` queue serially (kind to provider
//! rate limits). Each job is paged into chunks of the connector's `max_bars_per_req`; after
//! every chunk we persist bars + progress so the download page reflects it and a crash can
//! resume from `chunk_cursor`. Transient errors (429/5xx) get a bounded backoff-retry; a job
//! that wrote some bars before failing terminally ends `partial`, otherwise `error`.
//!
//! Provider limits are handled in three layers, because a batch download makes hitting them
//! the normal case rather than the exception:
//!   1. **Pacing** — `Capability.min_interval_ms` spaces two requests to the same provider,
//!      so a long batch stays under the published ceiling instead of tripping 429.
//!   2. **Parking** — the connector's declared quota being spent, or a 429 surviving the
//!      retries, suspends the job (`waiting` + `resume_at`) instead of failing it. The
//!      worker claims it again when the time comes and resumes from its cursor.
//!   3. **Telling the user** — a long park and the end of a batch raise a notification.
//!
//! Cancelling is cooperative: the API marks the job `cancelling` and the worker stops at the
//! next chunk boundary, keeping the bars already written.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Result;
use sqlx::PgPool;
use time::OffsetDateTime;

use otw_store::connectors as conn_store;
use otw_store::histdata as store;

use crate::histdata::{self, Connector};

/// How long to idle when the queue is empty before polling again.
const IDLE_TICK: Duration = Duration::from_secs(5);
/// Per-chunk retry budget for transient failures.
const MAX_RETRIES: u32 = 3;
/// First backoff step when a provider rate-limits us without a Retry-After header; the
/// ladder doubles per consecutive park, up to [`MAX_BACKOFF`].
const BASE_BACKOFF: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(3600);
/// A pause shorter than this is not worth a notification — the page shows the countdown.
const NOTIFY_WAIT: Duration = Duration::from_secs(300);

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
                    // Whatever the outcome, the batch may have just emptied.
                    announce_batch_end(&pool, &job).await;
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
    let cap = connector.capability();
    let client = histdata::client()?;
    run_with(pool, job, connector.as_ref(), &client, cap.max_bars_per_req).await
}

/// Why a chunk did not come back with bars.
enum ChunkFail {
    /// The provider rate-limited us. `retry_after` is what it asked for, when it said so.
    Limited { retry_after: Option<Duration> },
    /// Anything else: permanent for this job (bad symbol, no key, parse error…).
    Fatal(anyhow::Error),
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
        None => conn_store::default_for(pool, &job.provider).await?.map(|c| c.id),
    };
    // The worker is spawned with the pool only; the AEAD cipher is published once at startup.
    let secrets = match connector_id {
        Some(id) => conn_store::load_creds(pool, crate::histdata_cipher::get(), id).await?,
        None => Default::default(),
    };
    let quota_scope = connector_id.map(|id| format!("histconn:{id}"));
    let min_interval = Duration::from_millis(connector.capability().min_interval_ms);

    let step_secs = histdata::timeframe_secs(&job.timeframe)? * max_bars.max(1) as i64;
    // A parked job resumes where it stopped rather than refetching what it already stored.
    let mut cursor = job.chunk_cursor.unwrap_or(job.range_from).max(job.range_from);
    let end = job.range_to;
    // Estimate total chunks for the progress bar.
    let total = (((end - job.range_from).whole_seconds().max(0) / step_secs.max(1)) + 1) as i32;
    let mut done = job.chunks_done;
    let mut written = job.bars_written;
    let mut last_err: Option<String> = None;

    while cursor < end {
        // The user asked to stop: keep what is already stored and end here.
        if store::job_status(pool, job.id).await?.as_deref() == Some("cancelling") {
            store::finish_job(pool, job.id, "cancelled", None).await?;
            if written > 0 {
                store::set_dataset_status(pool, dataset, "partial").await?;
            } else {
                store::delete_dataset_if_empty(pool, dataset).await?;
            }
            return Ok(());
        }
        // The connector's declared quota is spent: park until its window rolls over. The
        // quota is observe-only everywhere else, but a batch that ignores it just burns
        // through the provider's own limit a minute later.
        if let Some(scope) = quota_scope.as_deref() {
            if let Some(resume) = quota_exhausted_until(pool, scope).await {
                park(pool, job, resume, "quota").await?;
                return Ok(());
            }
        }

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
            min_interval,
        )
        .await
        {
            Ok(b) => b,
            // Rate-limited even after the retries: this is a wait, not a failure.
            Err(ChunkFail::Limited { retry_after }) => {
                let wait = retry_after.unwrap_or_else(|| backoff_for(job.wait_count));
                let resume = OffsetDateTime::now_utc() + time::Duration::seconds(wait.as_secs() as i64);
                park(pool, job, resume, "rate_limit").await?;
                return Ok(());
            }
            Err(ChunkFail::Fatal(e)) => {
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
        // A chunk went through: the provider is answering again, so the backoff ladder and
        // the "why it paused" mark start from scratch.
        store::clear_wait(pool, job.id).await?;
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

// ── Waiting ────────────────────────────────────────────────────────────────────

/// Suspend the job until `resume_at` and, when the pause is long enough to be worth an
/// interruption, tell the user once (the first park; later ones only lengthen the wait).
async fn park(
    pool: &PgPool,
    job: &store::Job,
    resume_at: OffsetDateTime,
    reason: &str,
) -> Result<()> {
    store::park_job(pool, job.id, resume_at, reason).await?;
    let wait = (resume_at - OffsetDateTime::now_utc()).whole_seconds().max(0) as u64;
    tracing::info!(
        "histdata: {} {} parked ({reason}) for {wait}s",
        job.ticker,
        job.timeframe
    );
    if job.wait_count == 0 && wait >= NOTIFY_WAIT.as_secs() {
        let why = if reason == "quota" {
            "the connector's quota is spent"
        } else {
            "the provider is rate-limiting"
        };
        notify(
            pool,
            "Historical data: download paused",
            &format!(
                "{} {} ({}) paused because {why}. It resumes on its own in about {}.",
                job.ticker,
                job.timeframe,
                job.provider,
                human_duration(wait)
            ),
        )
        .await;
    }
    Ok(())
}

/// When the connector's declared quota is spent, the instant its window rolls over.
/// `None` = no quota declared, unlimited, or still under the cap.
async fn quota_exhausted_until(pool: &PgPool, scope: &str) -> Option<OffsetDateTime> {
    let q = otw_store::api_quota::get(pool, scope).await.ok().flatten()?;
    let max = q.max_requests?;
    (q.used >= max).then_some(q.resets_at)
}

/// Backoff ladder for a provider that rate-limits without saying for how long.
fn backoff_for(wait_count: i32) -> Duration {
    let steps = wait_count.clamp(0, 8) as u32;
    (BASE_BACKOFF * 2u32.saturating_pow(steps)).min(MAX_BACKOFF)
}

fn human_duration(secs: u64) -> String {
    match secs {
        s if s < 90 => format!("{s} s"),
        s if s < 5400 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    }
}

// ── Notifications ──────────────────────────────────────────────────────────────

/// In-app notification + the external channels the user granted to this module.
async fn notify(pool: &PgPool, name: &str, details: &str) {
    match otw_store::reminders::add_notification(pool, name, details).await {
        Ok(n) => {
            if let Ok(http) = histdata::client() {
                crate::notif_send::dispatch(
                    pool,
                    crate::histdata_cipher::get(),
                    &http,
                    &n,
                    "histdata",
                    None,
                )
                .await;
            }
        }
        Err(e) => tracing::warn!("histdata: notification failed: {e:#}"),
    }
}

/// Report a batch once its last job leaves the queue. A job that only parked is still live,
/// so a paused batch stays silent here — `park` already said why.
async fn announce_batch_end(pool: &PgPool, job: &store::Job) {
    let Some(batch) = job.batch_id else { return };
    let counts = match store::batch_counts(pool, batch).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("histdata: batch summary failed: {e:#}");
            return;
        }
    };
    let live = ["queued", "running", "waiting", "cancelling"];
    let total: i64 = counts.iter().map(|(_, n)| n).sum();
    if total <= 1 || counts.iter().any(|(s, n)| live.contains(&s.as_str()) && *n > 0) {
        return;
    }
    let of = |name: &str| counts.iter().find(|(s, _)| s == name).map(|(_, n)| *n).unwrap_or(0);
    let mut parts = vec![format!("{} downloaded", of("done"))];
    for (label, n) in [
        ("partial", of("partial")),
        ("failed", of("error")),
        ("cancelled", of("cancelled")),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    notify(
        pool,
        "Historical data: batch finished",
        &format!("{total} downloads on {}: {}.", job.provider, parts.join(", ")),
    )
    .await;
}

// ── Fetching ───────────────────────────────────────────────────────────────────

/// Last request per provider, so [`pace`] can honour `Capability.min_interval_ms` across
/// jobs: a batch of 20 instruments is one long conversation with the same API.
static LAST_REQUEST: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Wait out the provider's minimum spacing, then mark this instant as its last request.
/// The lock is never held across the await.
async fn pace(provider: &str, min_interval: Duration) {
    if min_interval.is_zero() {
        return;
    }
    let wait = {
        let guard = LAST_REQUEST.lock().ok();
        guard
            .and_then(|g| g.as_ref().and_then(|m| m.get(provider).copied()))
            .and_then(|last| min_interval.checked_sub(last.elapsed()))
    };
    if let Some(w) = wait {
        tokio::time::sleep(w).await;
    }
    if let Ok(mut guard) = LAST_REQUEST.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(provider.to_string(), Instant::now());
    }
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
    min_interval: Duration,
) -> std::result::Result<Vec<otw_store::histdata::Bar>, ChunkFail> {
    let mut attempt = 0u32;
    loop {
        if let Some(scope) = quota_scope {
            // Display-only counter; a failed write must not block the download.
            if let Err(e) = otw_store::api_quota::bump(pool, scope).await {
                tracing::debug!("histdata: quota bump failed: {e:#}");
            }
        }
        pace(&job.provider, min_interval).await;
        match connector
            .fetch_chunk(client, secrets, &job.ticker, &job.asset_type, &job.timeframe, from, to)
            .await
        {
            Ok(chunk) => return Ok(chunk.bars),
            Err(e) => {
                attempt += 1;
                let limited = is_rate_limited(&e);
                // Only transient errors (429/5xx, timeouts, connection drops) are worth
                // retrying. Permanent failures — 4xx, bad symbol, premium-gated endpoint,
                // bad/missing key — would just fail again and needlessly hold the single
                // worker through the backoff, so fail fast.
                if attempt > MAX_RETRIES || !(limited || is_transient(&e)) {
                    return Err(if limited {
                        // Hand the caller the provider's own Retry-After when it sent one.
                        ChunkFail::Limited {
                            retry_after: crate::rate::retry_after(&job.provider),
                        }
                    } else {
                        ChunkFail::Fatal(e)
                    });
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

/// Whether the provider said "too many requests". Some answer with HTTP 429, others
/// (Alpha Vantage, Massive) return 200 with a note in the body that the connector turns
/// into a plain error — hence the text check alongside the status one.
fn is_rate_limited(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
                return true;
            }
        }
    }
    let msg = err.to_string().to_lowercase();
    msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("too many requests")
        || msg.contains("requests per")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_then_caps() {
        assert_eq!(backoff_for(0), BASE_BACKOFF);
        assert_eq!(backoff_for(1), BASE_BACKOFF * 2);
        assert_eq!(backoff_for(3), BASE_BACKOFF * 8);
        // Far enough up the ladder the cap takes over, and stays there.
        assert_eq!(backoff_for(8), MAX_BACKOFF);
        assert_eq!(backoff_for(50), MAX_BACKOFF);
    }

    #[test]
    fn body_level_rate_limit_notes_are_recognized() {
        assert!(is_rate_limited(&anyhow::anyhow!(
            "alphavantage: our standard API rate limit is 25 requests per day"
        )));
        assert!(is_rate_limited(&anyhow::anyhow!("HTTP 429 Too Many Requests")));
        // A permanent failure must not be mistaken for a wait.
        assert!(!is_rate_limited(&anyhow::anyhow!("unknown symbol FOOBAR")));
        assert!(!is_rate_limited(&anyhow::anyhow!("NOT_AUTHORIZED: premium plan")));
    }

    #[test]
    fn human_duration_reads_in_the_right_unit() {
        assert_eq!(human_duration(45), "45 s");
        assert_eq!(human_duration(600), "10 min");
        assert_eq!(human_duration(7200), "2 h");
    }
}
