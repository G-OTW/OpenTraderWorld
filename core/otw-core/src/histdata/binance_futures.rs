//! Binance USDⓈ-M futures klines — keyless public OHLCV for perpetuals and dated contracts.
//!
//! `GET /fapi/v1/klines`, the futures twin of the spot connector. It is a separate provider
//! rather than a setting on the spot one because `BTCUSDT` exists on both services and the
//! two are different series: the perpetual trades at a basis to spot, and a dated contract
//! converges to it. Which one a dataset holds has to be unambiguous.
//!
//! A 24/7 market's daily candle *is* the epoch day here, so no alignment variant is needed:
//! `interval=1d` opens at midnight UTC.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, Connector, SymbolHit};

const BASE: &str = "https://fapi.binance.com";

pub struct BinanceFutures;

static CAP: Capability = Capability {
    provider: "binance_futures",
    label: "Binance USDⓈ-M Futures",
    website: "https://www.binance.com",
    docs_url: "https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api",
    rate_limit: "Keyless. 2400 request weight per minute per IP; a klines call costs 1–10 by \
                 page size. 429 = back off, repeated abuse → 418 IP ban. Downloads auto-retry \
                 with backoff.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1500,
    // 2400 weight/minute and a full page costs 10: 5 requests a second stays well under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    stream_asset_types: &["crypto"],
    // Binance publishes a kline channel per interval, so every downloadable timeframe
    // streams natively; a 24/7 market's daily candle is the epoch day.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live klines are public and keyless, like the history.",
};

#[async_trait::async_trait]
impl Connector for BinanceFutures {
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
        let symbol = ticker.trim().to_uppercase();
        if symbol.is_empty() {
            return Err(anyhow!(
                "name the Binance futures contract to download (BTCUSDT, ETHUSDT_250926)"
            ));
        }
        let url = format!(
            "{BASE}/fapi/v1/klines?symbol={}&interval={}&startTime={}&endTime={}&limit={}",
            super::enc(&symbol),
            interval(timeframe)?,
            ms(from),
            ms(to),
            CAP.max_bars_per_req,
        );
        let res = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("binance futures request")?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "Binance futures answered {status}: {}. Check the contract is spelled as \
                 Binance spells it on the futures market (BTCUSDT, ETHUSDT_250926).",
                super::trim_err(&body)
            ));
        }
        let rows: Vec<Vec<Value>> =
            serde_json::from_str(&body).context("binance futures decode")?;
        let from_ms = ms(from);
        let to_ms = ms(to);
        let now_ms = ms(OffsetDateTime::now_utc());
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // [ openTime, open, high, low, close, volume, closeTime, … ]
            let open_ms = r
                .first()
                .and_then(Value::as_i64)
                .ok_or_else(|| anyhow!("binance futures kline shape"))?;
            // Binance reads `endTime` as inclusive, so the bar opening on it comes back too;
            // the window is ours and is half-open, like every other connector's.
            if open_ms < from_ms || open_ms >= to_ms {
                continue;
            }
            // The period still running is not a row: `closeTime` is its last millisecond, so
            // a bar whose close is still ahead of us is partial and nothing would come back
            // to correct it.
            let close_ms = r.get(6).and_then(Value::as_i64).unwrap_or(open_ms);
            if close_ms >= now_ms {
                continue;
            }
            let num = |i: usize| -> Result<f64> {
                r.get(i)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| anyhow!("binance futures kline field {i}"))
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

    /// Binance publishes its whole futures universe in one keyless call, so the list is
    /// fetched once per process and filtered locally.
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
            client.get(format!("{BASE}/fapi/v1/exchangeInfo")),
        )
        .await
        .context("binance futures exchangeInfo request")?
        .error_for_status()
        .context("binance futures exchangeInfo status")?
        .json()
        .await
        .context("binance futures exchangeInfo decode")?;
        let all: Vec<SymbolHit> = body
            .get("symbols")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|s| s.get("status").and_then(Value::as_str) == Some("TRADING"))
            .filter_map(|s| {
                let symbol = s.get("symbol").and_then(Value::as_str)?;
                let base = s.get("baseAsset").and_then(Value::as_str).unwrap_or("");
                let quote = s.get("quoteAsset").and_then(Value::as_str).unwrap_or("");
                Some(SymbolHit {
                    symbol: symbol.to_string(),
                    name: format!("{base}/{quote}"),
                    asset_type: "crypto".into(),
                    // Perpetual or a dated contract: what it is belongs on the line.
                    exchange: format!(
                        "Binance {}",
                        s.get("contractType").and_then(Value::as_str).unwrap_or("futures")
                    ),
                })
            })
            .collect();
        super::cache_symbols(CAP.provider, &all);
        Ok(super::filter_symbols(&all, query, limit))
    }
}

/// Canonical timeframe → Binance interval token.
pub(crate) fn interval(tf: &str) -> Result<&'static str> {
    Ok(match tf {
        "1m" => "1m",
        "5m" => "5m",
        "15m" => "15m",
        "1h" => "1h",
        "4h" => "4h",
        "1d" => "1d",
        "1w" => "1w",
        other => return Err(anyhow!("Binance futures does not serve a {other} candle")),
    })
}

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeframes_map_or_are_refused() {
        assert_eq!(interval("4h").unwrap(), "4h");
        assert_eq!(interval("1w").unwrap(), "1w");
        assert!(interval("30m").is_err());
    }
}
