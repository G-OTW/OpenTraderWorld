//! TradeStation v3: bar charts, quotes and symbol details on the market-data side, and the
//! brokerage account read side. Authenticated with an OAuth refresh token exchanged for a
//! twenty-minute access token at the sign-in host (the SDK's [`OAuthRefresh`]); the
//! `MarketData` and `ReadAccount` scopes ride on the same key. The account id is written into
//! the path by the signer. Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event,
    HistoryItems, InstrumentId, InstrumentSpec, Price, Qty, Quote, TfUnit, TimeRange, Timeframe, Timestamp, Trade,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::signer::{SignableRequest, TokenRequest};
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, OAuthRefresh, RawFrame, Signer};
use serde::Deserialize;

pub const VENUE: &str = "TRADESTATION";
const REST: &str = "rest";
const AUTH: &str = "auth";
const BASES: &[(&str, &str)] = &[(REST, "https://api.tradestation.com"), (AUTH, "https://signin.tradestation.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Where the signer writes the account id.
const ACCOUNT: &str = "{account}";
/// An access token lasts twenty minutes when the answer does not say.
const TOKEN_TTL_SECS: i64 = 20 * 60;
/// Symbols per details call.
const RESOLVE_BATCH: usize = 50;

#[derive(Clone)]
pub struct TradeStation {
    caps: Capabilities,
}

impl TradeStation {
    pub fn new() -> Result<Self, CapsError> {
        Ok(TradeStation { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// One path segment, percent-encoded (`@ES`, `$SPX.X`, `AAPL 251219C150`, a comma list).
pub(crate) fn segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// TradeStation's asset type as a market. A type it adds later is not mapped (`None`): an
/// instrument nobody identified is never dressed up as one it is not.
fn class_of(asset_type: &str) -> Option<AssetClass> {
    Some(match asset_type.to_ascii_uppercase().as_str() {
        "STOCK" => AssetClass::Equity,
        "STOCKOPTION" | "INDEXOPTION" | "FUTUREOPTION" => AssetClass::Option,
        "FUTURE" => AssetClass::Future,
        "INDEX" => AssetClass::Index,
        "FOREX" => AssetClass::Forex,
        "CRYPTO" => AssetClass::Spot,
        _ => return None,
    })
}

/// An OCC option (`AAPL251219C00150000`) in TradeStation's spelling (`AAPL 251219C150`).
fn ts_option(occ: &str) -> Option<String> {
    let s = occ.trim();
    let n = s.len();
    if n < 16 || !s.is_ascii() {
        return None;
    }
    let (root, tail) = s.split_at(n - 15);
    let (date, rest) = tail.split_at(6);
    let (cp, strike) = rest.split_at(1);
    if root.is_empty() || !date.bytes().all(|b| b.is_ascii_digit()) || !matches!(cp, "C" | "P") || !strike.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let strike = Decimal::from_str_exact(strike).ok()?.checked_div(Decimal::from(1000))?.normalize();
    Some(format!("{root} {date}{cp}{strike}"))
}

fn bar_unit(tf: Timeframe) -> Option<(&'static str, u32)> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => ("Minute", 1),
        (5, TfUnit::Minute) => ("Minute", 5),
        (15, TfUnit::Minute) => ("Minute", 15),
        (1, TfUnit::Day) => ("Daily", 1),
        _ => return None,
    })
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Bars(CandleReq),
    Quote { instrument: InstrumentId, last: bool },
    Symbols,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
}

#[derive(Deserialize)]
struct BarRow {
    #[serde(rename = "TimeStamp")]
    time_stamp: String,
    #[serde(rename = "Open", default)]
    open: OptDec,
    #[serde(rename = "High", default)]
    high: OptDec,
    #[serde(rename = "Low", default)]
    low: OptDec,
    #[serde(rename = "Close", default)]
    close: OptDec,
    #[serde(rename = "TotalVolume", default)]
    total_volume: OptDec,
    #[serde(rename = "BarStatus", default)]
    bar_status: String,
}

#[derive(Deserialize)]
struct BarsAnswer {
    #[serde(rename = "Bars", default)]
    bars: Option<Vec<BarRow>>,
}

#[derive(Deserialize)]
struct QuoteRow {
    #[serde(rename = "Symbol", default)]
    symbol: String,
    #[serde(rename = "Bid", default)]
    bid: OptDec,
    #[serde(rename = "Ask", default)]
    ask: OptDec,
    #[serde(rename = "BidSize", default)]
    bid_size: OptDec,
    #[serde(rename = "AskSize", default)]
    ask_size: OptDec,
    #[serde(rename = "Last", default)]
    last: OptDec,
    #[serde(rename = "LastSize", default)]
    last_size: OptDec,
    #[serde(rename = "TradeTime", default)]
    trade_time: Option<String>,
}

#[derive(Deserialize)]
struct QuotesAnswer {
    #[serde(rename = "Quotes", default)]
    quotes: Vec<QuoteRow>,
}

#[derive(Deserialize)]
struct PriceFormat {
    #[serde(rename = "PointValue", default)]
    point_value: OptDec,
}

#[derive(Deserialize)]
struct SymbolRow {
    #[serde(rename = "Symbol", default)]
    symbol: String,
    #[serde(rename = "Description", default)]
    description: String,
    #[serde(rename = "AssetType", default)]
    asset_type: String,
    #[serde(rename = "Exchange", default)]
    exchange: String,
    #[serde(rename = "Currency", default)]
    currency: String,
    #[serde(rename = "PriceFormat", default)]
    price_format: Option<PriceFormat>,
}

#[derive(Deserialize)]
struct SymbolsAnswer {
    #[serde(rename = "Symbols", default)]
    symbols: Vec<SymbolRow>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

pub(crate) fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("tradestation {what}: {e}"))
}

/// The fault a status means. A 404 on a bar chart or a quote is a symbol TradeStation does
/// not know; a 401 a token it refused.
pub(crate) fn refusal(frame: &RawFrame) -> Option<ConnectorError> {
    let mut e = http::timeout(frame, "tradestation").or_else(|| http::classify(frame, "tradestation"))?;
    if e.provider_code.as_deref() == Some("404") {
        e.message = format!("{}: bad symbol", e.message);
    }
    if e.kind == ErrorKind::Auth {
        e.message = format!("{}. Check the refresh token belongs to this environment and that the sign-in granted ReadAccount and MarketData", e.message);
    }
    Some(e)
}

fn rfc3339(t: Timestamp) -> Result<String, ConnectorError> {
    t.to_offset_datetime()
        .and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok())
        .ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))
}

/// The OAuth token for every call, and the account id for the brokerage paths.
pub struct TradeStationSigner {
    oauth: OAuthRefresh,
    account: Option<String>,
}

impl Signer for TradeStationSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        if req.path.contains(ACCOUNT) {
            let a = self.account.as_deref().ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "TradeStation requires the account_id setting: the account number this connector reads"))?;
            req.path = req.path.replace(ACCOUNT, &segment(a));
        }
        self.oauth.bearer(req, now)
    }

    fn token_request(&self, now: Timestamp) -> Option<TokenRequest> {
        self.oauth.request(now)
    }

    fn accept_token(&self, status: u16, _headers: &[(String, String)], body: &[u8], now: Timestamp) -> Result<(), ConnectorError> {
        self.oauth.accept(status, body, now)
    }

    fn forget_token(&self) {
        self.oauth.forget();
    }
}

impl TradeStation {
    fn bars(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let (unit, interval) = bar_unit(tf).ok_or_else(|| {
            ConnectorError::unsupported(format!(
                "TradeStation builds intraday bars from the session open, so its {tf} candle would not line up with the rest of the library. Download 15m and read it at {tf} instead"
            ))
        })?;
        let h = HttpRequest::get("bars", REST, format!("/v3/marketdata/barcharts/{}", segment(&instrument.native)), req_id)
            .query("unit", unit)
            .query("interval", interval)
            .query("firstdate", rfc3339(range.from)?)
            .query("lastdate", rfc3339(range.to)?)
            .signed();
        s.pending.insert((req_id, "bars"), Pending::Bars(CandleReq { instrument: instrument.clone(), tf, range }));
        Ok(vec![NativeRequest::Http(h)])
    }

    /// A bar is stamped at its close: an intraday bar opened one interval earlier, a daily
    /// bar is its session's date at midnight UTC.
    fn opened_at(close: Timestamp, tf: Timeframe) -> Result<Timestamp, ConnectorError> {
        match tf.unit {
            TfUnit::Day => close
                .to_offset_datetime()
                .and_then(|d| Timestamp::from_offset_datetime(d.replace_time(time::Time::MIDNIGHT)))
                .ok_or_else(|| ConnectorError::decode(format!("tradestation bar time {close} out of range"))),
            _ => {
                let secs = tf.fixed_secs().ok_or_else(|| ConnectorError::decode(format!("tradestation: no fixed length for {tf}")))?;
                Ok(close.plus_secs(-secs))
            }
        }
    }

    fn decode_bars(&self, corr: CorrId, body: &[u8], req: CandleReq, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let a: BarsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("bars", e))?;
        let rows = a.bars.ok_or_else(|| ConnectorError::decode(format!("TradeStation answered no bar list for {}", req.instrument.native)))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let open_ts = Self::opened_at(ts_rfc3339(&r.time_stamp)?, req.tf)?;
            if !req.range.contains(open_ts) {
                continue;
            }
            let p = |d: OptDec, k: &str| d.0.map(Price).ok_or_else(|| ConnectorError::decode(format!("TradeStation bar field {k}")));
            out.push(Candle {
                instrument: req.instrument.clone(),
                tf: req.tf,
                open_ts,
                open: p(r.open, "Open")?,
                high: p(r.high, "High")?,
                low: p(r.low, "Low")?,
                close: p(r.close, "Close")?,
                volume: Qty(r.total_volume.0.unwrap_or_default()),
                quote_volume: None,
                trades: None,
                // `Open` is the bar still forming; every other status is a closed one.
                is_final: !r.bar_status.eq_ignore_ascii_case("Open"),
                adjusted: None,
                sides: None,
            });
        }
        out.sort_by_key(|c| c.open_ts);
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(out), next_cursor: None, done: true })])
    }

    fn symbols(&self, req_id: CorrId, list: &str, s: &mut Session) -> Vec<NativeRequest> {
        s.pending.insert((req_id, "symbols"), Pending::Symbols);
        vec![NativeRequest::Http(HttpRequest::get("symbols", REST, format!("/v3/marketdata/symbols/{}", segment(list)), req_id).signed())]
    }
}

impl Mapper for TradeStation {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// TradeStation's own spelling, uppercased (`AAPL`, `@ES`, `ESZ25`, `$SPX.X`,
    /// `AAPL 251219C150`).
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_ascii_uppercase();
        if s.is_empty() || s.chars().any(|c| c.is_control() || c == ',') {
            return Err(ConnectorError::not_found(format!("TradeStation: bad symbol `{symbol}`, write it as TradeStation does (AAPL, @ES, ESZ25)")));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    /// An option given as OCC is written TradeStation's way; anything else as typed.
    fn instrument_for(&self, symbol: &str, market: Option<AssetClass>) -> Result<InstrumentId, ConnectorError> {
        match market {
            Some(AssetClass::Option) => match ts_option(&symbol.trim().to_ascii_uppercase()) {
                Some(ts) => self.instrument_id(&ts),
                None => self.instrument_id(symbol),
            },
            _ => self.instrument_id(symbol),
        }
    }

    fn own_option_symbols(&self) -> bool {
        true
    }

    fn probe_symbol(&self) -> Option<&'static str> {
        Some("SPY")
    }

    fn resolve_batch(&self) -> usize {
        RESOLVE_BATCH
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: otw_connector_model::DataType::Candles { tf }, range, .. } => self.bars(*req_id, instrument, *tf, *range, s),
            Command::RequestSnapshot { req_id, instrument, data_type } => {
                let last = match data_type {
                    otw_connector_model::DataType::Quote => false,
                    otw_connector_model::DataType::Trades => true,
                    other => return Err(ConnectorError::unsupported(format!("TradeStation: no {other:?} snapshot"))),
                };
                s.pending.insert((*req_id, "quotes"), Pending::Quote { instrument: instrument.clone(), last });
                Ok(vec![NativeRequest::Http(HttpRequest::get("quotes", REST, format!("/v3/marketdata/quotes/{}", segment(&instrument.native)), *req_id).signed())])
            }
            // No free-text search: the symbol details say whether the one typed exists.
            Command::SearchInstruments { req_id, query, .. } => {
                let q = query.trim().to_ascii_uppercase();
                if q.is_empty() || q.contains(',') {
                    return Ok(vec![]);
                }
                Ok(self.symbols(*req_id, &q, s))
            }
            Command::ResolveInstruments { req_id, ids } => {
                if ids.len() > RESOLVE_BATCH {
                    return Err(ConnectorError::invalid(format!("TradeStation resolves {RESOLVE_BATCH} symbols per call")));
                }
                let list: Vec<&str> = ids.iter().map(|i| i.native.as_str()).collect();
                Ok(self.symbols(*req_id, &list.join(","), s))
            }
            Command::QueryAccount { .. } => account::encode(cmd, s),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("TradeStation: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body.clone()),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(http::timeout(frame, "tradestation").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = refusal(frame) {
            return Err(e);
        }
        match pending {
            Pending::Bars(req) => self.decode_bars(corr, &body, req, ctx),
            Pending::Quote { instrument, last } => {
                let a: QuotesAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("quotes", e))?;
                let Some(q) = a.quotes.into_iter().find(|q| q.symbol.eq_ignore_ascii_case(&instrument.native)) else { return Ok(vec![]) };
                let at = q.trade_time.as_deref().and_then(|t| ts_rfc3339(t).ok());
                let event = if last {
                    let Some(price) = q.last.0 else { return Ok(vec![]) };
                    Event::Trade(Trade { instrument, id: None, price: Price(price), qty: Qty(q.last_size.0.unwrap_or_default()), aggressor: None })
                } else {
                    let (Some(bid), Some(ask)) = (q.bid.0, q.ask.0) else { return Ok(vec![]) };
                    Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: q.bid_size.0.map(Qty), ask_qty: q.ask_size.0.map(Qty) })
                };
                let e = env(ctx, corr, event);
                Ok(vec![match at {
                    Some(t) => e.at(t),
                    None => e,
                }])
            }
            Pending::Symbols => {
                // Unknown symbols come back under `Errors`: simply not found.
                let a: SymbolsAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("symbols", e))?;
                let mut out = Vec::with_capacity(a.symbols.len() + 1);
                for r in a.symbols.into_iter().filter(|r| !r.symbol.is_empty()) {
                    let Some(class) = class_of(&r.asset_type) else { continue };
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &r.symbol.to_ascii_uppercase()), &r.symbol, &r.currency.to_ascii_uppercase());
                    spec.base = None;
                    spec.symbol = CanonicalSymbol(r.description);
                    spec.asset_class = class;
                    // The point value is read, never assumed: an adjusted option is not a
                    // hundred shares and a future is never one.
                    spec.multiplier = r.price_format.and_then(|f| f.point_value.0).filter(|v| v.is_sign_positive() && !v.is_zero()).unwrap_or(Decimal::ONE);
                    spec.exchange = Some(r.exchange).filter(|e| !e.is_empty());
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
                out.push(env(ctx, corr, Event::InstrumentsEnd));
                Ok(out)
            }
            Pending::Account(p) => account::decode(p, corr, &body, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |k: &str| secrets.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let need = |k: &str| get(k).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("TradeStation requires a {k} credential: set it on the settings page")));
        let oauth = OAuthRefresh::new("TradeStation", AUTH, "/oauth/token", need("client_id")?, need("client_secret")?, need("refresh_token")?, TOKEN_TTL_SECS);
        Ok(Some(Arc::new(TradeStationSigner { oauth, account: get("account_id").map(str::to_string) })))
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.exchange.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
