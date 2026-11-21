//! KuCoin accounts, read only, one platform per adapter.
//!
//!   - Spot: balances of the funding (`main`) and trading (`trade`) accounts, open orders
//!     asked symbol by symbol (the venue first names the symbols that have any), then the
//!     untriggered stop orders (Get Stop Orders List, each `cond:<id>` so its cancel reaches
//!     the stop-order endpoint), fills per
//!     named symbol (the venue answers no other way) over 7-day slices walked backwards from
//!     the period's end, each paged by `lastId`.
//!   - Futures: the USDT equity, positions per settlement coin (USDT, then USDC), working
//!     orders by page, fills over 7-day slices, each paged by `currentPage`.
//!
//! Futures orders and positions are read in the base coin, as the order path states them: the
//! venue counts lots, so the read first takes the contract list (`GET /api/v1/contracts/active`)
//! and multiplies each row by its contract's `multiplier`; a row whose contract size is not
//! known is an error, never a guess. Futures fills stay in contracts, valued by the instrument
//! list. Fees are positive for a cost, as KuCoin writes them. Coin-margined (inverse) rows are
//! not read.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill,
    InstrumentId, Mapped, Money, Order, Position, Price, Qty, Side, TimeRange,
};
use std::collections::HashMap;

use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Book, Data, REST, SETTLE_COINS};

const SPOT_FILL_PAGE: usize = 100;
const FUTURES_PAGE: usize = 1000;
/// Both platforms answer fills a week at a time.
const SLICE_MS: i64 = 7 * 86_400_000;
const MAX_PAGES: u32 = 500;
/// Get Stop Orders List's largest page (`pageSize` "maximum: 500").
const SPOT_STOP_PAGE: u32 = 500;
/// The spot open-orders cursor of the stop list, then its page.
const STOPS_CURSOR: &str = "stops:";

pub enum Pending {
    Summary,
    Balances,
    /// Spot: which symbols have working orders.
    OrderSymbols,
    /// Spot: the working orders of the first symbol; the rest follow.
    SymbolOrders { rest: Vec<String> },
    /// Spot: one page of the untriggered stop orders, after the plain ones.
    SpotStops { page: u32 },
    /// Futures: one settlement coin's positions.
    Positions { coin: usize },
    /// Futures: one page of working orders, then (`stops`) of the untriggered stop orders.
    FuturesOrders { page: u32, stops: bool },
    SpotFills { symbols: Vec<InstrumentId>, cur: SpotCursor, range: TimeRange },
    FuturesFills { cur: FuturesCursor, range: TimeRange },
}

/// Where a spot fill walk stands: the symbol, the slice's end, the id to read below, pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotCursor {
    pub idx: usize,
    pub slice_to: i64,
    pub below: Option<i64>,
    pub pages: u32,
}

/// Where a futures fill walk stands: the slice's end, the page in it, pages used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuturesCursor {
    pub slice_to: i64,
    pub page: u32,
    pub pages: u32,
}

fn bad(c: &str) -> ConnectorError {
    ConnectorError::invalid(format!("bad cursor `{c}`"))
}

impl SpotCursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}:{}", self.idx, self.slice_to, self.below.map(|b| b.to_string()).unwrap_or_default(), self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = c.split(':').collect();
        let [idx, to, below, pages] = parts.as_slice() else { return Err(bad(c)) };
        Ok(SpotCursor {
            idx: idx.parse().map_err(|_| bad(c))?,
            slice_to: to.parse().map_err(|_| bad(c))?,
            below: if below.is_empty() { None } else { Some(below.parse().map_err(|_| bad(c))?) },
            pages: pages.parse().map_err(|_| bad(c))?,
        })
    }
}

impl FuturesCursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}", self.slice_to, self.page, self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = c.split(':').collect();
        let [to, page, pages] = parts.as_slice() else { return Err(bad(c)) };
        Ok(FuturesCursor { slice_to: to.parse().map_err(|_| bad(c))?, page: page.parse().map_err(|_| bad(c))?, pages: pages.parse().map_err(|_| bad(c))? })
    }
}

/// A slice ends at `to` (inclusive) and spans at most a week, never before the period.
fn slice_from(to: i64, from: i64) -> i64 {
    (to - SLICE_MS + 1).max(from)
}

fn signed(endpoint: &'static str, path: &'static str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed()
}

fn too_many(book: Book) -> ConnectorError {
    ConnectorError::invalid(format!("this period holds more {} fills than one pull reads. Ask for a shorter period.", book.label()))
}

pub fn encode(book: Book, cmd: &Command, pending: &mut std::collections::HashMap<CorrId, super::Pending>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, symbols, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    let (req, p) = match (book, kind) {
        (Book::Spot, AccountKind::Summary) => (signed("apiKey", "/api/v1/user/api-key", req_id), Pending::Summary),
        (Book::Spot, AccountKind::Balances) => (signed("accounts", "/api/v1/accounts", req_id), Pending::Balances),
        (Book::Spot, AccountKind::OpenOrders) => match cursor.as_deref().filter(|c| !c.is_empty()) {
            None => (signed("activeSymbols", "/api/v1/hf/orders/active/symbols", req_id), Pending::OrderSymbols),
            Some(c) if c.starts_with(STOPS_CURSOR) => {
                let page = c[STOPS_CURSOR.len()..].parse::<u32>().map_err(|_| bad(c))?;
                let h = signed("stopOrders", "/api/v1/stop-order", req_id).query("currentPage", page).query("pageSize", SPOT_STOP_PAGE).weight(8);
                (h, Pending::SpotStops { page })
            }
            Some(c) => {
                let mut list: Vec<String> = c.split(',').map(str::to_string).collect();
                let first = list.remove(0);
                (signed("activeOrders", "/api/v1/hf/orders/active", req_id).query("symbol", first), Pending::SymbolOrders { rest: list })
            }
        },
        (Book::Spot, AccountKind::Fills) => {
            let range = period(range)?;
            if symbols.is_empty() {
                return Err(ConnectorError::invalid("KuCoin answers its spot trade history one pair at a time. Name the pairs to pull (BTC-USDT, ETH-EUR…)"));
            }
            let cur = match cursor {
                Some(c) => SpotCursor::parse(c)?,
                None => SpotCursor { idx: 0, slice_to: range.to.millis() - 1, below: None, pages: 0 },
            };
            if cur.pages >= MAX_PAGES {
                return Err(too_many(book));
            }
            let instrument = symbols.get(cur.idx).ok_or_else(|| ConnectorError::invalid("cursor past the symbol list"))?;
            let mut h = signed("hfFills", "/api/v1/hf/fills", req_id)
                .query("symbol", &instrument.native)
                .query("startAt", slice_from(cur.slice_to, range.from.millis()))
                .query("endAt", cur.slice_to)
                .query("limit", SPOT_FILL_PAGE)
                .weight(2);
            if let Some(b) = cur.below {
                h = h.query("lastId", b);
            }
            (h, Pending::SpotFills { symbols: symbols.clone(), cur, range })
        }
        (Book::Futures, AccountKind::Summary) => (signed("accountOverview", "/api/v1/account-overview", req_id).query("currency", "USDT").weight(5), Pending::Summary),
        (Book::Futures, AccountKind::Positions) => {
            let coin = match cursor {
                Some(c) => c.parse::<usize>().map_err(|_| bad(c))?,
                None => 0,
            };
            let name = SETTLE_COINS.get(coin).ok_or_else(|| bad(cursor.as_deref().unwrap_or_default()))?;
            (signed("positions", "/api/v1/positions", req_id).query("currency", name).weight(2), Pending::Positions { coin })
        }
        // The working orders, then the stop orders not triggered yet, which only their own list
        // holds ("Stop orders that have been triggered can be queried through the general order
        // interface"): cursor `N` is a page of the first, `sN` of the second.
        (Book::Futures, AccountKind::OpenOrders) => {
            let (stops, page) = match cursor.as_deref() {
                Some(c) => match c.strip_prefix('s') {
                    Some(n) => (true, n.parse::<u32>().map_err(|_| bad(c))?),
                    None => (false, c.parse::<u32>().map_err(|_| bad(c))?),
                },
                None => (false, 1),
            };
            let h = if stops {
                signed("stopOrders", "/api/v1/stopOrders", req_id).query("currentPage", page).query("pageSize", FUTURES_PAGE).weight(6)
            } else {
                signed("orders", "/api/v1/orders", req_id).query("status", "active").query("currentPage", page).query("pageSize", FUTURES_PAGE).weight(2)
            };
            (h, Pending::FuturesOrders { page, stops })
        }
        (Book::Futures, AccountKind::Fills) => {
            let range = period(range)?;
            let cur = match cursor {
                Some(c) => FuturesCursor::parse(c)?,
                None => FuturesCursor { slice_to: range.to.millis() - 1, page: 1, pages: 0 },
            };
            if cur.pages >= MAX_PAGES {
                return Err(too_many(book));
            }
            let h = signed("fills", "/api/v1/fills", req_id)
                .query("startAt", slice_from(cur.slice_to, range.from.millis()))
                .query("endAt", cur.slice_to)
                .query("currentPage", cur.page)
                .query("pageSize", FUTURES_PAGE)
                .weight(5);
            (h, Pending::FuturesFills { cur, range })
        }
        (b, k) => return Err(ConnectorError::unsupported(format!("{} has no {k:?}", b.label()))),
    };
    pending.insert(req_id, super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

fn period(range: &Option<TimeRange>) -> Result<TimeRange, ConnectorError> {
    let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
    if range.to.millis() <= range.from.millis() {
        return Err(ConnectorError::invalid("the period ends before it starts"));
    }
    Ok(range)
}

/// What to check when KuCoin refuses the key.
pub fn hint(book: Book, _p: &Pending, e: ConnectorError) -> ConnectorError {
    let fix = match (e.kind, e.provider_code.as_deref()) {
        (ErrorKind::Auth, Some("400004")) => "Check the passphrase: it is the one typed when the key was created, not the account password.",
        (ErrorKind::Auth, Some("400006")) => "The key is bound to IP addresses and this server is not one of them: add its address to the key, or remove the restriction.",
        (ErrorKind::Auth, _) => "Check the key, its secret and its passphrase.",
        (ErrorKind::Permission, _) if book == Book::Futures => "The key needs the General permission, and futures access for a futures account.",
        (ErrorKind::Permission, _) => "The key needs the General permission.",
        _ => return e,
    };
    ConnectorError { message: format!("{}. {fix}", e.message), ..e }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyInfo {
    #[serde(default)]
    permission: String,
    #[serde(default)]
    api_version: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotAccount {
    currency: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    balance: OptDec,
    #[serde(default)]
    available: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesOverview {
    #[serde(default)]
    account_equity: OptDec,
    #[serde(default)]
    currency: String,
}

#[derive(Deserialize)]
struct ActiveSymbols {
    #[serde(default)]
    symbols: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    #[serde(default)]
    id: String,
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    deal_size: OptDec,
    /// The trigger of a stop order.
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    settle_currency: Option<String>,
    #[serde(default)]
    created_at: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    symbol: String,
    #[serde(default)]
    current_qty: OptDec,
    #[serde(default)]
    avg_entry_price: OptDec,
    #[serde(default)]
    unrealised_pnl: OptDec,
    #[serde(default)]
    mark_price: OptDec,
    #[serde(default)]
    settle_currency: String,
    #[serde(default)]
    is_inverse: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Paged<T> {
    #[serde(default)]
    current_page: Option<Int>,
    #[serde(default)]
    total_page: Option<Int>,
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotFills {
    #[serde(default = "Vec::new")]
    items: Vec<FillRow>,
    #[serde(default)]
    last_id: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FillRow {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    trade_id: Option<serde_json::Value>,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: String,
    /// Futures only.
    #[serde(default)]
    settle_currency: Option<String>,
    created_at: Int,
}

impl FillRow {
    /// Spot writes the trade id as a number, futures as a string.
    fn id(&self) -> String {
        match &self.trade_id {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => String::new(),
        }
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
    ConnectorError::decode(format!("kucoin {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("kucoin", "side", raw),
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

/// A futures row of a USDT- or USDC-margined contract (the ones this adapter serves).
fn linear(settle: Option<&str>) -> bool {
    settle.is_some_and(|c| SETTLE_COINS.iter().any(|s| s.eq_ignore_ascii_case(c)))
}

fn summary(detail: String, permissions: Vec<String>, can_trade: Option<bool>) -> AccountSummary {
    AccountSummary { account_type: None, permissions, assets_held: 0, can_trade, open_positions: None, wallet_balance: None, detail: Some(detail) }
}

/// Base coin per lot of a futures contract (one on spot): the contract list just read, else the
/// loaded instruments. Unknown is an error naming the contract, never a guess.
fn lot(book: Book, symbol: &str, sizes: &HashMap<String, Decimal>, ctx: &MapCtx<'_>) -> Result<Decimal, ConnectorError> {
    if book == Book::Spot {
        return Ok(Decimal::ONE);
    }
    sizes
        .get(symbol)
        .copied()
        .or_else(|| ctx.instruments.get(&InstrumentId::new(book.venue(), symbol)).map(|c| c.multiplier))
        .filter(|m| m.is_sign_positive() && !m.is_zero())
        .ok_or_else(|| ConnectorError::decode(format!("KuCoin Futures: the contract size of {symbol} is not in the contract list, so its lots cannot be read in the coin")))
}

fn order(book: Book, o: OrderRow, sizes: &HashMap<String, Decimal>, ctx: &MapCtx<'_>) -> Result<Order, ConnectorError> {
    let symbol = o.symbol.to_ascii_uppercase();
    let lot = lot(book, &symbol, sizes, ctx)?;
    Ok(Order {
        id: o.id,
        instrument: InstrumentId::new(book.venue(), &symbol),
        side: side(&o.side),
        order_type: o.kind,
        qty: Qty(o.size.0.unwrap_or_default() * lot),
        filled_qty: Qty(o.deal_size.0.unwrap_or_default() * lot),
        limit_price: positive(o.price),
        stop_price: positive(o.stop_price),
        placed_at: o.created_at.map(|t| ts_millis(t.0)).transpose()?,
        quote_ccy: o.settle_currency.map(|c| c.to_ascii_uppercase()).filter(|c| !c.is_empty()),
        account: None,
        venue: None,
        class: None,
    })
}

fn fill(book: Book, r: FillRow) -> Result<Fill, ConnectorError> {
    Ok(Fill {
        id: r.id(),
        order_id: Some(r.order_id.clone()).filter(|o| !o.is_empty()),
        instrument: InstrumentId::new(book.venue(), &r.symbol.to_ascii_uppercase()),
        ts: ts_millis(r.created_at.0)?,
        side: side(&r.side),
        qty: Qty(r.size.0.unwrap_or_default()),
        price: Price(r.price.0.unwrap_or_default()),
        fee: Money(r.fee.0.unwrap_or_default()),
        fee_asset: Some(r.fee_currency.to_ascii_uppercase()).filter(|c| !c.is_empty()),
        quote_ccy: r.settle_currency.map(|c| c.to_ascii_uppercase()).filter(|c| !c.is_empty()),
        account: None,
        venue: None,
        class: None,
        multiplier: None,
    })
}

pub fn decode(book: Book, p: Pending, corr: CorrId, body: &[u8], sizes: &HashMap<String, Decimal>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Summary if book == Book::Spot => {
            let k: Data<KeyInfo> = serde_json::from_slice(body).map_err(|e| decode_err("api-key", e))?;
            let perms: Vec<String> = k.data.permission.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
            // `General` reads; `Spot`, `Margin` and `Futures` trade.
            let trades = perms.iter().any(|p| matches!(p.as_str(), "Spot" | "Margin" | "Futures"));
            let version = k.data.api_version.map(|v| format!(" (key version {})", v.0)).unwrap_or_default();
            Ok(vec![env(ctx, corr, Event::AccountSummary(summary(format!("KuCoin key accepted{version}, permissions [{}]", perms.join(", ")), perms, Some(trades))))])
        }
        Pending::Summary => {
            let a: Data<FuturesOverview> = serde_json::from_slice(body).map_err(|e| decode_err("account-overview", e))?;
            // Rounded, not cut: `Decimal` precision formatting truncates.
            let equity = a.data.account_equity.0.unwrap_or_default().round_dp(2);
            let ccy = if a.data.currency.is_empty() { "USDT".to_string() } else { a.data.currency.to_ascii_uppercase() };
            Ok(vec![env(ctx, corr, Event::AccountSummary(summary(format!("KuCoin Futures: {equity:.2} {ccy} of equity"), vec![], None)))])
        }
        Pending::Balances => {
            let page: Data<Vec<SpotAccount>> = serde_json::from_slice(body).map_err(|e| decode_err("accounts", e))?;
            // The funding and trading accounts hold the coins; a margin account's balance
            // stands against its own borrowing and is not read here.
            let mut by_coin: std::collections::BTreeMap<String, (Decimal, Decimal)> = std::collections::BTreeMap::new();
            for a in page.data.iter().filter(|a| matches!(a.kind.as_str(), "main" | "trade" | "trade_hf")) {
                let e = by_coin.entry(a.currency.to_ascii_uppercase()).or_default();
                e.0 += a.balance.0.unwrap_or_default();
                e.1 += a.available.0.unwrap_or_default();
            }
            let held = by_coin
                .into_iter()
                .filter(|(_, (t, _))| t.is_sign_positive() && !t.is_zero())
                .map(|(asset, (total, available))| Balance { asset, total: Money(total), available: Money(available), name: None, fiat: false, account: None })
                .collect();
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)])
        }
        Pending::OrderSymbols => {
            let page: Data<ActiveSymbols> = serde_json::from_slice(body).map_err(|e| decode_err("active symbols", e))?;
            let list: Vec<String> = page.data.symbols.into_iter().filter(|s| !s.is_empty() && !s.contains(',')).collect();
            let next = if list.is_empty() { format!("{STOPS_CURSOR}1") } else { list.join(",") };
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(vec![]), Some(next))])
        }
        Pending::SymbolOrders { rest } => {
            let page: Data<Option<Vec<OrderRow>>> = serde_json::from_slice(body).map_err(|e| decode_err("active orders", e))?;
            let orders = page.data.unwrap_or_default().into_iter().map(|o| order(book, o, sizes, ctx)).collect::<Result<Vec<_>, _>>()?;
            let next = if rest.is_empty() { format!("{STOPS_CURSOR}1") } else { rest.join(",") };
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(orders), Some(next))])
        }
        Pending::SpotStops { page } => {
            let p: Data<Paged<OrderRow>> = serde_json::from_slice(body).map_err(|e| decode_err("stop orders", e))?;
            let total = p.data.total_page.map(|t| t.0).unwrap_or(0);
            let current = p.data.current_page.map(|c| c.0).unwrap_or(i64::from(page));
            let mut orders = Vec::with_capacity(p.data.items.len());
            for o in p.data.items {
                let mut row = order(book, o, sizes, ctx)?;
                row.id = format!("cond:{}", row.id);
                orders.push(row);
            }
            let next = (current < total).then(|| format!("{STOPS_CURSOR}{}", page + 1));
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(orders), next)])
        }
        Pending::Positions { coin } => {
            let page: Data<Option<Vec<PositionRow>>> = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            let mut out = Vec::new();
            for p in page.data.unwrap_or_default() {
                let Some(qty) = p.current_qty.0.filter(|q| !q.is_zero()) else { continue };
                if p.is_inverse {
                    continue;
                }
                let symbol = p.symbol.to_ascii_uppercase();
                let lot = lot(book, &symbol, sizes, ctx)?;
                out.push(Position {
                    instrument: InstrumentId::new(book.venue(), &symbol),
                    // Lots in the coin, signed as the venue writes them (negative short).
                    qty: Qty(qty * lot),
                    avg_price: positive(p.avg_entry_price),
                    unrealized_pnl: p.unrealised_pnl.0.map(Money),
                    currency: Some(p.settle_currency.to_ascii_uppercase()).filter(|c| !c.is_empty()),
                    mark_price: positive(p.mark_price),
                    venue: None,
                    account: None,
                    class: None,
                    // Already in the coin: one unit is one coin, not one contract.
                    multiplier: Some(Decimal::ONE),
                    name: None,
                });
            }
            let next = (coin + 1 < SETTLE_COINS.len()).then(|| (coin + 1).to_string());
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out), next)])
        }
        Pending::FuturesOrders { page, stops } => {
            let p: Data<Paged<OrderRow>> = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            let total = p.data.total_page.map(|t| t.0).unwrap_or(0);
            let current = p.data.current_page.map(|c| c.0).unwrap_or(i64::from(page));
            let orders = p.data.items.into_iter().filter(|o| linear(o.settle_currency.as_deref())).map(|o| order(book, o, sizes, ctx)).collect::<Result<Vec<_>, _>>()?;
            let next = match (current < total, stops) {
                (true, false) => Some((page + 1).to_string()),
                (true, true) => Some(format!("s{}", page + 1)),
                (false, false) => Some("s1".to_string()),
                (false, true) => None,
            };
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(orders), next)])
        }
        Pending::SpotFills { symbols, cur, range } => {
            let page: Data<SpotFills> = serde_json::from_slice(body).map_err(|e| decode_err("hf fills", e))?;
            let n = page.data.items.len();
            let fills = page.data.items.into_iter().filter(|r| !r.symbol.is_empty()).map(|r| fill(book, r)).collect::<Result<Vec<_>, _>>()?;
            let pages = cur.pages + 1;
            let from = range.from.millis();
            let start = slice_from(cur.slice_to, from);
            let next = match page.data.last_id {
                Some(id) if n >= SPOT_FILL_PAGE => Some(SpotCursor { below: Some(id.0), pages, ..cur.clone() }),
                _ if start > from => Some(SpotCursor { idx: cur.idx, slice_to: start - 1, below: None, pages }),
                _ if cur.idx + 1 < symbols.len() => Some(SpotCursor { idx: cur.idx + 1, slice_to: range.to.millis() - 1, below: None, pages }),
                _ => None,
            };
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))])
        }
        Pending::FuturesFills { cur, range } => {
            let page: Data<Paged<FillRow>> = serde_json::from_slice(body).map_err(|e| decode_err("fills", e))?;
            let total = page.data.total_page.map(|t| t.0).unwrap_or(0);
            let rows = page.data.items;
            let fills = rows.into_iter().filter(|r| !r.symbol.is_empty() && linear(r.settle_currency.as_deref())).map(|r| fill(book, r)).collect::<Result<Vec<_>, _>>()?;
            let pages = cur.pages + 1;
            let from = range.from.millis();
            let start = slice_from(cur.slice_to, from);
            let next = if i64::from(cur.page) < total {
                Some(FuturesCursor { page: cur.page + 1, pages, ..cur })
            } else if start > from {
                Some(FuturesCursor { slice_to: start - 1, page: 1, pages })
            } else {
                None
            };
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))])
        }
    }
}
