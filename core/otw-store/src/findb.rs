//! Storage for the FinanceDatabase module ("findb").
//!
//! Read-only catalog of ~300k instruments (bulk-loaded on install) + a favorites system
//! organized in folders. Search uses pg_trgm for fuzzy symbol/name matching with optional
//! asset-type faceting. Single-user: no owner scoping.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Instruments (catalog + search)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Instrument {
    pub id: i64,
    pub asset_type: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub exchange: String,
    pub summary: String,
    pub sector: String,
    pub industry: String,
    pub country: String,
    pub market_cap: String,
    pub isin: String,
    pub category: String,
    pub family: String,
}

const INSTR_COLS: &str = "id, asset_type, symbol, name, currency, exchange, summary, \
     sector, industry, country, market_cap, isin, category, family";

/// Whether the catalog has been imported (version marker is non-empty).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Meta {
    pub version: String,
    pub count: i64,
    pub installed: bool,
}

pub async fn get_meta(pool: &PgPool) -> anyhow::Result<Meta> {
    let row: (String, i64) =
        sqlx::query_as("SELECT version, count FROM findb_meta WHERE id = TRUE")
            .fetch_one(pool)
            .await
            .context("fetching findb meta")?;
    Ok(Meta {
        installed: !row.0.is_empty(),
        version: row.0,
        count: row.1,
    })
}

pub async fn set_meta(pool: &PgPool, version: &str, count: i64) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE findb_meta SET version = $1, count = $2, updated_at = now() WHERE id = TRUE",
    )
    .bind(version)
    .bind(count)
    .execute(pool)
    .await
    .context("updating findb meta")?;
    Ok(())
}

/// Filter facets a search can be narrowed by. Empty/None = no constraint on that column.
#[derive(Debug, Default, Deserialize)]
pub struct SearchFilters {
    pub asset_type: Option<String>,
    pub exchange: Option<String>,
    pub currency: Option<String>,
    pub country: Option<String>,
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub category: Option<String>,
    pub family: Option<String>,
}

impl SearchFilters {
    /// (column, value) pairs that are actually set, for building the WHERE clause.
    fn active(&self) -> Vec<(&'static str, &str)> {
        let mut out = Vec::new();
        macro_rules! add {
            ($col:literal, $field:expr) => {
                if let Some(s) = $field.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    out.push(($col, s));
                }
            };
        }
        add!("asset_type", self.asset_type);
        add!("exchange", self.exchange);
        add!("currency", self.currency);
        add!("country", self.country);
        add!("sector", self.sector);
        add!("industry", self.industry);
        add!("category", self.category);
        add!("family", self.family);
        out
    }
}

/// How to order results. `Relevance` only applies when a query term is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    Relevance,
    Symbol,
    Name,
}

impl Default for SortBy {
    fn default() -> Self {
        SortBy::Relevance
    }
}

/// One page of search results plus whether more rows exist past `offset + len`.
#[derive(Debug, Serialize)]
pub struct SearchPage {
    pub results: Vec<Instrument>,
    pub has_more: bool,
}

/// Search/browse the catalog. `q` matches symbol/name (prefix + trigram); `filters` constrain
/// by exact facet values; results page via `offset`/`limit`. With an empty `q` this is a pure
/// faceted browse (so the user can list, e.g., every NASDAQ equity). `has_more` is computed by
/// fetching one extra row.
pub async fn search(
    pool: &PgPool,
    q: &str,
    filters: &SearchFilters,
    sort: SortBy,
    limit: i64,
    offset: i64,
) -> anyhow::Result<SearchPage> {
    let q = q.trim();
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);

    // Build the WHERE clause. $1=pattern, $2=upper, $3="upper%", $4=q (for similarity);
    // facet binds start at $5; limit/offset are the last two.
    let mut where_clauses: Vec<String> = Vec::new();
    if !q.is_empty() {
        where_clauses.push("(symbol ILIKE $1 OR name ILIKE $1)".into());
    }
    let active = filters.active();
    for (i, (col, _)) in active.iter().enumerate() {
        where_clauses.push(format!("{col} = ${}", 5 + i));
    }
    let where_sql = if where_clauses.is_empty() {
        "TRUE".to_string()
    } else {
        where_clauses.join(" AND ")
    };

    // ORDER BY: relevance ranking needs a query; with no q fall back to symbol order.
    let order_sql = match (sort, q.is_empty()) {
        (SortBy::Symbol, _) => "symbol, name".to_string(),
        (SortBy::Name, _) => "name, symbol".to_string(),
        (SortBy::Relevance, true) => "symbol, name".to_string(),
        (SortBy::Relevance, false) => {
            "(symbol = $2) DESC, (symbol ILIKE $3) DESC, similarity(name, $4) DESC, length(symbol), symbol".to_string()
        }
    };

    let n = active.len();
    let limit_param = 5 + n;
    let offset_param = 6 + n;
    let sql = format!(
        "SELECT {INSTR_COLS} FROM findb_instruments \
         WHERE {where_sql} ORDER BY {order_sql} LIMIT ${limit_param} OFFSET ${offset_param}"
    );

    let pattern = format!("%{q}%");
    let upper = q.to_uppercase();
    let mut query = sqlx::query_as::<_, Instrument>(sqlx::AssertSqlSafe(sql))
        .bind(&pattern) // $1
        .bind(&upper) // $2
        .bind(format!("{upper}%")) // $3
        .bind(q); // $4
    for (_, val) in &active {
        query = query.bind(*val);
    }
    // Fetch one extra row to detect has_more without a COUNT(*).
    let rows = query
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(pool)
        .await
        .context("searching findb")?;

    let has_more = rows.len() as i64 > limit;
    let results = rows.into_iter().take(limit as usize).collect();
    Ok(SearchPage { results, has_more })
}

/// Distinct values for a single facet column, optionally scoped to an asset type. Used to
/// populate the filter dropdowns from real data. Capped to keep the payload bounded.
pub async fn facet_values(
    pool: &PgPool,
    column: &str,
    asset_type: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    // Whitelist the column — it is interpolated as an identifier, so it must never come
    // straight from the request.
    const ALLOWED: &[&str] = &[
        "asset_type", "exchange", "currency", "country", "sector", "industry", "category",
        "family",
    ];
    if !ALLOWED.contains(&column) {
        anyhow::bail!("invalid facet column");
    }
    let sql = format!(
        "SELECT DISTINCT {column} AS v FROM findb_instruments \
         WHERE {column} <> '' AND ($1::text IS NULL OR asset_type = $1) \
         ORDER BY v LIMIT 500"
    );
    let rows: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(asset_type)
        .fetch_all(pool)
        .await
        .context("listing facet values")?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_instrument(pool: &PgPool, id: i64) -> anyhow::Result<Option<Instrument>> {
    let sql = format!("SELECT {INSTR_COLS} FROM findb_instruments WHERE id = $1");
    let row = sqlx::query_as::<_, Instrument>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching instrument")?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Folders
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct FolderInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
}

pub async fn list_folders(pool: &PgPool) -> anyhow::Result<Vec<Folder>> {
    let rows = sqlx::query_as::<_, Folder>(
        "SELECT id, name, color FROM findb_folders ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .context("listing folders")?;
    Ok(rows)
}

pub async fn add_folder(pool: &PgPool, input: &FolderInput) -> anyhow::Result<Folder> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO findb_folders (id, name, color) VALUES ($1,$2,$3)")
        .bind(id)
        .bind(&input.name)
        .bind(&input.color)
        .execute(pool)
        .await
        .context("inserting folder")?;
    Ok(Folder {
        id,
        name: input.name.clone(),
        color: input.color.clone(),
    })
}

pub async fn update_folder(pool: &PgPool, id: Uuid, input: &FolderInput) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE findb_folders SET name = $2, color = $3 WHERE id = $1")
        .bind(id)
        .bind(&input.name)
        .bind(&input.color)
        .execute(pool)
        .await
        .context("updating folder")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_folder(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM findb_folders WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

/// A favorite joined with its instrument, for list rendering.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Favorite {
    pub id: Uuid,
    pub instrument_id: i64,
    pub folder_id: Option<Uuid>,
    pub note: String,
    pub asset_type: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub exchange: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct FavoriteInput {
    pub instrument_id: i64,
    pub folder_id: Option<Uuid>,
    #[serde(default)]
    pub note: String,
}

pub async fn list_favorites(pool: &PgPool) -> anyhow::Result<Vec<Favorite>> {
    let rows = sqlx::query_as::<_, Favorite>(
        "SELECT f.id, f.instrument_id, f.folder_id, f.note, \
                i.asset_type, i.symbol, i.name, i.currency, i.exchange \
         FROM findb_favorites f \
         JOIN findb_instruments i ON i.id = f.instrument_id \
         ORDER BY f.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing favorites")?;
    Ok(rows)
}

pub async fn add_favorite(pool: &PgPool, input: &FavoriteInput) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    // Idempotent: re-favoriting updates the folder/note instead of erroring on the UNIQUE.
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO findb_favorites (id, instrument_id, folder_id, note) \
         VALUES ($1,$2,$3,$4) \
         ON CONFLICT (instrument_id) DO UPDATE SET folder_id = EXCLUDED.folder_id, note = EXCLUDED.note \
         RETURNING id",
    )
    .bind(id)
    .bind(input.instrument_id)
    .bind(input.folder_id)
    .bind(&input.note)
    .fetch_one(pool)
    .await
    .context("inserting favorite")?;
    Ok(row.0)
}

pub async fn update_favorite(pool: &PgPool, id: Uuid, input: &FavoriteInput) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE findb_favorites SET folder_id = $2, note = $3 WHERE id = $1")
        .bind(id)
        .bind(input.folder_id)
        .bind(&input.note)
        .execute(pool)
        .await
        .context("updating favorite")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_favorite(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM findb_favorites WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Bulk import (called by the otw-core importer)
// ---------------------------------------------------------------------------

/// A staged instrument row ready for bulk insert.
#[derive(Default)]
pub struct ImportRow {
    pub asset_type: String,
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub exchange: String,
    pub summary: String,
    pub sector: String,
    pub industry: String,
    pub country: String,
    pub market_cap: String,
    pub isin: String,
    pub category: String,
    pub family: String,
}

/// Clear the catalog before a fresh import (favorites cascade-delete).
pub async fn truncate_instruments(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query("TRUNCATE findb_instruments RESTART IDENTITY CASCADE")
        .execute(pool)
        .await
        .context("truncating findb_instruments")?;
    Ok(())
}

/// Insert one batch of instruments using a single multi-row INSERT.
pub async fn insert_batch(pool: &PgPool, rows: &[ImportRow]) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut sql = String::from(
        "INSERT INTO findb_instruments \
         (asset_type, symbol, name, currency, exchange, summary, sector, industry, \
          country, market_cap, isin, category, family) VALUES ",
    );
    let cols = 13;
    for i in 0..rows.len() {
        if i > 0 {
            sql.push(',');
        }
        sql.push('(');
        for c in 0..cols {
            if c > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("${}", i * cols + c + 1));
        }
        sql.push(')');
    }
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for r in rows {
        q = q
            .bind(&r.asset_type)
            .bind(&r.symbol)
            .bind(&r.name)
            .bind(&r.currency)
            .bind(&r.exchange)
            .bind(&r.summary)
            .bind(&r.sector)
            .bind(&r.industry)
            .bind(&r.country)
            .bind(&r.market_cap)
            .bind(&r.isin)
            .bind(&r.category)
            .bind(&r.family);
    }
    q.execute(pool).await.context("inserting findb batch")?;
    Ok(())
}
