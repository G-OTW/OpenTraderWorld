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

/// Name of the snapshot manifest published next to the archive. It is fetched from the
/// archive's own directory, so a mirror only has to host the two files side by side.
const MANIFEST_FILE: &str = "findb.json";

/// What the publisher says about the snapshot on offer. Written by `scripts/build-findb.sh`,
/// read both by the update check (is a newer snapshot out?) and by the import (does the
/// archive we downloaded hash to what was published?).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Manifest {
    /// Snapshot identity, a build date like `2026-09-07`. Compared as a plain string against
    /// the installed `findb_meta.version`: different means "an update is available".
    pub version: String,
    #[serde(default)]
    pub built_at: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub upstream_commit: String,
}

fn archive_url() -> String {
    // Treat unset AND empty/whitespace as "use the default". Compose commonly renders an
    // unset var as an empty string (`FINDB_ARCHIVE_URL: ${FINDB_ARCHIVE_URL:-}`), which is
    // Ok("") — not Err — so a plain unwrap_or_else would keep the empty value and fall into
    // the local-file branch of fetch_archive, failing with an empty path.
    non_empty_env("FINDB_ARCHIVE_URL").unwrap_or_else(|| DEFAULT_ARCHIVE_URL.to_string())
}

/// Version tag recorded when no manifest is published next to the archive (a private
/// mirror, a local file). The manifest wins when there is one: otherwise a fixed env value
/// would never match the published snapshot and every check would cry "update available".
fn version_fallback() -> String {
    non_empty_env("FINDB_ARCHIVE_VERSION").unwrap_or_else(|| "findb-data".to_string())
}

/// Where the snapshot manifest lives: `FINDB_MANIFEST_URL`, else the archive's own
/// location with its last path segment swapped for `findb.json`. A local archive gets a
/// local sibling, so an offline mirror can name (and checksum) its snapshot too.
fn manifest_src() -> Option<String> {
    if let Some(url) = non_empty_env("FINDB_MANIFEST_URL") {
        return Some(url);
    }
    let archive = archive_url();
    let (base, _) = archive.rsplit_once('/')?;
    Some(format!("{base}/{MANIFEST_FILE}"))
}

/// Read the published manifest. `Ok(None)` means "nothing to read": no sibling file, or an
/// archive location with no directory to look in. An `Err` means the fetch or the JSON
/// failed; either way the caller carries on, since an unreachable publisher is a normal
/// condition for a self-hosted box.
pub async fn fetch_manifest(client: &reqwest::Client) -> anyhow::Result<Option<Manifest>> {
    let Some(src) = manifest_src() else { return Ok(None) };

    // Local sibling: absent is not an error, it just means this archive names no snapshot.
    if !src.starts_with("http://") && !src.starts_with("https://") {
        let path = src.strip_prefix("file://").unwrap_or(&src);
        return match tokio::fs::read_to_string(path).await {
            Ok(text) => Ok(Some(
                serde_json::from_str(&text)
                    .with_context(|| format!("parsing findb manifest {path}"))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context(format!("reading findb manifest {path}"))),
        };
    }

    let mut req = client
        .get(&src)
        .header(reqwest::header::USER_AGENT, "otw-findb-importer")
        .header(reqwest::header::ACCEPT, "application/octet-stream");
    if let Some(token) = non_empty_env("GITHUB_TOKEN") {
        req = req.bearer_auth(token);
    }
    let text = crate::rate::send("findb", req)
        .await
        .context("requesting findb manifest")?
        .error_for_status()
        .context("findb manifest fetch failed")?
        .text()
        .await
        .context("reading findb manifest body")?;
    let manifest: Manifest = serde_json::from_str(&text).context("parsing findb manifest")?;
    Ok(Some(manifest))
}

/// Hex sha256 of the downloaded archive, compared against the manifest before we touch the
/// catalog. A truncated download must not be allowed to empty a working install.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
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
    // Best effort: the manifest names the snapshot and its checksum, but an install must
    // still work against a mirror that publishes the archive alone.
    let manifest = match fetch_manifest(&reqwest::Client::new()).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("findb: manifest unavailable, importing without it: {e:#}");
            None
        }
    };

    let bytes = fetch_archive(&archive_url()).await?;
    tracing::info!("findb: got {} bytes, importing", bytes.len());

    // Verify before truncating: a bad download leaves the existing catalog untouched.
    if let Some(expected) =
        manifest.as_ref().map(|m| m.sha256.as_str()).filter(|s| !s.is_empty())
    {
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(expected) {
            anyhow::bail!("findb archive checksum mismatch (expected {expected}, got {got})");
        }
    }

    let version = manifest
        .as_ref()
        .map(|m| m.version.clone())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(version_fallback);

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

    findb::set_meta(pool, &version, total).await?;
    // Favorites outlive the snapshot they were saved from; re-point them at the new rows.
    let missing = findb::relink_favorites(pool).await?;
    tracing::info!("findb: imported {total} instruments ({missing} favorites not in this snapshot)");
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
