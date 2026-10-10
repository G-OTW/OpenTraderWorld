//! Storage for the Historical Data module ("histdata").
//!
//! Two concerns: the `histdata_datasets` catalog the management page reads, and the
//! `histdata_jobs` download queue drained by the background worker. Bars themselves are
//! bulk-written via [`write_bars`], which also maintains the dataset summary so the
//! catalog never has to scan `histdata_bars`. Provider connectors and their credentials
//! moved to [`crate::connectors`] when they stopped belonging to one module.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

// ── Datasets (catalog) ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Dataset {
    pub id: Uuid,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub range_from: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub range_to: Option<OffsetDateTime>,
    pub bar_count: i64,
    pub size_bytes: i64,
    pub gaps: JsonValue,
    pub status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub last_updated: OffsetDateTime,
    /// The user's own name for the series. Only an import sets one.
    pub label: Option<String>,
    /// Where an imported file came from (broker, venue, vendor), free text. Empty for a
    /// download: there the provider already answers that question.
    pub source: String,
    /// Free-text tags, a JSON array of strings.
    pub tags: JsonValue,
}

const DATASET_COLS: &str = "id, provider, asset_type, ticker, timeframe, range_from, range_to, \
                            bar_count, size_bytes, gaps, status, last_updated, label, source, tags";

/// List every dataset, ordered for the management page: asset type → ticker → timeframe.
pub async fn list_datasets(pool: &PgPool) -> anyhow::Result<Vec<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         ORDER BY asset_type, ticker, timeframe"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn get_dataset(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Dataset>> {
    let sql = format!("SELECT {DATASET_COLS} FROM histdata_datasets WHERE id = $1");
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// The stored dataset for these coordinates, if one exists. Read-only counterpart of
/// [`upsert_dataset`] — the chart preview uses it to tell "already in store" from "live
/// look only" without creating anything.
pub async fn find_dataset(
    pool: &PgPool,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
) -> anyhow::Result<Option<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         WHERE provider = $1 AND asset_type = $2 AND ticker = $3 AND timeframe = $4"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(provider)
        .bind(asset_type)
        .bind(ticker)
        .bind(timeframe)
        .fetch_optional(pool)
        .await?)
}

/// The best stored dataset for an instrument, whichever provider filled it.
///
/// Bars are bars: a reader that only wants candles (the journal's trade enrichment) has
/// no reason to refuse a symbol because it was downloaded through another account. The
/// preferred provider still wins when it has a row, and past that the fullest dataset
/// does, so the pick is deterministic.
pub async fn find_dataset_any(
    pool: &PgPool,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
    preferred_provider: Option<&str>,
) -> anyhow::Result<Option<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         WHERE asset_type = $1 AND upper(ticker) = upper($2) AND timeframe = $3 \
         ORDER BY (provider = $4) DESC, bar_count DESC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(asset_type)
        .bind(ticker)
        .bind(timeframe)
        .bind(preferred_provider.unwrap_or(""))
        .fetch_optional(pool)
        .await?)
}

/// The best stored dataset for a ticker, whatever asset type it was filed under.
///
/// The portfolio's factor proxies need this: "USO" is an ETF to one provider and a
/// commodity to another, and refusing to find the series because the caller guessed the
/// wrong bucket would report "no bars" for bars that are sitting right there.
pub async fn find_dataset_by_ticker(
    pool: &PgPool,
    ticker: &str,
    timeframe: &str,
) -> anyhow::Result<Option<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         WHERE upper(ticker) = upper($1) AND timeframe = $2 \
         ORDER BY bar_count DESC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(ticker)
        .bind(timeframe)
        .fetch_optional(pool)
        .await?)
}

/// Of the given windows, which ones hold **no stored bar at all**.
///
/// This is how the journal finds the holes inside a range that already has data. The
/// alternative, flagging any gap wider than N timeframes, has to guess at weekends,
/// nights and holidays and gets it wrong on every asset class in turn. Asking the
/// question the caller actually has ("is there a candle where this trade lived?") needs
/// no calendar at all.
///
/// One index-backed existence check per window, all in one round trip. Returns the
/// indices into `windows`, so the caller keeps whatever it attached to them.
pub async fn windows_without_bars(
    pool: &PgPool,
    dataset_id: Uuid,
    windows: &[(OffsetDateTime, OffsetDateTime)],
) -> anyhow::Result<Vec<usize>> {
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    let los: Vec<OffsetDateTime> = windows.iter().map(|(a, _)| *a).collect();
    let his: Vec<OffsetDateTime> = windows.iter().map(|(_, b)| *b).collect();
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT w.i FROM unnest($2::timestamptz[], $3::timestamptz[]) \
              WITH ORDINALITY AS w(lo, hi, i) \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM histdata_bars b \
             WHERE b.dataset_id = $1 AND b.ts >= w.lo AND b.ts <= w.hi \
         )",
    )
    .bind(dataset_id)
    .bind(&los)
    .bind(&his)
    .fetch_all(pool)
    .await
    .context("checking window coverage")?;
    // `WITH ORDINALITY` is one-based.
    Ok(rows.into_iter().map(|(i,)| (i - 1) as usize).collect())
}

/// Find or create the dataset for these coordinates, returning its id.
pub async fn upsert_dataset(
    pool: &PgPool,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
) -> anyhow::Result<Uuid> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO histdata_datasets (id, provider, asset_type, ticker, timeframe) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (provider, asset_type, ticker, timeframe) WHERE provider <> 'import' \
         DO UPDATE SET last_updated = now() \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(provider)
    .bind(asset_type)
    .bind(ticker)
    .bind(timeframe)
    .fetch_one(pool)
    .await
    .context("upserting dataset")?;
    Ok(row.0)
}

/// Find or create the dataset an import writes into.
///
/// Keyed on the instrument **plus its source**: the same ticker exported by two brokers is
/// two series the user asked for separately, and merging them would silently average two
/// different tapes. Re-importing the same file lands back on the same dataset, which is
/// what makes an import idempotent. Name and tags are last-write-wins, except that an
/// empty one never erases what is already there.
pub async fn upsert_import_dataset(
    pool: &PgPool,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
    source: &str,
    label: &str,
    tags: &JsonValue,
) -> anyhow::Result<Uuid> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO histdata_datasets \
           (id, provider, asset_type, ticker, timeframe, source, label, tags, status) \
         VALUES ($1, 'import', $2, $3, $4, $5, NULLIF($6, ''), $7, 'complete') \
         ON CONFLICT (asset_type, ticker, timeframe, source) WHERE provider = 'import' \
         DO UPDATE SET \
           label        = COALESCE(NULLIF(EXCLUDED.label, ''), histdata_datasets.label), \
           tags         = EXCLUDED.tags, \
           status       = 'complete', \
           last_updated = now() \
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(asset_type)
    .bind(ticker)
    .bind(timeframe)
    .bind(source)
    .bind(label)
    .bind(tags)
    .fetch_one(pool)
    .await
    .context("upserting imported dataset")?;
    Ok(row.0)
}

/// The imported dataset for these coordinates, if one exists. Read-only counterpart of
/// [`upsert_import_dataset`]: the import preview uses it to say what a commit would
/// overwrite without creating anything.
pub async fn find_import_dataset(
    pool: &PgPool,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
    source: &str,
) -> anyhow::Result<Option<Dataset>> {
    let sql = format!(
        "SELECT {DATASET_COLS} FROM histdata_datasets \
         WHERE provider = 'import' AND asset_type = $1 AND ticker = $2 \
           AND timeframe = $3 AND source = $4"
    );
    Ok(sqlx::query_as::<_, Dataset>(AssertSqlSafe(sql))
        .bind(asset_type)
        .bind(ticker)
        .bind(timeframe)
        .bind(source)
        .fetch_optional(pool)
        .await?)
}

/// Which of these timestamps the dataset already holds: the periods an import would
/// overwrite rather than add. One round trip, not one query per bar.
pub async fn existing_ts(
    pool: &PgPool,
    dataset_id: Uuid,
    ts: &[OffsetDateTime],
) -> anyhow::Result<std::collections::HashSet<OffsetDateTime>> {
    if ts.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<(OffsetDateTime,)> = sqlx::query_as(
        "SELECT ts FROM histdata_bars WHERE dataset_id = $1 AND ts = ANY($2)",
    )
    .bind(dataset_id)
    .bind(ts)
    .fetch_all(pool)
    .await
    .context("reading bars already stored")?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

/// Delete a dataset only if it holds no bars (e.g. a download that returned nothing).
/// Won't touch a dataset that already has data from a prior download. Returns true if removed.
pub async fn delete_dataset_if_empty(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM histdata_datasets WHERE id = $1 AND bar_count = 0")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_dataset(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    // Bars cascade via FK. Jobs keep their history (dataset_id set NULL).
    let res = sqlx::query("DELETE FROM histdata_datasets WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Bars ───────────────────────────────────────────────────────────────────────

/// One OHLCV bar ready to persist. Adjusted fields are None for crypto/fx.
#[derive(Debug, Clone)]
pub struct Bar {
    pub ts: OffsetDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub adj_open: Option<f64>,
    pub adj_high: Option<f64>,
    pub adj_low: Option<f64>,
    pub adj_close: Option<f64>,
}

/// Both sides of one bar, for the providers that publish them. The mid stays in [`Bar`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quote {
    pub bid_open: f64,
    pub bid_high: f64,
    pub bid_low: f64,
    pub bid_close: f64,
    pub ask_open: f64,
    pub ask_high: f64,
    pub ask_low: f64,
    pub ask_close: f64,
}

/// A quote keyed to the bar it belongs to.
#[derive(Debug, Clone)]
pub struct QuoteBar {
    pub ts: OffsetDateTime,
    pub quote: Quote,
}

/// Rows per INSERT statement. Bars go in as arrays (11 binds whatever the count), so the
/// cap is about statement size and memory, not Postgres' 65535-parameter limit.
const BAR_CHUNK: usize = 5_000;

/// Attach bid/ask to bars already written by [`write_bars`]. An update, never an insert: a
/// quote without its bar is not stored. Returns the rows touched.
pub async fn write_quotes(
    pool: &PgPool,
    dataset_id: Uuid,
    quotes: &[QuoteBar],
) -> anyhow::Result<u64> {
    let mut written = 0u64;
    for chunk in quotes.chunks(BAR_CHUNK) {
        let ts: Vec<OffsetDateTime> = chunk.iter().map(|q| q.ts).collect();
        let col = |f: fn(&Quote) -> f64| chunk.iter().map(|q| f(&q.quote)).collect::<Vec<f64>>();
        let res = sqlx::query(
            "UPDATE histdata_bars b SET \
               bid_open = u.bo, bid_high = u.bh, bid_low = u.bl, bid_close = u.bc, \
               ask_open = u.ao, ask_high = u.ah, ask_low = u.al, ask_close = u.ac \
             FROM UNNEST($2::timestamptz[], $3::float8[], $4::float8[], $5::float8[], \
                         $6::float8[], $7::float8[], $8::float8[], $9::float8[], $10::float8[]) \
                  AS u(ts, bo, bh, bl, bc, ao, ah, al, ac) \
             WHERE b.dataset_id = $1 AND b.ts = u.ts",
        )
        .bind(dataset_id)
        .bind(&ts)
        .bind(col(|q| q.bid_open))
        .bind(col(|q| q.bid_high))
        .bind(col(|q| q.bid_low))
        .bind(col(|q| q.bid_close))
        .bind(col(|q| q.ask_open))
        .bind(col(|q| q.ask_high))
        .bind(col(|q| q.ask_low))
        .bind(col(|q| q.ask_close))
        .execute(pool)
        .await
        .context("writing bid/ask")?;
        written += res.rows_affected();
    }
    Ok(written)
}

/// Bid/ask rows of a dataset within `[from, to]`, ascending. Rows without both sides are
/// skipped: the caller falls back to the mid and its spread for those.
pub async fn read_quotes(
    pool: &PgPool,
    dataset_id: Uuid,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> anyhow::Result<Vec<QuoteBar>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        ts: OffsetDateTime,
        bo: f64,
        bh: f64,
        bl: f64,
        bc: f64,
        ao: f64,
        ah: f64,
        al: f64,
        ac: f64,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT ts, bid_open::float8 AS bo, bid_high::float8 AS bh, bid_low::float8 AS bl, \
                bid_close::float8 AS bc, ask_open::float8 AS ao, ask_high::float8 AS ah, \
                ask_low::float8 AS al, ask_close::float8 AS ac \
         FROM histdata_bars \
         WHERE dataset_id = $1 AND ts >= $2 AND ts <= $3 \
           AND bid_open IS NOT NULL AND bid_high IS NOT NULL AND bid_low IS NOT NULL \
           AND bid_close IS NOT NULL AND ask_open IS NOT NULL AND ask_high IS NOT NULL \
           AND ask_low IS NOT NULL AND ask_close IS NOT NULL \
         ORDER BY ts",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| QuoteBar {
            ts: r.ts,
            quote: Quote {
                bid_open: r.bo,
                bid_high: r.bh,
                bid_low: r.bl,
                bid_close: r.bc,
                ask_open: r.ao,
                ask_high: r.ah,
                ask_low: r.al,
                ask_close: r.ac,
            },
        })
        .collect())
}

/// Bars of a dataset within `[from, to)` (both bounds optional), and how many carry bid/ask.
pub async fn count_bars(
    pool: &PgPool,
    dataset_id: Uuid,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
) -> anyhow::Result<(i64, i64)> {
    let row: (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(bid_open) FROM histdata_bars \
         WHERE dataset_id = $1 \
           AND ($2::timestamptz IS NULL OR ts >= $2) \
           AND ($3::timestamptz IS NULL OR ts < $3)",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Asset types whose history a split or a dividend rewrites.
const ADJUSTABLE: [&str; 4] = ["stock", "etf", "fund", "equity"];

/// How the bars stored before an incoming batch move to the batch's price base.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Rebase {
    /// The provider's adjustment factor (adjusted / raw close) changed by this ratio: a split
    /// or a dividend happened since the stored bars were written.
    Adjusted(f64),
    /// The provider folds adjustments into the OHLC itself (no adjusted column): the raw
    /// prices of the same bars moved by this ratio.
    Raw(f64),
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// The rebase a batch implies, from the bars it shares with the store, each as
/// (stored close, stored adjusted close, incoming close, incoming adjusted close).
///
/// The adjusted factor of one bar is exact whatever its close (a bar first written before its
/// period ended has the same factor), so one shared bar is enough. Raw prices are compared only
/// on an adjustable asset, and only when at least three shared bars agree on the same move of
/// more than 1%: a revised or unsettled close, or a feed with other session hours, is not a split.
fn rebase_factor(shared: &[(f64, Option<f64>, f64, Option<f64>)], adjustable: bool) -> Option<Rebase> {
    let ok = |x: f64| x.is_finite() && x > 0.0;
    let adj: Vec<f64> = shared
        .iter()
        .filter_map(|&(sc, sa, ic, ia)| {
            let (sa, ia) = (sa?, ia?);
            (ok(sc) && ok(sa) && ok(ic) && ok(ia)).then(|| (ia / ic) / (sa / sc))
        })
        .collect();
    if !adj.is_empty() {
        let k = median(adj);
        return (ok(k) && (k - 1.0).abs() > 1e-6).then_some(Rebase::Adjusted(k));
    }
    if !adjustable {
        return None;
    }
    let raw: Vec<f64> = shared
        .iter()
        .filter(|&&(_, sa, _, ia)| sa.is_none() && ia.is_none())
        .filter_map(|&(sc, _, ic, _)| (ok(sc) && ok(ic)).then(|| ic / sc))
        .collect();
    if raw.len() < 3 {
        return None;
    }
    let k = median(raw.clone());
    let agree = raw.iter().filter(|r| ((*r / k) - 1.0).abs() < 1e-3).count();
    (ok(k) && agree >= 3 && (k - 1.0).abs() > 0.01).then_some(Rebase::Raw(k))
}

/// Bring the bars stored before `incoming` onto its price base. A split or a dividend after the
/// stored bars were downloaded rewrites the provider's whole history, while the store kept the
/// old one: without this the junction between the two downloads is a fake gap of the split
/// ratio. Needs the batch to overlap the stored bars (a top-up re-reads a few of them).
async fn rebase_on_overlap(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    dataset_id: Uuid,
    incoming: &[&Bar],
) -> anyhow::Result<()> {
    let Some(first) = incoming.iter().map(|b| b.ts).min() else { return Ok(()) };
    let ts: Vec<OffsetDateTime> = incoming.iter().map(|b| b.ts).collect();
    let stored: Vec<(OffsetDateTime, f64, Option<f64>)> = sqlx::query_as(
        "SELECT ts, close::float8, adj_close::float8 FROM histdata_bars \
         WHERE dataset_id = $1 AND ts = ANY($2) \
           AND EXISTS (SELECT 1 FROM histdata_bars WHERE dataset_id = $1 AND ts < $3)",
    )
    .bind(dataset_id)
    .bind(&ts)
    .bind(first)
    .fetch_all(&mut **tx)
    .await
    .context("reading the bars a batch overlaps")?;
    if stored.is_empty() {
        return Ok(());
    }
    let by_ts: HashMap<OffsetDateTime, &Bar> = incoming.iter().map(|b| (b.ts, *b)).collect();
    let shared: Vec<(f64, Option<f64>, f64, Option<f64>)> = stored
        .iter()
        .filter_map(|(t, sc, sa)| by_ts.get(t).map(|b| (*sc, *sa, b.close, b.adj_close)))
        .collect();
    let asset_type: Option<String> = sqlx::query_scalar("SELECT asset_type FROM histdata_datasets WHERE id = $1")
        .bind(dataset_id)
        .fetch_optional(&mut **tx)
        .await?;
    let adjustable = asset_type.is_some_and(|a| ADJUSTABLE.contains(&a.as_str()));
    match rebase_factor(&shared, adjustable) {
        // A bar with no adjusted close read as raw (factor 1): it takes the new factor too.
        Some(Rebase::Adjusted(k)) => {
            sqlx::query(
                "UPDATE histdata_bars SET adj_open = adj_open * $3, adj_high = adj_high * $3, \
                   adj_low = adj_low * $3, adj_close = COALESCE(adj_close, close) * $3 \
                 WHERE dataset_id = $1 AND ts < $2",
            )
            .bind(dataset_id)
            .bind(first)
            .bind(k)
            .execute(&mut **tx)
            .await
            .context("rebasing adjusted prices")?;
        }
        Some(Rebase::Raw(k)) => {
            sqlx::query(
                "UPDATE histdata_bars SET open = open * $3, high = high * $3, low = low * $3, \
                   close = close * $3, volume = volume / $3 \
                 WHERE dataset_id = $1 AND ts < $2 AND adj_close IS NULL",
            )
            .bind(dataset_id)
            .bind(first)
            .bind(k)
            .execute(&mut **tx)
            .await
            .context("rebasing split-adjusted prices")?;
        }
        None => {}
    }
    Ok(())
}

/// Upsert a batch of bars (idempotent on PK) and refresh the dataset summary in one
/// transaction. Returns the number of rows written. Re-downloads update in place.
pub async fn write_bars(
    pool: &PgPool,
    dataset_id: Uuid,
    bars: &[Bar],
) -> anyhow::Result<u64> {
    if bars.is_empty() {
        return Ok(0);
    }
    // A single INSERT may not touch the same key twice, so collapse repeated timestamps
    // first, last one wins (same order as a row-by-row upsert would have left).
    let mut seen: HashMap<OffsetDateTime, usize> = HashMap::with_capacity(bars.len());
    let mut unique: Vec<&Bar> = Vec::with_capacity(bars.len());
    for b in bars {
        match seen.get(&b.ts) {
            Some(&i) => unique[i] = b,
            None => {
                seen.insert(b.ts, unique.len());
                unique.push(b);
            }
        }
    }

    let mut tx = pool.begin().await?;
    rebase_on_overlap(&mut tx, dataset_id, &unique).await?;
    let mut written = 0u64;
    for chunk in unique.chunks(BAR_CHUNK) {
        let mut ts: Vec<OffsetDateTime> = Vec::with_capacity(chunk.len());
        let mut open: Vec<f64> = Vec::with_capacity(chunk.len());
        let mut high: Vec<f64> = Vec::with_capacity(chunk.len());
        let mut low: Vec<f64> = Vec::with_capacity(chunk.len());
        let mut close: Vec<f64> = Vec::with_capacity(chunk.len());
        let mut volume: Vec<f64> = Vec::with_capacity(chunk.len());
        let mut adj_open: Vec<Option<f64>> = Vec::with_capacity(chunk.len());
        let mut adj_high: Vec<Option<f64>> = Vec::with_capacity(chunk.len());
        let mut adj_low: Vec<Option<f64>> = Vec::with_capacity(chunk.len());
        let mut adj_close: Vec<Option<f64>> = Vec::with_capacity(chunk.len());
        for b in chunk {
            ts.push(b.ts);
            open.push(b.open);
            high.push(b.high);
            low.push(b.low);
            close.push(b.close);
            volume.push(b.volume);
            adj_open.push(b.adj_open);
            adj_high.push(b.adj_high);
            adj_low.push(b.adj_low);
            adj_close.push(b.adj_close);
        }
        let res = sqlx::query(
            "INSERT INTO histdata_bars \
               (dataset_id, ts, open, high, low, close, volume, adj_open, adj_high, adj_low, adj_close) \
             SELECT $1, * FROM UNNEST( \
               $2::timestamptz[], $3::float8[], $4::float8[], $5::float8[], $6::float8[], \
               $7::float8[], $8::float8[], $9::float8[], $10::float8[], $11::float8[]) \
             ON CONFLICT (dataset_id, ts) DO UPDATE SET \
               open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, \
               close = EXCLUDED.close, volume = EXCLUDED.volume, \
               adj_open = EXCLUDED.adj_open, adj_high = EXCLUDED.adj_high, \
               adj_low = EXCLUDED.adj_low, adj_close = EXCLUDED.adj_close",
        )
        .bind(dataset_id)
        .bind(&ts)
        .bind(&open)
        .bind(&high)
        .bind(&low)
        .bind(&close)
        .bind(&volume)
        .bind(&adj_open)
        .bind(&adj_high)
        .bind(&adj_low)
        .bind(&adj_close)
        .execute(&mut *tx)
        .await
        .context("inserting bars")?;
        written += res.rows_affected();
    }
    // Recompute the summary from the (now updated) bars. ~96 bytes/row is a rough
    // on-disk estimate good enough for the management page's size column.
    sqlx::query(
        "UPDATE histdata_datasets d SET \
           range_from   = s.lo, \
           range_to     = s.hi, \
           bar_count    = s.n, \
           size_bytes   = s.n * 96, \
           last_updated = now() \
         FROM (SELECT min(ts) AS lo, max(ts) AS hi, count(*) AS n \
               FROM histdata_bars WHERE dataset_id = $1) s \
         WHERE d.id = $1",
    )
    .bind(dataset_id)
    .execute(&mut *tx)
    .await
    .context("refreshing dataset summary")?;
    tx.commit().await?;
    Ok(written)
}

/// Stream all bars of a dataset in time order, for CSV export.
pub async fn export_rows(
    pool: &PgPool,
    dataset_id: Uuid,
) -> anyhow::Result<Vec<Bar>> {
    let rows: Vec<(
        OffsetDateTime,
        f64,
        f64,
        f64,
        f64,
        f64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    )> = sqlx::query_as(
        "SELECT ts, open::float8, high::float8, low::float8, close::float8, volume::float8, \
         adj_open::float8, adj_high::float8, adj_low::float8, adj_close::float8 \
         FROM histdata_bars WHERE dataset_id = $1 ORDER BY ts",
    )
    .bind(dataset_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            ts: r.0,
            open: r.1,
            high: r.2,
            low: r.3,
            close: r.4,
            volume: r.5,
            adj_open: r.6,
            adj_high: r.7,
            adj_low: r.8,
            adj_close: r.9,
        })
        .collect())
}

/// Read bars of a dataset for charting, optionally bounded by [from, to] and capped at
/// `limit` rows. When more than `limit` bars match, returns the most recent `limit`
/// (descending then reversed to ascending) so the chart shows the latest window. Adjusted
/// columns are omitted — the visualization uses raw OHLCV.
pub async fn read_bars(
    pool: &PgPool,
    dataset_id: Uuid,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
    limit: i64,
) -> anyhow::Result<Vec<Bar>> {
    let mut rows: Vec<(OffsetDateTime, f64, f64, f64, f64, f64)> = sqlx::query_as(
        "SELECT ts, open::float8, high::float8, low::float8, close::float8, volume::float8 \
         FROM histdata_bars \
         WHERE dataset_id = $1 \
           AND ($2::timestamptz IS NULL OR ts >= $2) \
           AND ($3::timestamptz IS NULL OR ts <= $3) \
         ORDER BY ts DESC LIMIT $4",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.reverse(); // back to ascending time order for the chart
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            ts: r.0,
            open: r.1,
            high: r.2,
            low: r.3,
            close: r.4,
            volume: r.5,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        })
        .collect())
}

/// [`read_bars`] with the adjusted columns, for a reader that prices on them (the backtest).
pub async fn read_bars_adj(
    pool: &PgPool,
    dataset_id: Uuid,
    from: Option<OffsetDateTime>,
    to: Option<OffsetDateTime>,
    limit: i64,
) -> anyhow::Result<Vec<Bar>> {
    #[allow(clippy::type_complexity)]
    let mut rows: Vec<(OffsetDateTime, f64, f64, f64, f64, f64, Option<f64>, Option<f64>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT ts, open::float8, high::float8, low::float8, close::float8, volume::float8, \
         adj_open::float8, adj_high::float8, adj_low::float8, adj_close::float8 \
         FROM histdata_bars \
         WHERE dataset_id = $1 \
           AND ($2::timestamptz IS NULL OR ts >= $2) \
           AND ($3::timestamptz IS NULL OR ts <= $3) \
         ORDER BY ts DESC LIMIT $4",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.reverse();
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            ts: r.0,
            open: r.1,
            high: r.2,
            low: r.3,
            close: r.4,
            volume: r.5,
            adj_open: r.6,
            adj_high: r.7,
            adj_low: r.8,
            adj_close: r.9,
        })
        .collect())
}

// ── Jobs (download queue) ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub dataset_id: Option<Uuid>,
    /// Connector whose credentials/quota the worker uses; NULL on pre-connector jobs
    /// (falls back to the provider's default connector).
    pub connector_id: Option<Uuid>,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
    #[serde(with = "time::serde::rfc3339")]
    pub range_from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub range_to: OffsetDateTime,
    pub kind: String,
    pub status: String,
    pub chunks_done: i32,
    pub chunks_total: i32,
    pub bars_written: i64,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Next bar ts to fetch. Set after every chunk, so a job that was parked or interrupted
    /// resumes here instead of refetching what it already stored.
    #[serde(with = "time::serde::rfc3339::option")]
    pub chunk_cursor: Option<OffsetDateTime>,
    /// When a parked (`waiting`) job may run again. NULL on every other status.
    #[serde(with = "time::serde::rfc3339::option")]
    pub resume_at: Option<OffsetDateTime>,
    /// Why it is parked: `quota` (the connector's declared cap is spent) or `rate_limit`
    /// (the provider answered 429). Kept after the resume so the page can explain a pause.
    pub wait_reason: Option<String>,
    /// Consecutive parks; drives the backoff when the provider sends no Retry-After.
    pub wait_count: i32,
    /// The submission this job was queued with (batch download). NULL for a lone job.
    pub batch_id: Option<Uuid>,
}

const JOB_COLS: &str = "id, dataset_id, connector_id, provider, asset_type, ticker, timeframe, \
                        range_from, range_to, kind, status, chunks_done, chunks_total, \
                        bars_written, error, created_at, chunk_cursor, resume_at, wait_reason, \
                        wait_count, batch_id";

/// Statuses a job can still leave on its own — anything else is terminal.
const LIVE_STATUSES: &str = "('queued', 'running', 'waiting', 'cancelling')";

/// Parameters for queueing a download.
pub struct NewJob<'a> {
    pub dataset_id: Uuid,
    pub connector_id: Option<Uuid>,
    pub provider: &'a str,
    pub asset_type: &'a str,
    pub ticker: &'a str,
    pub timeframe: &'a str,
    pub range_from: OffsetDateTime,
    pub range_to: OffsetDateTime,
    pub kind: &'a str,
    /// Groups the jobs one submission queued; None for a single download.
    pub batch_id: Option<Uuid>,
}

pub async fn enqueue_job(pool: &PgPool, j: &NewJob<'_>) -> anyhow::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO histdata_jobs \
           (id, dataset_id, connector_id, provider, asset_type, ticker, timeframe, \
            range_from, range_to, kind, batch_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(j.dataset_id)
    .bind(j.connector_id)
    .bind(j.provider)
    .bind(j.asset_type)
    .bind(j.ticker)
    .bind(j.timeframe)
    .bind(j.range_from)
    .bind(j.range_to)
    .bind(j.kind)
    .bind(j.batch_id)
    .execute(pool)
    .await
    .context("enqueuing job")?;
    Ok(id)
}

/// The last failure of each of `tickers` at `timeframe`, as `(ticker, provider, error)`.
///
/// A download that failed is the only place the real reason lives: the capability matrix says
/// which asset *types* a provider serves, never which listings, so "IBKR carries ETFs" and
/// "IBKR cannot reach its gateway to fetch SXRL.DE" are both true. A module showing coverage
/// asks for this so the answer is the worker's own words rather than a guess.
pub async fn last_job_errors(
    pool: &PgPool,
    tickers: &[String],
    timeframe: &str,
) -> anyhow::Result<std::collections::HashMap<String, (String, String)>> {
    if tickers.is_empty() {
        return Ok(Default::default());
    }
    let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT DISTINCT ON (ticker) ticker, provider, error            FROM histdata_jobs           WHERE ticker = ANY($1) AND timeframe = $2 AND status = 'error'           ORDER BY ticker, created_at DESC",
    )
    .bind(tickers)
    .bind(timeframe)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(t, p, e)| e.map(|e| (t, (p, e))))
        .collect())
}

/// Recent jobs for the download page (newest first).
pub async fn list_jobs(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Job>> {
    let sql = format!("SELECT {JOB_COLS} FROM histdata_jobs ORDER BY created_at DESC LIMIT $1");
    Ok(sqlx::query_as::<_, Job>(AssertSqlSafe(sql))
        .bind(limit)
        .fetch_all(pool)
        .await?)
}

// ── Worker-side job lifecycle ──────────────────────────────────────────────────

/// Claim the oldest runnable job, flipping it to `running`. Returns None when idle.
/// Runnable = freshly queued, or parked and past its `resume_at`. `FOR UPDATE SKIP LOCKED`
/// keeps this safe even if more than one worker ever runs.
pub async fn claim_next_job(pool: &PgPool) -> anyhow::Result<Option<Job>> {
    let sql = format!(
        "UPDATE histdata_jobs SET status = 'running', started_at = now() \
         WHERE id = (SELECT id FROM histdata_jobs \
                     WHERE status = 'queued' \
                        OR (status = 'waiting' AND resume_at IS NOT NULL AND resume_at <= now()) \
                     ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED) \
         RETURNING {JOB_COLS}"
    );
    Ok(sqlx::query_as::<_, Job>(AssertSqlSafe(sql))
        .fetch_optional(pool)
        .await?)
}

/// Re-queue any job stuck in `running` (e.g. process crash mid-download) so the worker
/// resumes it. A crash mid-cancel honours the user: `cancelling` becomes `cancelled`
/// rather than restarting. Called once at startup.
pub async fn requeue_orphans(pool: &PgPool) -> anyhow::Result<u64> {
    sqlx::query(
        "UPDATE histdata_jobs SET status = 'cancelled', finished_at = now() \
         WHERE status = 'cancelling'",
    )
    .execute(pool)
    .await?;
    let res = sqlx::query("UPDATE histdata_jobs SET status = 'queued' WHERE status = 'running'")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Park a running job until `resume_at`: it keeps its progress and is claimed again once
/// that instant passes. `reason` is `quota` or `rate_limit`.
pub async fn park_job(
    pool: &PgPool,
    job_id: Uuid,
    resume_at: OffsetDateTime,
    reason: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET status = 'waiting', resume_at = $2, wait_reason = $3, \
             wait_count = wait_count + 1 \
         WHERE id = $1",
    )
    .bind(job_id)
    .bind(resume_at)
    .bind(reason)
    .execute(pool)
    .await
    .context("parking job")?;
    Ok(())
}

/// Clear the parking marks after a chunk goes through, so the backoff ladder starts from
/// scratch the next time the provider pushes back.
pub async fn clear_wait(pool: &PgPool, job_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET resume_at = NULL, wait_reason = NULL, wait_count = 0 \
         WHERE id = $1 AND (resume_at IS NOT NULL OR wait_count > 0)",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Current status of one job — the worker polls this between chunks to notice a cancel.
pub async fn job_status(pool: &PgPool, job_id: Uuid) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT status FROM histdata_jobs WHERE id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0))
}

/// Ask a job to stop. A job that has not started yet (or is parked) ends immediately as
/// `cancelled`; a running one is marked `cancelling` and the worker finishes it at the next
/// chunk boundary, keeping the bars already written. Returns the new status, or None when
/// the job is already finished (nothing to cancel).
pub async fn cancel_job(pool: &PgPool, job_id: Uuid) -> anyhow::Result<Option<String>> {
    let sql = format!(
        "UPDATE histdata_jobs SET \
             status = CASE WHEN status = 'running' THEN 'cancelling' ELSE 'cancelled' END, \
             finished_at = CASE WHEN status = 'running' THEN finished_at ELSE now() END, \
             resume_at = NULL \
         WHERE id = $1 AND status IN {LIVE_STATUSES} \
         RETURNING status"
    );
    let row: Option<(String,)> = sqlx::query_as(AssertSqlSafe(sql))
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .context("cancelling job")?;
    Ok(row.map(|r| r.0))
}

/// Cancel every job of a batch that has not finished. Returns how many were affected.
pub async fn cancel_batch(pool: &PgPool, batch_id: Uuid) -> anyhow::Result<u64> {
    let sql = format!(
        "UPDATE histdata_jobs SET \
             status = CASE WHEN status = 'running' THEN 'cancelling' ELSE 'cancelled' END, \
             finished_at = CASE WHEN status = 'running' THEN finished_at ELSE now() END, \
             resume_at = NULL \
         WHERE batch_id = $1 AND status IN {LIVE_STATUSES}"
    );
    let res = sqlx::query(AssertSqlSafe(sql))
        .bind(batch_id)
        .execute(pool)
        .await
        .context("cancelling batch")?;
    Ok(res.rows_affected())
}

/// How a batch stands: one (status, count) row per status present.
pub async fn batch_counts(pool: &PgPool, batch_id: Uuid) -> anyhow::Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT status, count(*) FROM histdata_jobs WHERE batch_id = $1 GROUP BY status",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await
    .context("counting batch jobs")?)
}

/// Persist progress mid-download so the page reflects it and a crash can resume.
pub async fn update_job_progress(
    pool: &PgPool,
    job_id: Uuid,
    chunks_done: i32,
    chunks_total: i32,
    bars_written: i64,
    cursor: Option<OffsetDateTime>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET chunks_done = $2, chunks_total = $3, \
         bars_written = $4, chunk_cursor = $5 WHERE id = $1",
    )
    .bind(job_id)
    .bind(chunks_done)
    .bind(chunks_total)
    .bind(bars_written)
    .bind(cursor)
    .execute(pool)
    .await?;
    Ok(())
}

/// Terminal state: `done`, `partial`, or `error` (with message).
pub async fn finish_job(
    pool: &PgPool,
    job_id: Uuid,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_jobs SET status = $2, error = $3, finished_at = now() WHERE id = $1",
    )
    .bind(job_id)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Newest bar timestamp held for a dataset, or `None` if it has no bars yet. Used by the
/// live worker to seed/backfill only the gap between stored history and now.
pub async fn latest_bar_ts(
    pool: &PgPool,
    dataset_id: Uuid,
) -> anyhow::Result<Option<OffsetDateTime>> {
    // MAX over a possibly-empty set yields one NULL row → Option, fetch_one.
    let row: (Option<OffsetDateTime>,) =
        sqlx::query_as("SELECT max(ts) FROM histdata_bars WHERE dataset_id = $1")
            .bind(dataset_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

/// Mirror the worst job outcome onto the dataset status for the catalog view.
pub async fn set_dataset_status(pool: &PgPool, id: Uuid, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE histdata_datasets SET status = $2, last_updated = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Intrabar cache ─────────────────────────────────────────────────────────────

/// One instrument at one lower timeframe: the key of the intrabar cache.
#[derive(Debug, Clone)]
pub struct IntrabarKey {
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
}

/// A cached lower-timeframe candle, with its bid/ask when the provider sent both.
#[derive(Debug, Clone)]
pub struct IntrabarBar {
    pub ts: OffsetDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub quote: Option<Quote>,
}

/// Whether the window starting at `from` was already fetched (whatever it held).
pub async fn intrabar_window_known(pool: &PgPool, k: &IntrabarKey, from: OffsetDateTime) -> anyhow::Result<bool> {
    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT bars FROM histdata_intrabar_windows \
         WHERE provider = $1 AND asset_type = $2 AND ticker = $3 AND timeframe = $4 AND from_ts = $5",
    )
    .bind(&k.provider)
    .bind(&k.asset_type)
    .bind(&k.ticker)
    .bind(&k.timeframe)
    .bind(from)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Cached candles stamped in `[from, to)`, ascending.
pub async fn read_intrabar(
    pool: &PgPool,
    k: &IntrabarKey,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> anyhow::Result<Vec<IntrabarBar>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        ts: OffsetDateTime,
        o: f64,
        h: f64,
        l: f64,
        c: f64,
        bo: Option<f64>,
        bh: Option<f64>,
        bl: Option<f64>,
        bc: Option<f64>,
        ao: Option<f64>,
        ah: Option<f64>,
        al: Option<f64>,
        ac: Option<f64>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT ts, open::float8 AS o, high::float8 AS h, low::float8 AS l, close::float8 AS c, \
                bid_open::float8 AS bo, bid_high::float8 AS bh, bid_low::float8 AS bl, \
                bid_close::float8 AS bc, ask_open::float8 AS ao, ask_high::float8 AS ah, \
                ask_low::float8 AS al, ask_close::float8 AS ac \
         FROM histdata_intrabar_bars \
         WHERE provider = $1 AND asset_type = $2 AND ticker = $3 AND timeframe = $4 \
           AND ts >= $5 AND ts < $6 ORDER BY ts",
    )
    .bind(&k.provider)
    .bind(&k.asset_type)
    .bind(&k.ticker)
    .bind(&k.timeframe)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let quote = match (r.bo, r.bh, r.bl, r.bc, r.ao, r.ah, r.al, r.ac) {
                (Some(bo), Some(bh), Some(bl), Some(bc), Some(ao), Some(ah), Some(al), Some(ac)) => Some(Quote {
                    bid_open: bo,
                    bid_high: bh,
                    bid_low: bl,
                    bid_close: bc,
                    ask_open: ao,
                    ask_high: ah,
                    ask_low: al,
                    ask_close: ac,
                }),
                _ => None,
            };
            IntrabarBar { ts: r.ts, open: r.o, high: r.h, low: r.l, close: r.c, quote }
        })
        .collect())
}

/// Store a fetched window and its candles in one transaction. A window is recorded even when
/// the provider had nothing for it, so it is not asked again.
pub async fn write_intrabar(
    pool: &PgPool,
    k: &IntrabarKey,
    from: OffsetDateTime,
    to: OffsetDateTime,
    bars: &[IntrabarBar],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    for chunk in bars.chunks(BAR_CHUNK) {
        let ts: Vec<OffsetDateTime> = chunk.iter().map(|b| b.ts).collect();
        let f = |g: fn(&IntrabarBar) -> f64| chunk.iter().map(g).collect::<Vec<f64>>();
        let q = |g: fn(&Quote) -> f64| chunk.iter().map(|b| b.quote.as_ref().map(g)).collect::<Vec<Option<f64>>>();
        sqlx::query(
            "INSERT INTO histdata_intrabar_bars \
               (provider, asset_type, ticker, timeframe, ts, open, high, low, close, \
                bid_open, bid_high, bid_low, bid_close, ask_open, ask_high, ask_low, ask_close) \
             SELECT $1, $2, $3, $4, * FROM UNNEST( \
               $5::timestamptz[], $6::float8[], $7::float8[], $8::float8[], $9::float8[], \
               $10::float8[], $11::float8[], $12::float8[], $13::float8[], \
               $14::float8[], $15::float8[], $16::float8[], $17::float8[]) \
             ON CONFLICT (provider, asset_type, ticker, timeframe, ts) DO UPDATE SET \
               open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, close = EXCLUDED.close, \
               bid_open = EXCLUDED.bid_open, bid_high = EXCLUDED.bid_high, \
               bid_low = EXCLUDED.bid_low, bid_close = EXCLUDED.bid_close, \
               ask_open = EXCLUDED.ask_open, ask_high = EXCLUDED.ask_high, \
               ask_low = EXCLUDED.ask_low, ask_close = EXCLUDED.ask_close",
        )
        .bind(&k.provider)
        .bind(&k.asset_type)
        .bind(&k.ticker)
        .bind(&k.timeframe)
        .bind(&ts)
        .bind(f(|b| b.open))
        .bind(f(|b| b.high))
        .bind(f(|b| b.low))
        .bind(f(|b| b.close))
        .bind(q(|x| x.bid_open))
        .bind(q(|x| x.bid_high))
        .bind(q(|x| x.bid_low))
        .bind(q(|x| x.bid_close))
        .bind(q(|x| x.ask_open))
        .bind(q(|x| x.ask_high))
        .bind(q(|x| x.ask_low))
        .bind(q(|x| x.ask_close))
        .execute(&mut *tx)
        .await
        .context("caching intrabar candles")?;
    }
    sqlx::query(
        "INSERT INTO histdata_intrabar_windows (provider, asset_type, ticker, timeframe, from_ts, to_ts, bars) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (provider, asset_type, ticker, timeframe, from_ts) DO UPDATE SET \
           to_ts = EXCLUDED.to_ts, bars = EXCLUDED.bars, fetched_at = now()",
    )
    .bind(&k.provider)
    .bind(&k.asset_type)
    .bind(&k.ticker)
    .bind(&k.timeframe)
    .bind(from)
    .bind(to)
    .bind(bars.len() as i32)
    .execute(&mut *tx)
    .await
    .context("recording an intrabar window")?;
    tx.commit().await?;
    Ok(())
}

// ── Holes (see migration 0143) ─────────────────────────────────────────────────

/// A hole already seen in a dataset, with the status of the download sent to fill it.
#[derive(Debug, sqlx::FromRow)]
pub struct GapRow {
    pub gap_from: OffsetDateTime,
    pub gap_to: OffsetDateTime,
    pub status: String,
    /// The fill download's status; None when it is gone (or was never queued).
    pub job_status: Option<String>,
}

pub async fn gap_rows(pool: &PgPool, dataset_id: Uuid) -> anyhow::Result<Vec<GapRow>> {
    Ok(sqlx::query_as(
        "SELECT g.gap_from, g.gap_to, g.status, j.status AS job_status \
         FROM histdata_gaps g LEFT JOIN histdata_jobs j ON j.id = g.job_id \
         WHERE g.dataset_id = $1",
    )
    .bind(dataset_id)
    .fetch_all(pool)
    .await
    .context("reading dataset holes")?)
}

/// Remember the download sent to fill a hole (a retry replaces the previous one).
pub async fn record_gap_fill(
    pool: &PgPool,
    dataset_id: Uuid,
    from: OffsetDateTime,
    to: OffsetDateTime,
    job_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO histdata_gaps (dataset_id, gap_from, gap_to, job_id, status) \
         VALUES ($1, $2, $3, $4, 'filling') \
         ON CONFLICT (dataset_id, gap_from, gap_to) \
         DO UPDATE SET job_id = EXCLUDED.job_id, status = 'filling', checked_at = now()",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .bind(job_id)
    .execute(pool)
    .await
    .context("recording a hole fill")?;
    Ok(())
}

/// The provider has nothing for this hole: stop asking.
pub async fn confirm_gap(
    pool: &PgPool,
    dataset_id: Uuid,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE histdata_gaps SET status = 'confirmed', checked_at = now() \
         WHERE dataset_id = $1 AND gap_from = $2 AND gap_to = $3",
    )
    .bind(dataset_id)
    .bind(from)
    .bind(to)
    .execute(pool)
    .await
    .context("confirming a hole")?;
    Ok(())
}

#[cfg(test)]
mod rebase_tests {
    use super::*;

    /// A 2:1 split after the first download: the provider's factor on a shared bar halves, so the
    /// stored history moves by 0.5. One shared bar is enough, an unsettled close does not matter.
    #[test]
    fn a_split_rescales_the_adjusted_history() {
        let shared = [(101.0, Some(101.0), 99.0, Some(49.5))];
        assert_eq!(rebase_factor(&shared, true), Some(Rebase::Adjusted(0.5)));
        let same = [(100.0, Some(98.0), 100.0, Some(98.0))];
        assert_eq!(rebase_factor(&same, true), None);
    }

    /// Raw prices folded with splits: three shared bars agreeing on the move are needed, and a
    /// crypto dataset never rebases on raw prices.
    #[test]
    fn a_folded_split_needs_three_agreeing_bars() {
        let split = [(100.0, None, 25.0, None), (102.0, None, 25.5, None), (98.0, None, 24.5, None)];
        assert_eq!(rebase_factor(&split, true), Some(Rebase::Raw(0.25)));
        assert_eq!(rebase_factor(&split, false), None);
        let revised = [(100.0, None, 97.0, None), (102.0, None, 102.0, None), (98.0, None, 98.0, None)];
        assert_eq!(rebase_factor(&revised, true), None);
        assert_eq!(rebase_factor(&split[..2], true), None);
    }
}
