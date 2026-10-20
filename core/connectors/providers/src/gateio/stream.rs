//! Gate.io live candles. Spot on `wss://api.gateio.ws/ws/v4/` (`spot.candlesticks`), USDT
//! perpetuals on `wss://fx-ws.gateio.ws/v4/ws/usdt` (`futures.candlesticks`), each pane
//! subscribed as `["<interval>", "<symbol>"]` in its own frame with its id, so the venue's
//! answer (`result.status` `success` or `fail`, the id echoed) names the pane.
//!
//! Updates are keyed by `n` = `<interval>_<symbol>` and carry `w`, true once the bar is
//! closed (measured: pushed at the minute boundary on both books). Spot `v` is the quote
//! volume and `a` the base amount; futures `v` is contracts and `a` the quote volume, so a
//! futures pane reads its contract's size from the list before it subscribes. A ping goes
//! out after 20 quiet seconds.
//!
//! The best bid and ask ride the same sockets: `spot.book_ticker` / `futures.book_ticker`,
//! payload `["<symbol>"]`, an update on every change (`b`/`a` prices, `B`/`A` sizes, futures
//! sizes in contracts). Spot acks a symbol it does not list and then sends nothing (measured).

use std::collections::HashMap;

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event, InstrumentId, Qty, Quote, Timeframe, Timestamp};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{interval, Book, CandleRow, Gateio, FX_WS, SETTLE, WS};

const CONN: &str = "public";
const PING_EVERY_NS: i64 = 20_000_000_000;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the book ticker.
    tf: Option<Timeframe>,
    /// Base coin per unit of the futures volume (one on spot). A quote whose contract is not
    /// in the list carries no sizes rather than contracts read as coins.
    size: Option<Decimal>,
}

fn ticker_channel(book: Book) -> &'static str {
    match book {
        Book::Spot => "spot.book_ticker",
        Book::Futures => "futures.book_ticker",
    }
}

/// The routing name of a book ticker: its symbol, apart from the candles' `<interval>_<symbol>`.
fn ticker_name(native: &str) -> String {
    format!("book_ticker|{native}")
}

fn ticker_frame(book: Book, id: u64, event: &str, native: &str, now: Timestamp) -> String {
    serde_json::json!({ "time": now.secs(), "id": id, "channel": ticker_channel(book), "event": event, "payload": [native] }).to_string()
}

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    by_name: HashMap<String, Sub>,
    /// Panes refused before anything went out, reported on the next frame.
    refused: Vec<(CorrId, ConnectorError)>,
    last_sent: Option<Timestamp>,
}

fn channel(book: Book) -> &'static str {
    match book {
        Book::Spot => "spot.candlesticks",
        Book::Futures => "futures.candlesticks",
    }
}

fn ping_channel(book: Book) -> &'static str {
    match book {
        Book::Spot => "spot.ping",
        Book::Futures => "futures.ping",
    }
}

fn send(st: &mut LiveState, payload: String, now: Timestamp) -> NativeRequest {
    st.last_sent = Some(now);
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

fn frame(book: Book, id: u64, event: &str, token: &str, native: &str, now: Timestamp) -> String {
    serde_json::json!({ "time": now.secs(), "id": id, "channel": channel(book), "event": event, "payload": [token, native] }).to_string()
}

fn ping(book: Book, now: Timestamp) -> String {
    serde_json::json!({ "time": now.secs(), "channel": ping_channel(book) }).to_string()
}

pub fn encode(m: &Gateio, cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let book = m.book;
    let now = ctx.received_ts;
    let mut frames = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let token = interval(book, *tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("{} has no live candle topic for {tf}", book.label())))?;
                let size = match book {
                    Book::Spot => Some(Decimal::ONE),
                    Book::Futures => ctx.instruments.get(instrument).map(|s| s.multiplier).filter(|s| s.is_sign_positive() && !s.is_zero()),
                };
                let Some(size) = size else {
                    st.refused.push((*sub_id, ConnectorError::not_found(format!("{} lists no USDT-settled perpetual `{}`", book.label(), instrument.native))));
                    continue;
                };
                let name = format!("{token}_{}", instrument.native);
                st.by_sub.insert(*sub_id, name.clone());
                st.by_name.insert(name, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf), size: Some(size) });
                frames.push(frame(book, sub_id.0, "subscribe", token, &instrument.native, now));
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let size = match book {
                    Book::Spot => Some(Decimal::ONE),
                    Book::Futures => ctx.instruments.get(instrument).map(|s| s.multiplier).filter(|s| s.is_sign_positive() && !s.is_zero()),
                };
                let name = ticker_name(&instrument.native);
                st.by_sub.insert(*sub_id, name.clone());
                st.by_name.insert(name, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None, size });
                frames.push(ticker_frame(book, sub_id.0, "subscribe", &instrument.native, now));
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("{} streams candles and quotes only here, not {data_type:?}", book.label())));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(name) = st.by_sub.remove(sub_id) {
                    if let Some(sub) = st.by_name.remove(&name) {
                        match sub.tf {
                            None => frames.push(ticker_frame(book, sub_id.0, "unsubscribe", &sub.instrument.native, now)),
                            Some(tf) => {
                                if let Some(token) = interval(book, tf) {
                                    frames.push(frame(book, sub_id.0, "unsubscribe", token, &sub.instrument.native, now));
                                }
                            }
                        }
                    }
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    // A pane refused here is reported on the next frame: a ping makes the venue send one.
    if frames.is_empty() && !st.refused.is_empty() {
        frames.push(ping(book, now));
    }
    let mut out = Vec::new();
    if !st.opened && !frames.is_empty() {
        let (base, path) = match book {
            Book::Spot => (WS, "/ws/v4/".to_string()),
            Book::Futures => (FX_WS, format!("/v4/ws/{SETTLE}")),
        };
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base, path });
        st.opened = true;
    }
    for f in frames {
        out.push(send(st, f, now));
    }
    Ok(out)
}

pub fn keepalive(book: Book, st: &mut LiveState, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
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
    vec![send(st, ping(book, ctx.received_ts), ctx.received_ts)]
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    time_ms: Option<Int>,
    #[serde(default)]
    event: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    error: Option<SubError>,
    #[serde(default)]
    result: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
struct SubError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct Status {
    #[serde(default)]
    status: String,
}

/// One candle update. `t` is a string on spot and a number on futures.
#[derive(Deserialize)]
struct Row {
    t: Int,
    n: String,
    o: Dec,
    h: Dec,
    l: Dec,
    c: Dec,
    v: Dec,
    #[serde(default)]
    a: OptDec,
    #[serde(default)]
    w: bool,
}

/// `{"t":1792409781850,"u":..,"s":"BTC_USDT","b":"82165.7","B":"0.16","a":"82165.8","A":"0.06"}`;
/// the futures sizes are numbers, in contracts.
#[derive(Deserialize)]
struct BookTicker {
    #[serde(default)]
    t: Option<Int>,
    s: String,
    b: OptDec,
    #[serde(rename = "B", default)]
    bid_qty: OptDec,
    a: OptDec,
    #[serde(rename = "A", default)]
    ask_qty: OptDec,
}

pub fn decode(book: Book, payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    let mut out: Vec<Envelope> = st.refused.drain(..).map(|(sub, error)| at(sub, Event::SubscriptionFail { error })).collect();
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("gateio socket: {e}")))?;
    match f.event.as_str() {
        "subscribe" => {
            let sub = f.id.map(CorrId).filter(|s| st.by_sub.contains_key(s));
            let ok = f.error.is_none() && f.result.as_ref().and_then(|r| serde_json::from_str::<Status>(r.get()).ok()).is_some_and(|s| s.status == "success");
            match (sub, ok) {
                (Some(sub), true) => out.push(at(sub, Event::SubscriptionAck)),
                (Some(sub), false) => {
                    let msg = f.error.map(|e| e.message).unwrap_or_else(|| "refused".into());
                    if let Some(name) = st.by_sub.remove(&sub) {
                        st.by_name.remove(&name);
                    }
                    out.push(at(sub, Event::SubscriptionFail { error: ConnectorError::new(ErrorKind::NotFound, format!("{}: {msg}", book.label())) }));
                }
                (None, true) => {}
                (None, false) => {
                    let msg = f.error.map(|e| e.message).unwrap_or_else(|| "refused".into());
                    return Err(ConnectorError::new(ErrorKind::Transient, format!("{}: {msg}", book.label())));
                }
            }
        }
        "update" if f.channel == ticker_channel(book) => {
            let Some(result) = f.result.as_ref() else { return Ok(out) };
            let t: BookTicker = serde_json::from_str(result.get()).map_err(|e| ConnectorError::decode(format!("gateio book ticker: {e}")))?;
            let Some(sub) = st.by_name.get(&ticker_name(&t.s)) else { return Ok(out) };
            let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
            // An empty side is no quote.
            let (Some(bid), Some(ask)) = (positive(t.b), positive(t.a)) else { return Ok(out) };
            let coins = |q: OptDec| q.0.zip(sub.size).and_then(|(q, s)| q.checked_mul(s)).map(Qty);
            let q = Quote { instrument: sub.instrument.clone(), bid: otw_connector_model::Price(bid), ask: otw_connector_model::Price(ask), bid_qty: coins(t.bid_qty), ask_qty: coins(t.ask_qty) };
            let e = at(sub.sub_id, Event::Quote(q));
            out.push(match t.t.or(f.time_ms).map(|t| ts_millis(t.0)).transpose()? {
                Some(ts) => e.at(ts),
                None => e,
            });
        }
        "update" => {
            let Some(result) = f.result.as_ref() else { return Ok(out) };
            // Spot sends one row, futures a list.
            let rows: Vec<Row> = match book {
                Book::Spot => vec![serde_json::from_str(result.get()).map_err(|e| ConnectorError::decode(format!("gateio candle: {e}")))?],
                Book::Futures => serde_json::from_str(result.get()).map_err(|e| ConnectorError::decode(format!("gateio candle: {e}")))?,
            };
            let ts = f.time_ms.map(|t| ts_millis(t.0)).transpose()?;
            for r in rows {
                let Some(sub) = st.by_name.get(&r.n) else { continue };
                let (Some(tf), Some(size)) = (sub.tf, sub.size) else { continue };
                let (vol, turnover) = match book {
                    Book::Spot => (r.a.0.ok_or_else(|| ConnectorError::decode("gateio candle: no base amount"))?, Some(r.v.0)),
                    Book::Futures => (r.v.0.checked_mul(size).ok_or_else(|| ConnectorError::decode("gateio candle: volume overflow"))?, r.a.0),
                };
                let row = CandleRow { open_s: r.t.0, o: r.o, h: r.h, l: r.l, c: r.c, vol, turnover, closed: Some(r.w) };
                let e = at(sub.sub_id, Event::Candle(row.candle(sub.instrument.clone(), tf, r.w)?));
                out.push(match ts {
                    Some(t) => e.at(t),
                    None => e,
                });
            }
        }
        // `unsubscribe` answers, pongs (event `""`), and anything new: nothing to route.
        _ => {}
    }
    Ok(out)
}
