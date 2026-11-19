//! NinjaTrader's account, read only: fills, open positions and working orders, over the
//! Trader API NinjaTrader runs on the Tradovate platform it acquired (every host and path
//! says `tradovateapi`).
//!
//! The login, its password and the API key pair (`cid`, `sec`) are exchanged for a bearer
//! token of about ninety minutes, minted once per login and shared: the platform allows two
//! sessions per user and closes the oldest when a third opens, so minting per call would
//! knock the user's own trading application off.
//!
//! What the API's shape forces, every step chained through `Mapper::follow_ups`:
//!   - everything is an entity list, not a query: `fill/list` has no range and no cursor, so
//!     the period filters here;
//!   - a fill names its order and only the order names the account, so the orders are read
//!     first;
//!   - a contract is three hops from its point value (contract, maturity, product), walked
//!     once per contract and command. A futures point value is asked, never assumed.
//!
//! No market data: prices come over a separate socket on a separate entitlement.
//! Endpoint-by-endpoint mapping in PROVIDER.md.

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, AssetClass, Capabilities, CapsError, Command, ConnectorError, CorrId, Decimal, Envelope,
    ErrorKind, Event, Fill, InstrumentId, Mapped, Money, Order, Position, Price, Qty, Side, TimeRange, Timestamp,
};
use otw_connector_sdk::parse::{dec, ts_rfc3339};
use otw_connector_sdk::signer::{cached_token, credential_key, drop_token, keep_token, SignableRequest, TokenBody, TokenRequest};
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::value::RawValue;

pub const VENUE: &str = "NINJATRADER";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://live.tradovateapi.com/v1")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// A token lasts about ninety minutes; the answer carries the exact moment.
const TOKEN_TTL_SECS: i64 = 90 * 60;
/// What the app calls itself to the platform. Not a credential.
const APP_ID: &str = "OpenTraderWorld";
/// Order states that are still working: everything else is history.
const WORKING: &[&str] = &["Working", "PendingNew", "PendingReplace"];
/// Every charge the platform books against a fill, each on its own line.
const FEE_LINES: &[&str] = &["clearingFee", "exchangeFee", "nfaFee", "brokerageFee", "ipFee", "commission", "orderRoutingFee"];
/// Steps whose failure leaves a gap rather than failing the read: a fee, an order version or
/// a contract's terms the platform does not answer.
const LENIENT: &[&str] = &["fees", "versions", "contract", "maturity", "product", "currency"];

#[derive(Clone)]
pub struct NinjaTrader {
    caps: Capabilities,
}

impl NinjaTrader {
    pub fn new() -> Result<Self, CapsError> {
        Ok(NinjaTrader { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// An entity as the platform writes it, each field kept as its raw JSON text so a number is
/// read from its digits, never through a float.
type Row = HashMap<String, Box<RawValue>>;

fn raw<'a>(r: &'a Row, k: &str) -> Option<&'a str> {
    r.get(k).map(|v| v.get().trim())
}

fn unquote(t: &str) -> Option<String> {
    serde_json::from_str::<String>(t).ok()
}

fn text(r: &Row, k: &str) -> String {
    raw(r, k).and_then(unquote).unwrap_or_default()
}

/// An entity id; a missing or unreadable one is 0, which matches nothing.
fn int(r: &Row, k: &str) -> i64 {
    match raw(r, k) {
        Some(t) if t.starts_with('"') => unquote(t).and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        Some(t) => t.parse().unwrap_or(0),
        None => 0,
    }
}

fn num(r: &Row, k: &str) -> Decimal {
    let t = match raw(r, k) {
        Some(t) if t.starts_with('"') => unquote(t).unwrap_or_default(),
        Some(t) => t.to_string(),
        None => return Decimal::ZERO,
    };
    dec(&t).unwrap_or(Decimal::ZERO)
}

fn stamp(r: &Row, k: &str) -> Option<Timestamp> {
    ts_rfc3339(&text(r, k)).ok()
}

fn positive(d: Decimal) -> Option<Price> {
    (d > Decimal::ZERO).then_some(Price(d))
}

/// A list answer, element by element: one entity that is not an object is skipped, an answer
/// that is not a list is an empty one.
fn list(body: &[u8]) -> Result<Vec<Row>, ConnectorError> {
    let v: Box<RawValue> = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("ninjatrader: unreadable answer: {e}")))?;
    let items: Vec<Box<RawValue>> = serde_json::from_str(v.get()).unwrap_or_default();
    Ok(items.iter().filter_map(|i| serde_json::from_str::<Row>(i.get()).ok()).collect())
}

fn item(body: &[u8]) -> Result<Row, ConnectorError> {
    serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("ninjatrader: unreadable item: {e}")))
}

/// The platform's product type. A spread or anything unnamed is none of the other classes,
/// and filing it as a future would give it a point value it has not got.
pub(crate) fn class_of(product_type: &str) -> AssetClass {
    match product_type {
        "Futures" | "Continuous" => AssetClass::Future,
        "CommonStock" => AssetClass::Equity,
        "Cryptocurrency" => AssetClass::Spot,
        "Options" => AssetClass::Option,
        "MarketInternals" => AssetClass::Index,
        "Spread" => AssetClass::Other,
        other => {
            tracing::warn!(product_type = other, "ninjatrader: unknown product type, filed as other");
            AssetClass::Other
        }
    }
}

fn side(action: &str) -> Mapped<Side> {
    if action.eq_ignore_ascii_case("buy") {
        Mapped::Known(Side::Buy)
    } else if action.eq_ignore_ascii_case("sell") {
        Mapped::Known(Side::Sell)
    } else {
        tracing::warn!(action, "ninjatrader: unknown order action");
        Mapped::Unknown(action.to_string())
    }
}

/// Every fee line on a fill as one positive cost. A trade that showed only the commission
/// would understate what it cost.
pub(crate) fn total_fee(r: &Row) -> Decimal {
    FEE_LINES.iter().map(|k| num(r, k).abs()).sum()
}

/// What the platform says a contract is and is worth.
#[derive(Clone)]
struct Contract {
    /// The contract's own name, as the platform spells it (`ESH6`).
    name: String,
    class: AssetClass,
    /// Point value, from the product rather than from a habit.
    multiplier: Decimal,
    currency: String,
}

impl Default for Contract {
    /// An instrument nobody could identify: no name, filed as other, one per point.
    fn default() -> Self {
        Contract { name: String::new(), class: AssetClass::Other, multiplier: Decimal::ONE, currency: String::new() }
    }
}

/// One account read in progress.
struct Job {
    kind: AccountKind,
    range: Option<TimeRange>,
    /// The account the connector's setting names.
    wanted: Option<String>,
    account: String,
    /// The endpoint the next answer comes from.
    expect: &'static str,
    /// Order id to the account it belongs to (a fill names only its order).
    order_account: HashMap<i64, String>,
    fees: HashMap<i64, Decimal>,
    /// Order id to its latest version, where a working order's price and size live.
    versions: HashMap<i64, Row>,
    rows: Vec<Row>,
    contracts: HashMap<i64, Contract>,
    /// Contracts still to walk, in the order the rows name them.
    queue: Vec<i64>,
    /// The contract being walked.
    hop: Option<(i64, Contract)>,
}

#[derive(Default)]
pub struct Session {
    jobs: HashMap<CorrId, Job>,
    next: HashMap<CorrId, Vec<NativeRequest>>,
}

fn get(endpoint: &'static str, path: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, corr).signed()
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems) -> Envelope {
    env(ctx, corr, Event::AccountChunk { items, next_cursor: None, done: true })
}

fn trim(body: &[u8]) -> String {
    let t = String::from_utf8_lossy(body);
    let t = t.trim();
    match t.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

/// The fault a status means. A 401 or 403 is the session ended: spent, or another session
/// took the seat.
fn refusal(frame: &RawFrame) -> ConnectorError {
    let e = http::timeout(frame, "ninjatrader")
        .or_else(|| http::classify(frame, "ninjatrader"))
        .unwrap_or_else(|| ConnectorError::decode("ninjatrader: refused with no status"));
    if let RawFrame::Http { status: 401 | 403, body, .. } = frame {
        return ConnectorError::new(
            ErrorKind::Auth,
            format!(
                "NinjaTrader ended this session ({}). The platform allows two sessions per login and closes the oldest when a third opens, so check what else is signed in",
                trim(body)
            ),
        );
    }
    e
}

fn named(accounts: &[(String, String)]) -> String {
    accounts.iter().map(|(i, n)| format!("{n} ({i})")).collect::<Vec<_>>().join(", ")
}

/// The setting when there is one (and the login carries it), the only account otherwise. A
/// login with several has no obvious main account, and picking would file someone else's
/// trades.
fn pick(accounts: &[(String, String)], wanted: Option<&str>) -> Result<String, ConnectorError> {
    if let Some(id) = wanted {
        if !accounts.iter().any(|(known, _)| known == id) {
            return Err(ConnectorError::not_found(format!("this login does not carry account {id}. It has [{}]", named(accounts))));
        }
        return Ok(id.to_string());
    }
    match accounts {
        [(id, _)] => Ok(id.clone()),
        [] => Err(ConnectorError::new(ErrorKind::Permission, "this NinjaTrader login carries no open trading account")),
        many => Err(ConnectorError::invalid(format!(
            "this login carries {} accounts, so name one in the Account ID setting: [{}]",
            many.len(),
            named(many)
        ))),
    }
}

impl Job {
    fn ask(&mut self, s_next: &mut Vec<NativeRequest>, h: HttpRequest) {
        self.expect = h.endpoint_id;
        s_next.push(NativeRequest::Http(h));
    }

    /// The contracts the kept rows name, walked one at a time in the order the rows name
    /// them; the read ends when none is left.
    fn start_contracts(&mut self, corr: CorrId, next: &mut Vec<NativeRequest>, ctx: &MapCtx<'_>) -> Option<Vec<Envelope>> {
        let mut ids: Vec<i64> = Vec::new();
        for r in &self.rows {
            let id = int(r, "contractId");
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids.reverse();
        self.queue = ids;
        self.next_hop(corr, next, ctx)
    }

    fn next_hop(&mut self, corr: CorrId, next: &mut Vec<NativeRequest>, ctx: &MapCtx<'_>) -> Option<Vec<Envelope>> {
        match self.queue.pop() {
            Some(id) => {
                self.hop = Some((id, Contract::default()));
                self.ask(next, get("contract", "/contract/item", corr).query("id", id));
                None
            }
            None => Some(self.emit(corr, ctx)),
        }
    }

    /// The hop in progress is over: keep what it learned (nothing, when a step did not
    /// answer), walk the next.
    fn hop_done(&mut self, c: Option<Contract>, corr: CorrId, next: &mut Vec<NativeRequest>, ctx: &MapCtx<'_>) -> Option<Vec<Envelope>> {
        if let Some((id, _)) = self.hop.take() {
            self.contracts.insert(id, c.unwrap_or_default());
        }
        self.next_hop(corr, next, ctx)
    }

    fn contract(&self, r: &Row) -> Contract {
        self.contracts.get(&int(r, "contractId")).cloned().unwrap_or_default()
    }

    fn emit(&self, corr: CorrId, ctx: &MapCtx<'_>) -> Vec<Envelope> {
        let account = Some(self.account.clone());
        let ccy = |c: &Contract| Some(c.currency.clone());
        let items = match self.kind {
            AccountKind::Fills => AccountItems::Fills(
                self.rows
                    .iter()
                    .filter_map(|f| {
                        let c = self.contract(f);
                        let id = int(f, "id");
                        Some(Fill {
                            id: id.to_string(),
                            order_id: Some(int(f, "orderId").to_string()),
                            instrument: InstrumentId::new(VENUE, &c.name),
                            ts: stamp(f, "timestamp")?,
                            side: side(&text(f, "action")),
                            qty: Qty(num(f, "qty")),
                            price: Price(num(f, "price")),
                            fee: Money(self.fees.get(&id).copied().unwrap_or_default()),
                            fee_asset: ccy(&c),
                            quote_ccy: ccy(&c),
                            account: account.clone(),
                            venue: None,
                            class: Some(c.class),
                            multiplier: Some(c.multiplier),
                        })
                    })
                    .collect(),
            ),
            AccountKind::Positions => AccountItems::Positions(
                self.rows
                    .iter()
                    .map(|p| {
                        let c = self.contract(p);
                        Position {
                            instrument: InstrumentId::new(VENUE, &c.name),
                            // The sign of the net position is the side; there is no other field.
                            qty: Qty(num(p, "netPos")),
                            avg_price: positive(num(p, "netPrice")),
                            unrealized_pnl: None,
                            currency: ccy(&c),
                            mark_price: None,
                            venue: None,
                            account: account.clone(),
                            class: Some(c.class),
                            multiplier: Some(c.multiplier),
                            name: None,
                        }
                    })
                    .collect(),
            ),
            _ => AccountItems::Orders(
                self.rows
                    .iter()
                    .map(|o| {
                        let c = self.contract(o);
                        let v = self.versions.get(&int(o, "id"));
                        let of = |k: &str| v.map(|v| num(v, k)).unwrap_or_default();
                        Order {
                            id: int(o, "id").to_string(),
                            instrument: InstrumentId::new(VENUE, &c.name),
                            side: side(&text(o, "action")),
                            order_type: v.map(|v| text(v, "orderType")).unwrap_or_default(),
                            qty: Qty(of("orderQty")),
                            filled_qty: Qty(Decimal::ZERO),
                            limit_price: positive(of("price")),
                            stop_price: positive(of("stopPrice")),
                            placed_at: stamp(o, "timestamp"),
                            quote_ccy: ccy(&c),
                            account: account.clone(),
                            venue: None,
                            class: Some(c.class),
                        }
                    })
                    .collect(),
            ),
        };
        vec![chunk(ctx, corr, items)]
    }
}

/// The bearer token for a login, minted once and shared by every call on it.
pub struct NinjaTraderSigner {
    name: String,
    password: SecretString,
    cid: i64,
    sec: SecretString,
    /// `live` or `demo`, for a refusal that usually means the other one.
    env: &'static str,
    key: String,
}

impl Signer for NinjaTraderSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let t = cached_token(&self.key, now).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "no NinjaTrader access token: the sign-in did not answer"))?;
        req.headers.push(("Authorization".into(), format!("Bearer {}", t.expose_secret())));
        Ok(())
    }

    fn token_request(&self, now: Timestamp) -> Option<TokenRequest> {
        if cached_token(&self.key, now).is_some() {
            return None;
        }
        let body = serde_json::json!({
            "name": self.name,
            "password": self.password.expose_secret(),
            "appId": APP_ID,
            "appVersion": env!("CARGO_PKG_VERSION"),
            "cid": self.cid,
            "sec": self.sec.expose_secret(),
        });
        Some(TokenRequest::post(REST, "/auth/accesstokenrequest", TokenBody::Json(body.to_string())))
    }

    fn accept_token(&self, status: u16, _headers: &[(String, String)], body: &[u8], now: Timestamp) -> Result<(), ConnectorError> {
        #[derive(Deserialize)]
        struct Answer {
            #[serde(rename = "accessToken", default)]
            access_token: Option<String>,
            #[serde(rename = "expirationTime", default)]
            expiration: Option<String>,
            #[serde(rename = "errorText", default)]
            error_text: Option<String>,
        }
        if !(200..300).contains(&status) {
            return Err(ConnectorError::new(
                ErrorKind::Auth,
                format!(
                    "NinjaTrader refused the sign-in ({status}: {}). Check the username, the password and the API key pair, and that the key belongs to the {} environment",
                    trim(body),
                    self.env
                ),
            ));
        }
        let a: Answer = serde_json::from_slice(body).map_err(|e| ConnectorError::new(ErrorKind::Auth, format!("NinjaTrader token answer unreadable: {e}")))?;
        // A 200 can still carry a refusal: a login awaiting terms or a captcha answers here.
        let token = a.access_token.filter(|t| !t.is_empty()).ok_or_else(|| {
            ConnectorError::new(
                ErrorKind::Auth,
                format!(
                    "NinjaTrader returned no access token ({}). The login may need to accept new terms on the platform first",
                    a.error_text.unwrap_or_default()
                ),
            )
        })?;
        let ttl = a
            .expiration
            .and_then(|t| ts_rfc3339(&t).ok())
            .map(|exp| exp.secs().saturating_sub(now.secs()))
            .filter(|s| *s > 0)
            .unwrap_or(TOKEN_TTL_SECS);
        keep_token(&self.key, token, now, ttl);
        Ok(())
    }

    fn forget_token(&self) {
        drop_token(&self.key);
    }
}

impl Mapper for NinjaTrader {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// A contract's name as the platform writes it (`ESZ5`).
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_ascii_uppercase();
        if s.is_empty() || s.chars().any(char::is_control) {
            return Err(ConnectorError::not_found(format!("NinjaTrader: bad symbol `{symbol}`, name the contract (ESZ5)")));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    fn account_setting(&self) -> Option<&'static str> {
        Some("account_id")
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::QueryAccount { req_id, kind, range, account, .. } => {
                match kind {
                    AccountKind::Balances => return Err(ConnectorError::unsupported("NinjaTrader is read through its positions, not balances")),
                    AccountKind::Fills => {
                        let r = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
                        if r.to <= r.from {
                            return Err(ConnectorError::invalid("the period ends before it starts"));
                        }
                    }
                    _ => {}
                }
                s.jobs.insert(
                    *req_id,
                    Job {
                        kind: *kind,
                        range: *range,
                        wanted: account.clone().filter(|a| !a.trim().is_empty()),
                        account: String::new(),
                        expect: "accounts",
                        order_account: HashMap::new(),
                        fees: HashMap::new(),
                        versions: HashMap::new(),
                        rows: Vec::new(),
                        contracts: HashMap::new(),
                        queue: Vec::new(),
                        hop: None,
                    },
                );
                Ok(vec![NativeRequest::Http(get("accounts", "/account/list", *req_id))])
            }
            Command::RequestBoard { .. } => Err(ConnectorError::unsupported(
                "NinjaTrader serves prices through its market-data feed, which is a separate entitlement on its own socket rather than part of the trading API. Use a data connector to price these lines",
            )),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("NinjaTrader: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, status, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, status, body, .. } => (*corr_id, *endpoint_id, *status, body.clone()),
            RawFrame::Timeout { corr_id, endpoint_id } => (*corr_id, *endpoint_id, 0, bytes::Bytes::new()),
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let mut job = s.jobs.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if job.expect != endpoint {
            return Err(ConnectorError::decode(format!("ninjatrader: expected {}, answered {endpoint}", job.expect)));
        }
        let ok = (200..300).contains(&status);
        if !ok && !LENIENT.contains(&endpoint) {
            return Err(refusal(frame));
        }
        let mut next = Vec::new();
        let done = match endpoint {
            "accounts" => {
                let accounts: Vec<(String, String)> = list(&body)?
                    .iter()
                    .filter(|a| raw(a, "closed") != Some("true"))
                    .map(|a| (int(a, "id").to_string(), text(a, "name")))
                    .filter(|(id, _)| id != "0")
                    .collect();
                job.account = pick(&accounts, job.wanted.as_deref())?;
                match job.kind {
                    AccountKind::Summary => {
                        let picked = accounts.iter().find(|(id, _)| *id == job.account).map(|(id, n)| format!("{n} ({id})")).unwrap_or_else(|| job.account.clone());
                        let detail = format!("sign-in accepted, reading account {picked} of [{}]", named(&accounts));
                        Some(vec![env(
                            ctx,
                            corr,
                            Event::AccountSummary(AccountSummary {
                                account_type: None,
                                permissions: vec![],
                                assets_held: 0,
                                can_trade: None,
                                open_positions: None,
                                wallet_balance: None,
                                detail: Some(detail),
                            }),
                        )])
                    }
                    AccountKind::Positions => {
                        job.ask(&mut next, get("positions", "/position/list", corr));
                        None
                    }
                    _ => {
                        job.ask(&mut next, get("orders", "/order/list", corr));
                        None
                    }
                }
            }
            "orders" => {
                let rows = list(&body)?;
                if job.kind == AccountKind::Fills {
                    job.order_account = rows.iter().map(|o| (int(o, "id"), int(o, "accountId").to_string())).collect();
                    job.ask(&mut next, get("fees", "/fillFee/list", corr));
                    None
                } else {
                    let account = job.account.clone();
                    job.rows = rows.into_iter().filter(|o| int(o, "accountId").to_string() == account && WORKING.contains(&text(o, "ordStatus").as_str())).collect();
                    job.ask(&mut next, get("versions", "/orderVersion/list", corr));
                    None
                }
            }
            "fees" => {
                if ok {
                    job.fees = list(&body).unwrap_or_default().iter().map(|f| (int(f, "id"), total_fee(f))).collect();
                }
                job.ask(&mut next, get("fills", "/fill/list", corr));
                None
            }
            "fills" => {
                let range = job.range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
                let account = job.account.clone();
                job.rows = list(&body)?
                    .into_iter()
                    .filter(|f| job.order_account.get(&int(f, "orderId")) == Some(&account))
                    .filter(|f| stamp(f, "timestamp").is_some_and(|at| at >= range.from && at <= range.to))
                    .collect();
                job.start_contracts(corr, &mut next, ctx)
            }
            "positions" => {
                let account = job.account.clone();
                job.rows = list(&body)?.into_iter().filter(|p| int(p, "accountId").to_string() == account && !num(p, "netPos").is_zero()).collect();
                job.start_contracts(corr, &mut next, ctx)
            }
            "versions" => {
                if ok {
                    for v in list(&body).unwrap_or_default() {
                        let order = int(&v, "orderId");
                        // Versions are numbered in the order they were made; the highest is current.
                        if job.versions.get(&order).is_none_or(|prev| int(&v, "id") > int(prev, "id")) {
                            job.versions.insert(order, v);
                        }
                    }
                }
                job.start_contracts(corr, &mut next, ctx)
            }
            "contract" | "maturity" | "product" => {
                let answered = if ok { item(&body).ok() } else { None };
                match (answered, job.hop.take()) {
                    (Some(r), Some((id, mut c))) => {
                        let h = match endpoint {
                            "contract" => {
                                c.name = text(&r, "name");
                                get("maturity", "/contractMaturity/item", corr).query("id", int(&r, "contractMaturityId"))
                            }
                            "maturity" => get("product", "/product/item", corr).query("id", int(&r, "productId")),
                            _ => {
                                c.class = class_of(&text(&r, "productType"));
                                // A futures point value is the product's, never the usual one for that root.
                                let v = num(&r, "valuePerPoint");
                                c.multiplier = if v > Decimal::ZERO { v } else { Decimal::ONE };
                                get("currency", "/currency/item", corr).query("id", int(&r, "currencyId"))
                            }
                        };
                        job.hop = Some((id, c));
                        job.ask(&mut next, h);
                        None
                    }
                    // A hop that did not answer: the contract is one nobody could identify.
                    (_, hop) => {
                        job.hop = hop;
                        job.hop_done(None, corr, &mut next, ctx)
                    }
                }
            }
            "currency" => {
                let mut c = job.hop.as_ref().map(|(_, c)| c.clone()).unwrap_or_default();
                if ok {
                    c.currency = item(&body).map(|r| text(&r, "name").to_uppercase()).unwrap_or_default();
                }
                job.hop_done(Some(c), corr, &mut next, ctx)
            }
            other => return Err(ConnectorError::decode(format!("ninjatrader: answer from an unknown endpoint {other}"))),
        };
        match done {
            Some(evs) => Ok(evs),
            None => {
                s.jobs.insert(corr, job);
                s.next.entry(corr).or_default().extend(next);
                Ok(vec![])
            }
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
        let get = |k: &str| secrets.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let need = |k: &str| get(k).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("NinjaTrader requires a {k} credential: set it on the settings page")));
        let (name, password, cid, sec) = (need("username")?, need("password")?, need("cid")?, need("sec")?);
        let env = if get("env").is_some_and(|e| e.eq_ignore_ascii_case("demo")) { "demo" } else { "live" };
        Ok(Some(Arc::new(NinjaTraderSigner {
            key: credential_key(&["ninjatrader", env, name, password, cid, sec]),
            name: name.to_string(),
            password: SecretString::from(password.to_string()),
            cid: cid.parse::<i64>().unwrap_or_default(),
            sec: SecretString::from(sec.to_string()),
            env,
        })))
    }

    fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        match settings.get("account_id").map(|s| s.trim()) {
            None | Some("") => Ok(()),
            Some(id) if id.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
            Some(other) => Err(ConnectorError::invalid(format!("a NinjaTrader account id is a number, not \"{other}\""))),
        }
    }
}

#[cfg(test)]
mod tests;
