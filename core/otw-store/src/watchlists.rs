//! Storage for the Watchlists module.
//!
//! Watchlists hold items (pinned provider symbols, same resolution scheme as the Portfolio
//! Tracker: CoinGecko coin id / Yahoo ticker). Each item caches its latest computed quote as
//! JSONB (`quote` + `quoted_at`); the quote itself is produced in otw-core from a rolling
//! provider fetch. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Watchlist {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub sync_enabled: bool,
    pub refresh_secs: i32,
    /// Optional Historical Data connector quotes are fetched through. NULL = the default
    /// reconciliation scheme (CoinGecko / Yahoo, same as the Portfolio Tracker).
    pub connector_id: Option<Uuid>,
    pub position: f64,
    #[serde(with = "ts_opt")]
    pub refreshed_at: Option<OffsetDateTime>,
    #[serde(with = "ts")]
    pub created_at: OffsetDateTime,
    #[serde(with = "ts")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Item {
    pub id: Uuid,
    pub watchlist_id: Uuid,
    pub asset_class: String,
    pub provider: String,
    pub provider_id: String,
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    pub notes: String,
    /// Ticker in the list connector's own symbol format; empty = derived from `symbol`.
    pub quote_ticker: String,
    /// Quote source override: '' = follow the list default, 'auto' = force the default
    /// providers (CoinGecko / Yahoo), else a Historical Data connector id.
    pub quote_source: String,
    pub position: f64,
    /// Cached quote: { price_usd, change_24h, change_3d, change_7d, change_30d, spark, … }.
    pub quote: serde_json::Value,
    #[serde(with = "ts_opt")]
    pub quoted_at: Option<OffsetDateTime>,
}

// ── Serde helpers ─────────────────────────────────────────────────────────────

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

const WL_COLS: &str =
    "id, name, description, sync_enabled, refresh_secs, connector_id, position, refreshed_at, created_at, updated_at";
const ITEM_COLS: &str =
    "id, watchlist_id, asset_class, provider, provider_id, symbol, name, exchange, notes, quote_ticker, quote_source, position, quote, quoted_at";

// ── Watchlists CRUD ───────────────────────────────────────────────────────────

pub async fn list_watchlists(pool: &PgPool) -> anyhow::Result<Vec<Watchlist>> {
    let sql = format!("SELECT {WL_COLS} FROM watchlists ORDER BY position, created_at");
    Ok(sqlx::query_as::<_, Watchlist>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing watchlists")?)
}

/// Item counts per watchlist, for the list rail badges (one grouped query).
pub async fn item_counts(pool: &PgPool) -> anyhow::Result<Vec<(Uuid, i64)>> {
    Ok(sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT watchlist_id, COUNT(*) FROM watchlist_items GROUP BY watchlist_id",
    )
    .fetch_all(pool)
    .await
    .context("counting watchlist items")?)
}

pub async fn get_watchlist(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Watchlist>> {
    let sql = format!("SELECT {WL_COLS} FROM watchlists WHERE id = $1");
    Ok(sqlx::query_as::<_, Watchlist>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching watchlist")?)
}

pub async fn create_watchlist(
    pool: &PgPool,
    name: &str,
    description: &str,
) -> anyhow::Result<Watchlist> {
    let sql = format!(
        "INSERT INTO watchlists (id, name, description, position) \
         VALUES ($1,$2,$3, (SELECT COALESCE(MAX(position),0)+1 FROM watchlists)) \
         RETURNING {WL_COLS}"
    );
    Ok(sqlx::query_as::<_, Watchlist>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(description)
        .fetch_one(pool)
        .await
        .context("creating watchlist")?)
}

/// Patch mutable watchlist fields. Any `None` is left unchanged. `connector` is a
/// double option: outer None = untouched, `Some(None)` = back to the default providers.
pub async fn update_watchlist(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    sync_enabled: Option<bool>,
    refresh_secs: Option<i32>,
    connector: Option<Option<Uuid>>,
) -> anyhow::Result<Option<Watchlist>> {
    let sql = format!(
        "UPDATE watchlists SET \
         name = COALESCE($2, name), \
         description = COALESCE($3, description), \
         sync_enabled = COALESCE($4, sync_enabled), \
         refresh_secs = COALESCE($5, refresh_secs), \
         connector_id = CASE WHEN $6 THEN $7 ELSE connector_id END, \
         updated_at = now() \
         WHERE id = $1 RETURNING {WL_COLS}"
    );
    Ok(sqlx::query_as::<_, Watchlist>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(sync_enabled)
        .bind(refresh_secs)
        .bind(connector.is_some())
        .bind(connector.flatten())
        .fetch_optional(pool)
        .await
        .context("updating watchlist")?)
}

pub async fn delete_watchlist(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM watchlists WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting watchlist")?;
    Ok(res.rows_affected() > 0)
}

/// Sync-enabled watchlists whose last refresh is older than their own interval (or never ran).
pub async fn list_due(pool: &PgPool) -> anyhow::Result<Vec<Watchlist>> {
    let sql = format!(
        "SELECT {WL_COLS} FROM watchlists \
         WHERE sync_enabled AND (refreshed_at IS NULL \
               OR refreshed_at < now() - refresh_secs * interval '1 second') \
         ORDER BY position"
    );
    Ok(sqlx::query_as::<_, Watchlist>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing due watchlists")?)
}

pub async fn mark_refreshed(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE watchlists SET refreshed_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("marking watchlist refreshed")?;
    Ok(())
}

// ── Items ─────────────────────────────────────────────────────────────────────

pub async fn list_items(pool: &PgPool, watchlist_id: Uuid) -> anyhow::Result<Vec<Item>> {
    let sql = format!(
        "SELECT {ITEM_COLS} FROM watchlist_items WHERE watchlist_id = $1 ORDER BY position, symbol"
    );
    Ok(sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(sql))
        .bind(watchlist_id)
        .fetch_all(pool)
        .await
        .context("listing watchlist items")?)
}

pub async fn get_item(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Item>> {
    let sql = format!("SELECT {ITEM_COLS} FROM watchlist_items WHERE id = $1");
    Ok(sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching watchlist item")?)
}

/// Add an item (idempotent on the provider symbol: re-adding refreshes the display labels).
pub async fn add_item(
    pool: &PgPool,
    watchlist_id: Uuid,
    asset_class: &str,
    provider: &str,
    provider_id: &str,
    symbol: &str,
    name: &str,
) -> anyhow::Result<Item> {
    let sql = format!(
        "INSERT INTO watchlist_items (id, watchlist_id, asset_class, provider, provider_id, symbol, name, position) \
         VALUES ($1,$2,$3,$4,$5,$6,$7, \
                 (SELECT COALESCE(MAX(position),0)+1 FROM watchlist_items WHERE watchlist_id = $2)) \
         ON CONFLICT (watchlist_id, provider, provider_id) \
         DO UPDATE SET symbol = EXCLUDED.symbol, name = EXCLUDED.name \
         RETURNING {ITEM_COLS}"
    );
    Ok(sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::new_v4())
        .bind(watchlist_id)
        .bind(asset_class)
        .bind(provider)
        .bind(provider_id)
        .bind(symbol)
        .bind(name)
        .fetch_one(pool)
        .await
        .context("adding watchlist item")?)
}

/// Patch an item's user-editable fields. Any `None` is left unchanged.
pub async fn update_item(
    pool: &PgPool,
    id: Uuid,
    notes: Option<&str>,
    position: Option<f64>,
    quote_ticker: Option<&str>,
    quote_source: Option<&str>,
) -> anyhow::Result<Option<Item>> {
    let sql = format!(
        "UPDATE watchlist_items SET \
         notes = COALESCE($2, notes), \
         position = COALESCE($3, position), \
         quote_ticker = COALESCE($4, quote_ticker), \
         quote_source = COALESCE($5, quote_source) \
         WHERE id = $1 RETURNING {ITEM_COLS}"
    );
    Ok(sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(notes)
        .bind(position)
        .bind(quote_ticker)
        .bind(quote_source)
        .fetch_optional(pool)
        .await
        .context("updating watchlist item")?)
}

pub async fn delete_item(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM watchlist_items WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting watchlist item")?;
    Ok(res.rows_affected() > 0)
}

/// Store a freshly computed quote for an item (and the exchange label when discovered).
pub async fn set_item_quote(
    pool: &PgPool,
    id: Uuid,
    quote: &serde_json::Value,
    exchange: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE watchlist_items SET quote = $2, quoted_at = now(), \
         exchange = COALESCE($3, exchange) WHERE id = $1",
    )
    .bind(id)
    .bind(quote)
    .bind(exchange)
    .execute(pool)
    .await
    .context("setting item quote")?;
    Ok(())
}

/// Record why an item's quote failed, on top of the last good quote. A later successful
/// quote replaces the whole JSONB and thereby clears the error. `quoted_at` is left alone
/// so the staleness of the shown price stays honest.
pub async fn set_item_error(pool: &PgPool, id: Uuid, message: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE watchlist_items SET quote = quote || jsonb_build_object('error', $2::text) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(message)
    .execute(pool)
    .await
    .context("setting item quote error")?;
    Ok(())
}
