//! Fundamentals: macro series, company profiles, statements, filings and insider trades.
//!
//! What a provider returned, stored as periods (see migration 0133). Fetching lives in
//! `otw-core/src/fundamentals`; this module only reads and writes rows.

use std::collections::HashSet;

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

// ── Series ──────────────────────────────────────────────────────────────────

/// What a provider says about a series, before it is stored.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesMeta {
    pub provider: String,
    pub code: String,
    pub title: String,
    pub category: String,
    pub country: String,
    pub unit: String,
    pub frequency: String,
    pub seasonal_adj: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SeriesRow {
    pub id: Uuid,
    pub provider: String,
    pub code: String,
    pub title: String,
    pub category: String,
    pub country: String,
    pub unit: String,
    pub frequency: String,
    pub seasonal_adj: bool,
    pub notes: String,
    pub status: String,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_refreshed: Option<OffsetDateTime>,
    /// Latest two observations, for the catalog's last value and change.
    pub last: Option<f64>,
    pub prev: Option<f64>,
    pub last_period: Option<Date>,
}

const SERIES_COLS: &str = "s.id, s.provider, s.code, s.title, s.category, s.country, s.unit, \
     s.frequency, s.seasonal_adj, s.notes, s.status, s.error, s.last_refreshed, \
     (SELECT o.value FROM fund_observations o WHERE o.series_id = s.id \
        ORDER BY o.period_start DESC LIMIT 1) AS last, \
     (SELECT o.value FROM fund_observations o WHERE o.series_id = s.id \
        ORDER BY o.period_start DESC OFFSET 1 LIMIT 1) AS prev, \
     (SELECT max(o.period_start) FROM fund_observations o WHERE o.series_id = s.id) AS last_period";

pub async fn list_series(pool: &PgPool) -> anyhow::Result<Vec<SeriesRow>> {
    let sql = format!("SELECT {SERIES_COLS} FROM fund_series s ORDER BY s.category, s.title");
    Ok(sqlx::query_as::<_, SeriesRow>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing fundamentals series")?)
}

pub async fn get_series(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<SeriesRow>> {
    let sql = format!("SELECT {SERIES_COLS} FROM fund_series s WHERE s.id = $1");
    Ok(sqlx::query_as::<_, SeriesRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Insert a series or refresh its metadata, and mark it pending. Returns its id.
pub async fn upsert_series(pool: &PgPool, m: &SeriesMeta) -> anyhow::Result<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO fund_series \
             (id, provider, code, title, category, country, unit, frequency, seasonal_adj, notes) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (provider, code) DO UPDATE SET \
             title = EXCLUDED.title, country = EXCLUDED.country, unit = EXCLUDED.unit, \
             frequency = EXCLUDED.frequency, seasonal_adj = EXCLUDED.seasonal_adj, \
             notes = EXCLUDED.notes, status = 'pending', error = NULL \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(&m.provider)
    .bind(&m.code)
    .bind(&m.title)
    .bind(&m.category)
    .bind(&m.country)
    .bind(&m.unit)
    .bind(&m.frequency)
    .bind(m.seasonal_adj)
    .bind(&m.notes)
    .fetch_one(pool)
    .await
    .context("storing fundamentals series")?;
    Ok(id)
}

pub async fn set_series_category(pool: &PgPool, id: Uuid, category: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE fund_series SET category = $2 WHERE id = $1")
        .bind(id)
        .bind(category)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn set_series_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE fund_series SET status = $2, error = $3 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

/// Replace a series' observations with what the provider just returned (latest vintage
/// only) and mark it refreshed.
pub async fn replace_observations(
    pool: &PgPool,
    id: Uuid,
    obs: &[(Date, f64)],
) -> anyhow::Result<()> {
    let (dates, values): (Vec<Date>, Vec<f64>) = obs.iter().copied().unzip();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM fund_observations WHERE series_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO fund_observations (series_id, period_start, value) \
         SELECT $1, d, v FROM UNNEST($2::date[], $3::float8[]) AS t(d, v) \
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&dates)
    .bind(&values)
    .execute(&mut *tx)
    .await
    .context("storing observations")?;
    sqlx::query(
        "UPDATE fund_series SET status = 'ok', error = NULL, last_refreshed = now() WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn observations(
    pool: &PgPool,
    id: Uuid,
    from: Option<Date>,
    to: Option<Date>,
) -> anyhow::Result<Vec<(Date, f64)>> {
    Ok(sqlx::query_as::<_, (Date, f64)>(
        "SELECT period_start, value FROM fund_observations \
         WHERE series_id = $1 \
           AND ($2::date IS NULL OR period_start >= $2) \
           AND ($3::date IS NULL OR period_start < $3) \
         ORDER BY period_start",
    )
    .bind(id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?)
}

pub async fn delete_series(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM fund_series WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Companies ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct CompanyInput {
    pub ticker: String,
    pub cik: String,
    pub name: String,
    pub exchange: String,
    pub currency: String,
    pub country: String,
    pub sector: String,
    pub industry: String,
    pub sic: String,
    pub fiscal_year_end: String,
    pub website: String,
    pub description: String,
    pub shares_out: Option<f64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CompanyRow {
    pub id: Uuid,
    pub ticker: String,
    pub cik: String,
    pub name: String,
    pub exchange: String,
    pub currency: String,
    pub country: String,
    pub sector: String,
    pub industry: String,
    pub sic: String,
    pub fiscal_year_end: String,
    pub website: String,
    pub description: String,
    pub shares_out: Option<f64>,
    pub followed: bool,
    pub status: String,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_refreshed: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub opened_at: OffsetDateTime,
}

const COMPANY_COLS: &str = "id, ticker, cik, name, exchange, currency, country, sector, \
     industry, sic, fiscal_year_end, website, description, shares_out, followed, status, \
     error, last_refreshed, opened_at";

pub async fn list_companies(pool: &PgPool) -> anyhow::Result<Vec<CompanyRow>> {
    let sql = format!("SELECT {COMPANY_COLS} FROM fund_companies ORDER BY ticker");
    Ok(sqlx::query_as::<_, CompanyRow>(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await?)
}

pub async fn company_by_ticker(pool: &PgPool, ticker: &str) -> anyhow::Result<Option<CompanyRow>> {
    let sql = format!("SELECT {COMPANY_COLS} FROM fund_companies WHERE ticker = $1");
    Ok(sqlx::query_as::<_, CompanyRow>(sqlx::AssertSqlSafe(sql))
        .bind(ticker)
        .fetch_optional(pool)
        .await?)
}

/// Insert a company with what is known before the first fetch (ticker, CIK, name) and
/// mark it pending. An existing row keeps its data and is only marked pending.
pub async fn ensure_company(
    pool: &PgPool,
    ticker: &str,
    cik: &str,
    name: &str,
) -> anyhow::Result<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO fund_companies (id, ticker, cik, name) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (ticker) DO UPDATE SET status = 'pending', error = NULL \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(ticker)
    .bind(cik)
    .bind(name)
    .fetch_one(pool)
    .await
    .context("storing company")?;
    Ok(id)
}

pub async fn update_company(pool: &PgPool, id: Uuid, c: &CompanyInput) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE fund_companies SET cik = $2, name = $3, exchange = $4, currency = $5, \
             country = $6, sector = $7, industry = $8, sic = $9, fiscal_year_end = $10, \
             website = $11, description = $12, shares_out = $13 \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&c.cik)
    .bind(&c.name)
    .bind(&c.exchange)
    .bind(&c.currency)
    .bind(&c.country)
    .bind(&c.sector)
    .bind(&c.industry)
    .bind(&c.sic)
    .bind(&c.fiscal_year_end)
    .bind(&c.website)
    .bind(&c.description)
    .bind(c.shares_out)
    .execute(pool)
    .await
    .context("updating company")?;
    Ok(())
}

pub async fn set_company_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE fund_companies SET status = $2, error = $3, \
             last_refreshed = CASE WHEN $2 = 'ok' THEN now() ELSE last_refreshed END \
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_followed(pool: &PgPool, ticker: &str, followed: bool) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE fund_companies SET followed = $2 WHERE ticker = $1")
        .bind(ticker)
        .bind(followed)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// The user opened the company: it moves to the front of the recent ones.
pub async fn touch_company(pool: &PgPool, ticker: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE fund_companies SET opened_at = now() WHERE ticker = $1")
        .bind(ticker)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_company(pool: &PgPool, ticker: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM fund_companies WHERE ticker = $1")
        .bind(ticker)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── ETFs ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EtfRow {
    pub ticker: String,
    /// The fund's name from its stored snapshot.
    pub name: Option<String>,
    pub followed: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub opened_at: OffsetDateTime,
}

pub async fn list_etfs(pool: &PgPool) -> anyhow::Result<Vec<EtfRow>> {
    Ok(sqlx::query_as::<_, EtfRow>(
        "SELECT e.ticker, s.data->>'name' AS name, e.followed, e.opened_at FROM fund_etfs e \
         LEFT JOIN fund_snapshots s ON s.subject = e.ticker AND s.dataset = 'etf' \
         ORDER BY e.ticker",
    )
    .fetch_all(pool)
    .await?)
}

/// Record that the user opened an ETF. Only one whose snapshot is stored is kept: false
/// when there is none (the ticker never resolved).
pub async fn touch_etf(pool: &PgPool, ticker: &str) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "INSERT INTO fund_etfs (ticker) SELECT $1 WHERE EXISTS \
             (SELECT 1 FROM fund_snapshots WHERE subject = $1 AND dataset = 'etf') \
         ON CONFLICT (ticker) DO UPDATE SET opened_at = now()",
    )
    .bind(ticker)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn set_etf_followed(pool: &PgPool, ticker: &str, followed: bool) -> anyhow::Result<bool> {
    let res = sqlx::query("UPDATE fund_etfs SET followed = $2 WHERE ticker = $1")
        .bind(ticker)
        .bind(followed)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_etf(pool: &PgPool, ticker: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM fund_etfs WHERE ticker = $1")
        .bind(ticker)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Statements ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Statement {
    pub kind: String,
    pub fiscal_period: String,
    pub fiscal_year: i32,
    pub period_start: Date,
    pub period_end: Date,
    pub source: String,
    pub lines: serde_json::Value,
    pub tags: serde_json::Value,
}

/// Replace every statement `source` reported for a company.
pub async fn replace_statements(
    pool: &PgPool,
    company_id: Uuid,
    source: &str,
    rows: &[Statement],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM fund_statements WHERE company_id = $1 AND source = $2")
        .bind(company_id)
        .bind(source)
        .execute(&mut *tx)
        .await?;
    for s in rows {
        sqlx::query(
            "INSERT INTO fund_statements \
                 (company_id, kind, fiscal_period, fiscal_year, period_start, period_end, \
                  source, lines, tags) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT DO NOTHING",
        )
        .bind(company_id)
        .bind(&s.kind)
        .bind(&s.fiscal_period)
        .bind(s.fiscal_year)
        .bind(s.period_start)
        .bind(s.period_end)
        .bind(&s.source)
        .bind(&s.lines)
        .bind(&s.tags)
        .execute(&mut *tx)
        .await
        .context("storing statement")?;
    }
    tx.commit().await?;
    Ok(())
}

/// Statements of one kind, oldest first. `annual` picks FY rows, otherwise quarters.
pub async fn statements(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    annual: bool,
) -> anyhow::Result<Vec<Statement>> {
    Ok(sqlx::query_as::<_, Statement>(
        "SELECT kind, fiscal_period, fiscal_year, period_start, period_end, source, lines, tags \
         FROM fund_statements \
         WHERE company_id = $1 AND kind = $2 AND (fiscal_period = 'FY') = $3 \
         ORDER BY period_end",
    )
    .bind(company_id)
    .bind(kind)
    .bind(annual)
    .fetch_all(pool)
    .await?)
}

// ── Documents ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DocumentInput {
    pub kind: String,
    pub form: String,
    pub accession: String,
    pub filed_at: Date,
    pub period: Option<Date>,
    pub title: String,
    pub url: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DocumentRow {
    pub id: Uuid,
    pub ticker: String,
    pub kind: String,
    pub form: String,
    pub accession: String,
    pub filed_at: Date,
    pub period: Option<Date>,
    pub title: String,
    pub url: String,
    pub source: String,
    pub has_body: bool,
}

const DOC_COLS: &str = "d.id, c.ticker, d.kind, d.form, d.accession, d.filed_at, d.period, \
     d.title, d.url, d.source, (d.body IS NOT NULL) AS has_body";

/// Store documents; a known accession keeps its row (and its fetched body).
pub async fn upsert_documents(
    pool: &PgPool,
    company_id: Uuid,
    docs: &[DocumentInput],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    for d in docs {
        sqlx::query(
            "INSERT INTO fund_documents \
                 (id, company_id, kind, form, accession, filed_at, period, title, url, source) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (source, accession) DO UPDATE SET \
                 form = EXCLUDED.form, title = EXCLUDED.title, url = EXCLUDED.url, \
                 period = EXCLUDED.period",
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(&d.kind)
        .bind(&d.form)
        .bind(&d.accession)
        .bind(d.filed_at)
        .bind(d.period)
        .bind(&d.title)
        .bind(&d.url)
        .bind(&d.source)
        .execute(&mut *tx)
        .await
        .context("storing document")?;
    }
    tx.commit().await?;
    Ok(())
}

/// Documents, newest first. `company_id` None = across every company.
pub async fn documents(
    pool: &PgPool,
    company_id: Option<Uuid>,
    kind: Option<&str>,
    form: Option<&str>,
    limit: i64,
) -> anyhow::Result<Vec<DocumentRow>> {
    let sql = format!(
        "SELECT {DOC_COLS} FROM fund_documents d JOIN fund_companies c ON c.id = d.company_id \
         WHERE ($1::uuid IS NULL OR d.company_id = $1) \
           AND ($2::text IS NULL OR d.kind = $2) \
           AND ($3::text IS NULL OR d.form = $3) \
         ORDER BY d.filed_at DESC, d.accession DESC LIMIT $4"
    );
    Ok(sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(sql))
        .bind(company_id)
        .bind(kind)
        .bind(form)
        .bind(limit)
        .fetch_all(pool)
        .await?)
}

/// Full-text search over stored document titles and bodies, best match first.
pub async fn search_documents(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<DocumentRow>> {
    let sql = format!(
        "SELECT {DOC_COLS} FROM fund_documents d JOIN fund_companies c ON c.id = d.company_id \
         WHERE d.search @@ websearch_to_tsquery('english', $1) \
         ORDER BY ts_rank(d.search, websearch_to_tsquery('english', $1)) DESC, d.filed_at DESC \
         LIMIT $2"
    );
    Ok(sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(sql))
        .bind(q)
        .bind(limit)
        .fetch_all(pool)
        .await?)
}

// ── Insider trades ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct InsiderTrade {
    pub accession: String,
    pub ordinal: i32,
    pub filed_at: Date,
    pub tx_date: Date,
    pub insider: String,
    pub role: String,
    pub code: String,
    pub acquired: bool,
    pub shares: f64,
    pub price: Option<f64>,
    pub owned_after: Option<f64>,
}

/// Accessions already parsed for a company, so a refresh only fetches new filings.
/// Form 4 accessions already parsed for a company, whether or not they yielded a trade.
pub async fn form4_seen(pool: &PgPool, company_id: Uuid) -> anyhow::Result<HashSet<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT accession FROM fund_form4_seen WHERE company_id = $1 \
         UNION SELECT DISTINCT accession FROM fund_insider_trades WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

pub async fn mark_form4_seen(pool: &PgPool, company_id: Uuid, accessions: &[String]) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO fund_form4_seen (company_id, accession) SELECT $1, a FROM UNNEST($2::text[]) AS t(a) \
         ON CONFLICT DO NOTHING",
    )
    .bind(company_id)
    .bind(accessions)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_insider_trades(
    pool: &PgPool,
    company_id: Uuid,
    rows: &[InsiderTrade],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    for r in rows {
        sqlx::query(
            "INSERT INTO fund_insider_trades \
                 (company_id, accession, ordinal, filed_at, tx_date, insider, role, code, \
                  acquired, shares, price, owned_after) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT DO NOTHING",
        )
        .bind(company_id)
        .bind(&r.accession)
        .bind(r.ordinal)
        .bind(r.filed_at)
        .bind(r.tx_date)
        .bind(&r.insider)
        .bind(&r.role)
        .bind(&r.code)
        .bind(r.acquired)
        .bind(r.shares)
        .bind(r.price)
        .bind(r.owned_after)
        .execute(&mut *tx)
        .await
        .context("storing insider trade")?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn insider_trades(
    pool: &PgPool,
    company_id: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<InsiderTrade>> {
    Ok(sqlx::query_as::<_, InsiderTrade>(
        "SELECT accession, ordinal, filed_at, tx_date, insider, role, code, acquired, shares, \
                price, owned_after \
         FROM fund_insider_trades WHERE company_id = $1 \
         ORDER BY tx_date DESC, accession DESC, ordinal LIMIT $2",
    )
    .bind(company_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

// ── Snapshots ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Snapshot {
    pub subject: String,
    pub dataset: String,
    pub provider: String,
    pub data: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub fetched_at: OffsetDateTime,
}

pub async fn snapshot(pool: &PgPool, subject: &str, dataset: &str) -> anyhow::Result<Option<Snapshot>> {
    Ok(sqlx::query_as::<_, Snapshot>(
        "SELECT subject, dataset, provider, data, fetched_at FROM fund_snapshots \
         WHERE subject = $1 AND dataset = $2",
    )
    .bind(subject)
    .bind(dataset)
    .fetch_optional(pool)
    .await?)
}

pub async fn put_snapshot(
    pool: &PgPool,
    subject: &str,
    dataset: &str,
    provider: &str,
    data: &serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO fund_snapshots (subject, dataset, provider, data, fetched_at) \
         VALUES ($1, $2, $3, $4, now()) \
         ON CONFLICT (subject, dataset) DO UPDATE SET \
             provider = EXCLUDED.provider, data = EXCLUDED.data, fetched_at = now()",
    )
    .bind(subject)
    .bind(dataset)
    .bind(provider)
    .bind(data)
    .execute(pool)
    .await
    .context("storing snapshot")?;
    Ok(())
}

/// Every snapshot of a subject (a company's datasets go when the company goes).
pub async fn delete_snapshots(pool: &PgPool, subject: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM fund_snapshots WHERE subject = $1")
        .bind(subject)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Transcripts ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Segment {
    pub ordinal: i32,
    pub speaker: String,
    pub role: String,
    pub section: String,
    pub text: String,
}

pub async fn document(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<DocumentRow>> {
    let sql = format!(
        "SELECT {DOC_COLS} FROM fund_documents d JOIN fund_companies c ON c.id = d.company_id \
         WHERE d.id = $1"
    );
    Ok(sqlx::query_as::<_, DocumentRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// A transcript's body and speaker turns, replacing what was there.
pub async fn store_transcript(
    pool: &PgPool,
    document_id: Uuid,
    segments: &[Segment],
) -> anyhow::Result<()> {
    let body: String = segments
        .iter()
        .map(|s| format!("{}: {}", s.speaker, s.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM fund_transcript_segments WHERE document_id = $1")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;
    for s in segments {
        sqlx::query(
            "INSERT INTO fund_transcript_segments (document_id, ordinal, speaker, role, section, text) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(document_id)
        .bind(s.ordinal)
        .bind(&s.speaker)
        .bind(&s.role)
        .bind(&s.section)
        .bind(&s.text)
        .execute(&mut *tx)
        .await
        .context("storing transcript segment")?;
    }
    sqlx::query("UPDATE fund_documents SET body = $2 WHERE id = $1")
        .bind(document_id)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn segments(pool: &PgPool, document_id: Uuid) -> anyhow::Result<Vec<Segment>> {
    Ok(sqlx::query_as::<_, Segment>(
        "SELECT ordinal, speaker, role, section, text FROM fund_transcript_segments \
         WHERE document_id = $1 ORDER BY ordinal",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?)
}

/// Word count of a stored body, for the document list.
pub async fn body_words(pool: &PgPool, document_id: Uuid) -> anyhow::Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT array_length(regexp_split_to_array(trim(body), '\\s+'), 1)::bigint \
         FROM fund_documents WHERE id = $1 AND body IS NOT NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await?
    .flatten())
}

pub async fn document_id(pool: &PgPool, source: &str, accession: &str) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT id FROM fund_documents WHERE source = $1 AND accession = $2")
        .bind(source)
        .bind(accession)
        .fetch_optional(pool)
        .await?)
}

// ── Provider order ──────────────────────────────────────────────────────────

/// Every stored provider order, by dataset.
pub async fn provider_orders(pool: &PgPool) -> anyhow::Result<std::collections::HashMap<String, Vec<String>>> {
    let rows: Vec<(String, Vec<String>)> =
        sqlx::query_as("SELECT dataset, providers FROM fund_provider_order").fetch_all(pool).await?;
    Ok(rows.into_iter().collect())
}

pub async fn provider_order(pool: &PgPool, dataset: &str) -> anyhow::Result<Option<Vec<String>>> {
    Ok(sqlx::query_scalar("SELECT providers FROM fund_provider_order WHERE dataset = $1")
        .bind(dataset)
        .fetch_optional(pool)
        .await?)
}

pub async fn set_provider_order(pool: &PgPool, dataset: &str, providers: &[String]) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO fund_provider_order (dataset, providers) VALUES ($1, $2) \
         ON CONFLICT (dataset) DO UPDATE SET providers = EXCLUDED.providers, updated_at = now()",
    )
    .bind(dataset)
    .bind(providers)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_provider_order(pool: &PgPool, dataset: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM fund_provider_order WHERE dataset = $1").bind(dataset).execute(pool).await?;
    Ok(())
}

// ── Fetch failures ──────────────────────────────────────────────────────────

/// A provider's last refusal of a dataset for a subject, and when to ask it again.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FetchFailure {
    pub provider: String,
    pub error: String,
    pub failed_at: OffsetDateTime,
    pub retry_at: OffsetDateTime,
}

/// Refusals still in force for a dataset and subject, by provider.
pub async fn fetch_failures(
    pool: &PgPool,
    dataset: &str,
    subject: &str,
) -> anyhow::Result<std::collections::HashMap<String, FetchFailure>> {
    let rows: Vec<FetchFailure> = sqlx::query_as(
        "SELECT provider, error, failed_at, retry_at FROM fund_fetch_failures \
         WHERE dataset = $1 AND subject = $2 AND retry_at > now()",
    )
    .bind(dataset)
    .bind(subject)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|f| (f.provider.clone(), f)).collect())
}

pub async fn put_fetch_failure(
    pool: &PgPool,
    dataset: &str,
    subject: &str,
    provider: &str,
    error: &str,
    retry_at: OffsetDateTime,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO fund_fetch_failures (dataset, subject, provider, error, retry_at) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (dataset, subject, provider) DO UPDATE SET \
             error = EXCLUDED.error, failed_at = now(), retry_at = EXCLUDED.retry_at",
    )
    .bind(dataset)
    .bind(subject)
    .bind(provider)
    .bind(error)
    .bind(retry_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_fetch_failure(pool: &PgPool, dataset: &str, subject: &str, provider: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM fund_fetch_failures WHERE dataset = $1 AND subject = $2 AND provider = $3")
        .bind(dataset)
        .bind(subject)
        .bind(provider)
        .execute(pool)
        .await?;
    Ok(())
}
