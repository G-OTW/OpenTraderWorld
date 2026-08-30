//! Live market data over provider WebSockets.
//!
//! This module extends Historical Data *forward*: the same datasets and the same REST
//! [`crate::histdata::Connector`]s used for history, plus a WS feed that streams the
//! forming bar. The **current (unclosed) bar is built server-side only** — if the browser
//! aggregated its own ticks in parallel, two clients would show two different candles.
//!
//! Layers:
//!   - [`LiveEvent`] — one normalized bar update: `{ bar_ts, o,h,l,c,v, closed, provider_ts }`.
//!     `bar_ts` is the bar's **open time as the exchange reports it** (ms UTC), the canonical
//!     idempotency key; every application of an event is an upsert on it.
//!   - [`Feed`] + [`run_feed`] — the transport: connect one WS, send subscribe frames, answer
//!     pings, and turn provider messages (and, for trade-aggregating providers, a close timer)
//!     into `LiveEvent`s on an mpsc stream. Reconnection is *not* here — it's the supervisor's.
//!   - [`StreamConnector`] — a provider's live capability: build a [`Feed`] for one
//!     `(symbol, timeframe)`. Registry in [`stream_connector_for`].
//!   - [`hub::LiveHub`] — one supervisor task per live dataset: seed via REST, overlap-merge
//!     the buffered WS events (so there is no gap between REST and the stream), persist closed
//!     bars, backfill detected gaps, broadcast every update, and reconnect from cold on drop.

use std::pin::Pin;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use otw_store::histdata::Bar;

mod binance;
mod coinbase;
pub mod hub;
mod kraken;

/// How many trailing bars the supervisor seeds from REST when a dataset has no stored
/// history yet (enough to warm up typical indicators without pulling a full download).
pub const SEED_BARS: i64 = 300;

/// One normalized live bar update. Timestamps are UTC; `bar_ts` is the exchange's bar open
/// time (the idempotency key), `provider_ts` the event/emission time (used for lag metrics
/// and last-writer-wins when the same `bar_ts` arrives repeatedly).
#[derive(Debug, Clone)]
pub struct LiveEvent {
    pub bar_ts: OffsetDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// True once this is the bar's final value (persist + advance); false while it forms.
    pub closed: bool,
    pub provider_ts: OffsetDateTime,
}

impl LiveEvent {
    /// The persistable OHLCV bar (adjusted fields are always None for live crypto/fx).
    pub fn as_bar(&self) -> Bar {
        Bar {
            ts: self.bar_ts,
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        }
    }

    /// SSE payload for one event. `lag_ms` = now − provider_ts, the delivery delay we bear.
    pub fn to_wire(&self) -> Value {
        let now = OffsetDateTime::now_utc();
        let lag_ms = ((now - self.provider_ts).whole_milliseconds()).max(0);
        json!({
            "ts": self.bar_ts.format(&Rfc3339).unwrap_or_default(),
            "o": self.open,
            "h": self.high,
            "l": self.low,
            "c": self.close,
            "v": self.volume,
            "closed": self.closed,
            "lag_ms": lag_ms,
        })
    }
}

// ── Feed transport ──────────────────────────────────────────────────────────────

/// A provider's live socket, reduced to the few hooks [`run_feed`] needs. One instance is
/// built per `(symbol, timeframe)` subscription and consumed by a single reader task.
///
/// A provider that streams candles directly (Binance, Kraken) only implements `on_message`.
/// A provider that streams *trades* (Coinbase) aggregates them in `on_message` and uses
/// `on_tick` to close a bar on the interval boundary when trades go quiet.
pub trait Feed: Send + 'static {
    /// The `wss://…` URL to connect.
    fn url(&self) -> String;
    /// Text frames to send right after the connection opens (empty for path-based streams).
    fn subscribe(&self) -> Vec<String> {
        Vec::new()
    }
    /// Parse one inbound text frame into zero or more normalized events.
    fn on_message(&mut self, text: &str) -> Result<Vec<LiveEvent>>;
    /// Timer hook for trade-aggregating feeds: close the current bar once its interval has
    /// elapsed even if no further trade arrives. Candle feeds leave this empty.
    fn on_tick(&mut self, _now: OffsetDateTime) -> Vec<LiveEvent> {
        Vec::new()
    }
    /// Close-timer cadence, or `None` for candle feeds that need no timer.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }
}

/// A live stream of normalized events. `Err` ends the session (the supervisor reconnects).
pub type LiveStream = Pin<Box<dyn futures::Stream<Item = Result<LiveEvent>> + Send>>;

/// Connect a [`Feed`], drive it on its own task, and hand back the event stream. The task
/// owns the socket: it answers pings, forwards parsed events, and exits when the socket
/// closes/errors **or** when the returned stream is dropped (the mpsc send then fails).
pub async fn run_feed<F: Feed>(mut feed: F) -> Result<LiveStream> {
    let url = feed.url();
    let (ws, _) = connect_async(&url).await.with_context(|| format!("ws connect {url}"))?;
    let (tx, rx) = mpsc::channel::<Result<LiveEvent>>(512);

    tokio::spawn(async move {
        let (mut write, mut read) = ws.split();
        for frame in feed.subscribe() {
            if write.send(Message::Text(frame)).await.is_err() {
                return;
            }
        }
        // Optional close-timer for trade-aggregating feeds.
        let mut ticker = feed.tick_interval().map(tokio::time::interval);

        loop {
            tokio::select! {
                msg = read.next() => match msg {
                    Some(Ok(Message::Text(t))) => {
                        if !forward(&tx, feed.on_message(t.as_str())).await { return; }
                    }
                    // A few providers send JSON as binary frames.
                    Some(Ok(Message::Binary(b))) => {
                        if let Ok(s) = std::str::from_utf8(&b) {
                            if !forward(&tx, feed.on_message(s)).await { return; }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if write.send(Message::Pong(p)).await.is_err() { return; }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        let _ = tx.send(Err(anyhow!("ws read: {e}"))).await;
                        return;
                    }
                },
                _ = async { ticker.as_mut().unwrap().tick().await }, if ticker.is_some() => {
                    let evs = feed.on_tick(OffsetDateTime::now_utc());
                    for e in evs {
                        if tx.send(Ok(e)).await.is_err() { return; }
                    }
                }
            }
        }
    });

    Ok(Box::pin(ReceiverStream::new(rx)))
}

/// Push the result of a parse to the consumer. Returns false when the session should end
/// (consumer gone, or the parse itself failed).
async fn forward(tx: &mpsc::Sender<Result<LiveEvent>>, parsed: Result<Vec<LiveEvent>>) -> bool {
    match parsed {
        Ok(evs) => {
            for e in evs {
                if tx.send(Ok(e)).await.is_err() {
                    return false;
                }
            }
            true
        }
        Err(e) => {
            let _ = tx.send(Err(e)).await;
            false
        }
    }
}

// ── Provider registry ────────────────────────────────────────────────────────────

/// A provider that can open a live WS stream.
#[async_trait::async_trait]
pub trait StreamConnector: Send + Sync {
    /// Open a live stream for one instrument at one timeframe.
    async fn open(&self, symbol: &str, timeframe: &str) -> Result<LiveStream>;
}

/// The live connector for a provider, or `None` if it has no WS feed (the REST-only
/// `generic` providers — Yahoo/Stooq/… — fall back to polling elsewhere).
pub fn stream_connector_for(provider: &str) -> Option<Box<dyn StreamConnector>> {
    match provider {
        "binance" => Some(Box::new(binance::BinanceLive)),
        "kraken" => Some(Box::new(kraken::KrakenLive)),
        "coinbase" => Some(Box::new(coinbase::CoinbaseLive)),
        _ => None,
    }
}

/// Whether a provider exposes a live WS feed (drives the "Live" affordance in the UI and is
/// re-checked server-side before a subscription is started).
pub fn stream_capable(provider: &str) -> bool {
    stream_connector_for(provider).is_some()
}

// ── Pure helpers (unit-tested) ────────────────────────────────────────────────────

/// Start (open time) of the bar containing `ts`, for a `tf_secs`-second timeframe. Bars are
/// bucketed on the UTC epoch, matching every exchange's candle alignment.
pub fn bar_start(ts: OffsetDateTime, tf_secs: i64) -> OffsetDateTime {
    let secs = ts.unix_timestamp();
    let start = secs - secs.rem_euclid(tf_secs);
    OffsetDateTime::from_unix_timestamp(start).expect("valid epoch")
}

/// Given the last closed bar's open time and a newly closed bar's open time, the open times
/// of any bars missing strictly between them (a technical gap to backfill). Empty when the
/// new bar is the immediate successor, a duplicate, or older (out-of-order).
pub fn missing_buckets(
    last_closed: OffsetDateTime,
    new_closed: OffsetDateTime,
    tf_secs: i64,
) -> Vec<OffsetDateTime> {
    let mut out = Vec::new();
    let mut t = last_closed.unix_timestamp() + tf_secs;
    let end = new_closed.unix_timestamp();
    while t < end {
        if let Ok(dt) = OffsetDateTime::from_unix_timestamp(t) {
            out.push(dt);
        }
        t += tf_secs;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn bar_start_buckets_on_epoch() {
        let t = datetime!(2026-07-23 19:23:37 UTC);
        assert_eq!(bar_start(t, 60), datetime!(2026-07-23 19:23:00 UTC));
        assert_eq!(bar_start(t, 300), datetime!(2026-07-23 19:20:00 UTC));
        assert_eq!(bar_start(t, 3600), datetime!(2026-07-23 19:00:00 UTC));
        // Already on a boundary → unchanged.
        assert_eq!(bar_start(datetime!(2026-07-23 19:00:00 UTC), 3600),
                   datetime!(2026-07-23 19:00:00 UTC));
    }

    #[test]
    fn gap_detection() {
        let a = datetime!(2026-07-23 19:00:00 UTC);
        // Immediate successor at 1m → no gap.
        assert!(missing_buckets(a, datetime!(2026-07-23 19:01:00 UTC), 60).is_empty());
        // Duplicate / older → no gap (idempotent / out-of-order).
        assert!(missing_buckets(a, a, 60).is_empty());
        assert!(missing_buckets(a, datetime!(2026-07-23 18:59:00 UTC), 60).is_empty());
        // Two buckets skipped → the two missing opens between them.
        assert_eq!(
            missing_buckets(a, datetime!(2026-07-23 19:03:00 UTC), 60),
            vec![
                datetime!(2026-07-23 19:01:00 UTC),
                datetime!(2026-07-23 19:02:00 UTC),
            ]
        );
    }
}

/// Live tests against the real (keyless) exchange WebSockets. Network-gated: they no-op
/// unless `OTW_LIVE_TEST=1`, so the normal `cargo test` stays offline and deterministic.
/// Run with: `OTW_LIVE_TEST=1 cargo test -p otw-core live_ -- --nocapture`.
#[cfg(test)]
mod net_tests {
    use super::*;
    use std::time::Duration;

    fn enabled() -> bool {
        std::env::var("OTW_LIVE_TEST").as_deref() == Ok("1")
    }

    /// Pull events off a live stream until `max` collected or `budget` elapses.
    async fn collect(mut s: LiveStream, budget: Duration, max: usize) -> Vec<LiveEvent> {
        let deadline = tokio::time::Instant::now() + budget;
        let mut out = Vec::new();
        while out.len() < max {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                break;
            }
            match tokio::time::timeout(left, s.next()).await {
                Ok(Some(Ok(e))) => out.push(e),
                Ok(Some(Err(e))) => panic!("stream error: {e:#}"),
                Ok(None) => break,
                Err(_) => break, // budget exhausted
            }
        }
        out
    }

    /// OHLC sanity shared by every provider assertion.
    fn assert_valid(e: &LiveEvent) {
        assert!(e.open > 0.0 && e.high > 0.0 && e.low > 0.0 && e.close > 0.0, "prices > 0: {e:?}");
        assert!(e.high >= e.low, "high >= low: {e:?}");
        assert!(e.high >= e.open && e.high >= e.close, "high is the max: {e:?}");
        assert!(e.low <= e.open && e.low <= e.close, "low is the min: {e:?}");
        assert!(e.volume >= 0.0, "volume >= 0: {e:?}");
    }

    #[tokio::test]
    async fn live_binance_klines() {
        if !enabled() {
            return;
        }
        // 1s klines so a closed bar arrives within seconds (not a downloadable timeframe).
        let s = stream_connector_for("binance").unwrap().open("BTCUSDT", "1s").await.unwrap();
        let evs = collect(s, Duration::from_secs(12), 40).await;
        assert!(evs.len() >= 3, "expected several events, got {}", evs.len());
        assert!(evs.iter().any(|e| e.closed), "expected at least one closed bar");
        let mut prev = None;
        for e in &evs {
            assert_valid(e);
            // 1s bars align to the second and never move backwards.
            assert_eq!(e.bar_ts.unix_timestamp() % 1, 0);
            if let Some(p) = prev {
                assert!(e.bar_ts >= p, "bar_ts monotonic non-decreasing");
            }
            prev = Some(e.bar_ts);
        }
    }

    #[tokio::test]
    async fn live_kraken_ohlc_snapshot() {
        if !enabled() {
            return;
        }
        let s = stream_connector_for("kraken").unwrap().open("BTC/USD", "1m").await.unwrap();
        // The snapshot alone yields many historical (closed) candles right away.
        let evs = collect(s, Duration::from_secs(12), 50).await;
        assert!(!evs.is_empty(), "expected snapshot events");
        assert!(evs.iter().any(|e| e.closed), "snapshot should mark historical bars closed");
        for e in &evs {
            assert_valid(e);
            assert_eq!(e.bar_ts.unix_timestamp() % 60, 0, "1m bars align to the minute");
        }
    }

    #[tokio::test]
    async fn coinbase_rest_seed_probe() {
        if !enabled() {
            return;
        }
        // Exercise the *REST* seed path (not WS) through the exact reqwest client + connector
        // the live seed uses, to isolate a seed 400 from a stream issue.
        let rest = crate::histdata::connector_for("coinbase").unwrap();
        let http = crate::histdata::client().unwrap();
        let now = OffsetDateTime::now_utc();
        let from = bar_start(now, 60) - time::Duration::seconds(60 * 298);
        let secrets = std::collections::HashMap::new();
        let r = rest
            .fetch_chunk(&http, &secrets, "BTC-USD", "crypto", "1m", from, now)
            .await;
        match r {
            Ok(c) => println!("coinbase REST seed OK: {} bars", c.bars.len()),
            Err(e) => panic!("coinbase REST seed FAILED: {e:#}"),
        }
    }

    #[tokio::test]
    async fn live_coinbase_trades_aggregate() {
        if !enabled() {
            return;
        }
        let s = stream_connector_for("coinbase").unwrap().open("BTC-USD", "1m").await.unwrap();
        let evs = collect(s, Duration::from_secs(15), 20).await;
        assert!(!evs.is_empty(), "expected trade-aggregated events");
        for e in &evs {
            assert_valid(e);
            assert_eq!(e.bar_ts.unix_timestamp() % 60, 0, "1m buckets align to the minute");
        }
    }
}
