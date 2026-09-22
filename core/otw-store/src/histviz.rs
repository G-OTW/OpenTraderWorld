//! Storage for the chart module ("histviz"): workspaces, per-instrument layouts and the
//! chart's own instrument lists.
//!
//! Everything here is addressed by **instrument coordinates**, never by a dataset id. The
//! chart has been coordinate-addressed since `/api/histviz/series`; the layout was the last
//! thing keyed on storage, which meant a symbol you looked at without saving could not keep
//! the indicators and drawings you put on it.
//!
//! The JSON blobs (`layout`, `panes`, `settings`, `items`) are the client's schema, round
//! tripped as-is: the same contract the per-dataset layout already had. What the server owns
//! is the identity (the coordinate key, the grid shape) and nothing else.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Grid bounds, mirrored by the `histviz_workspaces_shape` check constraint. A workspace is
/// any rectangle in between, which is how a vertical split, a horizontal split and a 2x2 all
/// come out of one model.
pub const MAX_ROWS: i32 = 3;
pub const MAX_COLS: i32 = 4;

// ── Instrument layouts ─────────────────────────────────────────────────────────

/// The key a layout is stored under: the instrument's coordinates, provider and asset type
/// and timeframe lowercased, the ticker kept verbatim (case is the provider's business).
/// The connector is deliberately *not* part of it: two keys of the same provider are two
/// entitlements, not two charts.
pub fn coord_key(provider: &str, asset_type: &str, ticker: &str, timeframe: &str) -> String {
    format!(
        "{}|{}|{}|{}",
        provider.trim().to_lowercase(),
        asset_type.trim().to_lowercase(),
        ticker.trim(),
        timeframe.trim().to_lowercase()
    )
}

/// The saved layout for an instrument, or None when it was never customized.
pub async fn get_layout(pool: &PgPool, key: &str) -> anyhow::Result<Option<JsonValue>> {
    let row: Option<(JsonValue,)> =
        sqlx::query_as("SELECT layout FROM histviz_instrument_layouts WHERE coord_key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

/// Replace an instrument's layout.
pub async fn set_layout(pool: &PgPool, key: &str, layout: &JsonValue) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO histviz_instrument_layouts (coord_key, layout) VALUES ($1, $2) \
         ON CONFLICT (coord_key) DO UPDATE SET layout = EXCLUDED.layout, updated_at = now()",
    )
    .bind(key)
    .bind(layout)
    .execute(pool)
    .await
    .context("saving chart layout")?;
    Ok(())
}

/// Forget an instrument's layout. `false` when there was none.
pub async fn delete_layout(pool: &PgPool, key: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histviz_instrument_layouts WHERE coord_key = $1")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Workspaces ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub grid_rows: i32,
    pub grid_cols: i32,
    pub panes: JsonValue,
    pub settings: JsonValue,
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub async fn list_workspaces(pool: &PgPool) -> anyhow::Result<Vec<Workspace>> {
    let rows = sqlx::query_as::<_, Workspace>(
        "SELECT id, name, grid_rows, grid_cols, panes, settings, position, created_at, updated_at \
         FROM histviz_workspaces ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_workspace(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Workspace>> {
    let row = sqlx::query_as::<_, Workspace>(
        "SELECT id, name, grid_rows, grid_cols, panes, settings, position, created_at, updated_at \
         FROM histviz_workspaces WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Everything a workspace row holds, for a create or a full replace. The client owns
/// `panes` and `settings`; the server owns the shape.
pub struct WorkspaceInput<'a> {
    pub name: &'a str,
    pub grid_rows: i32,
    pub grid_cols: i32,
    pub panes: &'a JsonValue,
    pub settings: &'a JsonValue,
    pub position: i32,
}

pub async fn create_workspace(pool: &PgPool, w: WorkspaceInput<'_>) -> anyhow::Result<Workspace> {
    let row = sqlx::query_as::<_, Workspace>(
        "INSERT INTO histviz_workspaces (name, grid_rows, grid_cols, panes, settings, position) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, name, grid_rows, grid_cols, panes, settings, position, created_at, updated_at",
    )
    .bind(w.name)
    .bind(w.grid_rows)
    .bind(w.grid_cols)
    .bind(w.panes)
    .bind(w.settings)
    .bind(w.position)
    .fetch_one(pool)
    .await
    .context("creating chart workspace")?;
    Ok(row)
}

/// Replace a workspace. Returns None when the id is unknown.
pub async fn update_workspace(
    pool: &PgPool,
    id: Uuid,
    w: WorkspaceInput<'_>,
) -> anyhow::Result<Option<Workspace>> {
    let row = sqlx::query_as::<_, Workspace>(
        "UPDATE histviz_workspaces \
         SET name = $2, grid_rows = $3, grid_cols = $4, panes = $5, settings = $6, \
             position = $7, updated_at = now() \
         WHERE id = $1 \
         RETURNING id, name, grid_rows, grid_cols, panes, settings, position, created_at, updated_at",
    )
    .bind(id)
    .bind(w.name)
    .bind(w.grid_rows)
    .bind(w.grid_cols)
    .bind(w.panes)
    .bind(w.settings)
    .bind(w.position)
    .fetch_optional(pool)
    .await
    .context("saving chart workspace")?;
    Ok(row)
}

pub async fn delete_workspace(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histviz_workspaces WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Chart lists ────────────────────────────────────────────────────────────────
//
// The rail's own source, next to the read-only lists it shows from the Watchlists module.
// A list built by clicking "add this instrument" on a chart is a scratchpad, so it lives
// here; turning one into a real watchlist is an explicit action in the API layer, never a
// side effect of charting.

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChartList {
    pub id: Uuid,
    pub name: String,
    pub items: JsonValue,
    pub position: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub async fn list_lists(pool: &PgPool) -> anyhow::Result<Vec<ChartList>> {
    let rows = sqlx::query_as::<_, ChartList>(
        "SELECT id, name, items, position, created_at, updated_at \
         FROM histviz_lists ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_list(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<ChartList>> {
    let row = sqlx::query_as::<_, ChartList>(
        "SELECT id, name, items, position, created_at, updated_at FROM histviz_lists WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_list(
    pool: &PgPool,
    name: &str,
    items: &JsonValue,
    position: i32,
) -> anyhow::Result<ChartList> {
    let row = sqlx::query_as::<_, ChartList>(
        "INSERT INTO histviz_lists (name, items, position) VALUES ($1, $2, $3) \
         RETURNING id, name, items, position, created_at, updated_at",
    )
    .bind(name)
    .bind(items)
    .bind(position)
    .fetch_one(pool)
    .await
    .context("creating chart list")?;
    Ok(row)
}

pub async fn update_list(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    items: &JsonValue,
    position: i32,
) -> anyhow::Result<Option<ChartList>> {
    let row = sqlx::query_as::<_, ChartList>(
        "UPDATE histviz_lists SET name = $2, items = $3, position = $4, updated_at = now() \
         WHERE id = $1 RETURNING id, name, items, position, created_at, updated_at",
    )
    .bind(id)
    .bind(name)
    .bind(items)
    .bind(position)
    .fetch_optional(pool)
    .await
    .context("saving chart list")?;
    Ok(row)
}

pub async fn delete_list(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histviz_lists WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coord_key_normalizes_provider_and_timeframe_but_not_the_ticker() {
        assert_eq!(coord_key("Binance", "Crypto", "BTCUSDT", "1H"), "binance|crypto|BTCUSDT|1h");
        // A provider that cares about case keeps it: IB's `SAN:EUR` is not `san:eur`.
        assert_eq!(coord_key("ibkr", "equity", " SAN:EUR ", "1d"), "ibkr|equity|SAN:EUR|1d");
    }
}

// ── Alerts ─────────────────────────────────────────────────────────────────────
//
// A level on an instrument, watched by the server. Everything the evaluator needs is on the
// row, including what it last saw: `last_bar_ts` is what makes a wake-up idempotent (a bar
// is judged once, however often the loop runs), and `last_error` is why the alert list can
// say "no connector" instead of going quiet.

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Alert {
    pub id: Uuid,
    pub name: String,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    pub connector_id: Option<Uuid>,
    pub kind: String,
    pub source: String,
    pub indicator_id: Option<Uuid>,
    pub op: String,
    pub value: f64,
    pub channels: JsonValue,
    pub repeat: bool,
    pub cooldown_secs: i32,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_bar_ts: Option<OffsetDateTime>,
    pub last_value: Option<f64>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_fired_at: Option<OffsetDateTime>,
    pub fire_count: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub checked_at: Option<OffsetDateTime>,
    pub last_error: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

const ALERT_COLS: &str = "id, name, provider, asset_type, ticker, timeframe, connector_id, kind, \
                          source, indicator_id, op, value, channels, repeat, cooldown_secs, \
                          enabled, last_bar_ts, last_value, last_fired_at, fire_count, \
                          checked_at, last_error, created_at, updated_at";

/// Everything a create or a full replace sets. The bookkeeping columns are the evaluator's
/// and are never written from the API: an alert that could reset its own "last fired" would
/// be a way to re-fire it by editing it.
pub struct AlertInput<'a> {
    pub name: &'a str,
    pub provider: &'a str,
    pub asset_type: &'a str,
    pub ticker: &'a str,
    pub timeframe: &'a str,
    pub connector_id: Option<Uuid>,
    pub kind: &'a str,
    pub source: &'a str,
    pub indicator_id: Option<Uuid>,
    pub op: &'a str,
    pub value: f64,
    pub channels: &'a JsonValue,
    pub repeat: bool,
    pub cooldown_secs: i32,
    pub enabled: bool,
}

/// Every alert, newest first. Optionally narrowed to one instrument, which is what the pane
/// asks for when it draws its own levels.
pub async fn list_alerts(
    pool: &PgPool,
    instrument: Option<(&str, &str, &str, &str)>,
) -> anyhow::Result<Vec<Alert>> {
    let sql = match instrument {
        Some(_) => format!(
            "SELECT {ALERT_COLS} FROM histviz_alerts \
             WHERE provider = $1 AND asset_type = $2 AND ticker = $3 AND timeframe = $4 \
             ORDER BY created_at DESC"
        ),
        None => format!("SELECT {ALERT_COLS} FROM histviz_alerts ORDER BY created_at DESC"),
    };
    let q = sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql));
    Ok(match instrument {
        Some((p, a, t, tf)) => q.bind(p).bind(a).bind(t).bind(tf).fetch_all(pool).await?,
        None => q.fetch_all(pool).await?,
    })
}

pub async fn get_alert(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Alert>> {
    let sql = format!("SELECT {ALERT_COLS} FROM histviz_alerts WHERE id = $1");
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

pub async fn create_alert(pool: &PgPool, a: AlertInput<'_>) -> anyhow::Result<Alert> {
    let sql = format!(
        "INSERT INTO histviz_alerts \
           (name, provider, asset_type, ticker, timeframe, connector_id, kind, source, \
            indicator_id, op, value, channels, repeat, cooldown_secs, enabled) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) \
         RETURNING {ALERT_COLS}"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(a.name)
        .bind(a.provider)
        .bind(a.asset_type)
        .bind(a.ticker)
        .bind(a.timeframe)
        .bind(a.connector_id)
        .bind(a.kind)
        .bind(a.source)
        .bind(a.indicator_id)
        .bind(a.op)
        .bind(a.value)
        .bind(a.channels)
        .bind(a.repeat)
        .bind(a.cooldown_secs)
        .bind(a.enabled)
        .fetch_one(pool)
        .await
        .context("creating chart alert")?)
}

/// Replace an alert's settings. Re-arming it (enabling one that fired) clears the error and
/// the bar cursor, so the next closed bar is judged fresh rather than skipped for being the
/// one it already saw.
pub async fn update_alert(
    pool: &PgPool,
    id: Uuid,
    a: AlertInput<'_>,
) -> anyhow::Result<Option<Alert>> {
    let sql = format!(
        "UPDATE histviz_alerts SET \
           name = $2, provider = $3, asset_type = $4, ticker = $5, timeframe = $6, \
           connector_id = $7, kind = $8, source = $9, indicator_id = $10, op = $11, \
           value = $12, channels = $13, repeat = $14, cooldown_secs = $15, enabled = $16, \
           last_bar_ts = CASE WHEN $16 AND NOT enabled THEN NULL ELSE last_bar_ts END, \
           last_error = CASE WHEN $16 AND NOT enabled THEN '' ELSE last_error END, \
           updated_at = now() \
         WHERE id = $1 RETURNING {ALERT_COLS}"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(a.name)
        .bind(a.provider)
        .bind(a.asset_type)
        .bind(a.ticker)
        .bind(a.timeframe)
        .bind(a.connector_id)
        .bind(a.kind)
        .bind(a.source)
        .bind(a.indicator_id)
        .bind(a.op)
        .bind(a.value)
        .bind(a.channels)
        .bind(a.repeat)
        .bind(a.cooldown_secs)
        .bind(a.enabled)
        .fetch_optional(pool)
        .await
        .context("saving chart alert")?)
}

pub async fn delete_alert(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histviz_alerts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Alerts worth evaluating now: enabled, and not checked since `stale`.
///
/// The cadence is the caller's (it knows the timeframe), and the check timestamp is written
/// even when nothing fires, so an instrument nobody is watching costs one row read a minute
/// and not a provider request.
pub async fn due_alerts(
    pool: &PgPool,
    stale: OffsetDateTime,
) -> anyhow::Result<Vec<Alert>> {
    let sql = format!(
        "SELECT {ALERT_COLS} FROM histviz_alerts \
         WHERE enabled AND (checked_at IS NULL OR checked_at <= $1) \
         ORDER BY checked_at NULLS FIRST"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(stale)
        .fetch_all(pool)
        .await?)
}

/// Record an evaluation that ran: what it saw, and the bar it judged.
pub async fn mark_checked(
    pool: &PgPool,
    id: Uuid,
    bar_ts: Option<OffsetDateTime>,
    value: Option<f64>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histviz_alerts \
         SET checked_at = now(), last_bar_ts = COALESCE($2, last_bar_ts), \
             last_value = COALESCE($3, last_value), last_error = '' \
         WHERE id = $1",
    )
    .bind(id)
    .bind(bar_ts)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record why an evaluation could not run. It stays enabled: a quota that resets or a
/// gateway that comes back should not need the user to re-arm anything.
pub async fn mark_error(pool: &PgPool, id: Uuid, message: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE histviz_alerts SET checked_at = now(), last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(message)
        .execute(pool)
        .await?;
    Ok(())
}

/// Record a fire. A one-shot alert disables itself here rather than being deleted: the row is
/// the record that it happened, and re-arming it is one click.
pub async fn record_fire(
    pool: &PgPool,
    id: Uuid,
    value: f64,
    bar_ts: OffsetDateTime,
    repeat: bool,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histviz_alerts \
         SET last_fired_at = now(), last_value = $2, last_bar_ts = $3, checked_at = now(), \
             fire_count = fire_count + 1, last_error = '', enabled = $4 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(value)
    .bind(bar_ts)
    .bind(repeat)
    .execute(pool)
    .await?;
    Ok(())
}
