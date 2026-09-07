//! Historical Data connectors.
//!
//! Each provider is a [`Connector`]: it declares a capability matrix (which asset types,
//! timeframes, key requirement, adjusted-close availability, max bars per request) and a
//! `fetch_chunk` that pulls one page of bars. The download worker (see `histdata_job`)
//! drives connectors generically — chunking, progress, retries and persistence live there,
//! not in the connectors.
//!
//! There is no separate validate step: a connector returning no data / an HTTP error for a
//! bad ticker is surfaced as the job result.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::Serialize;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

mod binance;
mod coinbase;
mod generic;
pub(crate) mod ibkr;
mod kraken;

const TIMEOUT: Duration = Duration::from_secs(20);

/// Static description of what a provider can do — drives the UI's greying-out of
/// impossible combinations and is enforced again server-side before a job is queued.
#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    pub provider: &'static str,
    pub label: &'static str,
    pub website: &'static str,
    /// Link to the provider's API/historical-data documentation (shown as an info tooltip).
    pub docs_url: &'static str,
    /// Short human note on rate limits / error handling for this provider (info tooltip).
    /// The worker already retries 429/5xx with backoff; this sets expectations.
    pub rate_limit: &'static str,
    /// Secret names the connector needs (empty = keyless). Surfaced on the settings page.
    pub required_secrets: &'static [&'static str],
    pub asset_types: &'static [&'static str],
    pub timeframes: &'static [&'static str],
    /// Whether the provider returns split/dividend-adjusted OHLC (equities/ETFs).
    pub adjusted: bool,
    /// Max bars one request returns; the worker pages in steps of this.
    pub max_bars_per_req: u32,
    /// Minimum delay the worker leaves between two requests to this provider, in
    /// milliseconds. Derived from `rate_limit` above: it keeps a long batch under the
    /// published ceiling instead of tripping 429 and waiting out the penalty.
    pub min_interval_ms: u64,
    /// Whether the connector can look symbols up (`search_symbols`). When false the UI
    /// falls back to free-text entry validated by the first fetch.
    pub searchable: bool,
    /// Non-secret settings this provider needs on every connector (an IB Gateway host and
    /// port, say). Empty for the REST providers, whose only variable is the key. Values
    /// are stored in the clear on `histdata_connectors.config` and reach the connector
    /// through the same map as the credentials.
    pub config_fields: &'static [ConfigField],
    /// Whether `test` says something useful. A REST provider fails loudly on the first
    /// fetch; a socket provider needs a preflight, because "nothing came back" has half a
    /// dozen distinct causes the user has to fix in a different application.
    pub testable: bool,
    /// Asset types this provider can stream live, empty when it has no live feed. Live is
    /// not the same reach as history: Alpaca downloads daily equity bars but streams
    /// minute ones, and Massive's futures live on a different service entirely.
    pub stream_asset_types: &'static [&'static str],
    /// Timeframes the live feed can serve. A provider that publishes minute bars can fold
    /// them into any intraday period, but a **daily** candle is a session rather than 1440
    /// epoch-aligned minutes, so it is refused rather than approximated: a live daily bar
    /// built that way would disagree with the one the download stores.
    pub stream_timeframes: &'static [&'static str],
    /// One line on what live costs with this provider — the plan, the entitlement or the
    /// subscription it needs. Shown next to the live control, because an empty live chart
    /// is nearly always an entitlement rather than a fault.
    pub stream_note: &'static str,
}

/// One non-secret setting a provider needs, as the connector form should render it.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigField {
    /// Key in `histdata_connectors.config` and in the map the connector reads.
    pub name: &'static str,
    /// English label. The frontend translates `connectors.field.<name>` when it has one.
    pub label: &'static str,
    pub placeholder: &'static str,
    /// `text` or `number`: the input type, and how the value is stored.
    pub kind: &'static str,
    /// A connector cannot run without it.
    pub required: bool,
    /// One line under the field.
    pub help: &'static str,
}

/// One page of bars plus whether more remain after `next_from`.
pub struct Chunk {
    pub bars: Vec<Bar>,
}

/// One instrument a provider serves, as returned by a symbol lookup.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolHit {
    /// Symbol in the provider's own format — what goes into `ticker`.
    pub symbol: String,
    /// Human name ("Bitcoin / TetherUS", "Apple Inc."); empty when the provider has none.
    pub name: String,
    /// Which of the provider's asset types this instrument belongs to.
    pub asset_type: String,
    /// Exchange/market label when the provider reports one.
    pub exchange: String,
}

/// A live connector: capability metadata + the fetch primitive.
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    fn capability(&self) -> &'static Capability;

    /// Fetch bars for `[from, to)` at `timeframe`, starting at `from`. Connectors return up
    /// to `max_bars_per_req` bars; the worker advances `from` and calls again until `to`.
    /// `secrets` holds decrypted credentials (may be empty for keyless providers).
    async fn fetch_chunk(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk>;

    /// Look instruments up by free text. `asset_type` empty means "any this provider serves".
    /// Providers without a search endpoint keep the default and are reported as such — their
    /// symbols are typed by hand and validated by the first fetch.
    async fn search_symbols(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _query: &str,
        _asset_type: &str,
        _limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        Err(anyhow!(
            "{} has no symbol search — type the symbol as the provider spells it",
            self.capability().label
        ))
    }

    /// Check a connector's non-secret settings before they are stored. Providers with no
    /// `config_fields` have nothing to check; the required-field pass is the API's.
    fn validate_config(&self, _config: &HashMap<String, String>) -> Result<()> {
        Ok(())
    }

    /// Reach the provider and report what answered, or fail with a message naming what to
    /// fix. Only for connectors that declare `testable`.
    async fn test(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<String> {
        Err(anyhow!("{} has no connection test", self.capability().label))
    }
}

// ── Instrument-list cache ──────────────────────────────────────────────────────
//
// Exchanges that only expose "the whole universe" (Binance's exchangeInfo, Kraken's
// AssetPairs, Coinbase's products) answer one keystroke with a megabyte of JSON. Fetch
// that once per process and filter locally; the list changes on the scale of days.

const SYMBOLS_TTL: Duration = Duration::from_secs(6 * 3600);

/// Provider key → (fetched at, its whole instrument list).
type SymbolCache = HashMap<String, (Instant, Vec<SymbolHit>)>;

static SYMBOL_CACHE: Mutex<Option<SymbolCache>> = Mutex::new(None);

/// Cached full instrument list for `key`, or None when absent/stale.
pub(super) fn cached_symbols(key: &str) -> Option<Vec<SymbolHit>> {
    let guard = SYMBOL_CACHE.lock().ok()?;
    let map = guard.as_ref()?;
    let (at, hits) = map.get(key)?;
    (at.elapsed() < SYMBOLS_TTL).then(|| hits.clone())
}

pub(super) fn cache_symbols(key: &str, hits: &[SymbolHit]) {
    if let Ok(mut guard) = SYMBOL_CACHE.lock() {
        guard
            .get_or_insert_with(SymbolCache::new)
            .insert(key.to_string(), (Instant::now(), hits.to_vec()));
    }
}

/// Rank a provider's full instrument list against a query: exact symbol first, then
/// prefix, then any substring of symbol or name. Empty query keeps the provider's order.
pub(super) fn filter_symbols(all: &[SymbolHit], query: &str, limit: usize) -> Vec<SymbolHit> {
    let q = query.trim().to_ascii_uppercase();
    if q.is_empty() {
        return all.iter().take(limit).cloned().collect();
    }
    let mut scored: Vec<(u8, &SymbolHit)> = all
        .iter()
        .filter_map(|h| {
            let sym = h.symbol.to_ascii_uppercase();
            let name = h.name.to_ascii_uppercase();
            let rank = if sym == q {
                0
            } else if sym.starts_with(&q) {
                1
            } else if sym.contains(&q) {
                2
            } else if name.contains(&q) {
                3
            } else {
                return None;
            };
            Some((rank, h))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.symbol.cmp(&b.1.symbol)));
    scored.into_iter().take(limit).map(|(_, h)| h.clone()).collect()
}

/// All providers' capabilities, for the settings/download UI.
pub fn capabilities() -> Vec<&'static Capability> {
    all_connectors().iter().map(|c| c.capability()).collect()
}

fn all_connectors() -> Vec<Box<dyn Connector>> {
    vec![
        Box::new(binance::Binance),
        Box::new(coinbase::Coinbase),
        Box::new(kraken::Kraken),
        Box::new(generic::alphavantage()),
        Box::new(generic::eodhd()),
        Box::new(generic::yahoo()),
        Box::new(generic::alpaca()),
        Box::new(generic::massive()),
        Box::new(ibkr::Ibkr),
    ]
}

/// Look up a connector by provider id.
pub fn connector_for(provider: &str) -> Result<Box<dyn Connector>> {
    all_connectors()
        .into_iter()
        .find(|c| c.capability().provider == provider)
        .ok_or_else(|| anyhow!("unknown provider: {provider}"))
}

/// Validate a download request against the provider's capability matrix. Returns the
/// capability on success so the caller can reuse its `max_bars_per_req`, etc.
pub fn validate_request(
    provider: &str,
    asset_type: &str,
    timeframe: &str,
) -> Result<&'static Capability> {
    let cap = connector_for(provider)?.capability();
    if !cap.asset_types.contains(&asset_type) {
        return Err(anyhow!("{} does not support asset type {asset_type}", cap.label));
    }
    if !cap.timeframes.contains(&timeframe) {
        return Err(anyhow!("{} does not support timeframe {timeframe}", cap.label));
    }
    Ok(cap)
}

pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent("OpenTraderWorld/histdata")
        .build()
        .map_err(|e| anyhow!("building histdata http client: {e}"))
}

/// An option contract parsed from the canonical OCC symbol the download form submits.
///
/// The UI collects underlying / expiry / call-put / strike separately (the convention every
/// broker uses) and joins them into the OCC form `UND[YYMMDD][C|P][strike*1000, 8 digits]`,
/// e.g. `SPY251219C00650000`. Connectors take this apart and re-emit whatever wire format
/// their provider wants (Polygon prefixes `O:`, Alpaca uses the bare OCC string). An
/// incoming symbol may already carry an `O:` prefix (append jobs replay the stored ticker);
/// we strip it before parsing so both paths normalize to the same struct.
pub struct OptionContract {
    pub underlying: String,
    /// Expiry as `YYMMDD`.
    pub yymmdd: String,
    /// `'C'` or `'P'`.
    pub cp: char,
    /// Strike × 1000, zero-padded to 8 digits (OCC encoding).
    pub strike8: String,
}

impl OptionContract {
    /// The bare OCC symbol with no vendor prefix, e.g. `SPY251219C00650000`.
    pub fn occ(&self) -> String {
        format!("{}{}{}{}", self.underlying, self.yymmdd, self.cp, self.strike8)
    }
}

/// Parse an OCC option symbol (with or without a leading `O:`). Rejects anything that isn't a
/// well-formed contract so a hand-typed / malformed ticker fails fast with a clear message
/// rather than 404ing at the provider.
pub fn parse_option_symbol(sym: &str) -> Result<OptionContract> {
    let s = sym.trim();
    let s = s.strip_prefix("O:").or_else(|| s.strip_prefix("o:")).unwrap_or(s);
    // The fixed tail is always 15 chars: 6 (date) + 1 (C/P) + 8 (strike). The underlying is
    // whatever precedes it (1–6 chars).
    if s.len() < 16 {
        return Err(anyhow!("not a valid option symbol: {sym}"));
    }
    let (underlying, tail) = s.split_at(s.len() - 15);
    let (yymmdd, rest) = tail.split_at(6);
    let cp = rest.as_bytes()[0].to_ascii_uppercase() as char;
    let strike8 = &rest[1..];
    if underlying.is_empty()
        || !underlying.chars().all(|c| c.is_ascii_alphanumeric())
        || !yymmdd.chars().all(|c| c.is_ascii_digit())
        || !matches!(cp, 'C' | 'P')
        || strike8.len() != 8
        || !strike8.chars().all(|c| c.is_ascii_digit())
    {
        return Err(anyhow!("not a valid option symbol: {sym}"));
    }
    Ok(OptionContract {
        underlying: underlying.to_ascii_uppercase(),
        yymmdd: yymmdd.to_string(),
        cp,
        strike8: strike8.to_string(),
    })
}

/// The timeframes the download pipeline supports. Deliberately a closed list: the arithmetic
/// below would happily size a `2h` window that no provider mapping knows how to request.
pub const SUPPORTED_TIMEFRAMES: [&str; 7] = ["1m", "5m", "15m", "1h", "4h", "1d", "1w"];

/// Helper: seconds in one canonical timeframe (used for paging windows). The length itself
/// comes from [`crate::timeframe::Timeframe`], which is the single source for timeframe
/// arithmetic; this only guards the closed list above.
pub fn timeframe_secs(tf: &str) -> Result<i64> {
    if !SUPPORTED_TIMEFRAMES.contains(&tf) {
        return Err(anyhow!("unknown timeframe {tf}"));
    }
    crate::timeframe::Timeframe::parse(tf)?
        .secs()
        .ok_or_else(|| anyhow!("timeframe {tf} has no fixed length"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(symbol: &str, name: &str) -> SymbolHit {
        SymbolHit {
            symbol: symbol.into(),
            name: name.into(),
            asset_type: "crypto".into(),
            exchange: "X".into(),
        }
    }

    #[test]
    fn symbol_ranking_puts_the_exact_match_first() {
        let all = vec![
            hit("WBTCUSDT", "Wrapped BTC"),
            hit("BTCUSDT", "Bitcoin/TetherUS"),
            hit("BTCUSDC", "Bitcoin/USDC"),
            hit("ETHUSDT", "Ether/TetherUS"),
        ];
        let out = filter_symbols(&all, "btcusdt", 10);
        assert_eq!(out[0].symbol, "BTCUSDT"); // exact
        assert_eq!(out[1].symbol, "WBTCUSDT"); // substring
        assert_eq!(out.len(), 2);
        // Prefix beats substring, and a name-only match still shows up.
        let out = filter_symbols(&all, "BTC", 10);
        assert_eq!(out[0].symbol, "BTCUSDC");
        assert_eq!(out[1].symbol, "BTCUSDT");
        assert_eq!(out[2].symbol, "WBTCUSDT");
        // A name-only match (the symbol says nothing about "wrapped").
        let out = filter_symbols(&all, "wrapped", 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].symbol, "WBTCUSDT");
        // No query = the provider's own order, capped.
        assert_eq!(filter_symbols(&all, "  ", 2).len(), 2);
    }

    #[test]
    fn parses_occ_symbols() {
        let c = parse_option_symbol("SPY251219C00650000").unwrap();
        assert_eq!(c.underlying, "SPY");
        assert_eq!(c.yymmdd, "251219");
        assert_eq!(c.cp, 'C');
        assert_eq!(c.strike8, "00650000");
        assert_eq!(c.occ(), "SPY251219C00650000");
        // O: prefix (from a replayed append job) is stripped and normalized identically.
        assert_eq!(parse_option_symbol("O:SPY251219C00650000").unwrap().occ(), "SPY251219C00650000");
        // Single-letter underlying, put.
        assert_eq!(parse_option_symbol("F260116P00012500").unwrap().cp, 'P');
    }

    #[test]
    fn rejects_bad_option_symbols() {
        for bad in ["AAPL", "", "SPY251219X00650000", "SPY2512C00650000", "SPY251219C0065000"] {
            assert!(parse_option_symbol(bad).is_err(), "should reject {bad}");
        }
    }
}
