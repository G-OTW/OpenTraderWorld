//! FOREX.com's account, read only. Every read starts from the account tree: the trading
//! account is the one the connector's setting names, else the login's only one; a login with
//! several and no setting is refused with the list, never picked from.
//!
//! The trade history has no end date and no cursor: `tradehistory` takes a `from` and returns
//! at most 200 rows, so the period's end applies here and the walk moves `from` past the
//! newest row it read, a row read twice told apart by its order id.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event, Fill, InstrumentId, Mapped,
    Money, Order, Position, Price, Qty, Side, TimeRange,
};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{class_of, decode_err, int_text, tree_request, wcf, Pending as Step, Session, Then as After, Tree, REST, VENUE};

/// Rows one `tradehistory` page returns at most.
const PAGE: u32 = 200;
/// Hard stop on the walk.
const MAX_PAGES: u32 = 200;

pub struct Then {
    kind: AccountKind,
    range: Option<TimeRange>,
    account: Option<String>,
}

pub enum Pending {
    Positions { account: String },
    Orders { account: String },
    Trades { account: String, range: TimeRange, from: i64, pages: u32, seen: Vec<String>, rows: Vec<Fill> },
}

fn get(endpoint: &'static str, path: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, corr).signed()
}

pub fn encode(cmd: &Command, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, account, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    match kind {
        AccountKind::Balances => return Err(ConnectorError::unsupported("FOREX.com is read through its positions, not balances")),
        AccountKind::Fills => {
            let r = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if r.to <= r.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
        }
        _ => {}
    }
    Ok(vec![tree_request(s, *req_id, After::Account(Then { kind: *kind, range: *range, account: account.clone() }))])
}

fn list(tree: &[(String, String)]) -> String {
    tree.iter().map(|(i, n)| format!("{n} ({i})")).collect::<Vec<_>>().join(", ")
}

/// The setting when there is one (and the login carries it), the only one otherwise.
fn pick(trading: &[(String, String)], wanted: Option<&str>) -> Result<String, ConnectorError> {
    if let Some(id) = wanted {
        if !trading.iter().any(|(known, _)| known == id) {
            return Err(ConnectorError::new(ErrorKind::Permission, format!("this login does not carry trading account {id}. It has [{}]", list(trading))));
        }
        return Ok(id.to_string());
    }
    match trading {
        [(id, _)] => Ok(id.clone()),
        [] => Err(ConnectorError::new(ErrorKind::Permission, "this FOREX.com login carries no trading account")),
        many => Err(ConnectorError::invalid(format!(
            "this login carries {} trading accounts, so name one in the Trading account id setting: [{}]",
            many.len(),
            list(many)
        ))),
    }
}

fn trades(account: &str, from: i64, corr: CorrId) -> HttpRequest {
    get("trades", "/order/tradehistory", corr).query("TradingAccountId", account).query("maxResults", PAGE).query("from", from)
}

/// The request the tree leads to, or the summary it already is.
pub fn after_tree(t: Then, tree: Tree, corr: CorrId, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let client = int_text(&tree.client).unwrap_or_default();
    let trading: Vec<(String, String)> = tree.trading.into_iter().filter_map(|a| Some((int_text(&a.id)?, a.code))).collect();
    let account = pick(&trading, t.account.as_deref())?;
    let r = match t.kind {
        AccountKind::Summary => {
            // Answered from the tree itself.
            let detail = format!("signed in as client {client}, reading trading account {account} of [{}]", list(&trading));
            return Ok(vec![env(
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
            )]);
        }
        AccountKind::Positions => s.put(corr, get("positions", "/order/openpositions", corr).query("TradingAccountId", &account), Step::Account(Pending::Positions { account })),
        AccountKind::OpenOrders => s.put(corr, get("orders", "/order/activestoplimitorders", corr).query("TradingAccountId", &account), Step::Account(Pending::Orders { account })),
        AccountKind::Fills => {
            let range = t.range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            let from = range.from.secs();
            s.put(corr, trades(&account, from, corr), Step::Account(Pending::Trades { account, range, from, pages: 1, seen: vec![], rows: vec![] }))
        }
        AccountKind::Balances => return Err(ConnectorError::unsupported("FOREX.com is read through its positions, not balances")),
    };
    s.then(corr, r);
    Ok(vec![])
}

#[derive(Deserialize)]
struct TradeRow {
    #[serde(rename = "OrderId", default)]
    order_id: OptDec,
    #[serde(rename = "MarketName", default)]
    market_name: String,
    #[serde(rename = "Direction", default)]
    direction: String,
    #[serde(rename = "OriginalQuantity", default)]
    original_quantity: OptDec,
    #[serde(rename = "Quantity", default)]
    quantity: OptDec,
    #[serde(rename = "Price", default)]
    price: OptDec,
    #[serde(rename = "Currency", default)]
    currency: String,
    #[serde(rename = "ExecutedDateTimeUtc", default)]
    executed: Option<String>,
}

#[derive(Deserialize)]
struct TradesAnswer {
    #[serde(rename = "TradeHistory", default)]
    rows: Vec<TradeRow>,
}

#[derive(Deserialize)]
struct PositionRow {
    #[serde(rename = "MarketName", default)]
    market_name: String,
    #[serde(rename = "Quantity", default)]
    quantity: OptDec,
    #[serde(rename = "Direction", default)]
    direction: String,
    #[serde(rename = "Price", default)]
    price: OptDec,
    #[serde(rename = "Currency", default)]
    currency: String,
}

#[derive(Deserialize)]
struct PositionsAnswer {
    #[serde(rename = "OpenPositions", default)]
    rows: Vec<PositionRow>,
}

#[derive(Deserialize)]
struct OrderRow {
    #[serde(rename = "OrderId", default)]
    order_id: OptDec,
    #[serde(rename = "MarketName", default)]
    market_name: String,
    #[serde(rename = "Type", default)]
    kind: OptDec,
    #[serde(rename = "TriggerPrice", default)]
    trigger: OptDec,
    #[serde(rename = "Direction", default)]
    direction: String,
    #[serde(rename = "Quantity", default)]
    quantity: OptDec,
    #[serde(rename = "Currency", default)]
    currency: String,
    #[serde(rename = "CreatedDateTimeUTC", default)]
    created: Option<String>,
}

#[derive(Deserialize)]
struct OrdersAnswer {
    #[serde(rename = "ActiveStopLimitOrders", default)]
    rows: Vec<OrderRow>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
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

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

fn id(name: &str) -> InstrumentId {
    InstrumentId::new(VENUE, &name.to_ascii_uppercase())
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Positions { account } => {
            let a: PositionsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("open positions", e))?;
            let mut out = Vec::new();
            for r in a.rows {
                let qty = r.quantity.0.unwrap_or_default().abs();
                if qty.is_zero() {
                    continue;
                }
                let name = r.market_name.to_ascii_uppercase();
                out.push(Position {
                    qty: Qty(if r.direction.eq_ignore_ascii_case("sell") { -qty } else { qty }),
                    avg_price: positive(r.price),
                    unrealized_pnl: None,
                    currency: ccy(&r.currency),
                    mark_price: None,
                    venue: None,
                    account: Some(account.clone()),
                    class: Some(class_of(&name)),
                    instrument: id(&name),
                    multiplier: None,
                    name: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out))])
        }
        Pending::Orders { account } => {
            let a: OrdersAnswer = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            let mut out = Vec::new();
            for r in a.rows {
                let name = r.market_name.to_ascii_uppercase();
                // StoneX numbers its order types: 1 is a stop, 2 a limit. The trigger is one
                // field, so which price it is comes from the type.
                let kind = int_text(&r.kind).unwrap_or_default();
                let trigger = positive(r.trigger);
                out.push(Order {
                    id: int_text(&r.order_id).unwrap_or_default(),
                    side: side(&r.direction),
                    order_type: match kind.as_str() {
                        "1" => "stop".into(),
                        "2" => "limit".into(),
                        "" => "type 0".into(),
                        other => format!("type {other}"),
                    },
                    qty: Qty(r.quantity.0.unwrap_or_default()),
                    filled_qty: Qty(Decimal::ZERO),
                    limit_price: if kind == "2" { trigger } else { None },
                    stop_price: if kind == "2" { None } else { trigger },
                    placed_at: r.created.as_deref().and_then(wcf),
                    quote_ccy: ccy(&r.currency),
                    account: Some(account.clone()),
                    venue: None,
                    class: Some(class_of(&name)),
                    instrument: id(&name),
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out))])
        }
        Pending::Trades { account, range, from, pages, mut seen, mut rows } => {
            let a: TradesAnswer = serde_json::from_slice(body).map_err(|e| decode_err("trade history", e))?;
            let full = a.rows.len() >= usize::try_from(PAGE).unwrap_or(usize::MAX);
            let mut newest = from;
            for r in a.rows {
                let Some(at) = r.executed.as_deref().and_then(wcf) else { continue };
                newest = newest.max(at.secs());
                if at < range.from || at > range.to {
                    continue;
                }
                let name = r.market_name.to_ascii_uppercase();
                if name.is_empty() {
                    continue;
                }
                let oid = int_text(&r.order_id).unwrap_or_else(|| "0".into());
                // "Changed since": a row already read can come back on the next page.
                if seen.contains(&oid) {
                    continue;
                }
                seen.push(oid.clone());
                // The original quantity, before part closures: what was executed here.
                let qty = positive(r.original_quantity).map(|p| p.0).unwrap_or_else(|| r.quantity.0.unwrap_or_default()).abs();
                rows.push(Fill {
                    id: oid.clone(),
                    order_id: Some(oid),
                    instrument: id(&name),
                    ts: at,
                    side: side(&r.direction),
                    qty: Qty(qty),
                    price: Price(r.price.0.unwrap_or_default()),
                    // StoneX bills the spread, not a commission.
                    fee: Money(Decimal::ZERO),
                    fee_asset: None,
                    quote_ccy: ccy(&r.currency),
                    account: Some(account.clone()),
                    venue: None,
                    class: Some(class_of(&name)),
                    multiplier: None,
                });
            }
            // No cursor: the walk moves past the newest row read; a page that does not
            // advance the clock ends it.
            if full && newest > from && pages < MAX_PAGES {
                let r = s.put(corr, trades(&account, newest, corr), Step::Account(Pending::Trades { account, range, from: newest, pages: pages + 1, seen, rows }));
                s.then(corr, r);
                return Ok(vec![]);
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(rows))])
        }
    }
}
