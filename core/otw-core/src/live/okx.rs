//! OKX live candles (class B: candle channel with an explicit closed flag).
//!
//! `wss://ws.okx.com:8443/ws/v5/business`, subscribing to `candle<bar>`. Candles live on
//! the *business* socket rather than the public one, which is the single most common reason
//! an otherwise correct subscription answers nothing.
//!
//! Every data frame names its own `channel` and `instId`, so one socket carries every pane
//! on this connector, and the row's last field (`confirm`) says whether the bar is final:
//! nothing is inferred on rollover here.
//!
//! **A refusal does not name its `arg`.** OKX answers an unknown instrument with an
//! `error` event whose channel and instrument are written into the *message text* and
//! nowhere else. They are read back out of it, because the alternative on a shared socket
//! is either killing every other pane or leaving one pane connected to silence.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::histdata::okx::bar;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

/// How long the feed lets the socket go without saying anything before it pings. OKX drops
/// a connection it has heard nothing on for 30 seconds.
const PING_EVERY: Duration = Duration::from_secs(20);

pub struct OkxLive;

#[async_trait::async_trait]
impl StreamConnector for OkxLive {
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Native)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(key(channel(&inst.timeframe)?, &inst.symbol))
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        run_feed(
            OkxFeed { last_sent: Instant::now(), seen: HashMap::new() },
            req.instruments,
        )
        .await
    }
}

/// The key the hub routes on.
fn key(channel: &str, inst_id: &str) -> String {
    format!("{channel}|{}", inst_id.to_uppercase())
}

struct OkxFeed {
    /// When the socket last heard from us, for the keepalive.
    last_sent: Instant,
    /// Last bar open time seen per channel key, so a replayed snapshot row is not
    /// re-emitted as if it were news.
    seen: HashMap<String, OffsetDateTime>,
}

impl OkxFeed {
    fn frame(&mut self, op: &str, instruments: &[Instrument]) -> Vec<String> {
        let args: Vec<Value> = instruments
            .iter()
            .filter_map(|i| {
                Some(json!({
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

impl Feed for OkxFeed {
    fn url(&self) -> String {
        "wss://ws.okx.com:8443/ws/v5/business".to_string()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        self.frame("subscribe", instruments)
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        for i in instruments {
            if let Ok(c) = channel(&i.timeframe) {
                self.seen.remove(&key(c, &i.symbol));
            }
        }
        self.frame("unsubscribe", instruments)
    }

    fn drain_sends(&mut self) -> Vec<String> {
        if self.last_sent.elapsed() < PING_EVERY {
            return Vec::new();
        }
        self.last_sent = Instant::now();
        // OKX's keepalive is the bare word, not a JSON frame.
        vec!["ping".into()]
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        if text.trim() == "pong" {
            return Ok(Vec::new());
        }
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("okx json: {e}"))?;
        if v.get("event").and_then(Value::as_str) == Some("error") {
            let msg = v.get("msg").and_then(Value::as_str).unwrap_or("subscription refused");
            // The refusal names its channel and instrument inside the sentence and nowhere
            // else, so the pane it belongs to is found there or not at all.
            return Ok(match refused_channel(msg) {
                Some(k) => vec![FeedUpdate::fault(
                    k,
                    &LiveError::err(LiveFault::Symbol, format!("OKX refused this instrument: {msg}")),
                )],
                None => {
                    return Err(LiveError::err(LiveFault::Transport, format!("OKX: {msg}")));
                }
            });
        }
        let arg = v.get("arg").unwrap_or(&Value::Null);
        let channel_name = arg.get("channel").and_then(Value::as_str).unwrap_or_default();
        let inst_id = arg.get("instId").and_then(Value::as_str).unwrap_or_default();
        let Some(rows) = v.get("data").and_then(Value::as_array) else {
            // Subscription acks and anything else control-shaped.
            return Ok(Vec::new());
        };
        if channel_name.is_empty() || inst_id.is_empty() {
            return Ok(Vec::new());
        }
        let k = key(channel_name, inst_id);
        let mut out = Vec::new();
        for row in rows {
            let ev = parse_row(row)?;
            // A subscription replays recent candles; only the ones at or after the newest
            // bar already seen are news.
            if self.seen.get(&k).is_some_and(|prev| ev.bar_ts < *prev) {
                continue;
            }
            self.seen.insert(k.clone(), ev.bar_ts);
            out.push(FeedUpdate::bar(k.clone(), ev));
        }
        Ok(out)
    }
}

/// `[ ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm ]` as an event. `confirm` is `"1"`
/// once the bar is final, so nothing is inferred from a rollover here.
fn parse_row(row: &Value) -> Result<LiveEvent> {
    let cell = |i: usize| -> Result<f64> {
        row.get(i)
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("okx candle field {i}"))
    };
    let open_ms: i64 = row
        .get(0)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("okx candle open time"))?;
    Ok(LiveEvent {
        bar_ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
        open: cell(1)?,
        high: cell(2)?,
        low: cell(3)?,
        close: cell(4)?,
        volume: cell(5)?,
        closed: row.get(8).and_then(Value::as_str) == Some("1"),
        provider_ts: OffsetDateTime::now_utc(),
    })
}

/// The routing key inside a refusal's sentence, e.g. "…channel:candle1m,instId:FOO-BAR
/// doesn't exist…". `None` when the message names neither, which makes it the session's
/// problem rather than one pane's.
fn refused_channel(msg: &str) -> Option<String> {
    let after = |tag: &str| -> Option<String> {
        let rest = msg.split_once(tag)?.1;
        let end = rest
            .find(|c: char| c == ',' || c.is_whitespace() || c == '.')
            .unwrap_or(rest.len());
        Some(rest[..end].to_string()).filter(|s| !s.is_empty())
    };
    let channel = after("channel:")?;
    let inst = after("instId:")?;
    Some(key(&channel, &inst))
}

/// Canonical timeframe → OKX candle channel. The daily and weekly ones are the
/// UTC-anchored variants, matching what the download stores.
fn channel(tf: &str) -> Result<&'static str> {
    match bar(tf) {
        Ok(token) => Ok(match token {
            "1m" => "candle1m",
            "5m" => "candle5m",
            "15m" => "candle15m",
            "1H" => "candle1H",
            "4H" => "candle4H",
            "1Dutc" => "candle1Dutc",
            _ => "candle1Wutc",
        }),
        Err(_) => Err(LiveError::err(
            LiveFault::Unsupported,
            format!("OKX has no live candle channel for {tf}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> OkxFeed {
        OkxFeed { last_sent: Instant::now(), seen: HashMap::new() }
    }

    #[test]
    fn the_closed_flag_is_read_and_not_inferred() {
        let mut f = feed();
        let forming = r#"{"arg":{"channel":"candle1m","instId":"BTC-USDT"},
            "data":[["1790014800000","86050.4","86050.8","86049.9","86049.9","0.24","20689","20689","0"]]}"#;
        let out = f.on_message(forming).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, "candle1m|BTC-USDT");
        match &out[0].payload {
            super::super::Payload::Bar(ev) => assert!(!ev.closed),
            _ => panic!("expected a bar"),
        }
        let done = forming.replace(r#","20689","0"]]}"#, r#","20689","1"]]}"#);
        match &f.on_message(&done).unwrap()[0].payload {
            super::super::Payload::Bar(ev) => assert!(ev.closed),
            _ => panic!("expected a bar"),
        }
    }

    #[test]
    fn a_refusal_is_routed_to_the_pane_named_in_its_sentence() {
        let mut f = feed();
        let out = f
            .on_message(
                r#"{"event":"error","msg":"Subscribe failed, wrong URL or channel:candle1m,instId:NOSUCH-PAIR doesn't exist. Please use the correct URL.","code":"60018","connId":"84c65782"}"#,
            )
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, "candle1m|NOSUCH-PAIR");
        match &out[0].payload {
            super::super::Payload::Fault { fault, .. } => assert_eq!(*fault, LiveFault::Symbol),
            _ => panic!("expected a fault"),
        }
        // A refusal naming nothing belongs to the session, not to one pane.
        let Err(e) = f.on_message(r#"{"event":"error","msg":"Wrong URL","code":"60012"}"#) else {
            panic!("a refusal naming nothing should end the session");
        };
        assert_eq!(super::super::fault_of(&e), LiveFault::Transport);
    }

    #[test]
    fn the_routing_key_is_the_one_the_hub_asked_for() {
        let inst = Instrument::new("btc-usdt", "crypto", "1d");
        assert_eq!(OkxLive.channel_of(&inst).unwrap(), "candle1Dutc|BTC-USDT");
        let e = channel("30m").unwrap_err();
        assert_eq!(super::super::fault_of(&e), LiveFault::Unsupported);
    }
}
