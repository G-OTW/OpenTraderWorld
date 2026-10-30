//! Bybit unified trading account, read only, one book per adapter, on the host of the site
//! the key belongs to.
//!
//!   - Spot (bybit.com or Bybit EU): wallet balances, open orders, fills.
//!   - Linear: positions and open orders, each walked per settle coin (USDT, then USDC: the
//!     venue answers one settle coin per call), fills.
//!   - Inverse: positions and open orders, converted from USD contracts to coin quantities.
//!     No fills (refused by name in the mapper: the journal cannot count their profit).
//!   - Options: positions and open orders in one call each; fills walked per base coin
//!     (without one, the venue answers BTC alone), the coins read from the option list.
//!   - Every book: the summary is the key's own description (read-only or not, unified or
//!     classic) followed by the unified wallet's equity.
//!
//! Fills are answered seven days per call at most, so a period is walked in 7-day slices
//! backwards from its end, each paged by `nextPageCursor`. Bybit signs a maker rebate
//! negative and a charge positive, which is already the canonical sign: kept as is.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, AssetClass, Balance, Command, ConnectorError, CorrId, Decimal, Envelope, ErrorKind, Event,
    Fill, InstrumentId, Mapped, Money, Order, Position, Price, Qty, Side,
};
use otw_connector_sdk::parse::{ts_millis, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::{per, Answer, Category, List};

const FILL_PAGE: usize = 100;
const ORDER_PAGE: usize = 50;
const POSITION_PAGE: usize = 200;
const COIN_PAGE: usize = 1000;
const MAX_PAGES: u32 = 2000;
const SLICE_MS: i64 = 7 * 86_400_000;
/// Settle coins of the linear category, walked in turn.
const SETTLE: &[&str] = &["USDT", "USDC"];

pub struct Pending {
    /// The base key of the host the key belongs to, named again in a refusal.
    base: &'static str,
    step: Step,
}

enum Step {
    /// The key's description, then the wallet.
    Key,
    Wallet { read_only: bool, permissions: Vec<String> },
    Balances,
    Positions { settle: usize },
    Orders { settle: usize },
    /// Options: reading the option list for its base coins, before any fill.
    Coins { coins: Vec<String> },
    Fills { cur: FillCursor, from_ms: i64 },
}

/// Where a fill walk stands: the base coin (options only), the slice's end, the venue's
/// page cursor, pages used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillCursor {
    /// Options: every base coin to walk, and the one being read.
    pub coins: Vec<String>,
    pub coin: usize,
    /// The slice's end; `None` = the period's end (a walk, or a coin, just starting).
    pub slice_to: Option<i64>,
    pub page: Option<String>,
    pub pages: u32,
}

impl FillCursor {
    fn encode(&self) -> String {
        let to = self.slice_to.map(|t| t.to_string()).unwrap_or_default();
        format!("f|{}|{}|{to}|{}|{}", self.coins.join(","), self.coin, self.pages, self.page.as_deref().unwrap_or(""))
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
        // The venue's own cursor is last: it may hold anything but the split is bounded.
        let mut parts = c.splitn(6, '|');
        let (Some("f"), Some(coins), Some(coin), Some(to), Some(pages), Some(page)) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(bad());
        };
        Ok(FillCursor {
            coins: coins.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            coin: coin.parse().map_err(|_| bad())?,
            slice_to: if to.is_empty() { None } else { Some(to.parse().map_err(|_| bad())?) },
            page: Some(page.to_string()).filter(|p| !p.is_empty()),
            pages: pages.parse().map_err(|_| bad())?,
        })
    }
}

/// The option-list walk that collects base coins: `c|coins|venue_cursor`.
fn coins_cursor(coins: &[String], page: &str) -> String {
    format!("c|{}|{page}", coins.join(","))
}

/// A walk over settle coins: which coin, and the venue's page cursor within it.
fn walk_cursor(c: Option<&str>) -> Result<(usize, Option<String>), ConnectorError> {
    let Some(c) = c else { return Ok((0, None)) };
    let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
    let (i, page) = c.split_once('|').ok_or_else(bad)?;
    let i: usize = i.parse().map_err(|_| bad())?;
    if i >= SETTLE.len() {
        return Err(bad());
    }
    Ok((i, Some(page.to_string()).filter(|p| !p.is_empty())))
}

fn signed(endpoint: &'static str, base: &'static str, path: &'static str, req_id: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, base, path, req_id).signed()
}

fn wallet(base: &'static str, req_id: CorrId) -> HttpRequest {
    signed("walletBalance", base, "/v5/account/wallet-balance", req_id).query("accountType", "UNIFIED")
}

fn paged(mut h: HttpRequest, page: Option<String>) -> HttpRequest {
    if let Some(p) = page {
        h = h.query("cursor", p);
    }
    h
}

fn fills_request(category: Category, base: &'static str, req_id: CorrId, cur: &FillCursor, slice_to: i64, from_ms: i64) -> HttpRequest {
    let slice_from = (slice_to - SLICE_MS).max(from_ms);
    let mut h = signed("executionList", base, "/v5/execution/list", req_id)
        .query("category", category.name())
        .query("startTime", slice_from)
        .query("endTime", slice_to)
        .query("limit", FILL_PAGE);
    if let Some(c) = cur.coins.get(cur.coin) {
        h = h.query("baseCoin", c);
    }
    paged(h, cur.page.clone())
}

fn coins_request(base: &'static str, req_id: CorrId, page: Option<String>) -> HttpRequest {
    let h = HttpRequest::get("instruments", base, "/v5/market/instruments-info", req_id).query("category", "option").query("baseCoin", "All").query("limit", COIN_PAGE);
    paged(h, page)
}

pub fn encode(category: Category, base: &'static str, cmd: &Command, pending: &mut std::collections::HashMap<CorrId, super::Pending>) -> Result<Vec<NativeRequest>, ConnectorError> {
    let Command::QueryAccount { req_id, kind, range, cursor, .. } = cmd else {
        return Err(ConnectorError::invalid("not an account command"));
    };
    let (req, step) = match (category, kind) {
        (_, AccountKind::Summary) => (signed("queryApi", base, "/v5/user/query-api", *req_id), Step::Key),
        (Category::Spot, AccountKind::Balances) => (wallet(base, *req_id), Step::Balances),
        (Category::Linear, AccountKind::OpenOrders | AccountKind::Positions) => {
            let (i, page) = walk_cursor(cursor.as_deref())?;
            let h = if *kind == AccountKind::Positions {
                signed("positionList", base, "/v5/position/list", *req_id).query("limit", POSITION_PAGE)
            } else {
                signed("orderRealtime", base, "/v5/order/realtime", *req_id).query("limit", ORDER_PAGE)
            };
            let h = paged(h.query("category", "linear").query("settleCoin", SETTLE[i]), page);
            (h, if *kind == AccountKind::Positions { Step::Positions { settle: i } } else { Step::Orders { settle: i } })
        }
        // Spot, inverse and options answer every open order (and position) without a coin.
        (c, AccountKind::OpenOrders) => {
            let (_, page) = walk_cursor(cursor.as_deref())?;
            (paged(signed("orderRealtime", base, "/v5/order/realtime", *req_id).query("category", c.name()).query("limit", ORDER_PAGE), page), Step::Orders { settle: 0 })
        }
        (c @ (Category::Inverse | Category::Option), AccountKind::Positions) => {
            let (_, page) = walk_cursor(cursor.as_deref())?;
            (paged(signed("positionList", base, "/v5/position/list", *req_id).query("category", c.name()).query("limit", POSITION_PAGE), page), Step::Positions { settle: 0 })
        }
        (c, AccountKind::Fills) => {
            let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
            let (from_ms, to_ms) = (range.from.millis(), range.to.millis());
            if to_ms <= from_ms {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            match cursor.as_deref() {
                // Options start by listing the coins they are written on.
                None if c == Category::Option => (coins_request(base, *req_id, None), Step::Coins { coins: Vec::new() }),
                Some(k) if k.starts_with("c|") => {
                    let mut parts = k.splitn(3, '|').skip(1);
                    let coins: Vec<String> = parts.next().unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
                    let page = parts.next().map(str::to_string).filter(|p| !p.is_empty());
                    (coins_request(base, *req_id, page), Step::Coins { coins })
                }
                k => {
                    let mut cur = match k {
                        Some(k) => FillCursor::parse(k)?,
                        None => FillCursor { coins: Vec::new(), coin: 0, slice_to: None, page: None, pages: 0 },
                    };
                    if cur.pages >= MAX_PAGES {
                        return Err(ConnectorError::invalid("this period holds more Bybit fills than one pull reads. Bybit answers fills a week at a time, so ask for a shorter period."));
                    }
                    let slice_to = cur.slice_to.unwrap_or(to_ms);
                    cur.slice_to = Some(slice_to);
                    (fills_request(c, base, *req_id, &cur, slice_to, from_ms), Step::Fills { cur, from_ms })
                }
            }
        }
        (c, k) => return Err(ConnectorError::unsupported(format!("Bybit {c:?} has no {k:?}"))),
    };
    pending.insert(*req_id, super::Pending::Account(Pending { base, step }));
    Ok(vec![NativeRequest::Http(req)])
}

/// A refused key names the site it was sent to: a key works on the site that issued it only.
pub fn hint(p: &Pending, e: ConnectorError) -> ConnectorError {
    if e.kind != ErrorKind::Auth {
        return e;
    }
    let host = super::BASES.iter().find(|(k, _)| *k == p.base).map(|(_, h)| *h).unwrap_or_default();
    ConnectorError { message: format!("{}. The key was sent to {host}: a Bybit key works only on the site that issued it, set this account's site to match.", e.message), ..e }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyInfo {
    #[serde(default)]
    read_only: Option<Int>,
    /// `0` regular (classic), `1` unified.
    #[serde(default)]
    uta: Option<Int>,
    #[serde(default)]
    permissions: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletCoin {
    coin: String,
    #[serde(default)]
    wallet_balance: OptDec,
    /// Held by open spot orders.
    #[serde(default)]
    locked: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletAccount {
    #[serde(default)]
    total_equity: OptDec,
    #[serde(default)]
    coin: Vec<WalletCoin>,
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
    avg_price: OptDec,
    #[serde(default)]
    mark_price: OptDec,
    #[serde(default)]
    unrealised_pnl: OptDec,
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
    qty: OptDec,
    #[serde(default)]
    cum_exec_qty: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    trigger_price: OptDec,
    #[serde(default)]
    created_time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FillRow {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    exec_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    exec_price: OptDec,
    #[serde(default)]
    exec_qty: OptDec,
    #[serde(default)]
    exec_fee: OptDec,
    #[serde(default)]
    fee_currency: String,
    #[serde(default)]
    exec_type: String,
    exec_time: Int,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoinRow {
    #[serde(default)]
    base_coin: String,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("bybit {what}: {e}"))
}

fn side(raw: &str) -> Mapped<Side> {
    match raw {
        "Buy" => Mapped::Known(Side::Buy),
        "Sell" => Mapped::Known(Side::Sell),
        _ => otw_connector_sdk::enums::unknown("bybit", "side", raw),
    }
}

/// Whether an execution row is a trade. Funding and USDC session settlement are cash
/// movements on the position, not fills.
fn is_trade(exec_type: &str) -> bool {
    match exec_type {
        "Trade" | "AdlTrade" | "BustTrade" | "Delivery" | "BlockTrade" | "MovePosition" | "FutureSpread" => true,
        "Funding" | "Settle" => false,
        other => {
            tracing::warn!("bybit execution list: unknown execType `{other}`, left out of the fills");
            false
        }
    }
}

fn positive(d: OptDec) -> Option<Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(Price)
}

fn upper(s: &str) -> Option<String> {
    Some(s.to_ascii_uppercase()).filter(|s| !s.is_empty())
}

/// The cursor of a settle-coin walk after this page: the same coin's next page, else the
/// next coin (linear only), else done.
fn next_walk(category: Category, settle: usize, page: Option<String>) -> Option<String> {
    match page.filter(|p| !p.is_empty()) {
        Some(p) => Some(format!("{settle}|{p}")),
        None if category == Category::Linear && settle + 1 < SETTLE.len() => Some(format!("{}|", settle + 1)),
        None => None,
    }
}

/// A second call within the same read, and what its answer is.
pub type FollowUp = (NativeRequest, Pending);

pub fn decode(category: Category, venue: &'static str, p: Pending, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<(Vec<Envelope>, Option<FollowUp>), ConnectorError> {
    let base = p.base;
    let out = match p.step {
        Step::Key => {
            let page: Answer<KeyInfo> = serde_json::from_slice(body).map_err(|e| decode_err("query-api", e))?;
            let k = page.result;
            if k.uta.map(|u| u.0) == Some(0) {
                return Err(ConnectorError::new(
                    ErrorKind::Permission,
                    "this Bybit key belongs to a classic account, which Bybit no longer serves over v5: upgrade it to a unified trading account, then create a new key",
                ));
            }
            let read_only = k.read_only.map(|r| r.0) == Some(1);
            let permissions = k
                .permissions
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(group, v)| format!("{group}: {}", v.join(", ")))
                .collect();
            let step = Step::Wallet { read_only, permissions };
            return Ok((Vec::new(), Some((NativeRequest::Http(wallet(base, corr)), Pending { base, step }))));
        }
        Step::Wallet { read_only, permissions } => {
            let page: Answer<List<WalletAccount>> = serde_json::from_slice(body).map_err(|e| decode_err("wallet-balance", e))?;
            let equity: Decimal = page.result.list.iter().map(|a| a.total_equity.0.unwrap_or_default()).sum();
            let funded = page.result.list.iter().flat_map(|a| &a.coin).filter(|c| c.wallet_balance.0.is_some_and(|b| b.is_sign_positive() && !b.is_zero())).count();
            // Rounded, not cut: `Decimal` precision formatting truncates.
            let equity = equity.round_dp(2);
            let key = if read_only { "read-only key" } else { "this key can trade: a Read-Only key is enough here" };
            let detail = format!("unified account: {funded} coin(s) with a balance, {equity:.2} USD of equity; {key}");
            vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: Some("UNIFIED".into()),
                    permissions,
                    assets_held: u32::try_from(funded).unwrap_or(u32::MAX),
                    can_trade: Some(!read_only),
                    open_positions: None,
                    wallet_balance: None,
                    detail: Some(detail),
                }),
            )]
        }
        Step::Balances => {
            let page: Answer<List<WalletAccount>> = serde_json::from_slice(body).map_err(|e| decode_err("wallet-balance", e))?;
            let held = page
                .result
                .list
                .iter()
                .flat_map(|a| &a.coin)
                .filter_map(|c| {
                    let total = c.wallet_balance.0.filter(|b| b.is_sign_positive() && !b.is_zero())?;
                    let available = (total - c.locked.0.unwrap_or_default()).max(Decimal::ZERO);
                    Some(Balance { asset: c.coin.to_ascii_uppercase(), total: Money(total), available: Money(available), name: None, fiat: false, account: None })
                })
                .collect();
            vec![chunk(ctx, corr, AccountItems::Balances(held), None)]
        }
        Step::Positions { settle } => {
            let page: Answer<List<PositionRow>> = serde_json::from_slice(body).map_err(|e| decode_err("position list", e))?;
            let mut out = Vec::new();
            for p in page.result.list {
                let Some(size) = p.size.0.filter(|s| !s.is_zero()) else { continue };
                let sign = match p.side.as_str() {
                    "Sell" => -Decimal::ONE,
                    "Buy" => Decimal::ONE,
                    other => return Err(ConnectorError::decode(format!("bybit position list: side `{other}` on an open {} position", p.symbol))),
                };
                let (qty, currency, pnl, name) = match category {
                    // USD contracts to coins at the entry price; the profit is in the coin.
                    Category::Inverse => {
                        let coins = per(size.abs(), p.avg_price.0).ok_or_else(|| ConnectorError::decode(format!("bybit position list: {} has no entry price to count its coins", p.symbol)))?;
                        (coins, Some("USD".to_string()), None, Some(format!("inverse, {size} USD contracts, profit counted in the coin")))
                    }
                    Category::Linear => (size.abs(), Some(SETTLE[settle].to_string()), p.unrealised_pnl.0.map(Money), None),
                    _ => (size.abs(), None, p.unrealised_pnl.0.map(Money), None),
                };
                out.push(Position {
                    instrument: InstrumentId::new(venue, &p.symbol.to_ascii_uppercase()),
                    qty: Qty(sign * qty),
                    avg_price: positive(p.avg_price),
                    unrealized_pnl: pnl,
                    currency,
                    mark_price: positive(p.mark_price),
                    venue: None,
                    account: None,
                    class: (category == Category::Option).then_some(AssetClass::Option),
                    multiplier: None,
                    name,
                });
            }
            vec![chunk(ctx, corr, AccountItems::Positions(out), next_walk(category, settle, page.result.next_page_cursor))]
        }
        Step::Orders { settle } => {
            let page: Answer<List<OrderRow>> = serde_json::from_slice(body).map_err(|e| decode_err("order realtime", e))?;
            let mut out = Vec::new();
            for o in page.result.list {
                let qty = o.qty.0.unwrap_or_default();
                let filled = o.cum_exec_qty.0.unwrap_or_default();
                let (qty, filled, quote) = match category {
                    // USD contracts to coins at the order's own price, else its trigger.
                    Category::Inverse => {
                        let at = o.price.0.filter(|p| !p.is_zero()).or(o.trigger_price.0);
                        match (per(qty, at), per(filled, at)) {
                            (Some(q), Some(f)) => (q, f, Some("USD".to_string())),
                            _ => return Err(ConnectorError::decode(format!("bybit order realtime: inverse order {} names no price to count its coins", o.order_id))),
                        }
                    }
                    Category::Linear => (qty, filled, Some(SETTLE[settle].to_string())),
                    _ => (qty, filled, None),
                };
                out.push(Order {
                    id: o.order_id,
                    instrument: InstrumentId::new(venue, &o.symbol.to_ascii_uppercase()),
                    side: side(&o.side),
                    order_type: o.order_type,
                    qty: Qty(qty),
                    filled_qty: Qty(filled),
                    limit_price: positive(o.price),
                    stop_price: positive(o.trigger_price),
                    placed_at: o.created_time.map(|t| ts_millis(t.0)).transpose()?,
                    quote_ccy: quote,
                    account: None,
                    venue: None,
                    class: (category == Category::Option).then_some(AssetClass::Option),
                });
            }
            vec![chunk(ctx, corr, AccountItems::Orders(out), next_walk(category, settle, page.result.next_page_cursor))]
        }
        Step::Coins { mut coins } => {
            let page: Answer<List<CoinRow>> = serde_json::from_slice(body).map_err(|e| decode_err("instruments-info", e))?;
            for r in page.result.list {
                let c = r.base_coin.to_ascii_uppercase();
                if !c.is_empty() && !coins.contains(&c) {
                    coins.push(c);
                }
            }
            let next = match page.result.next_page_cursor.filter(|c| !c.is_empty()) {
                Some(p) => coins_cursor(&coins, &p),
                None if coins.is_empty() => return Ok((vec![chunk(ctx, corr, AccountItems::Fills(Vec::new()), None)], None)),
                // The coins are known: the fill walk starts, from the period's end.
                None => FillCursor { coins, coin: 0, slice_to: None, page: None, pages: 0 }.encode(),
            };
            vec![chunk(ctx, corr, AccountItems::Fills(Vec::new()), Some(next))]
        }
        Step::Fills { cur, from_ms } => {
            let page: Answer<List<FillRow>> = serde_json::from_slice(body).map_err(|e| decode_err("execution list", e))?;
            let mut fills = Vec::with_capacity(page.result.list.len());
            for r in page.result.list {
                if r.symbol.is_empty() || !is_trade(&r.exec_type) {
                    continue;
                }
                fills.push(Fill {
                    id: r.exec_id,
                    order_id: Some(r.order_id).filter(|o| !o.is_empty()),
                    instrument: InstrumentId::new(venue, &r.symbol.to_ascii_uppercase()),
                    ts: ts_millis(r.exec_time.0)?,
                    side: side(&r.side),
                    qty: Qty(r.exec_qty.0.unwrap_or_default()),
                    price: Price(r.exec_price.0.unwrap_or_default()),
                    fee: Money(r.exec_fee.0.unwrap_or_default()),
                    fee_asset: upper(&r.fee_currency),
                    quote_ccy: None,
                    account: None,
                    venue: None,
                    class: (category == Category::Option).then_some(AssetClass::Option),
                    multiplier: None,
                });
            }
            let pages = cur.pages + 1;
            let slice_to = cur.slice_to.ok_or_else(|| ConnectorError::invalid("a fill page with no slice"))?;
            let slice_from = (slice_to - SLICE_MS).max(from_ms);
            let next = match page.result.next_page_cursor.filter(|c| !c.is_empty()) {
                Some(p) => Some(FillCursor { page: Some(p), pages, ..cur }),
                None if slice_from > from_ms => Some(FillCursor { slice_to: Some(slice_from), page: None, pages, ..cur }),
                // This coin's period is read: the next coin starts again from the end.
                None if cur.coin + 1 < cur.coins.len() => Some(FillCursor { coin: cur.coin + 1, slice_to: None, page: None, pages, ..cur }),
                None => None,
            };
            vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))]
        }
    };
    Ok((out, None))
}
