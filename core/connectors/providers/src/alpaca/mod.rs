//! Alpaca: bars, quotes (latest and past), the asset list, live one-minute bars, and the
//! trading account read only. Stocks, crypto and options sit on different endpoints; the
//! market rides the instrument id's venue (`ALPACA`, `ALPACA-CRYPTO`, `ALPACA-OPTIONS`).
//! Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod signer;
mod stream;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind,
    Event, HistoryItems, InstrumentId, InstrumentSpec, Price, Qty, Quote, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::value::RawValue;

pub const VENUE: &str = "ALPACA";
pub const VENUE_CRYPTO: &str = "ALPACA-CRYPTO";
pub const VENUE_OPTIONS: &str = "ALPACA-OPTIONS";
const DATA: &str = "data";
const TRADING: &str = "trading";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[
    (DATA, "https://data.alpaca.markets"),
    (TRADING, "https://api.alpaca.markets"),
    (WS, "wss://stream.data.alpaca.markets"),
];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Bars per call, the most Alpaca answers.
const PAGE: u32 = 10_000;

/// Which market-data entitlement a key is on: it picks the live socket, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    /// Free plan: IEX equities, the indicative options feed.
    Iex,
    /// Paid plan: the full SIP tape, OPRA options.
    Sip,
}

impl Feed {
    fn label(self) -> &'static str {
        match self {
            Feed::Iex => "IEX",
            Feed::Sip => "SIP",
        }
    }
}

#[derive(Clone)]
pub struct Alpaca {
    caps: Capabilities,
    feed: Feed,
}

impl Alpaca {
    pub fn new(feed: Feed) -> Result<Self, CapsError> {
        Ok(Alpaca { caps: Capabilities::from_toml(CAPS_TOML)?, feed })
    }
}

/// Alpaca's three markets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Market {
    Stocks,
    Crypto,
    Options,
}

pub(crate) fn market_of(id: &InstrumentId) -> Market {
    match id.venue.0.as_str() {
        VENUE_CRYPTO => Market::Crypto,
        VENUE_OPTIONS => Market::Options,
        _ => Market::Stocks,
    }
}

/// The OCC symbol Alpaca names an option by: root (1 to 6), YYMMDD, C or P, strike x 1000
/// on 8 digits. Its own notation, read as such.
pub(crate) fn is_occ(s: &str) -> bool {
    if s.len() < 16 || s.len() > 21 {
        return false;
    }
    let (root, tail) = s.split_at(s.len() - 15);
    let (date, rest) = tail.split_at(6);
    !root.is_empty()
        && root.chars().all(|c| c.is_ascii_alphanumeric())
        && date.chars().all(|c| c.is_ascii_digit())
        && matches!(rest.as_bytes().first(), Some(b'C' | b'P'))
        && rest[1..].chars().all(|c| c.is_ascii_digit())
}

/// Alpaca's bar timeframe.
fn timeframe(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1Min",
        (5, TfUnit::Minute) => "5Min",
        (15, TfUnit::Minute) => "15Min",
        (1, TfUnit::Hour) => "1Hour",
        (1, TfUnit::Day) => "1Day",
        _ => return None,
    })
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Candles(CandleReq),
    Quote { instrument: InstrumentId },
    Assets { class: &'static str },
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    /// The account's currency, once read (what a stock or an option settles in).
    currency: Option<String>,
    live: stream::LiveState,
}

#[derive(Deserialize)]
pub(crate) struct BarRow {
    t: String,
    #[serde(default)]
    o: OptDec,
    #[serde(default)]
    h: OptDec,
    #[serde(default)]
    l: OptDec,
    #[serde(default)]
    c: OptDec,
    #[serde(default)]
    v: OptDec,
    #[serde(default)]
    n: Option<u64>,
}

impl BarRow {
    pub(crate) fn candle(&self, instrument: InstrumentId, tf: Timeframe, is_final: bool) -> Result<Candle, ConnectorError> {
        let p = |d: OptDec| Price(d.0.unwrap_or_default());
        Ok(Candle {
            instrument,
            tf,
            open_ts: ts_rfc3339(&self.t)?,
            open: p(self.o),
            high: p(self.h),
            low: p(self.l),
            close: p(self.c),
            volume: Qty(self.v.0.unwrap_or_default()),
            quote_volume: None,
            trades: self.n,
            is_final,
            adjusted: None,
            sides: None,
        })
    }
}

#[derive(Deserialize)]
struct QuoteRow {
    #[serde(default)]
    bp: OptDec,
    #[serde(default)]
    ap: OptDec,
    #[serde(default)]
    bs: OptDec,
    #[serde(rename = "as", default)]
    asz: OptDec,
    #[serde(default)]
    t: Option<String>,
}

#[derive(Deserialize)]
struct AssetRow {
    symbol: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    exchange: String,
    #[serde(default)]
    class: String,
    #[serde(default)]
    tradable: Option<bool>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

pub(crate) fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("alpaca {what}: {e}"))
}

/// Alpaca refuses by status with `{"message": ...}`: 403 is a plan or key the endpoint does
/// not accept, 404 an unknown symbol, 422 a malformed one.
pub(crate) fn status_error(frame: &RawFrame) -> Option<ConnectorError> {
    let mut e = http::timeout(frame, "alpaca").or_else(|| http::classify(frame, "alpaca"))?;
    if matches!(e.provider_code.as_deref(), Some("404" | "422")) {
        e.kind = ErrorKind::NotFound;
        e.message = format!("{}: bad symbol", e.message);
    }
    Some(e)
}

/// `RFC 3339` to the second, the fraction only when there is one.
fn rfc3339(t: Timestamp) -> Result<String, ConnectorError> {
    let d = t.to_offset_datetime().ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))?;
    d.format(&time::format_description::well_known::Rfc3339).map_err(|e| ConnectorError::invalid(format!("time {t}: {e}")))
}

/// Percent-encoding for a path segment (a crypto pair carries a slash).
pub(crate) fn segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Alpaca {
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
        let token = timeframe(tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("Alpaca serves no {tf} bars here")))?;
        let sym = &instrument.native;
        let h = match market_of(instrument) {
            Market::Stocks => HttpRequest::get("bars", DATA, format!("/v2/stocks/{}/bars", segment(sym)), req_id),
            Market::Crypto => HttpRequest::get("bars", DATA, "/v1beta3/crypto/us/bars", req_id).query("symbols", sym),
            Market::Options => HttpRequest::get("bars", DATA, "/v1beta1/options/bars", req_id).query("symbols", sym),
        };
        let mut h = h.query("timeframe", token).query("start", rfc3339(range.from)?).query("end", rfc3339(range.to)?).query("limit", PAGE).signed();
        if let Some(c) = cursor {
            h = h.query("page_token", c);
        }
        s.pending.insert((req_id, "bars"), Pending::Candles(CandleReq { instrument: instrument.clone(), tf, range }));
        Ok(vec![NativeRequest::Http(h)])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], req: CandleReq, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            bars: Option<Box<RawValue>>,
            #[serde(default)]
            next_page_token: Option<String>,
        }
        let page: Page = serde_json::from_slice(body).map_err(|e| decode_err("bars", e))?;
        let rows: Vec<BarRow> = match (page.bars, market_of(&req.instrument)) {
            (None, _) => return Err(ConnectorError::not_found(format!("alpaca bars: no bars for {}, bad symbol?", req.instrument.native))),
            (Some(raw), Market::Stocks) => serde_json::from_str::<Option<Vec<BarRow>>>(raw.get()).map_err(|e| decode_err("bars", e))?.unwrap_or_default(),
            // `{bars: {SYM: [..]}}`, keyed by the symbol as asked (a crypto pair has a slash).
            (Some(raw), _) => {
                let mut by: BTreeMap<String, Vec<BarRow>> = serde_json::from_str(raw.get()).map_err(|e| decode_err("bars", e))?;
                by.remove(&req.instrument.native).unwrap_or_default()
            }
        };
        let mut candles = Vec::with_capacity(rows.len());
        for r in &rows {
            // No forming flag: the engine finalizes at the period's end.
            let c = r.candle(req.instrument.clone(), req.tf, false)?;
            if req.range.contains(c.open_ts) {
                candles.push(c);
            }
        }
        candles.sort_by_key(|c| c.open_ts);
        let next = page.next_page_token.filter(|t| !t.is_empty());
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    fn quote(&self, req_id: CorrId, instrument: &InstrumentId, at: Option<(Timestamp, bool)>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let sym = &instrument.native;
        let window = |h: HttpRequest, (t, before): (Timestamp, bool)| -> Result<HttpRequest, ConnectorError> {
            Ok(if before { h.query("end", rfc3339(t)?).query("sort", "desc") } else { h.query("start", rfc3339(t)?).query("sort", "asc") }.query("limit", 1))
        };
        let h = match (market_of(instrument), at) {
            (Market::Crypto, None) => HttpRequest::get("quote", DATA, "/v1beta3/crypto/us/latest/quotes", req_id).query("symbols", sym),
            (Market::Crypto, Some(w)) => window(HttpRequest::get("quote", DATA, "/v1beta3/crypto/us/quotes", req_id).query("symbols", sym), w)?,
            (Market::Options, None) => HttpRequest::get("quote", DATA, "/v1beta1/options/quotes/latest", req_id).query("symbols", sym),
            (Market::Options, Some(_)) => return Err(ConnectorError::unsupported("Alpaca keeps no historical option quotes")),
            (Market::Stocks, None) => HttpRequest::get("quote", DATA, format!("/v2/stocks/{}/quotes/latest", segment(sym)), req_id),
            (Market::Stocks, Some(w)) => window(HttpRequest::get("quote", DATA, format!("/v2/stocks/{}/quotes", segment(sym)), req_id), w)?,
        };
        s.pending.insert((req_id, "quote"), Pending::Quote { instrument: instrument.clone() });
        Ok(vec![NativeRequest::Http(h.signed())])
    }

    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        // Latest: `{quote:{..}}` or `{quotes:{SYM:{..}}}`. Past: `{quotes:[..]}` or
        // `{quotes:{SYM:[..]}}`.
        let top: BTreeMap<String, Box<RawValue>> = serde_json::from_slice(body).map_err(|e| decode_err("quote", e))?;
        let one = |raw: &RawValue| -> Option<QuoteRow> {
            let t = raw.get().trim_start();
            if t.starts_with('[') {
                serde_json::from_str::<Vec<QuoteRow>>(raw.get()).ok()?.into_iter().next()
            } else {
                serde_json::from_str::<QuoteRow>(raw.get()).ok()
            }
        };
        let row = match (top.get("quote"), top.get("quotes")) {
            (Some(q), _) => one(q),
            (None, Some(qs)) => match serde_json::from_str::<BTreeMap<String, Box<RawValue>>>(qs.get()) {
                Ok(by) => by.get(&instrument.native).and_then(|r| one(r)),
                Err(_) => one(qs),
            },
            _ => None,
        };
        let Some(q) = row else { return Ok(vec![]) };
        let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
        let (Some(bid), Some(ask)) = (positive(q.bp), positive(q.ap)) else { return Ok(vec![]) };
        let e = env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: q.bs.0.map(Qty), ask_qty: q.asz.0.map(Qty) }));
        Ok(vec![match q.t.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
            Some(t) => e.at(t),
            None => e,
        }])
    }
}

/// Alpaca's asset class to a market. A class this adapter does not know is no instrument.
pub(crate) fn class_market(class: &str) -> Option<AssetClass> {
    match class {
        "us_equity" | "global_equity" => Some(AssetClass::Equity),
        "us_option" => Some(AssetClass::Option),
        "crypto" | "crypto_perp" => Some(AssetClass::Spot),
        _ => None,
    }
}

impl Mapper for Alpaca {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        self.instrument_for(symbol, None)
    }

    /// The market rides the id's venue. With no market named, Alpaca's own notation says it:
    /// a pair has a slash, an option is an OCC symbol, anything else a stock.
    fn instrument_for(&self, symbol: &str, market: Option<AssetClass>) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_ascii_uppercase();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-')) {
            return Err(ConnectorError::not_found(format!("Alpaca: bad symbol `{symbol}`, write it as Alpaca spells it (AAPL, BTC/USD, or an OCC option symbol)")));
        }
        let venue = match market {
            Some(AssetClass::Spot | AssetClass::Perp) => VENUE_CRYPTO,
            Some(AssetClass::Option) => VENUE_OPTIONS,
            Some(AssetClass::Equity | AssetClass::Etf) => VENUE,
            Some(other) => return Err(ConnectorError::unsupported(format!("Alpaca has no {other:?} market"))),
            None if s.contains('/') => VENUE_CRYPTO,
            None if is_occ(&s) => VENUE_OPTIONS,
            None => VENUE,
        };
        if venue == VENUE_OPTIONS && !is_occ(&s) {
            return Err(ConnectorError::not_found(format!("Alpaca: `{symbol}` is not an OCC option symbol")));
        }
        Ok(InstrumentId::new(venue, &s))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => self.quote(*req_id, instrument, None, s),
            Command::RequestQuoteAt { req_id, instrument, at, before } => {
                let class = match market_of(instrument) {
                    Market::Stocks => AssetClass::Equity,
                    Market::Crypto => AssetClass::Spot,
                    Market::Options => AssetClass::Option,
                };
                if !self.caps.history.quotes_at.contains(&class) {
                    return Err(ConnectorError::unsupported("Alpaca keeps no historical option quotes"));
                }
                self.quote(*req_id, instrument, Some((*at, *before)), s)
            }
            Command::LoadInstruments { req_id } => Ok([("assetsEquity", "us_equity"), ("assetsCrypto", "crypto")]
                .into_iter()
                .map(|(ep, class)| {
                    s.pending.insert((*req_id, ep), Pending::Assets { class });
                    NativeRequest::Http(HttpRequest::get(ep, TRADING, "/v2/assets", *req_id).query("status", "active").query("asset_class", class).signed())
                })
                .collect()),
            Command::ResolveInstruments { .. } => account::resolve(cmd, s),
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, s, ctx),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("Alpaca: {other:?} is not declared"))),
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
                return Err(status_error(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(self, payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Pending::Account(p) = pending {
            return account::decode(p, corr, frame, body, s, ctx);
        }
        if let Some(e) = status_error(frame) {
            return Err(e);
        }
        match pending {
            Pending::Candles(req) => self.decode_candles(corr, body, req, ctx),
            Pending::Quote { instrument } => self.decode_quote(corr, body, instrument, ctx),
            Pending::Assets { class } => {
                let rows: Vec<AssetRow> = serde_json::from_slice(body).map_err(|e| decode_err("assets", e))?;
                let mut out = Vec::with_capacity(rows.len() + 1);
                for a in rows {
                    let Some(m) = class_market(if a.class.is_empty() { class } else { &a.class }) else { continue };
                    let id = self.instrument_for(&a.symbol, Some(m))?;
                    let quote = if m == AssetClass::Spot { a.symbol.split_once('/').map(|(_, q)| q.to_string()).unwrap_or_default() } else { String::new() };
                    let mut spec = InstrumentSpec::spot(id, &a.symbol, &quote);
                    spec.symbol = CanonicalSymbol(a.name);
                    spec.asset_class = m;
                    spec.exchange = Some(a.exchange).filter(|e| !e.is_empty());
                    spec.active = a.tradable != Some(false);
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
                if class == "crypto" {
                    out.push(env(ctx, corr, Event::InstrumentsEnd));
                }
                Ok(out)
            }
            Pending::Account(_) => Ok(vec![]),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn keepalive(&self, s: &mut Session, _ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::drain(&mut s.live)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| SecretString::from(v.to_string()))
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this Alpaca connector")))
        };
        Ok(Some(Arc::new(signer::AlpacaSigner { key: get("api_key")?, secret: get("api_secret")? })))
    }

    fn resolve_batch(&self) -> usize {
        account::RESOLVE_BATCH
    }

    /// The options socket speaks MessagePack only (a JSON connection is refused, 412).
    fn ws_msgpack(&self, path: &str) -> bool {
        path.starts_with("/v1beta1/")
    }

    /// The options socket has no `bars` channel (410): an option's candles fold from prints.
    fn live_trades_only(&self, instrument: &InstrumentId) -> bool {
        market_of(instrument) == Market::Options
    }

    fn socket_class(&self, instrument: &InstrumentId, _data_type: &DataType) -> String {
        match market_of(instrument) {
            Market::Stocks => "stocks",
            Market::Crypto => "crypto",
            Market::Options => "options",
        }
        .into()
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.exchange.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
