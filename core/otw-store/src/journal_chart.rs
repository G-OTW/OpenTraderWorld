//! The journal a chart instrument books its trades into (`journal_chart_links`), and where
//! each trading account stands on it (`journal_chart_books`).
//!
//! A link is a period: a position whose opening fill falls between `since` and `until` is
//! filed into the link's journal, wherever it is closed. Only one link per instrument is open
//! at a time; changing the journal ends it and opens the next, in one transaction.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Link {
    pub id: Uuid,
    pub category_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub since: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub until: Option<OffsetDateTime>,
}

impl Link {
    /// Whether a position opened at `at` belongs to this link.
    pub fn covers(&self, at: OffsetDateTime) -> bool {
        at >= self.since && self.until.is_none_or(|u| at < u)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Book {
    pub account_id: Uuid,
    pub ticker: String,
    pub asset_type: String,
    pub resume_from: OffsetDateTime,
    pub seeds: serde_json::Value,
    pub mark: String,
    pub batches: serde_json::Value,
}

/// The journal the instrument books into now, if any.
pub async fn current(pool: &PgPool, ticker: &str, asset_type: &str) -> anyhow::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT category_id FROM journal_chart_links WHERE ticker = $1 AND asset_type = $2 AND until IS NULL",
    )
    .bind(ticker)
    .bind(asset_type)
    .fetch_optional(pool)
    .await
    .context("reading the chart's journal")?;
    Ok(row.map(|r| r.0))
}

/// Every period of the instrument, oldest first.
pub async fn links(pool: &PgPool, ticker: &str, asset_type: &str) -> anyhow::Result<Vec<Link>> {
    sqlx::query_as(
        "SELECT id, category_id, since, until FROM journal_chart_links \
         WHERE ticker = $1 AND asset_type = $2 ORDER BY since",
    )
    .bind(ticker)
    .bind(asset_type)
    .fetch_all(pool)
    .await
    .context("reading the chart's journal periods")
}

/// End the open period and, with a journal, start the next one. Returns false when the
/// instrument already books into that journal (nothing changed).
pub async fn set(pool: &PgPool, ticker: &str, asset_type: &str, category_id: Option<Uuid>) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let open: Option<(Uuid,)> = sqlx::query_as(
        "SELECT category_id FROM journal_chart_links \
         WHERE ticker = $1 AND asset_type = $2 AND until IS NULL FOR UPDATE",
    )
    .bind(ticker)
    .bind(asset_type)
    .fetch_optional(&mut *tx)
    .await
    .context("reading the chart's journal")?;
    if open.map(|r| r.0) == category_id {
        return Ok(false);
    }
    let now = OffsetDateTime::now_utc();
    sqlx::query("UPDATE journal_chart_links SET until = $3 WHERE ticker = $1 AND asset_type = $2 AND until IS NULL")
        .bind(ticker)
        .bind(asset_type)
        .bind(now)
        .execute(&mut *tx)
        .await
        .context("ending the chart's journal period")?;
    if let Some(category_id) = category_id {
        sqlx::query(
            "INSERT INTO journal_chart_links (id, ticker, asset_type, category_id, since) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(ticker)
        .bind(asset_type)
        .bind(category_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .context("starting the chart's journal period")?;
    }
    tx.commit().await?;
    Ok(true)
}

/// The instruments booking into a journal now, with the start of their open period.
pub async fn open_links(pool: &PgPool) -> anyhow::Result<Vec<(String, String, OffsetDateTime)>> {
    sqlx::query_as("SELECT ticker, asset_type, since FROM journal_chart_links WHERE until IS NULL")
        .fetch_all(pool)
        .await
        .context("reading the chart's journals")
}

/// The account's linked instruments.
pub async fn books(pool: &PgPool, account_id: Uuid) -> anyhow::Result<Vec<Book>> {
    sqlx::query_as(
        "SELECT account_id, ticker, asset_type, resume_from, seeds, mark, batches \
         FROM journal_chart_books WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
    .context("reading the chart's journal books")
}

/// Whether the account already follows the instrument.
pub async fn has_book(pool: &PgPool, account_id: Uuid, ticker: &str, asset_type: &str) -> anyhow::Result<bool> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM journal_chart_books WHERE account_id = $1 AND ticker = $2 AND asset_type = $3",
    )
    .bind(account_id)
    .bind(ticker)
    .bind(asset_type)
    .fetch_optional(pool)
    .await
    .context("reading the chart's journal book")?;
    Ok(row.is_some())
}

/// Start following the instrument on the account from `from`, with the positions it held.
pub async fn add_book(
    pool: &PgPool,
    account_id: Uuid,
    ticker: &str,
    asset_type: &str,
    from: OffsetDateTime,
    seeds: &serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_chart_books (account_id, ticker, asset_type, resume_from, seeds) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
    )
    .bind(account_id)
    .bind(ticker)
    .bind(asset_type)
    .bind(from)
    .bind(seeds)
    .execute(pool)
    .await
    .context("following the chart's instrument")?;
    Ok(())
}

/// What a pass leaves for the next one.
pub async fn save_book(pool: &PgPool, b: &Book, last_error: Option<&str>) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE journal_chart_books SET resume_from = $4, seeds = $5, mark = $6, batches = $7, last_error = $8 \
         WHERE account_id = $1 AND ticker = $2 AND asset_type = $3",
    )
    .bind(b.account_id)
    .bind(&b.ticker)
    .bind(&b.asset_type)
    .bind(b.resume_from)
    .bind(&b.seeds)
    .bind(&b.mark)
    .bind(&b.batches)
    .bind(last_error)
    .execute(pool)
    .await
    .context("saving the chart's journal book")?;
    Ok(())
}

/// Whether an import batch still exists (the user may have reverted or forgotten it).
pub async fn batch_exists(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM journal_import_batches WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading an import batch")?;
    Ok(row.is_some())
}

/// A chart batch grows with every fill: its count is the trades it holds now.
pub async fn count_batch(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE journal_import_batches \
         SET imported = (SELECT count(*) FROM journal_trades WHERE import_batch_id = $1)::int WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .context("counting a chart batch")?;
    Ok(())
}
