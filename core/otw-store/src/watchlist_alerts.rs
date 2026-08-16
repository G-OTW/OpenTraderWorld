//! Price alerts on watchlist items, and the price samples a rolling alert needs.
//!
//! Persistence only — the condition is evaluated in otw-core (it owns the refresh loop and
//! the notification senders). See migration 0087 for what `metric` / `direction` / `basis`
//! mean; the short version is that `basis` decides what a "variation" is measured *from*:
//! `anchor` freezes the reference at arm time, `rolling` reads it back out of
//! `watchlist_samples` on every evaluation.
//!
//! Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

pub const METRICS: &[&str] = &["price", "pct", "usd"];
pub const BASES: &[&str] = &["anchor", "rolling"];

/// Directions valid for a given metric. A price threshold is crossed in one direction; a
/// variation can also be watched either way (`move`).
pub fn directions_for(metric: &str) -> &'static [&'static str] {
    match metric {
        "price" => &["above", "below"],
        _ => &["up", "down", "move"],
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Alert {
    pub id: Uuid,
    pub item_id: Uuid,
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    pub basis: String,
    pub window_secs: i32,
    pub ref_price: Option<f64>,
    #[serde(with = "ts")]
    pub armed_at: OffsetDateTime,
    #[serde(with = "ts_opt")]
    pub expires_at: Option<OffsetDateTime>,
    pub repeat: bool,
    pub cooldown_secs: i32,
    pub enabled: bool,
    /// Destination channels; empty = every enabled channel.
    pub channel_ids: Vec<Uuid>,
    pub note: String,
    #[serde(with = "ts_opt")]
    pub last_fired_at: Option<OffsetDateTime>,
    pub last_price: Option<f64>,
    pub fire_count: i32,
}

mod ts {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}
mod ts_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
        match t {
            Some(t) => s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

/// Create/update payload. Validated in the API handler, not here.
#[derive(Debug, Clone, Deserialize)]
pub struct AlertInput {
    pub metric: String,
    pub direction: String,
    pub threshold: f64,
    #[serde(default = "default_basis")]
    pub basis: String,
    #[serde(default)]
    pub window_secs: i32,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub channel_ids: Vec<Uuid>,
    #[serde(default)]
    pub note: String,
}

fn default_basis() -> String {
    "anchor".into()
}
fn default_cooldown() -> i32 {
    3600
}
fn default_true() -> bool {
    true
}

const COLS: &str = "id, item_id, metric, direction, threshold, basis, window_secs, ref_price, \
     armed_at, expires_at, repeat, cooldown_secs, enabled, channel_ids, note, last_fired_at, \
     last_price, fire_count";

/// Every alert on the items of one watchlist, newest first — the detail endpoint's payload.
pub async fn list_for_watchlist(pool: &PgPool, watchlist_id: Uuid) -> anyhow::Result<Vec<Alert>> {
    let sql = format!(
        "SELECT {COLS} FROM watchlist_alerts a \
         WHERE a.item_id IN (SELECT id FROM watchlist_items WHERE watchlist_id = $1) \
         ORDER BY a.created_at DESC"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(watchlist_id)
        .fetch_all(pool)
        .await
        .context("listing watchlist alerts")?)
}

pub async fn list_for_item(pool: &PgPool, item_id: Uuid) -> anyhow::Result<Vec<Alert>> {
    let sql = format!("SELECT {COLS} FROM watchlist_alerts WHERE item_id = $1 ORDER BY created_at DESC");
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(item_id)
        .fetch_all(pool)
        .await
        .context("listing item alerts")?)
}

pub async fn get(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Alert>> {
    let sql = format!("SELECT {COLS} FROM watchlist_alerts WHERE id = $1");
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching watchlist alert")?)
}

/// Arm a new alert. `ref_price` is the item's current quote — frozen now for an anchor
/// basis, which is exactly what makes "from now on" mean the moment the user clicked.
pub async fn add(
    pool: &PgPool,
    item_id: Uuid,
    input: &AlertInput,
    ref_price: Option<f64>,
) -> anyhow::Result<Alert> {
    let expires_at = anchor_deadline(input, OffsetDateTime::now_utc());
    let sql = format!(
        "INSERT INTO watchlist_alerts \
            (id, item_id, metric, direction, threshold, basis, window_secs, ref_price, \
             expires_at, repeat, cooldown_secs, enabled, channel_ids, note) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(item_id)
        .bind(&input.metric)
        .bind(&input.direction)
        .bind(input.threshold)
        .bind(&input.basis)
        .bind(input.window_secs)
        .bind(ref_price)
        .bind(expires_at)
        .bind(input.repeat)
        .bind(input.cooldown_secs)
        .bind(input.enabled)
        .bind(&input.channel_ids)
        .bind(input.note.trim())
        .fetch_one(pool)
        .await
        .context("creating watchlist alert")?)
}

/// Rewrite an alert. Editing re-arms it: the reference and the deadline are taken from
/// *now*, because an edited condition is a new question about the current price.
pub async fn update(
    pool: &PgPool,
    id: Uuid,
    input: &AlertInput,
    ref_price: Option<f64>,
) -> anyhow::Result<Option<Alert>> {
    let now = OffsetDateTime::now_utc();
    let expires_at = anchor_deadline(input, now);
    let sql = format!(
        "UPDATE watchlist_alerts SET \
         metric=$2, direction=$3, threshold=$4, basis=$5, window_secs=$6, ref_price=$7, \
         armed_at=$8, expires_at=$9, repeat=$10, cooldown_secs=$11, enabled=$12, \
         channel_ids=$13, note=$14, updated_at=now() \
         WHERE id=$1 RETURNING {COLS}"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&input.metric)
        .bind(&input.direction)
        .bind(input.threshold)
        .bind(&input.basis)
        .bind(input.window_secs)
        .bind(ref_price)
        .bind(now)
        .bind(expires_at)
        .bind(input.repeat)
        .bind(input.cooldown_secs)
        .bind(input.enabled)
        .bind(&input.channel_ids)
        .bind(input.note.trim())
        .fetch_optional(pool)
        .await
        .context("updating watchlist alert")?)
}

/// Only an anchor basis has a deadline; a rolling window is a look-back, not a lifetime.
fn anchor_deadline(input: &AlertInput, now: OffsetDateTime) -> Option<OffsetDateTime> {
    (input.basis == "anchor" && input.window_secs > 0)
        .then(|| now + time::Duration::seconds(input.window_secs as i64))
}

pub async fn delete(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM watchlist_alerts WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting watchlist alert")?
        .rows_affected();
    Ok(n > 0)
}

/// Enabled alerts for the items of one watchlist, as `(item_id, alert)` — what the refresh
/// loop evaluates. Expired anchor alerts are excluded and disabled by [`expire_stale`].
pub async fn live_for_watchlist(pool: &PgPool, watchlist_id: Uuid) -> anyhow::Result<Vec<Alert>> {
    let sql = format!(
        "SELECT {COLS} FROM watchlist_alerts a \
         WHERE a.enabled \
           AND a.item_id IN (SELECT id FROM watchlist_items WHERE watchlist_id = $1) \
           AND (a.expires_at IS NULL OR a.expires_at > now())"
    );
    Ok(sqlx::query_as::<_, Alert>(sqlx::AssertSqlSafe(sql))
        .bind(watchlist_id)
        .fetch_all(pool)
        .await
        .context("listing live watchlist alerts")?)
}

/// Disable anchor alerts whose window elapsed without the move ever happening. Returns how
/// many were retired — they stay listed (with `enabled = false`) so the user sees the
/// question was asked and answered "no", rather than silently vanishing.
pub async fn expire_stale(pool: &PgPool) -> anyhow::Result<u64> {
    Ok(sqlx::query(
        "UPDATE watchlist_alerts SET enabled = FALSE, updated_at = now() \
         WHERE enabled AND expires_at IS NOT NULL AND expires_at <= now()",
    )
    .execute(pool)
    .await
    .context("expiring stale watchlist alerts")?
    .rows_affected())
}

/// Record a fire. A one-shot alert disables itself; a repeating one re-anchors its
/// reference to the price that just fired, so the next move is measured from there.
pub async fn record_fire(pool: &PgPool, id: Uuid, price: f64, repeat: bool) -> anyhow::Result<()> {
    if repeat {
        sqlx::query(
            "UPDATE watchlist_alerts SET last_fired_at = now(), last_price = $2, \
             fire_count = fire_count + 1, ref_price = $2, armed_at = now(), \
             expires_at = CASE WHEN basis = 'anchor' AND window_secs > 0 \
                               THEN now() + window_secs * interval '1 second' ELSE NULL END, \
             updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(price)
        .execute(pool)
        .await
        .context("recording repeating alert fire")?;
    } else {
        sqlx::query(
            "UPDATE watchlist_alerts SET last_fired_at = now(), last_price = $2, \
             fire_count = fire_count + 1, enabled = FALSE, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(price)
        .execute(pool)
        .await
        .context("recording alert fire")?;
    }
    Ok(())
}

// ── Price samples (rolling windows only) ──────────────────────────────────────

/// Record one price point. Called per refresh, but only for items that actually carry an
/// enabled rolling alert — no alert, no history, no cost.
pub async fn add_sample(pool: &PgPool, item_id: Uuid, price: f64) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO watchlist_samples (item_id, ts, price) VALUES ($1, now(), $2) \
         ON CONFLICT (item_id, ts) DO NOTHING",
    )
    .bind(item_id)
    .bind(price)
    .execute(pool)
    .await
    .context("recording watchlist price sample")?;
    Ok(())
}

/// The price as it was around `secs` ago: the newest sample at or before that instant.
/// `None` when history doesn't reach back that far yet — the caller must not treat a
/// missing reference as "no move", so a young rolling alert simply stays quiet.
pub async fn sample_at(pool: &PgPool, item_id: Uuid, secs: i32) -> anyhow::Result<Option<f64>> {
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT price FROM watchlist_samples \
         WHERE item_id = $1 AND ts <= now() - $2 * interval '1 second' \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(item_id)
    .bind(secs)
    .fetch_optional(pool)
    .await
    .context("reading watchlist price sample")?;
    Ok(row.map(|r| r.0))
}

/// Drop samples older than the longest rolling window still watching each item (with a
/// margin so the look-back never lands just past the edge of what we kept). Samples for
/// items with no rolling alert left are dropped entirely.
pub async fn purge_samples(pool: &PgPool) -> anyhow::Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM watchlist_samples s USING ( \
             SELECT i.id, COALESCE(MAX(a.window_secs) FILTER ( \
                        WHERE a.enabled AND a.basis = 'rolling'), 0) AS keep \
             FROM watchlist_items i LEFT JOIN watchlist_alerts a ON a.item_id = i.id \
             GROUP BY i.id) w \
         WHERE s.item_id = w.id \
           AND (w.keep = 0 OR s.ts < now() - (w.keep * 2) * interval '1 second')",
    )
    .execute(pool)
    .await
    .context("purging watchlist price samples")?
    .rows_affected())
}
