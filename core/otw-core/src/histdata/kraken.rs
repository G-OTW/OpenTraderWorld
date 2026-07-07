//! Kraken public OHLC — keyless, but only returns the last ~720 bars per timeframe,
//! so it cannot deep-backfill. `GET /0/public/OHLC?pair=&interval=&since=`.
//! `interval` is in minutes. Response: { result: { <pair>: [[time,o,h,l,c,vwap,vol,cnt],..] } }.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, Connector};

pub struct Kraken;

static CAP: Capability = Capability {
    provider: "kraken",
    label: "Kraken",
    website: "https://kraken.com",
    docs_url: "https://docs.kraken.com/api/docs/rest-api/get-ohlc-data",
    rate_limit: "Keyless public API ~1 req/s sustained; OHLC returns up to 720 bars ending now (limited history for fine timeframes). Downloads auto-retry with backoff.",
    required_secrets: &[],
    paid: false,
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 720,
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
}
