//! FOREX.com / StoneX Trading API: mid price bars with the bid and ask per side, market
//! search, and the account read side (trades, open positions, working stop and limit orders).
//!
//! A session is opened with the account's username, password and AppKey (no API key stands
//! on its own) and every call carries it in two headers; the signer opens it once and shares
//! it. Most calls need something another call answers first: the client account id (for a
//! market search), the trading account (for every account read), the numeric market id
//! behind a name (for bars). The mapper chains them (`Mapper::follow_ups`), one command at a
//! time. Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event,
    HistoryItems, InstrumentId, InstrumentSpec, Ohlc, Price, Qty, Sides, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::signer::{cached_token, credential_key, drop_token, keep_token, SignableRequest, TokenBody, TokenRequest};
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

pub const VENUE: &str = "FOREXCOM";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://ciapi.cityindex.com/TradingAPI")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// A session is guaranteed for twenty minutes.
const SESSION_TTL_SECS: i64 = 20 * 60;
/// Bars one call returns at most.
const MAX_BARS: u32 = 4000;
/// Markets a name lookup reads before deciding it matches exactly one.
const LOOKUP: u32 = 20;
/// Markets a search asks for, the endpoint's ceiling.
const SEARCH: u32 = 200;
/// Endpoint ids for the name lookups of one batch.
const FIND: &[&str] = &["find0", "find1", "find2", "find3", "find4", "find5", "find6", "find7", "find8", "find9", "find10", "find11", "find12", "find13", "find14", "find15"];

#[derive(Clone)]
pub struct ForexCom {
    caps: Capabilities,
}

impl ForexCom {
    pub fn new() -> Result<Self, CapsError> {
        Ok(ForexCom { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// A cash pair is written `EUR/USD`: two three-letter codes and a slash, the venue's own
/// notation. Everything else is a CFD on an index, a commodity or a share.
pub(crate) fn class_of(market_name: &str) -> AssetClass {
    match market_name.split_once('/') {
        Some((b, q)) if b.len() == 3 && q.len() == 3 && b.bytes().all(|c| c.is_ascii_alphabetic()) && q.bytes().all(|c| c.is_ascii_alphabetic()) => AssetClass::Forex,
        _ => AssetClass::Cfd,
    }
}

/// StoneX writes a time as `/Date(1289231327280)/`, milliseconds, sometimes with an offset
/// that says nothing (`+0000`): every time is UTC.
pub(crate) fn wcf(raw: &str) -> Option<Timestamp> {
    let inner = raw.trim().strip_prefix("/Date(")?.strip_suffix(")/")?;
    let end = inner.char_indices().skip(1).find(|(_, c)| !c.is_ascii_digit()).map_or(inner.len(), |(i, _)| i);
    Timestamp::from_millis(inner.get(..end)?.parse::<i64>().ok()?)
}

/// A number StoneX writes as a float (`401484347.0`), as the integer it is.
pub(crate) fn int_text(d: &OptDec) -> Option<String> {
    let v = d.0?.trunc().normalize();
    (!v.is_zero()).then(|| v.to_string())
}

fn bar_interval(tf: Timeframe) -> Option<(&'static str, u32)> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => ("MINUTE", 1),
        (5, TfUnit::Minute) => ("MINUTE", 5),
        (15, TfUnit::Minute) => ("MINUTE", 15),
        (1, TfUnit::Hour) => ("HOUR", 1),
        (1, TfUnit::Day) => ("DAY", 1),
        _ => return None,
    })
}

#[derive(Clone)]
pub(crate) struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

/// What a step of a chain is waiting for.
pub(crate) enum Pending {
    /// The account tree, then what the command needs it for.
    Tree(Then),
    /// The market id behind a name, then its bars.
    Find(CandleReq),
    /// One side of the bars; the sides already read ride along.
    Bars { req: CandleReq, market: String, side: u8, mid: Vec<(Timestamp, Ohlc)>, bid: Vec<(Timestamp, Ohlc)> },
    Search,
    Resolve(InstrumentId),
    Account(account::Pending),
}

pub(crate) enum Then {
    Find(CandleReq),
    Search(String),
    Resolve(Vec<InstrumentId>),
    Account(account::Then),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    next: HashMap<CorrId, Vec<NativeRequest>>,
}

impl Session {
    pub(crate) fn put(&mut self, corr: CorrId, h: HttpRequest, p: Pending) -> NativeRequest {
        self.pending.insert((corr, h.endpoint_id), p);
        NativeRequest::Http(h)
    }

    pub(crate) fn then(&mut self, corr: CorrId, r: NativeRequest) {
        self.next.entry(corr).or_default().push(r);
    }
}

#[derive(Deserialize)]
pub(crate) struct TradingAccount {
    #[serde(rename = "TradingAccountId", default)]
    pub id: OptDec,
    #[serde(rename = "TradingAccountCode", default)]
    pub code: String,
}

#[derive(Deserialize)]
pub(crate) struct Tree {
    #[serde(rename = "ClientAccountId", default)]
    pub client: OptDec,
    #[serde(rename = "TradingAccounts", default)]
    pub trading: Vec<TradingAccount>,
}

#[derive(Deserialize)]
struct MarketRow {
    #[serde(rename = "MarketId", default)]
    id: OptDec,
    #[serde(rename = "Name", default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct Markets {
    #[serde(rename = "Markets", default)]
    markets: Vec<MarketRow>,
}

#[derive(Deserialize)]
struct BarRow {
    #[serde(rename = "BarDate", default)]
    bar_date: String,
    #[serde(rename = "Open", default)]
    open: OptDec,
    #[serde(rename = "High", default)]
    high: OptDec,
    #[serde(rename = "Low", default)]
    low: OptDec,
    #[serde(rename = "Close", default)]
    close: OptDec,
}

#[derive(Deserialize)]
struct BarsAnswer {
    #[serde(rename = "PriceBars", default)]
    bars: Option<Vec<BarRow>>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

pub(crate) fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("forexcom {what}: {e}"))
}

/// The fault a status means. A 401 is a session ended elsewhere (a sign-in on another device
/// closes this one): the signer drops it and the next call opens another.
pub(crate) fn refusal(frame: &RawFrame) -> Option<ConnectorError> {
    let mut e = http::timeout(frame, "forexcom").or_else(|| http::classify(frame, "forexcom"))?;
    if e.kind == ErrorKind::Auth {
        e.message = "FOREX.com ended this session. Signing in again on another device closes the one here; retry, and check the password if it keeps happening".into();
    }
    Some(e)
}

fn get(endpoint: &'static str, path: impl Into<String>, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, corr).signed()
}

pub(crate) fn tree_request(s: &mut Session, corr: CorrId, then: Then) -> NativeRequest {
    s.put(corr, get("tree", "/useraccount/ClientAndTradingAccount", corr), Pending::Tree(then))
}

fn search_request(endpoint: &'static str, client: &str, query: &str, max: u32, corr: CorrId) -> HttpRequest {
    get(endpoint, "/market/search", corr)
        .query("searchByMarketName", "true")
        .query("searchByMarketCode", "true")
        .query("cfdProductType", "true")
        .query("query", query.trim())
        .query("maxResults", max)
        .query("ClientAccountId", client)
}

fn bars_request(req: &CandleReq, market: &str, side: u8, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
    let (interval, span) = bar_interval(req.tf).ok_or_else(|| ConnectorError::unsupported(format!("FOREX.com serves no {} bar here", req.tf)))?;
    let price = match side {
        0 => "MID",
        1 => "BID",
        _ => "ASK",
    };
    Ok(get("bars", format!("/market/{market}/barhistorybetween"), corr)
        .query("interval", interval)
        .query("span", span)
        .query("fromTimestampUTC", req.range.from.secs())
        .query("toTimestampUTC", req.range.to.secs())
        .query("maxResults", MAX_BARS)
        .query("priceType", price))
}

/// The session for a login, opened once and shared by every call on it.
pub struct ForexComSigner {
    user: String,
    password: SecretString,
    app_key: SecretString,
    key: String,
}

impl Signer for ForexComSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let t = cached_token(&self.key, now).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "no FOREX.com session: the sign-in did not answer"))?;
        // The two headers StoneX authenticates on; never the query string.
        req.headers.push(("UserName".into(), self.user.clone()));
        req.headers.push(("Session".into(), t.expose_secret().to_string()));
        Ok(())
    }

    fn token_request(&self, now: Timestamp) -> Option<TokenRequest> {
        if cached_token(&self.key, now).is_some() {
            return None;
        }
        let body = serde_json::json!({
            "UserName": self.user,
            "Password": self.password.expose_secret(),
            "AppKey": self.app_key.expose_secret(),
            "AppVersion": "1",
            "AppComments": "",
        });
        Some(TokenRequest::post(REST, "/session", TokenBody::Json(body.to_string())))
    }

    fn accept_token(&self, status: u16, _headers: &[(String, String)], body: &[u8], now: Timestamp) -> Result<(), ConnectorError> {
        #[derive(Deserialize)]
        struct Answer {
            #[serde(rename = "Session", default)]
            session: Option<String>,
        }
        if !(200..300).contains(&status) {
            let snippet: String = String::from_utf8_lossy(body).chars().take(200).collect();
            return Err(ConnectorError::new(
                ErrorKind::Auth,
                format!("FOREX.com refused the sign-in ({status}: {snippet}). Check the username, the password and the AppKey, and that the account is enabled for API access"),
            ));
        }
        let a: Answer = serde_json::from_slice(body).map_err(|e| ConnectorError::new(ErrorKind::Auth, format!("FOREX.com sign-in answer unreadable: {e}")))?;
        let s = a.session.filter(|s| !s.is_empty()).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "FOREX.com returned no session token for this login"))?;
        keep_token(&self.key, s, now, SESSION_TTL_SECS);
        Ok(())
    }

    fn forget_token(&self) {
        drop_token(&self.key);
    }
}

impl ForexCom {
    fn decode_bars(rows: Vec<BarRow>, req: &CandleReq) -> Result<Vec<(Timestamp, Ohlc)>, ConnectorError> {
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let ts = wcf(&r.bar_date).ok_or_else(|| ConnectorError::decode("FOREX.com bar carries no readable date"))?;
            if !req.range.contains(ts) {
                continue;
            }
            let p = |d: OptDec| Price(d.0.unwrap_or_default());
            out.push((ts, Ohlc { open: p(r.open), high: p(r.high), low: p(r.low), close: p(r.close) }));
        }
        out.sort_by_key(|(t, _)| *t);
        Ok(out)
    }

    fn chunk(req: &CandleReq, mid: Vec<(Timestamp, Ohlc)>, bid: Vec<(Timestamp, Ohlc)>, ask: Vec<(Timestamp, Ohlc)>) -> Vec<Candle> {
        let bids: HashMap<Timestamp, Ohlc> = bid.into_iter().collect();
        let asks: HashMap<Timestamp, Ohlc> = ask.into_iter().collect();
        mid.into_iter()
            .map(|(ts, m)| Candle {
                instrument: req.instrument.clone(),
                tf: req.tf,
                open_ts: ts,
                open: m.open,
                high: m.high,
                low: m.low,
                close: m.close,
                // A dealing desk publishes prices, not size.
                volume: Qty(Decimal::ZERO),
                quote_volume: None,
                trades: None,
                // `PriceBars` are closed; the forming one is `PartialPriceBar`, not read.
                is_final: true,
                adjusted: None,
                sides: match (bids.get(&ts), asks.get(&ts)) {
                    (Some(b), Some(a)) => Some(Sides { bid: *b, ask: *a }),
                    _ => None,
                },
            })
            .collect()
    }
}

impl Mapper for ForexCom {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// The market's name as StoneX writes it (`EUR/USD`, `UK 100`), or its numeric id.
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_ascii_uppercase();
        if s.is_empty() || s.chars().any(char::is_control) {
            return Err(ConnectorError::not_found(format!("FOREX.com: bad symbol `{symbol}`, name the market (EUR/USD) or its market id")));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    fn resolve_batch(&self) -> usize {
        FIND.len()
    }

    fn account_setting(&self) -> Option<&'static str> {
        Some("trading_account_id")
    }

    fn probe_symbol(&self) -> Option<&'static str> {
        Some("EUR/USD")
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: otw_connector_model::DataType::Candles { tf }, range, .. } => {
                let req = CandleReq { instrument: instrument.clone(), tf: *tf, range: *range };
                // A number is the market id itself; a name is looked up first.
                if instrument.native.bytes().all(|b| b.is_ascii_digit()) {
                    let h = bars_request(&req, &instrument.native, 0, *req_id)?;
                    let market = instrument.native.clone();
                    return Ok(vec![s.put(*req_id, h, Pending::Bars { req, market, side: 0, mid: vec![], bid: vec![] })]);
                }
                bar_interval(*tf).ok_or_else(|| ConnectorError::unsupported(format!("FOREX.com serves no {tf} bar here")))?;
                Ok(vec![tree_request(s, *req_id, Then::Find(req))])
            }
            Command::SearchInstruments { req_id, query, .. } => {
                if query.trim().is_empty() {
                    return Ok(vec![]);
                }
                Ok(vec![tree_request(s, *req_id, Then::Search(query.clone()))])
            }
            Command::ResolveInstruments { req_id, ids } => {
                if ids.len() > FIND.len() {
                    return Err(ConnectorError::invalid(format!("FOREX.com resolves {} markets per lookup", FIND.len())));
                }
                Ok(vec![tree_request(s, *req_id, Then::Resolve(ids.clone()))])
            }
            Command::QueryAccount { .. } => account::encode(cmd, s),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("FOREX.com: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body.clone()),
            RawFrame::Timeout { corr_id, .. } => {
                self.abandon(*corr_id, s);
                return Err(http::timeout(frame, "forexcom").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = refusal(frame) {
            self.abandon(corr, s);
            return Err(e);
        }
        match pending {
            Pending::Tree(then) => {
                let tree: Tree = serde_json::from_slice(&body).map_err(|e| decode_err("account tree", e))?;
                let client = int_text(&tree.client).ok_or_else(|| ConnectorError::decode("FOREX.com answered no client account"))?;
                match then {
                    Then::Find(req) => {
                        let h = search_request("find", &client, &req.instrument.native, LOOKUP, corr);
                        let r = s.put(corr, h, Pending::Find(req));
                        s.then(corr, r);
                    }
                    Then::Search(q) => {
                        let r = s.put(corr, search_request("search", &client, &q, SEARCH, corr), Pending::Search);
                        s.then(corr, r);
                    }
                    Then::Resolve(ids) => {
                        for (id, ep) in ids.into_iter().zip(FIND) {
                            let h = search_request(ep, &client, &id.native, LOOKUP, corr);
                            let r = s.put(corr, h, Pending::Resolve(id));
                            s.then(corr, r);
                        }
                    }
                    Then::Account(t) => return account::after_tree(t, tree, corr, s, ctx),
                }
                Ok(vec![])
            }
            Pending::Find(req) => {
                let m: Markets = serde_json::from_slice(&body).map_err(|e| decode_err("market search", e))?;
                let wanted = req.instrument.native.clone();
                let hits: Vec<(String, String)> = m.markets.into_iter().filter_map(|r| Some((int_text(&r.id)?, r.name?))).collect();
                let exact: Vec<&(String, String)> = hits.iter().filter(|(_, n)| n.eq_ignore_ascii_case(&wanted)).collect();
                let market = match exact.as_slice() {
                    [(id, _)] => id.clone(),
                    [] => {
                        return Err(ConnectorError::not_found(format!(
                            "FOREX.com lists no market called \"{wanted}\" (bad symbol). Its search answered [{}]",
                            hits.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>().join(", ")
                        )))
                    }
                    many => {
                        return Err(ConnectorError::invalid(format!(
                            "\"{wanted}\" matches {} markets on this account. Download by market id instead: [{}]",
                            many.len(),
                            many.iter().map(|(i, n)| format!("{n} = {i}")).collect::<Vec<_>>().join(", ")
                        )))
                    }
                };
                let h = bars_request(&req, &market, 0, corr)?;
                let r = s.put(corr, h, Pending::Bars { req, market, side: 0, mid: vec![], bid: vec![] });
                s.then(corr, r);
                Ok(vec![])
            }
            Pending::Bars { req, market, side, mid, bid } => {
                let a: BarsAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("bars", e))?;
                let rows = a.bars.ok_or_else(|| ConnectorError::decode(format!("FOREX.com answered no bar list for {}", req.instrument.native)))?;
                let read = Self::decode_bars(rows, &req)?;
                match side {
                    0 | 1 => {
                        let (mid, bid) = if side == 0 { (read, bid) } else { (mid, read) };
                        let h = bars_request(&req, &market, side + 1, corr)?;
                        let r = s.put(corr, h, Pending::Bars { req, market, side: side + 1, mid, bid });
                        s.then(corr, r);
                        Ok(vec![])
                    }
                    _ => {
                        let candles = Self::chunk(&req, mid, bid, read);
                        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })])
                    }
                }
            }
            Pending::Search | Pending::Resolve(_) => {
                let exact = match &pending {
                    Pending::Resolve(id) => Some(id.native.clone()),
                    _ => None,
                };
                let m: Markets = serde_json::from_slice(&body).map_err(|e| decode_err("market search", e))?;
                let mut out = Vec::new();
                for r in m.markets {
                    let (Some(id), Some(name)) = (int_text(&r.id), r.name) else { continue };
                    if exact.as_ref().is_some_and(|w| !w.eq_ignore_ascii_case(&name)) {
                        continue;
                    }
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &name.to_ascii_uppercase()), &name, "");
                    spec.base = None;
                    // The id is what the API takes, so it belongs on the line: a name that
                    // matches two markets is typed as its number instead.
                    spec.symbol = CanonicalSymbol(format!("{name} (market {id})"));
                    // Every FOREX.com market downloads as a pair priced in its quote, CFDs
                    // included: a search line offers it as such. Account rows carry their
                    // own class.
                    spec.asset_class = AssetClass::Forex;
                    spec.exchange = Some("FOREX.com".into());
                    spec.note = Some(id);
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
                if exact.is_none() {
                    out.push(env(ctx, corr, Event::InstrumentsEnd));
                }
                Ok(out)
            }
            Pending::Account(p) => account::decode(p, corr, &body, s, ctx),
        }
    }

    fn follow_ups(&self, corr_id: CorrId, s: &mut Session) -> Vec<NativeRequest> {
        s.next.remove(&corr_id).unwrap_or_default()
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
        s.next.remove(&corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |k: &str| secrets.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let need = |k: &str| get(k).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("FOREX.com requires a {k} credential: set it on the settings page")));
        let (user, password, app_key) = (need("username")?, need("password")?, need("app_key")?);
        Ok(Some(Arc::new(ForexComSigner {
            key: credential_key(&["forexcom", user, app_key]),
            user: user.to_string(),
            password: SecretString::from(password.to_string()),
            app_key: SecretString::from(app_key.to_string()),
        })))
    }

    fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        match settings.get("trading_account_id").map(|s| s.trim()) {
            None | Some("") => Ok(()),
            Some(id) if id.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
            Some(other) => Err(ConnectorError::invalid(format!("a StoneX trading account id is a number, not \"{other}\""))),
        }
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.exchange.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
