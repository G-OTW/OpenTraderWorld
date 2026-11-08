//! Bitget live candles on `/v2/ws/public`, `candle<interval>` per instrument, and the best
//! bid and ask on the same socket, `books1` (a one-level snapshot on every change).
//!
//! Every frame names its `instType`, `channel` and `instId`. There is **no closed flag**: a
//! row is final once one with a newer open time arrives, so closure is read on rollover, per
//! channel (the previous candle is re-emitted as final, then the new one). A refusal names
//! the `arg` that caused it: that pane fails, the socket carries on. The venue drops a socket
//! that has sent no `ping` for two minutes, so the bare word goes out after 20 quiet seconds.

use std::collections::HashMap;

use otw_connector_model::{Candle, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Qty, Quote, Timeframe, Timestamp};
use otw_connector_sdk::parse::{Dec, Int};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{Bitget, CandleRow, WS};

const CONN: &str = "public";
const PING_EVERY_NS: i64 = 20_000_000_000;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the best bid and ask.
    tf: Option<Timeframe>,
}

/// The one-level book.
const BOOK: &str = "books1";

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    by_key: HashMap<String, Sub>,
    /// The forming candle per key and the venue time it carried, for the rollover close.
    cur: HashMap<String, (Candle, Option<Timestamp>)>,
    last_sent: Option<Timestamp>,
}

/// UTC-anchored daily and weekly channels, like the download.
fn channel(tf: Timeframe) -> Option<&'static str> {
    use otw_connector_model::TfUnit::*;
    Some(match (tf.n, tf.unit) {
        (1, Minute) => "candle1m",
        (5, Minute) => "candle5m",
        (15, Minute) => "candle15m",
        (30, Minute) => "candle30m",
        (1, Hour) => "candle1H",
        (4, Hour) => "candle4H",
        (1, Day) => "candle1Dutc",
        (1, Week) => "candle1Wutc",
        _ => return None,
    })
}

fn key(channel: &str, inst_id: &str) -> String {
    format!("{channel}|{}", inst_id.to_ascii_uppercase())
}

fn send(st: &mut LiveState, payload: String, now: Timestamp) -> NativeRequest {
    st.last_sent = Some(now);
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

pub fn encode(m: &Bitget, cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let inst_type = m.book.inst_type();
    let mut add = Vec::new();
    let mut drop = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let ch = channel(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("Bitget has no live candle channel for {tf}")))?;
                let k = key(ch, &instrument.native);
                st.by_sub.insert(*sub_id, k.clone());
                st.by_key.insert(k, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf) });
                add.push(serde_json::json!({ "instType": inst_type, "channel": ch, "instId": instrument.native.to_ascii_uppercase() }));
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let k = key(BOOK, &instrument.native);
                st.by_sub.insert(*sub_id, k.clone());
                st.by_key.insert(k, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None });
                add.push(serde_json::json!({ "instType": inst_type, "channel": BOOK, "instId": instrument.native.to_ascii_uppercase() }));
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Bitget streams candles and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(k) = st.by_sub.remove(sub_id) {
                    st.cur.remove(&k);
                    if let Some(s) = st.by_key.remove(&k) {
                        if let Some(ch) = s.tf.map_or(Some(BOOK), channel) {
                            drop.push(serde_json::json!({ "instType": inst_type, "channel": ch, "instId": s.instrument.native.to_ascii_uppercase() }));
                        }
                    }
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened && !add.is_empty() {
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: "/v2/ws/public".into() });
        st.opened = true;
    }
    if !add.is_empty() {
        out.push(send(st, serde_json::json!({ "op": "subscribe", "args": add }).to_string(), ctx.received_ts));
    }
    if !drop.is_empty() {
        out.push(send(st, serde_json::json!({ "op": "unsubscribe", "args": drop }).to_string(), ctx.received_ts));
    }
    Ok(out)
}

pub fn keepalive(st: &mut LiveState, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
    if !st.opened {
        return Vec::new();
    }
    let Some(last) = st.last_sent else {
        st.last_sent = Some(ctx.received_ts);
        return Vec::new();
    };
    if ctx.received_ts.nanos() - last.nanos() < PING_EVERY_NS {
        return Vec::new();
    }
    vec![send(st, "ping".into(), ctx.received_ts)]
}

#[derive(Deserialize)]
struct Arg {
    #[serde(default)]
    channel: String,
    #[serde(rename = "instId", default)]
    inst_id: String,
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    arg: Option<Arg>,
    /// Candle rows (arrays), or book rows (objects): read once the channel says which.
    #[serde(default)]
    data: Option<Box<RawValue>>,
    #[serde(default)]
    ts: Option<Int>,
}

/// `{"asks":[["82165.42","0.0072"]],"bids":[["82165.41","0.41"]],"ts":"..","seq":..}`.
#[derive(Deserialize)]
struct BookRow {
    #[serde(default)]
    asks: Vec<(Dec, Dec)>,
    #[serde(default)]
    bids: Vec<(Dec, Dec)>,
    #[serde(default)]
    ts: Option<Int>,
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    if payload.trim_ascii() == b"pong" {
        return Ok(vec![]);
    }
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("bitget socket: {e}")))?;
    let arg = f.arg.unwrap_or(Arg { channel: String::new(), inst_id: String::new() });
    let at = |sub: CorrId, event: Event, ts: Option<Timestamp>| {
        let e = Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
        match ts {
            Some(t) => e.at(t),
            None => e,
        }
    };
    match f.event.as_deref() {
        Some("error") => {
            let msg = f.msg.unwrap_or_else(|| "subscription refused".into());
            if arg.channel.is_empty() || arg.inst_id.is_empty() {
                return Err(ConnectorError::new(ErrorKind::Transient, format!("Bitget: {msg}")));
            }
            let k = key(&arg.channel, &arg.inst_id);
            return Ok(st
                .by_key
                .get(&k)
                .map(|s| {
                    vec![at(
                        s.sub_id,
                        Event::SubscriptionFail {
                            error: ConnectorError::new(ErrorKind::NotFound, format!("Bitget does not list {} on this market: {msg}", arg.inst_id)),
                        },
                        None,
                    )]
                })
                .unwrap_or_default());
        }
        Some("subscribe") => {
            let k = key(&arg.channel, &arg.inst_id);
            return Ok(st.by_key.get(&k).map(|s| vec![at(s.sub_id, Event::SubscriptionAck, None)]).unwrap_or_default());
        }
        _ => {}
    }
    let Some(data) = f.data else { return Ok(vec![]) };
    if arg.channel.is_empty() || arg.inst_id.is_empty() {
        return Ok(vec![]);
    }
    let k = key(&arg.channel, &arg.inst_id);
    let Some(sub) = st.by_key.get(&k) else { return Ok(vec![]) };
    let event_ts = f.ts.map(|t| otw_connector_sdk::parse::ts_millis(t.0)).transpose()?;
    let Some(tf) = sub.tf else {
        let rows: Vec<BookRow> = serde_json::from_str(data.get()).map_err(|e| ConnectorError::decode(format!("bitget book: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            // An empty side is no quote.
            let (Some((bid, bq)), Some((ask, aq))) = (r.bids.first(), r.asks.first()) else { continue };
            let q = Quote { instrument: sub.instrument.clone(), bid: bid.price(), ask: ask.price(), bid_qty: Some(Qty(bq.0)), ask_qty: Some(Qty(aq.0)) };
            let ts = r.ts.map(|t| otw_connector_sdk::parse::ts_millis(t.0)).transpose()?.or(event_ts);
            out.push(at(sub.sub_id, Event::Quote(q), ts));
        }
        return Ok(out);
    };
    let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(data.get()).map_err(|e| ConnectorError::decode(format!("bitget socket: {e}")))?;
    let mut out = Vec::new();
    for raw in &rows {
        let r = CandleRow::read(raw).ok_or_else(|| ConnectorError::decode("bitget candle row"))?;
        let c = r.candle(sub.instrument.clone(), tf, false)?;
        match st.cur.get(&k) {
            Some((cur, _)) if c.open_ts < cur.open_ts => continue, // a snapshot replaying history
            Some((cur, cur_ts)) if c.open_ts > cur.open_ts => {
                // A newer candle started, so the previous one is final, as it last read.
                let mut closed = cur.clone();
                closed.is_final = true;
                out.push(at(sub.sub_id, Event::Candle(closed), *cur_ts));
            }
            _ => {}
        }
        out.push(at(sub.sub_id, Event::Candle(c.clone()), event_ts));
        st.cur.insert(k.clone(), (c, event_ts));
    }
    Ok(out)
}
