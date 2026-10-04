//! Binance USDⓈ-M futures live klines (class B: candle channel, no snapshot).
//!
//! `wss://fstream.binance.com`, the futures twin of the spot feed: one socket carries every
//! instrument the chart is watching on this connector, each message names its own symbol
//! and interval, and every frame carries the forming bar with an explicit closed flag
//! (`k.x`), so nothing is folded or inferred here. Keyless, like the REST connector.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::histdata::binance_futures::interval;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

pub struct BinanceFuturesLive;

#[async_trait::async_trait]
impl StreamConnector for BinanceFuturesLive {
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Native)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        channel(inst)
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        // The endpoint wants a stream in the path; the rest join by frame right after.
        let first = req
            .instruments
            .first()
            .ok_or_else(|| anyhow!("binance futures live: no instrument to open with"))?;
        run_feed(
            BinanceFuturesFeed { opening: channel(first)?, next_id: 1 },
            req.instruments,
        )
        .await
    }
}

/// `btcusdt@kline_1m`: what a subscribe frame names and what every message identifies itself
/// with, so one function serves both sides of the routing.
fn channel(inst: &Instrument) -> Result<String> {
    let token = interval(&inst.timeframe).map_err(|_| {
        LiveError::err(
            LiveFault::Unsupported,
            format!("Binance futures has no live kline channel for {}", inst.timeframe),
        )
    })?;
    Ok(format!("{}@kline_{token}", inst.symbol.to_lowercase()))
}

struct BinanceFuturesFeed {
    /// The stream in the connect URL. Binance has no bare socket: the first instrument opens
    /// the connection and the others are subscribed onto it.
    opening: String,
    next_id: u64,
}

impl BinanceFuturesFeed {
    fn frame(&mut self, method: &str, instruments: &[Instrument]) -> Vec<String> {
        let params: Vec<String> = instruments.iter().filter_map(|i| channel(i).ok()).collect();
        if params.is_empty() {
            return Vec::new();
        }
        self.next_id += 1;
        vec![json!({ "method": method, "params": params, "id": self.next_id }).to_string()]
    }
}

impl Feed for BinanceFuturesFeed {
    fn url(&self) -> String {
        format!("wss://fstream.binance.com/ws/{}", self.opening)
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        // Re-subscribing the stream already in the path is a no-op on Binance's side, so the
        // opening set is sent whole rather than special-cased.
        self.frame("SUBSCRIBE", instruments)
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        self.frame("UNSUBSCRIBE", instruments)
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| anyhow!("binance futures json: {e}"))?;
        let Some(k) = v.get("k") else {
            // Non-kline control frames (subscription acks) are ignored.
            return Ok(Vec::new());
        };
        let num = |key: &str| -> Result<f64> {
            k.get(key)
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow!("binance futures kline field {key}"))
        };
        let open_ms = k
            .get("t")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("binance futures kline t"))?;
        let event_ms = v.get("E").and_then(Value::as_i64).unwrap_or(open_ms);
        // The message names its own channel: symbol on the envelope, interval on the kline.
        let symbol = v
            .get("s")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("binance futures kline symbol"))?
            .to_lowercase();
        let token = k
            .get("i")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("binance futures kline interval"))?;
        Ok(vec![FeedUpdate::bar(
            format!("{symbol}@kline_{token}"),
            LiveEvent {
                bar_ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
                open: num("o")?,
                high: num("h")?,
                low: num("l")?,
                close: num("c")?,
                volume: num("v")?,
                closed: k.get("x").and_then(Value::as_bool).unwrap_or(false),
                provider_ts: OffsetDateTime::from_unix_timestamp(event_ms / 1000)?,
            },
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_names_the_channel_it_belongs_to() {
        let mut feed = BinanceFuturesFeed { opening: "btcusdt@kline_1m".into(), next_id: 1 };
        let text = r#"{"e":"kline","E":1750000000000,"s":"ETHUSDT","k":{"t":1750000000000,
            "i":"5m","o":"1","h":"2","l":"0.5","c":"1.5","v":"10","x":true}}"#;
        let out = feed.on_message(text).unwrap();
        assert_eq!(out.len(), 1);
        // Routing is by symbol *and* interval: two panes on one symbol at two timeframes
        // ride the same socket and must not be handed each other's bars.
        assert_eq!(out[0].channel, "ethusdt@kline_5m");
        assert_eq!(
            out[0].channel,
            channel(&Instrument::new("ETHUSDT", "crypto", "5m")).unwrap()
        );
    }

    #[test]
    fn an_unserved_timeframe_is_refused_rather_than_approximated() {
        let e = channel(&Instrument::new("BTCUSDT", "crypto", "30m")).unwrap_err();
        assert_eq!(super::super::fault_of(&e), LiveFault::Unsupported);
    }
}
