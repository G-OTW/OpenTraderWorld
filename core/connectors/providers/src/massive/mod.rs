//! Massive (formerly Polygon.io): aggregates for every market (futures on their own
//! service), quotes at a moment, ticker search, and live minute aggregates per market
//! cluster. Keyed (`apiKey` query parameter on REST, an `auth` message on the socket, both
//! written by the signer). Endpoint-by-endpoint mapping in PROVIDER.md.

mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind,
    Event, HistoryItems, InstrumentId, InstrumentSpec, Price, Qty, Quote, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_millis, ts_nanos, Int, OptDec};
use otw_connector_sdk::signer::SignableRequest;
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

pub const VENUE: &str = "MASSIVE";
const REST: &str = "rest";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[(REST, "https://api.massive.com"), (WS, "wss://socket.massive.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
const LIMIT: u32 = 50_000;
/// Results one search asks for (the most the endpoint answers).
const SEARCH_LIMIT: u32 = 50;

#[derive(Clone)]
pub struct Massive {
    caps: Capabilities,
}

impl Massive {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Massive { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// Massive's markets, each its own cluster on the socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Market {
    Stocks,
    Options,
    Crypto,
    Forex,
    Indices,
    Futures,
}

impl Market {
    fn venue(self) -> &'static str {
        match self {
            Market::Stocks => "MASSIVE",
            Market::Options => "MASSIVE-OPTIONS",
            Market::Crypto => "MASSIVE-CRYPTO",
            Market::Forex => "MASSIVE-FX",
            Market::Indices => "MASSIVE-INDICES",
            Market::Futures => "MASSIVE-FUTURES",
        }
    }

    pub(crate) fn cluster(self) -> &'static str {
        match self {
            Market::Stocks => "stocks",
            Market::Options => "options",
            Market::Crypto => "crypto",
            Market::Forex => "forex",
            Market::Indices => "indices",
            Market::Futures => "futures",
        }
    }

    /// The reference-search `market` parameter.
    fn search(self) -> &'static str {
        match self {
            Market::Stocks => "stocks",
            Market::Options => "options",
            Market::Crypto => "crypto",
            Market::Forex => "fx",
            Market::Indices => "indices",
            Market::Futures => "futures",
        }
    }

    fn class(self) -> AssetClass {
        match self {
            Market::Stocks => AssetClass::Equity,
            Market::Options => AssetClass::Option,
            Market::Crypto => AssetClass::Spot,
            Market::Forex => AssetClass::Forex,
            Market::Indices => AssetClass::Index,
            Market::Futures => AssetClass::Future,
        }
    }

    fn of_class(a: Option<AssetClass>) -> Result<Market, ConnectorError> {
        Ok(match a {
            None | Some(AssetClass::Equity | AssetClass::Etf) => Market::Stocks,
            Some(AssetClass::Option) => Market::Options,
            Some(AssetClass::Spot | AssetClass::Perp) => Market::Crypto,
            Some(AssetClass::Forex) => Market::Forex,
            Some(AssetClass::Index) => Market::Indices,
            Some(AssetClass::Future) => Market::Futures,
            Some(other) => return Err(ConnectorError::unsupported(format!("Massive has no {other:?} market"))),
        })
    }
}

pub(crate) fn market_of(id: &InstrumentId) -> Market {
    match id.venue.0.as_str() {
        "MASSIVE-OPTIONS" => Market::Options,
        "MASSIVE-CRYPTO" => Market::Crypto,
        "MASSIVE-FX" => Market::Forex,
        "MASSIVE-INDICES" => Market::Indices,
        "MASSIVE-FUTURES" => Market::Futures,
        _ => Market::Stocks,
    }
}

/// `(multiplier, timespan)` of the v2 aggregates.
fn span(tf: Timeframe) -> Option<(u32, &'static str)> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => (1, "minute"),
        (5, TfUnit::Minute) => (5, "minute"),
        (15, TfUnit::Minute) => (15, "minute"),
        (1, TfUnit::Hour) => (1, "hour"),
        (1, TfUnit::Day) => (1, "day"),
        (1, TfUnit::Week) => (1, "week"),
        _ => return None,
    })
}

/// The futures service's resolution; a futures day is its trading session.
fn resolution(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1min",
        (5, TfUnit::Minute) => "5min",
        (15, TfUnit::Minute) => "15min",
        (1, TfUnit::Hour) => "1hour",
        (1, TfUnit::Day) => "1session",
        (1, TfUnit::Week) => "1week",
        _ => return None,
    })
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Aggs(CandleReq),
    FuturesAggs(CandleReq),
    Quote { instrument: InstrumentId },
    Search { market: Market },
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    live: stream::LiveState,
}

#[derive(Deserialize)]
struct Envelope0 {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct AggRow {
    t: Int,
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

#[derive(Deserialize)]
struct FuturesRow {
    window_start: Int,
    #[serde(default)]
    open: OptDec,
    #[serde(default)]
    high: OptDec,
    #[serde(default)]
    low: OptDec,
    #[serde(default)]
    close: OptDec,
    #[serde(default)]
    volume: OptDec,
}

#[derive(Deserialize)]
struct QuoteRow {
    #[serde(default)]
    bid_price: OptDec,
    #[serde(default)]
    ask_price: OptDec,
    #[serde(default)]
    bid_size: OptDec,
    #[serde(default)]
    ask_size: OptDec,
    #[serde(default)]
    sip_timestamp: Option<Int>,
}

#[derive(Deserialize)]
struct TickerRow {
    #[serde(default)]
    ticker: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    primary_exchange: Option<String>,
}

#[derive(Deserialize)]
struct Results<T> {
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("massive {what}: {e}"))
}

/// A served answer carries status `OK` or `DELAYED` (a free key is delayed); anything else
/// names the reason in `message` or `error`. The status code decides first.
fn refusal(frame: &RawFrame, body: &[u8]) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "massive").or_else(|| http::classify(frame, "massive")) {
        return Some(e);
    }
    let env = serde_json::from_slice::<Envelope0>(body).ok()?;
    let status = env.status.clone().unwrap_or_default();
    if status.eq_ignore_ascii_case("OK") || status.eq_ignore_ascii_case("DELAYED") {
        return None;
    }
    let msg = env.message.or(env.error).unwrap_or_else(|| "request failed".into());
    let lower = msg.to_ascii_lowercase();
    let kind = if lower.contains("exceeded") || status.eq_ignore_ascii_case("ERROR") && lower.contains("limit") {
        ErrorKind::RateLimited
    } else if status.eq_ignore_ascii_case("NOT_AUTHORIZED") {
        ErrorKind::Permission
    } else if status.eq_ignore_ascii_case("NOT_FOUND") {
        ErrorKind::NotFound
    } else {
        ErrorKind::InvalidRequest
    };
    let message = if kind == ErrorKind::NotFound { format!("massive ({status}): bad symbol ({msg})") } else { format!("massive ({status}): {msg}") };
    Some(ConnectorError::new(kind, message).with_code(status))
}

pub struct MassiveSigner {
    key: SecretString,
}

impl Signer for MassiveSigner {
    fn sign(&self, req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        req.query.push(("apiKey".into(), self.key.expose_secret().to_string()));
        Ok(())
    }

    fn ws_login(&self, _now: Timestamp) -> Result<Option<bytes::Bytes>, ConnectorError> {
        Ok(Some(serde_json::json!({ "action": "auth", "params": self.key.expose_secret() }).to_string().into()))
    }
}

impl Massive {
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("Massive serves no {tf} aggregates here")));
        }
        let sym = &instrument.native;
        let req = CandleReq { instrument: instrument.clone(), tf, range };
        if market_of(instrument) == Market::Futures {
            // Their own service: nanosecond bounds, `[from, to)`.
            let res = resolution(tf).ok_or_else(|| ConnectorError::unsupported(format!("Massive futures serve no {tf} aggregates")))?;
            s.pending.insert((req_id, "futuresAggs"), Pending::FuturesAggs(req));
            return Ok(vec![NativeRequest::Http(
                HttpRequest::get("futuresAggs", REST, format!("/futures/v1/aggs/{sym}"), req_id)
                    .query("resolution", res)
                    .query("window_start.gte", range.from.nanos())
                    .query("window_start.lt", range.to.nanos())
                    .query("sort", "window_start.asc")
                    .query("limit", LIMIT)
                    .signed(),
            )]);
        }
        let (mult, span) = span(tf).ok_or_else(|| ConnectorError::unsupported(format!("Massive serves no {tf} aggregates")))?;
        // Millisecond bounds; `to` is inclusive on Massive's side, so one millisecond less.
        let from_ms = range.from.millis();
        let to_ms = range.to.millis().saturating_sub(1).max(from_ms);
        let mut h = HttpRequest::get("aggs", REST, format!("/v2/aggs/ticker/{sym}/range/{mult}/{span}/{from_ms}/{to_ms}"), req_id);
        // Unadjusted OHLC everywhere; the index endpoint takes no `adjusted`.
        if market_of(instrument) != Market::Indices {
            h = h.query("adjusted", "false");
        }
        s.pending.insert((req_id, "aggs"), Pending::Aggs(req));
        Ok(vec![NativeRequest::Http(h.query("sort", "asc").query("limit", LIMIT).signed())])
    }

    fn quote(&self, req_id: CorrId, instrument: &InstrumentId, at: Timestamp, before: bool, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let (bound, order) = if before { ("timestamp.lt", "desc") } else { ("timestamp.gte", "asc") };
        s.pending.insert((req_id, "quote"), Pending::Quote { instrument: instrument.clone() });
        Ok(vec![NativeRequest::Http(
            HttpRequest::get("quote", REST, format!("/v3/quotes/{}", instrument.native), req_id)
                .query(bound, at.nanos())
                .query("order", order)
                .query("sort", "timestamp")
                .query("limit", 1)
                .signed(),
        )])
    }
}

/// A bar of a window: kept when it opens inside it.
fn keep(req: &CandleReq, open_ts: Timestamp, row: (OptDec, OptDec, OptDec, OptDec, OptDec), trades: Option<u64>) -> Option<Candle> {
    if !req.range.contains(open_ts) {
        return None;
    }
    let p = |d: OptDec| Price(d.0.unwrap_or_default());
    Some(Candle {
        instrument: req.instrument.clone(),
        tf: req.tf,
        open_ts,
        open: p(row.0),
        high: p(row.1),
        low: p(row.2),
        close: p(row.3),
        volume: Qty(row.4 .0.unwrap_or_default()),
        quote_volume: None,
        trades,
        // No forming flag: the engine finalizes at the period's end.
        is_final: false,
        adjusted: None,
        sides: None,
    })
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, candles: Vec<Candle>) -> Vec<Envelope> {
    vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })]
}

impl Mapper for Massive {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // With no market named, Massive's own prefix says it (`O:`, `X:`, `C:`, `I:`).
        let s = symbol.trim().to_ascii_uppercase();
        let market = match s.get(..2) {
            Some("O:") => Market::Options,
            Some("X:") => Market::Crypto,
            Some("C:") => Market::Forex,
            Some("I:") => Market::Indices,
            _ => Market::Stocks,
        };
        self.instrument_for(&s, Some(market.class()))
    }

    /// The REST ticker: the market's prefix added to the symbol the form submits (`AAPL`,
    /// `O:` + OCC, `X:BTCUSD`, `C:EURUSD`, `I:SPX`, a futures contract bare), the market riding
    /// the id's venue.
    fn instrument_for(&self, symbol: &str, market: Option<AssetClass>) -> Result<InstrumentId, ConnectorError> {
        let m = Market::of_class(market)?;
        let t = symbol.trim().to_ascii_uppercase();
        if t.is_empty() || !t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '-')) {
            return Err(ConnectorError::not_found(format!("Massive: bad symbol `{symbol}`, write it as Massive spells it (AAPL, X:BTCUSD, C:EURUSD, I:SPX)")));
        }
        let strip = |p: &str| t.strip_prefix(p).unwrap_or(&t).to_string();
        let native = match m {
            Market::Options => format!("O:{}", strip("O:")),
            Market::Crypto => format!("X:{}", strip("X:")),
            Market::Forex => format!("C:{}", strip("C:")),
            Market::Indices => format!("I:{}", strip("I:")),
            Market::Stocks | Market::Futures => t.clone(),
        };
        Ok(InstrumentId::new(m.venue(), &native))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => self.candles(*req_id, instrument, *tf, *range, s),
            // The latest quote: the last one before now.
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => self.quote(*req_id, instrument, ctx.received_ts, true, s),
            Command::RequestQuoteAt { req_id, instrument, at, before } => {
                // A past quote is kept for the markets declared; stocks cover ETFs too.
                let class = market_of(instrument).class();
                if !self.caps.history.quotes_at.contains(&class) {
                    return Err(ConnectorError::unsupported("Massive keeps no past quotes for this market"));
                }
                self.quote(*req_id, instrument, *at, *before, s)
            }
            Command::SearchInstruments { req_id, query, asset_class } => {
                let market = Market::of_class(*asset_class)?;
                s.pending.insert((*req_id, "search"), Pending::Search { market });
                Ok(vec![NativeRequest::Http(
                    HttpRequest::get("search", REST, "/v3/reference/tickers", *req_id)
                        .query("search", query)
                        .query("market", market.search())
                        .query("active", "true")
                        .query("limit", SEARCH_LIMIT)
                        .signed(),
                )])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("Massive: {other:?} is not declared"))),
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
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body.clone()),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(http::timeout(frame, "massive").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = refusal(frame, &body) {
            return Err(e);
        }
        match pending {
            // No `results` under an OK status: the window held no bars.
            Pending::Aggs(req) => {
                let page: Results<AggRow> = serde_json::from_slice(&body).map_err(|e| decode_err("aggregates", e))?;
                let mut out = Vec::with_capacity(page.results.len());
                for r in page.results {
                    out.extend(keep(&req, ts_millis(r.t.0)?, (r.o, r.h, r.l, r.c, r.v), r.n));
                }
                Ok(chunk(ctx, corr, out))
            }
            Pending::FuturesAggs(req) => {
                let page: Results<FuturesRow> = serde_json::from_slice(&body).map_err(|e| decode_err("futures aggregates", e))?;
                let mut out = Vec::with_capacity(page.results.len());
                for r in page.results {
                    out.extend(keep(&req, ts_nanos(r.window_start.0), (r.open, r.high, r.low, r.close, r.volume), None));
                }
                Ok(chunk(ctx, corr, out))
            }
            Pending::Quote { instrument } => {
                let page: Results<QuoteRow> = serde_json::from_slice(&body).map_err(|e| decode_err("quotes", e))?;
                let Some(q) = page.results.into_iter().next() else { return Ok(vec![]) };
                let positive = |d: OptDec| d.0.filter(|p| p.is_sign_positive() && !p.is_zero());
                let (Some(bid), Some(ask)) = (positive(q.bid_price), positive(q.ask_price)) else { return Ok(vec![]) };
                let e = env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: q.bid_size.0.map(Qty), ask_qty: q.ask_size.0.map(Qty) }));
                Ok(vec![match q.sip_timestamp {
                    Some(t) => e.at(ts_nanos(t.0)),
                    None => e,
                }])
            }
            Pending::Search { market } => {
                let page: Results<TickerRow> = serde_json::from_slice(&body).map_err(|e| decode_err("tickers", e))?;
                let mut out = Vec::with_capacity(page.results.len() + 1);
                for r in page.results {
                    let Some(t) = r.ticker.filter(|t| !t.is_empty()) else { continue };
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(market.venue(), &t), &t, "");
                    spec.symbol = CanonicalSymbol(r.name);
                    spec.asset_class = market.class();
                    spec.exchange = r.primary_exchange;
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
                out.push(env(ctx, corr, Event::InstrumentsEnd));
                Ok(out)
            }
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn keepalive(&self, s: &mut Session, _ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::drain(&mut s.live)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let key = secrets
            .get("api_key")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "Massive requires an api_key credential: set it on the settings page"))?;
        Ok(Some(Arc::new(MassiveSigner { key: SecretString::from(key.to_string()) })))
    }

    fn socket_class(&self, instrument: &InstrumentId, _data_type: &DataType) -> String {
        market_of(instrument).cluster().into()
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.exchange.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
