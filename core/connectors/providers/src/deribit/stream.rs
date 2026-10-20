//! Deribit live candles on `wss://www.deribit.com/ws/api/v2`, JSON-RPC.
//!
//!   - `public/subscribe` with `{"channels":["chart.trades.<instrument>.<resolution>"]}`, the
//!     panes joining together in one frame. The answer echoes the frame's id and lists the
//!     channels it took: a pane whose channel is missing was refused (an unknown instrument,
//!     a resolution the venue does not stream, a spot pair matched on Coinbase Exchange: all
//!     measured, all answered by an empty list rather than an error).
//!   - Updates are `{"method":"subscription","params":{"channel","data":{"tick","open",
//!     "high","low","close","volume","cost"}}}`: `tick` is the bar's open in ms, `volume` the
//!     base coin, `cost` the quote. No frame says a bar is closed: every candle goes out as
//!     forming and the engine closes it when the next starts.
//!   - The best bid and ask: channel `quote.<instrument>`, joined and answered like a candle
//!     channel, data `{timestamp, best_bid_price, best_ask_price, best_bid_amount,
//!     best_ask_amount}` on every change (an inverse contract's amounts are USD: not carried).
//!   - Keepalive: `public/set_heartbeat` (30 s) goes out with the first subscription; the
//!     venue then sends `{"method":"heartbeat","params":{"type":"test_request"}}`, which
//!     must be answered with `public/test` or the socket is closed.
//!
//! A frame is 32 kB at most (the venue drops the socket past it, measured with 1000
//! channels): a subscription is split in frames of 16 kB of channel names. One socket took
//! 4000 channels without a refusal (measured), so no cap is declared.

use std::collections::{HashMap, HashSet};

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Price, Qty, Quote, Timeframe};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;

use super::{resolution, Bar, Deribit, WS};

const CONN: &str = "public";
/// Seconds between the venue's heartbeats (10 at least).
const HEARTBEAT_S: u32 = 30;
/// Channel-name bytes per subscribe frame, half the venue's frame limit.
const FRAME_BYTES: usize = 16_000;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the best bid and ask.
    tf: Option<Timeframe>,
}

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    /// The last JSON-RPC id handed out.
    rpc: u64,
    by_sub: HashMap<CorrId, String>,
    by_channel: HashMap<String, Sub>,
    /// Subscribe frames not answered yet: their id and the panes they carry.
    asks: HashMap<u64, Vec<CorrId>>,
    /// Ids of the frames whose answer carries nothing to route (heartbeat, test,
    /// unsubscribe).
    quiet: HashSet<u64>,
    /// The venue asked for a `public/test`.
    owe_test: bool,
}

fn channel(native: &str, token: &str) -> String {
    format!("chart.trades.{native}.{token}")
}

fn send(payload: String) -> NativeRequest {
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

impl LiveState {
    fn id(&mut self) -> u64 {
        self.rpc += 1;
        self.rpc
    }

    fn call(&mut self, method: &str, params: serde_json::Value, quiet: bool) -> (u64, String) {
        let id = self.id();
        if quiet {
            self.quiet.insert(id);
        }
        (id, serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string())
    }
}

pub fn encode(m: &Deribit, cmds: &[Command], st: &mut LiveState, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    // Every pane is checked before anything is written or remembered.
    let mut panes = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let token = resolution(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("Deribit has no live candle topic for {tf}")))?;
                panes.push((*sub_id, instrument, Some(*tf), channel(&instrument.native, token)));
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                panes.push((*sub_id, instrument, None, format!("quote.{}", instrument.native)));
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Deribit streams candles and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { .. } => {}
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened && !panes.is_empty() {
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: "/ws/api/v2".to_string() });
        let (_, hb) = st.call("public/set_heartbeat", serde_json::json!({ "interval": HEARTBEAT_S }), true);
        out.push(send(hb));
        st.opened = true;
    }
    for c in cmds {
        let Command::Unsubscribe { sub_id } = c else { continue };
        if let Some(ch) = st.by_sub.remove(sub_id) {
            st.by_channel.remove(&ch);
            for waiting in st.asks.values_mut() {
                waiting.retain(|s| s != sub_id);
            }
            let (_, f) = st.call("public/unsubscribe", serde_json::json!({ "channels": [ch] }), true);
            out.push(send(f));
        }
    }
    // The panes, grouped in frames under the size limit.
    let mut batches: Vec<Vec<(CorrId, String)>> = vec![Vec::new()];
    let mut bytes = 0usize;
    for (sub_id, instrument, tf, ch) in panes {
        if bytes + ch.len() > FRAME_BYTES {
            batches.push(Vec::new());
            bytes = 0;
        }
        bytes += ch.len() + 3;
        st.by_sub.insert(sub_id, ch.clone());
        st.by_channel.insert(ch.clone(), Sub { sub_id, instrument: instrument.clone(), tf });
        if let Some(b) = batches.last_mut() {
            b.push((sub_id, ch));
        }
    }
    for batch in batches.into_iter().filter(|b| !b.is_empty()) {
        let channels: Vec<&str> = batch.iter().map(|(_, ch)| ch.as_str()).collect();
        let (id, f) = st.call("public/subscribe", serde_json::json!({ "channels": channels }), false);
        st.asks.insert(id, batch.into_iter().map(|(s, _)| s).collect());
        out.push(send(f));
    }
    Ok(out)
}

/// The `public/test` the venue's heartbeat asked for, sent right after the frame that asked.
pub fn keepalive(st: &mut LiveState, _ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
    if !st.opened || !std::mem::take(&mut st.owe_test) {
        return Vec::new();
    }
    let (_, f) = st.call("public/test", serde_json::json!({}), true);
    vec![send(f)]
}

/// The socket is gone: the next subscription opens a new one.
pub fn closed(st: &mut LiveState) {
    st.opened = false;
    st.owe_test = false;
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Push {
    channel: String,
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct QuoteData {
    #[serde(default)]
    timestamp: Option<Int>,
    #[serde(default)]
    best_bid_price: OptDec,
    #[serde(default)]
    best_ask_price: OptDec,
    #[serde(default)]
    best_bid_amount: OptDec,
    #[serde(default)]
    best_ask_amount: OptDec,
}

#[derive(Deserialize)]
struct ChartBar {
    tick: Int,
    open: Dec,
    high: Dec,
    low: Dec,
    close: Dec,
    volume: Dec,
    cost: Dec,
}

fn refusal(e: &serde_json::Value) -> ConnectorError {
    let code = e.get("code").and_then(|c| c.as_i64()).unwrap_or_default();
    let param = e.get("data").and_then(|d| d.get("param")).and_then(|p| p.as_str());
    let msg = e.get("message").and_then(|m| m.as_str()).unwrap_or("refused");
    let reason = e.get("data").and_then(|d| d.get("reason")).and_then(|r| r.as_str()).map(|r| format!(" ({r})")).unwrap_or_default();
    ConnectorError::new(super::errors::kind_of(code, param), format!("Deribit socket: {msg}{reason} ({code})")).with_code(code.to_string())
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("deribit socket: {e}")))?;
    let mut out = Vec::new();
    match (f.method.as_deref(), f.id) {
        (Some("subscription"), _) => {
            let Some(params) = f.params else { return Ok(out) };
            let p: Push = serde_json::from_value(params).map_err(|e| ConnectorError::decode(format!("deribit candle: {e}")))?;
            let Some(sub) = st.by_channel.get(&p.channel) else { return Ok(out) };
            let Some(tf) = sub.tf else {
                let d: QuoteData = serde_json::from_value(p.data).map_err(|e| ConnectorError::decode(format!("deribit quote: {e}")))?;
                let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
                // An empty side is no quote.
                let (Some(bid), Some(ask)) = (positive(d.best_bid_price), positive(d.best_ask_price)) else { return Ok(out) };
                // An inverse contract's sizes are USD, not coins: they are not carried.
                let sized = !super::Name::of(&sub.instrument.native).inverse();
                let size = |d: OptDec| d.0.filter(|_| sized).map(Qty);
                let q = Quote { instrument: sub.instrument.clone(), bid: Price(bid), ask: Price(ask), bid_qty: size(d.best_bid_amount), ask_qty: size(d.best_ask_amount) };
                let e = at(sub.sub_id, Event::Quote(q));
                out.push(match d.timestamp {
                    Some(t) => e.at(ts_millis(t.0)?),
                    None => e,
                });
                return Ok(out);
            };
            let d: ChartBar = serde_json::from_value(p.data).map_err(|e| ConnectorError::decode(format!("deribit candle: {e}")))?;
            let bar = Bar { open_ms: d.tick.0, o: d.open, h: d.high, l: d.low, c: d.close, volume: d.volume, cost: d.cost };
            out.push(at(sub.sub_id, Event::Candle(bar.candle(sub.instrument.clone(), tf, false)?)));
        }
        (Some("heartbeat"), _) => {
            let kind = f.params.as_ref().and_then(|p| p.get("type")).and_then(|t| t.as_str());
            if kind == Some("test_request") {
                st.owe_test = true;
            }
        }
        (_, Some(id)) if st.quiet.remove(&id) => {
            if let Some(e) = f.error {
                tracing::warn!("deribit socket: {}", refusal(&e).message);
            }
        }
        (_, Some(id)) => {
            let Some(panes) = st.asks.remove(&id) else { return Ok(out) };
            match (f.result, f.error) {
                (_, Some(e)) => {
                    let err = refusal(&e);
                    for sub in panes {
                        if let Some(ch) = st.by_sub.remove(&sub) {
                            st.by_channel.remove(&ch);
                        }
                        out.push(at(sub, Event::SubscriptionFail { error: err.clone() }));
                    }
                }
                (Some(serde_json::Value::Array(taken)), None) => {
                    let taken: HashSet<&str> = taken.iter().filter_map(|c| c.as_str()).collect();
                    for sub in panes {
                        let Some(ch) = st.by_sub.get(&sub).cloned() else { continue };
                        if taken.contains(ch.as_str()) {
                            out.push(at(sub, Event::SubscriptionAck));
                            continue;
                        }
                        st.by_sub.remove(&sub);
                        st.by_channel.remove(&ch);
                        out.push(at(
                            sub,
                            Event::SubscriptionFail {
                                error: ConnectorError::not_found(format!(
                                    "Deribit refused `{ch}`: it lists no such instrument, or publishes no candles for it (a spot pair matched on Coinbase Exchange)"
                                )),
                            },
                        ));
                    }
                }
                _ => return Err(ConnectorError::decode("deribit socket: a subscription answer with no channel list")),
            }
        }
        // An error tied to no request (a frame the venue could not read).
        (_, None) if f.error.is_some() => {
            let e = f.error.as_ref().map(refusal).unwrap_or_else(|| ConnectorError::new(ErrorKind::Transient, "Deribit socket error"));
            return Err(ConnectorError { kind: ErrorKind::Transient, ..e });
        }
        _ => {}
    }
    Ok(out)
}
