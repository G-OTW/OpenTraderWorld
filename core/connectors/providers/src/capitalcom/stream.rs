//! Capital.com live candles on `wss://api-streaming-capital.backend-capital.com/connect`.
//!
//! - **Every frame carries the session.** No handshake: the two tokens go on each subscribe
//!   and ping, written by the signer (`WsSigned`), so the REST sign-in happens first.
//! - **A candle arrives twice, once per side.** An `ohlc.event` names its `priceType`; the
//!   candle is sent once both sides of its period have been seen, at their mid, with both
//!   sides attached, and again on every event after that with the freshest of each.
//! - **No closed flag and no volume.** The engine closes a candle when the next one starts;
//!   the volume stays zero rather than invented.
//!
//! One socket carries every pane, up to the forty instruments a session allows.

use std::collections::HashMap;

use otw_connector_model::{Candle, Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event, InstrumentId, Ohlc, Price, Qty, Sides, Timestamp};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::{ConnKey, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{env, resolution, timeframe_of, VENUE, WS};

const CONN: &str = "main";
/// Capital.com drops a socket it has heard nothing on for ten minutes.
const PING_SECS: i64 = 4 * 60;

/// Per channel (epic, resolution) and period open (ms), the sides seen so far: bid, ask.
type SidesSeen = HashMap<(String, String, i64), (Option<Ohlc>, Option<Ohlc>)>;

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    next_id: u64,
    last_sent: Option<Timestamp>,
    by_sub: HashMap<CorrId, (String, &'static str)>,
    by_key: HashMap<(String, String), CorrId>,
    pub(super) sides: SidesSeen,
}

impl LiveState {
    fn frame(&mut self, destination: &str, payload: Option<serde_json::Value>, now: Timestamp) -> NativeRequest {
        self.next_id += 1;
        let mut v = serde_json::json!({ "destination": destination, "correlationId": self.next_id.to_string() });
        if let (Some(p), Some(o)) = (payload, v.as_object_mut()) {
            o.insert("payload".into(), p);
        }
        self.last_sent = Some(now);
        NativeRequest::WsSigned { conn_key: ConnKey(CONN.into()), payload: v.to_string().into(), corr_id: None }
    }
}

/// One frame per resolution: the payload lists epics and resolutions, and every pairing of the
/// two is subscribed.
fn frames(st: &mut LiveState, destination: &str, by_res: Vec<(&'static str, Vec<String>)>, now: Timestamp) -> Vec<NativeRequest> {
    let mut out = Vec::new();
    for (res, mut epics) in by_res {
        epics.sort();
        epics.dedup();
        out.push(st.frame(destination, Some(serde_json::json!({ "epics": epics, "resolutions": [res], "type": "classic" })), now));
    }
    out
}

fn group(into: &mut Vec<(&'static str, Vec<String>)>, res: &'static str, epic: String) {
    match into.iter_mut().find(|(r, _)| *r == res) {
        Some((_, e)) => e.push(epic),
        None => into.push((res, vec![epic])),
    }
}

pub fn encode(cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut out = Vec::new();
    let mut add = Vec::new();
    let mut drop = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let res = resolution(*tf).ok_or_else(|| ConnectorError::unsupported(format!("Capital.com has no live candle resolution for {tf}")))?;
                if !st.opened {
                    out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: "/connect".into() });
                    st.opened = true;
                }
                let epic = instrument.native.clone();
                st.by_sub.insert(*sub_id, (epic.clone(), res));
                st.by_key.insert((epic.clone(), res.to_string()), *sub_id);
                group(&mut add, res, epic);
            }
            Command::Subscribe { data_type, .. } => return Err(ConnectorError::unsupported(format!("Capital.com streams candles only here, not {data_type:?}"))),
            Command::Unsubscribe { sub_id } => {
                if let Some((epic, res)) = st.by_sub.remove(sub_id) {
                    st.by_key.remove(&(epic.clone(), res.to_string()));
                    st.sides.retain(|(e, r, _), _| !(e == &epic && r == res));
                    group(&mut drop, res, epic);
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    out.extend(frames(st, "OHLCMarketData.subscribe", add, ctx.received_ts));
    out.extend(frames(st, "OHLCMarketData.unsubscribe", drop, ctx.received_ts));
    Ok(out)
}

/// A ping when the socket has been quiet for four minutes.
pub fn keepalive(st: &mut LiveState, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
    if !st.opened {
        return vec![];
    }
    match st.last_sent {
        Some(t) if ctx.received_ts.secs() - t.secs() < PING_SECS => vec![],
        _ => vec![st.frame("ping", None, ctx.received_ts)],
    }
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OhlcEvent {
    #[serde(default)]
    resolution: String,
    #[serde(default)]
    epic: String,
    #[serde(default)]
    price_type: Option<String>,
    t: i64,
    o: OptDec,
    h: OptDec,
    l: OptDec,
    c: OptDec,
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("capitalcom socket: {e}")))?;
    if f.destination == "OHLCMarketData.subscribe" {
        // The ack says, per epic and resolution, whether it took. A refusal belongs to that
        // pane alone, not to the socket.
        let mut out = Vec::new();
        for (name, state) in f.payload.get("subscriptions").and_then(|s| s.as_object()).into_iter().flatten() {
            if state.as_str() == Some("PROCESSED") {
                continue;
            }
            // "EPIC:RESOLUTION:type"
            let mut parts = name.split(':');
            let (Some(epic), Some(res)) = (parts.next(), parts.next()) else { continue };
            let Some(sub) = st.by_key.get(&(epic.to_ascii_uppercase(), res.to_string())) else { continue };
            let why = state.as_str().unwrap_or("not processed");
            out.push(env(ctx, *sub, Event::SubscriptionFail { error: ConnectorError::not_found(format!("Capital.com refused {epic} at {res}: {why}")) }));
        }
        return Ok(out);
    }
    if f.status.as_deref().is_some_and(|s| s != "OK") && f.destination != "ohlc.event" {
        let msg = f.payload.get("errorCode").and_then(|c| c.as_str()).unwrap_or("refused").to_string();
        // A rejected token is terminal: retrying it burns the sign-in allowance.
        let kind = if msg.contains("token") || msg.contains("unauthor") { ErrorKind::Auth } else { ErrorKind::Transient };
        return Err(ConnectorError::new(kind, format!("Capital.com: {msg}")));
    }
    if f.destination != "ohlc.event" {
        return Ok(vec![]);
    }
    let e: OhlcEvent = serde_json::from_value(f.payload).map_err(|e| ConnectorError::decode(format!("capitalcom ohlc: {e}")))?;
    let epic = e.epic.to_ascii_uppercase();
    let Some(sub) = st.by_key.get(&(epic.clone(), e.resolution.clone())).copied() else { return Ok(vec![]) };
    let tf = timeframe_of(&e.resolution).ok_or_else(|| ConnectorError::decode(format!("capitalcom resolution {}", e.resolution)))?;
    let p = |d: OptDec, name: &str| d.0.map(Price).ok_or_else(|| ConnectorError::decode(format!("capitalcom ohlc field {name}")));
    let side = Ohlc { open: p(e.o, "o")?, high: p(e.h, "h")?, low: p(e.l, "l")?, close: p(e.c, "c")? };
    let entry = st.sides.entry((epic.clone(), e.resolution.clone(), e.t)).or_default();
    match e.price_type.as_deref() {
        Some("ask") | Some("offer") => entry.1 = Some(side),
        // Anything else is the bid: it is the default the platform streams.
        _ => entry.0 = Some(side),
    }
    // The chart shows the mid: a candle waits for both sides rather than being drawn half a
    // spread away from the history it continues.
    let (Some(bid), Some(ask)) = *entry else { return Ok(vec![]) };
    // Nothing older on this channel will be completed now.
    st.sides.retain(|(ep, r, at), _| !(ep == &epic && r == &e.resolution && *at < e.t));
    let m = |b: Price, a: Price| Price(((b.0 + a.0) / Decimal::TWO).normalize());
    let open_ts = Timestamp::from_millis(e.t).ok_or_else(|| ConnectorError::decode("capitalcom ohlc t"))?;
    let candle = Candle {
        instrument: InstrumentId::new(VENUE, &epic),
        tf,
        open_ts,
        open: m(bid.open, ask.open),
        high: m(bid.high, ask.high),
        low: m(bid.low, ask.low),
        close: m(bid.close, ask.close),
        // No size on a streamed candle.
        volume: Qty(Decimal::ZERO),
        quote_volume: None,
        trades: None,
        is_final: false,
        adjusted: None,
        sides: Some(Sides { bid, ask }),
    };
    Ok(vec![env(ctx, sub, Event::Candle(candle))])
}
