//! Gate.io accounts, read only, one book per adapter.
//!
//!   - Spot: balances of the classic spot account, or of the unified account when the key's
//!     account runs in a unified mode (asked first, `unified_mode`); open orders of every
//!     pair; fills over time slices walked backwards from the period's end, each paged.
//!   - Futures (USDT-settled): the futures account, positions, open orders and fills, the
//!     same way.
//!
//! The fill endpoints filter on time and page by number or offset, with a ceiling on how
//! deep a page may go. A slice whose first page comes back full is asked again at half the
//! width before anything of it is kept, down to an hour, so a busy week is read in pieces
//! that page shallowly; a quiet slice lets the next one widen again, up to a week.
//!
//! Futures quantities are contracts; the instrument list carries what one is worth. Fees are
//! positive for a cost, as Gate.io writes them.

use std::collections::{BTreeMap, HashMap};

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill,
    InstrumentId, Mapped, Money, Order, Position, Price, Qty, Side, TimeRange, Timestamp,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Book, API, REST, SETTLE};

const FILL_PAGE: usize = 1000;
const LIST_PAGE: usize = 100;
/// The widest and narrowest fill slices, in seconds.
const MAX_SPAN: i64 = 7 * 86_400;
const MIN_SPAN: i64 = 3_600;
const MAX_PAGES: u32 = 2_000;

pub enum Pending {
    Summary,
    /// Spot balances, step one: which mode the account runs in.
    UnifiedMode,
    SpotBalances,
    UnifiedBalances,
    SpotOrders { page: u32 },
    Positions { offset: u32 },
    FuturesOrders { offset: u32 },
    Fills { cur: Cursor, range: TimeRange },
}

/// Where a fill walk stands: the slice's end (inclusive, seconds), its width, the page in
/// it, pages used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub slice_to: i64,
    pub span: i64,
    pub page: u32,
    pub pages: u32,
}

fn bad(c: &str) -> ConnectorError {
    ConnectorError::invalid(format!("bad cursor `{c}`"))
}

impl Cursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}:{}", self.slice_to, self.span, self.page, self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = c.split(':').collect();
        let [to, span, page, pages] = parts.as_slice() else { return Err(bad(c)) };
        Ok(Cursor {
            slice_to: to.parse().map_err(|_| bad(c))?,
            span: span.parse().map_err(|_| bad(c))?,
            page: page.parse().map_err(|_| bad(c))?,
            pages: pages.parse().map_err(|_| bad(c))?,
        })
    }

    fn slice_from(&self, from_s: i64) -> i64 {
        (self.slice_to - self.span + 1).max(from_s)
    }
}

/// The period in whole seconds, both ends inclusive.
fn secs(range: &TimeRange) -> (i64, i64) {
    (range.from.millis().div_euclid(1000), (range.to.millis() - 1).div_euclid(1000))
}

fn signed(endpoint: &'static str, path: String, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed()
}

fn spot_balances(req_id: CorrId, pending: &mut HashMap<CorrId, super::Pending>) -> NativeRequest {
    pending.insert(req_id, super::Pending::Account(Pending::SpotBalances));
    NativeRequest::Http(signed("spotAccounts", format!("{API}/spot/accounts"), req_id))
}

fn period(range: &Option<TimeRange>) -> Result<TimeRange, ConnectorError> {
    let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
    if range.to.millis() <= range.from.millis() {
        return Err(ConnectorError::invalid("the period ends before it starts"));
    }
    Ok(range)
}

fn fills_request(book: Book, req_id: CorrId, cur: &Cursor, range: &TimeRange) -> Result<HttpRequest, ConnectorError> {
    if cur.pages >= MAX_PAGES {
        return Err(ConnectorError::invalid(format!("this period holds more {} fills than one pull reads. Ask for a shorter period.", book.label())));
    }
    let (from_s, _) = secs(range);
    let h = match book {
        Book::Spot => signed("myTrades", format!("{API}/spot/my_trades"), req_id).query("page", cur.page),
        Book::Futures => signed("myTradesTimerange", format!("{API}/futures/{SETTLE}/my_trades_timerange"), req_id)
            .query("offset", (cur.page.saturating_sub(1) as usize) * FILL_PAGE),
    };
    Ok(h.query("limit", FILL_PAGE).query("from", cur.slice_from(from_s)).query("to", cur.slice_to))
}

pub fn encode(book: Book, cmd: &Command, pending: &mut HashMap<CorrId, super::Pending>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    let num = |c: &Option<String>, default: u32| -> Result<u32, ConnectorError> {
        match c {
            Some(c) => c.parse::<u32>().map_err(|_| bad(c)),
            None => Ok(default),
        }
    };
    let (req, p) = match (book, kind) {
        (Book::Spot, AccountKind::Summary) => (signed("accountDetail", format!("{API}/account/detail"), req_id), Pending::Summary),
        (Book::Spot, AccountKind::Balances) => (signed("unifiedMode", format!("{API}/unified/unified_mode"), req_id), Pending::UnifiedMode),
        (Book::Spot, AccountKind::OpenOrders) => {
            let page = num(cursor, 1)?;
            (signed("openOrders", format!("{API}/spot/open_orders"), req_id).query("page", page).query("limit", LIST_PAGE), Pending::SpotOrders { page })
        }
        (Book::Futures, AccountKind::Summary) => (signed("futuresAccounts", format!("{API}/futures/{SETTLE}/accounts"), req_id), Pending::Summary),
        (Book::Futures, AccountKind::Positions) => {
            let offset = num(cursor, 0)?;
            let h = signed("positions", format!("{API}/futures/{SETTLE}/positions"), req_id).query("holding", "true").query("limit", LIST_PAGE).query("offset", offset);
            (h, Pending::Positions { offset })
        }
        (Book::Futures, AccountKind::OpenOrders) => {
            let offset = num(cursor, 0)?;
            let h = signed("orders", format!("{API}/futures/{SETTLE}/orders"), req_id).query("status", "open").query("limit", LIST_PAGE).query("offset", offset);
            (h, Pending::FuturesOrders { offset })
        }
        (_, AccountKind::Fills) => {
            let range = period(range)?;
            let cur = match cursor {
                Some(c) => Cursor::parse(c)?,
                None => Cursor { slice_to: secs(&range).1, span: MAX_SPAN, page: 1, pages: 0 },
            };
            (fills_request(book, req_id, &cur, &range)?, Pending::Fills { cur, range })
        }
        (b, k) => return Err(ConnectorError::unsupported(format!("{} has no {k:?}", b.label()))),
    };
    pending.insert(req_id, super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

/// A refused account call. The mode check may be refused to a key without unified-account
/// access: the classic spot account is read instead. Anything else says what to check.
pub fn on_refusal(p: Pending, corr: CorrId, e: ConnectorError, pending: &mut HashMap<CorrId, super::Pending>) -> Result<NativeRequest, ConnectorError> {
    match p {
        Pending::UnifiedMode if matches!(e.kind, ErrorKind::Permission | ErrorKind::InvalidRequest | ErrorKind::NotFound) => Ok(spot_balances(corr, pending)),
        p => Err(hint(&p, e)),
    }
}

/// What to check when Gate.io refuses the key.
fn hint(p: &Pending, e: ConnectorError) -> ConnectorError {
    let futures = matches!(p, Pending::Positions { .. } | Pending::FuturesOrders { .. });
    let fix = match (e.kind, e.provider_code.as_deref()) {
        (ErrorKind::Auth, Some("IP_FORBIDDEN")) => "The key is bound to IP addresses and this server is not one of them: add its address to the key, or remove the restriction.",
        (ErrorKind::Auth, Some("REQUEST_EXPIRED")) => "This server's clock is more than 60 seconds off: set it right (NTP).",
        (ErrorKind::Auth, _) => "Check the key and its secret.",
        (ErrorKind::Permission, _) if futures => "The key needs read access to Perpetual Futures.",
        (ErrorKind::Permission, _) => "The key needs read access to Spot (and to Unified Account for a unified account), and to Perpetual Futures for the futures book.",
        _ => return e,
    };
    ConnectorError { message: format!("{}. {fix}", e.message), ..e }
}

#[derive(Deserialize)]
struct AccountDetail {
    #[serde(default)]
    user_id: Option<Int>,
    #[serde(default)]
    ip_whitelist: Vec<String>,
    #[serde(default)]
    key: Option<KeyDetail>,
}

#[derive(Deserialize)]
struct KeyDetail {
    /// 1 classic, 2 legacy unified.
    #[serde(default)]
    mode: Option<Int>,
}

#[derive(Deserialize)]
struct UnifiedMode {
    #[serde(default)]
    mode: String,
}

#[derive(Deserialize)]
struct SpotAccount {
    currency: String,
    #[serde(default)]
    available: OptDec,
    #[serde(default)]
    locked: OptDec,
}

#[derive(Deserialize)]
struct UnifiedAccount {
    #[serde(default)]
    balances: BTreeMap<String, UnifiedBalance>,
}

#[derive(Deserialize)]
struct UnifiedBalance {
    #[serde(default)]
    available: OptDec,
    #[serde(default)]
    freeze: OptDec,
    #[serde(default)]
    borrowed: OptDec,
}

#[derive(Deserialize)]
struct FuturesAccount {
    #[serde(default)]
    total: OptDec,
    #[serde(default)]
    unrealised_pnl: OptDec,
    #[serde(default)]
    currency: String,
}

#[derive(Deserialize)]
struct PairOrders {
    #[serde(default)]
    orders: Vec<SpotOrder>,
}

#[derive(Deserialize)]
struct SpotOrder {
    #[serde(default)]
    id: String,
    currency_pair: String,
    #[serde(default)]
    side: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    filled_amount: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    create_time_ms: OptDec,
}

#[derive(Deserialize)]
struct SpotTrade {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    currency_pair: String,
    #[serde(default)]
    order_id: Option<serde_json::Value>,
    #[serde(default)]
    side: String,
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: String,
    /// Fees paid in GT or in point cards instead of the fee currency.
    #[serde(default)]
    gt_fee: OptDec,
    #[serde(default)]
    point_fee: OptDec,
    create_time_ms: Dec,
}

#[derive(Deserialize)]
struct PositionRow {
    contract: String,
    /// Contracts, signed.
    size: Dec,
    #[serde(default)]
    entry_price: OptDec,
    #[serde(default)]
    mark_price: OptDec,
    #[serde(default)]
    unrealised_pnl: OptDec,
}

#[derive(Deserialize)]
struct FuturesOrder {
    #[serde(default)]
    id: Option<serde_json::Value>,
    contract: String,
    /// Contracts, positive to buy and negative to sell.
    size: Dec,
    /// Contracts still working.
    #[serde(default)]
    left: OptDec,
    /// `0` with `ioc` is a market order.
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    tif: String,
    /// Seconds, with a fraction.
    #[serde(default)]
    create_time: OptDec,
}

#[derive(Deserialize)]
struct FuturesTrade {
    #[serde(default)]
    trade_id: Option<serde_json::Value>,
    #[serde(default)]
    contract: String,
    #[serde(default)]
    order_id: Option<serde_json::Value>,
    /// Contracts, positive bought and negative sold.
    size: Dec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    /// Seconds, with a fraction.
    create_time: Dec,
}

/// An id Gate.io writes as a number on one endpoint and a string on another.
fn id_of(v: &Option<serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("gateio {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("gateio", "side", raw),
    }
}

/// A futures side is the sign of the size.
fn side_of(size: Decimal) -> Mapped<Side> {
    if size.is_zero() {
        otw_connector_sdk::enums::unknown("gateio", "size", "0")
    } else if size.is_sign_negative() {
        Mapped::Known(Side::Sell)
    } else {
        Mapped::Known(Side::Buy)
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

/// Milliseconds, as a decimal count of milliseconds (`"1792392000123.456"`).
fn at_ms(ms: Decimal) -> Result<Timestamp, ConnectorError> {
    let v = i64::try_from(ms.trunc()).map_err(|_| ConnectorError::decode(format!("gateio time {ms} out of range")))?;
    ts_millis(v)
}

/// Seconds with a fraction (`1792392000.123`).
fn at_secs(s: Decimal) -> Result<Timestamp, ConnectorError> {
    at_ms(s.checked_mul(Decimal::from(1000)).ok_or_else(|| ConnectorError::decode(format!("gateio time {s} out of range")))?)
}

fn summary(detail: String) -> AccountSummary {
    AccountSummary { account_type: None, permissions: vec![], assets_held: 0, can_trade: None, open_positions: None, wallet_balance: None, detail: Some(detail) }
}

fn held(asset: String, total: Decimal, available: Decimal) -> Option<Balance> {
    (total.is_sign_positive() && !total.is_zero()).then(|| Balance { asset, total: Money(total), available: Money(available.min(total)), name: None, fiat: false, account: None })
}

/// What a spot fill cost: the fee in its currency, else the GT or points paid instead.
fn spot_fee(t: &SpotTrade) -> (Money, Option<String>) {
    let nonzero = |d: OptDec| d.0.filter(|v| !v.is_zero());
    if let Some(f) = nonzero(t.fee) {
        return (Money(f), Some(t.fee_currency.to_ascii_uppercase()).filter(|c| !c.is_empty()));
    }
    if let Some(f) = nonzero(t.gt_fee) {
        return (Money(f), Some("GT".into()));
    }
    if let Some(f) = nonzero(t.point_fee) {
        return (Money(f), Some("POINT".into()));
    }
    (Money(Decimal::ZERO), Some(t.fee_currency.to_ascii_uppercase()).filter(|c| !c.is_empty()))
}

/// The next step of a fill walk, from how full this page came back. `None`: done.
/// `keep`: whether this page's rows are kept (a full first page of a wide slice is not:
/// the slice is asked again at half the width).
fn next_cursor(cur: Cursor, n: usize, from_s: i64) -> (Option<Cursor>, bool) {
    let pages = cur.pages + 1;
    if n >= FILL_PAGE && cur.page == 1 && cur.span > MIN_SPAN {
        return (Some(Cursor { span: (cur.span / 2).max(MIN_SPAN), pages, ..cur }), false);
    }
    if n >= FILL_PAGE {
        return (Some(Cursor { page: cur.page + 1, pages, ..cur }), true);
    }
    let start = cur.slice_from(from_s);
    if start <= from_s {
        return (None, true);
    }
    let span = if cur.page == 1 && n < FILL_PAGE / 2 { (cur.span * 2).min(MAX_SPAN) } else { cur.span };
    (Some(Cursor { slice_to: start - 1, span, page: 1, pages }), true)
}

pub fn decode(book: Book, p: Pending, corr: CorrId, body: &[u8], pending: &mut HashMap<CorrId, super::Pending>, ctx: &MapCtx<'_>) -> Result<(Vec<Envelope>, Option<NativeRequest>), ConnectorError> {
    let venue = book.venue();
    let evs = match p {
        Pending::Summary if book == Book::Spot => {
            let d: AccountDetail = serde_json::from_slice(body).map_err(|e| decode_err("account detail", e))?;
            let mode = match d.key.and_then(|k| k.mode).map(|m| m.0) {
                Some(1) => "classic",
                Some(2) => "legacy unified",
                _ => "unknown",
            };
            let user = d.user_id.map(|u| format!(" for user {}", u.0)).unwrap_or_default();
            let ips = if d.ip_whitelist.is_empty() { "no IP list".to_string() } else { format!("{} whitelisted IP(s)", d.ip_whitelist.len()) };
            // Gate.io publishes no call that names a key's permissions.
            vec![env(ctx, corr, Event::AccountSummary(summary(format!("Gate.io key accepted{user}, {mode} key mode, {ips}"))))]
        }
        Pending::Summary => {
            let a: FuturesAccount = serde_json::from_slice(body).map_err(|e| decode_err("futures accounts", e))?;
            let ccy = if a.currency.is_empty() { SETTLE.to_ascii_uppercase() } else { a.currency.to_ascii_uppercase() };
            // Rounded, not cut: `Decimal` precision formatting truncates.
            let total = a.total.0.unwrap_or_default().round_dp(2);
            let upnl = a.unrealised_pnl.0.unwrap_or_default().round_dp(2);
            vec![env(ctx, corr, Event::AccountSummary(summary(format!("Gate.io futures: {total:.2} {ccy} in the account, {upnl:.2} unrealised"))))]
        }
        Pending::UnifiedMode => {
            let m: UnifiedMode = serde_json::from_slice(body).map_err(|e| decode_err("unified mode", e))?;
            let req = if m.mode.is_empty() || m.mode == "classic" {
                spot_balances(corr, pending)
            } else {
                pending.insert(corr, super::Pending::Account(Pending::UnifiedBalances));
                NativeRequest::Http(signed("unifiedAccounts", format!("{API}/unified/accounts"), corr))
            };
            return Ok((vec![], Some(req)));
        }
        Pending::SpotBalances => {
            let rows: Vec<SpotAccount> = serde_json::from_slice(body).map_err(|e| decode_err("spot accounts", e))?;
            let out = rows
                .into_iter()
                .filter_map(|a| {
                    let available = a.available.0.unwrap_or_default();
                    held(a.currency.to_ascii_uppercase(), available + a.locked.0.unwrap_or_default(), available)
                })
                .collect();
            vec![chunk(ctx, corr, AccountItems::Balances(out), None)]
        }
        Pending::UnifiedBalances => {
            let u: UnifiedAccount = serde_json::from_slice(body).map_err(|e| decode_err("unified accounts", e))?;
            // What the account owns of each coin: held, frozen in orders, less what it borrowed.
            let out = u
                .balances
                .into_iter()
                .filter_map(|(coin, b)| {
                    let available = b.available.0.unwrap_or_default();
                    let total = available + b.freeze.0.unwrap_or_default() - b.borrowed.0.unwrap_or_default();
                    held(coin.to_ascii_uppercase(), total, available)
                })
                .collect();
            vec![chunk(ctx, corr, AccountItems::Balances(out), None)]
        }
        Pending::SpotOrders { page } => {
            let pairs: Vec<PairOrders> = serde_json::from_slice(body).map_err(|e| decode_err("open orders", e))?;
            // The page size counts orders per pair: another page is due while a pair filled one.
            let more = pairs.iter().any(|p| p.orders.len() >= LIST_PAGE);
            let mut orders = Vec::new();
            for o in pairs.into_iter().flat_map(|p| p.orders) {
                let qty = o.amount.0.unwrap_or_default();
                orders.push(Order {
                    id: o.id,
                    instrument: InstrumentId::new(venue, &o.currency_pair.to_ascii_uppercase()),
                    side: side(&o.side),
                    order_type: o.kind,
                    qty: Qty(qty),
                    filled_qty: Qty(o.filled_amount.0.unwrap_or_default()),
                    limit_price: positive(o.price),
                    stop_price: None,
                    placed_at: o.create_time_ms.0.map(at_ms).transpose()?,
                    quote_ccy: o.currency_pair.rsplit_once('_').map(|(_, q)| q.to_ascii_uppercase()),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            vec![chunk(ctx, corr, AccountItems::Orders(orders), more.then(|| (page + 1).to_string()))]
        }
        Pending::Positions { offset } => {
            let rows: Vec<PositionRow> = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            let n = rows.len();
            let out = rows
                .into_iter()
                .filter(|p| !p.size.0.is_zero())
                .map(|p| Position {
                    instrument: InstrumentId::new(venue, &p.contract.to_ascii_uppercase()),
                    qty: Qty(p.size.0),
                    avg_price: positive(p.entry_price),
                    unrealized_pnl: p.unrealised_pnl.0.map(Money),
                    currency: Some(SETTLE.to_ascii_uppercase()),
                    mark_price: positive(p.mark_price),
                    venue: None,
                    account: None,
                    class: None,
                    multiplier: None,
                    name: None,
                })
                .collect();
            let next = (n >= LIST_PAGE).then(|| (offset as usize + n).to_string());
            vec![chunk(ctx, corr, AccountItems::Positions(out), next)]
        }
        Pending::FuturesOrders { offset } => {
            let rows: Vec<FuturesOrder> = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            let n = rows.len();
            let mut orders = Vec::with_capacity(n);
            for o in rows {
                let qty = o.size.0.abs();
                let left = o.left.0.map(|l| l.abs()).unwrap_or(qty);
                let market = o.price.0.is_none_or(|p| p.is_zero());
                orders.push(Order {
                    id: id_of(&o.id),
                    instrument: InstrumentId::new(venue, &o.contract.to_ascii_uppercase()),
                    side: side_of(o.size.0),
                    order_type: if market { format!("market {}", o.tif) } else { format!("limit {}", o.tif) }.trim().to_string(),
                    qty: Qty(qty),
                    filled_qty: Qty((qty - left).max(Decimal::ZERO)),
                    limit_price: positive(o.price),
                    stop_price: None,
                    placed_at: o.create_time.0.map(at_secs).transpose()?,
                    quote_ccy: Some(SETTLE.to_ascii_uppercase()),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            let next = (n >= LIST_PAGE).then(|| (offset as usize + n).to_string());
            vec![chunk(ctx, corr, AccountItems::Orders(orders), next)]
        }
        Pending::Fills { cur, range } => {
            let (from_s, _) = secs(&range);
            let mut fills = Vec::new();
            let n = match book {
                Book::Spot => {
                    let rows: Vec<SpotTrade> = serde_json::from_slice(body).map_err(|e| decode_err("my trades", e))?;
                    let n = rows.len();
                    for t in rows.into_iter().filter(|t| !t.currency_pair.is_empty()) {
                        let (fee, fee_asset) = spot_fee(&t);
                        fills.push(Fill {
                            id: id_of(&t.id),
                            order_id: Some(id_of(&t.order_id)).filter(|o| !o.is_empty()),
                            instrument: InstrumentId::new(venue, &t.currency_pair.to_ascii_uppercase()),
                            ts: at_ms(t.create_time_ms.0)?,
                            side: side(&t.side),
                            qty: Qty(t.amount.0.unwrap_or_default()),
                            price: Price(t.price.0.unwrap_or_default()),
                            fee,
                            fee_asset,
                            quote_ccy: t.currency_pair.rsplit_once('_').map(|(_, q)| q.to_ascii_uppercase()),
                            account: None,
                            venue: None,
                            class: None,
                            multiplier: None,
                        });
                    }
                    n
                }
                Book::Futures => {
                    let rows: Vec<FuturesTrade> = serde_json::from_slice(body).map_err(|e| decode_err("my trades timerange", e))?;
                    let n = rows.len();
                    for t in rows.into_iter().filter(|t| !t.contract.is_empty()) {
                        fills.push(Fill {
                            id: id_of(&t.trade_id),
                            order_id: Some(id_of(&t.order_id)).filter(|o| !o.is_empty()),
                            instrument: InstrumentId::new(venue, &t.contract.to_ascii_uppercase()),
                            ts: at_secs(t.create_time.0)?,
                            side: side_of(t.size.0),
                            qty: Qty(t.size.0.abs()),
                            price: Price(t.price.0.unwrap_or_default()),
                            fee: Money(t.fee.0.unwrap_or_default()),
                            fee_asset: Some(SETTLE.to_ascii_uppercase()),
                            quote_ccy: Some(SETTLE.to_ascii_uppercase()),
                            account: None,
                            venue: None,
                            class: None,
                            multiplier: None,
                        });
                    }
                    n
                }
            };
            let (next, keep) = next_cursor(cur, n, from_s);
            if !keep {
                fills.clear();
            }
            vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))]
        }
    };
    Ok((evs, None))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_full_first_page_halves_the_slice_and_a_quiet_one_widens_the_next() {
        let week = Cursor { slice_to: 1_000_000, span: MAX_SPAN, page: 1, pages: 0 };
        let (next, keep) = next_cursor(week, FILL_PAGE, 0);
        assert_eq!((next, keep), (Some(Cursor { span: MAX_SPAN / 2, pages: 1, ..week }), false));
        // At the narrowest width a full page pages on instead.
        let hour = Cursor { span: MIN_SPAN, ..week };
        assert_eq!(next_cursor(hour, FILL_PAGE, 0), (Some(Cursor { page: 2, pages: 1, ..hour }), true));
        // A quiet slice moves on below itself, twice as wide.
        let half = Cursor { span: MAX_SPAN / 2, ..week };
        assert_eq!(next_cursor(half, 3, 0), (Some(Cursor { slice_to: 1_000_000 - MAX_SPAN / 2, span: MAX_SPAN, page: 1, pages: 1 }), true));
        // The slice that reaches the period's start is the last.
        assert_eq!(next_cursor(week, 3, 1_000_000 - 10), (None, true));
        assert_eq!(Cursor::parse(&week.encode()).unwrap(), week);
    }
}
