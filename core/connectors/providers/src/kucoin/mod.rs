//! KuCoin, spot and futures, one adapter per platform: candle history, the instrument lists,
//! best bid/ask, the spot price board, live candles and the account read only. Spot and
//! futures are separate platforms on separate hosts, with their own symbols (`BTC-USDT`,
//! `XBTUSDTM`) and sockets, so each is its own provider. Futures are the USDT- and
//! USDC-margined contracts; the coin-margined (inverse) ones are not served.
//! Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Candle, Capabilities, CapsError, Command, ConnectorError, ContractType, CorrId, DataType, Decimal, Envelope,
    ErrorKind, Event, HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TfUnit, TimeRange, Timeframe,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, Signer};
use serde::Deserialize;
use serde_json::value::RawValue;

const REST: &str = "rest";
const SPOT_BASES: &[(&str, &str)] = &[(REST, "https://api.kucoin.com")];
const FUTURES_BASES: &[(&str, &str)] = &[(REST, "https://api-futures.kucoin.com")];
/// Candles per history page. Spot answers the newest 1500 bars of a range, futures the
/// oldest 200 (both measured on 2026-10-19).
const SPOT_PAGE: usize = 1500;
const FUTURES_PAGE: usize = 200;
/// Neither archive flags its forming bar. Measured on 2026-10-19 by polling it through
/// minute boundaries: no spot bar changed after its close; a futures bar was rewritten up to
/// 12.3 s after it (8 bars over XBTUSDTM and ETHUSDTM). A bar younger than its period plus
/// this is not final.
const SPOT_SETTLE_MS: i64 = 10_000;
const FUTURES_SETTLE_MS: i64 = 60_000;
/// The settlement coins of the futures this adapter serves (the `rootSymbol` of a
/// USDT- or USDC-margined contract). Coin-margined contracts settle in the coin.
pub(crate) const SETTLE_COINS: &[&str] = &["USDT", "USDC"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Spot,
    Futures,
}

impl Book {
    pub fn venue(self) -> &'static str {
        match self {
            Book::Spot => "KUCOIN",
            Book::Futures => "KUCOIN_FUTURES",
        }
    }

    fn settle_ms(self) -> i64 {
        match self {
            Book::Spot => SPOT_SETTLE_MS,
            Book::Futures => FUTURES_SETTLE_MS,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Book::Spot => "KuCoin",
            Book::Futures => "KuCoin Futures",
        }
    }
}

#[derive(Clone)]
pub struct Kucoin {
    caps: Capabilities,
    book: Book,
}

impl Kucoin {
    pub fn spot() -> Result<Self, CapsError> {
        Ok(Kucoin { caps: Capabilities::from_toml(include_str!("capabilities_spot.toml"))?, book: Book::Spot })
    }

    pub fn futures() -> Result<Self, CapsError> {
        Ok(Kucoin { caps: Capabilities::from_toml(include_str!("capabilities_futures.toml"))?, book: Book::Futures })
    }

    pub fn book(&self) -> Book {
        self.book
    }
}

/// The call a contract lookup was made for, sent once the contract's size is known.
enum Then {
    Candles { tf: Timeframe, range: TimeRange, cursor: Option<String> },
    Quote,
}

enum Pending {
    /// `size`: base coin per unit the venue counts volume in (one on spot, the contract's
    /// size on futures).
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange, size: Decimal },
    Contract { instrument: InstrumentId, then: Then },
    Instruments,
    Quote { instrument: InstrumentId, size: Decimal },
    Board,
    Bullet,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    /// Futures contract sizes already read, base coin per contract, by symbol.
    sizes: HashMap<String, Decimal>,
    /// The request a contract lookup was asked for, sent as its follow-up.
    next: Option<(CorrId, NativeRequest)>,
    live: stream::LiveState,
}

/// KuCoin's interval token, the same on spot history and on both sockets. The weekly bar is
/// a spot token too, but spot weeks open on Thursday (the epoch's weekday), so spot does
/// not declare it and its week is folded from days.
fn interval(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1min",
        (3, TfUnit::Minute) => "3min",
        (5, TfUnit::Minute) => "5min",
        (15, TfUnit::Minute) => "15min",
        (30, TfUnit::Minute) => "30min",
        (1, TfUnit::Hour) => "1hour",
        (2, TfUnit::Hour) => "2hour",
        (4, TfUnit::Hour) => "4hour",
        (6, TfUnit::Hour) => "6hour",
        (8, TfUnit::Hour) => "8hour",
        (12, TfUnit::Hour) => "12hour",
        (1, TfUnit::Day) => "1day",
        (1, TfUnit::Week) => "1week",
        _ => return None,
    })
}

/// Futures history granularity, in minutes.
fn granularity(tf: Timeframe) -> Option<i64> {
    let m = tf.fixed_secs()? / 60;
    [1, 5, 15, 30, 60, 120, 240, 480, 720, 1440, 10080].contains(&m).then_some(m)
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SymbolRow {
    symbol: String,
    #[serde(default)]
    base_currency: String,
    #[serde(default)]
    quote_currency: String,
    #[serde(default)]
    base_min_size: OptDec,
    #[serde(default)]
    base_increment: OptDec,
    #[serde(default)]
    price_increment: OptDec,
    #[serde(default)]
    enable_trading: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractRow {
    pub symbol: String,
    /// `FFWCSX` perpetual, `FFICSX` dated future.
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    base_currency: String,
    #[serde(default)]
    quote_currency: String,
    #[serde(default)]
    settle_currency: String,
    /// Base coin per contract (negative on an inverse contract: USD per contract).
    #[serde(default)]
    pub multiplier: OptDec,
    #[serde(default)]
    lot_size: OptDec,
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    pub is_inverse: bool,
    #[serde(default)]
    expire_date: Option<Int>,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotLevel1 {
    #[serde(default)]
    best_bid: OptDec,
    #[serde(default)]
    best_bid_size: OptDec,
    #[serde(default)]
    best_ask: OptDec,
    #[serde(default)]
    best_ask_size: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesTicker {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    best_bid_price: OptDec,
    #[serde(default)]
    best_bid_size: OptDec,
    #[serde(default)]
    best_ask_price: OptDec,
    #[serde(default)]
    best_ask_size: OptDec,
}

#[derive(Deserialize)]
struct AllTickers {
    #[serde(default)]
    ticker: Vec<BoardRow>,
}

#[derive(Deserialize)]
struct BoardRow {
    symbol: String,
    #[serde(default)]
    last: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bullet {
    token: String,
    #[serde(default)]
    instance_servers: Vec<Server>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Server {
    endpoint: String,
    #[serde(default)]
    ping_interval: Option<Int>,
}

/// Where each candle shape keeps its fields. Spot (archive and socket):
/// `[time s, open, close, high, low, volume, turnover]`. Futures archive:
/// `[time ms, open, high, low, close, volume, turnover]`. Futures socket:
/// `[time s, open, close, high, low, turnover, volume]`. Futures volume is in contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    Spot,
    FuturesRest,
    FuturesWs,
}

pub(crate) struct CandleRow {
    pub open_ms: i64,
    pub o: Dec,
    pub h: Dec,
    pub l: Dec,
    pub c: Dec,
    /// In the base coin.
    pub vol: Decimal,
    pub turnover: Option<Dec>,
}

impl CandleRow {
    /// `size`: base coin per unit of the row's volume.
    pub fn read(row: &[Box<RawValue>], layout: Layout, size: Decimal) -> Option<CandleRow> {
        let at = |i: usize| row.get(i).map(|v| v.get());
        let dec = |i: usize| at(i).and_then(|t| serde_json::from_str::<Dec>(t).ok());
        let t = at(0).and_then(|t| serde_json::from_str::<Int>(t).ok())?.0;
        let (open_ms, o, c, h, l, vol, turnover) = match layout {
            Layout::Spot => (t.checked_mul(1000)?, dec(1)?, dec(2)?, dec(3)?, dec(4)?, dec(5)?, dec(6)),
            Layout::FuturesRest => (t, dec(1)?, dec(4)?, dec(2)?, dec(3)?, dec(5)?, dec(6)),
            Layout::FuturesWs => (t.checked_mul(1000)?, dec(1)?, dec(2)?, dec(3)?, dec(4)?, dec(6)?, dec(5)),
        };
        Some(CandleRow { open_ms, o, h, l, c, vol: vol.0.checked_mul(size)?, turnover })
    }

    pub fn candle(&self, instrument: InstrumentId, tf: Timeframe, is_final: bool) -> Result<Candle, ConnectorError> {
        Ok(Candle {
            instrument,
            tf,
            open_ts: ts_millis(self.open_ms)?,
            open: self.o.price(),
            high: self.h.price(),
            low: self.l.price(),
            close: self.c.price(),
            volume: Qty(self.vol),
            quote_volume: self.turnover.map(|q| q.money()),
            trades: None,
            is_final,
            adjusted: None,
            sides: None,
        })
    }
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kucoin {what}: {e}"))
}

/// A positive contract size, base coin per contract.
fn positive(d: Decimal) -> Option<Decimal> {
    (d.is_sign_positive() && !d.is_zero()).then_some(d)
}

impl Kucoin {
    fn bases(&self) -> &'static [(&'static str, &'static str)] {
        match self.book {
            Book::Spot => SPOT_BASES,
            Book::Futures => FUTURES_BASES,
        }
    }

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

    /// A futures contract's size as already known (the loaded list, or an earlier lookup).
    fn size_of(&self, instrument: &InstrumentId, s: &Session, ctx: &MapCtx<'_>) -> Option<Decimal> {
        if self.book == Book::Spot {
            return Some(Decimal::ONE);
        }
        ctx.instruments.get(instrument).and_then(|c| positive(c.multiplier)).or_else(|| s.sizes.get(&instrument.native).copied())
    }

    /// Ask for the contract first: its size turns contracts into coins, and an unknown
    /// symbol is refused by name rather than answered with an empty page.
    fn contract_first(&self, req_id: CorrId, instrument: &InstrumentId, then: Then, s: &mut Session) -> Vec<NativeRequest> {
        s.pending.insert(req_id, Pending::Contract { instrument: instrument.clone(), then });
        vec![NativeRequest::Http(HttpRequest::get("contract", REST, format!("/api/v1/contracts/{}", instrument.native), req_id))]
    }

    #[allow(clippy::too_many_arguments)]
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("{} serves no {tf} history", self.caps.label)));
        }
        let Some(size) = self.size_of(instrument, s, ctx) else {
            return Ok(self.contract_first(req_id, instrument, Then::Candles { tf, range, cursor: cursor.map(str::to_string) }, s));
        };
        Ok(vec![self.candle_request(req_id, instrument, tf, range, cursor, size, s)?])
    }

    #[allow(clippy::too_many_arguments)]
    fn candle_request(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, size: Decimal, s: &mut Session) -> Result<NativeRequest, ConnectorError> {
        let bad = |c: &str| ConnectorError::invalid(format!("bad cursor `{c}`"));
        let h = match self.book {
            // Backwards from `endAt` (exclusive), in seconds, newest bar first.
            Book::Spot => {
                let token = interval(tf).ok_or_else(|| ConnectorError::unsupported(format!("KuCoin has no {tf} candles")))?;
                let end_s = match cursor {
                    Some(c) => c.parse::<i64>().map_err(|_| bad(c))?,
                    None => (range.to.millis() + 999).div_euclid(1000),
                };
                HttpRequest::get("candles", REST, "/api/v1/market/candles", req_id)
                    .query("type", token)
                    .query("symbol", &instrument.native)
                    .query("startAt", range.from.millis().div_euclid(1000))
                    .query("endAt", end_s)
                    .weight(3)
            }
            // Forwards from `from`, both ends inclusive, in milliseconds, oldest bar first.
            Book::Futures => {
                let g = granularity(tf).ok_or_else(|| ConnectorError::unsupported(format!("KuCoin Futures has no {tf} candles")))?;
                let from = match cursor {
                    Some(c) => c.parse::<i64>().map_err(|_| bad(c))?,
                    None => range.from.millis(),
                };
                HttpRequest::get("kline", REST, "/api/v1/kline/query", req_id)
                    .query("symbol", &instrument.native)
                    .query("granularity", g)
                    .query("from", from)
                    .query("to", range.to.millis() - 1)
                    .weight(3)
            }
        };
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range, size });
        Ok(NativeRequest::Http(h))
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, size: Decimal, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Data<Option<Vec<Vec<Box<RawValue>>>>> = serde_json::from_slice(body).map_err(|e| decode_err("candles", e))?;
        let rows = page.data.unwrap_or_default();
        let (layout, page_len) = match self.book {
            Book::Spot => (Layout::Spot, SPOT_PAGE),
            Book::Futures => (Layout::FuturesRest, FUTURES_PAGE),
        };
        let full = rows.len() >= page_len;
        let step_ms = tf.fixed_secs().map(|s| s * 1000).unwrap_or(i64::MAX / 4);
        let closed_before = ctx.received_ts.millis() - step_ms - self.book.settle_ms();
        let mut candles = Vec::with_capacity(rows.len());
        let (mut oldest, mut newest): (Option<i64>, Option<i64>) = (None, None);
        for raw in &rows {
            let r = CandleRow::read(raw, layout, size).ok_or_else(|| ConnectorError::decode("kucoin candles: row shape"))?;
            oldest = Some(oldest.map_or(r.open_ms, |o| o.min(r.open_ms)));
            newest = Some(newest.map_or(r.open_ms, |n| n.max(r.open_ms)));
            if !range.contains(ts_millis(r.open_ms)?) {
                continue;
            }
            candles.push(r.candle(instrument.clone(), tf, r.open_ms <= closed_before)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = match (self.book, full) {
            (Book::Spot, true) => oldest.filter(|o| *o > range.from.millis()).map(|o| o.div_euclid(1000).to_string()),
            (Book::Futures, true) => newest.map(|n| n + step_ms).filter(|n| *n < range.to.millis()).map(|n| n.to_string()),
            _ => None,
        };
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    fn contract_spec(&self, r: &ContractRow) -> Result<Option<InstrumentSpec>, ConnectorError> {
        let id = r.symbol.to_ascii_uppercase();
        if id.is_empty() || r.is_inverse {
            return Ok(None);
        }
        let perp = match r.kind.as_str() {
            "FFWCSX" => true,
            "FFICSX" => false,
            other => {
                tracing::warn!("kucoin contracts: unknown type `{other}` on {id}, left out");
                return Ok(None);
            }
        };
        let Some(size) = r.multiplier.0.and_then(positive) else {
            tracing::warn!("kucoin contracts: {id} has no contract size, left out");
            return Ok(None);
        };
        let mut spec = InstrumentSpec::spot(InstrumentId::new(Book::Futures.venue(), &id), &r.base_currency, &r.quote_currency);
        spec.asset_class = if perp { AssetClass::Perp } else { AssetClass::Future };
        spec.contract = if perp { ContractType::Perp } else { ContractType::Future };
        // What one contract is worth in the coin; fills and positions count contracts.
        spec.multiplier = size;
        spec.tick_size = r.tick_size.0.map(Price);
        spec.lot_size = r.lot_size.0.and_then(|l| l.checked_mul(size)).map(Qty);
        spec.min_qty = spec.lot_size;
        spec.settlement_ccy = Some(r.settle_currency.to_ascii_uppercase()).filter(|c| !c.is_empty());
        spec.active = r.status == "Open";
        if !perp {
            spec.expiry = r.expire_date.map(|d| ts_millis(d.0)).transpose()?;
        }
        Ok(Some(spec))
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let mut out = Vec::new();
        match self.book {
            Book::Spot => {
                let page: Data<Vec<SymbolRow>> = serde_json::from_slice(body).map_err(|e| decode_err("symbols", e))?;
                for r in page.data {
                    let id = r.symbol.to_ascii_uppercase();
                    if id.is_empty() {
                        continue;
                    }
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(Book::Spot.venue(), &id), &r.base_currency, &r.quote_currency);
                    spec.active = r.enable_trading;
                    spec.tick_size = r.price_increment.0.map(Price);
                    spec.lot_size = r.base_increment.0.map(Qty);
                    spec.min_qty = r.base_min_size.0.map(Qty);
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
            }
            Book::Futures => {
                let page: Data<Vec<ContractRow>> = serde_json::from_slice(body).map_err(|e| decode_err("contracts", e))?;
                for r in &page.data {
                    if let Some(spec) = self.contract_spec(r)? {
                        s.sizes.insert(spec.id.native.clone(), spec.multiplier);
                        out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                    }
                }
            }
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }

    /// The contract a deferred call was waiting for: its size kept, the call queued.
    fn decode_contract(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, then: Then, s: &mut Session) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Data<ContractRow> = serde_json::from_slice(body).map_err(|e| decode_err("contract", e))?;
        if page.data.is_inverse {
            return Err(ConnectorError::unsupported(format!(
                "{} is a coin-margined (inverse) contract, which this connector does not serve: its volume is counted in USD. \
                 Use the USDT-margined one ({}USDTM).",
                instrument.native, page.data.base_currency
            )));
        }
        let size = page.data.multiplier.0.and_then(positive).ok_or_else(|| ConnectorError::decode(format!("kucoin contract {}: no contract size", instrument.native)))?;
        s.sizes.insert(instrument.native.clone(), size);
        let req = match then {
            Then::Candles { tf, range, cursor } => self.candle_request(corr, &instrument, tf, range, cursor.as_deref(), size, s)?,
            Then::Quote => self.quote_request(corr, &instrument, size, s),
        };
        s.next = Some((corr, req));
        Ok(vec![])
    }

    fn quote_request(&self, req_id: CorrId, instrument: &InstrumentId, size: Decimal, s: &mut Session) -> NativeRequest {
        s.pending.insert(req_id, Pending::Quote { instrument: instrument.clone(), size });
        let h = match self.book {
            Book::Spot => HttpRequest::get("level1", REST, "/api/v1/market/orderbook/level1", req_id),
            Book::Futures => HttpRequest::get("ticker", REST, "/api/v1/ticker", req_id),
        };
        NativeRequest::Http(h.query("symbol", &instrument.native))
    }

    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, size: Decimal, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let unlisted = || ConnectorError::not_found(format!("{} does not list `{}`", self.book.label(), instrument.native));
        let (bid, bid_qty, ask, ask_qty) = match self.book {
            Book::Spot => {
                let page: Data<Option<SpotLevel1>> = serde_json::from_slice(body).map_err(|e| decode_err("level1", e))?;
                let t = page.data.ok_or_else(unlisted)?;
                (t.best_bid.0, t.best_bid_size.0, t.best_ask.0, t.best_ask_size.0)
            }
            Book::Futures => {
                let page: Data<Option<FuturesTicker>> = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
                let t = page.data.ok_or_else(unlisted)?;
                if !t.symbol.eq_ignore_ascii_case(&instrument.native) {
                    return Err(ConnectorError::decode(format!("kucoin ticker answered {} for {}", t.symbol, instrument.native)));
                }
                (t.best_bid_price.0, t.best_bid_size.0, t.best_ask_price.0, t.best_ask_size.0)
            }
        };
        let (Some(bid), Some(ask)) = (bid, ask) else { return Ok(vec![]) };
        let coins = |q: Option<Decimal>| q.and_then(|q| q.checked_mul(size)).map(Qty);
        Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: coins(bid_qty), ask_qty: coins(ask_qty) }))])
    }

    /// A market-data refusal that is about the symbol says which one.
    fn about(&self, e: ConnectorError, instrument: &InstrumentId) -> ConnectorError {
        let symbol_code = matches!(e.provider_code.as_deref(), Some("400100" | "900001" | "200003" | "404000"));
        if e.kind == ErrorKind::NotFound || symbol_code {
            let mut out = ConnectorError::not_found(format!("{} does not list `{}`: {}", self.book.label(), instrument.native, e.message));
            out.provider_code = e.provider_code;
            return out;
        }
        e
    }
}

impl Mapper for Kucoin {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.bases()
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim();
        let ok = !s.is_empty()
            && match self.book {
                Book::Spot => s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                Book::Futures => s.chars().all(|c| c.is_ascii_alphanumeric()),
            };
        if !ok {
            return Err(ConnectorError::not_found(match self.book {
                Book::Spot => format!("KuCoin: bad symbol `{symbol}`, write the pair as KuCoin spells it (BTC-USDT)"),
                Book::Futures => format!("KuCoin Futures: bad symbol `{symbol}`, write the contract as KuCoin spells it (XBTUSDTM, ETHUSDCM)"),
            }));
        }
        Ok(InstrumentId::new(self.book.venue(), &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s, ctx)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert(*req_id, Pending::Instruments);
                let h = match self.book {
                    Book::Spot => HttpRequest::get("symbols", REST, "/api/v2/symbols", *req_id),
                    Book::Futures => HttpRequest::get("contracts", REST, "/api/v1/contracts/active", *req_id),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => match self.size_of(instrument, s, ctx) {
                Some(size) => Ok(vec![self.quote_request(*req_id, instrument, size, s)]),
                None => Ok(self.contract_first(*req_id, instrument, Then::Quote, s)),
            },
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert(*req_id, Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("allTickers", REST, "/api/v1/market/allTickers", *req_id))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(self.book, cmd, &mut s.pending),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: KuCoin accepts the host clock within its window.
            other => Err(ConnectorError::unsupported(format!("{}: {other:?} is not declared", self.caps.label))),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
            return stream::encode(self, cmds, &mut s.live, ctx);
        }
        let mut out = Vec::new();
        for c in cmds {
            out.extend(self.encode(c, s, ctx)?);
        }
        Ok(out)
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, body) = match frame {
            RawFrame::Http { corr_id, body, .. } => (*corr_id, body),
            RawFrame::Timeout { corr_id, .. } => {
                s.pending.remove(corr_id);
                return Err(errors::from_frame(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(self.book, payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s.pending.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr}")))?;
        if let Some(e) = errors::from_frame(frame) {
            return Err(match &pending {
                Pending::Candles { instrument, .. } | Pending::Quote { instrument, .. } | Pending::Contract { instrument, .. } => self.about(e, instrument),
                Pending::Account(p) => account::hint(self.book, p, e),
                _ => e,
            });
        }
        match pending {
            Pending::Candles { instrument, tf, range, size } => self.decode_candles(corr, body, instrument, tf, range, size, ctx),
            Pending::Contract { instrument, then } => self.decode_contract(corr, body, instrument, then, s),
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::Quote { instrument, size } => self.decode_quote(corr, body, instrument, size, ctx),
            Pending::Board => {
                let page: Data<AllTickers> = serde_json::from_slice(body).map_err(|e| decode_err("allTickers", e))?;
                let board = page
                    .data
                    .ticker
                    .into_iter()
                    .filter_map(|t| t.last.0.map(|p| LastPrice { instrument: InstrumentId::new(Book::Spot.venue(), &t.symbol.to_ascii_uppercase()), price: Price(p) }))
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Bullet => {
                let page: Data<Bullet> = serde_json::from_slice(body).map_err(|e| decode_err("bullet-public", e))?;
                let server = page.data.instance_servers.into_iter().next().ok_or_else(|| ConnectorError::decode("kucoin bullet-public: no socket server"))?;
                s.live.handed_out(&server.endpoint, &page.data.token, server.ping_interval.map(|p| p.0));
                Ok(vec![])
            }
            Pending::Account(p) => account::decode(self.book, p, corr, body, ctx),
        }
    }

    fn follow_ups(&self, corr_id: CorrId, s: &mut Session) -> Vec<NativeRequest> {
        match s.next.take() {
            Some((corr, req)) if corr == corr_id => vec![req],
            other => {
                s.next = other;
                Vec::new()
            }
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.remove(&corr_id);
        if s.next.as_ref().is_some_and(|(c, _)| *c == corr_id) {
            s.next = None;
        }
    }

    fn socket_bootstrap(&self, corr_id: CorrId, s: &mut Session) -> Option<HttpRequest> {
        s.pending.insert(corr_id, Pending::Bullet);
        let mut h = HttpRequest::get("bullet", REST, "/api/v1/bullet-public", corr_id);
        h.method = Method::Post;
        Some(h)
    }

    /// The socket acknowledges a symbol it does not list and then stays silent, so a pane is
    /// checked against the list before it subscribes (and futures read their contract size
    /// from it).
    fn live_requires_instruments(&self) -> bool {
        true
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(Some(Arc::new(signer::KucoinSigner::from_secrets(secrets)?)))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        (self.book == Book::Spot).then(|| format!("{}-{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }
}

#[cfg(test)]
mod tests;
