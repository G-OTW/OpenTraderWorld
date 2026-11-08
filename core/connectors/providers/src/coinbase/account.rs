//! Coinbase Advanced Trade account, read only: fills over a window, wallets, open orders and
//! the key's own summary. Every call carries its own JWT (the signer's business).
//!
//! What the API's shape forces:
//!   - a fill's `size` is in the quote currency when `size_in_quote` is set, so the quantity
//!     is the size divided by the price rather than taken at face value;
//!   - the quote currency is on neither fills nor orders: it comes from the product list,
//!     never from cutting `BTC-USD` on its dash;
//!   - fills page by an opaque cursor, 100 rows at a time; 500 pages at most for one pull.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Envelope, Event, Fill, InstrumentId,
    Mapped, Money, Order, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;
use serde_json::Value;

use super::{rfc3339, Session, BROKERAGE, PREFIX, VENUE};

const PAGE: usize = 100;
const MAX_PAGES: u32 = 500;

pub enum Pending {
    Summary,
    Balances,
    Orders,
    Fills { pages: u32 },
}

fn signed(endpoint: &'static str, path: &str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, BROKERAGE, format!("{PREFIX}{path}"), req_id).signed()
}

pub fn encode(cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, symbols, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let mut out = Vec::new();
    let (req, p) = match kind {
        AccountKind::Summary => (signed("accounts", "/accounts", *req_id).query("limit", 250), Pending::Summary),
        AccountKind::Balances => (signed("accounts", "/accounts", *req_id).query("limit", 250), Pending::Balances),
        AccountKind::OpenOrders => {
            out.extend(s.need_quotes(*req_id, ctx.received_ts));
            (
                signed("ordersOpen", "/orders/historical/batch", *req_id).query("order_status", "OPEN").query("limit", PAGE),
                Pending::Orders,
            )
        }
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            // Cursor `<pages>:<venue cursor>`.
            let (pages, venue_cursor) = match cursor {
                Some(c) => {
                    let (p, v) = c.split_once(':').ok_or_else(|| ConnectorError::invalid(format!("bad cursor `{c}`")))?;
                    (p.parse::<u32>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?, Some(v.to_string()))
                }
                None => (0, None),
            };
            if pages >= MAX_PAGES {
                return Err(ConnectorError::invalid("this period holds more Coinbase fills than one pull reads. Ask for a shorter period."));
            }
            out.extend(s.need_quotes(*req_id, ctx.received_ts));
            let mut h = signed("fills", "/orders/historical/fills", *req_id)
                .query("limit", PAGE)
                .query("start_sequence_timestamp", rfc3339(range.from)?)
                .query("end_sequence_timestamp", rfc3339(range.to)?);
            // The venue filters by product itself: a narrowed pull walks fewer pages.
            for i in symbols {
                h = h.query("product_ids", &i.native);
            }
            if let Some(v) = venue_cursor.filter(|v| !v.is_empty()) {
                h = h.query("cursor", v);
            }
            (h, Pending::Fills { pages })
        }
        AccountKind::Positions => return Err(ConnectorError::unsupported("Coinbase spot holds no positions")),
    };
    s.pending.insert((*req_id, req.endpoint_id), super::Pending::Account(p));
    out.push(NativeRequest::Http(req));
    Ok(out)
}

#[derive(Deserialize, Default)]
struct Amount {
    #[serde(default)]
    value: OptDec,
}

#[derive(Deserialize)]
struct AccountRow {
    #[serde(default)]
    currency: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    available_balance: Amount,
    #[serde(default)]
    hold: Amount,
    #[serde(default)]
    retail_portfolio_id: Option<String>,
}

#[derive(Deserialize)]
struct Accounts {
    #[serde(default)]
    accounts: Vec<AccountRow>,
}

#[derive(Deserialize)]
struct FillRow {
    #[serde(default)]
    entry_id: String,
    #[serde(default)]
    trade_id: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    product_id: String,
    #[serde(default)]
    trade_time: Option<String>,
    #[serde(default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    size_in_quote: bool,
    #[serde(default)]
    commission: OptDec,
    #[serde(default)]
    retail_portfolio_id: Option<String>,
}

#[derive(Deserialize)]
struct Fills {
    #[serde(default)]
    fills: Vec<FillRow>,
    #[serde(default)]
    cursor: String,
}

#[derive(Deserialize)]
struct OrderRow {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    product_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    order_configuration: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    filled_size: OptDec,
    #[serde(default)]
    created_time: Option<String>,
    #[serde(default)]
    retail_portfolio_id: Option<String>,
}

#[derive(Deserialize)]
struct Orders {
    #[serde(default)]
    orders: Vec<OrderRow>,
}

/// The prices of an order live inside whichever configuration it was placed with
/// (`limit_limit_gtc`, `stop_limit_stop_limit_gtc`...), read by name from whatever is there.
#[derive(Deserialize, Default)]
struct OrderConfig {
    #[serde(default)]
    base_size: OptDec,
    #[serde(default)]
    limit_price: OptDec,
    #[serde(default)]
    stop_price: OptDec,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("coinbase {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("coinbase", "side", raw),
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

fn portfolio(p: Option<String>) -> Option<String> {
    p.filter(|p| !p.is_empty())
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Summary => {
            let page: Accounts = serde_json::from_slice(body).map_err(|e| decode_err("accounts", e))?;
            let funded = page.accounts.iter().filter(|a| a.available_balance.value.0.is_some_and(|v| v.is_sign_positive() && !v.is_zero())).count();
            Ok(vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: None,
                    permissions: vec![],
                    assets_held: u32::try_from(funded).unwrap_or(u32::MAX),
                    can_trade: None,
                    open_positions: None,
                    wallet_balance: None,
                    detail: Some(format!("key accepted, {} account(s), {funded} with a balance", page.accounts.len())),
                }),
            )])
        }
        Pending::Balances => {
            let page: Accounts = serde_json::from_slice(body).map_err(|e| decode_err("accounts", e))?;
            let mut out = Vec::new();
            for a in page.accounts {
                // What the account owns: available plus what an open order holds.
                let available = a.available_balance.value.0.unwrap_or_default();
                let total = available + a.hold.value.0.unwrap_or_default();
                if !total.is_sign_positive() || total.is_zero() {
                    continue;
                }
                out.push(Balance {
                    asset: a.currency,
                    total: Money(total),
                    available: Money(available),
                    name: Some(a.name).filter(|n| !n.is_empty()),
                    // A fiat wallet is reported too, spelled as the currency it is.
                    fiat: a.kind.contains("FIAT"),
                    account: portfolio(a.retail_portfolio_id),
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(out), None)])
        }
        Pending::Orders => {
            let page: Orders = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
            let quotes = s.quotes();
            let mut out = Vec::with_capacity(page.orders.len());
            for o in page.orders {
                let cfg: OrderConfig = o
                    .order_configuration
                    .as_ref()
                    .and_then(|m| m.values().next())
                    .map(|v| serde_json::from_str(v.to_string().as_str()))
                    .transpose()
                    .map_err(|e| decode_err("order configuration", e))?
                    .unwrap_or_default();
                let product = o.product_id.to_ascii_uppercase();
                out.push(Order {
                    id: o.order_id,
                    side: side(&o.side),
                    order_type: o.order_type,
                    qty: Qty(cfg.base_size.0.unwrap_or_default()),
                    filled_qty: Qty(o.filled_size.0.unwrap_or_default()),
                    limit_price: positive(cfg.limit_price),
                    stop_price: positive(cfg.stop_price),
                    placed_at: o.created_time.as_deref().and_then(|t| ts_rfc3339(t).ok()),
                    quote_ccy: quotes.get(&product).cloned(),
                    instrument: InstrumentId::new(VENUE, &product),
                    account: portfolio(o.retail_portfolio_id),
                    venue: None,
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Fills { pages } => {
            let page: Fills = serde_json::from_slice(body).map_err(|e| decode_err("fills", e))?;
            let quotes = s.quotes();
            let n = page.fills.len();
            let mut fills = Vec::with_capacity(n);
            for f in page.fills {
                let product = f.product_id.to_ascii_uppercase();
                let quote = quotes.get(&product).cloned();
                let price = f.price.0.unwrap_or_default();
                let size = f.size.0.unwrap_or_default();
                // `size_in_quote` means the size is money, not coins.
                let qty = if f.size_in_quote {
                    if price.is_zero() {
                        return Err(ConnectorError::decode(format!(
                            "Coinbase reported a {product} fill sized in {} at price 0, which cannot be converted to a quantity",
                            quote.as_deref().unwrap_or("quote currency")
                        )));
                    }
                    size.checked_div(price).ok_or_else(|| ConnectorError::decode(format!("{product}: {size} / {price} overflows")))?
                } else {
                    size
                };
                let at = f.trade_time.as_deref().ok_or_else(|| ConnectorError::decode("a Coinbase fill carries no readable trade time"))?;
                fills.push(Fill {
                    id: if f.entry_id.is_empty() { f.trade_id } else { f.entry_id },
                    order_id: Some(f.order_id),
                    instrument: InstrumentId::new(VENUE, &product),
                    ts: ts_rfc3339(at)?,
                    side: side(&f.side),
                    qty: Qty(qty),
                    price: Price(price),
                    fee: Money(f.commission.0.unwrap_or_default().abs()),
                    // Coinbase bills the commission in the product's quote currency.
                    fee_asset: quote.clone(),
                    quote_ccy: quote,
                    account: portfolio(f.retail_portfolio_id),
                    venue: None,
                    class: None,
                    multiplier: None,
                });
            }
            let next = (n >= PAGE && !page.cursor.is_empty()).then(|| format!("{}:{}", pages + 1, page.cursor));
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next)])
        }
    }
}
