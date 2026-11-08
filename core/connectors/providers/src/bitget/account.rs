//! Bitget accounts, read only, one book per adapter.
//!
//!   - Spot: balances (available + frozen + locked), unfilled orders, fills over 90-day
//!     slices walked backwards from the period's end, each paged by `idLessThan` on the last
//!     row's `tradeId`.
//!   - USDT-M futures: positions with their side, pending orders, the futures equity, fills
//!     over 7-day slices, paged by the `endId` the page hands back.
//!
//! Bitget signs a charge negative; a cost is filed positive. Each book has its own budget of
//! 500 pages per pull.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, Event, Fill, InstrumentId,
    Mapped, Money, Order, Position, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Book, Data, FUTURES, REST};

const PAGE: usize = 100;
const MAX_PAGES: u32 = 500;
const SPOT_SLICE_MS: i64 = 90 * 86_400_000;
const FUTURES_SLICE_MS: i64 = 7 * 86_400_000;

pub enum Pending {
    Summary,
    Balances,
    Positions,
    Orders,
    Fills { cur: FillCursor, from_ms: i64 },
}

/// Where a fill walk stands: the slice's end, the id to read below, pages used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillCursor {
    pub slice_to: i64,
    pub below: Option<String>,
    pub pages: u32,
}

impl FillCursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}", self.slice_to, self.below.as_deref().unwrap_or(""), self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
        let parts: Vec<&str> = c.split(':').collect();
        let [to, below, pages] = parts.as_slice() else { return Err(bad()) };
        Ok(FillCursor {
            slice_to: to.parse().map_err(|_| bad())?,
            below: Some(below.to_string()).filter(|b| !b.is_empty()),
            pages: pages.parse().map_err(|_| bad())?,
        })
    }
}

fn slice_ms(book: Book) -> i64 {
    match book {
        Book::Spot => SPOT_SLICE_MS,
        Book::UsdtFutures => FUTURES_SLICE_MS,
    }
}

fn signed(endpoint: &'static str, path: &'static str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed()
}

pub fn encode(book: Book, cmd: &Command, pending: &mut std::collections::HashMap<CorrId, super::Pending>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let (req, p) = match (book, kind) {
        (Book::Spot, AccountKind::Summary) => (signed("assets", "/api/v2/spot/account/assets", *req_id), Pending::Summary),
        (Book::Spot, AccountKind::Balances) => (signed("assets", "/api/v2/spot/account/assets", *req_id).query("assetType", "hold_only"), Pending::Balances),
        (Book::Spot, AccountKind::OpenOrders) => (signed("unfilledOrders", "/api/v2/spot/trade/unfilled-orders", *req_id).query("limit", PAGE), Pending::Orders),
        (Book::UsdtFutures, AccountKind::Summary) => (signed("accounts", "/api/v2/mix/account/accounts", *req_id).query("productType", FUTURES), Pending::Summary),
        (Book::UsdtFutures, AccountKind::Positions) => {
            (signed("allPosition", "/api/v2/mix/position/all-position", *req_id).query("productType", FUTURES), Pending::Positions)
        }
        (Book::UsdtFutures, AccountKind::OpenOrders) => (
            signed("ordersPending", "/api/v2/mix/order/orders-pending", *req_id).query("productType", FUTURES).query("limit", PAGE),
            Pending::Orders,
        ),
        (_, AccountKind::Fills) => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            let (from_ms, to_ms) = (range.from.millis(), range.to.millis());
            if to_ms <= from_ms {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let cur = match cursor {
                Some(c) => FillCursor::parse(c)?,
                None => FillCursor { slice_to: to_ms, below: None, pages: 0 },
            };
            if cur.pages >= MAX_PAGES {
                return Err(ConnectorError::invalid(match book {
                    Book::Spot => "this period holds more Bitget spot fills than one pull reads. Ask for a shorter period.",
                    Book::UsdtFutures => {
                        "this period holds more Bitget futures fills than one pull reads. Bitget answers futures fills a week at a time, so ask for a shorter period."
                    }
                }));
            }
            let slice_from = (cur.slice_to - slice_ms(book)).max(from_ms);
            let mut h = match book {
                Book::Spot => signed("fills", "/api/v2/spot/trade/fills", *req_id),
                Book::UsdtFutures => signed("fillHistory", "/api/v2/mix/order/fill-history", *req_id).query("productType", FUTURES),
            }
            .query("startTime", slice_from)
            .query("endTime", cur.slice_to)
            .query("limit", PAGE);
            if let Some(b) = &cur.below {
                h = h.query("idLessThan", b);
            }
            (h, Pending::Fills { cur, from_ms })
        }
        (b, k) => return Err(ConnectorError::unsupported(format!("Bitget {b:?} has no {k:?}"))),
    };
    pending.insert(*req_id, super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

#[derive(Deserialize)]
struct SpotAsset {
    coin: String,
    #[serde(default)]
    available: OptDec,
    #[serde(default)]
    frozen: OptDec,
    #[serde(default)]
    locked: OptDec,
}

impl SpotAsset {
    /// Everything a spot line holds: free, frozen by an order, and locked.
    fn held(&self) -> Decimal {
        self.available.0.unwrap_or_default() + self.frozen.0.unwrap_or_default() + self.locked.0.unwrap_or_default()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesAccount {
    #[serde(default)]
    usdt_equity: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    symbol: String,
    #[serde(default)]
    total: OptDec,
    #[serde(default)]
    hold_side: String,
    #[serde(default)]
    open_price_avg: OptDec,
    #[serde(default)]
    margin_coin: String,
    #[serde(default)]
    unrealized_pl: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    symbol: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    base_volume: OptDec,
    /// Spot: the order's price.
    #[serde(default)]
    price_avg: OptDec,
    #[serde(default)]
    trigger_price: OptDec,
    /// Futures: the order's price and its preset stop.
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    preset_stop_loss_price: OptDec,
    #[serde(default)]
    margin_coin: String,
    #[serde(default)]
    c_time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesOrders {
    #[serde(default)]
    entrusted_list: Option<Vec<OrderRow>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeeLine {
    #[serde(default)]
    fee_coin: String,
    #[serde(default)]
    total_fee: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotFill {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    trade_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    price_avg: OptDec,
    #[serde(default)]
    size: OptDec,
    c_time: Int,
    #[serde(default)]
    fee_detail: Option<FeeLine>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesFill {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    trade_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    base_volume: OptDec,
    c_time: Int,
    #[serde(default)]
    margin_coin: String,
    #[serde(default)]
    fee_detail: Option<Vec<FeeLine>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesFills {
    #[serde(default)]
    fill_list: Option<Vec<FuturesFill>>,
    #[serde(default)]
    end_id: Option<String>,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("bitget {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("bitget", "side", raw),
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

fn upper(s: &str) -> Option<String> {
    Some(s.to_ascii_uppercase()).filter(|s| !s.is_empty())
}

pub fn decode(book: Book, p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let venue = book.venue();
    match (book, p) {
        (Book::Spot, Pending::Summary) => {
            let page: Data<Vec<SpotAsset>> = serde_json::from_slice(body).map_err(|e| decode_err("assets", e))?;
            let funded = page.data.iter().filter(|a| a.held().is_sign_positive() && !a.held().is_zero()).count();
            Ok(vec![env(ctx, corr, Event::AccountSummary(summary(format!("spot: {funded} coin(s) with a balance"))))])
        }
        (Book::UsdtFutures, Pending::Summary) => {
            let page: Data<Vec<FuturesAccount>> = serde_json::from_slice(body).map_err(|e| decode_err("accounts", e))?;
            let equity: Decimal = page.data.iter().map(|a| a.usdt_equity.0.unwrap_or_default()).sum();
            // Rounded, not cut: `Decimal` precision formatting truncates.
            let equity = equity.round_dp(2);
            Ok(vec![env(ctx, corr, Event::AccountSummary(summary(format!("USDT-M futures: {equity:.2} USDT of equity"))))])
        }
        (_, Pending::Balances) => {
            let page: Data<Vec<SpotAsset>> = serde_json::from_slice(body).map_err(|e| decode_err("assets", e))?;
            let held = page
                .data
                .iter()
                .filter(|a| a.held().is_sign_positive() && !a.held().is_zero())
                .map(|a| Balance { asset: a.coin.to_ascii_uppercase(), total: Money(a.held()), available: Money(a.available.0.unwrap_or_default()), name: None, fiat: false, account: None })
                .collect();
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)])
        }
        (_, Pending::Positions) => {
            let page: Data<Vec<PositionRow>> = serde_json::from_slice(body).map_err(|e| decode_err("all-position", e))?;
            let mut out = Vec::new();
            for p in page.data {
                let Some(total) = p.total.0.filter(|t| !t.is_zero()) else { continue };
                let qty = if p.hold_side == "short" { -total.abs() } else { total.abs() };
                out.push(Position {
                    instrument: InstrumentId::new(venue, &p.symbol.to_ascii_uppercase()),
                    qty: Qty(qty),
                    avg_price: positive(p.open_price_avg),
                    unrealized_pnl: p.unrealized_pl.0.map(Money),
                    currency: upper(&p.margin_coin),
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
        (b, Pending::Orders) => {
            let rows: Vec<OrderRow> = match b {
                Book::Spot => serde_json::from_slice::<Data<Vec<OrderRow>>>(body).map_err(|e| decode_err("unfilled-orders", e))?.data,
                Book::UsdtFutures => serde_json::from_slice::<Data<Option<FuturesOrders>>>(body)
                    .map_err(|e| decode_err("orders-pending", e))?
                    .data
                    .and_then(|d| d.entrusted_list)
                    .unwrap_or_default(),
            };
            let mut out = Vec::new();
            for o in rows {
                let (limit, stop, quote) = match b {
                    Book::Spot => (positive(o.price_avg), positive(o.trigger_price), None),
                    Book::UsdtFutures => (positive(o.price), positive(o.preset_stop_loss_price), Some(o.margin_coin.to_ascii_uppercase())),
                };
                out.push(Order {
                    id: o.order_id,
                    instrument: InstrumentId::new(venue, &o.symbol.to_ascii_uppercase()),
                    side: side(&o.side),
                    order_type: o.order_type,
                    qty: Qty(o.size.0.unwrap_or_default()),
                    filled_qty: Qty(o.base_volume.0.unwrap_or_default()),
                    limit_price: limit,
                    stop_price: stop,
                    placed_at: o.c_time.map(|t| ts_millis(t.0)).transpose()?,
                    quote_ccy: quote,
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        (b, Pending::Fills { cur, from_ms }) => {
            let (fills, rows_len, below) = match b {
                Book::Spot => {
                    let page: Data<Vec<SpotFill>> = serde_json::from_slice(body).map_err(|e| decode_err("fills", e))?;
                    let below = page.data.last().map(|r| r.trade_id.clone()).filter(|s| !s.is_empty());
                    let n = page.data.len();
                    let mut fills = Vec::with_capacity(n);
                    for r in page.data {
                        if r.symbol.is_empty() {
                            continue;
                        }
                        let fee = r.fee_detail.as_ref();
                        fills.push(Fill {
                            id: r.trade_id,
                            order_id: Some(r.order_id),
                            instrument: InstrumentId::new(venue, &r.symbol.to_ascii_uppercase()),
                            ts: ts_millis(r.c_time.0)?,
                            side: side(&r.side),
                            qty: Qty(r.size.0.unwrap_or_default()),
                            price: Price(r.price_avg.0.unwrap_or_default()),
                            fee: Money(fee.and_then(|f| f.total_fee.0).unwrap_or_default().abs()),
                            fee_asset: Some(fee.map(|f| f.fee_coin.to_ascii_uppercase()).unwrap_or_default()),
                            quote_ccy: None,
                            account: None,
                            venue: None,
                            class: None,
                            multiplier: None,
                        });
                    }
                    (fills, n, below)
                }
                Book::UsdtFutures => {
                    let page: Data<Option<FuturesFills>> = serde_json::from_slice(body).map_err(|e| decode_err("fill-history", e))?;
                    let page = page.data.unwrap_or(FuturesFills { fill_list: None, end_id: None });
                    let rows = page.fill_list.unwrap_or_default();
                    let n = rows.len();
                    let mut fills = Vec::with_capacity(n);
                    for r in rows {
                        if r.symbol.is_empty() {
                            continue;
                        }
                        // One fee line per coin it was charged in.
                        let lines = r.fee_detail.unwrap_or_default();
                        let fee: Decimal = lines.iter().map(|f| f.total_fee.0.unwrap_or_default().abs()).sum();
                        fills.push(Fill {
                            id: r.trade_id,
                            order_id: Some(r.order_id),
                            instrument: InstrumentId::new(venue, &r.symbol.to_ascii_uppercase()),
                            ts: ts_millis(r.c_time.0)?,
                            side: side(&r.side),
                            qty: Qty(r.base_volume.0.unwrap_or_default()),
                            price: Price(r.price.0.unwrap_or_default()),
                            fee: Money(fee),
                            fee_asset: Some(lines.first().map(|f| f.fee_coin.to_ascii_uppercase()).unwrap_or_default()),
                            quote_ccy: upper(&r.margin_coin),
                            account: None,
                            venue: None,
                            class: None,
                            multiplier: None,
                        });
                    }
                    (fills, n, page.end_id.filter(|s| !s.is_empty()))
                }
            };
            let pages = cur.pages + 1;
            let slice_from = (cur.slice_to - slice_ms(b)).max(from_ms);
            let next_slice = || (slice_from > from_ms).then_some(FillCursor { slice_to: slice_from, below: None, pages });
            let next = if rows_len < PAGE {
                next_slice()
            } else {
                match below {
                    Some(id) => Some(FillCursor { slice_to: cur.slice_to, below: Some(id), pages }),
                    None => next_slice(),
                }
            };
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))])
        }
    }
}

fn summary(detail: String) -> AccountSummary {
    AccountSummary { account_type: None, permissions: vec![], assets_held: 0, can_trade: None, open_positions: None, wallet_balance: None, detail: Some(detail) }
}
