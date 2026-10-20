//! Yahoo Finance: candle history (chart v8) and symbol search (search v1). Keyless,
//! unofficial, best effort. Endpoint-by-endpoint mapping in PROVIDER.md.

use std::collections::HashMap;

use otw_connector_model::{
    Adjusted, AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope,
    ErrorKind, Event, HistoryItems, InstrumentId, InstrumentSpec, Price, TfUnit, TimeRange, Timeframe,
};
use otw_connector_sdk::parse::{ts_secs, OptDec};
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame};
use serde::Deserialize;

pub const VENUE: &str = "YAHOO";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://query1.finance.yahoo.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Results one search asks for (the most the endpoint answers).
const SEARCH_COUNT: u32 = 25;

#[derive(Clone)]
pub struct Yahoo {
    caps: Capabilities,
}

impl Yahoo {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Yahoo { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Candles(CandleReq),
    Search { market: Option<AssetClass> },
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
}

/// Yahoo's interval token, and the days of history it keeps at that interval (it refuses,
/// rather than clamps, a window starting earlier).
fn interval(tf: Timeframe) -> Option<(&'static str, Option<i64>)> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => ("1m", Some(30)),
        (5, TfUnit::Minute) => ("5m", Some(60)),
        (15, TfUnit::Minute) => ("15m", Some(60)),
        (1, TfUnit::Hour) => ("1h", Some(730)),
        (1, TfUnit::Day) => ("1d", None),
        (1, TfUnit::Week) => ("1wk", None),
        _ => return None,
    })
}

/// Yahoo's `quoteType` to a market. Anything unnamed is an equity, as the search shows it.
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

#[derive(Deserialize)]
struct ChartAnswer {
    chart: Chart,
}

#[derive(Deserialize)]
struct Chart {
    #[serde(default)]
    result: Option<Vec<ChartResult>>,
    #[serde(default)]
    error: Option<ChartError>,
}

#[derive(Deserialize)]
struct ChartError {
    #[serde(default)]
    code: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct ChartResult {
    #[serde(default)]
    timestamp: Vec<Option<i64>>,
    #[serde(default)]
    indicators: Indicators,
}

#[derive(Deserialize, Default)]
struct Indicators {
    #[serde(default)]
    quote: Vec<Columns>,
    #[serde(default)]
    adjclose: Vec<AdjColumn>,
}

#[derive(Deserialize, Default)]
struct Columns {
    #[serde(default)]
    open: Vec<OptDec>,
    #[serde(default)]
    high: Vec<OptDec>,
    #[serde(default)]
    low: Vec<OptDec>,
    #[serde(default)]
    close: Vec<OptDec>,
    #[serde(default)]
    volume: Vec<OptDec>,
}

#[derive(Deserialize, Default)]
struct AdjColumn {
    #[serde(default)]
    adjclose: Vec<OptDec>,
}

#[derive(Deserialize)]
struct SearchAnswer {
    #[serde(default)]
    quotes: Vec<SearchRow>,
}

#[derive(Deserialize)]
struct SearchRow {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(rename = "quoteType", default)]
    quote_type: String,
    #[serde(default)]
    shortname: Option<String>,
    #[serde(default)]
    longname: Option<String>,
    #[serde(rename = "exchDisp", default)]
    exch_disp: Option<String>,
    #[serde(default)]
    exchange: Option<String>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("yahoo {what}: {e}"))
}

/// Yahoo puts its refusal in the body whatever the status (a 404 for an unknown symbol, a
/// 422 for a window past what it keeps), and a bare 429 when throttled.
fn chart_error(e: &ChartError, endpoint: &str) -> ConnectorError {
    let lower = format!("{} {}", e.code, e.description).to_ascii_lowercase();
    let kind = if lower.contains("too many requests") {
        ErrorKind::RateLimited
    } else if e.code == "Not Found" {
        ErrorKind::NotFound
    } else {
        ErrorKind::InvalidRequest
    };
    let message = if kind == ErrorKind::NotFound {
        format!("yahoo {endpoint}: bad symbol ({})", e.description)
    } else {
        format!("yahoo {endpoint}: {}", e.description)
    };
    ConnectorError::new(kind, message).with_code(e.code.clone())
}

impl Yahoo {
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let (token, keeps_days) = interval(tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("Yahoo serves no {tf} history here")))?;
        // Pull the floor in by a day so a window sitting exactly on the depth Yahoo keeps
        // does not tip past it while the request is in flight.
        let from = match keeps_days {
            Some(days) => range.from.max(range.to.plus_secs(-(days - 1) * 86_400)),
            None => range.from,
        };
        s.pending.insert((req_id, "chart"), Pending::Candles(CandleReq { instrument: instrument.clone(), tf, range: TimeRange::new(from, range.to) }));
        Ok(vec![NativeRequest::Http(
            HttpRequest::get("chart", REST, format!("/v8/finance/chart/{}", instrument.native), req_id)
                .query("interval", token)
                .query("period1", from.secs())
                .query("period2", range.to.secs()),
        )])
    }

    fn decode_candles(&self, corr: CorrId, frame: &RawFrame, body: &[u8], req: CandleReq, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let CandleReq { instrument, tf, range } = req;
        let answer = match serde_json::from_slice::<ChartAnswer>(body) {
            Ok(a) => a,
            // Not Yahoo's JSON: the status says what happened (a bare 429 when throttled).
            Err(e) => return Err(http::classify(frame, "yahoo").unwrap_or_else(|| decode_err("chart", e))),
        };
        if let Some(e) = &answer.chart.error {
            return Err(chart_error(e, "chart"));
        }
        let res = answer
            .chart
            .result
            .and_then(|r| r.into_iter().next())
            .ok_or_else(|| ConnectorError::not_found(format!("yahoo chart: no data for {}, bad symbol?", instrument.native)))?;
        let cols = res.indicators.quote.into_iter().next().unwrap_or_default();
        let adj = res.indicators.adjclose.into_iter().next().unwrap_or_default();
        let at = |v: &[OptDec], i: usize| v.get(i).and_then(|d| d.0);
        let mut candles = Vec::with_capacity(res.timestamp.len());
        for (i, t) in res.timestamp.iter().enumerate() {
            let Some(t) = t else { continue };
            // Yahoo writes nulls for a gap: the row is skipped, not invented.
            let (Some(o), Some(h), Some(l), Some(c)) = (at(&cols.open, i), at(&cols.high, i), at(&cols.low, i), at(&cols.close, i)) else {
                continue;
            };
            let open_ts = ts_secs(*t)?;
            if !range.contains(open_ts) {
                continue;
            }
            candles.push(Candle {
                instrument: instrument.clone(),
                tf,
                open_ts,
                open: Price(o),
                high: Price(h),
                low: Price(l),
                close: Price(c),
                volume: otw_connector_model::Qty(at(&cols.volume, i).unwrap_or_default()),
                quote_volume: None,
                trades: None,
                // No forming flag: the engine finalizes at the period's end.
                is_final: false,
                adjusted: at(&adj.adjclose, i).map(|a| Adjusted { close: Some(Price(a)), ..Adjusted::default() }),
                sides: None,
            });
        }
        candles.sort_by_key(|c| c.open_ts);
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })])
    }

    fn decode_search(&self, corr: CorrId, frame: &RawFrame, body: &[u8], market: Option<AssetClass>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let answer: SearchAnswer = serde_json::from_slice(body).map_err(|e| http::classify(frame, "yahoo").unwrap_or_else(|| decode_err("search", e)))?;
        let mut out = Vec::with_capacity(answer.quotes.len() + 1);
        for q in answer.quotes {
            let Some(symbol) = q.symbol.filter(|s| !s.is_empty()) else { continue };
            let class = market_of(&q.quote_type);
            if market.is_some_and(|m| m != class) {
                continue;
            }
            let name = q.shortname.or(q.longname).unwrap_or_default();
            let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &symbol), &symbol, "");
            spec.symbol = CanonicalSymbol(name);
            spec.asset_class = class;
            spec.note = q.exch_disp.or(q.exchange);
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for Yahoo {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // Yahoo's own spelling (AAPL, ^GSPC, BTC-USD, EURUSD=X, BRK-B), kept as typed. It
        // lands in a URL path, so nothing else passes.
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '^' | '.' | '-' | '=' | '_')) {
            return Err(ConnectorError::not_found(format!("Yahoo: bad symbol `{symbol}`, write it as Yahoo spells it (AAPL, ^GSPC, EURUSD=X)")));
        }
        Ok(InstrumentId::new(VENUE, s))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => self.candles(*req_id, instrument, *tf, *range, s),
            Command::SearchInstruments { req_id, query, asset_class } => {
                s.pending.insert((*req_id, "search"), Pending::Search { market: *asset_class });
                Ok(vec![NativeRequest::Http(
                    HttpRequest::get("search", REST, "/v1/finance/search", *req_id).query("q", query).query("quotesCount", SEARCH_COUNT).query("newsCount", 0),
                )])
            }
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("Yahoo: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(http::timeout(frame, "yahoo").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        match pending {
            Pending::Candles(req) => self.decode_candles(corr, frame, body, req, ctx),
            Pending::Search { market } => self.decode_search(corr, frame, body, market, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.note.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
