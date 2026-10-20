//! FX rate storage + currency conversion for the Trading Journal breakdown.
//!
//! Rates are stored USD-based (1 USD = `rate` of `quote`) one row per date+quote.
//! Reads carry forward: the rate for a date is the most recent row on or before that date
//! (ECB publishes on business days only, so weekends/holidays reuse the prior close). A
//! cross-rate between two non-USD currencies is rate(USD→to) / rate(USD→from).
//!
//! A (date, currency) pair no source can cover is recorded in `journal_fx_pending` and
//! surfaced to the user. The resolution chain is: stored rate, then the online API (majors
//! only), then a stored histdata series when a market for the pair exists, then manual
//! entry (source 'manual', which always wins). Nothing is ever guessed: a currency with no
//! market simply stays pending until the user types a number.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use time::{format_description::well_known::Iso8601, Date, OffsetDateTime};
use uuid::Uuid;

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

/// A (date, currency) we could not price from any source — a pending task for the user.
#[derive(Debug, Serialize)]
pub struct FxPending {
    #[serde(with = "date_iso")]
    pub pending_date: Date,
    pub quote: String,
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
    // Storing a rate clears that currency's pending task, and only that one: a date can
    // hold a task per currency, and writing EUR says nothing about USDT.
    if !rates.is_empty() {
        let quotes: Vec<String> = rates.iter().map(|(q, _)| q.clone()).collect();
        sqlx::query("DELETE FROM journal_fx_pending WHERE pending_date = $1 AND quote = ANY($2)")
            .bind(date)
            .bind(&quotes)
            .execute(pool)
            .await
            .context("clearing resolved fx pending")?;
    }
    Ok(n)
}

/// Record a (date, currency) no source could price, so the user can resolve it by hand.
pub async fn mark_pending(
    pool: &PgPool,
    date: Date,
    quote: &str,
    reason: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_fx_pending (pending_date, quote, reason) VALUES ($1, $2, $3) \
         ON CONFLICT (pending_date, quote) DO UPDATE SET reason = EXCLUDED.reason",
    )
    .bind(date)
    .bind(quote)
    .bind(reason)
    .execute(pool)
    .await
    .context("marking fx pair pending")?;
    Ok(())
}

// ── Reading rates ─────────────────────────────────────────────────────────────

/// The most recent USD→quote rate on or before `date` (carry-forward), falling back to the
/// oldest stored rate when `date` predates our coverage. `None` only when the quote has no
/// rate at all.
pub async fn usd_rate_on(pool: &PgPool, quote: &str, date: Date) -> anyhow::Result<Option<f64>> {
    // Carry-forward: the newest rate on/before the date. Failing that (the date predates our
    // whole history, e.g. a net-worth chart reaching back further than the FX table), carry
    // the oldest rate *backward* instead. Returning None there would silently drop the asset
    // from the sum and read as a value crash rather than a missing rate.
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
    if let Some(r) = row {
        return Ok(Some(r.0));
    }
    let earliest: Option<(f64,)> = sqlx::query_as(
        "SELECT rate FROM journal_fx_rates \
         WHERE base = 'USD' AND quote = $1 \
         ORDER BY rate_date ASC LIMIT 1",
    )
    .bind(quote)
    .fetch_optional(pool)
    .await
    .context("reading earliest fx rate")?;
    Ok(earliest.map(|r| r.0))
}

/// Convert `amount` from `from` currency to `to` currency using rates effective on `date`
/// (carry-forward, and the oldest rate for dates before coverage starts). Returns `None`
/// only when a currency has no stored rate at all (caller flags/excludes).
/// USD is the implicit base (rate 1). One-shot; loops over many trades should use
/// [`FxCache`] instead, which memoizes the per-(quote, date) lookups.
pub async fn convert(
    pool: &PgPool,
    amount: f64,
    from: &str,
    to: &str,
    date: Date,
) -> anyhow::Result<Option<f64>> {
    FxCache::new().convert(pool, amount, from, to, date).await
}

/// Per-request FX conversion cache: memoizes the carry-forward (quote, date) lookups so a
/// breakdown/report/calendar over N trades issues one query per distinct quote+date rather
/// than several per trade. Cheap to build, meant to live for one request.
///
/// It also records every pair it could not price. Callers that own a user-visible total
/// (the breakdown, the analytics pass) drain [`FxCache::misses`] once at the end and queue
/// them, which is why `convert` can stay a plain `Option` at all 37 of its call sites: the
/// currency that failed is remembered here rather than threaded back through every return
/// type.
#[derive(Default)]
pub struct FxCache {
    rates: std::collections::HashMap<(String, Date), Option<f64>>,
    missed: std::collections::BTreeSet<(Date, String)>,
}

impl FxCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The (date, currency) pairs this cache could not price, deduplicated and ordered.
    pub fn misses(&self) -> impl Iterator<Item = (Date, &str)> {
        self.missed.iter().map(|(d, q)| (*d, q.as_str()))
    }

    /// Queue every unpriced pair seen so far as a pending task. Idempotent: the same pair
    /// on a later pass simply refreshes the reason.
    pub async fn queue_misses(&self, pool: &PgPool, reason: &str) -> anyhow::Result<()> {
        for (date, quote) in &self.missed {
            mark_pending(pool, *date, quote, reason).await?;
        }
        Ok(())
    }

    async fn usd_rate(
        &mut self,
        pool: &PgPool,
        quote: &str,
        date: Date,
    ) -> anyhow::Result<Option<f64>> {
        if let Some(v) = self.rates.get(&(quote.to_string(), date)) {
            return Ok(*v);
        }
        let v = usd_rate_on(pool, quote, date).await?;
        if v.is_none() {
            self.missed.insert((date, quote.to_string()));
        }
        self.rates.insert((quote.to_string(), date), v);
        Ok(v)
    }

    /// Cached variant of [`convert`] — same semantics.
    pub async fn convert(
        &mut self,
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
        let from_rate = if from == "USD" { Some(1.0) } else { self.usd_rate(pool, from, date).await? };
        let to_rate = if to == "USD" { Some(1.0) } else { self.usd_rate(pool, to, date).await? };
        match (from_rate, to_rate) {
            (Some(fr), Some(tr)) if fr != 0.0 => {
                // amount in `from`; to USD = amount / fr (since 1 USD = fr from);
                // then to `to` = usd * tr.
                Ok(Some(amount / fr * tr))
            }
            _ => Ok(None),
        }
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

/// Dates that have some rates stored but fewer than the full quote set (e.g. a partial
/// upstream answer, or a backup source missing a currency). Newest first, bounded — the
/// catch-up job retries these to complete coverage.
pub async fn incomplete_dates(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Date>> {
    let rows: Vec<(Date,)> = sqlx::query_as(
        "SELECT rate_date FROM journal_fx_rates WHERE base = 'USD' \
         GROUP BY rate_date HAVING COUNT(*) < $1 \
         ORDER BY rate_date DESC LIMIT $2",
    )
    .bind(FX_QUOTES.len() as i64)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("listing incomplete fx dates")?;
    Ok(rows.into_iter().map(|(d,)| d).collect())
}

/// Of `quotes`, the ones with nothing stored on `date`. The catch-up job files a pending
/// task per currency it actually failed to get, rather than one for the whole date: a date
/// is never "missing" as a whole, a currency is.
pub async fn missing_quotes_on(
    pool: &PgPool,
    date: Date,
    quotes: &[&str],
) -> anyhow::Result<Vec<String>> {
    let list: Vec<String> = quotes.iter().map(|q| q.to_string()).collect();
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT q FROM unnest($2::text[]) AS q \
         WHERE NOT EXISTS ( \
           SELECT 1 FROM journal_fx_rates \
           WHERE base = 'USD' AND rate_date = $1 AND quote = q)",
    )
    .bind(date)
    .bind(&list)
    .fetch_all(pool)
    .await
    .context("listing missing fx quotes")?;
    Ok(rows.into_iter().map(|(q,)| q).collect())
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
    let rows = sqlx::query_as::<_, (Date, String, String)>(
        "SELECT pending_date, quote, reason FROM journal_fx_pending \
         ORDER BY pending_date DESC, quote",
    )
    .fetch_all(pool)
    .await
    .context("listing fx pending")?;
    Ok(rows
        .into_iter()
        .map(|(pending_date, quote, reason)| FxPending { pending_date, quote, reason })
        .collect())
}

pub async fn pending_count(pool: &PgPool) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM journal_fx_pending")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Remove one pending task (used when the user resolves or dismisses it).
pub async fn clear_pending(pool: &PgPool, date: Date, quote: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM journal_fx_pending WHERE pending_date = $1 AND quote = $2")
        .bind(date)
        .bind(quote)
        .execute(pool)
        .await
        .context("clearing fx pending")?;
    Ok(())
}

/// Every currency the journal actually holds a value in, USD aside. This is what needs a
/// rate, as opposed to [`FX_QUOTES`] which is only what the online source can serve: a user
/// importing a Binance book trades in USDT, and refusing that currency at the door would
/// lose real trades to satisfy a hardcoded list.
///
/// Pending tasks count as much as stored positions: a fee billed in BNB raises a task for
/// BNB, and a currency the user is asked to price must be one they are allowed to price.
pub async fn required_quotes(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT q FROM ( \
           SELECT upper(trim(currency)) AS q FROM journal_trades \
           UNION SELECT upper(trim(currency)) FROM journal_capital_events \
           UNION SELECT upper(trim(quote)) FROM journal_fx_pending \
         ) c WHERE q <> '' AND q <> 'USD' ORDER BY q",
    )
    .fetch_all(pool)
    .await
    .context("listing required fx quotes")?;
    let mut quotes: Vec<String> = rows.into_iter().map(|(q,)| q).collect();
    // The majors are always offerable even when nothing is booked in them yet, so the
    // manual-entry form is never empty on a fresh install.
    for q in FX_QUOTES {
        if !quotes.iter().any(|x| x == q) {
            quotes.push(q.to_string());
        }
    }
    quotes.sort();
    Ok(quotes)
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

/// The rates in effect on `date`: each quote's most recent row at or before it. Unlike
/// [`rates_on`] this carries forward over weekends, holidays and any not-yet-fetched tail,
/// so a caller asking for a date the job hasn't reached still gets the last known rate.
/// Quotes are returned with their own `rate_date`, which may be earlier than `date`.
pub async fn rates_asof(pool: &PgPool, date: Date) -> anyhow::Result<Vec<FxRate>> {
    let rows = sqlx::query_as::<_, (Date, String, f64, String)>(
        "SELECT DISTINCT ON (quote) rate_date, quote, rate, source \
         FROM journal_fx_rates \
         WHERE base = 'USD' AND rate_date <= $1 \
         ORDER BY quote, rate_date DESC",
    )
    .bind(date)
    .fetch_all(pool)
    .await
    .context("reading rates as-of date")?;
    Ok(rows
        .into_iter()
        .map(|(rate_date, quote, rate, source)| FxRate { rate_date, quote, rate, source })
        .collect())
}

// ── Stated sources ────────────────────────────────────────────────────────────

/// The market a currency is priced on, as the user stated it.
///
/// The conventional tickers (`USD<ccy>`, `<ccy>USD`, the stablecoin pairs) find a major and
/// not much else: a venue names its pair its own way, and a currency can only be reached
/// through the connector that lists it. One row per currency says which connector and which
/// ticker, and `invert` says which way it is quoted.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FxSource {
    pub quote: String,
    /// `None` only when the connector was deleted: the row stays so the user can repoint
    /// it, and nothing reads or downloads through it meanwhile.
    pub connector_id: Option<Uuid>,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    /// `false`: the ticker quotes `quote` per USD (USDJPY). `true`: it quotes USD per
    /// `quote` (USDTUSD), so the stored rate is `1 / close`.
    pub invert: bool,
}

const SOURCE_COLS: &str = "quote, connector_id, provider, asset_type, ticker, invert";

pub async fn list_sources(pool: &PgPool) -> anyhow::Result<Vec<FxSource>> {
    let sql = format!("SELECT {SOURCE_COLS} FROM journal_fx_sources ORDER BY quote");
    Ok(sqlx::query_as::<_, FxSource>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing fx sources")?)
}

pub async fn get_source(pool: &PgPool, quote: &str) -> anyhow::Result<Option<FxSource>> {
    let sql = format!("SELECT {SOURCE_COLS} FROM journal_fx_sources WHERE quote = $1");
    Ok(sqlx::query_as::<_, FxSource>(sqlx::AssertSqlSafe(sql))
        .bind(quote.to_uppercase())
        .fetch_optional(pool)
        .await
        .context("reading fx source")?)
}

/// Last write wins: a currency has one market, and repointing it is the whole point.
pub async fn set_source(pool: &PgPool, s: &FxSource) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_fx_sources \
           (quote, connector_id, provider, asset_type, ticker, invert, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, now()) \
         ON CONFLICT (quote) DO UPDATE SET connector_id = EXCLUDED.connector_id, \
           provider = EXCLUDED.provider, asset_type = EXCLUDED.asset_type, \
           ticker = EXCLUDED.ticker, invert = EXCLUDED.invert, updated_at = now()",
    )
    .bind(s.quote.to_uppercase())
    .bind(s.connector_id)
    .bind(&s.provider)
    .bind(&s.asset_type)
    .bind(s.ticker.trim())
    .bind(s.invert)
    .execute(pool)
    .await
    .context("storing fx source")?;
    Ok(())
}

/// Forget the mapping, falling back to the conventional tickers. Rates already stored stay:
/// they are ordinary rows, and dropping them would reopen tasks that were answered.
pub async fn delete_source(pool: &PgPool, quote: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM journal_fx_sources WHERE quote = $1")
        .bind(quote.to_uppercase())
        .execute(pool)
        .await
        .context("deleting fx source")?;
    Ok(())
}

/// Yesterday (UTC) — the latest business close we could expect to have.
pub fn yesterday_utc() -> Date {
    (OffsetDateTime::now_utc() - time::Duration::days(1)).date()
}
