//! EODHD: end-of-day and intraday history, and symbol search. Keyed (`api_token` query
//! parameter, added by the signer). Endpoint-by-endpoint mapping in PROVIDER.md.

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    Adjusted, AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope,
    ErrorKind, Event, HistoryItems, InstrumentId, InstrumentSpec, Price, Qty, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_secs, Int, OptDec};
use otw_connector_sdk::signer::ApiKeyQuerySigner;
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;

pub const VENUE: &str = "EODHD";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://eodhd.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Results one search asks for (the most the endpoint answers).
const SEARCH_LIMIT: u32 = 30;

#[derive(Clone)]
pub struct Eodhd {
    caps: Capabilities,
}

impl Eodhd {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Eodhd { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Eod(CandleReq),
    Intraday(CandleReq),
    Search,
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
}

enum Series {
    /// `period` token of the end-of-day endpoint.
    Eod(&'static str),
    /// `interval` token of the intraday endpoint.
    Intraday(&'static str),
}

fn series(tf: Timeframe) -> Option<Series> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => Series::Intraday("1m"),
        (5, TfUnit::Minute) => Series::Intraday("5m"),
        (1, TfUnit::Hour) => Series::Intraday("1h"),
        (1, TfUnit::Day) => Series::Eod("d"),
        (1, TfUnit::Week) => Series::Eod("w"),
        _ => return None,
    })
}

/// EODHD's instrument type to a market. Anything unnamed is an equity.
fn market_of(kind: &str) -> AssetClass {
    match kind.to_ascii_lowercase().as_str() {
        "etf" | "etp" | "fund" | "mutual fund" | "mutualfund" => AssetClass::Etf,
        "crypto" | "cryptocurrency" | "digital currency" => AssetClass::Spot,
        "currency" | "fx" | "forex" | "physical currency" => AssetClass::Forex,
        "index" | "indices" => AssetClass::Index,
        "option" | "options" => AssetClass::Option,
        "future" | "futures" => AssetClass::Future,
        _ => AssetClass::Equity,
    }
}

/// Percent-encoding for a path segment (a search query lands in the path).
fn path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn date(t: Timestamp) -> Result<String, ConnectorError> {
    let d = t.to_offset_datetime().ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))?;
    Ok(format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()))
}

fn day(s: &str) -> Result<Timestamp, ConnectorError> {
    let d = time::Date::parse(s.get(..10).unwrap_or(s), &time::format_description::well_known::Iso8601::DATE)
        .map_err(|_| ConnectorError::decode(format!("eodhd date `{s}`")))?;
    Timestamp::from_offset_datetime(d.midnight().assume_utc()).ok_or_else(|| ConnectorError::decode(format!("eodhd date `{s}`")))
}

#[derive(Deserialize)]
struct EodRow {
    date: String,
    #[serde(default)]
    open: OptDec,
    #[serde(default)]
    high: OptDec,
    #[serde(default)]
    low: OptDec,
    #[serde(default)]
    close: OptDec,
    #[serde(default)]
    adjusted_close: OptDec,
    #[serde(default)]
    volume: OptDec,
}

#[derive(Deserialize)]
struct IntradayRow {
    timestamp: Int,
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
struct SearchRow {
    #[serde(rename = "Code", default)]
    code: Option<String>,
    #[serde(rename = "Exchange", default)]
    exchange: Option<String>,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Type", default)]
    kind: String,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("eodhd {what}: {e}"))
}

/// EODHD refuses by status: 401 bad token, 402 plan, 404 unknown ticker, 429 allowance.
fn status_error(frame: &RawFrame) -> Option<ConnectorError> {
    let mut e = http::timeout(frame, "eodhd").or_else(|| http::classify(frame, "eodhd"))?;
    match e.provider_code.as_deref() {
        Some("402") => e.kind = ErrorKind::Permission,
        Some("404") => e.message = format!("{}: bad symbol", e.message),
        _ => {}
    }
    Some(e)
}

fn candle(req: &CandleReq, open_ts: Timestamp, o: OptDec, h: OptDec, l: OptDec, c: OptDec, v: OptDec) -> Candle {
    let p = |d: OptDec| Price(d.0.unwrap_or_default());
    Candle {
        instrument: req.instrument.clone(),
        tf: req.tf,
        open_ts,
        open: p(o),
        high: p(h),
        low: p(l),
        close: p(c),
        volume: Qty(v.0.unwrap_or_default()),
        quote_volume: None,
        trades: None,
        is_final: false,
        adjusted: None,
        sides: None,
    }
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, mut candles: Vec<Candle>) -> Vec<Envelope> {
    candles.sort_by_key(|c| c.open_ts);
    vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })]
}

impl Mapper for Eodhd {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // EODHD addresses an instrument as CODE.EXCHANGE (AAPL.US, BMW.XETRA, EURUSD.FOREX).
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            return Err(ConnectorError::not_found(format!("EODHD: bad symbol `{symbol}`, write it as CODE.EXCHANGE (AAPL.US)")));
        }
        Ok(InstrumentId::new(VENUE, s))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => {
                let kind = series(*tf)
                    .filter(|_| self.caps.history_timeframes().contains(tf))
                    .ok_or_else(|| ConnectorError::unsupported(format!("EODHD serves no {tf} history here")))?;
                let req = CandleReq { instrument: instrument.clone(), tf: *tf, range: *range };
                let (endpoint, h, p) = match kind {
                    Series::Eod(period) => (
                        "eod",
                        HttpRequest::get("eod", REST, format!("/api/eod/{}", instrument.native), *req_id)
                            .query("from", date(range.from)?)
                            .query("to", date(range.to)?)
                            .query("period", period),
                        Pending::Eod(req),
                    ),
                    Series::Intraday(iv) => (
                        "intraday",
                        HttpRequest::get("intraday", REST, format!("/api/intraday/{}", instrument.native), *req_id)
                            .query("interval", iv)
                            .query("from", range.from.secs())
                            .query("to", range.to.secs()),
                        Pending::Intraday(req),
                    ),
                };
                s.pending.insert((*req_id, endpoint), p);
                Ok(vec![NativeRequest::Http(h.query("fmt", "json").signed())])
            }
            Command::SearchInstruments { req_id, query, .. } => {
                s.pending.insert((*req_id, "search"), Pending::Search);
                Ok(vec![NativeRequest::Http(
                    HttpRequest::get("search", REST, format!("/api/search/{}", path_segment(query)), *req_id)
                        .query("fmt", "json")
                        .query("limit", SEARCH_LIMIT)
                        .signed(),
                )])
            }
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("EODHD: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(status_error(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = status_error(frame) {
            return Err(e);
        }
        match pending {
            Pending::Eod(req) => {
                let rows: Vec<EodRow> = serde_json::from_slice(body).map_err(|e| decode_err("eod", e))?;
                let mut out = Vec::with_capacity(rows.len());
                for r in rows {
                    let open_ts = day(&r.date)?;
                    if req.range.contains(open_ts) {
                        let mut c = candle(&req, open_ts, r.open, r.high, r.low, r.close, r.volume);
                        c.adjusted = r.adjusted_close.0.map(|a| Adjusted { close: Some(Price(a)), ..Adjusted::default() });
                        out.push(c);
                    }
                }
                Ok(chunk(ctx, corr, out))
            }
            Pending::Intraday(req) => {
                let rows: Vec<IntradayRow> = serde_json::from_slice(body).map_err(|e| decode_err("intraday", e))?;
                let mut out = Vec::with_capacity(rows.len());
                for r in rows {
                    let open_ts = ts_secs(r.timestamp.0)?;
                    if req.range.contains(open_ts) {
                        out.push(candle(&req, open_ts, r.open, r.high, r.low, r.close, r.volume));
                    }
                }
                Ok(chunk(ctx, corr, out))
            }
            Pending::Search => {
                let rows: Vec<SearchRow> = serde_json::from_slice(body).map_err(|e| decode_err("search", e))?;
                let mut out = Vec::with_capacity(rows.len() + 1);
                for r in rows {
                    let Some(code) = r.code.filter(|c| !c.is_empty()) else { continue };
                    let exchange = r.exchange.filter(|e| !e.is_empty()).unwrap_or_else(|| "US".into());
                    // The joined CODE.EXCHANGE is what the bar endpoints take.
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &format!("{code}.{exchange}")), &code, "");
                    spec.symbol = CanonicalSymbol(r.name);
                    spec.asset_class = market_of(&r.kind);
                    spec.note = Some(exchange);
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

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let key = secrets
            .get("api_key")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "EODHD requires an api_key credential: set it on the settings page"))?;
        Ok(Some(Arc::new(ApiKeyQuerySigner { param: "api_token", key: SecretString::from(key.to_string()) })))
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.note.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
