//! Bitget Classic (v2) orders, spot and USDT-M futures, written from the documentation alone:
//! no account, no demo key. Every field below is the doc's; each claim is sourced in
//! PROVIDER.md ("Trading"). The order path itself is the shared trader's; this file only says
//! what the calls are and what the answers mean.
//!
//! - Place: spot `POST /api/v2/spot/trade/place-order`, futures `POST /api/v2/mix/order/place-order`.
//!   A stop is a plan order (`place-plan-order`, market or limit once triggered), kept in its own
//!   id space: its id is followed as `cond:<id>` ([`OrderSpace`]).
//! - Client id: `clientOid`, at most 49 letters, digits, `-`, `+`, `_` and `#`, the OTW key unchanged.
//! - A spot market buy is sized in the quote coin: the trader values it at the best ask read just
//!   before sending ([`TradeMapper::market_buy_in_quote`]).
//! - Futures: `marginMode` and `marginCoin` read from the account for the symbol, sent with every
//!   order, never chosen; the position mode read from the account (`posMode`), never switched.
//!   One-way mode sends `reduceOnly`; hedge mode names the position with `side` and opens or
//!   closes it with `tradeSide`.
//! - Demo trading (`demo` setting): the same host with the header `paptrading: 1`, the private
//!   socket on `wspap.bitget.com`.

#![deny(clippy::float_arithmetic)]

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderRequest, OrderStatus, OrderType, Side, TimeInForce, TriggerKind};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, CancelRow, ClientIdRule, DecodeCtx, ErrorRow, ExitChange, ExitKind, FeeLine, MarginKind, MarginMode, OrderSpace, Page, Placing, PositionMode, PositionSide, PrivateEvent,
    PrivateSocket, Settings, TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{Bitget, Book, FUTURES, REST};

/// Futures margin coin: the USDT-M book margins in USDT (`supportMarginCoins`).
const MARGIN_COIN: &str = "USDT";
/// Rows per list page ("Default: 100, maximum: 100").
const PAGE: usize = 100;
/// How often the private socket is pinged ("set a 30 seconds timer to send string ping").
const PING_EVERY_MS: u64 = 30_000;
/// The symbol the account read is asked with when it is read for the position mode, which is
/// the product type's ("Changes the user's position mode for all symbol futures"), while the
/// endpoint takes a symbol (`Get Single Account`, `symbol` required).
const MODE_SYMBOL: &str = "BTCUSDT";

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("bitget {what}: {e}"))
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn int(v: &Value) -> Option<i64> {
    text(v).trim().parse::<i64>().ok()
}

/// A positive amount, or none: Bitget writes `"0"`, `""` and `null` for a price an order lacks.
fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn side_of(word: &str) -> Mapped<Side> {
    match word.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

pub(super) fn is_demo(s: &Settings) -> bool {
    s.get("demo").is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on" | "demo"))
}

/// Whether an endpoint lists or answers conditional (plan) orders.
fn conditional(endpoint: &str) -> bool {
    endpoint.contains("plan")
}

/// The futures plan types other than `normal_plan` (Cancel Trigger Order, `planType`). The
/// account read, the order lists and the look-ups all follow such a plan order as
/// `cond:<planType>:<orderId>`, so its cancel names it and a look-up finds it by that one id.
const PLAN_TYPES: &[&str] = &["track_plan", "moving_plan", "profit_plan", "loss_plan", "pos_profit", "pos_loss"];

/// The plan types a cancel all removes in one call each (Cancel Trigger Order with no id list:
/// "All orders that fall under that productType and symbol will be cancelled"), with the
/// endpoint id each call goes out under. Only the values its `planType` enum lists: a trailing
/// stop (`track_plan`) is not among them, so it is cancelled by its id once the cancel all has
/// read what is left. A position's own take profit or stop loss (`pos_profit`, `pos_loss`)
/// belongs to the position and is left to it.
const CANCEL_ALL_PLANS: &[(&str, &str)] =
    &[("normal_plan", "mix_plan_cancel_all"), ("profit_plan", "mix_plan_cancel_all_profit"), ("loss_plan", "mix_plan_cancel_all_loss"), ("moving_plan", "mix_plan_cancel_all_moving")];

/// The open-orders read of the take profit and stop loss list (`profit_loss`): its position
/// take profits and stop losses are the position's, not working orders a cancel all removes.
const OPEN_EXITS: &str = "mix_plan_open_exits";

/// The id a futures plan row is followed by: `cond:<planType>:<orderId>` for a plan type other
/// than `normal_plan`, `cond:<orderId>` otherwise (and on spot, whose `planType` is a sizing).
fn plan_venue_id(futures: bool, plan_type: &str, order_id: &str) -> String {
    if futures && PLAN_TYPES.contains(&plan_type) {
        OrderSpace::Conditional.tag(&format!("{plan_type}:{order_id}"))
    } else {
        OrderSpace::Conditional.tag(order_id)
    }
}

/// A futures order's direction: in hedge mode `side` names the position (`posSide`) and
/// `tradeSide` opens or closes it ("Close long: side=buy, tradeSide=close"), so the trade is
/// read from those two; otherwise the row's `side`. `None`: not a hedge row.
pub(super) fn hedge_side(pos_side: &str, trade_side: &str) -> Option<Side> {
    match (pos_side, trade_side) {
        ("long", "open") | ("short", "close") => Some(Side::Buy),
        ("long", "close") | ("short", "open") => Some(Side::Sell),
        _ => None,
    }
}

/// A conditional id's plan type and the venue's order id: `normal_plan` unless it names one.
pub(crate) fn plan_of(id: &str) -> (&str, &str) {
    match id.split_once(':') {
        Some((t, rest)) if PLAN_TYPES.contains(&t) => (t, rest),
        _ => ("normal_plan", id),
    }
}

/// The `planType` the pending and history trigger lists take for a plan type: the take
/// profits, stop losses and trailing exits are all listed under `profit_loss`.
fn listed_as(plan_type: &str) -> &str {
    match plan_type {
        "normal_plan" | "track_plan" => plan_type,
        _ => "profit_loss",
    }
}

/// One order row, in any of the shapes the order endpoints and the `orders` channel answer.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct OrderRow {
    order_id: String,
    client_oid: Option<String>,
    #[serde(alias = "instId")]
    symbol: String,
    side: String,
    order_type: String,
    /// The spot `orders` channel's table names it `ordType` (its example `orderType`).
    ord_type: String,
    status: String,
    state: String,
    plan_status: String,
    size: OptDec,
    new_size: OptDec,
    base_volume: OptDec,
    acc_base_volume: OptDec,
    price: OptDec,
    price_avg: OptDec,
    base_price: OptDec,
    trigger_price: OptDec,
    execute_price: OptDec,
    reduce_only: Value,
    pos_side: String,
    trade_side: String,
    plan_type: String,
    c_time: Value,
}

/// One execution, REST or `fill` channel.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FillRow {
    trade_id: String,
    order_id: String,
    client_oid: Option<String>,
    symbol: String,
    side: String,
    /// Spot.
    price_avg: OptDec,
    size: OptDec,
    /// Futures.
    price: OptDec,
    base_volume: OptDec,
    trade_side: String,
    /// An object on spot REST, a list elsewhere.
    fee_detail: Value,
    c_time: Value,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PositionRow {
    #[serde(alias = "instId")]
    symbol: String,
    hold_side: String,
    pos_mode: String,
    total: OptDec,
    leverage: OptDec,
    margin_mode: String,
    margin_coin: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RuleRow {
    symbol: String,
    quote_coin: String,
    // Spot.
    status: String,
    price_precision: OptDec,
    quantity_precision: OptDec,
    quote_precision: OptDec,
    #[serde(rename = "minTradeUSDT")]
    min_trade_usdt: OptDec,
    max_limit_order_value: OptDec,
    // Futures.
    symbol_status: String,
    price_end_step: OptDec,
    price_place: OptDec,
    size_multiplier: OptDec,
    min_trade_num: OptDec,
    max_order_qty: OptDec,
    max_market_order_qty: OptDec,
}

/// `10^-places`: the step a number of decimal places allows.
fn step_of(places: OptDec) -> Option<Decimal> {
    let p = places.0?;
    let n = u32::try_from(i64::try_from(p.trunc()).ok()?).ok()?;
    (n <= 28).then(|| Decimal::new(1, n))
}

/// The rows of a list answer and the cursor of its next page, whatever its shape: a list, an
/// object holding the list (`entrustedList`, `orderList`, `fillList`), or one object (the
/// futures order detail). `data: null` is an empty list.
fn rows_and_cursor(body: &[u8], what: &str) -> Result<(Vec<Value>, Option<String>), ConnectorError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| decode_err(what, e))?;
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    Ok(match data {
        Value::Array(rows) => {
            let next = if rows.len() >= PAGE { rows.last().map(|r| text(r.get("tradeId").or_else(|| r.get("orderId")).unwrap_or(&Value::Null))).filter(|c| !c.is_empty()) } else { None };
            (rows, next)
        }
        Value::Object(o) => {
            let list = ["entrustedList", "orderList", "fillList"].iter().find_map(|k| o.get(*k).cloned());
            match list {
                Some(Value::Array(rows)) => {
                    let next = if o.get("nextFlag").and_then(Value::as_bool) == Some(true) {
                        o.get("idLessThan").map(text)
                    } else if rows.len() >= PAGE {
                        o.get("endId").map(text)
                    } else {
                        None
                    };
                    (rows, next.filter(|c| !c.is_empty()))
                }
                Some(_) => (Vec::new(), None),
                None if o.contains_key("orderId") => (vec![Value::Object(o)], None),
                None => (Vec::new(), None),
            }
        }
        _ => (Vec::new(), None),
    })
}

/// A fee line's amount: Bitget writes a charge negative, OTW files the cost positive (as the
/// account read does).
fn fee_lines(v: &Value) -> Result<Vec<(Decimal, String)>, ConnectorError> {
    let rows: Vec<Value> = match v {
        Value::Null => return Ok(Vec::new()),
        Value::Array(a) => a.clone(),
        Value::Object(_) => vec![v.clone()],
        // The order read writes it as a string holding JSON (`feeDetail` of `orderInfo`): not a
        // fill's fee, never read as one.
        _ => return Ok(Vec::new()),
    };
    rows.iter()
        .map(|r| {
            let amount = r.get("totalFee").or_else(|| r.get("fee")).map(text).unwrap_or_default();
            let fee = if amount.trim().is_empty() { Decimal::ZERO } else { amount.trim().parse::<Decimal>().map_err(|_| ConnectorError::decode(format!("bitget fee {amount:?} is not a number")))? };
            Ok((fee.abs(), text(&r["feeCoin"]).to_ascii_uppercase()))
        })
        .collect()
}

impl Bitget {
    fn futures(&self) -> bool {
        self.book == Book::UsdtFutures
    }

    fn demo_header(h: &mut HttpRequest, s: &Settings) {
        if is_demo(s) {
            h.headers.push(("paptrading".into(), "1".into()));
        }
    }

    fn post(&self, endpoint: &'static str, s: &Settings, path: &str, body: Value, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let bytes = serde_json::to_vec(&body).map_err(|e| ConnectorError::invalid(format!("bitget {endpoint}: {e}")))?;
        let mut h = HttpRequest::get(endpoint, REST, path, corr).signed();
        h.method = Method::Post;
        h.body = Some(bytes.into());
        Self::demo_header(&mut h, s);
        Ok(h)
    }

    fn get(&self, endpoint: &'static str, s: &Settings, path: &str, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, REST, path, corr).signed();
        if self.futures() {
            h = h.query("productType", FUTURES);
        }
        Self::demo_header(&mut h, s);
        h
    }

    fn public(&self, endpoint: &'static str, s: &Settings, path: &str, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, REST, path, corr);
        Self::demo_header(&mut h, s);
        h
    }

    fn margin(&self, b: &mut Map<String, Value>, p: &Placing) {
        if let Some(m) = &p.margin {
            b.insert(
                "marginMode".into(),
                json!(match m.kind {
                    MarginKind::Cross => "crossed",
                    MarginKind::Isolated => "isolated",
                }),
            );
        }
        let coin = p.margin.as_ref().and_then(|m| m.coin.clone()).unwrap_or_else(|| MARGIN_COIN.into());
        b.insert("marginCoin".into(), json!(coin));
    }

    /// Futures direction fields: one-way mode sends the trade's side (and `reduceOnly`); hedge
    /// mode names the position with `side` and opens or closes it with `tradeSide` ("Close long:
    /// side=buy, tradeSide=close").
    fn futures_side(b: &mut Map<String, Value>, o: &OrderRequest, p: &Placing, reduce_word: &str) {
        let trade = if o.side == Side::Buy { "buy" } else { "sell" };
        match p.position {
            PositionSide::Net => {
                b.insert("side".into(), json!(trade));
                if o.opts.reduce_only {
                    b.insert("reduceOnly".into(), json!(reduce_word));
                }
            }
            PositionSide::Long | PositionSide::Short => {
                b.insert("side".into(), json!(if p.position == PositionSide::Long { "buy" } else { "sell" }));
                b.insert("tradeSide".into(), json!(if o.opts.reduce_only { "close" } else { "open" }));
            }
        }
    }

    fn force(o: &OrderRequest) -> Result<&'static str, ConnectorError> {
        if o.opts.post_only {
            return Ok("post_only");
        }
        Ok(match o.tif {
            TimeInForce::Gtc => "gtc",
            TimeInForce::Ioc => "ioc",
            TimeInForce::Fok => "fok",
            other => return Err(ConnectorError::unsupported(format!("Bitget takes no {:?} time in force", other.kind()))),
        })
    }

    fn trigger_type(o: &OrderRequest) -> Result<&'static str, ConnectorError> {
        Ok(match o.opts.trigger {
            None | Some(TriggerKind::Last) => "fill_price",
            Some(TriggerKind::Mark) => "mark_price",
            Some(other) => return Err(ConnectorError::unsupported(format!("Bitget triggers on the last or the mark price, not {other:?}"))),
        })
    }

    /// The body of a plain order (Place Order) or a plan order (Place Plan Order / Place Trigger
    /// Order), and the call it goes to.
    fn order_call(&self, o: &OrderRequest, p: &Placing) -> Result<(&'static str, &'static str, Value), ConnectorError> {
        let mut b = Map::new();
        b.insert("symbol".into(), json!(o.instrument.native));
        let qty = dec_text(o.qty.value());
        let cid = (!p.client_id.is_empty()).then(|| p.client_id.clone());
        let (kind, triggered) = match o.order_type {
            OrderType::Market => ("market", false),
            OrderType::Limit => ("limit", false),
            OrderType::Stop => ("market", true),
            OrderType::StopLimit if self.futures() => ("limit", true),
            other => return Err(ConnectorError::unsupported(format!("{} takes no {other:?} order", self.caps.label))),
        };
        b.insert("orderType".into(), json!(kind));
        let limit = || o.limit.map(|p| dec_text(p.value())).ok_or_else(|| ConnectorError::invalid("a limit order needs its price"));
        if !self.futures() {
            if o.opts.reduce_only {
                return Err(ConnectorError::unsupported(format!("{} takes no reduce-only order: a spot sale of what is held is a plain sell", self.caps.label)));
            }
            b.insert("side".into(), json!(if o.side == Side::Buy { "buy" } else { "sell" }));
            if triggered {
                let stop = o.stop.ok_or_else(|| ConnectorError::invalid("a stop order needs its stop price"))?;
                b.insert("triggerPrice".into(), json!(dec_text(stop.value())));
                b.insert("triggerType".into(), json!(Self::trigger_type(o)?));
                // `amount`: the size is in the base coin (Place Plan Order, `planType`).
                b.insert("planType".into(), json!("amount"));
                b.insert("size".into(), json!(qty));
                if let Some(c) = cid {
                    b.insert("clientOid".into(), json!(c));
                }
                return Ok(("spot_plan_place", "/api/v2/spot/trade/place-plan-order", Value::Object(b)));
            }
            // "It is invalid when orderType is market", and required: sent as asked.
            b.insert("force".into(), json!(Self::force(o)?));
            let size = if kind == "market" && o.side == Side::Buy {
                // "For Market-Buy orders, it represents the number of quote coins."
                p.quote_amount.map(dec_text).ok_or_else(|| ConnectorError::invalid("Bitget takes a spot market buy as an amount to spend, and none was valued: nothing was sent"))?
            } else {
                qty
            };
            b.insert("size".into(), json!(size));
            if kind == "limit" {
                b.insert("price".into(), json!(limit()?));
            }
            if let Some(c) = cid {
                b.insert("clientOid".into(), json!(c));
            }
            return Ok(("spot_place", "/api/v2/spot/trade/place-order", Value::Object(b)));
        }
        b.insert("productType".into(), json!(FUTURES));
        self.margin(&mut b, p);
        b.insert("size".into(), json!(qty));
        if triggered {
            let stop = o.stop.ok_or_else(|| ConnectorError::invalid("a stop order needs its stop price"))?;
            b.insert("planType".into(), json!("normal_plan"));
            b.insert("triggerPrice".into(), json!(dec_text(stop.value())));
            b.insert("triggerType".into(), json!(Self::trigger_type(o)?));
            if kind == "limit" {
                b.insert("price".into(), json!(limit()?));
            }
            // Place Trigger Order spells it `yes`.
            Self::futures_side(&mut b, o, p, "yes");
            if let Some(c) = cid {
                b.insert("clientOid".into(), json!(c));
            }
            return Ok(("mix_plan_place", "/api/v2/mix/order/place-plan-order", Value::Object(b)));
        }
        if kind == "limit" {
            b.insert("price".into(), json!(limit()?));
            // "Required if the orderType is limit"; a market order sends none (doc silent on it).
            b.insert("force".into(), json!(Self::force(o)?));
        }
        // Place Order spells it `YES`.
        Self::futures_side(&mut b, o, p, "YES");
        if let Some(c) = cid {
            b.insert("clientOid".into(), json!(c));
        }
        Ok(("mix_place", "/api/v2/mix/order/place-order", Value::Object(b)))
    }

    fn order_row(&self, endpoint: &str, mut r: OrderRow) -> VenueOrder {
        let plan = conditional(endpoint);
        if r.order_type.is_empty() {
            r.order_type = std::mem::take(&mut r.ord_type);
        }
        let word = [&r.plan_status, &r.status, &r.state].into_iter().find(|w| !w.is_empty()).cloned().unwrap_or_default();
        let mut status = self.status_of(&word);
        // The `orders` channel's `baseVolume` is the latest fill; `accBaseVolume` the total.
        let mut filled = positive(r.acc_base_volume).or(positive(r.base_volume)).unwrap_or_default();
        let mut qty = positive(r.size).unwrap_or_default();
        let market_buy = !self.futures() && !plan && r.order_type == "market" && r.side.eq_ignore_ascii_case("buy");
        if market_buy {
            // A spot market buy's `size` is the amount spent in the quote coin; what it bought is
            // its filled base volume.
            qty = filled;
        } else if !self.futures() && plan && r.plan_type == "total" {
            // A plan sized in the quote coin (placed elsewhere): no base size is stated.
            qty = Decimal::ZERO;
        } else if !plan {
            qty = positive(r.new_size).filter(|_| !self.futures() && r.order_type == "limit").unwrap_or(qty);
        }
        if plan && word == "executed" {
            if self.futures() {
                // "the order triggered": the history row states what traded. Not all of it: the
                // order it placed still works.
                if filled < qty {
                    status = Mapped::Known(if filled.is_zero() { OrderStatus::New } else { OrderStatus::PartiallyFilled });
                }
            } else {
                // A spot plan row states no traded amount: a stop sent at the market once triggered.
                filled = qty;
            }
        }
        let (limit, avg) = if plan {
            let px = if self.futures() { positive(r.price) } else { positive(r.execute_price) };
            (px.filter(|_| r.order_type == "limit"), positive(r.price_avg))
        } else if endpoint == "spot_unfilled" {
            // Get Current Orders: `priceAvg` is the order's price, `basePrice` the filled one.
            (positive(r.price_avg).filter(|_| r.order_type == "limit"), positive(r.base_price))
        } else {
            (positive(r.price).filter(|_| r.order_type == "limit"), positive(r.price_avg))
        };
        let position = match r.pos_side.as_str() {
            "long" => PositionSide::Long,
            "short" => PositionSide::Short,
            _ => PositionSide::Net,
        };
        let closes = r.trade_side == "close";
        // Hedge mode: `side` names the position, `tradeSide` opens or closes it.
        let side = hedge_side(&r.pos_side, &r.trade_side).map(Mapped::Known).unwrap_or_else(|| side_of(&r.side));
        let reduce = matches!(text(&r.reduce_only).to_ascii_lowercase().as_str(), "yes" | "true") || (position != PositionSide::Net && closes);
        let pending = plan && matches!(word.as_str(), "live" | "not_trigger");
        VenueOrder {
            venue_id: if plan { plan_venue_id(self.futures(), &r.plan_type, &r.order_id) } else { r.order_id },
            client_id: r.client_oid.filter(|c| !c.is_empty()),
            symbol: r.symbol.to_ascii_uppercase(),
            side,
            order_type: r.order_type,
            status,
            qty,
            filled_qty: filled,
            limit,
            stop: positive(r.trigger_price),
            avg_price: avg,
            reduce_only: reduce,
            position,
            created_ms: int(&r.c_time),
            trigger_pending: pending,
            parent_client_id: None,
            // A position's own take profit or stop loss (Place Position TP/SL, no size).
            exit: match r.plan_type.as_str() {
                "pos_profit" => Some(ExitKind::TakeProfit),
                "pos_loss" => Some(ExitKind::StopLoss),
                _ => None,
            },
            successor_id: None,
        }
    }

    /// `tradeSide` words that move a position without an order of the account behind it: a
    /// liquidation, an ADL, a delivery, a netting offset (Get Order Fill Details, `tradeSide`).
    fn venue_driven(trade_side: &str) -> bool {
        ["burst_", "delivery_", "dte_sys_adl", "offset_", "reduce_close_", "reduce_buy_single", "reduce_sell_single"].iter().any(|p| trade_side.starts_with(p))
    }

    fn fill_row(&self, r: FillRow) -> Result<VenueFill, ConnectorError> {
        let lines = fee_lines(&r.fee_detail)?;
        let mut it = lines.into_iter();
        let (fee, fee_currency) = it.next().unwrap_or((Decimal::ZERO, String::new()));
        let extra_fees = it.map(|(fee, currency)| FeeLine { fee, currency, kind: "fee".into() }).collect();
        let qty = if self.futures() { r.base_volume.0 } else { r.size.0 };
        let price = if self.futures() { r.price.0 } else { r.price_avg.0 };
        Ok(VenueFill {
            position_only: r.order_id.is_empty() || Self::venue_driven(&r.trade_side),
            exec_id: r.trade_id,
            venue_order_id: r.order_id,
            client_id: r.client_oid.filter(|c| !c.is_empty()),
            symbol: r.symbol.to_ascii_uppercase(),
            side: side_of(&r.side),
            qty: qty.unwrap_or_default(),
            price: price.unwrap_or_default(),
            fee,
            fee_currency,
            at_ms: int(&r.c_time).ok_or_else(|| ConnectorError::decode("bitget fill: no cTime"))?,
            extra_fees,
            fee_pending: false,
        })
    }

    fn position_row(r: PositionRow) -> VenuePosition {
        let direction = match r.hold_side.as_str() {
            "long" | "buy" => Some(Side::Buy),
            "short" | "sell" => Some(Side::Sell),
            _ => None,
        };
        let side = match (r.pos_mode.as_str(), direction) {
            ("hedge_mode", Some(Side::Buy)) => PositionSide::Long,
            ("hedge_mode", Some(Side::Sell)) => PositionSide::Short,
            _ => PositionSide::Net,
        };
        let qty = r.total.0.unwrap_or_default().abs();
        VenuePosition {
            symbol: r.symbol.to_ascii_uppercase(),
            side,
            direction: direction.filter(|_| qty > Decimal::ZERO),
            qty,
            leverage: positive(r.leverage),
            margin: margin_of(&r.margin_mode, &r.margin_coin),
        }
    }

    fn rule_row(&self, r: RuleRow) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(self.book.venue(), &r.symbol.to_ascii_uppercase()));
        let usdt = r.quote_coin.eq_ignore_ascii_case("USDT");
        if self.futures() {
            // `pricePlace` decimal places, `priceEndStep` the step in units of the last of them.
            let place = step_of(r.price_place);
            v.tick = match (place, positive(r.price_end_step)) {
                (Some(unit), Some(n)) => Some(unit * n),
                (unit, _) => unit,
            };
            v.lot_step = positive(r.size_multiplier);
            v.min_qty = positive(r.min_trade_num);
            v.max_qty = positive(r.max_order_qty);
            v.market_max_qty = positive(r.max_market_order_qty);
            v.min_notional = positive(r.min_trade_usdt).filter(|_| usdt);
            v.status = match r.symbol_status.as_str() {
                "normal" => TradingStatus::Trading,
                "limit_open" => TradingStatus::CloseOnly,
                other => TradingStatus::Closed(other.to_string()),
            };
        } else {
            v.tick = step_of(r.price_precision);
            v.lot_step = step_of(r.quantity_precision);
            v.quote_step = step_of(r.quote_precision);
            // Both stated in USDT: a bound only on a USDT pair.
            v.min_notional = positive(r.min_trade_usdt).filter(|_| usdt);
            v.max_notional = positive(r.max_limit_order_value).filter(|_| usdt);
            v.status = match r.status.as_str() {
                "online" => TradingStatus::Trading,
                // `gray` ("grey scale"): whether it trades is doc silent, so nothing is sent.
                other => TradingStatus::Closed(other.to_string()),
            };
        }
        v
    }

    fn decode_order_rows(&self, endpoint: &str, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let (rows, next) = rows_and_cursor(body, "orders")?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let r = serde_json::from_value::<OrderRow>(r).map_err(|e| decode_err("order row", e))?;
            if endpoint == OPEN_EXITS && r.plan_type.starts_with("pos_") {
                continue;
            }
            out.push(self.order_row(endpoint, r));
        }
        Ok((out, next))
    }
}

fn margin_of(mode: &str, coin: &str) -> Option<MarginMode> {
    let kind = match mode {
        // The enum says `crossed`; the order detail example writes `cross`.
        "crossed" | "cross" => MarginKind::Cross,
        "isolated" => MarginKind::Isolated,
        _ => return None,
    };
    Some(MarginMode { kind, coin: Some(coin.to_ascii_uppercase()).filter(|c| !c.is_empty()) })
}

impl TradeMapper for Bitget {
    /// FAQ Q12: "[0-9], [a-z], [A-Z] and [-,+,_,#], with a length less than 50". The comma is
    /// left out: the list's own separator makes it ambiguous.
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule { max_len: 49, charset: "letters, digits, -, +, _ and #", allowed: |c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '_' | '#') }
    }

    fn is_sandbox(&self, settings: &Settings) -> bool {
        is_demo(settings)
    }

    fn place(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let (endpoint, path, body) = self.order_call(o, p)?;
        self.post(endpoint, settings, path, body, p.corr)
    }

    /// A position's take profit or stop loss, triggered on the last price and closed at the
    /// market. Set with none working: Place Position TP/SL (`place-pos-tpsl`, no size: the whole
    /// position). Set with one working: Modify the TP/SL order (`modify-tpsl-order`, by its id,
    /// `size` empty "for position TP/SL orders"). Removed: Cancel Trigger Order with its plan type
    /// (`pos_profit`, `pos_loss`).
    fn change_exit(&self, settings: &Settings, x: &ExitChange<'_>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if !self.futures() {
            return Err(ConnectorError::unsupported("Bitget spot takes no take profit or stop loss on a position"));
        }
        let what = match x.kind {
            ExitKind::TakeProfit => "take profit",
            ExitKind::StopLoss => "stop loss",
        };
        let current = x.current.map(|o| plan_of(OrderSpace::of(&o.venue_id).1).1.to_string());
        match (x.price, current) {
            (Some(px), None) => {
                // holdSide: `long`/`short` in hedge mode, the position's direction in one-way mode.
                let hold = match x.position {
                    PositionSide::Long => "long",
                    PositionSide::Short => "short",
                    PositionSide::Net if x.entry_side == Side::Buy => "buy",
                    PositionSide::Net => "sell",
                };
                let (price, kind) = match x.kind {
                    ExitKind::TakeProfit => ("stopSurplusTriggerPrice", "stopSurplusTriggerType"),
                    ExitKind::StopLoss => ("stopLossTriggerPrice", "stopLossTriggerType"),
                };
                let mut b = Map::new();
                b.insert("symbol".into(), json!(x.symbol));
                b.insert("productType".into(), json!(FUTURES));
                b.insert("marginCoin".into(), json!(MARGIN_COIN));
                b.insert("holdSide".into(), json!(hold));
                b.insert(price.into(), json!(dec_text(px)));
                b.insert(kind.into(), json!("fill_price"));
                self.post("mix_pos_tpsl", settings, "/api/v2/mix/order/place-pos-tpsl", Value::Object(b), corr)
            }
            (Some(px), Some(id)) => self.post(
                "mix_tpsl_modify",
                settings,
                "/api/v2/mix/order/modify-tpsl-order",
                json!({ "orderId": id, "symbol": x.symbol, "productType": FUTURES, "marginCoin": MARGIN_COIN, "triggerPrice": dec_text(px), "triggerType": "fill_price", "size": "" }),
                corr,
            ),
            (None, Some(id)) => {
                let plan = match x.kind {
                    ExitKind::TakeProfit => "pos_profit",
                    ExitKind::StopLoss => "pos_loss",
                };
                self.post(
                    "mix_tpsl_cancel",
                    settings,
                    "/api/v2/mix/order/cancel-plan-order",
                    json!({ "orderIdList": [{ "orderId": id }], "symbol": x.symbol, "productType": FUTURES, "marginCoin": MARGIN_COIN, "planType": plan }),
                    corr,
                )
            }
            (None, None) => Err(ConnectorError::not_found(format!("Bitget shows no {what} on the {} position to remove", x.symbol))),
        }
    }

    /// The position take profits and stop losses working on `symbol` (Get Pending Trigger
    /// Orders, `planType=profit_loss`): what a change names.
    fn position_exits(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.futures() {
            return Ok(None);
        }
        Ok(Some(self.get("mix_plan_tpsl", settings, "/api/v2/mix/order/orders-plan-pending", corr).query("planType", "profit_loss").query("symbol", symbol)))
    }

    fn cancel(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let (space, id) = OrderSpace::of(venue_id);
        match (self.book, space) {
            (Book::Spot, OrderSpace::Plain) => self.post("spot_cancel", settings, "/api/v2/spot/trade/cancel-order", json!({ "symbol": symbol, "orderId": id }), corr),
            (Book::Spot, OrderSpace::Conditional) => self.post("spot_plan_cancel", settings, "/api/v2/spot/trade/cancel-plan-order", json!({ "orderId": id }), corr),
            (Book::UsdtFutures, OrderSpace::Plain) => {
                self.post("mix_cancel", settings, "/api/v2/mix/order/cancel-order", json!({ "symbol": symbol, "productType": FUTURES, "marginCoin": MARGIN_COIN, "orderId": id }), corr)
            }
            (Book::UsdtFutures, OrderSpace::Conditional) => {
                let (plan, id) = plan_of(id);
                self.post(
                    "mix_plan_cancel",
                    settings,
                    "/api/v2/mix/order/cancel-plan-order",
                    json!({ "orderIdList": [{ "orderId": id }], "symbol": symbol, "productType": FUTURES, "marginCoin": MARGIN_COIN, "planType": plan }),
                    corr,
                )
            }
        }
    }

    /// Spot: one call per symbol ("Cancel Order by Symbol", executed asynchronously), then the
    /// symbols' plan orders in one batch. Futures: the whole product type ("Cancel All Orders",
    /// no symbol), then its trigger orders of each plan type the cancel takes (Cancel Trigger
    /// Order with no id list: "All orders that fall under that productType and symbol will be
    /// cancelled"). A trailing stop is then cancelled by its id (see [`CANCEL_ALL_PLANS`]).
    fn cancel_all(&self, settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if self.futures() {
            let mut out = vec![self.post("mix_cancel_all", settings, "/api/v2/mix/order/cancel-all-orders", json!({ "productType": FUTURES, "marginCoin": MARGIN_COIN }), corr)?];
            for (plan, endpoint) in CANCEL_ALL_PLANS {
                out.push(self.post(endpoint, settings, "/api/v2/mix/order/cancel-plan-order", json!({ "productType": FUTURES, "marginCoin": MARGIN_COIN, "planType": plan }), corr)?);
            }
            return Ok(out);
        }
        let mut out = Vec::new();
        for s in symbols {
            out.push(self.post("spot_cancel_symbol", settings, "/api/v2/spot/trade/cancel-symbol-order", json!({ "symbol": s }), corr)?);
        }
        if !symbols.is_empty() {
            out.push(self.post("spot_plan_cancel_batch", settings, "/api/v2/spot/trade/batch-cancel-plan-order", json!({ "symbolList": symbols }), corr)?);
        }
        Ok(out)
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        if self.futures() {
            CancelAllWay::Book
        } else {
            CancelAllWay::BySymbol
        }
    }

    /// The `failureList` of a cancel-all or batch answer, one row per order it left.
    fn decode_cancel_rows(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("cancel answer", e))?;
        let Some(rows) = v["data"].get("failureList").and_then(Value::as_array) else { return Ok(Vec::new()) };
        let plan = CANCEL_ALL_PLANS.iter().find(|(_, e)| *e == ctx.endpoint).map(|(p, _)| *p).unwrap_or("normal_plan");
        let id = |r: &Value| {
            let id = text(&r["orderId"]);
            if conditional(ctx.endpoint) {
                plan_venue_id(self.futures(), plan, &id)
            } else {
                id
            }
        };
        Ok(rows.iter().map(|r| CancelRow { venue_id: id(r), error: Some(super::trade_errors::failure_row(r)) }).collect())
    }

    /// Spot: Get Order Info by `clientOid` (no symbol), then the symbol's current and past plan
    /// orders (they take no client id filter). Futures: the order detail, then the pending and
    /// past trigger orders, each by `clientOid`.
    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(if self.futures() {
            vec![
                self.get("mix_detail", settings, "/api/v2/mix/order/detail", corr).query("symbol", symbol).query("clientOid", client_id),
                self.get("mix_plan_pending", settings, "/api/v2/mix/order/orders-plan-pending", corr).query("planType", "normal_plan").query("symbol", symbol).query("clientOid", client_id),
                self.get("mix_plan_history", settings, "/api/v2/mix/order/orders-plan-history", corr).query("planType", "normal_plan").query("symbol", symbol).query("clientOid", client_id),
            ]
        } else {
            vec![
                self.get("spot_order_info", settings, "/api/v2/spot/trade/orderInfo", corr).query("clientOid", client_id),
                self.get("spot_plan_current", settings, "/api/v2/spot/trade/current-plan-order", corr).query("symbol", symbol).query("limit", PAGE),
                self.get("spot_plan_history", settings, "/api/v2/spot/trade/history-plan-order", corr).query("symbol", symbol).query("limit", PAGE),
            ]
        })
    }

    fn query_by_venue_id(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let (space, id) = OrderSpace::of(venue_id);
        Ok(match (self.book, space) {
            (Book::Spot, OrderSpace::Plain) => vec![self.get("spot_order_info", settings, "/api/v2/spot/trade/orderInfo", corr).query("orderId", id)],
            (Book::Spot, OrderSpace::Conditional) => vec![
                self.get("spot_plan_current", settings, "/api/v2/spot/trade/current-plan-order", corr).query("symbol", symbol).query("limit", PAGE),
                self.get("spot_plan_history", settings, "/api/v2/spot/trade/history-plan-order", corr).query("symbol", symbol).query("limit", PAGE),
            ],
            (Book::UsdtFutures, OrderSpace::Plain) => vec![self.get("mix_detail", settings, "/api/v2/mix/order/detail", corr).query("symbol", symbol).query("orderId", id)],
            (Book::UsdtFutures, OrderSpace::Conditional) => {
                let (plan, id) = plan_of(id);
                let listed = listed_as(plan);
                vec![
                    self.get("mix_plan_pending", settings, "/api/v2/mix/order/orders-plan-pending", corr).query("planType", listed).query("symbol", symbol).query("orderId", id),
                    self.get("mix_plan_history", settings, "/api/v2/mix/order/orders-plan-history", corr).query("planType", listed).query("symbol", symbol).query("orderId", id),
                ]
            }
        })
    }

    /// Spot: unfilled orders and current plan orders. Futures: pending orders, asked by state
    /// ("If not specified, all ordered with a status of live ... will be returned", so the
    /// partially filled ones apart), and pending trigger orders of each list (`normal_plan`,
    /// `track_plan`, `profit_loss` without the position's own take profit and stop loss).
    fn open_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let on = |h: HttpRequest| match symbol {
            Some(s) => h.query("symbol", s).query("limit", PAGE),
            None => h.query("limit", PAGE),
        };
        Ok(if self.futures() {
            vec![
                on(self.get("mix_pending", settings, "/api/v2/mix/order/orders-pending", corr).query("status", "live")),
                on(self.get("mix_pending", settings, "/api/v2/mix/order/orders-pending", corr).query("status", "partially_filled")),
                on(self.get("mix_plan_pending", settings, "/api/v2/mix/order/orders-plan-pending", corr).query("planType", "normal_plan")),
                on(self.get("mix_plan_pending", settings, "/api/v2/mix/order/orders-plan-pending", corr).query("planType", "track_plan")),
                on(self.get(OPEN_EXITS, settings, "/api/v2/mix/order/orders-plan-pending", corr).query("planType", "profit_loss")),
            ]
        } else {
            vec![
                on(self.get("spot_unfilled", settings, "/api/v2/spot/trade/unfilled-orders", corr)),
                // Its own page size (default 20), walked by `nextFlag`: the look-up asks 100.
                match symbol {
                    Some(sym) => self.get("spot_plan_current", settings, "/api/v2/spot/trade/current-plan-order", corr).query("symbol", sym),
                    None => self.get("spot_plan_current", settings, "/api/v2/spot/trade/current-plan-order", corr),
                },
            ]
        })
    }

    /// Spot Get Fills ("must not exceed 90 days"); futures Get Order Fill Details ("The maximum
    /// time span supported is three months"), which, unlike the fill history, answers in demo.
    fn fills_since(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let h = if self.futures() { self.get("mix_fills", settings, "/api/v2/mix/order/fills", corr) } else { self.get("spot_fills", settings, "/api/v2/spot/trade/fills", corr) };
        Ok(h.query("symbol", symbol).query("startTime", from_ms).query("endTime", to_ms).query("limit", PAGE))
    }

    /// 90 days on spot; 89 on futures, the shortest three months a calendar has.
    fn fills_window_ms(&self) -> Option<i64> {
        Some(if self.futures() { 89 } else { 90 } * 86_400_000)
    }

    fn instrument_rules(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let mut h = if self.futures() {
            self.public("mix_contracts", settings, "/api/v2/mix/market/contracts", corr).query("productType", FUTURES)
        } else {
            self.public("spot_symbols", settings, "/api/v2/spot/public/symbols", corr)
        };
        if let Some(s) = symbol {
            h = h.query("symbol", s);
        }
        Ok(h)
    }

    /// A market order fills at once: fill-or-kill on it is ignored by the venue ("invalid when
    /// orderType is market", spot) or not described (futures), so it is refused. Plan orders take
    /// no `force` at all: good till cancelled only.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        if o.order_type == OrderType::Market && o.tif == TimeInForce::Fok {
            return Err(ConnectorError::unsupported(format!(
                "{} takes no fill-or-kill on a market order (it fills at once): send it IOC or GTC, or as a limit order FOK",
                self.caps.label
            )));
        }
        if o.order_type.triggered() && o.tif != TimeInForce::Gtc {
            return Err(ConnectorError::unsupported(format!("{} stop orders carry no time in force: send it GTC", self.caps.label)));
        }
        Ok(())
    }

    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        next.query.retain(|(k, _)| k != "idLessThan");
        next.query.push(("idLessThan".into(), cursor.to_string()));
        next
    }

    fn positions(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.futures() {
            return Ok(None);
        }
        Ok(Some(self.get("mix_position", settings, "/api/v2/mix/position/single-position", corr).query("symbol", symbol).query("marginCoin", MARGIN_COIN)))
    }

    fn all_positions(&self, settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if !self.futures() {
            return Ok(Vec::new());
        }
        Ok(vec![self.get("mix_all_position", settings, "/api/v2/mix/position/all-position", corr).query("marginCoin", MARGIN_COIN)])
    }

    /// `43075` "Position pattern mismatch", `45021` "When one-way position is held, the order
    /// type must also be one-way position type", `45109` "The current account is a two-way
    /// position", `45020` "Liquidation can only occur under two-way positions".
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.futures() && matches!(e.code.as_deref(), Some("43075" | "45021" | "45109" | "45020"))
    }

    /// Get Single Account: `marginMode` for the symbol and its `marginCoin`.
    fn symbol_margin(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.futures() {
            return Ok(None);
        }
        Ok(Some(self.get("mix_account", settings, "/api/v2/mix/account/account", corr).query("symbol", symbol).query("marginCoin", MARGIN_COIN)))
    }

    /// An answer that names no margin mode is refused: the order needs it and OTW never picks one.
    fn decode_symbol_margin(&self, symbol: &str, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("account", e))?;
        let d = &v["data"];
        margin_of(&text(&d["marginMode"]), &text(&d["marginCoin"]))
            .map(Some)
            .ok_or_else(|| ConnectorError::invalid(format!("Bitget did not say whether {symbol} is margined crossed or isolated on this account, so nothing was sent: check it at Bitget")))
    }

    /// `40716` "This trading pair not support Cross Margin mode", `22005` "This symbol does not
    /// support cross mode": the margin read is out of date.
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.futures() && matches!(e.code.as_deref(), Some("40716" | "22005"))
    }

    /// Get Single Account's `posMode`, the product type's: read with no position open.
    fn account_position_mode(&self, settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.futures() {
            return Ok(None);
        }
        Ok(Some(self.get("mix_account", settings, "/api/v2/mix/account/account", corr).query("symbol", MODE_SYMBOL).query("marginCoin", MARGIN_COIN)))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("account", e))?;
        Ok(match v["data"]["posMode"].as_str() {
            Some("one_way_mode") => Some(PositionMode::OneWay),
            Some("hedge_mode") => Some(PositionMode::Hedge),
            _ => None,
        })
    }

    fn market_buy_in_quote(&self, o: &OrderRequest) -> bool {
        !self.futures() && o.order_type == OrderType::Market && o.side == Side::Buy
    }

    /// The best ask (a buy spends at it) from Get Ticker Information.
    fn reference_price(&self, settings: &Settings, symbol: &str, _side: Side, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.futures() {
            return Ok(None);
        }
        Ok(Some(self.public("spot_ticker", settings, "/api/v2/spot/market/tickers", corr).query("symbol", symbol)))
    }

    fn decode_reference_price(&self, body: &[u8], side: Side) -> Result<Decimal, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
        let row = v["data"].as_array().and_then(|a| a.first()).ok_or_else(|| ConnectorError::decode("bitget tickers: no row"))?;
        let field = if side == Side::Buy { "askPr" } else { "bidPr" };
        text(&row[field]).trim().parse::<Decimal>().map_err(|_| ConnectorError::decode(format!("bitget tickers: no {field}")))
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("order ack", e))?;
        let d = &v["data"];
        let id = text(&d["orderId"]);
        let cid = Some(text(&d["clientOid"])).filter(|c| !c.is_empty());
        // A one-way reduce-only order that made the venue cancel older ones: "the response for
        // the latest reduce-only order request will not include an orderId".
        Ok(if id.is_empty() { Ack::without_id(cid) } else { Ack { venue_id: id, client_id: cid } })
    }

    fn decode_ack_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Ack, ConnectorError> {
        let mut ack = self.decode_ack(body)?;
        if ack.has_id() && conditional(ctx.endpoint) {
            ack.venue_id = OrderSpace::Conditional.tag(&ack.venue_id);
        }
        Ok(ack)
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_order_rows("", body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_order_rows(ctx.endpoint, body)
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let (rows, next) = rows_and_cursor(body, "fills")?;
        let out = rows.into_iter().map(|r| serde_json::from_value::<FillRow>(r).map_err(|e| decode_err("fill row", e)).and_then(|r| self.fill_row(r))).collect::<Result<Vec<_>, _>>()?;
        Ok((out, next))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let (rows, _) = rows_and_cursor(body, "symbols")?;
        let out = rows.into_iter().map(|r| serde_json::from_value::<RuleRow>(r).map_err(|e| decode_err("symbol row", e))).collect::<Result<Vec<_>, _>>()?;
        Ok((out.into_iter().filter(|r| !r.symbol.is_empty()).map(|r| self.rule_row(r)).collect(), None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let (rows, _) = rows_and_cursor(body, "positions")?;
        rows.into_iter().map(|r| serde_json::from_value::<PositionRow>(r).map(Self::position_row).map_err(|e| decode_err("position row", e))).collect()
    }

    /// Plain orders: `live` (and `new`, the unfilled-orders example's word), `partially_filled`,
    /// `filled`, `cancelled` (spot) or `canceled` (futures). Plan orders: `live` / `not_trigger`
    /// (waiting), `executing` (placing its order), `executed` ("the order triggered"),
    /// `fail_execute` (the venue dropped it), `cancelled`. Bitget has no rejected state.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "live" | "new" | "not_trigger" | "executing" => OrderStatus::New,
            "partially_filled" => OrderStatus::PartiallyFilled,
            "filled" | "executed" => OrderStatus::Filled,
            "cancelled" | "canceled" | "fail_execute" => OrderStatus::Cancelled,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    /// `wss://ws.bitget.com/v2/ws/private` (demo: `wss://wspap.bitget.com/v2/ws/private`), one
    /// subscribe frame per channel (`orders`, `fill`, and `positions` on futures, every symbol
    /// with `instId` `default`), the string `ping` every 30 s.
    fn private_socket(&self, settings: &Settings) -> Option<PrivateSocket> {
        let base = if is_demo(settings) { super::WS_DEMO } else { super::WS };
        let mut channels = vec!["orders", "fill"];
        if self.futures() {
            channels.push("positions");
        }
        let frame = |c: &str| json!({ "op": "subscribe", "args": [{ "instType": self.book.inst_type(), "channel": c, "instId": "default" }] }).to_string().into_bytes();
        let mut s = PrivateSocket::new(base, "/v2/ws/private", frame(channels[0]), b"ping".to_vec(), PING_EVERY_MS);
        s.more = channels[1..].iter().map(|c| frame(c)).collect();
        Some(s)
    }

    /// The `orders`, `fill` and `positions` channels as pushed, the answers to `login`
    /// (`{"event":"login","code":"0"}`), `subscribe` and an `error` event, and the `pong` string.
    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        if payload.trim_ascii() == b"pong" {
            return Ok(vec![PrivateEvent::Answer { op: "ping".into(), ok: true, message: String::new() }]);
        }
        let v: Value = serde_json::from_slice(payload).map_err(|e| decode_err("private frame", e))?;
        if let Some(event) = v.get("event").and_then(Value::as_str) {
            let code = text(v.get("code").unwrap_or(&Value::Null));
            let message = text(v.get("msg").unwrap_or(&Value::Null));
            return Ok(vec![match event {
                "login" => PrivateEvent::Answer { op: "auth".into(), ok: code.is_empty() || code == "0", message },
                "subscribe" => PrivateEvent::Answer { op: "subscribe".into(), ok: code.is_empty() || code == "0", message },
                // A refusal: of a subscription when it names its channel, of the login otherwise.
                _ => PrivateEvent::Answer { op: if v.get("arg").is_some() { "subscribe".into() } else { "auth".into() }, ok: false, message: format!("{message} (code {code})") },
            }]);
        }
        let arg = &v["arg"];
        if text(&arg["instType"]) != self.book.inst_type() {
            return Ok(Vec::new());
        }
        let rows = v["data"].as_array().cloned().unwrap_or_default();
        let mut out = Vec::new();
        for row in rows {
            match text(&arg["channel"]).as_str() {
                "orders" => {
                    let r: OrderRow = serde_json::from_value(row).map_err(|e| decode_err("order push", e))?;
                    out.push(PrivateEvent::Order(self.order_row("ws_orders", r)));
                }
                "fill" => {
                    let r: FillRow = serde_json::from_value(row).map_err(|e| decode_err("fill push", e))?;
                    out.push(PrivateEvent::Fill(self.fill_row(r)?));
                }
                "positions" if self.futures() => {
                    let r: PositionRow = serde_json::from_value(row).map_err(|e| decode_err("position push", e))?;
                    out.push(PrivateEvent::Position(Self::position_row(r)));
                }
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
