//! Bybit live candles on `/v5/public/{spot|linear|inverse}` (Bybit EU: its own host, spot
//! only), topic `kline.<interval>.<symbol>`, and the best bid and ask, topic
//! `orderbook.1.<symbol>` (a level-1 snapshot on every change).
//!
//! Each topic is subscribed in its own frame with the topic as `req_id`, so the venue's
//! answer names which pane it is about: a refusal fails that pane and the socket carries on.
//! Every row carries `confirm`, true once the bar is closed. The venue drops a socket silent
//! for a while, so `{"op":"ping"}` goes out after 20 quiet seconds (its documented cadence).

use std::collections::HashMap;

use otw_connector_model::{Candle, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Qty, Quote, Timeframe};
use otw_connector_sdk::parse::{ts_millis, Dec, Int};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;

use super::{interval, volumes, Bybit, Category, WS, WS_EU};

const CONN: &str = "public";
const PING_EVERY_NS: i64 = 20_000_000_000;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the level-1 book.
    tf: Option<Timeframe>,
}

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    by_topic: HashMap<String, Sub>,
    last_sent: Option<otw_connector_model::Timestamp>,
    /// The level-1 book per topic, so a delta naming one side keeps the other.
    books: HashMap<String, (Option<Level>, Option<Level>)>,
}

type Level = (Dec, Dec);

fn book_topic(native: &str) -> String {
    format!("orderbook.1.{}", native.to_ascii_uppercase())
}

fn topic(tf: Timeframe, native: &str) -> Option<String> {
    interval(tf).map(|i| format!("kline.{i}.{}", native.to_ascii_uppercase()))
}

/// Options publish no candles, so their book declares no live candles and never gets here.
fn path(category: Category) -> &'static str {
    match category {
        Category::Spot => "/v5/public/spot",
        Category::Linear => "/v5/public/linear",
        Category::Inverse => "/v5/public/inverse",
        Category::Option => "/v5/public/option",
    }
}

fn send(st: &mut LiveState, payload: String, now: otw_connector_model::Timestamp) -> NativeRequest {
    st.last_sent = Some(now);
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

pub fn encode(m: &Bybit, cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut add = Vec::new();
    let mut drop = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let t = topic(*tf, &instrument.native)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("Bybit has no live candle topic for {tf}")))?;
                st.by_sub.insert(*sub_id, t.clone());
                st.by_topic.insert(t.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf) });
                add.push(t);
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let t = book_topic(&instrument.native);
                st.by_sub.insert(*sub_id, t.clone());
                st.by_topic.insert(t.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None });
                add.push(t);
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Bybit streams candles and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(t) = st.by_sub.remove(sub_id) {
                    st.by_topic.remove(&t);
                    st.books.remove(&t);
                    drop.push(t);
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened && !add.is_empty() {
        let base = if m.eu { WS_EU } else { WS };
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base, path: path(m.category).into() });
        st.opened = true;
    }
    for t in add {
        out.push(send(st, serde_json::json!({ "op": "subscribe", "req_id": t, "args": [t] }).to_string(), ctx.received_ts));
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
    vec![send(st, r#"{"op":"ping"}"#.into(), ctx.received_ts)]
}

#[derive(Deserialize)]
struct Row {
    start: Int,
    open: Dec,
    high: Dec,
    low: Dec,
    close: Dec,
    volume: Dec,
    #[serde(default)]
    turnover: Option<Dec>,
    confirm: bool,
    #[serde(default)]
    timestamp: Option<Int>,
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    ret_msg: Option<String>,
    #[serde(default)]
    req_id: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    ts: Option<Int>,
    /// Candle rows, or one book object: read once the topic says which.
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// `{"s":"BTCUSDT","b":[["82250.9","1.12"]],"a":[["82251","0.50"]],"u":..,"seq":..}`.
#[derive(Deserialize)]
struct Book {
    #[serde(default)]
    b: Vec<Level>,
    #[serde(default)]
    a: Vec<Level>,
}

/// The topic a refusal names when it carries no `req_id`: `Invalid symbol :[kline.1.X]`.
fn named_topic(msg: &str) -> Option<&str> {
    let (_, rest) = msg.split_once('[')?;
    rest.split_once(']').map(|(t, _)| t)
}

pub fn decode(category: Category, payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("bybit socket: {e}")))?;
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    match f.op.as_deref() {
        Some("subscribe") => {
            let msg = f.ret_msg.unwrap_or_default();
            let t = f.req_id.filter(|r| !r.is_empty()).or_else(|| named_topic(&msg).map(str::to_string));
            let sub = t.as_deref().and_then(|t| st.by_topic.get(t));
            return match (f.success, sub) {
                (Some(true), Some(s)) => Ok(vec![at(s.sub_id, Event::SubscriptionAck)]),
                (Some(true), None) => Ok(vec![]),
                (_, Some(s)) => Ok(vec![at(
                    s.sub_id,
                    Event::SubscriptionFail {
                        error: ConnectorError::new(ErrorKind::NotFound, format!("Bybit does not list {} on this market: {msg}", s.instrument.native)),
                    },
                )]),
                (_, None) => Err(ConnectorError::new(ErrorKind::Transient, format!("Bybit: {msg}"))),
            };
        }
        Some("ping") | Some("pong") | Some("unsubscribe") => return Ok(vec![]),
        _ => {}
    }
    let (Some(t), Some(data)) = (f.topic, f.data) else { return Ok(vec![]) };
    let Some(sub) = st.by_topic.get(&t) else { return Ok(vec![]) };
    let Some(tf) = sub.tf else {
        let book: Book = serde_json::from_value(data).map_err(|e| ConnectorError::decode(format!("bybit book: {e}")))?;
        // Level 1 comes as snapshots; a delta names the side that moved, and a zero size
        // empties it.
        let entry = st.books.entry(t).or_default();
        let live = |l: Option<&Level>| l.copied().filter(|(_, q)| !q.0.is_zero());
        if f.kind.as_deref() == Some("snapshot") {
            *entry = (live(book.b.first()), live(book.a.first()));
        } else {
            if let Some(l) = book.b.first() {
                entry.0 = live(Some(l));
            }
            if let Some(l) = book.a.first() {
                entry.1 = live(Some(l));
            }
        }
        // An empty side is no quote: nothing rather than half a price.
        let (Some((bid, bq)), Some((ask, aq))) = *entry else { return Ok(vec![]) };
        let q = Quote { instrument: sub.instrument.clone(), bid: bid.price(), ask: ask.price(), bid_qty: Some(Qty(bq.0)), ask_qty: Some(Qty(aq.0)) };
        let e = at(sub.sub_id, Event::Quote(q));
        return Ok(vec![match f.ts.map(|t| ts_millis(t.0)).transpose()? {
            Some(ts) => e.at(ts),
            None => e,
        }]);
    };
    let rows: Vec<Row> = serde_json::from_value(data).map_err(|e| ConnectorError::decode(format!("bybit socket: {e}")))?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let (volume, quote_volume) = volumes(category, r.volume, r.turnover);
        let c = Candle {
            instrument: sub.instrument.clone(),
            tf,
            open_ts: ts_millis(r.start.0)?,
            open: r.open.price(),
            high: r.high.price(),
            low: r.low.price(),
            close: r.close.price(),
            volume,
            quote_volume,
            trades: None,
            is_final: r.confirm,
            adjusted: None,
            sides: None,
        };
        let e = at(sub.sub_id, Event::Candle(c));
        out.push(match r.timestamp.map(|t| ts_millis(t.0)).transpose()? {
            Some(ts) => e.at(ts),
            None => e,
        });
    }
    Ok(out)
}
