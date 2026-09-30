//! Storage for the Managers' Portfolios module.
//!
//! A scheduled job scrapes Dataroma's superinvestor summaries into these tables; the API reads
//! only from here. A scrape replaces a portfolio's holdings wholesale (delete + reinsert) inside
//! a transaction, so a partial/failed portfolio never leaves a half-updated state. Distinct from
//! the future user "portfolios" module. Single-user: no owner scoping.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Portfolio {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub value_text: String,
    pub value_num: Option<f64>,
    pub stock_count: i32,
    pub period: String,
    pub source_url: String,
    #[serde(with = "ts")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Holding {
    pub position: i32,
    pub ticker: String,
    pub company: String,
    pub pct: Option<f64>,
    pub activity: String,
    pub shares: Option<f64>,
    pub reported_price: Option<f64>,
    pub value: Option<f64>,
    pub current_price: Option<f64>,
    pub change_pct: Option<f64>,
    pub week52_low: Option<f64>,
    pub week52_high: Option<f64>,
}

mod ts {
    use serde::Serializer;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    pub fn serialize<S: Serializer>(t: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.format(&Rfc3339).map_err(serde::ser::Error::custom)?)
    }
}

/// Plain (unkeyed) inputs the scraper produces before persistence.
#[derive(Debug, Clone, Default)]
pub struct PortfolioInput {
    pub slug: String,
    pub name: String,
    pub value_text: String,
    pub value_num: Option<f64>,
    pub stock_count: i32,
    pub period: String,
    pub source_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct HoldingInput {
    pub position: i32,
    pub ticker: String,
    pub company: String,
    pub pct: Option<f64>,
    pub activity: String,
    pub shares: Option<f64>,
    pub reported_price: Option<f64>,
    pub value: Option<f64>,
    pub current_price: Option<f64>,
    pub change_pct: Option<f64>,
    pub week52_low: Option<f64>,
    pub week52_high: Option<f64>,
}

const PORTFOLIO_COLS: &str =
    "id, slug, name, value_text, value_num, stock_count, period, source_url, updated_at";

/// List all portfolios. `q` filters by name (case-insensitive substring); `ticker` keeps only
/// portfolios that hold that ticker. Both are optional and combine (AND).
pub async fn list_portfolios(
    pool: &PgPool,
    q: Option<&str>,
    ticker: Option<&str>,
) -> anyhow::Result<Vec<Portfolio>> {
    let mut sql = format!("SELECT {PORTFOLIO_COLS} FROM manager_portfolios p");
    if ticker.is_some() {
        sql.push_str(
            " WHERE EXISTS (SELECT 1 FROM manager_holdings h \
             WHERE h.portfolio_id = p.id AND upper(h.ticker) = upper($2))",
        );
    } else {
        sql.push_str(" WHERE TRUE");
    }
    if q.is_some() {
        sql.push_str(" AND p.name ILIKE $1");
    }
    sql.push_str(" ORDER BY p.name");

    let like = q.map(|s| format!("%{s}%")).unwrap_or_default();
    let rows = sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .bind(like)
        .bind(ticker.unwrap_or(""))
        .fetch_all(pool)
        .await
        .context("listing manager portfolios")?;
    Ok(rows)
}

pub async fn get_portfolio(pool: &PgPool, slug: &str) -> anyhow::Result<Option<Portfolio>> {
    let sql = format!("SELECT {PORTFOLIO_COLS} FROM manager_portfolios WHERE slug = $1");
    let row = sqlx::query_as::<_, Portfolio>(sqlx::AssertSqlSafe(sql))
        .bind(slug)
        .fetch_optional(pool)
        .await
        .context("fetching manager portfolio")?;
    Ok(row)
}

pub async fn list_holdings(pool: &PgPool, portfolio_id: Uuid) -> anyhow::Result<Vec<Holding>> {
    let rows = sqlx::query_as::<_, Holding>(
        "SELECT position, ticker, company, pct, activity, shares, reported_price, value, \
         current_price, change_pct, week52_low, week52_high \
         FROM manager_holdings WHERE portfolio_id = $1 ORDER BY position",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await
    .context("listing manager holdings")?;
    Ok(rows)
}

/// When the cache was last refreshed (newest portfolio `updated_at`), or None if empty.
pub async fn last_refreshed(pool: &PgPool) -> anyhow::Result<Option<OffsetDateTime>> {
    let row: (Option<OffsetDateTime>,) =
        sqlx::query_as("SELECT MAX(updated_at) FROM manager_portfolios")
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

/// Upsert a portfolio and replace its holdings, transactionally. Returns the portfolio id.
pub async fn replace_portfolio(
    pool: &PgPool,
    p: &PortfolioInput,
    holdings: &[HoldingInput],
) -> anyhow::Result<Uuid> {
    let mut tx = pool.begin().await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO manager_portfolios \
         (id, slug, name, value_text, value_num, stock_count, period, source_url, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now()) \
         ON CONFLICT (slug) DO UPDATE SET \
         name = EXCLUDED.name, value_text = EXCLUDED.value_text, value_num = EXCLUDED.value_num, \
         stock_count = EXCLUDED.stock_count, period = EXCLUDED.period, \
         source_url = EXCLUDED.source_url, updated_at = now() \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(&p.slug)
    .bind(&p.name)
    .bind(&p.value_text)
    .bind(p.value_num)
    .bind(p.stock_count)
    .bind(&p.period)
    .bind(&p.source_url)
    .fetch_one(&mut *tx)
    .await
    .context("upserting manager portfolio")?;

    sqlx::query("DELETE FROM manager_holdings WHERE portfolio_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    for h in holdings {
        sqlx::query(
            "INSERT INTO manager_holdings \
             (id, portfolio_id, position, ticker, company, pct, activity, shares, reported_price, \
              value, current_price, change_pct, week52_low, week52_high) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(h.position)
        .bind(&h.ticker)
        .bind(&h.company)
        .bind(h.pct)
        .bind(&h.activity)
        .bind(h.shares)
        .bind(h.reported_price)
        .bind(h.value)
        .bind(h.current_price)
        .bind(h.change_pct)
        .bind(h.week52_low)
        .bind(h.week52_high)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// User snapshots — immutable point-in-time copies (see 0032 migration).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Snapshot {
    pub id: Uuid,
    pub source_slug: String,
    pub name: String,
    pub value_text: String,
    pub value_num: Option<f64>,
    pub stock_count: i32,
    pub period: String,
    pub source_url: String,
    #[serde(with = "ts")]
    pub taken_at: OffsetDateTime,
}

const SNAPSHOT_COLS: &str =
    "id, source_slug, name, value_text, value_num, stock_count, period, source_url, taken_at";

/// Take a snapshot of a live portfolio (by slug): copy its summary + all holdings into a new,
/// immutable dated row. Returns the new snapshot id, or None if the slug has no live portfolio.
pub async fn create_snapshot(pool: &PgPool, slug: &str) -> anyhow::Result<Option<Uuid>> {
    let Some(p) = get_portfolio(pool, slug).await? else {
        return Ok(None);
    };
    let holdings = list_holdings(pool, p.id).await?;

    let mut tx = pool.begin().await?;
    let snap_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO manager_snapshots \
         (id, source_slug, name, value_text, value_num, stock_count, period, source_url, taken_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now())",
    )
    .bind(snap_id)
    .bind(&p.slug)
    .bind(&p.name)
    .bind(&p.value_text)
    .bind(p.value_num)
    .bind(p.stock_count)
    .bind(&p.period)
    .bind(&p.source_url)
    .execute(&mut *tx)
    .await
    .context("inserting snapshot")?;

    for h in &holdings {
        sqlx::query(
            "INSERT INTO manager_snapshot_holdings \
             (id, snapshot_id, position, ticker, company, pct, activity, shares, reported_price, \
              value, current_price, change_pct, week52_low, week52_high) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(Uuid::new_v4())
        .bind(snap_id)
        .bind(h.position)
        .bind(&h.ticker)
        .bind(&h.company)
        .bind(h.pct)
        .bind(&h.activity)
        .bind(h.shares)
        .bind(h.reported_price)
        .bind(h.value)
        .bind(h.current_price)
        .bind(h.change_pct)
        .bind(h.week52_low)
        .bind(h.week52_high)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(Some(snap_id))
}

/// List all snapshots (newest first), optionally filtered by portfolio name (case-insensitive
/// substring). The UI groups these by `source_slug` into per-portfolio sections.
pub async fn list_snapshots(pool: &PgPool, q: Option<&str>) -> anyhow::Result<Vec<Snapshot>> {
    let mut sql = format!("SELECT {SNAPSHOT_COLS} FROM manager_snapshots WHERE TRUE");
    if q.is_some() {
        sql.push_str(" AND name ILIKE $1");
    }
    sql.push_str(" ORDER BY name, taken_at DESC");

    let like = q.map(|s| format!("%{s}%")).unwrap_or_default();
    let rows = sqlx::query_as::<_, Snapshot>(sqlx::AssertSqlSafe(sql))
        .bind(like)
        .fetch_all(pool)
        .await
        .context("listing snapshots")?;
    Ok(rows)
}

/// One snapshot + its frozen holdings, or None if the id is unknown.
pub async fn get_snapshot(
    pool: &PgPool,
    id: Uuid,
) -> anyhow::Result<Option<(Snapshot, Vec<Holding>)>> {
    let sql = format!("SELECT {SNAPSHOT_COLS} FROM manager_snapshots WHERE id = $1");
    let snap = sqlx::query_as::<_, Snapshot>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching snapshot")?;
    let Some(snap) = snap else { return Ok(None) };

    let holdings = sqlx::query_as::<_, Holding>(
        "SELECT position, ticker, company, pct, activity, shares, reported_price, value, \
         current_price, change_pct, week52_low, week52_high \
         FROM manager_snapshot_holdings WHERE snapshot_id = $1 ORDER BY position",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .context("listing snapshot holdings")?;
    Ok(Some((snap, holdings)))
}

/// Delete one snapshot (holdings cascade). Returns true if a row was removed.
pub async fn delete_snapshot(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let n = sqlx::query("DELETE FROM manager_snapshots WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting snapshot")?
        .rows_affected();
    Ok(n > 0)
}

/// Delete every snapshot for a given source portfolio. Returns how many were removed.
pub async fn delete_snapshots_by_slug(pool: &PgPool, slug: &str) -> anyhow::Result<u64> {
    let n = sqlx::query("DELETE FROM manager_snapshots WHERE source_slug = $1")
        .bind(slug)
        .execute(pool)
        .await
        .context("deleting snapshots by slug")?
        .rows_affected();
    Ok(n)
}
