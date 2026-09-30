//! Coinbase live (class C: trades only → candles folded by the hub).
//!
//! Coinbase's public `matches` channel streams individual trades, not candles. They are
//! tagged with their product and handed up as-is: the hub folds them into whatever
//! timeframes are watching that product, which is what lets one socket serve a 1m pane and a
//! 1h pane on the same pair. Empty buckets are not invented, the supervisor's gap detection
//! backfills them from REST. Keyless, like the REST connector.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveSession, SessionRequest, SourceGrain,
    StreamConnector,
};

pub struct CoinbaseLive;

#[async_trait::async_trait]
impl StreamConnector for CoinbaseLive {
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Trades)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(inst.symbol.to_uppercase())
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        run_feed(CoinbaseFeed, req.instruments).await
    }
}

struct CoinbaseFeed;

impl Feed for CoinbaseFeed {
    fn url(&self) -> String {
        "wss://ws-feed.exchange.coinbase.com".to_string()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        frame("subscribe", instruments)
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        frame("unsubscribe", instruments)
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("coinbase json: {e}"))?;
        match v.get("type").and_then(Value::as_str) {
            Some("match") | Some("last_match") => {}
            _ => return Ok(Vec::new()), // subscriptions ack, heartbeat, errors…
        }
        let product = v
            .get("product_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("coinbase match product_id"))?
            .to_uppercase();
        let price: f64 = v
            .get("price")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("coinbase match price"))?;
        let size: f64 = v
            .get("size")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let ts = v
            .get("time")
            .and_then(Value::as_str)
            .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
            .unwrap_or_else(OffsetDateTime::now_utc);
        Ok(vec![FeedUpdate::trade(product, price, size, ts)])
    }
}

fn frame(action: &str, instruments: &[Instrument]) -> Vec<String> {
    if instruments.is_empty() {
        return Vec::new();
    }
    let products: Vec<String> = instruments.iter().map(|i| i.symbol.to_uppercase()).collect();
    vec![json!({
        "type": action,
        "product_ids": products,
        "channels": ["matches"],
    })
    .to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Payload;

    #[test]
    fn a_trade_carries_its_product_so_the_hub_can_route_it() {
        let mut feed = CoinbaseFeed;
        let text = r#"{"type":"match","product_id":"ETH-USD","price":"2500.5","size":"0.3",
                       "time":"2026-09-05T10:00:01Z"}"#;
        let out = feed.on_message(text).unwrap();
        assert_eq!(out[0].channel, "ETH-USD");
        match out[0].payload {
            Payload::Trade { price, size, .. } => assert_eq!((price, size), (2500.5, 0.3)),
            _ => panic!("a match is a trade"),
        }
    }
}
