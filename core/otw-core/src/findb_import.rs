//! FinanceDatabase importer.
//!
//! On module install we download a single zstd-compressed tarball of the upstream CSVs
//! (hosted as a GitHub Release asset — free, unmetered, no Docker pull limits), decompress
//! it, and bulk-load every instrument into Postgres. The catalog lives in PG afterward;
//! search never touches the network. Re-running replaces the catalog and is idempotent.
//!
//! The archive is expected to contain the upstream `database/` layout:
//!   database/equities/<EXCHANGE>.csv   (sharded by exchange code)
//!   database/etfs/<EXCHANGE>.csv
//!   database/funds/<EXCHANGE>.csv
//!   database/indices.csv
//!   database/moneymarkets.csv
//!   database/currencies.csv
//!   database/cryptos.csv

use std::io::Read;

use anyhow::Context;
use sqlx::PgPool;

use otw_store::findb::{self, ImportRow};

/// Default release asset URL. Overridable via the `FINDB_ARCHIVE_URL` env var so the
/// archive host can be chosen at deploy time without a rebuild.
const DEFAULT_ARCHIVE_URL: &str =
    "https://github.com/G-OTW/OpenTraderWorld/releases/download/findb-data/findb.tar.zst";

/// Rows per multi-row INSERT. ~13 cols × 1000 = 13k bind params, under Postgres' 65535 cap.
const BATCH: usize = 1000;

fn archive_url() -> String {
    // Treat unset AND empty/whitespace as "use the default". Compose commonly renders an
    // unset var as an empty string (`FINDB_ARCHIVE_URL: ${FINDB_ARCHIVE_URL:-}`), which is
    // Ok("") — not Err — so a plain unwrap_or_else would keep the empty value and fall into
    // the local-file branch of fetch_archive, failing with an empty path.
    non_empty_env("FINDB_ARCHIVE_URL").unwrap_or_else(|| DEFAULT_ARCHIVE_URL.to_string())
}

/// A short version tag for the installed dataset (the release tag or a content hash).
fn archive_version() -> String {
    non_empty_env("FINDB_ARCHIVE_VERSION").unwrap_or_else(|| "findb-data".to_string())
}

/// Read an env var, returning `None` when it is unset OR empty/whitespace-only.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Fetch the archive bytes from `src`, which is one of:
///   - a local path or `file://…` URI — read from disk (no network; best for local testing);
///   - an `http(s)://` URL — downloaded. If `GITHUB_TOKEN` is set, an `Authorization: Bearer`
///     header + `Accept: application/octet-stream` are sent, so a **private** GitHub Release
///     API asset URL works. The header is harmless against a public asset URL, so the same
///     build serves both private-now and public-later without changes.
async fn fetch_archive(src: &str) -> anyhow::Result<Vec<u8>> {
    // Local file (explicit file:// or a path that isn't an http URL).
    if let Some(path) = src.strip_prefix("file://") {
        tracing::info!("findb: reading archive from {path}");
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("reading archive file {path}"));
    }
    if !src.starts_with("http://") && !src.starts_with("https://") {
        tracing::info!("findb: reading archive from {src}");
        return tokio::fs::read(src)
            .await
            .with_context(|| format!("reading archive file {src}"));
    }

    tracing::info!("findb: downloading archive {src}");
    let mut req = reqwest::Client::new()
        .get(src)
        // GitHub requires a User-Agent; the asset endpoint needs the octet-stream Accept.
        .header(reqwest::header::USER_AGENT, "otw-findb-importer")
        .header(reqwest::header::ACCEPT, "application/octet-stream");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let bytes = crate::rate::send("findb", req)
        .await
        .context("requesting findb archive")?
        .error_for_status()
        .context("findb archive download failed")?
        .bytes()
        .await
        .context("reading findb archive body")?;
    Ok(bytes.to_vec())
}

/// Download + import the full catalog. Returns the number of instruments loaded.
/// Long-running (tens of seconds); callers should run it off the request path.
pub async fn run(pool: &PgPool) -> anyhow::Result<i64> {
    let bytes = fetch_archive(&archive_url()).await?;
    tracing::info!("findb: got {} bytes, importing", bytes.len());

    findb::truncate_instruments(pool).await?;

    // Parse the tarball on a blocking thread (CPU-bound), streaming batches back over a
    // channel to async DB inserts. Keeps memory bounded — we never hold all 300k rows.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<ImportRow>>(8);
    let parse = tokio::task::spawn_blocking(move || parse_archive(&bytes, &tx));

    let mut total: i64 = 0;
    while let Some(batch) = rx.recv().await {
        total += batch.len() as i64;
        findb::insert_batch(pool, &batch).await?;
    }
    parse.await.context("findb parse task panicked")??;

    findb::set_meta(pool, &archive_version(), total).await?;
    tracing::info!("findb: imported {total} instruments");
    Ok(total)
}

/// Decompress the zstd tarball and walk its CSV entries, emitting batches of rows.
fn parse_archive(bytes: &[u8], tx: &tokio::sync::mpsc::Sender<Vec<ImportRow>>) -> anyhow::Result<()> {
    let decoder = zstd::stream::read::Decoder::new(bytes).context("opening zstd decoder")?;
    let mut archive = tar::Archive::new(decoder);
    let mut batch: Vec<ImportRow> = Vec::with_capacity(BATCH);

    for entry in archive.entries().context("reading tar entries")? {
        let mut entry = entry.context("reading tar entry")?;
        let path = entry.path().context("entry path")?.to_path_buf();
        let Some(asset) = classify(&path) else { continue };

        let mut buf = String::new();
        entry.read_to_string(&mut buf).ok();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_reader(buf.as_bytes());
        let headers = rdr.headers().cloned().unwrap_or_default();
        let idx = |name: &str| headers.iter().position(|h| h == name);
        let (
            i_symbol, i_name, i_currency, i_exchange, i_summary, i_sector, i_industry, i_country,
            i_mcap, i_isin, i_cat, i_catg, i_family,
        ) = (
            idx("symbol"), idx("name"), idx("currency"), idx("exchange"), idx("summary"),
            idx("sector"), idx("industry"), idx("country"), idx("market_cap"), idx("isin"),
            idx("category"), idx("category_group"), idx("family"),
        );

        for rec in rdr.records().flatten() {
            let get = |i: Option<usize>| i.and_then(|i| rec.get(i)).unwrap_or("").to_string();
            let symbol = get(i_symbol);
            if symbol.is_empty() {
                continue;
            }
            batch.push(ImportRow {
                asset_type: asset.to_string(),
                symbol,
                name: get(i_name),
                currency: get(i_currency),
                exchange: get(i_exchange),
                summary: get(i_summary),
                sector: get(i_sector),
                industry: get(i_industry),
                country: get(i_country),
                market_cap: get(i_mcap),
                isin: get(i_isin),
                // Funds/ETFs use `category`; indices use `category_group` as the label.
                category: { let c = get(i_cat); if c.is_empty() { get(i_catg) } else { c } },
                family: get(i_family),
            });
            if batch.len() >= BATCH {
                let full = std::mem::replace(&mut batch, Vec::with_capacity(BATCH));
                if tx.blocking_send(full).is_err() {
                    return Ok(()); // receiver gone (DB error upstream)
                }
            }
        }
    }
    if !batch.is_empty() {
        let _ = tx.blocking_send(batch);
    }
    Ok(())
}

/// Map an archive path to an asset_type, or None for non-CSV / unknown entries.
fn classify(path: &std::path::Path) -> Option<&'static str> {
    let s = path.to_string_lossy();
    if !s.ends_with(".csv") {
        return None;
    }
    if s.contains("/equities/") {
        Some("equity")
    } else if s.contains("/etfs/") {
        Some("etf")
    } else if s.contains("/funds/") {
        Some("fund")
    } else if s.ends_with("indices.csv") {
        Some("index")
    } else if s.ends_with("moneymarkets.csv") {
        Some("moneymarket")
    } else if s.ends_with("currencies.csv") {
        Some("currency")
    } else if s.ends_with("cryptos.csv") {
        Some("crypto")
    } else {
        None
    }
}
