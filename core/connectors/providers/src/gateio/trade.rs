//! Gate.io API v4 orders (spot and USDT perpetuals), written from the documentation alone: no
//! account, no testnet key. Every field below is the doc's; each claim is sourced in
//! PROVIDER.md ("Trading"). The order path itself is the shared trader's; this file only says
//! what the calls are and what the answers mean.
//!
//! - Place: spot `POST /spot/orders` (`type` limit or market, `amount`, `price`,
//!   `time_in_force`); futures `POST /futures/usdt/orders` (`size` in whole contracts, its sign
//!   the side, `price` "0" with `tif` ioc for a market order, `reduce_only`).
//! - Client id: `text`, `t-` then at most 28 letters, digits, `_`, `-` and `.`: the OTW key
//!   behind the prefix.
//! - A spot market buy is an amount to spend in the quote coin (`market_buy_in_quote`): the
//!   trader values it at the best ask read just before sending.
//! - Futures sizes are contracts of `quanto_multiplier` base coins (`counts_contracts`).
//! - Position mode: the futures account's `position_mode`, read only, never switched.
//! - Private streams: one socket per book, every subscribe frame signed on its own by the
//!   signer; the futures channels name the user id, read from `/account/detail` before each
//!   connection.

#![deny(clippy::float_arithmetic)]

use std::str::FromStr;

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, TimeInForce};
use otw_connector_sdk::parse::{Int, OptDec};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, CancelRow, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, FeeLine, MarginMode, OrderSpace, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Settings,
    TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};

use super::{Book, Gateio, API, FX_WS, REST, REST_TESTNET, SETTLE, WS, WS_TESTNET};

/// Rows per list call: the default `limit` of every list endpoint used here.
const PAGE: usize = 100;
/// Rows per spot fills page: the `limit` maximum of `GET /spot/my_trades` ("maximum: 1000").
const SPOT_FILL_PAGE: usize = 1000;
/// The spot fills window. The range is "not allowed to exceed 30 days", and paging stops at
/// `limit * (page - 1) <= 1000` (SDK `list_my_trades`): two pages of 1000 per window at most.
/// One day per window keeps a busy pair under that depth, and well inside the range once
/// `from` and `to` are rounded out to whole seconds.
const SPOT_FILLS_WINDOW_MS: i64 = 86_400_000;
/// The private stream's `*.ping` interval. The pages state none; their demos ping every 10 s.
const PING_EVERY_MS: u64 = 10_000;
/// The client id's prefix ("prefixed with `t-`").
const CID_PREFIX: &str = "t-";
/// The user id the futures channels name before the bootstrap read fills it in.
const NO_USER: &str = "0";
/// The `text` of a futures order Gate.io placed itself (forced liquidation, deleveraging,
/// insurance), as the `FuturesOrder.text` list names them: its fills moved a position with no
/// order of the account behind them.
const SYSTEM_TEXTS: &[&str] = &["auto_deleveraging", "liquidation", "pm_liquidate", "comb_margin_liquidate", "scm_liquidate", "insurance"];

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("gateio {what}: {e}"))
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

/// A positive value, or none: Gate.io writes "0" for a price an order does not have.
fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

/// Whole milliseconds of a time written in milliseconds, possibly with a fraction.
fn whole_ms(d: Decimal) -> Option<i64> {
    d.trunc().normalize().to_string().parse().ok()
}

/// Milliseconds of a time written in seconds with a fraction (`1546569968.184`).
fn ms_of_secs(d: Decimal) -> Option<i64> {
    whole_ms(d * Decimal::ONE_THOUSAND)
}

/// A whole number of contracts as the JSON number Gate.io reads `size` as.
fn size_number(n: Decimal) -> Result<Value, ConnectorError> {
    serde_json::Number::from_str(&dec_text(n)).map(Value::Number).map_err(|e| ConnectorError::invalid(format!("gateio size {n}: {e}")))
}

fn side_word(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn side_of(word: &str) -> Mapped<Side> {
    match word {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

/// A futures size: positive a buy (or a long), negative a sell (or a short).
fn side_of_size(size: Decimal) -> Mapped<Side> {
    if size > Decimal::ZERO {
        Mapped::Known(Side::Buy)
    } else if size < Decimal::ZERO {
        Mapped::Known(Side::Sell)
    } else {
        // A close order (`size` 0, `close` or `auto_size`): its side is the position's.
        Mapped::Unknown("close".into())
    }
}

/// The client id OTW sent, as the venue echoes it; any other `text` is the order's source
/// (`web`, `api`, `app`, `apiv4`...), not a client id.
fn client_text(text: &str) -> Option<String> {
    Some(text.to_string()).filter(|t| t.starts_with(CID_PREFIX) && t.len() > CID_PREFIX.len())
}

fn next_if_full(n: usize) -> Option<String> {
    (n >= PAGE).then(|| "next".to_string())
}

/// The account's environment: the `site` setting, `live` (or empty) or `testnet`.
fn testnet(s: &Settings) -> Result<bool, ConnectorError> {
    match s.get("site").map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default().as_str() {
        "" | "live" => Ok(false),
        "testnet" => Ok(true),
        other => Err(ConnectorError::invalid(format!("Gate.io site \"{other}\" is not live or testnet (empty = live)"))),
    }
}

#[derive(Deserialize)]
struct SpotOrder {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    currency_pair: String,
    /// REST only: `open`, `closed`, `cancelled`.
    #[serde(default)]
    status: String,
    /// The stream only: `put`, `update`, `finish`.
    #[serde(default)]
    event: String,
    #[serde(default)]
    finish_as: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    filled_amount: OptDec,
    #[serde(default)]
    avg_deal_price: OptDec,
    #[serde(default)]
    create_time_ms: OptDec,
}

#[derive(Deserialize)]
struct FuturesOrder {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    contract: String,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    left: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fill_price: OptDec,
    /// `open`, `finished`.
    #[serde(default)]
    status: String,
    #[serde(default)]
    finish_as: String,
    #[serde(default)]
    is_reduce_only: bool,
    #[serde(default)]
    is_close: bool,
    #[serde(default)]
    create_time: OptDec,
    #[serde(default)]
    create_time_ms: OptDec,
}

/// A spot open-orders group (`GET /spot/open_orders`): one pair's working orders.
#[derive(Deserialize)]
struct OpenGroup {
    #[serde(default)]
    orders: Vec<SpotOrder>,
}

#[derive(Deserialize)]
struct SpotTrade {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(deserialize_with = "id_text", default)]
    order_id: String,
    #[serde(default)]
    currency_pair: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    fee_currency: String,
    #[serde(default)]
    gt_fee: OptDec,
    #[serde(default)]
    point_fee: OptDec,
    #[serde(default)]
    text: String,
    #[serde(default)]
    create_time_ms: OptDec,
    #[serde(default)]
    create_time: OptDec,
}

/// A futures fill: `trade_id` over REST, `id` on the stream.
#[derive(Deserialize)]
struct FuturesTrade {
    #[serde(alias = "id", deserialize_with = "id_text", default)]
    trade_id: String,
    #[serde(deserialize_with = "id_text", default)]
    order_id: String,
    #[serde(default)]
    contract: String,
    #[serde(default)]
    size: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    point_fee: OptDec,
    #[serde(default)]
    text: String,
    #[serde(default)]
    create_time: OptDec,
    #[serde(default)]
    create_time_ms: OptDec,
}

#[derive(Deserialize)]
struct PairRow {
    id: String,
    #[serde(default)]
    min_base_amount: OptDec,
    #[serde(default)]
    max_base_amount: OptDec,
    #[serde(default)]
    min_quote_amount: OptDec,
    #[serde(default)]
    max_quote_amount: OptDec,
    #[serde(default)]
    amount_precision: Option<Int>,
    #[serde(default)]
    precision: Option<Int>,
    #[serde(default)]
    trade_status: String,
    #[serde(default)]
    market_order_max_stock: OptDec,
}

#[derive(Deserialize)]
struct ContractRow {
    name: String,
    #[serde(default)]
    quanto_multiplier: OptDec,
    #[serde(default)]
    order_price_round: OptDec,
    #[serde(default)]
    order_size_min: OptDec,
    #[serde(default)]
    order_size_max: OptDec,
    #[serde(default)]
    market_order_size_max: OptDec,
    #[serde(default)]
    status: String,
    #[serde(default)]
    in_delisting: bool,
    /// "Current total long position size": with `in_delisting`, above 0 the delisting
    /// transition, 0 delisted.
    #[serde(default)]
    position_size: OptDec,
}

#[derive(Deserialize)]
struct PositionRow {
    #[serde(default)]
    contract: String,
    #[serde(default)]
    size: OptDec,
    /// "0 means cross margin; positive number means isolated margin".
    #[serde(default)]
    leverage: OptDec,
    #[serde(default)]
    cross_leverage_limit: OptDec,
    /// `single`, `dual_long`, `dual_short`.
    #[serde(default)]
    mode: String,
    /// `cross` or `isolated`, where the row carries it.
    #[serde(default)]
    pos_margin_mode: String,
}

#[derive(Deserialize)]
struct Ticker {
    #[serde(default)]
    lowest_ask: OptDec,
    #[serde(default)]
    highest_bid: OptDec,
}

#[derive(Deserialize)]
struct OrderCancel {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(default)]
    succeeded: Option<bool>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    message: String,
}

/// `SpotPriceTriggeredOrder` (official SDK): a spot price-triggered order.
#[derive(Deserialize)]
struct SpotPriceOrder {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    #[serde(default)]
    market: String,
    /// `open`, `cancelled`, `finish`, `failed`, `expired`.
    #[serde(default)]
    status: String,
    #[serde(default)]
    trigger: PriceTrigger,
    #[serde(default)]
    put: SpotPut,
    /// Seconds.
    #[serde(default)]
    ctime: OptDec,
}

#[derive(Deserialize, Default)]
struct PriceTrigger {
    #[serde(default)]
    price: OptDec,
}

/// `SpotPricePutOrder`: the order placed once triggered.
#[derive(Deserialize, Default)]
struct SpotPut {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    amount: OptDec,
    #[serde(default)]
    text: String,
}

/// `FuturesPriceTriggeredOrder` (official SDK): a futures price-triggered order.
#[derive(Deserialize)]
struct FuturesPriceOrder {
    #[serde(deserialize_with = "id_text", default)]
    id: String,
    /// `open`, `finished`, `inactive`, `invalid`.
    #[serde(default)]
    status: String,
    /// `cancelled`, `succeeded`, `failed`, `expired`.
    #[serde(default)]
    finish_as: String,
    #[serde(default)]
    initial: FuturesInitial,
    #[serde(default)]
    trigger: PriceTrigger,
    /// Seconds, with a fraction.
    #[serde(default)]
    create_time: OptDec,
    /// `close-long-order`, `close-short-order`, `close-long-position`, `close-short-position`,
    /// `plan-close-long-position`, `plan-close-short-position`.
    #[serde(default)]
    order_type: String,
}

/// `FuturesInitialOrder`: the order placed once triggered.
#[derive(Deserialize, Default)]
struct FuturesInitial {
    #[serde(default)]
    contract: String,
    /// Contracts, the sign the side; `0` closes the whole position.
    #[serde(default)]
    size: OptDec,
    /// `0`: at the market.
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    text: String,
    #[serde(default)]
    is_reduce_only: bool,
    #[serde(default)]
    is_close: bool,
    /// Dual mode, whole position: `close_long` or `close_short`.
    #[serde(default)]
    auto_size: String,
}

/// `GET /unified/unified_mode`: `classic` or a unified mode.
#[derive(Deserialize)]
struct UnifiedMode {
    #[serde(default)]
    mode: String,
}

/// A price-triggered order's state. Spot `status` `open`, `cancelled`, `finish` ("Successfully
/// completed": it placed its order), `failed`, `expired`; futures `status` `open`, `inactive`
/// (an order take profit or stop loss whose order has not filled yet), then `finished` with
/// `finish_as` `cancelled`, `succeeded`, `failed`, `expired`. A fired one is Filled: what traded
/// is on the order it placed, not on it. `invalid` and any other word stay unknown.
fn price_status(word: &str) -> Mapped<OrderStatus> {
    Mapped::Known(match word {
        "open" | "inactive" => OrderStatus::New,
        "cancelled" => OrderStatus::Cancelled,
        "finish" | "succeeded" => OrderStatus::Filled,
        "failed" => OrderStatus::Rejected,
        "expired" => OrderStatus::Expired,
        other => return Mapped::Unknown(other.to_string()),
    })
}

/// A frame of a private socket: a notification, an answer to a subscribe, a pong.
#[derive(Deserialize)]
struct Push {
    #[serde(default)]
    channel: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    result: Value,
}

/// `10^-places`, the step a precision in decimal places stands for.
fn step_of(places: Option<Int>) -> Option<Decimal> {
    let p = u32::try_from(places?.0).ok().filter(|p| *p <= 28)?;
    Some(Decimal::new(1, p))
}

impl Gateio {
    fn base(&self, s: &Settings) -> Result<&'static str, ConnectorError> {
        Ok(if testnet(s)? { REST_TESTNET } else { REST })
    }

    fn call(&self, endpoint: &'static str, method: Method, s: &Settings, path: String, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let mut h = HttpRequest::get(endpoint, self.base(s)?, format!("{API}{path}"), corr).signed();
        h.method = method;
        Ok(h)
    }

    fn with_body(mut h: HttpRequest, body: &Value) -> Result<HttpRequest, ConnectorError> {
        let bytes = serde_json::to_vec(body).map_err(|e| ConnectorError::invalid(format!("gateio {}: {e}", h.endpoint_id)))?;
        h.body = Some(bytes.into());
        Ok(h)
    }

    fn is_futures(&self) -> bool {
        self.book == Book::Futures
    }

    fn venue_label(&self) -> &str {
        &self.caps.label
    }

    /// The `time_in_force` / `tif` word. A market order works immediate-or-cancel ("Only `ioc`
    /// and `fok` are supported when `type`=`market`"; futures: "Price of 0 with `tif` set to
    /// `ioc` represents a market order"): asked GTC or IOC it goes `ioc`.
    fn tif_word(&self, o: &OrderRequest) -> Result<&'static str, ConnectorError> {
        let market = o.order_type == OrderType::Market;
        Ok(match o.tif {
            TimeInForce::Gtc if market => "ioc",
            TimeInForce::Gtc => "gtc",
            TimeInForce::Ioc => "ioc",
            TimeInForce::Fok if market && self.is_futures() => {
                return Err(ConnectorError::unsupported(format!("{} works a market order immediate-or-cancel only: send it IOC, or as a limit order FOK", self.venue_label())))
            }
            TimeInForce::Fok => "fok",
            other => return Err(ConnectorError::unsupported(format!("{} takes no {:?} time in force", self.venue_label(), other.kind()))),
        })
    }

    fn spot_body(&self, o: &OrderRequest, p: &Placing) -> Result<Value, ConnectorError> {
        if o.opts.reduce_only {
            return Err(ConnectorError::unsupported(format!("{} takes no reduce-only order: a spot sale of what is held is a plain sell", self.venue_label())));
        }
        let mut b = Map::new();
        b.insert("currency_pair".into(), json!(o.instrument.native));
        b.insert("side".into(), json!(side_word(o.side)));
        match o.order_type {
            OrderType::Limit => {
                let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
                b.insert("type".into(), json!("limit"));
                b.insert("amount".into(), json!(dec_text(o.qty.value())));
                b.insert("price".into(), json!(dec_text(px.value())));
            }
            OrderType::Market => {
                b.insert("type".into(), json!("market"));
                // A market buy's `amount` is the quote coin to spend; a sell's the base sold.
                let amount = match o.side {
                    Side::Buy => p.quote_amount.ok_or_else(|| {
                        ConnectorError::invalid(format!("{} takes a market buy as an amount to spend, and none was set: nothing was sent", self.venue_label()))
                    })?,
                    Side::Sell => o.qty.value(),
                };
                b.insert("amount".into(), json!(dec_text(amount)));
            }
            other => return Err(ConnectorError::unsupported(format!("{} takes no {other:?} order", self.venue_label()))),
        }
        b.insert("time_in_force".into(), json!(self.tif_word(o)?));
        if !p.client_id.is_empty() {
            b.insert("text".into(), json!(p.client_id));
        }
        Ok(Value::Object(b))
    }

    fn futures_body(&self, o: &OrderRequest, p: &Placing) -> Result<Value, ConnectorError> {
        let sym = &o.instrument.native;
        let contracts = p.contracts(self.venue_label(), sym, o.qty.value())?;
        let size = if o.side == Side::Buy { contracts } else { -contracts };
        let mut b = Map::new();
        b.insert("contract".into(), json!(sym));
        b.insert("size".into(), size_number(size)?);
        match o.order_type {
            OrderType::Limit => {
                let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
                b.insert("price".into(), json!(dec_text(px.value())));
            }
            OrderType::Market => {
                b.insert("price".into(), json!("0"));
            }
            other => return Err(ConnectorError::unsupported(format!("{} takes no {other:?} order", self.venue_label()))),
        }
        b.insert("tif".into(), json!(self.tif_word(o)?));
        if o.opts.reduce_only {
            b.insert("reduce_only".into(), json!(true));
        }
        if !p.client_id.is_empty() {
            b.insert("text".into(), json!(p.client_id));
        }
        Ok(Value::Object(b))
    }

    fn spot_row(&self, r: SpotOrder) -> VenueOrder {
        // REST rows carry `status`; stream rows carry `event`, and `finish_as` once finished.
        let word = if !r.status.is_empty() {
            r.status.clone()
        } else if r.event == "finish" {
            r.finish_as.clone()
        } else if matches!(r.event.as_str(), "put" | "update") {
            "open".to_string()
        } else {
            r.event.clone()
        };
        let filled = r.filled_amount.0.unwrap_or_default();
        let mut status = self.status_of(&word);
        if status == Mapped::Known(OrderStatus::New) && filled > Decimal::ZERO {
            status = Mapped::Known(OrderStatus::PartiallyFilled);
        }
        // A market buy's `amount` is in the quote coin: its size is what filled.
        let quote_sized = r.kind == "market" && r.side == "buy";
        VenueOrder {
            venue_id: r.id,
            client_id: client_text(&r.text),
            symbol: r.currency_pair.to_ascii_uppercase(),
            side: side_of(&r.side),
            limit: if r.kind == "limit" { positive(r.price) } else { None },
            order_type: r.kind,
            status,
            qty: if quote_sized { filled } else { r.amount.0.unwrap_or_default() },
            filled_qty: filled,
            stop: None,
            avg_price: positive(r.avg_deal_price),
            reduce_only: false,
            position: PositionSide::Net,
            created_ms: r.create_time_ms.0.and_then(whole_ms),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }
    }

    fn futures_row(&self, sizes: &ContractSizes, r: FuturesOrder) -> Result<VenueOrder, ConnectorError> {
        let sym = r.contract.to_ascii_uppercase();
        let size = r.size.0.unwrap_or_default();
        let total = size.abs();
        let left = r.left.0.unwrap_or_default().abs();
        let word = match r.status.as_str() {
            "finished" if !r.finish_as.is_empty() => r.finish_as.clone(),
            other => other.to_string(),
        };
        let mut status = self.status_of(&word);
        if status == Mapped::Known(OrderStatus::New) && left < total {
            status = Mapped::Known(OrderStatus::PartiallyFilled);
        }
        let price = positive(r.price);
        Ok(VenueOrder {
            venue_id: r.id,
            client_id: client_text(&r.text),
            side: side_of_size(size),
            order_type: if price.is_some() { "limit".into() } else { "market".into() },
            status,
            qty: sizes.to_base(self.venue_label(), &sym, total)?,
            filled_qty: sizes.to_base(self.venue_label(), &sym, (total - left).max(Decimal::ZERO))?,
            limit: price,
            stop: None,
            avg_price: positive(r.fill_price),
            reduce_only: r.is_reduce_only || r.is_close,
            position: PositionSide::Net,
            created_ms: r.create_time_ms.0.and_then(whole_ms).or_else(|| r.create_time.0.and_then(ms_of_secs)),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            symbol: sym,
            successor_id: None,
        })
    }

    /// A spot price-triggered order, followed as `cond:<id>`: its order's side, type, price and
    /// size. A market buy's `amount` is the quote coin to spend: no base size is stated.
    fn spot_price_row(&self, r: SpotPriceOrder) -> VenueOrder {
        let market = r.put.kind == "market";
        let status = price_status(&r.status);
        VenueOrder {
            venue_id: OrderSpace::Conditional.tag(&r.id),
            client_id: client_text(&r.put.text),
            symbol: r.market.to_ascii_uppercase(),
            side: side_of(&r.put.side),
            order_type: format!("trigger {}", if r.put.kind.is_empty() { "limit" } else { r.put.kind.as_str() }),
            trigger_pending: status == Mapped::Known(OrderStatus::New),
            status,
            qty: if market && r.put.side == "buy" { Decimal::ZERO } else { r.put.amount.0.unwrap_or_default() },
            filled_qty: Decimal::ZERO,
            limit: if market { None } else { positive(r.put.price) },
            stop: positive(r.trigger.price),
            avg_price: None,
            reduce_only: false,
            position: PositionSide::Net,
            created_ms: r.ctime.0.and_then(ms_of_secs),
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }
    }

    /// A futures price-triggered order, followed as `cond:<id>`, sized in the base coin. A
    /// whole-position close (`size` 0) states no size: its side is the one that closes the
    /// position its `order_type` or `auto_size` names.
    fn futures_price_row(&self, sizes: &ContractSizes, r: FuturesPriceOrder) -> Result<VenueOrder, ConnectorError> {
        let sym = r.initial.contract.to_ascii_uppercase();
        let size = r.initial.size.0.unwrap_or_default();
        let long = r.order_type.contains("close-long") || r.initial.auto_size == "close_long";
        let short = r.order_type.contains("close-short") || r.initial.auto_size == "close_short";
        let side = match side_of_size(size) {
            Mapped::Unknown(_) if long => Mapped::Known(Side::Sell),
            Mapped::Unknown(_) if short => Mapped::Known(Side::Buy),
            s => s,
        };
        let word = if r.status == "finished" { r.finish_as.as_str() } else { r.status.as_str() };
        let status = price_status(word);
        let market = r.initial.price.0.is_none_or(|p| p.is_zero());
        let kind = if r.order_type.is_empty() { "trigger" } else { r.order_type.as_str() };
        let whole = if size.is_zero() { " (whole position)" } else { "" };
        Ok(VenueOrder {
            venue_id: OrderSpace::Conditional.tag(&r.id),
            client_id: client_text(&r.initial.text),
            side,
            order_type: format!("{kind} {}{whole}", if market { "market" } else { "limit" }),
            trigger_pending: status == Mapped::Known(OrderStatus::New),
            status,
            qty: sizes.to_base(self.venue_label(), &sym, size.abs())?,
            filled_qty: Decimal::ZERO,
            limit: if market { None } else { positive(r.initial.price) },
            stop: positive(r.trigger.price),
            avg_price: None,
            reduce_only: r.initial.is_reduce_only || r.initial.is_close || long || short,
            position: match r.initial.auto_size.as_str() {
                "close_long" => PositionSide::Long,
                "close_short" => PositionSide::Short,
                _ => PositionSide::Net,
            },
            created_ms: r.create_time.0.and_then(ms_of_secs),
            parent_client_id: None,
            exit: None,
            symbol: sym,
            successor_id: None,
        })
    }

    /// The working price-triggered orders of the book: futures on `symbol` or all; spot all of
    /// them, the account's mode asked first (a unified account's list is asked with
    /// `account=unified`, "Unified account must be set to `unified`"). Spot on one pair lists
    /// none: no caller asks for a pair's price orders.
    fn price_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            let mut h = self.call("futures_price_orders", Method::Get, settings, format!("/futures/{SETTLE}/price_orders"), corr)?.query("status", "open").query("limit", PAGE).query("offset", 0);
            if let Some(s) = symbol {
                h = h.query("contract", s);
            }
            return Ok(Some(h));
        }
        if symbol.is_some() {
            return Ok(None);
        }
        Ok(Some(self.call(super::trade_errors::PRICE_MODE, Method::Get, settings, "/unified/unified_mode".into(), corr)?))
    }

    /// A spot fill: `fee` in `fee_currency`; GT and points paid instead are fee lines of their own.
    fn spot_fill(r: SpotTrade) -> VenueFill {
        let mut extra_fees = Vec::new();
        for (amount, currency, kind) in [(r.gt_fee, "GT", "gt_fee"), (r.point_fee, "POINT", "point_fee")] {
            if let Some(fee) = amount.0.filter(|f| !f.is_zero()) {
                extra_fees.push(FeeLine { fee, currency: currency.into(), kind: kind.into() });
            }
        }
        VenueFill {
            exec_id: r.id,
            position_only: r.order_id.is_empty(),
            venue_order_id: r.order_id,
            client_id: client_text(&r.text),
            symbol: r.currency_pair.to_ascii_uppercase(),
            side: side_of(&r.side),
            qty: r.amount.0.unwrap_or_default(),
            price: r.price.0.unwrap_or_default(),
            fee: r.fee.0.unwrap_or_default(),
            fee_currency: r.fee_currency.to_ascii_uppercase(),
            at_ms: r.create_time_ms.0.and_then(whole_ms).or_else(|| r.create_time.0.and_then(ms_of_secs)).unwrap_or_default(),
            extra_fees,
            fee_pending: false,
        }
    }

    /// A futures fill: the side is the sign of `size` (contracts); the fee is in the settlement
    /// currency (the doc names none on the fill); points paid are a fee line of their own. A
    /// fill of an order Gate.io placed itself (liquidation, deleveraging, insurance) moved a
    /// position with no order of the account behind it.
    fn futures_fill(&self, sizes: &ContractSizes, r: FuturesTrade) -> Result<VenueFill, ConnectorError> {
        let sym = r.contract.to_ascii_uppercase();
        let size = r.size.0.unwrap_or_default();
        let system = SYSTEM_TEXTS.contains(&r.text.as_str()) || r.text.starts_with("liq-") || r.text.starts_with("hedge-liq-");
        let extra_fees = match r.point_fee.0.filter(|f| !f.is_zero()) {
            Some(fee) => vec![FeeLine { fee, currency: "POINT".into(), kind: "point_fee".into() }],
            None => Vec::new(),
        };
        Ok(VenueFill {
            exec_id: r.trade_id,
            position_only: system || r.order_id.is_empty() || r.order_id == "0",
            venue_order_id: r.order_id,
            client_id: client_text(&r.text),
            side: side_of_size(size),
            qty: sizes.to_base(self.venue_label(), &sym, size.abs())?,
            price: r.price.0.unwrap_or_default(),
            fee: r.fee.0.unwrap_or_default(),
            fee_currency: SETTLE.to_ascii_uppercase(),
            at_ms: r.create_time_ms.0.and_then(whole_ms).or_else(|| r.create_time.0.and_then(ms_of_secs)).unwrap_or_default(),
            extra_fees,
            fee_pending: false,
            symbol: sym,
        })
    }

    fn position_row(&self, sizes: &ContractSizes, r: PositionRow) -> Result<VenuePosition, ConnectorError> {
        let sym = r.contract.to_ascii_uppercase();
        let size = r.size.0.unwrap_or_default();
        let side = match r.mode.as_str() {
            "" | "single" => PositionSide::Net,
            "dual_long" => PositionSide::Long,
            "dual_short" => PositionSide::Short,
            other => return Err(ConnectorError::decode(format!("gateio position {sym}: mode {other:?} is not single, dual_long or dual_short"))),
        };
        let lev = r.leverage.0.unwrap_or_default();
        let margin = match r.pos_margin_mode.as_str() {
            "cross" => Some(MarginMode::cross()),
            "isolated" => Some(MarginMode::isolated()),
            _ if r.leverage.0.is_some() => Some(if lev.is_zero() { MarginMode::cross() } else { MarginMode::isolated() }),
            _ => None,
        };
        Ok(VenuePosition {
            side,
            direction: side_of_size(size).known().copied(),
            qty: sizes.to_base(self.venue_label(), &sym, size.abs())?,
            leverage: if lev > Decimal::ZERO { Some(lev) } else { positive(r.cross_leverage_limit) },
            margin,
            symbol: sym,
        })
    }

    fn spot_rules(&self, r: PairRow) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(self.book.venue(), &r.id.to_ascii_uppercase()));
        v.tick = step_of(r.precision);
        v.lot_step = step_of(r.amount_precision);
        // The amount a market buy spends, in the quote coin, is rounded down to the pair's
        // quote precision (`precision`).
        v.quote_step = step_of(r.precision);
        v.min_qty = positive(r.min_base_amount);
        v.max_qty = positive(r.max_base_amount);
        v.min_notional = positive(r.min_quote_amount);
        v.max_notional = positive(r.max_quote_amount);
        v.market_max_qty = positive(r.market_order_max_stock);
        v.status = match r.trade_status.as_str() {
            "tradable" => TradingStatus::Trading,
            // A one-way window (buy only or sell only): the venue refuses the other side itself,
            // OTW has no side-specific status to hold it with.
            "buyable" | "sellable" => TradingStatus::Trading,
            other => TradingStatus::Closed(if other.is_empty() { "unstated".into() } else { other.to_string() }),
        };
        v
    }

    fn futures_rules(&self, r: ContractRow) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(self.book.venue(), &r.name.to_ascii_uppercase()));
        let cs = positive(r.quanto_multiplier);
        let base = |d: OptDec| positive(d).zip(cs).map(|(n, c)| n * c);
        v.contract_size = cs;
        v.tick = positive(r.order_price_round);
        // Whole contracts: the decimal size (`enable_decimal`) states no step, so it is not used.
        v.lot_step = cs;
        v.min_qty = base(r.order_size_min);
        v.max_qty = base(r.order_size_max);
        v.market_max_qty = base(r.market_order_size_max);
        // "`in_delisting=true` and position_size=0 indicates the contract is delisted".
        let delisted = r.in_delisting && r.position_size.0.is_some_and(|p| p.is_zero());
        v.status = match (r.status.as_str(), r.in_delisting) {
            _ if delisted => TradingStatus::Closed("delisted".into()),
            ("trading", false) => TradingStatus::Trading,
            // CONTRACT_IN_DELISTING: "only reduce-only order or close order is allowed".
            ("trading", true) | ("delisting", _) => TradingStatus::CloseOnly,
            (other, _) => TradingStatus::Closed(if other.is_empty() { "unstated".into() } else { other.to_string() }),
        };
        v
    }

    /// The subscribe frames of the book's private channels: written unsigned, with no `time`
    /// yet (the signer stamps and signs each one).
    fn frames(&self, user: &str) -> Vec<Vec<u8>> {
        let frame = |channel: &str, payload: Value| json!({ "time": 0, "channel": channel, "event": "subscribe", "payload": payload }).to_string().into_bytes();
        if self.is_futures() {
            ["futures.orders", "futures.usertrades", "futures.positions"].iter().map(|c| frame(c, json!([user, "!all"]))).collect()
        } else {
            ["spot.orders", "spot.usertrades"].iter().map(|c| frame(c, json!(["!all"]))).collect()
        }
    }

    fn with_frames(&self, mut socket: PrivateSocket, user: &str) -> PrivateSocket {
        let mut frames = self.frames(user).into_iter();
        socket.subscribe = frames.next().unwrap_or_default();
        socket.more = frames.collect();
        socket
    }
}

/// Rows of an answer that is one object or a list of them.
fn rows_of<T: for<'de> Deserialize<'de>>(what: &str, body: &[u8]) -> Result<Vec<T>, ConnectorError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| decode_err(what, e))?;
    match v {
        Value::Array(items) => items.into_iter().map(|i| serde_json::from_value(i).map_err(|e| decode_err(what, e))).collect(),
        Value::Null => Ok(Vec::new()),
        one => Ok(vec![serde_json::from_value(one).map_err(|e| decode_err(what, e))?]),
    }
}

impl TradeMapper for Gateio {
    fn client_id_rule(&self) -> ClientIdRule {
        // "no longer than 28 bytes without `t-` prefix", "0-9, A-Z, a-z, underscore(_),
        // hyphen(-) or dot(.)".
        ClientIdRule { max_len: 28, charset: "letters, digits, _, - and .", allowed: |c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') }
    }

    fn client_id(&self, otw_id: &str) -> Result<String, ConnectorError> {
        self.client_id_rule().check(self.venue_name(), otw_id)?;
        Ok(format!("{CID_PREFIX}{otw_id}"))
    }

    fn otw_id(&self, venue_client_id: &str) -> String {
        venue_client_id.strip_prefix(CID_PREFIX).unwrap_or(venue_client_id).to_string()
    }

    fn is_sandbox(&self, settings: &Settings) -> bool {
        testnet(settings).unwrap_or(false)
    }

    fn place(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if self.is_futures() {
            let body = self.futures_body(o, p)?;
            Self::with_body(self.call("futures_create", Method::Post, settings, format!("/futures/{SETTLE}/orders"), p.corr)?, &body)
        } else {
            let body = self.spot_body(o, p)?;
            Self::with_body(self.call("spot_create", Method::Post, settings, "/spot/orders".into(), p.corr)?, &body)
        }
    }

    /// Futures: `PUT /futures/usdt/orders/{id}` with the new `price` and/or `size`, a number of
    /// contracts "including filled part", signed like the order ("Order side must be identical
    /// to the original one"). Spot: the single amend is a `PATCH`, which OTW's requests cannot carry, so the batch amend
    /// (`POST /spot/amend_batch_orders`, one row) carries it: "Only one of `amount` or `price`
    /// can be specified".
    fn amend(&self, settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let corr = p.corr;
        if change.stop.is_some() || change.tif.is_some() {
            return Err(ConnectorError::unsupported(format!("{} changes an order's price or size only", self.venue_label())));
        }
        if self.is_futures() {
            let mut b = Map::new();
            if let Some(q) = change.qty {
                let contracts = p.contracts(self.venue_label(), &cur.symbol, q.value())?;
                let side = cur.side.known().copied().ok_or_else(|| ConnectorError::invalid(format!("order {} states no side, so its size cannot be changed", cur.venue_id)))?;
                let size = if side == Side::Buy { contracts } else { -contracts };
                b.insert("size".into(), size_number(size)?);
            }
            if let Some(px) = change.limit {
                b.insert("price".into(), json!(dec_text(px.value())));
            }
            if b.is_empty() {
                return Err(ConnectorError::invalid("nothing to change"));
            }
            let h = self.call("futures_amend", Method::Put, settings, format!("/futures/{SETTLE}/orders/{}", cur.venue_id), corr)?;
            return Self::with_body(h, &Value::Object(b));
        }
        let mut row = Map::new();
        row.insert("order_id".into(), json!(cur.venue_id));
        row.insert("currency_pair".into(), json!(cur.symbol));
        match (change.limit, change.qty) {
            (Some(px), None) => row.insert("price".into(), json!(dec_text(px.value()))),
            (None, Some(q)) => row.insert("amount".into(), json!(dec_text(q.value()))),
            (Some(_), Some(_)) => return Err(ConnectorError::unsupported(format!("{} changes a spot order's price or its size, one at a time", self.venue_label()))),
            (None, None) => return Err(ConnectorError::invalid("nothing to change")),
        };
        Self::with_body(self.call("spot_amend_batch", Method::Post, settings, "/spot/amend_batch_orders".into(), corr)?, &Value::Array(vec![Value::Object(row)]))
    }

    /// A price-triggered order (`cond:<id>`, listed by the account read) is cancelled on its own
    /// endpoint: `DELETE /spot/price_orders/{order_id}`, `DELETE /futures/usdt/price_orders/{order_id}`
    /// (official SDK: the documentation site answered no page).
    fn cancel(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if let (OrderSpace::Conditional, id) = OrderSpace::of(venue_id) {
            return if self.is_futures() {
                self.call("futures_price_cancel", Method::Delete, settings, format!("/futures/{SETTLE}/price_orders/{id}"), corr)
            } else {
                self.call("spot_price_cancel", Method::Delete, settings, format!("/spot/price_orders/{id}"), corr)
            };
        }
        if self.is_futures() {
            self.call("futures_cancel", Method::Delete, settings, format!("/futures/{SETTLE}/orders/{venue_id}"), corr)
        } else {
            Ok(self.call("spot_cancel", Method::Delete, settings, format!("/spot/orders/{venue_id}"), corr)?.query("currency_pair", symbol))
        }
    }

    /// Spot: one call for the whole book ("When `currency_pair` is not specified, all trading
    /// pair pending orders will be cancelled"). Futures: one per contract (the SDK requires it).
    fn cancel_all_way(&self) -> CancelAllWay {
        if self.is_futures() {
            CancelAllWay::BySymbol
        } else {
            CancelAllWay::Book
        }
    }

    /// The plain orders, then the price-triggered ones ("Cancel all auto orders": `DELETE
    /// /spot/price_orders`, `DELETE /futures/usdt/price_orders?contract`, official SDK). A price
    /// order the call leaves (a unified account's, listed with `account=unified`) is listed
    /// again and cancelled on its own by id.
    fn cancel_all(&self, settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut out = Vec::new();
        if self.is_futures() {
            for s in symbols {
                out.push(self.call("futures_cancel_all", Method::Delete, settings, format!("/futures/{SETTLE}/orders"), corr)?.query("contract", s));
                out.push(self.call("futures_price_cancel_all", Method::Delete, settings, format!("/futures/{SETTLE}/price_orders"), corr)?.query("contract", s));
            }
            return Ok(out);
        }
        if symbols.is_empty() {
            out.push(self.call("spot_cancel_all", Method::Delete, settings, "/spot/orders".into(), corr)?);
            out.push(self.call("spot_price_cancel_all", Method::Delete, settings, "/spot/price_orders".into(), corr)?);
            return Ok(out);
        }
        for s in symbols {
            out.push(self.call("spot_cancel_all", Method::Delete, settings, "/spot/orders".into(), corr)?.query("currency_pair", s));
            out.push(self.call("spot_price_cancel_all", Method::Delete, settings, "/spot/price_orders".into(), corr)?.query("market", s));
        }
        Ok(out)
    }

    /// The working order by its `text` (pending only on spot; on futures until 60 s after it
    /// ends), then the finished orders of the instrument, whose `text` the trader matches.
    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            return Ok(vec![
                self.call("futures_order", Method::Get, settings, format!("/futures/{SETTLE}/orders/{client_id}"), corr)?,
                self.call("futures_orders", Method::Get, settings, format!("/futures/{SETTLE}/orders"), corr)?
                    .query("contract", symbol)
                    .query("status", "finished")
                    .query("limit", PAGE)
                    .query("offset", 0),
            ]);
        }
        Ok(vec![
            self.call("spot_order", Method::Get, settings, format!("/spot/orders/{client_id}"), corr)?.query("currency_pair", symbol),
            self.call("spot_orders", Method::Get, settings, "/spot/orders".into(), corr)?
                .query("currency_pair", symbol)
                .query("status", "finished")
                .query("page", 1)
                .query("limit", PAGE),
        ])
    }

    /// A price-triggered order (`cond:<id>`) is read on its own endpoint: `GET
    /// /spot/price_orders/{order_id}`, `GET /futures/usdt/price_orders/{order_id}` (official SDK).
    fn query_by_venue_id(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if let (OrderSpace::Conditional, id) = OrderSpace::of(venue_id) {
            return Ok(vec![if self.is_futures() {
                self.call("futures_price_order", Method::Get, settings, format!("/futures/{SETTLE}/price_orders/{id}"), corr)?
            } else {
                self.call("spot_price_order", Method::Get, settings, format!("/spot/price_orders/{id}"), corr)?
            }]);
        }
        if self.is_futures() {
            return Ok(vec![self.call("futures_order", Method::Get, settings, format!("/futures/{SETTLE}/orders/{venue_id}"), corr)?]);
        }
        Ok(vec![self.call("spot_order", Method::Get, settings, format!("/spot/orders/{venue_id}"), corr)?.query("currency_pair", symbol)])
    }

    /// The plain working orders, then the price-triggered ones ([`Gateio::price_orders`]), so a
    /// cancel all reaches both.
    fn open_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let plain = if self.is_futures() {
            let mut h = self.call("futures_orders", Method::Get, settings, format!("/futures/{SETTLE}/orders"), corr)?.query("status", "open").query("limit", PAGE).query("offset", 0);
            if let Some(s) = symbol {
                h = h.query("contract", s);
            }
            h
        } else {
            match symbol {
                Some(s) => self.call("spot_orders", Method::Get, settings, "/spot/orders".into(), corr)?.query("currency_pair", s).query("status", "open").query("page", 1).query("limit", PAGE),
                None => self.call("spot_open_orders", Method::Get, settings, "/spot/open_orders".into(), corr)?.query("page", 1).query("limit", PAGE),
            }
        };
        Ok(std::iter::once(plain).chain(self.price_orders(settings, symbol, corr)?).collect())
    }

    /// `from` and `to` in seconds; spot pages by `page`, futures by `offset`.
    fn fills_since(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let (from, to) = (from_ms.div_euclid(1000), (to_ms + 999).div_euclid(1000));
        if self.is_futures() {
            return Ok(self
                .call("futures_my_trades", Method::Get, settings, format!("/futures/{SETTLE}/my_trades_timerange"), corr)?
                .query("contract", symbol)
                .query("from", from)
                .query("to", to)
                .query("limit", PAGE)
                .query("offset", 0));
        }
        Ok(self
            .call("spot_my_trades", Method::Get, settings, "/spot/my_trades".into(), corr)?
            .query("currency_pair", symbol)
            .query("from", from)
            .query("to", to)
            .query("limit", SPOT_FILL_PAGE)
            .query("page", 1))
    }

    fn fills_window_ms(&self) -> Option<i64> {
        (!self.is_futures()).then_some(SPOT_FILLS_WINDOW_MS)
    }

    /// The whole list (a single pair or contract call answers an unknown name with an error, not
    /// an empty list): spot in one call, futures paged by `offset`.
    fn instrument_rules(&self, settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let base = self.base(settings)?;
        Ok(if self.is_futures() {
            HttpRequest::get("futures_contracts", base, format!("{API}/futures/{SETTLE}/contracts"), corr).query("limit", PAGE).query("offset", 0)
        } else {
            HttpRequest::get("spot_pairs", base, format!("{API}/spot/currency_pairs"), corr)
        })
    }

    /// The next page: `offset` moves by the page's `limit`, `page` by one. After the account
    /// mode (spot price orders), the cursor is the mode: the price-triggered list's first page,
    /// asked with `account=unified` for a unified account.
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        if req.endpoint_id == super::trade_errors::PRICE_MODE {
            next.endpoint_id = "spot_price_orders";
            next.path = format!("{API}/spot/price_orders");
            next.query = vec![("status".into(), "open".into()), ("limit".into(), PAGE.to_string()), ("offset".into(), "0".into())];
            if cursor == "unified" {
                next.query.push(("account".into(), "unified".into()));
            }
            return next;
        }
        let get = |k: &str| req.query.iter().find(|(n, _)| n == k).and_then(|(_, v)| v.parse::<i64>().ok());
        let limit = get("limit").unwrap_or(PAGE as i64);
        if let Some(off) = get("offset") {
            next.query.retain(|(k, _)| k != "offset");
            next.query.push(("offset".into(), (off + limit).to_string()));
        } else if let Some(page) = get("page") {
            next.query.retain(|(k, _)| k != "page");
            next.query.push(("page".into(), (page + 1).to_string()));
        }
        next
    }

    /// A spot market order works IOC or FOK; a futures market order IOC only ("market orders
    /// currently only support ioc mode"; FOK on a futures market order: doc silent).
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        if o.order_type == OrderType::Market && o.tif == TimeInForce::Fok && self.is_futures() {
            return Err(ConnectorError::unsupported(format!("{} works a market order immediate-or-cancel only: send it IOC, or as a limit order FOK", self.venue_label())));
        }
        Ok(())
    }

    fn positions(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.call("futures_position", Method::Get, settings, format!("/futures/{SETTLE}/positions/{symbol}"), corr)?))
    }

    /// In dual mode the contract's two sides are read by the dual-mode call ("For dual mode
    /// position operations, refer to `/futures/{settle}/dual_comp/positions` API group",
    /// changelog v4.16.0; SDK `get_dual_mode_position`: `GET
    /// /futures/{settle}/dual_comp/positions/{contract}`, a list). Single mode, or no mode held
    /// yet: the single-mode call.
    fn positions_in_mode(&self, settings: &Settings, symbol: &str, mode: Option<PositionMode>, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() || mode != Some(PositionMode::Hedge) {
            return self.positions(settings, symbol, corr);
        }
        Ok(Some(self.call("futures_dual_position", Method::Get, settings, format!("/futures/{SETTLE}/dual_comp/positions/{symbol}"), corr)?))
    }

    fn all_positions(&self, settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(Vec::new());
        }
        Ok(vec![self.call("futures_positions", Method::Get, settings, format!("/futures/{SETTLE}/positions"), corr)?.query("holding", "true").query("limit", PAGE).query("offset", 0)])
    }

    /// `POSITION_DUAL_MODE` ("Operation forbidden in dual-mode"): the account switched mode.
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.is_futures() && e.code.as_deref() == Some("POSITION_DUAL_MODE")
    }

    /// The futures account's `position_mode` (`single`, `dual`, `dual_plus` / `split`), read
    /// with no position open.
    fn account_position_mode(&self, settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.call("futures_account", Method::Get, settings, format!("/futures/{SETTLE}/accounts"), corr)?))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("futures account", e))?;
        match v.get("position_mode").and_then(Value::as_str) {
            Some("single") => Ok(Some(PositionMode::OneWay)),
            Some("dual") => Ok(Some(PositionMode::Hedge)),
            Some(other) if !other.is_empty() => Err(ConnectorError::unsupported(format!(
                "{}: this account holds positions in {other} mode, which OpenTraderWorld does not send orders in: nothing was sent, switch it to single or dual at Gate.io",
                self.venue_label()
            ))),
            // `in_dual_mode`, deprecated, where `position_mode` is absent.
            _ => Ok(v.get("in_dual_mode").and_then(Value::as_bool).map(|d| if d { PositionMode::Hedge } else { PositionMode::OneWay })),
        }
    }

    fn counts_contracts(&self) -> bool {
        self.is_futures()
    }

    fn market_buy_in_quote(&self, o: &OrderRequest) -> bool {
        !self.is_futures() && o.order_type == OrderType::Market && o.side == Side::Buy
    }

    /// The pair's ticker (public): a buy is valued at its `lowest_ask` ("Recent lowest ask").
    fn reference_price(&self, settings: &Settings, symbol: &str, _side: Side, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.is_futures() {
            return Ok(None);
        }
        Ok(Some(HttpRequest::get("spot_ticker", self.base(settings)?, format!("{API}/spot/tickers"), corr).query("currency_pair", symbol)))
    }

    fn decode_reference_price(&self, body: &[u8], side: Side) -> Result<Decimal, ConnectorError> {
        let rows: Vec<Ticker> = rows_of("spot ticker", body)?;
        let t = rows.into_iter().next().ok_or_else(|| ConnectorError::decode("gateio spot ticker: no row"))?;
        let px = match side {
            Side::Buy => t.lowest_ask,
            Side::Sell => t.highest_bid,
        };
        positive(px).ok_or_else(|| ConnectorError::decode("gateio spot ticker: no price on that side"))
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        let batch = matches!(frame, RawFrame::Http { endpoint_id: "spot_amend_batch", .. });
        super::trade_errors::decode(frame, batch)
    }

    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let (venue_id, text) = if self.is_futures() {
            let r: FuturesOrder = serde_json::from_slice(body).map_err(|e| decode_err("order ack", e))?;
            (r.id, r.text)
        } else {
            let r: SpotOrder = serde_json::from_slice(body).map_err(|e| decode_err("order ack", e))?;
            (r.id, r.text)
        };
        if venue_id.is_empty() {
            return Err(ConnectorError::decode("gateio order ack: no id"));
        }
        Ok(Ack { venue_id, client_id: client_text(&text) })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_orders_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    /// One order (`/orders/{id}`), a list, or spot's open orders grouped by pair.
    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        match ctx.endpoint {
            // The account's mode, or the refusal a key without unified access gets: classic.
            p if p == super::trade_errors::PRICE_MODE => {
                let mode = serde_json::from_slice::<UnifiedMode>(body).map(|m| m.mode).unwrap_or_default();
                let unified = !mode.is_empty() && mode != "classic";
                return Ok((Vec::new(), Some(if unified { "unified" } else { "classic" }.to_string())));
            }
            "spot_price_orders" | "spot_price_order" => {
                let rows: Vec<SpotPriceOrder> = rows_of("spot price orders", body)?;
                let n = rows.len();
                return Ok((rows.into_iter().map(|r| self.spot_price_row(r)).collect(), if ctx.endpoint == "spot_price_order" { None } else { next_if_full(n) }));
            }
            "futures_price_orders" | "futures_price_order" => {
                let rows: Vec<FuturesPriceOrder> = rows_of("futures price orders", body)?;
                let n = rows.len();
                let out = rows.into_iter().map(|r| self.futures_price_row(ctx.sizes, r)).collect::<Result<Vec<_>, _>>()?;
                return Ok((out, if ctx.endpoint == "futures_price_order" { None } else { next_if_full(n) }));
            }
            _ => {}
        }
        if self.is_futures() {
            let rows: Vec<FuturesOrder> = rows_of("orders", body)?;
            let n = rows.len();
            let out = rows.into_iter().map(|r| self.futures_row(ctx.sizes, r)).collect::<Result<Vec<_>, _>>()?;
            return Ok((out, if ctx.endpoint == "futures_order" { None } else { next_if_full(n) }));
        }
        if ctx.endpoint == "spot_open_orders" {
            let groups: Vec<OpenGroup> = rows_of("open orders", body)?;
            let full = groups.iter().any(|g| g.orders.len() >= PAGE);
            let rows = groups.into_iter().flat_map(|g| g.orders).map(|r| self.spot_row(r)).collect();
            return Ok((rows, full.then(|| "next".to_string())));
        }
        let rows: Vec<SpotOrder> = rows_of("orders", body)?;
        let n = rows.len();
        Ok((rows.into_iter().map(|r| self.spot_row(r)).collect(), if ctx.endpoint == "spot_order" { None } else { next_if_full(n) }))
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.decode_fills_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        if self.is_futures() {
            let rows: Vec<FuturesTrade> = rows_of("my_trades_timerange", body)?;
            let n = rows.len();
            return Ok((rows.into_iter().map(|r| self.futures_fill(ctx.sizes, r)).collect::<Result<Vec<_>, _>>()?, next_if_full(n)));
        }
        let rows: Vec<SpotTrade> = rows_of("my_trades", body)?;
        let n = rows.len();
        Ok((rows.into_iter().map(Self::spot_fill).collect(), (n >= SPOT_FILL_PAGE).then(|| "next".to_string())))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        if self.is_futures() {
            let rows: Vec<ContractRow> = rows_of("contracts", body)?;
            let n = rows.len();
            return Ok((rows.into_iter().filter(|r| !r.name.is_empty()).map(|r| self.futures_rules(r)).collect(), next_if_full(n)));
        }
        let rows: Vec<PairRow> = rows_of("currency_pairs", body)?;
        Ok((rows.into_iter().filter(|r| !r.id.is_empty()).map(|r| self.spot_rules(r)).collect(), None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.decode_positions_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let rows: Vec<PositionRow> = rows_of("positions", body)?;
        rows.into_iter().map(|r| self.position_row(ctx.sizes, r)).collect()
    }

    fn decode_cancel_rows(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        if ctx.endpoint != "spot_cancel_all" {
            return Ok(Vec::new());
        }
        let rows: Vec<OrderCancel> = rows_of("cancel all", body)?;
        Ok(rows
            .into_iter()
            .map(|r| CancelRow { error: (r.succeeded == Some(false)).then(|| super::trade_errors::mapped(&r.label, &r.message)), venue_id: r.id })
            .collect())
    }

    /// Spot `status` (`open`, `closed`, `cancelled`) and the `finish_as` words of both books
    /// (spot REST and stream, futures). `open` with a fill is made PartiallyFilled by the row.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "open" => OrderStatus::New,
            "closed" | "filled" => OrderStatus::Filled,
            // Spot: "cancelled ... regardless of partial fills"; every other way an order ends
            // unfilled or partly filled.
            "cancelled" | "liquidate_cancelled" | "small" | "depth_not_enough" | "trader_not_enough" | "ioc" | "poc" | "fok" | "stp" | "price_protect_cancelled"
            | "unified_check_failed" | "liquidated" | "auto_deleveraged" | "reduce_only" | "position_closed" | "reduce_out" => OrderStatus::Cancelled,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    /// Spot `wss://api.gateio.ws/ws/v4/` (`spot.orders`, `spot.usertrades`); futures
    /// `wss://fx-ws.gateio.ws/v4/ws/usdt` (`futures.orders`, `futures.usertrades`,
    /// `futures.positions`); the testnet's on `ws-testnet.gate.com`.
    fn private_socket(&self, settings: &Settings) -> Option<PrivateSocket> {
        let test = testnet(settings).ok()?;
        let (base, path, ping) = match (self.is_futures(), test) {
            (false, false) => (WS, "/ws/v4/", "spot.ping"),
            (false, true) => (WS_TESTNET, "/v4/ws/spot", "spot.ping"),
            (true, false) => (FX_WS, "/v4/ws/usdt", "futures.ping"),
            (true, true) => (WS_TESTNET, "/v4/ws/futures/usdt", "futures.ping"),
        };
        let socket = PrivateSocket::new(base, path, Vec::new(), json!({ "time": 0, "channel": ping }).to_string().into_bytes(), PING_EVERY_MS);
        Some(self.with_frames(socket, NO_USER))
    }

    /// Futures: the channels name the user id, read from `GET /account/detail` (`user_id`).
    fn private_socket_bootstrap(&self, settings: &Settings, _socket: &PrivateSocket, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.is_futures() {
            return Ok(None);
        }
        Ok(Some(self.call("account_detail", Method::Get, settings, "/account/detail".into(), corr)?))
    }

    fn decode_private_socket_bootstrap(&self, socket: PrivateSocket, body: &[u8]) -> Result<PrivateSocket, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("account detail", e))?;
        let user = match v.get("user_id") {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => return Err(ConnectorError::decode("gateio account detail: no user_id, so the futures channels cannot be named")),
        };
        Ok(self.with_frames(socket, &user))
    }

    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.decode_private_in(&DecodeCtx::new("", &ContractSizes::default()), payload)
    }

    /// The book's order, fill and position channels, and the answers to `subscribe` and the
    /// ping. A channel of the other book is left out.
    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let push: Push = serde_json::from_slice(payload).map_err(|e| decode_err("private frame", e))?;
        if push.channel.ends_with(".pong") {
            return Ok(vec![PrivateEvent::Answer { op: "pong".into(), ok: true, message: String::new() }]);
        }
        let error = push.error.as_ref().filter(|e| !e.is_null());
        if matches!(push.event.as_str(), "subscribe" | "unsubscribe") {
            let ok = error.is_none() && push.result.get("status").and_then(Value::as_str) == Some("success");
            let message = error.and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or_default().to_string();
            return Ok(vec![PrivateEvent::Answer { op: push.event, ok, message }]);
        }
        let mine = if self.is_futures() { push.channel.starts_with("futures.") } else { push.channel.starts_with("spot.") };
        if !mine || error.is_some() {
            return Ok(Vec::new());
        }
        let items = match push.result {
            Value::Array(a) => a,
            Value::Null => Vec::new(),
            one => vec![one],
        };
        let mut out = Vec::new();
        for item in items {
            match push.channel.as_str() {
                "spot.orders" => out.push(PrivateEvent::Order(self.spot_row(serde_json::from_value(item).map_err(|e| decode_err("spot.orders", e))?))),
                "spot.usertrades" => out.push(PrivateEvent::Fill(Self::spot_fill(serde_json::from_value(item).map_err(|e| decode_err("spot.usertrades", e))?))),
                "futures.orders" => out.push(PrivateEvent::Order(self.futures_row(ctx.sizes, serde_json::from_value(item).map_err(|e| decode_err("futures.orders", e))?)?)),
                "futures.usertrades" => out.push(PrivateEvent::Fill(self.futures_fill(ctx.sizes, serde_json::from_value(item).map_err(|e| decode_err("futures.usertrades", e))?)?)),
                "futures.positions" => out.push(PrivateEvent::Position(self.position_row(ctx.sizes, serde_json::from_value(item).map_err(|e| decode_err("futures.positions", e))?)?)),
                _ => {}
            }
        }
        Ok(out)
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        super::trade_errors::ERRORS
    }
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod tests;
