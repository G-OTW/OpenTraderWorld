//! Bitfinex live candles on `wss://api-pub.bitfinex.com/ws/2`, both books on one socket.
//!
//! A pane subscribes `{"event":"subscribe","channel":"candles","key":"trade:1m:tBTCUSD"}`.
//! The answer `subscribed` echoes the key with the channel id (`chanId`) every later frame
//! is routed by: first a snapshot `[chanId, [[MTS, OPEN, CLOSE, HIGH, LOW, VOLUME], ...]]`
//! (the newest 240 bars, newest first), then updates `[chanId, [MTS, ...]]` and a heartbeat
//! `[chanId, "hb"]` every 15 s. Measured on 2026-10-19:
//!   - an unknown pair, a name without its `t` or an unknown timeframe is *accepted*, with an
//!     empty snapshot: since bars only exist where something traded, the newest 240 of a
//!     real pair are never empty, so an empty snapshot refuses the pane by name;
//!   - each update repeats the previous bar next to the forming one; the older is no news;
//!   - a refusal is `{"event":"error","code","msg","channel","key"}` (10305 `subscribe:
//!     limit`, 10301 `subscribe: dup`), routed by its key.
//!
//! The best bid and ask ride the same socket: `{"event":"subscribe","channel":"book",
//! "symbol":"tBTCUSD","prec":"P0","freq":"F0","len":"1"}`, a one-level book. The snapshot
//! `[chanId, [[PRICE, COUNT, AMOUNT], ...]]` holds the bid (amount > 0) and the ask
//! (amount < 0); each update `[chanId, [PRICE, COUNT, AMOUNT]]` replaces its side, and a
//! count of 0 empties it until the next level arrives (measured: the removal and the new top
//! come as two frames). The `ticker` channel carries a bid and ask too, but only every few
//! seconds (measured). An unknown symbol is refused (10300 `symbol: invalid`), named by
//! `channel` and `symbol`, not by a key.
//!
//! No frame says when a bar is closed: every candle goes out as forming and the engine
//! closes it when the next starts. `info` 20051 (server restarting) and 20061 (maintenance
//! over, resubscribe) end the socket so the hub reconnects and subscribes again.

use std::collections::HashMap;

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event, InstrumentId, Price, Qty, Quote, Timeframe};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{interval, Bitfinex, CandleRow, WS};

const CONN: &str = "public";
/// Subscriptions one socket carries: 30 by the socket docs (the general limits page says
/// 25; 56 were accepted on 2026-10-19 before `subscribe: limit`). A 31st pane is refused here.
pub(crate) const MAX_SUBS: usize = 30;

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the one-level book.
    tf: Option<Timeframe>,
    /// The book's top per side, (price, size): bid, ask.
    top: (Option<(Decimal, Decimal)>, Option<(Decimal, Decimal)>),
    chan: Option<i64>,
    /// Closed by its pane before the venue acknowledged it: dropped when its id arrives.
    closed: bool,
}

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    /// By candle key (`trade:1m:tBTCUSD`).
    by_key: HashMap<String, Sub>,
    by_chan: HashMap<i64, String>,
    /// Panes refused before anything went out, reported on the next frame.
    refused: Vec<(CorrId, ConnectorError)>,
    /// Frames owed to the venue, sent on the next keepalive: the unsubscribe of a pane closed
    /// before its channel id was known.
    owed: Vec<String>,
}

fn key(token: &str, native: &str) -> String {
    format!("trade:{token}:{native}")
}

/// A book subscription's routing key: its answers name the channel and the symbol.
fn book_key(native: &str) -> String {
    format!("book:{native}")
}

fn ws_send(payload: String) -> NativeRequest {
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.into(), corr_id: None }
}

fn unsubscribe(chan: i64) -> String {
    serde_json::json!({ "event": "unsubscribe", "chanId": chan }).to_string()
}

const PING: &str = r#"{"event":"ping","cid":1}"#;

pub fn encode(m: &Bitfinex, cmds: &[Command], st: &mut LiveState, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let book = m.book;
    let mut frames = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let token = interval(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("{} has no live candle channel for {tf}", book.label())))?;
                if let Err(e) = m.mine(instrument) {
                    st.refused.push((*sub_id, e));
                    continue;
                }
                if st.by_key.len() >= MAX_SUBS {
                    st.refused.push((
                        *sub_id,
                        ConnectorError::new(ErrorKind::RateLimited, format!("{} streams {MAX_SUBS} panes per socket at most: close one before opening `{}`", book.label(), instrument.native)),
                    ));
                    continue;
                }
                let k = key(token, &instrument.native);
                frames.push(serde_json::json!({ "event": "subscribe", "channel": "candles", "key": k }).to_string());
                st.by_sub.insert(*sub_id, k.clone());
                st.by_key.insert(k, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf), top: (None, None), chan: None, closed: false });
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                if let Err(e) = m.mine(instrument) {
                    st.refused.push((*sub_id, e));
                    continue;
                }
                if st.by_key.len() >= MAX_SUBS {
                    st.refused.push((
                        *sub_id,
                        ConnectorError::new(ErrorKind::RateLimited, format!("{} streams {MAX_SUBS} panes per socket at most: close one before opening `{}`", book.label(), instrument.native)),
                    ));
                    continue;
                }
                let k = book_key(&instrument.native);
                frames.push(serde_json::json!({ "event": "subscribe", "channel": "book", "symbol": instrument.native, "prec": "P0", "freq": "F0", "len": "1" }).to_string());
                st.by_sub.insert(*sub_id, k.clone());
                st.by_key.insert(k, Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None, top: (None, None), chan: None, closed: false });
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("{} streams candles and quotes only here, not {data_type:?}", book.label())));
            }
            Command::Unsubscribe { sub_id } => {
                let Some(k) = st.by_sub.remove(sub_id) else { continue };
                let Some(sub) = st.by_key.remove(&k) else { continue };
                match sub.chan {
                    Some(chan) => {
                        st.by_chan.remove(&chan);
                        frames.push(unsubscribe(chan));
                    }
                    // Not acknowledged yet: the channel is dropped when its id arrives.
                    None => {
                        st.by_key.insert(k, Sub { closed: true, ..sub });
                    }
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    // A pane refused here is reported on the next frame: a ping makes the venue send one.
    if frames.is_empty() && !st.refused.is_empty() {
        frames.push(PING.to_string());
    }
    let mut out = Vec::new();
    if !st.opened && !frames.is_empty() {
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: "/ws/2".to_string() });
        st.opened = true;
    }
    out.extend(frames.into_iter().map(ws_send));
    Ok(out)
}

/// The venue heartbeats every channel each 15 s, so no ping is owed; only the unsubscribes
/// of panes closed before their channel id arrived.
pub fn keepalive(st: &mut LiveState, _ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
    std::mem::take(&mut st.owed).into_iter().map(ws_send).collect()
}

#[derive(Deserialize)]
struct EventFrame {
    event: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default, rename = "chanId")]
    chan_id: Option<i64>,
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    msg: Option<String>,
}

fn decode_event(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>, out: &mut Vec<Envelope>) -> Result<(), ConnectorError> {
    let at = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    let f: EventFrame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("bitfinex socket: {e}")))?;
    let msg = f.msg.clone().unwrap_or_default();
    // A candle answer names its key; a book answer its channel and symbol.
    let routed = match (f.key.clone(), f.channel.as_deref(), f.symbol.as_deref()) {
        (Some(k), _, _) => Some(k),
        (None, Some("book"), Some(sym)) => Some(book_key(sym)),
        _ => None,
    };
    match f.event.as_str() {
        "subscribed" => {
            let (Some(k), Some(chan)) = (routed, f.chan_id) else { return Ok(()) };
            let Some(sub) = st.by_key.get_mut(&k) else { return Ok(()) };
            sub.chan = Some(chan);
            if sub.closed {
                // Closed while the subscription was on its way.
                st.by_key.remove(&k);
                st.owed.push(unsubscribe(chan));
            } else {
                st.by_chan.insert(chan, k);
            }
        }
        "unsubscribed" => {
            if let Some(chan) = f.chan_id {
                if let Some(k) = st.by_chan.remove(&chan) {
                    st.by_key.remove(&k);
                }
            }
        }
        "error" => {
            // A second subscription to a key the socket already carries: the first stands.
            if f.code == Some(10301) {
                return Ok(());
            }
            let Some(sub) = routed.as_ref().and_then(|k| st.by_key.remove(k)) else {
                // A refusal naming no pane is the socket's (an unsubscribe of a channel
                // already gone among them): reported, the stream goes on.
                out.push(Envelope::new(ctx.connector.clone(), None, ctx.received_ts, Event::Error(ConnectorError::new(ErrorKind::InvalidRequest, format!("Bitfinex socket: {msg} ({})", f.code.unwrap_or_default())))));
                return Ok(());
            };
            if sub.closed {
                return Ok(());
            }
            st.by_sub.remove(&sub.sub_id);
            let kind = match f.code {
                Some(10305) => ErrorKind::RateLimited,
                _ => ErrorKind::NotFound,
            };
            out.push(at(sub.sub_id, Event::SubscriptionFail { error: ConnectorError::new(kind, format!("Bitfinex refused `{}`: {msg}", sub.instrument.native)) }));
        }
        "info" => match f.code {
            Some(20051) => return Err(ConnectorError::new(ErrorKind::Transient, "Bitfinex is restarting its socket server: reconnecting")),
            Some(20061) => return Err(ConnectorError::new(ErrorKind::Transient, "Bitfinex maintenance is over: reconnecting to subscribe again")),
            // 20060: maintenance starts, the channels fall quiet until 20061. The opening
            // `info` carries the version and the platform status and nothing to route.
            _ => {}
        },
        // `pong`, `conf`, and anything new: nothing to route.
        _ => {}
    }
    Ok(())
}

/// One book level: its price, how many orders (0 = gone), and the signed amount.
fn level(price: &RawValue, count: i64, amount: &RawValue) -> Option<(Decimal, i64, Decimal)> {
    let d = |r: &RawValue| otw_connector_sdk::parse::dec(r.get().trim_matches('"')).ok();
    Some((d(price)?, count, d(amount)?))
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let mut out: Vec<Envelope> =
        st.refused.drain(..).map(|(sub, error)| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, Event::SubscriptionFail { error })).collect();
    let text = payload.trim_ascii_start();
    if text.starts_with(b"{") {
        decode_event(text, st, ctx, &mut out)?;
        return Ok(out);
    }
    let bad = |e: serde_json::Error| ConnectorError::decode(format!("bitfinex socket: {e}"));
    let frame: Vec<Box<RawValue>> = serde_json::from_slice(text).map_err(bad)?;
    let (Some(chan), Some(data)) = (frame.first(), frame.get(1)) else { return Ok(out) };
    let Ok(chan) = serde_json::from_str::<i64>(chan.get()) else { return Ok(out) };
    let Some(k) = st.by_chan.get(&chan).cloned() else { return Ok(out) };
    let Some((sub_id, instrument, tf)) = st.by_key.get(&k).map(|s| (s.sub_id, s.instrument.clone(), s.tf)) else { return Ok(out) };
    let data = data.get().trim_start();
    // `"hb"` and anything else that is not a list.
    if !data.starts_with('[') {
        return Ok(out);
    }
    let Some(tf) = tf else {
        let shape = || ConnectorError::decode("bitfinex socket: book level shape");
        // `[PRICE, COUNT, AMOUNT]`, or a list of them in the snapshot.
        let levels: Vec<(Decimal, i64, Decimal)> = if data.starts_with("[[") {
            serde_json::from_str::<Vec<(Box<RawValue>, i64, Box<RawValue>)>>(data).map_err(bad)?.into_iter().map(|(p, c, a)| level(&p, c, &a).ok_or_else(shape)).collect::<Result<_, _>>()?
        } else {
            let (p, c, a): (Box<RawValue>, i64, Box<RawValue>) = serde_json::from_str(data).map_err(bad)?;
            vec![level(&p, c, &a).ok_or_else(shape)?]
        };
        let Some(sub) = st.by_key.get_mut(&k) else { return Ok(out) };
        if data.starts_with("[[") {
            sub.top = (None, None);
        }
        for (price, count, amount) in levels {
            let side = if amount.is_sign_negative() { &mut sub.top.1 } else { &mut sub.top.0 };
            *side = (count > 0).then_some((price, amount.abs()));
        }
        // An empty side is no quote: the next level is on its way.
        let (Some((bid, bq)), Some((ask, aq))) = sub.top else { return Ok(out) };
        let q = Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: Some(Qty(bq)), ask_qty: Some(Qty(aq)) };
        out.push(Envelope::new(ctx.connector.clone(), Some(sub_id), ctx.received_ts, Event::Quote(q)));
        return Ok(out);
    };
    let rows: Vec<Box<RawValue>> = serde_json::from_str(data).map_err(bad)?;
    let shape = || ConnectorError::decode("bitfinex socket: candle shape");
    let at = |event: Event| Envelope::new(ctx.connector.clone(), Some(sub_id), ctx.received_ts, event);
    let snapshot = rows.first().is_none_or(|r| r.get().trim_start().starts_with('['));
    if !snapshot {
        let bar = CandleRow::parse(&rows).ok_or_else(shape)?;
        out.push(at(Event::Candle(bar.candle(instrument, tf, false)?)));
        return Ok(out);
    }
    let bars = rows.iter().map(|r| serde_json::from_str::<Vec<Box<RawValue>>>(r.get()).ok().and_then(|r| CandleRow::parse(&r)).ok_or_else(shape)).collect::<Result<Vec<_>, _>>()?;
    // Only the newest bar of the snapshot is live news; the history is read over REST.
    match bars.iter().max_by_key(|b| b.open_ms) {
        Some(newest) => {
            out.push(at(Event::SubscriptionAck));
            out.push(at(Event::Candle(newest.candle(instrument, tf, false)?)));
        }
        None => {
            st.by_chan.remove(&chan);
            st.by_key.remove(&k);
            st.by_sub.remove(&sub_id);
            // The channel stays open on the venue's side until it is unsubscribed.
            st.owed.push(unsubscribe(chan));
            let error = ConnectorError::not_found(format!(
                "Bitfinex has no bar for `{}` at {tf}: check the name (BTCUSD, AVAX:USD, BTCF0:USTF0)",
                instrument.native
            ));
            out.push(at(Event::SubscriptionFail { error }));
        }
    }
    Ok(out)
}
