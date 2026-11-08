//! Paper sessions executing on a broker account: the instruments each one owns on its account,
//! the orders it decided to send and the fills the broker reported for them (migration 0169).
//!
//! The session row carries the target (`execution`), whether it is armed, and what the last
//! sync found ([`crate::paper::Session`]). Nothing here decides an order: the engine decides,
//! `otw-core`'s `paper_exec` converges, and this keeps the record that makes a decision sent
//! once, across restarts. Quantities and prices cross this module as [`Decimal`] and are
//! stored as `NUMERIC`; JSON carries them as strings, never as floats.

use anyhow::Context;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

// ── Instruments: one account, one instrument, one session ────────────────────

/// A session already executing one of these symbols on the account: `(symbol, session name)`.
pub async fn taken(pool: &PgPool, account_id: Uuid, symbols: &[String], except: Option<Uuid>) -> anyhow::Result<Vec<(String, String)>> {
    let upper: Vec<String> = symbols.iter().map(|s| s.to_uppercase()).collect();
    Ok(sqlx::query_as(
        "SELECT i.symbol, s.name FROM paper_exec_instruments i JOIN paper_sessions s ON s.id = i.session_id \
         WHERE i.account_id = $1 AND i.symbol = ANY($2) AND ($3::uuid IS NULL OR i.session_id <> $3) ORDER BY i.symbol",
    )
    .bind(account_id)
    .bind(&upper)
    .bind(except)
    .fetch_all(pool)
    .await?)
}

/// Set a session's execution target and claim its instruments on the account, all or none.
/// `Ok(Some(symbol))` names a symbol another session holds: nothing is written.
pub async fn attach(pool: &PgPool, session_id: Uuid, account_id: Uuid, execution: &Value, symbols: &[String]) -> anyhow::Result<Option<String>> {
    let mut tx = pool.begin().await?;
    for s in symbols {
        let inserted = sqlx::query(
            "INSERT INTO paper_exec_instruments (account_id, symbol, session_id) VALUES ($1, $2, $3) \
             ON CONFLICT (account_id, symbol) DO NOTHING",
        )
        .bind(account_id)
        .bind(s.to_uppercase())
        .bind(session_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            tx.rollback().await?;
            return Ok(Some(s.clone()));
        }
    }
    sqlx::query("UPDATE paper_sessions SET execution = $2, exec_account_id = $3, updated_at = now() WHERE id = $1")
        .bind(session_id)
        .bind(execution)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .context("setting the session's execution")?;
    tx.commit().await?;
    Ok(None)
}

// ── Arming ───────────────────────────────────────────────────────────────────

/// Arm a session: it may send from now on. `baseline` is what the account held of each
/// instrument that the strategy's own fills do not explain, as read at this moment.
pub async fn arm(pool: &PgPool, id: Uuid, by: Uuid, baseline: &Value) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "UPDATE paper_sessions SET armed_at = now(), armed_by = $2, exec_baseline = $3, exec_state = 'synced', \
           exec_note = '', updated_at = now() \
         WHERE id = $1 AND execution IS NOT NULL",
    )
    .bind(id)
    .bind(by)
    .bind(baseline)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// Disarm one session. True when it was armed.
pub async fn disarm(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "UPDATE paper_sessions SET armed_at = NULL, armed_by = NULL, exec_state = 'idle', updated_at = now() \
         WHERE id = $1 AND armed_at IS NOT NULL",
    )
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// Disarm every session armed on one account, or on every account (`None`). Answers the
/// sessions it disarmed.
pub async fn disarm_where(pool: &PgPool, account_id: Option<Uuid>) -> anyhow::Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "UPDATE paper_sessions SET armed_at = NULL, armed_by = NULL, exec_state = 'idle', updated_at = now() \
         WHERE armed_at IS NOT NULL AND ($1::uuid IS NULL OR exec_account_id = $1) RETURNING id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}

/// Re-measure what the account held outside the strategy (the user adopted a drift).
pub async fn set_baseline(pool: &PgPool, id: Uuid, baseline: &Value) -> anyhow::Result<()> {
    sqlx::query("UPDATE paper_sessions SET exec_baseline = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(baseline)
        .execute(pool)
        .await?;
    Ok(())
}

/// What a sync found. `book` None keeps the last one (a sync that could not read the account
/// still says so without erasing what was last seen).
pub async fn set_state(pool: &PgPool, id: Uuid, state: &str, note: &str, book: Option<&Value>) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE paper_sessions SET exec_state = CASE WHEN armed_at IS NULL AND $2 IN ('synced', 'idle') THEN 'idle' ELSE $2 END, \
           exec_note = $3, exec_book = COALESCE($4, exec_book), exec_synced_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(state)
    .bind(note)
    .bind(book)
    .execute(pool)
    .await?;
    Ok(())
}

/// Sessions that execute on a broker account, armed or not: the sync reads every one (a
/// disarmed session may still hold a position, and its dashboard must say so), sending only
/// for the armed ones.
pub async fn broker_sessions(pool: &PgPool) -> anyhow::Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM paper_sessions WHERE execution->>'target' = 'broker' ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?)
}

// ── Orders ───────────────────────────────────────────────────────────────────

/// One leg a session decided to send.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Order {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub client_id: String,
    pub intent: String,
    /// `entry` (market, at a close) | `limit` (a resting entry) | `stop` | `target` | `close`
    pub role: String,
    pub ticker: String,
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
    pub limit_price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub engine_qty: Option<f64>,
    pub engine_price: Option<f64>,
    pub position: String,
    /// `sending` | `sent` | `refused` | `unknown`
    pub outcome: String,
    pub reason: Option<String>,
    pub venue_id: Option<String>,
    /// The broker's number its fills are named by, where that is not `venue_id`.
    pub fills_as: Option<String>,
    /// Where the broker last said it stands (`working`, `filled`, `cancelled`, `rejected`,
    /// `expired`), `None` until it was first looked up.
    pub status: Option<String>,
    pub filled_qty: Decimal,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const ORDER_COLS: &str = "session_id, account_id, client_id, intent, role, ticker, symbol, side, qty, limit_price, \
     stop_price, engine_qty, engine_price, position, outcome, reason, venue_id, fills_as, status, filled_qty, \
     created_at, updated_at";

/// A leg as written before it leaves.
#[derive(Debug, Clone)]
pub struct NewOrder {
    pub client_id: String,
    pub intent: String,
    pub role: String,
    pub ticker: String,
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
    pub limit_price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub engine_qty: Option<f64>,
    pub engine_price: Option<f64>,
    pub position: String,
}

/// Write a ticket's legs `sending`, all or none. False when one of its intents (or client
/// ids) was already written: that decision was taken before, and nothing may leave again.
pub async fn claim(pool: &PgPool, session_id: Uuid, account_id: Uuid, legs: &[NewOrder]) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    for l in legs {
        let n = sqlx::query(
            "INSERT INTO paper_exec_orders (session_id, account_id, client_id, intent, role, ticker, symbol, side, qty, \
               limit_price, stop_price, engine_qty, engine_price, position, outcome) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'sending') ON CONFLICT DO NOTHING",
        )
        .bind(session_id)
        .bind(account_id)
        .bind(&l.client_id)
        .bind(&l.intent)
        .bind(&l.role)
        .bind(&l.ticker)
        .bind(&l.symbol)
        .bind(&l.side)
        .bind(l.qty)
        .bind(l.limit_price)
        .bind(l.stop_price)
        .bind(l.engine_qty)
        .bind(l.engine_price)
        .bind(&l.position)
        .execute(&mut *tx)
        .await
        .context("recording a strategy order")?
        .rows_affected();
        if n == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
    }
    tx.commit().await?;
    Ok(true)
}

/// What the sender answered for one leg.
pub async fn set_outcome(pool: &PgPool, account_id: Uuid, client_id: &str, outcome: &str, reason: Option<&str>, venue_id: Option<&str>) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE paper_exec_orders SET outcome = $3, reason = $4, venue_id = COALESCE($5, venue_id), updated_at = now() \
         WHERE account_id = $1 AND client_id = $2",
    )
    .bind(account_id)
    .bind(client_id)
    .bind(outcome)
    .bind(reason)
    .bind(venue_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Where the broker says a leg stands, as looked up by its client id.
pub async fn set_status(pool: &PgPool, account_id: Uuid, client_id: &str, status: &str, filled: Decimal, venue_id: Option<&str>, fills_as: Option<&str>) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE paper_exec_orders SET status = $3, filled_qty = $4, venue_id = COALESCE($5, venue_id), \
           fills_as = COALESCE($6, fills_as), outcome = CASE WHEN outcome = 'unknown' THEN 'sent' ELSE outcome END, updated_at = now() \
         WHERE account_id = $1 AND client_id = $2",
    )
    .bind(account_id)
    .bind(client_id)
    .bind(status)
    .bind(filled)
    .bind(venue_id)
    .bind(fills_as)
    .execute(pool)
    .await?;
    Ok(())
}

/// A working leg moved at the broker (a trailing stop ratcheted, a target re-priced).
pub async fn set_prices(pool: &PgPool, account_id: Uuid, client_id: &str, limit: Option<Decimal>, stop: Option<Decimal>, engine_price: Option<f64>) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE paper_exec_orders SET limit_price = COALESCE($3, limit_price), stop_price = COALESCE($4, stop_price), \
           engine_price = COALESCE($5, engine_price), updated_at = now() WHERE account_id = $1 AND client_id = $2",
    )
    .bind(account_id)
    .bind(client_id)
    .bind(limit)
    .bind(stop)
    .bind(engine_price)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every leg of a session, oldest first.
pub async fn orders(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<Order>> {
    Ok(sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(format!(
        "SELECT {ORDER_COLS} FROM paper_exec_orders WHERE session_id = $1 ORDER BY created_at, client_id"
    )))
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

/// Legs left `sending` by a process that stopped mid-call: the sync settles each from
/// `broker_orders` (it left, and is followed there) or marks it refused (it never left).
pub async fn sending(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<Order>> {
    Ok(sqlx::query_as::<_, Order>(sqlx::AssertSqlSafe(format!(
        "SELECT {ORDER_COLS} FROM paper_exec_orders WHERE session_id = $1 AND outcome = 'sending' \
           AND updated_at < now() - interval '2 minutes'"
    )))
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

// ── Fills ────────────────────────────────────────────────────────────────────

/// One fill the broker reported for a session's order.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Fill {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub exec_id: String,
    pub client_id: String,
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_currency: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

/// Record a fill. False when it was already recorded (read twice, kept once).
pub async fn add_fill(pool: &PgPool, f: &Fill) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "INSERT INTO paper_exec_fills (session_id, account_id, exec_id, client_id, symbol, side, qty, price, fee, fee_currency, at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (session_id, exec_id) DO NOTHING",
    )
    .bind(f.session_id)
    .bind(f.account_id)
    .bind(&f.exec_id)
    .bind(&f.client_id)
    .bind(&f.symbol)
    .bind(&f.side)
    .bind(f.qty)
    .bind(f.price)
    .bind(f.fee)
    .bind(&f.fee_currency)
    .bind(f.at)
    .execute(pool)
    .await
    .context("recording a strategy fill")?
    .rows_affected();
    Ok(n > 0)
}

/// Every fill of a session, oldest first.
pub async fn fills(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<Fill>> {
    Ok(sqlx::query_as::<_, Fill>(
        "SELECT session_id, account_id, exec_id, client_id, symbol, side, qty, price, fee, fee_currency, at \
         FROM paper_exec_fills WHERE session_id = $1 ORDER BY at, exec_id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}
