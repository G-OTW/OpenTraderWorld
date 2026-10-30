//! Massive live: minute aggregates on one socket per market cluster
//! (`/stocks`, `/options`, `/crypto`, `/forex`, `/indices`, `/futures`), the larger timeframes
//! folded by the engine. Live is a paid Massive feature: a free key opens the socket and is
//! refused at login.
//!
//! The handshake is staged: Massive greets with `status: connected`, accepts `auth` only
//! then, and `subscribe` only after `auth_success`. The login frame is the signer's
//! (`WsLogin`); subscriptions that arrive earlier wait for it.
//!
//! The channels differ from the REST tickers: `AM.<ticker>` for stocks, options, indices and
//! futures, `XA.BTC-USD` for crypto and `CA.EUR/USD` for forex, where REST glues the pair
//! (`X:BTCUSD`, `C:EURUSD`). [`channel`] is the one place that translation lives.
//!
//! The best bid and ask: `Q.<ticker>` on stocks, options and futures, `XQ.BTC-USD` on
//! crypto, `C.EUR/USD` on forex (the aggregate channel's spelling; indices have no quote).
//! Quotes are a separate plan (stocks: Advanced and up), and the socket is the only one the
//! key gets per market, so **a refusal must never end it**: quote channels go out in their
//! own frame and stay "unconfirmed" until the venue's `success` names them; an `error` that
//! names a quote channel, or that arrives while a quote channel is unconfirmed, fails those
//! quote panes and the socket carries on. Massive does not document the wording of a
//! refused channel, so this is matched on what can be known: the channel name or the timing.

use std::collections::{HashMap, HashSet};

use otw_connector_model::{Candle, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Price, Qty, Quote, Timeframe};
use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;

use super::{market_of, Market, Massive, WS};

const CONN: &str = "aggs";

/// Quote currencies a glued crypto pair can end in, longest first so `BTCUSDT` splits at
/// `USDT` rather than `USD`.
const CRYPTO_QUOTES: &[&str] = &["USDT", "USDC", "TUSD", "BUSD", "USD", "EUR", "GBP", "JPY", "BTC", "ETH"];

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    authed: bool,
    by_sub: HashMap<CorrId, String>,
    by_channel: HashMap<String, (CorrId, InstrumentId, Timeframe)>,
    /// Channels waiting for the handshake.
    pending: Vec<String>,
    /// Quote panes by channel, and by sub id.
    by_quote: HashMap<String, (CorrId, InstrumentId)>,
    quote_by_sub: HashMap<CorrId, String>,
    /// Quote channels waiting for the handshake, and those sent but not confirmed yet.
    pending_quotes: Vec<String>,
    unconfirmed: HashSet<String>,
    /// Panes refused before anything went out, reported on the next frame.
    refused: Vec<(CorrId, ConnectorError)>,
    /// Frames owed to the venue, drained by the transport.
    queued: Vec<NativeRequest>,
}

/// The socket channel of one instrument.
pub(crate) fn channel(id: &InstrumentId) -> String {
    let n = id.native.as_str();
    match market_of(id) {
        Market::Crypto => format!("XA.{}", crypto_pair(n.strip_prefix("X:").unwrap_or(n))),
        Market::Forex => format!("CA.{}", fx_pair(n.strip_prefix("C:").unwrap_or(n))),
        Market::Stocks | Market::Options | Market::Indices | Market::Futures => format!("AM.{n}"),
    }
}

/// The quote channel of one instrument; `None` where the cluster publishes no quote.
pub(crate) fn quote_channel(id: &InstrumentId) -> Option<String> {
    let n = id.native.as_str();
    match market_of(id) {
        Market::Crypto => Some(format!("XQ.{}", crypto_pair(n.strip_prefix("X:").unwrap_or(n)))),
        Market::Forex => Some(format!("C.{}", fx_pair(n.strip_prefix("C:").unwrap_or(n)))),
        Market::Stocks | Market::Options | Market::Futures => Some(format!("Q.{n}")),
        Market::Indices => None,
    }
}

/// `BTCUSD` to `BTC-USD`; a pair already carrying a separator keeps it.
fn crypto_pair(raw: &str) -> String {
    if raw.contains('-') || raw.contains('/') {
        return raw.replace('/', "-");
    }
    for q in CRYPTO_QUOTES {
        if let Some(base) = raw.strip_suffix(q).filter(|b| !b.is_empty()) {
            return format!("{base}-{q}");
        }
    }
    raw.to_string()
}

/// `EURUSD` to `EUR/USD`; a pair already carrying a separator keeps it.
fn fx_pair(raw: &str) -> String {
    if raw.contains('/') || raw.contains('-') {
        return raw.replace('-', "/");
    }
    match (raw.get(..3), raw.get(3..)) {
        (Some(base), Some(quote)) if raw.len() == 6 => format!("{base}/{quote}"),
        _ => raw.to_string(),
    }
}

fn frame(action: &str, channels: &[String]) -> NativeRequest {
    let payload = serde_json::json!({ "action": action, "params": channels.join(",") });
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.to_string().into(), corr_id: None }
}

pub fn encode(m: &Massive, cmds: &[Command], st: &mut LiveState) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut out = Vec::new();
    let mut add: Vec<String> = Vec::new();
    let mut drop: Vec<String> = Vec::new();
    let mut add_q: Vec<String> = Vec::new();
    let mut drop_q: Vec<String> = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                if !m.capabilities().live_timeframes().contains(tf) {
                    return Err(ConnectorError::unsupported(format!("Massive streams minute aggregates, not {tf}")));
                }
                if !st.opened {
                    let path = format!("/{}", market_of(instrument).cluster());
                    out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path });
                    st.opened = true;
                }
                let ch = channel(instrument);
                st.by_sub.insert(*sub_id, ch.clone());
                st.by_channel.insert(ch.clone(), (*sub_id, instrument.clone(), *tf));
                if !add.contains(&ch) {
                    add.push(ch);
                }
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let Some(ch) = quote_channel(instrument) else {
                    st.refused.push((*sub_id, ConnectorError::unsupported("Massive publishes no bid and ask for an index")));
                    continue;
                };
                if !st.opened {
                    let path = format!("/{}", market_of(instrument).cluster());
                    out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path });
                    st.opened = true;
                }
                st.quote_by_sub.insert(*sub_id, ch.clone());
                st.by_quote.insert(ch.clone(), (*sub_id, instrument.clone()));
                if !add_q.contains(&ch) {
                    add_q.push(ch);
                }
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Massive streams aggregates and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(ch) = st.by_sub.remove(sub_id) {
                    st.by_channel.remove(&ch);
                    st.pending.retain(|p| *p != ch);
                    drop.push(ch);
                }
                if let Some(ch) = st.quote_by_sub.remove(sub_id) {
                    st.by_quote.remove(&ch);
                    st.pending_quotes.retain(|p| *p != ch);
                    st.unconfirmed.remove(&ch);
                    drop_q.push(ch);
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    if !add.is_empty() {
        if st.authed {
            out.push(frame("subscribe", &add));
        } else {
            st.pending.extend(add);
        }
    }
    if !drop.is_empty() && st.authed {
        out.push(frame("unsubscribe", &drop));
    }
    if !add_q.is_empty() {
        if st.authed {
            out.push(quote_frame(st, add_q));
        } else {
            st.pending_quotes.extend(add_q);
        }
    }
    if !drop_q.is_empty() && st.authed {
        out.push(frame("unsubscribe", &drop_q));
    }
    Ok(out)
}

/// Quote channels go out in a frame of their own and wait for their confirmation.
fn quote_frame(st: &mut LiveState, channels: Vec<String>) -> NativeRequest {
    let f = frame("subscribe", &channels);
    st.unconfirmed.extend(channels);
    f
}

/// The quote panes an `error` is about: the channels it names, else the ones still
/// unconfirmed (the refusal answers the last frame sent). Empty when it concerns no quote.
fn refused_quotes(st: &LiveState, msg: &str) -> Vec<String> {
    let named: Vec<String> = st.by_quote.keys().filter(|ch| msg.contains(ch.as_str())).cloned().collect();
    if !named.is_empty() {
        return named;
    }
    st.unconfirmed.iter().cloned().collect()
}

/// Frames the handshake owes the venue.
pub fn drain(st: &mut LiveState) -> Vec<NativeRequest> {
    std::mem::take(&mut st.queued)
}

#[derive(Deserialize)]
struct Item {
    #[serde(default)]
    ev: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<String>,
    /// `sym` on stocks, options, indices and futures; `pair` on crypto and forex.
    #[serde(default)]
    sym: Option<String>,
    #[serde(default)]
    pair: Option<String>,
    #[serde(default)]
    s: Option<Int>,
    #[serde(default)]
    o: OptDec,
    #[serde(default)]
    h: OptDec,
    #[serde(default)]
    l: OptDec,
    #[serde(default)]
    c: OptDec,
    #[serde(default)]
    v: OptDec,
    /// Quotes: `bp`/`ap` (stocks, options, futures, crypto), `b`/`a` and the pair in `p`
    /// (forex), sizes `bs`/`as`, time `t` in ms.
    #[serde(default)]
    bp: OptDec,
    #[serde(default)]
    ap: OptDec,
    #[serde(default)]
    bs: OptDec,
    #[serde(default, rename = "as")]
    r#as: OptDec,
    #[serde(default)]
    b: OptDec,
    #[serde(default)]
    a: OptDec,
    #[serde(default)]
    p: Option<String>,
    #[serde(default)]
    t: Option<Int>,
}

/// A refusal in the words of the fix it calls for. The free key is the common case and it is
/// not a typo: Massive sells the socket separately from the REST history.
fn refusal(status: &str, msg: &str) -> Option<ConnectorError> {
    let (kind, message) = match status {
        "auth_failed" => (
            ErrorKind::Permission,
            format!(
                "Massive refused the live connection. Live streaming is a paid Massive feature and is not part of the free (end-of-day) plan; the same key still downloads history ({msg})"
            ),
        ),
        "auth_timeout" => (ErrorKind::Auth, format!("Massive did not accept the login in time ({msg})")),
        "max_connections" => (ErrorKind::RateLimited, max_connections()),
        "error" => {
            let low = msg.to_ascii_lowercase();
            if low.contains("maximum number of websocket connections") || low.contains("max connection") {
                (ErrorKind::RateLimited, max_connections())
            } else if low.contains("not authorized") || low.contains("insufficient") || low.contains("upgrade") {
                (ErrorKind::Permission, format!("this Massive plan does not cover this live feed ({msg})"))
            } else if low.contains("unknown ticker") || low.contains("invalid ticker") {
                (ErrorKind::NotFound, format!("Massive does not know one of the subscribed symbols ({msg})"))
            } else {
                (ErrorKind::Transient, format!("Massive: {msg}"))
            }
        }
        _ => return None,
    };
    Some(ConnectorError::new(kind, message).with_code(status.to_string()))
}

fn max_connections() -> String {
    "Massive allows one live connection per market and this key is already using it. Close the other program on this key and this reconnects on its own".into()
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let err = |e: serde_json::Error| ConnectorError::decode(format!("massive socket: {e}"));
    let items: Vec<Item> = if payload.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') {
        serde_json::from_slice(payload).map_err(err)?
    } else {
        vec![serde_json::from_slice(payload).map_err(err)?]
    };
    let mut out: Vec<Envelope> =
        st.refused.drain(..).map(|(sub, error)| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, Event::SubscriptionFail { error })).collect();
    for it in items {
        match it.ev.as_str() {
            "status" => {
                let status = it.status.as_deref().unwrap_or("");
                match status {
                    "connected" => st.queued.push(NativeRequest::WsLogin { conn_key: ConnKey(CONN.into()) }),
                    "auth_success" => {
                        st.authed = true;
                        let waiting = std::mem::take(&mut st.pending);
                        if !waiting.is_empty() {
                            st.queued.push(frame("subscribe", &waiting));
                        }
                        let waiting = std::mem::take(&mut st.pending_quotes);
                        if !waiting.is_empty() {
                            let f = quote_frame(st, waiting);
                            st.queued.push(f);
                        }
                    }
                    "success" => {
                        // `subscribed to: Q.MSFT`: that quote channel is the venue's now.
                        let msg = it.message.as_deref().unwrap_or("");
                        let confirmed: Vec<String> = st.unconfirmed.iter().filter(|ch| msg.contains(ch.as_str())).cloned().collect();
                        for ch in confirmed {
                            st.unconfirmed.remove(&ch);
                            if let Some((sub, _)) = st.by_quote.get(&ch) {
                                out.push(Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::SubscriptionAck));
                            }
                        }
                    }
                    "error" if !refused_quotes(st, it.message.as_deref().unwrap_or("")).is_empty() => {
                        // A refused quote fails its own panes: the bars on this socket stay.
                        let msg = it.message.as_deref().unwrap_or("");
                        for ch in refused_quotes(st, msg) {
                            st.unconfirmed.remove(&ch);
                            let Some((sub, _)) = st.by_quote.remove(&ch) else { continue };
                            st.quote_by_sub.remove(&sub);
                            let error = ConnectorError::new(
                                ErrorKind::Permission,
                                format!("Massive refused the bid and ask ({msg}): quotes are a separate Massive plan (stocks: Advanced and up)"),
                            );
                            out.push(Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, Event::SubscriptionFail { error }));
                        }
                    }
                    _ => {
                        if let Some(e) = refusal(status, it.message.as_deref().unwrap_or("")) {
                            return Err(e);
                        }
                    }
                }
            }
            ev @ ("Q" | "XQ" | "C") => {
                // The forex pair is in `p`, crypto's in `pair`, the rest in `sym`.
                let Some(name) = it.sym.or(it.pair).or(it.p) else { return Err(ConnectorError::decode("massive quote: no symbol")) };
                let Some((sub, instrument)) = st.by_quote.get(&format!("{ev}.{name}")) else { continue };
                // Any quote at all confirms its channel.
                st.unconfirmed.remove(&format!("{ev}.{name}"));
                let (bid, ask) = if ev == "C" { (it.b, it.a) } else { (it.bp, it.ap) };
                let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
                // A one-sided or empty market omits a side: no quote.
                let (Some(bid), Some(ask)) = (positive(bid), positive(ask)) else { continue };
                // Shares and coins are carried; contracts and forex (no size) are not.
                let sized = matches!(market_of(instrument), Market::Stocks | Market::Crypto);
                let size = |d: OptDec| d.0.filter(|_| sized).map(Qty);
                let q = Quote { instrument: instrument.clone(), bid: Price(bid), ask: Price(ask), bid_qty: size(it.bs), ask_qty: size(it.r#as) };
                let e = Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::Quote(q));
                out.push(match it.t.map(|t| ts_millis(t.0)).transpose()? {
                    Some(t) => e.at(t),
                    None => e,
                });
            }
            ev @ ("AM" | "XA" | "CA") => {
                let Some(name) = it.sym.or(it.pair) else { return Err(ConnectorError::decode("massive aggregate: no symbol")) };
                let Some((sub, instrument, tf)) = st.by_channel.get(&format!("{ev}.{name}")) else { continue };
                let Some(start) = it.s else { return Err(ConnectorError::decode("massive aggregate: no start")) };
                let p = |d: OptDec| Price(d.0.unwrap_or_default());
                let candle = Candle {
                    instrument: instrument.clone(),
                    tf: *tf,
                    open_ts: ts_millis(start.0)?,
                    open: p(it.o),
                    high: p(it.h),
                    low: p(it.l),
                    close: p(it.c),
                    volume: Qty(it.v.0.unwrap_or_default()),
                    quote_volume: None,
                    trades: None,
                    // A published minute aggregate is final; the engine folds the larger ones.
                    is_final: true,
                    adjusted: None,
                    sides: None,
                };
                out.push(Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::Candle(candle)));
            }
            _ => {}
        }
    }
    Ok(out)
}
