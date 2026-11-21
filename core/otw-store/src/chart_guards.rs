//! Guards on manual trading from the charts (`0213_chart_guards.sql`): the rules, when a chart
//! session ends, each account's session, and what the rules fired.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Guard {
    pub id: i64,
    /// `None`: every account.
    pub account_id: Option<Uuid>,
    pub rule: serde_json::Value,
    pub action: String,
    pub close_positions: bool,
    pub enabled: bool,
}

/// A guard as the user writes it.
#[derive(Debug, Clone, Deserialize)]
pub struct NewGuard {
    #[serde(default)]
    pub account_id: Option<Uuid>,
    pub rule: serde_json::Value,
    pub action: String,
    #[serde(default)]
    pub close_positions: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

const GUARD_COLUMNS: &str = "id, account_id, rule, action, close_positions, enabled";

pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<Guard>> {
    Ok(sqlx::query_as::<_, Guard>(sqlx::AssertSqlSafe(format!("SELECT {GUARD_COLUMNS} FROM chart_guards ORDER BY id")))
        .fetch_all(pool)
        .await?)
}

pub async fn insert(pool: &PgPool, g: &NewGuard) -> anyhow::Result<Guard> {
    Ok(sqlx::query_as::<_, Guard>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chart_guards (account_id, rule, action, close_positions, enabled) VALUES ($1, $2, $3, $4, $5) RETURNING {GUARD_COLUMNS}"
    )))
    .bind(g.account_id)
    .bind(&g.rule)
    .bind(&g.action)
    .bind(g.close_positions)
    .bind(g.enabled)
    .fetch_one(pool)
    .await?)
}

pub async fn update(pool: &PgPool, id: i64, g: &NewGuard) -> anyhow::Result<Option<Guard>> {
    Ok(sqlx::query_as::<_, Guard>(sqlx::AssertSqlSafe(format!(
        "UPDATE chart_guards SET account_id = $2, rule = $3, action = $4, close_positions = $5, enabled = $6, updated_at = now()
         WHERE id = $1 RETURNING {GUARD_COLUMNS}"
    )))
    .bind(id)
    .bind(g.account_id)
    .bind(&g.rule)
    .bind(&g.action)
    .bind(g.close_positions)
    .bind(g.enabled)
    .fetch_optional(pool)
    .await?)
}

pub async fn delete(pool: &PgPool, id: i64) -> anyhow::Result<bool> {
    Ok(sqlx::query("DELETE FROM chart_guards WHERE id = $1").bind(id).execute(pool).await?.rows_affected() > 0)
}

/// When a chart session ends on its own: `day` or `manual`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Settings {
    pub reset: String,
    pub timezone: String,
    /// When the settings were last written: a manual session never reset starts there.
    #[serde(skip_deserializing, with = "time::serde::rfc3339::option", default)]
    pub updated_at: Option<OffsetDateTime>,
}

pub async fn settings(pool: &PgPool) -> anyhow::Result<Settings> {
    Ok(sqlx::query_as::<_, Settings>("SELECT reset, timezone, updated_at FROM chart_guard_settings WHERE id")
        .fetch_optional(pool)
        .await?
        .unwrap_or(Settings { reset: "day".into(), timezone: "UTC".into(), updated_at: None }))
}

pub async fn set_settings(pool: &PgPool, s: &Settings) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO chart_guard_settings (id, reset, timezone) VALUES (TRUE, $1, $2)
         ON CONFLICT (id) DO UPDATE SET reset = EXCLUDED.reset, timezone = EXCLUDED.timezone, updated_at = now()",
    )
    .bind(&s.reset)
    .bind(&s.timezone)
    .execute(pool)
    .await?;
    Ok(())
}

/// One account's session record.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct Session {
    pub reset_at: Option<OffsetDateTime>,
    pub session_start: Option<OffsetDateTime>,
    pub equity: Option<Decimal>,
    pub currency: Option<String>,
}

pub async fn session(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Session> {
    Ok(sqlx::query_as::<_, Session>("SELECT reset_at, session_start, equity, currency FROM chart_guard_sessions WHERE account_id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_default())
}

/// Every account's last reset.
pub async fn resets(pool: &PgPool) -> anyhow::Result<Vec<(Uuid, Option<OffsetDateTime>)>> {
    Ok(sqlx::query_as("SELECT account_id, reset_at FROM chart_guard_sessions").fetch_all(pool).await?)
}

/// The equity the session starting at `start` began with.
pub async fn set_mark(pool: &PgPool, account_id: Uuid, start: OffsetDateTime, equity: Decimal, currency: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO chart_guard_sessions (account_id, session_start, equity, currency) VALUES ($1, $2, $3, $4)
         ON CONFLICT (account_id) DO UPDATE SET session_start = EXCLUDED.session_start, equity = EXCLUDED.equity, currency = EXCLUDED.currency",
    )
    .bind(account_id)
    .bind(start)
    .bind(equity)
    .bind(currency)
    .execute(pool)
    .await?;
    Ok(())
}

/// The user's reset: a new session starts now on the account, and whatever the last one
/// stopped is cleared.
pub async fn reset(pool: &PgPool, account_id: Uuid, now: OffsetDateTime) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO chart_guard_sessions (account_id, reset_at) VALUES ($1, $2)
         ON CONFLICT (account_id) DO UPDATE SET reset_at = EXCLUDED.reset_at, session_start = NULL, equity = NULL, currency = NULL",
    )
    .bind(account_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE chart_guard_events SET cleared_at = $2 WHERE account_id = $1 AND cleared_at IS NULL")
        .bind(account_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    pub guard_id: Option<i64>,
    pub account_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub session_start: OffsetDateTime,
    pub key: String,
    pub action: String,
    pub detail: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub acknowledged_at: Option<OffsetDateTime>,
}

const EVENT_COLUMNS: &str = "id, guard_id, account_id, session_start, key, action, detail, at, acknowledged_at";

/// Record a firing. `None` when this guard already fired for this occurrence: nothing to do.
pub async fn fire(pool: &PgPool, guard_id: i64, account_id: Uuid, session_start: OffsetDateTime, key: &str, action: &str, detail: &str) -> anyhow::Result<Option<Event>> {
    Ok(sqlx::query_as::<_, Event>(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chart_guard_events (guard_id, account_id, session_start, key, action, detail) VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (guard_id, account_id, session_start, key) DO NOTHING RETURNING {EVENT_COLUMNS}"
    )))
    .bind(guard_id)
    .bind(account_id)
    .bind(session_start)
    .bind(key)
    .bind(action)
    .bind(detail)
    .fetch_optional(pool)
    .await?)
}

/// The account's stop in force: a `stop` of its current session, not cleared.
pub async fn stopped(pool: &PgPool, account_id: Uuid, session_start: OffsetDateTime) -> anyhow::Result<Option<Event>> {
    Ok(sqlx::query_as::<_, Event>(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_COLUMNS} FROM chart_guard_events
         WHERE account_id = $1 AND session_start = $2 AND action = 'stop' AND cleared_at IS NULL ORDER BY at LIMIT 1"
    )))
    .bind(account_id)
    .bind(session_start)
    .fetch_optional(pool)
    .await?)
}

/// What fired since `since`, newest first: what the chart shows.
pub async fn recent(pool: &PgPool, since: OffsetDateTime) -> anyhow::Result<Vec<Event>> {
    Ok(sqlx::query_as::<_, Event>(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_COLUMNS} FROM chart_guard_events WHERE at >= $1 AND cleared_at IS NULL ORDER BY at DESC LIMIT 200"
    )))
    .bind(since)
    .fetch_all(pool)
    .await?)
}

/// The user saw it: one event, or every one not yet seen when `None`.
pub async fn acknowledge(pool: &PgPool, id: Option<i64>) -> anyhow::Result<u64> {
    Ok(sqlx::query("UPDATE chart_guard_events SET acknowledged_at = now() WHERE acknowledged_at IS NULL AND ($1::bigint IS NULL OR id = $1)")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected())
}

/// The account's chart legs sent since `since`: the client id, the broker's id, the minted
/// venue client id, whether it is an entry, the instrument and when.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChartLeg {
    pub client_id: String,
    pub venue_id: Option<String>,
    pub venue_client_id: Option<String>,
    pub entry: bool,
    pub ticker: String,
    pub asset_type: String,
    pub status: String,
    pub created_at: OffsetDateTime,
}

pub async fn chart_legs(pool: &PgPool, account_id: Uuid, since: OffsetDateTime) -> anyhow::Result<Vec<ChartLeg>> {
    Ok(sqlx::query_as::<_, ChartLeg>(
        "SELECT client_id, venue_id, venue_client_id, (action = 'place' AND parent IS NULL) AS entry, ticker, asset_type, status, created_at
         FROM broker_orders WHERE account_id = $1 AND source = 'session' AND created_at >= $2 ORDER BY created_at",
    )
    .bind(account_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}
