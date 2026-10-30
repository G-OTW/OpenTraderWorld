//! Trades on alert (`alert_trades`): the ticket a chart alert sends to a broker account when it
//! fires, whether it is armed, and how its last fire went.
//!
//! The row is the truth across restarts. A fire takes it with [`take`], which disarms it in
//! the same statement that reads it: two evaluators racing on one alert, or a process that
//! dies after the order left, can never send it a second time. Re-arming is the user's act.
//!
//! The ticket is stored as the client sent it (a template validated in otw-core); this module
//! does not read inside it.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Statuses a fire leaves a row in, and the ones still followed after it.
pub const FOLLOWED: &[&str] = &["sent", "unknown"];

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AlertTrade {
    pub alert_id: Uuid,
    pub account_id: Uuid,
    /// `enter` | `exit`.
    pub intent: String,
    pub ticket: serde_json::Value,
    /// `none` | `cancel_symbol` | `close_symbol` | `flatten_account`.
    pub fallback: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub armed_at: Option<OffsetDateTime>,
    pub armed_by: Option<Uuid>,
    pub status: String,
    pub last_result: serde_json::Value,
    #[serde(with = "time::serde::rfc3339::option")]
    pub fired_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl AlertTrade {
    pub fn armed(&self) -> bool {
        self.armed_at.is_some()
    }
}

const COLS: &str = "alert_id, account_id, intent, ticket, fallback, armed_at, armed_by, status, \
                    last_result, fired_at, updated_at";

/// An armed trade with the instrument its alert watches: what the exposure line adds up.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Armed {
    pub alert_id: Uuid,
    pub alert_name: String,
    pub intent: String,
    pub ticket: serde_json::Value,
}

pub async fn get(pool: &PgPool, alert_id: Uuid) -> anyhow::Result<Option<AlertTrade>> {
    let sql = format!("SELECT {COLS} FROM alert_trades WHERE alert_id = $1");
    Ok(sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql)).bind(alert_id).fetch_optional(pool).await?)
}

/// Every trade on alert, newest change first.
pub async fn list(pool: &PgPool) -> anyhow::Result<Vec<AlertTrade>> {
    let sql = format!("SELECT {COLS} FROM alert_trades ORDER BY updated_at DESC");
    Ok(sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

/// Write the trade of an alert. A change to the account, the intent, the ticket or the
/// fallback disarms it: the user arms what they last saw, never an edit made after. The same
/// content written again keeps it as it was. `.1` says whether this write disarmed it.
pub async fn save(
    pool: &PgPool,
    alert_id: Uuid,
    account_id: Uuid,
    intent: &str,
    ticket: &serde_json::Value,
    fallback: &str,
) -> anyhow::Result<(AlertTrade, bool)> {
    let was_armed = get(pool, alert_id).await?.is_some_and(|t| t.armed());
    let sql = format!(
        "INSERT INTO alert_trades (alert_id, account_id, intent, ticket, fallback) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (alert_id) DO UPDATE SET \
           account_id = EXCLUDED.account_id, intent = EXCLUDED.intent, ticket = EXCLUDED.ticket, \
           fallback = EXCLUDED.fallback, \
           armed_at = CASE WHEN alert_trades.account_id = EXCLUDED.account_id AND alert_trades.intent = EXCLUDED.intent \
                            AND alert_trades.ticket = EXCLUDED.ticket AND alert_trades.fallback = EXCLUDED.fallback \
                           THEN alert_trades.armed_at END, \
           armed_by = CASE WHEN alert_trades.account_id = EXCLUDED.account_id AND alert_trades.intent = EXCLUDED.intent \
                            AND alert_trades.ticket = EXCLUDED.ticket AND alert_trades.fallback = EXCLUDED.fallback \
                           THEN alert_trades.armed_by END, \
           updated_at = now() \
         RETURNING {COLS}"
    );
    let row = sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql))
        .bind(alert_id)
        .bind(account_id)
        .bind(intent)
        .bind(ticket)
        .bind(fallback)
        .fetch_one(pool)
        .await
        .context("saving the trade on alert")?;
    let disarmed = was_armed && !row.armed();
    Ok((row, disarmed))
}

pub async fn delete(pool: &PgPool, alert_id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM alert_trades WHERE alert_id = $1").bind(alert_id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

/// Arm the trade: a fire may now send it. The last fire's outcome stays readable in
/// `last_result`; the status starts over.
pub async fn arm(pool: &PgPool, alert_id: Uuid, by: Uuid) -> anyhow::Result<Option<AlertTrade>> {
    let sql = format!(
        "UPDATE alert_trades SET armed_at = now(), armed_by = $2, status = 'idle', updated_at = now() \
         WHERE alert_id = $1 RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql)).bind(alert_id).bind(by).fetch_optional(pool).await?)
}

/// Disarm one trade. `false` when it was not armed.
pub async fn disarm(pool: &PgPool, alert_id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE alert_trades SET armed_at = NULL, armed_by = NULL, updated_at = now() \
         WHERE alert_id = $1 AND armed_at IS NOT NULL",
    )
    .bind(alert_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Disarm every trade armed on one account; how many were.
pub async fn disarm_account(pool: &PgPool, account_id: Uuid) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "UPDATE alert_trades SET armed_at = NULL, armed_by = NULL, updated_at = now() \
         WHERE account_id = $1 AND armed_at IS NOT NULL",
    )
    .bind(account_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Disarm every armed trade; how many were.
pub async fn disarm_all(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "UPDATE alert_trades SET armed_at = NULL, armed_by = NULL, updated_at = now() WHERE armed_at IS NOT NULL",
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Take an armed trade for a fire: disarmed and marked `sending` in one statement, so it is
/// taken once whatever races it. `None` when it was not armed (or already taken).
pub async fn take(pool: &PgPool, alert_id: Uuid) -> anyhow::Result<Option<AlertTrade>> {
    let sql = format!(
        "UPDATE alert_trades SET armed_at = NULL, armed_by = NULL, status = 'sending', fired_at = now(), \
           last_result = '{{}}'::jsonb, updated_at = now() \
         WHERE alert_id = $1 AND armed_at IS NOT NULL RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql)).bind(alert_id).fetch_optional(pool).await?)
}

/// Record how a fire went.
pub async fn finish(pool: &PgPool, alert_id: Uuid, status: &str, result: &serde_json::Value) -> anyhow::Result<()> {
    sqlx::query("UPDATE alert_trades SET status = $2, last_result = $3, updated_at = now() WHERE alert_id = $1")
        .bind(alert_id)
        .bind(status)
        .bind(result)
        .execute(pool)
        .await?;
    Ok(())
}

/// Move a trade from one of `from` to `to`, merging `patch` into its result. `false` when it
/// was not in one of `from`: whoever moved it first owns the notification that goes with it.
pub async fn advance(pool: &PgPool, alert_id: Uuid, from: &[&str], to: &str, patch: &serde_json::Value) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE alert_trades SET status = $3, last_result = last_result || $4, updated_at = now() \
         WHERE alert_id = $1 AND status = ANY($2)",
    )
    .bind(alert_id)
    .bind(from)
    .bind(to)
    .bind(patch)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Trades whose order is still followed: placed and not filled, or with an unknown outcome.
pub async fn followed(pool: &PgPool) -> anyhow::Result<Vec<AlertTrade>> {
    let sql = format!("SELECT {COLS} FROM alert_trades WHERE status = ANY($1) ORDER BY fired_at");
    Ok(sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql)).bind(FOLLOWED).fetch_all(pool).await?)
}

/// After a restart: a fire caught mid-send may have reached the broker. Never sent again; the
/// row says to check, and its order (if it left) is followed by its client id. The rows moved.
pub async fn sending_to_unknown(pool: &PgPool) -> anyhow::Result<Vec<AlertTrade>> {
    let sql = format!(
        "UPDATE alert_trades SET status = 'unknown', \
           last_result = last_result || jsonb_build_object('message', 'the app stopped while this order was being sent: check it at the broker'), \
           updated_at = now() \
         WHERE status = 'sending' RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, AlertTrade>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

/// Armed trades on one account and instrument, other than `except`: what is already armed
/// there before the user arms one more.
pub async fn armed_on(pool: &PgPool, account_id: Uuid, asset_type: &str, ticker: &str, except: Uuid) -> anyhow::Result<Vec<Armed>> {
    Ok(sqlx::query_as::<_, Armed>(
        "SELECT t.alert_id, a.name AS alert_name, t.intent, t.ticket FROM alert_trades t \
         JOIN histviz_alerts a ON a.id = t.alert_id \
         WHERE t.account_id = $1 AND t.armed_at IS NOT NULL AND a.asset_type = $2 AND a.ticker = $3 \
           AND t.alert_id <> $4 ORDER BY t.armed_at",
    )
    .bind(account_id)
    .bind(asset_type)
    .bind(ticker)
    .bind(except)
    .fetch_all(pool)
    .await?)
}

/// An alert with a trade fires once: set its frequency so. The rows changed (0 or 1).
pub async fn force_once(pool: &PgPool, alert_id: Uuid) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "UPDATE histviz_alerts SET frequency = 'once', cooldown_secs = 0, session_id = NULL, updated_at = now() \
         WHERE id = $1 AND frequency <> 'once'",
    )
    .bind(alert_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
