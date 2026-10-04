//! Capital.com live candles (class A: candle channel, two sides, no closed flag).
//!
//! `wss://api-streaming-capital.backend-capital.com/connect`. Three things make this feed
//! unlike the crypto ones:
//!
//!   • **Every frame carries its own credentials.** There is no handshake: the session's
//!     `CST` and `X-SECURITY-TOKEN` go on each subscribe message, so the socket is opened
//!     only after the REST sign-in has happened.
//!   • **A candle arrives twice, once per side.** An `ohlc.event` names `priceType` as bid
//!     or ask, and what the chart shows is the mid, so a bar is emitted once both sides have
//!     been seen and then on every event using the freshest of each. A pane that stays empty
//!     is a market only one side of which is ticking, and `stream_note` says so.
//!   • **No closed flag and no volume.** A bar is final once one with a newer open time
//!     arrives, so closure is inferred on rollover; the volume stays zero rather than being
//!     invented.
//!
//! One socket carries every pane on this connector, up to the forty instruments Capital.com
//! allows per session.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::histdata::capitalcom::resolution;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

/// Capital.com drops a socket it has heard nothing on for ten minutes.
const PING_EVERY: Duration = Duration::from_secs(4 * 60);

pub struct CapitalComLive;

#[async_trait::async_trait]
impl StreamConnector for CapitalComLive {
    fn grain(&self, _inst: &Instrument) -> Result<SourceGrain> {
        Ok(SourceGrain::Native)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(key(&inst.symbol, token(&inst.timeframe)?))
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        // The socket authenticates per message, so the REST session is opened first and its
        // two tokens travel on every frame.
        let s = crate::brokers::capitalcom::session(&crate::brokers::client()?, req.secrets)
            .await
            .map_err(|e| LiveError::err(LiveFault::Auth, format!("Capital.com sign-in: {e:#}")))?;
        run_feed(
            CapitalFeed {
                cst: s.cst,
                security: s.security,
                next_id: 1,
                sides: HashMap::new(),
                cur: HashMap::new(),
                last_sent: Instant::now(),
            },
            req.instruments,
        )
        .await
    }
}

/// The key the hub routes on: the epic and the resolution.
fn key(epic: &str, resolution: &str) -> String {
    format!("{}|{resolution}", epic.to_uppercase())
}

/// The two sides of one forming candle, as far as the socket has told us.
#[derive(Default, Clone)]
struct Sides {
    bid: Option<(f64, f64, f64, f64)>,
    ask: Option<(f64, f64, f64, f64)>,
}

struct CapitalFeed {
    cst: String,
    security: String,
    next_id: u64,
    /// Per channel and bar open time, the sides seen so far.
    sides: HashMap<(String, i64), Sides>,
    /// Last emitted event per channel, for the rollover close.
    cur: HashMap<String, LiveEvent>,
    last_sent: Instant,
}

impl CapitalFeed {
    fn frame(&mut self, destination: &str, instruments: &[Instrument]) -> Vec<String> {
        // One frame per resolution: the payload carries a list of epics and a list of
        // resolutions, and every pairing of the two is subscribed.
        let mut by_resolution: HashMap<&'static str, Vec<String>> = HashMap::new();
        for i in instruments {
            if let Ok(r) = token(&i.timeframe) {
                by_resolution.entry(r).or_default().push(i.symbol.to_uppercase());
            }
        }
        let mut out = Vec::new();
        for (r, mut epics) in by_resolution {
            epics.sort();
            epics.dedup();
            self.next_id += 1;
            out.push(
                json!({
                    "destination": destination,
                    "correlationId": self.next_id.to_string(),
                    "cst": self.cst,
                    "securityToken": self.security,
                    "payload": { "epics": epics, "resolutions": [r], "type": "classic" },
                })
                .to_string(),
            );
        }
        if !out.is_empty() {
            self.last_sent = Instant::now();
        }
        out
    }
}

impl Feed for CapitalFeed {
    fn url(&self) -> String {
        "wss://api-streaming-capital.backend-capital.com/connect".to_string()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        self.frame("OHLCMarketData.subscribe", instruments)
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        for i in instruments {
            if let Ok(r) = token(&i.timeframe) {
                let k = key(&i.symbol, r);
                self.cur.remove(&k);
                self.sides.retain(|(channel, _), _| channel != &k);
            }
        }
        self.frame("OHLCMarketData.unsubscribe", instruments)
    }

    fn drain_sends(&mut self) -> Vec<String> {
        if self.last_sent.elapsed() < PING_EVERY {
            return Vec::new();
        }
        self.last_sent = Instant::now();
        self.next_id += 1;
        vec![json!({
            "destination": "ping",
            "correlationId": self.next_id.to_string(),
            "cst": self.cst,
            "securityToken": self.security,
        })
        .to_string()]
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        let v: Value = serde_json::from_str(text).map_err(|e| anyhow!("capital.com json: {e}"))?;
        let destination = v.get("destination").and_then(Value::as_str).unwrap_or_default();
        let payload = v.get("payload").unwrap_or(&Value::Null);

        if destination == "OHLCMarketData.subscribe" {
            // The ack says, per epic and resolution, whether the subscription took. A
            // refusal belongs to that pane alone, not to the socket.
            let mut out = Vec::new();
            for (name, state) in payload
                .get("subscriptions")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                if state.as_str() == Some("PROCESSED") {
                    continue;
                }
                // "EPIC:RESOLUTION:type"
                let mut parts = name.split(':');
                let (Some(epic), Some(res)) = (parts.next(), parts.next()) else { continue };
                out.push(FeedUpdate::fault(
                    key(epic, res),
                    &LiveError::err(
                        LiveFault::Symbol,
                        format!(
                            "Capital.com refused {epic} at {res}: {}",
                            state.as_str().unwrap_or("not processed")
                        ),
                    ),
                ));
            }
            return Ok(out);
        }
        if v.get("status").and_then(Value::as_str).is_some_and(|s| s != "OK")
            && destination != "ohlc.event"
        {
            let msg = v
                .get("payload")
                .and_then(|p| p.get("errorCode"))
                .and_then(Value::as_str)
                .unwrap_or("refused");
            return Err(LiveError::err(
                // A rejected token is terminal: retrying it burns the sign-in allowance.
                if msg.contains("token") || msg.contains("unauthor") {
                    LiveFault::Auth
                } else {
                    LiveFault::Transport
                },
                format!("Capital.com: {msg}"),
            ));
        }
        if destination != "ohlc.event" {
            return Ok(Vec::new());
        }

        let epic = payload.get("epic").and_then(Value::as_str).unwrap_or_default();
        let res = payload.get("resolution").and_then(Value::as_str).unwrap_or_default();
        if epic.is_empty() || res.is_empty() {
            return Ok(Vec::new());
        }
        let k = key(epic, res);
        let open_ms = payload
            .get("t")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("capital.com ohlc t"))?;
        let ohlc = |name: &str| -> Result<f64> {
            payload
                .get(name)
                .and_then(Value::as_f64)
                .ok_or_else(|| anyhow!("capital.com ohlc field {name}"))
        };
        let side = (ohlc("o")?, ohlc("h")?, ohlc("l")?, ohlc("c")?);
        let entry = self.sides.entry((k.clone(), open_ms)).or_default();
        match payload.get("priceType").and_then(Value::as_str) {
            Some("ask") | Some("offer") => entry.ask = Some(side),
            // Anything else is the bid: it is the default the platform streams.
            _ => entry.bid = Some(side),
        }
        // The chart shows the mid, so a bar waits for both sides rather than being drawn
        // half a spread away from the history it continues.
        let (Some(bid), Some(ask)) = (entry.bid, entry.ask) else {
            return Ok(Vec::new());
        };
        // A new bar's arrival is what closes the previous one; the sides of the old bar are
        // dropped with it.
        let ev = LiveEvent {
            bar_ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
            open: (bid.0 + ask.0) / 2.0,
            high: (bid.1 + ask.1) / 2.0,
            low: (bid.2 + ask.2) / 2.0,
            close: (bid.3 + ask.3) / 2.0,
            // Capital.com publishes no size on a streamed candle.
            volume: 0.0,
            closed: false,
            provider_ts: OffsetDateTime::now_utc(),
        };
        let mut out = Vec::new();
        match self.cur.get(&k) {
            Some(cur) if ev.bar_ts < cur.bar_ts => return Ok(Vec::new()),
            Some(cur) if ev.bar_ts > cur.bar_ts => {
                let mut closed = cur.clone();
                closed.closed = true;
                // The bar that just ended has no more sides to collect, and neither has
                // anything older: dropping them keeps the map the size of the open panes.
                self.sides
                    .retain(|(channel, at), _| channel != &k || *at >= open_ms);
                out.push(FeedUpdate::bar(k.clone(), closed));
            }
            _ => {}
        }
        out.push(FeedUpdate::bar(k.clone(), ev.clone()));
        self.cur.insert(k, ev);
        Ok(out)
    }
}

/// Canonical timeframe → Capital.com resolution, as a live fault when it has none.
fn token(tf: &str) -> Result<&'static str> {
    resolution(tf).map_err(|_| {
        LiveError::err(
            LiveFault::Unsupported,
            format!("Capital.com has no live candle resolution for {tf}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> CapitalFeed {
        CapitalFeed {
            cst: "c".into(),
            security: "s".into(),
            next_id: 1,
            sides: HashMap::new(),
            cur: HashMap::new(),
            last_sent: Instant::now(),
        }
    }

    fn event(epic: &str, res: &str, price_type: &str, t: i64, c: f64) -> String {
        format!(
            r#"{{"status":"OK","destination":"ohlc.event","payload":{{"resolution":"{res}",
               "epic":"{epic}","type":"classic","priceType":"{price_type}","t":{t},
               "h":{c},"l":{c},"o":{c},"c":{c}}}}}"#
        )
    }

    #[test]
    fn a_bar_waits_for_both_sides_and_then_shows_their_mid() {
        let mut f = feed();
        // The bid alone is half a spread away from the stored history, so nothing is drawn.
        assert!(f.on_message(&event("AAPL", "MINUTE_5", "bid", 1_671_714_000_000, 134.0)).unwrap().is_empty());
        let out = f
            .on_message(&event("AAPL", "MINUTE_5", "ask", 1_671_714_000_000, 135.0))
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, "AAPL|MINUTE_5");
        match &out[0].payload {
            super::super::Payload::Bar(ev) => {
                assert_eq!(ev.close, 134.5);
                assert!(!ev.closed);
            }
            _ => panic!("expected a bar"),
        }
    }

    #[test]
    fn a_newer_bar_closes_the_one_before_it() {
        let mut f = feed();
        f.on_message(&event("AAPL", "MINUTE_5", "bid", 1_671_714_000_000, 134.0)).unwrap();
        f.on_message(&event("AAPL", "MINUTE_5", "ask", 1_671_714_000_000, 135.0)).unwrap();
        f.on_message(&event("AAPL", "MINUTE_5", "bid", 1_671_714_300_000, 136.0)).unwrap();
        let out = f
            .on_message(&event("AAPL", "MINUTE_5", "ask", 1_671_714_300_000, 137.0))
            .unwrap();
        assert_eq!(out.len(), 2);
        match (&out[0].payload, &out[1].payload) {
            (super::super::Payload::Bar(done), super::super::Payload::Bar(next)) => {
                assert!(done.closed && done.close == 134.5);
                assert!(!next.closed && next.close == 136.5);
            }
            _ => panic!("expected two bars"),
        }
        // The closed bar's sides are dropped rather than accumulating for the session.
        assert_eq!(f.sides.len(), 1);
    }

    #[test]
    fn a_refused_epic_faults_its_own_pane_and_not_the_socket() {
        let mut f = feed();
        let out = f
            .on_message(
                r#"{"status":"OK","destination":"OHLCMarketData.subscribe","correlationId":"3",
                    "payload":{"subscriptions":{"OIL_CRUDE:MINUTE_5:classic":"PROCESSED",
                                                "NOPE:MINUTE_5:classic":"ERROR"}}}"#,
            )
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, "NOPE|MINUTE_5");
        match &out[0].payload {
            super::super::Payload::Fault { fault, .. } => assert_eq!(*fault, LiveFault::Symbol),
            _ => panic!("expected a fault"),
        }
    }

    #[test]
    fn one_frame_per_resolution_carries_every_epic_on_it() {
        let mut f = feed();
        let frames = f.subscribe(&[
            Instrument::new("gold", "fx", "5m"),
            Instrument::new("US500", "index", "5m"),
            Instrument::new("GOLD", "fx", "1h"),
        ]);
        assert_eq!(frames.len(), 2);
        let five = frames.iter().find(|s| s.contains("MINUTE_5")).unwrap();
        assert!(five.contains("GOLD") && five.contains("US500"));
        // The credentials ride on the frame: this socket has no handshake.
        assert!(five.contains("\"cst\":\"c\"") && five.contains("\"securityToken\":\"s\""));
    }
}
