//! Coinbase Advanced Trade orders, spot book only, written from the documentation alone (no
//! account, no key; the static sandbox's canned answers as fixtures). Every field below is the
//! OpenAPI document's; each claim is sourced in PROVIDER.md ("Trading"). The order path itself is
//! the shared trader's; this file only says what the calls are and what the answers mean.
//!
//! - Place: `POST /orders`, the order one key of `order_configuration`: `market_market_ioc`,
//!   `limit_limit_gtc` / `_gtd` (with `post_only`), `sor_limit_ioc`, `limit_limit_fok`,
//!   `stop_limit_stop_limit_gtc` / `_gtd` (`stop_direction` from the side: a buy stop-limit
//!   triggers up, a sell down; the opposite direction is OTW's limit-if-touched). Every size is
//!   `base_size`, in the base coin, a market buy included ("To buy a product, provide a
//!   `quote_size` or `base_size`").
//! - Client id: `client_order_id`, the OTW key unchanged (charset and length doc silent: 36
//!   letters, digits, `-` and `_` at most, the commonest rule).
//! - Cancel: `POST /orders/batch_cancel` (the only cancel), one id; the kill switch batches ids.
//! - Change: `POST /orders/edit`, price and size of a working limit order, in place (the venue
//!   id is kept).
//! - Look up: by the venue's id `GET /orders/historical/{order_id}`; by the client id there is no
//!   call, so the product's orders are listed from a little before the order was sent and
//!   matched (`lookup_pages` pages at most).
//! - Refusals travel inside HTTP 200 bodies (`trade_errors.rs`).
//! - Stream: the `user` channel on `advanced-trade-ws-user`, a JWT in each subscribe frame (the
//!   signer's `sign_frame`), `heartbeats` beside it; an order frame does not say the unit of its
//!   remaining size, so the order is read again by its id.

#![deny(clippy::float_arithmetic)]

use otw_connector_model::{ConnectorError, CorrId, Decimal, ErrorKind, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, TimeInForce, Timestamp};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, CancelRow, ClientIdRule, DecodeCtx, ErrorRow, LookupAt, Page, Placing, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper, TradingStatus, VenueError,
    VenueFill, VenueOrder, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::trade_errors::{CANCEL, CANCEL_BATCH, CREATE, EDIT, GET_ORDER, PREVIEW};
use super::{rfc3339, Coinbase, BROKERAGE, PREFIX, VENUE, WS_USER};

/// Orders per page of a list (List Orders: "no default amount"; the largest it takes is doc
/// silent, and 100 is what the account read has always asked).
const ORDERS_LIMIT: u32 = 100;
/// Fills per page (List Fills: "default is 100").
const FILLS_LIMIT: usize = 100;
/// Ids per batch cancel of the kill switch: the doc names no maximum, so a small batch.
const CANCEL_BATCH_MAX: usize = 50;
/// Pages a lookup by client id reads before it says the order was not found there.
const LOOKUP_PAGES: usize = 5;
/// How long before the order was sent the lookup's window opens (the two clocks may differ).
const LOOKUP_SLACK_MS: i64 = 60_000;
/// The heartbeats channel sends "every second"; silence for twice this drops the socket.
const SILENCE_MS: u64 = 10_000;
/// The placement sources whose orders the account lists, each asked for apart: List Orders
/// takes one (`order_placement_source`, "By default, returns RETAIL_ADVANCED placement source"),
/// and its enum names these five.
pub(super) const SOURCES: [&str; 5] = ["RETAIL_ADVANCED", "RETAIL_SIMPLE", "RETAIL_ADMIN", "RETAIL_RAISE", "UNKNOWN_PLACEMENT_SOURCE"];
/// The `OrderExecutionStatus` words of an order still working, all asked in one list (List
/// Orders `order_status` takes several): `OPEN`, and `PENDING`, `QUEUED`, `CANCEL_QUEUED`,
/// `EDIT_QUEUED`, which may still fill.
pub(super) const WORKING: [&str; 5] = ["OPEN", "PENDING", "QUEUED", "CANCEL_QUEUED", "EDIT_QUEUED"];
/// What an attached take profit and stop loss adds to its entry's type words (`LIMIT with take
/// profit 99990 and stop loss 99800 attached`).
const ATTACHED: &str = " with take profit ";

/// List Orders and List Fills answer `proof_token_required` "Indicates that a valid proof token
/// is required to access this data (EU SCA compliance)": the list may then be incomplete, so it
/// is an error, never read as empty.
pub(super) fn proof_token_refused(what: &str) -> ConnectorError {
    ConnectorError::new(
        ErrorKind::Permission,
        format!(
            "Coinbase withholds the {what} until a proof token is given (EU strong customer authentication, proof_token_required), which OpenTraderWorld does not send: the list is not read as empty. Check them at Coinbase."
        ),
    )
}

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("coinbase {what}: {e}"))
}

fn side_word(s: Side) -> &'static str {
    match s {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

fn side_of(word: &str) -> Mapped<Side> {
    match word {
        "BUY" => Mapped::Known(Side::Buy),
        "SELL" => Mapped::Known(Side::Sell),
        other => otw_connector_sdk::enums::unknown("coinbase", "side", other),
    }
}

/// A positive amount, or none: Coinbase writes `"0"` and `""` for one an order does not have.
fn positive(d: &OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

/// A venue id that lands in a URL path: Coinbase's ids are UUIDs, so letters, digits and `-`.
fn path_id(id: &str) -> Result<&str, ConnectorError> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(ConnectorError::invalid(format!("Coinbase order id {id:?} is not one Coinbase issues")));
    }
    Ok(id)
}

fn millis(t: &str) -> Option<i64> {
    ts_rfc3339(t).ok().map(|t| t.millis())
}

/// The prices and sizes of an order, read by name from whichever configuration it was placed
/// with (`limit_limit_gtc`, `stop_limit_stop_limit_gtc`, `trigger_bracket_gtc`...).
#[derive(Deserialize, Default)]
pub(super) struct Config {
    #[serde(default)]
    pub base_size: OptDec,
    #[serde(default)]
    pub quote_size: OptDec,
    #[serde(default)]
    pub limit_price: OptDec,
    #[serde(default)]
    pub stop_price: OptDec,
    /// `trigger_bracket_gtc` / `_gtd`: "The price level (in quote currency) where the position
    /// will be exited", its stop where a stop-limit says `stop_price`.
    #[serde(default)]
    pub stop_trigger_price: OptDec,
}

impl Config {
    /// The one configuration an order carries (an empty or absent one reads as nothing).
    pub(super) fn of(cfg: Option<&Map<String, Value>>) -> Result<Config, ConnectorError> {
        match cfg.and_then(|m| m.values().find(|v| v.is_object())) {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| decode_err("order configuration", e)),
            None => Ok(Config::default()),
        }
    }

    /// The stop: `stop_price` (stop-limit), else `stop_trigger_price` (bracket).
    pub(super) fn stop(&self) -> Option<Decimal> {
        positive(&self.stop_price).or_else(|| positive(&self.stop_trigger_price))
    }
}

/// The type words of an order with its attached take profit and stop loss said
/// (`attached_order_configuration`, "Only TriggerBracketGtc is eligible": its `limit_price` the
/// take profit, its `stop_trigger_price` the stop loss), so a working entry shows its exits.
pub(super) fn with_attached(words: String, attached: Option<&Map<String, Value>>) -> Result<String, ConnectorError> {
    let a = Config::of(attached)?;
    let n = |d: Option<Decimal>| d.map(dec_text).unwrap_or_else(|| "?".into());
    Ok(match (positive(&a.limit_price), a.stop()) {
        (None, None) => words,
        (tp, sl) => format!("{words}{ATTACHED}{} and stop loss {} attached", n(tp), n(sl)),
    })
}

/// The base size of an order, and the words its type is shown with: an order placed in the quote
/// currency (`size_in_quote`, `quote_size`) states no base size until it fills, so its size is
/// what it has filled (never one computed from the quote amount), and its type says so.
pub(super) fn base_size(order_type: &str, cfg: &Config, size_in_quote: bool, filled: Decimal) -> (Decimal, String) {
    match (positive(&cfg.base_size), positive(&cfg.quote_size)) {
        (Some(b), _) if !size_in_quote => (b, order_type.to_string()),
        (_, Some(q)) => (filled, format!("{order_type} ({} in the quote currency)", dec_text(q))),
        (Some(b), None) => (b, order_type.to_string()),
        (None, None) => (filled, order_type.to_string()),
    }
}

#[derive(Deserialize)]
struct OrderRow {
    order_id: String,
    #[serde(default)]
    product_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    client_order_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    order_configuration: Option<Map<String, Value>>,
    #[serde(default)]
    attached_order_configuration: Option<Map<String, Value>>,
    #[serde(default)]
    filled_size: OptDec,
    #[serde(default)]
    average_filled_price: OptDec,
    #[serde(default)]
    size_in_quote: bool,
    #[serde(default)]
    pending_cancel: bool,
    #[serde(default)]
    trigger_status: String,
    #[serde(default)]
    created_time: Option<String>,
}

/// List Orders (`orders`, `has_next`, `cursor`) and Get Order (`order`), read alike.
#[derive(Deserialize)]
struct Orders {
    #[serde(default)]
    orders: Vec<OrderRow>,
    #[serde(default)]
    order: Option<OrderRow>,
    #[serde(default)]
    has_next: bool,
    #[serde(default)]
    cursor: String,
    #[serde(default)]
    proof_token_required: bool,
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
    trade_time: String,
    #[serde(default)]
    trade_type: String,
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
}

#[derive(Deserialize)]
struct Fills {
    #[serde(default)]
    fills: Vec<FillRow>,
    #[serde(default)]
    cursor: String,
    #[serde(default)]
    proof_token_required: bool,
}

/// One product of the public product list (`/market/products`).
#[derive(Deserialize)]
struct ProductRow {
    product_id: String,
    #[serde(default)]
    product_type: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    price_increment: OptDec,
    #[serde(default)]
    base_increment: OptDec,
    #[serde(default)]
    quote_increment: OptDec,
    #[serde(default)]
    base_min_size: OptDec,
    #[serde(default)]
    base_max_size: OptDec,
    #[serde(default)]
    quote_min_size: OptDec,
    #[serde(default)]
    is_disabled: bool,
    #[serde(default)]
    trading_disabled: bool,
    #[serde(default)]
    cancel_only: bool,
    #[serde(default)]
    limit_only: bool,
    #[serde(default)]
    post_only: bool,
    #[serde(default)]
    view_only: bool,
}

#[derive(Deserialize, Default)]
struct Pagination {
    #[serde(default)]
    next_cursor: String,
    #[serde(default)]
    has_next: bool,
}

#[derive(Deserialize)]
struct Products {
    #[serde(default)]
    products: Vec<ProductRow>,
    #[serde(default)]
    pagination: Option<Pagination>,
}

#[derive(Deserialize)]
struct CreateAnswer {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    success_response: Option<SuccessResponse>,
}

#[derive(Deserialize)]
struct SuccessResponse {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    client_order_id: Option<String>,
}

#[derive(Deserialize)]
struct Preview {
    #[serde(default)]
    order_total: OptDec,
    #[serde(default)]
    commission_total: OptDec,
    #[serde(default)]
    best_bid: OptDec,
    #[serde(default)]
    best_ask: OptDec,
    #[serde(default)]
    warning: Vec<String>,
}

impl Coinbase {
    fn get(endpoint: &'static str, path: &str, corr: CorrId) -> HttpRequest {
        HttpRequest::get(endpoint, BROKERAGE, format!("{PREFIX}{path}"), corr).signed()
    }

    fn post(endpoint: &'static str, path: &str, body: &Value, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let bytes = serde_json::to_vec(body).map_err(|e| ConnectorError::invalid(format!("coinbase {endpoint}: {e}")))?;
        let mut h = HttpRequest::get(endpoint, BROKERAGE, format!("{PREFIX}{path}"), corr).signed();
        h.method = Method::Post;
        h.headers.push(("Content-Type".into(), "application/json".into()));
        h.body = Some(bytes.into());
        Ok(h)
    }

    /// List Orders on `symbol` (every product when `None`).
    fn list(endpoint: &'static str, symbol: Option<&str>, corr: CorrId) -> HttpRequest {
        let mut h = Self::get(endpoint, "/orders/historical/batch", corr).query("limit", ORDERS_LIMIT);
        if let Some(s) = symbol {
            h = h.query("product_ids", s);
        }
        h
    }

    /// The `order_configuration` of `o`: its one key and its fields (Create Order).
    fn configuration(&self, o: &OrderRequest) -> Result<Value, ConnectorError> {
        if o.opts.reduce_only {
            return Err(ConnectorError::unsupported("Coinbase spot takes no reduce-only order: a sale of what is held is a plain sell"));
        }
        let size = dec_text(o.qty.value());
        let limit = || o.limit.map(|p| dec_text(p.value())).ok_or_else(|| ConnectorError::invalid("this order needs its limit price"));
        let end = |t: Timestamp| rfc3339(t);
        let (key, mut fields) = match (o.order_type, o.tif) {
            (OrderType::Market, TimeInForce::Gtc | TimeInForce::Ioc) => ("market_market_ioc", json!({ "base_size": size })),
            (OrderType::Limit, TimeInForce::Gtc) => ("limit_limit_gtc", json!({ "base_size": size, "limit_price": limit()? })),
            (OrderType::Limit, TimeInForce::Gtd(t)) => ("limit_limit_gtd", json!({ "base_size": size, "limit_price": limit()?, "end_time": end(t)? })),
            (OrderType::Limit, TimeInForce::Ioc) => ("sor_limit_ioc", json!({ "base_size": size, "limit_price": limit()? })),
            (OrderType::Limit, TimeInForce::Fok) => ("limit_limit_fok", json!({ "base_size": size, "limit_price": limit()? })),
            (OrderType::StopLimit | OrderType::Lit, TimeInForce::Gtc | TimeInForce::Gtd(_)) => {
                let stop = o.stop.map(|p| dec_text(p.value())).ok_or_else(|| ConnectorError::invalid("this order needs its stop price"))?;
                // "If Up, then the Order will trigger when the last trade price goes above the
                // stop_price. If Down, ... below": a buy stop waits for a rise, a sell stop for a
                // fall; a limit-if-touched the other way.
                let up = (o.side == Side::Buy) == (o.order_type == OrderType::StopLimit);
                let direction = if up { "STOP_DIRECTION_STOP_UP" } else { "STOP_DIRECTION_STOP_DOWN" };
                let mut f = json!({ "base_size": size, "limit_price": limit()?, "stop_price": stop, "stop_direction": direction });
                match o.tif {
                    TimeInForce::Gtd(t) => {
                        f["end_time"] = json!(end(t)?);
                        ("stop_limit_stop_limit_gtd", f)
                    }
                    _ => ("stop_limit_stop_limit_gtc", f),
                }
            }
            (t, tif) => return Err(ConnectorError::unsupported(format!("Coinbase takes no {t:?} order {:?}", tif.kind()))),
        };
        if o.opts.post_only {
            if !matches!(key, "limit_limit_gtc" | "limit_limit_gtd") {
                return Err(ConnectorError::unsupported("Coinbase takes post-only on a good-till-cancelled or good-till-date limit order only"));
            }
            fields["post_only"] = json!(true);
        }
        let mut cfg = Map::new();
        cfg.insert(key.into(), fields);
        Ok(Value::Object(cfg))
    }

    /// The Create Order body; a preview's carries no `client_order_id` (Preview Orders takes
    /// none: the sandbox answers "unknown field \"client_order_id\"").
    fn order_body(&self, o: &OrderRequest, client_id: Option<&str>) -> Result<Value, ConnectorError> {
        let mut b = Map::new();
        if let Some(cid) = client_id {
            b.insert("client_order_id".into(), json!(cid));
        }
        b.insert("product_id".into(), json!(o.instrument.native));
        b.insert("side".into(), json!(side_word(o.side)));
        b.insert("order_configuration".into(), self.configuration(o)?);
        Ok(Value::Object(b))
    }

    fn order_row(&self, r: OrderRow) -> Result<VenueOrder, ConnectorError> {
        let cfg = Config::of(r.order_configuration.as_ref())?;
        let filled = r.filled_size.0.unwrap_or_default();
        let (qty, order_type) = base_size(&r.order_type, &cfg, r.size_in_quote, filled);
        let order_type = with_attached(order_type, r.attached_order_configuration.as_ref())?;
        let mut status = self.status_of(&r.status);
        // OPEN with a fill is partly filled; OPEN with a cancel requested is pending its cancel.
        if status == Mapped::Known(OrderStatus::New) && r.status == "OPEN" {
            if r.pending_cancel {
                status = Mapped::Known(OrderStatus::PendingCancel);
            } else if filled > Decimal::ZERO {
                status = Mapped::Known(OrderStatus::PartiallyFilled);
            }
        }
        Ok(VenueOrder {
            venue_id: r.order_id,
            client_id: Some(r.client_order_id).filter(|c| !c.is_empty()),
            symbol: r.product_id.to_ascii_uppercase(),
            side: side_of(&r.side),
            order_type,
            status,
            qty,
            filled_qty: filled,
            limit: positive(&cfg.limit_price),
            stop: cfg.stop(),
            avg_price: positive(&r.average_filled_price),
            reduce_only: false,
            position: PositionSide::Net,
            created_ms: r.created_time.as_deref().and_then(millis),
            // "Trigger status for stop orders": waiting until the last trade price crosses.
            trigger_pending: r.trigger_status == "STOP_PENDING",
            parent_client_id: None,
            exit: None,
            successor_id: None,
        })
    }

    /// `trade_type` "Regular fills have the value `FILL`. Adjusted fills have possible values
    /// `REVERSAL`, `CORRECTION`, `SYNTHETIC`": the doc does not say how an adjusted row's size and
    /// sign undo or replace the fill it adjusts, so one is never read as a fill of its own
    /// ([`Coinbase::decode_fills`] leaves it out, says so, and keeps the others).
    fn fill_row(r: FillRow) -> Result<VenueFill, ConnectorError> {
        if !r.trade_type.is_empty() && r.trade_type != "FILL" {
            return Err(ConnectorError::decode(format!(
                "Coinbase listed an adjusted fill ({} {} of order {}), which OpenTraderWorld does not read: check the order at Coinbase",
                r.trade_type, r.entry_id, r.order_id
            )));
        }
        let exec_id = if r.entry_id.is_empty() { r.trade_id } else { r.entry_id };
        if exec_id.is_empty() {
            return Err(ConnectorError::decode(format!("a Coinbase fill of order {} carries no entry_id", r.order_id)));
        }
        let price = r.price.0.unwrap_or_default();
        let size = r.size.0.unwrap_or_default();
        // As the account read does: a size placed in the quote currency is money, not coins.
        let qty = if r.size_in_quote {
            size.checked_div(price).filter(|_| !price.is_zero()).ok_or_else(|| ConnectorError::decode(format!("Coinbase fill {exec_id}: a quote size at price {price} cannot be read in the coin")))?
        } else {
            size
        };
        let at_ms = millis(&r.trade_time).ok_or_else(|| ConnectorError::decode(format!("Coinbase fill {exec_id}: no readable trade_time")))?;
        Ok(VenueFill {
            exec_id,
            venue_order_id: r.order_id,
            client_id: None,
            symbol: r.product_id.to_ascii_uppercase(),
            side: side_of(&r.side),
            qty,
            price,
            // "Fee amount for fill": its sign is doc silent, so it is taken as stated (positive a
            // cost); a rebate written negative stays one.
            fee: r.commission.0.unwrap_or_default(),
            // No currency on the row (doc silent): fees are charged in the quote currency, so
            // the trader names it from the product list ([`TradeMapper::fees_in_quote`]), as the
            // account read does.
            fee_currency: String::new(),
            at_ms,
            position_only: false,
            extra_fees: Vec::new(),
            fee_pending: false,
        })
    }

    /// A product's rules. The status words are doc silent (`status` "Status of the product.");
    /// `online` is the one seen trading, any other word is not trading. The flags are the doc's:
    /// `cancel_only` "can only be cancelled, not placed or edited", `limit_only` "can only be
    /// limit orders", `post_only` "can only be posted", `trading_disabled` "disabled for trading
    /// for all market participants", `is_disabled` "disabled for trading", `view_only`.
    fn rule_row(r: ProductRow) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(VENUE, &r.product_id.to_ascii_uppercase()));
        v.tick = positive(&r.price_increment);
        v.lot_step = positive(&r.base_increment);
        v.quote_step = positive(&r.quote_increment);
        v.min_qty = positive(&r.base_min_size);
        v.max_qty = positive(&r.base_max_size);
        // `quote_min_size` "Minimum size that can be represented of quote currency", worded as
        // `base_min_size` is: the smallest order value (`PREVIEW_INVALID_QUOTE_SIZE_TOO_SMALL`).
        v.min_notional = positive(&r.quote_min_size);
        v.status = if r.trading_disabled {
            TradingStatus::Closed("trading_disabled".into())
        } else if r.is_disabled {
            TradingStatus::Closed("is_disabled".into())
        } else if r.view_only {
            TradingStatus::Closed("view_only".into())
        } else if r.status != "online" {
            TradingStatus::Closed(if r.status.is_empty() { "no status".into() } else { r.status })
        } else if r.cancel_only {
            TradingStatus::Closed("cancel_only".into())
        } else if r.post_only {
            TradingStatus::PostOnly("post_only".into())
        } else if r.limit_only {
            TradingStatus::LimitOnly("limit_only".into())
        } else {
            TradingStatus::Trading
        };
        v
    }
}

impl TradeMapper for Coinbase {
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(36)
    }

    fn place(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if p.client_id.is_empty() {
            return Err(ConnectorError::invalid("a Coinbase order needs its client_order_id"));
        }
        Self::post(CREATE, "/orders", &self.order_body(o, Some(&p.client_id))?, p.corr)
    }

    /// Edit Order: `order_id`, `price` and `size`, all three required; only a limit order
    /// (`ONLY_LIMIT_ORDER_EDITS_SUPPORTED`), never one placed in the quote currency
    /// (`SIZE_IN_QUOTE_EDITS_NOT_ALLOWED`). `size` is the order's whole size: whether it is the
    /// total or what remains is doc silent, and `CANNOT_EDIT_TO_BELOW_FILLED_SIZE` reads as the
    /// total. A stop price or time in force is not changed here.
    fn amend(&self, _settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if change.stop.is_some() || change.tif.is_some() {
            return Err(ConnectorError::unsupported("Coinbase changes the price and size of a limit order only"));
        }
        // The type word before what an attached exit adds to it.
        if cur.order_type.split(ATTACHED).next() != Some("LIMIT") {
            return Err(ConnectorError::unsupported(format!("Coinbase changes a limit order sized in the coin only; this one is {}", cur.order_type)));
        }
        let price = change.limit.map(|p| p.value()).or(cur.limit).ok_or_else(|| ConnectorError::invalid("the order's limit price is not known"))?;
        let size = change.qty.map(|q| q.value()).unwrap_or(cur.qty);
        let body = json!({ "order_id": path_id(&cur.venue_id)?, "price": dec_text(price), "size": dec_text(size) });
        Self::post(EDIT, "/orders/edit", &body, p.corr)
    }

    /// Cancel Orders with one id: "Initiate cancel requests for one or more orders".
    fn cancel(&self, _settings: &Settings, _symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Self::post(CANCEL, "/orders/batch_cancel", &json!({ "order_ids": [path_id(venue_id)?] }), corr)
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::ByIds { max: CANCEL_BATCH_MAX }
    }

    fn cancel_ids(&self, _settings: &Settings, orders: &[VenueOrder], corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if orders.is_empty() || orders.len() > CANCEL_BATCH_MAX {
            return Err(ConnectorError::invalid(format!("OpenTraderWorld cancels 1 to {CANCEL_BATCH_MAX} Coinbase orders per batch ({} given)", orders.len())));
        }
        let ids = orders.iter().map(|o| path_id(&o.venue_id).map(str::to_string)).collect::<Result<Vec<_>, _>>()?;
        Self::post(CANCEL_BATCH, "/orders/batch_cancel", &json!({ "order_ids": ids }), corr)
    }

    /// `results[]` of `{success, failure_reason, order_id}`, one per id.
    fn decode_cancel_rows(&self, _ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("batch_cancel", e))?;
        let rows = v.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(rows
            .iter()
            .map(|r| CancelRow { venue_id: r.get("order_id").and_then(Value::as_str).unwrap_or_default().to_string(), error: super::trade_errors::cancel_row(r) })
            .collect())
    }

    /// No call takes a client id (Get Order's `client_order_id` query is "(Deprecated)", and the
    /// path still wants the venue's id; List Orders has no client id filter): the product's
    /// orders, newest first as far as the doc goes ("Default is to sort by creation time"),
    /// matched by `client_order_id`.
    fn query_by_client_id(&self, _settings: &Settings, symbol: &str, _client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![Self::list("orders_lookup", Some(symbol), corr)])
    }

    /// The same list from a minute before the order was sent (`start_date`, "orders created
    /// after this date", inclusive), when that time is held.
    fn query_by_client_id_at(&self, settings: &Settings, symbol: &str, client_id: &str, at: &LookupAt, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut reqs = self.query_by_client_id(settings, symbol, client_id, corr)?;
        if let Some(t) = at.created_ms.and_then(|c| Timestamp::from_millis(c.saturating_sub(LOOKUP_SLACK_MS))) {
            let since = rfc3339(t)?;
            reqs = reqs.into_iter().map(|r| r.query("start_date", since.clone())).collect();
        }
        Ok(reqs)
    }

    fn lookup_pages(&self) -> usize {
        LOOKUP_PAGES
    }

    fn query_by_venue_id(&self, _settings: &Settings, _symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![Self::get(GET_ORDER, &format!("/orders/historical/{}", path_id(venue_id)?), corr)])
    }

    /// Every working status ([`WORKING`]), once per placement source ([`SOURCES`]: only
    /// `RETAIL_ADVANCED` is listed unless another is asked), every page by `cursor`.
    fn open_orders(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(SOURCES
            .iter()
            .map(|src| WORKING.iter().fold(Self::list("orders_open", symbol, corr), |h, st| h.query("order_status", *st)).query("order_placement_source", *src))
            .collect())
    }

    /// `product_types=SPOT`: "By default, returns all product types", so a futures fill of the
    /// same account would land in the spot book otherwise.
    fn fills_since(&self, _settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let at = |ms: i64| Timestamp::from_millis(ms).ok_or_else(|| ConnectorError::invalid(format!("time {ms} out of range"))).and_then(rfc3339);
        Ok(Self::get("fills", "/orders/historical/fills", corr)
            .query("product_ids", symbol)
            .query("product_types", "SPOT")
            .query("start_sequence_timestamp", at(from_ms)?)
            .query("end_sequence_timestamp", at(to_ms)?)
            .query("limit", FILLS_LIMIT))
    }

    /// The public product list ("cached for 1 second"), spot only: the same rules the signed
    /// `/products` answers, with no key.
    fn instrument_rules(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let mut h = HttpRequest::get("trade_products", BROKERAGE, format!("{PREFIX}/market/products"), corr).query("product_type", "SPOT");
        if let Some(s) = symbol {
            h = h.query("product_ids", s);
        }
        Ok(h)
    }

    /// What the order configurations cannot say: a market order fill-or-kill
    /// (`market_market_fok` is "(Perpetuals only)") or good-till-date, a stop-limit
    /// immediate-or-cancel or fill-or-kill (its configurations are GTC and GTD only).
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        let no = |m: &str| Err(ConnectorError::unsupported(m.to_string()));
        match (o.order_type, o.tif) {
            (OrderType::Market, TimeInForce::Fok) => no("Coinbase spot works a market order immediate-or-cancel, never fill-or-kill: send it IOC, or as a limit order FOK"),
            (OrderType::Market, TimeInForce::Gtd(_)) => no("Coinbase takes no good-till-date market order"),
            (OrderType::StopLimit | OrderType::Lit, TimeInForce::Ioc | TimeInForce::Fok) => no("Coinbase works a stop-limit good-till-cancelled or good-till-date only"),
            _ => Ok(()),
        }
    }

    /// Preview Orders: the order's own body, no client id, answered with `errs[]` ("potential
    /// failure reasons were this order to be submitted") read as a refusal by `decode_error`.
    fn dry_run(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<Option<HttpRequest>, ConnectorError> {
        Self::post(PREVIEW, "/orders/preview", &self.order_body(o, None)?, p.corr).map(Some)
    }

    fn decode_dry_run(&self, body: &[u8]) -> Result<String, ConnectorError> {
        let p: Preview = serde_json::from_slice(body).map_err(|e| decode_err("preview", e))?;
        let n = |d: &OptDec| d.0.map(dec_text).unwrap_or_else(|| "?".into());
        // "UNKNOWN ... can therefore be seen as no warning" (FAQ).
        let warnings: Vec<&str> = p.warning.iter().map(String::as_str).filter(|w| !w.is_empty() && *w != "UNKNOWN").collect();
        let mut s = format!("order total {}, commission {}, best bid {}, best ask {}", n(&p.order_total), n(&p.commission_total), n(&p.best_bid), n(&p.best_ask));
        if !warnings.is_empty() {
            s.push_str(&format!(", warnings {}", warnings.join(", ")));
        }
        Ok(s)
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    /// `success_response.order_id` (the FAQ's example repeats it at the top as `order_id`).
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let a: CreateAnswer = serde_json::from_slice(body).map_err(|e| decode_err("create order", e))?;
        if !a.success {
            return Err(ConnectorError::decode("coinbase create order: success is not true"));
        }
        let (id, cid) = match a.success_response {
            Some(s) if !s.order_id.is_empty() => (s.order_id, s.client_order_id),
            _ => (a.order_id.unwrap_or_default(), None),
        };
        if id.is_empty() {
            return Ok(Ack::without_id(None));
        }
        Ok(Ack { venue_id: id, client_id: cid.filter(|c| !c.is_empty()) })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let a: Orders = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
        if a.proof_token_required {
            return Err(proof_token_refused("orders"));
        }
        let next = (a.has_next && !a.cursor.is_empty()).then_some(a.cursor);
        let rows = a.orders.into_iter().chain(a.order).map(|r| self.order_row(r)).collect::<Result<Vec<_>, _>>()?;
        Ok((rows, next))
    }

    /// The next page only when this one was full: List Fills answers a `cursor` and no
    /// `has_next`.
    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let a: Fills = serde_json::from_slice(body).map_err(|e| decode_err("fills", e))?;
        if a.proof_token_required {
            return Err(proof_token_refused("fills"));
        }
        let full = a.fills.len() >= FILLS_LIMIT;
        let rows = a
            .fills
            .into_iter()
            .filter(|r| {
                let adjusted = !r.trade_type.is_empty() && r.trade_type != "FILL";
                if adjusted {
                    tracing::error!(
                        "Coinbase listed an adjusted fill ({} {} of order {}), which OpenTraderWorld does not read: it is left out, check the order at Coinbase",
                        r.trade_type,
                        r.entry_id,
                        r.order_id
                    );
                }
                !adjusted
            })
            .map(Self::fill_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, (full && !a.cursor.is_empty()).then_some(a.cursor)))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let a: Products = serde_json::from_slice(body).map_err(|e| decode_err("market/products", e))?;
        let next = a.pagination.filter(|p| p.has_next && !p.next_cursor.is_empty()).map(|p| p.next_cursor);
        let rows = a.products.into_iter().filter(|p| p.product_type.is_empty() || p.product_type == "SPOT").map(Self::rule_row).collect();
        Ok((rows, next))
    }

    /// `OrderExecutionStatus`: `PENDING` ("not yet open") and `QUEUED` are pending new; `OPEN` is
    /// working (partly filled when `filled_size` says so, read in `order_row`); `CANCEL_QUEUED`
    /// pending its cancel, `EDIT_QUEUED` its change; `FAILED` ("could not be placed") rejected.
    /// `UNKNOWN_ORDER_STATUS` and every other word stay unknown.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "PENDING" | "QUEUED" => OrderStatus::PendingNew,
            "OPEN" => OrderStatus::New,
            "FILLED" => OrderStatus::Filled,
            "CANCELLED" => OrderStatus::Cancelled,
            "EXPIRED" => OrderStatus::Expired,
            "FAILED" => OrderStatus::Rejected,
            "CANCEL_QUEUED" => OrderStatus::PendingCancel,
            "EDIT_QUEUED" => OrderStatus::PendingReplace,
            other => return otw_connector_sdk::enums::unknown("coinbase", "order status", other),
        })
    }

    /// The User Order Data endpoint: `user` for every product first (`product_ids` omitted),
    /// then `heartbeats` (it keeps the socket open: "Most channels close within 60-90 seconds
    /// when no updates arrive"). `user` goes first so that no heartbeat, which comes every
    /// second once subscribed, can be taken for its answer. Each frame gets its JWT from the
    /// signer. No client heartbeat is documented: none is sent.
    fn private_socket(&self, _settings: &Settings) -> Option<PrivateSocket> {
        let mut s = PrivateSocket::new(WS_USER, "", json!({ "type": "subscribe", "channel": "user" }).to_string().into_bytes(), Vec::new(), SILENCE_MS);
        s.more.push(json!({ "type": "subscribe", "channel": "heartbeats" }).to_string().into_bytes());
        Some(s)
    }

    /// The `user` subscribe is answered by the channel's own `snapshot` or by a `subscriptions`
    /// frame that lists `user`; the `heartbeats` subscribe after it by a heartbeat. A
    /// `subscriptions` frame without `user` answers nothing, and an `error` frame refuses.
    /// A `user` update names orders whose remaining size is "in the same currency the order was
    /// placed in (quote or base)" without saying which: each is read again by its id. The
    /// snapshot (the open orders at connect) is the book the account already read.
    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let v: Value = serde_json::from_slice(payload).map_err(|e| decode_err("user frame", e))?;
        if v.get("type").and_then(Value::as_str) == Some("error") {
            let message = v.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
            return Ok(vec![PrivateEvent::Answer { op: "subscribe".into(), ok: false, message }]);
        }
        let events = || v.get("events").and_then(Value::as_array).into_iter().flatten();
        let subscribed = || PrivateEvent::Answer { op: "subscribe".into(), ok: true, message: String::new() };
        match v.get("channel").and_then(Value::as_str).unwrap_or_default() {
            "heartbeats" => Ok(vec![subscribed()]),
            "subscriptions" => Ok(events().any(|e| e.get("subscriptions").and_then(|s| s.get("user")).is_some()).then(subscribed).into_iter().collect()),
            "user" => {
                let mut out = Vec::new();
                if events().any(|e| e.get("type").and_then(Value::as_str) == Some("snapshot")) {
                    out.push(subscribed());
                }
                for ev in events() {
                    if ev.get("type").and_then(Value::as_str) != Some("update") {
                        continue;
                    }
                    for o in ev.get("orders").and_then(Value::as_array).into_iter().flatten() {
                        let venue_id = o.get("order_id").and_then(Value::as_str).unwrap_or_default();
                        if venue_id.is_empty() {
                            continue;
                        }
                        let symbol = o.get("product_id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_ascii_uppercase);
                        out.push(PrivateEvent::OrderChanged { symbol, venue_id: venue_id.to_string() });
                    }
                }
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// `sequence_num`, "Per-connection message sequence number; use it to detect dropped or
    /// out-of-order messages" (the channel pages): one count over every channel of the socket.
    fn private_sequence(&self, payload: &[u8]) -> Option<u64> {
        #[derive(Deserialize)]
        struct Numbered {
            sequence_num: Option<u64>,
        }
        serde_json::from_slice::<Numbered>(payload).ok().and_then(|n| n.sequence_num)
    }

    /// Advanced Trade charges its fees in the quote currency of the product; a fill row names
    /// no currency, so the trader takes the product's quote.
    fn fees_in_quote(&self) -> bool {
        true
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        super::trade_errors::ERRORS
    }
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod tests;
