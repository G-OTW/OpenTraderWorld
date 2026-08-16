//! Binance live klines (class B: candle channel, no snapshot).
//!
//! Path-based stream `wss://stream.binance.com:9443/ws/<sym>@kline_<interval>` — no subscribe
//! frame needed. Each message carries the forming bar with an explicit closed flag (`k.x`),
//! so close detection is exact; the supervisor pairs it with a REST seed/backfill for history
//! and gaps. Keyless, like the REST connector.

use anyhow::{anyhow, Result};
use serde_json::Value;
use time::OffsetDateTime;

use super::{run_feed, Feed, LiveEvent, LiveStream, StreamConnector};

pub struct BinanceLive;

#[async_trait::async_trait]
impl StreamConnector for BinanceLive {
    async fn open(&self, symbol: &str, timeframe: &str) -> Result<LiveStream> {
        run_feed(BinanceFeed {
            symbol: symbol.to_lowercase(),
            interval: interval(timeframe).to_string(),
        })
        .await
    }
}

struct BinanceFeed {
    symbol: String,
    interval: String,
}

impl Feed for BinanceFeed {
    fn url(&self) -> String {
        format!(
            "wss://stream.binance.com:9443/ws/{}@kline_{}",
            self.symbol, self.interval
        )
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<LiveEvent>> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("binance json: {e}"))?;
        let Some(k) = v.get("k") else {
            // Non-kline control frames (e.g. subscription acks) are ignored.
            return Ok(Vec::new());
        };
        let num = |key: &str| -> Result<f64> {
            k.get(key)
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow!("binance kline field {key}"))
        };
        let open_ms = k.get("t").and_then(Value::as_i64).ok_or_else(|| anyhow!("binance kline t"))?;
        let event_ms = v.get("E").and_then(Value::as_i64).unwrap_or(open_ms);
        Ok(vec![LiveEvent {
            bar_ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
            open: num("o")?,
            high: num("h")?,
            low: num("l")?,
            close: num("c")?,
            volume: num("v")?,
            closed: k.get("x").and_then(Value::as_bool).unwrap_or(false),
            provider_ts: OffsetDateTime::from_unix_timestamp(event_ms / 1000)?,
        }])
    }
}

/// Map a canonical timeframe to Binance's interval token. `1s` is accepted for fast tests
/// (it is not a downloadable timeframe, so it never reaches here from the normal UI path).
fn interval(tf: &str) -> &'static str {
    match tf {
        "1s" => "1s",
        "1m" => "1m",
        "5m" => "5m",
        "15m" => "15m",
        "1h" => "1h",
        "4h" => "4h",
        "1d" => "1d",
        _ => "1w",
    }
}
