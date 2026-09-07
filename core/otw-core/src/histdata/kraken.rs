//! Kraken public OHLC — keyless, but only returns the last ~720 bars per timeframe,
//! so it cannot deep-backfill. `GET /0/public/OHLC?pair=&interval=&since=`.
//! `interval` is in minutes. Response: { result: { <pair>: [[time,o,h,l,c,vwap,vol,cnt],..] } }.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, Connector, SymbolHit};

pub struct Kraken;

static CAP: Capability = Capability {
    provider: "kraken",
    label: "Kraken",
    website: "https://kraken.com",
    docs_url: "https://docs.kraken.com/api/docs/rest-api/get-ohlc-data",
    rate_limit: "Keyless public API ~1 req/s sustained; OHLC returns up to 720 bars ending now (limited history for fine timeframes). Downloads auto-retry with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 720,
    // ~1 req/s sustained is the documented ceiling.
    min_interval_ms: 1100,
    searchable: true,
    config_fields: &[],
    testable: false,
    stream_asset_types: &["crypto"],
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live OHLC is public and keyless, like the history.",
};

fn interval_min(tf: &str) -> i64 {
    match tf {
        "1m" => 1,
        "5m" => 5,
        "15m" => 15,
        "1h" => 60,
        "4h" => 240,
        "1d" => 1440,
        _ => 10080, // 1w
    }
}

#[async_trait::async_trait]
impl Connector for Kraken {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    async fn fetch_chunk(
        &self,
        client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        ticker: &str,
        _asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        _to: OffsetDateTime,
    ) -> Result<Chunk> {
        let url = format!(
            "https://api.kraken.com/0/public/OHLC?pair={}&interval={}&since={}",
            ticker.to_uppercase(),
            interval_min(timeframe),
            from.unix_timestamp(),
        );
        let body: Value = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("kraken request")?
            .error_for_status()
            .context("kraken status")?
            .json()
            .await
            .context("kraken decode")?;

        if let Some(errs) = body.get("error").and_then(Value::as_array) {
            if let Some(first) = errs.first().and_then(Value::as_str) {
                // Kraken signals throttling in the body (HTTP 200) with an EAPIRateLimit code.
                if first.contains("RateLimit") {
                    crate::rate::note_limited(CAP.provider, &url, first);
                }
                return Err(anyhow!("kraken error: {first}"));
            }
        }
        let result = body.get("result").ok_or_else(|| anyhow!("kraken: no result"))?;
        // The pair key is whatever Kraken normalized our input to; take the first array value.
        let series = result
            .as_object()
            .and_then(|o| o.iter().find(|(k, _)| *k != "last").map(|(_, v)| v))
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("kraken: empty result (bad pair?)"))?;

        let num = |v: &Value| -> Result<f64> {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_f64())
                .ok_or_else(|| anyhow!("kraken numeric field"))
        };
        let mut bars = Vec::with_capacity(series.len());
        for row in series {
            let r = row.as_array().ok_or_else(|| anyhow!("kraken row shape"))?;
            let t = r.first().and_then(Value::as_i64).ok_or_else(|| anyhow!("kraken time"))?;
            bars.push(Bar {
                ts: OffsetDateTime::from_unix_timestamp(t)?,
                open: num(&r[1])?,
                high: num(&r[2])?,
                low: num(&r[3])?,
                close: num(&r[4])?,
                volume: num(&r[6])?,
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        Ok(Chunk { bars })
    }

    /// Kraken's tradable pairs come as one keyless object; cache it and filter locally.
    /// The searchable name is the `wsname` (BTC/USD), the fetchable symbol the `altname`.
    async fn search_symbols(
        &self,
        client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        query: &str,
        _asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        if let Some(all) = super::cached_symbols(CAP.provider) {
            return Ok(super::filter_symbols(&all, query, limit));
        }
        let body: Value = crate::rate::send(
            CAP.provider,
            client.get("https://api.kraken.com/0/public/AssetPairs"),
        )
        .await
        .context("kraken AssetPairs request")?
        .error_for_status()
        .context("kraken AssetPairs status")?
        .json()
        .await
        .context("kraken AssetPairs decode")?;

        if let Some(first) = body
            .get("error")
            .and_then(Value::as_array)
            .and_then(|e| e.first())
            .and_then(Value::as_str)
        {
            return Err(anyhow!("kraken error: {first}"));
        }
        let map = body
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("kraken: no pair list"))?;
        let all: Vec<SymbolHit> = map
            .values()
            .filter(|p| {
                p.get("status")
                    .and_then(Value::as_str)
                    .map(|s| s == "online")
                    .unwrap_or(true)
            })
            .filter_map(|p| {
                let altname = p.get("altname").and_then(Value::as_str)?;
                Some(SymbolHit {
                    symbol: altname.to_string(),
                    name: p
                        .get("wsname")
                        .and_then(Value::as_str)
                        .unwrap_or(altname)
                        .to_string(),
                    asset_type: "crypto".into(),
                    exchange: "Kraken".into(),
                })
            })
            .collect();
        super::cache_symbols(CAP.provider, &all);
        Ok(super::filter_symbols(&all, query, limit))
    }
}
