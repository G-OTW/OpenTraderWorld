//! Alpha Vantage: daily and intraday time series, and symbol search. Keyed (`apikey` query
//! parameter, added by the signer). Endpoint-by-endpoint mapping in PROVIDER.md.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind,
    Event, HistoryItems, InstrumentId, InstrumentSpec, Price, Qty, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::signer::ApiKeyQuerySigner;
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::value::RawValue;

pub const VENUE: &str = "ALPHAVANTAGE";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://www.alphavantage.co")];
const CAPS_TOML: &str = include_str!("capabilities.toml");

#[derive(Clone)]
pub struct AlphaVantage {
    caps: Capabilities,
}

impl AlphaVantage {
    pub fn new() -> Result<Self, CapsError> {
        Ok(AlphaVantage { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Candles(CandleReq),
    Search,
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
}

/// The intraday interval token; daily has its own function.
fn intraday(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1min",
        (5, TfUnit::Minute) => "5min",
        (15, TfUnit::Minute) => "15min",
        (1, TfUnit::Hour) => "60min",
        _ => return None,
    })
}

/// Alpha Vantage's instrument type to a market. Anything unnamed is an equity.
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

/// `2024-01-02` or `2024-01-02 19:55:00`, the wall-clock time the series is written in.
fn wall(s: &str) -> Result<time::PrimitiveDateTime, ConnectorError> {
    let bad = || ConnectorError::decode(format!("alphavantage stamp `{s}`"));
    let (date, time) = s.trim().split_once([' ', 'T']).unwrap_or((s.trim(), "00:00:00"));
    let d = time::Date::parse(date, &time::format_description::well_known::Iso8601::DATE).map_err(|_| bad())?;
    let mut parts = time.split(':').map(|p| p.parse::<u8>().unwrap_or(0));
    let t = time::Time::from_hms(parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0)).map_err(|_| bad())?;
    Ok(d.with_time(t))
}

/// A daily row is its date at UTC midnight; an intraday stamp is read in the series' own
/// zone (`US/Eastern`) and becomes the UTC instant it names.
fn stamp(s: &str, zone: Option<&time_tz::Tz>) -> Result<Timestamp, ConnectorError> {
    use time_tz::{OffsetResult, PrimitiveDateTimeExt};
    let local = wall(s)?;
    let at = match zone {
        None => local.assume_utc(),
        Some(tz) => match local.assume_timezone(tz) {
            OffsetResult::Some(t) => t,
            // The repeated autumn hour: the series runs 04:00 to 20:00 and never writes it,
            // so the first occurrence is as good as the second.
            OffsetResult::Ambiguous(t, _) => t,
            OffsetResult::None => return Err(ConnectorError::decode(format!("alphavantage stamp `{s}` does not exist in the series' zone"))),
        },
    };
    Timestamp::from_offset_datetime(at).ok_or_else(|| ConnectorError::decode(format!("alphavantage stamp `{s}`")))
}

/// The zone an intraday series is written in, from its metadata (`6. Time Zone`).
fn series_zone(top: &BTreeMap<String, Box<RawValue>>) -> Result<&'static time_tz::Tz, ConnectorError> {
    let meta: BTreeMap<String, serde_json::Value> = top
        .get("Meta Data")
        .and_then(|m| serde_json::from_str(m.get()).ok())
        .ok_or_else(|| ConnectorError::decode("alphavantage series: no metadata, so no time zone"))?;
    let zone = meta
        .iter()
        .find(|(k, _)| k.ends_with("Time Zone"))
        .and_then(|(_, v)| v.as_str())
        .ok_or_else(|| ConnectorError::decode("alphavantage series: the metadata names no time zone"))?;
    time_tz::timezones::get_by_name(zone).ok_or_else(|| ConnectorError::decode(format!("alphavantage series: unknown time zone `{zone}`")))
}

#[derive(Deserialize)]
struct Bar {
    #[serde(rename = "1. open", default)]
    open: OptDec,
    #[serde(rename = "2. high", default)]
    high: OptDec,
    #[serde(rename = "3. low", default)]
    low: OptDec,
    #[serde(rename = "4. close", default)]
    close: OptDec,
    #[serde(rename = "5. volume", default)]
    volume5: OptDec,
    #[serde(rename = "6. volume", default)]
    volume6: OptDec,
    #[serde(rename = "5. adjusted close", default)]
    adjusted_close: OptDec,
}

#[derive(Deserialize)]
struct SearchAnswer {
    #[serde(rename = "bestMatches", default)]
    best_matches: Option<Vec<SearchRow>>,
}

#[derive(Deserialize)]
struct SearchRow {
    #[serde(rename = "1. symbol", default)]
    symbol: Option<String>,
    #[serde(rename = "2. name", default)]
    name: String,
    #[serde(rename = "3. type", default)]
    kind: String,
    #[serde(rename = "4. region", default)]
    region: String,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("alphavantage {what}: {e}"))
}

/// Alpha Vantage answers 200 and writes its refusal in the body: `Note` / `Information` for
/// the call allowance (or a premium-only endpoint), `Error Message` for a bad call.
fn body_error(top: &BTreeMap<String, Box<RawValue>>, endpoint: &str) -> Option<ConnectorError> {
    let text = |k: &str| top.get(k).and_then(|v| serde_json::from_str::<String>(v.get()).ok());
    if let Some(note) = text("Note").or_else(|| text("Information")) {
        let lower = note.to_ascii_lowercase();
        let kind = if lower.contains("rate limit") || lower.contains("requests per") || lower.contains("call frequency") {
            ErrorKind::RateLimited
        } else if lower.contains("premium") {
            ErrorKind::Permission
        } else {
            ErrorKind::InvalidRequest
        };
        return Some(ConnectorError::new(kind, format!("alphavantage {endpoint}: {note}")));
    }
    text("Error Message").map(|m| ConnectorError::new(ErrorKind::NotFound, format!("alphavantage {endpoint}: bad symbol ({m})")))
}

impl AlphaVantage {
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("Alpha Vantage serves no {tf} history here")));
        }
        // The free daily series is TIME_SERIES_DAILY, compact (the last 100 days): the
        // adjusted variant and the full size are premium. Intraday full is the recent month.
        let h = HttpRequest::get("series", REST, "/query", req_id).signed();
        let h = match intraday(tf) {
            None => h.query("function", "TIME_SERIES_DAILY").query("symbol", &instrument.native).query("outputsize", "compact"),
            Some(iv) => h.query("function", "TIME_SERIES_INTRADAY").query("symbol", &instrument.native).query("interval", iv).query("outputsize", "full"),
        };
        s.pending.insert((req_id, "series"), Pending::Candles(CandleReq { instrument: instrument.clone(), tf, range }));
        Ok(vec![NativeRequest::Http(h)])
    }

    fn decode_candles(&self, corr: CorrId, frame: &RawFrame, body: &[u8], req: CandleReq, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let CandleReq { instrument, tf, range } = req;
        let top: BTreeMap<String, Box<RawValue>> = serde_json::from_slice(body).map_err(|e| http::classify(frame, "alphavantage").unwrap_or_else(|| decode_err("series", e)))?;
        if let Some(e) = body_error(&top, "series") {
            return Err(e);
        }
        let series = top
            .iter()
            .find(|(k, _)| k.starts_with("Time Series"))
            .map(|(_, v)| v)
            .ok_or_else(|| ConnectorError::not_found(format!("alphavantage series: no series for {}, bad symbol?", instrument.native)))?;
        let rows: BTreeMap<String, Bar> = serde_json::from_str(series.get()).map_err(|e| decode_err("series rows", e))?;
        let zone = if intraday(tf).is_some() { Some(series_zone(&top)?) } else { None };
        let mut candles = Vec::with_capacity(rows.len());
        for (when, b) in rows {
            let open_ts = stamp(&when, zone)?;
            if !range.contains(open_ts) {
                continue;
            }
            let p = |d: OptDec| Price(d.0.unwrap_or_default());
            candles.push(Candle {
                instrument: instrument.clone(),
                tf,
                open_ts,
                open: p(b.open),
                high: p(b.high),
                low: p(b.low),
                close: p(b.close),
                volume: Qty(b.volume6.0.or(b.volume5.0).unwrap_or_default()),
                quote_volume: None,
                trades: None,
                is_final: false,
                adjusted: b.adjusted_close.0.map(|a| otw_connector_model::Adjusted { close: Some(Price(a)), ..Default::default() }),
                sides: None,
            });
        }
        candles.sort_by_key(|c| c.open_ts);
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })])
    }

    fn decode_search(&self, corr: CorrId, frame: &RawFrame, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let top: BTreeMap<String, Box<RawValue>> = serde_json::from_slice(body).map_err(|e| http::classify(frame, "alphavantage").unwrap_or_else(|| decode_err("search", e)))?;
        if let Some(e) = body_error(&top, "search") {
            return Err(e);
        }
        let answer: SearchAnswer = serde_json::from_slice(body).map_err(|e| decode_err("search", e))?;
        let rows = answer.best_matches.ok_or_else(|| ConnectorError::not_found("alphavantage search: no matches"))?;
        let mut out = Vec::with_capacity(rows.len() + 1);
        for r in rows {
            let Some(symbol) = r.symbol.filter(|s| !s.is_empty()) else { continue };
            let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &symbol), &symbol, "");
            spec.symbol = CanonicalSymbol(r.name);
            spec.asset_class = market_of(&r.kind);
            spec.note = Some(r.region);
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for AlphaVantage {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // As Alpha Vantage spells it (IBM, TSCO.LON, SHOP.TRT), kept as typed.
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':')) {
            return Err(ConnectorError::not_found(format!("Alpha Vantage: bad symbol `{symbol}`, write it as Alpha Vantage spells it (IBM, TSCO.LON)")));
        }
        Ok(InstrumentId::new(VENUE, s))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => self.candles(*req_id, instrument, *tf, *range, s),
            Command::SearchInstruments { req_id, query, .. } => {
                s.pending.insert((*req_id, "search"), Pending::Search);
                Ok(vec![NativeRequest::Http(
                    HttpRequest::get("search", REST, "/query", *req_id).query("function", "SYMBOL_SEARCH").query("keywords", query).signed(),
                )])
            }
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("Alpha Vantage: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(http::timeout(frame, "alphavantage").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        match pending {
            Pending::Candles(req) => self.decode_candles(corr, frame, body, req, ctx),
            Pending::Search => self.decode_search(corr, frame, body, ctx),
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
            .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "Alpha Vantage requires an api_key credential: set it on the settings page"))?;
        Ok(Some(Arc::new(ApiKeyQuerySigner { param: "apikey", key: SecretString::from(key.to_string()) })))
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.note.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
