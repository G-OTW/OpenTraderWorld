//! Coinbase live (class C: trades only → locally aggregated candles).
//!
//! Coinbase's public `matches` channel streams individual trades, not candles, so we build
//! the bar ourselves: bucket each trade by `floor(trade_ts / interval)` and close a bucket
//! either when a trade for a later bucket arrives **or** on a timer once the interval has
//! elapsed (so a quiet market still closes on time). Empty buckets (no trades) are not
//! emitted here — the supervisor's gap detection backfills them from REST. This is the
//! representative path for any provider whose native timeframes don't match what we want.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::{bar_start, run_feed, Feed, LiveEvent, LiveStream, StreamConnector};

/// Grace after a bucket's end before the close timer fires, to absorb slightly out-of-order
/// trade timestamps before we finalize the bar.
const CLOSE_GRACE: i64 = 2;

pub struct CoinbaseLive;

#[async_trait::async_trait]
impl StreamConnector for CoinbaseLive {
    async fn open(&self, symbol: &str, timeframe: &str) -> Result<LiveStream> {
        run_feed(CoinbaseFeed {
            product: symbol.to_uppercase(),
            tf_secs: tf_secs(timeframe),
            cur: None,
            last_closed: None,
        })
        .await
    }
}

struct CoinbaseFeed {
    product: String,
    tf_secs: i64,
    /// The forming bucket, if any.
    cur: Option<LiveEvent>,
    /// Newest bucket already emitted closed, so late trades can't resurrect it.
    last_closed: Option<OffsetDateTime>,
}

impl Feed for CoinbaseFeed {
    fn url(&self) -> String {
        "wss://ws-feed.exchange.coinbase.com".to_string()
    }

    fn subscribe(&self) -> Vec<String> {
        vec![json!({
            "type": "subscribe",
            "product_ids": [self.product],
            "channels": ["matches"],
        })
        .to_string()]
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<LiveEvent>> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("coinbase json: {e}"))?;
        match v.get("type").and_then(Value::as_str) {
            Some("match") | Some("last_match") => {}
            _ => return Ok(Vec::new()), // subscriptions ack, heartbeat, errors…
        }
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
        let bucket = bar_start(ts, self.tf_secs);

        // Drop trades for a bucket we've already finalized (out-of-order stragglers).
        if let Some(lc) = self.last_closed {
            if bucket <= lc {
                return Ok(Vec::new());
            }
        }

        let mut out = Vec::new();
        match self.cur.take() {
            None => {
                self.cur = Some(new_bar(bucket, price, size, ts));
                out.push(self.cur.clone().unwrap());
            }
            Some(mut cur) if bucket == cur.bar_ts => {
                fold_trade(&mut cur, price, size, ts);
                out.push(cur.clone());
                self.cur = Some(cur);
            }
            Some(mut cur) => {
                // A trade for a later bucket → close the current bar, open a fresh one.
                cur.closed = true;
                self.last_closed = Some(cur.bar_ts);
                out.push(cur);
                let fresh = new_bar(bucket, price, size, ts);
                out.push(fresh.clone());
                self.cur = Some(fresh);
            }
        }
        Ok(out)
    }

    fn on_tick(&mut self, now: OffsetDateTime) -> Vec<LiveEvent> {
        let Some(cur) = &self.cur else { return Vec::new() };
        let bucket_end = cur.bar_ts.unix_timestamp() + self.tf_secs;
        if now.unix_timestamp() >= bucket_end + CLOSE_GRACE {
            let mut closed = self.cur.take().unwrap();
            closed.closed = true;
            self.last_closed = Some(closed.bar_ts);
            return vec![closed];
        }
        Vec::new()
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(500))
    }
}

fn new_bar(bucket: OffsetDateTime, price: f64, size: f64, ts: OffsetDateTime) -> LiveEvent {
    LiveEvent {
        bar_ts: bucket,
        open: price,
        high: price,
        low: price,
        close: price,
        volume: size,
        closed: false,
        provider_ts: ts,
    }
}

fn fold_trade(bar: &mut LiveEvent, price: f64, size: f64, ts: OffsetDateTime) {
    bar.high = bar.high.max(price);
    bar.low = bar.low.min(price);
    bar.close = price;
    bar.volume += size;
    bar.provider_ts = ts;
}

fn tf_secs(tf: &str) -> i64 {
    match tf {
        "1s" => 1,
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "1h" => 3600,
        "4h" => 14400,
        "1d" => 86400,
        _ => 604800,
    }
}
