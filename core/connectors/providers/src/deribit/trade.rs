//! Deribit orders on futures and perpetuals, written from the documentation alone (no account,
//! no key, no live order): https://docs.deribit.com/, snapshot read 2026-11-12 (OpenAPI 2.1.1).
//! Every field below is the doc's; each claim is sourced in PROVIDER.md ("Trading"). The order
//! path itself is the shared trader's; this file only says what the calls are and what the
//! answers mean.
//!
//! - **Two books on one venue, read from the instrument's name.** Linear USDC contracts
//!   (`BTC_USDC-PERPETUAL`, `SOL_USDC-27DEC26`) are sized in the base coin; inverse ones
//!   (`BTC-PERPETUAL`, `ETH-PERPETUAL`, `BTC-27DEC26`) in USD, in steps of the contract
//!   (`min_trade_amount`, 10 USD on BTC). An inverse book is quote-sized
//!   ([`VenueRules::size_unit`]): the trader hands `place` the USD amount, rounded down to the
//!   step ([`Placing::quote_amount`]), never a coin size. Positions, orders and fills on it are
//!   in USD, the unit the venue sizes them in. Options, combos and spot are refused by name.
//! - **JSON-RPC over HTTP.** Orders go out as `POST /api/v2/private/<method>` with the
//!   JSON-RPC body (the doc's own examples); reads as `GET` with the query string. Numbers go
//!   out as JSON numbers, written from exact decimals.
//! - **Orders**: `private/buy` and `private/sell` (the direction is the method): limit, market,
//!   `stop_market`, `stop_limit`, `take_market`, `take_limit` with the trigger the user picked
//!   (`last_price`, `mark_price`, `index_price`: the venue has no default); GTC, IOC, FOK,
//!   `good_til_day`; `reduce_only`; `post_only` always with `reject_post_only` (alone, the venue
//!   moves the price). The client id is the `label`.
//! - **A trigger order fires as a new order** with a new id (`trigger_order_id` names the
//!   trigger on it). A list that holds both is merged: the spawned order first, the trigger row
//!   naming it as its successor ([`VenueOrder::successor_id`]), which the trader follows for the
//!   state, the fills and the cancel. A trigger read alone by its id is linked through
//!   `private/get_trigger_order_history`.
//! - **Change in place**: `private/edit` (same id), always resending the amount and
//!   `reduce_only`, and the post-only flags of a post-only order.
//! - **Cancel** by id (`private/cancel`); cancel all per instrument
//!   (`private/cancel_all_by_instrument`, type `all`: trigger orders included).
//! - **Lookup by label**: the instrument's open orders, then `private/get_order_state_by_label`
//!   (orders closed within 30 minutes), then the instrument's order history (`historical=true`),
//!   searched by label.
//! - **Fills** from two stores: `historical=false` (the last 24 hours) and `historical=true`
//!   (older, indexed after a short delay), each asked where the window reaches it.
//! - **Private stream**: `user.changes.future.any.raw` after `public/auth`
//!   (`client_signature`), with `public/set_heartbeat` and `public/test` as the ping.
//! - **Environments**: `env` = `live` (www.deribit.com) or `test` (test.deribit.com), reads and
//!   orders alike.

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, SizeUnit, TimeInForce, TriggerKind};
use otw_connector_sdk::parse::{Int, OptDec};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, ClientIdRule, DecodeCtx, ErrorRow, Page, Placing, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper, TradingStatus, VenueError, VenueFill,
    VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::account::PriceOrWord;
use super::{test_env, Deribit, Kind, Name, ENV, REST, REST_TEST, VENUE, WS, WS_TEST};

// Endpoint ids: what an answer is read as (`DecodeCtx::endpoint`).
// The matching-engine calls share one count of their own ("Matching engine requests", Rate
// Limits): one `me` bucket takes every id under the `me:` prefix.
const BUY: &str = "me:buy";
const SELL: &str = "me:sell";
const EDIT: &str = "me:edit";
const CANCEL: &str = "me:cancel";
const CANCEL_ALL: &str = "me:cancelAllByInstrument";
/// `private/get_order_state`, asked by id: a trigger row that fired is passed over there, so
/// the trigger history that names its successor is read next.
const ORDER_STATE: &str = "orderState";
const BY_LABEL: &str = "orderStateByLabel";
const OPEN: &str = "openOrders";
const OPEN_BY_INSTRUMENT: &str = "openOrdersByInstrument";
/// The instrument's history searched by label (a lookup by client id).
const HISTORY: &str = "orderHistory";
/// The same history searched by id: a trigger row that fired is passed over, as above.
const HISTORY_BY_ID: &str = "orderHistoryById";
const TRIGGER_HISTORY: &str = "triggerHistory";
const FILLS: &str = "userTradesByInstrument";
const POSITION: &str = "position";
const POSITIONS: &str = "futurePositions";
/// Shares the instrument list's rate bucket (capabilities.toml).
const INSTRUMENTS: &str = "instruments";
const INSTRUMENT: &str = "instrument";
const TICKER: &str = "ticker";

/// "Number of requested items, default - `20`, maximum - `1000`" (history, trigger history);
/// "default - `10`, maximum - `1000`" (user trades).
const PAGE: usize = 1000;
/// "Recent trades: Available for 24 hours" (Accessing Historical Trades and Orders).
const RECENT_FILLS_MS: i64 = 86_400_000;
/// The historical store is read from an hour inside the recent one, so a fill at the edge of
/// the 24 hours is read by one store or the other.
const STORE_OVERLAP_MS: i64 = 3_600_000;
/// The socket's channel: every change of the account's futures, every currency, unbatched.
pub(crate) const CHANNEL: &str = "user.changes.future.any.raw";
/// `public/set_heartbeat` interval, seconds ("allowed range: 10 to 300"). The venue closes a
/// socket silent for `2 * interval + 500 ms`; OTW's ping goes out every 15 s.
const HEARTBEAT_S: u64 = 30;
const PING_MS: u64 = 15_000;
// JSON-RPC ids of the socket's frames (the login's is the signer's, 1).
const SUBSCRIBE_ID: u64 = 2;
const HEARTBEAT_ID: u64 = 3;
const PING_ID: u64 = 4;

/// Marks a post-only order's type word (the venue states `post_only` apart): what a change
/// resends the post-only flags by.
const POST_ONLY_MARK: &str = " post-only";

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("deribit {what}: {e}"))
}

fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn side_of(w: &str) -> Mapped<Side> {
    match w {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

/// The host of the account's environment.
fn rest(s: &Settings) -> Result<&'static str, ConnectorError> {
    Ok(if test_env(s.get(ENV).map(String::as_str))? { REST_TEST } else { REST })
}

/// A JSON-RPC parameter, written as the doc types it.
enum P {
    Num(Decimal),
    Text(String),
    Flag(bool),
    Texts(Vec<String>),
}

/// The JSON-RPC body: `{"jsonrpc":"2.0","id":..,"method":..,"params":{..}}`. Numbers are
/// written from the exact decimal (no float on the way).
fn rpc_body(id: &str, method: &str, params: &[(&str, P)]) -> String {
    let fields: Vec<String> = params
        .iter()
        .map(|(k, v)| {
            let val = match v {
                P::Num(d) => d.normalize().to_string(),
                P::Text(t) => serde_json::Value::String(t.clone()).to_string(),
                P::Flag(b) => b.to_string(),
                P::Texts(ts) => serde_json::Value::from(ts.clone()).to_string(),
            };
            format!("{}:{val}", serde_json::Value::String((*k).to_string()))
        })
        .collect();
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":{},"params":{{{}}}}}"#, serde_json::Value::String(method.to_string()), fields.join(","))
}

/// A signed JSON-RPC call: `POST /api/v2/<method>` with its body (JSON-RPC overview, "HTTP
/// REST Requests": "POST: Parameters passed in request body as JSON-RPC").
fn post(endpoint: &'static str, base: &'static str, method: &str, id: &str, params: &[(&str, P)], corr: CorrId) -> HttpRequest {
    let mut r = HttpRequest::get(endpoint, base, format!("/api/v2/{method}"), corr).signed();
    r.method = Method::Post;
    r.headers.push(("Content-Type".into(), "application/json".into()));
    r.body = Some(bytes::Bytes::from(rpc_body(id, method, params)));
    r
}

fn get(endpoint: &'static str, base: &'static str, method: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get(endpoint, base, format!("/api/v2/{method}"), corr)
}

/// A contract OTW sends orders on: a perpetual or a dated future, linear or inverse. Anything
/// else is refused by name.
fn tradable<'a>(venue: &str, symbol: &'a str) -> Result<Name<'a>, ConnectorError> {
    let n = Name::of(symbol);
    match n.kind {
        Kind::Perp | Kind::Future => Ok(n),
        Kind::Option => Err(ConnectorError::unsupported(format!("{venue} options orders are not sent from OpenTraderWorld ({symbol}): futures and perpetuals only"))),
        Kind::Spot => Err(ConnectorError::unsupported(format!("{venue} spot orders are not sent from OpenTraderWorld ({symbol}): futures and perpetuals only"))),
        Kind::Combo => Err(ConnectorError::unsupported(format!("{venue} combos are not traded from OpenTraderWorld ({symbol}): futures and perpetuals only"))),
    }
}

/// The currencies `currency` takes on the label and trigger history reads (their enum:
/// "BTC, ETH, USDC, USDT, EURR").
const CURRENCIES: &[&str] = &["BTC", "ETH", "USDC", "USDT", "EURR"];

/// The currencies the venue files an instrument's labels and trigger history under: the coin
/// of an inverse contract; for a linear one the doc does not say whether it is the settlement
/// coin (USDC) or the base, so both are asked where the enum takes them (an empty answer is no
/// error, Using the currency Parameter): `SOL_USDC-PERPETUAL` asks USDC only, SOL is no value
/// of the enum.
fn currencies(n: &Name<'_>) -> Vec<String> {
    let base = n.base().to_string();
    let mut out = match n.quote().filter(|_| n.linear()) {
        Some(q) => vec![q, base],
        None => vec![base],
    };
    out.retain(|c| CURRENCIES.contains(&c.as_str()));
    out
}

fn trigger_word(t: TriggerKind) -> Option<&'static str> {
    match t {
        TriggerKind::Last => Some("last_price"),
        TriggerKind::Mark => Some("mark_price"),
        TriggerKind::Index => Some("index_price"),
        TriggerKind::BidAsk | TriggerKind::Mid => None,
    }
}

fn type_word(t: OrderType) -> Option<&'static str> {
    match t {
        OrderType::Market => Some("market"),
        OrderType::Limit => Some("limit"),
        OrderType::Stop => Some("stop_market"),
        OrderType::StopLimit => Some("stop_limit"),
        OrderType::Mit => Some("take_market"),
        OrderType::Lit => Some("take_limit"),
        OrderType::TrailingStop => None,
    }
}

fn tif_word(t: TimeInForce) -> Option<&'static str> {
    match t {
        TimeInForce::Gtc => Some("good_til_cancelled"),
        TimeInForce::Day => Some("good_til_day"),
        TimeInForce::Ioc => Some("immediate_or_cancel"),
        TimeInForce::Fok => Some("fill_or_kill"),
        _ => None,
    }
}

/// The venue's type words that carry a limit price, and those that wait for a trigger.
fn limit_type(w: &str) -> bool {
    matches!(w, "limit" | "stop_limit" | "take_limit")
}

fn trigger_type(w: &str) -> bool {
    matches!(w, "stop_market" | "stop_limit" | "take_market" | "take_limit" | "trailing_stop")
}

// ── Answers ──

#[derive(Deserialize)]
struct Rpc<'a> {
    #[serde(borrow)]
    result: &'a RawValue,
}

/// The `result` of a JSON-RPC answer, raw.
fn result<'a>(what: &str, body: &'a [u8]) -> Result<&'a RawValue, ConnectorError> {
    serde_json::from_slice::<Rpc<'a>>(body).map(|r| r.result).map_err(|e| decode_err(what, e))
}

/// One row, or a list of them: every order (position, instrument) answer has the same row,
/// alone (`get_order_state`, `get_position`, `get_instrument`) or in a list.
fn one_or_many<T: for<'de> Deserialize<'de>>(what: &str, raw: &RawValue) -> Result<Vec<T>, ConnectorError> {
    let t = raw.get().trim_start();
    if t.starts_with('[') {
        serde_json::from_str(raw.get()).map_err(|e| decode_err(what, e))
    } else {
        serde_json::from_str(raw.get()).map(|one| vec![one]).map_err(|e| decode_err(what, e))
    }
}

#[derive(Deserialize)]
pub(crate) struct OrderRow {
    order_id: String,
    #[serde(default)]
    instrument_name: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    order_state: String,
    /// USD on an inverse contract, the coin on a linear one.
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    filled_amount: OptDec,
    /// A number, or `"market_price"` on an open trigger market order.
    #[serde(default)]
    price: PriceOrWord,
    #[serde(default)]
    trigger_price: OptDec,
    #[serde(default)]
    average_price: OptDec,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    reduce_only: bool,
    #[serde(default)]
    post_only: bool,
    #[serde(default)]
    creation_timestamp: Option<Int>,
    /// "Id of the trigger order that created the order (Only for orders that were created by
    /// triggered orders)".
    #[serde(default)]
    trigger_order_id: Option<String>,
}

impl OrderRow {
    fn order(self, m: &Deribit) -> VenueOrder {
        let filled = self.filled_amount.0.unwrap_or_default();
        let status = match (m.status_of(&self.order_state), filled > Decimal::ZERO) {
            (Mapped::Known(OrderStatus::New), true) => Mapped::Known(OrderStatus::PartiallyFilled),
            (s, _) => s,
        };
        let order_type = if self.post_only { format!("{}{POST_ONLY_MARK}", self.order_type) } else { self.order_type.clone() };
        VenueOrder {
            venue_id: self.order_id,
            client_id: self.label.filter(|l| !l.is_empty()),
            symbol: self.instrument_name,
            side: side_of(&self.direction),
            status,
            qty: self.amount.0.unwrap_or_default(),
            filled_qty: filled,
            limit: self.price.0.filter(|p| *p > Decimal::ZERO && limit_type(&self.order_type)),
            stop: positive(self.trigger_price).filter(|_| trigger_type(&self.order_type)),
            avg_price: positive(self.average_price),
            reduce_only: self.reduce_only,
            position: PositionSide::Net,
            created_ms: self.creation_timestamp.map(|t| t.0),
            trigger_pending: self.order_state == "untriggered",
            parent_client_id: None,
            exit: None,
            successor_id: None,
            order_type,
        }
    }
}

/// A trigger order and the order it spawned, in one list: the spawned one first, the trigger
/// naming it as its successor. `drop_fired`: a trigger row that fired and whose successor the
/// list does not hold is passed over (a read by id, where the trigger history names it next).
fn merged(m: &Deribit, rows: Vec<OrderRow>, drop_fired: bool) -> Vec<VenueOrder> {
    let spawned: Vec<(String, String)> = rows.iter().filter_map(|r| r.trigger_order_id.clone().filter(|t| !t.is_empty()).map(|t| (t, r.order_id.clone()))).collect();
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for r in rows {
        let child = r.trigger_order_id.as_deref().is_some_and(|t| !t.is_empty());
        let fired = r.order_state == "triggered";
        let succ = spawned.iter().find(|(t, _)| *t == r.order_id).map(|(_, id)| id.clone());
        if fired && succ.is_none() && drop_fired {
            continue;
        }
        let mut o = r.order(m);
        o.successor_id = succ;
        if child {
            first.push(o);
        } else {
            rest.push(o);
        }
    }
    first.extend(rest);
    first
}

#[derive(Deserialize)]
struct TriggerHistory {
    #[serde(default)]
    entries: Vec<TriggerEntry>,
}

#[derive(Deserialize)]
struct TriggerEntry {
    #[serde(default)]
    request: String,
    #[serde(default)]
    trigger_order_id: Option<String>,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    instrument_name: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    trigger_price: OptDec,
    #[serde(default)]
    timestamp: Option<Int>,
}

/// The trigger history's `trigger:order` entries ("when trigger order spawned market or limit
/// order after being triggered"): the trigger, naming the order it became. Its own state is the
/// fired trigger's (`triggered`): the trader reads the spawned order for the rest.
fn trigger_links(m: &Deribit, body: &[u8]) -> Result<Vec<VenueOrder>, ConnectorError> {
    let h: TriggerHistory = serde_json::from_str(result("trigger order history", body)?.get()).map_err(|e| decode_err("trigger order history", e))?;
    Ok(h.entries
        .into_iter()
        .filter(|e| e.request == "trigger:order")
        .filter_map(|e| {
            let (t, o) = (e.trigger_order_id.filter(|t| !t.is_empty())?, e.order_id.filter(|o| !o.is_empty())?);
            Some(VenueOrder {
                venue_id: t,
                client_id: None,
                symbol: e.instrument_name,
                side: side_of(&e.direction),
                order_type: String::new(),
                status: m.status_of("triggered"),
                qty: e.amount.0.unwrap_or_default(),
                filled_qty: Decimal::ZERO,
                limit: None,
                stop: positive(e.trigger_price),
                avg_price: None,
                reduce_only: false,
                position: PositionSide::Net,
                created_ms: e.timestamp.map(|t| t.0),
                trigger_pending: false,
                parent_client_id: None,
                exit: None,
                successor_id: Some(o),
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct TradeRow {
    trade_id: String,
    #[serde(default)]
    instrument_name: String,
    timestamp: Int,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    direction: String,
    /// USD on an inverse contract, the coin on a linear one (the order's unit).
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    price: OptDec,
    /// "User's fee in units of the specified `fee_currency`": its sign is not documented, so it
    /// is carried as stated, never made positive.
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: Option<String>,
    /// `limit`, `market` or `liquidation`.
    #[serde(default)]
    order_type: String,
    /// "only for trades caused by liquidation": `M` when the maker side was under liquidation,
    /// `T` the taker side, `MT` both. The counterparty's liquidation sets it too.
    #[serde(default)]
    liquidation: Option<String>,
    /// "what was role of users order": `M` maker, `T` taker.
    #[serde(default)]
    liquidity: Option<String>,
}

impl TradeRow {
    fn fill(self) -> VenueFill {
        // The account's own side was under liquidation: a liquidation order, or the side named
        // by `liquidation` is the account's role (unread role: taken as the account's).
        let own_side = |l: &str| self.liquidity.as_deref().filter(|r| !r.is_empty()).is_none_or(|r| l.contains(r));
        let liquidation = self.liquidation.as_deref().is_some_and(|l| !l.is_empty() && own_side(l)) || self.order_type == "liquidation";
        VenueFill {
            exec_id: self.trade_id,
            venue_order_id: self.order_id.unwrap_or_default(),
            client_id: self.label.filter(|l| !l.is_empty()),
            symbol: self.instrument_name,
            side: side_of(&self.direction),
            qty: self.amount.0.unwrap_or_default(),
            price: self.price.0.unwrap_or_default(),
            fee: self.fee.0.unwrap_or_default(),
            fee_currency: self.fee_currency.unwrap_or_default().to_ascii_uppercase(),
            at_ms: self.timestamp.0,
            position_only: liquidation,
            extra_fees: Vec::new(),
            fee_pending: false,
        }
    }
}

#[derive(Deserialize)]
struct TradesPage {
    #[serde(default)]
    trades: Vec<TradeRow>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Deserialize)]
struct PositionRow {
    instrument_name: String,
    /// `buy`, `sell` or `zero`.
    #[serde(default)]
    direction: String,
    /// "Position size for futures size in quote currency (e.g. USD)": USD on an inverse
    /// contract.
    #[serde(default)]
    size: OptDec,
    /// "Only for futures, position size in base currency": a linear contract's size, in the
    /// coin its orders are sized in.
    #[serde(default)]
    size_currency: OptDec,
    /// "Current available leverage for a future position", read only.
    #[serde(default)]
    leverage: OptDec,
}

impl PositionRow {
    fn position(self) -> Result<VenuePosition, ConnectorError> {
        let direction = match self.direction.as_str() {
            "buy" => Some(Side::Buy),
            "sell" => Some(Side::Sell),
            "zero" | "" => None,
            other => return Err(ConnectorError::decode(format!("deribit position: direction `{other}` on {}", self.instrument_name))),
        };
        let n = Name::of(&self.instrument_name);
        // In the unit its orders are sized in: USD on an inverse contract, the coin on a linear
        // one (`size` there is "in quote currency"), the coin on an option.
        let qty = match (n.inverse(), n.linear() && n.kind != Kind::Option) {
            (true, _) => self.size.0,
            (false, true) => self.size_currency.0,
            (false, false) => self.size.0,
        };
        if qty.is_none() && direction.is_some() {
            return Err(ConnectorError::decode(format!("deribit position: {} states no size in the unit its orders take", self.instrument_name)));
        }
        Ok(VenuePosition {
            symbol: self.instrument_name,
            side: PositionSide::Net,
            direction,
            qty: qty.unwrap_or_default().abs(),
            leverage: positive(self.leverage),
            margin: None,
        })
    }
}

#[derive(Deserialize)]
struct Ticker {
    #[serde(default)]
    last_price: OptDec,
}

/// One instrument row's rules: futures and perpetuals only (anything else is not traded here).
fn rules_of(r: &super::InstrumentRow) -> Option<VenueRules> {
    if r.kind != "future" || r.instrument_name.trim().is_empty() {
        return None;
    }
    let step = positive(r.min_trade_amount);
    let status = match r.state.as_deref() {
        Some("open") => TradingStatus::Trading,
        Some(word) => TradingStatus::Closed(word.to_string()),
        None => TradingStatus::Closed("no book state stated".into()),
    };
    Some(VenueRules {
        tick: positive(r.tick_size),
        lot_step: step,
        min_qty: step,
        status,
        size_unit: if r.instrument_type.as_deref() == Some("reversed") { SizeUnit::Quote } else { SizeUnit::Base },
        ..VenueRules::new(InstrumentId::new(VENUE, r.instrument_name.trim()))
    })
}

impl Deribit {
    /// A list of order rows read as the call it answers says.
    fn orders_in(&self, endpoint: &str, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        if endpoint == TRIGGER_HISTORY {
            return Ok((trigger_links(self, body)?, None));
        }
        let rows: Vec<OrderRow> = one_or_many("orders", result("orders", body)?)?;
        let full = rows.len() >= PAGE;
        let by_id = matches!(endpoint, ORDER_STATE | HISTORY_BY_ID);
        let out = merged(self, rows, by_id);
        // A full history page: the next one by offset (`with_cursor`).
        let next = (full && matches!(endpoint, HISTORY | HISTORY_BY_ID)).then(|| PAGE.to_string());
        Ok((out, next))
    }

    fn history(&self, s: &Settings, endpoint: &'static str, symbol: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(get(endpoint, rest(s)?, "private/get_order_history_by_instrument", corr)
            .signed()
            .query("instrument_name", symbol)
            .query("count", PAGE)
            .query("include_old", "true")
            .query("include_unfilled", "true")
            .query("historical", "true"))
    }

    /// The user trades of `symbol` over `[from, to]`, in one store.
    fn fills_call(&self, s: &Settings, symbol: &str, from: i64, to: i64, historical: bool, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(get(FILLS, rest(s)?, "private/get_user_trades_by_instrument_and_time", corr)
            .signed()
            .query("instrument_name", symbol)
            .query("start_timestamp", from)
            .query("end_timestamp", to)
            .query("count", PAGE)
            .query("sorting", "asc")
            .query("historical", historical))
    }

    fn frame_events(&self, data: &serde_json::Value) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let mut out = Vec::new();
        if let Some(rows) = data.get("orders").filter(|v| !v.is_null()) {
            let rows: Vec<OrderRow> = serde_json::from_value(rows.clone()).map_err(|e| decode_err("stream orders", e))?;
            let spawned: Vec<String> = rows.iter().filter_map(|r| r.trigger_order_id.clone()).collect();
            for r in rows {
                if r.order_state == "triggered" {
                    // The trigger fired as a new order: the trader follows it to that one (in
                    // this frame or read by id).
                    if !spawned.contains(&r.order_id) {
                        out.push(PrivateEvent::OrderChanged { symbol: Some(r.instrument_name.clone()), venue_id: r.order_id });
                    }
                    continue;
                }
                out.push(PrivateEvent::Order(r.order(self)));
            }
        }
        if let Some(rows) = data.get("trades").filter(|v| !v.is_null()) {
            let rows: Vec<TradeRow> = serde_json::from_value(rows.clone()).map_err(|e| decode_err("stream trades", e))?;
            out.extend(rows.into_iter().map(|t| PrivateEvent::Fill(t.fill())));
        }
        if let Some(rows) = data.get("positions").filter(|v| !v.is_null()) {
            let rows: Vec<PositionRow> = serde_json::from_value(rows.clone()).map_err(|e| decode_err("stream positions", e))?;
            // A row that cannot be read (a linear row without `size_currency`) is left out
            // alone: the frame's orders, fills and other positions still count, and the next
            // positions read states it.
            for p in rows {
                let name = p.instrument_name.clone();
                match p.position() {
                    Ok(row) => out.push(PrivateEvent::Position(row)),
                    Err(e) => tracing::warn!("deribit stream: the position row of {name} is left out: {}", e.message),
                }
            }
        }
        Ok(out)
    }
}

impl TradeMapper for Deribit {
    fn venue_name(&self) -> &str {
        "Deribit"
    }

    /// `label`: "user defined label for the order (maximum 64 characters)". The charset is
    /// not documented: OTW's keys keep to letters, digits, `-` and `_`.
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(64)
    }

    fn is_sandbox(&self, settings: &Settings) -> bool {
        test_env(settings.get(ENV).map(String::as_str)).unwrap_or(false)
    }

    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        let v = self.venue_name();
        tradable(v, &o.instrument.native)?;
        if o.order_type.triggered() {
            match o.opts.trigger {
                None => {
                    return Err(ConnectorError::invalid(format!(
                        "{v} needs the price a stop or if-touched order triggers on, and has no default: pick last, mark or index"
                    )))
                }
                Some(t) if trigger_word(t).is_none() => return Err(ConnectorError::unsupported(format!("{v} triggers on the last, mark or index price, not {t:?}"))),
                Some(_) => {}
            }
        } else if o.opts.trigger.is_some() {
            return Err(ConnectorError::invalid(format!("{v}: a trigger price kind applies to stop and if-touched orders only")));
        }
        if o.order_type == OrderType::Market && o.tif == TimeInForce::Day {
            return Err(ConnectorError::invalid(format!("{v}: a market order takes no good-til-day time in force: send it GTC, IOC or FOK")));
        }
        // "Only valid for non-market, non-advanced orders with time_in_force=good_til_cancelled
        // or good_til_day" (private/buy, `post_only`).
        if o.opts.post_only && !(o.order_type == OrderType::Limit && matches!(o.tif, TimeInForce::Gtc | TimeInForce::Day)) {
            return Err(ConnectorError::invalid(format!("{v} takes post-only on a GTC or good-til-day limit order only")));
        }
        Ok(())
    }

    fn place(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let v = self.venue_name();
        let sym = o.instrument.native.as_str();
        let n = tradable(v, sym)?;
        let kind = type_word(o.order_type).ok_or_else(|| ConnectorError::unsupported(format!("{v} takes no {:?} order from OpenTraderWorld", o.order_type)))?;
        let amount = if n.inverse() {
            // Sized in USD: the trader's amount, rounded down to the step, never a coin size.
            p.quote_amount.ok_or_else(|| ConnectorError::invalid(format!("{v} sizes {sym} in USD, and the order was not sized in it: nothing was sent")))?
        } else {
            if o.opts.size_unit == SizeUnit::Quote {
                return Err(ConnectorError::invalid(format!("{v} sizes {sym} in the base coin: give the size in it")));
            }
            o.qty.value()
        };
        let mut params: Vec<(&str, P)> = vec![("instrument_name", P::Text(sym.to_string())), ("amount", P::Num(amount)), ("type", P::Text(kind.into()))];
        if !p.client_id.is_empty() {
            params.push(("label", P::Text(p.client_id.clone())));
        }
        if let Some(px) = o.limit {
            params.push(("price", P::Num(px.value())));
        }
        if let Some(stop) = o.stop {
            params.push(("trigger_price", P::Num(stop.value())));
        }
        if let Some(t) = o.opts.trigger.and_then(trigger_word) {
            params.push(("trigger", P::Text(t.into())));
        }
        // A market order goes without one unless IOC or FOK is asked (the venue's default is
        // good_til_cancelled).
        let tif = tif_word(o.tif).ok_or_else(|| ConnectorError::unsupported(format!("{v} takes GTC, IOC, FOK or good-til-day")))?;
        if !(o.order_type == OrderType::Market && o.tif == TimeInForce::Gtc) {
            params.push(("time_in_force", P::Text(tif.into())));
        }
        if o.opts.post_only {
            // Alone, `post_only` moves a crossing price "just below the spread": with
            // `reject_post_only` the order is "put to the order book unmodified or the request
            // is rejected".
            params.push(("post_only", P::Flag(true)));
            params.push(("reject_post_only", P::Flag(true)));
        }
        if o.opts.reduce_only {
            params.push(("reduce_only", P::Flag(true)));
        }
        let (endpoint, method) = match o.side {
            Side::Buy => (BUY, "private/buy"),
            Side::Sell => (SELL, "private/sell"),
        };
        // The JSON-RPC id is the label: unique per order, and the answer echoes it.
        let id = if p.client_id.is_empty() { p.corr.0.to_string() } else { serde_json::Value::String(p.client_id.clone()).to_string() };
        Ok(post(endpoint, rest(settings)?, method, &id, &params, p.corr))
    }

    /// `private/edit`: the same order id, the new total amount ("the filled 100 remains ... the
    /// remaining quantity is updated"), always resent with `reduce_only` (whether an edit that
    /// leaves it out clears it is not documented) and, on a post-only order, both post-only
    /// flags (alone, the venue "will adjust the price on edit").
    fn amend(&self, settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let v = self.venue_name();
        tradable(v, &cur.symbol)?;
        if change.tif.is_some() {
            return Err(ConnectorError::unsupported(format!("{v}'s edit keeps the order's time in force: cancel it and send a new one")));
        }
        let word = cur.order_type.trim_end_matches(POST_ONLY_MARK);
        let post_only = cur.order_type.ends_with(POST_ONLY_MARK);
        if change.limit.is_some() && !limit_type(word) {
            return Err(ConnectorError::invalid(format!("{v}: order {} ({word}) has no limit price to change", cur.venue_id)));
        }
        if change.stop.is_some() && !trigger_type(word) {
            return Err(ConnectorError::invalid(format!("{v}: order {} ({word}) has no trigger price to change (a limit order does not become a stop by an edit)", cur.venue_id)));
        }
        let amount = change.qty.map(|q| q.value()).unwrap_or(cur.qty);
        let mut params: Vec<(&str, P)> = vec![("order_id", P::Text(cur.venue_id.clone())), ("amount", P::Num(amount))];
        if let Some(px) = change.limit.map(|x| x.value()).or(cur.limit.filter(|_| limit_type(word))) {
            params.push(("price", P::Num(px)));
        }
        if let Some(stop) = change.stop.map(|x| x.value()).or(cur.stop.filter(|_| trigger_type(word))) {
            params.push(("trigger_price", P::Num(stop)));
        }
        if post_only {
            params.push(("post_only", P::Flag(true)));
            params.push(("reject_post_only", P::Flag(true)));
        }
        params.push(("reduce_only", P::Flag(cur.reduce_only)));
        Ok(post(EDIT, rest(settings)?, "private/edit", &p.corr.0.to_string(), &params, p.corr))
    }

    fn cancel(&self, settings: &Settings, _symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(post(CANCEL, rest(settings)?, "private/cancel", &corr.0.to_string(), &[("order_id", P::Text(venue_id.to_string()))], corr))
    }

    /// One `private/cancel_all_by_instrument` per instrument with working orders, type `all`
    /// (trigger orders included), whatever placed them.
    fn cancel_all(&self, settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let base = rest(settings)?;
        Ok(symbols
            .iter()
            .map(|s| post(CANCEL_ALL, base, "private/cancel_all_by_instrument", &corr.0.to_string(), &[("instrument_name", P::Text(s.clone())), ("type", P::Text("all".into()))], corr))
            .collect())
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::BySymbol
    }

    /// "Total number of successfully cancelled orders": a count, nothing else to read.
    fn decode_cancel_all(&self, body: &[u8]) -> Result<(), ConnectorError> {
        result("cancel all", body)?.get().trim().parse::<u64>().map(|_| ()).map_err(|_| ConnectorError::decode("deribit cancel all: the answer is not a count"))
    }

    /// The instrument's working orders (trigger orders included, `type=all`), then the orders
    /// closed within 30 minutes under that label, then the instrument's history.
    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let base = rest(settings)?;
        let n = Name::of(symbol);
        let mut out = vec![get(OPEN_BY_INSTRUMENT, base, "private/get_open_orders_by_instrument", corr).signed().query("instrument_name", symbol).query("type", "all")];
        for c in currencies(&n) {
            out.push(get(BY_LABEL, base, "private/get_order_state_by_label", corr).signed().query("currency", c).query("label", client_id));
        }
        out.push(self.history(settings, HISTORY, symbol, corr)?);
        Ok(out)
    }

    /// `private/get_order_state` (a working order, "status (open, filled, cancelled)", or one
    /// that "closed within the last 30 minutes"), the instrument's working orders, the trigger
    /// history (where a trigger that fired names the order it became), then the history.
    fn query_by_venue_id(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let base = rest(settings)?;
        let n = Name::of(symbol);
        let mut out = vec![
            get(ORDER_STATE, base, "private/get_order_state", corr).signed().query("order_id", venue_id),
            get(OPEN_BY_INSTRUMENT, base, "private/get_open_orders_by_instrument", corr).signed().query("instrument_name", symbol).query("type", "all"),
        ];
        for c in currencies(&n) {
            out.push(get(TRIGGER_HISTORY, base, "private/get_trigger_order_history", corr).signed().query("currency", c).query("instrument_name", symbol).query("count", PAGE));
        }
        out.push(self.history(settings, HISTORY_BY_ID, symbol, corr)?);
        Ok(out)
    }

    /// Two pages of history (2000 orders on the instrument) besides the other calls.
    fn lookup_pages(&self) -> usize {
        2
    }

    /// The working orders of `symbol`, or of the whole account (every kind: what the account
    /// holds outside futures is listed, and cancelled by cancel all, too).
    fn open_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let base = rest(settings)?;
        Ok(vec![match symbol {
            Some(s) => get(OPEN_BY_INSTRUMENT, base, "private/get_open_orders_by_instrument", corr).signed().query("instrument_name", s),
            None => get(OPEN, base, "private/get_open_orders", corr).signed(),
        }])
    }

    /// The recent store (the last 24 hours, available at once).
    fn fills_since(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.fills_call(settings, symbol, from_ms, to_ms, false, corr)
    }

    /// The recent store where the window reaches the last 24 hours, the historical one where it
    /// starts before them ("Recent data is not included" there); an hour of overlap, a fill
    /// read twice dropped by its id.
    fn fills_requests(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, now_ms: i64, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let edge = now_ms - RECENT_FILLS_MS;
        let mut out = Vec::new();
        if from_ms < edge + STORE_OVERLAP_MS {
            out.push(self.fills_call(settings, symbol, from_ms, to_ms, true, corr)?);
        }
        if to_ms >= edge {
            out.push(self.fills_call(settings, symbol, from_ms.max(edge), to_ms, false, corr)?);
        }
        Ok(out)
    }

    /// The futures of the instrument's currency: the coin for an inverse contract, the
    /// settlement coin for a linear one ("every instrument whose name involves the currency");
    /// every future and perpetual when no symbol is named.
    fn instrument_rules(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let base = rest(settings)?;
        let currency = match symbol {
            Some(s) => {
                let n = tradable(self.venue_name(), s)?;
                if n.linear() {
                    n.quote().unwrap_or_default()
                } else {
                    n.base().to_string()
                }
            }
            None => "any".to_string(),
        };
        Ok(get(INSTRUMENTS, base, "public/get_instruments", corr).query("currency", currency).query("kind", "future"))
    }

    /// One instrument, "including ... expired or archivized instruments".
    fn instrument_rules_any_status(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        tradable(self.venue_name(), symbol)?;
        Ok(vec![get(INSTRUMENT, rest(settings)?, "public/get_instrument", corr).query("instrument_name", symbol)])
    }

    /// The next page: the history by offset, the fills from the last fill's millisecond (a
    /// fill read twice is dropped by its id).
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        let key = if req.endpoint_id == FILLS { "start_timestamp" } else { "offset" };
        let value = if key == "offset" {
            let at: u64 = req.query.iter().find(|(k, _)| k == "offset").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
            (at + cursor.parse::<u64>().unwrap_or(PAGE as u64)).to_string()
        } else {
            cursor.to_string()
        };
        next.query.retain(|(k, _)| k != key);
        next.query.push((key.to_string(), value));
        next
    }

    fn positions(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(get(POSITION, rest(settings)?, "private/get_position", corr).signed().query("instrument_name", symbol)))
    }

    fn all_positions(&self, settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![get(POSITIONS, rest(settings)?, "private/get_positions", corr).signed().query("currency", "any").query("kind", "future")])
    }

    /// The last trade, what a size given in the coin is converted to USD at on an inverse
    /// contract.
    fn reference_price(&self, settings: &Settings, symbol: &str, _side: Side, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(get(TICKER, rest(settings)?, "public/ticker", corr).query("instrument_name", symbol)))
    }

    fn decode_reference_price(&self, body: &[u8], _side: Side) -> Result<Decimal, ConnectorError> {
        let t: Ticker = serde_json::from_str(result("ticker", body)?.get()).map_err(|e| decode_err("ticker", e))?;
        positive(t.last_price).ok_or_else(|| ConnectorError::decode("deribit ticker: no last price to convert the coin size at"))
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        #[derive(Deserialize)]
        struct Placed {
            order: Option<OrderRow>,
        }
        let p: Placed = serde_json::from_str(result("order", body)?.get()).map_err(|e| decode_err("order", e))?;
        let o = p.order.ok_or_else(|| ConnectorError::decode("deribit order: the answer names no order"))?;
        Ok(Ack { venue_id: o.order_id, client_id: o.label.filter(|l| !l.is_empty()) })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.orders_in(OPEN, body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.orders_in(ctx.endpoint, body)
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let p: TradesPage = serde_json::from_str(result("user trades", body)?.get()).map_err(|e| decode_err("user trades", e))?;
        let last = p.trades.iter().map(|t| t.timestamp.0).max();
        let next = last.filter(|_| p.has_more).map(|t| t.to_string());
        Ok((p.trades.into_iter().map(TradeRow::fill).collect(), next))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let rows: Vec<super::InstrumentRow> = one_or_many("instruments", result("instruments", body)?)?;
        Ok((rows.iter().filter_map(rules_of).collect(), None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let rows: Vec<PositionRow> = one_or_many("position", result("position", body)?)?;
        rows.into_iter().map(PositionRow::position).collect()
    }

    /// `order_state` ("open", "filled", "rejected", "cancelled", "untriggered", "triggered").
    /// `open` is read as partly filled when something filled; `untriggered` is working, its
    /// trigger pending. `triggered` is a trigger that fired as a new order: its own state says
    /// nothing of that order's, which the trader follows instead; read alone, it is not known.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        match word {
            "open" | "untriggered" => Mapped::Known(OrderStatus::New),
            "filled" => Mapped::Known(OrderStatus::Filled),
            "rejected" => Mapped::Known(OrderStatus::Rejected),
            "cancelled" => Mapped::Known(OrderStatus::Cancelled),
            other => Mapped::Unknown(other.to_string()),
        }
    }

    fn private_socket(&self, settings: &Settings) -> Option<PrivateSocket> {
        let base = if self.is_sandbox(settings) { WS_TEST } else { WS };
        let subscribe = rpc_body(&SUBSCRIBE_ID.to_string(), "private/subscribe", &[("channels", P::Texts(vec![CHANNEL.to_string()]))]);
        let heartbeat = rpc_body(&HEARTBEAT_ID.to_string(), "public/set_heartbeat", &[("interval", P::Num(Decimal::from(HEARTBEAT_S)))]);
        let ping = rpc_body(&PING_ID.to_string(), "public/test", &[]);
        let mut s = PrivateSocket::new(base, "/ws/api/v2", subscribe.into_bytes(), ping.into_bytes(), PING_MS);
        s.more.push(heartbeat.into_bytes());
        Some(s)
    }

    /// The socket's frames: the answers to OTW's own (by their JSON-RPC id: the login, the
    /// subscription and the heartbeat setting, the ping), the venue's heartbeat (answered by the
    /// next ping), and the `user.changes` notifications.
    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let v: serde_json::Value = serde_json::from_slice(payload).map_err(|e| decode_err("stream frame", e))?;
        if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
            let op = match id {
                super::signer::AUTH_ID => "auth",
                SUBSCRIBE_ID | HEARTBEAT_ID => "subscribe",
                PING_ID => "ping",
                _ => return Ok(Vec::new()),
            };
            if let Some(err) = v.get("error") {
                let message = format!("{} (code {})", err.get("message").and_then(|m| m.as_str()).unwrap_or("refused"), err.get("code").map(|c| c.to_string()).unwrap_or_default());
                return Ok(vec![PrivateEvent::Answer { op: op.into(), ok: false, message }]);
            }
            let res = v.get("result");
            // A subscription answers the channels it took: the one asked must be among them.
            if id == SUBSCRIBE_ID && !res.and_then(|r| r.as_array()).is_some_and(|a| a.iter().any(|c| c.as_str() == Some(CHANNEL))) {
                return Ok(vec![PrivateEvent::Answer { op: op.into(), ok: false, message: format!("{CHANNEL} was not subscribed") }]);
            }
            return Ok(vec![PrivateEvent::Answer { op: op.into(), ok: res.is_some(), message: String::new() }]);
        }
        match v.get("method").and_then(|m| m.as_str()) {
            Some("subscription") => {
                let params = v.get("params").cloned().unwrap_or_default();
                let channel = params.get("channel").and_then(|c| c.as_str()).unwrap_or_default();
                if !channel.starts_with("user.changes.") {
                    return Ok(Vec::new());
                }
                self.frame_events(params.get("data").unwrap_or(&serde_json::Value::Null))
            }
            _ => Ok(Vec::new()),
        }
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        super::trade_errors::ERRORS
    }
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod tests;
