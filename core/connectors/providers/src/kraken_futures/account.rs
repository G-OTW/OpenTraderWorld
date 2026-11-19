//! Kraken Futures account, read only. Every private call is a signed GET.
//!
//!   - Summary and balances: `/accounts`, every wallet at once: the multi-collateral wallet
//!     (`flex`, its `currencies`), the holding wallet (`cash`) and any single-collateral
//!     margin account (`fi_xbtusd`, whose `balances` also list contracts: left out).
//!   - Positions: `/openpositions`, marked with the public `/tickers` (`markPrice`).
//!   - Open orders: `/openorders`, limit and trigger orders alike: one id space, every id is
//!     an `order_id` that `/cancelorder` takes, so no id is tagged.
//!   - Fills: the history API's `/executions` over the period, oldest first, 1000 a page,
//!     paged by the `continuationToken` the page hands back (in the body, else the
//!     `Next-Continuation-Token` header); 500 pages at most. It carries the fee (`/fills` does
//!     not).
//!
//! Quantities in coins: a linear row is `size` × `contractSize`; an inverse one (USD
//! contracts) is turned into coins at its own price, and its fills are left out (their
//! profit is counted in the coin, which the journal does not compute).

use std::collections::{BTreeMap, HashMap};

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, Event, Fill, InstrumentId, Mapped,
    Money, Order, Position, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_millis, ts_rfc3339, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{decode_err, env, Contract, Session, Sizing, API, HISTORY, REST, VENUE};

/// Executions per page, the venue's most.
const PAGE: u32 = 1000;
/// Hard stop on one pull.
const MAX_PAGES: u32 = 500;

pub enum Pending {
    Summary,
    Balances,
    /// The public tickers, for the positions' marks.
    Marks,
    Positions,
    Orders,
    /// `header`: the answer's `Next-Continuation-Token` header, set when the answer is read.
    Fills { pages: u32, header: Option<String> },
}

/// A signed read weighing `cost` in the account's 500 per 10 s (`accounts`, `openpositions`,
/// `openorders` cost 2; a history page costs 1 history token).
fn signed(endpoint: &'static str, path: String, cost: u32, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed().weight(cost)
}

/// Cursor of a fill walk: pages read, then the venue's continuation token.
fn parse_cursor(c: &str) -> Result<(u32, String), ConnectorError> {
    let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
    let (pages, token) = c.split_once(':').ok_or_else(bad)?;
    Ok((pages.parse().map_err(|_| bad())?, token.to_string()))
}

pub fn encode(cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let mut out = Vec::new();
    let (req, p) = match kind {
        AccountKind::Summary => (signed("accounts", format!("{API}/accounts"), 2, *req_id), Pending::Summary),
        AccountKind::Balances => (signed("accounts", format!("{API}/accounts"), 2, *req_id), Pending::Balances),
        AccountKind::Positions => {
            out.extend(s.need_contracts(*req_id, ctx.received_ts));
            s.pending.insert((*req_id, "tickers"), super::Pending::Account(Pending::Marks));
            out.push(NativeRequest::Http(HttpRequest::get("tickers", REST, format!("{API}/tickers"), *req_id)));
            (signed("openpositions", format!("{API}/openpositions"), 2, *req_id), Pending::Positions)
        }
        AccountKind::OpenOrders => {
            out.extend(s.need_contracts(*req_id, ctx.received_ts));
            (signed("openorders", format!("{API}/openorders"), 2, *req_id), Pending::Orders)
        }
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let (pages, token) = match cursor {
                Some(c) => {
                    let (p, t) = parse_cursor(c)?;
                    (p, Some(t))
                }
                None => (0, None),
            };
            if pages >= MAX_PAGES {
                return Err(ConnectorError::invalid("this period holds more Kraken Futures fills than one pull reads. Ask for a shorter period."));
            }
            out.extend(s.need_contracts(*req_id, ctx.received_ts));
            let mut h = signed("executions", format!("{HISTORY}/executions"), 1, *req_id)
                .query("since", range.from.millis())
                .query("before", range.to.millis())
                .query("sort", "asc")
                .query("count", PAGE);
            if let Some(t) = token {
                h = h.query("continuation_token", t);
            }
            (h, Pending::Fills { pages, header: None })
        }
    };
    s.pending.insert((*req_id, req.endpoint_id), super::Pending::Account(p));
    out.push(NativeRequest::Http(req));
    Ok(out)
}

// ── Answers ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Accounts {
    #[serde(default)]
    accounts: BTreeMap<String, Box<RawValue>>,
}

#[derive(Deserialize)]
struct WalletKind {
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Deserialize)]
struct PlainWallet {
    #[serde(default)]
    balances: BTreeMap<String, OptDec>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlexWallet {
    #[serde(default)]
    currencies: BTreeMap<String, FlexCurrency>,
    #[serde(default)]
    portfolio_value: OptDec,
    #[serde(default)]
    available_margin: OptDec,
}

#[derive(Deserialize)]
struct FlexCurrency {
    #[serde(default)]
    quantity: OptDec,
    #[serde(default)]
    available: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionsPage {
    #[serde(default)]
    open_positions: Vec<PositionRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    unrealized_pnl: OptDec,
    #[serde(default)]
    pnl_currency: Option<String>,
    #[serde(default)]
    max_fixed_leverage: OptDec,
    #[serde(default)]
    cross_max_leverage: OptDec,
}

#[derive(Deserialize)]
struct TickersPage {
    #[serde(default)]
    tickers: Vec<TickerRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TickerRow {
    symbol: String,
    #[serde(default)]
    mark_price: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrdersPage {
    #[serde(default)]
    open_orders: Vec<OrderRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    #[serde(rename = "order_id")]
    order_id: String,
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    limit_price: OptDec,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    filled_size: OptDec,
    #[serde(default)]
    unfilled_size: OptDec,
    #[serde(default)]
    received_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionsPage {
    #[serde(default)]
    elements: Vec<Element>,
    #[serde(default)]
    continuation_token: Option<String>,
}

#[derive(Deserialize)]
struct Element {
    #[serde(default)]
    event: ElementEvent,
}

#[derive(Deserialize, Default)]
struct ElementEvent {
    #[serde(default)]
    execution: Option<ExecutionWrap>,
}

#[derive(Deserialize)]
struct ExecutionWrap {
    execution: Execution,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Execution {
    uid: String,
    order: ExecutionOrder,
    timestamp: Int,
    #[serde(default)]
    quantity: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    order_data: Option<OrderData>,
}

#[derive(Deserialize)]
struct ExecutionOrder {
    #[serde(default)]
    uid: String,
    #[serde(default)]
    tradeable: String,
    #[serde(default)]
    direction: String,
}

#[derive(Deserialize)]
struct OrderData {
    #[serde(default)]
    fee: OptDec,
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("kraken_futures", "side", raw),
    }
}

fn positive(d: Option<Decimal>) -> Option<Price> {
    d.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

const FIAT: &[&str] = &["USD", "EUR", "GBP", "CHF", "CAD", "AUD", "JPY"];

/// What every wallet holds, coin by coin; a margin account's contract balances are left out.
fn balances(page: &Accounts) -> Result<(Vec<Balance>, Option<FlexWallet>), ConnectorError> {
    let mut out = Vec::new();
    let mut flex = None;
    for (name, raw) in &page.accounts {
        let kind: WalletKind = serde_json::from_str(raw.get()).map_err(|e| decode_err("accounts", e))?;
        let mut push = |asset: &str, total: Decimal, available: Decimal, wallet: &str| {
            if total.is_zero() {
                return;
            }
            let asset = asset.to_ascii_uppercase();
            out.push(Balance {
                fiat: FIAT.contains(&asset.as_str()),
                asset,
                total: Money(total),
                available: Money(available),
                name: Some(wallet.to_string()),
                account: None,
            });
        };
        match kind.kind.as_str() {
            "multiCollateralMarginAccount" => {
                let w: FlexWallet = serde_json::from_str(raw.get()).map_err(|e| decode_err("accounts flex", e))?;
                for (asset, c) in &w.currencies {
                    let q = c.quantity.0.unwrap_or_default();
                    push(asset, q, c.available.0.unwrap_or(q), "multi-collateral wallet");
                }
                flex = Some(w);
            }
            "cashAccount" | "marginAccount" => {
                let w: PlainWallet = serde_json::from_str(raw.get()).map_err(|e| decode_err("accounts", e))?;
                let wallet = if kind.kind == "cashAccount" { "holding wallet".to_string() } else { format!("{name} margin account") };
                // A margin account also lists its contracts (`FI_XBTUSD_180615`): coins only.
                for (asset, q) in w.balances.iter().filter(|(a, _)| !a.contains('_')) {
                    let q = q.0.unwrap_or_default();
                    push(asset, q, q, &wallet);
                }
            }
            other => tracing::warn!("kraken_futures accounts: wallet `{name}` of type `{other}` is not read"),
        }
    }
    Ok((out, flex))
}

fn unlisted(symbol: &str) -> ConnectorError {
    ConnectorError::not_found(format!("Kraken Futures lists no contract named {symbol}, current or expired"))
}

/// Coins in `size` contracts of `c` at `price`.
fn coins(c: &Contract, size: Decimal, price: Option<Decimal>, symbol: &str) -> Result<Decimal, ConnectorError> {
    let notional = size * c.contract_size;
    match c.sizing {
        Sizing::Linear => Ok(notional),
        Sizing::Inverse => price
            .filter(|p| !p.is_zero())
            .and_then(|p| notional.checked_div(p))
            .ok_or_else(|| ConnectorError::decode(format!("kraken_futures: {symbol} is an inverse contract with no price to count its coins"))),
        Sizing::Other => Err(ConnectorError::unsupported(format!("kraken_futures: {symbol} is a `{}` contract, not read here", c.kind))),
    }
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    // Rows naming a contract the current list lacks wait for the expired list, once.
    let symbols: Vec<String> = match &p {
        Pending::Positions => serde_json::from_slice::<PositionsPage>(body).map_err(|e| decode_err("openpositions", e))?.open_positions.into_iter().map(|r| r.symbol).collect(),
        Pending::Orders => serde_json::from_slice::<OrdersPage>(body).map_err(|e| decode_err("openorders", e))?.open_orders.into_iter().map(|r| r.symbol).collect(),
        Pending::Fills { .. } => serde_json::from_slice::<ExecutionsPage>(body)
            .map_err(|e| decode_err("executions", e))?
            .elements
            .into_iter()
            .filter_map(|e| e.event.execution.map(|x| x.execution.order.tradeable))
            .collect(),
        _ => vec![],
    };
    // A contract neither list names is an error naming it.
    if let Some(sym) = symbols.iter().find(|sym| s.contract(sym).is_none()).cloned() {
        if !s.expired_read(ctx.received_ts) {
            s.defer(corr, p, body);
            return Ok(vec![]);
        }
        return Err(unlisted(&sym));
    }
    match p {
        Pending::Summary | Pending::Balances => {
            let page: Accounts = serde_json::from_slice(body).map_err(|e| decode_err("accounts", e))?;
            let (held, flex) = balances(&page)?;
            if matches!(p, Pending::Summary) {
                let value = flex
                    .as_ref()
                    .and_then(|f| f.portfolio_value.0.zip(f.available_margin.0))
                    .map(|(pv, am)| format!(", multi-collateral portfolio value {} USD, available margin {} USD", pv.normalize(), am.normalize()))
                    .unwrap_or_default();
                return Ok(vec![env(
                    ctx,
                    corr,
                    Event::AccountSummary(AccountSummary {
                        account_type: Some("FUTURES".into()),
                        permissions: vec![],
                        assets_held: u32::try_from(held.len()).unwrap_or(u32::MAX),
                        can_trade: None,
                        open_positions: None,
                        wallet_balance: None,
                        detail: Some(format!("key accepted, {} wallet balance(s){value}", held.len())),
                    }),
                )]);
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)])
        }
        Pending::Marks => {
            let page: TickersPage = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
            let marks = page.tickers.into_iter().filter_map(|t| Some((t.symbol.to_ascii_uppercase(), t.mark_price.0?))).collect();
            s.marks.insert(corr, marks);
            Ok(vec![])
        }
        Pending::Positions => {
            let page: PositionsPage = serde_json::from_slice(body).map_err(|e| decode_err("openpositions", e))?;
            let marks: HashMap<String, Decimal> = s.marks.remove(&corr).unwrap_or_default();
            let mut out = Vec::with_capacity(page.open_positions.len());
            for r in page.open_positions {
                let symbol = r.symbol.to_ascii_uppercase();
                let c = s.contract(&symbol).ok_or_else(|| unlisted(&symbol))?.clone();
                if c.sizing == Sizing::Other {
                    tracing::warn!("kraken_futures positions: {symbol} is a `{}` contract, left out", c.kind);
                    continue;
                }
                let sign = match r.side.as_str() {
                    "long" => Decimal::ONE,
                    "short" => -Decimal::ONE,
                    other => return Err(ConnectorError::decode(format!("kraken_futures positions: side `{other}` on {symbol}"))),
                };
                let size = r.size.0.unwrap_or_default().abs();
                if size.is_zero() {
                    continue;
                }
                let qty = coins(&c, size, r.price.0, &symbol)?;
                let mut label = Vec::new();
                let pnl = match c.sizing {
                    Sizing::Inverse => {
                        label.push(format!("inverse, {} {} contracts, profit counted in the coin", size.normalize(), c.quote));
                        None
                    }
                    // The profit is carried only in the contract's own quote currency.
                    _ => r.unrealized_pnl.0.filter(|_| r.pnl_currency.as_deref().is_none_or(|p| p.eq_ignore_ascii_case(&c.quote))).map(Money),
                };
                if let Some(l) = r.max_fixed_leverage.0 {
                    label.push(format!("isolated, max leverage {}", l.normalize()));
                } else if let Some(l) = r.cross_max_leverage.0 {
                    label.push(format!("cross, max leverage {}", l.normalize()));
                } else if c.sizing == Sizing::Linear {
                    // `crossMaxLeverage`: "Absent when the position is uncapped or isolated" (the
                    // leverage preference of a multi-collateral contract).
                    label.push("cross, leverage uncapped".into());
                }
                out.push(Position {
                    qty: Qty(sign * qty),
                    avg_price: positive(r.price.0),
                    unrealized_pnl: pnl,
                    currency: Some(c.quote.clone()).filter(|q| !q.is_empty()),
                    mark_price: positive(marks.get(&symbol).copied()),
                    venue: None,
                    account: None,
                    class: Some(c.class()),
                    // Already in coins.
                    multiplier: Some(Decimal::ONE),
                    name: Some(label.join("; ")).filter(|l| !l.is_empty()),
                    instrument: InstrumentId::new(VENUE, &symbol),
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Positions(out), None)])
        }
        Pending::Orders => {
            let page: OrdersPage = serde_json::from_slice(body).map_err(|e| decode_err("openorders", e))?;
            let mut out = Vec::with_capacity(page.open_orders.len());
            for o in page.open_orders {
                let symbol = o.symbol.to_ascii_uppercase();
                let c = s.contract(&symbol).ok_or_else(|| unlisted(&symbol))?.clone();
                if c.sizing == Sizing::Other {
                    tracing::warn!("kraken_futures open orders: {symbol} is a `{}` contract, left out", c.kind);
                    continue;
                }
                let filled = o.filled_size.0.unwrap_or_default();
                let total = filled + o.unfilled_size.0.unwrap_or_default();
                // An inverse order is counted at its own price, else its trigger.
                let at = o.limit_price.0.filter(|p| !p.is_zero()).or(o.stop_price.0);
                out.push(Order {
                    id: o.order_id,
                    instrument: InstrumentId::new(VENUE, &symbol),
                    side: side(&o.side),
                    order_type: o.order_type,
                    qty: Qty(coins(&c, total, at, &symbol)?),
                    filled_qty: Qty(coins(&c, filled, at, &symbol)?),
                    limit_price: positive(o.limit_price.0),
                    stop_price: positive(o.stop_price.0),
                    placed_at: o.received_time.as_deref().map(ts_rfc3339).transpose()?,
                    quote_ccy: Some(c.quote.clone()).filter(|q| !q.is_empty()),
                    account: None,
                    venue: None,
                    class: Some(c.class()),
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Fills { pages, header } => {
            let page: ExecutionsPage = serde_json::from_slice(body).map_err(|e| decode_err("executions", e))?;
            let n = page.elements.len();
            let mut fills = Vec::with_capacity(n);
            for e in page.elements {
                let Some(x) = e.event.execution.map(|w| w.execution) else { continue };
                let symbol = x.order.tradeable.to_ascii_uppercase();
                let c = s.contract(&symbol).ok_or_else(|| unlisted(&symbol))?.clone();
                if c.sizing != Sizing::Linear {
                    tracing::warn!("kraken_futures fills: {symbol} is a `{}` contract, its fill {} is not imported", c.kind, x.uid);
                    continue;
                }
                fills.push(Fill {
                    id: x.uid,
                    order_id: Some(x.order.uid).filter(|o| !o.is_empty()),
                    instrument: InstrumentId::new(VENUE, &symbol),
                    ts: ts_millis(x.timestamp.0)?,
                    side: side(&x.order.direction),
                    qty: Qty(coins(&c, x.quantity.0.unwrap_or_default(), None, &symbol)?),
                    price: Price(x.price.0.unwrap_or_default()),
                    // The doc names no fee currency on an execution: left unsaid.
                    fee: Money(x.order_data.and_then(|d| d.fee.0).unwrap_or_default()),
                    fee_asset: None,
                    quote_ccy: Some(c.quote.clone()).filter(|q| !q.is_empty()),
                    account: None,
                    venue: None,
                    class: Some(c.class()),
                    multiplier: Some(Decimal::ONE),
                });
            }
            let next = page.continuation_token.filter(|t| !t.is_empty()).or(header.filter(|t| !t.is_empty())).filter(|_| n > 0).map(|t| format!("{}:{t}", pages + 1));
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next)])
        }
    }
}
