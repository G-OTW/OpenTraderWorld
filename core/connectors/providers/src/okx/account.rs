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
//!   - Pending orders are the plain ones, then the untriggered algo orders
//!     (`orders-algo-pending`), one call per `ordType` the doc lists (`conditional` and `oco`
//!     together, as it allows), each paged by `after` on the last `algoId`. An algo order is
//!     `cond:<algoId>`: its cancel (`cancel-algos`) names the algo id and the instrument only.
//!     One type the venue refuses to list is logged and skipped; the others are read.
//!   - Positions and pending orders on a linear swap or future are stated in the base coin
//!     (`pos`, `sz`, `accFillSz` times `ctVal`), the unit the order path trades in, so a row
//!     read here and sent back (a close, an amend) means the same size. The contract sizes come
//!     from the public instrument list, read before the page that needs them when not already
//!     known; an inverse contract (`ctVal` in USD) or one the list does not name stays in
//!     contracts, valued by its multiplier.

use std::collections::HashMap;

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, Event, Fill, InstrumentId,
    Mapped, Money, Order, Position, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{Data, VENUE};

/// The books a full pull walks, one request type at a time.
const FILL_TYPES: &[&str] = &["SPOT", "MARGIN", "SWAP", "FUTURES", "OPTION"];
/// Rows a fill page returns at most.
const PAGE: usize = 100;
/// Hard stop on the whole pull.
const MAX_PAGES: u32 = 500;
/// The algo order types read after the plain orders, as the Get algo order list's `ordType`
/// names them ("conditional and oco both can be used and separated with comma").
const ALGO_TYPES: &[&str] = &["conditional,oco", "trigger", "move_order_stop", "chase", "iceberg", "twap", "smart_iceberg"];
/// The open-orders cursor of the algo lists: `algo:<type index>:<after>`.
const ALGO_CURSOR: &str = "algo:";

pub enum Pending {
    Balance { kind: AccountKind },
    Positions,
    Orders,
    /// The algo orders of one `ordType` ([`ALGO_TYPES`] index).
    AlgoOrders { idx: usize },
    Fills { cur: FillCursor },
    /// The contract sizes of one instrument type, read for a page held until they are known.
    Terms,
}

/// The `ordType` an algo list index reads.
pub fn algo_type(idx: usize) -> &'static str {
    ALGO_TYPES.get(idx).copied().unwrap_or_default()
}

/// One contract's size: `ctVal` base coins on a linear contract; an inverse one (`ctVal` in
/// USD) is kept in contracts.
#[derive(Clone, Copy)]
struct Size {
    ct_val: Decimal,
    linear: bool,
}

/// A page held until the contract sizes its rows need are read.
struct Held {
    pending: Pending,
    body: Vec<u8>,
    waiting: usize,
}

/// The contract sizes the account read states its sizes with, the pages waiting for them, and
/// the size reads to send next.
#[derive(Default)]
pub struct Terms {
    sizes: HashMap<String, Size>,
    held: HashMap<CorrId, Held>,
    next: Vec<(CorrId, NativeRequest)>,
}

impl Terms {
    /// One instrument row's contract size (`ctVal`, `ctType`); a row without one (spot) is none.
    pub fn learn(&mut self, inst_id: &str, ct_val: Option<Decimal>, ct_type: &str) {
        if let Some(v) = ct_val.filter(|v| *v > Decimal::ZERO) {
            self.sizes.insert(inst_id.to_ascii_uppercase(), Size { ct_val: v, linear: ct_type == "linear" });
        }
    }

    pub fn forget(&mut self, corr: CorrId) {
        self.held.remove(&corr);
        self.next.retain(|(c, _)| *c != corr);
    }

    pub fn follow_ups(&mut self, corr: CorrId) -> Vec<NativeRequest> {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.next).into_iter().partition(|(c, _)| *c == corr);
        self.next = rest;
        mine.into_iter().map(|(_, r)| r).collect()
    }

    /// `v` in the base coin, and whether it was turned into it: a linear contract's count
    /// times its `ctVal`; anything else as written.
    fn base(&self, inst_type: &str, inst_id: &str, v: Decimal) -> (Decimal, bool) {
        if contract_type(inst_type, inst_id).is_none() {
            return (v, false);
        }
        match self.sizes.get(&inst_id.to_ascii_uppercase()) {
            Some(s) if s.linear => (v * s.ct_val, true),
            _ => (v, false),
        }
    }

    /// The instrument types whose list a contract row of `ids` needs, its size not known yet.
    fn missing<'a>(&self, ids: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = ids
            .filter(|(_, id)| !self.sizes.contains_key(&id.to_ascii_uppercase()))
            .filter_map(|(t, id)| contract_type(t, id))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// `SWAP` or `FUTURES` for a contract row: its `instType`, else its id as the doc's naming
/// table writes it (`BTC-USDT-SWAP`, `BTC-USD-250808`); `None` for anything else.
fn contract_type(inst_type: &str, inst_id: &str) -> Option<&'static str> {
    match inst_type {
        "SWAP" => Some("SWAP"),
        "FUTURES" => Some("FUTURES"),
        "" => {
            let parts: Vec<&str> = inst_id.split('-').collect();
            if parts.len() == 3 && parts[2].eq_ignore_ascii_case("SWAP") {
                Some("SWAP")
            } else if parts.len() == 3 && !parts[2].is_empty() && parts[2].bytes().all(|b| b.is_ascii_digit()) {
                Some("FUTURES")
            } else {
                None
            }
        }
        _ => None,
    }
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

type PendingMap = HashMap<(CorrId, &'static str), super::Pending>;

/// One account read on the host of the account's region (`base`).
pub fn encode(base: &'static str, cmd: &Command, pending: &mut PendingMap) -> Result<Vec<NativeRequest>, ConnectorError> {
    let signed = |endpoint: &'static str, path: &'static str, req_id: CorrId| HttpRequest::get(endpoint, base, path, req_id).signed();
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let (req, p) = match kind {
        AccountKind::Summary | AccountKind::Balances => (signed("balance", "/api/v5/account/balance", *req_id), Pending::Balance { kind: *kind }),
        AccountKind::Positions => (signed("positions", "/api/v5/account/positions", *req_id), Pending::Positions),
        AccountKind::OpenOrders => match cursor.as_deref().and_then(|c| c.strip_prefix(ALGO_CURSOR)) {
            None => (signed("ordersPending", "/api/v5/trade/orders-pending", *req_id).query("limit", PAGE), Pending::Orders),
            Some(rest) => {
                let bad = || ConnectorError::invalid(format!("bad cursor `{rest}`"));
                let (idx, after) = rest.split_once(':').unwrap_or((rest, ""));
                let idx: usize = idx.parse().map_err(|_| bad())?;
                let ord_type = ALGO_TYPES.get(idx).ok_or_else(bad)?;
                let mut h = signed("ordersAlgoPending", "/api/v5/trade/orders-algo-pending", *req_id).query("ordType", ord_type).query("limit", PAGE);
                if !after.is_empty() {
                    h = h.query("after", after);
                }
                (h, Pending::AlgoOrders { idx })
            }
        },
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
    #[serde(default)]
    inst_type: String,
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
    #[serde(default)]
    inst_type: String,
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

/// A row of Get algo order list.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlgoRow {
    #[serde(default)]
    inst_type: String,
    inst_id: String,
    #[serde(default)]
    algo_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    ord_type: String,
    #[serde(default)]
    sz: OptDec,
    #[serde(default)]
    trigger_px: OptDec,
    #[serde(default)]
    sl_trigger_px: OptDec,
    #[serde(default)]
    tp_trigger_px: OptDec,
    /// "Only applicable to move_order_stop order".
    #[serde(default)]
    move_trigger_px: OptDec,
    /// "Order price for the trigger order"; `-1` at the market.
    #[serde(default)]
    ord_px: OptDec,
    #[serde(default)]
    sl_ord_px: OptDec,
    #[serde(default)]
    tp_ord_px: OptDec,
    /// "Only applicable to iceberg order or twap order".
    #[serde(default)]
    px_limit: OptDec,
    #[serde(default)]
    c_time: Option<Int>,
}

/// An instrument row, for its contract size only.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TermsRow {
    #[serde(default)]
    inst_id: String,
    #[serde(default)]
    ct_val: OptDec,
    #[serde(default)]
    ct_type: String,
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

/// The chunk of an algo list the venue refused: no rows, and the next list.
pub fn skip_algo(idx: usize, corr: CorrId, ctx: &MapCtx<'_>) -> Vec<Envelope> {
    let next = (idx + 1 < ALGO_TYPES.len()).then(|| format!("{ALGO_CURSOR}{}", idx + 1));
    vec![chunk(ctx, corr, AccountItems::Orders(Vec::new()), next)]
}

/// The contract rows of a positions or orders page: (`instType`, `instId`).
fn contract_ids(p: &Pending, body: &[u8]) -> Result<Vec<(String, String)>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        #[serde(default)]
        inst_type: String,
        #[serde(default)]
        inst_id: String,
        #[serde(default)]
        pos: OptDec,
    }
    if !matches!(p, Pending::Positions | Pending::Orders | Pending::AlgoOrders { .. }) {
        return Ok(Vec::new());
    }
    let page: Data<Row> = serde_json::from_slice(body).map_err(|e| decode_err("account page", e))?;
    // A flat position is skipped below: its size needs no contract size.
    let held = |r: &Row| !matches!(p, Pending::Positions) || r.pos.0.is_some_and(|q| !q.is_zero());
    Ok(page.data.into_iter().filter(held).map(|r| (r.inst_type, r.inst_id)).collect())
}

pub fn decode(p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>, pending: &mut PendingMap, terms: &mut Terms) -> Result<Vec<Envelope>, ConnectorError> {
    if let Pending::Terms = p {
        let page: Data<TermsRow> = serde_json::from_slice(body).map_err(|e| decode_err("instruments", e))?;
        for r in &page.data {
            terms.learn(&r.inst_id, r.ct_val.0, &r.ct_type);
        }
        let Some(h) = terms.held.get_mut(&corr) else { return Ok(Vec::new()) };
        h.waiting = h.waiting.saturating_sub(1);
        if h.waiting > 0 {
            return Ok(Vec::new());
        }
        let Some(h) = terms.held.remove(&corr) else { return Ok(Vec::new()) };
        return rows(h.pending, corr, &h.body, ctx, terms);
    }
    let ids = contract_ids(&p, body)?;
    let need = terms.missing(ids.iter().map(|(t, i)| (t.as_str(), i.as_str())));
    if need.is_empty() {
        return rows(p, corr, body, ctx, terms);
    }
    // Hold the page; read the lists that state its contract sizes first (public, the global
    // host, as the instrument load reads them).
    for t in &need {
        let endpoint = if *t == "SWAP" { "termsSwap" } else { "termsFutures" };
        let req = HttpRequest::get(endpoint, super::REST, "/api/v5/public/instruments", corr).query("instType", *t);
        pending.insert((corr, endpoint), super::Pending::Account(Pending::Terms));
        terms.next.push((corr, NativeRequest::Http(req)));
    }
    terms.held.insert(corr, Held { pending: p, body: body.to_vec(), waiting: need.len() });
    Ok(Vec::new())
}

fn rows(p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>, terms: &Terms) -> Result<Vec<Envelope>, ConnectorError> {
    let base = |t: &str, id: &str, v: Decimal| {
        let (q, done) = terms.base(t, id, v);
        if !done && !v.is_zero() && contract_type(t, id).is_some() {
            tracing::debug!("okx: {id} is read in contracts (no linear contract size known)");
        }
        (q, done)
    };
    match p {
        Pending::Terms => Ok(Vec::new()),
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
                // In the base coin on a linear contract: one coin is worth one coin.
                let (qty, coins) = base(&p.inst_type, &p.inst_id, qty);
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
                    multiplier: coins.then_some(Decimal::ONE),
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
                    qty: Qty(base(&o.inst_type, &o.inst_id, o.sz.0.unwrap_or_default()).0),
                    filled_qty: Qty(base(&o.inst_type, &o.inst_id, o.acc_fill_sz.0.unwrap_or_default()).0),
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
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), Some(format!("{ALGO_CURSOR}0")))])
        }
        Pending::AlgoOrders { idx } => {
            let page: Data<AlgoRow> = serde_json::from_slice(body).map_err(|e| decode_err("orders-algo-pending", e))?;
            let n = page.data.len();
            let last = page.data.last().map(|o| o.algo_id.clone()).filter(|a| !a.is_empty());
            let mut out = Vec::with_capacity(n);
            for o in page.data {
                let id = o.inst_id.to_ascii_uppercase();
                let stop = [o.trigger_px, o.sl_trigger_px, o.tp_trigger_px, o.move_trigger_px].into_iter().find_map(positive);
                let limit = [o.ord_px, o.sl_ord_px, o.tp_ord_px, o.px_limit].into_iter().find_map(positive);
                out.push(Order {
                    id: format!("cond:{}", o.algo_id),
                    side: side(&o.side),
                    order_type: o.ord_type,
                    qty: Qty(base(&o.inst_type, &o.inst_id, o.sz.0.unwrap_or_default()).0),
                    filled_qty: Qty(Decimal::ZERO),
                    limit_price: limit,
                    stop_price: stop,
                    placed_at: o.c_time.map(|t| ts_millis(t.0)).transpose()?,
                    quote_ccy: Some(dash_quote(&id).unwrap_or_default()),
                    instrument: InstrumentId::new(VENUE, &id),
                    account: None,
                    venue: None,
                    class: None,
                });
            }
            let next = match last {
                Some(a) if n >= PAGE => Some(format!("{ALGO_CURSOR}{idx}:{a}")),
                _ => (idx + 1 < ALGO_TYPES.len()).then(|| format!("{ALGO_CURSOR}{}", idx + 1)),
            };
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(out), next)])
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
