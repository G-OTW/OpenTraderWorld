//! TradeStation's brokerage account, read only: balances (for the connection test), fills,
//! positions and working orders.
//!
//! There is no executions endpoint: a fill is an executed leg of a closed order from
//! `historicalorders`, identified by the order id and the leg's place in the order (stable,
//! the broker returns legs in order). The archive is 90 days and the endpoint takes a `since`
//! date and no end, so the period's end filters here. A refusal for the account arrives
//! inside a 200 (`Errors`) and is refused, never filed as an empty answer.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill, InstrumentId, Mapped,
    Money, Order, Position, Price, Qty, Side, TimeRange,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{decode_err, Session, ACCOUNT, REST, VENUE};

/// Orders per page, the endpoint's ceiling.
const PAGE: u32 = 600;
/// Hard stop on the page walk.
const MAX_PAGES: u32 = 200;

pub enum Pending {
    Summary,
    Positions,
    Orders,
    Fills { pages: u32, range: TimeRange },
}

fn path(rest: &str) -> String {
    format!("/v3/brokerage/accounts/{ACCOUNT}{rest}")
}

fn put(s: &mut Session, req_id: CorrId, h: HttpRequest, p: Pending) -> Vec<NativeRequest> {
    s.pending.insert((req_id, h.endpoint_id), super::Pending::Account(p));
    vec![NativeRequest::Http(h)]
}

pub fn encode(cmd: &Command, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let req_id = *req_id;
    let get = |endpoint: &'static str, p: String| HttpRequest::get(endpoint, REST, p, req_id).signed();
    match kind {
        AccountKind::Summary => Ok(put(s, req_id, get("balances", path("/balances")), Pending::Summary)),
        AccountKind::Positions => Ok(put(s, req_id, get("positions", path("/positions")), Pending::Positions)),
        AccountKind::OpenOrders => Ok(put(s, req_id, get("orders", path("/orders")), Pending::Orders)),
        AccountKind::Balances => Err(ConnectorError::unsupported("TradeStation is read through its positions, not balances")),
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            // Cursor `<pages>:<next token>`.
            let (pages, token) = match cursor {
                Some(c) => {
                    let (p, t) = c.split_once(':').ok_or_else(|| ConnectorError::invalid(format!("bad cursor `{c}`")))?;
                    (p.parse::<u32>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?, Some(t.to_string()))
                }
                None => (0, None),
            };
            if pages >= MAX_PAGES {
                return Err(ConnectorError::invalid("this period holds more TradeStation orders than one pull reads. Ask for a shorter period."));
            }
            let since = range.from.to_offset_datetime().map(|d| d.date().to_string()).ok_or_else(|| ConnectorError::invalid("period start out of range"))?;
            let mut h = get("fills", path("/historicalorders")).query("since", since).query("pageSize", PAGE);
            if let Some(t) = token {
                h = h.query("nextToken", t);
            }
            Ok(put(s, req_id, h, Pending::Fills { pages: pages + 1, range }))
        }
    }
}

#[derive(Deserialize)]
struct AccountError {
    #[serde(rename = "AccountID", default)]
    account_id: String,
    #[serde(rename = "Error", default)]
    error: String,
    #[serde(rename = "Message", default)]
    message: String,
}

#[derive(Deserialize)]
struct Leg {
    #[serde(rename = "Symbol", default)]
    symbol: String,
    #[serde(rename = "BuyOrSell", default)]
    buy_or_sell: String,
    #[serde(rename = "ExecQuantity", default)]
    exec_quantity: OptDec,
    #[serde(rename = "QuantityOrdered", default)]
    quantity_ordered: OptDec,
    #[serde(rename = "ExecutionPrice", default)]
    execution_price: OptDec,
}

#[derive(Deserialize)]
struct OrderRow {
    #[serde(rename = "OrderID", default)]
    order_id: String,
    #[serde(rename = "OrderType", default)]
    order_type: String,
    #[serde(rename = "Currency", default)]
    currency: String,
    #[serde(rename = "CommissionFee", default)]
    commission_fee: OptDec,
    #[serde(rename = "LimitPrice", default)]
    limit_price: OptDec,
    #[serde(rename = "StopPrice", default)]
    stop_price: OptDec,
    #[serde(rename = "OpenedDateTime", default)]
    opened: Option<String>,
    #[serde(rename = "ClosedDateTime", default)]
    closed: Option<String>,
    #[serde(rename = "Legs", default)]
    legs: Vec<Leg>,
}

#[derive(Deserialize)]
struct OrdersAnswer {
    #[serde(rename = "Orders", default)]
    orders: Vec<OrderRow>,
    #[serde(rename = "NextToken", default)]
    next_token: Option<String>,
    #[serde(rename = "Errors", default)]
    errors: Vec<AccountError>,
}

#[derive(Deserialize)]
struct PositionRow {
    #[serde(rename = "Symbol", default)]
    symbol: String,
    #[serde(rename = "Quantity", default)]
    quantity: OptDec,
    #[serde(rename = "LongShort", default)]
    long_short: String,
    #[serde(rename = "AveragePrice", default)]
    average_price: OptDec,
    #[serde(rename = "UnrealizedProfitLoss", default)]
    unrealized: OptDec,
}

#[derive(Deserialize)]
struct PositionsAnswer {
    #[serde(rename = "Positions", default)]
    positions: Vec<PositionRow>,
    #[serde(rename = "Errors", default)]
    errors: Vec<AccountError>,
}

#[derive(Deserialize)]
struct BalanceRow {
    #[serde(rename = "AccountID", default)]
    account_id: String,
    #[serde(rename = "AccountType", default)]
    account_type: String,
    #[serde(rename = "Equity", default)]
    equity: String,
}

#[derive(Deserialize)]
struct BalancesAnswer {
    #[serde(rename = "Balances", default)]
    balances: Vec<BalanceRow>,
    #[serde(rename = "Errors", default)]
    errors: Vec<AccountError>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

/// A per-account failure inside a 200 is a refusal.
fn refused(errors: &[AccountError]) -> Result<(), ConnectorError> {
    match errors.first() {
        None => Ok(()),
        Some(e) => Err(ConnectorError::new(ErrorKind::Permission, format!("TradeStation refused account {}: {} {}", e.account_id, e.error, e.message))),
    }
}

/// `Sell`, `SellShort`, `SellToOpen`, `SellToClose` sell; the buys buy.
fn side(raw: &str) -> Mapped<Side> {
    let r = raw.to_ascii_lowercase();
    if r.starts_with("sell") {
        Mapped::Known(Side::Sell)
    } else if r.starts_with("buy") {
        Mapped::Known(Side::Buy)
    } else {
        otw_connector_sdk::enums::unknown("tradestation", "side", raw)
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(VENUE, &symbol.to_ascii_uppercase())
}

fn ccy(c: &str) -> Option<String> {
    Some(c.to_ascii_uppercase()).filter(|c| !c.is_empty())
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Summary => {
            let a: BalancesAnswer = serde_json::from_slice(body).map_err(|e| decode_err("balances", e))?;
            refused(&a.errors)?;
            let b = a.balances.into_iter().next().ok_or_else(|| ConnectorError::new(ErrorKind::Permission, "the key reached TradeStation but the account answered no balance. Check the Account ID setting"))?;
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
                    detail: Some(format!("account {}, type {}, equity {}", b.account_id, b.account_type, b.equity)),
                }),
            )])
        }
        Pending::Positions => {
            let a: PositionsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            refused(&a.errors)?;
            let mut out = Vec::new();
            for p in a.positions.into_iter().filter(|p| !p.symbol.is_empty()) {
                let qty = p.quantity.0.unwrap_or_default().abs();
                if qty.is_zero() {
                    continue;
                }
                out.push(Position {
                    instrument: id(&p.symbol),
                    qty: Qty(if p.long_short.eq_ignore_ascii_case("short") { -qty } else { qty }),
                    avg_price: positive(p.average_price),
                    unrealized_pnl: p.unrealized.0.map(Money),
                    currency: None,
                    mark_price: None,
                    venue: None,
                    account: None,
                    class: None,
                    multiplier: None,
                    name: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out), None)])
        }
        Pending::Orders => {
            let a: OrdersAnswer = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            refused(&a.errors)?;
            let mut out = Vec::new();
            for o in a.orders {
                // An order's instrument is its first leg's; a leg naming none is not drawn.
                let Some(leg) = o.legs.first() else { continue };
                if leg.symbol.is_empty() {
                    continue;
                }
                out.push(Order {
                    id: o.order_id.clone(),
                    instrument: id(&leg.symbol),
                    side: side(&leg.buy_or_sell),
                    order_type: o.order_type.clone(),
                    qty: Qty(leg.quantity_ordered.0.unwrap_or_default()),
                    filled_qty: Qty(leg.exec_quantity.0.unwrap_or_default()),
                    limit_price: positive(o.limit_price),
                    stop_price: positive(o.stop_price),
                    placed_at: o.opened.as_deref().and_then(|t| ts_rfc3339(t).ok()),
                    quote_ccy: ccy(&o.currency),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Fills { pages, range } => {
            let a: OrdersAnswer = serde_json::from_slice(body).map_err(|e| decode_err("historical orders", e))?;
            refused(&a.errors)?;
            let empty = a.orders.is_empty();
            let mut out = Vec::new();
            for o in a.orders {
                // The order's own closing stamp is when it filled; the endpoint takes no end
                // date, so the period's end applies here.
                let Some(at) = o.closed.as_deref().or(o.opened.as_deref()).and_then(|t| ts_rfc3339(t).ok()) else { continue };
                if at < range.from || at > range.to {
                    continue;
                }
                let fee = o.commission_fee.0.unwrap_or_default().abs();
                for (i, leg) in o.legs.iter().enumerate() {
                    let qty = leg.exec_quantity.0.unwrap_or_default();
                    if !qty.is_sign_positive() || qty.is_zero() || leg.symbol.is_empty() {
                        continue;
                    }
                    out.push(Fill {
                        // The order id plus the leg's place in it: stable across re-syncs.
                        id: format!("{}-{i}", o.order_id),
                        order_id: Some(o.order_id.clone()),
                        instrument: id(&leg.symbol),
                        ts: at,
                        side: side(&leg.buy_or_sell),
                        qty: Qty(qty),
                        price: Price(leg.execution_price.0.unwrap_or_default()),
                        // The commission is the order's: carried on its first leg, not once
                        // per leg.
                        fee: Money(if i == 0 { fee } else { Decimal::ZERO }),
                        fee_asset: ccy(&o.currency),
                        quote_ccy: ccy(&o.currency),
                        account: None,
                        venue: None,
                        class: None,
                        multiplier: None,
                    });
                }
            }
            let next = a.next_token.filter(|t| !t.is_empty() && !empty).map(|t| format!("{pages}:{t}"));
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(out), next)])
        }
    }
}
