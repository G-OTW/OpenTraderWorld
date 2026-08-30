//! Kraken v2 live OHLC (class A: candle channel *with* snapshot).
//!
//! `wss://ws.kraken.com/v2`, subscribe to `ohlc`. The first message is a `snapshot` of recent
//! candles (this is the "snapshot replaces REST" property), then `update`s for the forming
//! candle. Kraken sends **no closed flag**: a bar is final once a row with a newer
//! `interval_begin` arrives, so we infer closure on rollover — feeding the snapshot rows
//! through the same path naturally emits every historical candle as closed and leaves the
//! last one forming.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{run_feed, Feed, LiveEvent, LiveStream, StreamConnector};

pub struct KrakenLive;

#[async_trait::async_trait]
impl StreamConnector for KrakenLive {
    async fn open(&self, symbol: &str, timeframe: &str) -> Result<LiveStream> {
        run_feed(KrakenFeed {
            symbol: symbol.to_uppercase(),
            interval_min: interval_min(timeframe),
            cur: None,
        })
        .await
    }
}

struct KrakenFeed {
    /// Kraken v2 uses `BASE/QUOTE` pairs (e.g. `BTC/USD`); the download form already stores
    /// tickers that way for Kraken datasets.
    symbol: String,
    interval_min: u32,
    /// The last partial event for the currently-forming candle (for rollover close).
    cur: Option<LiveEvent>,
}

impl Feed for KrakenFeed {
    fn url(&self) -> String {
        "wss://ws.kraken.com/v2".to_string()
    }

    fn subscribe(&self) -> Vec<String> {
        vec![json!({
            "method": "subscribe",
            "params": {
                "channel": "ohlc",
                "symbol": [self.symbol],
                "interval": self.interval_min,
            }
        })
        .to_string()]
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<LiveEvent>> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("kraken json: {e}"))?;
        // Only OHLC data frames matter; acks/heartbeat/status are ignored.
        if v.get("channel").and_then(Value::as_str) != Some("ohlc") {
            return Ok(Vec::new());
        }
        let Some(rows) = v.get("data").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for row in rows {
            let ev = parse_row(row)?;
            match &self.cur {
                // First candle we see becomes the current one.
                None => {
                    out.push(ev.clone());
                    self.cur = Some(ev);
                }
                Some(cur) if ev.bar_ts == cur.bar_ts => {
                    // Same candle updated.
                    out.push(ev.clone());
                    self.cur = Some(ev);
                }
                Some(cur) if ev.bar_ts > cur.bar_ts => {
                    // A newer candle started → the previous one is final.
                    let mut closed = cur.clone();
                    closed.closed = true;
                    out.push(closed);
                    out.push(ev.clone());
                    self.cur = Some(ev);
                }
                // Older/out-of-order row: ignore.
                Some(_) => {}
            }
        }
        Ok(out)
    }
}

/// Parse one v2 OHLC row into a (partial) event. Numbers are JSON numbers here, not strings.
fn parse_row(row: &Value) -> Result<LiveEvent> {
    let num = |key: &str| -> Result<f64> {
        row.get(key).and_then(Value::as_f64).ok_or_else(|| anyhow!("kraken ohlc field {key}"))
    };
    let begin = row
        .get("interval_begin")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("kraken ohlc interval_begin"))?;
    let bar_ts = OffsetDateTime::parse(begin, &Rfc3339)
        .map_err(|e| anyhow!("kraken interval_begin parse: {e}"))?;
    let provider_ts = row
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
        .unwrap_or_else(OffsetDateTime::now_utc);
    Ok(LiveEvent {
        bar_ts,
        open: num("open")?,
        high: num("high")?,
        low: num("low")?,
        close: num("close")?,
        volume: num("volume")?,
        closed: false,
        provider_ts,
    })
}

/// Canonical timeframe → Kraken interval in minutes (its accepted set).
fn interval_min(tf: &str) -> u32 {
    match tf {
        "1m" => 1,
        "5m" => 5,
        "15m" => 15,
        "1h" => 60,
        "4h" => 240,
        "1d" => 1440,
        _ => 10080,
    }
}
