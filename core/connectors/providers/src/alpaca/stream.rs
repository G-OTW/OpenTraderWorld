//! Alpaca live: one-minute bars on one socket per market (stocks, crypto, options), the
//! larger timeframes folded by the engine.
//!
//! The handshake is staged: Alpaca greets with `success/connected`, accepts `auth` only
//! then, and `subscribe` only after `success/authenticated`. The login frame is the
//! signer's (`WsLogin`); subscriptions that arrive earlier wait for it.
//!
//! The best bid and ask ride the same socket: `{"action":"subscribe","quotes":[..]}`, then
//! `{"T":"q","S","bp","bs","ap","as","t"}` (Alpaca's streaming docs, not measured: no key
//! here). On the free plan the stock feed is IEX, so the quote is IEX's book, not the
//! national best. Stock sizes are round lots and are not carried.
//!
//! The options socket speaks MessagePack only (412 on a JSON connection) and has no `bars`
//! channel (410): the transport transcodes it to JSON (`Mapper::ws_msgpack`), and an
//! option's candles are folded from its prints (`trades`, `{T:"t", S, p, s, t}`), like
//! Coinbase's. Its quotes are the same `q` message as the stocks'.

use std::collections::HashMap;

use otw_connector_model::{Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Price, Qty, Quote, Timeframe, Trade};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{ConnKey, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{market_of, Alpaca, BarRow, Feed, Market, WS};

const CONN: &str = "bars";

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    authed: bool,
    by_sub: HashMap<CorrId, String>,
    by_symbol: HashMap<String, (CorrId, InstrumentId, Timeframe)>,
    /// Symbols waiting for the handshake.
    pending: Vec<String>,
    /// Quote subscriptions, by symbol, and those waiting for the handshake.
    by_quote: HashMap<String, (CorrId, InstrumentId)>,
    quote_by_sub: HashMap<CorrId, String>,
    pending_quotes: Vec<String>,
    /// Trade subscriptions (options), by symbol and by sub id.
    by_trade: HashMap<String, (CorrId, InstrumentId)>,
    trade_by_sub: HashMap<CorrId, String>,
    pending_trades: Vec<String>,
    /// Panes refused before anything went out, reported on the next frame: a refusal here
    /// must fail that pane, never the socket the others ride.
    refused: Vec<(CorrId, ConnectorError)>,
    /// Frames owed to the venue, drained by the transport.
    queued: Vec<NativeRequest>,
}

fn frame(action: &str, symbols: &[String]) -> NativeRequest {
    channel_frame(action, "bars", symbols)
}

fn channel_frame(action: &str, channel: &str, symbols: &[String]) -> NativeRequest {
    let payload = serde_json::json!({ "action": action, channel: symbols });
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.to_string().into(), corr_id: None }
}

fn path(market: Market, feed: Feed) -> &'static str {
    match (market, feed) {
        (Market::Crypto, _) => "/v1beta3/crypto/us",
        (Market::Options, Feed::Iex) => "/v1beta1/indicative",
        (Market::Options, Feed::Sip) => "/v1beta1/opra",
        (Market::Stocks, Feed::Iex) => "/v2/iex",
        (Market::Stocks, Feed::Sip) => "/v2/sip",
    }
}

pub fn encode(m: &Alpaca, cmds: &[Command], st: &mut LiveState) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut out = Vec::new();
    let mut add: Vec<String> = Vec::new();
    let mut drop: Vec<String> = Vec::new();
    let mut add_q: Vec<String> = Vec::new();
    let mut drop_q: Vec<String> = Vec::new();
    let mut add_t: Vec<String> = Vec::new();
    let mut drop_t: Vec<String> = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                if !m.capabilities().live_timeframes().contains(tf) {
                    return Err(ConnectorError::unsupported(format!("Alpaca streams one-minute bars, not {tf}")));
                }
                if !st.opened {
                    out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: path(market_of(instrument), m.feed).into() });
                    st.opened = true;
                }
                let symbol = instrument.native.clone();
                st.by_sub.insert(*sub_id, symbol.clone());
                st.by_symbol.insert(symbol.clone(), (*sub_id, instrument.clone(), *tf));
                if !add.contains(&symbol) {
                    add.push(symbol);
                }
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                if !st.opened {
                    out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: path(market_of(instrument), m.feed).into() });
                    st.opened = true;
                }
                let symbol = instrument.native.clone();
                st.quote_by_sub.insert(*sub_id, symbol.clone());
                st.by_quote.insert(symbol.clone(), (*sub_id, instrument.clone()));
                if !add_q.contains(&symbol) {
                    add_q.push(symbol);
                }
            }
            // Options stream prints, not bars: their candles fold from these.
            Command::Subscribe { sub_id, instrument, data_type: DataType::Trades } if market_of(instrument) == Market::Options => {
                if !st.opened {
                    out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: path(market_of(instrument), m.feed).into() });
                    st.opened = true;
                }
                let symbol = instrument.native.clone();
                st.trade_by_sub.insert(*sub_id, symbol.clone());
                st.by_trade.insert(symbol.clone(), (*sub_id, instrument.clone()));
                if !add_t.contains(&symbol) {
                    add_t.push(symbol);
                }
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Alpaca streams bars, option prints and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(symbol) = st.by_sub.remove(sub_id) {
                    st.by_symbol.remove(&symbol);
                    st.pending.retain(|p| *p != symbol);
                    drop.push(symbol);
                }
                if let Some(symbol) = st.quote_by_sub.remove(sub_id) {
                    st.by_quote.remove(&symbol);
                    st.pending_quotes.retain(|p| *p != symbol);
                    drop_q.push(symbol);
                }
                if let Some(symbol) = st.trade_by_sub.remove(sub_id) {
                    st.by_trade.remove(&symbol);
                    st.pending_trades.retain(|p| *p != symbol);
                    drop_t.push(symbol);
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
            out.push(channel_frame("subscribe", "quotes", &add_q));
        } else {
            st.pending_quotes.extend(add_q);
        }
    }
    if !drop_q.is_empty() && st.authed {
        out.push(channel_frame("unsubscribe", "quotes", &drop_q));
    }
    if !add_t.is_empty() {
        if st.authed {
            out.push(channel_frame("subscribe", "trades", &add_t));
        } else {
            st.pending_trades.extend(add_t);
        }
    }
    if !drop_t.is_empty() && st.authed {
        out.push(channel_frame("unsubscribe", "trades", &drop_t));
    }
    Ok(out)
}

/// Frames the handshake owes the venue.
pub fn drain(st: &mut LiveState) -> Vec<NativeRequest> {
    std::mem::take(&mut st.queued)
}

#[derive(Deserialize)]
struct Item {
    #[serde(rename = "T", default)]
    kind: String,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    code: Option<i64>,
    #[serde(rename = "S", default)]
    symbol: Option<String>,
}

/// `{"T":"t","S":"SPY251219C00650000","p":9.5,"s":1,"t":"..","x":"C","c":"I"}`.
#[derive(Deserialize)]
struct TradeRow {
    #[serde(default)]
    p: OptDec,
    #[serde(default)]
    s: OptDec,
    #[serde(default)]
    t: Option<String>,
}

/// `{"T":"q","S":"AMD","bp":87.66,"bs":1,"ap":87.68,"as":4,"t":"2021-02-22T15:51:45.335689322Z",..}`.
#[derive(Deserialize)]
struct QuoteRow {
    #[serde(default)]
    bp: OptDec,
    #[serde(default)]
    bs: OptDec,
    #[serde(default)]
    ap: OptDec,
    #[serde(default, rename = "as")]
    r#as: OptDec,
    #[serde(default)]
    t: Option<String>,
}

/// Alpaca's numbered socket error, as the fix it calls for: 402 is a key to correct, 409 a
/// plan to upgrade, 406 another program holding the one connection the free plan allows.
fn refusal(code: i64, msg: &str, feed: Feed) -> ConnectorError {
    let (kind, message) = match code {
        401 | 404 => (ErrorKind::Auth, format!("Alpaca did not accept the login ({msg})")),
        402 => (ErrorKind::Auth, "Alpaca rejected the API key. Check the key and secret, and that they are live keys rather than paper ones".to_string()),
        403 => (ErrorKind::Transient, format!("Alpaca: {msg}")),
        405 => (ErrorKind::Unsupported, format!("Alpaca refused the subscription: {msg}")),
        406 => (
            ErrorKind::RateLimited,
            "Alpaca allows one live connection per account and it is already in use. Close the other program (or browser tab) using this key, and this reconnects on its own".to_string(),
        ),
        408 => (ErrorKind::Permission, "this Alpaca account has no market-data streaming enabled".to_string()),
        409 => (
            ErrorKind::Permission,
            format!("this Alpaca plan does not include the {} feed. Set the connector's Feed setting to iex, or upgrade the plan", feed.label()),
        ),
        410 => (ErrorKind::Unsupported, format!("Alpaca's {} feed does not carry this instrument", feed.label())),
        _ => (ErrorKind::Transient, format!("Alpaca error {code}: {msg}")),
    };
    ConnectorError::new(kind, message).with_code(code.to_string())
}

pub fn decode(m: &Alpaca, payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    // Every frame is an array, control messages included.
    let raw: Box<RawValue> = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("alpaca socket: {e}")))?;
    let items: Vec<Box<RawValue>> = if raw.get().trim_start().starts_with('[') {
        serde_json::from_str(raw.get()).map_err(|e| ConnectorError::decode(format!("alpaca socket: {e}")))?
    } else {
        vec![raw]
    };
    let mut out: Vec<Envelope> =
        st.refused.drain(..).map(|(sub, error)| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, Event::SubscriptionFail { error })).collect();
    for raw in items {
        let it: Item = serde_json::from_str(raw.get()).map_err(|e| ConnectorError::decode(format!("alpaca socket: {e}")))?;
        match it.kind.as_str() {
            "success" => match it.msg.as_deref() {
                Some("connected") => st.queued.push(NativeRequest::WsLogin { conn_key: ConnKey(CONN.into()) }),
                Some("authenticated") => {
                    st.authed = true;
                    let waiting = std::mem::take(&mut st.pending);
                    if !waiting.is_empty() {
                        st.queued.push(frame("subscribe", &waiting));
                    }
                    let waiting = std::mem::take(&mut st.pending_trades);
                    if !waiting.is_empty() {
                        st.queued.push(channel_frame("subscribe", "trades", &waiting));
                    }
                    let waiting = std::mem::take(&mut st.pending_quotes);
                    if !waiting.is_empty() {
                        st.queued.push(channel_frame("subscribe", "quotes", &waiting));
                    }
                }
                _ => {}
            },
            "error" => return Err(refusal(it.code.unwrap_or(0), it.msg.as_deref().unwrap_or(""), m.feed)),
            "b" => {
                let Some(symbol) = it.symbol else { return Err(ConnectorError::decode("alpaca bar: no symbol")) };
                let Some((sub, instrument, tf)) = st.by_symbol.get(&symbol) else { continue };
                let row: BarRow = serde_json::from_str(raw.get()).map_err(|e| super::decode_err("socket bar", e))?;
                // A published minute bar is final; the engine folds the larger timeframes.
                let candle = row.candle(instrument.clone(), *tf, true)?;
                out.push(Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::Candle(candle)));
            }
            "t" => {
                let Some(symbol) = it.symbol else { return Err(ConnectorError::decode("alpaca trade: no symbol")) };
                let Some((sub, instrument)) = st.by_trade.get(&symbol) else { continue };
                let t: TradeRow = serde_json::from_str(raw.get()).map_err(|e| super::decode_err("socket trade", e))?;
                let Some(price) = t.p.0.filter(|p| p.is_sign_positive() && !p.is_zero()) else { continue };
                let trade = Trade { instrument: instrument.clone(), id: None, price: Price(price), qty: Qty(t.s.0.unwrap_or_default()), aggressor: None };
                let e = Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::Trade(trade));
                out.push(match t.t.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
                    Some(t) => e.at(t),
                    None => e,
                });
            }
            "q" => {
                let Some(symbol) = it.symbol else { return Err(ConnectorError::decode("alpaca quote: no symbol")) };
                let Some((sub, instrument)) = st.by_quote.get(&symbol) else { continue };
                let q: QuoteRow = serde_json::from_str(raw.get()).map_err(|e| super::decode_err("socket quote", e))?;
                let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
                // An empty side is no quote.
                let (Some(bid), Some(ask)) = (positive(q.bp), positive(q.ap)) else { continue };
                // Stock sizes are round lots, not shares: only crypto's are carried.
                let sized = market_of(instrument) == Market::Crypto;
                let size = |d: OptDec| d.0.filter(|_| sized).map(Qty);
                let quote = Quote { instrument: instrument.clone(), bid: Price(bid), ask: Price(ask), bid_qty: size(q.bs), ask_qty: size(q.r#as) };
                let e = Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::Quote(quote));
                out.push(match q.t.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
                    Some(t) => e.at(t),
                    None => e,
                });
            }
            // `u` corrects a bar already published, `d` is the daily bar: neither moves a
            // candle the chart has closed; the REST seed reconciles. Ignored on purpose.
            _ => {}
        }
    }
    Ok(out)
}
