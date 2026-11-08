//! OKX account, read only: fills across every book, the unified balance, positions, pending
//! orders and the key's own summary.
//!
//! What the API's shape forces:
//!   - `fills-history` is the 90-day archive and is asked one instrument type at a time, so
//!     a pull walks SPOT, MARGIN, SWAP, FUTURES and OPTION in turn, each paged by the last
//!     row's `billId`; 500 pages at most for the whole pull.
//!   - A derivative fill is sized in contracts; what a contract is worth comes from the
//!     instrument list (the caller's business), never assumed to be one.
//!   - OKX signs a charge negative and a rebate positive; the canonical fee is a cost, so
//!     the sign flips here and a rebate stays a rebate.

use std::collections::HashMap;

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Envelope, Event, Fill, InstrumentId,
    Mapped, Money, Order, Position, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Data, REST, VENUE};

/// The books a full pull walks, one request type at a time.
const FILL_TYPES: &[&str] = &["SPOT", "MARGIN", "SWAP", "FUTURES", "OPTION"];
/// Rows a fill page returns at most.
const PAGE: usize = 100;
/// Hard stop on the whole pull.
const MAX_PAGES: u32 = 500;

pub enum Pending {
    Balance { kind: AccountKind },
    Positions,
    Orders,
    Fills { cur: FillCursor },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillCursor {
    pub type_idx: usize,
    pub after: Option<String>,
    pub pages: u32,
}

impl FillCursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}", self.type_idx, self.after.as_deref().unwrap_or(""), self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
        let parts: Vec<&str> = c.split(':').collect();
        let [t, after, pages] = parts.as_slice() else { return Err(bad()) };
        Ok(FillCursor {
            type_idx: t.parse().map_err(|_| bad())?,
            after: Some(after.to_string()).filter(|a| !a.is_empty()),
            pages: pages.parse().map_err(|_| bad())?,
        })
    }
}

fn signed(endpoint: &'static str, path: &'static str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).signed()
}

type PendingMap = HashMap<(CorrId, &'static str), super::Pending>;

pub fn encode(cmd: &Command, pending: &mut PendingMap) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let (req, p) = match kind {
        AccountKind::Summary | AccountKind::Balances => (signed("balance", "/api/v5/account/balance", *req_id), Pending::Balance { kind: *kind }),
        AccountKind::Positions => (signed("positions", "/api/v5/account/positions", *req_id), Pending::Positions),
        AccountKind::OpenOrders => (signed("ordersPending", "/api/v5/trade/orders-pending", *req_id).query("limit", PAGE), Pending::Orders),
        AccountKind::Fills => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            if range.to <= range.from {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let cur = match cursor {
                Some(c) => FillCursor::parse(c)?,
                None => FillCursor { type_idx: 0, after: None, pages: 0 },
            };
            if cur.pages >= MAX_PAGES {
                return Err(ConnectorError::invalid("this period holds more OKX fills than one pull reads. Ask for a shorter period."));
            }
            let inst_type = FILL_TYPES.get(cur.type_idx).ok_or_else(|| ConnectorError::invalid("cursor past the book list"))?;
            let mut h = signed("fillsHistory", "/api/v5/trade/fills-history", *req_id)
                .query("instType", inst_type)
                .query("begin", range.from.millis())
                .query("end", range.to.millis())
                .query("limit", PAGE);
            if let Some(a) = &cur.after {
                h = h.query("after", a);
            }
            (h, Pending::Fills { cur })
        }
    };
    pending.insert((*req_id, req.endpoint_id), super::Pending::Account(p));
    Ok(vec![NativeRequest::Http(req)])
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BalanceRow {
    #[serde(default)]
    total_eq: Option<String>,
    #[serde(default)]
    details: Vec<BalanceDetail>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BalanceDetail {
    ccy: String,
    #[serde(default)]
    eq: OptDec,
    #[serde(default)]
    avail_bal: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    inst_id: String,
    #[serde(default)]
    pos: OptDec,
    #[serde(default)]
    pos_side: String,
    #[serde(default)]
    avg_px: OptDec,
    #[serde(default)]
    upl: OptDec,
    #[serde(default)]
    ccy: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    inst_id: String,
    #[serde(default)]
    ord_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    ord_type: String,
    #[serde(default)]
    sz: OptDec,
    #[serde(default)]
    acc_fill_sz: OptDec,
    #[serde(default)]
    px: OptDec,
    #[serde(default)]
    sl_trigger_px: OptDec,
    #[serde(default)]
    c_time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FillRow {
    #[serde(default)]
    inst_id: String,
    #[serde(default)]
    trade_id: String,
    #[serde(default)]
    ord_id: String,
    #[serde(default)]
    bill_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    fill_sz: OptDec,
    #[serde(default)]
    fill_px: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_ccy: String,
    ts: Int,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("okx {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("okx", "side", raw),
    }
}

/// The quote coin of an instrument id: OKX writes them dash-separated, so the second field
/// is the quote (`BTC-USDT`, `BTC-USDT-SWAP`, `BTC-USD-241227`). The fallback when the
/// instrument list does not know the id.
fn dash_quote(inst_id: &str) -> Option<String> {
    inst_id.split('-').nth(1).map(str::to_ascii_uppercase).filter(|q| !q.is_empty())
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match p {
        Pending::Balance { kind } => {
            let page: Data<BalanceRow> = serde_json::from_slice(body).map_err(|e| decode_err("balance", e))?;
            let acct = page.data.into_iter().next();
            let details = acct.as_ref().map(|a| a.details.as_slice()).unwrap_or_default();
            let held: Vec<Balance> = details
                .iter()
                .filter_map(|d| {
                    let eq = d.eq.0.filter(|e| e.is_sign_positive() && !e.is_zero())?;
                    Some(Balance { asset: d.ccy.to_ascii_uppercase(), total: Money(eq), available: Money(d.avail_bal.0.unwrap_or(eq)), name: None, fiat: false, account: None })
                })
                .collect();
            if kind == AccountKind::Summary {
                let total = acct.as_ref().and_then(|a| a.total_eq.clone()).unwrap_or_default();
                return Ok(vec![env(
                    ctx,
                    corr,
                    Event::AccountSummary(AccountSummary {
                        account_type: None,
                        permissions: vec![],
                        assets_held: u32::try_from(held.len()).unwrap_or(u32::MAX),
                        can_trade: None,
                        open_positions: None,
                        wallet_balance: Some(total.clone()),
                        detail: Some(format!("key accepted, {} currency(ies) with a balance, total equity {total} USD", held.len())),
                    }),
                )]);
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)])
        }
        Pending::Positions => {
            let page: Data<PositionRow> = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
            let mut out = Vec::new();
            for p in page.data {
                let Some(pos) = p.pos.0.filter(|q| !q.is_zero()) else { continue };
                // Hedge mode names the side on `posSide`; one-way mode puts it in the sign.
                let qty = match p.pos_side.as_str() {
                    "short" => -pos.abs(),
                    "long" => pos.abs(),
                    _ => pos,
                };
                out.push(Position {
                    instrument: InstrumentId::new(VENUE, &p.inst_id.to_ascii_uppercase()),
                    qty: Qty(qty),
                    avg_price: positive(p.avg_px),
                    unrealized_pnl: p.upl.0.map(Money),
                    currency: Some(p.ccy).filter(|c| !c.is_empty()),
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
            let page: Data<OrderRow> = serde_json::from_slice(body).map_err(|e| decode_err("orders-pending", e))?;
            let mut out = Vec::new();
            for o in page.data {
                let id = o.inst_id.to_ascii_uppercase();
                out.push(Order {
                    id: o.ord_id,
                    side: side(&o.side),
                    order_type: o.ord_type,
                    qty: Qty(o.sz.0.unwrap_or_default()),
                    filled_qty: Qty(o.acc_fill_sz.0.unwrap_or_default()),
                    limit_price: positive(o.px),
                    // A conditional order carries its trigger separately; a plain one has none.
                    stop_price: positive(o.sl_trigger_px),
                    placed_at: o.c_time.map(|t| ts_millis(t.0)).transpose()?,
                    quote_ccy: Some(dash_quote(&id).unwrap_or_default()),
                    instrument: InstrumentId::new(VENUE, &id),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), None)])
        }
        Pending::Fills { cur } => {
            let page: Data<FillRow> = serde_json::from_slice(body).map_err(|e| decode_err("fills-history", e))?;
            let rows = page.data;
            let mut fills = Vec::with_capacity(rows.len());
            for r in &rows {
                let id = r.inst_id.to_ascii_uppercase();
                if id.is_empty() {
                    continue;
                }
                fills.push(Fill {
                    id: r.trade_id.clone(),
                    order_id: Some(r.ord_id.clone()),
                    ts: ts_millis(r.ts.0)?,
                    side: side(&r.side),
                    // Contracts on a derivative, coins on spot: the multiplier carries the difference.
                    qty: Qty(r.fill_sz.0.unwrap_or_default()),
                    price: Price(r.fill_px.0.unwrap_or_default()),
                    fee: Money(-r.fee.0.unwrap_or_default()),
                    fee_asset: Some(r.fee_ccy.to_ascii_uppercase()),
                    quote_ccy: dash_quote(&id),
                    instrument: InstrumentId::new(VENUE, &id),
                    account: None,
                    venue: None,
                    class: None,
                    multiplier: None,
                });
            }
            let pages = cur.pages + 1;
            let next_type = || (cur.type_idx + 1 < FILL_TYPES.len()).then(|| FillCursor { type_idx: cur.type_idx + 1, after: None, pages });
            let next = if rows.is_empty() || rows.len() < PAGE {
                next_type()
            } else {
                match rows.last().map(|r| r.bill_id.clone()).filter(|b| !b.is_empty()) {
                    Some(b) => Some(FillCursor { type_idx: cur.type_idx, after: Some(b), pages }),
                    None => next_type(),
                }
            };
            Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))])
        }
    }
}
