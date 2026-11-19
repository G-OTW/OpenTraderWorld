//! KuCoin orders, spot (HF orders) and futures (USDT- and USDC-margined contracts), written from
//! the documentation alone: no account, no sandbox (KuCoin has none). Every field below is the
//! doc's; each claim is sourced in PROVIDER.md ("Trading"). The order path itself is the shared
//! trader's; this file only says what the calls are and what the answers mean.
//!
//! - Spot: `POST /api/v1/hf/orders`, limit (GTC, IOC, FOK, post-only) and market sized in the
//!   base coin (`size`, never `funds`). No stop, no reduce-only, no change (the doc's change is a
//!   cancel and replace, left out).
//! - Futures: `POST /api/v1/orders`, limit, market, stop and stop-limit (`stop` up or down,
//!   `stopPrice`, `stopPriceType=TP`), `reduceOnly`, `positionSide` from the account's position
//!   mode (read at account level, never switched), `marginMode` from the symbol's margin mode
//!   (read per symbol, never switched). The size goes out as `qty` in the base coin, checked
//!   first to be a whole number of contracts; every answer counts contracts, read back in the
//!   base coin through each contract's `multiplier`.
//! - Status: KuCoin's REST answers carry no status word (spot `active`/`cancelExist`, futures
//!   `open`/`done` and the filled size), so the mapper derives one of its own words
//!   (`open`, `partial`, `filled`, `canceled`); anything else is unknown and alerts.
//! - Private stream: the socket's URL, token and heartbeat are handed out by a signed
//!   `POST /api/v1/bullet-private` before each connection; the order topics carry no fee, so a
//!   streamed fill is `fee_pending` and its fee is read from the REST fills.
//! - A spot whole-book listing names the symbols with working orders first, then walks them
//!   (the venue lists working orders per symbol only).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderRequest, OrderStatus, OrderType, Side, TimeInForce};
use otw_connector_sdk::parse::{Int, OptDec};
use otw_connector_sdk::trade::{
    Ack, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, MarginKind, MarginMode, OrderSpace, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Settings,
    TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{Book, Kucoin, REST, SETTLE_COINS};

// Endpoint ids: what an answer is read as (`DecodeCtx::endpoint`) and what the error table
// reads a `400100` on (a cancel is looked up).
const SPOT_PLACE: &str = "spot_place";
const SPOT_TEST: &str = "spot_test";
const SPOT_CANCEL: &str = "spot_cancel";
const SPOT_CANCEL_ALL: &str = "spot_cancel_all";
const SPOT_BY_CID: &str = "spot_by_cid";
const SPOT_BY_ID: &str = "spot_by_id";
/// Cancel Stop Order By OrderId: a spot stop order the account read lists (`cond:<id>`).
const SPOT_STOP_CANCEL: &str = "spot_stop_cancel";
/// The symbols with working orders, first step of a whole-book listing.
const SPOT_OPEN_SYMBOLS: &str = "spot_open_symbols";
/// One symbol's working orders, more symbols to walk after it.
const SPOT_OPEN_MORE: &str = "spot_open_more";
/// One symbol's working orders, the last (or only) symbol.
const SPOT_OPEN_PAGE: &str = "spot_open_page";
/// A walk whose remaining symbols were lost (expired, or the mapper restarted): its answer is
/// an error, never an empty last page.
const SPOT_OPEN_END: &str = "spot_open_end";
/// Get Stop Orders List: the spot stop orders, listed with the whole book (`cond:<id>`).
const SPOT_OPEN_STOPS: &str = "spot_open_stops";
/// Batch Cancel Stop Orders, per symbol, with cancel all.
const SPOT_CANCEL_STOPS: &str = "spot_cancel_stops";
const SPOT_FILLS: &str = "spot_fills";
const SPOT_SYMBOLS: &str = "spot_symbols";
const FUT_PLACE: &str = "fut_place";
const FUT_TEST: &str = "fut_test";
const FUT_CANCEL: &str = "fut_cancel";
const FUT_CANCEL_ALL: &str = "fut_cancel_all";
const FUT_CANCEL_STOPS: &str = "fut_cancel_stops";
const FUT_BY_CID: &str = "fut_by_cid";
const FUT_BY_ID: &str = "fut_by_id";
const FUT_OPEN: &str = "fut_open";
const FUT_OPEN_STOPS: &str = "fut_open_stops";
const FUT_FILLS: &str = "fut_fills";
const FUT_CONTRACTS: &str = "fut_contracts";
const FUT_CONTRACT: &str = "fut_contract";
const FUT_POSITION: &str = "fut_position";
const FUT_POSITIONS: &str = "fut_positions";
const FUT_POSITION_MODE: &str = "fut_position_mode";
const FUT_MARGIN_MODE: &str = "fut_margin_mode";
const BULLET: &str = "bullet_private";

/// "Default20，Max100" (spot Get Trade History, `limit`).
const SPOT_FILL_PAGE: usize = 100;
/// Spot open orders by page: 50 per page (`pageSize` "maximum: 50").
const SPOT_OPEN_PAGE_SIZE: u32 = 50;
/// Get Stop Orders List's largest page (`pageSize` "maximum: 500").
const SPOT_STOP_PAGE_SIZE: u32 = 500;
/// How long a spot whole-book walk's remaining symbols are kept: a listing that never finished
/// is dropped after this, by age, never because others started.
const WALK_TTL: std::time::Duration = std::time::Duration::from_secs(600);
/// Futures lists: "The maximum cannot exceed 1000".
const FUTURES_PAGE_SIZE: u32 = 1000;
/// "each individual query is limited to a time range of 7 × 24 hours" (futures fills); spot:
/// "you can only get data within the time range of 7 * 24 hours".
const FILLS_WINDOW_MS: i64 = 7 * 86_400_000;
/// "Current server value: 18 seconds", when the token's answer names none.
const DEFAULT_PING_MS: u64 = 18_000;

/// The ping cadence for a venue `pingInterval`: half of it. "send a `ping` at least once every
/// `pingInterval` seconds", and "Network delay should also be considered; some users implement a
/// heartbeat cycle of 9 seconds" (WebSocket introduction).
fn ping_cadence(interval_ms: u64) -> u64 {
    (interval_ms / 2).max(1)
}

// The mapper's own status words (KuCoin's REST answers carry none).
const OPEN: &str = "open";
const PARTIAL: &str = "partial";
const FILLED: &str = "filled";
const CANCELED: &str = "canceled";

/// The symbols a spot whole-book listing still has to walk, by the listing's request id, with
/// when the walk last moved.
pub(super) type Walks = Arc<Mutex<HashMap<u64, (std::time::Instant, Vec<String>)>>>;

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kucoin {what}: {e}"))
}

/// A positive amount, or none: KuCoin writes "0" and null for a price an order does not have.
fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn side_word(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn side_of(w: &str) -> Mapped<Side> {
    match w {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

fn position_of(w: Option<&str>) -> Result<PositionSide, ConnectorError> {
    match w.unwrap_or("BOTH") {
        "BOTH" | "" => Ok(PositionSide::Net),
        "LONG" => Ok(PositionSide::Long),
        "SHORT" => Ok(PositionSide::Short),
        other => Err(ConnectorError::decode(format!("kucoin: positionSide {other} is not BOTH, LONG or SHORT"))),
    }
}

/// A futures row of a USDT- or USDC-margined contract (a row that names no settle coin passes).
fn linear(settle: Option<&str>) -> bool {
    settle.is_none_or(|c| c.is_empty() || SETTLE_COINS.iter().any(|s| s.eq_ignore_ascii_case(c)))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The `data` of an answer as rows: null is none, an array its rows, a page its `items` (with
/// its page numbers), an object one row.
/// An answer's rows, and its page numbers (current, total) when it is a page.
type Rows = (Vec<Value>, Option<(i64, i64)>);

fn rows_of(body: &[u8], what: &str) -> Result<Rows, ConnectorError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| decode_err(what, e))?;
    Ok(match v.get("data").cloned().unwrap_or(Value::Null) {
        Value::Null => (Vec::new(), None),
        Value::Array(a) => (a, None),
        Value::Object(o) if o.contains_key("items") => {
            let n = |k: &str| o.get(k).and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
            let pages = n("currentPage").zip(n("totalPage"));
            let items = match o.get("items") {
                Some(Value::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            (items, pages)
        }
        other @ Value::Object(_) => (vec![other], None),
        other => return Err(ConnectorError::decode(format!("kucoin {what}: data is {other}, not an order, a list or a page"))),
    })
}

fn next_page(pages: Option<(i64, i64)>) -> Option<String> {
    pages.filter(|(cur, total)| cur < total).map(|(cur, _)| format!("page:{}", cur + 1))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    #[serde(default)]
    id: String,
    /// The stream's name for it.
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    client_oid: Option<String>,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    /// The stream's order type (`type` is the event there).
    #[serde(default)]
    order_type: Option<String>,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    origin_size: OptDec,
    #[serde(default)]
    deal_size: OptDec,
    #[serde(default)]
    filled_size: OptDec,
    #[serde(default)]
    deal_funds: OptDec,
    #[serde(default)]
    avg_deal_price: OptDec,
    #[serde(default)]
    stop: Option<String>,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    stop_triggered: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    /// Spot REST: "true-The status of the order is active; false-... done".
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    cancel_exist: Option<bool>,
    /// Spot REST: "Number of canceled transactions".
    #[serde(default)]
    cancelled_size: OptDec,
    /// Spot REST: "Funds of canceled transactions" (an order sized in funds).
    #[serde(default)]
    cancelled_funds: OptDec,
    #[serde(default)]
    reduce_only: Option<bool>,
    #[serde(default)]
    position_side: Option<String>,
    #[serde(default)]
    settle_currency: Option<String>,
    #[serde(default)]
    created_at: Option<Int>,
    #[serde(default)]
    order_time: Option<Int>,
}

impl OrderRow {
    fn venue_id(&self) -> String {
        if self.id.is_empty() {
            self.order_id.clone()
        } else {
            self.id.clone()
        }
    }

    fn filled(&self) -> Decimal {
        self.deal_size.0.or(self.filled_size.0).unwrap_or_default()
    }

    /// The filled size, when the row writes one.
    fn filled_stated(&self) -> Option<Decimal> {
        self.deal_size.0.or(self.filled_size.0)
    }

    fn is_stop(&self) -> bool {
        self.stop.as_deref().is_some_and(|s| !s.is_empty())
    }
}

/// The mapper's word for a spot REST row: `active` decides working or done ("Canceled or fully
/// filled orders are marked as completed Done status"). A done row is filled when all of it
/// filled; otherwise its rest was cancelled (a cancel, an IOC rest, price protection), the part
/// filled kept on the row: cancelled. An order sized in funds (no `size`) with no cancellation of
/// any kind (`cancelExist`, `cancelledSize`, `cancelledFunds`) is unknown.
fn spot_rest_word(r: &OrderRow) -> String {
    let (qty, filled) = (r.size.0.unwrap_or_default(), r.filled());
    let positive = |d: OptDec| d.0.is_some_and(|v| v > Decimal::ZERO);
    let cancelled = r.cancel_exist == Some(true) || positive(r.cancelled_size) || positive(r.cancelled_funds);
    match r.active {
        Some(true) if filled > Decimal::ZERO => PARTIAL.into(),
        Some(true) => OPEN.into(),
        Some(false) if qty > Decimal::ZERO && filled >= qty => FILLED.into(),
        Some(false) if cancelled || qty > Decimal::ZERO => CANCELED.into(),
        Some(false) => "done, neither filled nor cancelled".into(),
        None => "no active flag".into(),
    }
}

/// The mapper's word for a futures REST row: `open` or `done`, read with the filled size. A
/// done row not all filled had its rest cancelled: cancelled, with what filled.
fn futures_rest_word(r: &OrderRow) -> String {
    let (qty, filled) = (r.size.0.unwrap_or_default(), r.filled());
    match r.status.as_deref().unwrap_or_default() {
        "open" if filled > Decimal::ZERO => PARTIAL.into(),
        "open" => OPEN.into(),
        "done" if qty > Decimal::ZERO && filled >= qty => FILLED.into(),
        "done" if r.cancel_exist == Some(true) || qty > Decimal::ZERO => CANCELED.into(),
        "done" => "done, neither filled nor cancelled".into(),
        other => other.to_string(),
    }
}

/// The mapper's word for an order frame of the stream: the status (`new`, `open`, `match`,
/// `done`) read with the event type (`received`, `open`, `match`, `update`, `filled`,
/// `canceled`) and the filled size. `filled` is "the status of the order changes to DONE after
/// the transaction": all of it filled, unless the frame's own filled size says less, in which
/// case the rest went away unfilled (cancelled, with what filled).
fn stream_word(status: &str, event: &str, qty: Decimal, filled: Decimal, filled_stated: bool) -> String {
    match (status, event) {
        (_, "filled") if filled_stated && qty > Decimal::ZERO && filled < qty => CANCELED.into(),
        (_, "filled") => FILLED.into(),
        (_, "canceled") => CANCELED.into(),
        // "partial amounts of the order have been filled and the unfilled part got canceled".
        ("done", "update") => CANCELED.into(),
        // All of it filled: filled, whatever the frame's status (a taker `match` that took it
        // all comes before its `filled`).
        ("done" | "open" | "match", _) if qty > Decimal::ZERO && filled >= qty => FILLED.into(),
        ("new" | "open" | "match", _) if filled > Decimal::ZERO => PARTIAL.into(),
        ("new" | "open" | "match", _) => OPEN.into(),
        (s, e) => format!("{s}/{e}"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotRule {
    symbol: String,
    #[serde(default)]
    base_min_size: OptDec,
    #[serde(default)]
    base_max_size: OptDec,
    #[serde(default)]
    base_increment: OptDec,
    #[serde(default)]
    quote_increment: OptDec,
    #[serde(default)]
    price_increment: OptDec,
    #[serde(default)]
    min_funds: OptDec,
    #[serde(default)]
    enable_trading: Option<bool>,
    #[serde(default)]
    callauction_is_enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractRule {
    symbol: String,
    #[serde(default)]
    multiplier: OptDec,
    #[serde(default)]
    lot_size: OptDec,
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    max_order_qty: OptDec,
    #[serde(default)]
    market_max_order_qty: OptDec,
    #[serde(default)]
    max_price: OptDec,
    #[serde(default)]
    is_inverse: Option<bool>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    settle_currency: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FillRow {
    /// A number on spot, a string on futures.
    #[serde(default)]
    trade_id: Option<Value>,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: String,
    /// Spot: "Trade execution time", ms.
    #[serde(default)]
    created_at: Option<Int>,
    /// Futures: "Trade time in nanoseconds".
    #[serde(default)]
    trade_time: Option<Int>,
    /// Futures: `trade`, `liquid`, `adl`, `settlement`.
    #[serde(default)]
    trade_type: Option<String>,
    #[serde(default)]
    settle_currency: Option<String>,
}

/// A decimal written as a JSON number or string (the stream writes sizes both ways).
fn dec_val(v: Option<&Value>) -> Option<Decimal> {
    match v? {
        Value::String(t) => otw_connector_sdk::parse::dec(t).ok(),
        Value::Number(n) => otw_connector_sdk::parse::dec(&n.to_string()).ok(),
        _ => None,
    }
}

fn id_text(v: &Option<Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    current_qty: OptDec,
    #[serde(default)]
    position_side: Option<String>,
    #[serde(default)]
    margin_mode: Option<String>,
    #[serde(default)]
    cross_mode: Option<bool>,
    #[serde(default)]
    leverage: OptDec,
    #[serde(default)]
    is_inverse: Option<bool>,
    #[serde(default)]
    settle_currency: Option<String>,
}

fn margin_of(word: Option<&str>, cross: Option<bool>) -> Option<MarginMode> {
    match word {
        Some("CROSS") => Some(MarginMode::cross()),
        Some("ISOLATED") => Some(MarginMode::isolated()),
        _ => cross.map(|c| if c { MarginMode::cross() } else { MarginMode::isolated() }),
    }
}

impl Kucoin {
    fn label(&self) -> &'static str {
        self.book.label()
    }

    fn signed(&self, endpoint: &'static str, method: Method, path: impl Into<String>, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, REST, path, corr).signed();
        h.method = method;
        h
    }

    fn post(&self, endpoint: &'static str, path: &str, body: Value, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let bytes = serde_json::to_vec(&body).map_err(|e| ConnectorError::invalid(format!("kucoin {endpoint}: {e}")))?;
        let mut h = self.signed(endpoint, Method::Post, path, corr);
        h.body = Some(bytes.into());
        Ok(h)
    }

    fn tif_word(&self, tif: TimeInForce) -> Result<&'static str, ConnectorError> {
        match tif {
            TimeInForce::Gtc => Ok("GTC"),
            TimeInForce::Ioc => Ok("IOC"),
            TimeInForce::Fok => Ok("FOK"),
            other => Err(ConnectorError::unsupported(format!("{} takes no {:?} time in force from OpenTraderWorld", self.label(), other.kind()))),
        }
    }

    /// The spot order body (Add Order request parameters). A dry run carries no `clientOid`.
    fn spot_body(&self, o: &OrderRequest, p: &Placing) -> Result<Value, ConnectorError> {
        if o.opts.reduce_only {
            return Err(ConnectorError::unsupported(format!("{} takes no reduce-only order: a spot sale of what is held is a plain sell", self.label())));
        }
        let mut b = Map::new();
        if !p.client_id.is_empty() {
            b.insert("clientOid".into(), json!(p.client_id));
        }
        b.insert("symbol".into(), json!(o.instrument.native));
        b.insert("side".into(), json!(side_word(o.side)));
        match o.order_type {
            OrderType::Market => {
                // "Market orders are not supported by the Time in Force strategy": none sent;
                // the size in the base coin ("select one out of two: size or funds").
                b.insert("type".into(), json!("market"));
                b.insert("size".into(), json!(dec_text(o.qty.value())));
            }
            OrderType::Limit => {
                let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
                b.insert("type".into(), json!("limit"));
                b.insert("price".into(), json!(dec_text(px.value())));
                b.insert("size".into(), json!(dec_text(o.qty.value())));
                b.insert("timeInForce".into(), json!(self.tif_word(o.tif)?));
                if o.opts.post_only {
                    b.insert("postOnly".into(), json!(true));
                }
            }
            other => return Err(ConnectorError::unsupported(format!("{} takes no {other:?} order from OpenTraderWorld", self.label()))),
        }
        Ok(Value::Object(b))
    }

    /// The futures order body (Add Order request parameters). The size is `qty` in the base coin
    /// ("must be an integer multiple of the multiplier"), refused before I/O when it is not a
    /// whole number of contracts.
    fn futures_body(&self, o: &OrderRequest, p: &Placing, client_id: &str) -> Result<Value, ConnectorError> {
        let sym = o.instrument.native.as_str();
        p.contracts(self.label(), sym, o.qty.value())?;
        let margin = p.margin.as_ref().ok_or_else(|| {
            ConnectorError::invalid(format!("{}: the margin mode of {sym} was not read, so nothing was sent: try again, or check the symbol's margin mode at KuCoin", self.label()))
        })?;
        let mut b = Map::new();
        b.insert("clientOid".into(), json!(client_id));
        b.insert("symbol".into(), json!(sym));
        b.insert("side".into(), json!(side_word(o.side)));
        let (kind, stop) = match o.order_type {
            OrderType::Market => ("market", false),
            OrderType::Limit => ("limit", false),
            OrderType::Stop => ("market", true),
            OrderType::StopLimit => ("limit", true),
            other => return Err(ConnectorError::unsupported(format!("{} takes no {other:?} order from OpenTraderWorld", self.label()))),
        };
        b.insert("type".into(), json!(kind));
        b.insert("qty".into(), json!(dec_text(o.qty.value())));
        if kind == "limit" {
            let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
            b.insert("price".into(), json!(dec_text(px.value())));
            b.insert("timeInForce".into(), json!(self.tif_word(o.tif)?));
            if o.opts.post_only {
                b.insert("postOnly".into(), json!(true));
            }
        }
        if stop {
            let px = o.stop.ok_or_else(|| ConnectorError::invalid("a stop order needs its stop price"))?;
            // A buy stop waits for the price to rise to it ("up": "reaches or goes above"), a
            // sell stop for it to fall ("down": "reaches or goes below").
            b.insert("stop".into(), json!(if o.side == Side::Buy { "up" } else { "down" }));
            b.insert("stopPrice".into(), json!(dec_text(px.value())));
            // "TP for trade price": the last trade.
            b.insert("stopPriceType".into(), json!("TP"));
        }
        if o.opts.reduce_only {
            b.insert("reduceOnly".into(), json!(true));
        }
        b.insert(
            "marginMode".into(),
            json!(match margin.kind {
                MarginKind::Cross => "CROSS",
                MarginKind::Isolated => "ISOLATED",
            }),
        );
        b.insert(
            "positionSide".into(),
            json!(match p.position {
                PositionSide::Net => "BOTH",
                PositionSide::Long => "LONG",
                PositionSide::Short => "SHORT",
            }),
        );
        Ok(Value::Object(b))
    }

    fn order_row(&self, ctx: &DecodeCtx<'_>, r: OrderRow, from_stream: Option<(&str, &str)>) -> Result<VenueOrder, ConnectorError> {
        let symbol = r.symbol.to_ascii_uppercase();
        let lots = |d: Decimal| -> Result<Decimal, ConnectorError> {
            match self.book {
                Book::Spot => Ok(d),
                Book::Futures => ctx.sizes.to_base(self.label(), &symbol, d),
            }
        };
        let word = match (from_stream, self.book) {
            (Some((status, event)), _) => stream_word(status, event, r.origin_size.0.or(r.size.0).unwrap_or_default(), r.filled(), r.filled_stated().is_some()),
            (None, Book::Spot) => spot_rest_word(&r),
            (None, Book::Futures) => futures_rest_word(&r),
        };
        let mut kind = r.kind.clone().filter(|_| from_stream.is_none()).or(r.order_type.clone()).unwrap_or_default();
        // The futures `open` frame carries no `orderType` (its example): "open" is "the order is
        // in the order book", where only a limit order rests, at its price.
        if kind.is_empty() && from_stream.is_some_and(|(_, e)| e == "open") && positive(r.price).is_some() {
            kind = "limit".into();
        }
        let stop = r.is_stop();
        let qty = lots(r.origin_size.0.or(r.size.0).unwrap_or_default())?;
        let filled = lots(r.filled())?;
        let avg = positive(r.avg_deal_price).or_else(|| match (r.deal_funds.0, r.deal_size.0) {
            (Some(f), Some(s)) if s > Decimal::ZERO && self.book == Book::Spot => f.checked_div(s),
            _ => None,
        });
        let created_ms = match (self.book, from_stream, r.created_at, r.order_time) {
            (_, None, Some(t), _) => Some(t.0),
            // Spot frames time the order in ms, futures frames in ns.
            (Book::Spot, Some(_), _, Some(t)) => Some(t.0),
            (Book::Futures, Some(_), _, Some(t)) => Some(t.0 / 1_000_000),
            _ => None,
        };
        let status = self.status_of(&word);
        Ok(VenueOrder {
            venue_id: r.venue_id(),
            client_id: r.client_oid.clone().filter(|c| !c.is_empty()),
            symbol,
            side: side_of(&r.side),
            order_type: if stop { format!("{kind}_stop") } else { kind.clone() },
            status,
            qty,
            filled_qty: filled,
            limit: if kind == "limit" { positive(r.price) } else { None },
            stop: if stop { positive(r.stop_price) } else { None },
            avg_price: avg,
            reduce_only: r.reduce_only.unwrap_or(false),
            position: position_of(r.position_side.as_deref())?,
            created_ms,
            // An untriggered stop: still waiting for its price.
            trigger_pending: stop && r.stop_triggered != Some(true) && word == OPEN && from_stream.is_none(),
            parent_client_id: None,
            exit: None,
            successor_id: None,
        })
    }

    fn orders_page(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        if ctx.endpoint == SPOT_OPEN_END {
            // An empty page here would read as "no more working orders" and a cancel all would
            // leave the rest working: the listing fails and is asked again.
            return Err(ConnectorError::new(
                otw_connector_model::ErrorKind::Transient,
                format!("{}: the walk over the symbols with working orders was lost before its end, so the list is incomplete: list again", self.label()),
            ));
        }
        if ctx.endpoint == SPOT_OPEN_SYMBOLS {
            #[derive(Deserialize)]
            struct Symbols {
                #[serde(default)]
                symbols: Vec<String>,
            }
            #[derive(Deserialize)]
            struct Answer {
                data: Option<Symbols>,
            }
            let a: Answer = serde_json::from_slice(body).map_err(|e| decode_err("symbols with open orders", e))?;
            let list: Vec<String> = a.data.map(|d| d.symbols).unwrap_or_default().into_iter().filter(|s| !s.is_empty() && !s.contains(',')).collect();
            return Ok((Vec::new(), (!list.is_empty()).then(|| format!("walk:{}", list.join(",")))));
        }
        let (rows, pages) = rows_of(body, "orders")?;
        let mut out = Vec::new();
        if ctx.endpoint == SPOT_OPEN_STOPS {
            for v in rows {
                out.push(self.spot_stop_row(ctx, v)?);
            }
            return Ok((out, next_page(pages)));
        }
        for v in rows {
            let r: OrderRow = serde_json::from_value(v).map_err(|e| decode_err("order", e))?;
            if self.book == Book::Futures && !linear(r.settle_currency.as_deref()) {
                continue;
            }
            out.push(self.order_row(ctx, r, None)?);
        }
        let next = match (ctx.endpoint, next_page(pages)) {
            (_, Some(n)) => Some(n),
            (SPOT_OPEN_MORE, None) => Some("next".to_string()),
            _ => None,
        };
        Ok((out, next))
    }

    /// A row of Get Stop Orders List ("Order status, include NEW, TRIGGERED"): working, under
    /// `cond:<id>` (the account read's id, its cancel the stop-order endpoint's); waiting for
    /// its trigger while `NEW`.
    fn spot_stop_row(&self, ctx: &DecodeCtx<'_>, v: Value) -> Result<VenueOrder, ConnectorError> {
        let mut r: OrderRow = serde_json::from_value(v).map_err(|e| decode_err("stop order", e))?;
        let status = r.status.clone().unwrap_or_default();
        if !matches!(status.as_str(), "NEW" | "TRIGGERED") {
            return Err(ConnectorError::decode(format!("kucoin stop order {}: status {status} is not NEW or TRIGGERED", r.venue_id())));
        }
        r.active = Some(true);
        let mut o = self.order_row(ctx, r, None)?;
        o.venue_id = format!("cond:{}", o.venue_id);
        o.trigger_pending = status == "NEW";
        Ok(o)
    }

    fn fills_page(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let (rows, pages) = rows_of(body, "fills")?;
        let n = rows.len();
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("fills", e))?;
        let mut out = Vec::new();
        for row in rows {
            let r: FillRow = serde_json::from_value(row).map_err(|e| decode_err("fill", e))?;
            if self.book == Book::Futures && !linear(r.settle_currency.as_deref()) {
                continue;
            }
            let symbol = r.symbol.to_ascii_uppercase();
            let size = r.size.0.unwrap_or_default();
            let (qty, at_ms, position_only) = match self.book {
                Book::Spot => (size, r.created_at.map(|t| t.0).unwrap_or_default(), false),
                // `tradeType` `liquid`, `adl` and `settlement` move a position without an order of
                // the account behind it: kept, marked.
                Book::Futures => (
                    ctx.sizes.to_base(self.label(), &symbol, size)?,
                    r.trade_time.map(|t| t.0 / 1_000_000).or(r.created_at.map(|t| t.0)).unwrap_or_default(),
                    !matches!(r.trade_type.as_deref(), Some("trade") | None),
                ),
            };
            out.push(VenueFill {
                exec_id: id_text(&r.trade_id),
                venue_order_id: r.order_id,
                client_id: None,
                symbol,
                side: side_of(&r.side),
                qty,
                price: r.price.0.unwrap_or_default(),
                fee: r.fee.0.unwrap_or_default(),
                fee_currency: r.fee_currency.to_ascii_uppercase(),
                at_ms,
                position_only,
                extra_fees: Vec::new(),
                fee_pending: false,
            });
        }
        let next = match self.book {
            Book::Futures => next_page(pages),
            Book::Spot => {
                let last = v["data"]["lastId"].as_i64().or_else(|| v["data"]["lastId"].as_str().and_then(|s| s.parse().ok()));
                last.filter(|_| n >= SPOT_FILL_PAGE).map(|l| format!("lastId:{l}"))
            }
        };
        Ok((out, next))
    }

    fn position_row(&self, ctx: &DecodeCtx<'_>, r: PositionRow) -> Result<Option<VenuePosition>, ConnectorError> {
        if r.is_inverse == Some(true) || !linear(r.settle_currency.as_deref()) || r.symbol.is_empty() {
            return Ok(None);
        }
        let symbol = r.symbol.to_ascii_uppercase();
        let lots = r.current_qty.0.unwrap_or_default();
        let qty = ctx.sizes.to_base(self.label(), &symbol, lots.abs())?;
        let side = position_of(r.position_side.as_deref())?;
        let direction = match side {
            _ if qty.is_zero() => None,
            PositionSide::Long => Some(Side::Buy),
            PositionSide::Short => Some(Side::Sell),
            PositionSide::Net if lots.is_sign_negative() => Some(Side::Sell),
            PositionSide::Net => Some(Side::Buy),
        };
        Ok(Some(VenuePosition { symbol, side, direction, qty, leverage: positive(r.leverage), margin: margin_of(r.margin_mode.as_deref(), r.cross_mode) }))
    }

    fn positions_of(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let (rows, _) = rows_of(body, "positions")?;
        let mut out = Vec::new();
        for v in rows {
            let r: PositionRow = serde_json::from_value(v).map_err(|e| decode_err("position", e))?;
            out.extend(self.position_row(ctx, r)?);
        }
        Ok(out)
    }

    fn rule_rows(&self, body: &[u8]) -> Result<Vec<VenueRules>, ConnectorError> {
        let (rows, _) = rows_of(body, "instrument list")?;
        let venue = self.book.venue();
        let mut out = Vec::new();
        for v in rows {
            match self.book {
                Book::Spot => {
                    let Ok(r) = serde_json::from_value::<SpotRule>(v) else { continue };
                    let mut x = VenueRules::new(InstrumentId::new(venue, &r.symbol.to_ascii_uppercase()));
                    x.tick = positive(r.price_increment);
                    x.lot_step = positive(r.base_increment);
                    x.min_qty = positive(r.base_min_size);
                    x.max_qty = positive(r.base_max_size);
                    x.min_notional = positive(r.min_funds);
                    x.quote_step = positive(r.quote_increment);
                    x.status = match (r.enable_trading, r.callauction_is_enabled) {
                        (Some(false), _) => TradingStatus::Closed("enableTrading false".into()),
                        // A call auction takes orders by phase, which the list does not time
                        // against the venue clock here: no order is sent during one.
                        (_, Some(true)) => TradingStatus::Closed("call auction".into()),
                        _ => TradingStatus::Trading,
                    };
                    out.push(x);
                }
                Book::Futures => {
                    let Ok(r) = serde_json::from_value::<ContractRule>(v) else { continue };
                    let Some(size) = positive(r.multiplier) else { continue };
                    if r.is_inverse == Some(true) || !linear(r.settle_currency.as_deref()) {
                        continue;
                    }
                    let mut x = VenueRules::new(InstrumentId::new(venue, &r.symbol.to_ascii_uppercase()));
                    let coins = |d: OptDec| positive(d).map(|n| n * size);
                    x.contract_size = Some(size);
                    x.tick = positive(r.tick_size);
                    x.max_price = positive(r.max_price);
                    // Sizes are whole contracts ("must be a positive integer"); `lotSize` is the
                    // "Minimum lot size".
                    x.lot_step = Some(size);
                    x.min_qty = coins(r.lot_size).or(Some(size));
                    x.max_qty = coins(r.max_order_qty);
                    x.market_max_qty = coins(r.market_max_order_qty);
                    x.status = match r.status.as_str() {
                        "Open" => TradingStatus::Trading,
                        // `Init`, `BeingSettled`, `Settled`, `Paused`, `Closed`, and
                        // `CancelOnly` ("Orders can only be canceled"): no new order.
                        other => TradingStatus::Closed(other.to_string()),
                    };
                    out.push(x);
                }
            }
        }
        Ok(out)
    }

    /// A spot page request for `symbol`, on `endpoint` (more symbols to come, or the last).
    fn spot_open_request(&self, endpoint: &'static str, symbol: &str, corr: CorrId) -> HttpRequest {
        self.signed(endpoint, Method::Get, "/api/v1/hf/orders/active/page", corr).query("symbol", symbol).query("pageNum", 1).query("pageSize", SPOT_OPEN_PAGE_SIZE)
    }

    fn private_frame(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let v: Value = serde_json::from_slice(payload).map_err(|e| decode_err("private frame", e))?;
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "welcome" => return Ok(vec![PrivateEvent::Answer { op: "welcome".into(), ok: true, message: String::new() }]),
            "ack" => return Ok(vec![PrivateEvent::Answer { op: "subscribe".into(), ok: true, message: String::new() }]),
            "pong" => return Ok(vec![PrivateEvent::Answer { op: "pong".into(), ok: true, message: String::new() }]),
            "error" => {
                let code = v.get("code").map(|c| c.to_string()).unwrap_or_default();
                return Ok(vec![PrivateEvent::Answer { op: "subscribe".into(), ok: false, message: format!("{} (code {code})", v.get("data").map(|d| d.to_string()).unwrap_or_default()) }]);
            }
            "message" => {}
            _ => return Ok(Vec::new()),
        }
        let topic = text("topic");
        let data = v.get("data").cloned().unwrap_or(Value::Null);
        let mut out = Vec::new();
        match self.book {
            Book::Spot if topic.starts_with("/spotMarket/tradeOrdersV2") => self.stream_order(ctx, data, &mut out)?,
            Book::Futures if topic.starts_with("/contractMarket/tradeOrders") => self.stream_order(ctx, data, &mut out)?,
            Book::Futures if topic.starts_with("/contractMarket/advancedOrders") => {
                let symbol = data["symbol"].as_str().unwrap_or_default().to_ascii_uppercase();
                let qty = ctx.sizes.to_base(self.label(), &symbol, dec_val(data.get("size")).unwrap_or_default())?;
                let event = data["type"].as_str().unwrap_or_default();
                let word = match event {
                    "open" | "triggered" => OPEN.to_string(),
                    "cancel" => CANCELED.to_string(),
                    other => format!("stop {other}"),
                };
                let limit = dec_val(data.get("orderPrice")).filter(|p| *p > Decimal::ZERO);
                let stop = dec_val(data.get("stopPrice")).filter(|p| *p > Decimal::ZERO);
                out.push(PrivateEvent::Order(VenueOrder {
                    venue_id: data["orderId"].as_str().unwrap_or_default().to_string(),
                    client_id: data["clientOid"].as_str().map(str::to_string).filter(|c| !c.is_empty()),
                    symbol,
                    side: side_of(data["side"].as_str().unwrap_or_default()),
                    order_type: if limit.is_some() { "limit_stop".into() } else { "market_stop".into() },
                    status: self.status_of(&word),
                    qty,
                    filled_qty: Decimal::ZERO,
                    limit,
                    stop,
                    avg_price: None,
                    // The frame's example names neither: read when a frame carries them, else
                    // the defaults, as the account read shows the row.
                    reduce_only: data["reduceOnly"].as_bool().unwrap_or(false),
                    position: position_of(data["positionSide"].as_str())?,
                    created_ms: data["createdAt"].as_i64(),
                    trigger_pending: event == "open",
                    parent_client_id: None,
                    exit: None,
                    successor_id: None,
                }));
            }
            // `position.change` carries the row; a funding settlement does not change it.
            Book::Futures if topic.starts_with("/contract/position") && text("subject") == "position.change" => {
                let r: PositionRow = serde_json::from_value(data).map_err(|e| decode_err("position push", e))?;
                if let Some(p) = self.position_row(ctx, r)? {
                    out.push(PrivateEvent::Position(p));
                }
            }
            _ => {}
        }
        Ok(out)
    }

    /// One order frame: the order, and the execution its `match` carries (no fee amount: read
    /// later from the fills).
    fn stream_order(&self, ctx: &DecodeCtx<'_>, data: Value, out: &mut Vec<PrivateEvent>) -> Result<(), ConnectorError> {
        let status = data["status"].as_str().unwrap_or_default().to_string();
        let event = data["type"].as_str().unwrap_or_default().to_string();
        let trade_type = data["tradeType"].as_str().map(str::to_string);
        let ts_ms = data["ts"].as_i64().map(|t| t / 1_000_000);
        let fill = if event == "match" {
            Some((id_text(&data.get("tradeId").cloned()), dec_val(data.get("matchPrice")), dec_val(data.get("matchSize"))))
        } else {
            None
        };
        let r: OrderRow = serde_json::from_value(data).map_err(|e| decode_err("order push", e))?;
        let o = self.order_row(ctx, r, Some((&status, &event)))?;
        if let Some((exec_id, Some(price), Some(size))) = fill {
            let qty = match self.book {
                Book::Spot => size,
                Book::Futures => ctx.sizes.to_base(self.label(), &o.symbol, size)?,
            };
            out.push(PrivateEvent::Fill(VenueFill {
                exec_id,
                venue_order_id: o.venue_id.clone(),
                client_id: o.client_id.clone(),
                symbol: o.symbol.clone(),
                side: o.side.clone(),
                qty,
                price,
                fee: Decimal::ZERO,
                fee_currency: String::new(),
                at_ms: ts_ms.unwrap_or_default(),
                position_only: self.book == Book::Futures && !matches!(trade_type.as_deref(), Some("trade") | None),
                extra_fees: Vec::new(),
                fee_pending: true,
            }));
        }
        out.push(PrivateEvent::Order(o));
        Ok(())
    }
}

impl TradeMapper for Kucoin {
    fn venue_name(&self) -> &str {
        self.label()
    }

    /// "can only contain numbers, letters, underscores (_), and hyphens (-)", "The maximum
    /// length for the ClientOid is 40 characters" (both books).
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(40)
    }

    fn place(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        match self.book {
            Book::Spot => self.post(SPOT_PLACE, "/api/v1/hf/orders", self.spot_body(o, p)?, p.corr),
            Book::Futures => self.post(FUT_PLACE, "/api/v1/orders", self.futures_body(o, p, &p.client_id)?, p.corr),
        }
    }

    fn cancel(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let (space, id) = OrderSpace::of(venue_id);
        Ok(match self.book {
            Book::Spot if space == OrderSpace::Conditional => self.signed(SPOT_STOP_CANCEL, Method::Delete, format!("/api/v1/stop-order/{id}"), corr).weight(3),
            Book::Spot => self.signed(SPOT_CANCEL, Method::Delete, format!("/api/v1/hf/orders/{id}"), corr).query("symbol", symbol),
            // "Cancel an order (including a stop order)".
            Book::Futures => self.signed(FUT_CANCEL, Method::Delete, format!("/api/v1/orders/{id}"), corr),
        })
    }

    /// Spot: Cancel All Orders By Symbol and Batch Cancel Stop Orders (the stop orders keep an
    /// endpoint of their own). Futures: Cancel All Orders ("excluding stop orders", symbol
    /// required) and Cancel All Stop orders, per symbol.
    fn cancel_all(&self, _settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut out = Vec::new();
        for s in symbols {
            match self.book {
                Book::Spot => {
                    out.push(self.signed(SPOT_CANCEL_ALL, Method::Delete, "/api/v1/hf/orders", corr).query("symbol", s));
                    out.push(self.signed(SPOT_CANCEL_STOPS, Method::Delete, "/api/v1/stop-order/cancel", corr).query("symbol", s).query("tradeType", "TRADE").weight(3));
                }
                Book::Futures => {
                    out.push(self.signed(FUT_CANCEL_ALL, Method::Delete, "/api/v3/orders", corr).query("symbol", s));
                    out.push(self.signed(FUT_CANCEL_STOPS, Method::Delete, "/api/v1/stopOrders", corr).query("symbol", s));
                }
            }
        }
        Ok(out)
    }

    /// Spot by-symbol cancel-all answers `data: "success"`; Batch Cancel Stop Orders
    /// `data.cancelledOrderIds`.
    fn decode_cancel_all(&self, body: &[u8]) -> Result<(), ConnectorError> {
        if self.book == Book::Futures {
            return Ok(());
        }
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("cancel all", e))?;
        match v.get("data") {
            Some(Value::String(s)) if s == "success" => Ok(()),
            Some(Value::Object(o)) if o.get("cancelledOrderIds").is_some_and(Value::is_array) => Ok(()),
            other => Err(ConnectorError::new(otw_connector_model::ErrorKind::Unknown, format!("kucoin cancel all answered {other:?}, not \"success\""))),
        }
    }

    fn query_by_client_id(&self, _settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![match self.book {
            Book::Spot => self.signed(SPOT_BY_CID, Method::Get, format!("/api/v1/hf/orders/client-order/{client_id}"), corr).query("symbol", symbol),
            // "Get a single order by client order ID (including a stop order)".
            Book::Futures => self.signed(FUT_BY_CID, Method::Get, "/api/v1/orders/byClientOid", corr).query("clientOid", client_id),
        }])
    }

    /// A spot stop order is not read back here (its rows are not HF orders' rows): none.
    fn query_by_venue_id(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let (space, id) = OrderSpace::of(venue_id);
        if self.book == Book::Spot && space == OrderSpace::Conditional {
            return Ok(Vec::new());
        }
        Ok(vec![match self.book {
            Book::Spot => self.signed(SPOT_BY_ID, Method::Get, format!("/api/v1/hf/orders/{id}"), corr).query("symbol", symbol),
            Book::Futures => self.signed(FUT_BY_ID, Method::Get, format!("/api/v1/orders/{id}"), corr),
        }])
    }

    /// Spot lists working orders per symbol only: the whole book starts from the symbols with
    /// working orders and walks them ([`TradeMapper::with_cursor`]), then lists the stop orders. Futures: the working orders
    /// and the untriggered stops ("Stop orders that have been triggered can be queried through
    /// the general order interface").
    fn open_orders(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(match self.book {
            Book::Spot => match symbol {
                Some(s) => vec![self.spot_open_request(SPOT_OPEN_PAGE, s, corr)],
                // The whole book: the HF orders symbol by symbol, then the stop orders (Get Stop
                // Orders List, `tradeType` "Default is TRADE", sent anyway), so a cancel all
                // reaches them too.
                None => vec![
                    self.signed(SPOT_OPEN_SYMBOLS, Method::Get, "/api/v1/hf/orders/active/symbols", corr),
                    self.signed(SPOT_OPEN_STOPS, Method::Get, "/api/v1/stop-order", corr)
                        .query("tradeType", "TRADE")
                        .query("currentPage", 1)
                        .query("pageSize", SPOT_STOP_PAGE_SIZE)
                        .weight(8),
                ],
            },
            Book::Futures => {
                let mut orders = self.signed(FUT_OPEN, Method::Get, "/api/v1/orders", corr).query("status", "active");
                let mut stops = self.signed(FUT_OPEN_STOPS, Method::Get, "/api/v1/stopOrders", corr);
                if let Some(s) = symbol {
                    orders = orders.query("symbol", s);
                    stops = stops.query("symbol", s);
                }
                vec![orders.query("currentPage", 1).query("pageSize", FUTURES_PAGE_SIZE), stops.query("currentPage", 1).query("pageSize", FUTURES_PAGE_SIZE)]
            }
        })
    }

    fn fills_since(&self, _settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(match self.book {
            Book::Spot => {
                self.signed(SPOT_FILLS, Method::Get, "/api/v1/hf/fills", corr).query("symbol", symbol).query("startAt", from_ms).query("endAt", to_ms).query("limit", SPOT_FILL_PAGE)
            }
            Book::Futures => self
                .signed(FUT_FILLS, Method::Get, "/api/v1/fills", corr)
                .query("symbol", symbol)
                .query("startAt", from_ms)
                .query("endAt", to_ms)
                .query("currentPage", 1)
                .query("pageSize", FUTURES_PAGE_SIZE),
        })
    }

    fn fills_window_ms(&self) -> Option<i64> {
        Some(FILLS_WINDOW_MS)
    }

    /// The whole list ("all contracts that can be traded" on futures, every pair on spot),
    /// whatever `symbol`: the trader picks the symbol, and a futures book reads every contract's
    /// size from the same call.
    fn instrument_rules(&self, _settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(match self.book {
            Book::Spot => HttpRequest::get(SPOT_SYMBOLS, REST, "/api/v2/symbols", corr).weight(4),
            Book::Futures => HttpRequest::get(FUT_CONTRACTS, REST, "/api/v1/contracts/active", corr).weight(3),
        })
    }

    /// A contract missing from the active list is asked by name: listed but not open (paused,
    /// settling, cancel-only), it is refused as such, never as unknown.
    fn instrument_rules_any_status(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(match self.book {
            Book::Spot => Vec::new(),
            Book::Futures => vec![HttpRequest::get(FUT_CONTRACT, REST, format!("/api/v1/contracts/{symbol}"), corr).weight(3)],
        })
    }

    /// "Market orders are not supported by the Time in Force strategy": a market order (or a
    /// stop, market once triggered) asked fill-or-kill is refused rather than worked otherwise.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        if matches!(o.order_type, OrderType::Market | OrderType::Stop) && o.tif == TimeInForce::Fok {
            return Err(ConnectorError::unsupported(format!(
                "{} takes no time in force on a market order, so it cannot be fill-or-kill: send it as is, or as a limit order FOK",
                self.label()
            )));
        }
        Ok(())
    }

    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let set = |key: &str, value: String| {
            let mut next = req.clone();
            next.query.retain(|(k, _)| k != key);
            next.query.push((key.to_string(), value));
            next
        };
        if let Some(n) = cursor.strip_prefix("page:") {
            let key = if matches!(req.endpoint_id, SPOT_OPEN_MORE | SPOT_OPEN_PAGE) { "pageNum" } else { "currentPage" };
            return set(key, n.to_string());
        }
        if let Some(id) = cursor.strip_prefix("lastId:") {
            return set("lastId", id.to_string());
        }
        let corr = req.corr_id;
        if let Some(list) = cursor.strip_prefix("walk:") {
            let mut rest: Vec<String> = list.split(',').map(str::to_string).collect();
            let first = rest.remove(0);
            let endpoint = if rest.is_empty() { SPOT_OPEN_PAGE } else { SPOT_OPEN_MORE };
            let mut walks = lock(&self.walks);
            // A listing that never finished leaves its rest behind: dropped by age, so a walk
            // still running is never wiped by others starting.
            let now = std::time::Instant::now();
            walks.retain(|_, (moved, _)| now.duration_since(*moved) < WALK_TTL);
            if !rest.is_empty() {
                walks.insert(corr.0, (now, rest));
            }
            return self.spot_open_request(endpoint, &first, corr);
        }
        if cursor == "next" {
            let mut walks = lock(&self.walks);
            let next = walks.get_mut(&corr.0).filter(|(_, r)| !r.is_empty()).map(|(moved, r)| {
                *moved = std::time::Instant::now();
                r.remove(0)
            });
            let done = walks.get(&corr.0).is_none_or(|(_, r)| r.is_empty());
            if done {
                walks.remove(&corr.0);
            }
            return match next {
                Some(s) => self.spot_open_request(if done { SPOT_OPEN_PAGE } else { SPOT_OPEN_MORE }, &s, corr),
                None => self.signed(SPOT_OPEN_END, Method::Get, "/api/v1/hf/orders/active/symbols", corr),
            };
        }
        set("cursor", cursor.to_string())
    }

    /// Futures: Get Position Details for the symbol (both sides in hedge mode).
    fn positions(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(match self.book {
            Book::Spot => None,
            Book::Futures => Some(self.signed(FUT_POSITION, Method::Get, "/api/v2/position", corr).query("symbol", symbol)),
        })
    }

    fn all_positions(&self, _settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(match self.book {
            Book::Spot => Vec::new(),
            Book::Futures => SETTLE_COINS.iter().map(|c| self.signed(FUT_POSITIONS, Method::Get, "/api/v1/positions", corr).query("currency", c)).collect(),
        })
    }

    /// `330011` ("Before initiating any Futures trades, please use the Switch Position Mode
    /// endpoint ...") and a parameter error on an order (the doc names no code for a
    /// `positionSide` that does not match the mode): the mode is read again before the next
    /// order. The refused order is not sent again.
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.book == Book::Futures && matches!(e.code.as_deref(), Some("330011" | "300000" | "100001"))
    }

    /// `330005` ("Please switch your margin mode to cross margin mode and try again") and a
    /// parameter error: the symbol's margin mode is read again before the next order.
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.book == Book::Futures && matches!(e.code.as_deref(), Some("330005" | "300000" | "100001"))
    }

    /// Get Position Mode: "Position mode setting applies to all futures trading pairs".
    fn account_position_mode(&self, _settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok((self.book == Book::Futures).then(|| self.signed(FUT_POSITION_MODE, Method::Get, "/api/v2/position/getPositionMode", corr)))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("position mode", e))?;
        let raw = &v["data"]["positionMode"];
        match raw.as_i64().or_else(|| raw.as_str().and_then(|s| s.parse().ok())) {
            Some(0) => Ok(Some(PositionMode::OneWay)),
            Some(1) => Ok(Some(PositionMode::Hedge)),
            _ => Err(ConnectorError::decode(format!("kucoin position mode: {raw} is not 0 (one-way) or 1 (hedge)"))),
        }
    }

    /// Get Margin Mode: "query the margin mode of the current symbol".
    fn symbol_margin(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok((self.book == Book::Futures).then(|| self.signed(FUT_MARGIN_MODE, Method::Get, "/api/v2/position/getMarginMode", corr).query("symbol", symbol)))
    }

    fn decode_symbol_margin(&self, symbol: &str, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("margin mode", e))?;
        match v["data"]["marginMode"].as_str() {
            Some(w) => margin_of(Some(w), None).map(Some).ok_or_else(|| ConnectorError::decode(format!("kucoin margin mode of {symbol}: {w} is not CROSS or ISOLATED"))),
            None => Ok(None),
        }
    }

    fn counts_contracts(&self) -> bool {
        self.book == Book::Futures
    }

    /// Add Order Test: "the request parameters and return parameters ... exactly the same as the
    /// order endpoint", and "the order will not enter the matching system". Futures requires a
    /// `clientOid` even there: a check id of its own, never the live order's.
    fn dry_run(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(match self.book {
            Book::Spot => self.post(SPOT_TEST, "/api/v1/hf/orders/test", self.spot_body(o, p)?, p.corr)?,
            Book::Futures => {
                let cid = if p.client_id.is_empty() { format!("otw-check-{}", p.corr.0) } else { p.client_id.clone() };
                self.post(FUT_TEST, "/api/v1/orders/test", self.futures_body(o, p, &cid)?, p.corr)?
            }
        }))
    }

    fn decode_dry_run(&self, body: &[u8]) -> Result<String, ConnectorError> {
        let ack = self.decode_ack(body)?;
        Ok(format!("accepted by KuCoin's order test (test id {}, not placed)", ack.venue_id))
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(self.book, frame)
    }

    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("order ack", e))?;
        let id = v["data"]["orderId"].as_str().unwrap_or_default();
        if id.is_empty() {
            return Err(ConnectorError::decode("kucoin order ack: no orderId"));
        }
        Ok(Ack { venue_id: id.to_string(), client_id: v["data"]["clientOid"].as_str().map(str::to_string).filter(|c| !c.is_empty()) })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.orders_page(&DecodeCtx::new("", &ContractSizes::new()), body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.orders_page(ctx, body)
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.fills_page(&DecodeCtx::new("", &ContractSizes::new()), body)
    }

    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.fills_page(ctx, body)
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        Ok((self.rule_rows(body)?, None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.positions_of(&DecodeCtx::new("", &ContractSizes::new()), body)
    }

    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.positions_of(ctx, body)
    }

    /// The mapper's own words (KuCoin's REST answers carry none, see [`spot_rest_word`] and
    /// [`futures_rest_word`]): exhaustive, anything else unknown.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            OPEN => OrderStatus::New,
            PARTIAL => OrderStatus::PartiallyFilled,
            FILLED => OrderStatus::Filled,
            CANCELED => OrderStatus::Cancelled,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    /// The private order topics (spot `/spotMarket/tradeOrdersV2`; futures
    /// `/contractMarket/tradeOrders`, `/contractMarket/advancedOrders`, `/contract/positionAll`).
    /// The URL and the heartbeat come from the token call before each connection.
    fn private_socket(&self, _settings: &Settings) -> Option<PrivateSocket> {
        let sub = |id: &str, topic: &str| json!({ "id": id, "type": "subscribe", "topic": topic, "privateChannel": true, "response": true }).to_string().into_bytes();
        let ping = json!({ "id": "otw-ping", "type": "ping" }).to_string().into_bytes();
        Some(match self.book {
            Book::Spot => PrivateSocket::new(REST, "/spotMarket/tradeOrdersV2", sub("otw-orders", "/spotMarket/tradeOrdersV2"), ping, ping_cadence(DEFAULT_PING_MS)),
            Book::Futures => {
                let mut s = PrivateSocket::new(REST, "/contractMarket/tradeOrders", sub("otw-orders", "/contractMarket/tradeOrders"), ping, ping_cadence(DEFAULT_PING_MS));
                s.more = vec![sub("otw-stops", "/contractMarket/advancedOrders"), sub("otw-positions", "/contract/positionAll")];
                s
            }
        })
    }

    /// Get Private Token: a signed `POST /api/v1/bullet-private` on the book's own host ("cannot
    /// be mixed").
    fn private_socket_bootstrap(&self, _settings: &Settings, _socket: &PrivateSocket, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(self.signed(BULLET, Method::Post, "/api/v1/bullet-private", corr).weight(10)))
    }

    /// `endpoint?token=..&connectId=..` ("Create connection"), pinged every half `pingInterval`.
    fn decode_private_socket_bootstrap(&self, mut socket: PrivateSocket, body: &[u8]) -> Result<PrivateSocket, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("bullet-private", e))?;
        let token = v["data"]["token"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| ConnectorError::decode("kucoin bullet-private: no token"))?;
        let server = &v["data"]["instanceServers"][0];
        let endpoint = server["endpoint"].as_str().filter(|e| !e.is_empty()).ok_or_else(|| ConnectorError::decode("kucoin bullet-private: no socket server"))?;
        // "The connection ID is a unique value taken from the client side": one per token.
        let connect_id = hex::encode(&otw_connector_sdk::signer::sha256(token.as_bytes())[..8]);
        socket.url = Some(format!("{endpoint}?token={token}&connectId={connect_id}"));
        if let Some(ms) = server["pingInterval"].as_u64().filter(|ms| *ms > 0) {
            socket.ping_every_ms = ping_cadence(ms);
        }
        Ok(socket)
    }

    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.private_frame(&DecodeCtx::new("", &ContractSizes::new()), payload)
    }

    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.private_frame(ctx, payload)
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        super::trade_errors::table(self.book)
    }
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod tests;
