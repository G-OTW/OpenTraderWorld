//! OANDA's account, read only: the summary, open positions (one row per side), pending
//! orders, and the fill history.
//!
//! Fills are transactions, fetched in two steps: `GET /transactions` answers a list of page
//! URLs, each an id range fetched in turn, and accepts at most 365 days, so a wider period is
//! walked a year at a time. The cursor carries both walks: `<pages read>:<next slice start in
//! ms, or end>:<id ranges still to read>`. Only the id pair is taken from a page URL, and the
//! call is rebuilt against the configured host: a URL a remote service hands back is data,
//! not a place to send a bearer token.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Command, ConnectorError, CorrId, Decimal, Envelope, Event, Fill, Mapped, Money, Order,
    Position, Price, Qty, Side, Timestamp,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{acct, decode_err, quote_ccy, rfc3339, Session, REST, VENUE};

/// Widest window `GET /transactions` accepts.
const MAX_WINDOW_MS: i64 = 365 * 86_400_000;
/// Transactions per page, OANDA's maximum.
const PAGE: u32 = 1000;
/// Hard stop on the page walk, so a mis-specified window cannot loop.
const MAX_PAGES: u32 = 500;

pub enum Pending {
    Summary,
    Positions,
    Orders,
    /// The page index of one slice; `next` is where the following slice starts.
    Index { pages: u32, next: Option<i64> },
    /// One id range; the rest of the walk rides along.
    Range { pages: u32, next: Option<i64>, rest: Vec<(String, String)> },
}

fn get(endpoint: &'static str, path: String, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed()
}

fn put(s: &mut Session, req_id: CorrId, h: HttpRequest, p: Pending) -> Vec<NativeRequest> {
    s.pending.insert((req_id, h.endpoint_id), super::Pending::Account(p));
    vec![NativeRequest::Http(h)]
}

fn bad_cursor(c: &str) -> ConnectorError {
    ConnectorError::invalid(format!("bad cursor `{c}`"))
}

/// Where the fill walk stands: pages read, the next slice's start, the id ranges left.
type Walk = (u32, Option<i64>, Vec<(String, String)>);

/// `<pages>:<next ms | end>:<from-to,from-to,...>`
fn cursor_of(pages: u32, next: Option<i64>, ranges: &[(String, String)]) -> String {
    let next = next.map_or_else(|| "end".to_string(), |n| n.to_string());
    let ranges: Vec<String> = ranges.iter().map(|(a, b)| format!("{a}-{b}")).collect();
    format!("{pages}:{next}:{}", ranges.join(","))
}

fn parse_cursor(c: &str) -> Result<Walk, ConnectorError> {
    let mut parts = c.splitn(3, ':');
    let (Some(p), Some(n), Some(r)) = (parts.next(), parts.next(), parts.next()) else { return Err(bad_cursor(c)) };
    let pages = p.parse::<u32>().map_err(|_| bad_cursor(c))?;
    let next = if n == "end" { None } else { Some(n.parse::<i64>().map_err(|_| bad_cursor(c))?) };
    let mut ranges = Vec::new();
    for pair in r.split(',').filter(|x| !x.is_empty()) {
        let (a, b) = pair.split_once('-').ok_or_else(|| bad_cursor(c))?;
        ranges.push((a.to_string(), b.to_string()));
    }
    Ok((pages, next, ranges))
}

/// The two transaction ids a page URL covers, both digits only.
pub(crate) fn idrange_of(url: &str) -> Option<(String, String)> {
    let query = url.split_once('?')?.1;
    let (mut from, mut to) = (None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("from", v)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => from = Some(v.to_string()),
            Some(("to", v)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => to = Some(v.to_string()),
            _ => {}
        }
    }
    Some((from?, to?))
}

fn ms(t: Timestamp) -> Result<Timestamp, ConnectorError> {
    Timestamp::from_millis(t.millis()).ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))
}

pub fn encode(cmd: &Command, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    match kind {
        AccountKind::Summary => Ok(put(s, req_id, get("summary", acct("/summary"), req_id), Pending::Summary)),
        AccountKind::Positions => Ok(put(s, req_id, get("positions", acct("/openPositions"), req_id), Pending::Positions)),
        AccountKind::OpenOrders => Ok(put(s, req_id, get("orders", acct("/pendingOrders"), req_id), Pending::Orders)),
        AccountKind::Balances => Err(ConnectorError::unsupported("OANDA reports positions, not balances")),
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let (pages, next, mut ranges) = match cursor {
                Some(c) => parse_cursor(c)?,
                None => (0, Some(range.from.millis()), Vec::new()),
            };
            if !ranges.is_empty() {
                if pages >= MAX_PAGES {
                    return Err(ConnectorError::invalid("OANDA split this period into more pages than one pull reads. Ask for a shorter period."));
                }
                let (from, to) = ranges.remove(0);
                let h = get("idrange", acct("/transactions/idrange"), req_id).query("from", from).query("to", to).query("type", "ORDER_FILL");
                return Ok(put(s, req_id, h, Pending::Range { pages: pages + 1, next, rest: ranges }));
            }
            let from = next.ok_or_else(|| ConnectorError::invalid("the fill walk is already over"))?;
            let to_ms = range.to.millis();
            let slice_to = from.saturating_add(MAX_WINDOW_MS).min(to_ms);
            let at = |m: i64| Timestamp::from_millis(m).ok_or_else(|| ConnectorError::invalid(format!("time {m} out of range")));
            let h = get("transactions", acct("/transactions"), req_id)
                .query("from", rfc3339(ms(at(from)?)?)?)
                .query("to", rfc3339(at(slice_to)?)?)
                .query("pageSize", PAGE)
                .query("type", "ORDER_FILL");
            let next = (slice_to < to_ms).then_some(slice_to);
            Ok(put(s, req_id, h, Pending::Index { pages, next }))
        }
    }
}

#[derive(Deserialize)]
struct SummaryAnswer {
    account: SummaryRow,
}

#[derive(Deserialize)]
struct SummaryRow {
    #[serde(default)]
    id: String,
    #[serde(default)]
    alias: String,
    #[serde(default)]
    balance: String,
    #[serde(default)]
    currency: String,
    #[serde(rename = "openTradeCount", default)]
    open_trades: i64,
    #[serde(rename = "pendingOrderCount", default)]
    pending_orders: i64,
}

#[derive(Deserialize)]
struct Leg {
    #[serde(default)]
    units: OptDec,
    #[serde(rename = "averagePrice", default)]
    average_price: OptDec,
}

#[derive(Deserialize)]
struct PositionRow {
    #[serde(default)]
    instrument: String,
    #[serde(default)]
    long: Option<Leg>,
    #[serde(default)]
    short: Option<Leg>,
}

#[derive(Deserialize)]
struct PositionsAnswer {
    #[serde(default)]
    positions: Vec<PositionRow>,
}

#[derive(Deserialize)]
struct OrderRow {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    instrument: String,
    #[serde(default)]
    units: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(rename = "createTime", default)]
    create_time: Option<String>,
}

#[derive(Deserialize)]
struct OrdersAnswer {
    #[serde(default)]
    orders: Vec<OrderRow>,
}

#[derive(Deserialize)]
struct IndexAnswer {
    #[serde(default)]
    pages: Vec<String>,
}

#[derive(Deserialize)]
struct TransactionRow {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(rename = "orderID", default)]
    order_id: String,
    #[serde(rename = "accountID", default)]
    account_id: String,
    #[serde(default)]
    instrument: String,
    #[serde(default)]
    units: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(rename = "fullVWAP", default)]
    full_vwap: OptDec,
    #[serde(default)]
    commission: OptDec,
    #[serde(rename = "guaranteedExecutionFee", default)]
    guaranteed_fee: OptDec,
    #[serde(default)]
    time: String,
}

#[derive(Deserialize)]
struct TransactionsAnswer {
    #[serde(default)]
    transactions: Vec<TransactionRow>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

/// OANDA signs units: a negative amount is a sell.
fn side_of(units: Decimal) -> Mapped<Side> {
    Mapped::Known(if units.is_sign_negative() { Side::Sell } else { Side::Buy })
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

fn id(symbol: &str) -> otw_connector_model::InstrumentId {
    otw_connector_model::InstrumentId::new(VENUE, &symbol.to_ascii_uppercase())
}

/// The walk after this answer: the ranges still to read, else the next slice, else over.
fn walk_on(pages: u32, next: Option<i64>, rest: &[(String, String)]) -> Option<String> {
    (!rest.is_empty() || next.is_some()).then(|| cursor_of(pages, next, rest))
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Summary => {
            let a = serde_json::from_slice::<SummaryAnswer>(body).map_err(|e| decode_err("summary", e))?.account;
            let alias = if a.alias.is_empty() { String::new() } else { format!(" ({})", a.alias) };
            let detail = format!(
                "account {}{alias}, balance {} {}, {} open trade(s) and {} pending order(s)",
                a.id, a.balance, a.currency, a.open_trades, a.pending_orders
            );
            Ok(vec![env(
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
        Pending::Positions => {
            let a: PositionsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            let mut out = Vec::new();
            for p in a.positions.into_iter().filter(|p| !p.instrument.is_empty()) {
                // One row per side: OANDA may hold both on one instrument (hedging accounts).
                for (leg, short) in [(p.long, false), (p.short, true)] {
                    let Some(leg) = leg else { continue };
                    let units = leg.units.0.unwrap_or_default().abs();
                    if units.is_zero() {
                        continue;
                    }
                    out.push(Position {
                        instrument: id(&p.instrument),
                        qty: Qty(if short { -units } else { units }),
                        avg_price: positive(leg.average_price),
                        unrealized_pnl: None,
                        currency: quote_ccy(&p.instrument),
                        mark_price: None,
                        venue: Some(VENUE.into()),
                        account: None,
                        class: None,
                        multiplier: None,
                        name: None,
                    });
                }
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out), None)])
        }
        Pending::Orders => {
            let a: OrdersAnswer = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            let mut out = Vec::new();
            // A take-profit or stop-loss hangs off a trade and names no instrument: there is
            // nothing to draw it on, so it is left out rather than filed against a guess.
            for o in a.orders.into_iter().filter(|o| !o.instrument.is_empty()) {
                let units = o.units.0.unwrap_or_default();
                // One `price`: a limit on a LIMIT order, a trigger on a STOP or a
                // market-if-touched.
                let stops = o.kind.contains("STOP") || o.kind.contains("MARKET_IF_TOUCHED");
                let price = positive(o.price);
                out.push(Order {
                    id: o.id,
                    instrument: id(&o.instrument),
                    side: side_of(units),
                    order_type: o.kind,
                    qty: Qty(units.abs()),
                    filled_qty: Qty(Decimal::ZERO),
                    limit_price: if stops { None } else { price },
                    stop_price: if stops { price } else { None },
                    placed_at: o.create_time.as_deref().and_then(|t| ts_rfc3339(t).ok()),
                    quote_ccy: quote_ccy(&o.instrument),
                    account: None,
                    venue: Some(VENUE.into()),
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Index { pages, next } => {
            let a: IndexAnswer = serde_json::from_slice(body).map_err(|e| decode_err("transactions", e))?;
            let ranges: Vec<(String, String)> = a.pages.iter().filter_map(|u| idrange_of(u)).collect();
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(vec![]), walk_on(pages, next, &ranges))])
        }
        Pending::Range { pages, next, rest } => {
            let a: TransactionsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("transactions", e))?;
            let mut out = Vec::new();
            for t in a.transactions.into_iter().filter(|t| t.kind == "ORDER_FILL" && !t.instrument.is_empty()) {
                let units = t.units.0.unwrap_or_default();
                // `price` is deprecated for the volume-weighted price of the whole fill.
                let price = positive(t.full_vwap).or_else(|| positive(t.price)).unwrap_or(Price(Decimal::ZERO));
                // Both are charges, signed as OANDA pleases; a cost is filed positive. Billed
                // in the account's currency, not the instrument's.
                let fee = t.commission.0.unwrap_or_default().abs() + t.guaranteed_fee.0.unwrap_or_default().abs();
                out.push(Fill {
                    quote_ccy: quote_ccy(&t.instrument),
                    instrument: id(&t.instrument),
                    id: t.id,
                    order_id: Some(t.order_id),
                    ts: ts_rfc3339(&t.time)?,
                    side: side_of(units),
                    qty: Qty(units.abs()),
                    price,
                    fee: Money(fee),
                    fee_asset: None,
                    account: Some(t.account_id).filter(|a| !a.is_empty()),
                    venue: Some(VENUE.into()),
                    class: None,
                    multiplier: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(out), walk_on(pages, next, &rest))])
        }
    }
}
