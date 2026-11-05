//! Fundamentals providers: macro series and company data, as opposed to price bars.
//!
//! Each provider is still a data-broker connector (grants, credentials, quota and the rate
//! dashboard apply like anywhere else), so it implements [`Connector`] with no asset types:
//! it serves no bars and is granted to the `fundamentals` module only. What it does serve
//! is declared in `Capability.fundamentals` and fetched by the functions of its own module.
//!
//! Fetching and storing a company or a series takes seconds (EDGAR's company facts run to
//! several MB), so the API spawns [`refresh_company`] / [`refresh_series`] and the row
//! carries the outcome in `status` / `error`.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::histdata::{Capability, Chunk};
use otw_store::fundamentals as store;

pub mod alt;
pub mod datasets;
pub mod edgar;
pub mod finnhub;
pub mod fmp;
pub mod fred;
pub mod sdmx;
pub mod skips;
pub mod stats;
pub mod transcripts;
pub mod treasury;
pub mod util;
pub mod vendors;

pub use edgar::Edgar;
pub use fred::Fred;
pub use treasury::Treasury;

/// Every fundamentals-only connector, for the broker's registry.
pub fn connectors() -> Vec<Box<dyn crate::histdata::Connector>> {
    vec![
        Box::new(Edgar),
        Box::new(Fred),
        Box::new(Treasury),
        Box::new(sdmx::Ecb),
        Box::new(sdmx::Eurostat),
        Box::new(sdmx::Bis),
        Box::new(sdmx::Oecd),
        Box::new(sdmx::Imf),
        Box::new(stats::bls::Bls),
        Box::new(stats::bea::Bea),
        Box::new(stats::eia::Eia),
        Box::new(stats::census::Census),
        Box::new(stats::worldbank::WorldBank),
        Box::new(stats::cftc::Cftc),
        Box::new(vendors::finra::Finra),
        Box::new(fmp::Fmp),
        Box::new(finnhub::Finnhub),
        Box::new(alt::usaspending::UsaSpending),
        Box::new(alt::lda::Lda),
        Box::new(alt::uspto::Uspto),
        Box::new(alt::quiver::Quiver),
    ]
}

/// A provider's client and credentials for one call.
pub struct Ctx<'a> {
    pub client: &'a reqwest::Client,
    pub secrets: &'a HashMap<String, String>,
}

/// A fundamentals-only capability: no bars, no live, no settings.
pub const fn fcap(
    provider: &'static str,
    label: &'static str,
    website: &'static str,
    docs_url: &'static str,
    rate_limit: &'static str,
    required_secrets: &'static [&'static str],
    fundamentals: &'static [&'static str],
) -> Capability {
    Capability {
        provider,
        label,
        website,
        docs_url,
        rate_limit,
        required_secrets,
        asset_types: &[],
        timeframes: &[],
        adjusted: false,
        max_bars_per_req: 0,
        min_interval_ms: 500,
        searchable: false,
        config_fields: &[],
        testable: true,
        fundamentals,
        stream_asset_types: &[],
        stream_timeframes: &[],
        stream_note: "",
    }
}

/// `Connector` for a fundamentals-only provider: no bars, and a test that fetches one
/// thing it serves.
macro_rules! fundamentals_connector {
    ($ty:ident, $cap:path) => {
        #[async_trait::async_trait]
        impl crate::histdata::Connector for $ty {
            fn capability(&self) -> &'static crate::histdata::Capability {
                &$cap
            }

            async fn fetch_chunk(
                &self,
                _client: &reqwest::Client,
                _secrets: &std::collections::HashMap<String, String>,
                _ticker: &str,
                _asset_type: &str,
                _timeframe: &str,
                _from: time::OffsetDateTime,
                _to: time::OffsetDateTime,
            ) -> anyhow::Result<crate::histdata::Chunk> {
                crate::fundamentals::refuse_chunk(&$cap).await
            }

            async fn test(
                &self,
                client: &reqwest::Client,
                secrets: &std::collections::HashMap<String, String>,
            ) -> anyhow::Result<String> {
                crate::fundamentals::probe($cap.provider, client, secrets).await
            }
        }
    };
}
pub(crate) use fundamentals_connector;

/// Reach a provider with one cheap call that proves the key and the network.
pub async fn probe(provider: &str, client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
    let ctx = Ctx { client, secrets };
    match provider {
        fmp::PROVIDER => fmp::probe(&ctx).await,
        finnhub::PROVIDER => finnhub::probe(&ctx).await,
        alt::usaspending::PROVIDER => alt::usaspending::probe(&ctx).await,
        alt::lda::PROVIDER => alt::lda::probe(&ctx).await,
        alt::uspto::PROVIDER => alt::uspto::probe(&ctx).await,
        alt::quiver::PROVIDER => alt::quiver::probe(&ctx).await,
        vendors::finra::PROVIDER => {
            let v = vendors::finra::short_interest(&ctx, "AAPL").await?;
            let n = v.get("series").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
            Ok(format!("FINRA answered: {n} short-interest reports for AAPL"))
        }
        p if serves_macro(p) => {
            let first = search_series(p, client, secrets, "")
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("{p} has no catalog to test against"))?;
            let (m, obs) = fetch_series(p, client, secrets, &first.code).await?;
            Ok(format!("{}: {} observations", m.title, obs.len()))
        }
        _ => Err(anyhow!("{provider} has no connection test")),
    }
}

/// The module id connectors are granted to.
pub const MODULE: &str = "fundamentals";

/// Macro categories a series can be filed under (the Macro page groups by these).
pub const CATEGORIES: &[&str] = &[
    "growth", "inflation", "labour", "rates", "money", "surveys", "housing", "energy", "fiscal",
    "positioning", "other",
];

/// A fundamentals provider has no bars to serve; the histdata worker never gets this far
/// because the capability lists no asset type, but the trait wants an answer.
pub(crate) fn no_bars(cap: &Capability) -> anyhow::Error {
    anyhow!("{} serves fundamentals, not price bars", cap.label)
}

/// Shared `fetch_chunk` body for the fundamentals connectors.
pub(crate) async fn refuse_chunk(cap: &'static Capability) -> Result<Chunk> {
    Err(no_bars(cap))
}

/// The macro providers, by id, for search and fetch dispatch.
pub fn serves_macro(provider: &str) -> bool {
    matches!(provider, fred::PROVIDER | treasury::PROVIDER)
        || sdmx::publisher(provider).is_some()
        || matches!(
            provider,
            stats::bls::PROVIDER
                | stats::bea::PROVIDER
                | stats::eia::PROVIDER
                | stats::census::PROVIDER
                | stats::worldbank::PROVIDER
                | stats::cftc::PROVIDER
        )
}

/// The curated catalog of a provider whose own API has no free-text search.
fn catalog(provider: &str) -> Option<&'static [util::Entry]> {
    Some(match provider {
        stats::bls::PROVIDER => stats::bls::CATALOG,
        stats::bea::PROVIDER => stats::bea::CATALOG,
        stats::eia::PROVIDER => stats::eia::CATALOG,
        stats::census::PROVIDER => stats::census::CATALOG,
        stats::worldbank::PROVIDER => stats::worldbank::CATALOG,
        stats::cftc::PROVIDER => stats::cftc::CATALOG,
        p => sdmx::publisher(p)?.catalog,
    })
}

/// What a series is, before it is stored: a curated entry costs nothing, a typed code is
/// checked by fetching it (an unknown code is an error, never a row that fails later).
pub async fn series_meta(
    provider: &str,
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    code: &str,
) -> Result<store::SeriesMeta> {
    if provider == fred::PROVIDER {
        return fred::meta(client, secrets, code).await;
    }
    if let Some(e) = catalog(provider).and_then(|c| util::find_entry(c, code)) {
        return Ok(util::entry_meta(provider, e, ""));
    }
    let (m, obs) = fetch_series(provider, client, secrets, code).await?;
    if obs.is_empty() {
        return Err(anyhow!("{provider} returned no observations for {code}"));
    }
    Ok(m)
}

/// Search a macro provider's catalog.
pub async fn search_series(
    provider: &str,
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    q: &str,
) -> Result<Vec<store::SeriesMeta>> {
    match provider {
        fred::PROVIDER => fred::search(client, secrets, q).await,
        treasury::PROVIDER => Ok(treasury::search(q)),
        stats::cftc::PROVIDER => stats::cftc::search(client, q).await,
        p => match sdmx::publisher(p) {
            Some(pb) => Ok(sdmx::search(pb, q)),
            None => catalog(p)
                .map(|c| util::search_catalog(p, c, q))
                .ok_or_else(|| anyhow!("{provider} has no macro series")),
        },
    }
}

/// Metadata and full history of one series.
pub async fn fetch_series(
    provider: &str,
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    code: &str,
) -> Result<(store::SeriesMeta, Vec<(time::Date, f64)>)> {
    match provider {
        fred::PROVIDER => fred::fetch(client, secrets, code).await,
        treasury::PROVIDER => treasury::fetch(client, code).await,
        stats::bls::PROVIDER => stats::bls::fetch(client, secrets, code).await,
        stats::bea::PROVIDER => stats::bea::fetch(client, secrets, code).await,
        stats::eia::PROVIDER => stats::eia::fetch(client, secrets, code).await,
        stats::census::PROVIDER => stats::census::fetch(client, secrets, code).await,
        stats::worldbank::PROVIDER => stats::worldbank::fetch(client, code).await,
        stats::cftc::PROVIDER => stats::cftc::fetch(client, code).await,
        p => match sdmx::publisher(p) {
            Some(pb) => sdmx::fetch(pb, client, code).await,
            None => Err(anyhow!("{provider} has no macro series")),
        },
    }
}

// ── Background refresh ──────────────────────────────────────────────────────

fn running() -> &'static Mutex<HashSet<String>> {
    static RUNNING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Claim `key` for one run; false when a run for it is already in flight.
fn claim(key: &str) -> bool {
    running().lock().map(|mut s| s.insert(key.to_string())).unwrap_or(false)
}

fn release(key: &str) {
    if let Ok(mut s) = running().lock() {
        s.remove(key);
    }
}

/// Connector settings + credentials for `provider`, from the oldest connector granted to
/// Fundamentals. None = the user has not added one, and the caller names that fix.
pub async fn creds_for(
    pool: &PgPool,
    cipher: &otw_store::crypto::SecretCipher,
    provider: &str,
) -> Result<Option<(Uuid, HashMap<String, String>)>> {
    let Some(c) = otw_store::connectors::default_for_module(pool, provider, MODULE).await? else {
        return Ok(None);
    };
    let secrets = otw_store::connectors::load_creds(pool, cipher, c.id).await?;
    Ok(Some((c.id, secrets)))
}

/// The error a missing connector produces, naming the fix.
pub fn missing_connector(provider: &str) -> String {
    let label = crate::histdata::connector_for(provider)
        .map(|c| c.capability().label)
        .unwrap_or(provider);
    format!(
        "no {label} connector is granted to Fundamentals: add one from the data broker \
         (plug icon) and grant it to Fundamentals"
    )
}

/// Count one provider request against the connector's quota (display only).
pub(crate) async fn bump(pool: &PgPool, connector: Uuid) {
    let _ = otw_store::api_quota::bump(pool, &crate::connectors_api::quota_scope(connector)).await;
}

/// Fetch a series and store it, in the background. The row is already `pending`.
pub fn spawn_series_refresh(state: crate::AppState, id: Uuid, provider: String, code: String) {
    let key = format!("series:{id}");
    if !claim(&key) {
        return;
    }
    tokio::spawn(async move {
        let outcome = refresh_series(&state, id, &provider, &code).await;
        if let Err(e) = outcome {
            let msg = format!("{e:#}");
            tracing::warn!("fundamentals: series {provider}:{code} refresh failed: {msg}");
            let _ = store::set_series_status(&state.pool, id, "error", Some(&msg)).await;
        }
        release(&key);
    });
}

async fn refresh_series(
    state: &crate::AppState,
    id: Uuid,
    provider: &str,
    code: &str,
) -> Result<()> {
    let (connector, secrets) = creds_for(&state.pool, &state.cipher, provider)
        .await?
        .ok_or_else(|| anyhow!(missing_connector(provider)))?;
    bump(&state.pool, connector).await;
    // The error is stored on the row and shown: mask the connector's own credentials in it.
    let (meta, obs) = fetch_series(provider, &state.http, &secrets, code)
        .await
        .map_err(|e| anyhow!(skips::redact(&format!("{e:#}"), &secrets)))?;
    if obs.is_empty() {
        return Err(anyhow!("{provider} returned no observations for {code}"));
    }
    // Title, unit and frequency follow the provider (and the curated catalog); the
    // category stays the user's.
    store::upsert_series(&state.pool, &meta).await?;
    store::replace_observations(&state.pool, id, &obs).await
}

/// Fetch a company's profile, statements, filings and insider trades from EDGAR and store
/// them, in the background. The row is already `pending`.
pub fn spawn_company_refresh(state: crate::AppState, id: Uuid, ticker: String, cik: String) {
    let key = format!("company:{id}");
    if !claim(&key) {
        return;
    }
    tokio::spawn(async move {
        match edgar::refresh(&state, id, &ticker, &cik).await {
            Ok(()) => {
                let _ = store::set_company_status(&state.pool, id, "ok", None).await;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::warn!("fundamentals: {ticker} refresh failed: {msg}");
                let _ = store::set_company_status(&state.pool, id, "error", Some(&msg)).await;
            }
        }
        release(&key);
    });
}

/// Refresh a stored series now and wait for it (the scheduled refresh). Skipped (Ok
/// false) when a refresh of it is already in flight; a failure is stored on the row.
pub async fn refresh_series_now(state: &crate::AppState, s: &store::SeriesRow) -> Result<bool> {
    let key = format!("series:{}", s.id);
    if !claim(&key) {
        return Ok(false);
    }
    let outcome = refresh_series(state, s.id, &s.provider, &s.code).await;
    release(&key);
    if let Err(e) = &outcome {
        let _ = store::set_series_status(&state.pool, s.id, "error", Some(&format!("{e:#}"))).await;
    }
    outcome.map(|_| true)
}

/// Refresh a stored company from EDGAR now and wait for it. Same contract as
/// [`refresh_series_now`].
pub async fn refresh_company_now(state: &crate::AppState, c: &store::CompanyRow) -> Result<bool> {
    let key = format!("company:{}", c.id);
    if !claim(&key) {
        return Ok(false);
    }
    let outcome = edgar::refresh(state, c.id, &c.ticker, &c.cik).await;
    release(&key);
    match &outcome {
        Ok(()) => store::set_company_status(&state.pool, c.id, "ok", None).await?,
        Err(e) => store::set_company_status(&state.pool, c.id, "error", Some(&format!("{e:#}"))).await?,
    }
    outcome.map(|_| true)
}

/// Midnight UTC of a date, in milliseconds (the frontend's chart timestamps).
pub fn date_ms(d: time::Date) -> i64 {
    (OffsetDateTime::new_utc(d, time::Time::MIDNIGHT).unix_timestamp()) * 1000
}

/// `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Option<time::Date> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    time::Date::parse(s.get(..10)?, &fmt).ok()
}
