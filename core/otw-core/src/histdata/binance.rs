//! Binance spot klines — keyless public OHLCV, the deepest crypto history source.
//! `GET /api/v3/klines?symbol=&interval=&startTime=&endTime=&limit=1000`.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, Connector, SymbolHit};

pub struct Binance;

static CAP: Capability = Capability {
    provider: "binance",
    label: "Binance",
    website: "https://binance.com",
    docs_url: "https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints",
    rate_limit: "Keyless. IP request-weight ~1200/min; each klines call costs 1–2. 429 = back off, repeated abuse → 418 IP ban. Downloads auto-retry with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    // ~1200 weight/min, klines cost 1-2: 5 req/s stays far under it.
    min_interval_ms: 200,
    searchable: true,
};

fn interval(tf: &str) -> &'static str {
    match tf {
        "1m" => "1m",
        "5m" => "5m",
        "15m" => "15m",
        "1h" => "1h",
        "4h" => "4h",
        "1d" => "1d",
        _ => "1w",
    }
}

#[async_trait::async_trait]
impl Connector for Binance {
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
        let url = format!(
            "https://api.binance.com/api/v3/klines?symbol={}&interval={}&startTime={}&endTime={}&limit={}",
            ticker.to_uppercase(),
            interval(timeframe),
            from.unix_timestamp() * 1000,
            to.unix_timestamp() * 1000,
            CAP.max_bars_per_req,
        );
        let rows: Vec<Vec<Value>> = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("binance request")?
            .error_for_status()
            .context("binance status (bad symbol?)")?
            .json()
            .await
            .context("binance decode")?;

        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // [ openTime, open, high, low, close, volume, ... ]
            let open_ms = r.first().and_then(Value::as_i64).ok_or_else(|| anyhow!("kline shape"))?;
            let num = |i: usize| -> Result<f64> {
                r.get(i)
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| anyhow!("kline field {i}"))
            };
            bars.push(Bar {
                ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
                open: num(1)?,
                high: num(2)?,
                low: num(3)?,
                close: num(4)?,
                volume: num(5)?,
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        Ok(Chunk { bars })
    }

    /// Binance publishes its whole spot universe in one call (~1 MB, no key), so the list is
    /// fetched once per process and filtered locally instead of hitting the API per keystroke.
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
            client.get("https://api.binance.com/api/v3/exchangeInfo?permissions=SPOT"),
        )
        .await
        .context("binance exchangeInfo request")?
        .error_for_status()
        .context("binance exchangeInfo status")?
        .json()
        .await
        .context("binance exchangeInfo decode")?;

        let arr = body
            .get("symbols")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("binance: no symbol list"))?;
        let all: Vec<SymbolHit> = arr
            .iter()
            .filter(|s| s.get("status").and_then(Value::as_str) == Some("TRADING"))
            .filter_map(|s| {
                let symbol = s.get("symbol").and_then(Value::as_str)?;
                let base = s.get("baseAsset").and_then(Value::as_str).unwrap_or("");
                let quote = s.get("quoteAsset").and_then(Value::as_str).unwrap_or("");
                Some(SymbolHit {
                    symbol: symbol.to_string(),
                    name: format!("{base}/{quote}"),
                    asset_type: "crypto".into(),
                    exchange: "Binance".into(),
                })
            })
            .collect();
        super::cache_symbols(CAP.provider, &all);
        Ok(super::filter_symbols(&all, query, limit))
    }
}
