//! Gate.io API v4, spot and USDT-settled perpetuals, one adapter per book: candle history,
//! the instrument lists, best bid/ask, the spot price board, live candles and the account
//! read only. `BTC_USDT` is a spot pair and a perpetual at once and they are different
//! series, so which book a connector reads is its setting, never a guess at the ticker.
//! The BTC-settled (inverse) and dated (delivery) contracts are not served.
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
    Timestamp,
};
use otw_connector_sdk::parse::{ts_secs, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use serde::Deserialize;
use serde_json::value::RawValue;

const REST: &str = "rest";
const WS: &str = "ws";
const FX_WS: &str = "fx-ws";
const BASES: &[(&str, &str)] = &[(REST, "https://api.gateio.ws"), (WS, "wss://api.gateio.ws"), (FX_WS, "wss://fx-ws.gateio.ws")];
/// Every REST path starts here, and the signature covers it.
const API: &str = "/api/v4";
/// The settlement currency of the contracts this adapter serves.
pub(crate) const SETTLE: &str = "usdt";
/// Candles per history call: spot refuses a range of more than 1000 points, futures answers
/// at most 2000 (both measured on 2026-10-19).
const SPOT_PAGE: i64 = 1000;
const FUTURES_PAGE: i64 = 2000;
/// Both archives answer the last 10000 bars of a timeframe and refuse anything older
/// ("Candlestick too long ago. Maximum 10000 points ago are allowed", measured). The floor
/// keeps 20 bars of margin so a request near it does not tip past it in flight.
const LOOKBACK_BARS: i64 = 9_980;
/// The futures archive flags no forming bar. Measured on 2026-10-19 by polling it through
/// ten minute boundaries over BTC, ETH, SOL and XRP: a closed minute was rewritten up to
/// 7.3 s after its close (18 rewrites). A bar is final once its period plus this has passed,
/// four times the worst one seen. Spot rows carry their own closed flag, and no flagged row
/// changed afterwards.
const FUTURES_SETTLE_MS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Spot,
    Futures,
}

impl Book {
    pub fn venue(self) -> &'static str {
        match self {
            Book::Spot => "GATEIO",
            Book::Futures => "GATEIO_FUTURES",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Book::Spot => "Gate.io",
            Book::Futures => "Gate.io futures",
        }
    }

    fn page(self) -> i64 {
        match self {
            Book::Spot => SPOT_PAGE,
            Book::Futures => FUTURES_PAGE,
        }
    }
}

#[derive(Clone)]
pub struct Gateio {
    caps: Capabilities,
    book: Book,
}

impl Gateio {
    pub fn spot() -> Result<Self, CapsError> {
        Ok(Gateio { caps: Capabilities::from_toml(include_str!("capabilities_spot.toml"))?, book: Book::Spot })
    }

    pub fn futures() -> Result<Self, CapsError> {
        Ok(Gateio { caps: Capabilities::from_toml(include_str!("capabilities_futures.toml"))?, book: Book::Futures })
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
    /// size on futures). `next`: where the following page ends, when one is due.
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange, size: Decimal, next: Option<i64> },
    Contract { instrument: InstrumentId, then: Then },
    Instruments,
    Quote { instrument: InstrumentId, size: Decimal },
    Board,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    /// Contract sizes already read, base coin per contract, by contract name.
    sizes: HashMap<String, Decimal>,
    /// The request an answer named as the next step (a contract lookup's call, the second
    /// step of an account read), sent as its follow-up.
    next: Option<(CorrId, NativeRequest)>,
    live: stream::LiveState,
}

/// Gate.io's interval token. The same on the archive and the socket of a book, except the
/// week: spot calls it `7d` (Monday), futures `1w` (Monday; the futures `7d` opens on
/// Thursday). The month is spot's `30d`, a calendar month.
fn interval(book: Book, tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1m",
        (5, TfUnit::Minute) => "5m",
        (15, TfUnit::Minute) => "15m",
        (30, TfUnit::Minute) => "30m",
        (1, TfUnit::Hour) => "1h",
        (4, TfUnit::Hour) => "4h",
        (8, TfUnit::Hour) => "8h",
        (1, TfUnit::Day) => "1d",
        (1, TfUnit::Week) => match book {
            Book::Spot => "7d",
            Book::Futures => "1w",
        },
        (1, TfUnit::Month) if book == Book::Spot => "30d",
        _ => return None,
    })
}

/// The base and quote of a contract name (`BTC_USDT`): the list carries no separate fields.
fn contract_pair(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once('_').filter(|(b, q)| !b.is_empty() && !q.is_empty())
}

fn align_up(t: i64, step: i64) -> i64 {
    -(-t).div_euclid(step) * step
}

/// `10^-places`, the step a precision in decimal places stands for.
fn step_of(places: Option<Int>) -> Option<Decimal> {
    let p = u32::try_from(places?.0).ok().filter(|p| *p <= 28)?;
    Some(Decimal::new(1, p))
}

#[derive(Deserialize)]
struct PairRow {
    id: String,
    #[serde(default)]
    base: String,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    min_base_amount: OptDec,
    /// Decimal places of the amount.
    #[serde(default)]
    amount_precision: Option<Int>,
    /// Decimal places of the price.
    #[serde(default)]
    precision: Option<Int>,
    /// `tradable`, `sellable`, `buyable`, `untradable`.
    #[serde(default)]
    trade_status: String,
}

#[derive(Deserialize)]
pub(crate) struct ContractRow {
    pub name: String,
    /// `direct` (settled in the quote) or `inverse`.
    #[serde(rename = "type", default)]
    kind: String,
    /// Base coin per contract.
    #[serde(default)]
    pub quanto_multiplier: OptDec,
    #[serde(default)]
    order_price_round: OptDec,
    /// Contracts.
    #[serde(default)]
    order_size_min: OptDec,
    #[serde(default)]
    status: String,
    #[serde(default)]
    in_delisting: bool,
    /// Empty for crypto; `stocks`, `indices`, `metals`, `forex`, `commodities` otherwise.
    #[serde(default)]
    contract_type: String,
}

#[derive(Deserialize)]
struct TickerRow {
    #[serde(default)]
    currency_pair: Option<String>,
    #[serde(default)]
    contract: Option<String>,
    #[serde(default)]
    last: OptDec,
    #[serde(default)]
    highest_bid: OptDec,
    #[serde(default)]
    highest_size: OptDec,
    #[serde(default)]
    lowest_ask: OptDec,
    #[serde(default)]
    lowest_size: OptDec,
}

impl TickerRow {
    fn name(&self) -> &str {
        self.currency_pair.as_deref().or(self.contract.as_deref()).unwrap_or_default()
    }
}

/// A futures archive row: prices as strings, volume as a number of contracts, `sum` the
/// turnover in the settlement currency.
#[derive(Deserialize)]
struct FuturesRow {
    t: Int,
    o: Dec,
    h: Dec,
    l: Dec,
    c: Dec,
    v: Dec,
    #[serde(default)]
    sum: OptDec,
}

pub(crate) struct CandleRow {
    pub open_s: i64,
    pub o: Dec,
    pub h: Dec,
    pub l: Dec,
    pub c: Dec,
    /// In the base coin.
    pub vol: Decimal,
    pub turnover: Option<Decimal>,
    /// The venue's own closed flag, when the row carries one.
    pub closed: Option<bool>,
}

/// The spot `window_closed` flag as written (`"true"`), anything else read as forming.
fn closed_flag(raw: &str) -> bool {
    match raw {
        "true" => true,
        "false" => false,
        other => {
            tracing::warn!("gateio candles: closed flag `{other}`, read as forming");
            false
        }
    }
}

impl CandleRow {
    /// Spot archive: `[t s, quote volume, close, high, low, open, base volume, closed]`, all
    /// strings.
    fn spot(row: &[Box<RawValue>]) -> Option<CandleRow> {
        let at = |i: usize| row.get(i).map(|v| v.get());
        let dec = |i: usize| at(i).and_then(|t| serde_json::from_str::<Dec>(t).ok());
        let closed = at(7).and_then(|t| serde_json::from_str::<String>(t).ok()).map(|f| closed_flag(&f));
        Some(CandleRow {
            open_s: at(0).and_then(|t| serde_json::from_str::<Int>(t).ok())?.0,
            o: dec(5)?,
            h: dec(3)?,
            l: dec(4)?,
            c: dec(2)?,
            vol: dec(6)?.0,
            turnover: dec(1).map(|d| d.0),
            closed,
        })
    }

    /// `size`: base coin per contract.
    fn futures(r: FuturesRow, size: Decimal) -> Option<CandleRow> {
        Some(CandleRow { open_s: r.t.0, o: r.o, h: r.h, l: r.l, c: r.c, vol: r.v.0.checked_mul(size)?, turnover: r.sum.0, closed: None })
    }

    pub fn candle(&self, instrument: InstrumentId, tf: Timeframe, is_final: bool) -> Result<Candle, ConnectorError> {
        Ok(Candle {
            instrument,
            tf,
            open_ts: ts_secs(self.open_s)?,
            open: self.o.price(),
            high: self.h.price(),
            low: self.l.price(),
            close: self.c.price(),
            volume: Qty(self.vol),
            quote_volume: self.turnover.map(otw_connector_model::Money),
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
    ConnectorError::decode(format!("gateio {what}: {e}"))
}

/// A positive contract size, base coin per contract.
fn positive(d: Decimal) -> Option<Decimal> {
    (d.is_sign_positive() && !d.is_zero()).then_some(d)
}

/// The date a Unix second falls on, for a message.
fn day(secs: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(secs).map(|t| t.date().to_string()).unwrap_or_else(|_| secs.to_string())
}

impl Gateio {
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

    /// A contract's size as already known (the loaded list, or an earlier lookup).
    fn size_of(&self, instrument: &InstrumentId, s: &Session, ctx: &MapCtx<'_>) -> Option<Decimal> {
        if self.book == Book::Spot {
            return Some(Decimal::ONE);
        }
        ctx.instruments.get(instrument).and_then(|c| positive(c.multiplier)).or_else(|| s.sizes.get(&instrument.native).copied())
    }

    /// Ask for the contract first: its size turns contracts into coins, and an unknown
    /// contract is refused by name rather than answered with an empty page.
    fn contract_first(&self, req_id: CorrId, instrument: &InstrumentId, then: Then, s: &mut Session) -> Vec<NativeRequest> {
        s.pending.insert(req_id, Pending::Contract { instrument: instrument.clone(), then });
        vec![NativeRequest::Http(HttpRequest::get("contract", REST, format!("{API}/futures/{SETTLE}/contracts/{}", instrument.native), req_id))]
    }

    #[allow(clippy::too_many_arguments)]
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("{} serves no {tf} history", self.caps.label)));
        }
        let Some(size) = self.size_of(instrument, s, ctx) else {
            return Ok(self.contract_first(req_id, instrument, Then::Candles { tf, range, cursor: cursor.map(str::to_string) }, s));
        };
        Ok(vec![self.candle_request(req_id, instrument, tf, range, cursor, size, s, ctx.received_ts)?])
    }

    /// One call covers at most a page of bars, both ends inclusive, in seconds. Pages walk
    /// backwards from the range's end and stop at its start or at the oldest bar the venue
    /// keeps, whichever is later.
    #[allow(clippy::too_many_arguments)]
    fn candle_request(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, size: Decimal, s: &mut Session, now: Timestamp) -> Result<NativeRequest, ConnectorError> {
        let token = interval(self.book, tf).ok_or_else(|| ConnectorError::unsupported(format!("{} has no {tf} candles", self.book.label())))?;
        let last_open_s = (range.to.millis() - 1).div_euclid(1000);
        let (from_s, to_s, next) = match tf.fixed_secs() {
            Some(step) => {
                let floor = align_up(now.secs() - LOOKBACK_BARS * step, step);
                let lower = align_up(range.from.millis().div_euclid(1000), step).max(floor);
                let end = match cursor {
                    Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
                    None => last_open_s.div_euclid(step) * step,
                };
                if end < floor {
                    return Err(ConnectorError::unsupported(format!(
                        "{} keeps the last 10000 {tf} bars only: its oldest {tf} bar opened on {}. Ask for a later period or a longer timeframe.",
                        self.book.label(),
                        day(floor)
                    )));
                }
                let start = (end - (self.book.page() - 1) * step).max(lower).min(end);
                (start, end, (start > lower).then(|| start - step))
            }
            // A calendar month: a few hundred bars at most, one call.
            None => (range.from.millis().div_euclid(1000), last_open_s, None),
        };
        let h = match self.book {
            Book::Spot => HttpRequest::get("candlesticks", REST, format!("{API}/spot/candlesticks"), req_id).query("currency_pair", &instrument.native),
            Book::Futures => HttpRequest::get("candlesticks", REST, format!("{API}/futures/{SETTLE}/candlesticks"), req_id).query("contract", &instrument.native),
        };
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range, size, next });
        Ok(NativeRequest::Http(h.query("interval", token).query("from", from_s).query("to", to_s)))
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, size: Decimal, next: Option<i64>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let rows: Vec<CandleRow> = match self.book {
            Book::Spot => {
                let raw: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("candlesticks", e))?;
                raw.iter().map(|r| CandleRow::spot(r).ok_or_else(|| ConnectorError::decode("gateio candlesticks: row shape"))).collect::<Result<_, _>>()?
            }
            Book::Futures => {
                let raw: Vec<FuturesRow> = serde_json::from_slice(body).map_err(|e| decode_err("candlesticks", e))?;
                raw.into_iter().map(|r| CandleRow::futures(r, size).ok_or_else(|| ConnectorError::decode("gateio candlesticks: volume overflow"))).collect::<Result<_, _>>()?
            }
        };
        let step_ms = tf.fixed_secs().map(|s| s * 1000).unwrap_or(i64::MAX / 4);
        let settled_before = ctx.received_ts.millis() - step_ms - FUTURES_SETTLE_MS;
        let mut candles = Vec::with_capacity(rows.len());
        for r in &rows {
            if !range.contains(ts_secs(r.open_s)?) {
                continue;
            }
            let is_final = match r.closed {
                Some(c) => c,
                None => r.open_s.saturating_mul(1000) <= settled_before,
            };
            candles.push(r.candle(instrument.clone(), tf, is_final)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = next.map(|n| n.to_string());
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    pub(crate) fn contract_spec(r: &ContractRow) -> Option<InstrumentSpec> {
        let id = r.name.to_ascii_uppercase();
        match r.kind.as_str() {
            "direct" => {}
            "inverse" => return None,
            other => {
                tracing::warn!("gateio contracts: unknown type `{other}` on {id}, left out");
                return None;
            }
        }
        let Some((base, quote)) = contract_pair(&id) else {
            tracing::warn!("gateio contracts: `{id}` is not BASE_QUOTE, left out");
            return None;
        };
        let Some(size) = r.quanto_multiplier.0.and_then(positive) else {
            tracing::warn!("gateio contracts: {id} has no contract size, left out");
            return None;
        };
        let mut spec = InstrumentSpec::spot(InstrumentId::new(Book::Futures.venue(), &id), base, quote);
        spec.asset_class = AssetClass::Perp;
        spec.contract = ContractType::Perp;
        // What one contract is worth in the base; fills and positions count contracts.
        spec.multiplier = size;
        spec.tick_size = r.order_price_round.0.map(Price);
        spec.lot_size = Some(Qty(size));
        spec.min_qty = r.order_size_min.0.and_then(|m| m.checked_mul(size)).map(Qty);
        spec.settlement_ccy = Some(SETTLE.to_ascii_uppercase());
        spec.active = r.status == "trading" && !r.in_delisting;
        spec.note = Some(r.contract_type.clone()).filter(|t| !t.is_empty());
        Some(spec)
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let mut out = Vec::new();
        match self.book {
            Book::Spot => {
                let rows: Vec<PairRow> = serde_json::from_slice(body).map_err(|e| decode_err("currency_pairs", e))?;
                for r in rows {
                    let id = r.id.to_ascii_uppercase();
                    if id.is_empty() {
                        continue;
                    }
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(Book::Spot.venue(), &id), &r.base, &r.quote);
                    // `sellable` and `buyable` are one-way windows around a listing or delisting.
                    spec.active = r.trade_status == "tradable";
                    spec.tick_size = step_of(r.precision).map(Price);
                    spec.lot_size = step_of(r.amount_precision).map(Qty);
                    spec.min_qty = r.min_base_amount.0.map(Qty);
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
            }
            Book::Futures => {
                let rows: Vec<ContractRow> = serde_json::from_slice(body).map_err(|e| decode_err("contracts", e))?;
                for r in &rows {
                    if let Some(spec) = Self::contract_spec(r) {
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
    fn decode_contract(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, then: Then, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let row: ContractRow = serde_json::from_slice(body).map_err(|e| decode_err("contract", e))?;
        let size = Self::contract_spec(&row)
            .map(|c| c.multiplier)
            .ok_or_else(|| ConnectorError::unsupported(format!("{} is not a USDT-settled perpetual this connector reads", instrument.native)))?;
        s.sizes.insert(instrument.native.clone(), size);
        let req = match then {
            Then::Candles { tf, range, cursor } => self.candle_request(corr, &instrument, tf, range, cursor.as_deref(), size, s, ctx.received_ts)?,
            Then::Quote => self.quote_request(corr, &instrument, size, s),
        };
        s.next = Some((corr, req));
        Ok(vec![])
    }

    fn quote_request(&self, req_id: CorrId, instrument: &InstrumentId, size: Decimal, s: &mut Session) -> NativeRequest {
        s.pending.insert(req_id, Pending::Quote { instrument: instrument.clone(), size });
        let h = match self.book {
            Book::Spot => HttpRequest::get("tickers", REST, format!("{API}/spot/tickers"), req_id).query("currency_pair", &instrument.native),
            Book::Futures => HttpRequest::get("tickers", REST, format!("{API}/futures/{SETTLE}/tickers"), req_id).query("contract", &instrument.native),
        };
        NativeRequest::Http(h)
    }

    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, size: Decimal, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let rows: Vec<TickerRow> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
        let Some(t) = rows.into_iter().next() else { return Ok(vec![]) };
        if !t.name().eq_ignore_ascii_case(&instrument.native) {
            return Err(ConnectorError::decode(format!("gateio tickers answered {} for {}", t.name(), instrument.native)));
        }
        let (Some(bid), Some(ask)) = (t.highest_bid.0, t.lowest_ask.0) else { return Ok(vec![]) };
        // Futures sizes are contracts.
        let coins = |q: Option<Decimal>| q.and_then(|q| q.checked_mul(size)).map(Qty);
        Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: coins(t.highest_size.0), ask_qty: coins(t.lowest_size.0) }))])
    }

    /// A market-data refusal that is about the symbol says which one.
    fn about(&self, e: ConnectorError, instrument: &InstrumentId) -> ConnectorError {
        if e.kind == ErrorKind::NotFound {
            let mut out = ConnectorError::not_found(format!("{} does not list `{}`: {}", self.book.label(), instrument.native, e.message));
            out.provider_code = e.provider_code;
            return out;
        }
        e
    }
}

impl Mapper for Gateio {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ConnectorError::not_found(match self.book {
                Book::Spot => format!("Gate.io: bad symbol `{symbol}`, write the pair as Gate.io spells it (BTC_USDT)"),
                Book::Futures => format!("Gate.io futures: bad symbol `{symbol}`, write the contract as Gate.io spells it (BTC_USDT)"),
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
                    Book::Spot => HttpRequest::get("currency_pairs", REST, format!("{API}/spot/currency_pairs"), *req_id),
                    Book::Futures => HttpRequest::get("contracts", REST, format!("{API}/futures/{SETTLE}/contracts"), *req_id),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => match self.size_of(instrument, s, ctx) {
                Some(size) => Ok(vec![self.quote_request(*req_id, instrument, size, s)]),
                None => Ok(self.contract_first(*req_id, instrument, Then::Quote, s)),
            },
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert(*req_id, Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("tickers", REST, format!("{API}/spot/tickers"), *req_id))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(self.book, cmd, &mut s.pending),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: Gate.io accepts the host clock within 60 s.
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
            return match pending {
                Pending::Candles { instrument, .. } | Pending::Quote { instrument, .. } | Pending::Contract { instrument, .. } => Err(self.about(e, &instrument)),
                Pending::Account(p) => match account::on_refusal(p, corr, e, &mut s.pending) {
                    Ok(next) => {
                        s.next = Some((corr, next));
                        Ok(vec![])
                    }
                    Err(e) => Err(e),
                },
                _ => Err(e),
            };
        }
        match pending {
            Pending::Candles { instrument, tf, range, size, next } => self.decode_candles(corr, body, instrument, tf, range, size, next, ctx),
            Pending::Contract { instrument, then } => self.decode_contract(corr, body, instrument, then, s, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::Quote { instrument, size } => self.decode_quote(corr, body, instrument, size, ctx),
            Pending::Board => {
                let rows: Vec<TickerRow> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
                let board = rows
                    .into_iter()
                    .filter_map(|t| t.last.0.map(|p| LastPrice { instrument: InstrumentId::new(Book::Spot.venue(), &t.name().to_ascii_uppercase()), price: Price(p) }))
                    .filter(|p| !p.instrument.native.is_empty())
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Account(p) => {
                let (evs, next) = account::decode(self.book, p, corr, body, &mut s.pending, ctx)?;
                if let Some(req) = next {
                    s.next = Some((corr, req));
                }
                Ok(evs)
            }
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

    /// Futures volume is counted in contracts, so a futures pane reads its contract's size
    /// from the list before it subscribes.
    fn live_requires_instruments(&self) -> bool {
        self.book == Book::Futures
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(self.book, &mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(Some(Arc::new(signer::GateSigner::from_secrets(secrets)?)))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        (self.book == Book::Spot).then(|| format!("{}_{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }
}

#[cfg(test)]
mod tests;
