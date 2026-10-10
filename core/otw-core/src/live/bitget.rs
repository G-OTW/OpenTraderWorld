//! Bitget live candles (class A: candle channel *with* snapshot).
//!
//! `wss://ws.bitget.com/v2/ws/public`, subscribing to `candle<interval>`. Every message
//! names its own `instType`, `channel` and `instId`, so one socket carries every pane on
//! this connector and the routing is unambiguous.
//!
//! Three properties of the feed decide the shape here:
//!   • **No closed flag.** A row is final once one with a newer open time arrives, so
//!     closure is inferred on rollover, per channel.
//!   • **A refusal names its instrument.** An unknown pair comes back as an `error` event
//!     carrying the `arg` that caused it, which is exactly the per-instrument fault a
//!     shared socket needs: one bad symbol must not take the other panes down.
//!   • **Bitget wants to hear from us.** The server drops a socket it has had no `ping`
//!     from for two minutes. There is no timer on this transport, so a ping rides out with
//!     whatever the feed says next; a socket quiet for two minutes is dropped and
//!     reconnected by the supervisor, which is a transient fault rather than a fatal one.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::histdata::bitget::{market, Market};

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

/// How long the feed lets a socket go without saying anything before it pings.
const PING_EVERY: Duration = Duration::from_secs(20);

pub struct BitgetLive;

#[async_trait::async_trait]
impl StreamConnector for BitgetLive {
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Native)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        // Which book this connector reads is its own setting, not the instrument's, so the
        // channel key is completed when the socket opens; see `BitgetFeed::key`.
        Ok(format!("{}|{}", channel(&inst.timeframe)?, inst.symbol.to_uppercase()))
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        let inst_type = inst_type(market(req.secrets)?);
        run_feed(
            BitgetFeed { inst_type, cur: HashMap::new(), last_sent: Instant::now(), pending: Vec::new() },
            req.instruments,
        )
        .await
    }
}

/// The `instType` a market trades under on the socket.
fn inst_type(m: Market) -> &'static str {
    match m {
        Market::Spot => "SPOT",
        Market::Futures => "USDT-FUTURES",
    }
}

struct BitgetFeed {
    /// Every instrument on this connector shares it: it is the connector's setting.
    inst_type: &'static str,
    /// Last partial event per channel key, for the rollover close.
    cur: HashMap<String, LiveEvent>,
    /// When the socket last heard from us, for the keepalive.
    last_sent: Instant,
    /// Frames queued for the next drain (the keepalive ping).
    pending: Vec<String>,
}

/// The key the hub routes on: what `channel_of` builds, which is the channel token and the
/// instrument. The market is the connector's setting, not the instrument's, so it is not
/// part of the key: one connector reads one book.
fn key(channel: &str, inst_id: &str) -> String {
    format!("{channel}|{}", inst_id.to_uppercase())
}

impl BitgetFeed {
    fn frame(&mut self, op: &str, instruments: &[Instrument]) -> Vec<String> {
        let args: Vec<Value> = instruments
            .iter()
            .filter_map(|i| {
                Some(json!({
                    "instType": self.inst_type,
                    "channel": channel(&i.timeframe).ok()?,
                    "instId": i.symbol.to_uppercase(),
                }))
            })
            .collect();
        if args.is_empty() {
            return Vec::new();
        }
        self.last_sent = Instant::now();
        vec![json!({ "op": op, "args": args }).to_string()]
    }
}

impl Feed for BitgetFeed {
    fn url(&self) -> String {
        "wss://ws.bitget.com/v2/ws/public".to_string()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        self.frame("subscribe", instruments)
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        for i in instruments {
            if let Ok(c) = channel(&i.timeframe) {
                self.cur.remove(&key(c, &i.symbol));
            }
        }
        self.frame("unsubscribe", instruments)
    }

    fn drain_sends(&mut self) -> Vec<String> {
        let mut out = std::mem::take(&mut self.pending);
        if self.last_sent.elapsed() >= PING_EVERY {
            // Bitget's keepalive is the bare word, not a JSON frame.
            out.push("ping".into());
            self.last_sent = Instant::now();
        }
        out
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        if text.trim() == "pong" {
            return Ok(Vec::new());
        }
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("bitget json: {e}"))?;
        let arg = v.get("arg").unwrap_or(&Value::Null);
        let channel_name = arg.get("channel").and_then(Value::as_str).unwrap_or_default();
        let inst_id = arg.get("instId").and_then(Value::as_str).unwrap_or_default();

        // A refusal that names its own instrument: the pane it belongs to stops, the socket
        // and every other pane on it carry on.
        if v.get("event").and_then(Value::as_str) == Some("error") {
            let msg = v.get("msg").and_then(Value::as_str).unwrap_or("subscription refused");
            if channel_name.is_empty() || inst_id.is_empty() {
                return Err(LiveError::err(LiveFault::Transport, format!("Bitget: {msg}")));
            }
            return Ok(vec![FeedUpdate::fault(
                key(channel_name, inst_id),
                &LiveError::err(
                    LiveFault::Symbol,
                    format!("Bitget does not list {inst_id} on this market: {msg}"),
                ),
            )]);
        }
        let Some(rows) = v.get("data").and_then(Value::as_array) else {
            // Subscription acks and anything else control-shaped.
            return Ok(Vec::new());
        };
        if channel_name.is_empty() || inst_id.is_empty() {
            return Ok(Vec::new());
        }
        let key = key(channel_name, inst_id);
        let provider_ts = v
            .get("ts")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|ms| OffsetDateTime::from_unix_timestamp(ms / 1000).ok())
            .unwrap_or_else(OffsetDateTime::now_utc);
        let mut out = Vec::new();
        for row in rows {
            let ev = parse_row(row, provider_ts)?;
            match self.cur.get(&key) {
                None => {
                    out.push(FeedUpdate::bar(key.clone(), ev.clone()));
                    self.cur.insert(key.clone(), ev);
                }
                Some(cur) if ev.bar_ts == cur.bar_ts => {
                    out.push(FeedUpdate::bar(key.clone(), ev.clone()));
                    self.cur.insert(key.clone(), ev);
                }
                Some(cur) if ev.bar_ts > cur.bar_ts => {
                    // A newer candle started, so the previous one is final.
                    let mut closed = cur.clone();
                    closed.closed = true;
                    out.push(FeedUpdate::bar(key.clone(), closed));
                    out.push(FeedUpdate::bar(key.clone(), ev.clone()));
                    self.cur.insert(key.clone(), ev);
                }
                // Older row (a snapshot replaying history): nothing to update.
                Some(_) => {}
            }
        }
        Ok(out)
    }
}

/// One `[ openTime, open, high, low, close, baseVolume, … ]` row as a partial event.
fn parse_row(row: &Value, provider_ts: OffsetDateTime) -> Result<LiveEvent> {
    let cell = |i: usize| -> Result<f64> {
        row.get(i)
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("bitget candle field {i}"))
    };
    let open_ms: i64 = row
        .get(0)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("bitget candle open time"))?;
    Ok(LiveEvent {
        bar_ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
        open: cell(1)?,
        high: cell(2)?,
        low: cell(3)?,
        close: cell(4)?,
        volume: cell(5)?,
        closed: false,
        provider_ts,
    })
}

/// Canonical timeframe → Bitget candle channel. The daily and weekly ones are the
/// UTC-anchored variants, matching what the download stores: Bitget's plain `candle1D`
/// opens at 16:00 UTC.
fn channel(tf: &str) -> Result<&'static str> {
    Ok(match tf {
        "1m" => "candle1m",
        "5m" => "candle5m",
        "15m" => "candle15m",
        "1h" => "candle1H",
        "4h" => "candle4H",
        "1d" => "candle1Dutc",
        "1w" => "candle1Wutc",
        other => {
            return Err(LiveError::err(
                LiveFault::Unsupported,
                format!("Bitget has no live candle channel for {other}"),
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> BitgetFeed {
        BitgetFeed {
            inst_type: "SPOT",
            cur: HashMap::new(),
            last_sent: Instant::now(),
            pending: Vec::new(),
        }
    }

    fn row(channel: &str, inst: &str, open_ms: i64, close: f64) -> String {
        format!(
            r#"{{"action":"update","arg":{{"instType":"SPOT","channel":"{channel}","instId":"{inst}"}},
               "data":[["{open_ms}","1","2","0.5","{close}","10","900","900"]],"ts":"1790014542099"}}"#
        )
    }

    #[test]
    fn two_panes_on_one_socket_keep_their_own_forming_candle() {
        let mut f = feed();
        f.on_message(&row("candle1m", "BTCUSDT", 1_790_014_500_000, 10.0)).unwrap();
        f.on_message(&row("candle1m", "ETHUSDT", 1_790_014_500_000, 20.0)).unwrap();
        // BTC rolls over; ETH's candle must not be closed by it.
        let out = f.on_message(&row("candle1m", "BTCUSDT", 1_790_014_560_000, 11.0)).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|u| u.channel == "candle1m|BTCUSDT"));
        assert_eq!(f.cur["candle1m|ETHUSDT"].close, 20.0);
        assert!(!f.cur["candle1m|ETHUSDT"].closed);
    }

    #[test]
    fn the_routing_key_is_the_one_the_hub_asked_for() {
        let inst = Instrument::new("btcusdt", "crypto", "1h");
        assert_eq!(BitgetLive.channel_of(&inst).unwrap(), "candle1H|BTCUSDT");
        let mut f = feed();
        let out = f
            .on_message(&row("candle1H", "BTCUSDT", 1_790_013_600_000, 3.0))
            .unwrap();
        assert_eq!(out[0].channel, BitgetLive.channel_of(&inst).unwrap());
    }

    #[test]
    fn an_unknown_pair_faults_its_own_pane_and_not_the_socket() {
        let mut f = feed();
        let out = f
            .on_message(
                r#"{"event":"error","arg":{"instType":"SPOT","channel":"candle1m","instId":"NOSUCHPAIR"},
                    "code":30001,"msg":"instId:NOSUCHPAIR doesn't exist","op":"subscribe"}"#,
            )
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, "candle1m|NOSUCHPAIR");
        match &out[0].payload {
            super::super::Payload::Fault { fault, .. } => assert_eq!(*fault, LiveFault::Symbol),
            _ => panic!("expected a fault"),
        }
    }

    #[test]
    fn a_daily_candle_is_the_utc_one() {
        assert_eq!(channel("1d").unwrap(), "candle1Dutc");
        assert_eq!(channel("1w").unwrap(), "candle1Wutc");
        let e = channel("30m").unwrap_err();
        assert_eq!(super::super::fault_of(&e), LiveFault::Unsupported);
    }
}
