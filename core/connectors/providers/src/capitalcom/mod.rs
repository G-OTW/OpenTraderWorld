//! Capital.com Open API: mid candles with the bid and ask per side, market search, the best
//! price, the account read side (activity, positions, working orders), and live candles.
//!
//! A session is opened with the API key, the login and the key's custom password, and hands
//! back two tokens in response headers (`CST`, `X-SECURITY-TOKEN`) that every call carries.
//! They live ten minutes from their last use. A login with several financial accounts starts
//! on the preferred one; the `account_id` setting switches to another as a second sign-in
//! step. The socket authenticates every frame with the same two tokens, so the signer
//! completes each one (`Signer::sign_frame`). Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event,
    HistoryItems, InstrumentId, InstrumentSpec, Ohlc, Price, Qty, Quote, Sides, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::signer::{cached_token, credential_key, drop_token, keep_token, SignableRequest, TokenBody, TokenRequest};
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, Signer};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

pub use stream::LiveState;

pub const VENUE: &str = "CAPITALCOM";
const REST: &str = "rest";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[(REST, "https://api-capital.backend-capital.com"), (WS, "wss://api-streaming-capital.backend-capital.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// The tokens die ten minutes after their last use.
const SESSION_TTL_SECS: i64 = 10 * 60;
/// Bars one price request returns at most.
const MAX_BARS: u32 = 1000;
/// Epics one market details call takes at most.
const EPICS: usize = 50;

#[derive(Clone)]
pub struct CapitalCom {
    caps: Capabilities,
}

impl CapitalCom {
    pub fn new() -> Result<Self, CapsError> {
        Ok(CapitalCom { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// The journal class of an instrument type. Every market is a CFD: a currency pair is still a
/// currency pair, a share CFD is not the share and a crypto CFD is not the coin.
pub(crate) fn class_of(instrument_type: &str) -> AssetClass {
    if instrument_type.eq_ignore_ascii_case("CURRENCIES") {
        AssetClass::Forex
    } else {
        AssetClass::Cfd
    }
}

/// What a search line offers the instrument as, in the download form's vocabulary.
fn search_class(instrument_type: &str) -> AssetClass {
    match instrument_type.to_ascii_uppercase().as_str() {
        "CURRENCIES" => AssetClass::Forex,
        "INDICES" => AssetClass::Index,
        "CRYPTOCURRENCIES" => AssetClass::Spot,
        _ => AssetClass::Equity,
    }
}

/// A `…UTC` field: an ISO date and time with no zone on it, which the docs say is UTC. A
/// stamp that does carry a zone is read as it stands.
pub(crate) fn parse_utc(raw: &str) -> Option<Timestamp> {
    use time::{Date, Month, PrimitiveDateTime, Time};
    let s = raw.trim();
    if let Ok(t) = otw_connector_sdk::parse::ts_rfc3339(s) {
        return Some(t);
    }
    let (date, rest) = s.split_once('T')?;
    let (hms, frac) = rest.split_once('.').unwrap_or((rest, "0"));
    let mut d = date.split('-');
    let (y, m, day) = (d.next()?, d.next()?, d.next()?);
    let mut t = hms.split(':');
    let (h, mi, sec) = (t.next()?, t.next()?, t.next().unwrap_or("0"));
    let date = Date::from_calendar_date(y.parse().ok()?, Month::try_from(m.parse::<u8>().ok()?).ok()?, day.parse().ok()?).ok()?;
    // Milliseconds, padded or truncated to three digits.
    let ms: u16 = format!("{frac:0<3}").get(..3)?.parse().ok()?;
    let clock = Time::from_hms_milli(h.parse().ok()?, mi.parse().ok()?, sec.parse().ok()?, ms).ok()?;
    Timestamp::from_offset_datetime(PrimitiveDateTime::new(date, clock).assume_utc())
}

/// The date the query parameters take: `YYYY-MM-DDTHH:MM:SS`, no zone, in UTC (the filter
/// runs on the UTC fields).
pub(crate) fn query_stamp(t: Timestamp) -> Result<String, ConnectorError> {
    let t = t.to_offset_datetime().ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))?;
    Ok(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute(), t.second()))
}

pub(crate) fn resolution(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "MINUTE",
        (5, TfUnit::Minute) => "MINUTE_5",
        (15, TfUnit::Minute) => "MINUTE_15",
        (1, TfUnit::Hour) => "HOUR",
        (4, TfUnit::Hour) => "HOUR_4",
        (1, TfUnit::Day) => "DAY",
        (1, TfUnit::Week) => "WEEK",
        _ => return None,
    })
}

pub(crate) fn timeframe_of(resolution: &str) -> Option<Timeframe> {
    Some(match resolution {
        "MINUTE" => Timeframe::M1,
        "MINUTE_5" => Timeframe::M5,
        "MINUTE_15" => Timeframe::M15,
        "HOUR" => Timeframe::H1,
        "HOUR_4" => Timeframe::H4,
        "DAY" => Timeframe::D1,
        "WEEK" => Timeframe::W1,
        _ => return None,
    })
}

pub(crate) enum Pending {
    Prices { instrument: InstrumentId, tf: Timeframe, range: TimeRange },
    Snapshot(InstrumentId),
    Search,
    Resolve,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    next: HashMap<CorrId, Vec<NativeRequest>>,
    live: LiveState,
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

/// One side of a candle price: `{"bid": …, "ask": …}`.
#[derive(Deserialize, Default, Clone, Copy)]
struct Two {
    #[serde(default)]
    bid: OptDec,
    #[serde(default)]
    ask: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PriceRow {
    #[serde(rename = "snapshotTimeUTC", default)]
    snapshot_time_utc: String,
    #[serde(default)]
    open_price: Option<Two>,
    #[serde(default)]
    high_price: Option<Two>,
    #[serde(default)]
    low_price: Option<Two>,
    #[serde(default)]
    close_price: Option<Two>,
    #[serde(default)]
    last_traded_volume: OptDec,
}

#[derive(Deserialize)]
struct PricesAnswer {
    #[serde(default)]
    prices: Option<Vec<PriceRow>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRow {
    #[serde(default)]
    epic: Option<String>,
    #[serde(default)]
    instrument_name: String,
    #[serde(default)]
    instrument_type: String,
}

#[derive(Deserialize)]
struct SearchAnswer {
    #[serde(default)]
    markets: Vec<SearchRow>,
}

#[derive(Deserialize)]
pub(crate) struct Instrument {
    #[serde(default)]
    pub epic: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub currency: String,
}

#[derive(Deserialize, Default)]
struct Snapshot {
    #[serde(default)]
    bid: OptDec,
    #[serde(default)]
    offer: OptDec,
}

#[derive(Deserialize)]
struct MarketDetails {
    #[serde(default)]
    instrument: Option<Instrument>,
    #[serde(default)]
    snapshot: Option<Snapshot>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailsAnswer {
    #[serde(default)]
    market_details: Vec<MarketDetails>,
}

pub(crate) fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

pub(crate) fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("capitalcom {what}: {e}"))
}

fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| v.is_sign_positive() && !v.is_zero())
}

/// The venue's error code (`error.not-found.epic`), when the body carries one.
fn error_code(body: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct E {
        #[serde(rename = "errorCode", default)]
        code: Option<String>,
    }
    serde_json::from_slice::<E>(body).ok()?.code
}

/// The fault a status means. A 401 is a session timed out or replaced: the signer drops it and
/// the next call signs in again.
pub(crate) fn refusal(frame: &RawFrame) -> Option<ConnectorError> {
    let mut e = http::timeout(frame, "capitalcom").or_else(|| http::classify(frame, "capitalcom"))?;
    if e.kind == ErrorKind::Auth {
        e.message = "Capital.com ended this session. It expires after ten minutes without use; retry, and check the key if it keeps happening".into();
    }
    Some(e)
}

/// The mid of a price's two sides; one side alone is still the venue's number.
fn mid(two: Option<Two>) -> Option<Decimal> {
    let t = two?;
    match (positive(t.bid), positive(t.ask)) {
        (Some(b), Some(a)) => Some(((b + a) / Decimal::TWO).normalize()),
        (Some(b), None) => Some(b),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

fn get(endpoint: &'static str, path: impl Into<String>, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, corr).signed()
}

pub(crate) fn details_request(ids: &[InstrumentId], corr: CorrId) -> HttpRequest {
    let epics: Vec<&str> = ids.iter().map(|i| i.native.as_str()).collect();
    get("details", "/api/v1/markets", corr).query("epics", epics.join(","))
}

/// A market's details as the spec the bridge reads its currency and name from. One unit of
/// size is one unit: a position carries its own contract size.
pub(crate) fn spec_of(i: &Instrument) -> InstrumentSpec {
    let epic = i.epic.to_ascii_uppercase();
    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &epic), &epic, &i.currency.to_ascii_uppercase());
    spec.base = None;
    spec.symbol = CanonicalSymbol(if i.name.is_empty() { epic.clone() } else { i.name.clone() });
    spec.asset_class = class_of(&i.kind);
    spec.contract = otw_connector_model::ContractType::Cfd;
    spec.exchange = Some("Capital.com".into());
    spec
}

/// The session for a login: its two tokens, opened once and shared by every call on it, and
/// moved to the financial account the `account_id` setting names.
pub struct CapitalComSigner {
    api_key: SecretString,
    identifier: String,
    password: SecretString,
    account: Option<String>,
    /// The session in force, on the account asked for.
    key: String,
    /// The sign-in before the switch to `account`.
    login_key: String,
}

impl CapitalComSigner {
    fn tokens(&self, key: &str, now: Timestamp) -> Option<(String, String)> {
        let t = cached_token(key, now)?;
        let (cst, security) = t.expose_secret().split_once('\n')?;
        Some((cst.to_string(), security.to_string()))
    }

    /// The session in force, its life extended: the venue counts ten minutes from last use.
    fn in_force(&self, now: Timestamp) -> Result<(String, String), ConnectorError> {
        let (cst, security) = self.tokens(&self.key, now).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "no Capital.com session: the sign-in did not answer"))?;
        keep_token(&self.key, format!("{cst}\n{security}"), now, SESSION_TTL_SECS);
        Ok((cst, security))
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
}

fn snippet(body: &[u8]) -> String {
    String::from_utf8_lossy(body).chars().take(200).collect()
}

impl Signer for CapitalComSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let (cst, security) = self.in_force(now)?;
        req.headers.push(("X-CAP-API-KEY".into(), self.api_key.expose_secret().to_string()));
        req.headers.push(("CST".into(), cst));
        req.headers.push(("X-SECURITY-TOKEN".into(), security));
        Ok(())
    }

    /// The two tokens on the frame itself: the socket has no handshake.
    fn sign_frame(&self, frame: bytes::Bytes, now: Timestamp) -> Result<bytes::Bytes, ConnectorError> {
        let (cst, security) = self.in_force(now)?;
        let mut v: serde_json::Value = serde_json::from_slice(&frame).map_err(|e| ConnectorError::invalid(format!("capitalcom frame: {e}")))?;
        let o = v.as_object_mut().ok_or_else(|| ConnectorError::invalid("capitalcom frame is not an object"))?;
        o.insert("cst".into(), cst.into());
        o.insert("securityToken".into(), security.into());
        Ok(v.to_string().into())
    }

    fn token_request(&self, now: Timestamp) -> Option<TokenRequest> {
        if self.tokens(&self.key, now).is_some() {
            return None;
        }
        let api_key = ("X-CAP-API-KEY".to_string(), self.api_key.expose_secret().to_string());
        if let (Some(account), Some((cst, security))) = (&self.account, self.tokens(&self.login_key, now)) {
            // The session starts on the preferred account; the one asked for may be another.
            let mut t = TokenRequest::post(REST, "/api/v1/session", TokenBody::Json(serde_json::json!({ "accountId": account }).to_string()));
            t.method = Method::Put;
            t.headers = vec![api_key, ("CST".into(), cst), ("X-SECURITY-TOKEN".into(), security)];
            return Some(t);
        }
        let body = serde_json::json!({ "identifier": self.identifier, "password": self.password.expose_secret(), "encryptedPassword": false });
        let mut t = TokenRequest::post(REST, "/api/v1/session", TokenBody::Json(body.to_string()));
        t.headers = vec![api_key];
        Some(t)
    }

    fn accept_token(&self, status: u16, headers: &[(String, String)], body: &[u8], now: Timestamp) -> Result<(), ConnectorError> {
        let switching = self.account.is_some() && self.tokens(&self.login_key, now).is_some();
        if switching {
            let (cst, security) = self.tokens(&self.login_key, now).unwrap_or_default();
            let account = self.account.clone().unwrap_or_default();
            if !(200..300).contains(&status) {
                // Already on it: the session is the one asked for.
                if error_code(body).is_some_and(|c| c.contains("not-different")) {
                    keep_token(&self.key, format!("{cst}\n{security}"), now, SESSION_TTL_SECS);
                    return Ok(());
                }
                drop_token(&self.login_key);
                return Err(ConnectorError::invalid(format!(
                    "Capital.com would not switch to account {account} ({status}: {}). Check the Account ID setting against the list on the platform",
                    snippet(body)
                )));
            }
            // The switch answers a fresh security token for the account it moved to.
            let security = header(headers, "x-security-token").map_or(security, str::to_string);
            keep_token(&self.key, format!("{cst}\n{security}"), now, SESSION_TTL_SECS);
            return Ok(());
        }
        if !(200..300).contains(&status) {
            return Err(ConnectorError::new(
                ErrorKind::Auth,
                format!(
                    "Capital.com refused the sign-in ({status}: {}). Check the API key, the login and the custom password set on the key, and that two-factor authentication is on",
                    snippet(body)
                ),
            ));
        }
        let (Some(cst), Some(security)) = (header(headers, "cst"), header(headers, "x-security-token")) else {
            return Err(ConnectorError::new(ErrorKind::Auth, "Capital.com accepted the sign-in but returned no session tokens"));
        };
        let key = if self.account.is_some() { &self.login_key } else { &self.key };
        keep_token(key, format!("{cst}\n{security}"), now, SESSION_TTL_SECS);
        Ok(())
    }

    fn forget_token(&self) {
        drop_token(&self.key);
        drop_token(&self.login_key);
    }
}

impl CapitalCom {
    fn candles(rows: Vec<PriceRow>, instrument: &InstrumentId, tf: Timeframe, range: TimeRange) -> Result<Vec<Candle>, ConnectorError> {
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let ts = parse_utc(&r.snapshot_time_utc).ok_or_else(|| ConnectorError::decode("Capital.com price carries no readable time"))?;
            if !range.contains(ts) {
                continue;
            }
            let side = |name: &str, two: Option<Two>| mid(two).ok_or_else(|| ConnectorError::decode(format!("Capital.com price {name} carries neither side")));
            let (open, high, low, close) = (side("openPrice", r.open_price)?, side("highPrice", r.high_price)?, side("lowPrice", r.low_price)?, side("closePrice", r.close_price)?);
            // Both sides, when all eight prices are there; a candle missing one keeps its mid.
            let px = |two: Option<Two>| two.and_then(|t| Some((positive(t.bid)?, positive(t.ask)?)));
            let sides = match (px(r.open_price), px(r.high_price), px(r.low_price), px(r.close_price)) {
                (Some(o), Some(h), Some(l), Some(c)) => Some(Sides {
                    bid: Ohlc { open: Price(o.0), high: Price(h.0), low: Price(l.0), close: Price(c.0) },
                    ask: Ohlc { open: Price(o.1), high: Price(h.1), low: Price(l.1), close: Price(c.1) },
                }),
                _ => None,
            };
            out.push(Candle {
                instrument: instrument.clone(),
                tf,
                open_ts: ts,
                open: Price(open),
                high: Price(high),
                low: Price(low),
                close: Price(close),
                volume: Qty(r.last_traded_volume.0.unwrap_or_default()),
                quote_volume: None,
                trades: None,
                // The last one is forming and nothing says so: its period's end decides.
                is_final: false,
                adjusted: None,
                sides,
            });
        }
        out.sort_by_key(|c| c.open_ts);
        Ok(out)
    }
}

impl Mapper for CapitalCom {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// An epic, Capital.com's own name for a market (`GOLD`, `EURUSD`, `US500`).
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_ascii_uppercase();
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) {
            return Err(ConnectorError::not_found(format!("Capital.com: bad symbol `{symbol}`, name the epic (GOLD, EURUSD, US500)")));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    fn resolve_batch(&self) -> usize {
        EPICS
    }

    fn account_setting(&self) -> Option<&'static str> {
        Some("account_id")
    }

    fn probe_symbol(&self) -> Option<&'static str> {
        Some("EURUSD")
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => {
                let res = resolution(*tf).ok_or_else(|| ConnectorError::unsupported(format!("Capital.com serves no {tf} candle")))?;
                let h = get("prices", format!("/api/v1/prices/{}", instrument.native), *req_id)
                    .query("resolution", res)
                    .query("from", query_stamp(range.from)?)
                    .query("to", query_stamp(range.to)?)
                    .query("max", MAX_BARS);
                Ok(vec![s.put(*req_id, h, Pending::Prices { instrument: instrument.clone(), tf: *tf, range: *range })])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                let h = get("market", format!("/api/v1/markets/{}", instrument.native), *req_id);
                Ok(vec![s.put(*req_id, h, Pending::Snapshot(instrument.clone()))])
            }
            Command::SearchInstruments { req_id, query, .. } => {
                if query.trim().is_empty() {
                    return Ok(vec![]);
                }
                Ok(vec![s.put(*req_id, get("search", "/api/v1/markets", *req_id).query("searchTerm", query.trim()), Pending::Search)])
            }
            Command::ResolveInstruments { req_id, ids } => {
                if ids.len() > EPICS {
                    return Err(ConnectorError::invalid(format!("Capital.com details {EPICS} epics per call")));
                }
                Ok(vec![s.put(*req_id, details_request(ids, *req_id), Pending::Resolve)])
            }
            Command::QueryAccount { .. } => account::encode(cmd, s),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("Capital.com: {other:?} is not declared"))),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !cmds.is_empty() && cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
            return stream::encode(cmds, &mut s.live, ctx);
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
            RawFrame::Timeout { corr_id, .. } => {
                self.abandon(*corr_id, s);
                return Err(http::timeout(frame, "capitalcom").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        let status = match frame {
            RawFrame::Http { status, .. } => *status,
            _ => 0,
        };
        if let Some(mut e) = refusal(frame) {
            let code = error_code(&body).unwrap_or_default();
            match &pending {
                // A window with no prices in it (a weekend, before the listing) is an empty
                // chunk, not a fault.
                Pending::Prices { .. } if status == 404 && code.contains("prices.not-found") => {
                    return Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(vec![]), next_cursor: None, done: true })]);
                }
                // A market Capital.com does not list has no price; the caller says so.
                Pending::Snapshot(_) if status == 404 => return Ok(vec![]),
                Pending::Prices { instrument, .. } if status == 404 || code.contains("epic") => {
                    e = ConnectorError::not_found(format!("Capital.com lists no epic `{}` (bad symbol). Search for it to find its epic", instrument.native));
                }
                _ => {}
            }
            self.abandon(corr, s);
            return Err(e);
        }
        match pending {
            Pending::Prices { instrument, tf, range } => {
                let a: PricesAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("prices", e))?;
                let rows = a.prices.ok_or_else(|| ConnectorError::decode(format!("Capital.com answered no price list for {}", instrument.native)))?;
                let candles = Self::candles(rows, &instrument, tf, range)?;
                Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })])
            }
            Pending::Snapshot(instrument) => {
                let m: MarketDetails = serde_json::from_slice(&body).map_err(|e| decode_err("market", e))?;
                let snap = m.snapshot.unwrap_or_default();
                let (Some(bid), Some(ask)) = (positive(snap.bid), positive(snap.offer)) else { return Ok(vec![]) };
                Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: None, ask_qty: None }))])
            }
            Pending::Search => {
                let a: SearchAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("market search", e))?;
                let mut out = Vec::new();
                for r in a.markets {
                    let Some(epic) = r.epic.filter(|e| !e.is_empty()) else { continue };
                    let epic = epic.to_ascii_uppercase();
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &epic), &epic, "");
                    spec.base = None;
                    spec.symbol = CanonicalSymbol(r.instrument_name);
                    spec.asset_class = search_class(&r.instrument_type);
                    spec.contract = otw_connector_model::ContractType::Cfd;
                    // Every one is a CFD, and the line says which kind.
                    spec.exchange = Some(format!("Capital.com {}", r.instrument_type));
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
                out.push(env(ctx, corr, Event::InstrumentsEnd));
                Ok(out)
            }
            Pending::Resolve => {
                let a: DetailsAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("market details", e))?;
                Ok(a.market_details
                    .into_iter()
                    .filter_map(|m| m.instrument)
                    .filter(|i| !i.epic.is_empty())
                    .map(|i| env(ctx, corr, Event::Instrument { spec: Box::new(spec_of(&i)) }))
                    .collect())
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

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |k: &str| secrets.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let need = |k: &str| get(k).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("Capital.com requires a {k} credential: set it on the settings page")));
        let (api_key, identifier, password) = (need("api_key")?, need("identifier")?, need("api_password")?);
        let env = get("env").unwrap_or("live").to_ascii_lowercase();
        let account = get("account_id");
        Ok(Some(Arc::new(CapitalComSigner {
            key: credential_key(&["capitalcom", &env, api_key, identifier, account.unwrap_or_default()]),
            login_key: credential_key(&["capitalcom-login", &env, api_key, identifier]),
            api_key: SecretString::from(api_key.to_string()),
            identifier: identifier.to_string(),
            password: SecretString::from(password.to_string()),
            account: account.map(str::to_string),
        })))
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.exchange.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
