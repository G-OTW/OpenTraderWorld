//! KuCoin live candles. The socket's address is handed out per connection: a POST to
//! `bullet-public` answers a host and a token (the live adapter asks it first, through
//! `socket_bootstrap`), and the socket opens at `host?token=...`.
//!
//! Topics: spot `/market/candles:<symbol>_<interval>`, futures
//! `/contractMarket/limitCandle:<symbol>_<interval>`. Each pane subscribes in its own frame
//! with its id, so the venue's `ack` or `error` names the pane. There is **no closed flag**:
//! every candle goes out forming and the engine closes it when the next period starts. The
//! venue acknowledges a symbol it does not list and then sends nothing, so a pane is checked
//! against the instrument list first and refused by name. A `ping` goes out within the
//! interval the bullet answer names (18 s), or the venue drops the socket.
//!
//! The best bid and ask ride the same socket: spot `/market/ticker:<symbol>` (`bestBid`,
//! `bestAsk`, `time` in ms), futures `/contractMarket/tickerV2:<symbol>` (`bestBidPrice`,
//! `bestAskPrice`, sizes in contracts, `ts` in ns).

use std::collections::HashMap;

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event, InstrumentId, Price, Qty, Quote, Timeframe, Timestamp};
use otw_connector_sdk::parse::{ts_millis, ts_nanos, Int, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{interval, Book, CandleRow, Kucoin, Layout};

const CONN: &str = "public";
/// The bullet answer's ping interval when it names none.
const DEFAULT_PING_MS: i64 = 18_000;
/// The idle check runs every 5 s: a ping is due this much before the venue's interval.
const PING_MARGIN_MS: i64 = 5_000;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the ticker (best bid and ask).
    tf: Option<Timeframe>,
    /// Base coin per unit of the topic's volume.
    size: Decimal,
}

#[derive(Default)]
pub struct LiveState {
    /// `host?token=...`, from the bullet answer.
    url: Option<String>,
    ping_ms: i64,
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    by_topic: HashMap<String, Sub>,
    /// Panes refused before anything went out, reported on the next frame.
    refused: Vec<(CorrId, ConnectorError)>,
    last_sent: Option<Timestamp>,
}

impl LiveState {
    /// The address and ping cadence `bullet-public` handed out for this connection.
    pub fn handed_out(&mut self, endpoint: &str, token: &str, ping_ms: Option<i64>) {
        self.url = Some(format!("{endpoint}?token={token}"));
        self.ping_ms = ping_ms.filter(|p| *p > 0).unwrap_or(DEFAULT_PING_MS);
    }
}

fn topic(book: Book, native: &str, token: &str) -> String {
    match book {
        Book::Spot => format!("/market/candles:{native}_{token}"),
        Book::Futures => format!("/contractMarket/limitCandle:{native}_{token}"),
    }
}

fn ticker_topic(book: Book, native: &str) -> String {
    match book {
        Book::Spot => format!("/market/ticker:{native}"),
        Book::Futures => format!("/contractMarket/tickerV2:{native}"),
    }
}

fn send(st: &mut LiveState, payload: String, now: Timestamp) -> NativeRequest {
    st.last_sent = Some(now);
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

fn frame(id: String, kind: &str, topic: &str) -> String {
    serde_json::json!({ "id": id, "type": kind, "topic": topic, "privateChannel": false, "response": true }).to_string()
}

pub fn encode(m: &Kucoin, cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let book = m.book;
    let mut frames = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type } if matches!(data_type, DataType::Candles { .. }) || (*data_type == DataType::Quote && !m.capabilities().live.quote.is_none()) => {
                let tf = match data_type {
                    DataType::Candles { tf } => Some(*tf),
                    _ => None,
                };
                let token = match tf {
                    Some(tf) => Some(
                        interval(tf)
                            .filter(|_| m.capabilities().live_timeframes().contains(&tf))
                            .ok_or_else(|| ConnectorError::unsupported(format!("{} has no live candle topic for {tf}", book.label())))?,
                    ),
                    None => None,
                };
                let listed = ctx.instruments.get(instrument);
                let size = match (book, listed) {
                    (Book::Spot, Some(_)) => Some(Decimal::ONE),
                    (Book::Futures, Some(spec)) => Some(spec.multiplier).filter(|s| s.is_sign_positive() && !s.is_zero()),
                    (_, None) => None,
                };
                let Some(size) = size else {
                    let what = if book == Book::Spot { "pair" } else { "USDT- or USDC-margined contract" };
                    st.refused.push((*sub_id, ConnectorError::not_found(format!("{} lists no {what} `{}`", book.label(), instrument.native))));
                    continue;
                };
                let t = match token {
                    Some(token) => topic(book, &instrument.native, token),
                    None => ticker_topic(book, &instrument.native),
                };
                st.by_sub.insert(*sub_id, t.clone());
                st.by_topic.insert(t.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf, size });
                frames.push(frame(sub_id.0.to_string(), "subscribe", &t));
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("{} streams candles and quotes only here, not {data_type:?}", book.label())));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(t) = st.by_sub.remove(sub_id) {
                    st.by_topic.remove(&t);
                    frames.push(frame(format!("u{}", sub_id.0), "unsubscribe", &t));
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened && (!frames.is_empty() || !st.refused.is_empty()) {
        let url = st.url.clone().ok_or_else(|| ConnectorError::new(ErrorKind::Transient, format!("{}: no socket address was handed out (bullet-public)", book.label())))?;
        out.push(NativeRequest::WsOpenUrl { conn_key: ConnKey(CONN.into()), url });
        st.opened = true;
    }
    for f in frames {
        out.push(send(st, f, ctx.received_ts));
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
    let every_ms = (st.ping_ms - PING_MARGIN_MS).max(PING_MARGIN_MS);
    if ctx.received_ts.millis() - last.millis() < every_ms {
        return Vec::new();
    }
    let ping = serde_json::json!({ "id": ctx.received_ts.millis().to_string(), "type": "ping" }).to_string();
    vec![send(st, ping, ctx.received_ts)]
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    data: Option<Box<RawValue>>,
}

/// Spot `{"bestAsk":"..","bestAskSize":"..","bestBid":"..","bestBidSize":"..","time":ms}`;
/// futures `{"bestBidPrice":"..","bestBidSize":1108,"bestAskPrice":"..","bestAskSize":104,"ts":ns}`.
#[derive(Deserialize)]
struct TickerData {
    #[serde(alias = "bestBidPrice", default)]
    #[serde(rename = "bestBid")]
    bid: OptDec,
    #[serde(alias = "bestAskPrice", default)]
    #[serde(rename = "bestAsk")]
    ask: OptDec,
    #[serde(rename = "bestBidSize", default)]
    bid_qty: OptDec,
    #[serde(rename = "bestAskSize", default)]
    ask_qty: OptDec,
    #[serde(default)]
    time: Option<Int>,
    #[serde(default)]
    ts: Option<Int>,
}

#[derive(Deserialize)]
struct CandleData {
    candles: Vec<Box<RawValue>>,
    /// Spot: nanoseconds. Futures: milliseconds.
    #[serde(default)]
    time: Option<Int>,
}

pub fn decode(book: Book, payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    let mut out: Vec<Envelope> = st.refused.drain(..).map(|(sub, error)| at(sub, Event::SubscriptionFail { error })).collect();
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("kucoin socket: {e}")))?;
    let sub_of_id = |id: &Option<String>| id.as_deref().and_then(|i| i.parse::<u64>().ok()).map(CorrId);
    match f.kind.as_str() {
        "ack" => {
            if let Some(sub) = sub_of_id(&f.id).filter(|s| st.by_sub.contains_key(s)) {
                out.push(at(sub, Event::SubscriptionAck));
            }
        }
        "error" => {
            let msg = f.data.as_ref().map(|d| d.get().trim_matches('"').to_string()).unwrap_or_else(|| "refused".into());
            match sub_of_id(&f.id).filter(|s| st.by_sub.contains_key(s)) {
                Some(sub) => out.push(at(sub, Event::SubscriptionFail { error: ConnectorError::new(ErrorKind::NotFound, format!("{}: {msg}", book.label())) })),
                None => return Err(ConnectorError::new(ErrorKind::Transient, format!("{}: {msg}", book.label()))),
            }
        }
        "message" => {
            let (Some(t), Some(data)) = (f.topic.as_deref(), f.data.as_ref()) else { return Ok(out) };
            let Some(sub) = st.by_topic.get(t) else { return Ok(out) };
            let Some(tf) = sub.tf else {
                let d: TickerData = serde_json::from_str(data.get()).map_err(|e| ConnectorError::decode(format!("kucoin ticker: {e}")))?;
                let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
                // An empty side is no quote.
                let (Some(bid), Some(ask)) = (positive(d.bid), positive(d.ask)) else { return Ok(out) };
                let coins = |q: OptDec| q.0.and_then(|q| q.checked_mul(sub.size)).map(Qty);
                let q = Quote { instrument: sub.instrument.clone(), bid: Price(bid), ask: Price(ask), bid_qty: coins(d.bid_qty), ask_qty: coins(d.ask_qty) };
                let ts = match (d.time, d.ts) {
                    (Some(ms), _) => Some(ts_millis(ms.0)?),
                    (None, Some(ns)) => Some(ts_nanos(ns.0)),
                    _ => None,
                };
                let e = at(sub.sub_id, Event::Quote(q));
                out.push(match ts {
                    Some(t) => e.at(t),
                    None => e,
                });
                return Ok(out);
            };
            let d: CandleData = serde_json::from_str(data.get()).map_err(|e| ConnectorError::decode(format!("kucoin candle: {e}")))?;
            let layout = if book == Book::Spot { Layout::Spot } else { Layout::FuturesWs };
            let r = CandleRow::read(&d.candles, layout, sub.size).ok_or_else(|| ConnectorError::decode("kucoin candle row"))?;
            let c = r.candle(sub.instrument.clone(), tf, false)?;
            let ts = match (book, d.time) {
                (Book::Spot, Some(t)) => Some(ts_nanos(t.0)),
                (Book::Futures, Some(t)) => Some(ts_millis(t.0)?),
                (_, None) => None,
            };
            let e = at(sub.sub_id, Event::Candle(c));
            out.push(match ts {
                Some(t) => e.at(t),
                None => e,
            });
        }
        // `welcome`, `pong`, and anything new: nothing to route.
        _ => {}
    }
    Ok(out)
}
