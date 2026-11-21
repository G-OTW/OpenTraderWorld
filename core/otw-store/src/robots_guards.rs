//! What a robot's guards did, and the per-day counters its caps read (migration 0204).
//!
//! The rules and the state they hold live on the robot row (`guards`, `guard_state`, see
//! [`crate::robots::Session`]); `otw-core`'s `robots::guards` decides, this keeps the record:
//! one row per trigger or restoration for the dashboard's timeline, and one row per robot and
//! day for the order cap and the daily figures, so neither scans the fills.

use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// One trigger or one restoration.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct GuardEvent {
    pub id: Uuid,
    pub session_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The rule as it was written (`{ when, then }`), empty for a manual resume.
    pub rule: Value,
    /// What it did: `{ kind: pause|halt|scale|restore|resume, ... }`.
    pub action: Value,
    /// What made it fire: the count, the drawdown, the window's P&L, the trade that closed.
    pub trigger: Value,
    #[serde(with = "time::serde::rfc3339::option")]
    pub cleared_at: Option<OffsetDateTime>,
}

pub async fn add_event(
    pool: &PgPool,
    session_id: Uuid,
    at: OffsetDateTime,
    rule: &Value,
    action: &Value,
    trigger: &Value,
) -> anyhow::Result<GuardEvent> {
    Ok(sqlx::query_as::<_, GuardEvent>(
        "INSERT INTO robot_guard_events (id, session_id, at, rule, action, trigger) VALUES ($1,$2,$3,$4,$5,$6) \
         RETURNING id, session_id, at, rule, action, trigger, cleared_at",
    )
    .bind(Uuid::new_v4())
    .bind(session_id)
    .bind(at)
    .bind(rule)
    .bind(action)
    .bind(trigger)
    .fetch_one(pool)
    .await?)
}

/// Mark the triggers still in force as cleared (a restoration, a resume).
pub async fn clear_open(pool: &PgPool, session_id: Uuid, kinds: &[&str], at: OffsetDateTime) -> anyhow::Result<u64> {
    let kinds: Vec<String> = kinds.iter().map(|k| k.to_string()).collect();
    Ok(sqlx::query(
        "UPDATE robot_guard_events SET cleared_at = $3 \
         WHERE session_id = $1 AND cleared_at IS NULL AND action->>'kind' = ANY($2)",
    )
    .bind(session_id)
    .bind(&kinds)
    .bind(at)
    .execute(pool)
    .await?
    .rows_affected())
}

/// Newest first.
pub async fn events(pool: &PgPool, session_id: Uuid, limit: i64) -> anyhow::Result<Vec<GuardEvent>> {
    Ok(sqlx::query_as::<_, GuardEvent>(
        "SELECT id, session_id, at, rule, action, trigger, cleared_at FROM robot_guard_events \
         WHERE session_id = $1 ORDER BY at DESC, id LIMIT $2",
    )
    .bind(session_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// One robot's day.
#[derive(Debug, Clone, Default, Serialize, sqlx::FromRow)]
pub struct Daily {
    pub day: Option<Date>,
    pub orders_sent: i32,
    /// Sums of the robot's settled trade money (`robot_trades.robot_pnl`, `robot_fees`).
    pub realized: Decimal,
    pub fees: Decimal,
}

/// Count one opening order sent (or one entry taken, for a simulated robot).
pub async fn bump_orders(pool: &PgPool, session_id: Uuid, day: Date) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO robot_daily (session_id, day, orders_sent) VALUES ($1, $2, 1) \
         ON CONFLICT (session_id, day) DO UPDATE SET orders_sent = robot_daily.orders_sent + 1",
    )
    .bind(session_id)
    .bind(day)
    .execute(pool)
    .await?;
    Ok(())
}

/// Recount one day's realized P&L and fees from the robot's closed trades (its own size), so
/// a trade the replay re-prices is never counted twice.
pub async fn refresh_realized(pool: &PgPool, session_id: Uuid, day: Date) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO robot_daily (session_id, day, realized, fees) \
         SELECT $1, $2, COALESCE(sum(robot_pnl), 0), COALESCE(sum(robot_fees), 0) \
         FROM robot_trades WHERE session_id = $1 AND NOT open AND robot_pnl IS NOT NULL \
           AND (closed_at AT TIME ZONE 'UTC')::date = $2 \
         ON CONFLICT (session_id, day) DO UPDATE SET realized = EXCLUDED.realized, fees = EXCLUDED.fees",
    )
    .bind(session_id)
    .bind(day)
    .execute(pool)
    .await?;
    Ok(())
}

/// One day of one robot; zeros when nothing happened.
pub async fn day(pool: &PgPool, session_id: Uuid, day: Date) -> anyhow::Result<Daily> {
    Ok(sqlx::query_as::<_, Daily>(
        "SELECT day, orders_sent, realized, fees FROM robot_daily WHERE session_id = $1 AND day = $2",
    )
    .bind(session_id)
    .bind(day)
    .fetch_optional(pool)
    .await?
    .unwrap_or(Daily { day: Some(day), ..Default::default() }))
}

/// Today of every robot at once, for the fleet.
pub async fn today_all(pool: &PgPool, day: Date) -> anyhow::Result<Vec<(Uuid, Daily)>> {
    let rows: Vec<(Uuid, Date, i32, Decimal, Decimal)> =
        sqlx::query_as("SELECT session_id, day, orders_sent, realized, fees FROM robot_daily WHERE day = $1")
            .bind(day)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, d, o, r, f)| (id, Daily { day: Some(d), orders_sent: o, realized: r, fees: f }))
        .collect())
}
