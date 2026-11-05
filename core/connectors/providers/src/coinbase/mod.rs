//! Coinbase: candle history, the product list and best bid/ask from the keyless Exchange API,
//! live trades on its public feed (candles are folded by OTW), the price board from Advanced
//! Trade's public product list, and the account read only through Advanced Trade (an Ed25519
//! JWT per request). Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind, Event, HistoryItems,
    InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_secs, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::value::RawValue;

pub const VENUE: &str = "COINBASE";
/// The Exchange API: keyless market data.
const REST: &str = "rest";
/// Advanced Trade: the account, and the price board.
const BROKERAGE: &str = "brokerage";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[
    (REST, "https://api.exchange.coinbase.com"),
    (BROKERAGE, "https://api.coinbase.com"),
    (WS, "wss://ws-feed.exchange.coinbase.com"),
];
const PREFIX: &str = "/api/v3/brokerage";
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Candles per request (the venue counts both window ends).
const PAGE: i64 = 300;
/// How long the product quotes the session keeps are trusted before a reload.
const NAMES_TTL_NS: i64 = 3_600_000_000_000;

#[derive(Clone)]
pub struct Coinbase {
    caps: Capabilities,
}

impl Coinbase {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Coinbase { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// One candle request: the window asked for and the slice of it this call covers.
struct CandlePage {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
    page: TimeRange,
}

enum Pending {
    Candles(CandlePage),
    Instruments,
    Quote { instrument: InstrumentId },
    /// Advanced Trade's product list: the board, and the quote currency of every product.
    Board { emit: bool },
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    /// Product id to its quote currency, with when it was loaded.
    quotes: Option<(Timestamp, Arc<HashMap<String, String>>)>,
    live: stream::LiveState,
}

impl Session {
    pub(crate) fn quotes(&self) -> Arc<HashMap<String, String>> {
        self.quotes.as_ref().map(|(_, q)| q.clone()).unwrap_or_default()
    }

    /// The product list request, when the quotes the session keeps are missing or stale.
    pub(crate) fn need_quotes(&mut self, req_id: CorrId, now: Timestamp) -> Option<NativeRequest> {
        if self.quotes.as_ref().is_some_and(|(at, _)| now.nanos().saturating_sub(at.nanos()) < NAMES_TTL_NS) {
            return None;
        }
        self.pending.insert((req_id, "productQuotes"), Pending::Board { emit: false });
        Some(NativeRequest::Http(board_request("productQuotes", req_id)))
    }
}

fn board_request(endpoint: &'static str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, BROKERAGE, format!("{PREFIX}/market/products"), req_id).query("product_type", "SPOT")
}

/// RFC 3339 to the second, with the fraction only when there is one (as the legacy reader
/// wrote it).
pub(crate) fn rfc3339(t: Timestamp) -> Result<String, ConnectorError> {
    let d = t.to_offset_datetime().ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))?;
    d.format(&time::format_description::well_known::Rfc3339).map_err(|e| ConnectorError::invalid(format!("time {t}: {e}")))
}

#[derive(Deserialize)]
struct ProductRow {
    id: String,
    #[serde(default)]
    base_currency: String,
    #[serde(default)]
    quote_currency: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    trading_disabled: Option<bool>,
    #[serde(default)]
    quote_increment: OptDec,
    #[serde(default)]
    base_increment: OptDec,
}

#[derive(Deserialize)]
struct TickerRow {
    #[serde(default)]
    bid: OptDec,
    #[serde(default)]
    ask: OptDec,
}

#[derive(Deserialize)]
struct MarketProducts {
    #[serde(default)]
    products: Vec<MarketProduct>,
}

#[derive(Deserialize)]
struct MarketProduct {
    product_id: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    quote_currency_id: String,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("coinbase {what}: {e}"))
}

impl Coinbase {
    fn account_declared(&self, kind: otw_connector_model::AccountKind) -> bool {
        use otw_connector_model::AccountKind as K;
        let a = &self.caps.account;
        match kind {
            K::Balances => a.balances,
            K::Positions => a.positions,
            K::Fills => a.fills_history,
            K::OpenOrders => a.open_orders,
            K::Summary => a.summary,
        }
    }

    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let gran = tf
            .fixed_secs()
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("Coinbase serves no {tf} history here")))?;
        let start = match cursor {
            Some(c) => ts_secs(c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?)?,
            None => range.from,
        };
        // Both ends count: 300 steps is the most one call answers.
        let end = range.to.min(start.plus_secs(PAGE * gran));
        let page = TimeRange::new(start, end);
        s.pending.insert((req_id, "candles"), Pending::Candles(CandlePage { instrument: instrument.clone(), tf, range, page }));
        Ok(vec![NativeRequest::Http(
            HttpRequest::get("candles", REST, format!("/products/{}/candles", instrument.native), req_id)
                .query("granularity", gran)
                .query("start", rfc3339(start)?)
                .query("end", rfc3339(end)?),
        )])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], p: CandlePage, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let CandlePage { instrument, tf, range, page } = p;
        // `[time, low, high, open, close, volume]`, newest first, numbers.
        let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("candles", e))?;
        let last_page = page.to >= range.to;
        let mut candles = Vec::with_capacity(rows.len());
        for row in &rows {
            let field = |i: usize| row.get(i).map(|v| v.get()).ok_or_else(|| ConnectorError::decode("coinbase candles: row shape"));
            let num = |i: usize| -> Result<Dec, ConnectorError> { serde_json::from_str::<Dec>(field(i)?).map_err(|e| decode_err("candle number", e)) };
            let open_ts = ts_secs(serde_json::from_str::<Int>(field(0)?).map_err(|e| decode_err("candle time", e))?.0)?;
            // The page's end bar opens the next page, unless this is the last one.
            let in_page = open_ts >= page.from && (open_ts < page.to || (last_page && open_ts == page.to));
            if !in_page || !range.contains(open_ts) {
                continue;
            }
            candles.push(Candle {
                instrument: instrument.clone(),
                tf,
                open_ts,
                open: num(3)?.price(),
                high: num(2)?.price(),
                low: num(1)?.price(),
                close: num(4)?.price(),
                volume: num(5)?.qty(),
                quote_volume: None,
                trades: None,
                // No closed flag: the engine finalizes at the period's end.
                is_final: false,
                adjusted: None,
                sides: None,
            });
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = (!last_page).then(|| page.to.secs().to_string());
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let rows: Vec<ProductRow> = serde_json::from_slice(body).map_err(|e| decode_err("products", e))?;
        let mut out = Vec::with_capacity(rows.len() + 1);
        for p in rows {
            let id = p.id.to_ascii_uppercase();
            let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &id), &p.base_currency, &p.quote_currency);
            spec.symbol = otw_connector_model::CanonicalSymbol(p.display_name.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| p.id.clone()));
            spec.tick_size = p.quote_increment.0.map(Price);
            spec.lot_size = p.base_increment.0.map(Qty);
            spec.active = p.trading_disabled != Some(true);
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for Coinbase {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // Coinbase's own product id: BTC-USD. It lands in a URL path, so nothing else passes.
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ConnectorError::not_found(format!("Coinbase: bad symbol `{symbol}`, write the product as Coinbase spells it (BTC-USD)")));
        }
        Ok(InstrumentId::new(VENUE, &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert((*req_id, "products"), Pending::Instruments);
                Ok(vec![NativeRequest::Http(HttpRequest::get("products", REST, "/products", *req_id))])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert((*req_id, "ticker"), Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(HttpRequest::get("ticker", REST, format!("/products/{}/ticker", instrument.native), *req_id))])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert((*req_id, "board"), Pending::Board { emit: true });
                Ok(vec![NativeRequest::Http(board_request("board", *req_id))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, s, ctx),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: the token carries the host clock (as before).
            other => Err(ConnectorError::unsupported(format!("Coinbase: {other:?} is not declared"))),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
            return stream::encode(self, cmds, &mut s.live);
        }
        let mut out = Vec::new();
        for c in cmds {
            out.extend(self.encode(c, s, ctx)?);
        }
        Ok(out)
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(errors::from_frame(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = errors::from_frame(frame) {
            return Err(e);
        }
        match pending {
            Pending::Candles(p) => self.decode_candles(corr, body, p, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, ctx),
            Pending::Quote { instrument } => {
                let t: TickerRow = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
                // A one-sided book is no quote: nothing is emitted rather than half a price.
                let (Some(bid), Some(ask)) = (t.bid.0, t.ask.0) else { return Ok(vec![]) };
                Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: None, ask_qty: None }))])
            }
            Pending::Board { emit } => {
                let page: MarketProducts = serde_json::from_slice(body).map_err(|e| decode_err("market/products", e))?;
                let quotes: HashMap<String, String> = page
                    .products
                    .iter()
                    .filter(|p| !p.quote_currency_id.is_empty())
                    .map(|p| (p.product_id.to_ascii_uppercase(), p.quote_currency_id.to_ascii_uppercase()))
                    .collect();
                s.quotes = Some((ctx.received_ts, Arc::new(quotes)));
                if !emit {
                    return Ok(vec![]);
                }
                // A product with no price yet is no market to value anything on.
                let board = page
                    .products
                    .into_iter()
                    .filter_map(|p| {
                        let price = p.price.0.filter(|x| x.is_sign_positive() && !x.is_zero())?;
                        Some(LastPrice { instrument: InstrumentId::new(VENUE, &p.product_id.to_ascii_uppercase()), price: Price(price) })
                    })
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Account(p) => account::decode(p, corr, body, s, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
        };
        let key_name = get("key_name")?;
        let key = SecretString::from(get("api_private_key")?);
        Ok(Some(Arc::new(signer::CoinbaseSigner::new(key_name, key)?)))
    }

    fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        match settings.get("key_name").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(name) if !name.starts_with("organizations/") || !name.contains("/apiKeys/") => Err(ConnectorError::invalid(
                "the key name is the whole string Coinbase shows, organizations/<org>/apiKeys/<key>",
            )),
            _ => Ok(()),
        }
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        Some(format!("{}-{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }
}

#[cfg(test)]
mod tests;
