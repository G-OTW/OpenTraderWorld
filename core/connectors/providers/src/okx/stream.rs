//! OKX live candles on the *business* socket (`/ws/v5/business`): candles live there, not on
//! the public one, which is the most common reason a correct subscription answers nothing.
//! The best bid and ask (`bbo-tbt`) is the other way round: public socket only (the business
//! one refuses the channel, measured), so a quote is a socket class of its own
//! (`Okx::socket_class`).
//!
//! Every data frame names its `channel` and `instId`, and the row's last field (`confirm`)
//! says whether the bar is final. A subscription replays recent candles, so a row older than
//! the newest already seen is not news. A refusal does not name its `arg`: the channel and
//! instrument are written into the message text and read back out of it. OKX drops a socket
//! it has heard nothing on for 30 seconds, so the bare word `ping` goes out after 20 quiet
//! ones.

use std::collections::HashMap;

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Qty, Quote, Timeframe, Timestamp};
use otw_connector_sdk::parse::{ts_millis, Dec, Int};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{bar, CandleRow, Okx, WS};

const CONN: &str = "business";
const PING_EVERY_NS: i64 = 20_000_000_000;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the best bid and ask.
    tf: Option<Timeframe>,
}

/// Best bid and ask, pushed on every change.
const BBO: &str = "bbo-tbt";

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    by_key: HashMap<String, Sub>,
    /// Newest bar open seen per key, so a replayed row is not re-emitted as news.
    seen: HashMap<String, i64>,
    last_sent: Option<Timestamp>,
}

fn channel(tf: Timeframe) -> Option<String> {
    bar(tf).map(|b| format!("candle{b}"))
}

/// The routing key: `candle1m|BTC-USDT`.
fn key(channel: &str, inst_id: &str) -> String {
    format!("{channel}|{}", inst_id.to_ascii_uppercase())
}

fn send(st: &mut LiveState, payload: String, now: Timestamp) -> NativeRequest {
    st.last_sent = Some(now);
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

pub fn encode(m: &Okx, cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut add = Vec::new();
    let mut drop = Vec::new();
    let mut quotes = false;
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let ch = channel(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("OKX has no live candle channel for {tf}")))?;
                let k = key(&ch, &instrument.native);
                st.by_sub.insert(*sub_id, k.clone());
                st.by_key.insert(k, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf) });
                add.push(serde_json::json!({ "channel": ch, "instId": instrument.native.to_ascii_uppercase() }));
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let k = key(BBO, &instrument.native);
                st.by_sub.insert(*sub_id, k.clone());
                st.by_key.insert(k, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None });
                add.push(serde_json::json!({ "channel": BBO, "instId": instrument.native.to_ascii_uppercase() }));
                quotes = true;
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("OKX streams candles and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(k) = st.by_sub.remove(sub_id) {
                    st.seen.remove(&k);
                    if let Some(s) = st.by_key.remove(&k) {
                        if let Some(ch) = s.tf.map_or(Some(BBO.to_string()), channel) {
                            drop.push(serde_json::json!({ "channel": ch, "instId": s.instrument.native.to_ascii_uppercase() }));
                        }
                    }
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened && !add.is_empty() {
        // The socket class keeps the two apart, so the first subscription says which this is.
        let path = if quotes { "/ws/v5/public" } else { "/ws/v5/business" };
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: path.into() });
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
    // OKX's keepalive is the bare word, not a JSON frame.
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
    /// Candle rows (arrays), or best bid/ask rows (objects): read once the channel says which.
    #[serde(default)]
    data: Option<Box<RawValue>>,
}

/// `{"asks":[["82245.5","0.34","0","8"]],"bids":[["82245.4","0.95","0","17"]],"ts":"..","seqId":..}`.
#[derive(Deserialize)]
struct BboRow {
    #[serde(default)]
    asks: Vec<Vec<Dec>>,
    #[serde(default)]
    bids: Vec<Vec<Dec>>,
    #[serde(default)]
    ts: Option<Int>,
}

/// The routing key inside a refusal's sentence: "...channel:candle1m,instId:FOO-BAR doesn't
/// exist...". `None` when the sentence names neither.
fn refused_key(msg: &str) -> Option<String> {
    let after = |tag: &str| -> Option<String> {
        let rest = msg.split_once(tag)?.1;
        let end = rest.find(|c: char| c == ',' || c.is_whitespace() || c == '.').unwrap_or(rest.len());
        Some(rest[..end].to_string()).filter(|s| !s.is_empty())
    };
    Some(key(&after("channel:")?, &after("instId:")?))
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    if payload.trim_ascii() == b"pong" {
        return Ok(vec![]);
    }
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("okx socket: {e}")))?;
    let envelope = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    match f.event.as_deref() {
        Some("error") => {
            let msg = f.msg.unwrap_or_else(|| "subscription refused".into());
            return match refused_key(&msg).and_then(|k| st.by_key.get(&k).map(|s| s.sub_id)) {
                Some(sub) => Ok(vec![envelope(
                    sub,
                    Event::SubscriptionFail { error: ConnectorError::new(ErrorKind::NotFound, format!("OKX refused this instrument: {msg}")) },
                )]),
                // A refusal naming nothing is the socket's, not one pane's.
                None => Err(ConnectorError::new(ErrorKind::Transient, format!("OKX: {msg}"))),
            };
        }
        Some("subscribe") => {
            let sub = f.arg.and_then(|a| st.by_key.get(&key(&a.channel, &a.inst_id)).map(|s| s.sub_id));
            return Ok(sub.map(|s| vec![envelope(s, Event::SubscriptionAck)]).unwrap_or_default());
        }
        _ => {}
    }
    let (Some(arg), Some(data)) = (f.arg, f.data) else { return Ok(vec![]) };
    if arg.channel.is_empty() || arg.inst_id.is_empty() {
        return Ok(vec![]);
    }
    let k = key(&arg.channel, &arg.inst_id);
    let Some(sub) = st.by_key.get(&k) else { return Ok(vec![]) };
    let Some(tf) = sub.tf else {
        let rows: Vec<BboRow> = serde_json::from_str(data.get()).map_err(|e| ConnectorError::decode(format!("okx bbo: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            // `[price, size, 0, orders]`; an empty side is no quote.
            let (Some(b), Some(a)) = (r.bids.first(), r.asks.first()) else { continue };
            let (Some(bid), Some(ask)) = (b.first(), a.first()) else { continue };
            let q = Quote { instrument: sub.instrument.clone(), bid: bid.price(), ask: ask.price(), bid_qty: b.get(1).map(|d| Qty(d.0)), ask_qty: a.get(1).map(|d| Qty(d.0)) };
            let e = envelope(sub.sub_id, Event::Quote(q));
            out.push(match r.ts.map(|t| ts_millis(t.0)).transpose()? {
                Some(ts) => e.at(ts),
                None => e,
            });
        }
        return Ok(out);
    };
    let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(data.get()).map_err(|e| ConnectorError::decode(format!("okx socket: {e}")))?;
    let mut out = Vec::new();
    for raw in &rows {
        let r = CandleRow::read(raw).ok_or_else(|| ConnectorError::decode("okx candle row"))?;
        if st.seen.get(&k).is_some_and(|prev| r.open_ms < *prev) {
            continue;
        }
        st.seen.insert(k.clone(), r.open_ms);
        // `confirm` is "1" once the bar is final: nothing is inferred from a rollover.
        let candle = r.candle(sub.instrument.clone(), tf, r.confirm.as_deref() == Some("1"))?;
        out.push(envelope(sub.sub_id, Event::Candle(candle)));
    }
    Ok(out)
}
