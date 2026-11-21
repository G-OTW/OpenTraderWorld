//! Trading robots (stored as paper sessions): a strategy left running forward.
//!
//! A session is a frozen `settings` blob plus the datasets it reads, a recurrence rule and a
//! delivery policy. The engine state is not stored: a tick re-runs the ordinary backtest over
//! the moving window and diffs its trades against [`Trade`] rows, so a paper fill is by
//! construction the fill the backtest would have shown for the same bars.
//!
//! `robot_events` doubles as the notification log and the offline inbox: `notified_at` NULL
//! means nothing was ever sent (engine down, no channel, digest still pending) and `seen_at`
//! NULL means the app has not acknowledged it yet.

use anyhow::Context;
use rust_decimal::Decimal;
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
     bars, last_error, running, created_at, updated_at, execution, exec_account_id, armed_at, \
     armed_by, exec_state, exec_note, exec_synced_at, exec_book, exec_baseline, priority, capital, \
     guards, guard_state, strategy_version, strategy_name, exclusions, excluded_at, next_report_at, data_connectors";

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
    /// Where the fills happen: NULL simulates (the session as it always was); `{target:
    /// "broker", account_id, symbols, exits, max_position_qty}` executes on a broker account.
    /// Set at creation, never edited.
    pub execution: Option<Value>,
    pub exec_account_id: Option<Uuid>,
    /// Armed: the session may send orders. Cleared by every disarm.
    #[serde(with = "time::serde::rfc3339::option")]
    pub armed_at: Option<OffsetDateTime>,
    pub armed_by: Option<Uuid>,
    /// What the last sync found: idle, synced, degraded, paused, blocked or error.
    pub exec_state: String,
    pub exec_note: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub exec_synced_at: Option<OffsetDateTime>,
    /// The intended and the broker book as the last sync read them.
    pub exec_book: Option<Value>,
    /// What the account held of each instrument, outside the strategy, when it was armed.
    pub exec_baseline: Value,
    /// Lower runs first when several robots are due at once or compete for one venue.
    pub priority: i32,
    /// `{ allocation, currency, percent_in, max_positions, max_orders_per_day }`; `{}` = none.
    pub capital: Value,
    /// The guard rules, `[{ when, then }]`.
    pub guards: Value,
    /// What the guards hold now (pause, halt, scale), see `otw-core`'s `robots::guards`.
    pub guard_state: Value,
    /// The saved strategy's version and name when the robot froze it.
    pub strategy_version: Option<i32>,
    pub strategy_name: String,
    /// `{ weekdays, hours: [{from, to}], dates: [{from, to}], leave_market }`, in `timezone`.
    pub exclusions: Value,
    /// When the robot entered the exclusion it is in now; NULL outside one.
    #[serde(with = "time::serde::rfc3339::option")]
    pub excluded_at: Option<OffsetDateTime>,
    /// The status report's next occurrence (`cadence = scheduled`).
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_report_at: Option<OffsetDateTime>,
    /// The data source of each instrument, `{ dataset id: connector id }`, as picked.
    pub data_connectors: Value,
}

impl Session {
    /// The connector this robot reads `dataset_id` through, when one was picked.
    pub fn data_connector_of(&self, dataset_id: Uuid) -> Option<Uuid> {
        self.data_connectors.get(dataset_id.to_string()).and_then(Value::as_str).and_then(|s| s.parse().ok())
    }

    /// Whether the session executes on a broker account rather than simulating.
    pub fn on_broker(&self) -> bool {
        self.execution.as_ref().and_then(|e| e.get("target")).and_then(Value::as_str) == Some("broker")
    }
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
    /// Where the fills happen, read at creation only: absent or `{target: "simulate"}` keeps
    /// the session simulated; `{target: "broker", account_id, exits?, max_position_qty?}`
    /// executes it on a broker account (refused to an agent or a workflow).
    #[serde(default)]
    #[schemars(with = "Option<serde_json::Map<String, Value>>")]
    pub execution: Option<Value>,
    /// Lower runs first (default 100).
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// `{ allocation, currency, percent_in, max_positions, max_orders_per_day }`, all optional.
    #[serde(default = "empty_object")]
    #[schemars(with = "serde_json::Map<String, Value>")]
    pub capital: Value,
    /// Guard rules `[{ when: { kind, ... }, then: { kind, ... } }]`.
    #[serde(default = "empty_array")]
    #[schemars(with = "Vec<serde_json::Map<String, Value>>")]
    pub guards: Value,
    /// The saved strategy's version when it was frozen (set by the server from `strategy_id`).
    #[serde(default)]
    pub strategy_version: Option<i32>,
    #[serde(default)]
    pub strategy_name: String,
    /// Periods with no new entry: `{ weekdays, hours: [{from, to}], dates: [{from, to}],
    /// leave_market }`, in the robot's timezone.
    #[serde(default = "empty_object")]
    #[schemars(with = "serde_json::Map<String, Value>")]
    pub exclusions: Value,
    /// The data source of each instrument, `{ dataset id: connector id }`. An instrument
    /// left out is read through its provider's default source.
    #[serde(default = "empty_object")]
    #[schemars(with = "serde_json::Map<String, Value>")]
    pub data_connectors: Value,
}

fn yes() -> bool {
    true
}

fn default_priority() -> i32 {
    100
}

fn empty_object() -> Value {
    json!({})
}

fn empty_array() -> Value {
    json!([])
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
    /// The size the robot took: the engine's times the guard scale in force at entry, cut by
    /// the capital policy. NULL on a row written before robots (the engine's own size); 0 when
    /// the robot did not take it (`robot_note` says why).
    pub robot_qty: Option<Decimal>,
    pub scale: Decimal,
    pub robot_note: String,
    /// The robot's P&L and fees on a closed trade, at its own size, in the instrument currency's
    /// precision: the engine's f64 crosses into Decimal once, where its trade became the
    /// robot's. NULL while the trade is open or not settled yet.
    pub robot_pnl: Option<Decimal>,
    pub robot_fees: Option<Decimal>,
    /// A broker robot's closed trade priced from its own fills at the venue, fees included.
    pub money_at_broker: bool,
}

impl Trade {
    /// The engine's size on this trade.
    pub fn engine_qty(&self) -> f64 {
        self.trade.get("qty").and_then(Value::as_f64).unwrap_or(0.0).abs()
    }

    /// The share of the engine's trade the robot holds: 1 when it took it whole, 0 when it
    /// did not take it, 1 when no robot size was recorded (a row from before robots).
    pub fn robot_ratio(&self) -> Decimal {
        let Some(q) = self.robot_qty else { return Decimal::ONE };
        match crate::paper_broker::decimal_of(self.engine_qty()) {
            Ok(engine) if engine > Decimal::ZERO => q / engine,
            _ => Decimal::ZERO,
        }
    }

    /// The robot's P&L at its own size, `dp` decimals: the engine's figure converted once.
    /// What [`settle`](set_robot_money) stores; the stored value wins when there is one.
    pub fn money_at(&self, engine: f64, dp: u32) -> Decimal {
        let v = crate::paper_broker::decimal_of(engine).unwrap_or_default();
        (v * self.robot_ratio()).round_dp(dp).normalize()
    }

    /// The robot's P&L on this trade, as stored when it closed (else derived at 8 decimals).
    pub fn robot_pnl(&self) -> Decimal {
        self.robot_pnl.unwrap_or_else(|| self.money_at(self.pnl, 8))
    }

    pub fn robot_fees(&self) -> Decimal {
        self.robot_fees.unwrap_or_else(|| self.money_at(self.fees, 8))
    }
}

/// The robot's size and scale are read without trailing zeros: 0.5, never 0.50.
const TRADE_COLS: &str = "session_id, trade_key, ticker, direction, entry_ts, exit_ts, open, pnl, fees, \
     trade, opened_at, closed_at, trim_scale(robot_qty) AS robot_qty, trim_scale(scale) AS scale, robot_note, \
     trim_scale(robot_pnl) AS robot_pnl, trim_scale(robot_fees) AS robot_fees, money_at_broker";

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
    let sql = format!("SELECT {SESSION_COLS} FROM robots ORDER BY created_at DESC");
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_session(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Session>> {
    let sql = format!("SELECT {SESSION_COLS} FROM robots WHERE id = $1");
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn create_session(
    pool: &PgPool,
    input: &SessionInput,
    next_run_at: Option<OffsetDateTime>,
    next_report_at: Option<OffsetDateTime>,
) -> anyhow::Result<Session> {
    let sql = format!(
        "INSERT INTO robots \
         (id, name, strategy_id, settings, dataset_ids, window_bars, start_ts, feed, kind, \
          timezone, every_minutes, at_hour, at_minute, weekdays, day_of_month, run_at, \
          next_run_at, notify_when, cadence, channel_ids, entry_title_template, \
          entry_template, exit_title_template, exit_template, summary_title_template, \
          summary_template, notify_open, notify_close, notify_tickers, status, priority, capital, \
          guards, strategy_version, strategy_name, exclusions, next_report_at, data_connectors) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21, \
                 $22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38) \
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
        .bind(input.priority)
        .bind(&input.capital)
        .bind(&input.guards)
        .bind(input.strategy_version)
        .bind(&input.strategy_name)
        .bind(&input.exclusions)
        .bind(next_report_at)
        .bind(&input.data_connectors)
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
    next_report_at: Option<OffsetDateTime>,
) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE robots SET \
           name = $2, strategy_id = $3, settings = $4, dataset_ids = $5, window_bars = $6, \
           start_ts = $7, feed = $8, kind = $9, timezone = $10, every_minutes = $11, \
           at_hour = $12, at_minute = $13, weekdays = $14, day_of_month = $15, run_at = $16, \
           next_run_at = $17, notify_when = $18, cadence = $19, channel_ids = $20, \
           entry_title_template = $21, entry_template = $22, exit_title_template = $23, \
           exit_template = $24, summary_title_template = $25, summary_template = $26, \
           notify_open = $27, notify_close = $28, notify_tickers = $29, \
           status = $30, priority = $31, capital = $32, guards = $33, \
           strategy_version = COALESCE($34, strategy_version), \
           strategy_name = CASE WHEN $35 = '' THEN strategy_name ELSE $35 END, \
           exclusions = $36, next_report_at = $37, data_connectors = $38, updated_at = now() \
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
        .bind(input.priority)
        .bind(&input.capital)
        .bind(&input.guards)
        .bind(input.strategy_version)
        .bind(&input.strategy_name)
        .bind(&input.exclusions)
        .bind(next_report_at)
        .bind(&input.data_connectors)
        .fetch_optional(pool)
        .await
        .context("updating paper session")?)
}

pub async fn delete_session(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM robots WHERE id = $1")
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
    next_report_at: Option<OffsetDateTime>,
) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE robots SET status = $2, next_run_at = $3, next_report_at = $4, \
           last_error = CASE WHEN $2 = 'active' THEN '' ELSE last_error END, updated_at = now() \
         WHERE id = $1 RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(id)
        .bind(status)
        .bind(next_run_at)
        .bind(next_report_at)
        .fetch_optional(pool)
        .await?)
}

// ── Worker side ───────────────────────────────────────────────────────────────

/// A manual evaluation claims the same running flag and timestamp as a scheduled one.
/// This also prevents a dashboard click from racing an evaluation already in flight.
pub async fn claim_manual(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE robots SET running = true, last_run_at = now() \
         WHERE id = $1 AND NOT running RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Claim one session whose occurrence is due, flipping `running`. A due session already
/// running is skipped rather than queued: a two-minute rule over a five-minute run must not
/// build a backlog. `FOR UPDATE SKIP LOCKED` keeps it correct if a second worker ever exists.
pub async fn claim_due(pool: &PgPool, now: OffsetDateTime) -> anyhow::Result<Option<Session>> {
    let sql = format!(
        "UPDATE robots SET running = true, last_run_at = $1 \
         WHERE id = (SELECT id FROM robots \
                     WHERE status = 'active' AND feed = 'hist' AND NOT running \
                       AND next_run_at IS NOT NULL AND next_run_at <= $1 \
                     ORDER BY priority, next_run_at LIMIT 1 FOR UPDATE SKIP LOCKED) \
         RETURNING {SESSION_COLS}"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql))
        .bind(now)
        .fetch_optional(pool)
        .await?)
}

/// Clear the in-flight flag left behind by a crash. Called once at boot.
pub async fn release_orphans(pool: &PgPool) -> anyhow::Result<u64> {
    let res = sqlx::query("UPDATE robots SET running = false WHERE running")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// The soonest tick planned, so the worker sleeps until that instant rather than polling.
pub async fn next_due_at(pool: &PgPool) -> anyhow::Result<Option<OffsetDateTime>> {
    let row: (Option<OffsetDateTime>,) = sqlx::query_as(
        "SELECT min(next_run_at) FROM robots WHERE status = 'active' AND feed = 'hist' AND NOT running",
    )
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Robots whose status report is due.
pub async fn due_reports(pool: &PgPool, now: OffsetDateTime) -> anyhow::Result<Vec<Session>> {
    let sql = format!(
        "SELECT {SESSION_COLS} FROM robots \
         WHERE status = 'active' AND next_report_at IS NOT NULL AND next_report_at <= $1 \
         ORDER BY priority, next_report_at"
    );
    Ok(sqlx::query_as::<_, Session>(AssertSqlSafe(sql)).bind(now).fetch_all(pool).await?)
}

pub async fn set_next_report(pool: &PgPool, id: Uuid, at: Option<OffsetDateTime>) -> anyhow::Result<()> {
    sqlx::query("UPDATE robots SET next_report_at = $2 WHERE id = $1").bind(id).bind(at).execute(pool).await?;
    Ok(())
}

/// Enter (`Some`) or leave (`None`) an exclusion.
pub async fn set_excluded(pool: &PgPool, id: Uuid, at: Option<OffsetDateTime>) -> anyhow::Result<()> {
    sqlx::query("UPDATE robots SET excluded_at = $2 WHERE id = $1").bind(id).bind(at).execute(pool).await?;
    Ok(())
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
        "UPDATE robots SET running = false, next_run_at = $2, \
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

/// Every trade key the robot has recorded, open or closed.
pub async fn trade_keys(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT trade_key FROM robot_trades WHERE session_id = $1")
        .bind(session_id)
        .fetch_all(pool)
        .await?)
}

/// The positions the robot holds: open rows it took, pending entries aside.
pub async fn held(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<Trade>> {
    Ok(sqlx::query_as::<_, Trade>(AssertSqlSafe(format!(
        "SELECT {TRADE_COLS} FROM robot_trades WHERE session_id = $1 AND open \
           AND COALESCE(robot_qty, 1) <> 0 AND trade_key NOT LIKE '%|pending:%' ORDER BY entry_ts"
    )))
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

pub async fn list_trades(
    pool: &PgPool,
    session_id: Uuid,
    open_only: bool,
) -> anyhow::Result<Vec<Trade>> {
    Ok(sqlx::query_as::<_, Trade>(AssertSqlSafe(format!(
        "SELECT {TRADE_COLS} FROM robot_trades WHERE session_id = $1 AND ($2 = false OR open) \
         ORDER BY entry_ts DESC"
    )))
    .bind(session_id)
    .bind(open_only)
    .fetch_all(pool)
    .await?)
}

/// Insert or refresh one round trip. Returns true when the row is new, which is what makes it
/// an "opened" event: the diff is the database's answer, not a second bookkeeping pass.
/// `sim_entry` and `sim_exit` keep the engine's prices as they were when the position opened
/// and closed: a later run, or the venue's fills merged in, never moves them.
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
        "INSERT INTO robot_trades \
           (session_id, trade_key, ticker, direction, entry_ts, exit_ts, open, pnl, fees, \
            trade, closed_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, \
                 $10 || jsonb_strip_nulls(jsonb_build_object('sim_entry', $10->'entry_price', \
                          'sim_exit', CASE WHEN $7 THEN NULL ELSE $10->'exit_price' END)), \
                 CASE WHEN $7 THEN NULL ELSE now() END) \
         ON CONFLICT (session_id, trade_key) DO UPDATE SET \
           exit_ts = CASE WHEN NOT EXCLUDED.open AND robot_trades.trade ? 'broker_exit' \
                          THEN COALESCE(robot_trades.trade->'broker_exit'->>'exit_ts', EXCLUDED.exit_ts) \
                          ELSE EXCLUDED.exit_ts END, \
           open = EXCLUDED.open, pnl = EXCLUDED.pnl, fees = EXCLUDED.fees, \
           trade = CASE WHEN NOT EXCLUDED.open AND robot_trades.trade ? 'broker_exit' \
                        THEN EXCLUDED.trade || (robot_trades.trade->'broker_exit') \
                             || jsonb_build_object('broker_exit', robot_trades.trade->'broker_exit') \
                        ELSE EXCLUDED.trade END \
                   || jsonb_strip_nulls(jsonb_build_object( \
                        'sim_entry', COALESCE(robot_trades.trade->'sim_entry', EXCLUDED.trade->'sim_entry'), \
                        'sim_exit', COALESCE(robot_trades.trade->'sim_exit', EXCLUDED.trade->'sim_exit'))), \
           closed_at = CASE WHEN EXCLUDED.open THEN NULL \
                            ELSE COALESCE(robot_trades.closed_at, now()) END, \
           robot_pnl = CASE WHEN EXCLUDED.open THEN NULL \
                            WHEN robot_trades.money_at_broker THEN robot_trades.robot_pnl \
                            WHEN robot_trades.pnl IS DISTINCT FROM EXCLUDED.pnl \
                              OR robot_trades.fees IS DISTINCT FROM EXCLUDED.fees THEN NULL \
                            ELSE robot_trades.robot_pnl END, \
           robot_fees = CASE WHEN EXCLUDED.open THEN NULL \
                            WHEN robot_trades.money_at_broker THEN robot_trades.robot_fees \
                            WHEN robot_trades.pnl IS DISTINCT FROM EXCLUDED.pnl \
                              OR robot_trades.fees IS DISTINCT FROM EXCLUDED.fees THEN NULL \
                            ELSE robot_trades.robot_fees END, \
           money_at_broker = robot_trades.money_at_broker AND NOT EXCLUDED.open \
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
        sqlx::query_as("SELECT trade_key FROM robot_trades WHERE session_id = $1 AND open")
            .bind(session_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Drop the **open** rows the last simulation no longer produces and the robot did not take.
/// A position the robot took is its own, kept until it exits it or the robot is deleted: the
/// window losing sight of it changes nothing about what was executed. A market entry announced
/// at a close (`...|pending:<signal>`, never sized) goes once the run that follows has it as
/// the filled trade, or no longer has it at all.
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
        "DELETE FROM robot_trades \
         WHERE session_id = $1 AND open AND NOT (trade_key = ANY($2)) \
           AND (COALESCE(robot_qty, 1) = 0 OR (robot_qty IS NULL AND trade_key LIKE '%|pending:%'))",
    )
    .bind(session_id)
    .bind(keep)
    .execute(pool)
    .await
    .context("pruning the paper book")?;
    Ok(res.rows_affected())
}

/// Drop the rows of trades a hole in the data cut through: their exit is unknown, so they
/// leave the book until a fill gives them a real one.
pub async fn drop_trades(pool: &PgPool, session_id: Uuid, keys: &[String]) -> anyhow::Result<u64> {
    if keys.is_empty() {
        return Ok(0);
    }
    // A trade the robot took is kept: it was executed, whatever the data says since.
    let res = sqlx::query(
        "DELETE FROM robot_trades WHERE session_id = $1 AND trade_key = ANY($2) AND COALESCE(robot_qty, 1) = 0",
    )
    .bind(session_id)
    .bind(keys)
    .execute(pool)
    .await
    .context("dropping excluded trades")?;
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
    sqlx::query("DELETE FROM robot_trades WHERE session_id = $1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM robot_live_fills WHERE session_id = $1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE robots SET inputs_hash = '', seeded_at = NULL, stats = NULL, bars = 0 \
         WHERE id = $1",
    )
    .bind(session_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// A trade's P&L as the robot holds it: the engine's, scaled to the size the robot took (a
/// row written before robots has no robot size and counts whole).
const RPNL: &str = "COALESCE(robot_pnl::float8, pnl * COALESCE(robot_qty::float8 / NULLIF(abs((trade->>'qty')::float8), 0), 1))";
const RFEES: &str = "COALESCE(robot_fees::float8, fees * COALESCE(robot_qty::float8 / NULLIF(abs((trade->>'qty')::float8), 0), 1))";
/// A trade the robot took (one it refused has `robot_qty` 0).
const TAKEN: &str = "COALESCE(robot_qty, 1) <> 0";

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
        sqlx::query_as(AssertSqlSafe(format!(
            "SELECT \
               count(*) FILTER (WHERE closed_at >= $2), \
               count(*) FILTER (WHERE closed_at >= $2 AND {RPNL} > 0), \
               count(*) FILTER (WHERE closed_at >= $2 AND {RPNL} < 0), \
               COALESCE(sum({RPNL})  FILTER (WHERE closed_at >= $2), 0), \
               COALESCE(sum({RFEES}) FILTER (WHERE closed_at >= $2), 0), \
               COALESCE(max({RPNL})  FILTER (WHERE closed_at >= $2), 0), \
               COALESCE(min({RPNL})  FILTER (WHERE closed_at >= $2), 0), \
               count(*), \
               count(*) FILTER (WHERE {RPNL} > 0), \
               count(*) FILTER (WHERE {RPNL} < 0), \
               COALESCE(sum({RPNL}), 0), \
               COALESCE(sum({RFEES}), 0), \
               COALESCE(max({RPNL}), 0), \
               COALESCE(min({RPNL}), 0) \
             FROM robot_trades \
             WHERE session_id = $1 AND NOT open AND closed_at >= $3 AND {TAKEN}"
        )))
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
    sqlx::query("UPDATE robots SET seeded_at = $2 WHERE id = $1 AND seeded_at IS NULL")
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
        "INSERT INTO robot_events (id, session_id, kind, payload, title, message, notify) \
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
         FROM robot_events \
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
         FROM robot_events \
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
    sqlx::query("UPDATE robot_events SET notified_at = now() WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?;
    Ok(())
}

/// Acknowledge in-app. `session` None acknowledges everything.
pub async fn mark_seen(pool: &PgPool, session: Option<Uuid>) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "UPDATE robot_events SET seen_at = now() \
         WHERE seen_at IS NULL AND ($1::uuid IS NULL OR session_id = $1)",
    )
    .bind(session)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Advance the digest watermark once a rollup has been sent.
pub async fn set_digest_mark(pool: &PgPool, id: Uuid, at: OffsetDateTime) -> anyhow::Result<()> {
    sqlx::query("UPDATE robots SET last_digest_at = $2 WHERE id = $1")
        .bind(id)
        .bind(at)
        .execute(pool)
        .await?;
    Ok(())
}

/// Trim a session's event log to its most recent `keep` rows.
pub async fn trim_events(pool: &PgPool, session_id: Uuid, keep: i64) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM robot_events WHERE session_id = $1 AND id NOT IN \
           (SELECT id FROM robot_events WHERE session_id = $1 ORDER BY at DESC LIMIT $2)",
    )
    .bind(session_id)
    .bind(keep)
    .execute(pool)
    .await?;
    Ok(())
}

/// Unseen events per robot, for the fleet line. Robots with none are absent.
pub async fn unseen_by_session(pool: &PgPool) -> anyhow::Result<Vec<(Uuid, i64)>> {
    Ok(sqlx::query_as(
        "SELECT session_id, count(*) FROM robot_events WHERE seen_at IS NULL GROUP BY session_id",
    )
    .fetch_all(pool)
    .await?)
}

/// Counters for the module badge: sessions by status and unseen events.
pub async fn overview(pool: &PgPool) -> anyhow::Result<Value> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM robots WHERE status = 'active'), \
           (SELECT count(*) FROM robots WHERE status = 'error'), \
           (SELECT count(*) FROM robot_events WHERE seen_at IS NULL)",
    )
    .fetch_one(pool)
    .await?;
    Ok(json!({ "active": row.0, "errored": row.1, "unseen": row.2 }))
}

// ── Live fills ────────────────────────────────────────────────────────────────

/// A fill taken on the live price between two runs.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LiveFill {
    pub id: Uuid,
    pub session_id: Uuid,
    pub trade_key: String,
    pub ticker: String,
    /// `entry` | `exit`
    pub kind: String,
    pub direction: String,
    pub reason: String,
    pub bar_ts: String,
    pub price: f64,
    pub maker: bool,
    pub quoted: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

/// Record a live fill. False when that trade already has one of this kind (a second trigger
/// on the same position is not a second fill).
pub async fn add_live_fill(pool: &PgPool, f: &LiveFill) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "INSERT INTO robot_live_fills \
           (id, session_id, trade_key, ticker, kind, direction, reason, bar_ts, price, maker, quoted, at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (session_id, trade_key, kind) DO NOTHING",
    )
    .bind(f.id)
    .bind(f.session_id)
    .bind(&f.trade_key)
    .bind(&f.ticker)
    .bind(&f.kind)
    .bind(&f.direction)
    .bind(&f.reason)
    .bind(&f.bar_ts)
    .bind(f.price)
    .bind(f.maker)
    .bind(f.quoted)
    .bind(f.at)
    .execute(pool)
    .await
    .context("recording a live fill")?;
    Ok(res.rows_affected() > 0)
}

pub async fn list_live_fills(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<LiveFill>> {
    Ok(sqlx::query_as::<_, LiveFill>(
        "SELECT id, session_id, trade_key, ticker, kind, direction, reason, bar_ts, price, maker, \
                quoted, at \
         FROM robot_live_fills WHERE session_id = $1 ORDER BY at",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

// ── Robot layers: size taken, guards, priority ────────────────────────────────

/// Record the size the robot took on a trade it just met. Written once: a trade's robot size
/// is decided at entry and never moved by a later tick.
pub async fn set_trade_size(
    pool: &PgPool,
    session_id: Uuid,
    key: &str,
    robot_qty: Decimal,
    scale: Decimal,
    note: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE robot_trades SET robot_qty = $3, scale = $4, robot_note = $5 \
         WHERE session_id = $1 AND trade_key = $2 AND robot_qty IS NULL",
    )
    .bind(session_id)
    .bind(key)
    .bind(robot_qty.normalize())
    .bind(scale.normalize())
    .bind(note)
    .execute(pool)
    .await
    .context("recording the bot's size")?;
    Ok(())
}

/// A trade the robot sized at its tick but whose opening order was refused (by the broker or
/// by the robot's own caps): not taken after all. Its size goes to 0 with the reason and its
/// money with it (0 once closed). Answers when it closed, `None` when it is open or was
/// already not taken.
pub async fn untake_trade(pool: &PgPool, session_id: Uuid, key: &str, note: &str) -> anyhow::Result<Option<OffsetDateTime>> {
    let closed: Option<Option<OffsetDateTime>> = sqlx::query_scalar(
        "UPDATE robot_trades SET robot_qty = 0, robot_note = $3, money_at_broker = false, \
           robot_pnl = CASE WHEN open THEN NULL ELSE 0 END, robot_fees = CASE WHEN open THEN NULL ELSE 0 END \
         WHERE session_id = $1 AND trade_key = $2 AND robot_qty IS NOT NULL AND robot_qty <> 0 \
         RETURNING closed_at",
    )
    .bind(session_id)
    .bind(key)
    .bind(note)
    .fetch_optional(pool)
    .await
    .context("writing a refused trade as not taken")?;
    Ok(closed.flatten())
}

/// The keys of `keys` the robot took (size above zero, or not sized yet).
pub async fn taken_of(pool: &PgPool, session_id: Uuid, keys: &[String]) -> anyhow::Result<Vec<String>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_scalar(
        "SELECT trade_key FROM robot_trades WHERE session_id = $1 AND trade_key = ANY($2) \
           AND COALESCE(robot_qty, 1) <> 0",
    )
    .bind(session_id)
    .bind(keys)
    .fetch_all(pool)
    .await?)
}

/// Open trades the robot holds (taken, size above zero).
pub async fn open_taken(pool: &PgPool, session_id: Uuid) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM robot_trades WHERE session_id = $1 AND open \
           AND COALESCE(robot_qty, 1) <> 0 AND trade_key NOT LIKE '%|pending:%'",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?)
}

/// What the guards hold now, written after every evaluation.
pub async fn set_guard_state(pool: &PgPool, id: Uuid, state: &Value) -> anyhow::Result<()> {
    sqlx::query("UPDATE robots SET guard_state = $2 WHERE id = $1")
        .bind(id)
        .bind(state)
        .execute(pool)
        .await?;
    Ok(())
}

/// Re-freeze the robot on its strategy's current version: new settings, the version and name
/// it now reads. The caller clears the book, so the next tick re-learns it.
pub async fn set_strategy(
    pool: &PgPool,
    id: Uuid,
    settings: &Value,
    version: Option<i32>,
    name: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE robots SET settings = $2, strategy_version = $3, strategy_name = $4, \
           updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(settings)
    .bind(version)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

/// One robot executing on a broker account, as the sync orders them: by priority, then by
/// age, with the broker its account is on (the provider whose rate limit it shares).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BrokerRobot {
    pub id: Uuid,
    pub priority: i32,
    pub broker: String,
}

pub async fn broker_robots(pool: &PgPool) -> anyhow::Result<Vec<BrokerRobot>> {
    Ok(sqlx::query_as::<_, BrokerRobot>(
        "SELECT s.id, s.priority, COALESCE(a.broker, '') AS broker FROM robots s \
         LEFT JOIN broker_accounts a ON a.id = s.exec_account_id \
         WHERE s.execution->>'target' = 'broker' ORDER BY s.priority, s.created_at",
    )
    .fetch_all(pool)
    .await?)
}

/// The version number of a saved strategy: how many versions it has saved (0 for none).
pub async fn strategy_version(pool: &PgPool, strategy_id: Uuid) -> anyhow::Result<i32> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM strategy_versions WHERE strategy_id = $1")
        .bind(strategy_id)
        .fetch_one(pool)
        .await?;
    Ok(n as i32)
}

/// Closed trades whose robot money is not settled yet: written once they close, cleared when
/// the replay re-prices them.
pub async fn unsettled(pool: &PgPool, session_id: Uuid) -> anyhow::Result<Vec<Trade>> {
    Ok(sqlx::query_as::<_, Trade>(AssertSqlSafe(format!(
        "SELECT {TRADE_COLS} FROM robot_trades WHERE session_id = $1 AND NOT open AND robot_pnl IS NULL"
    )))
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

/// Price a broker robot's closed trade from its fills at the venue: its net P&L and the fees
/// it paid. Kept over the engine's figure, even when a replay re-prices the trade. True when
/// the stored figure changed.
pub async fn set_broker_money(pool: &PgPool, session_id: Uuid, key: &str, pnl: Decimal, fees: Decimal) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "UPDATE robot_trades SET robot_pnl = $3, robot_fees = $4, money_at_broker = true \
         WHERE session_id = $1 AND trade_key = $2 AND NOT open AND robot_qty IS DISTINCT FROM 0 \
           AND (NOT money_at_broker OR robot_pnl IS DISTINCT FROM $3 OR robot_fees IS DISTINCT FROM $4)",
    )
    .bind(session_id)
    .bind(key)
    .bind(pnl.normalize())
    .bind(fees.normalize())
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// A closed trade as the broker filled it: its entry and exit prices and, for an exit the
/// engine did not decide (Stop and flatten, the user's close at the broker), its time and
/// reason, replace the engine's and stay through every later run (`upsert_trade` keeps them).
/// False when already written.
pub async fn set_broker_exit(pool: &PgPool, session_id: Uuid, key: &str, exit: &Value) -> anyhow::Result<bool> {
    let n = sqlx::query(
        "UPDATE robot_trades SET exit_ts = COALESCE($3->>'exit_ts', exit_ts), \
           trade = trade || $3 || jsonb_build_object('broker_exit', $3) \
         WHERE session_id = $1 AND trade_key = $2 AND NOT open AND trade->'broker_exit' IS DISTINCT FROM $3",
    )
    .bind(session_id)
    .bind(key)
    .bind(exit)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// Settle a closed trade's robot money, in decimals.
pub async fn set_robot_money(pool: &PgPool, session_id: Uuid, key: &str, pnl: Decimal, fees: Decimal) -> anyhow::Result<()> {
    sqlx::query("UPDATE robot_trades SET robot_pnl = $3, robot_fees = $4 WHERE session_id = $1 AND trade_key = $2 AND NOT open")
        .bind(session_id)
        .bind(key)
        .bind(pnl.normalize())
        .bind(fees.normalize())
        .execute(pool)
        .await?;
    Ok(())
}
