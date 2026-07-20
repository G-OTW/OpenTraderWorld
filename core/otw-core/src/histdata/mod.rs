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
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Serialize;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

mod binance;
mod coinbase;
mod generic;
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
}

/// One page of bars plus whether more remain after `next_from`.
pub struct Chunk {
    pub bars: Vec<Bar>,
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

/// Helper: seconds in one canonical timeframe (used for paging windows).
pub fn timeframe_secs(tf: &str) -> Result<i64> {
    Ok(match tf {
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "1h" => 3600,
        "4h" => 14400,
        "1d" => 86400,
        "1w" => 604800,
        other => return Err(anyhow!("unknown timeframe {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
