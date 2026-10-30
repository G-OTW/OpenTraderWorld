//! Order legs sent to a broker and where each stands (`broker_orders`), plus the positions
//! each trading account held at its last reconciliation (`broker_positions_seen`).
//!
//! The audit log ([`crate::order_log`]) says what was asked and answered; this says what is
//! still out, so a restart neither forgets an order nor sends one twice. Quantities cross this
//! module as decimal strings and are stored as `NUMERIC`: no float decides what is held.

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Statuses of a leg OTW still follows.
pub const OPEN: &[&str] = &["pending", "unknown", "working"];

/// A leg the broker answered it never received: it waits for the user, who sends it again
/// (`resent`) or dismisses it (`dismissed`).
pub const NOT_RECEIVED: &str = "not_received";

/// One leg as it is written before the call leaves.
pub struct NewLeg<'a> {
    pub client_id: &'a str,
    pub ticker: &'a str,
    pub asset_type: &'a str,
    pub action: &'a str,
    pub parent: Option<&'a str>,
    pub group: Option<&'a str>,
    pub request: &'a serde_json::Value,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Leg {
    pub account_id: Uuid,
    pub client_id: String,
    pub broker: String,
    pub ticker: String,
    pub asset_type: String,
    pub action: String,
    pub venue_id: Option<String>,
    pub parent: Option<String>,
    pub grp: Option<String>,
    pub status: String,
    pub filled_qty: Option<String>,
    pub reason: Option<String>,
    pub not_found: i32,
    pub alerted: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const COLUMNS: &str = "account_id, client_id, broker, ticker, asset_type, action, venue_id, parent, grp, status, \
                       filled_qty::text AS filled_qty, reason, not_found, alerted, created_at, updated_at";

/// Write a ticket's legs `pending`, all or none. `Ok(Some(id))` names a client id this
/// account was already sent: nothing is written, and the ticket must not leave.
pub async fn claim(pool: &PgPool, account_id: Uuid, broker: &str, legs: &[NewLeg<'_>]) -> anyhow::Result<Option<String>> {
    let mut tx = pool.begin().await?;
    for l in legs {
        let done = sqlx::query(
            "INSERT INTO broker_orders (account_id, client_id, broker, ticker, asset_type, action, parent, grp, status, request)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending', $9)
             ON CONFLICT (account_id, client_id) DO NOTHING",
        )
        .bind(account_id)
        .bind(l.client_id)
        .bind(broker)
        .bind(l.ticker)
        .bind(l.asset_type)
        .bind(l.action)
        .bind(l.parent)
        .bind(l.group)
        .bind(l.request)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(Some(l.client_id.to_string()));
        }
    }
    tx.commit().await?;
    Ok(None)
}

/// The broker accepted the leg under `venue_id`.
pub async fn accepted(pool: &PgPool, account_id: Uuid, client_id: &str, venue_id: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE broker_orders SET venue_id = $3, status = 'working', not_found = 0, updated_at = now()
         WHERE account_id = $1 AND client_id = $2",
    )
    .bind(account_id)
    .bind(client_id)
    .bind(venue_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set a leg's status (and why, the broker's id and what filled, when known). A final leg
/// never reopens: a status other than a final one is ignored on a leg already final.
pub async fn set_status(
    pool: &PgPool,
    account_id: Uuid,
    client_id: &str,
    status: &str,
    reason: Option<&str>,
    venue_id: Option<&str>,
    filled_qty: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE broker_orders
         SET status = $3, reason = COALESCE($4, reason), venue_id = COALESCE($5, venue_id),
             filled_qty = COALESCE($6::numeric, filled_qty), not_found = 0, updated_at = now()
         WHERE account_id = $1 AND client_id = $2
           AND (status = ANY($7) OR $3 <> ALL($7))",
    )
    .bind(account_id)
    .bind(client_id)
    .bind(status)
    .bind(reason)
    .bind(venue_id)
    .bind(filled_qty)
    .bind(OPEN)
    .execute(pool)
    .await?;
    Ok(())
}

/// One more "never seen" answer for an open leg; the count after it.
pub async fn not_found(pool: &PgPool, account_id: Uuid, client_id: &str) -> anyhow::Result<i32> {
    Ok(sqlx::query_scalar(
        "UPDATE broker_orders SET not_found = not_found + 1, updated_at = now()
         WHERE account_id = $1 AND client_id = $2 RETURNING not_found",
    )
    .bind(account_id)
    .bind(client_id)
    .fetch_optional(pool)
    .await?
    .unwrap_or(0))
}

/// A leg the broker never received, as it was asked: what the user sends again or dismisses.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Unsent {
    pub client_id: String,
    pub ticker: String,
    pub asset_type: String,
    pub parent: Option<String>,
    pub grp: Option<String>,
    pub request: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// The legs of one account the broker never received, oldest first.
pub async fn not_received(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<Unsent>> {
    Ok(sqlx::query_as::<_, Unsent>(
        "SELECT client_id, ticker, asset_type, parent, grp, request, created_at FROM broker_orders
         WHERE account_id = $1 AND status = $2 ORDER BY created_at, client_id",
    )
    .bind(account_id)
    .bind(NOT_RECEIVED)
    .fetch_all(pool)
    .await?)
}

/// Close legs waiting for the user as `resent` or `dismissed`; only legs still waiting move.
/// The client ids that moved.
pub async fn close_unsent(pool: &PgPool, account_id: Uuid, client_ids: &[String], status: &str) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "UPDATE broker_orders SET status = $3, updated_at = now()
         WHERE account_id = $1 AND client_id = ANY($2) AND status = $4 RETURNING client_id",
    )
    .bind(account_id)
    .bind(client_ids)
    .bind(status)
    .bind(NOT_RECEIVED)
    .fetch_all(pool)
    .await?)
}

/// Mark the user told about this leg; `false` when it already was.
pub async fn mark_alerted(pool: &PgPool, account_id: Uuid, client_id: &str) -> anyhow::Result<bool> {
    Ok(sqlx::query(
        "UPDATE broker_orders SET alerted = TRUE WHERE account_id = $1 AND client_id = $2 AND NOT alerted",
    )
    .bind(account_id)
    .bind(client_id)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// Legs `pending` at start belong to a process that died before the answer: unknown now.
pub async fn pending_to_unknown(pool: &PgPool) -> anyhow::Result<u64> {
    Ok(sqlx::query(
        "UPDATE broker_orders SET status = 'unknown', reason = 'no answer before the app stopped', updated_at = now()
         WHERE status = 'pending'",
    )
    .execute(pool)
    .await?
    .rows_affected())
}

/// The legs of one account OTW still follows, oldest first.
pub async fn open(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<Leg>> {
    Ok(sqlx::query_as::<_, Leg>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM broker_orders WHERE account_id = $1 AND status = ANY($2) ORDER BY created_at, client_id"
    )))
    .bind(account_id)
    .bind(OPEN)
    .fetch_all(pool)
    .await?)
}

/// One leg, by its client id.
pub async fn get(pool: &PgPool, account_id: Uuid, client_id: &str) -> anyhow::Result<Option<Leg>> {
    Ok(sqlx::query_as::<_, Leg>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM broker_orders WHERE account_id = $1 AND client_id = $2"
    )))
    .bind(account_id)
    .bind(client_id)
    .fetch_optional(pool)
    .await?)
}

/// Client ids on this instrument whose outcome is not known: new orders on it wait.
pub async fn unknown_on(pool: &PgPool, account_id: Uuid, ticker: &str) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT client_id FROM broker_orders
         WHERE account_id = $1 AND status = 'unknown' AND upper(ticker) = upper($2)
         ORDER BY created_at",
    )
    .bind(account_id)
    .bind(ticker)
    .fetch_all(pool)
    .await?)
}

/// Accounts with a leg OTW still follows.
pub async fn accounts_open(pool: &PgPool) -> anyhow::Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar("SELECT DISTINCT account_id FROM broker_orders WHERE status = ANY($1)")
        .bind(OPEN)
        .fetch_all(pool)
        .await?)
}

/// One position as last seen: the broker's key for it, its symbol, its signed quantity.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Seen {
    pub position: String,
    pub symbol: String,
    pub qty: String,
    pub seen_at: OffsetDateTime,
}

pub async fn positions_seen(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<Seen>> {
    Ok(sqlx::query_as::<_, Seen>(
        "SELECT position, symbol, qty::text AS qty, seen_at FROM broker_positions_seen WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}

/// Replace what the account was last seen holding (`(position, symbol, signed qty)`).
pub async fn save_positions(pool: &PgPool, account_id: Uuid, rows: &[(String, String, String)]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM broker_positions_seen WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    for (position, symbol, qty) in rows {
        sqlx::query("INSERT INTO broker_positions_seen (account_id, position, symbol, qty) VALUES ($1, $2, $3, $4::numeric)")
            .bind(account_id)
            .bind(position)
            .bind(symbol)
            .bind(qty)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A working order the broker could only change as a new one: the new leg takes over the old
/// one's instrument and links, and the old one is closed as replaced. `false` when no leg works
/// under `old_venue_id` (an order placed elsewhere): nothing to follow.
pub async fn replaced(pool: &PgPool, account_id: Uuid, old_venue_id: &str, new_client_id: &str, new_venue_id: &str) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let old: Option<String> = sqlx::query_scalar(
        "UPDATE broker_orders SET status = 'cancelled', reason = 'replaced by ' || $3, updated_at = now()
         WHERE account_id = $1 AND venue_id = $2 AND status = ANY($4) RETURNING client_id",
    )
    .bind(account_id)
    .bind(old_venue_id)
    .bind(new_client_id)
    .bind(OPEN)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(old) = old else {
        tx.rollback().await?;
        return Ok(false);
    };
    sqlx::query(
        "INSERT INTO broker_orders (account_id, client_id, broker, ticker, asset_type, action, venue_id, parent, grp, status, request)
         SELECT account_id, $3, broker, ticker, asset_type, 'replace', $4, parent, grp, 'working', jsonb_build_object('replaces', client_id)
         FROM broker_orders WHERE account_id = $1 AND client_id = $2
         ON CONFLICT (account_id, client_id) DO UPDATE SET venue_id = EXCLUDED.venue_id, status = 'working', updated_at = now()",
    )
    .bind(account_id)
    .bind(&old)
    .bind(new_client_id)
    .bind(new_venue_id)
    .execute(&mut *tx)
    .await?;
    // Exits hung on a replaced entry hang on its replacement.
    sqlx::query("UPDATE broker_orders SET parent = $3, updated_at = now() WHERE account_id = $1 AND parent = $2")
        .bind(account_id)
        .bind(&old)
        .bind(new_client_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}
