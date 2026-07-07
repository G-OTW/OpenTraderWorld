//! FX rate storage + currency conversion for the Trading Journal breakdown.
//!
//! Rates are stored USD-based (1 USD = `rate` of `quote`) one row per date+quote.
//! Reads carry forward: the rate for a date is the most recent row on or before that date
//! (ECB publishes on business days only, so weekends/holidays reuse the prior close). A
//! cross-rate between two non-USD currencies is rate(USD→to) / rate(USD→from).
//!
//! Dates no online source could cover are recorded in `journal_fx_pending` and surfaced to
//! the user, who can enter rates by hand (stored with source 'manual', which wins).

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::{format_description::well_known::Iso8601, Date, OffsetDateTime};

/// The currencies we track (must match journal::CURRENCIES). USD is the base and implicit.
pub const FX_QUOTES: [&str; 11] = [
    "EUR", "GBP", "JPY", "CNY", "CHF", "CAD", "AUD", "HKD", "SEK", "NOK", "DKK",
];

/// One fetched/stored rate row, USD-based.
#[derive(Debug, Clone, Serialize)]
pub struct FxRate {
    #[serde(with = "date_iso")]
    pub rate_date: Date,
    pub quote: String,
    pub rate: f64,
    pub source: String,
}

/// A date we could not fetch from any online source — a pending task for the user.
#[derive(Debug, Serialize)]
pub struct FxPending {
    #[serde(with = "date_iso")]
    pub pending_date: Date,
    pub reason: String,
}

/// Serialize/parse a `Date` as `YYYY-MM-DD` for the API and inputs.
mod date_iso {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

/// Parse a `YYYY-MM-DD` string into a `Date`.
pub fn parse_date(s: &str) -> anyhow::Result<Date> {
    Date::parse(s, &Iso8601::DATE).with_context(|| format!("invalid date '{s}'"))
}

// ── Writing rates ─────────────────────────────────────────────────────────────

/// Upsert a batch of USD-based rates for one date. A 'manual' source always overwrites;
/// otherwise a fetched value replaces an existing fetched value (idempotent re-fetch).
pub async fn upsert_rates(
    pool: &PgPool,
    date: Date,
    rates: &[(String, f64)],
    source: &str,
) -> anyhow::Result<u64> {
    let mut n = 0;
    for (quote, rate) in rates {
        let res = sqlx::query(
            "INSERT INTO journal_fx_rates (rate_date, base, quote, rate, source, fetched_at) \
             VALUES ($1, 'USD', $2, $3, $4, now()) \
             ON CONFLICT (rate_date, base, quote) \
             DO UPDATE SET rate = EXCLUDED.rate, source = EXCLUDED.source, fetched_at = now()",
        )
        .bind(date)
        .bind(quote)
        .bind(rate)
        .bind(source)
        .execute(pool)
        .await
        .context("upserting fx rate")?;
        n += res.rows_affected();
    }
    // A successful insert clears any pending task for this date.
    if !rates.is_empty() {
        sqlx::query("DELETE FROM journal_fx_pending WHERE pending_date = $1")
            .bind(date)
            .execute(pool)
            .await
            .context("clearing resolved fx pending")?;
    }
    Ok(n)
}

/// Record a date that no online source could cover, so the user can resolve it by hand.
pub async fn mark_pending(pool: &PgPool, date: Date, reason: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_fx_pending (pending_date, reason) VALUES ($1, $2) \
         ON CONFLICT (pending_date) DO UPDATE SET reason = EXCLUDED.reason",
    )
    .bind(date)
    .bind(reason)
    .execute(pool)
    .await
    .context("marking fx date pending")?;
    Ok(())
}

// ── Reading rates ─────────────────────────────────────────────────────────────

/// The most recent USD→quote rate on or before `date` (carry-forward). `None` if no row
/// exists at or before the date (i.e. before our coverage starts).
async fn usd_rate_on(pool: &PgPool, quote: &str, date: Date) -> anyhow::Result<Option<f64>> {
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT rate FROM journal_fx_rates \
         WHERE base = 'USD' AND quote = $1 AND rate_date <= $2 \
         ORDER BY rate_date DESC LIMIT 1",
    )
    .bind(quote)
    .bind(date)
    .fetch_optional(pool)
    .await
    .context("reading fx rate")?;
    Ok(row.map(|r| r.0))
}

/// Convert `amount` from `from` currency to `to` currency using rates effective on `date`
/// (carry-forward). Returns `None` when a needed rate is missing (caller flags/excludes).
/// USD is the implicit base (rate 1).
pub async fn convert(
    pool: &PgPool,
    amount: f64,
    from: &str,
    to: &str,
    date: Date,
) -> anyhow::Result<Option<f64>> {
    if from == to {
        return Ok(Some(amount));
    }
    // Rate of 1 unit of `cur` in USD: USD→cur is stored, so 1 cur = 1/rate USD.
    // We instead express everything via USD→cur and cross-divide.
    let from_rate = if from == "USD" { Some(1.0) } else { usd_rate_on(pool, from, date).await? };
    let to_rate = if to == "USD" { Some(1.0) } else { usd_rate_on(pool, to, date).await? };
    match (from_rate, to_rate) {
        (Some(fr), Some(tr)) if fr != 0.0 => {
            // amount in `from`; to USD = amount / fr (since 1 USD = fr from);
            // then to `to` = usd * tr.
            Ok(Some(amount / fr * tr))
        }
        _ => Ok(None),
    }
}

// ── Coverage / catch-up bounds ────────────────────────────────────────────────

/// The most recent date for which we have any stored rate, or `None` if the table is empty.
pub async fn latest_rate_date(pool: &PgPool) -> anyhow::Result<Option<Date>> {
    // MAX over an empty table yields one NULL row, so decode the column as Option.
    let row: (Option<Date>,) =
        sqlx::query_as("SELECT MAX(rate_date) FROM journal_fx_rates")
            .fetch_one(pool)
            .await
            .context("reading latest fx date")?;
    Ok(row.0)
}

/// Whether a date already has a full set of quotes stored (so the job can skip it).
pub async fn has_full_coverage(pool: &PgPool, date: Date) -> anyhow::Result<bool> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM journal_fx_rates WHERE base = 'USD' AND rate_date = $1",
    )
    .bind(date)
    .fetch_one(pool)
    .await
    .context("counting fx coverage")?;
    Ok(row.0 as usize >= FX_QUOTES.len())
}

// ── Pending tasks ─────────────────────────────────────────────────────────────

pub async fn list_pending(pool: &PgPool) -> anyhow::Result<Vec<FxPending>> {
    let rows = sqlx::query_as::<_, (Date, String)>(
        "SELECT pending_date, reason FROM journal_fx_pending ORDER BY pending_date DESC",
    )
    .fetch_all(pool)
    .await
    .context("listing fx pending")?;
    Ok(rows
        .into_iter()
        .map(|(pending_date, reason)| FxPending { pending_date, reason })
        .collect())
}

pub async fn pending_count(pool: &PgPool) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM journal_fx_pending")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Remove a date's pending task (used when the user resolves it manually).
pub async fn clear_pending(pool: &PgPool, date: Date) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM journal_fx_pending WHERE pending_date = $1")
        .bind(date)
        .execute(pool)
        .await
        .context("clearing fx pending")?;
    Ok(())
}

/// The rates stored for a specific date (for display when resolving a pending task).
pub async fn rates_on(pool: &PgPool, date: Date) -> anyhow::Result<Vec<FxRate>> {
    let rows = sqlx::query_as::<_, (Date, String, f64, String)>(
        "SELECT rate_date, quote, rate, source FROM journal_fx_rates \
         WHERE base = 'USD' AND rate_date = $1 ORDER BY quote",
    )
    .bind(date)
    .fetch_all(pool)
    .await
    .context("reading rates for date")?;
    Ok(rows
        .into_iter()
        .map(|(rate_date, quote, rate, source)| FxRate { rate_date, quote, rate, source })
        .collect())
}

/// Yesterday (UTC) — the latest business close we could expect to have.
pub fn yesterday_utc() -> Date {
    (OffsetDateTime::now_utc() - time::Duration::days(1)).date()
}
