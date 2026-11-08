//! Scanner: items, hives, reusable signals, alerts, hits, boards and their views
//! (migrations 0170, 0172).
//!
//! What a signal *means* and how a bar is judged lives in `otw-core` (`scanner/`), next to
//! the backtest engine it reuses. This module only stores rows and the evaluator's
//! bookkeeping: the item cache the board reads, the hits, and each alert's last error.

use anyhow::Context;
use serde::Serialize;
use serde_json::Value as JsonValue;
use sqlx::{AssertSqlSafe, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

/// Items a hive may hold unless the user changed it (Scanner settings).
pub const DEFAULT_HIVE_CAP: i64 = 200;
/// Highest cap the setting accepts: past it a scan is a bulk download every bar.
pub const MAX_HIVE_CAP: i64 = 2000;
const HIVE_CAP_KEY: &str = "scanner_hive_cap";

/// Why a hive of `count` items is refused under `cap`, with the count named; `None` = fits.
pub fn cap_refusal(count: i64, cap: i64) -> Option<String> {
    (count > cap).then(|| {
        format!(
            "This hive would hold {count} items, over the cap of {cap}. Remove {} or raise the \
             cap in the Scanner settings.",
            count - cap
        )
    })
}

pub async fn hive_cap(pool: &PgPool) -> anyhow::Result<i64> {
    let raw = crate::settings::get(pool, HIVE_CAP_KEY).await?;
    Ok(raw
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|v| (1..=MAX_HIVE_CAP).contains(v))
        .unwrap_or(DEFAULT_HIVE_CAP))
}

pub async fn set_hive_cap(pool: &PgPool, cap: i64) -> anyhow::Result<()> {
    crate::settings::set(pool, HIVE_CAP_KEY, &cap.to_string()).await
}

// ── Items ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Item {
    pub id: Uuid,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    pub connector_id: Option<Uuid>,
    pub label: String,
    pub color: String,
    pub last_close: Option<f64>,
    pub last_open: Option<f64>,
    pub last_volume: Option<f64>,
    pub avg_volume: Option<f64>,
    pub prev_close: Option<f64>,
    pub day_open: Option<f64>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_bar_ts: Option<OffsetDateTime>,
    pub spark: JsonValue,
    pub signal_state: JsonValue,
    #[serde(with = "time::serde::rfc3339::option")]
    pub checked_at: Option<OffsetDateTime>,
    pub last_error: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub updated_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Hives holding this item.
    pub hive_ids: Vec<Uuid>,
}

const ITEM_COLS: &str = "i.id, i.provider, i.asset_type, i.ticker, i.timeframe, i.connector_id, \
     i.label, i.color, i.last_close, i.last_open, i.last_volume, i.avg_volume, i.prev_close, \
     i.day_open, i.last_bar_ts, i.spark, i.signal_state, i.checked_at, i.last_error, \
     i.updated_at, i.created_at, \
     COALESCE((SELECT array_agg(h.hive_id) FROM scanner_hive_items h WHERE h.item_id = i.id), \
              '{}') AS hive_ids";

pub struct ItemInput<'a> {
    pub provider: &'a str,
    pub asset_type: &'a str,
    pub ticker: &'a str,
    pub timeframe: &'a str,
    pub connector_id: Option<Uuid>,
    pub label: &'a str,
    pub color: &'a str,
}

pub async fn list_items(pool: &PgPool) -> anyhow::Result<Vec<Item>> {
    let sql = format!("SELECT {ITEM_COLS} FROM scanner_items i ORDER BY i.ticker, i.timeframe");
    Ok(sqlx::query_as::<_, Item>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_item(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Item>> {
    let sql = format!("SELECT {ITEM_COLS} FROM scanner_items i WHERE i.id = $1");
    Ok(sqlx::query_as::<_, Item>(AssertSqlSafe(sql)).bind(id).fetch_optional(pool).await?)
}

pub async fn items_by_ids(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<Vec<Item>> {
    let sql = format!(
        "SELECT {ITEM_COLS} FROM scanner_items i WHERE i.id = ANY($1) ORDER BY i.ticker, i.timeframe"
    );
    Ok(sqlx::query_as::<_, Item>(AssertSqlSafe(sql)).bind(ids).fetch_all(pool).await?)
}

/// The items of every hive a board view shows: the job keeps their cache fresh even when no
/// alert scans them.
pub async fn board_items(pool: &PgPool) -> anyhow::Result<Vec<Item>> {
    let sql = format!(
        "SELECT {ITEM_COLS} FROM scanner_items i WHERE i.id IN \
           (SELECT m.item_id FROM scanner_hive_items m JOIN scanner_views v ON v.hive_id = m.hive_id) \
         ORDER BY i.ticker, i.timeframe"
    );
    Ok(sqlx::query_as::<_, Item>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

/// The item already holding these coordinates, if any (the unique key of the table).
pub async fn find_item(
    pool: &PgPool,
    provider: &str,
    ticker: &str,
    timeframe: &str,
    connector_id: Option<Uuid>,
) -> anyhow::Result<Option<Item>> {
    let sql = format!(
        "SELECT {ITEM_COLS} FROM scanner_items i \
         WHERE i.provider = $1 AND i.ticker = $2 AND i.timeframe = $3 \
           AND i.connector_id IS NOT DISTINCT FROM $4"
    );
    Ok(sqlx::query_as::<_, Item>(AssertSqlSafe(sql))
        .bind(provider)
        .bind(ticker)
        .bind(timeframe)
        .bind(connector_id)
        .fetch_optional(pool)
        .await?)
}

pub async fn create_item(pool: &PgPool, i: ItemInput<'_>) -> anyhow::Result<Item> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO scanner_items (provider, asset_type, ticker, timeframe, connector_id, label, color) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id",
    )
    .bind(i.provider)
    .bind(i.asset_type)
    .bind(i.ticker)
    .bind(i.timeframe)
    .bind(i.connector_id)
    .bind(i.label)
    .bind(i.color)
    .fetch_one(pool)
    .await
    .context("creating scanner item")?;
    get_item(pool, id).await?.context("reading the new scanner item")
}

/// Replace an item. A different instrument drops the cache: the old closes describe
/// something else.
pub async fn update_item(pool: &PgPool, id: Uuid, i: ItemInput<'_>) -> anyhow::Result<Option<Item>> {
    let done = sqlx::query(
        "UPDATE scanner_items SET \
           last_close = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN last_close END, \
           last_open = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN last_open END, \
           last_volume = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN last_volume END, \
           avg_volume = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN avg_volume END, \
           prev_close = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN prev_close END, \
           day_open = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN day_open END, \
           last_bar_ts = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN last_bar_ts END, \
           spark = CASE WHEN (provider, asset_type, ticker, timeframe) = ($2,$3,$4,$5) THEN spark ELSE '[]'::jsonb END, \
           provider = $2, asset_type = $3, ticker = $4, timeframe = $5, connector_id = $6, \
           label = $7, color = $8, last_error = '' \
         WHERE id = $1",
    )
    .bind(id)
    .bind(i.provider)
    .bind(i.asset_type)
    .bind(i.ticker)
    .bind(i.timeframe)
    .bind(i.connector_id)
    .bind(i.label)
    .bind(i.color)
    .execute(pool)
    .await
    .context("saving scanner item")?;
    if done.rows_affected() == 0 {
        return Ok(None);
    }
    get_item(pool, id).await
}

pub async fn delete_item(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM scanner_items WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

/// What the evaluator learned from one item's last closed bars.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemCache {
    pub last_close: Option<f64>,
    pub last_open: Option<f64>,
    pub last_volume: Option<f64>,
    pub avg_volume: Option<f64>,
    pub prev_close: Option<f64>,
    pub day_open: Option<f64>,
    pub last_bar_ts: Option<OffsetDateTime>,
    /// Recent closes as `[unix seconds, close]`, oldest first.
    pub spark: Vec<(i64, f64)>,
}

pub async fn save_item_cache(pool: &PgPool, id: Uuid, c: &ItemCache) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE scanner_items SET last_close = $2, last_open = $3, last_volume = $4, \
           avg_volume = $5, prev_close = $6, day_open = $7, last_bar_ts = $8, spark = $9, \
           checked_at = now(), updated_at = now(), last_error = '' \
         WHERE id = $1",
    )
    .bind(id)
    .bind(c.last_close)
    .bind(c.last_open)
    .bind(c.last_volume)
    .bind(c.avg_volume)
    .bind(c.prev_close)
    .bind(c.day_open)
    .bind(c.last_bar_ts)
    .bind(serde_json::json!(c.spark))
    .execute(pool)
    .await?;
    Ok(())
}

/// An item that was read and has no new closed bar yet: only the check time moves.
pub async fn mark_item_checked(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE scanner_items SET checked_at = now(), last_error = '' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_item_error(pool: &PgPool, id: Uuid, msg: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE scanner_items SET checked_at = now(), last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(msg)
        .execute(pool)
        .await?;
    Ok(())
}

/// Merge the signals judged on an item's last bar into its state map.
pub async fn merge_signal_state(pool: &PgPool, id: Uuid, state: &JsonValue) -> anyhow::Result<()> {
    sqlx::query("UPDATE scanner_items SET signal_state = signal_state || $2 WHERE id = $1")
        .bind(id)
        .bind(state)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Hives ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Hive {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub sort: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub item_ids: Vec<Uuid>,
}

const HIVE_COLS: &str = "h.id, h.name, h.color, h.sort, h.created_at, h.updated_at, \
     COALESCE((SELECT array_agg(m.item_id) FROM scanner_hive_items m WHERE m.hive_id = h.id), \
              '{}') AS item_ids";

pub async fn list_hives(pool: &PgPool) -> anyhow::Result<Vec<Hive>> {
    let sql = format!("SELECT {HIVE_COLS} FROM scanner_hives h ORDER BY h.sort, h.created_at");
    Ok(sqlx::query_as::<_, Hive>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_hive(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Hive>> {
    let sql = format!("SELECT {HIVE_COLS} FROM scanner_hives h WHERE h.id = $1");
    Ok(sqlx::query_as::<_, Hive>(AssertSqlSafe(sql)).bind(id).fetch_optional(pool).await?)
}

pub async fn create_hive(pool: &PgPool, name: &str, color: &str) -> anyhow::Result<Hive> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO scanner_hives (name, color, sort) \
         VALUES ($1, $2, (SELECT COALESCE(MAX(sort) + 1, 0) FROM scanner_hives)) RETURNING id",
    )
    .bind(name)
    .bind(color)
    .fetch_one(pool)
    .await
    .context("creating hive")?;
    get_hive(pool, id).await?.context("reading the new hive")
}

pub async fn update_hive(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    color: &str,
    sort: Option<i32>,
) -> anyhow::Result<Option<Hive>> {
    let res = sqlx::query(
        "UPDATE scanner_hives SET name = $2, color = $3, sort = COALESCE($4, sort), updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .bind(color)
    .bind(sort)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    get_hive(pool, id).await
}

pub async fn delete_hive(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM scanner_hives WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

/// Replace a hive's members. The caller checked the cap.
pub async fn set_hive_items(pool: &PgPool, hive_id: Uuid, item_ids: &[Uuid]) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM scanner_hive_items WHERE hive_id = $1")
        .bind(hive_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO scanner_hive_items (hive_id, item_id) \
         SELECT $1, x FROM unnest($2::uuid[]) AS x ON CONFLICT DO NOTHING",
    )
    .bind(hive_id)
    .bind(item_ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE scanner_hives SET updated_at = now() WHERE id = $1")
        .bind(hive_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Items in a hive, or one item, as an alert's target.
pub async fn hive_item_ids(pool: &PgPool, hive_id: Uuid) -> anyhow::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT item_id FROM scanner_hive_items WHERE hive_id = $1")
        .bind(hive_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

// ── Signals ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Signal {
    pub id: Uuid,
    pub name: String,
    pub builder: String,
    pub config: JsonValue,
    pub color: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const SIGNAL_COLS: &str = "id, name, builder, config, color, created_at, updated_at";

pub async fn list_signals(pool: &PgPool) -> anyhow::Result<Vec<Signal>> {
    let sql = format!("SELECT {SIGNAL_COLS} FROM scanner_signals ORDER BY name, created_at");
    Ok(sqlx::query_as::<_, Signal>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_signal(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Signal>> {
    let sql = format!("SELECT {SIGNAL_COLS} FROM scanner_signals WHERE id = $1");
    Ok(sqlx::query_as::<_, Signal>(AssertSqlSafe(sql)).bind(id).fetch_optional(pool).await?)
}

pub async fn signals_by_ids(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<Vec<Signal>> {
    let sql = format!("SELECT {SIGNAL_COLS} FROM scanner_signals WHERE id = ANY($1)");
    Ok(sqlx::query_as::<_, Signal>(AssertSqlSafe(sql)).bind(ids).fetch_all(pool).await?)
}

pub async fn create_signal(
    pool: &PgPool,
    name: &str,
    builder: &str,
    config: &JsonValue,
    color: &str,
) -> anyhow::Result<Signal> {
    let sql = format!(
        "INSERT INTO scanner_signals (name, builder, config, color) VALUES ($1,$2,$3,$4) \
         RETURNING {SIGNAL_COLS}"
    );
    sqlx::query_as::<_, Signal>(AssertSqlSafe(sql))
        .bind(name)
        .bind(builder)
        .bind(config)
        .bind(color)
        .fetch_one(pool)
        .await
        .context("creating scanner signal")
}

pub async fn update_signal(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    builder: &str,
    config: &JsonValue,
    color: &str,
) -> anyhow::Result<Option<Signal>> {
    let sql = format!(
        "UPDATE scanner_signals SET name = $2, builder = $3, config = $4, color = $5, \
           updated_at = now() WHERE id = $1 RETURNING {SIGNAL_COLS}"
    );
    sqlx::query_as::<_, Signal>(AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(builder)
        .bind(config)
        .bind(color)
        .fetch_optional(pool)
        .await
        .context("saving scanner signal")
}

pub async fn delete_signal(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM scanner_signals WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

/// Names of the alerts that AND this signal, so a delete can say which ones it would break.
pub async fn alerts_using_signal(pool: &PgPool, id: Uuid) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT CASE WHEN name = '' THEN 'unnamed alert' ELSE name END \
         FROM scanner_alerts WHERE $1 = ANY(signal_ids) ORDER BY name",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

// ── Alerts ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Alert {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub target_kind: String,
    pub target_id: Uuid,
    pub signal_ids: Vec<Uuid>,
    pub notify: String,
    pub channels: JsonValue,
    pub cooldown_secs: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    pub last_error: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const ALERT_COLS: &str = "id, name, enabled, target_kind, target_id, signal_ids, notify, channels, \
     cooldown_secs, last_run_at, last_error, created_at, updated_at";

pub struct AlertInput<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub target_kind: &'a str,
    pub target_id: Uuid,
    pub signal_ids: &'a [Uuid],
    pub notify: &'a str,
    pub channels: &'a JsonValue,
    pub cooldown_secs: i32,
}

pub async fn list_alerts(pool: &PgPool) -> anyhow::Result<Vec<Alert>> {
    let sql = format!("SELECT {ALERT_COLS} FROM scanner_alerts ORDER BY created_at DESC");
    Ok(sqlx::query_as::<_, Alert>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn enabled_alerts(pool: &PgPool) -> anyhow::Result<Vec<Alert>> {
    let sql = format!("SELECT {ALERT_COLS} FROM scanner_alerts WHERE enabled ORDER BY created_at");
    Ok(sqlx::query_as::<_, Alert>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn get_alert(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Alert>> {
    let sql = format!("SELECT {ALERT_COLS} FROM scanner_alerts WHERE id = $1");
    Ok(sqlx::query_as::<_, Alert>(AssertSqlSafe(sql)).bind(id).fetch_optional(pool).await?)
}

pub async fn create_alert(pool: &PgPool, a: AlertInput<'_>) -> anyhow::Result<Alert> {
    let sql = format!(
        "INSERT INTO scanner_alerts (name, enabled, target_kind, target_id, signal_ids, notify, \
           channels, cooldown_secs) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {ALERT_COLS}"
    );
    sqlx::query_as::<_, Alert>(AssertSqlSafe(sql))
        .bind(a.name)
        .bind(a.enabled)
        .bind(a.target_kind)
        .bind(a.target_id)
        .bind(a.signal_ids)
        .bind(a.notify)
        .bind(a.channels)
        .bind(a.cooldown_secs)
        .fetch_one(pool)
        .await
        .context("creating scanner alert")
}

/// Replace an alert. Re-enabling one clears its error, so the next run reports afresh.
pub async fn update_alert(pool: &PgPool, id: Uuid, a: AlertInput<'_>) -> anyhow::Result<Option<Alert>> {
    let sql = format!(
        "UPDATE scanner_alerts SET name = $2, \
           last_error = CASE WHEN $3 AND NOT enabled THEN '' ELSE last_error END, \
           enabled = $3, target_kind = $4, target_id = $5, signal_ids = $6, notify = $7, \
           channels = $8, cooldown_secs = $9, updated_at = now() \
         WHERE id = $1 RETURNING {ALERT_COLS}"
    );
    sqlx::query_as::<_, Alert>(AssertSqlSafe(sql))
        .bind(id)
        .bind(a.name)
        .bind(a.enabled)
        .bind(a.target_kind)
        .bind(a.target_id)
        .bind(a.signal_ids)
        .bind(a.notify)
        .bind(a.channels)
        .bind(a.cooldown_secs)
        .fetch_optional(pool)
        .await
        .context("saving scanner alert")
}

pub async fn delete_alert(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM scanner_alerts WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

/// Record a run and the error it ended with ('' = it worked).
pub async fn mark_alert_run(pool: &PgPool, id: Uuid, error: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE scanner_alerts SET last_run_at = now(), last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Hits ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Hit {
    pub id: Uuid,
    pub alert_id: Uuid,
    pub item_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub bar_ts: OffsetDateTime,
    pub values: JsonValue,
    #[serde(with = "time::serde::rfc3339::option")]
    pub notified_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub alert_name: String,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    pub connector_id: Option<Uuid>,
}

/// Record a hit. `None` = this (alert, item, bar) was already recorded: a bar judged twice
/// is one hit and one notification.
pub async fn insert_hit(
    pool: &PgPool,
    alert_id: Uuid,
    item_id: Uuid,
    bar_ts: OffsetDateTime,
    values: &JsonValue,
) -> anyhow::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO scanner_hits (alert_id, item_id, bar_ts, \"values\") VALUES ($1,$2,$3,$4) \
         ON CONFLICT ON CONSTRAINT scanner_hits_once DO NOTHING RETURNING id",
    )
    .bind(alert_id)
    .bind(item_id)
    .bind(bar_ts)
    .bind(values)
    .fetch_optional(pool)
    .await
    .context("recording scanner hit")?;
    Ok(row.map(|r| r.0))
}

pub async fn mark_hits_notified(pool: &PgPool, ids: &[Uuid]) -> anyhow::Result<()> {
    sqlx::query("UPDATE scanner_hits SET notified_at = now() WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?;
    Ok(())
}

/// When this alert last hit this item (its bar), for the cooldown.
pub async fn last_hit_bar(pool: &PgPool, alert_id: Uuid, item_id: Uuid) -> anyhow::Result<Option<OffsetDateTime>> {
    let row: (Option<OffsetDateTime>,) = sqlx::query_as(
        "SELECT MAX(bar_ts) FROM scanner_hits WHERE alert_id = $1 AND item_id = $2",
    )
    .bind(alert_id)
    .bind(item_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub struct HitFilter {
    pub alert_id: Option<Uuid>,
    pub item_id: Option<Uuid>,
    pub hive_id: Option<Uuid>,
    pub limit: i64,
}

pub async fn list_hits(pool: &PgPool, f: &HitFilter) -> anyhow::Result<Vec<Hit>> {
    Ok(sqlx::query_as::<_, Hit>(
        "SELECT h.id, h.alert_id, h.item_id, h.bar_ts, h.\"values\", h.notified_at, h.created_at, \
           COALESCE(NULLIF(a.name, ''), 'unnamed alert') AS alert_name, \
           i.provider, i.asset_type, i.ticker, i.timeframe, i.connector_id \
         FROM scanner_hits h \
         JOIN scanner_alerts a ON a.id = h.alert_id \
         JOIN scanner_items i ON i.id = h.item_id \
         WHERE ($1::uuid IS NULL OR h.alert_id = $1) \
           AND ($2::uuid IS NULL OR h.item_id = $2) \
           AND ($3::uuid IS NULL OR h.item_id IN (SELECT item_id FROM scanner_hive_items WHERE hive_id = $3)) \
         ORDER BY h.created_at DESC, h.bar_ts DESC LIMIT $4",
    )
    .bind(f.alert_id)
    .bind(f.item_id)
    .bind(f.hive_id)
    .bind(f.limit)
    .fetch_all(pool)
    .await?)
}

/// Last hit per item, for the board's "last hit" column and the hit-recency color.
pub async fn last_hits(pool: &PgPool, item_ids: &[Uuid]) -> anyhow::Result<Vec<(Uuid, OffsetDateTime)>> {
    Ok(sqlx::query_as(
        "SELECT item_id, MAX(created_at) FROM scanner_hits WHERE item_id = ANY($1) GROUP BY item_id",
    )
    .bind(item_ids)
    .fetch_all(pool)
    .await?)
}

/// Hits older than `days` are dropped: the log is a recent history, not an archive.
pub async fn prune_hits(pool: &PgPool, days: i64) -> anyhow::Result<u64> {
    let res = sqlx::query("DELETE FROM scanner_hits WHERE created_at < now() - make_interval(days => $1::int)")
        .bind(days as i32)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

// ── Boards ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Board {
    pub id: Uuid,
    pub name: String,
    pub sort: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const BOARD_COLS: &str = "id, name, sort, created_at, updated_at";

pub async fn list_boards(pool: &PgPool) -> anyhow::Result<Vec<Board>> {
    let sql = format!("SELECT {BOARD_COLS} FROM scanner_boards ORDER BY sort, created_at");
    Ok(sqlx::query_as::<_, Board>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn create_board(pool: &PgPool, name: &str, sort: Option<i32>) -> anyhow::Result<Board> {
    let sql = format!(
        "INSERT INTO scanner_boards (name, sort) \
         VALUES ($1, COALESCE($2, (SELECT COALESCE(MAX(sort) + 1, 0) FROM scanner_boards))) \
         RETURNING {BOARD_COLS}"
    );
    sqlx::query_as::<_, Board>(AssertSqlSafe(sql))
        .bind(name)
        .bind(sort)
        .fetch_one(pool)
        .await
        .context("creating scanner board")
}

pub async fn update_board(pool: &PgPool, id: Uuid, name: &str, sort: Option<i32>) -> anyhow::Result<Option<Board>> {
    let sql = format!(
        "UPDATE scanner_boards SET name = $2, sort = COALESCE($3, sort), updated_at = now() \
         WHERE id = $1 RETURNING {BOARD_COLS}"
    );
    sqlx::query_as::<_, Board>(AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(sort)
        .fetch_optional(pool)
        .await
        .context("saving scanner board")
}

/// Deletes the board and its views.
pub async fn delete_board(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM scanner_boards WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

// ── Views ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct View {
    pub id: Uuid,
    pub board_id: Uuid,
    pub name: String,
    pub kind: String,
    pub hive_id: Option<Uuid>,
    pub color_by: JsonValue,
    pub layout: JsonValue,
    pub sort: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const VIEW_COLS: &str = "id, board_id, name, kind, hive_id, color_by, layout, sort, created_at, updated_at";

pub struct ViewInput<'a> {
    pub board_id: Uuid,
    pub name: &'a str,
    pub kind: &'a str,
    pub hive_id: Option<Uuid>,
    pub color_by: &'a JsonValue,
    pub layout: &'a JsonValue,
    pub sort: Option<i32>,
}

pub async fn list_views(pool: &PgPool) -> anyhow::Result<Vec<View>> {
    let sql = format!("SELECT {VIEW_COLS} FROM scanner_views ORDER BY sort, created_at");
    Ok(sqlx::query_as::<_, View>(AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn create_view(pool: &PgPool, v: ViewInput<'_>) -> anyhow::Result<View> {
    let sql = format!(
        "INSERT INTO scanner_views (board_id, name, kind, hive_id, color_by, layout, sort) \
         VALUES ($1,$2,$3,$4,$5,$6, \
           COALESCE($7, (SELECT COALESCE(MAX(sort) + 1, 0) FROM scanner_views WHERE board_id = $1))) \
         RETURNING {VIEW_COLS}"
    );
    sqlx::query_as::<_, View>(AssertSqlSafe(sql))
        .bind(v.board_id)
        .bind(v.name)
        .bind(v.kind)
        .bind(v.hive_id)
        .bind(v.color_by)
        .bind(v.layout)
        .bind(v.sort)
        .fetch_one(pool)
        .await
        .context("creating scanner view")
}

pub async fn update_view(pool: &PgPool, id: Uuid, v: ViewInput<'_>) -> anyhow::Result<Option<View>> {
    let sql = format!(
        "UPDATE scanner_views SET board_id = $2, name = $3, kind = $4, hive_id = $5, color_by = $6, \
           layout = $7, sort = COALESCE($8, sort), updated_at = now() WHERE id = $1 RETURNING {VIEW_COLS}"
    );
    sqlx::query_as::<_, View>(AssertSqlSafe(sql))
        .bind(id)
        .bind(v.board_id)
        .bind(v.name)
        .bind(v.kind)
        .bind(v.hive_id)
        .bind(v.color_by)
        .bind(v.layout)
        .bind(v.sort)
        .fetch_optional(pool)
        .await
        .context("saving scanner view")
}

pub async fn delete_view(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM scanner_views WHERE id = $1").bind(id).execute(pool).await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_hive_cap_refuses_above_the_cap_with_the_count_named() {
        assert_eq!(cap_refusal(200, 200), None);
        assert_eq!(cap_refusal(0, 200), None);
        let why = cap_refusal(230, 200).unwrap();
        assert!(why.contains("230 items"), "{why}");
        assert!(why.contains("cap of 200"), "{why}");
        assert!(why.contains("Remove 30"), "{why}");
    }

    #[test]
    fn scanner_cache_defaults_to_nothing_known() {
        let c = ItemCache::default();
        assert!(c.last_close.is_none() && c.spark.is_empty());
    }
}
