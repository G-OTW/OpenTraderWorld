//! MEXC, spot (API v3) and the perpetual contracts settled in their quote coin (contract API
//! v1), one adapter per book: candle history, the instrument lists, best bid/ask, the spot
//! price board, live candles and the account read only. A spot pair is written `BTCUSDT`, a
//! contract `BTC_USDT`; which book a connector reads is its setting all the same. The
//! coin-settled (inverse) contracts are not served. Endpoint-by-endpoint mapping in
//! PROVIDER.md.

mod account;
mod errors;
mod proto;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Candle, Capabilities, CapsError, Command, ConnectorError, ContractType, CorrId, DataType, Decimal, Envelope, ErrorKind,
    Event, HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_millis, ts_secs, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use serde::Deserialize;
use serde_json::value::RawValue;

const REST: &str = "rest";
const FUT: &str = "contract";
const WS: &str = "ws";
const FUT_WS: &str = "contract-ws";
const BASES: &[(&str, &str)] =
    &[(REST, "https://api.mexc.com"), (FUT, "https://contract.mexc.com"), (WS, "wss://wbs-api.mexc.com"), (FUT_WS, "wss://contract.mexc.com")];
/// Candles per history call: spot answers 500 at most whatever `limit` says (its docs say
/// 1000), the contract API 2000 (both measured on 2026-10-19).
const SPOT_PAGE: i64 = 500;
const FUTURES_PAGE: i64 = 2000;
/// How far back the archives answer, by timeframe (measured on 2026-10-19 on BTC, both
/// books): minutes 30 days, 5 and 15 minutes 360 days, 30 minutes 360 days on spot and from
/// the listing on futures, an hour and longer from the listing. The window moves with the
/// clock, and a range that starts before it answers nothing at all, so a request is cut at
/// the window plus an hour of margin.
const DAY: i64 = 86_400;
const LOOKBACK_MARGIN: i64 = 3_600;
/// Neither archive flags a forming bar. Measured on 2026-10-19 by polling both through ten
/// minute boundaries over BTC, ETH, SOL and XRP: a closed minute was rewritten up to 2.5 s
/// after its close on spot (21 rewrites) and up to 12.8 s on futures (26 rewrites). A bar is
/// final once its period plus this has passed, about four times the worst one seen.
const SPOT_SETTLE_MS: i64 = 10_000;
const FUTURES_SETTLE_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Spot,
    Futures,
}

impl Book {
    pub fn venue(self) -> &'static str {
        match self {
            Book::Spot => "MEXC",
            Book::Futures => "MEXC_FUTURES",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Book::Spot => "MEXC",
            Book::Futures => "MEXC futures",
        }
    }

    fn page(self) -> i64 {
        match self {
            Book::Spot => SPOT_PAGE,
            Book::Futures => FUTURES_PAGE,
        }
    }

    fn settle_ms(self) -> i64 {
        match self {
            Book::Spot => SPOT_SETTLE_MS,
            Book::Futures => FUTURES_SETTLE_MS,
        }
    }

    /// Whether a name is spelled the way this book writes it: `BTC_USDT` for a contract,
    /// `BTCUSDT` for a pair.
    pub(crate) fn owns(self, native: &str) -> bool {
        native.contains('_') == (self == Book::Futures)
    }
}

#[derive(Clone)]
pub struct Mexc {
    caps: Capabilities,
    book: Book,
}

impl Mexc {
    pub fn spot() -> Result<Self, CapsError> {
        Ok(Mexc { caps: Capabilities::from_toml(include_str!("capabilities_spot.toml"))?, book: Book::Spot })
    }

    pub fn futures() -> Result<Self, CapsError> {
        Ok(Mexc { caps: Capabilities::from_toml(include_str!("capabilities_futures.toml"))?, book: Book::Futures })
    }

    pub fn book(&self) -> Book {
        self.book
    }
}

enum Pending {
    /// `size`: base coin per unit the venue counts volume in (one on spot, the contract's
    /// size on futures). `next`: where the following page starts, when one is due.
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange, size: Decimal, next: Option<i64> },
    /// A contract's terms, asked before its candles.
    Contract { instrument: InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<String> },
    Instruments,
    Quote { instrument: InstrumentId },
    Board,
    Time,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    /// Contract sizes already read, base coin per contract, by contract name.
    sizes: HashMap<String, Decimal>,
    /// The request an answer named as the next step, sent as its follow-up.
    next: Option<(CorrId, NativeRequest)>,
    live: stream::LiveState,
}

/// The archive's interval token. Spot speaks Binance's tokens (`60m`, `1W`, `1M`); the
/// contract archive and both sockets speak `Min1`..`Month1`.
fn archive_interval(book: Book, tf: Timeframe) -> Option<&'static str> {
    match book {
        Book::Spot => Some(match (tf.n, tf.unit) {
            (1, TfUnit::Minute) => "1m",
            (5, TfUnit::Minute) => "5m",
            (15, TfUnit::Minute) => "15m",
            (30, TfUnit::Minute) => "30m",
            (1, TfUnit::Hour) => "60m",
            (4, TfUnit::Hour) => "4h",
            (8, TfUnit::Hour) => "8h",
            (1, TfUnit::Day) => "1d",
            (1, TfUnit::Week) => "1W",
            (1, TfUnit::Month) => "1M",
            _ => return None,
        }),
        Book::Futures => socket_interval(tf),
    }
}

pub(crate) fn socket_interval(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "Min1",
        (5, TfUnit::Minute) => "Min5",
        (15, TfUnit::Minute) => "Min15",
        (30, TfUnit::Minute) => "Min30",
        (1, TfUnit::Hour) => "Min60",
        (4, TfUnit::Hour) => "Hour4",
        (8, TfUnit::Hour) => "Hour8",
        (1, TfUnit::Day) => "Day1",
        (1, TfUnit::Week) => "Week1",
        (1, TfUnit::Month) => "Month1",
        _ => return None,
    })
}

/// Days of history the archive keeps for a timeframe, `None` back to the listing.
fn lookback_days(book: Book, tf: Timeframe) -> Option<i64> {
    match (tf.n, tf.unit, book) {
        (1, TfUnit::Minute, _) => Some(30),
        (5 | 15, TfUnit::Minute, _) => Some(360),
        (30, TfUnit::Minute, Book::Spot) => Some(360),
        _ => None,
    }
}

fn align_up(t: i64, step: i64) -> i64 {
    -(-t).div_euclid(step) * step
}

/// `10^-places`.
fn step_of(places: Option<Int>) -> Option<Decimal> {
    let p = u32::try_from(places?.0).ok().filter(|p| *p <= 28)?;
    Some(Decimal::new(1, p))
}

/// The contract API's answer envelope.
#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
struct ExchangeInfo {
    #[serde(default)]
    symbols: Vec<PairRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRow {
    symbol: String,
    /// `1` online, `2` paused, `3` offline.
    #[serde(default)]
    status: String,
    #[serde(default)]
    base_asset: String,
    #[serde(default)]
    quote_asset: String,
    /// Decimal places of the price.
    #[serde(default)]
    quote_precision: Option<Int>,
    /// The quantity step, as a decimal.
    #[serde(default)]
    base_size_precision: OptDec,
    #[serde(default)]
    is_spot_trading_allowed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContractRow {
    pub symbol: String,
    #[serde(default)]
    base_coin: String,
    #[serde(default)]
    quote_coin: String,
    #[serde(default)]
    settle_coin: String,
    /// Base coin per contract.
    #[serde(default)]
    pub contract_size: OptDec,
    #[serde(default)]
    price_unit: OptDec,
    /// Contracts.
    #[serde(default)]
    vol_unit: OptDec,
    #[serde(default)]
    min_vol: OptDec,
    /// `0` enabled, `1` delivery, `2` delivered, `3` offline, `4` paused.
    #[serde(default)]
    state: Option<Int>,
    /// `1` perpetual.
    #[serde(default)]
    future_type: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookTicker {
    symbol: String,
    #[serde(default)]
    bid_price: OptDec,
    #[serde(default)]
    bid_qty: OptDec,
    #[serde(default)]
    ask_price: OptDec,
    #[serde(default)]
    ask_qty: OptDec,
}

#[derive(Deserialize)]
struct ContractTicker {
    symbol: String,
    #[serde(default)]
    bid1: OptDec,
    #[serde(default)]
    ask1: OptDec,
}

#[derive(Deserialize)]
struct PriceRow {
    symbol: String,
    #[serde(default)]
    price: OptDec,
}

/// The contract archive answers columns, one list per field.
#[derive(Deserialize)]
struct Columns {
    #[serde(default)]
    time: Vec<Int>,
    #[serde(default)]
    open: Vec<Dec>,
    #[serde(default)]
    close: Vec<Dec>,
    #[serde(default)]
    high: Vec<Dec>,
    #[serde(default)]
    low: Vec<Dec>,
    /// Contracts.
    #[serde(default)]
    vol: Vec<Dec>,
    /// Quote turnover.
    #[serde(default)]
    amount: Vec<Dec>,
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
}

impl CandleRow {
    /// Spot archive: `[open ms, open, high, low, close, base volume, close ms, quote volume]`.
    fn spot(row: &[Box<RawValue>]) -> Option<CandleRow> {
        let dec = |i: usize| row.get(i).and_then(|t| serde_json::from_str::<Dec>(t.get()).ok());
        let open_ms = row.first().and_then(|t| serde_json::from_str::<Int>(t.get()).ok())?.0;
        Some(CandleRow { open_s: open_ms.div_euclid(1000), o: dec(1)?, h: dec(2)?, l: dec(3)?, c: dec(4)?, vol: dec(5)?.0, turnover: dec(7).map(|d| d.0) })
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
    ConnectorError::decode(format!("mexc {what}: {e}"))
}

fn positive(d: Decimal) -> Option<Decimal> {
    (d.is_sign_positive() && !d.is_zero()).then_some(d)
}

/// The first second of the calendar month after the one `secs` falls in.
fn month_after(secs: i64) -> Option<i64> {
    let d = time::OffsetDateTime::from_unix_timestamp(secs).ok()?.date();
    let (y, m) = if d.month() == time::Month::December { (d.year() + 1, time::Month::January) } else { (d.year(), d.month().next()) };
    Some(time::Date::from_calendar_date(y, m, 1).ok()?.midnight().assume_utc().unix_timestamp())
}

/// The date a Unix second falls on, for a message.
fn day(secs: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(secs).map(|t| t.date().to_string()).unwrap_or_else(|_| secs.to_string())
}

impl Mexc {
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

    fn size_of(&self, instrument: &InstrumentId, s: &Session, ctx: &MapCtx<'_>) -> Option<Decimal> {
        if self.book == Book::Spot {
            return Some(Decimal::ONE);
        }
        ctx.instruments.get(instrument).and_then(|c| positive(c.multiplier)).or_else(|| s.sizes.get(&instrument.native).copied())
    }

    #[allow(clippy::too_many_arguments)]
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("{} serves no {tf} history", self.caps.label)));
        }
        let Some(size) = self.size_of(instrument, s, ctx) else {
            // The contract's size turns contracts into coins, and an unknown contract is
            // refused by name rather than answered with an empty page.
            s.pending.insert(req_id, Pending::Contract { instrument: instrument.clone(), tf, range, cursor: cursor.map(str::to_string) });
            let h = HttpRequest::get("contract", FUT, "/api/v1/contract/detail", req_id).query("symbol", &instrument.native);
            return Ok(vec![NativeRequest::Http(h)]);
        };
        Ok(vec![self.candle_request(req_id, instrument, tf, range, cursor, size, s, ctx.received_ts)?])
    }

    /// Pages walk forwards from the range's start (or the oldest bar the archive keeps,
    /// whichever is later), a page of bars at a time.
    #[allow(clippy::too_many_arguments)]
    fn candle_request(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, size: Decimal, s: &mut Session, now: Timestamp) -> Result<NativeRequest, ConnectorError> {
        let token = archive_interval(self.book, tf).ok_or_else(|| ConnectorError::unsupported(format!("{} has no {tf} candles", self.book.label())))?;
        let to_s = range.to.millis().div_euclid(1000);
        // The window `[start, end)` in seconds.
        let (start, end, next) = match tf.fixed_secs() {
            Some(step) => {
                let floor = lookback_days(self.book, tf).map(|d| align_up(now.secs() - d * DAY + LOOKBACK_MARGIN, step));
                let start = match cursor {
                    Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
                    None => align_up(range.from.millis().div_euclid(1000), step),
                };
                let start = match floor {
                    Some(f) if to_s <= f => {
                        return Err(ConnectorError::unsupported(format!(
                            "{} keeps {} days of {tf} bars only: its oldest {tf} bar opened on {}. Ask for a later period or a longer timeframe.",
                            self.book.label(),
                            lookback_days(self.book, tf).unwrap_or_default(),
                            day(f)
                        )));
                    }
                    Some(f) => start.max(f),
                    None => start,
                };
                let end = (start + self.book.page() * step).min(to_s).max(start);
                (start, end, (end < to_s).then_some(end))
            }
            // A calendar month: a few hundred bars at most, one call.
            None => (range.from.millis().div_euclid(1000), to_s, None),
        };
        let h = match self.book {
            // `[startTime, endTime)` in milliseconds (measured).
            Book::Spot => HttpRequest::get("klines", REST, "/api/v3/klines", req_id)
                .query("symbol", &instrument.native)
                .query("interval", token)
                .query("startTime", start * 1000)
                .query("endTime", end * 1000)
                .query("limit", SPOT_PAGE),
            // `start` and `end` in seconds, both inclusive (measured).
            Book::Futures => HttpRequest::get("kline", FUT, format!("/api/v1/contract/kline/{}", instrument.native), req_id)
                .query("interval", token)
                .query("start", start)
                .query("end", (end - 1).max(start)),
        };
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range, size, next });
        Ok(NativeRequest::Http(h))
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, size: Decimal, next: Option<i64>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let rows: Vec<CandleRow> = match self.book {
            Book::Spot => {
                let raw: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("klines", e))?;
                raw.iter().map(|r| CandleRow::spot(r).ok_or_else(|| ConnectorError::decode("mexc klines: row shape"))).collect::<Result<_, _>>()?
            }
            Book::Futures => {
                let d: Data<Columns> = serde_json::from_slice(body).map_err(|e| decode_err("kline", e))?;
                let c = d.data;
                let n = c.time.len();
                if [c.open.len(), c.close.len(), c.high.len(), c.low.len(), c.vol.len()].iter().any(|l| *l != n) {
                    return Err(ConnectorError::decode("mexc kline: columns of different lengths"));
                }
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let vol = c.vol[i].0.checked_mul(size).ok_or_else(|| ConnectorError::decode("mexc kline: volume overflow"))?;
                    out.push(CandleRow { open_s: c.time[i].0, o: c.open[i], h: c.high[i], l: c.low[i], c: c.close[i], vol, turnover: c.amount.get(i).map(|a| a.0) });
                }
                out
            }
        };
        let step_ms = tf.fixed_secs().map(|s| s * 1000).unwrap_or(i64::MAX / 4);
        let settled_before = ctx.received_ts.millis() - step_ms - self.book.settle_ms();
        let mut candles = Vec::with_capacity(rows.len());
        for r in &rows {
            if !range.contains(ts_secs(r.open_s)?) {
                continue;
            }
            let is_final = match tf.fixed_secs() {
                Some(_) => r.open_s.saturating_mul(1000) <= settled_before,
                // A month is over when the next one has opened plus the settle window.
                None => month_after(r.open_s).is_some_and(|n| n.saturating_mul(1000) + self.book.settle_ms() <= ctx.received_ts.millis()),
            };
            candles.push(r.candle(instrument.clone(), tf, is_final)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = next.map(|n| n.to_string());
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    /// A perpetual settled in its quote coin; anything else is not served.
    pub(crate) fn contract_spec(r: &ContractRow) -> Option<InstrumentSpec> {
        let id = r.symbol.to_ascii_uppercase();
        if r.future_type.is_some_and(|t| t.0 != 1) || r.settle_coin != r.quote_coin || r.base_coin.is_empty() || r.quote_coin.is_empty() {
            return None;
        }
        let Some(size) = r.contract_size.0.and_then(positive) else {
            tracing::warn!("mexc contracts: {id} has no contract size, left out");
            return None;
        };
        let mut spec = InstrumentSpec::spot(InstrumentId::new(Book::Futures.venue(), &id), &r.base_coin.to_ascii_uppercase(), &r.quote_coin.to_ascii_uppercase());
        spec.asset_class = AssetClass::Perp;
        spec.contract = ContractType::Perp;
        // What one contract is worth in the base; fills and positions count contracts.
        spec.multiplier = size;
        spec.tick_size = r.price_unit.0.map(Price);
        spec.lot_size = r.vol_unit.0.unwrap_or(Decimal::ONE).checked_mul(size).map(Qty);
        spec.min_qty = r.min_vol.0.and_then(|m| m.checked_mul(size)).map(Qty);
        spec.settlement_ccy = Some(r.settle_coin.to_ascii_uppercase());
        spec.active = r.state.is_some_and(|s| s.0 == 0);
        Some(spec)
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let mut out = Vec::new();
        match self.book {
            Book::Spot => {
                let info: ExchangeInfo = serde_json::from_slice(body).map_err(|e| decode_err("exchangeInfo", e))?;
                for r in info.symbols {
                    let id = r.symbol.to_ascii_uppercase();
                    if id.is_empty() || r.base_asset.is_empty() || r.quote_asset.is_empty() {
                        continue;
                    }
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(Book::Spot.venue(), &id), &r.base_asset.to_ascii_uppercase(), &r.quote_asset.to_ascii_uppercase());
                    spec.active = r.status == "1" && r.is_spot_trading_allowed;
                    spec.tick_size = step_of(r.quote_precision).map(Price);
                    spec.lot_size = r.base_size_precision.0.and_then(positive).map(Qty);
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
            }
            Book::Futures => {
                let d: Data<Vec<ContractRow>> = serde_json::from_slice(body).map_err(|e| decode_err("contract detail", e))?;
                for r in &d.data {
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

    /// The contract a candle page was waiting for: its size kept, the page queued.
    #[allow(clippy::too_many_arguments)]
    fn decode_contract(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<String>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let d: Data<ContractRow> = serde_json::from_slice(body).map_err(|e| decode_err("contract detail", e))?;
        let size = Self::contract_spec(&d.data)
            .map(|c| c.multiplier)
            .ok_or_else(|| ConnectorError::unsupported(format!("{} is not a perpetual settled in its quote coin, which this connector reads", instrument.native)))?;
        s.sizes.insert(instrument.native.clone(), size);
        let req = self.candle_request(corr, &instrument, tf, range, cursor.as_deref(), size, s, ctx.received_ts)?;
        s.next = Some((corr, req));
        Ok(vec![])
    }

    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (name, bid, ask, bid_qty, ask_qty) = match self.book {
            Book::Spot => {
                let t: BookTicker = serde_json::from_slice(body).map_err(|e| decode_err("bookTicker", e))?;
                (t.symbol, t.bid_price.0, t.ask_price.0, t.bid_qty.0.map(Qty), t.ask_qty.0.map(Qty))
            }
            // The contract ticker carries no sizes.
            Book::Futures => {
                let t: Data<ContractTicker> = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
                (t.data.symbol, t.data.bid1.0, t.data.ask1.0, None, None)
            }
        };
        if !name.eq_ignore_ascii_case(&instrument.native) {
            return Err(ConnectorError::decode(format!("mexc ticker answered {name} for {}", instrument.native)));
        }
        let (Some(bid), Some(ask)) = (bid.and_then(positive), ask.and_then(positive)) else { return Ok(vec![]) };
        Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty, ask_qty }))])
    }

    /// A market-data refusal that is about the symbol says which one, and how this book
    /// spells its names.
    fn about(&self, e: ConnectorError, instrument: &InstrumentId) -> ConnectorError {
        if e.kind != ErrorKind::NotFound {
            return e;
        }
        let hint = match (self.book, self.book.owns(&instrument.native)) {
            (Book::Spot, false) => " MEXC spot pairs are written without a separator (BTCUSDT); BTC_USDT names a futures contract.",
            (Book::Futures, false) => " MEXC futures contracts are written with an underscore (BTC_USDT); BTCUSDT names a spot pair.",
            _ => "",
        };
        let mut out = ConnectorError::not_found(format!("{} does not list `{}`: {}.{hint}", self.book.label(), instrument.native, e.message));
        out.provider_code = e.provider_code;
        out
    }
}

impl Mapper for Mexc {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// Both books take either spelling: a broker account names spot pairs and contracts in
    /// one list, and each book reads its own. The venue refuses a name it does not list.
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(ConnectorError::not_found(match self.book {
                Book::Spot => format!("MEXC: bad symbol `{symbol}`, write the pair as MEXC spells it (BTCUSDT)"),
                Book::Futures => format!("MEXC futures: bad symbol `{symbol}`, write the contract as MEXC spells it (BTC_USDT)"),
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
                    Book::Spot => HttpRequest::get("exchangeInfo", REST, "/api/v3/exchangeInfo", *req_id),
                    Book::Futures => HttpRequest::get("contracts", FUT, "/api/v1/contract/detail", *req_id),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert(*req_id, Pending::Quote { instrument: instrument.clone() });
                let h = match self.book {
                    Book::Spot => HttpRequest::get("bookTicker", REST, "/api/v3/ticker/bookTicker", *req_id),
                    Book::Futures => HttpRequest::get("ticker", FUT, "/api/v1/contract/ticker", *req_id),
                };
                Ok(vec![NativeRequest::Http(h.query("symbol", &instrument.native))])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert(*req_id, Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("tickerPrice", REST, "/api/v3/ticker/price", *req_id))])
            }
            Command::SyncTime { req_id } => {
                s.pending.insert(*req_id, Pending::Time);
                let h = match self.book {
                    Book::Spot => HttpRequest::get("time", REST, "/api/v3/time", *req_id),
                    Book::Futures => HttpRequest::get("ping", FUT, "/api/v1/contract/ping", *req_id),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(self.book, cmd, &mut s.pending),
            Command::Connect | Command::Disconnect => Ok(vec![]),
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
                Pending::Candles { instrument, .. } | Pending::Quote { instrument } | Pending::Contract { instrument, .. } => Err(self.about(e, &instrument)),
                Pending::Account(p) => Err(account::hint(self.book, &p, e)),
                _ => Err(e),
            };
        }
        match pending {
            Pending::Candles { instrument, tf, range, size, next } => self.decode_candles(corr, body, instrument, tf, range, size, next, ctx),
            Pending::Contract { instrument, tf, range, cursor } => self.decode_contract(corr, body, instrument, tf, range, cursor, s, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::Quote { instrument } => self.decode_quote(corr, body, instrument, ctx),
            Pending::Board => {
                let rows: Vec<PriceRow> = serde_json::from_slice(body).map_err(|e| decode_err("ticker price", e))?;
                let board = rows
                    .into_iter()
                    .filter(|r| !r.symbol.is_empty())
                    .filter_map(|r| r.price.0.map(|p| LastPrice { instrument: InstrumentId::new(Book::Spot.venue(), &r.symbol.to_ascii_uppercase()), price: Price(p) }))
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Time => {
                let ms = match self.book {
                    Book::Spot => {
                        #[derive(Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct T {
                            server_time: Int,
                        }
                        serde_json::from_slice::<T>(body).map_err(|e| decode_err("time", e))?.server_time.0
                    }
                    Book::Futures => serde_json::from_slice::<Data<Int>>(body).map_err(|e| decode_err("ping", e))?.data.0,
                };
                Ok(vec![env(ctx, corr, Event::ServerTime(ts_millis(ms)?))])
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

    /// Futures volume is counted in contracts, so a futures pane reads its contract's size
    /// from the list before it subscribes.
    fn live_requires_instruments(&self) -> bool {
        self.book == Book::Futures
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(self.book, &mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(Some(match self.book {
            Book::Spot => Arc::new(signer::spot(secrets)?),
            Book::Futures => Arc::new(signer::ContractSigner::from_secrets(secrets)?),
        }))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        (self.book == Book::Spot).then(|| format!("{}{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }
}

#[cfg(test)]
mod tests;
