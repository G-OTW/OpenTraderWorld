//! Centralized API request quotas — user-declared limits + rolling usage counters.
//!
//! One row per tracked *scope* (`feed:<uuid>` for news API sources, `histconn:<uuid>` for
//! histdata connectors). The owning module declares the limit (`set`) and records each
//! outbound request (`bump`); the UI reads `used / max_requests` for its progress bar or
//! pie. The window is calendar-aligned via `date_trunc(period, now())` and rolls lazily:
//! a bump or read outside the stored window sees `used` reset to zero.
//!
//! Like `api_rate`, this is observe-and-display only — nothing here blocks a request.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::time::OffsetDateTime;
use sqlx::PgPool;

/// Allowed quota periods, matching PostgreSQL `date_trunc` field names.
pub const PERIODS: &[&str] = &["minute", "hour", "day", "week", "month"];

pub fn valid_period(p: &str) -> bool {
    PERIODS.contains(&p)
}

/// A quota row with the usage already normalized to the current window.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Quota {
    pub scope: String,
    /// None = unlimited: usage is tracked and displayed, but there is no cap to fill.
    pub max_requests: Option<i64>,
    pub period: String,
    pub used: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub resets_at: OffsetDateTime,
}

/// Columns with the window normalization applied (used=0 once the period rolled over).
const QUOTA_COLS: &str = "scope, max_requests, period, \
     CASE WHEN date_trunc(period, now()) > period_start THEN 0 ELSE used END AS used, \
     date_trunc(period, now()) + ('1 ' || period)::interval AS resets_at";

/// Declare (or replace) the quota for a scope. Changing the period resets the counter;
/// changing only the cap keeps the current window's usage.
pub async fn set(
    pool: &PgPool,
    scope: &str,
    max_requests: Option<i64>,
    period: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(valid_period(period), "invalid quota period: {period}");
    sqlx::query(
        "INSERT INTO api_quotas (scope, max_requests, period, period_start, used) \
         VALUES ($1, $2, $3, date_trunc($3, now()), 0) \
         ON CONFLICT (scope) DO UPDATE SET \
             max_requests = EXCLUDED.max_requests, \
             period = EXCLUDED.period, \
             used = CASE WHEN api_quotas.period = EXCLUDED.period THEN api_quotas.used ELSE 0 END, \
             period_start = CASE WHEN api_quotas.period = EXCLUDED.period \
                                 THEN api_quotas.period_start ELSE date_trunc($3, now()) END, \
             updated_at = now()",
    )
    .bind(scope)
    .bind(max_requests)
    .bind(period)
    .execute(pool)
    .await
    .context("setting api quota")?;
    Ok(())
}

/// Stop tracking a scope (checkbox off / owner deleted).
pub async fn remove(pool: &PgPool, scope: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM api_quotas WHERE scope = $1")
        .bind(scope)
        .execute(pool)
        .await
        .context("removing api quota")?;
    Ok(())
}

/// Count one outbound request against a scope, rolling the window if the period elapsed.
/// A scope with no declared quota is a no-op, so call sites don't need to check first.
pub async fn bump(pool: &PgPool, scope: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE api_quotas SET \
             used = CASE WHEN date_trunc(period, now()) > period_start THEN 1 ELSE used + 1 END, \
             period_start = date_trunc(period, now()), \
             updated_at = now() \
         WHERE scope = $1",
    )
    .bind(scope)
    .execute(pool)
    .await
    .context("bumping api quota")?;
    Ok(())
}

pub async fn get(pool: &PgPool, scope: &str) -> anyhow::Result<Option<Quota>> {
    let sql = format!("SELECT {QUOTA_COLS} FROM api_quotas WHERE scope = $1");
    Ok(sqlx::query_as::<_, Quota>(sqlx::AssertSqlSafe(sql))
        .bind(scope)
        .fetch_optional(pool)
        .await
        .context("reading api quota")?)
}

/// All quotas whose scope starts with `prefix` (e.g. `feed:`), for batch display.
pub async fn list_prefixed(pool: &PgPool, prefix: &str) -> anyhow::Result<Vec<Quota>> {
    let sql = format!("SELECT {QUOTA_COLS} FROM api_quotas WHERE scope LIKE $1 || '%'");
    Ok(sqlx::query_as::<_, Quota>(sqlx::AssertSqlSafe(sql))
        .bind(prefix)
        .fetch_all(pool)
        .await
        .context("listing api quotas")?)
}
