//! Kraken v2 live candles (`wss://ws.kraken.com/v2`, channel `ohlc`).
//!
//! The first message is a snapshot of recent candles, then updates of the forming one.
//! Kraken sends no closed flag: every candle goes out forming (`candles_final_flag = false`
//! in the capabilities), and the engine closes it when the next one starts.
//!
//! One socket per interval (the adapter's `socket_class`), as before the port: the
//! subscription frame takes one interval, so a socket's rows are routed on their symbol. A
//! row naming another interval (it carries `interval`) is not this socket's.
//!
//! The best bid and ask are a socket of their own (class `""`): channel `ticker` with
//! `event_trigger: "bbo"`, a snapshot then an update on every change of the best bid or ask
//! (`bid`, `ask`, `bid_qty`, `ask_qty`, `timestamp`). An unknown pair is refused by name.

use std::collections::HashMap;

use otw_connector_model::{Candle, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, InstrumentId, Qty, Quote, Timeframe};
use otw_connector_sdk::parse::{ts_rfc3339, Dec, OptDec};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, Mapper, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{interval, Kraken, PairName, WS};

const CONN: &str = "ohlc";

struct Sub {
    sub_id: CorrId,
    instrument: InstrumentId,
    /// `None`: the best bid and ask.
    tf: Option<Timeframe>,
}

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    /// The socket's interval in minutes, set by its first subscription.
    interval: Option<u32>,
    /// A best bid/ask socket, set by its first subscription.
    quotes: bool,
    by_sub: HashMap<CorrId, String>,
    by_symbol: HashMap<String, Sub>,
}

fn frame(method: &str, symbols: &[String], minutes: Option<u32>) -> NativeRequest {
    let payload = match minutes {
        Some(minutes) => serde_json::json!({
            "method": method,
            "params": { "channel": "ohlc", "symbol": symbols, "interval": minutes },
        }),
        None => serde_json::json!({
            "method": method,
            "params": { "channel": "ticker", "symbol": symbols, "event_trigger": "bbo" },
        }),
    };
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.to_string().into(), corr_id: None }
}

pub fn encode(m: &Kraken, cmds: &[Command], st: &mut LiveState, pairs: &HashMap<String, PairName>, instruments: &InstrumentCache) -> Result<Vec<NativeRequest>, ConnectorError> {
    let mut add = Vec::new();
    let mut drop = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Candles { tf } } => {
                let minutes = interval(*tf)
                    .filter(|_| m.capabilities().live_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("Kraken has no live candle channel for {tf}")))?;
                if st.quotes || st.interval.is_some_and(|i| i != minutes) {
                    return Err(ConnectorError::invalid("a Kraken candle socket carries one interval"));
                }
                st.interval = Some(minutes);
                let symbol = ws_symbol(instrument, pairs, instruments)?;
                st.by_sub.insert(*sub_id, symbol.clone());
                st.by_symbol.insert(symbol.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: Some(*tf) });
                add.push(symbol);
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                if st.interval.is_some() {
                    return Err(ConnectorError::invalid("a Kraken candle socket carries no quotes"));
                }
                st.quotes = true;
                let symbol = ws_symbol(instrument, pairs, instruments)?;
                st.by_sub.insert(*sub_id, symbol.clone());
                st.by_symbol.insert(symbol.clone(), Sub { sub_id: *sub_id, instrument: instrument.clone(), tf: None });
                add.push(symbol);
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Kraken streams candles and quotes only here, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(symbol) = st.by_sub.remove(sub_id) {
                    st.by_symbol.remove(&symbol);
                    drop.push(symbol);
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if st.interval.is_none() && !st.quotes {
        return Ok(out);
    }
    let minutes = st.interval;
    if !st.opened && !add.is_empty() {
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: "/v2".into() });
        st.opened = true;
    }
    if !add.is_empty() {
        out.push(frame("subscribe", &add, minutes));
    }
    if !drop.is_empty() {
        out.push(frame("unsubscribe", &drop, minutes));
    }
    Ok(out)
}

/// Kraken v2 wants `wsname` (SOL/USD), while REST instruments use `altname` (SOLUSD).
fn ws_symbol(instrument: &InstrumentId, pairs: &HashMap<String, PairName>, instruments: &InstrumentCache) -> Result<String, ConnectorError> {
    let native = instrument.native.to_ascii_uppercase();
    match pairs.get(&native).map(|p| p.wsname.as_str()).or_else(|| instruments.get(instrument).map(|spec| spec.symbol.0.as_str())) {
        Some(wsname) if wsname.contains('/') => Ok(wsname.to_ascii_uppercase()),
        Some(_) => Err(ConnectorError::not_found(format!("Kraken live: pair `{}` has no slash-separated websocket name", instrument.native))),
        None if native.contains('/') => Ok(native),
        None => Err(ConnectorError::not_found(format!("Kraken live: no pair metadata for `{}`; load Kraken instruments before subscribing", instrument.native))),
    }
}

/// One `ticker` row: of it, the best bid and ask.
#[derive(Deserialize)]
struct TickerRow {
    symbol: String,
    #[serde(default)]
    bid: OptDec,
    #[serde(default)]
    ask: OptDec,
    #[serde(default)]
    bid_qty: OptDec,
    #[serde(default)]
    ask_qty: OptDec,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct Row {
    symbol: String,
    open: Dec,
    high: Dec,
    low: Dec,
    close: Dec,
    volume: Dec,
    #[serde(default)]
    trades: Option<u64>,
    interval_begin: String,
    #[serde(default)]
    interval: Option<u32>,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct Ack {
    #[serde(default)]
    symbol: Option<String>,
}

#[derive(Deserialize)]
struct Frame {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    data: Option<Vec<Box<RawValue>>>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    result: Option<Ack>,
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let f: Frame = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("kraken socket: {e}")))?;
    let envelope = |sub: CorrId, event: Event| Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, event);
    if f.method.as_deref() == Some("subscribe") {
        let symbol = f.symbol.or_else(|| f.result.and_then(|r| r.symbol)).map(|s| s.to_ascii_uppercase());
        let sub = symbol.as_ref().and_then(|s| st.by_symbol.get(s)).map(|s| s.sub_id);
        if f.success == Some(false) {
            let msg = f.error.unwrap_or_else(|| "subscription refused".into());
            return match (sub, symbol) {
                (Some(sub), _) => Ok(vec![envelope(
                    sub,
                    Event::SubscriptionFail { error: ConnectorError::new(ErrorKind::NotFound, format!("Kraken refused this pair: {msg}")) },
                )]),
                // Refused for a symbol this socket does not carry (any more): nobody to tell.
                (None, Some(_)) => Ok(vec![]),
                // A refusal naming nothing is the socket's, not one pane's.
                (None, None) => Err(ConnectorError::new(ErrorKind::Transient, format!("Kraken: {msg}"))),
            };
        }
        return Ok(sub.map(|s| vec![envelope(s, Event::SubscriptionAck)]).unwrap_or_default());
    }
    if f.channel.as_deref() == Some("ticker") {
        let mut out = Vec::new();
        for v in f.data.unwrap_or_default() {
            let r: TickerRow = serde_json::from_str(v.get()).map_err(|e| ConnectorError::decode(format!("kraken ticker row: {e}")))?;
            let Some(sub) = st.by_symbol.get(&r.symbol.to_ascii_uppercase()).filter(|s| s.tf.is_none()) else { continue };
            let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
            // An empty side is no quote.
            let (Some(bid), Some(ask)) = (positive(r.bid), positive(r.ask)) else { continue };
            let q = Quote { instrument: sub.instrument.clone(), bid: otw_connector_model::Price(bid), ask: otw_connector_model::Price(ask), bid_qty: r.bid_qty.0.map(Qty), ask_qty: r.ask_qty.0.map(Qty) };
            let env = envelope(sub.sub_id, Event::Quote(q));
            out.push(match r.timestamp.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
                Some(t) => env.at(t),
                None => env,
            });
        }
        return Ok(out);
    }
    // Only candle frames matter; heartbeats and status are ignored.
    if f.channel.as_deref() != Some("ohlc") {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for v in f.data.unwrap_or_default() {
        let r: Row = serde_json::from_str(v.get()).map_err(|e| ConnectorError::decode(format!("kraken ohlc row: {e}")))?;
        if r.interval.is_some() && r.interval != st.interval {
            continue;
        }
        let symbol = r.symbol.to_ascii_uppercase();
        let Some(sub) = st.by_symbol.get(&symbol) else { continue };
        let Some(tf) = sub.tf else { continue };
        let candle = Candle {
            instrument: sub.instrument.clone(),
            tf,
            open_ts: ts_rfc3339(&r.interval_begin)?,
            open: r.open.price(),
            high: r.high.price(),
            low: r.low.price(),
            close: r.close.price(),
            volume: r.volume.qty(),
            quote_volume: None,
            trades: r.trades,
            is_final: false,
            adjusted: None,
            sides: None,
        };
        let env = envelope(sub.sub_id, Event::Candle(candle));
        out.push(match r.timestamp.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
            Some(t) => env.at(t),
            None => env,
        });
    }
    Ok(out)
}
