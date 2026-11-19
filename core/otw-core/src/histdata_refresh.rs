//! Scheduled refresh of stored datasets.
//!
//! A dataset with `refresh_every_secs` set has its trailing `refresh_window_bars` bars pulled
//! again on that cadence and upserted with `write_bars`, so a stored series keeps up (and a
//! revised bar is corrected) without anyone queueing a download. Where the bars come from
//! depends on the dataset:
//!
//! - a downloaded dataset (any provider, the `http` kind included) is fetched through its
//!   connector's own `fetch_chunk`, paged by `max_bars_per_req`, spaced by `min_interval_ms`
//!   and counted against the connector's declared quota, which it never overruns;
//! - an imported dataset is re-read from where its file lives, a URL (under the `http`
//!   kind's outbound guard) or a path under the mounted import folder (`OTW_IMPORT_DIR`),
//!   with the mapping its last commit saved;
//! - a push source has nothing to fetch, and an import with no source has nothing to read:
//!   both are skipped with the reason recorded on the dataset.
//!
//! It runs here rather than in `otw-scheduler`: that crate only knows news feeds, and the
//! connectors, the quota and the outbound guard all live in this one. Its own loop, not the
//! download worker's: the worker drains a queue serially, and a refresh must not wait behind
//! a ten-year download, nor show up in the jobs list every minute.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use sqlx::PgPool;
use time::OffsetDateTime;

use otw_store::connectors as conn_store;
use otw_store::histdata::{self as store, Dataset};

use crate::histdata::{self, kinds};

/// The shortest cadence a dataset may ask for.
pub const MIN_EVERY_SECS: i32 = 60;
/// The longest: a refresh once a month is already a manual download.
pub const MAX_EVERY_SECS: i32 = 31 * 86_400;
/// Bars re-pulled when the page sets none.
pub const DEFAULT_WINDOW: i32 = 200;
pub const MAX_WINDOW: i32 = 10_000;
/// The folder a re-import may read from, when `OTW_IMPORT_DIR` is not set.
pub const DEFAULT_IMPORT_DIR: &str = "/data/import";
/// How often the loop looks for due datasets.
const TICK: Duration = Duration::from_secs(30);
/// Refreshes per pass, so one pass never holds a provider for long.
const MAX_PER_TICK: usize = 8;
/// A re-imported file is read whole into memory: the import's own cap.
const MAX_FILE_BYTES: usize = 30 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// Where a due dataset's bars come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The provider's own fetch path.
    Fetch,
    /// An imported file at a URL.
    Url(String),
    /// An imported file under the import folder (the path as stored, relative to it).
    Path(String),
}

/// Whether a dataset's refresh is due at `now`: on, and never run or run at least its
/// cadence ago.
pub fn due(ds: &Dataset, now: OffsetDateTime) -> bool {
    let Some(every) = ds.refresh_every_secs else { return false };
    ds.last_refresh_at.is_none_or(|t| (now - t).whole_seconds() >= i64::from(every.max(MIN_EVERY_SECS)))
}

/// Where the refresh reads from, or why there is nothing to refresh. The reason is what the
/// dataset page shows, so it names the fix.
pub fn plan(ds: &Dataset) -> std::result::Result<Source, String> {
    if ds.provider == histdata::IMPORT_PROVIDER {
        let url = ds.import_source_url.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let path = ds.import_source_path.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let source = match (url, path) {
            (Some(_), Some(_)) => return Err("set a source URL or a folder path, not both".into()),
            (Some(u), None) => Source::Url(u.to_string()),
            (None, Some(p)) => Source::Path(p.to_string()),
            (None, None) => {
                return Err("an imported file has nothing to re-read: set the URL or the folder path it comes from".into())
            }
        };
        if !ds.has_import_mapping() {
            return Err("this dataset was imported before mappings were saved: import the file once more, then turn the refresh on".into());
        }
        return Ok(source);
    }
    if kinds::kind_of(&ds.provider) == Some(kinds::push::KIND) {
        return Err("a push source has nothing to fetch: its script pushes the bars to the webhook".into());
    }
    Ok(Source::Fetch)
}

/// The import folder: `OTW_IMPORT_DIR`, else [`DEFAULT_IMPORT_DIR`].
pub fn import_root() -> PathBuf {
    std::env::var("OTW_IMPORT_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_IMPORT_DIR))
}

/// A stored path, checked by its words alone: relative, and with no `..`, so it cannot name
/// anything outside the folder whatever the folder is.
pub fn check_relative(rel: &str) -> Result<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        bail!("the folder path is empty");
    }
    let p = Path::new(rel);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => bail!("{rel:?} climbs out of the import folder (..): give a path inside it"),
            Component::RootDir | Component::Prefix(_) => {
                bail!("{rel:?} is absolute: give a path relative to the import folder")
            }
        }
    }
    if out.as_os_str().is_empty() {
        bail!("{rel:?} names the import folder itself, not a file in it");
    }
    Ok(out)
}

/// The file a stored path names under `root`, refused when it leaves the folder: by its words
/// ([`check_relative`]) and by where it lands once links are followed, so a symlink planted in
/// the folder cannot point the server at `/etc`.
pub fn resolve_import_path(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = check_relative(rel)?;
    let root = root
        .canonicalize()
        .with_context(|| format!("the import folder {} is not mounted", root.display()))?;
    let full = root
        .join(&rel)
        .canonicalize()
        .with_context(|| format!("{} is not in the import folder", rel.display()))?;
    if !full.starts_with(&root) {
        bail!("{} leads outside the import folder: links out of it are refused", rel.display());
    }
    if !full.is_file() {
        bail!("{} is not a file", rel.display());
    }
    Ok(full)
}

/// Download a file for a re-import, under the `http` kind's outbound guard: the host is
/// resolved and a loopback, link-local or private address refused, the checked address is
/// pinned and redirects are not followed.
pub async fn download(url: &str) -> Result<Vec<u8>> {
    let parsed = reqwest::Url::parse(url.trim()).map_err(|e| anyhow!("the source URL is not a URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("the source URL must start with http:// or https://");
    }
    let pin = kinds::http::check_host(&parsed, false).await.map_err(|e| {
        anyhow!("{e:#}. For a file on your own network, put it in the import folder and set a folder path instead")
    })?;
    let mut builder = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent("OpenTraderWorld/histdata")
        .redirect(reqwest::redirect::Policy::none());
    if let Some((host, addr)) = pin {
        builder = builder.resolve(&host, addr);
    }
    let client = builder.build().map_err(|e| anyhow!("building the http client: {e}"))?;
    let mut resp = client.get(parsed).send().await.map_err(|e| anyhow!("fetching the source URL: {}", e.without_url()))?;
    let status = resp.status();
    if status.is_redirection() {
        let to = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("?");
        bail!("the source URL answered {status}, a redirect to {to}: redirects are not followed, set the final URL");
    }
    if !status.is_success() {
        bail!("the source URL answered {status}");
    }
    let mut bytes = Vec::new();
    while let Some(part) = resp.chunk().await.map_err(|e| anyhow!("reading the source URL: {}", e.without_url()))? {
        bytes.extend_from_slice(&part);
        if bytes.len() > MAX_FILE_BYTES {
            bail!("the file at the source URL is past {} MB", MAX_FILE_BYTES >> 20);
        }
    }
    if bytes.is_empty() {
        bail!("the source URL answered an empty file");
    }
    Ok(bytes)
}

/// The bars a re-import writes: the file read with the saved mapping, its trailing window.
pub fn reimport_bars(ds: &Dataset, bytes: &[u8]) -> Result<Vec<store::Bar>> {
    let saved = ds.import_mapping.as_ref().ok_or_else(|| anyhow!("no saved mapping"))?;
    let filename = saved.get("filename").and_then(|v| v.as_str()).unwrap_or("data.csv").to_string();
    let mapping: crate::histdata_import::Mapping = serde_json::from_value(saved.get("mapping").cloned().unwrap_or_default())
        .map_err(|e| anyhow!("the saved mapping does not read ({e}): import the file once more"))?;
    let destination = crate::histdata_import::Destination {
        asset_type: ds.asset_type.clone(),
        ticker: ds.ticker.clone(),
        timeframe: ds.timeframe.clone(),
        source: ds.source.clone(),
        label: ds.label.clone().unwrap_or_default(),
        tags: Vec::new(),
        ..Default::default()
    };
    let mut bars = crate::histdata_import::series(&filename, bytes, &mapping, &destination)?;
    let keep = usize::try_from(ds.refresh_window_bars.clamp(1, MAX_WINDOW)).unwrap_or(DEFAULT_WINDOW as usize);
    if bars.len() > keep {
        bars.drain(..bars.len() - keep);
    }
    Ok(bars)
}

pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            // The demo's datasets are a seed every visitor shares: nothing rewrites them.
            if crate::demo::enabled() {
                continue;
            }
            if let Err(e) = tick(&pool).await {
                tracing::warn!("histdata refresh: {e:#}");
            }
        }
    });
}

async fn tick(pool: &PgPool) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let due_now: Vec<Dataset> = store::refresh_candidates(pool).await?.into_iter().filter(|d| due(d, now)).collect();
    for ds in due_now.into_iter().take(MAX_PER_TICK) {
        let outcome = refresh_one(pool, &ds).await;
        let at = OffsetDateTime::now_utc();
        match outcome {
            Ok(n) => {
                tracing::debug!("histdata refresh: {} {} {}: {n} bars", ds.provider, ds.ticker, ds.timeframe);
                store::record_refresh(pool, ds.id, at, None).await?;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::info!("histdata refresh: {} {} {}: {msg}", ds.provider, ds.ticker, ds.timeframe);
                store::record_refresh(pool, ds.id, at, Some(&msg)).await?;
            }
        }
    }
    Ok(())
}

/// Refresh one dataset now. Returns the bars written.
pub async fn refresh_one(pool: &PgPool, ds: &Dataset) -> Result<usize> {
    match plan(ds).map_err(|reason| anyhow!(reason))? {
        Source::Fetch => refresh_fetch(pool, ds).await,
        Source::Url(url) => {
            let bytes = download(&url).await?;
            write(pool, ds, reimport_bars(ds, &bytes)?).await
        }
        Source::Path(rel) => {
            let path = resolve_import_path(&import_root(), &rel)?;
            let bytes = tokio::fs::read(&path).await.with_context(|| format!("reading {rel}"))?;
            if bytes.len() > MAX_FILE_BYTES {
                bail!("{rel} is past {} MB", MAX_FILE_BYTES >> 20);
            }
            write(pool, ds, reimport_bars(ds, &bytes)?).await
        }
    }
}

async fn write(pool: &PgPool, ds: &Dataset, bars: Vec<store::Bar>) -> Result<usize> {
    if bars.is_empty() {
        bail!("the file holds no closed bar");
    }
    Ok(store::write_bars(pool, ds.id, &bars).await? as usize)
}

/// The trailing window through the connector, chunk by chunk, under its pacing and quota.
async fn refresh_fetch(pool: &PgPool, ds: &Dataset) -> Result<usize> {
    let connector = histdata::connector_for(&ds.provider)?;
    let cap = connector.capability();
    if !cap.timeframes.contains(&ds.timeframe.as_str()) {
        bail!("{} does not serve {}: nothing to refresh at this timeframe", cap.label, ds.timeframe);
    }
    let connector_id = conn_store::default_for(pool, &ds.provider)
        .await?
        .map(|c| c.id)
        .ok_or_else(|| anyhow!("no {} connector is configured: add one in the data broker", cap.label))?;
    let secrets: HashMap<String, String> = conn_store::load_creds(pool, crate::histdata_cipher::get(), connector_id).await?;
    let scope = format!("histconn:{connector_id}");
    let min_interval = Duration::from_millis(cap.min_interval_ms);

    let tf_secs = histdata::timeframe_secs(&ds.timeframe)?;
    let window = i64::from(ds.refresh_window_bars.clamp(1, MAX_WINDOW));
    let to = OffsetDateTime::now_utc();
    let from = to - time::Duration::seconds(tf_secs * window);
    let step = time::Duration::seconds(tf_secs * i64::from(cap.max_bars_per_req.max(1)));
    let client = histdata::client()?;
    let mut cursor = from;
    let mut written = 0usize;
    while cursor < to {
        // The quota is the user's ceiling: a refresh waits for the next window, it never
        // spends past it.
        if let Some(q) = otw_store::api_quota::get(pool, &scope).await.ok().flatten() {
            if q.max_requests.is_some_and(|max| q.used >= max) {
                bail!(
                    "the connector's quota is spent until {}: the refresh resumes after it",
                    q.resets_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
                );
            }
        }
        let chunk_to = (cursor + step).min(to);
        if let Err(e) = otw_store::api_quota::bump(pool, &scope).await {
            tracing::debug!("histdata refresh: quota bump failed: {e:#}");
        }
        crate::histdata_job::pace(&ds.provider, min_interval).await;
        let chunk = connector
            .fetch_chunk(&client, &secrets, &ds.ticker, &ds.asset_type, &ds.timeframe, cursor, chunk_to)
            .await?;
        let chunk = histdata::closed_only(chunk, &ds.timeframe, OffsetDateTime::now_utc());
        if !chunk.bars.is_empty() {
            written += store::write_bars(pool, ds.id, &chunk.bars).await? as usize;
            if !chunk.quotes.is_empty() {
                store::write_quotes(pool, ds.id, &chunk.quotes).await?;
            }
        }
        cursor = chunk_to;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::macros::datetime;

    fn dataset(provider: &str) -> Dataset {
        Dataset {
            id: uuid::Uuid::nil(),
            provider: provider.into(),
            asset_type: "equity".into(),
            ticker: "ACME".into(),
            timeframe: "1d".into(),
            range_from: None,
            range_to: None,
            bar_count: 0,
            size_bytes: 0,
            gaps: json!([]),
            status: "complete".into(),
            last_updated: OffsetDateTime::UNIX_EPOCH,
            label: None,
            source: "broker".into(),
            tags: json!([]),
            refresh_every_secs: Some(300),
            refresh_window_bars: 200,
            last_refresh_at: None,
            last_refresh_error: None,
            import_source_url: None,
            import_source_path: None,
            import_mapping: None,
        }
    }

    #[test]
    fn a_refresh_is_due_once_its_cadence_has_passed() {
        let now = datetime!(2024-01-01 12:00:00 UTC);
        let mut ds = dataset("yahoo");
        // Never run: due at once.
        assert!(due(&ds, now));
        ds.last_refresh_at = Some(now - time::Duration::seconds(299));
        assert!(!due(&ds, now));
        ds.last_refresh_at = Some(now - time::Duration::seconds(300));
        assert!(due(&ds, now));
        // Off is never due, whenever it last ran.
        ds.refresh_every_secs = None;
        assert!(!due(&ds, now));
        // A cadence under the floor is read at the floor.
        ds.refresh_every_secs = Some(5);
        ds.last_refresh_at = Some(now - time::Duration::seconds(30));
        assert!(!due(&ds, now));
    }

    #[test]
    fn push_and_sourceless_imports_are_skipped_with_a_reason() {
        assert_eq!(plan(&dataset("yahoo")), Ok(Source::Fetch));
        assert_eq!(plan(&dataset("http-1a2b3c4d")), Ok(Source::Fetch));
        let push = plan(&dataset("push-1a2b3c4d")).unwrap_err();
        assert!(push.contains("push source"), "{push}");

        let mut imp = dataset("import");
        let none = plan(&imp).unwrap_err();
        assert!(none.contains("nothing to re-read"), "{none}");
        imp.import_source_url = Some("https://example.com/acme.csv".into());
        let unmapped = plan(&imp).unwrap_err();
        assert!(unmapped.contains("import the file once more"), "{unmapped}");
        imp.import_mapping = Some(json!({ "filename": "acme.csv", "mapping": {} }));
        assert_eq!(plan(&imp), Ok(Source::Url("https://example.com/acme.csv".into())));
        imp.import_source_path = Some("acme.csv".into());
        assert!(plan(&imp).unwrap_err().contains("not both"));
        imp.import_source_url = None;
        assert_eq!(plan(&imp), Ok(Source::Path("acme.csv".into())));
    }

    #[test]
    fn a_path_leaving_the_import_folder_is_refused() {
        for bad in ["../etc/passwd", "a/../../b.csv", "/etc/passwd", "", "."] {
            assert!(check_relative(bad).is_err(), "{bad:?} must be refused");
        }
        assert_eq!(check_relative("./broker/acme.csv").unwrap(), PathBuf::from("broker/acme.csv"));

        let root = std::env::temp_dir().join(format!("otw-import-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(root.join("broker")).unwrap();
        std::fs::write(root.join("broker/acme.csv"), "x").unwrap();
        let outside = std::env::temp_dir().join(format!("otw-outside-{}.csv", uuid::Uuid::new_v4().simple()));
        std::fs::write(&outside, "secret").unwrap();
        assert!(resolve_import_path(&root, "broker/acme.csv").is_ok());
        assert!(resolve_import_path(&root, "broker/missing.csv").is_err());
        assert!(resolve_import_path(&root, "broker").unwrap_err().to_string().contains("not a file"));
        // A link planted in the folder that points out of it is refused where it lands.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("escape.csv")).unwrap();
            let err = resolve_import_path(&root, "escape.csv").unwrap_err().to_string();
            assert!(err.contains("outside the import folder"), "{err}");
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn a_url_reimport_reads_the_file_with_the_saved_mapping() {
        // A semicolon export with a comma decimal and day-first dates: nothing a fresh
        // detection would be sure of, everything the saved mapping says.
        let file = "Date;Close\n02/01/2024;101,5\n03/01/2024;102,25\n04/01/2024;103\n";
        let mapping = crate::histdata_import::Mapping {
            header_row: Some(0),
            delimiter: Some(";".into()),
            decimal: ",".into(),
            date_order: "dmy".into(),
            ts_unit: "text".into(),
            columns: [("0".to_string(), "ts".to_string()), ("1".to_string(), "close".to_string())].into_iter().collect(),
            ..Default::default()
        };
        let mut ds = dataset("import");
        ds.import_source_url = Some("https://example.com/acme.csv".into());
        ds.import_mapping = Some(json!({ "filename": "acme.csv", "mapping": mapping }));
        let bars = reimport_bars(&ds, file.as_bytes()).unwrap();
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0].ts, datetime!(2024-01-02 00:00:00 UTC));
        assert_eq!(bars[0].close, 101.5);
        assert_eq!(bars[1].close, 102.25);
        // Only the trailing window is written again.
        ds.refresh_window_bars = 2;
        let tail = reimport_bars(&ds, file.as_bytes()).unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].ts, datetime!(2024-01-03 00:00:00 UTC));
        // A mapping that no longer reads is named.
        ds.import_mapping = Some(json!({ "filename": "acme.csv", "mapping": { "columns": 3 } }));
        assert!(reimport_bars(&ds, file.as_bytes()).unwrap_err().to_string().contains("import the file once more"));
    }
}
