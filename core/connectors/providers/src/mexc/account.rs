//! MEXC accounts, read only, one book per adapter.
//!
//!   - Spot: balances and the key's trade permission from `/api/v3/account`, open orders of
//!     every pair (no symbol, as ccxt asks them), fills per named pair. `myTrades` answers
//!     one pair at a time, the last month only, at most 100 rows and with no cursor: a
//!     slice whose page comes back full is asked again at half the width before anything of
//!     it is kept, down to a second; a quiet slice lets the next one widen again, up to a
//!     week.
//!   - Futures (contracts settled in their quote coin): the futures assets, open positions,
//!     open orders by page, fills per named contract over 30-day slices (the venue allows
//!     90), each paged.
//!
//! A broker account names spot pairs (`BTCUSDT`) and contracts (`BTC_USDT`) in one list;
//! each book reads the names spelled its way and leaves the others to the other book.
//! Futures quantities are contracts; the instrument list carries what one is worth.

use std::collections::HashMap;

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill, InstrumentId,
    Mapped, Money, Order, Position, Price, Qty, Side, TimeRange,
};
use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Book, Data, FUT, REST};

const SPOT_FILL_PAGE: usize = 100;
const FUT_PAGE: usize = 100;
/// Spot fill slices, widest and narrowest, in milliseconds.
const MAX_SPAN: i64 = 7 * 86_400_000;
const MIN_SPAN: i64 = 1_000;
/// Futures fill slices: the venue takes 90 days per call.
const FUT_SPAN: i64 = 30 * 86_400_000;
const MAX_PAGES: u32 = 2_000;

pub enum Pending {
    Summary,
    Balances,
    SpotOrders,
    Positions,
    FuturesOrders { page: u32 },
    Fills { symbols: Vec<InstrumentId>, cur: Cursor, range: TimeRange },
}

/// Where a fill walk stands: the symbol, the slice's end (inclusive, ms), its width (spot)
/// or the page in it (futures), pages used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub idx: usize,
    pub slice_to: i64,
    pub span: i64,
    pub page: u32,
    pub pages: u32,
}

fn bad(c: &str) -> ConnectorError {
    ConnectorError::invalid(format!("bad cursor `{c}`"))
}

impl Cursor {
    fn first(range: &TimeRange, book: Book) -> Self {
        Cursor { idx: 0, slice_to: range.to.millis() - 1, span: if book == Book::Spot { MAX_SPAN } else { FUT_SPAN }, page: 1, pages: 0 }
    }

    fn encode(&self) -> String {
        format!("{}:{}:{}:{}:{}", self.idx, self.slice_to, self.span, self.page, self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let parts: Vec<&str> = c.split(':').collect();
        let [idx, to, span, page, pages] = parts.as_slice() else { return Err(bad(c)) };
        Ok(Cursor {
            idx: idx.parse().map_err(|_| bad(c))?,
            slice_to: to.parse().map_err(|_| bad(c))?,
            span: span.parse().map_err(|_| bad(c))?,
            page: page.parse().map_err(|_| bad(c))?,
            pages: pages.parse().map_err(|_| bad(c))?,
        })
    }

    fn slice_from(&self, from_ms: i64) -> i64 {
        (self.slice_to - self.span + 1).max(from_ms)
    }
}

fn signed(endpoint: &'static str, base: &'static str, path: &str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, base, path, req_id).signed()
}

fn period(range: &Option<TimeRange>) -> Result<TimeRange, ConnectorError> {
    let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
    if range.to.millis() <= range.from.millis() {
        return Err(ConnectorError::invalid("the period ends before it starts"));
    }
    Ok(range)
}

fn fills_request(book: Book, req_id: CorrId, symbols: &[InstrumentId], cur: &Cursor, range: &TimeRange) -> Result<HttpRequest, ConnectorError> {
    if cur.pages >= MAX_PAGES {
        return Err(ConnectorError::invalid(format!("this period holds more {} fills than one pull reads. Ask for a shorter period.", book.label())));
    }
    let symbol = symbols.get(cur.idx).ok_or_else(|| ConnectorError::invalid("cursor past the symbol list"))?;
    let from = cur.slice_from(range.from.millis());
    Ok(match book {
        Book::Spot => signed("myTrades", REST, "/api/v3/myTrades", req_id)
            .query("symbol", &symbol.native)
            .query("startTime", from)
            .query("endTime", cur.slice_to)
            .query("limit", SPOT_FILL_PAGE),
        Book::Futures => signed("orderDeals", FUT, "/api/v1/private/order/list/order_deals", req_id)
            .query("symbol", &symbol.native)
            .query("start_time", from)
            .query("end_time", cur.slice_to)
            .query("page_num", cur.page)
            .query("page_size", FUT_PAGE),
    })
}

pub fn encode(book: Book, cmd: &Command, pending: &mut HashMap<CorrId, super::Pending>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, symbols, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    let (req, p) = match (book, kind) {
        (Book::Spot, AccountKind::Summary) => (signed("account", REST, "/api/v3/account", req_id), Pending::Summary),
        (Book::Spot, AccountKind::Balances) => (signed("account", REST, "/api/v3/account", req_id), Pending::Balances),
        (Book::Spot, AccountKind::OpenOrders) => (signed("openOrders", REST, "/api/v3/openOrders", req_id), Pending::SpotOrders),
        (Book::Futures, AccountKind::Summary) => (signed("assets", FUT, "/api/v1/private/account/assets", req_id), Pending::Summary),
        (Book::Futures, AccountKind::Positions) => (signed("openPositions", FUT, "/api/v1/private/position/open_positions", req_id), Pending::Positions),
        (Book::Futures, AccountKind::OpenOrders) => {
            let page = match cursor {
                Some(c) => c.parse::<u32>().map_err(|_| bad(c))?,
                None => 1,
            };
            let h = signed("openOrders", FUT, "/api/v1/private/order/list/open_orders", req_id).query("page_num", page).query("page_size", FUT_PAGE);
            (h, Pending::FuturesOrders { page })
        }
        (_, AccountKind::Fills) => {
            let range = period(range)?;
            // The names spelled this book's way; the other book reads the rest.
            let mine: Vec<InstrumentId> = symbols.iter().filter(|s| book.owns(&s.native)).cloned().collect();
            if symbols.is_empty() {
                return Err(ConnectorError::invalid(format!(
                    "{} answers its trade history one instrument at a time. Name the pairs (BTCUSDT) or contracts (BTC_USDT) to pull.",
                    book.label()
                )));
            }
            if mine.is_empty() {
                return Ok(vec![]);
            }
            let cur = match cursor {
                Some(c) => Cursor::parse(c)?,
                None => Cursor::first(&range, book),
            };
            (fills_request(book, req_id, &mine, &cur, &range)?, Pending::Fills { symbols: mine, cur, range })
        }
        (b, k) => return Err(ConnectorError::unsupported(format!("{} has no {k:?}", b.label()))),
    };
    pending.insert(req_id, super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

/// What to check when MEXC refuses the key.
pub fn hint(book: Book, p: &Pending, e: ConnectorError) -> ConnectorError {
    let code = e.provider_code.as_deref().unwrap_or_default();
    let fix = match (e.kind, code, book) {
        (ErrorKind::Auth, "700006" | "406", _) => "The key is bound to IP addresses and this server is not one of them: add its address to the key, or remove the restriction.",
        (ErrorKind::Auth, "700003", _) | (ErrorKind::Auth, "513", _) => "This server's clock is off from MEXC's: set it right (NTP).",
        (ErrorKind::Auth, _, _) => "Check the key and its secret. A key without a bound IP address expires after 90 days.",
        (ErrorKind::Permission, _, Book::Spot) => "The key needs the spot Account read and Order read permissions.",
        (ErrorKind::Permission, _, Book::Futures) => "The key needs the futures Order and Position read permissions.",
        // Measured: the contract API answers an unknown key with code 500.
        (ErrorKind::Transient, "500", Book::Futures) if !matches!(p, Pending::Fills { .. }) => {
            "MEXC's futures API answers an unknown or mistyped key this way too: check the key if it persists."
        }
        _ => return e,
    };
    ConnectorError { message: format!("{}. {fix}", e.message), ..e }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotAccount {
    #[serde(default)]
    can_trade: Option<bool>,
    #[serde(default)]
    account_type: Option<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    balances: Vec<SpotBalance>,
}

#[derive(Deserialize)]
struct SpotBalance {
    asset: String,
    #[serde(default)]
    free: OptDec,
    #[serde(default)]
    locked: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotOrder {
    symbol: String,
    #[serde(default)]
    order_id: Option<serde_json::Value>,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    orig_qty: OptDec,
    #[serde(default)]
    executed_qty: OptDec,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotTrade {
    symbol: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    order_id: Option<serde_json::Value>,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    qty: OptDec,
    #[serde(default)]
    commission: OptDec,
    #[serde(default)]
    commission_asset: Option<String>,
    time: Int,
    #[serde(default)]
    is_buyer: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutAsset {
    currency: String,
    #[serde(default)]
    equity: OptDec,
    #[serde(default)]
    unrealized: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutPosition {
    symbol: String,
    /// Contracts held.
    #[serde(default)]
    hold_vol: OptDec,
    /// 1 long, 2 short.
    #[serde(default)]
    position_type: Option<Int>,
    #[serde(default)]
    hold_avg_price: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutOrder {
    symbol: String,
    #[serde(default)]
    order_id: Option<serde_json::Value>,
    #[serde(default)]
    price: OptDec,
    /// Contracts.
    #[serde(default)]
    vol: OptDec,
    #[serde(default)]
    deal_vol: OptDec,
    /// 1 open long, 2 close short, 3 open short, 4 close long.
    #[serde(default)]
    side: Option<Int>,
    /// 1 limit, 2 post only, 3 IOC, 4 FOK, 5 market, 6 market to limit.
    #[serde(default)]
    order_type: Option<Int>,
    #[serde(default)]
    create_time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutDeal {
    symbol: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    order_id: Option<serde_json::Value>,
    #[serde(default)]
    side: Option<Int>,
    /// Contracts.
    #[serde(default)]
    vol: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: Option<String>,
    timestamp: Int,
}

/// An id MEXC writes as a number on one endpoint and a string on another.
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
    ConnectorError::decode(format!("mexc {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_uppercase().as_str() {
        "BUY" => Mapped::Known(Side::Buy),
        "SELL" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("mexc", "side", raw),
    }
}

/// A futures side: opening a long and closing a short buy, the other two sell.
fn fut_side(v: Option<Int>) -> Mapped<Side> {
    match v.map(|v| v.0) {
        Some(1 | 2) => Mapped::Known(Side::Buy),
        Some(3 | 4) => Mapped::Known(Side::Sell),
        other => otw_connector_sdk::enums::unknown("mexc", "futures side", &other.map(|v| v.to_string()).unwrap_or_default()),
    }
}

fn fut_order_type(v: Option<Int>) -> String {
    match v.map(|v| v.0) {
        Some(1) => "limit",
        Some(2) => "post only",
        Some(3) => "ioc",
        Some(4) => "fok",
        Some(5) => "market",
        Some(6) => "market to limit",
        _ => "",
    }
    .to_string()
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

/// The quote of a contract name (`BTC_USDT`): what it settles in.
fn quote_of(contract: &str) -> Option<String> {
    contract.rsplit_once('_').map(|(_, q)| q.to_ascii_uppercase()).filter(|q| !q.is_empty())
}

/// The next step of a spot fill walk, from how full this page came back. `keep`: whether
/// this page's rows are kept (a full page of a slice wider than a second is not: the slice
/// is asked again at half the width).
fn next_spot(cur: Cursor, n: usize, from_ms: i64, symbols: usize) -> Result<(Option<Cursor>, bool), ConnectorError> {
    let pages = cur.pages + 1;
    if n >= SPOT_FILL_PAGE {
        if cur.span > MIN_SPAN {
            return Ok((Some(Cursor { span: (cur.span / 2).max(MIN_SPAN), pages, ..cur }), false));
        }
        // MEXC pages no further than this: more fills than that in one second would be lost.
        return Err(ConnectorError::invalid(format!(
            "MEXC answers {SPOT_FILL_PAGE} spot fills per call and had that many in one second; this period cannot be read over the API. Export it from the MEXC website."
        )));
    }
    let start = cur.slice_from(from_ms);
    if start > from_ms {
        let span = if n < SPOT_FILL_PAGE / 2 { (cur.span * 2).min(MAX_SPAN) } else { cur.span };
        return Ok((Some(Cursor { slice_to: start - 1, span, pages, ..cur }), true));
    }
    Ok((next_symbol(cur, pages, symbols, MAX_SPAN), true))
}

fn next_symbol(cur: Cursor, pages: u32, symbols: usize, span: i64) -> Option<Cursor> {
    (cur.idx + 1 < symbols).then_some(Cursor { idx: cur.idx + 1, slice_to: i64::MAX, span, page: 1, pages })
}

pub fn decode(book: Book, p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let venue = book.venue();
    let evs = match p {
        Pending::Summary if book == Book::Spot => {
            let a: SpotAccount = serde_json::from_slice(body).map_err(|e| decode_err("account", e))?;
            let held = a.balances.iter().filter(|b| b.free.0.unwrap_or_default() + b.locked.0.unwrap_or_default() > Decimal::ZERO).count();
            vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: a.account_type,
                    permissions: a.permissions,
                    assets_held: u32::try_from(held).unwrap_or(u32::MAX),
                    can_trade: a.can_trade,
                    open_positions: None,
                    wallet_balance: None,
                    detail: None,
                }),
            )]
        }
        Pending::Summary => {
            let d: Data<Vec<FutAsset>> = serde_json::from_slice(body).map_err(|e| decode_err("assets", e))?;
            let main = d.data.iter().find(|a| a.currency.eq_ignore_ascii_case("USDT"));
            let detail = match main {
                // Rounded, not cut: `Decimal` precision formatting truncates.
                Some(a) => format!(
                    "MEXC futures: {:.2} USDT equity, {:.2} unrealised",
                    a.equity.0.unwrap_or_default().round_dp(2),
                    a.unrealized.0.unwrap_or_default().round_dp(2)
                ),
                None => format!("MEXC futures: {} currency account(s)", d.data.len()),
            };
            vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary { account_type: None, permissions: vec![], assets_held: 0, can_trade: None, open_positions: None, wallet_balance: None, detail: Some(detail) }),
            )]
        }
        Pending::Balances => {
            let a: SpotAccount = serde_json::from_slice(body).map_err(|e| decode_err("account", e))?;
            let out = a
                .balances
                .into_iter()
                .filter_map(|b| {
                    let free = b.free.0.unwrap_or_default();
                    let total = free + b.locked.0.unwrap_or_default();
                    (total > Decimal::ZERO).then(|| Balance { asset: b.asset.to_ascii_uppercase(), total: Money(total), available: Money(free.min(total)), name: None, fiat: false, account: None })
                })
                .collect();
            vec![chunk(ctx, corr, AccountItems::Balances(out), None)]
        }
        Pending::SpotOrders => {
            let rows: Vec<SpotOrder> = serde_json::from_slice(body).map_err(|e| decode_err("open orders", e))?;
            let mut orders = Vec::with_capacity(rows.len());
            for o in rows {
                orders.push(Order {
                    id: id_of(&o.order_id),
                    instrument: InstrumentId::new(venue, &o.symbol.to_ascii_uppercase()),
                    side: side(&o.side),
                    order_type: o.kind.to_ascii_lowercase(),
                    qty: Qty(o.orig_qty.0.unwrap_or_default()),
                    filled_qty: Qty(o.executed_qty.0.unwrap_or_default()),
                    limit_price: positive(o.price),
                    stop_price: positive(o.stop_price),
                    placed_at: o.time.map(|t| ts_millis(t.0)).transpose()?,
                    quote_ccy: None,
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            vec![chunk(ctx, corr, AccountItems::Orders(orders), None)]
        }
        Pending::Positions => {
            let d: Data<Vec<FutPosition>> = serde_json::from_slice(body).map_err(|e| decode_err("open positions", e))?;
            let out = d
                .data
                .into_iter()
                .filter_map(|p| {
                    let held = p.hold_vol.0.filter(|v| !v.is_zero())?;
                    let qty = match p.position_type.map(|t| t.0) {
                        Some(1) => held.abs(),
                        Some(2) => -held.abs(),
                        other => {
                            tracing::warn!("mexc position {}: position type {other:?}, left out", p.symbol);
                            return None;
                        }
                    };
                    Some(Position {
                        instrument: InstrumentId::new(venue, &p.symbol.to_ascii_uppercase()),
                        qty: Qty(qty),
                        avg_price: positive(p.hold_avg_price),
                        unrealized_pnl: None,
                        currency: quote_of(&p.symbol),
                        mark_price: None,
                        venue: None,
                        account: None,
                        class: None,
                        multiplier: None,
                        name: None,
                    })
                })
                .collect();
            vec![chunk(ctx, corr, AccountItems::Positions(out), None)]
        }
        Pending::FuturesOrders { page } => {
            let d: Data<Vec<FutOrder>> = serde_json::from_slice(body).map_err(|e| decode_err("open orders", e))?;
            let n = d.data.len();
            let mut orders = Vec::with_capacity(n);
            for o in d.data {
                orders.push(Order {
                    id: id_of(&o.order_id),
                    instrument: InstrumentId::new(venue, &o.symbol.to_ascii_uppercase()),
                    side: fut_side(o.side),
                    order_type: fut_order_type(o.order_type),
                    qty: Qty(o.vol.0.unwrap_or_default()),
                    filled_qty: Qty(o.deal_vol.0.unwrap_or_default()),
                    limit_price: positive(o.price),
                    stop_price: None,
                    placed_at: o.create_time.map(|t| ts_millis(t.0)).transpose()?,
                    quote_ccy: quote_of(&o.symbol),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            vec![chunk(ctx, corr, AccountItems::Orders(orders), (n >= FUT_PAGE).then(|| (page + 1).to_string()))]
        }
        Pending::Fills { symbols, cur, range } => {
            let from_ms = range.from.millis();
            let mut fills = Vec::new();
            let (next, keep) = match book {
                Book::Spot => {
                    let rows: Vec<SpotTrade> = serde_json::from_slice(body).map_err(|e| decode_err("my trades", e))?;
                    let n = rows.len();
                    for t in rows {
                        let symbol = t.symbol.to_ascii_uppercase();
                        fills.push(Fill {
                            id: id_of(&t.id),
                            order_id: Some(id_of(&t.order_id)).filter(|o| !o.is_empty()),
                            instrument: InstrumentId::new(venue, &symbol),
                            ts: ts_millis(t.time.0)?,
                            side: Mapped::Known(if t.is_buyer { Side::Buy } else { Side::Sell }),
                            qty: Qty(t.qty.0.unwrap_or_default()),
                            price: Price(t.price.0.unwrap_or_default()),
                            fee: Money(t.commission.0.unwrap_or_default()),
                            fee_asset: t.commission_asset.map(|a| a.to_ascii_uppercase()).filter(|a| !a.is_empty()),
                            // The row names no quote; the instrument list does.
                            quote_ccy: None,
                            account: None,
                            venue: None,
                            class: None,
                            multiplier: None,
                        });
                    }
                    next_spot(cur, n, from_ms, symbols.len())?
                }
                Book::Futures => {
                    let d: Data<Vec<FutDeal>> = serde_json::from_slice(body).map_err(|e| decode_err("order deals", e))?;
                    let n = d.data.len();
                    for t in d.data {
                        fills.push(Fill {
                            id: id_of(&t.id),
                            order_id: Some(id_of(&t.order_id)).filter(|o| !o.is_empty()),
                            instrument: InstrumentId::new(venue, &t.symbol.to_ascii_uppercase()),
                            ts: ts_millis(t.timestamp.0)?,
                            side: fut_side(t.side),
                            qty: Qty(t.vol.0.unwrap_or_default()),
                            price: Price(t.price.0.unwrap_or_default()),
                            fee: Money(t.fee.0.unwrap_or_default()),
                            fee_asset: t.fee_currency.map(|a| a.to_ascii_uppercase()).filter(|a| !a.is_empty()),
                            quote_ccy: quote_of(&t.symbol),
                            account: None,
                            venue: None,
                            class: None,
                            multiplier: None,
                        });
                    }
                    let pages = cur.pages + 1;
                    let start = cur.slice_from(from_ms);
                    let next = if n >= FUT_PAGE {
                        Some(Cursor { page: cur.page + 1, pages, ..cur })
                    } else if start > from_ms {
                        Some(Cursor { slice_to: start - 1, page: 1, pages, ..cur })
                    } else {
                        next_symbol(cur, pages, symbols.len(), FUT_SPAN)
                    };
                    (next, true)
                }
            };
            if !keep {
                fills.clear();
            }
            // A new symbol starts again from the period's end.
            let next = next.map(|c| if c.slice_to == i64::MAX { Cursor { slice_to: range.to.millis() - 1, ..c } } else { c });
            vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))]
        }
    };
    Ok(evs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_full_spot_page_halves_the_slice_and_a_quiet_one_widens_the_next() {
        let week = Cursor { idx: 0, slice_to: 10_000_000_000, span: MAX_SPAN, page: 1, pages: 0 };
        assert_eq!(next_spot(week, SPOT_FILL_PAGE, 0, 1).unwrap(), (Some(Cursor { span: MAX_SPAN / 2, pages: 1, ..week }), false));
        // At a second, a full page cannot be read further.
        let second = Cursor { span: MIN_SPAN, ..week };
        assert!(next_spot(second, SPOT_FILL_PAGE, 0, 1).unwrap_err().message.contains("Export"));
        // A quiet slice moves on below itself, twice as wide.
        let half = Cursor { span: MAX_SPAN / 2, ..week };
        assert_eq!(next_spot(half, 3, 0, 1).unwrap(), (Some(Cursor { slice_to: 10_000_000_000 - MAX_SPAN / 2, span: MAX_SPAN, pages: 1, ..half }), true));
        // The slice that reaches the period's start moves to the next symbol, or ends.
        assert_eq!(next_spot(week, 3, 10_000_000_000 - 10, 1).unwrap(), (None, true));
        assert_eq!(next_spot(week, 3, 10_000_000_000 - 10, 2).unwrap().0.map(|c| c.idx), Some(1));
        assert_eq!(Cursor::parse(&week.encode()).unwrap(), week);
    }
}
