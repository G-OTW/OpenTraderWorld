//! Binance live klines and best bid/ask over one socket.
//!
//! The raw stream endpoint wants a stream in its path (`/ws/btcusdt@kline_1m`), so the first
//! subscription opens the socket and every subscription, the first included, is sent as a
//! `SUBSCRIBE` frame. Each message names its own symbol and interval (or is a book ticker):
//! that is how a message is routed back to its subscription.
//!
//! USDⓈ-M serves its book ticker on the `public` route only: the `market` route acks the
//! subscription and never sends a frame (measured). So the quote rides its own socket class
//! there (`Binance::socket_class`), opened on that route.

use std::collections::HashMap;

use otw_connector_model::{Candle, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Qty, Quote, Timeframe};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;

use super::market::Kind;
use super::{interval, Binance, WS, WS_PUBLIC};

const CONN: &str = "main";

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the book ticker.
    tf: Option<Timeframe>,
}

/// Socket state: one per open socket.
#[derive(Default)]
pub struct LiveState {
    opened: bool,
    next_id: u64,
    by_sub: HashMap<CorrId, String>,
    by_stream: HashMap<String, Sub>,
    /// Frame id to the subscriptions it carried, for the ack or the refusal.
    frames: HashMap<u64, Vec<CorrId>>,
}

/// `btcusdt@kline_1m`: what a frame names and what a message identifies itself with.
fn stream_name(native: &str, token: &str) -> String {
    format!("{}@kline_{token}", native.to_ascii_lowercase())
}

/// `btcusdt@bookTicker`: the best bid and ask, pushed on every change.
fn ticker_name(native: &str) -> String {
    format!("{}@bookTicker", native.to_ascii_lowercase())
}

fn frame(st: &mut LiveState, method: &str, params: &[String], subs: Vec<CorrId>) -> NativeRequest {
    // Ids start at 2, as the legacy socket did, so a recorded exchange replays unchanged.
    st.next_id = st.next_id.max(1) + 1;
    st.frames.insert(st.next_id, subs);
    let payload = serde_json::json!({ "method": method, "params": params, "id": st.next_id }).to_string();
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

pub fn encode(m: &Binance, cmds: &[Command], st: &mut LiveState) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut add: Vec<String> = Vec::new();
    let mut add_subs = Vec::new();
    let mut drop: Vec<String> = Vec::new();
    let mut quotes = false;
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let token = interval(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("{} streams no {tf} candles here", m.capabilities().label)))?;
                let name = stream_name(&instrument.native, token);
                st.by_sub.insert(*sub_id, name.clone());
                st.by_stream.insert(name.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf) });
                add.push(name);
                add_subs.push(*sub_id);
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let name = ticker_name(&instrument.native);
                st.by_sub.insert(*sub_id, name.clone());
                st.by_stream.insert(name.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None });
                add.push(name);
                add_subs.push(*sub_id);
                quotes = true;
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("{} streams candles and quotes only, not {data_type:?}", m.capabilities().label)));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(name) = st.by_sub.remove(sub_id) {
                    st.by_stream.remove(&name);
                    drop.push(name);
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened {
        if let Some(first) = add.first() {
            // The socket class keeps quotes and klines apart on USDⓈ-M, so the first stream
            // says which route this socket is.
            let base = if quotes && m.market().kind == Kind::UsdM { WS_PUBLIC } else { WS };
            out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base, path: format!("/ws/{first}") });
            st.opened = true;
        }
    }
    if !add.is_empty() {
        out.push(frame(st, "SUBSCRIBE", &add, add_subs));
    }
    if !drop.is_empty() {
        out.push(frame(st, "UNSUBSCRIBE", &drop, Vec::new()));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct KlineMsg {
    #[serde(rename = "E")]
    event_ms: Option<Int>,
    s: String,
    k: Kline,
}

#[derive(Deserialize)]
struct Kline {
    t: Int,
    i: String,
    o: Dec,
    h: Dec,
    l: Dec,
    c: Dec,
    v: Dec,
    #[serde(default)]
    x: bool,
    #[serde(default)]
    q: OptDec,
    #[serde(default)]
    n: Option<Int>,
}

/// Spot: `{"u":..,"s":"BTCUSDT","b":..,"B":..,"a":..,"A":..}`, no event time. USDⓈ-M adds
/// `"e":"bookTicker"` and the event time `E`.
#[derive(Deserialize)]
struct BookTickerMsg {
    #[serde(rename = "E")]
    event_ms: Option<Int>,
    s: String,
    b: Dec,
    #[serde(rename = "B")]
    bid_qty: OptDec,
    a: Dec,
    #[serde(rename = "A")]
    ask_qty: OptDec,
}

#[derive(Deserialize)]
struct Reply {
    id: Option<u64>,
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    error: Option<ReplyError>,
}

#[derive(Deserialize)]
struct ReplyError {
    code: i64,
    #[serde(default)]
    msg: String,
}

pub fn decode(name: &str, payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let v: serde_json::Value = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("{name} socket: {e}")))?;
    if v.get("k").is_some() {
        let msg: KlineMsg = serde_json::from_value(v).map_err(|e| ConnectorError::decode(format!("{name} kline: {e}")))?;
        let name = format!("{}@kline_{}", msg.s.to_ascii_lowercase(), msg.k.i);
        // A message for a stream nobody holds any more: the unsubscribe crossed it in flight.
        let Some(sub) = st.by_stream.get(&name) else { return Ok(vec![]) };
        let open_ts = ts_millis(msg.k.t.0)?;
        let event_ts = ts_millis(msg.event_ms.map(|e| e.0).unwrap_or(msg.k.t.0))?;
        let Some(tf) = sub.tf else { return Ok(vec![]) };
        let candle = Candle {
            instrument: sub.instrument.clone(),
            tf,
            open_ts,
            open: msg.k.o.price(),
            high: msg.k.h.price(),
            low: msg.k.l.price(),
            close: msg.k.c.price(),
            volume: msg.k.v.qty(),
            quote_volume: msg.k.q.0.map(otw_connector_model::Money),
            trades: msg.k.n.and_then(|n| u64::try_from(n.0).ok()),
            is_final: msg.k.x,
            adjusted: None,
            sides: None,
        };
        return Ok(vec![Envelope::new(ctx.connector.clone(), Some(sub.sub_id), ctx.received_ts, Event::Candle(candle)).at(event_ts)]);
    }
    if v.get("b").is_some() && v.get("a").is_some() && v.get("s").is_some() {
        let msg: BookTickerMsg = serde_json::from_value(v).map_err(|e| ConnectorError::decode(format!("{name} bookTicker: {e}")))?;
        let Some(sub) = st.by_stream.get(&ticker_name(&msg.s)) else { return Ok(vec![]) };
        // An empty side (a halted or delisted book answers zeros) is no quote: nothing rather
        // than half a price.
        let positive = |d: Dec| d.0.is_sign_positive() && !d.0.is_zero();
        if !positive(msg.b) || !positive(msg.a) {
            return Ok(vec![]);
        }
        let quote = Quote { instrument: sub.instrument.clone(), bid: msg.b.price(), ask: msg.a.price(), bid_qty: msg.bid_qty.0.map(Qty), ask_qty: msg.ask_qty.0.map(Qty) };
        let e = Envelope::new(ctx.connector.clone(), Some(sub.sub_id), ctx.received_ts, Event::Quote(quote));
        return Ok(vec![match msg.event_ms {
            Some(ms) => e.at(ts_millis(ms.0)?),
            None => e,
        }]);
    }
    // Control replies: `{"result":null,"id":2}` acks a frame, `{"code":2,"msg":..,"id":2}`
    // (or the newer `{"error":{..},"id":2}`) refuses it.
    let Ok(reply) = serde_json::from_value::<Reply>(v) else { return Ok(vec![]) };
    let Some(id) = reply.id else { return Ok(vec![]) };
    let subs = st.frames.remove(&id).unwrap_or_default();
    let refusal = reply.error.map(|e| (e.code, e.msg)).or(reply.code.map(|c| (c, reply.msg.unwrap_or_default())));
    let mut out = Vec::new();
    for sub in subs {
        let event = match &refusal {
            None => Event::SubscriptionAck,
            Some((code, msg)) => Event::SubscriptionFail {
                error: ConnectorError::new(ErrorKind::InvalidRequest, format!("{name} socket refused: {msg}")).with_code(code.to_string()),
            },
        };
        out.push(Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event));
    }
    Ok(out)
}
