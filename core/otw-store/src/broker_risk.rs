//! The user's risk limits per trading account, the halt policy, the halts themselves and the
//! equity each account started its loss period with (`0160_broker_risk.sql`).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// One account's limits; `None` is off.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Limits {
    #[serde(default)]
    pub max_order_qty: Option<Decimal>,
    #[serde(default)]
    pub max_order_value: Option<Decimal>,
    #[serde(default)]
    pub max_position_qty: Option<Decimal>,
    #[serde(default)]
    pub max_position_value: Option<Decimal>,
    #[serde(default)]
    pub collar_pct: Option<Decimal>,
    #[serde(default)]
    pub max_orders_per_min: Option<i32>,
    #[serde(default)]
    pub max_open_orders: Option<i32>,
    #[serde(default)]
    pub max_loss: Option<Decimal>,
    #[serde(default)]
    pub duplicate_secs: Option<i32>,
}

impl Limits {
    pub fn is_empty(&self) -> bool {
        *self == Limits::default()
    }
}

pub async fn limits(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Limits> {
    Ok(sqlx::query_as::<_, Limits>(
        "SELECT max_order_qty, max_order_value, max_position_qty, max_position_value, collar_pct, max_orders_per_min, max_open_orders, max_loss, duplicate_secs
         FROM broker_risk_limits WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or_default())
}

/// Every account's limits, by account.
pub async fn all_limits(pool: &PgPool) -> anyhow::Result<Vec<(Uuid, Limits)>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        account_id: Uuid,
        #[sqlx(flatten)]
        limits: Limits,
    }
    Ok(sqlx::query_as::<_, Row>(
        "SELECT account_id, max_order_qty, max_order_value, max_position_qty, max_position_value, collar_pct, max_orders_per_min, max_open_orders, max_loss, duplicate_secs
         FROM broker_risk_limits",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| (r.account_id, r.limits))
    .collect())
}

pub async fn set_limits(pool: &PgPool, account_id: Uuid, l: &Limits) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO broker_risk_limits (account_id, max_order_qty, max_order_value, max_position_qty, max_position_value, collar_pct, max_orders_per_min, max_open_orders, max_loss, duplicate_secs)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (account_id) DO UPDATE SET
           max_order_qty = EXCLUDED.max_order_qty, max_order_value = EXCLUDED.max_order_value,
           max_position_qty = EXCLUDED.max_position_qty, max_position_value = EXCLUDED.max_position_value, collar_pct = EXCLUDED.collar_pct,
           max_orders_per_min = EXCLUDED.max_orders_per_min, max_open_orders = EXCLUDED.max_open_orders,
           max_loss = EXCLUDED.max_loss, duplicate_secs = EXCLUDED.duplicate_secs, updated_at = now()",
    )
    .bind(account_id)
    .bind(l.max_order_qty)
    .bind(l.max_order_value)
    .bind(l.max_position_qty)
    .bind(l.max_position_value)
    .bind(l.collar_pct)
    .bind(l.max_orders_per_min)
    .bind(l.max_open_orders)
    .bind(l.max_loss)
    .bind(l.duplicate_secs)
    .execute(pool)
    .await?;
    Ok(())
}

/// How a halt ends: `reset` is hour | day | week | manual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Policy {
    pub reset: String,
    pub password_lift: bool,
    pub timezone: String,
}

pub async fn policy(pool: &PgPool) -> anyhow::Result<Policy> {
    Ok(sqlx::query_as::<_, Policy>("SELECT reset, password_lift, timezone FROM trading_halt_policy WHERE id")
        .fetch_optional(pool)
        .await?
        .unwrap_or(Policy { reset: "day".into(), password_lift: true, timezone: "UTC".into() }))
}

pub async fn set_policy(pool: &PgPool, p: &Policy) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO trading_halt_policy (id, reset, password_lift, timezone) VALUES (TRUE, $1, $2, $3)
         ON CONFLICT (id) DO UPDATE SET reset = EXCLUDED.reset, password_lift = EXCLUDED.password_lift,
           timezone = EXCLUDED.timezone, updated_at = now()",
    )
    .bind(&p.reset)
    .bind(p.password_lift)
    .bind(&p.timezone)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Halt {
    pub id: i64,
    /// `None`: every account.
    pub account_id: Option<Uuid>,
    pub reason: String,
    pub detail: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub until: Option<OffsetDateTime>,
}

/// Halts in force now: on `account` or on every account, or every one at all when `None`.
pub async fn halts(pool: &PgPool, account: Option<Uuid>, now: OffsetDateTime) -> anyhow::Result<Vec<Halt>> {
    Ok(sqlx::query_as::<_, Halt>(
        "SELECT id, account_id, reason, detail, created_at, until FROM trading_halts
         WHERE lifted_at IS NULL AND (until IS NULL OR until > $2)
           AND ($1::uuid IS NULL OR account_id IS NULL OR account_id = $1)
         ORDER BY created_at",
    )
    .bind(account)
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn add_halt(pool: &PgPool, account: Option<Uuid>, reason: &str, detail: &str, until: Option<OffsetDateTime>) -> anyhow::Result<Halt> {
    Ok(sqlx::query_as::<_, Halt>(
        "INSERT INTO trading_halts (account_id, reason, detail, until) VALUES ($1, $2, $3, $4)
         RETURNING id, account_id, reason, detail, created_at, until",
    )
    .bind(account)
    .bind(reason)
    .bind(detail)
    .bind(until)
    .fetch_one(pool)
    .await?)
}

/// Lift halts: one by id, or every one in force. How many were lifted.
pub async fn lift(pool: &PgPool, id: Option<i64>) -> anyhow::Result<u64> {
    Ok(sqlx::query("UPDATE trading_halts SET lifted_at = now() WHERE lifted_at IS NULL AND ($1::bigint IS NULL OR id = $1)")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected())
}

/// Equity at the start of the account's current loss period, if it was recorded.
pub async fn equity_mark(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Option<(OffsetDateTime, Decimal, String)>> {
    Ok(sqlx::query_as("SELECT period_start, equity, currency FROM broker_equity_marks WHERE account_id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await?)
}

pub async fn set_equity_mark(pool: &PgPool, account_id: Uuid, period_start: OffsetDateTime, equity: Decimal, currency: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO broker_equity_marks (account_id, period_start, equity, currency) VALUES ($1, $2, $3, $4)
         ON CONFLICT (account_id) DO UPDATE SET period_start = EXCLUDED.period_start, equity = EXCLUDED.equity, currency = EXCLUDED.currency",
    )
    .bind(account_id)
    .bind(period_start)
    .bind(equity)
    .bind(currency)
    .execute(pool)
    .await?;
    Ok(())
}

/// Legs OTW sent from the account since `since` (refused ones included): the order rate.
pub async fn sent_since(pool: &PgPool, account_id: Uuid, since: OffsetDateTime) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM broker_orders WHERE account_id = $1 AND created_at >= $2")
        .bind(account_id)
        .bind(since)
        .fetch_one(pool)
        .await?)
}

/// Legs on this instrument sent since `since`, as their requests: what a duplicate is
/// compared with.
pub async fn recent_requests(pool: &PgPool, account_id: Uuid, ticker: &str, since: OffsetDateTime) -> anyhow::Result<Vec<serde_json::Value>> {
    Ok(sqlx::query_scalar(
        "SELECT request FROM broker_orders
         WHERE account_id = $1 AND upper(ticker) = upper($2) AND created_at >= $3 AND action = 'place' AND status <> 'rejected'",
    )
    .bind(account_id)
    .bind(ticker)
    .bind(since)
    .fetch_all(pool)
    .await?)
}
