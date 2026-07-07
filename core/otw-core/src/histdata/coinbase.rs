//! Coinbase Exchange candles — keyless public OHLCV, max 300 bars/req.
//! `GET /products/{id}/candles?granularity=&start=&end=` (ISO8601 bounds).
//! Response rows: [ time, low, high, open, close, volume ], newest-first.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{timeframe_secs, Capability, Chunk, Connector};

pub struct Coinbase;

static CAP: Capability = Capability {
    provider: "coinbase",
    label: "Coinbase",
    website: "https://coinbase.com",
    docs_url: "https://docs.cdp.coinbase.com/exchange/reference/exchangerestapi_getproductcandles",
    rate_limit: "Keyless public API ~10 req/s per IP; 300 candles max per request. Bursts return 429. Downloads auto-retry with backoff.",
    required_secrets: &[],
    paid: false,
    asset_types: &["crypto"],
    // Coinbase granularities: 60,300,900,3600,21600,86400 — no 4h or 1w.
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    adjusted: false,
    max_bars_per_req: 300,
};

#[async_trait::async_trait]
impl Connector for Coinbase {
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
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        let gran = timeframe_secs(timeframe)?;
        let fmt = &time::format_description::well_known::Rfc3339;
        let url = format!(
            "https://api.exchange.coinbase.com/products/{}/candles?granularity={}&start={}&end={}",
            ticker.to_uppercase(),
            gran,
            from.format(fmt)?,
            to.format(fmt)?,
        );
        let rows: Vec<[f64; 6]> = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("coinbase request")?
            .error_for_status()
            .context("coinbase status (bad product?)")?
            .json()
            .await
            .context("coinbase decode")?;

        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // [ time, low, high, open, close, volume ]
            bars.push(Bar {
                ts: OffsetDateTime::from_unix_timestamp(r[0] as i64)?,
                open: r[3],
                high: r[2],
                low: r[1],
                close: r[4],
                volume: r[5],
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        // Coinbase returns newest-first; normalize to ascending.
        bars.sort_by_key(|b| b.ts);
        if bars.is_empty() && gran == 0 {
            return Err(anyhow!("invalid granularity"));
        }
        Ok(Chunk { bars })
    }
}
