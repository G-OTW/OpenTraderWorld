//! Bitget v2, spot and USDT-M futures, one adapter per book: candle history, the instrument
//! lists, best bid/ask, the spot price board, live candles and the account read only.
//! `BTCUSDT` is a spot pair and a perpetual at once and they are different series, so which
//! book a connector reads is its setting, never a guess at the ticker.
//! Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Capabilities, CapsError, Candle, Command, ConnectorError, ContractType, CorrId, DataType, Envelope, ErrorKind, Event,
    HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TfUnit, TimeRange, Timeframe,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::value::RawValue;

const REST: &str = "rest";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[(REST, "https://api.bitget.com"), (WS, "wss://ws.bitget.com")];
/// Candles per history page.
const PAGE: u32 = 200;
/// How long after a period closes Bitget still rewrites its candle (measured around 25 s on
/// the spot board). A bar younger than its period plus this is not final.
const SETTLE_MS: i64 = 60_000;
/// The only futures product type this adapter reads.
pub const FUTURES: &str = "USDT-FUTURES";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Spot,
    UsdtFutures,
}

impl Book {
    pub fn venue(self) -> &'static str {
        match self {
            Book::Spot => "BITGET",
            Book::UsdtFutures => "BITGET_USDTM",
        }
    }

    /// The `instType` the socket names this book by.
    fn inst_type(self) -> &'static str {
        match self {
            Book::Spot => "SPOT",
            Book::UsdtFutures => FUTURES,
        }
    }
}

#[derive(Clone)]
pub struct Bitget {
    caps: Capabilities,
    book: Book,
}

impl Bitget {
    pub fn spot() -> Result<Self, CapsError> {
        Ok(Bitget { caps: Capabilities::from_toml(include_str!("capabilities_spot.toml"))?, book: Book::Spot })
    }

    pub fn usdt_futures() -> Result<Self, CapsError> {
        Ok(Bitget { caps: Capabilities::from_toml(include_str!("capabilities_futures.toml"))?, book: Book::UsdtFutures })
    }

    pub fn book(&self) -> Book {
        self.book
    }
}

enum Pending {
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange },
    Instruments,
    Quote { instrument: InstrumentId },
    Board,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    live: stream::LiveState,
}

/// Bitget's granularity token. The two books spell the intraday ones differently; daily and
/// weekly are the UTC-anchored variants on both.
fn granularity(book: Book, tf: Timeframe) -> Option<&'static str> {
    Some(match (book, tf.n, tf.unit) {
        (Book::Spot, 1, TfUnit::Minute) => "1min",
        (Book::Spot, 5, TfUnit::Minute) => "5min",
        (Book::Spot, 15, TfUnit::Minute) => "15min",
        (Book::Spot, 30, TfUnit::Minute) => "30min",
        (Book::Spot, 1, TfUnit::Hour) => "1h",
        (Book::Spot, 4, TfUnit::Hour) => "4h",
        (Book::UsdtFutures, 1, TfUnit::Minute) => "1m",
        (Book::UsdtFutures, 5, TfUnit::Minute) => "5m",
        (Book::UsdtFutures, 15, TfUnit::Minute) => "15m",
        (Book::UsdtFutures, 30, TfUnit::Minute) => "30m",
        (Book::UsdtFutures, 1, TfUnit::Hour) => "1H",
        (Book::UsdtFutures, 4, TfUnit::Hour) => "4H",
        (_, 1, TfUnit::Day) => "1Dutc",
        (_, 1, TfUnit::Week) => "1Wutc",
        _ => return None,
    })
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
    base_coin: String,
    #[serde(default)]
    quote_coin: String,
    /// Spot: `online`; futures: `symbolStatus` = `normal`.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    symbol_status: Option<String>,
    #[serde(default)]
    symbol_type: Option<String>,
    #[serde(default)]
    min_trade_amount: OptDec,
    #[serde(default)]
    min_trade_num: OptDec,
    #[serde(default)]
    size_multiplier: OptDec,
    #[serde(default)]
    delivery_time: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TickerRow {
    symbol: String,
    #[serde(default)]
    bid_pr: OptDec,
    #[serde(default)]
    bid_sz: OptDec,
    #[serde(default)]
    ask_pr: OptDec,
    #[serde(default)]
    ask_sz: OptDec,
    #[serde(default)]
    last_pr: OptDec,
}

/// `[openTime, open, high, low, close, baseVolume, quoteVolume, ...]`, strings (numbers too).
pub(crate) struct CandleRow {
    pub open_ms: i64,
    pub o: Dec,
    pub h: Dec,
    pub l: Dec,
    pub c: Dec,
    pub vol: Dec,
    pub vol_quote: Option<Dec>,
}

impl CandleRow {
    pub fn read(row: &[Box<RawValue>]) -> Option<CandleRow> {
        let at = |i: usize| row.get(i).map(|v| v.get());
        let dec = |i: usize| at(i).and_then(|t| serde_json::from_str::<Dec>(t).ok());
        Some(CandleRow {
            open_ms: at(0).and_then(|t| serde_json::from_str::<Int>(t).ok())?.0,
            o: dec(1)?,
            h: dec(2)?,
            l: dec(3)?,
            c: dec(4)?,
            vol: dec(5)?,
            vol_quote: dec(6),
        })
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
            volume: self.vol.qty(),
            quote_volume: self.vol_quote.map(|q| q.money()),
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
    ConnectorError::decode(format!("bitget {what}: {e}"))
}

impl Bitget {
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
        let token = granularity(self.book, tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("{} serves no {tf} history here", self.caps.label)))?;
        // History reads backwards from `endTime`.
        let end = match cursor {
            Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
            None => range.to.millis(),
        };
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range });
        let path = if self.book == Book::Spot { "/api/v2/spot/market/history-candles" } else { "/api/v2/mix/market/history-candles" };
        let mut h = HttpRequest::get("historyCandles", REST, path, req_id)
            .query("symbol", &instrument.native)
            .query("granularity", token)
            .query("endTime", end)
            .query("limit", PAGE);
        if self.book == Book::UsdtFutures {
            h = h.query("productType", FUTURES);
        }
        Ok(vec![NativeRequest::Http(h)])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Data<Vec<Vec<Box<RawValue>>>> = serde_json::from_slice(body).map_err(|e| decode_err("history-candles", e))?;
        let full = page.data.len() >= PAGE as usize;
        // No close time and no confirm flag: how old a bar is decides whether it is final.
        let step_ms = tf.fixed_secs().map(|s| s * 1000).unwrap_or(i64::MAX / 4);
        let closed_before = ctx.received_ts.millis() - step_ms - SETTLE_MS;
        let mut candles = Vec::with_capacity(page.data.len());
        let mut oldest: Option<i64> = None;
        for raw in &page.data {
            let r = CandleRow::read(raw).ok_or_else(|| ConnectorError::decode("bitget history-candles: row shape"))?;
            oldest = Some(oldest.map_or(r.open_ms, |o| o.min(r.open_ms)));
            if !range.contains(ts_millis(r.open_ms)?) {
                continue;
            }
            candles.push(r.candle(instrument.clone(), tf, r.open_ms <= closed_before)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = match (full, oldest) {
            (true, Some(o)) if o > range.from.millis() => Some((o - 1).to_string()),
            _ => None,
        };
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Data<Vec<SymbolRow>> = serde_json::from_slice(body).map_err(|e| decode_err("symbols", e))?;
        let mut out = Vec::with_capacity(page.data.len() + 1);
        for s in page.data {
            let id = s.symbol.to_ascii_uppercase();
            if id.is_empty() {
                continue;
            }
            let mut spec = InstrumentSpec::spot(InstrumentId::new(self.book.venue(), &id), &s.base_coin, &s.quote_coin);
            // A delisted or halted pair is not worth offering: it downloads nothing.
            let state = s.status.as_deref().or(s.symbol_status.as_deref());
            spec.active = matches!(state, None | Some("online") | Some("normal"));
            spec.min_qty = s.min_trade_num.0.or(s.min_trade_amount.0).map(Qty);
            if self.book == Book::UsdtFutures {
                let perp = s.symbol_type.as_deref() != Some("delivery");
                spec.asset_class = if perp { AssetClass::Perp } else { AssetClass::Future };
                spec.contract = if perp { ContractType::Perp } else { ContractType::Future };
                spec.lot_size = s.size_multiplier.0.map(Qty);
                if !perp {
                    spec.expiry = s.delivery_time.0.and_then(|d| i64::try_from(d.trunc()).ok()).map(ts_millis).transpose()?;
                }
            }
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for Bitget {
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
            return Err(ConnectorError::not_found(format!("Bitget: bad symbol `{symbol}`, write the pair as Bitget spells it (BTCUSDT)")));
        }
        Ok(InstrumentId::new(self.book.venue(), &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert(*req_id, Pending::Instruments);
                let h = match self.book {
                    Book::Spot => HttpRequest::get("symbols", REST, "/api/v2/spot/public/symbols", *req_id),
                    Book::UsdtFutures => HttpRequest::get("contracts", REST, "/api/v2/mix/market/contracts", *req_id).query("productType", FUTURES),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert(*req_id, Pending::Quote { instrument: instrument.clone() });
                let h = match self.book {
                    Book::Spot => HttpRequest::get("tickers", REST, "/api/v2/spot/market/tickers", *req_id).query("symbol", &instrument.native),
                    Book::UsdtFutures => HttpRequest::get("ticker", REST, "/api/v2/mix/market/ticker", *req_id)
                        .query("symbol", &instrument.native)
                        .query("productType", FUTURES),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert(*req_id, Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("board", REST, "/api/v2/spot/market/tickers", *req_id))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(self.book, cmd, &mut s.pending),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: Bitget accepts the host clock (as the legacy reader did).
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
            RawFrame::Ws { payload, .. } => return stream::decode(payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s.pending.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr}")))?;
        if let Some(e) = errors::from_frame(frame) {
            return Err(e);
        }
        match pending {
            Pending::Candles { instrument, tf, range } => self.decode_candles(corr, body, instrument, tf, range, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, ctx),
            Pending::Quote { instrument } => {
                let page: Data<Vec<TickerRow>> = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
                let Some(t) = page.data.into_iter().next() else { return Ok(vec![]) };
                if !t.symbol.eq_ignore_ascii_case(&instrument.native) {
                    return Err(ConnectorError::decode(format!("bitget ticker answered {} for {}", t.symbol, instrument.native)));
                }
                let (Some(bid), Some(ask)) = (t.bid_pr.0, t.ask_pr.0) else { return Ok(vec![]) };
                Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: t.bid_sz.0.map(Qty), ask_qty: t.ask_sz.0.map(Qty) }))])
            }
            Pending::Board => {
                let page: Data<Vec<TickerRow>> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
                let board = page
                    .data
                    .into_iter()
                    .filter_map(|t| t.last_pr.0.map(|p| LastPrice { instrument: InstrumentId::new(Book::Spot.venue(), &t.symbol.to_ascii_uppercase()), price: Price(p) }))
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Account(p) => account::decode(self.book, p, corr, body, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.remove(&corr_id);
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| SecretString::from(v.to_string()))
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
        };
        Ok(Some(Arc::new(signer::BitgetSigner { key: get("api_key")?, secret: get("api_secret")?, passphrase: get("api_passphrase")? })))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        Some(format!("{}{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }
}

#[cfg(test)]
mod tests;
