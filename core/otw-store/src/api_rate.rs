//! API rate tracking persistence — outbound-call counters + over-limit events.
//!
//! `otw-core`'s `rate` module records every external provider call here (fire-and-forget):
//! `bump` increments the per-day rollup, and rate-limit hits also `log_event`. The Settings
//! "API Rate" section reads `usage_since` (today's per-provider volume) and `recent_events`.
//! Nothing here blocks a request — it is observe-and-alert only.

use std::sync::OnceLock;

use anyhow::Context;
use serde::Serialize;
use sqlx::types::time::OffsetDateTime;
use sqlx::PgPool;

/// Keep at most this many over-limit event rows.
pub const EVENT_RETENTION_ROWS: i64 = 5_000;
/// Drop daily rollups older than this many days.
pub const DAILY_RETENTION_DAYS: i64 = 30;

/// Outcome class of a recorded call, used to bump the right counter.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// Rate-limited (HTTP 429 or a provider "too many requests" body note).
    Limited,
    /// Any other failure (>=400, network/timeout).
    Error,
}

/// A per-provider usage row for the dashboard.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Usage {
    pub provider: String,
    pub host: String,
    pub requests: i64,
    pub limited: i64,
    pub errors: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub last_at: OffsetDateTime,
}

/// One over-limit hit, for the recent-hits list.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub provider: String,
    pub host: String,
    pub status: Option<i32>,
    pub detail: String,
}

// ── Process-wide recorder ───────────────────────────────────────────────────────────────
//
// The pool is published once at startup so any crate (core call sites, the feed scheduler)
// can record outbound calls without threading a pool through. Recording is fire-and-forget:
// each event spawns a detached task, so a slow/failed DB write never stalls the caller.

static POOL: OnceLock<PgPool> = OnceLock::new();

/// Publish the pool. Called once at startup, before any call is instrumented.
pub fn init(pool: PgPool) {
    let _ = POOL.set(pool);
}

/// Record one outbound call to `provider` at `host`, classified by `outcome`. Never blocks,
/// never errors. A `Limited` outcome also writes a dashboard event and a WARN log. `status`
/// is the HTTP status when known; `detail` a short human note (e.g. "retry-after: 30").
pub fn record(provider: &str, host: &str, outcome: Outcome, status: Option<i32>, detail: &str) {
    let Some(pool) = POOL.get() else { return };
    let (provider, host, detail) = (provider.to_string(), host.to_string(), detail.to_string());
    let pool = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = bump(&pool, &provider, &host, outcome).await {
            tracing::debug!("api rate: bump failed: {e:#}");
        }
        if outcome == Outcome::Limited {
            tracing::warn!(
                "api rate limit: {provider} ({host}) returned too-many-requests{}{}",
                status.map(|s| format!(" [{s}]")).unwrap_or_default(),
                if detail.is_empty() { String::new() } else { format!(" — {detail}") },
            );
            if let Err(e) = log_event(&pool, &provider, &host, status, &detail).await {
                tracing::debug!("api rate: log_event failed: {e:#}");
            }
        }
    });
}

/// Increment today's rollup for `(provider, host)`, classifying the call by `outcome`.
/// Day is UTC so "since the beginning of the day" is stable regardless of host timezone.
pub async fn bump(
    pool: &PgPool,
    provider: &str,
    host: &str,
    outcome: Outcome,
) -> anyhow::Result<()> {
    let (lim, err) = match outcome {
        Outcome::Ok => (0i64, 0i64),
        Outcome::Limited => (1, 0),
        Outcome::Error => (0, 1),
    };
    sqlx::query(
        "INSERT INTO api_rate_daily (provider, host, requests, limited, errors, last_at) \
         VALUES ($1, $2, 1, $3, $4, now()) \
         ON CONFLICT (provider, host, day) DO UPDATE SET \
             requests = api_rate_daily.requests + 1, \
             limited  = api_rate_daily.limited  + EXCLUDED.limited, \
             errors   = api_rate_daily.errors   + EXCLUDED.errors, \
             last_at  = now()",
    )
    .bind(provider)
    .bind(host)
    .bind(lim)
    .bind(err)
    .execute(pool)
    .await
    .context("bumping api rate counter")?;
    Ok(())
}

/// Record a single over-limit hit for the recent-events list.
pub async fn log_event(
    pool: &PgPool,
    provider: &str,
    host: &str,
    status: Option<i32>,
    detail: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO api_rate_events (provider, host, status, detail) VALUES ($1, $2, $3, $4)",
    )
    .bind(provider)
    .bind(host)
    .bind(status)
    .bind(detail)
    .execute(pool)
    .await
    .context("inserting api rate event")?;
    Ok(())
}

/// Per-provider usage rolled up across hosts for the last `days` days (1 = today only),
/// ordered by request volume descending. UTC day boundaries.
pub async fn usage_since(pool: &PgPool, days: i64) -> anyhow::Result<Vec<Usage>> {
    let days = days.clamp(1, DAILY_RETENTION_DAYS);
    let rows = sqlx::query_as::<_, Usage>(
        "SELECT provider, \
                MAX(host) AS host, \
                SUM(requests)::bigint AS requests, \
                SUM(limited)::bigint  AS limited, \
                SUM(errors)::bigint   AS errors, \
                MAX(last_at) AS last_at \
         FROM api_rate_daily \
         WHERE day > (now() AT TIME ZONE 'UTC')::date - $1::int \
         GROUP BY provider \
         ORDER BY requests DESC, provider",
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await
    .context("reading api rate usage")?;
    Ok(rows)
}

/// Most recent over-limit events, newest first.
pub async fn recent_events(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Event>> {
    let rows = sqlx::query_as::<_, Event>(
        "SELECT id, at, provider, host, status, detail \
         FROM api_rate_events ORDER BY at DESC, id DESC LIMIT $1",
    )
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await
    .context("listing api rate events")?;
    Ok(rows)
}

/// Drop rollups older than the retention window and trim the event log. Cheap; call on a cadence.
pub async fn trim(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM api_rate_daily \
         WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int",
    )
    .bind(DAILY_RETENTION_DAYS as i32)
    .execute(pool)
    .await
    .context("trimming api rate daily")?;
    sqlx::query(
        "DELETE FROM api_rate_events WHERE id NOT IN \
         (SELECT id FROM api_rate_events ORDER BY id DESC LIMIT $1)",
    )
    .bind(EVENT_RETENTION_ROWS)
    .execute(pool)
    .await
    .context("trimming api rate events")?;
    Ok(())
}
