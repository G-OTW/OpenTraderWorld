//! MEXC orders (spot v3 and the USDT perpetuals of the contract API), written from the
//! documentation alone: no account, no key. Every field below is the doc's; each claim is
//! sourced in PROVIDER.md ("Trading"). The order path itself is the shared trader's; this file
//! only says what the calls are and what the answers mean.
//!
//! - Spot place: `POST /api/v3/order`, the parameters in the body as the raw `key=value&...`
//!   string the doc asks for (`Content-Type: application/json`, never form-encoded); the signer
//!   puts the window, the time and the signature in the query (totalParams = query + body).
//!   The time in force is the type: `LIMIT`, `LIMIT_MAKER` (post-only), `IMMEDIATE_OR_CANCEL`,
//!   `FILL_OR_KILL`, `MARKET`. A market buy is an amount to spend (`quoteOrderQty`).
//! - Futures place: `POST /api/v1/private/order/create` on `api.mexc.com`, a JSON body signed as
//!   sent. Sizes are whole contracts (`counts_contracts`). Hedge mode only (`positionMode` 1):
//!   the doc does not say which `side` code a one-way order takes, nor how `reduceOnly` ("only
//!   applicable in one-way mode") combines with it, so every order of a one-way account, a close
//!   included, is refused before I/O. The margin (`openType`) and leverage are the ones the
//!   venue holds (`position/leverage`), never chosen by OpenTraderWorld.
//! - Client id: spot `newClientOrderId`, futures `externalOid` ("max. 32 digit").
//! - Private streams: spot protobuf frames behind a listen key (made before each connection,
//!   extended every 30 minutes); futures JSON on `contract.mexc.com/edge`, everything pushed
//!   after the login.

#![deny(clippy::float_arithmetic)]

use std::str::FromStr;

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderRequest, OrderStatus, OrderType, Side, TimeInForce};
use otw_connector_sdk::parse::{Int, OptDec};
use otw_connector_sdk::signer::encode_query;
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, MarginMode, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper, TradingStatus,
    VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{proto, Book, Mexc, CONTRACTS_PATH, FUT, FUT_WS, REST, WS};

/// The futures single cancel's endpoint id: its answer is a row per id, read for the error.
pub(super) const FUT_CANCEL: &str = "fut_cancel";
/// Spot rows per fills call: `limit` "Default 100; max 100", no cursor.
const SPOT_FILL_PAGE: usize = 100;
/// The spot fills window: one call answers 100 rows at most and pages no further, so a window
/// stays short; a full one is said, never cut silently.
const SPOT_FILLS_WINDOW_MS: i64 = 3_600_000;
/// Futures rows per list call (`page_size` "max 100" on open orders).
const FUT_PAGE: usize = 100;
/// The futures fills window: the doc gives `order_deals` "the maximum span is 90 days".
const FUT_FILLS_WINDOW_MS: i64 = 30 * 86_400_000;
/// Futures ping: "Send a ping every 10-20 seconds."
const FUT_PING_MS: u64 = 15_000;
/// Spot ping: "If the subscription is successful but there is no data flow, the server will
/// disconnect after one minute."
const SPOT_PING_MS: u64 = 20_000;
/// Listen key extension: "It is recommended to send a request every 30 minutes."
const LISTEN_KEY_EVERY_MS: u64 = 30 * 60_000;
const SPOT_ORDERS: &str = "spot@private.orders.v3.api.pb";
const SPOT_DEALS: &str = "spot@private.deals.v3.api.pb";
const LISTEN_KEY: &str = "listenKey=";

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("mexc {what}: {e}"))
}

/// An id written as a number on one endpoint and a string on another.
fn id_text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

/// A JSON object written by hand, so a decimal travels as the exact number it is (the venue
/// reads `price` and `vol` as numbers; no float on the way).
struct Body(Vec<(&'static str, String)>);

impl Body {
    fn text(&mut self, k: &'static str, v: &str) {
        self.0.push((k, Value::String(v.to_string()).to_string()));
    }

    fn number(&mut self, k: &'static str, v: Decimal) {
        self.0.push((k, dec_text(v)));
    }

    fn int(&mut self, k: &'static str, v: i64) {
        self.0.push((k, v.to_string()));
    }

    fn bytes(self) -> Vec<u8> {
        let fields: Vec<String> = self.0.into_iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        format!("{{{}}}", fields.join(",")).into_bytes()
    }
}

/// The futures `side` code: what the order does to which position (hedge mode).
fn fut_side_code(position: PositionSide, side: Side) -> Option<i64> {
    match (position, side) {
        (PositionSide::Long, Side::Buy) => Some(1),
        (PositionSide::Short, Side::Buy) => Some(2),
        (PositionSide::Short, Side::Sell) => Some(3),
        (PositionSide::Long, Side::Sell) => Some(4),
        (PositionSide::Net, _) => None,
    }
}

/// A futures `side` code read back: the trade's direction (closing a long sells) and the
/// position it acts on.
fn fut_side_of(code: i64) -> (Mapped<Side>, PositionSide, bool) {
    match code {
        1 => (Mapped::Known(Side::Buy), PositionSide::Long, false),
        2 => (Mapped::Known(Side::Buy), PositionSide::Short, true),
        3 => (Mapped::Known(Side::Sell), PositionSide::Short, false),
        4 => (Mapped::Known(Side::Sell), PositionSide::Long, true),
        other => (Mapped::Unknown(other.to_string()), PositionSide::Net, false),
    }
}

/// The same, on a row that states its `positionMode` ("1 Hedge Mode; 2 One-way Mode"): a
/// one-way row acts on the one net position, its direction still the code's.
fn fut_side_in(code: i64, position_mode: Option<i64>) -> (Mapped<Side>, PositionSide, bool) {
    let (side, position, closing) = fut_side_of(code);
    if position_mode == Some(2) {
        return (side, PositionSide::Net, closing);
    }
    (side, position, closing)
}

fn fut_type_word(code: i64) -> String {
    match code {
        1 => "limit",
        2 => "post only",
        3 => "ioc",
        4 => "fok",
        5 => "market",
        6 => "market to limit",
        _ => "",
    }
    .to_string()
}

fn margin_of(open_type: Option<i64>) -> Option<MarginMode> {
    match open_type {
        Some(1) => Some(MarginMode::isolated()),
        Some(2) => Some(MarginMode::cross()),
        _ => None,
    }
}

/// Rows of an answer that is one object, a list of them, or nothing.
fn rows_of<T: for<'de> Deserialize<'de>>(what: &str, v: Value) -> Result<Vec<T>, ConnectorError> {
    match v {
        Value::Array(items) => items.into_iter().map(|i| serde_json::from_value(i).map_err(|e| decode_err(what, e))).collect(),
        Value::Null => Ok(Vec::new()),
        one => Ok(vec![serde_json::from_value(one).map_err(|e| decode_err(what, e))?]),
    }
}

/// The contract API's envelope: `data` is what is read.
fn fut_data(what: &str, body: &[u8]) -> Result<Value, ConnectorError> {
    #[derive(Deserialize)]
    struct Env {
        #[serde(default)]
        data: Value,
    }
    Ok(serde_json::from_slice::<Env>(body).map_err(|e| decode_err(what, e))?.data)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotOrder {
    #[serde(default)]
    symbol: String,
    #[serde(deserialize_with = "id_text", default)]
    order_id: String,
    #[serde(default)]
    client_order_id: Option<String>,
    #[serde(default)]
    orig_client_order_id: Option<String>,
    #[serde(default)]
    price: OptDec,
    /// "Qty" in the Query Order example, `origQty` everywhere else.
    #[serde(default, alias = "Qty")]
    orig_qty: OptDec,
    #[serde(default)]
    executed_qty: OptDec,
    #[serde(default)]
    cummulative_quote_qty: OptDec,
    #[serde(default)]
    status: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotTrade {
    #[serde(default)]
    symbol: String,
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(deserialize_with = "id_text", default)]
    order_id: String,
    #[serde(default)]
    client_order_id: Option<String>,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    qty: OptDec,
    #[serde(default)]
    commission: OptDec,
    #[serde(default)]
    commission_asset: Option<String>,
    #[serde(default)]
    time: Option<Int>,
    #[serde(default)]
    is_buyer: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRow {
    #[serde(default)]
    symbol: String,
    /// "1 - online, 2 - Pause, 3 - offline" (the example writes `ENABLED`).
    #[serde(default, deserialize_with = "id_text")]
    status: String,
    #[serde(default)]
    base_asset_precision: Option<Int>,
    #[serde(default)]
    quote_precision: Option<Int>,
    #[serde(default)]
    quote_asset_precision: Option<Int>,
    /// "min order quantity".
    #[serde(default)]
    base_size_precision: OptDec,
    /// "min order amount".
    #[serde(default)]
    quote_amount_precision: OptDec,
    #[serde(default)]
    max_quote_amount: OptDec,
    #[serde(default)]
    is_spot_trading_allowed: Option<bool>,
    /// "1 - All, 2 - buy order only, 3 - Sell order only, 4 - Close".
    #[serde(default, deserialize_with = "id_text")]
    trade_side_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookTicker {
    #[serde(default)]
    bid_price: OptDec,
    #[serde(default)]
    ask_price: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutOrder {
    #[serde(deserialize_with = "id_text", default)]
    order_id: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    vol: OptDec,
    #[serde(default)]
    deal_vol: OptDec,
    #[serde(default)]
    deal_avg_price: OptDec,
    #[serde(default)]
    side: Option<Int>,
    #[serde(default)]
    order_type: Option<Int>,
    #[serde(default)]
    state: Option<Int>,
    #[serde(default)]
    external_oid: Option<String>,
    #[serde(default)]
    reduce_only: Option<bool>,
    /// "Position mode: 1 Hedge Mode; 2 One-way Mode".
    #[serde(default)]
    position_mode: Option<Int>,
    #[serde(default)]
    create_time: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutDeal {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: Option<Int>,
    #[serde(default)]
    vol: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: Option<String>,
    #[serde(default)]
    timestamp: Option<Int>,
    #[serde(deserialize_with = "id_text", default)]
    order_id: String,
    #[serde(default)]
    external_oid: Option<String>,
    /// "1 limit, 2 forced liquidation custody, 3 custody close, 4 ADL".
    #[serde(default)]
    category: Option<Int>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FutPosition {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    hold_vol: OptDec,
    /// "1 long 2 short", in either position mode.
    #[serde(default)]
    position_type: Option<Int>,
    /// "1 isolated 2 cross".
    #[serde(default)]
    open_type: Option<Int>,
}

/// A `position/leverage` row: one per position side.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeverageRow {
    #[serde(default)]
    position_type: Option<Int>,
    #[serde(default)]
    open_type: Option<Int>,
    #[serde(default)]
    leverage: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContractRules {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    quote_coin: String,
    #[serde(default)]
    settle_coin: String,
    #[serde(default)]
    contract_size: OptDec,
    #[serde(default)]
    price_unit: OptDec,
    #[serde(default)]
    vol_unit: OptDec,
    #[serde(default)]
    min_vol: OptDec,
    #[serde(default)]
    max_vol: OptDec,
    #[serde(default)]
    limit_max_vol: OptDec,
    #[serde(default)]
    state: Option<Int>,
    #[serde(default)]
    api_allowed: Option<bool>,
    #[serde(default)]
    future_type: Option<Int>,
}

/// The futures fills answer: a list (the doc's example) or a page holding `resultList` (its
/// field table).
fn deals_of(v: Value) -> Result<Vec<FutDeal>, ConnectorError> {
    match v {
        Value::Object(mut o) if o.contains_key("resultList") => rows_of("order deals", o.remove("resultList").unwrap_or(Value::Null)),
        other => rows_of("order deals", other),
    }
}

/// A futures conditional order MEXC keeps on endpoints of its own, placed elsewhere (the MEXC
/// app, another program): a plan (trigger) order, a position's take profit and stop loss
/// (`stoporder`), a trailing order (`trackorder`). Listed with the account's orders under
/// `cond:<kind>:<id>` and cancelled (one, or every one with cancel all); never placed from OTW.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CondKind {
    Plan,
    Stop,
    Track,
}

impl CondKind {
    pub(super) const ALL: [CondKind; 3] = [CondKind::Plan, CondKind::Stop, CondKind::Track];

    pub(super) fn word(self) -> &'static str {
        match self {
            CondKind::Plan => "plan",
            CondKind::Stop => "stop",
            CondKind::Track => "track",
        }
    }

    /// The kind a list call's endpoint id reads: a working list ([`CondList`]) or a lookup's.
    fn of_endpoint(endpoint: &str) -> Option<CondKind> {
        match endpoint {
            PLAN_OPEN | PLAN_ANY => Some(CondKind::Plan),
            STOP_OPEN | STOP_ANY => Some(CondKind::Stop),
            TRACK_OPEN | TRACK_ANY => Some(CondKind::Track),
            _ => None,
        }
    }

    /// The id OTW follows it by: `cond:plan:123`.
    pub(super) fn tag(self, id: &str) -> String {
        format!("{}{}:{id}", otw_connector_sdk::trade::OrderSpace::CONDITIONAL, self.word())
    }

    /// The kind and MEXC's id an id OTW follows names, when it names a conditional order.
    fn parse(venue_id: &str) -> Option<(CondKind, &str)> {
        let rest = venue_id.strip_prefix(otw_connector_sdk::trade::OrderSpace::CONDITIONAL)?;
        let (word, id) = rest.split_once(':')?;
        Some((CondKind::ALL.into_iter().find(|k| k.word() == word)?, id))
    }

    /// The calls that find one of this kind by its id, whatever its state (working, cancelled,
    /// triggered), asked in turn:
    /// - plan: `GET planorder/list/orders` with no `states` (every state), in the window the
    ///   signer writes ([`super::signer::PLAN_LIST_PATH`]);
    /// - stop: the working ones (`stoporder/open_orders`), then `GET stoporder/list/orders`
    ///   with no `is_finished` (every state);
    /// - track: `GET trackorder/list/orders` once per state, 0 to 4 ([`CondList::Track`]).
    pub(super) fn lookup_requests(self, corr: CorrId) -> Vec<HttpRequest> {
        match self {
            CondKind::Plan => vec![HttpRequest::get(PLAN_ANY, FUT, super::signer::PLAN_LIST_PATH, corr).signed().query("page_num", 1).query("page_size", FUT_PAGE)],
            CondKind::Stop => vec![
                CondList::Stop.request(1, corr),
                HttpRequest::get(STOP_ANY, FUT, "/api/v1/private/stoporder/list/orders", corr).signed().query("page_num", 1).query("page_size", FUT_PAGE),
            ],
            CondKind::Track => (0..=4).map(|s| track_list(TRACK_ANY, s, corr)).collect(),
        }
    }
}

/// Endpoint ids of the conditional lists: the working ones ([`CondList`]), and the lookups'
/// over every state ([`CondKind::lookup_requests`]).
pub(super) const PLAN_OPEN: &str = "fut_plan_open";
pub(super) const STOP_OPEN: &str = "fut_stop_open";
pub(super) const TRACK_OPEN: &str = "fut_track_open";
const PLAN_ANY: &str = "fut_plan_orders";
const STOP_ANY: &str = "fut_stop_orders";
const TRACK_ANY: &str = "fut_track_orders";

/// `GET trackorder/list/orders` for one state: `states` is a `List<int>` and the doc does not
/// say how a GET writes several, so each call names one.
fn track_list(endpoint: &'static str, state: u8, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, FUT, "/api/v1/private/trackorder/list/orders", corr).signed().query("states", state).query("pageIndex", 1).query("pageSize", FUT_PAGE)
}

/// One list of working conditional orders, read with the account's orders:
/// - plan: `GET planorder/list/orders`, `states` "1: untriggered", by page, in the window the
///   signer writes (`start_time` and `end_time` are required);
/// - stop: `GET stoporder/open_orders` (the current TP/SL orders, no page, no window);
/// - track: `GET trackorder/list/orders`, one call for `states` 0 ("Not activated") and one
///   for 1 ("Activated"), read in one page (`pageIndex`, which the cursor does not move).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CondList {
    Plan,
    Stop,
    Track(u8),
}

impl CondList {
    /// In the order they are read.
    pub(super) const OPEN: [CondList; 4] = [CondList::Plan, CondList::Stop, CondList::Track(0), CondList::Track(1)];

    pub(super) fn kind(self) -> CondKind {
        match self {
            CondList::Plan => CondKind::Plan,
            CondList::Stop => CondKind::Stop,
            CondList::Track(_) => CondKind::Track,
        }
    }

    /// Its word in an account cursor (`plan:2`, `track1:1`).
    pub(super) fn word(self) -> &'static str {
        match self {
            CondList::Plan => "plan",
            CondList::Stop => "stop",
            CondList::Track(0) => "track0",
            CondList::Track(_) => "track1",
        }
    }

    pub(super) fn of_word(word: &str) -> Option<CondList> {
        CondList::OPEN.into_iter().find(|l| l.word() == word)
    }

    /// The list after this one, if any.
    pub(super) fn next(self) -> Option<CondList> {
        let at = CondList::OPEN.iter().position(|l| *l == self)?;
        CondList::OPEN.get(at + 1).copied()
    }

    /// Whether a full page asks the next (`page_num`): the plan list only.
    pub(super) fn paged(self) -> bool {
        self == CondList::Plan
    }

    pub(super) fn request(self, page: u32, corr: CorrId) -> HttpRequest {
        match self {
            CondList::Plan => HttpRequest::get(PLAN_OPEN, FUT, super::signer::PLAN_LIST_PATH, corr).signed().query("states", 1).query("page_num", page).query("page_size", FUT_PAGE),
            CondList::Stop => HttpRequest::get(STOP_OPEN, FUT, "/api/v1/private/stoporder/open_orders", corr).signed(),
            CondList::Track(s) => track_list(TRACK_OPEN, s, corr),
        }
    }
}

/// One row of a conditional list, as each of the three writes it (only the fields read).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CondRow {
    #[serde(deserialize_with = "id_text", default)]
    pub id: String,
    #[serde(default)]
    pub symbol: String,
    /// Plan and trailing: "1 open long, 2 close short, 3 open short, 4 close long".
    #[serde(default)]
    pub side: Option<Int>,
    /// Stop (take profit, stop loss): the position it guards, "1: long, 2: short".
    #[serde(default)]
    pub position_type: Option<Int>,
    #[serde(default)]
    pub vol: OptDec,
    /// Plan: the execution price.
    #[serde(default)]
    pub price: OptDec,
    /// Plan: "1: Limit ... 5: Market".
    #[serde(default)]
    pub order_type: Option<Int>,
    #[serde(default)]
    pub trigger_price: OptDec,
    /// Trailing: the price it activates at.
    #[serde(default)]
    pub active_price: OptDec,
    #[serde(default)]
    pub take_profit_price: OptDec,
    #[serde(default)]
    pub stop_loss_price: OptDec,
    #[serde(default)]
    pub state: Option<Int>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    /// Plan: "User-set position mode. Default 0: no record (historical orders); 1: Hedge Mode;
    /// 2: One-way Mode".
    #[serde(default)]
    pub position_mode: Option<Int>,
    #[serde(default)]
    pub create_time: Option<Int>,
}

impl CondRow {
    /// The trade's direction and whether it closes, from `side` (plan, trailing) or the guarded
    /// position (stop: closing a long sells).
    pub(super) fn direction(&self) -> (Mapped<Side>, PositionSide, bool) {
        match (self.side, self.position_type.map(|t| t.0)) {
            (Some(s), _) => fut_side_in(s.0, self.position_mode.map(|m| m.0)),
            (None, Some(1)) => (Mapped::Known(Side::Sell), PositionSide::Long, true),
            (None, Some(2)) => (Mapped::Known(Side::Buy), PositionSide::Short, true),
            (None, other) => (Mapped::Unknown(format!("{other:?}")), PositionSide::Net, true),
        }
    }

    /// Its type word, its limit (a plan's execution price, a stop's take profit) and its trigger
    /// (a plan's, a stop's stop loss, a trailing order's trigger or activation price).
    pub(super) fn shape(&self, kind: CondKind) -> (&'static str, Option<Decimal>, Option<Decimal>) {
        match kind {
            CondKind::Plan if self.order_type.map(|t| t.0) == Some(5) => ("stop", None, positive(self.trigger_price)),
            CondKind::Plan => ("stop_limit", positive(self.price), positive(self.trigger_price)),
            CondKind::Stop => ("take_profit_stop_loss", positive(self.take_profit_price), positive(self.stop_loss_price)),
            CondKind::Track => ("trailing_stop", None, positive(self.trigger_price).or(positive(self.active_price))),
        }
    }

    /// Plan and stop: "1: untriggered" works; trailing: "0 Not activated; 1 Activated" work.
    /// The rest are final as their words say.
    fn status(&self, kind: CondKind) -> Mapped<OrderStatus> {
        let st = self.state.map(|s| s.0);
        Mapped::Known(match (kind, st) {
            (CondKind::Track, Some(0 | 1)) | (CondKind::Plan | CondKind::Stop, Some(1)) => OrderStatus::New,
            (CondKind::Track, Some(2)) | (CondKind::Plan | CondKind::Stop, Some(3)) => OrderStatus::Filled,
            (CondKind::Track, Some(4)) | (CondKind::Plan | CondKind::Stop, Some(2)) => OrderStatus::Cancelled,
            (CondKind::Track, Some(3)) | (CondKind::Plan | CondKind::Stop, Some(4 | 5)) => OrderStatus::Rejected,
            (_, other) => return Mapped::Unknown(format!("{} state {other:?}", kind.word())),
        })
    }
}

/// The rows of a conditional list: the list itself, or a page object holding it.
pub(super) fn cond_rows(v: Value) -> Result<Vec<CondRow>, ConnectorError> {
    match v {
        Value::Object(mut o) if o.contains_key("resultList") => rows_of("conditional orders", o.remove("resultList").unwrap_or(Value::Null)),
        other => rows_of("conditional orders", other),
    }
}

impl Mexc {
    fn is_futures(&self) -> bool {
        self.book == Book::Futures
    }

    fn label(&self) -> &str {
        &self.caps.label
    }

    fn spot_call(&self, endpoint: &'static str, method: Method, path: &str, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, REST, path, corr).signed();
        h.method = method;
        h
    }

    fn fut_call(&self, endpoint: &'static str, method: Method, path: String, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, FUT, path, corr).signed();
        h.method = method;
        h
    }

    /// A spot write with its parameters in the body, as the raw `key=value&...` string, under
    /// `Content-Type: application/json` ("do not use `application/x-www-form-urlencoded`").
    fn spot_body(mut h: HttpRequest, params: &[(String, String)]) -> HttpRequest {
        h.body = Some(encode_query(params).into_bytes().into());
        h.headers.push(("Content-Type".into(), "application/json".into()));
        h
    }

    /// A futures write: the JSON body, signed as sent (the signer writes the one Content-Type).
    fn fut_body(mut h: HttpRequest, body: Vec<u8>) -> HttpRequest {
        h.body = Some(body.into());
        h
    }

    /// The spot `type`, which carries the time in force.
    fn spot_type(&self, o: &OrderRequest) -> Result<&'static str, ConnectorError> {
        match (o.order_type, o.tif, o.opts.post_only) {
            (OrderType::Limit, TimeInForce::Gtc, true) => Ok("LIMIT_MAKER"),
            (OrderType::Limit, TimeInForce::Gtc, false) => Ok("LIMIT"),
            (OrderType::Limit, TimeInForce::Ioc, false) => Ok("IMMEDIATE_OR_CANCEL"),
            (OrderType::Limit, TimeInForce::Fok, false) => Ok("FILL_OR_KILL"),
            (OrderType::Market, TimeInForce::Gtc | TimeInForce::Ioc, false) => Ok("MARKET"),
            _ => Err(self.unsupported_combo(o)),
        }
    }

    /// The futures `type`: "1: limit,2: Post Only (maker only),3: IOC,4: FOK,5: market".
    fn fut_type(&self, o: &OrderRequest) -> Result<i64, ConnectorError> {
        match (o.order_type, o.tif, o.opts.post_only) {
            (OrderType::Limit, TimeInForce::Gtc, true) => Ok(2),
            (OrderType::Limit, TimeInForce::Gtc, false) => Ok(1),
            (OrderType::Limit, TimeInForce::Ioc, false) => Ok(3),
            (OrderType::Limit, TimeInForce::Fok, false) => Ok(4),
            (OrderType::Market, TimeInForce::Gtc | TimeInForce::Ioc, false) => Ok(5),
            _ => Err(self.unsupported_combo(o)),
        }
    }

    fn unsupported_combo(&self, o: &OrderRequest) -> ConnectorError {
        let what = match (o.order_type, o.opts.post_only) {
            (OrderType::Market, true) => "a post-only market order".to_string(),
            (OrderType::Market, false) => format!("a market order {:?}", o.tif.kind()),
            (OrderType::Limit, true) => format!("a post-only limit order {:?} (post-only rests, good till cancelled)", o.tif.kind()),
            (t, _) => format!("a {t:?} order {:?}", o.tif.kind()),
        };
        ConnectorError::unsupported(format!("{} takes no {what}: a market order works at once, a limit order GTC, IOC, FOK or post-only", self.label()))
    }

    fn spot_params(&self, o: &OrderRequest, p: &Placing) -> Result<Vec<(String, String)>, ConnectorError> {
        if o.opts.reduce_only {
            return Err(ConnectorError::unsupported(format!("{} takes no reduce-only order: a spot sale of what is held is a plain sell", self.label())));
        }
        let kind = self.spot_type(o)?;
        let mut q = vec![
            ("symbol".to_string(), o.instrument.native.clone()),
            ("side".to_string(), if o.side == Side::Buy { "BUY" } else { "SELL" }.to_string()),
            ("type".to_string(), kind.to_string()),
        ];
        if o.order_type == OrderType::Market && o.side == Side::Buy {
            // "MARKET orders using quoteOrderQty specifies the amount the user wants to spend".
            let amount = p.quote_amount.ok_or_else(|| ConnectorError::invalid(format!("{} takes a market buy as an amount to spend, and none was set: nothing was sent", self.label())))?;
            q.push(("quoteOrderQty".into(), dec_text(amount)));
        } else {
            q.push(("quantity".into(), dec_text(o.qty.value())));
        }
        if o.order_type == OrderType::Limit {
            let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
            q.push(("price".into(), dec_text(px.value())));
        }
        if !p.client_id.is_empty() {
            q.push(("newClientOrderId".into(), p.client_id.clone()));
        }
        Ok(q)
    }

    fn fut_order_body(&self, o: &OrderRequest, p: &Placing) -> Result<Vec<u8>, ConnectorError> {
        let sym = &o.instrument.native;
        // Hedge mode only: `positionMode` "2: One-way Mode" and `reduceOnly` "only applicable in
        // one-way mode" are stated, but no page says which `side` code a one-way order (a close
        // included) takes, so none is sent.
        let side = fut_side_code(p.position, o.side).ok_or_else(|| {
            ConnectorError::unsupported(format!(
                "{}: this account holds positions in one-way mode, and MEXC's documentation does not say which order side a one-way order takes: OpenTraderWorld sends futures orders in hedge mode only, a close included, so nothing was sent. Switch the futures account to hedge mode at MEXC to trade from here",
                self.label()
            ))
        })?;
        let margin = p.margin.as_ref().ok_or_else(|| {
            ConnectorError::invalid(format!("{}: the margin mode (isolated or cross) held on {sym} was not read, so nothing was sent", self.label()))
        })?;
        let mut b = Body(Vec::new());
        b.text("symbol", sym);
        // A market order's `price` is marked mandatory with no meaning given: none is sent.
        if o.order_type == OrderType::Limit {
            let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
            b.number("price", px.value());
        }
        b.number("vol", p.contracts(self.label(), sym, o.qty.value())?);
        if let Some(l) = p.leverage {
            // "Leverage, must be provided when opening a position": the one the venue holds.
            b.number("leverage", l);
        }
        b.int("side", side);
        b.int("type", self.fut_type(o)?);
        b.int("openType", if margin.kind == otw_connector_sdk::trade::MarginKind::Isolated { 1 } else { 2 });
        if !p.client_id.is_empty() {
            b.text("externalOid", &p.client_id);
        }
        // "1: Hedge Mode; 2: One-way Mode": sent, never left to the venue's default.
        b.int("positionMode", 1);
        Ok(b.bytes())
    }

    fn spot_row(&self, r: SpotOrder) -> VenueOrder {
        let filled = r.executed_qty.0.unwrap_or_default();
        let mut status = self.status_of(&r.status);
        if status == Mapped::Known(OrderStatus::New) && filled > Decimal::ZERO {
            status = Mapped::Known(OrderStatus::PartiallyFilled);
        }
        let orig = r.orig_qty.0.unwrap_or_default();
        // A market buy sized by `quoteOrderQty` states no base size: its size is what filled.
        let qty = if orig.is_zero() && r.kind == "MARKET" { filled } else { orig };
        let avg = r.cummulative_quote_qty.0.filter(|_| filled > Decimal::ZERO).and_then(|q| q.checked_div(filled)).filter(|p| *p > Decimal::ZERO);
        let limit = if r.kind == "MARKET" { None } else { positive(r.price) };
        let client = r.client_order_id.filter(|c| !c.is_empty()).or(r.orig_client_order_id.filter(|c| !c.is_empty()));
        VenueOrder {
            venue_id: r.order_id,
            client_id: client,
            symbol: r.symbol.to_ascii_uppercase(),
            side: match r.side.as_str() {
                "BUY" => Mapped::Known(Side::Buy),
                "SELL" => Mapped::Known(Side::Sell),
                other => Mapped::Unknown(other.to_string()),
            },
            order_type: r.kind.to_ascii_lowercase(),
            status,
            qty,
            filled_qty: filled,
            limit,
            stop: positive(r.stop_price),
            avg_price: avg,
            reduce_only: false,
            position: PositionSide::Net,
            created_ms: r.time.map(|t| t.0),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }
    }

    fn fut_row(&self, sizes: &ContractSizes, r: FutOrder) -> Result<VenueOrder, ConnectorError> {
        let sym = r.symbol.to_ascii_uppercase();
        let (side, position, closing) = fut_side_in(r.side.map(|s| s.0).unwrap_or_default(), r.position_mode.map(|m| m.0));
        let total = r.vol.0.unwrap_or_default().abs();
        let dealt = r.deal_vol.0.unwrap_or_default().abs();
        let state = r.state.map(|s| s.0.to_string()).unwrap_or_default();
        let mut status = self.status_of(&state);
        if status == Mapped::Known(OrderStatus::New) && dealt > Decimal::ZERO {
            status = Mapped::Known(OrderStatus::PartiallyFilled);
        }
        let kind = r.order_type.map(|t| t.0).unwrap_or_default();
        Ok(VenueOrder {
            venue_id: r.order_id,
            client_id: r.external_oid.filter(|c| !c.is_empty()),
            side,
            order_type: fut_type_word(kind),
            status,
            qty: sizes.to_base(self.label(), &sym, total)?,
            filled_qty: sizes.to_base(self.label(), &sym, dealt)?,
            limit: if matches!(kind, 1..=4) { positive(r.price) } else { None },
            stop: None,
            avg_price: positive(r.deal_avg_price),
            reduce_only: closing || r.reduce_only == Some(true),
            position,
            created_ms: r.create_time.map(|t| t.0),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            symbol: sym,
            successor_id: None,
        })
    }

    /// A conditional order placed elsewhere, under `cond:<kind>:<id>`, in the base coin.
    fn cond_row(&self, sizes: &ContractSizes, kind: CondKind, r: CondRow) -> Result<VenueOrder, ConnectorError> {
        let sym = r.symbol.to_ascii_uppercase();
        let (side, position, closing) = r.direction();
        let (word, limit, stop) = r.shape(kind);
        let status = r.status(kind);
        Ok(VenueOrder {
            venue_id: kind.tag(&r.id),
            client_id: None,
            side,
            order_type: word.to_string(),
            status,
            qty: sizes.to_base(self.label(), &sym, r.vol.0.unwrap_or_default().abs())?,
            filled_qty: Decimal::ZERO,
            limit,
            stop,
            avg_price: None,
            reduce_only: closing || r.reduce_only == Some(true),
            position,
            created_ms: r.create_time.map(|t| t.0),
            trigger_pending: true,
            parent_client_id: None,
            exit: None,
            symbol: sym,
            successor_id: None,
        })
    }

    /// A futures fill. `fee`: "positive = paid by user, negative = received by user", a cost
    /// as OTW writes it. A forced liquidation custody (`category` 2) or an ADL reduction (4)
    /// moved a position with no order of the account behind it.
    fn fut_fill(&self, sizes: &ContractSizes, r: FutDeal) -> Result<VenueFill, ConnectorError> {
        let sym = r.symbol.to_ascii_uppercase();
        let (side, _, _) = fut_side_of(r.side.map(|s| s.0).unwrap_or_default());
        Ok(VenueFill {
            exec_id: r.id,
            venue_order_id: r.order_id,
            client_id: r.external_oid.filter(|c| !c.is_empty()),
            side,
            qty: sizes.to_base(self.label(), &sym, r.vol.0.unwrap_or_default().abs())?,
            price: r.price.0.unwrap_or_default(),
            fee: r.fee.0.unwrap_or_default(),
            fee_currency: r.fee_currency.unwrap_or_default().to_ascii_uppercase(),
            at_ms: r.timestamp.map(|t| t.0).unwrap_or_default(),
            position_only: matches!(r.category.map(|c| c.0), Some(2 | 4)),
            extra_fees: Vec::new(),
            fee_pending: false,
            symbol: sym,
        })
    }

    /// A position, sided by `positionType` (MEXC sides every row, in either mode). The leverage
    /// is not kept from the row: the trader holds one per instrument, and MEXC holds one per
    /// side; it is read from `position/leverage` instead ([`TradeMapper::decode_symbol_leverage`]).
    fn fut_position(&self, sizes: &ContractSizes, r: FutPosition) -> Result<Option<VenuePosition>, ConnectorError> {
        let sym = r.symbol.to_ascii_uppercase();
        let (side, direction) = match r.position_type.map(|t| t.0) {
            Some(1) => (PositionSide::Long, Side::Buy),
            Some(2) => (PositionSide::Short, Side::Sell),
            _ => return Ok(None),
        };
        Ok(Some(VenuePosition {
            side,
            direction: Some(direction),
            qty: sizes.to_base(self.label(), &sym, r.hold_vol.0.unwrap_or_default().abs())?,
            leverage: None,
            margin: margin_of(r.open_type.map(|t| t.0)),
            symbol: sym,
        }))
    }

    fn spot_rules(&self, r: PairRow) -> VenueRules {
        let step = |p: Option<Int>| p.and_then(|p| u32::try_from(p.0).ok()).filter(|p| *p <= 28).map(|p| Decimal::new(1, p));
        let mut v = VenueRules::new(InstrumentId::new(self.book.venue(), &r.symbol.to_ascii_uppercase()));
        v.tick = step(r.quote_precision);
        v.lot_step = step(r.base_asset_precision);
        v.quote_step = step(r.quote_asset_precision);
        v.min_qty = positive(r.base_size_precision);
        v.min_notional = positive(r.quote_amount_precision);
        v.max_notional = positive(r.max_quote_amount);
        v.status = match (r.status.as_str(), r.is_spot_trading_allowed, r.trade_side_type.as_str()) {
            (s, _, _) if !matches!(s, "1" | "ENABLED") => TradingStatus::Closed(match s {
                "2" => "2 (Pause)".into(),
                "3" => "3 (offline)".into(),
                "" => "unstated".into(),
                other => other.to_string(),
            }),
            (_, Some(false), _) => TradingStatus::Closed("isSpotTradingAllowed false".into()),
            (_, _, "2") => TradingStatus::BuyOnly("tradeSideType 2 (buy order only)".into()),
            (_, _, "3") => TradingStatus::SellOnly("tradeSideType 3 (sell order only)".into()),
            (_, _, "4") => TradingStatus::Closed("tradeSideType 4 (Close)".into()),
            _ => TradingStatus::Trading,
        };
        v
    }

    /// A contract's rules in the base coin, or `None` for one this connector does not serve
    /// (delivery, coin-settled). `type` is not read: the live list writes 2 ("suspended") on
    /// hundreds of contracts that trade.
    fn fut_rules(&self, r: ContractRules) -> Option<VenueRules> {
        if r.future_type.is_some_and(|t| t.0 != 1) || r.settle_coin != r.quote_coin || r.symbol.is_empty() {
            return None;
        }
        let cs = positive(r.contract_size)?;
        let base = |d: OptDec| positive(d).map(|n| n * cs);
        let mut v = VenueRules::new(InstrumentId::new(self.book.venue(), &r.symbol.to_ascii_uppercase()));
        v.contract_size = Some(cs);
        v.tick = positive(r.price_unit);
        v.lot_step = base(r.vol_unit).or(Some(cs));
        v.min_qty = base(r.min_vol);
        // "Max contracts per limit order", and "Maximum order size (contracts)" for the rest.
        v.max_qty = base(r.limit_max_vol).or(base(r.max_vol));
        v.market_max_qty = base(r.max_vol);
        v.status = match (r.state.map(|s| s.0), r.api_allowed) {
            (_, Some(false)) => TradingStatus::Closed("apiAllowed false (no API trading)".into()),
            (Some(0), _) => TradingStatus::Trading,
            (Some(1), _) => TradingStatus::Closed("1 (delivery)".into()),
            (Some(2), _) => TradingStatus::Closed("2 (delivered)".into()),
            (Some(3), _) => TradingStatus::Closed("3 (offline)".into()),
            (Some(4), _) => TradingStatus::Closed("4 (paused)".into()),
            (Some(other), _) => TradingStatus::Closed(other.to_string()),
            (None, _) => TradingStatus::Closed("unstated".into()),
        };
        Some(v)
    }

    /// A spot private frame: protobuf for the channels, JSON text for the answers.
    fn spot_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        if payload.trim_ascii_start().starts_with(b"{") {
            // `{"id":0,"code":0,"msg":"spot@private.orders.v3.api.pb"}`, `{"id":0,"code":0,"msg":"PONG"}`.
            let v: Value = serde_json::from_slice(payload).map_err(|e| decode_err("private frame", e))?;
            let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
            let msg = v.get("msg").and_then(Value::as_str).unwrap_or_default().to_string();
            if msg.eq_ignore_ascii_case("PONG") {
                return Ok(vec![PrivateEvent::Answer { op: "pong".into(), ok: true, message: String::new() }]);
            }
            return Ok(vec![PrivateEvent::Answer { op: "subscribe".into(), ok: code == 0, message: msg }]);
        }
        let bad = || ConnectorError::decode("mexc private frame: not a protobuf message");
        let w = proto::wrapper(payload).map_err(|_| bad())?;
        let symbol = Some(w.symbol.to_ascii_uppercase()).filter(|s| !s.is_empty());
        if let Some(b) = w.private_orders {
            // The frame's order id against the REST id: the doc's examples write them apart
            // (`C02__...` on the stream), so the order is read again by its id; one the REST call
            // does not know reads the whole book. A frame without the id (field 1) is read again
            // by its `clientId` (field 2); with neither, it is not read and the book is.
            let o = proto::private_order(b).map_err(|_| bad())?;
            if !o.id.is_empty() {
                return Ok(vec![PrivateEvent::OrderChanged { symbol, venue_id: o.id.to_string() }]);
            }
            if !o.client_id.is_empty() {
                return Ok(vec![PrivateEvent::OrderChangedByClient { symbol, client_id: o.client_id.to_string() }]);
            }
            return Err(ConnectorError::decode("mexc private order: neither an id nor a clientId"));
        }
        if let Some(b) = w.private_deals {
            let d = proto::private_deal(b).map_err(|_| bad())?;
            let dec = |s: &str| Decimal::from_str(s).map_err(|e| ConnectorError::decode(format!("mexc private deal {s:?}: {e}")));
            return Ok(vec![PrivateEvent::Fill(VenueFill {
                exec_id: d.trade_id.to_string(),
                venue_order_id: d.order_id.to_string(),
                client_id: Some(d.client_order_id.to_string()).filter(|c| !c.is_empty()),
                symbol: symbol.unwrap_or_default(),
                side: match d.trade_type {
                    1 => Mapped::Known(Side::Buy),
                    2 => Mapped::Known(Side::Sell),
                    other => Mapped::Unknown(other.to_string()),
                },
                qty: dec(d.quantity)?,
                price: dec(d.price)?,
                fee: if d.fee_amount.is_empty() { Decimal::ZERO } else { dec(d.fee_amount)? },
                fee_currency: d.fee_currency.to_ascii_uppercase(),
                at_ms: d.time,
                position_only: false,
                extra_fees: Vec::new(),
                fee_pending: false,
            })]);
        }
        Ok(Vec::new())
    }

    /// A futures private frame: the login's answer, the pong, and the order, fill, position and
    /// position-mode channels (every other channel pushed after the login is left out).
    fn fut_private(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        #[derive(Deserialize)]
        struct Push {
            #[serde(default)]
            channel: String,
            #[serde(default)]
            data: Value,
        }
        let p: Push = serde_json::from_slice(payload).map_err(|e| decode_err("private frame", e))?;
        let text = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        Ok(match p.channel.as_str() {
            "rs.login" => vec![PrivateEvent::Answer { op: "auth".into(), ok: p.data.as_str() == Some("success"), message: text(&p.data) }],
            // "Failure: return error info, channel = rs.error".
            "rs.error" => vec![PrivateEvent::Answer { op: "auth".into(), ok: false, message: text(&p.data) }],
            "pong" => vec![PrivateEvent::Answer { op: "pong".into(), ok: true, message: String::new() }],
            "push.personal.order" => vec![PrivateEvent::Order(self.fut_row(ctx.sizes, serde_json::from_value(p.data).map_err(|e| decode_err("push.personal.order", e))?)?)],
            "push.personal.order.deal" => vec![PrivateEvent::Fill(self.fut_fill(ctx.sizes, serde_json::from_value(p.data).map_err(|e| decode_err("push.personal.order.deal", e))?)?)],
            "push.personal.position" => {
                let row: FutPosition = serde_json::from_value(p.data).map_err(|e| decode_err("push.personal.position", e))?;
                self.fut_position(ctx.sizes, row)?.map(PrivateEvent::Position).into_iter().collect()
            }
            "push.personal.position.mode" => {
                let mode = match p.data.get("positionMode").and_then(Value::as_i64) {
                    Some(1) => Some(PositionMode::Hedge),
                    Some(2) => Some(PositionMode::OneWay),
                    _ => None,
                };
                vec![PrivateEvent::PositionMode { symbol: None, mode }]
            }
            _ => Vec::new(),
        })
    }
}

impl TradeMapper for Mexc {
    /// `externalOid`: "External order ID too long (max. 32 digit)"; `newClientOrderId` states no
    /// rule, held to the same. Letters, digits, `-` and `_` (both in the doc's examples).
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(32)
    }

    fn place(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if self.is_futures() {
            let body = self.fut_order_body(o, p)?;
            return Ok(Self::fut_body(self.fut_call("fut_create", Method::Post, "/api/v1/private/order/create".into(), p.corr), body));
        }
        let params = self.spot_params(o, p)?;
        Ok(Self::spot_body(self.spot_call("spot_order", Method::Post, "/api/v3/order", p.corr), &params))
    }

    /// Spot `POST /api/v3/order/test` ("Creates and validates a new order but does not send it
    /// into the matching engine"), the same parameters. A market buy not yet valued (no amount
    /// to spend) is not covered. Futures: none.
    fn dry_run(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.is_futures() || (o.order_type == OrderType::Market && o.side == Side::Buy && p.quote_amount.is_none()) {
            return Ok(None);
        }
        let params = self.spot_params(o, p)?;
        Ok(Some(Self::spot_body(self.spot_call("spot_order_test", Method::Post, "/api/v3/order/test", p.corr), &params)))
    }

    fn decode_dry_run(&self, body: &[u8]) -> Result<String, ConnectorError> {
        serde_json::from_slice::<Value>(body).map_err(|e| decode_err("order test", e))?;
        Ok("accepted by MEXC's order test".into())
    }

    /// Spot `DELETE /api/v3/order` (symbol, `orderId`); futures `POST order/cancel` with the id
    /// as the one item of the list ("orderIds", `List<Long>`; the body is the bare list, as the
    /// mirror's table writes it).
    fn cancel(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if let Some((kind, id)) = CondKind::parse(venue_id).filter(|_| self.is_futures()) {
            let id: u64 = id.parse().map_err(|_| ConnectorError::invalid(format!("{}: {venue_id:?} is not a futures conditional order id", self.label())))?;
            // Plan: a list of `{symbol, orderId}`; stop: a list of `{stopPlanOrderId}`; trailing:
            // `{symbol, trackOrderId}`.
            let (endpoint, path, body) = match kind {
                CondKind::Plan => ("fut_plan_cancel", "planorder/cancel", serde_json::json!([{ "symbol": symbol, "orderId": id.to_string() }])),
                CondKind::Stop => ("fut_stop_cancel", "stoporder/cancel", serde_json::json!([{ "stopPlanOrderId": id }])),
                CondKind::Track => ("fut_track_cancel", "trackorder/cancel", serde_json::json!({ "symbol": symbol, "trackOrderId": id })),
            };
            return Ok(Self::fut_body(self.fut_call(endpoint, Method::Post, format!("/api/v1/private/{path}"), corr), body.to_string().into_bytes()));
        }
        if self.is_futures() {
            let id: u64 = venue_id.parse().map_err(|_| ConnectorError::invalid(format!("{}: {venue_id:?} is not a futures order id", self.label())))?;
            return Ok(Self::fut_body(self.fut_call(FUT_CANCEL, Method::Post, "/api/v1/private/order/cancel".into(), corr), format!("[{id}]").into_bytes()));
        }
        // Counted with the place call: "Shared with the order endpoint, 12 requests/second".
        Ok(self.spot_call("spot_order:cancel", Method::Delete, "/api/v3/order", corr).query("symbol", symbol).query("orderId", venue_id))
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::BySymbol
    }

    /// One call per symbol with working orders: spot `DELETE /api/v3/openOrders` ("Cancel all
    /// pending orders for a single symbol"), futures `POST order/cancel_all` ("it will cancel
    /// orders under that specific contract only").
    ///
    /// Futures conditional orders placed elsewhere go too: `planorder/cancel_all` and
    /// `stoporder/cancel_all` per contract (`symbol`); trailing orders have no cancel all, so
    /// the trader cancels what is left one by one.
    fn cancel_all(&self, _settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut out = Vec::new();
        for s in symbols {
            if self.is_futures() {
                for (endpoint, path) in [("fut_cancel_all", "order/cancel_all"), ("fut_plan_cancel_all", "planorder/cancel_all"), ("fut_stop_cancel_all", "stoporder/cancel_all")] {
                    let mut b = Body(Vec::new());
                    b.text("symbol", s);
                    out.push(Self::fut_body(self.fut_call(endpoint, Method::Post, format!("/api/v1/private/{path}"), corr), b.bytes()));
                }
            } else {
                out.push(self.spot_call("spot_cancel_all", Method::Delete, "/api/v3/openOrders", corr).query("symbol", s));
            }
        }
        Ok(out)
    }

    /// Spot `GET /api/v3/order` by `origClientOrderId` ("up to 7 days"); futures `GET
    /// order/external/{symbol}/{external_oid}`.
    fn query_by_client_id(&self, _settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            return Ok(vec![self.fut_call("fut_order_external", Method::Get, format!("/api/v1/private/order/external/{symbol}/{client_id}"), corr)]);
        }
        Ok(vec![self.spot_call("spot_query", Method::Get, "/api/v3/order", corr).query("symbol", symbol).query("origClientOrderId", client_id)])
    }

    fn query_by_venue_id(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        // A conditional order has no read by id: its kind's lists, every state.
        if let Some((kind, _)) = CondKind::parse(venue_id).filter(|_| self.is_futures()) {
            return Ok(kind.lookup_requests(corr));
        }
        if self.is_futures() {
            return Ok(vec![self.fut_call("fut_order", Method::Get, format!("/api/v1/private/order/get/{venue_id}"), corr)]);
        }
        Ok(vec![self.spot_call("spot_query", Method::Get, "/api/v3/order", corr).query("symbol", symbol).query("orderId", venue_id)])
    }

    /// Spot `GET /api/v3/openOrders` (every pair, or one); futures `GET order/list/open_orders`
    /// by page (no symbol parameter: every contract), then the working plan, stop and trailing
    /// orders ([`CondList`]). One of those lists refused leaves the others read
    /// ([`super::trade_errors::decode`]).
    fn open_orders(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            let mut out = vec![self.fut_call("fut_open_orders", Method::Get, "/api/v1/private/order/list/open_orders".into(), corr).query("page_num", 1).query("page_size", FUT_PAGE)];
            // The conditional orders placed elsewhere, each kind from its own list.
            out.extend(CondList::OPEN.into_iter().map(|l| l.request(1, corr)));
            return Ok(out);
        }
        let mut h = self.spot_call("spot_open_orders", Method::Get, "/api/v3/openOrders", corr);
        if let Some(s) = symbol {
            h = h.query("symbol", s);
        }
        Ok(vec![h])
    }

    fn fills_since(&self, _settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if self.is_futures() {
            return Ok(self
                .fut_call("fut_deals", Method::Get, "/api/v1/private/order/list/order_deals/v3".into(), corr)
                .query("symbol", symbol)
                .query("start_time", from_ms)
                .query("end_time", to_ms)
                .query("page_num", 1)
                .query("page_size", FUT_PAGE));
        }
        Ok(self
            .spot_call("spot_my_trades", Method::Get, "/api/v3/myTrades", corr)
            .query("symbol", symbol)
            .query("startTime", from_ms)
            .query("endTime", to_ms)
            .query("limit", SPOT_FILL_PAGE))
    }

    fn fills_window_ms(&self) -> Option<i64> {
        Some(if self.is_futures() { FUT_FILLS_WINDOW_MS } else { SPOT_FILLS_WINDOW_MS })
    }

    /// Spot `GET /api/v3/exchangeInfo`; futures `GET /api/v1/contract/detail/country` (public).
    fn instrument_rules(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let h = if self.is_futures() { HttpRequest::get("contracts:rules", FUT, CONTRACTS_PATH, corr) } else { HttpRequest::get("spot_exchange_info", REST, "/api/v3/exchangeInfo", corr) };
        Ok(match symbol {
            Some(s) => h.query("symbol", s),
            None => h,
        })
    }

    /// A market order works at once: asked FOK, it is a limit order's job. Post-only rests.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        if self.is_futures() {
            self.fut_type(o).map(|_| ())
        } else {
            self.spot_type(o).map(|_| ())
        }
    }

    /// The next page: `page_num` moves by one.
    fn with_cursor(&self, req: &HttpRequest, _cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        let page = req.query.iter().find(|(k, _)| k == "page_num").and_then(|(_, v)| v.parse::<i64>().ok()).unwrap_or(1);
        next.query.retain(|(k, _)| k != "page_num");
        next.query.push(("page_num".into(), (page + 1).to_string()));
        next
    }

    fn positions(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.fut_call("fut_positions", Method::Get, "/api/v1/private/position/open_positions".into(), corr).query("symbol", symbol)))
    }

    fn all_positions(&self, _settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(Vec::new());
        }
        Ok(vec![self.fut_call("fut_positions", Method::Get, "/api/v1/private/position/open_positions".into(), corr)])
    }

    /// 7002 "Position mode mismatch" and 2043 "The order does not match the position": the
    /// account switched mode.
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.is_futures() && matches!(e.code.as_deref(), Some("7002" | "2043"))
    }

    /// `GET position/position_mode`: "1 Hedge Mode; 2 One-way Mode", account wide. Position rows
    /// never say it (every row is sided by `positionType`).
    fn account_position_mode(&self, _settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.fut_call("fut_position_mode", Method::Get, "/api/v1/private/position/position_mode".into(), corr)))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        Ok(match fut_data("position mode", body)?.as_i64() {
            Some(1) => Some(PositionMode::Hedge),
            Some(2) => Some(PositionMode::OneWay),
            _ => None,
        })
    }

    /// `GET position/leverage`: one row per side, each with its `openType`.
    fn symbol_margin(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.fut_call("fut_leverage", Method::Get, "/api/v1/private/position/leverage".into(), corr).query("symbol", symbol)))
    }

    fn margin_per_side(&self) -> bool {
        self.is_futures()
    }

    fn decode_symbol_margin(&self, symbol: &str, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        self.decode_symbol_margin_on(symbol, PositionSide::Net, body)
    }

    /// The side's own row (`positionType` 1 long, 2 short); for no side, the margin both rows
    /// agree on.
    fn decode_symbol_margin_on(&self, _symbol: &str, side: PositionSide, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        let rows: Vec<LeverageRow> = rows_of("position leverage", fut_data("position leverage", body)?)?;
        let of = |t: i64| rows.iter().find(|r| r.position_type.map(|p| p.0) == Some(t)).and_then(|r| margin_of(r.open_type.map(|o| o.0)));
        Ok(match side {
            PositionSide::Long => of(1),
            PositionSide::Short => of(2),
            PositionSide::Net => {
                let all: Vec<Option<MarginMode>> = rows.iter().map(|r| margin_of(r.open_type.map(|o| o.0))).collect();
                all.first().cloned().flatten().filter(|m| all.iter().all(|x| x.as_ref() == Some(m)))
            }
        })
    }

    /// 2027 (cross and isolated of one side), 7003, 6007, 9070, 9071: the margin held is out of
    /// date.
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.is_futures() && matches!(e.code.as_deref(), Some("2027" | "7003" | "6007" | "9070" | "9071"))
    }

    /// 2021 "Order leverage is inconsistent with the existing position leverage", 7004 "The new
    /// order leverage must be consistent with the existing order or the leverage of the position".
    fn invalidates_leverage(&self, e: &VenueError) -> bool {
        self.is_futures() && matches!(e.code.as_deref(), Some("2021" | "7004"))
    }

    fn symbol_leverage(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.symbol_margin(settings, symbol, corr)
    }

    /// The leverage both sides hold: the trader keeps one per instrument, MEXC one per side, so
    /// a value is answered only when the two agree (the side's own would be sent on the other
    /// side's orders). Otherwise none, and the venue answers.
    fn decode_symbol_leverage(&self, _symbol: &str, _side: PositionSide, body: &[u8]) -> Result<Option<Decimal>, ConnectorError> {
        let rows: Vec<LeverageRow> = rows_of("position leverage", fut_data("position leverage", body)?)?;
        let levels: Vec<Decimal> = rows.iter().filter_map(|r| positive(OptDec(r.leverage.0))).collect();
        Ok(levels.first().copied().filter(|l| !levels.is_empty() && levels.iter().all(|x| x == l)))
    }

    fn counts_contracts(&self) -> bool {
        self.is_futures()
    }

    fn market_buy_in_quote(&self, o: &OrderRequest) -> bool {
        !self.is_futures() && o.order_type == OrderType::Market && o.side == Side::Buy
    }

    /// The pair's best bid and ask (`GET /api/v3/ticker/bookTicker`, public): a buy is valued at
    /// the ask.
    fn reference_price(&self, _settings: &Settings, symbol: &str, _side: Side, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            return Ok(None);
        }
        Ok(Some(HttpRequest::get("spot_book_ticker", REST, "/api/v3/ticker/bookTicker", corr).query("symbol", symbol)))
    }

    fn decode_reference_price(&self, body: &[u8], side: Side) -> Result<Decimal, ConnectorError> {
        let t: BookTicker = serde_json::from_slice(body).map_err(|e| decode_err("bookTicker", e))?;
        let px = match side {
            Side::Buy => t.ask_price,
            Side::Sell => t.bid_price,
        };
        positive(px).ok_or_else(|| ConnectorError::decode("mexc bookTicker: no price on that side"))
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(!self.is_futures(), frame)
    }

    /// Spot: the order row (`orderId`; the client id is not echoed). Futures: `data` with
    /// `orderId` (or the bare id, as the mirror writes it); none (a 6008 "Order placed
    /// successfully") is an acceptance without the id, found by `externalOid`.
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        if self.is_futures() {
            let id = match fut_data("order ack", body)? {
                Value::Object(o) => o.get("orderId").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).filter(|s| s != "null").unwrap_or_default(),
                Value::Number(n) => n.to_string(),
                Value::String(s) => s,
                _ => String::new(),
            };
            return Ok(if id.is_empty() { Ack::without_id(None) } else { Ack { venue_id: id, client_id: None } });
        }
        let r: SpotOrder = serde_json::from_slice(body).map_err(|e| decode_err("order ack", e))?;
        if r.order_id.is_empty() {
            return Err(ConnectorError::decode("mexc order ack: no orderId"));
        }
        Ok(Ack { venue_id: r.order_id, client_id: None })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_orders_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        if let Some(kind) = CondKind::of_endpoint(ctx.endpoint).filter(|_| self.is_futures()) {
            let rows = cond_rows(fut_data("conditional orders", body)?)?;
            let n = rows.len();
            let out = rows.into_iter().map(|r| self.cond_row(ctx.sizes, kind, r)).collect::<Result<Vec<_>, _>>()?;
            // Only the plan list of working ones pages (`page_num`): the stop list of working ones
            // has no page, the trailing list pages by `pageIndex`, which the cursor does not move,
            // and a lookup reads its first page.
            return Ok((out, (ctx.endpoint == PLAN_OPEN && n >= FUT_PAGE).then(|| "next".to_string())));
        }
        if self.is_futures() {
            let rows: Vec<FutOrder> = rows_of("orders", fut_data("orders", body)?)?;
            let n = rows.len();
            let out = rows.into_iter().map(|r| self.fut_row(ctx.sizes, r)).collect::<Result<Vec<_>, _>>()?;
            let paged = ctx.endpoint == "fut_open_orders" && n >= FUT_PAGE;
            return Ok((out, paged.then(|| "next".to_string())));
        }
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
        let rows: Vec<SpotOrder> = rows_of("orders", v)?;
        Ok((rows.into_iter().map(|r| self.spot_row(r)).collect(), None))
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.decode_fills_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    /// Futures: paged while a page comes back full. Spot: `myTrades` pages no further than 100
    /// rows, so a full page is said rather than cut.
    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        if self.is_futures() {
            let rows = deals_of(fut_data("order deals", body)?)?;
            let n = rows.len();
            let out = rows.into_iter().map(|r| self.fut_fill(ctx.sizes, r)).collect::<Result<Vec<_>, _>>()?;
            return Ok((out, (n >= FUT_PAGE).then(|| "next".to_string())));
        }
        let rows: Vec<SpotTrade> = serde_json::from_slice(body).map_err(|e| decode_err("myTrades", e))?;
        if rows.len() >= SPOT_FILL_PAGE {
            let sym = rows.first().map(|r| r.symbol.clone()).unwrap_or_default();
            return Err(ConnectorError::invalid(format!(
                "MEXC answers {SPOT_FILL_PAGE} spot fills per call with no further page, and an hour of {sym} holds that many: its fills cannot all be read over the API"
            )));
        }
        Ok((
            rows.into_iter()
                .map(|r| VenueFill {
                    exec_id: r.id,
                    position_only: r.order_id.is_empty(),
                    venue_order_id: r.order_id,
                    client_id: r.client_order_id.filter(|c| !c.is_empty()),
                    symbol: r.symbol.to_ascii_uppercase(),
                    side: Mapped::Known(if r.is_buyer { Side::Buy } else { Side::Sell }),
                    qty: r.qty.0.unwrap_or_default(),
                    price: r.price.0.unwrap_or_default(),
                    // The sign is not stated: a commission is read as a cost.
                    fee: r.commission.0.unwrap_or_default(),
                    fee_currency: r.commission_asset.unwrap_or_default().to_ascii_uppercase(),
                    at_ms: r.time.map(|t| t.0).unwrap_or_default(),
                    extra_fees: Vec::new(),
                    fee_pending: false,
                })
                .collect(),
            None,
        ))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        if self.is_futures() {
            let rows: Vec<ContractRules> = rows_of("contract detail", fut_data("contract detail", body)?)?;
            return Ok((rows.into_iter().filter_map(|r| self.fut_rules(r)).collect(), None));
        }
        #[derive(Deserialize)]
        struct Info {
            #[serde(default)]
            symbols: Vec<PairRow>,
        }
        let info: Info = serde_json::from_slice(body).map_err(|e| decode_err("exchangeInfo", e))?;
        Ok((info.symbols.into_iter().filter(|r| !r.symbol.is_empty()).map(|r| self.spot_rules(r)).collect(), None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.decode_positions_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let rows: Vec<FutPosition> = rows_of("open positions", fut_data("open positions", body)?)?;
        let mut out = Vec::new();
        for r in rows {
            out.extend(self.fut_position(ctx.sizes, r)?);
        }
        Ok(out)
    }

    /// Spot `status`: `NEW`, `PARTIALLY_FILLED`, `FILLED`, `CANCELED`, `PARTIALLY_CANCELED`
    /// (cancelled after a partial fill). Futures `state`: "1 pending, 2 unfilled, 3 filled, 4
    /// canceled, 5 invalid" (2 with a fill is made PartiallyFilled by the row).
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match (self.is_futures(), word) {
            (false, "NEW") | (true, "1" | "2") => OrderStatus::New,
            (false, "PARTIALLY_FILLED") => OrderStatus::PartiallyFilled,
            (false, "FILLED") | (true, "3") => OrderStatus::Filled,
            (false, "CANCELED" | "PARTIALLY_CANCELED") | (true, "4") => OrderStatus::Cancelled,
            (true, "5") => OrderStatus::Rejected,
            (_, other) => return Mapped::Unknown(other.to_string()),
        })
    }

    /// Spot: `wss://wbs-api.mexc.com/ws?listenKey=...`, the key made before each connection
    /// ([`TradeMapper::private_socket_bootstrap`]), the orders and deals channels. Futures:
    /// `wss://contract.mexc.com/edge`, the login alone ("After login, all personal data will be
    /// pushed by default"), nothing subscribed.
    fn private_socket(&self, _settings: &Settings) -> Option<PrivateSocket> {
        if self.is_futures() {
            return Some(PrivateSocket::new(FUT_WS, "/edge", Vec::new(), br#"{"method":"ping"}"#.to_vec(), FUT_PING_MS));
        }
        let frame = |c: &str| serde_json::json!({ "method": "SUBSCRIPTION", "params": [c] }).to_string().into_bytes();
        let mut s = PrivateSocket::new(WS, "/ws", frame(SPOT_ORDERS), br#"{"method":"PING"}"#.to_vec(), SPOT_PING_MS);
        s.more.push(frame(SPOT_DEALS));
        Some(s)
    }

    /// Spot: `POST /api/v3/userDataStream` answers `{"listenKey": ...}`.
    fn private_socket_bootstrap(&self, _settings: &Settings, _socket: &PrivateSocket, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.spot_call("spot_listen_key", Method::Post, "/api/v3/userDataStream", corr)))
    }

    fn decode_private_socket_bootstrap(&self, mut socket: PrivateSocket, body: &[u8]) -> Result<PrivateSocket, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("userDataStream", e))?;
        let key = v.get("listenKey").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or_else(|| ConnectorError::decode("mexc userDataStream: no listenKey"))?;
        socket.path = format!("/ws?{LISTEN_KEY}{key}");
        Ok(socket)
    }

    /// Spot: `PUT /api/v3/userDataStream` with the connection's key, every 30 minutes ("Extends
    /// the validity to 60 minutes from the time of this call").
    fn private_socket_keepalive(&self, _settings: &Settings, socket: &PrivateSocket, corr: CorrId) -> Result<Option<(HttpRequest, u64)>, ConnectorError> {
        if self.is_futures() {
            return Ok(None);
        }
        let Some((_, key)) = socket.path.split_once(LISTEN_KEY) else { return Ok(None) };
        Ok(Some((self.spot_call("spot_listen_key", Method::Put, "/api/v3/userDataStream", corr).query("listenKey", key), LISTEN_KEY_EVERY_MS)))
    }

    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.decode_private_in(&DecodeCtx::new("", &ContractSizes::default()), payload)
    }

    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        if self.is_futures() {
            self.fut_private(ctx, payload)
        } else {
            self.spot_private(payload)
        }
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        if self.is_futures() {
            super::trade_errors::FUT_ERRORS
        } else {
            super::trade_errors::SPOT_ERRORS
        }
    }
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod tests;
