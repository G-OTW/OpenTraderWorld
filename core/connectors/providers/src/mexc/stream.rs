//! MEXC live candles.
//!
//!   - Spot on `wss://wbs-api.mexc.com/ws`: each pane its own `SUBSCRIPTION` frame with its
//!     id, the channel `spot@public.kline.v3.api.pb@<symbol>@<interval>`. The answer echoes
//!     the id, its `msg` the channel on success and "Not Subscribed successfully! ..." on a
//!     refusal (an unknown pair among them, measured). Updates are protobuf binary frames,
//!     keyed by their channel; the JSON channel is refused ("Blocked!", measured). One socket
//!     takes 30 subscriptions at most: a 31st pane is refused here.
//!   - Contracts on `wss://contract.mexc.com/edge`: `sub.kline` with the symbol and interval.
//!     The answers (`rs.sub.kline`, or `rs.error` naming the contract) carry no id and come
//!     in the order asked, so they are matched to the panes in that order. Updates
//!     (`push.kline`) are keyed by symbol and interval; `q` counts contracts and `a` is the
//!     quote turnover, so a futures pane reads its contract's size from the list before it
//!     subscribes.
//!
//! The best bid and ask: spot `spot@public.aggre.bookTicker.v3.api.pb@100ms@<symbol>` (protobuf,
//! at most every 100 ms), contracts `sub.ticker` (`push.ticker`'s `bid1`/`ask1`, about once a
//! second, measured), answered like a candle subscription.
//!
//! Neither socket says when a bar is closed: every candle goes out as forming and the
//! engine closes it when the next starts. A ping goes out on a quiet socket (spot after 20
//! s; the contract socket drops a client that has not pinged for a minute, so every 15 s).

use std::collections::{HashMap, VecDeque};

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event, InstrumentId, Price, Qty, Quote, Timeframe, Timestamp};
use otw_connector_sdk::parse::{dec, ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;

use super::{proto, socket_interval, Book, CandleRow, Mexc, FUT_WS, WS};

const CONN: &str = "public";
/// One spot socket carries 30 subscriptions at most (MEXC's spot docs).
const SPOT_MAX_SUBS: usize = 30;

fn ping_every_ns(book: Book) -> i64 {
    match book {
        Book::Spot => 20_000_000_000,
        Book::Futures => 15_000_000_000,
    }
}

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the best bid and ask.
    tf: Option<Timeframe>,
    /// Base coin per contract (one on spot).
    size: Decimal,
}

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    /// Spot: the channel name; futures: `<symbol>@<interval>`.
    by_name: HashMap<String, Sub>,
    /// Futures: panes sent and not answered yet, in the order sent.
    unanswered: VecDeque<CorrId>,
    /// Panes refused before anything went out, reported on the next frame.
    refused: Vec<(CorrId, ConnectorError)>,
    last_sent: Option<Timestamp>,
}

fn channel(native: &str, token: &str) -> String {
    format!("spot@public.kline.v3.api.pb@{native}@{token}")
}

fn ticker_channel(native: &str) -> String {
    format!("spot@public.aggre.bookTicker.v3.api.pb@100ms@{native}")
}

fn send(st: &mut LiveState, payload: String, now: Timestamp) -> NativeRequest {
    st.last_sent = Some(now);
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

fn ping(book: Book) -> String {
    match book {
        Book::Spot => r#"{"method":"PING"}"#.to_string(),
        Book::Futures => r#"{"method":"ping"}"#.to_string(),
    }
}

pub fn encode(m: &Mexc, cmds: &[Command], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let book = m.book;
    let now = ctx.received_ts;
    let mut frames = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                if book == Book::Spot && st.by_name.len() >= SPOT_MAX_SUBS {
                    st.refused.push((
                        *sub_id,
                        ConnectorError::new(ErrorKind::RateLimited, format!("{} streams {SPOT_MAX_SUBS} panes per socket at most: close one before opening `{}`", book.label(), instrument.native)),
                    ));
                    continue;
                }
                let (name, frame) = match book {
                    Book::Spot => {
                        let ch = ticker_channel(&instrument.native);
                        let f = serde_json::json!({ "method": "SUBSCRIPTION", "params": [ch], "id": sub_id.0 }).to_string();
                        (ch, f)
                    }
                    Book::Futures => {
                        st.unanswered.push_back(*sub_id);
                        let f = serde_json::json!({ "method": "sub.ticker", "param": { "symbol": instrument.native } }).to_string();
                        (format!("{}@ticker", instrument.native), f)
                    }
                };
                // Sizes are not read off a quote, so no contract size is needed.
                st.by_sub.insert(*sub_id, name.clone());
                st.by_name.insert(name, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None, size: Decimal::ONE });
                frames.push(frame);
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let token = socket_interval(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("{} has no live candle topic for {tf}", book.label())))?;
                let size = match book {
                    Book::Spot => Some(Decimal::ONE),
                    Book::Futures => ctx.instruments.get(instrument).map(|s| s.multiplier).filter(|s| s.is_sign_positive() && !s.is_zero()),
                };
                let Some(size) = size else {
                    st.refused.push((*sub_id, ConnectorError::not_found(format!("{} lists no perpetual `{}` settled in its quote coin", book.label(), instrument.native))));
                    continue;
                };
                if book == Book::Spot && st.by_name.len() >= SPOT_MAX_SUBS {
                    st.refused.push((
                        *sub_id,
                        ConnectorError::new(ErrorKind::RateLimited, format!("{} streams {SPOT_MAX_SUBS} panes per socket at most: close one before opening `{}`", book.label(), instrument.native)),
                    ));
                    continue;
                }
                let (name, frame) = match book {
                    Book::Spot => {
                        let ch = channel(&instrument.native, token);
                        let f = serde_json::json!({ "method": "SUBSCRIPTION", "params": [ch], "id": sub_id.0 }).to_string();
                        (ch, f)
                    }
                    Book::Futures => {
                        st.unanswered.push_back(*sub_id);
                        let f = serde_json::json!({ "method": "sub.kline", "param": { "symbol": instrument.native, "interval": token } }).to_string();
                        (format!("{}@{token}", instrument.native), f)
                    }
                };
                st.by_sub.insert(*sub_id, name.clone());
                st.by_name.insert(name, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf), size });
                frames.push(frame);
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("{} streams candles and quotes only here, not {data_type:?}", book.label())));
            }
            Command::Unsubscribe { sub_id } => {
                st.unanswered.retain(|s| s != sub_id);
                if let Some(name) = st.by_sub.remove(sub_id) {
                    if let Some(sub) = st.by_name.remove(&name) {
                        frames.push(match (book, sub.tf) {
                            (Book::Spot, _) => serde_json::json!({ "method": "UNSUBSCRIPTION", "params": [name], "id": sub_id.0 }).to_string(),
                            (Book::Futures, None) => serde_json::json!({ "method": "unsub.ticker", "param": { "symbol": sub.instrument.native } }).to_string(),
                            (Book::Futures, Some(tf)) => {
                                let token = socket_interval(tf).unwrap_or_default();
                                serde_json::json!({ "method": "unsub.kline", "param": { "symbol": sub.instrument.native, "interval": token } }).to_string()
                            }
                        });
                    }
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    // A pane refused here is reported on the next frame: a ping makes the venue send one.
    if frames.is_empty() && !st.refused.is_empty() {
        frames.push(ping(book));
    }
    let mut out = Vec::new();
    if !st.opened && !frames.is_empty() {
        let (base, path) = match book {
            Book::Spot => (WS, "/ws"),
            Book::Futures => (FUT_WS, "/edge"),
        };
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base, path: path.to_string() });
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
    if ctx.received_ts.nanos() - last.nanos() < ping_every_ns(book) {
        return Vec::new();
    }
    vec![send(st, ping(book), ctx.received_ts)]
}

/// A spot text frame: a subscription answer or a pong.
#[derive(Deserialize)]
struct SpotAnswer {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    msg: String,
}

/// A contract socket frame.
#[derive(Deserialize)]
struct FutFrame {
    #[serde(default)]
    channel: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    ts: Option<Int>,
}

/// `push.ticker`: the contract's ticker, of which the best bid and ask.
#[derive(Deserialize)]
struct FutTicker {
    symbol: String,
    #[serde(default)]
    bid1: OptDec,
    #[serde(default)]
    ask1: OptDec,
    #[serde(default)]
    timestamp: Option<Int>,
}

#[derive(Deserialize)]
struct FutKline {
    symbol: String,
    interval: String,
    t: Int,
    o: Dec,
    c: Dec,
    h: Dec,
    l: Dec,
    /// Contracts.
    q: Dec,
    /// Quote turnover.
    #[serde(default)]
    a: OptDec,
}

fn decode_spot(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>, out: &mut Vec<Envelope>) -> Result<(), ConnectorError> {
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    if payload.trim_ascii_start().starts_with(b"{") {
        let a: SpotAnswer = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("mexc socket: {e}")))?;
        let Some(sub) = a.id.map(CorrId).filter(|s| st.by_sub.contains_key(s)) else { return Ok(()) };
        let name = st.by_sub.get(&sub).cloned().unwrap_or_default();
        if a.msg == name {
            out.push(at(sub, Event::SubscriptionAck));
        } else {
            st.by_sub.remove(&sub);
            st.by_name.remove(&name);
            out.push(at(sub, Event::SubscriptionFail { error: ConnectorError::new(ErrorKind::NotFound, format!("MEXC refused `{name}`: {}", a.msg.trim())) }));
        }
        return Ok(());
    }
    let bad = || ConnectorError::decode("mexc socket: malformed protobuf frame");
    let w = proto::wrapper(payload).map_err(|_| bad())?;
    let Some(sub) = st.by_name.get(w.channel) else { return Ok(()) };
    let stamp = w.send_time.or(w.create_time);
    if let (None, Some(t)) = (sub.tf, w.book_ticker) {
        let t = proto::book_ticker(t).map_err(|_| bad())?;
        let positive = |s: &str| dec(s).ok().filter(|p| p.is_sign_positive() && !p.is_zero());
        // An empty side is no quote.
        let (Some(bid), Some(ask)) = (positive(t.bid_price), positive(t.ask_price)) else { return Ok(()) };
        let q = Quote { instrument: sub.instrument.clone(), bid: Price(bid), ask: Price(ask), bid_qty: dec(t.bid_qty).ok().map(Qty), ask_qty: dec(t.ask_qty).ok().map(Qty) };
        let e = at(sub.sub_id, Event::Quote(q));
        out.push(match stamp {
            Some(t) => e.at(ts_millis(t)?),
            None => e,
        });
        return Ok(());
    }
    let (Some(tf), Some(k)) = (sub.tf, w.kline) else { return Ok(()) };
    let k = proto::kline(k).map_err(|_| bad())?;
    let num = |s: &str| dec(s).map(Dec);
    let row = CandleRow { open_s: k.window_start, o: num(k.open)?, h: num(k.high)?, l: num(k.low)?, c: num(k.close)?, vol: dec(k.volume)?, turnover: Some(dec(k.amount)?) };
    let e = at(sub.sub_id, Event::Candle(row.candle(sub.instrument.clone(), tf, false)?));
    out.push(match stamp {
        Some(t) => e.at(ts_millis(t)?),
        None => e,
    });
    Ok(())
}

fn decode_futures(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>, out: &mut Vec<Envelope>) -> Result<(), ConnectorError> {
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    let f: FutFrame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("mexc socket: {e}")))?;
    match f.channel.as_str() {
        "rs.sub.kline" | "rs.sub.ticker" => {
            if let Some(sub) = st.unanswered.pop_front() {
                out.push(at(sub, Event::SubscriptionAck));
            }
        }
        "rs.error" => {
            let msg = f.data.as_ref().and_then(|d| d.as_str()).unwrap_or("refused").to_string();
            // The refusal names the contract (`Contract [NOPE_USDT] not exists`); the oldest
            // unanswered pane otherwise.
            let named = st.unanswered.iter().position(|s| st.by_sub.get(s).and_then(|n| n.split('@').next()).is_some_and(|sym| msg.contains(&format!("[{sym}]"))));
            let Some(sub) = named.or((!st.unanswered.is_empty()).then_some(0)).and_then(|i| st.unanswered.remove(i)) else {
                return Err(ConnectorError::new(ErrorKind::Transient, format!("MEXC futures socket: {msg}")));
            };
            if let Some(name) = st.by_sub.remove(&sub) {
                st.by_name.remove(&name);
            }
            out.push(at(sub, Event::SubscriptionFail { error: ConnectorError::new(ErrorKind::NotFound, format!("MEXC futures: {msg}")) }));
        }
        "push.ticker" => {
            let Some(data) = f.data else { return Ok(()) };
            let t: FutTicker = serde_json::from_value(data).map_err(|e| ConnectorError::decode(format!("mexc ticker: {e}")))?;
            let Some(sub) = st.by_name.get(&format!("{}@ticker", t.symbol)) else { return Ok(()) };
            let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
            let (Some(bid), Some(ask)) = (positive(t.bid1), positive(t.ask1)) else { return Ok(()) };
            let q = Quote { instrument: sub.instrument.clone(), bid: Price(bid), ask: Price(ask), bid_qty: None, ask_qty: None };
            let e = at(sub.sub_id, Event::Quote(q));
            out.push(match t.timestamp.or(f.ts) {
                Some(t) => e.at(ts_millis(t.0)?),
                None => e,
            });
        }
        "push.kline" => {
            let Some(data) = f.data else { return Ok(()) };
            let k: FutKline = serde_json::from_value(data).map_err(|e| ConnectorError::decode(format!("mexc candle: {e}")))?;
            let Some(sub) = st.by_name.get(&format!("{}@{}", k.symbol, k.interval)) else { return Ok(()) };
            let vol = k.q.0.checked_mul(sub.size).ok_or_else(|| ConnectorError::decode("mexc candle: volume overflow"))?;
            let row = CandleRow { open_s: k.t.0, o: k.o, h: k.h, l: k.l, c: k.c, vol, turnover: k.a.0 };
            let Some(tf) = sub.tf else { return Ok(()) };
            let e = at(sub.sub_id, Event::Candle(row.candle(sub.instrument.clone(), tf, false)?));
            out.push(match f.ts {
                Some(t) => e.at(ts_millis(t.0)?),
                None => e,
            });
        }
        // `rs.unsub.kline`, `pong`, and anything new: nothing to route.
        _ => {}
    }
    Ok(())
}

pub fn decode(book: Book, payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let mut out: Vec<Envelope> =
        st.refused.drain(..).map(|(sub, error)| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, Event::SubscriptionFail { error })).collect();
    match book {
        Book::Spot => decode_spot(payload, st, ctx, &mut out)?,
        Book::Futures => decode_futures(payload, st, ctx, &mut out)?,
    }
    Ok(out)
}
