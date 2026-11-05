//! Capital.com's account, read only: the financial accounts of the login, the activity log
//! (fills), open positions and working orders.
//!
//! The activity log answers 24 hours at a time, so a period is walked day by day, one call
//! each (`Mapper::follow_ups`), and a period wider than [`MAX_DAYS`] is refused by name rather
//! than left to run into the rate limit. A deal id names a position, not a fill: the row that
//! opened it and the row that closed it share it, so a fill's identity is the deal, its time
//! and its direction together.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Command, ConnectorError, CorrId, Decimal, Envelope, Event, Fill, InstrumentId, Mapped, Money, Order,
    Position, Price, Qty, Side, TimeRange,
};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{class_of, decode_err, env, parse_utc, query_stamp, Pending as Step, Session, REST, VENUE};

/// Widest window the activity log accepts.
const WINDOW_SECS: i64 = 24 * 3600;
/// Hard stop on the day-by-day walk.
const MAX_DAYS: i64 = 400;

pub enum Pending {
    Summary { account: Option<String> },
    Positions,
    Orders,
    Activity { range: TimeRange, from: i64, rows: Vec<Fill> },
}

fn get(endpoint: &'static str, path: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, corr).signed()
}

fn activity(from: i64, to: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
    let stamp = |s: i64| otw_connector_model::Timestamp::from_secs(s).ok_or_else(|| ConnectorError::invalid("time out of range")).and_then(query_stamp);
    Ok(get("activity", "/api/v1/history/activity", corr)
        .query("from", stamp(from)?)
        .query("to", stamp(to)?)
        // Without it a row carries no size, no price and no currency.
        .query("detailed", "true")
        // Only accepted position activity is a fill; an edited stop is not.
        .query("filter", "type==POSITION;status==ACCEPTED"))
}

pub fn encode(cmd: &Command, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, account, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let corr = *req_id;
    Ok(vec![match kind {
        AccountKind::Summary => s.put(corr, get("accounts", "/api/v1/accounts", corr), Step::Account(Pending::Summary { account: account.clone() })),
        AccountKind::Positions => s.put(corr, get("positions", "/api/v1/positions", corr), Step::Account(Pending::Positions)),
        AccountKind::OpenOrders => s.put(corr, get("orders", "/api/v1/workingorders", corr), Step::Account(Pending::Orders)),
        AccountKind::Fills => {
            let r = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if r.to <= r.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let days = (r.to.secs() - r.from.secs()) / 3600 / 24 + 1;
            if days > MAX_DAYS {
                return Err(ConnectorError::invalid(format!(
                    "Capital.com answers its activity log one day at a time, so this period would cost {days} calls. Ask for {MAX_DAYS} days or fewer and pull the rest separately."
                )));
            }
            let from = r.from.secs();
            let to = (from + WINDOW_SECS).min(r.to.secs());
            s.put(corr, activity(from, to, corr)?, Step::Account(Pending::Activity { range: r, from, rows: vec![] }))
        }
        AccountKind::Balances => return Err(ConnectorError::unsupported("Capital.com is read through its positions, not balances")),
    }])
}

#[derive(Deserialize, Default)]
struct Balance {
    #[serde(default)]
    balance: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountRow {
    #[serde(default)]
    account_id: String,
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    preferred: bool,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    balance: Option<Balance>,
}

#[derive(Deserialize)]
struct AccountsAnswer {
    #[serde(default)]
    accounts: Vec<AccountRow>,
}

#[derive(Deserialize)]
struct Details {
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    level: OptDec,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    currency: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityRow {
    #[serde(rename = "dateUTC", default)]
    date_utc: String,
    #[serde(default)]
    epic: String,
    #[serde(default)]
    deal_id: String,
    #[serde(default)]
    details: Option<Details>,
}

#[derive(Deserialize)]
struct ActivityAnswer {
    #[serde(default)]
    activities: Vec<ActivityRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionData {
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    level: OptDec,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    contract_size: OptDec,
    #[serde(default)]
    upl: OptDec,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct MarketData {
    #[serde(default)]
    epic: String,
    #[serde(default)]
    instrument_type: String,
}

#[derive(Deserialize)]
struct PositionRow {
    #[serde(default)]
    position: Option<PositionData>,
    #[serde(default)]
    market: Option<MarketData>,
}

#[derive(Deserialize)]
struct PositionsAnswer {
    #[serde(default)]
    positions: Vec<PositionRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkingOrder {
    #[serde(default)]
    deal_id: String,
    #[serde(default)]
    epic: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    order_size: OptDec,
    #[serde(default)]
    order_level: OptDec,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    currency_code: String,
    #[serde(rename = "createdDateUTC", default)]
    created_date_utc: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    #[serde(default)]
    working_order_data: Option<WorkingOrder>,
    #[serde(default)]
    market_data: Option<MarketData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrdersAnswer {
    #[serde(default)]
    working_orders: Vec<OrderRow>,
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems) -> Envelope {
    env(ctx, corr, Event::AccountChunk { items, next_cursor: None, done: true })
}

fn side(direction: &str) -> Mapped<Side> {
    Mapped::Known(if direction.eq_ignore_ascii_case("sell") { Side::Sell } else { Side::Buy })
}

fn ccy(c: &str) -> Option<String> {
    Some(c.to_ascii_uppercase()).filter(|c| !c.is_empty())
}

fn positive(d: &OptDec) -> Option<Decimal> {
    d.0.filter(|v| v.is_sign_positive() && !v.is_zero())
}

fn id(epic: &str) -> InstrumentId {
    InstrumentId::new(VENUE, &epic.to_ascii_uppercase())
}

/// A decimal as the account summary writes it: the shortest form of the number.
fn plain(d: Decimal) -> String {
    d.to_string().parse::<f64>().map(|f| f.to_string()).unwrap_or_else(|_| d.normalize().to_string())
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Summary { account } => {
            let a: AccountsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("accounts", e))?;
            // The account the setting names, else the preferred one the session starts on.
            let active = a.accounts.iter().find(|r| match &account {
                Some(id) => r.account_id == id.trim(),
                None => r.preferred,
            });
            let (name, currency, balance) = match active {
                Some(r) => (r.account_name.clone(), r.currency.clone(), r.balance.as_ref().and_then(|b| b.balance.0).unwrap_or_default()),
                None => (String::new(), String::new(), Decimal::ZERO),
            };
            let detail = format!("sign-in accepted, {} account(s); reading {name} ({currency}), balance {}", a.accounts.len(), plain(balance));
            Ok(vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: None,
                    permissions: vec![],
                    assets_held: 0,
                    // No key is graded read-only; PROVIDER.md says so rather than a test line.
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
            for r in a.positions {
                let (Some(p), m) = (r.position, r.market.unwrap_or_default()) else { continue };
                let size = p.size.0.unwrap_or_default().abs();
                if size.is_zero() {
                    continue;
                }
                out.push(Position {
                    instrument: id(&m.epic),
                    qty: Qty(if p.direction.eq_ignore_ascii_case("sell") { -size } else { size }),
                    avg_price: positive(&p.level).map(Price),
                    unrealized_pnl: p.upl.0.map(Money),
                    currency: ccy(&p.currency),
                    mark_price: None,
                    venue: Some("Capital.com".into()),
                    account: None,
                    class: Some(class_of(&m.instrument_type)),
                    // What one unit of size is worth, as the deal carries it.
                    multiplier: positive(&p.contract_size),
                    name: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out))])
        }
        Pending::Orders => {
            let a: OrdersAnswer = serde_json::from_slice(body).map_err(|e| decode_err("working orders", e))?;
            let mut out = Vec::new();
            for r in a.working_orders {
                let Some(o) = r.working_order_data else { continue };
                let m = r.market_data.unwrap_or_default();
                let level = positive(&o.order_level).map(Price);
                // One level field, filed as whichever price the order type says it is.
                let limit = o.order_type.eq_ignore_ascii_case("limit");
                out.push(Order {
                    id: o.deal_id,
                    instrument: id(&o.epic),
                    side: side(&o.direction),
                    qty: Qty(o.order_size.0.unwrap_or_default()),
                    filled_qty: Qty(Decimal::ZERO),
                    limit_price: if limit { level } else { None },
                    stop_price: if limit { None } else { level },
                    order_type: o.order_type,
                    placed_at: parse_utc(&o.created_date_utc),
                    quote_ccy: ccy(&o.currency_code),
                    account: None,
                    venue: Some("Capital.com".into()),
                    class: Some(class_of(&m.instrument_type)),
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out))])
        }
        Pending::Activity { range, from, mut rows } => {
            let a: ActivityAnswer = serde_json::from_slice(body).map_err(|e| decode_err("activity", e))?;
            for r in a.activities {
                let epic = r.epic.to_ascii_uppercase();
                let Some(d) = r.details else { continue };
                let (Some(size), Some(level)) = (positive(&d.size), positive(&d.level)) else { continue };
                let Some(at) = parse_utc(&r.date_utc) else { continue };
                if epic.is_empty() {
                    continue;
                }
                let direction = d.direction.to_ascii_lowercase();
                rows.push(Fill {
                    id: format!("{}:{}:{direction}", r.deal_id, r.date_utc),
                    order_id: Some(r.deal_id),
                    instrument: id(&epic),
                    ts: at,
                    side: side(&direction),
                    qty: Qty(size),
                    price: Price(level),
                    // Capital.com bills the spread and an overnight charge, not a commission.
                    fee: Money(Decimal::ZERO),
                    fee_asset: None,
                    quote_ccy: ccy(&d.currency),
                    account: None,
                    venue: Some("Capital.com".into()),
                    // The row names no instrument type: a CFD, never a guess at what it tracks.
                    class: Some(otw_connector_model::AssetClass::Cfd),
                    multiplier: None,
                });
            }
            let next = (from + WINDOW_SECS).min(range.to.secs());
            if next < range.to.secs() {
                let to = (next + WINDOW_SECS).min(range.to.secs());
                let r = s.put(corr, activity(next, to, corr)?, Step::Account(Pending::Activity { range, from: next, rows }));
                s.then(corr, r);
                return Ok(vec![]);
            }
            rows.sort_by_key(|f| f.ts);
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(rows))])
        }
    }
}
