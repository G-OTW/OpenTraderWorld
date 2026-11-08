//! Binance accounts, read only: fills, balances, positions, open orders, the key's own
//! summary, plus the exchange clock and the public price board those reads lean on.
//!
//! The fill histories are per symbol on both markets and page differently, both carried by
//! the cursor:
//!   - spot `myTrades`: a period opens on `startTime` alone and pages forward by `fromId`
//!     (the 24 h ceiling applies to `startTime` + `endTime` together); the period end is
//!     enforced here, on the first fill past it.
//!   - USDⓈ-M `userTrades`: `startTime`/`endTime` may not span more than seven days and
//!     `fromId` may not be combined with them, so a period is walked week by week and each
//!     week paged forward on the clock, one millisecond past the last fill read.
//!
//! A fill does not carry its quote currency (`BTCUSDT` is a string, not a price in
//! dollars); the caller reads it from the instrument list, never from a guessed suffix.

use otw_connector_model::{
    AccountItems, AccountKind, AccountSummary, Balance, Command, ConnectorError, CorrId, Envelope, Event, Fill, InstrumentId,
    LastPrice, Mapped, Money, Order, Position, Qty, Side, TimeRange,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, NativeRequest};
use serde::Deserialize;

use super::market::{Kind, Market};
use super::{Pending, Session, REST};

/// Rows one fill page returns at most, on both markets.
pub const TRADES_PAGE: usize = 1000;
/// Widest window `userTrades` accepts.
const WEEK_MS: i64 = 7 * 86_400_000;
/// Hard stop on the futures page walk, so a mis-specified window cannot loop.
const MAX_USER_PAGES: u32 = 500;

/// Where a futures fill walk stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserCursor {
    pub idx: usize,
    pub window_from: i64,
    pub at: i64,
    pub pages: u32,
}

impl UserCursor {
    fn encode(&self) -> String {
        format!("{}:{}:{}:{}", self.idx, self.window_from, self.at, self.pages)
    }

    fn parse(c: &str) -> Result<Self, ConnectorError> {
        let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
        let parts: Vec<&str> = c.split(':').collect();
        let [idx, window_from, at, pages] = parts.as_slice() else { return Err(bad()) };
        Ok(UserCursor {
            idx: idx.parse().map_err(|_| bad())?,
            window_from: window_from.parse().map_err(|_| bad())?,
            at: at.parse().map_err(|_| bad())?,
            pages: pages.parse().map_err(|_| bad())?,
        })
    }
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, items: AccountItems, next: Option<String>) -> Envelope {
    let done = next.is_none();
    env(ctx, corr, Event::AccountChunk { items, next_cursor: next, done })
}

fn decode_err(m: &Market, what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("{} {what}: {e}", m.name))
}

/// Spot cursor: `"<symbol index>:"` or `"<symbol index>:<fromId>"`; none = the first symbol.
fn parse_spot_cursor(c: &str) -> Result<(usize, Option<i64>), ConnectorError> {
    let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
    let (i, id) = c.split_once(':').ok_or_else(bad)?;
    let i = i.parse().map_err(|_| bad())?;
    let id = if id.is_empty() { None } else { Some(id.parse().map_err(|_| bad())?) };
    Ok((i, id))
}

fn signed(endpoint: &'static str, path: &'static str, req_id: CorrId, weight: u32) -> HttpRequest {
    HttpRequest::get(endpoint, REST, path, req_id).weight(weight).signed()
}

pub fn encode(m: &'static Market, cmd: &Command, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    match cmd {
        Command::SyncTime { req_id } => {
            s.pending.insert(*req_id, Pending::Time);
            Ok(vec![NativeRequest::Http(HttpRequest::get("time", REST, m.time, *req_id))])
        }
        Command::RequestBoard { req_id } => {
            s.pending.insert(*req_id, Pending::Board);
            Ok(vec![NativeRequest::Http(HttpRequest::get("tickerPrice", REST, m.ticker_price, *req_id).weight(4))])
        }
        Command::QueryAccount { req_id, kind, symbols, range, cursor, .. } => {
            let req = match (m.kind, kind) {
                (Kind::Spot, AccountKind::Summary | AccountKind::Balances) => signed("account", "/api/v3/account", *req_id, 20),
                (Kind::UsdM, AccountKind::Summary) => signed("account", "/fapi/v3/account", *req_id, 5),
                (Kind::UsdM, AccountKind::Balances) => signed("balance", "/fapi/v3/balance", *req_id, 5),
                (Kind::UsdM, AccountKind::Positions) => signed("positionRisk", "/fapi/v3/positionRisk", *req_id, 5),
                (Kind::Spot, AccountKind::Positions) => {
                    return Err(ConnectorError::unsupported("a Binance spot account has balances, not positions"))
                }
                (_, AccountKind::OpenOrders) => signed("openOrders", m.open_orders, *req_id, if m.kind == Kind::Spot { 80 } else { 40 }),
                (_, AccountKind::Fills) => return encode_fills(m, *req_id, symbols, *range, cursor.as_deref(), s),
            };
            let pending = match kind {
                AccountKind::OpenOrders => Pending::OpenOrders,
                AccountKind::Positions => Pending::Positions,
                k => Pending::Account { kind: *k },
            };
            s.pending.insert(*req_id, pending);
            Ok(vec![NativeRequest::Http(req)])
        }
        _ => Err(ConnectorError::invalid("not an account command")),
    }
}

fn encode_fills(m: &'static Market, req_id: CorrId, symbols: &[InstrumentId], range: Option<TimeRange>, cursor: Option<&str>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
    let range = range.ok_or_else(|| ConnectorError::invalid("a fill history needs a period"))?;
    if symbols.is_empty() {
        return Err(ConnectorError::invalid(format!(
            "{} answers its trade history one instrument at a time. Name the symbols to pull",
            m.name
        )));
    }
    match m.kind {
        Kind::Spot => {
            let (idx, from_id) = match cursor {
                Some(c) => parse_spot_cursor(c)?,
                None => (0, None),
            };
            let instrument = symbols.get(idx).ok_or_else(|| ConnectorError::invalid("cursor past the symbol list"))?;
            let h = signed("myTrades", "/api/v3/myTrades", req_id, 20).query("symbol", &instrument.native).query("limit", TRADES_PAGE);
            // The first page opens on `startTime` alone, the next ones continue by id; neither
            // carries `endTime` (the 24 h ceiling applies to the pair).
            let h = match from_id {
                Some(id) => h.query("fromId", id),
                None => h.query("startTime", range.from.millis()),
            };
            s.pending.insert(req_id, Pending::Trades { symbols: symbols.to_vec(), idx, range });
            Ok(vec![NativeRequest::Http(h)])
        }
        Kind::UsdM => {
            let (start, end) = (range.from.millis(), range.to.millis());
            if end <= start {
                return Err(ConnectorError::invalid("the period ends before it starts"));
            }
            let cur = match cursor {
                Some(c) => UserCursor::parse(c)?,
                None => UserCursor { idx: 0, window_from: start, at: start, pages: 0 },
            };
            if cur.pages >= MAX_USER_PAGES {
                return Err(ConnectorError::invalid(
                    "this period holds more Binance futures fills than one pull reads. Ask for a shorter period.",
                ));
            }
            let instrument = symbols.get(cur.idx).ok_or_else(|| ConnectorError::invalid("cursor past the symbol list"))?;
            let window_to = (cur.window_from + WEEK_MS).min(end);
            let h = signed("userTrades", "/fapi/v1/userTrades", req_id, 5)
                .query("symbol", &instrument.native)
                .query("startTime", cur.at)
                .query("endTime", window_to)
                .query("limit", TRADES_PAGE);
            s.pending.insert(req_id, Pending::UserTrades { symbols: symbols.to_vec(), cur, range });
            Ok(vec![NativeRequest::Http(h)])
        }
    }
}

#[derive(Deserialize)]
struct ServerTime {
    #[serde(rename = "serverTime")]
    server_time: Int,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotAccount {
    #[serde(default)]
    account_type: Option<String>,
    #[serde(default)]
    can_trade: Option<bool>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    balances: Vec<SpotBalance>,
}

#[derive(Deserialize)]
struct SpotBalance {
    asset: String,
    free: Dec,
    locked: Dec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesAccount {
    #[serde(default)]
    can_trade: Option<bool>,
    #[serde(default)]
    total_wallet_balance: Option<serde_json::Value>,
    #[serde(default)]
    positions: Vec<FuturesAccountPosition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesAccountPosition {
    #[serde(default)]
    position_amt: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FuturesBalance {
    asset: String,
    balance: Dec,
    #[serde(default)]
    available_balance: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    symbol: String,
    position_amt: Dec,
    #[serde(default)]
    entry_price: OptDec,
    #[serde(default)]
    position_side: Option<String>,
    #[serde(default)]
    un_realized_profit: OptDec,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    symbol: String,
    order_id: Int,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    orig_qty: Dec,
    executed_qty: Dec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotTrade {
    id: Int,
    order_id: Int,
    price: Dec,
    qty: Dec,
    commission: Dec,
    #[serde(default)]
    commission_asset: String,
    time: Int,
    is_buyer: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserTrade {
    id: Int,
    order_id: Int,
    #[serde(default)]
    side: String,
    #[serde(default)]
    buyer: bool,
    price: Dec,
    qty: Dec,
    commission: Dec,
    #[serde(default)]
    commission_asset: String,
    time: Int,
}

#[derive(Deserialize)]
struct PriceRow {
    symbol: String,
    price: Dec,
}

fn side(raw: &str) -> Mapped<Side> {
    match raw {
        "BUY" => Mapped::Known(Side::Buy),
        "SELL" => Mapped::Known(Side::Sell),
        other => otw_connector_sdk::enums::unknown("binance", "order side", other),
    }
}

fn positive(d: OptDec) -> Option<otw_connector_model::Price> {
    d.0.filter(|p| p.is_sign_positive() && !p.is_zero()).map(otw_connector_model::Price)
}

fn orders_out(m: &Market, rows: &[OrderRow]) -> Result<Vec<Order>, ConnectorError> {
    rows.iter()
        .map(|o| {
            Ok(Order {
                id: o.order_id.0.to_string(),
                instrument: InstrumentId::new(m.venue, &o.symbol.to_ascii_uppercase()),
                side: side(&o.side),
                order_type: o.order_type.clone(),
                qty: o.orig_qty.qty(),
                filled_qty: o.executed_qty.qty(),
                limit_price: positive(o.price),
                stop_price: positive(o.stop_price),
                placed_at: o.time.map(|t| ts_millis(t.0)).transpose()?,
                quote_ccy: None,
                account: None,
                venue: None,
                class: None,
            })
        })
        .collect()
}

/// `frame_error` is the error the frame carries (already classified), if any.
pub fn decode(m: &'static Market, pending: Pending, corr: CorrId, body: &[u8], frame_error: Option<ConnectorError>, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    if let Some(e) = frame_error {
        return Err(e);
    }
    match pending {
        Pending::Time => {
            let t: ServerTime = serde_json::from_slice(body).map_err(|e| decode_err(m, "time", e))?;
            Ok(vec![env(ctx, corr, Event::ServerTime(ts_millis(t.server_time.0)?))])
        }
        Pending::Board => {
            let rows: Vec<PriceRow> = serde_json::from_slice(body).map_err(|e| decode_err(m, "ticker/price", e))?;
            let board = rows
                .into_iter()
                .map(|r| LastPrice { instrument: InstrumentId::new(m.venue, &r.symbol.to_ascii_uppercase()), price: r.price.price() })
                .collect();
            Ok(vec![env(ctx, corr, Event::Board(board))])
        }
        Pending::Account { kind } => match m.kind {
            Kind::Spot => decode_spot_account(m, kind, corr, body, ctx),
            Kind::UsdM => decode_futures_account(m, kind, corr, body, ctx),
        },
        Pending::Positions => {
            let rows: Vec<PositionRow> = serde_json::from_slice(body).map_err(|e| decode_err(m, "positionRisk", e))?;
            let mut out = Vec::new();
            for p in rows {
                if p.position_amt.0.is_zero() {
                    continue;
                }
                // Hedge mode names the side; one-way mode puts it in the sign.
                let abs = p.position_amt.0.abs();
                let qty = match p.position_side.as_deref() {
                    Some("SHORT") => -abs,
                    Some("LONG") => abs,
                    _ => p.position_amt.0,
                };
                out.push(Position {
                    instrument: InstrumentId::new(m.venue, &p.symbol.to_ascii_uppercase()),
                    qty: Qty(qty),
                    avg_price: positive(p.entry_price),
                    unrealized_pnl: p.un_realized_profit.0.map(Money),
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
        Pending::OpenOrders => {
            let rows: Vec<OrderRow> = serde_json::from_slice(body).map_err(|e| decode_err(m, "openOrders", e))?;
            Ok(vec![chunk(ctx, corr, AccountItems::Orders(orders_out(m, &rows)?), None)])
        }
        Pending::Trades { symbols, idx, range } => decode_spot_trades(m, corr, body, &symbols, idx, range, ctx),
        Pending::UserTrades { symbols, cur, range } => decode_user_trades(m, corr, body, &symbols, cur, range, ctx),
        _ => Err(ConnectorError::invalid("not an account answer")),
    }
}

fn decode_spot_account(m: &Market, kind: AccountKind, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let a: SpotAccount = serde_json::from_slice(body).map_err(|e| decode_err(m, "account", e))?;
    let held: Vec<Balance> = a
        .balances
        .iter()
        .filter_map(|b| {
            let total = b.free.0 + b.locked.0;
            (total.is_sign_positive() && !total.is_zero()).then(|| Balance { asset: b.asset.clone(), total: Money(total), available: b.free.money(), name: None, fiat: false, account: None })
        })
        .collect();
    match kind {
        AccountKind::Summary => Ok(vec![env(
            ctx,
            corr,
            Event::AccountSummary(AccountSummary {
                account_type: a.account_type,
                permissions: a.permissions,
                assets_held: u32::try_from(held.len()).unwrap_or(u32::MAX),
                can_trade: a.can_trade,
                open_positions: None,
                wallet_balance: None,
                detail: None,
            }),
        )]),
        _ => Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)]),
    }
}

fn decode_futures_account(m: &Market, kind: AccountKind, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    match kind {
        AccountKind::Summary => {
            let a: FuturesAccount = serde_json::from_slice(body).map_err(|e| decode_err(m, "account", e))?;
            let open = a.positions.iter().filter(|p| p.position_amt.0.is_some_and(|q| !q.is_zero())).count();
            Ok(vec![env(
                ctx,
                corr,
                Event::AccountSummary(AccountSummary {
                    account_type: None,
                    permissions: vec![],
                    assets_held: 0,
                    can_trade: a.can_trade,
                    open_positions: Some(u32::try_from(open).unwrap_or(u32::MAX)),
                    // As the venue writes it: a string field, shown verbatim.
                    wallet_balance: Some(a.total_wallet_balance.as_ref().and_then(|v| v.as_str()).unwrap_or_default().to_string()),
                    detail: None,
                }),
            )])
        }
        _ => {
            let rows: Vec<FuturesBalance> = serde_json::from_slice(body).map_err(|e| decode_err(m, "balance", e))?;
            let held = rows
                .into_iter()
                .filter(|b| b.balance.0.is_sign_positive() && !b.balance.0.is_zero())
                .map(|b| Balance {
                    asset: b.asset.to_ascii_uppercase(),
                    total: b.balance.money(),
                    available: Money(b.available_balance.0.unwrap_or(b.balance.0)),
                    name: None,
                    fiat: false,
                    account: None,
                })
                .collect();
            Ok(vec![chunk(ctx, corr, AccountItems::Balances(held), None)])
        }
    }
}

fn decode_spot_trades(m: &Market, corr: CorrId, body: &[u8], symbols: &[InstrumentId], idx: usize, range: TimeRange, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let rows: Vec<SpotTrade> = serde_json::from_slice(body).map_err(|e| decode_err(m, "myTrades", e))?;
    let instrument = symbols.get(idx).ok_or_else(|| ConnectorError::invalid("trade page past the symbol list"))?;
    // The period's end is inclusive, as the legacy reader had it.
    let (start, end) = (range.from.millis(), range.to.millis());
    let mut fills = Vec::new();
    let mut last_id = None;
    let mut past_end = false;
    for r in &rows {
        // Fills come back in id order, which is time order: the first one past the period
        // ends the walk rather than being skipped.
        if r.time.0 > end {
            past_end = true;
            break;
        }
        last_id = Some(r.id.0);
        if r.time.0 < start {
            continue;
        }
        fills.push(Fill {
            id: r.id.0.to_string(),
            order_id: Some(r.order_id.0.to_string()),
            instrument: instrument.clone(),
            ts: ts_millis(r.time.0)?,
            side: Mapped::Known(if r.is_buyer { Side::Buy } else { Side::Sell }),
            qty: r.qty.qty(),
            price: r.price.price(),
            fee: r.commission.money(),
            fee_asset: Some(r.commission_asset.clone()),
            quote_ccy: None,
            account: None,
            venue: None,
            class: None,
            multiplier: None,
        });
    }
    let next_symbol = || (idx + 1 < symbols.len()).then(|| format!("{}:", idx + 1));
    let next = if past_end || rows.len() < TRADES_PAGE {
        next_symbol()
    } else {
        match last_id {
            Some(id) => Some(format!("{idx}:{}", id + 1)),
            None => next_symbol(),
        }
    };
    Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next)])
}

fn decode_user_trades(m: &Market, corr: CorrId, body: &[u8], symbols: &[InstrumentId], cur: UserCursor, range: TimeRange, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
    let rows: Vec<UserTrade> = serde_json::from_slice(body).map_err(|e| decode_err(m, "userTrades", e))?;
    let instrument = symbols.get(cur.idx).ok_or_else(|| ConnectorError::invalid("trade page past the symbol list"))?;
    let end = range.to.millis();
    let window_to = (cur.window_from + WEEK_MS).min(end);
    let mut fills = Vec::with_capacity(rows.len());
    let mut last_ms = cur.at;
    for r in &rows {
        last_ms = last_ms.max(r.time.0);
        // `side` is BUY or SELL; `buyer` says the same thing and is the fallback.
        let side = match r.side.as_str() {
            "BUY" => Side::Buy,
            "SELL" => Side::Sell,
            _ if r.buyer => Side::Buy,
            _ => Side::Sell,
        };
        fills.push(Fill {
            id: r.id.0.to_string(),
            order_id: Some(r.order_id.0.to_string()),
            instrument: instrument.clone(),
            ts: ts_millis(r.time.0)?,
            side: Mapped::Known(side),
            qty: r.qty.qty(),
            price: r.price.price(),
            fee: r.commission.money(),
            fee_asset: Some(r.commission_asset.to_ascii_uppercase()),
            quote_ccy: None,
            account: None,
            venue: None,
            class: None,
            multiplier: None,
        });
    }
    let pages = cur.pages + 1;
    let next = if rows.len() < TRADES_PAGE || last_ms >= window_to {
        // This week is read: the next one, or the next symbol from the period's start.
        if window_to < end {
            Some(UserCursor { idx: cur.idx, window_from: window_to, at: window_to, pages })
        } else if cur.idx + 1 < symbols.len() {
            let start = range.from.millis();
            Some(UserCursor { idx: cur.idx + 1, window_from: start, at: start, pages })
        } else {
            None
        }
    } else {
        // A full page and more to come: one millisecond past the last fill read. Two fills
        // in the same millisecond would otherwise repeat.
        Some(UserCursor { at: last_ms + 1, pages, ..cur })
    };
    Ok(vec![chunk(ctx, corr, AccountItems::Fills(fills), next.map(|c| c.encode()))])
}
