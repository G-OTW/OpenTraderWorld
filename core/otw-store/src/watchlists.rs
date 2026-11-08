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
    /// Table columns shown, ids from the API's fixed catalog (migration 0190).
    pub columns: Vec<String>,
    /// News items published after this count on the row badges (migration 0192).
    #[serde(with = "ts")]
    pub news_seen_at: OffsetDateTime,
    /// Daily movers digest: 'off' | 'daily', the UTC hour it goes out at or after, the
    /// channels (empty = every channel granted to Watchlists) and the once-a-day gate.
    pub digest: String,
    pub digest_hour: i16,
    pub digest_channels: Vec<Uuid>,
    #[serde(with = "ts_opt")]
    pub digest_last_at: Option<OffsetDateTime>,
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
    /// Group inside the list; None = ungrouped (migration 0191).
    pub section: Option<String>,
    /// One colour of [`FLAGS`], or None.
    pub flag: Option<String>,
    pub target_buy: Option<f64>,
    pub target_sell: Option<f64>,
    /// Virtual position: "if I had bought `virt_qty` at `virt_price` on `virt_date`".
    pub virt_qty: Option<f64>,
    pub virt_price: Option<f64>,
    #[serde(with = "date_opt")]
    pub virt_date: Option<time::Date>,
}

/// The flag palette a row can carry (mirrors the CHECK in migration 0191).
pub const FLAGS: &[&str] = &["red", "orange", "yellow", "green", "blue", "purple"];

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

mod date_opt {
    use serde::Serializer;
    pub fn serialize<S: Serializer>(d: &Option<time::Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.to_string()),
            None => s.serialize_none(),
        }
    }
}

const WL_COLS: &str =
    "id, name, description, sync_enabled, refresh_secs, connector_id, position, refreshed_at, created_at, updated_at, \
     columns, news_seen_at, digest, digest_hour, digest_channels, digest_last_at";
const ITEM_COLS: &str =
    "id, watchlist_id, asset_class, provider, provider_id, symbol, name, exchange, notes, quote_ticker, quote_source, position, quote, quoted_at, \
     section, flag, target_buy, target_sell, virt_qty, virt_price, virt_date";

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

// ── List view settings ────────────────────────────────────────────────────────

/// Replace the columns a list shows. Ids are validated by the API against its catalog.
pub async fn set_columns(pool: &PgPool, id: Uuid, columns: &[String]) -> anyhow::Result<()> {
    sqlx::query("UPDATE watchlists SET columns = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(columns)
        .execute(pool)
        .await
        .context("setting watchlist columns")?;
    Ok(())
}

/// Replace the daily movers digest settings ('off' | 'daily', UTC hour, channels).
pub async fn set_digest(
    pool: &PgPool,
    id: Uuid,
    digest: &str,
    hour: i16,
    channels: &[Uuid],
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE watchlists SET digest = $2, digest_hour = $3, digest_channels = $4, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(digest)
    .bind(hour)
    .bind(channels)
    .execute(pool)
    .await
    .context("setting watchlist digest")?;
    Ok(())
}

/// Every list with the daily digest on. The once-a-day gate is decided in otw-core.
pub async fn list_digest_on(pool: &PgPool) -> anyhow::Result<Vec<Watchlist>> {
    let sql = format!("SELECT {WL_COLS} FROM watchlists WHERE digest = 'daily' ORDER BY position");
    Ok(sqlx::query_as::<_, Watchlist>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing digest watchlists")?)
}

pub async fn mark_digest_sent(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE watchlists SET digest_last_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("marking digest sent")?;
    Ok(())
}

// ── Item fields set on a row ──────────────────────────────────────────────────

/// A patch of the row fields from migration 0191. Outer `None` = untouched, `Some(None)`
/// = cleared. Validated in the API handler.
#[derive(Debug, Default, Clone)]
pub struct ItemFields {
    pub section: Option<Option<String>>,
    pub flag: Option<Option<String>>,
    pub target_buy: Option<Option<f64>>,
    pub target_sell: Option<Option<f64>>,
    pub virt_qty: Option<Option<f64>>,
    pub virt_price: Option<Option<f64>>,
    pub virt_date: Option<Option<time::Date>>,
}

impl ItemFields {
    pub fn is_empty(&self) -> bool {
        self.section.is_none()
            && self.flag.is_none()
            && self.target_buy.is_none()
            && self.target_sell.is_none()
            && self.virt_qty.is_none()
            && self.virt_price.is_none()
            && self.virt_date.is_none()
    }
}

pub async fn update_item_fields(
    pool: &PgPool,
    id: Uuid,
    f: &ItemFields,
) -> anyhow::Result<Option<Item>> {
    let sql = format!(
        "UPDATE watchlist_items SET \
         section = CASE WHEN $2 THEN $3 ELSE section END, \
         flag = CASE WHEN $4 THEN $5 ELSE flag END, \
         target_buy = CASE WHEN $6 THEN $7 ELSE target_buy END, \
         target_sell = CASE WHEN $8 THEN $9 ELSE target_sell END, \
         virt_qty = CASE WHEN $10 THEN $11 ELSE virt_qty END, \
         virt_price = CASE WHEN $12 THEN $13 ELSE virt_price END, \
         virt_date = CASE WHEN $14 THEN $15 ELSE virt_date END \
         WHERE id = $1 RETURNING {ITEM_COLS}"
    );
    Ok(sqlx::query_as::<_, Item>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(f.section.is_some())
        .bind(f.section.clone().flatten())
        .bind(f.flag.is_some())
        .bind(f.flag.clone().flatten())
        .bind(f.target_buy.is_some())
        .bind(f.target_buy.flatten())
        .bind(f.target_sell.is_some())
        .bind(f.target_sell.flatten())
        .bind(f.virt_qty.is_some())
        .bind(f.virt_qty.flatten())
        .bind(f.virt_price.is_some())
        .bind(f.virt_price.flatten())
        .bind(f.virt_date.is_some())
        .bind(f.virt_date.flatten())
        .fetch_optional(pool)
        .await
        .context("updating watchlist item fields")?)
}

/// Rewrite a list's order and sections in one transaction: `rows` is the full display
/// order, each id with its section. Position becomes the index, so ties cannot happen.
/// The caller checks the ids belong to the list ([`check_reorder`]).
pub async fn reorder(
    pool: &PgPool,
    watchlist_id: Uuid,
    rows: &[(Uuid, Option<String>)],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await.context("starting reorder")?;
    for (i, (id, section)) in rows.iter().enumerate() {
        sqlx::query(
            "UPDATE watchlist_items SET position = $3, section = $4 \
             WHERE id = $1 AND watchlist_id = $2",
        )
        .bind(id)
        .bind(watchlist_id)
        .bind((i + 1) as f64)
        .bind(section)
        .execute(&mut *tx)
        .await
        .context("reordering watchlist item")?;
    }
    tx.commit().await.context("committing reorder")?;
    Ok(())
}

/// A reorder payload must name every item of the list exactly once: a partial one would
/// leave the rest at stale positions that collide with the new ones.
pub fn check_reorder(existing: &[Uuid], rows: &[(Uuid, Option<String>)]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for (id, _) in rows {
        if !existing.contains(id) {
            return Err(format!("item {id} is not on this list"));
        }
        if !seen.insert(*id) {
            return Err(format!("item {id} is listed twice"));
        }
    }
    if seen.len() != existing.len() {
        return Err("the new order must list every item of the list".into());
    }
    Ok(())
}

/// Trimmed section name; empty means ungrouped.
pub fn normalize_section(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().take(60).collect())
}

/// Rename a section across the list (an empty target ungroups its rows). Returns rows moved.
pub async fn rename_section(
    pool: &PgPool,
    watchlist_id: Uuid,
    from: &str,
    to: Option<&str>,
) -> anyhow::Result<u64> {
    let res = sqlx::query(
        "UPDATE watchlist_items SET section = $3 WHERE watchlist_id = $1 AND section = $2",
    )
    .bind(watchlist_id)
    .bind(from)
    .bind(to)
    .execute(pool)
    .await
    .context("renaming watchlist section")?;
    Ok(res.rows_affected())
}

// ── News badge (read only over the News & Research items) ─────────────────────

/// Move the list's news reset point to now.
pub async fn mark_news_seen(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<OffsetDateTime>> {
    Ok(sqlx::query_scalar::<_, OffsetDateTime>(
        "UPDATE watchlists SET news_seen_at = now() WHERE id = $1 RETURNING news_seen_at",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("marking watchlist news seen")?)
}

/// Escape a literal for a PostgreSQL regular expression.
fn re_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Legal-form words dropped from a company name before it is matched in a headline:
/// "Apple Inc." is written "Apple" in the news.
const NAME_SUFFIXES: &[&str] = &[
    "inc", "inc.", "incorporated", "corp", "corp.", "corporation", "co", "co.", "company",
    "ltd", "ltd.", "limited", "plc", "n.v.", "nv", "s.a.", "sa", "ag", "se", "holding",
    "holdings", "group", "trust", "etf", "the",
];

/// The two title patterns one row counts on: the ticker as a whole word (case-sensitive,
/// tickers are written in capitals) and the name without its legal form (case-insensitive,
/// whole words; empty when nothing distinctive is left). A one-letter ticker is not matched
/// on its own: "A" is in every headline.
pub fn news_patterns(symbol: &str, name: &str) -> (String, String) {
    let sym = symbol.trim();
    let sym_re = if sym.chars().count() >= 2 {
        format!("\\m{}\\M", re_escape(sym))
    } else {
        String::new()
    };
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .filter(|w| !NAME_SUFFIXES.contains(&w.to_lowercase().as_str()))
        .collect();
    let core = words.join(" ");
    let name_re = if core.chars().count() >= 3 && !core.eq_ignore_ascii_case(sym) {
        format!("\\m{}\\M", re_escape(&core))
    } else {
        String::new()
    };
    (sym_re, name_re)
}

/// News items published (or fetched, when undated) after `since` whose title matches each
/// row's patterns. One query for the whole list; rows with no match count 0.
pub async fn news_counts(
    pool: &PgPool,
    rows: &[(Uuid, String, String)],
    since: OffsetDateTime,
) -> anyhow::Result<Vec<(Uuid, i64)>> {
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    let sym: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
    let name: Vec<String> = rows.iter().map(|r| r.2.clone()).collect();
    Ok(sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT k.id, COUNT(f.id) \
         FROM unnest($1::uuid[], $2::text[], $3::text[]) AS k(id, sym_re, name_re) \
         LEFT JOIN feed_items f \
           ON COALESCE(f.published_at, f.fetched_at) > $4 \
          AND ((k.sym_re <> '' AND f.title ~ k.sym_re) \
            OR (k.name_re <> '' AND f.title ~* k.name_re)) \
         GROUP BY k.id",
    )
    .bind(&ids)
    .bind(&sym)
    .bind(&name)
    .bind(since)
    .fetch_all(pool)
    .await
    .context("counting watchlist news")?)
}

// ── Events (read only over stored Fundamentals snapshots) ─────────────────────

/// Stored Fundamentals snapshots that can carry a dated event: per ticker `earnings`
/// (`next`) and `dividends` (ex-dates, splits), plus the market-wide `calendar` (`_`).
/// Read only: nothing here triggers a fetch, so an empty answer means "never fetched".
pub async fn event_snapshots(
    pool: &PgPool,
    subjects: &[String],
) -> anyhow::Result<Vec<(String, String, String, serde_json::Value)>> {
    Ok(sqlx::query_as::<_, (String, String, String, serde_json::Value)>(
        "SELECT subject, dataset, provider, data FROM fund_snapshots \
         WHERE (subject = ANY($1) AND dataset IN ('earnings', 'dividends')) \
            OR (subject = '_' AND dataset = 'calendar')",
    )
    .bind(subjects)
    .fetch_all(pool)
    .await
    .context("reading event snapshots")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn news_patterns_match_the_ticker_and_the_bare_name() {
        let (s, n) = news_patterns("AAPL", "Apple Inc.");
        assert_eq!(s, "\\mAAPL\\M");
        assert_eq!(n, "\\mApple\\M");
        let (_, n) = news_patterns("META", "Meta Platforms, Inc.");
        assert_eq!(n, "\\mMeta Platforms\\M");
        // A name that is the ticker again adds nothing; a one-letter ticker is not matched.
        assert_eq!(news_patterns("BTC", "BTC").1, "");
        assert_eq!(news_patterns("A", "Agilent Technologies Inc.").0, "");
    }

    #[test]
    fn news_patterns_escape_regex_characters() {
        let (s, _) = news_patterns("BRK.B", "Berkshire Hathaway Inc.");
        assert_eq!(s, "\\mBRK\\.B\\M");
        let (_, n) = news_patterns("XYZ", "X (Y) + Z");
        assert_eq!(n, "\\mX \\(Y\\) \\+ Z\\M");
    }

    #[test]
    fn reorder_must_list_every_item_once() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let ok = [(b, Some("core".into())), (a, None)];
        assert!(check_reorder(&[a, b], &ok).is_ok());
        assert!(check_reorder(&[a, b], &[(a, None)]).is_err());
        assert!(check_reorder(&[a, b], &[(a, None), (a, None)]).is_err());
        assert!(check_reorder(&[a], &[(a, None), (b, None)]).is_err());
    }

    #[test]
    fn sections_are_trimmed_and_empty_means_ungrouped() {
        assert_eq!(normalize_section(Some("  core ")), Some("core".into()));
        assert_eq!(normalize_section(Some("   ")), None);
        assert_eq!(normalize_section(None), None);
    }

    async fn scratch_pool() -> Option<PgPool> {
        let url = std::env::var("OTW_TEST_DATABASE_URL").ok()?;
        Some(crate::connect_and_migrate(&url).await.unwrap())
    }

    /// Over a scratch database (`OTW_TEST_DATABASE_URL`, skipped without it): a reorder
    /// rewrites positions and sections, the news badge counts a matching headline published
    /// after the reset point, and marking the list read brings it back to zero.
    #[tokio::test]
    async fn reorder_and_news_count_round_trip() {
        let Some(pool) = scratch_pool().await else { return };
        let wl = create_watchlist(&pool, "test watchlist", "").await.unwrap();
        let a = add_item(&pool, wl.id, "stock", "yahoo", "ZZTA", "ZZTA", "Zztest Alpha Inc.").await.unwrap();
        let b = add_item(&pool, wl.id, "stock", "yahoo", "ZZTB", "ZZTB", "").await.unwrap();
        reorder(&pool, wl.id, &[(b.id, Some("core".into())), (a.id, None)]).await.unwrap();
        let items = list_items(&pool, wl.id).await.unwrap();
        assert_eq!(items[0].id, b.id);
        assert_eq!(items[0].section.as_deref(), Some("core"));
        assert_eq!(items[1].section, None);

        let feed = Uuid::new_v4();
        sqlx::query("INSERT INTO feeds (id, name, kind) VALUES ($1, 'test feed', 'rss')")
            .bind(feed)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO feed_items (id, feed_id, dedup_key, title, published_at) \
             VALUES ($1, $2, 'k1', 'Zztest Alpha beats estimates', now() + interval '1 minute')",
        )
        .bind(Uuid::new_v4())
        .bind(feed)
        .execute(&pool)
        .await
        .unwrap();
        let rows: Vec<(Uuid, String, String)> = items
            .iter()
            .map(|i| {
                let (s, n) = news_patterns(&i.symbol, &i.name);
                (i.id, s, n)
            })
            .collect();
        let counts: std::collections::HashMap<Uuid, i64> =
            news_counts(&pool, &rows, wl.news_seen_at).await.unwrap().into_iter().collect();
        assert_eq!(counts[&a.id], 1);
        assert_eq!(counts[&b.id], 0);
        // Reset point after the headline: nothing new left.
        let later = OffsetDateTime::now_utc() + time::Duration::minutes(2);
        let counts: std::collections::HashMap<Uuid, i64> =
            news_counts(&pool, &rows, later).await.unwrap().into_iter().collect();
        assert_eq!(counts[&a.id], 0);
        assert!(mark_news_seen(&pool, wl.id).await.unwrap().is_some());

        // Row fields, columns and digest settings round-trip through their own writes.
        let f = ItemFields {
            flag: Some(Some("green".into())),
            target_buy: Some(Some(90.0)),
            virt_date: Some(Some(time::macros::date!(2026 - 01 - 02))),
            ..ItemFields::default()
        };
        let it = update_item_fields(&pool, a.id, &f).await.unwrap().unwrap();
        assert_eq!(it.flag.as_deref(), Some("green"));
        assert_eq!(it.target_buy, Some(90.0));
        assert_eq!(serde_json::to_value(&it).unwrap()["virt_date"], "2026-01-02");
        let it = update_item_fields(&pool, a.id, &ItemFields { flag: Some(None), ..ItemFields::default() })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(it.flag, None);
        assert_eq!(it.target_buy, Some(90.0));
        assert!(update_item_fields(&pool, a.id, &ItemFields { flag: Some(Some("pink".into())), ..ItemFields::default() })
            .await
            .is_err());
        set_columns(&pool, wl.id, &["c24".into(), "rsi".into()]).await.unwrap();
        set_digest(&pool, wl.id, "daily", 7, &[]).await.unwrap();
        let back = get_watchlist(&pool, wl.id).await.unwrap().unwrap();
        assert_eq!(back.columns, vec!["c24", "rsi"]);
        assert!(list_digest_on(&pool).await.unwrap().iter().any(|w| w.id == wl.id && w.digest_hour == 7));
        assert!(event_snapshots(&pool, &["ZZTA".into()]).await.is_ok());

        sqlx::query("DELETE FROM feeds WHERE id = $1").bind(feed).execute(&pool).await.unwrap();
        delete_watchlist(&pool, wl.id).await.unwrap();
    }
}
