//! Coinbase live trades on the public Exchange feed (`wss://ws-feed.exchange.coinbase.com`,
//! channel `matches`). Coinbase streams trades, not candles: each one is handed up with its
//! product and OTW folds them into whatever timeframes are watching it, so one channel serves
//! every pane on a product. Keyless.
//!
//! The best bid and ask come from the book: channel `level2_batch` (public, answered as
//! `level2_50`, measured), a `snapshot` of the whole book (about 1 MB on BTC-USD) then
//! `l2update` changes every 50 ms, a size of 0 removing the level. The top of that book is
//! the quote. The `ticker` channel carries a best bid and ask too, but only when a trade
//! prints, so it is not used.

use std::collections::{BTreeMap, HashMap};

use otw_connector_model::{Aggressor, Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event, InstrumentId, Mapped, Price, Qty, Quote, Trade};
use otw_connector_sdk::parse::{ts_rfc3339, Dec};
use otw_connector_sdk::{ConnKey, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Coinbase, WS};

const CONN: &str = "feed";

#[derive(Default)]
pub struct LiveState {
    opened: bool,
    by_sub: HashMap<CorrId, String>,
    by_product: HashMap<String, (CorrId, InstrumentId)>,
    /// Quote subscriptions, by sub id to product.
    book_by_sub: HashMap<CorrId, String>,
    books: HashMap<String, Book>,
}

/// One product's book, price to size per side.
struct Book {
    sub_id: CorrId,
    instrument: InstrumentId,
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

const BOOK_CHANNEL: &str = "level2_batch";

fn frame(action: &str, products: &[String]) -> NativeRequest {
    channel_frame(action, products, "matches")
}

fn channel_frame(action: &str, products: &[String], channel: &str) -> NativeRequest {
    let payload = serde_json::json!({ "type": action, "product_ids": products, "channels": [channel] });
    NativeRequest::WsSend { conn_key: ConnKey(CONN.into()), payload: payload.to_string().into(), corr_id: None }
}

pub fn encode(m: &Coinbase, cmds: &[Command], st: &mut LiveState) -> Result<Vec<NativeRequest>, ConnectorError> {
    use otw_connector_sdk::Mapper as _;
    let mut add = Vec::new();
    let mut drop = Vec::new();
    let mut add_books = Vec::new();
    let mut drop_books = Vec::new();
    for c in cmds {
        match c {
            Command::Subscribe { sub_id, instrument, data_type: DataType::Trades } => {
                let product = instrument.native.to_ascii_uppercase();
                st.by_sub.insert(*sub_id, product.clone());
                st.by_product.insert(product.clone(), (*sub_id, instrument.clone()));
                add.push(product);
            }
            Command::Subscribe { sub_id, instrument, data_type: DataType::Quote } if !m.capabilities().live.quote.is_none() => {
                let product = instrument.native.to_ascii_uppercase();
                st.book_by_sub.insert(*sub_id, product.clone());
                st.books.insert(product.clone(), Book { sub_id: *sub_id, instrument: instrument.clone(), bids: BTreeMap::new(), asks: BTreeMap::new() });
                add_books.push(product);
            }
            Command::Subscribe { data_type, .. } => {
                return Err(ConnectorError::unsupported(format!("Coinbase streams trades and quotes only, not {data_type:?}")));
            }
            Command::Unsubscribe { sub_id } => {
                if let Some(p) = st.by_sub.remove(sub_id) {
                    st.by_product.remove(&p);
                    drop.push(p);
                }
                if let Some(p) = st.book_by_sub.remove(sub_id) {
                    st.books.remove(&p);
                    drop_books.push(p);
                }
            }
            _ => return Err(ConnectorError::invalid("not a socket command")),
        }
    }
    let mut out = Vec::new();
    if !st.opened && !(add.is_empty() && add_books.is_empty()) {
        out.push(NativeRequest::WsOpen { conn_key: ConnKey(CONN.into()), base: WS, path: String::new() });
        st.opened = true;
    }
    if !add.is_empty() {
        out.push(frame("subscribe", &add));
    }
    if !add_books.is_empty() {
        out.push(channel_frame("subscribe", &add_books, BOOK_CHANNEL));
    }
    if !drop.is_empty() {
        out.push(frame("unsubscribe", &drop));
    }
    if !drop_books.is_empty() {
        out.push(channel_frame("unsubscribe", &drop_books, BOOK_CHANNEL));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct Msg {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    product_id: Option<String>,
    #[serde(default)]
    price: Option<Dec>,
    #[serde(default)]
    size: Option<Dec>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    trade_id: Option<u64>,
    #[serde(default)]
    time: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    /// `snapshot`: the whole book, `[price, size]` per level.
    #[serde(default)]
    bids: Vec<(Dec, Dec)>,
    #[serde(default)]
    asks: Vec<(Dec, Dec)>,
    /// `l2update`: `[side, price, size]`, a size of 0 removing the level.
    #[serde(default)]
    changes: Vec<(String, Dec, Dec)>,
}

/// The top of a product's book after a snapshot or an update, as a quote.
fn book_quote(st: &mut LiveState, m: &Msg, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let Some(product) = m.product_id.as_deref().map(str::to_ascii_uppercase) else { return Ok(vec![]) };
    let Some(book) = st.books.get_mut(&product) else { return Ok(vec![]) };
    if m.kind == "snapshot" {
        book.bids = m.bids.iter().filter(|(_, q)| !q.0.is_zero()).map(|(p, q)| (p.0, q.0)).collect();
        book.asks = m.asks.iter().filter(|(_, q)| !q.0.is_zero()).map(|(p, q)| (p.0, q.0)).collect();
    }
    for (side, price, size) in &m.changes {
        let levels = match side.as_str() {
            "buy" => &mut book.bids,
            "sell" => &mut book.asks,
            _ => continue,
        };
        if size.0.is_zero() {
            levels.remove(&price.0);
        } else {
            levels.insert(price.0, size.0);
        }
    }
    // An empty side is no quote.
    let (Some((bid, bq)), Some((ask, aq))) = (book.bids.last_key_value(), book.asks.first_key_value()) else { return Ok(vec![]) };
    let q = Quote { instrument: book.instrument.clone(), bid: Price(*bid), ask: Price(*ask), bid_qty: Some(Qty(*bq)), ask_qty: Some(Qty(*aq)) };
    let env = Envelope::new(ctx.connector.clone(), Some(book.sub_id), ctx.received_ts, Event::Quote(q));
    Ok(vec![match m.time.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
        Some(t) => env.at(t),
        None => env,
    }])
}

pub fn decode(payload: &[u8], st: &mut LiveState, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let m: Msg = serde_json::from_slice(payload).map_err(|e| ConnectorError::decode(format!("coinbase socket: {e}")))?;
    match m.kind.as_str() {
        "match" | "last_match" => {}
        "snapshot" | "l2update" => return book_quote(st, &m, ctx),
        "error" => {
            let text = format!("{} {}", m.message.unwrap_or_default(), m.reason.unwrap_or_default()).trim().to_string();
            // A refusal naming a product this socket carries is that pane's; any other is
            // reported, not fatal (the legacy feed ignored them all).
            let hit = st
                .by_product
                .iter()
                .find(|(p, _)| text.contains(p.as_str()))
                .map(|(_, (sub, _))| *sub)
                .or_else(|| st.books.iter().find(|(p, _)| text.contains(p.as_str())).map(|(_, b)| b.sub_id));
            let event = |e: ConnectorError| match hit {
                Some(sub) => Envelope::new(ctx.connector.clone(), Some(sub), ctx.received_ts, Event::SubscriptionFail { error: e }),
                None => Envelope::new(ctx.connector.clone(), None, ctx.received_ts, Event::Error(e)),
            };
            return Ok(vec![event(ConnectorError::new(ErrorKind::NotFound, format!("Coinbase refused this product: {text}")))]);
        }
        _ => return Ok(vec![]),
    }
    let product = m.product_id.ok_or_else(|| ConnectorError::decode("coinbase match: no product_id"))?.to_ascii_uppercase();
    let Some((sub, instrument)) = st.by_product.get(&product) else { return Ok(vec![]) };
    let price = m.price.ok_or_else(|| ConnectorError::decode("coinbase match: no price"))?;
    // The side is the resting (maker) order's: the aggressor is the other one.
    let aggressor = match m.side.as_deref() {
        Some("buy") => Some(Mapped::Known(Aggressor::Sell)),
        Some("sell") => Some(Mapped::Known(Aggressor::Buy)),
        Some(other) => Some(otw_connector_sdk::enums::unknown("coinbase", "side", other)),
        None => None,
    };
    let trade = Trade {
        instrument: instrument.clone(),
        id: m.trade_id.map(|t| t.to_string()),
        price: price.price(),
        qty: m.size.map(Dec::qty).unwrap_or(otw_connector_model::Qty::ZERO),
        aggressor,
    };
    let env = Envelope::new(ctx.connector.clone(), Some(*sub), ctx.received_ts, Event::Trade(trade));
    Ok(vec![match m.time.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
        Some(t) => env.at(t),
        None => env,
    }])
}
