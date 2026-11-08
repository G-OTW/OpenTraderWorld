//! Bybit v5, one adapter per book: spot, linear contracts, inverse contracts and options on
//! bybit.com, and the spot book of Bybit EU, which is a separate platform with its own order
//! book and its own keys. Candle history, the instrument lists, best bid/ask, the spot price
//! board, live candles and the account read only. `BTCUSDT` is a spot pair and a perpetual
//! at once and they are different series, so which book a connector reads is its setting,
//! never a guess at the ticker. Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Candle, Capabilities, CapsError, Command, ConnectorError, ContractType, CorrId, DataType, Decimal, Envelope,
    Event, HistoryItems, InstrumentId, InstrumentSpec, LastPrice, OptionRight, Price, Qty, Quote, TfUnit, TimeRange, Timeframe,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use serde::Deserialize;
use serde_json::value::RawValue;

const REST: &str = "rest";
const WS: &str = "ws";
const EU: &str = "eu";
const WS_EU: &str = "ws-eu";
/// Every host a Bybit key can belong to. The regional sites of bybit.com serve the same
/// book as the global host, but each issues its own keys, valid on its own host only.
const BASES: &[(&str, &str)] = &[
    (REST, "https://api.bybit.com"),
    (WS, "wss://stream.bybit.com"),
    ("bytick", "https://api.bytick.com"),
    ("tr", "https://api.bybit.tr"),
    ("kz", "https://api.bybit.kz"),
    ("ge", "https://api.bybitgeorgia.ge"),
    ("ae", "https://api.bybit.ae"),
    ("id", "https://api.bybit.id"),
    ("jp", "https://api.manepa.jp"),
    ("hk", "https://api.spark-fintech.com"),
    ("testnet", "https://api-testnet.bybit.com"),
    (EU, "https://api.bybit.eu"),
    (WS_EU, "wss://stream.bybit.eu"),
    ("eu-testnet", "https://api-testnet.bybit.eu"),
];
/// `site` values an account on bybit.com may name, with the base each one reads.
const GLOBAL_SITES: &[(&str, &str)] = &[
    ("global", REST),
    ("bytick", "bytick"),
    ("tr", "tr"),
    ("kz", "kz"),
    ("ge", "ge"),
    ("ae", "ae"),
    ("id", "id"),
    ("jp", "jp"),
    ("hk", "hk"),
    ("testnet", "testnet"),
];
/// `site` values an account on Bybit EU may name.
const EU_SITES: &[(&str, &str)] = &[("eu", EU), ("eu-testnet", "eu-testnet")];
/// Candles per history page.
const PAGE: u32 = 1000;
/// Contracts per instrument page (the spot list is not paged).
const INSTRUMENT_PAGE: u32 = 1000;
/// The history rows carry no closed flag. Measured on 2026-10-18 by polling the archive
/// through minute boundaries: on bybit.com a closed minute was last rewritten up to 2.2 s
/// after its close (20 bars over spot, linear and inverse), on Bybit EU never after it. Ten
/// seconds keeps a margin of four over the worst one seen.
pub(crate) const SETTLE_MS: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Spot,
    Linear,
    Inverse,
    Option,
}

impl Category {
    /// The `category` every v5 call names.
    fn name(self) -> &'static str {
        match self {
            Category::Spot => "spot",
            Category::Linear => "linear",
            Category::Inverse => "inverse",
            Category::Option => "option",
        }
    }
}

#[derive(Clone)]
pub struct Bybit {
    caps: Capabilities,
    category: Category,
    /// The Bybit EU platform: its own book, hosts and keys.
    eu: bool,
}

impl Bybit {
    fn with(src: &str, category: Category, eu: bool) -> Result<Self, CapsError> {
        Ok(Bybit { caps: Capabilities::from_toml(src)?, category, eu })
    }

    pub fn spot() -> Result<Self, CapsError> {
        Self::with(include_str!("capabilities_spot.toml"), Category::Spot, false)
    }

    pub fn linear() -> Result<Self, CapsError> {
        Self::with(include_str!("capabilities_linear.toml"), Category::Linear, false)
    }

    pub fn inverse() -> Result<Self, CapsError> {
        Self::with(include_str!("capabilities_inverse.toml"), Category::Inverse, false)
    }

    pub fn option() -> Result<Self, CapsError> {
        Self::with(include_str!("capabilities_option.toml"), Category::Option, false)
    }

    /// The spot book of Bybit EU (bybit.eu).
    pub fn eu_spot() -> Result<Self, CapsError> {
        Self::with(include_str!("capabilities_eu.toml"), Category::Spot, true)
    }

    pub fn category(&self) -> Category {
        self.category
    }

    pub fn venue(&self) -> &'static str {
        match (self.eu, self.category) {
            (true, _) => "BYBIT_EU",
            (false, Category::Spot) => "BYBIT",
            (false, Category::Linear) => "BYBIT_LINEAR",
            (false, Category::Inverse) => "BYBIT_INVERSE",
            (false, Category::Option) => "BYBIT_OPTION",
        }
    }

    /// The public market-data host of this book.
    fn public_base(&self) -> &'static str {
        if self.eu {
            EU
        } else {
            REST
        }
    }

    /// The host an account's key belongs to, by its `site` setting. Exact or an error
    /// naming the fix: a key sent to the wrong site is refused as unknown.
    fn site_base(&self, site: Option<&str>) -> Result<&'static str, ConnectorError> {
        let sites = if self.eu { EU_SITES } else { GLOBAL_SITES };
        let raw = site.map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty());
        let Some(name) = raw else { return Ok(sites[0].1) };
        if let Some((_, base)) = sites.iter().find(|(n, _)| *n == name) {
            return Ok(base);
        }
        let names: Vec<&str> = sites.iter().map(|(n, _)| *n).collect();
        Err(ConnectorError::invalid(if self.eu {
            format!("the Bybit EU book reads a bybit.eu key: site is {} (or empty), not \"{name}\"", names.join(" or "))
        } else if EU_SITES.iter().any(|(n, _)| *n == name) {
            "a bybit.eu key belongs to Bybit EU, a separate platform from bybit.com: add it as a Bybit EU account".to_string()
        } else {
            format!("Bybit site \"{name}\" is not one of {} (empty = global)", names.join(", "))
        }))
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
    /// The next page of a contract list, asked as a follow-up of the page that named it.
    next_instruments: Option<(CorrId, String)>,
    /// The next step of an account read made of two calls (the key, then the wallet).
    next_account: Option<(CorrId, NativeRequest, account::Pending)>,
    live: stream::LiveState,
}

/// Bybit's interval token, the same on every category and on the socket.
fn interval(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1",
        (3, TfUnit::Minute) => "3",
        (5, TfUnit::Minute) => "5",
        (15, TfUnit::Minute) => "15",
        (30, TfUnit::Minute) => "30",
        (1, TfUnit::Hour) => "60",
        (2, TfUnit::Hour) => "120",
        (4, TfUnit::Hour) => "240",
        (6, TfUnit::Hour) => "360",
        (12, TfUnit::Hour) => "720",
        (1, TfUnit::Day) => "D",
        (1, TfUnit::Week) => "W",
        (1, TfUnit::Month) => "M",
        _ => return None,
    })
}

/// When a bar that opened at `open_ms` closes: a calendar month for `1M`, a fixed length
/// otherwise.
fn close_ms(open_ms: i64, tf: Timeframe) -> Option<i64> {
    if let Some(s) = tf.fixed_secs() {
        return open_ms.checked_add(s * 1000);
    }
    let t = time::OffsetDateTime::from_unix_timestamp(open_ms / 1000).ok()?;
    let (y, m) = if t.month() == time::Month::December { (t.year() + 1, time::Month::January) } else { (t.year(), t.month().next()) };
    let next = time::Date::from_calendar_date(y, m, 1).ok()?.midnight().assume_utc();
    Some(next.unix_timestamp() * 1000)
}

/// A candle's base and quote volume. An inverse contract counts its volume in contracts of
/// one USD and its turnover in the coin, the other way round from every other book.
fn volumes(category: Category, volume: Dec, turnover: Option<Dec>) -> (Qty, Option<otw_connector_model::Money>) {
    match (category, turnover) {
        (Category::Inverse, Some(t)) => (t.qty(), Some(volume.money())),
        _ => (volume.qty(), turnover.map(|q| q.money())),
    }
}

#[derive(Deserialize)]
struct Answer<T> {
    result: T,
}

#[derive(Deserialize)]
struct List<T> {
    #[serde(default = "Vec::new")]
    list: Vec<T>,
    #[serde(rename = "nextPageCursor", default)]
    next_page_cursor: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LotSize {
    #[serde(default)]
    min_order_qty: OptDec,
    /// Contracts: the quantity step. Spot steps by `basePrecision`.
    #[serde(default)]
    qty_step: OptDec,
    #[serde(default)]
    base_precision: OptDec,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PriceFilter {
    #[serde(default)]
    tick_size: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentRow {
    symbol: String,
    #[serde(default)]
    base_coin: String,
    #[serde(default)]
    quote_coin: String,
    #[serde(default)]
    settle_coin: String,
    #[serde(default)]
    status: String,
    /// Contracts: `LinearPerpetual`, `LinearFutures`, `InversePerpetual`, `InverseFutures`.
    #[serde(default)]
    contract_type: Option<String>,
    /// Options: `Call` or `Put`.
    #[serde(default)]
    options_type: Option<String>,
    #[serde(default)]
    delivery_time: OptDec,
    #[serde(default)]
    lot_size_filter: Option<LotSize>,
    #[serde(default)]
    price_filter: Option<PriceFilter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TickerRow {
    symbol: String,
    #[serde(default)]
    bid1_price: OptDec,
    #[serde(default)]
    bid1_size: OptDec,
    #[serde(default)]
    ask1_price: OptDec,
    #[serde(default)]
    ask1_size: OptDec,
    #[serde(default)]
    last_price: OptDec,
}

/// `[startTime, open, high, low, close, volume, turnover]`, all strings.
struct CandleRow {
    open_ms: i64,
    o: Dec,
    h: Dec,
    l: Dec,
    c: Dec,
    vol: Dec,
    turnover: Option<Dec>,
}

impl CandleRow {
    fn read(row: &[Box<RawValue>]) -> Option<CandleRow> {
        let at = |i: usize| row.get(i).map(|v| v.get());
        let dec = |i: usize| at(i).and_then(|t| serde_json::from_str::<Dec>(t).ok());
        Some(CandleRow {
            open_ms: at(0).and_then(|t| serde_json::from_str::<Int>(t).ok())?.0,
            o: dec(1)?,
            h: dec(2)?,
            l: dec(3)?,
            c: dec(4)?,
            vol: dec(5)?,
            turnover: dec(6),
        })
    }

    fn candle(&self, category: Category, instrument: InstrumentId, tf: Timeframe, is_final: bool) -> Result<Candle, ConnectorError> {
        let (volume, quote_volume) = volumes(category, self.vol, self.turnover);
        Ok(Candle {
            instrument,
            tf,
            open_ts: ts_millis(self.open_ms)?,
            open: self.o.price(),
            high: self.h.price(),
            low: self.l.price(),
            close: self.c.price(),
            volume,
            quote_volume,
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
    ConnectorError::decode(format!("bybit {what}: {e}"))
}

impl Bybit {
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

    fn instruments_page(&self, req_id: CorrId, cursor: Option<&str>, s: &mut Session) -> NativeRequest {
        s.pending.insert(req_id, Pending::Instruments);
        let mut h = HttpRequest::get("instruments", self.public_base(), "/v5/market/instruments-info", req_id).query("category", self.category.name());
        if self.category == Category::Option {
            // Without it, options answer BTC alone.
            h = h.query("baseCoin", "All");
        }
        if self.category != Category::Spot {
            h = h.query("limit", INSTRUMENT_PAGE);
        }
        if let Some(c) = cursor {
            h = h.query("cursor", c);
        }
        NativeRequest::Http(h)
    }

    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let token = interval(tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("{} serves no {tf} history here", self.caps.label)))?;
        // History reads backwards from `end`, newest bar first.
        let end = match cursor {
            Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
            None => range.to.millis(),
        };
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range });
        let h = HttpRequest::get("kline", self.public_base(), "/v5/market/kline", req_id)
            .query("category", self.category.name())
            .query("symbol", &instrument.native)
            .query("interval", token)
            .query("start", range.from.millis())
            .query("end", end)
            .query("limit", PAGE);
        Ok(vec![NativeRequest::Http(h)])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Answer<List<Vec<Box<RawValue>>>> = serde_json::from_slice(body).map_err(|e| decode_err("kline", e))?;
        let rows = page.result.list;
        let full = rows.len() >= PAGE as usize;
        // No closed flag on the archive: a bar is final once its period has ended.
        let settled = ctx.received_ts.millis() - SETTLE_MS;
        let mut candles = Vec::with_capacity(rows.len());
        let mut oldest: Option<i64> = None;
        for raw in &rows {
            let r = CandleRow::read(raw).ok_or_else(|| ConnectorError::decode("bybit kline: row shape"))?;
            oldest = Some(oldest.map_or(r.open_ms, |o| o.min(r.open_ms)));
            if !range.contains(ts_millis(r.open_ms)?) {
                continue;
            }
            let is_final = close_ms(r.open_ms, tf).is_some_and(|c| c <= settled);
            candles.push(r.candle(self.category, instrument.clone(), tf, is_final)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = match (full, oldest) {
            (true, Some(o)) if o > range.from.millis() => Some((o - 1).to_string()),
            _ => None,
        };
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    fn spec(&self, r: InstrumentRow) -> Result<Option<InstrumentSpec>, ConnectorError> {
        let id = r.symbol.to_ascii_uppercase();
        if id.is_empty() {
            return Ok(None);
        }
        let mut spec = InstrumentSpec::spot(InstrumentId::new(self.venue(), &id), &r.base_coin, &r.quote_coin);
        // `PreLaunch`, `Delivering` and `Closed` download nothing worth offering.
        spec.active = r.status == "Trading";
        spec.tick_size = r.price_filter.as_ref().and_then(|p| p.tick_size.0).map(Price);
        let lot = r.lot_size_filter.unwrap_or_default();
        spec.min_qty = lot.min_order_qty.0.map(Qty);
        spec.lot_size = lot.qty_step.0.or(lot.base_precision.0).map(Qty);
        let expiry = r.delivery_time.0.and_then(|d| i64::try_from(d.trunc()).ok()).filter(|d| *d > 0).map(ts_millis).transpose()?;
        let settle = Some(r.settle_coin.to_ascii_uppercase()).filter(|c| !c.is_empty());
        match self.category {
            Category::Spot => {}
            Category::Linear | Category::Inverse => {
                let kind = r.contract_type.as_deref().unwrap_or_default();
                let (perp, inverse) = match kind {
                    "LinearPerpetual" => (true, false),
                    "LinearFutures" => (false, false),
                    "InversePerpetual" => (true, true),
                    "InverseFutures" => (false, true),
                    other => {
                        tracing::warn!("bybit instruments-info: unknown contractType `{other}` on {id}, left out");
                        return Ok(None);
                    }
                };
                spec.asset_class = if perp { AssetClass::Perp } else { AssetClass::Future };
                spec.contract = if perp { ContractType::Perp } else { ContractType::Future };
                // An inverse contract is one USD, settled in the coin.
                spec.inverse = inverse;
                spec.settlement_ccy = settle;
                spec.note = Some(kind.to_string());
                if !perp {
                    spec.expiry = expiry;
                }
            }
            Category::Option => {
                let right = match r.options_type.as_deref().unwrap_or_default() {
                    "Call" => OptionRight::Call,
                    "Put" => OptionRight::Put,
                    other => {
                        tracing::warn!("bybit instruments-info: unknown optionsType `{other}` on {id}, left out");
                        return Ok(None);
                    }
                };
                spec.asset_class = AssetClass::Option;
                spec.contract = ContractType::Option;
                spec.right = Some(right);
                spec.expiry = expiry;
                spec.settlement_ccy = settle;
                // Delivering options still report their fills and positions.
                spec.active = matches!(r.status.as_str(), "Trading" | "Delivering");
            }
        }
        Ok(Some(spec))
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Answer<List<InstrumentRow>> = serde_json::from_slice(body).map_err(|e| decode_err("instruments-info", e))?;
        let mut out = Vec::with_capacity(page.result.list.len() + 1);
        for r in page.result.list {
            if let Some(spec) = self.spec(r)? {
                out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
            }
        }
        match page.result.next_page_cursor.filter(|c| !c.is_empty()) {
            // Contract and option lists are paged; the spot list comes whole.
            Some(c) if self.category != Category::Spot => s.next_instruments = Some((corr, c)),
            _ => out.push(env(ctx, corr, Event::InstrumentsEnd)),
        }
        Ok(out)
    }
}

impl Mapper for Bybit {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ConnectorError::not_found(format!(
                "Bybit: bad symbol `{symbol}`, write it as Bybit spells it (BTCUSDT, BTCPERP, BTCUSDT-26DEC26, BTCUSD, BTC-26DEC26-100000-C-USDT)"
            )));
        }
        Ok(InstrumentId::new(self.venue(), &s.to_ascii_uppercase()))
    }

    fn own_option_symbols(&self) -> bool {
        self.category == Category::Option
    }

    fn account_setting(&self) -> Option<&'static str> {
        Some("site")
    }

    fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        self.site_base(settings.get("site").map(String::as_str)).map(|_| ())
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } if self.caps.history.candles.is_some() => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::LoadInstruments { req_id } => Ok(vec![self.instruments_page(*req_id, None, s)]),
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert(*req_id, Pending::Quote { instrument: instrument.clone() });
                let h = HttpRequest::get("tickers", self.public_base(), "/v5/market/tickers", *req_id)
                    .query("category", self.category.name())
                    .query("symbol", &instrument.native);
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert(*req_id, Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("board", self.public_base(), "/v5/market/tickers", *req_id).query("category", self.category.name()))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } if !self.caps.live.candles.is_none() => {
                stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx)
            }
            Command::QueryAccount { kind: otw_connector_model::AccountKind::Fills, .. } if self.category == Category::Inverse => Err(ConnectorError::unsupported(
                "Bybit inverse fills are not imported: an inverse contract's profit is counted in the coin, quantity × (1/entry − 1/exit), \
                 and the journal counts quantity × price. Its positions and orders are read.",
            )),
            Command::QueryAccount { kind, account, .. } if self.account_declared(*kind) => {
                let base = self.site_base(account.as_deref())?;
                account::encode(self.category, base, cmd, &mut s.pending)
            }
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: the signature's receive window absorbs the host clock.
            other => Err(ConnectorError::unsupported(format!("{}: {other:?} is not declared", self.caps.label))),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.live.candles.is_none() && cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
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
            RawFrame::Ws { payload, .. } => return stream::decode(self.category, payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s.pending.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr}")))?;
        if let Some(e) = errors::from_frame(frame) {
            return Err(match &pending {
                Pending::Account(p) => account::hint(p, e),
                _ => e,
            });
        }
        match pending {
            Pending::Candles { instrument, tf, range } => self.decode_candles(corr, body, instrument, tf, range, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::Quote { instrument } => {
                let page: Answer<List<TickerRow>> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
                let Some(t) = page.result.list.into_iter().next() else { return Ok(vec![]) };
                if !t.symbol.eq_ignore_ascii_case(&instrument.native) {
                    return Err(ConnectorError::decode(format!("bybit tickers answered {} for {}", t.symbol, instrument.native)));
                }
                let (Some(bid), Some(ask)) = (t.bid1_price.0, t.ask1_price.0) else { return Ok(vec![]) };
                Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: t.bid1_size.0.map(Qty), ask_qty: t.ask1_size.0.map(Qty) }))])
            }
            Pending::Board => {
                let page: Answer<List<TickerRow>> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
                let venue = self.venue();
                let board = page
                    .result
                    .list
                    .into_iter()
                    .filter_map(|t| t.last_price.0.map(|p| LastPrice { instrument: InstrumentId::new(venue, &t.symbol.to_ascii_uppercase()), price: Price(p) }))
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Account(p) => {
                let (evs, next) = account::decode(self.category, self.venue(), p, corr, body, ctx)?;
                if let Some((req, p)) = next {
                    s.next_account = Some((corr, req, p));
                }
                Ok(evs)
            }
        }
    }

    fn follow_ups(&self, corr_id: CorrId, s: &mut Session) -> Vec<NativeRequest> {
        if let Some((corr, cursor)) = s.next_instruments.take() {
            if corr == corr_id {
                return vec![self.instruments_page(corr, Some(&cursor), s)];
            }
            s.next_instruments = Some((corr, cursor));
        }
        if let Some((corr, req, p)) = s.next_account.take() {
            if corr == corr_id {
                s.pending.insert(corr, Pending::Account(p));
                return vec![req];
            }
            s.next_account = Some((corr, req, p));
        }
        Vec::new()
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.remove(&corr_id);
        if s.next_instruments.as_ref().is_some_and(|(c, _)| *c == corr_id) {
            s.next_instruments = None;
        }
        if s.next_account.as_ref().is_some_and(|(c, ..)| *c == corr_id) {
            s.next_account = None;
        }
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(Some(Arc::new(signer::BybitSigner::from_secrets(secrets)?)))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        (self.category == Category::Spot).then(|| format!("{}{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }
}

/// `a / b` rounded to the coin's eight decimals, `None` when `b` is not a positive price.
fn per(a: Decimal, b: Option<Decimal>) -> Option<Decimal> {
    b.filter(|p| p.is_sign_positive() && !p.is_zero()).and_then(|p| a.checked_div(p)).map(|q| q.round_dp(8).normalize())
}

#[cfg(test)]
mod tests;
