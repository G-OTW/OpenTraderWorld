//! Kraken v2 live OHLC (class A: candle channel *with* snapshot).
//!
//! `wss://ws.kraken.com/v2`, subscribe to `ohlc`. The first message is a `snapshot` of recent
//! candles (this is the "snapshot replaces REST" property), then `update`s for the forming
//! candle. Kraken sends **no closed flag**: a bar is final once a row with a newer
//! `interval_begin` arrives, so closure is inferred on rollover, per symbol.
//!
//! **One socket per interval**, not per instrument: a v2 OHLC row names its symbol but not
//! the interval it belongs to, so two intervals on one socket could not be told apart. Making
//! the interval the socket class keeps the routing unambiguous and still puts every symbol of
//! a timeframe on a single connection.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveEvent, LiveSession, SessionRequest, SourceGrain,
    StreamConnector,
};

pub struct KrakenLive;

#[async_trait::async_trait]
impl StreamConnector for KrakenLive {
    fn socket_class(&self, inst: &Instrument) -> Result<String> {
        Ok(interval_min(&inst.timeframe).to_string())
    }

    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Native)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(inst.symbol.to_uppercase())
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        let first = req
            .instruments
            .first()
            .ok_or_else(|| anyhow!("kraken live: no instrument to open with"))?;
        run_feed(
            KrakenFeed { interval_min: interval_min(&first.timeframe), cur: HashMap::new() },
            req.instruments,
        )
        .await
    }
}

struct KrakenFeed {
    /// Every instrument on this socket shares it: it is the socket's class.
    interval_min: u32,
    /// Last partial event per symbol, for the rollover close.
    cur: HashMap<String, LiveEvent>,
}

impl Feed for KrakenFeed {
    fn url(&self) -> String {
        "wss://ws.kraken.com/v2".to_string()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        self.frame("subscribe", instruments)
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        for i in instruments {
            self.cur.remove(&i.symbol.to_uppercase());
        }
        self.frame("unsubscribe", instruments)
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
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
            let symbol = row
                .get("symbol")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("kraken ohlc symbol"))?
                .to_uppercase();
            let ev = parse_row(row)?;
            match self.cur.get(&symbol) {
                // First candle we see for this symbol becomes its current one.
                None => {
                    out.push(FeedUpdate::bar(symbol.clone(), ev.clone()));
                    self.cur.insert(symbol, ev);
                }
                Some(cur) if ev.bar_ts == cur.bar_ts => {
                    out.push(FeedUpdate::bar(symbol.clone(), ev.clone()));
                    self.cur.insert(symbol, ev);
                }
                Some(cur) if ev.bar_ts > cur.bar_ts => {
                    // A newer candle started → the previous one is final.
                    let mut closed = cur.clone();
                    closed.closed = true;
                    out.push(FeedUpdate::bar(symbol.clone(), closed));
                    out.push(FeedUpdate::bar(symbol.clone(), ev.clone()));
                    self.cur.insert(symbol, ev);
                }
                // Older/out-of-order row: ignore.
                Some(_) => {}
            }
        }
        Ok(out)
    }
}

impl KrakenFeed {
    fn frame(&self, method: &str, instruments: &[Instrument]) -> Vec<String> {
        if instruments.is_empty() {
            return Vec::new();
        }
        let symbols: Vec<String> = instruments.iter().map(|i| i.symbol.to_uppercase()).collect();
        vec![json!({
            "method": method,
            "params": {
                "channel": "ohlc",
                "symbol": symbols,
                "interval": self.interval_min,
            }
        })
        .to_string()]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(symbol: &str, begin: &str, close: f64) -> String {
        format!(
            r#"{{"channel":"ohlc","data":[{{"symbol":"{symbol}","interval_begin":"{begin}",
               "open":1.0,"high":2.0,"low":0.5,"close":{close},"volume":3.0}}]}}"#
        )
    }

    #[test]
    fn two_symbols_on_one_socket_keep_their_own_forming_candle() {
        let mut feed = KrakenFeed { interval_min: 1, cur: HashMap::new() };
        feed.on_message(&row("BTC/USD", "2026-09-05T10:00:00Z", 10.0)).unwrap();
        feed.on_message(&row("ETH/USD", "2026-09-05T10:00:00Z", 20.0)).unwrap();
        // BTC rolls over; ETH's candle must not be closed by it.
        let out = feed.on_message(&row("BTC/USD", "2026-09-05T10:01:00Z", 11.0)).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|u| u.channel == "BTC/USD"));
        assert_eq!(feed.cur["ETH/USD"].close, 20.0);
        assert!(!feed.cur["ETH/USD"].closed);
    }

    #[test]
    fn the_socket_class_is_the_interval() {
        // Kraken's rows do not name their interval, so two timeframes are two sockets.
        let k = KrakenLive;
        assert_eq!(k.socket_class(&Instrument::new("BTC/USD", "crypto", "1h")).unwrap(), "60");
        assert_eq!(k.socket_class(&Instrument::new("BTC/USD", "crypto", "1m")).unwrap(), "1");
    }
}
