//! Paper trading sessions: a strategy left running forward.
//!
//! A session is a frozen `settings` blob plus the datasets it reads, a recurrence rule and a
//! delivery policy. The engine state is not stored: a tick re-runs the ordinary backtest over
//! the moving window and diffs its trades against [`Trade`] rows, so a paper fill is by
//! construction the fill the backtest would have shown for the same bars.
//!
//! `paper_events` doubles as the notification log and the offline inbox: `notified_at` NULL
//! means nothing was ever sent (engine down, no channel, digest still pending) and `seen_at`
//! NULL means the app has not acknowledged it yet.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{AssertSqlSafe, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

const SESSION_COLS: &str = "id, name, strategy_id, settings, dataset_ids, window_bars, start_ts, \
     feed, kind, timezone, every_minutes, at_hour, at_minute, weekdays, day_of_month, run_at, \
     next_run_at, last_run_at, notify_when, cadence, channel_ids, entry_title_template, \
     entry_template, exit_title_template, exit_template, summary_title_template, \
     summary_template, last_digest_at, seeded_at, \
     notify_open, notify_close, notify_tickers, status, inputs_hash, engine_version, stats, \
     bars, last_error, running, created_at, updated_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub strategy_id: Option<Uuid>,
    pub settings: Value,
    pub dataset_ids: Vec<Uuid>,
    pub window_bars: Option<i32>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub start_ts: Option<OffsetDateTime>,
    /// `hist` (the stored catalog, on a schedule) or `live` (the hub, on each closed bar).
    pub feed: String,
    pub kind: String,
    pub timezone: String,
    pub every_minutes: Option<i32>,
    pub at_hour: Option<i32>,
    pub at_minute: Option<i32>,
    pub weekdays: Option<i16>,
    pub day_of_month: Option<i32>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub run_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_run_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    pub notify_when: String,
    pub cadence: String,
    pub channel_ids: Vec<Uuid>,
    /// One wording per kind of message. An entry knows the entry half of the trade, an
    /// exit the whole of it, and the summary talks about the period, never about a trade.
    pub entry_title_template: String,
    pub entry_template: String,
    pub exit_title_template: String,
    pub exit_template: String,
    /// Sent when several events are grouped, rendered on the period aggregates.
    pub summary_title_template: String,
    pub summary_template: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_digest_at: Option<OffsetDateTime>,
    /// When the session learned its own book. The boundary of "since the beginning".
    #[serde(with = "time::serde::rfc3339::option")]
    pub seeded_at: Option<OffsetDateTime>,
    /// Which side of a round trip is worth a message, and on which instruments (empty = all).
    pub notify_open: bool,
    pub notify_close: bool,
    pub notify_tickers: Vec<String>,
    pub status: String,
    pub inputs_hash: String,
    pub engine_version: i32,
    pub stats: Option<Value>,
    pub bars: i32,
    pub last_error: String,
    pub running: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// What a create/update carries. The recurrence half mirrors `automator::ScheduleInput` so the
/// same resolver validates it; `next_run_at` is computed by the caller from that rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct SessionInput {
    pub name: String,
    #[serde(default)]
    pub strategy_id: Option<Uuid>,
    pub settings: Value,
    #[serde(default)]
    pub dataset_ids: Vec<Uuid>,
    #[serde(default)]
    pub window_bars: Option<i32>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schemars(with = "Option<String>")]
    pub start_ts: Option<OffsetDateTime>,
    #[serde(default)]
    pub feed: String,
    pub kind: String,
    pub timezone: String,
    #[serde(default)]
    pub every_minutes: Option<i32>,
    #[serde(default)]
    pub at_hour: Option<i32>,
    #[serde(default)]
    pub at_minute: Option<i32>,
    #[serde(default)]
    pub weekdays: Option<i16>,
    #[serde(default)]
    pub day_of_month: Option<i32>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schemars(with = "Option<String>")]
    pub run_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub notify_when: String,
    #[serde(default)]
    pub cadence: String,
    #[serde(default)]
    pub channel_ids: Vec<Uuid>,
    #[serde(default)]
    pub entry_title_template: String,
    #[serde(default)]
    pub entry_template: String,
    #[serde(default)]
    pub exit_title_template: String,
    #[serde(default)]
    pub exit_template: String,
    #[serde(default)]
    pub summary_title_template: String,
    #[serde(default)]
    pub summary_template: String,
    #[serde(default = "yes")]
    pub notify_open: bool,
    #[serde(default = "yes")]
    pub notify_close: bool,
    #[serde(default)]
    pub notify_tickers: Vec<String>,
    #[serde(default)]
    pub status: String,
}

fn yes() -> bool {
    true
}

/// One round trip the paper session is holding or has closed. `trade` is the engine's own
/// `Trade`, stored verbatim: the message template reads it, and nothing here re-derives it.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Trade {
    pub session_id: Uuid,
    pub trade_key: String,
    pub ticker: String,
    pub direction: String,
    pub entry_ts: String,
    pub exit_ts: String,
    pub open: bool,
    /// Denormalized from `trade` so a period aggregate is an indexed sum, not a JSONB scan.
    pub pnl: f64,
    pub fees: f64,
    pub trade: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub opened_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub closed_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Event {
    pub id: Uuid,
    pub session_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub kind: String,
    pub payload: Value,
    pub title: String,
    pub message: String,
    /// False = recorded for the app, never pushed to a channel (the session filtered it out).
    pub notify: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub notified_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub seen_at: Option<OffsetDateTime>,
}

// ── Sessions ──────────────────────────────────────────────────────────────────

pub async fn list_sessions(pool: &PgPool) -> anyhow::Result<Vec<Session>> {
    let sql = format!("SELECT {SESSION_COLS} FROM paper_sessions ORDER BY created_at DESC");
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_session(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Session>> {
    let sql = format!("SELECT {SESSION_COLS} FROM paper_sessions WHERE id = $1");
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn create_session(
    pool: &PgPool,
    input: &SessionInput,
    next_run_at: Option<OffsetDateTime>,
) -> anyhow::Result<Session> {
    let sql = format!(
        "INSERT INTO paper_sessions \
         (id, name, strategy_id, settings, dataset_ids, window_bars, start_ts, feed, kind, \
          timezone, every_minutes, at_hour, at_minute, weekdays, day_of_month, run_at, \
          next_run_at, notify_when, cadence, channel_ids, entry_title_template, \
          entry_template, exit_title_template, exit_template, summary_title_template, \
          summary_template, notify_open, notify_close, notify_tickers, status) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21, \
                 $22,$23,$24,$25,$26,$27,$28,$29,$30) \
         RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(&input.name)
        .bind(input.strategy_id)
        .bind(&input.settings)
        .bind(&input.dataset_ids)
        .bind(input.window_bars)
        .bind(input.start_ts)
        .bind(&input.feed)
        .bind(&input.kind)
        .bind(&input.timezone)
        .bind(input.every_minutes)
        .bind(input.at_hour)
        .bind(input.at_minute)
        .bind(input.weekdays)
        .bind(input.day_of_month)
        .bind(input.run_at)
        .bind(next_run_at)
        .bind(&input.notify_when)
        .bind(&input.cadence)
        .bind(&input.channel_ids)
        .bind(&input.entry_title_template)
        .bind(&input.entry_template)
        .bind(&input.exit_title_template)
        .bind(&input.exit_template)
        .bind(&input.summary_title_template)
        .bind(&input.summary_template)
        .bind(input.notify_open)
        .bind(input.notify_close)
        .bind(&input.notify_tickers)
        .bind(&input.status)
        .fetch_one(pool)
        .await
        .context("creating paper session")?)
}

/// Full replace of the editable fields. Runtime state (`stats`, `inputs_hash`, `running`,
/// `last_run_at`) is the worker's and is never touched here.
pub async fn update_session(
    pool: &PgPool,
    id: Uuid,
    input: &SessionInput,
    next_run_at: Option<OffsetDateTime>,
) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE paper_sessions SET \
           name = $2, strategy_id = $3, settings = $4, dataset_ids = $5, window_bars = $6, \
           start_ts = $7, feed = $8, kind = $9, timezone = $10, every_minutes = $11, \
           at_hour = $12, at_minute = $13, weekdays = $14, day_of_month = $15, run_at = $16, \
           next_run_at = $17, notify_when = $18, cadence = $19, channel_ids = $20, \
           entry_title_template = $21, entry_template = $22, exit_title_template = $23, \
           exit_template = $24, summary_title_template = $25, summary_template = $26, \
           notify_open = $27, notify_close = $28, notify_tickers = $29, \
           status = $30, updated_at = now() \
         WHERE id = $1 RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(id)
        .bind(&input.name)
        .bind(input.strategy_id)
        .bind(&input.settings)
        .bind(&input.dataset_ids)
        .bind(input.window_bars)
        .bind(input.start_ts)
        .bind(&input.feed)
        .bind(&input.kind)
        .bind(&input.timezone)
        .bind(input.every_minutes)
        .bind(input.at_hour)
        .bind(input.at_minute)
        .bind(input.weekdays)
        .bind(input.day_of_month)
        .bind(input.run_at)
        .bind(next_run_at)
        .bind(&input.notify_when)
        .bind(&input.cadence)
        .bind(&input.channel_ids)
        .bind(&input.entry_title_template)
        .bind(&input.entry_template)
        .bind(&input.exit_title_template)
        .bind(&input.exit_template)
        .bind(&input.summary_title_template)
        .bind(&input.summary_template)
        .bind(input.notify_open)
        .bind(input.notify_close)
        .bind(&input.notify_tickers)
        .bind(&input.status)
        .fetch_optional(pool)
        .await
        .context("updating paper session")?)
}

pub async fn delete_session(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM paper_sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Pause or resume. Resuming recomputes the next occurrence from the caller's rule.
pub async fn set_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    next_run_at: Option<OffsetDateTime>,
) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE paper_sessions SET status = $2, next_run_at = $3, \
           last_error = CASE WHEN $2 = 'active' THEN '' ELSE last_error END, updated_at = now() \
         WHERE id = $1 RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(id)
        .bind(status)
        .bind(next_run_at)
        .fetch_optional(pool)
        .await?)
}

// ── Worker side ───────────────────────────────────────────────────────────────

/// Claim one session whose occurrence is due, flipping `running`. A due session already
/// running is skipped rather than queued: a two-minute rule over a five-minute run must not
/// build a backlog. `FOR UPDATE SKIP LOCKED` keeps it correct if a second worker ever exists.
pub async fn claim_due(pool: &PgPool, now: OffsetDateTime) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE paper_sessions SET running = true, last_run_at = $1 \
         WHERE id = (SELECT id FROM paper_sessions \
                     WHERE status = 'active' AND feed = 'hist' AND NOT running \
                       AND next_run_at IS NOT NULL AND next_run_at <= $1 \
                     ORDER BY next_run_at LIMIT 1 FOR UPDATE SKIP LOCKED) \
         RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(now)
        .fetch_optional(pool)
        .await?)
}

/// Clear the in-flight flag left behind by a crash. Called once at boot.
pub async fn release_orphans(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("UPDATE paper_sessions SET running = false WHERE running")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Record the outcome of a tick and schedule the next one.
pub async fn finish_tick(
    pool: &PgPool,
    id: Uuid,
    next_run_at: Option<OffsetDateTime>,
    stats: Option<&Value>,
    bars: i32,
    inputs_hash: &str,
    engine_version: i32,
    error: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE paper_sessions SET running = false, next_run_at = $2, \
           stats = COALESCE($3, stats), bars = $4, inputs_hash = $5, engine_version = $6, \
           last_error = $7, status = CASE WHEN $7 = '' THEN status ELSE 'error' END, \
           updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(next_run_at)
    .bind(stats)
    .bind(bars)
    .bind(inputs_hash)
    .bind(engine_version)
    .bind(error)
    .execute(pool)
    .await
    .context("closing paper tick")?;
    Ok(())
}

// ── Trades ────────────────────────────────────────────────────────────────────

pub async fn list_trades(
    pool: &PgPool,
    session_id: Uuid,
    open_only: bool,
) -> anyhow::Result<Vec<Trade>> {
    Ok(sqlx::query_as::<_, Trade>(
        "SELECT session_id, trade_key, ticker, direction, entry_ts, exit_ts, open, pnl, fees, \
                trade, opened_at, closed_at \
         FROM paper_trades WHERE session_id = $1 AND ($2 = false OR open) \
         ORDER BY entry_ts DESC",
    )
    .bind(session_id)
    .bind(open_only)
    .fetch_all(pool)
    .await?)
}

/// Insert or refresh one round trip. Returns true when the row is new, which is what makes it
/// an "opened" event: the diff is the database's answer, not a second bookkeeping pass.
pub async fn upsert_trade(
    pool: &PgPool,
    session_id: Uuid,
    key: &str,
    ticker: &str,
    direction: &str,
    entry_ts: &str,
    exit_ts: &str,
    open: bool,
    pnl: f64,
    fees: f64,
    trade: &Value,
) -> anyhow::Result<bool> {
    let row: (bool,) = sqlx::query_as(
        "INSERT INTO paper_trades \
           (session_id, trade_key, ticker, direction, entry_ts, exit_ts, open, pnl, fees, \
            trade, closed_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, CASE WHEN $7 THEN NULL ELSE now() END) \
         ON CONFLICT (session_id, trade_key) DO UPDATE SET \
           exit_ts = EXCLUDED.exit_ts, open = EXCLUDED.open, pnl = EXCLUDED.pnl, \
           fees = EXCLUDED.fees, trade = EXCLUDED.trade, \
           closed_at = CASE WHEN EXCLUDED.open THEN NULL \
                            ELSE COALESCE(paper_trades.closed_at, now()) END \
         RETURNING (xmax = 0) AS inserted",
    )
    .bind(session_id)
    .bind(key)
    .bind(ticker)
    .bind(direction)
    .bind(entry_ts)
    .bind(exit_ts)
    .bind(open)
    .bind(pnl)
    .bind(fees)
    .bind(trade)
    .fetch_one(pool)
    .await
    .context("upserting paper trade")?;
    Ok(row.0)
}

/// Keys currently held open by the session, so the worker can tell a close from an entry.
pub async fn open_keys(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT trade_key FROM paper_trades WHERE session_id = $1 AND open")
            .bind(session_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Drop the **open** rows the last simulation no longer produces: an entry that fell out of
/// the trailing window cannot be followed any more, so keeping it would show a position
/// nothing is holding.
///
/// A closed round trip is never dropped. It is history: the period aggregates are read from
/// it, and deleting it would make "since the beginning" forget its own past and make a
/// widened window report an old trade as a new fill.
pub async fn prune_open_trades(
    pool: &PgPool,
    session_id: Uuid,
    keep: &[String],
) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "DELETE FROM paper_trades \
         WHERE session_id = $1 AND open AND NOT (trade_key = ANY($2))",
    )
    .bind(session_id)
    .bind(keep)
    .execute(pool)
    .await
    .context("pruning the paper book")?;
    Ok(res.rows_affected())
}

/// Drop the session's trades. Used when the window moved so far that a stored round trip is
/// no longer produced by the engine: a paper book that no longer matches the simulation is
/// worse than an empty one.
///
/// The session goes back to being unseeded: `inputs_hash` and `seeded_at` are cleared with the
/// book. Otherwise the next tick refills it from history and reports every inherited round trip
/// as a fresh fill, which is a burst of alerts about the past.
pub async fn clear_trades(pool: &PgPool, session_id: Uuid) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM paper_trades WHERE session_id = $1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE paper_sessions SET inputs_hash = '', seeded_at = NULL, stats = NULL, bars = 0 \
         WHERE id = $1",
    )
    .bind(session_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Closed round trips over one period. Everything a message can say about "since when".
#[derive(Debug, Clone, Default, Serialize)]
pub struct Agg {
    pub trades: i64,
    pub wins: i64,
    pub losses: i64,
    pub pnl: f64,
    pub fees: f64,
    pub best: f64,
    pub worst: f64,
}

/// Both periods in one round trip: `since` (the last delivered notification) and `total`
/// (the session's own beginning). The wider bound is the WHERE, the narrower one a FILTER,
/// so the index on (session_id, closed_at) is walked once and neither window is a scan of
/// the whole book.
pub async fn aggregates(
    pool: &PgPool,
    session_id: Uuid,
    since: OffsetDateTime,
    total_from: OffsetDateTime,
) -> anyhow::Result<(Agg, Agg)> {
    let row: (i64, i64, i64, f64, f64, f64, f64, i64, i64, i64, f64, f64, f64, f64) =
        sqlx::query_as(
            "SELECT \
               count(*) FILTER (WHERE closed_at >= $2), \
               count(*) FILTER (WHERE closed_at >= $2 AND pnl > 0), \
               count(*) FILTER (WHERE closed_at >= $2 AND pnl < 0), \
               COALESCE(sum(pnl)  FILTER (WHERE closed_at >= $2), 0), \
               COALESCE(sum(fees) FILTER (WHERE closed_at >= $2), 0), \
               COALESCE(max(pnl)  FILTER (WHERE closed_at >= $2), 0), \
               COALESCE(min(pnl)  FILTER (WHERE closed_at >= $2), 0), \
               count(*), \
               count(*) FILTER (WHERE pnl > 0), \
               count(*) FILTER (WHERE pnl < 0), \
               COALESCE(sum(pnl), 0), \
               COALESCE(sum(fees), 0), \
               COALESCE(max(pnl), 0), \
               COALESCE(min(pnl), 0) \
             FROM paper_trades \
             WHERE session_id = $1 AND NOT open AND closed_at >= $3",
        )
        .bind(session_id)
        .bind(since)
        .bind(total_from)
        .fetch_one(pool)
        .await
        .context("aggregating the paper book")?;
    let since = Agg {
        trades: row.0,
        wins: row.1,
        losses: row.2,
        pnl: row.3,
        fees: row.4,
        best: row.5,
        worst: row.6,
    };
    let total = Agg {
        trades: row.7,
        wins: row.8,
        losses: row.9,
        pnl: row.10,
        fees: row.11,
        best: row.12,
        worst: row.13,
    };
    Ok((since, total))
}

/// Stamp the instant the session learned its book. Written once, by the seeding tick.
pub async fn set_seeded_at(pool: &PgPool, id: Uuid, at: OffsetDateTime) -> anyhow::Result<()> {
    sqlx::query("UPDATE paper_sessions SET seeded_at = $2 WHERE id = $1 AND seeded_at IS NULL")
        .bind(id)
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Events ────────────────────────────────────────────────────────────────────

/// `notify` is decided here and stamped on the row: the in-app log keeps every event (it is
/// the trader's record) while the channel only carries what the session asked for.
pub async fn add_event(
    pool: &PgPool,
    session_id: Uuid,
    kind: &str,
    payload: &Value,
    title: &str,
    message: &str,
    notify: bool,
) -> anyhow::Result<Event> {
    Ok(sqlx::query_as::<_, Event>(
        "INSERT INTO paper_events (id, session_id, kind, payload, title, message, notify) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) \
         RETURNING id, session_id, at, kind, payload, title, message, notify, notified_at, \
                   seen_at",
    )
    .bind(Uuid::new_v4())
    .bind(session_id)
    .bind(kind)
    .bind(payload)
    .bind(title)
    .bind(message)
    .bind(notify)
    .fetch_one(pool)
    .await
    .context("recording paper event")?)
}

/// Newest first. `session` narrows to one session, `unseen` to what the app has not shown yet.
pub async fn list_events(
    pool: &PgPool,
    session: Option<Uuid>,
    unseen: bool,
    limit: i64,
) -> anyhow::Result<Vec<Event>> {
    Ok(sqlx::query_as::<_, Event>(
        "SELECT id, session_id, at, kind, payload, title, message, notify, notified_at, seen_at \
         FROM paper_events \
         WHERE ($1::uuid IS NULL OR session_id = $1) AND ($2 = false OR seen_at IS NULL) \
         ORDER BY at DESC LIMIT $3",
    )
    .bind(session)
    .bind(unseen)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Events still waiting to be delivered on a channel, oldest first (a digest reads them in
/// the order they happened).
pub async fn undelivered(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<Event>> {
    Ok(sqlx::query_as::<_, Event>(
        "SELECT id, session_id, at, kind, payload, title, message, notify, notified_at, seen_at \
         FROM paper_events \
         WHERE session_id = $1 AND notify AND notified_at IS NULL ORDER BY at",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

pub async fn mark_notified(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("UPDATE paper_events SET notified_at = now() WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?;
    Ok(())
}

/// Acknowledge in-app. `session` None acknowledges everything.
pub async fn mark_seen(pool: &PgPool, session: Option<Uuid>) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "UPDATE paper_events SET seen_at = now() \
         WHERE seen_at IS NULL AND ($1::uuid IS NULL OR session_id = $1)",
    )
    .bind(session)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Advance the digest watermark once a rollup has been sent.
pub async fn set_digest_mark(pool: &PgPool, id: Uuid, at: OffsetDateTime) -> anyhow::Result<()> {
    sqlx::query("UPDATE paper_sessions SET last_digest_at = $2 WHERE id = $1")
        .bind(id)
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

/// Trim a session's event log to its most recent `keep` rows.
pub async fn trim_events(pool: &PgPool, session_id: Uuid, keep: i64) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM paper_events WHERE session_id = $1 AND id NOT IN \
           (SELECT id FROM paper_events WHERE session_id = $1 ORDER BY at DESC LIMIT $2)",
    )
    .bind(session_id)
    .bind(keep)
    .execute(pool)
    .await?;
    Ok(())
}

/// Counters for the module badge: sessions by status and unseen events.
pub async fn overview(pool: &PgPool) -> anyhow::Result<Value> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM paper_sessions WHERE status = 'active'), \
           (SELECT count(*) FROM paper_sessions WHERE status = 'error'), \
           (SELECT count(*) FROM paper_events WHERE seen_at IS NULL)",
    )
    .fetch_one(pool)
    .await?;
    Ok(json!({ "active": row.0, "errored": row.1, "unseen": row.2 }))
}
