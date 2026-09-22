//! OKX v5 candles — keyless public OHLCV for spot, swaps and futures.
//!
//! `GET /api/v5/market/history-candles`. Three things shape this connector:
//!
//!   - **A day is not the epoch day by default.** OKX's `1D` bar opens at 16:00 UTC
//!     (midnight in UTC+8). The `utc` variants (`1Dutc`, `1Wutc`) open on midnight UTC and
//!     Monday, which is where every other dataset here is anchored.
//!   - **History pages backwards.** `history-candles` answers the bars *before* `after`,
//!     newest first, 300 at a time. The worker's window never holds more than
//!     `max_bars_per_req` bars, so one call per window covers it and what falls outside is
//!     dropped.
//!   - **The instrument type is in the id.** `BTC-USDT` is spot and `BTC-USDT-SWAP` is a
//!     perpetual, so nothing has to be configured: OKX's own spelling says which market a
//!     ticker belongs to.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, Connector, SymbolHit};

const BASE: &str = "https://www.okx.com";
/// Instrument types offered in the picker, in the order they are searched.
const INST_TYPES: &[&str] = &["SPOT", "SWAP", "FUTURES"];

pub struct Okx;

static CAP: Capability = Capability {
    provider: "okx",
    label: "OKX",
    website: "https://www.okx.com",
    docs_url: "https://www.okx.com/docs-v5/en/#order-book-trading-market-data",
    rate_limit: "Keyless. 20 requests per 2 seconds per IP on the candle archive. A page is \
                 300 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 300,
    // 20 requests per 2 seconds is the published ceiling; 5 a second stays under it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: false,
    stream_asset_types: &["crypto"],
    // OKX publishes a candle channel per interval, so every downloadable timeframe streams
    // natively. The daily and weekly channels are the UTC-anchored ones, like the download.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. They ride OKX's \
                  business socket, which allows a few connections per IP.",
};

#[async_trait::async_trait]
impl Connector for Okx {
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
        let inst_id = ticker.trim().to_uppercase();
        if inst_id.is_empty() {
            return Err(anyhow!(
                "name the OKX instrument to download (BTC-USDT, BTC-USDT-SWAP)"
            ));
        }
        // `after` is exclusive and reads backwards, so the window's end opens the page.
        let url = format!(
            "{BASE}/api/v5/market/history-candles?instId={}&bar={}&after={}&limit={}",
            super::enc(&inst_id),
            bar(timeframe)?,
            ms(to),
            CAP.max_bars_per_req,
        );
        let body: Value = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("okx request")?
            .json()
            .await
            .context("okx decode")?;
        let code = body.get("code").and_then(Value::as_str).unwrap_or_default();
        if code != "0" {
            let msg = body.get("msg").and_then(Value::as_str).unwrap_or_default();
            return Err(anyhow!(
                "OKX refused this request ({code} {msg}). Check the instrument is spelled as \
                 OKX spells it: BTC-USDT for spot, BTC-USDT-SWAP for a perpetual."
            ));
        }
        let rows = body
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("OKX answered no candle list for {inst_id}"))?;
        let from_ms = ms(from);
        let to_ms = ms(to);
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // [ ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm ]
            let open_ms = r
                .get(0)
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(|| anyhow!("okx candle open time"))?;
            if open_ms < from_ms || open_ms >= to_ms {
                continue;
            }
            // `confirm` is 0 while the bar is still forming; storing it would persist a
            // partial bar nothing later would come back to correct.
            if r.get(8).and_then(Value::as_str) == Some("0") {
                continue;
            }
            let num = |i: usize| -> Result<f64> {
                r.get(i)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| anyhow!("okx candle field {i}"))
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
        bars.sort_by_key(|b| b.ts);
        Ok(Chunk { bars })
    }

    /// OKX publishes each instrument type in one keyless call, so the three tradeable ones
    /// are fetched once per process and filtered locally.
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
        let mut all: Vec<SymbolHit> = Vec::new();
        for inst_type in INST_TYPES {
            let url = format!("{BASE}/api/v5/public/instruments?instType={inst_type}");
            let body: Value = crate::rate::send(CAP.provider, client.get(&url))
                .await
                .context("okx instruments request")?
                .json()
                .await
                .context("okx instruments decode")?;
            for i in body.get("data").and_then(Value::as_array).into_iter().flatten() {
                if i.get("state").and_then(Value::as_str) != Some("live") {
                    continue;
                }
                let Some(id) = i.get("instId").and_then(Value::as_str) else { continue };
                let base = i
                    .get("baseCcy")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .or_else(|| i.get("ctValCcy").and_then(Value::as_str))
                    .unwrap_or("");
                let quote = i
                    .get("quoteCcy")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .or_else(|| i.get("settleCcy").and_then(Value::as_str))
                    .unwrap_or("");
                all.push(SymbolHit {
                    symbol: id.to_string(),
                    name: format!("{base}/{quote}"),
                    asset_type: "crypto".into(),
                    exchange: format!("OKX {inst_type}"),
                });
            }
        }
        super::cache_symbols(CAP.provider, &all);
        Ok(super::filter_symbols(&all, query, limit))
    }
}

/// Canonical timeframe → OKX bar token. The daily and weekly ones are the UTC-anchored
/// variants on purpose: OKX's plain `1D` opens at 16:00 UTC.
pub(crate) fn bar(tf: &str) -> Result<&'static str> {
    Ok(match tf {
        "1m" => "1m",
        "5m" => "5m",
        "15m" => "15m",
        "1h" => "1H",
        "4h" => "4H",
        "1d" => "1Dutc",
        "1w" => "1Wutc",
        other => return Err(anyhow!("OKX does not serve a {other} candle")),
    })
}

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_is_the_utc_day() {
        // OKX's plain 1D bar opens at 16:00 UTC; the utc variant is the one that agrees
        // with every other dataset here.
        assert_eq!(bar("1d").unwrap(), "1Dutc");
        assert_eq!(bar("1w").unwrap(), "1Wutc");
        assert_eq!(bar("4h").unwrap(), "4H");
        assert!(bar("30m").is_err());
    }
}
