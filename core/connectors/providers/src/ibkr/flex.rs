//! Interactive Brokers' account history over the **Flex Web Service**: the fills and open
//! positions of an Activity Flex Query the user saved in Account Management.
//!
//! Two calls turn a query into an XML statement: `SendRequest` asks for it and answers a
//! reference code, `GetStatement` collects it once IBKR has built it. While it is still being
//! built (code 1019) the collection is asked again, at most twelve times, paced five seconds
//! apart by the declared `statement` rate limit (the engine's token bucket, not a sleep here).
//!
//! **The period belongs to the query**, not to the caller: a Flex query carries its own date
//! range and the API takes no dates, so the period asked for filters what the statement
//! returned. The statement stamps its times in the query's own zone and never says which;
//! the account's `tz_offset` setting says it, and rides with the query id in
//! `QueryAccount.account` ([`Mapper::account_selector`]).
//!
//! Every row describes itself (class, exchange, currency, point value), so no instrument is
//! looked up (`account.rows_carry_terms`).

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, AssetClass, Capabilities, CapsError, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind,
    Event, Fill, InstrumentId, Mapped, Money, Position, Price, Qty, Side, TimeRange, Timestamp,
};
use otw_connector_sdk::parse::dec;
use otw_connector_sdk::signer::ApiKeyQuerySigner;
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use time::{Date, Month, PrimitiveDateTime, Time, UtcOffset};

pub const VENUE: &str = "IBKR";
const FLEX: &str = "flex";
const BASES: &[(&str, &str)] = &[(FLEX, "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService")];
const CAPS_TOML: &str = include_str!("flex.toml");
/// How many times a statement still being built is collected again.
const POLL_TRIES: u32 = 12;
/// The pace of those collections, as the `statement` rate limit declares it.
const POLL_DELAY_SECS: u32 = 5;
/// Between the query id and the zone offset in the account selector.
const SEP: char = '|';

/// One element's attributes. A Flex statement is a flat list of attribute-carrying elements.
type Attrs = HashMap<String, String>;

#[derive(Clone)]
pub struct IbkrFlex {
    caps: Capabilities,
}

impl IbkrFlex {
    pub fn new() -> Result<Self, CapsError> {
        Ok(IbkrFlex { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

struct Job {
    kind: AccountKind,
    range: Option<TimeRange>,
    /// The instruments asked for, in capitals; empty = all.
    symbols: Vec<String>,
    offset: UtcOffset,
    reference: String,
    attempt: u32,
}

#[derive(Default)]
pub struct Session {
    jobs: HashMap<CorrId, Job>,
    next: HashMap<CorrId, Vec<NativeRequest>>,
}

// ── XML reading ──────────────────────────────────────────────────────────────

/// Every element named `name`, as its attribute map.
pub(crate) fn elements(xml: &str, name: &str) -> Vec<Attrs> {
    use quick_xml::events::Event as X;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(X::Start(e)) | Ok(X::Empty(e)) => {
                if e.name().as_ref() == name {
                    let mut map = HashMap::new();
                    for a in e.attributes().flatten() {
                        let key = a.key.as_ref().to_string();
                        let val = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.to_string()).unwrap_or_default();
                        map.insert(key, val);
                    }
                    out.push(map);
                }
            }
            Ok(X::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Text content of the first `<name>` element.
fn text_of(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml.get(start..)?.find(&close)? + start;
    Some(xml.get(start..end)?.trim().to_string())
}

/// `<Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>…` as its code and one line.
pub(crate) fn flex_error(xml: &str) -> Option<(String, String)> {
    let status = text_of(xml, "Status")?;
    if status.eq_ignore_ascii_case("Success") {
        return None;
    }
    let code = text_of(xml, "ErrorCode").unwrap_or_default();
    let msg = text_of(xml, "ErrorMessage").unwrap_or_else(|| status.clone());
    Some((code.clone(), format!("{code} {msg}").trim().to_string()))
}

/// What a Flex error code means for the caller.
pub(crate) fn code_kind(code: &str) -> ErrorKind {
    match code {
        // Not ready, under load, or still being built: try again shortly.
        "1001" | "1004" | "1005" | "1006" | "1007" | "1008" | "1009" | "1019" | "1021" => ErrorKind::Transient,
        "1018" => ErrorKind::RateLimited,
        // Token expired, invalid, IP restriction, inactive service account, invalid account.
        "1011" | "1012" | "1013" | "1015" | "1016" => ErrorKind::Auth,
        // A legacy query, an invalid query, reference code or request.
        "1010" | "1014" | "1017" | "1020" => ErrorKind::InvalidRequest,
        "1003" => ErrorKind::NotFound,
        other => {
            tracing::warn!(code = other, "ibkr flex: unknown error code");
            ErrorKind::Unknown
        }
    }
}

/// A Flex query can report the same fill at several levels of detail. Executions win; an
/// order-level row is used only when there is no execution-level one, and a closed-lot row
/// never is: counting both would double every trade.
pub(crate) fn execution_level(rows: Vec<Attrs>) -> Vec<Attrs> {
    let level = |r: &Attrs| get(r, "levelOfDetail").to_ascii_uppercase();
    let has_exec = rows.iter().any(|r| level(r) == "EXECUTION");
    rows.into_iter()
        .filter(|r| {
            let l = level(r);
            if l == "CLOSED_LOT" || l == "LOT" {
                return false;
            }
            !has_exec || l == "EXECUTION"
        })
        .collect()
}

// ── Field reading ────────────────────────────────────────────────────────────

fn get(row: &Attrs, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default()
}

fn first_of(row: &Attrs, keys: &[&str]) -> String {
    keys.iter().find_map(|k| row.get(*k).map(|v| v.trim()).filter(|v| !v.is_empty())).unwrap_or_default().to_string()
}

/// A number as the statement writes it, thousands separators and all; blank is zero.
fn num(row: &Attrs, key: &str) -> Decimal {
    row.get(key).map(|v| v.replace(',', "")).and_then(|v| dec(&v).ok()).unwrap_or(Decimal::ZERO)
}

/// IBKR's asset categories. A category nobody mapped is other, never a guessed stock.
pub(crate) fn class_of(category: &str) -> AssetClass {
    match category.trim().to_ascii_uppercase().as_str() {
        "STK" => AssetClass::Equity,
        "ETF" | "FUND" => AssetClass::Etf,
        "OPT" | "FOP" | "WAR" => AssetClass::Option,
        "FUT" => AssetClass::Future,
        "CFD" => AssetClass::Cfd,
        "CASH" => AssetClass::Forex,
        "CRYPTO" => AssetClass::Spot,
        "BOND" | "BILL" => AssetClass::Bond,
        "CMDTY" => AssetClass::Commodity,
        other => {
            tracing::warn!(category = other, "ibkr flex: unknown asset category, filed as other");
            AssetClass::Other
        }
    }
}

/// Which way a trade row went. The sign leads, `buySell` follows: a cancellation is written
/// `BUY (Ca.)` with a negative quantity, and reading the word first would double the position
/// instead of folding it back. A query with unsigned quantities has only the word.
pub(crate) fn side_of(row: &Attrs) -> Mapped<Side> {
    if num(row, "quantity") < Decimal::ZERO {
        return Mapped::Known(Side::Sell);
    }
    let word = get(row, "buySell").trim().to_ascii_uppercase();
    if word.starts_with("SELL") {
        Mapped::Known(Side::Sell)
    } else if word.is_empty() || word.starts_with("BUY") {
        Mapped::Known(Side::Buy)
    } else {
        tracing::warn!(buy_sell = %word, "ibkr flex: unknown trade direction");
        Mapped::Unknown(word)
    }
}

/// The instrument as the statement names it. Options are the one rewrite: IBKR pads the OSI
/// symbol with spaces (`SPY   251219C00650000`), and without them it is the OCC form the rest
/// of the app reads. Everything else is kept verbatim, a future's local symbol included.
pub(crate) fn symbol_of(row: &Attrs) -> String {
    let raw = first_of(row, &["symbol", "underlyingSymbol", "description"]);
    match get(row, "assetCategory").to_ascii_uppercase().as_str() {
        "OPT" | "FOP" => raw.split_whitespace().collect::<String>(),
        _ => raw.trim().to_string(),
    }
}

/// A Flex time: `20240115;103000`, `20240115;10:30:00`, `2024-01-15;10:30:00`, or a bare
/// date, in the query's zone.
pub(crate) fn stamp(row: &Attrs, offset: UtcOffset) -> Option<Timestamp> {
    let raw = first_of(row, &["dateTime", "tradeDate", "reportDate", "settleDateTarget"]);
    if raw.is_empty() {
        return None;
    }
    let (date_part, time_part) = match raw.split_once([';', ' ', 'T']) {
        Some((d, t)) => (d.trim().to_string(), t.trim().to_string()),
        None => (raw.trim().to_string(), String::new()),
    };
    let digits: String = date_part.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 8 {
        return None;
    }
    let year: i32 = digits.get(0..4)?.parse().ok()?;
    let month = Month::try_from(digits.get(4..6)?.parse::<u8>().ok()?).ok()?;
    let day: u8 = digits.get(6..8)?.parse().ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let t: String = time_part.chars().filter(char::is_ascii_digit).collect();
    let part = |r: std::ops::Range<usize>| t.get(r).and_then(|s| s.parse::<u8>().ok());
    let time = if t.len() >= 6 {
        Time::from_hms(part(0..2)?, part(2..4)?, part(4..6)?).ok()?
    } else if t.len() == 4 {
        Time::from_hms(part(0..2)?, part(2..4)?, 0).ok()?
    } else {
        Time::MIDNIGHT
    };
    Timestamp::from_offset_datetime(PrimitiveDateTime::new(date, time).assume_offset(offset))
}

fn at_least_one(d: Decimal) -> Decimal {
    d.max(Decimal::ONE)
}

// ── Mapping ──────────────────────────────────────────────────────────────────

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn statement_request(reference: &str, corr: CorrId) -> NativeRequest {
    NativeRequest::Http(HttpRequest::get("statement", FLEX, "/GetStatement", corr).query("q", reference).query("v", 3).signed())
}

fn trim(body: &str) -> String {
    let t = body.trim();
    match t.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

impl IbkrFlex {
    fn emit(job: &Job, xml: &str, corr: CorrId, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let chunk = |items| env(ctx, corr, Event::AccountChunk { items, next_cursor: None, done: true });
        match job.kind {
            AccountKind::Summary => {
                let statement = elements(xml, "FlexStatement").into_iter().next().unwrap_or_default();
                let detail = format!(
                    "statement for {} covering {} → {}, {} trade row(s)",
                    get(&statement, "accountId"),
                    get(&statement, "fromDate"),
                    get(&statement, "toDate"),
                    elements(xml, "Trade").len()
                );
                Ok(vec![env(
                    ctx,
                    corr,
                    Event::AccountSummary(AccountSummary {
                        account_type: None,
                        permissions: vec![],
                        assets_held: 0,
                        can_trade: Some(false),
                        open_positions: None,
                        wallet_balance: None,
                        detail: Some(detail),
                    }),
                )])
            }
            AccountKind::Fills => {
                let range = job.range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
                let mut out = Vec::new();
                for r in execution_level(elements(xml, "Trade")) {
                    // A cancellation carries the original trade's id with a reversed quantity;
                    // both are kept, so the position folds back to where it started.
                    let qty = num(&r, "quantity").abs();
                    if qty.is_zero() {
                        continue;
                    }
                    let symbol = symbol_of(&r);
                    if !job.symbols.is_empty() && !job.symbols.contains(&symbol.to_uppercase()) {
                        continue;
                    }
                    let at = stamp(&r, job.offset).ok_or_else(|| {
                        ConnectorError::invalid(format!(
                            "a trade row carries no readable date ({:?}). Set the Flex query's date format to yyyyMMdd and its time format to HHmmss",
                            get(&r, "dateTime")
                        ))
                    })?;
                    if at < range.from || at > range.to {
                        continue;
                    }
                    out.push(Fill {
                        id: first_of(&r, &["tradeID", "transactionID", "ibExecID"]),
                        order_id: Some(first_of(&r, &["ibOrderID", "orderID"])),
                        instrument: InstrumentId::new(VENUE, &symbol),
                        ts: at,
                        side: side_of(&r),
                        qty: Qty(qty),
                        price: Price(num(&r, "tradePrice")),
                        // A commission is a cost whatever sign the query writes it with: a cash
                        // flow in some queries, a plain amount in others, and nothing says which.
                        fee: Money(num(&r, "ibCommission").abs()),
                        fee_asset: Some(first_of(&r, &["ibCommissionCurrency", "currency"])),
                        quote_ccy: Some(get(&r, "currency")),
                        account: Some(get(&r, "accountId")),
                        venue: Some(first_of(&r, &["exchange", "listingExchange"])),
                        class: Some(class_of(&get(&r, "assetCategory"))),
                        multiplier: Some(at_least_one(num(&r, "multiplier"))),
                    });
                }
                Ok(vec![chunk(AccountItems::Fills(out))])
            }
            _ => {
                let rows = elements(xml, "OpenPosition");
                if rows.is_empty() {
                    return Err(ConnectorError::invalid(
                        "this Flex query returns no open positions. Add the Open Positions section to it in Account Management",
                    ));
                }
                let out = rows
                    .iter()
                    .map(|r| Position {
                        instrument: InstrumentId::new(VENUE, &symbol_of(r)),
                        qty: Qty(num(r, "position")),
                        avg_price: Some(num(r, "costBasisPrice")).filter(|p| *p > Decimal::ZERO).map(Price),
                        unrealized_pnl: None,
                        currency: Some(get(r, "currency")),
                        mark_price: None,
                        venue: Some(first_of(r, &["listingExchange", "exchange"])),
                        account: Some(get(r, "accountId")),
                        class: Some(class_of(&get(r, "assetCategory"))),
                        multiplier: Some(at_least_one(num(r, "multiplier"))),
                        name: Some(get(r, "description")),
                    })
                    .collect();
                Ok(vec![chunk(AccountItems::Positions(out))])
            }
        }
    }
}

impl Mapper for IbkrFlex {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// A symbol as the statement writes it (`AAPL`, `MNQU6`, an option without its padding).
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_uppercase();
        if s.is_empty() || s.chars().any(char::is_control) {
            return Err(ConnectorError::not_found(format!("IBKR: bad symbol `{symbol}`")));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    fn account_setting(&self) -> Option<&'static str> {
        Some("query_id")
    }

    /// The query id and the minutes its statement's zone is off UTC.
    fn account_selector(&self, settings: &HashMap<String, String>) -> Option<String> {
        let query = settings.get("query_id").map(|v| v.trim()).filter(|v| !v.is_empty())?;
        let tz = settings.get("tz_offset").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
        Some(format!("{query}{SEP}{tz}"))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::QueryAccount { req_id, kind, range, account, symbols, .. } => {
                if matches!(kind, AccountKind::Balances | AccountKind::OpenOrders) {
                    return Err(ConnectorError::unsupported(format!("an IBKR Flex statement has no {kind:?} section read here")));
                }
                let selector = account.as_deref().unwrap_or_default();
                let (query, tz) = selector.split_once(SEP).unwrap_or((selector, "0"));
                if query.trim().is_empty() {
                    return Err(ConnectorError::invalid("query_id is not set on this broker account"));
                }
                let minutes: i32 = tz.trim().parse().unwrap_or(0);
                s.jobs.insert(
                    *req_id,
                    Job {
                        kind: *kind,
                        range: *range,
                        symbols: symbols.iter().map(|i| i.native.to_uppercase()).collect(),
                        offset: UtcOffset::from_whole_seconds(minutes.saturating_mul(60)).unwrap_or(UtcOffset::UTC),
                        reference: String::new(),
                        attempt: 0,
                    },
                );
                Ok(vec![NativeRequest::Http(HttpRequest::get("send", FLEX, "/SendRequest", *req_id).query("q", query.trim()).query("v", 3).signed())])
            }
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("IBKR Flex: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, status, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, status, body, .. } => (*corr_id, *endpoint_id, *status, body.clone()),
            RawFrame::Timeout { corr_id, .. } => {
                self.abandon(*corr_id, s);
                return Err(http::timeout(frame, "ibkr flex").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let mut job = s.jobs.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        let text = String::from_utf8_lossy(&body).to_string();
        if !(200..300).contains(&status) {
            let kind = http::classify(frame, "ibkr").map(|e| e.kind).unwrap_or(ErrorKind::Unknown);
            return Err(ConnectorError::new(kind, format!("the Flex Web Service answered {status}: {}", trim(&text))));
        }
        match endpoint {
            "send" => {
                if let Some((code, e)) = flex_error(&text) {
                    return Err(ConnectorError::new(code_kind(&code), format!("IBKR refused the Flex request: {e}")).with_code(code));
                }
                job.reference = text_of(&text, "ReferenceCode").ok_or_else(|| ConnectorError::decode("IBKR did not return a reference code for this Flex query"))?;
                job.attempt = 1;
                s.next.entry(corr).or_default().push(statement_request(&job.reference, corr));
                s.jobs.insert(corr, job);
                Ok(vec![])
            }
            "statement" => {
                if text.contains("<FlexQueryResponse") {
                    return Self::emit(&job, &text, corr, ctx);
                }
                match flex_error(&text) {
                    // 1019: the statement is still being generated, the normal path.
                    Some((code, e)) if code == "1019" || e.to_lowercase().contains("in progress") => {
                        if job.attempt >= POLL_TRIES {
                            return Err(ConnectorError::new(
                                ErrorKind::Transient,
                                format!(
                                    "IBKR was still generating the statement after {}s. A large query can take longer, try again in a minute",
                                    POLL_TRIES * POLL_DELAY_SECS
                                ),
                            ));
                        }
                        job.attempt += 1;
                        s.next.entry(corr).or_default().push(statement_request(&job.reference, corr));
                        s.jobs.insert(corr, job);
                        Ok(vec![])
                    }
                    Some((code, e)) => Err(ConnectorError::new(code_kind(&code), format!("IBKR could not deliver the statement: {e}")).with_code(code)),
                    None => Err(ConnectorError::decode("IBKR returned an answer that is not a Flex statement")),
                }
            }
            other => Err(ConnectorError::decode(format!("ibkr flex: answer from an unknown endpoint {other}"))),
        }
    }

    fn follow_ups(&self, corr_id: CorrId, s: &mut Session) -> Vec<NativeRequest> {
        s.next.remove(&corr_id).unwrap_or_default()
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.jobs.remove(&corr_id);
        s.next.remove(&corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let token = secrets
            .get("flex_token")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "IBKR Flex requires a flex_token credential: set it on the settings page"))?;
        Ok(Some(Arc::new(ApiKeyQuerySigner { param: "t", key: SecretString::from(token.to_string()) })))
    }

    fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        if let Some(q) = settings.get("query_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !q.chars().all(|c| c.is_ascii_digit()) {
                return Err(ConnectorError::invalid("a Flex query id is the number IBKR shows next to the query"));
            }
        }
        if let Some(tz) = settings.get("tz_offset").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let mins: i32 = tz.parse().map_err(|_| ConnectorError::invalid("the time offset is a number of minutes, e.g. -300"))?;
            if !(-840..=840).contains(&mins) {
                return Err(ConnectorError::invalid("a time offset lies between -840 and 840 minutes"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "flex_tests.rs"]
mod tests;
