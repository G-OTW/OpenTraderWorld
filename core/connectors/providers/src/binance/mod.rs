//! Binance, spot and USDⓈ-M futures: klines history, the instrument list, best bid/ask, the
//! price board, live klines, the account read, and orders ([`trade`]). Keyless market data.
//! Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod dto;
mod errors;
pub mod market;
mod stream;
pub mod trade;
pub mod user_stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Capabilities, CapsError, Candle, Command, ConnectorError, ContractType, CorrId, DataType, Envelope, ErrorKind,
    Event, HistoryItems, InstrumentId, InstrumentSpec, Price, Qty, Quote, TfUnit, TimeRange, Timeframe,
};
use otw_connector_sdk::parse::ts_millis;
use otw_connector_sdk::signer::HmacQuerySigner;
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;

use market::{Kind, Market};

/// Spot's venue id, kept as a constant for the callers that name it.
pub const VENUE: &str = market::SPOT.venue;
const REST: &str = "rest";
/// The wallet endpoints, on the spot host for either market.
const SAPI: &str = "sapi";
const WS: &str = "ws";
/// USDⓈ-M's high-frequency route (book ticker).
const WS_PUBLIC: &str = "ws_public";

#[derive(Clone)]
pub struct Binance {
    caps: Capabilities,
    market: &'static Market,
}

/// The spot market, under the name the first port used.
pub type BinanceSpot = Binance;

impl Binance {
    pub fn spot() -> Result<Self, CapsError> {
        Self::on(&market::SPOT)
    }

    pub fn usdm() -> Result<Self, CapsError> {
        Self::on(&market::USDM)
    }

    /// Spot (kept for the callers of the first port).
    pub fn new() -> Result<Self, CapsError> {
        Self::spot()
    }

    fn on(market: &'static Market) -> Result<Self, CapsError> {
        Ok(Binance { caps: Capabilities::from_toml(market.caps_toml)?, market })
    }

    pub fn market(&self) -> &'static Market {
        self.market
    }
}

enum Pending {
    Klines { instrument: InstrumentId, tf: Timeframe, range: TimeRange, start_ms: i64 },
    Instruments,
    Quote { instrument: InstrumentId },
    Time,
    Board,
    Account { kind: otw_connector_model::AccountKind },
    OpenOrders,
    Positions,
    Trades { symbols: Vec<InstrumentId>, idx: usize, range: TimeRange },
    UserTrades { symbols: Vec<InstrumentId>, cur: account::UserCursor, range: TimeRange },
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    live: stream::LiveState,
}

/// Binance's interval token for a timeframe; `None` = not one Binance serves.
fn interval(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Second) => "1s",
        (1, TfUnit::Minute) => "1m",
        (3, TfUnit::Minute) => "3m",
        (5, TfUnit::Minute) => "5m",
        (15, TfUnit::Minute) => "15m",
        (30, TfUnit::Minute) => "30m",
        (1, TfUnit::Hour) => "1h",
        (2, TfUnit::Hour) => "2h",
        (4, TfUnit::Hour) => "4h",
        (6, TfUnit::Hour) => "6h",
        (8, TfUnit::Hour) => "8h",
        (12, TfUnit::Hour) => "12h",
        (1, TfUnit::Day) => "1d",
        (3, TfUnit::Day) => "3d",
        (1, TfUnit::Week) => "1w",
        (1, TfUnit::Month) => "1M",
        _ => return None,
    })
}

/// Contract types that never expire.
fn perpetual(contract_type: &str) -> bool {
    matches!(contract_type, "PERPETUAL" | "TRADIFI_PERPETUAL")
}

impl Binance {
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

    fn klines(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let token = interval(tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("{} serves no {tf} history here", self.caps.label)))?;
        let start_ms = match cursor {
            Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
            None => range.from.millis(),
        };
        s.pending.insert(req_id, Pending::Klines { instrument: instrument.clone(), tf, range, start_ms });
        Ok(vec![NativeRequest::Http(
            HttpRequest::get("klines", REST, self.market.klines, req_id)
                .query("symbol", &instrument.native)
                .query("interval", token)
                .query("startTime", start_ms)
                // endTime is inclusive on Binance's side; the half-open range is enforced on decode.
                .query("endTime", range.to.millis())
                .query("limit", self.market.page)
                .weight(self.market.page_weight),
        )])
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_klines(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, start_ms: i64, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let name = self.market.name;
        let rows: dto::Klines = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("{name} klines: {e}")))?;
        let now_ms = ctx.received_ts.millis();
        let full_page = rows.len() >= self.market.page as usize;
        let mut candles = Vec::with_capacity(rows.len());
        let mut last_open = None;
        for raw in &rows {
            let r = dto::KlineRow::read(raw).ok_or_else(|| ConnectorError::decode(format!("{name} klines: row shape")))?;
            last_open = Some(r.open_ms);
            let open_ts = ts_millis(r.open_ms)?;
            // Below the page start = the previous page's last bar repeated; outside the range
            // = what the inclusive endTime let through.
            if r.open_ms < start_ms || !range.contains(open_ts) {
                continue;
            }
            candles.push(Candle {
                instrument: instrument.clone(),
                tf,
                open_ts,
                open: r.open.price(),
                high: r.high.price(),
                low: r.low.price(),
                close: r.close.price(),
                volume: r.volume.qty(),
                quote_volume: r.quote_volume.map(|q| q.money()),
                trades: r.trades.and_then(|n| u64::try_from(n).ok()),
                // closeTime is the bar's last millisecond: still forming until it has passed.
                is_final: r.close_ms < now_ms,
                adjusted: None,
                sides: None,
            });
        }
        let step_ms = tf.fixed_secs().map(|s| s * 1000);
        let next = match (full_page, last_open, step_ms) {
            (true, Some(o), Some(step)) if o + step < range.to.millis() => Some((o + step).to_string()),
            _ => None,
        };
        let done = next.is_none();
        Ok(vec![Envelope::new(
            ctx.connector.clone(),
            Some(corr),
            ctx.received_ts,
            Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done },
        )])
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let info: dto::ExchangeInfo =
            serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("{} exchangeInfo: {e}", self.market.name)))?;
        let mut out = Vec::with_capacity(info.symbols.len() + 1);
        for s in info.symbols {
            let mut spec = InstrumentSpec::spot(InstrumentId::new(self.market.venue, &s.symbol), &s.base_asset, &s.quote_asset);
            spec.active = s.status == "TRADING";
            if self.market.kind == Kind::UsdM {
                let ct = s.contract_type.clone().unwrap_or_default();
                let perp = perpetual(&ct);
                spec.asset_class = if perp { AssetClass::Perp } else { AssetClass::Future };
                spec.contract = if perp { ContractType::Perp } else { ContractType::Future };
                spec.settlement_ccy = s.margin_asset.clone();
                if !perp {
                    spec.expiry = s.delivery_date.map(|d| ts_millis(d.0)).transpose()?;
                }
                spec.note = s.contract_type.clone();
            }
            for f in &s.filters {
                match f.filter_type.as_str() {
                    "PRICE_FILTER" => spec.tick_size = f.tick_size.0.map(Price),
                    "LOT_SIZE" => {
                        spec.lot_size = f.step_size.0.map(Qty);
                        spec.min_qty = f.min_qty.0.map(Qty);
                    }
                    _ => {}
                }
            }
            // Notional, band, market lot, open-order caps: checked before an order leaves.
            spec.rules = trade::trading_rules(&self.market, &s.filters);
            out.push(Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, Event::InstrumentsEnd));
        Ok(out)
    }

    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let name = self.market.name;
        let t: dto::BookTicker = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("{name} bookTicker: {e}")))?;
        if t.symbol != instrument.native {
            return Err(ConnectorError::decode(format!("{name} bookTicker answered {} for {}", t.symbol, instrument.native)));
        }
        Ok(vec![Envelope::new(
            ctx.connector.clone(),
            Some(corr),
            ctx.received_ts,
            Event::Quote(Quote {
                instrument,
                bid: t.bid_price.price(),
                ask: t.ask_price.price(),
                bid_qty: t.bid_qty.0.map(Qty),
                ask_qty: t.ask_qty.0.map(Qty),
            }),
        )])
    }
}

impl Mapper for Binance {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        self.market.bases
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // Binance symbols are the concatenated pair in upper case (`BTCUSDT`); a dated futures
        // contract adds its delivery date (`ETHUSDT_250926`). A display form with a separator
        // is not guessed into one: the instrument list resolves those.
        let s = symbol.trim();
        let allowed = |c: char| c.is_ascii_alphanumeric() || (self.market.kind == Kind::UsdM && c == '_');
        if s.is_empty() || !s.chars().all(allowed) {
            let example = if self.market.kind == Kind::UsdM { "BTCUSDT, ETHUSDT_250926" } else { "BTCUSDT" };
            return Err(ConnectorError::not_found(format!(
                "{}: bad symbol `{symbol}`, write it as Binance spells it ({example})",
                self.caps.label
            )));
        }
        Ok(InstrumentId::new(self.market.venue, &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.klines(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert(*req_id, Pending::Instruments);
                let mut h = HttpRequest::get("exchangeInfo", REST, self.market.exchange_info, *req_id).weight(20);
                if self.market.kind == Kind::Spot {
                    h = h.query("permissions", "SPOT");
                }
                Ok(vec![NativeRequest::Http(h)])
            }
            // Spot answers a few symbols' terms on their own, not the whole list.
            Command::ResolveInstruments { req_id, ids } if self.market.kind == Kind::Spot && !ids.is_empty() => {
                s.pending.insert(*req_id, Pending::Instruments);
                let h = HttpRequest::get("exchangeInfo", REST, self.market.exchange_info, *req_id).weight(20);
                let h = match ids.as_slice() {
                    [one] => h.query("symbol", &one.native),
                    many => h.query("symbols", &format!("[{}]", many.iter().map(|i| format!("\"{}\"", i.native)).collect::<Vec<_>>().join(","))),
                };
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert(*req_id, Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(
                    HttpRequest::get("bookTicker", REST, self.market.book_ticker, *req_id).query("symbol", &instrument.native).weight(2),
                )])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(self.market, cmd, s),
            Command::SyncTime { .. } => account::encode(self.market, cmd, s),
            Command::RequestBoard { .. } if self.caps.history.board => account::encode(self.market, cmd, s),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("{}: {} is not declared", self.caps.label, command_name(other)))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, body) = match frame {
            RawFrame::Http { corr_id, body, .. } => (*corr_id, body),
            RawFrame::Timeout { corr_id, .. } => {
                s.pending.remove(corr_id);
                return Err(errors::from_frame(frame, self.market.name).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(self.market.name, payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s.pending.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr}")))?;
        let err = errors::from_frame(frame, self.market.name);
        match pending {
            Pending::Klines { instrument, tf, range, start_ms } => match err {
                Some(e) => Err(e),
                None => self.decode_klines(corr, body, instrument, tf, range, start_ms, ctx),
            },
            Pending::Instruments => match err {
                Some(e) => Err(e),
                None => self.decode_instruments(corr, body, ctx),
            },
            Pending::Quote { instrument } => match err {
                Some(e) => Err(e),
                None => self.decode_quote(corr, body, instrument, ctx),
            },
            other => account::decode(self.market, other, corr, body, err, ctx),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        // Subscriptions joining together share one frame: Binance caps incoming frames per
        // second per connection.
        if cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
            return stream::encode(self, cmds, &mut s.live);
        }
        let mut out = Vec::new();
        for c in cmds {
            out.extend(self.encode(c, s, ctx)?);
        }
        Ok(out)
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.remove(&corr_id);
    }

    /// USDⓈ-M serves the book ticker on another route than the klines, so a quote is a
    /// socket of its own there. Spot carries both on one.
    fn socket_class(&self, _instrument: &InstrumentId, data_type: &DataType) -> String {
        match (self.market.kind, data_type) {
            (Kind::UsdM, DataType::Quote) => "public".into(),
            _ => String::new(),
        }
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
        Ok(Some(Arc::new(HmacQuerySigner {
            api_key: get("api_key")?,
            secret: get("api_secret")?,
            key_header: "X-MBX-APIKEY",
            timestamp_param: "timestamp",
            signature_param: "signature",
            recv_window: Some(("recvWindow", 20_000)),
        })))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        Some(format!("{}{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        match self.market.kind {
            Kind::Spot => "Binance".into(),
            // Perpetual or a dated contract: what it is belongs on the line.
            Kind::UsdM => format!("Binance {}", spec.note.as_deref().unwrap_or("futures")),
        }
    }
}

fn command_name(c: &Command) -> &'static str {
    match c {
        Command::Connect => "connect",
        Command::Disconnect => "disconnect",
        Command::SearchInstruments { .. } => "native search",
        Command::LoadInstruments { .. } => "instrument list",
        Command::Subscribe { .. } => "live subscription",
        Command::Unsubscribe { .. } => "unsubscribe",
        Command::RequestHistory { .. } => "this history",
        Command::RequestSnapshot { .. } => "this snapshot",
        Command::QueryAccount { .. } => "account query",
        Command::SyncTime { .. } => "clock",
        Command::RequestBoard { .. } => "price board",
        Command::RequestQuoteAt { .. } => "historical quote",
        Command::ResolveInstruments { .. } => "instrument lookup",
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_usdm;
