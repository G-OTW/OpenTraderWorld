//! The broker order audit log: one row per order call sent to a broker (place, move, cancel,
//! cancel all) with the broker's answer and how long it took. Append-only in the database
//! (a trigger refuses UPDATE and DELETE); this module only appends and reads.
//!
//! The per-account order metrics (calls, refusals, latency) are computed from it rather than
//! kept as counters, so they survive a restart and cannot drift from the trail.

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// One call, as it is written.
pub struct NewEntry<'a> {
    pub account_id: Uuid,
    pub account: &'a str,
    pub broker: &'a str,
    pub action: &'a str,
    pub request: &'a serde_json::Value,
    pub ok: bool,
    pub response: &'a serde_json::Value,
    pub latency_ms: i32,
    /// Who sent it: `session`, `alert:<id>`, `strategy:<id>`; `None` for a reconciliation
    /// finding, which no sender caused.
    pub source: Option<&'a str>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub account_id: Uuid,
    pub account: String,
    pub broker: String,
    pub action: String,
    pub request: serde_json::Value,
    pub ok: bool,
    pub response: serde_json::Value,
    pub latency_ms: i32,
    pub source: Option<String>,
}

pub async fn append(pool: &PgPool, e: &NewEntry<'_>) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO broker_order_log (account_id, account, broker, action, request, ok, response, latency_ms, source)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(e.account_id)
    .bind(e.account)
    .bind(e.broker)
    .bind(e.action)
    .bind(e.request)
    .bind(e.ok)
    .bind(e.response)
    .bind(e.latency_ms)
    .bind(e.source)
    .execute(pool)
    .await?;
    Ok(())
}

/// Newest first, for one account or all of them.
pub async fn list(pool: &PgPool, account_id: Option<Uuid>, limit: i64) -> anyhow::Result<Vec<Entry>> {
    Ok(sqlx::query_as::<_, Entry>(
        "SELECT id, at, account_id, account, broker, action, request, ok, response, latency_ms, source
         FROM broker_order_log
         WHERE $1::uuid IS NULL OR account_id = $1
         ORDER BY at DESC, id DESC
         LIMIT $2",
    )
    .bind(account_id)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?)
}

/// One account's order metrics over the whole trail.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Stats {
    pub account_id: Uuid,
    /// Order calls of any kind.
    pub calls: i64,
    /// Orders placed (legs of accepted tickets count once per ticket).
    pub placed: i64,
    /// Calls the broker refused or never answered.
    pub refused: i64,
    pub latency_p50_ms: Option<f64>,
    pub latency_p99_ms: Option<f64>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_at: Option<OffsetDateTime>,
}

pub async fn stats(pool: &PgPool) -> anyhow::Result<Vec<Stats>> {
    Ok(sqlx::query_as::<_, Stats>(
        "SELECT account_id,
                COUNT(*) AS calls,
                COUNT(*) FILTER (WHERE action = 'place' AND ok) AS placed,
                COUNT(*) FILTER (WHERE NOT ok) AS refused,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY latency_ms) AS latency_p50_ms,
                percentile_cont(0.99) WITHIN GROUP (ORDER BY latency_ms) AS latency_p99_ms,
                MAX(at) AS last_at
         FROM broker_order_log
         WHERE action <> 'reconcile'
         GROUP BY account_id",
    )
    .fetch_all(pool)
    .await?)
}
