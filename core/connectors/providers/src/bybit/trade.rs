//! Bybit v5 orders (spot and linear books), written from the documentation alone: no account,
//! no sandbox key. Every field below is the doc's; each claim is sourced in PROVIDER.md
//! ("Trading"). The order path itself is the shared trader's; this file only says what the
//! calls are and what the answers mean.
//!
//! - Place: `POST /v5/order/create`. A stop is `Market` with `triggerPrice` and the direction it
//!   triggers in (`triggerDirection`, linear only), a stop-limit `Limit` with both prices.
//! - Client id: `orderLinkId`, at most 36 letters, digits, `-` and `_`, the OTW key unchanged.
//! - A spot market order's size is in the base coin (`marketUnit = baseCoin`): Bybit reads a
//!   spot market buy in the quote coin otherwise.
//! - Linear position mode: read from `/v5/position/list` on the symbol (one row `positionIdx`
//!   0, or two rows 1 and 2), sent as `positionIdx`, never switched.
//! - Look up: `/v5/order/realtime` (open orders and the last 500 closed), then
//!   `/v5/order/history`, by `orderLinkId` or `orderId`.
//! - Bybit EU: the spot book on `api.bybit.eu` / `api-testnet.bybit.eu` (`site` `eu` /
//!   `eu-testnet`), the same calls; polled only (the doc names no EU private stream host), and
//!   its `extraFees` read as fee lines of their own.

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderRequest, OrderStatus, OrderType, Side, TimeInForce};
use otw_connector_sdk::parse::{Int, OptDec};
use otw_connector_sdk::trade::{
    Ack, ClientIdRule, ErrorRow, ExitChange, ExitKind, FeeLine, OrderSpace, Page, Placing, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder,
    VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{Bybit, Category};

/// The `orderFilter` a spot order of this `stopOrderType` is cancelled under: a spot cancel
/// takes "`Order` by default", so a conditional one names its kind. `None`: a plain order.
pub(super) fn spot_filter(stop_order_type: &str) -> Option<&'static str> {
    match stop_order_type {
        "" | "UNKNOWN" => None,
        "tpslOrder" => Some("tpslOrder"),
        "OcoOrder" => Some("OcoOrder"),
        "BidirectionalTpslOrder" => Some("BidirectionalTpslOrder"),
        _ => Some("StopOrder"),
    }
}

/// The id a spot order is followed by: a conditional one `cond:<orderFilter>:<orderId>`
/// ([`OrderSpace`]), so its cancel names its kind.
pub(super) fn spot_order_id(order_id: String, stop_order_type: &str) -> String {
    match spot_filter(stop_order_type) {
        Some(f) => OrderSpace::Conditional.tag(&format!("{f}:{order_id}")),
        None => order_id,
    }
}

/// The venue's order id and, for a spot conditional order, its `orderFilter`.
fn spot_split(venue_id: &str) -> (&str, Option<&str>) {
    match OrderSpace::of(venue_id) {
        (OrderSpace::Conditional, rest) => match rest.split_once(':') {
            Some((f, id)) => (id, Some(f)),
            None => (rest, None),
        },
        (OrderSpace::Plain, id) => (id, None),
    }
}

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("bybit {what}: {e}"))
}

#[derive(Deserialize)]
struct Answer<T> {
    result: T,
}

#[derive(Deserialize)]
struct List<T> {
    #[serde(default = "Vec::new")]
    list: Vec<T>,
    #[serde(rename = "nextPageCursor", default)]
    next_page_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AckRow {
    order_id: String,
    #[serde(default)]
    order_link_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    order_id: String,
    #[serde(default)]
    order_link_id: String,
    symbol: String,
    side: String,
    #[serde(default)]
    order_type: String,
    order_status: String,
    #[serde(default)]
    qty: OptDec,
    #[serde(default)]
    cum_exec_qty: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    trigger_price: OptDec,
    #[serde(default)]
    avg_price: OptDec,
    #[serde(default)]
    reduce_only: bool,
    #[serde(default)]
    position_idx: Option<i64>,
    #[serde(default)]
    created_time: Option<Int>,
    /// "the linked parent order for attached take-profit and stop-loss orders".
    #[serde(default)]
    parent_order_link_id: String,
    #[serde(default)]
    stop_order_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FillRow {
    exec_id: String,
    order_id: String,
    #[serde(default)]
    order_link_id: String,
    symbol: String,
    side: String,
    exec_qty: OptDec,
    exec_price: OptDec,
    #[serde(default)]
    exec_fee: OptDec,
    #[serde(default)]
    fee_currency: String,
    #[serde(default)]
    exec_type: String,
    exec_time: Int,
    /// "returned only for ... spot fiat currency orders placed on the EU site": a string over
    /// REST, a list on the private stream (Get Trade History, Execution stream).
    #[serde(default)]
    extra_fees: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtraFee {
    #[serde(default)]
    fee_coin: String,
    #[serde(default)]
    fee_type: String,
    #[serde(default)]
    sub_fee_type: String,
    #[serde(default)]
    fee: OptDec,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LotFilter {
    #[serde(default)]
    min_order_qty: OptDec,
    #[serde(default)]
    max_order_qty: OptDec,
    #[serde(default)]
    max_mkt_order_qty: OptDec,
    #[serde(default)]
    qty_step: OptDec,
    #[serde(default)]
    min_notional_value: OptDec,
    // Spot.
    #[serde(default)]
    base_precision: OptDec,
    #[serde(default)]
    min_order_amt: OptDec,
    #[serde(default)]
    max_limit_order_qty: OptDec,
    #[serde(default)]
    max_market_order_qty: OptDec,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PriceFilter {
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    min_price: OptDec,
    #[serde(default)]
    max_price: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleRow {
    symbol: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    lot_size_filter: Option<LotFilter>,
    #[serde(default)]
    price_filter: Option<PriceFilter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionRow {
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    size: OptDec,
    position_idx: i64,
    /// "for portfolio margin mode, this field returns \"\"".
    #[serde(default)]
    leverage: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreCheck {
    #[serde(default)]
    pre_imr_e4: OptDec,
    #[serde(default)]
    post_imr_e4: OptDec,
    #[serde(default)]
    pre_mmr_e4: OptDec,
    #[serde(default)]
    post_mmr_e4: OptDec,
}

#[derive(Deserialize)]
struct Push {
    #[serde(default)]
    topic: String,
    #[serde(default)]
    data: Vec<Value>,
    /// An answer to a frame OTW sent: `auth`, `subscribe`, `ping` / `pong`.
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    ret_msg: Option<String>,
}

/// How often the private stream is pinged ("send the ping heartbeat packet every 20 seconds").
const PING_EVERY_MS: u64 = 20_000;

fn side_of(word: &str) -> Mapped<Side> {
    match word {
        "Buy" => Mapped::Known(Side::Buy),
        "Sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

fn position_of(idx: i64) -> Result<PositionSide, ConnectorError> {
    match idx {
        0 => Ok(PositionSide::Net),
        1 => Ok(PositionSide::Long),
        2 => Ok(PositionSide::Short),
        other => Err(ConnectorError::decode(format!("bybit: positionIdx {other} is not 0, 1 or 2"))),
    }
}

/// Bybit EU's `extraFees` (enum `extraFeesFeeType`: `WHT` "EU withholding tax",
/// `AUT_WITHHOLDING_TAX`), each a fee line of its own beside `execFee` (the doc does not say
/// whether `execFee` includes it, so nothing is netted). Absent, `""` or `[]`: none. The stream
/// sends a list; REST a string, whose non-empty form the doc never shows: read when it is the
/// same list written as JSON. Anything else, or a line with no amount, is left out and logged
/// (never a guess at a fee): only that fee is lost, the execution itself is kept.
fn extra_fees(v: &Value) -> Vec<FeeLine> {
    let list = match v {
        Value::Null => return Vec::new(),
        Value::String(t) if t.trim().is_empty() => return Vec::new(),
        Value::String(t) => match serde_json::from_str::<Value>(t) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("bybit extraFees {t:?} is not a fee list ({e}): the extra fee is left out, the fill is kept");
                return Vec::new();
            }
        },
        other => other.clone(),
    };
    let rows: Vec<Value> = match list {
        Value::Array(rows) => rows,
        other => {
            tracing::warn!("bybit extraFees {other} is not a fee list: the extra fee is left out, the fill is kept");
            return Vec::new();
        }
    };
    rows.into_iter()
        .filter_map(|row| {
            let f = match serde_json::from_value::<ExtraFee>(row.clone()) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("bybit extraFees line {row} is not read ({e}): left out, the fill is kept");
                    return None;
                }
            };
            let Some(fee) = f.fee.0 else {
                tracing::warn!("bybit extraFees: a {} line carries no fee, left out (the fill is kept)", f.fee_type);
                return None;
            };
            let kind = if f.sub_fee_type.is_empty() { f.fee_type } else { format!("{} {}", f.fee_type, f.sub_fee_type) };
            Some(FeeLine { fee, currency: f.fee_coin, kind })
        })
        .collect()
}

/// A positive price, or none: Bybit writes "0" and "0.00" for a price an order does not have.
fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

impl Bybit {
    fn trades(&self) -> Result<(), ConnectorError> {
        match self.category {
            // Bybit EU: its spot book, through the same v5 spot calls on its own host.
            Category::Spot => Ok(()),
            Category::Linear if !self.eu => Ok(()),
            _ => Err(ConnectorError::unsupported(format!("{} takes no orders from OpenTraderWorld", self.caps.label))),
        }
    }

    fn base(&self, s: &Settings) -> Result<&'static str, ConnectorError> {
        self.site_base(s.get("site").map(String::as_str))
    }

    fn post(&self, endpoint: &'static str, s: &Settings, path: &str, body: Value, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let bytes = serde_json::to_vec(&body).map_err(|e| ConnectorError::invalid(format!("bybit {endpoint}: {e}")))?;
        let mut h = HttpRequest::get(endpoint, self.base(s)?, path, corr).signed();
        h.method = Method::Post;
        h.headers.push(("Content-Type".into(), "application/json".into()));
        h.body = Some(bytes.into());
        Ok(h)
    }

    fn get(&self, endpoint: &'static str, s: &Settings, path: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(HttpRequest::get(endpoint, self.base(s)?, path, corr).signed().query("category", self.category.name()))
    }

    /// The create-order body for `o` (create-order request parameters).
    fn order_body(&self, o: &OrderRequest, p: &Placing) -> Result<Value, ConnectorError> {
        self.trades()?;
        let mut b = Map::new();
        b.insert("category".into(), json!(self.category.name()));
        b.insert("symbol".into(), json!(o.instrument.native));
        b.insert("side".into(), json!(if o.side == Side::Buy { "Buy" } else { "Sell" }));
        let (kind, trigger) = match o.order_type {
            OrderType::Market => ("Market", false),
            OrderType::Limit => ("Limit", false),
            OrderType::Stop if self.category == Category::Linear => ("Market", true),
            OrderType::StopLimit if self.category == Category::Linear => ("Limit", true),
            other => return Err(ConnectorError::unsupported(format!("{} takes no {other:?} order", self.caps.label))),
        };
        b.insert("orderType".into(), json!(kind));
        b.insert("qty".into(), json!(dec_text(o.qty.value())));
        if kind == "Limit" {
            let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
            b.insert("price".into(), json!(dec_text(px.value())));
            let tif = match o.tif {
                TimeInForce::Gtc => "GTC",
                TimeInForce::Ioc => "IOC",
                TimeInForce::Fok => "FOK",
                other => return Err(ConnectorError::unsupported(format!("{} takes no {:?} time in force", self.caps.label, other.kind()))),
            };
            b.insert("timeInForce".into(), json!(tif));
        } else if !matches!(o.tif, TimeInForce::Gtc | TimeInForce::Ioc | TimeInForce::Fok) {
            // A market order always works IOC at Bybit: its time in force is not sent.
            return Err(ConnectorError::unsupported(format!("{} takes no {:?} time in force", self.caps.label, o.tif.kind())));
        }
        if trigger {
            let stop = o.stop.ok_or_else(|| ConnectorError::invalid("a stop order needs its stop price"))?;
            b.insert("triggerPrice".into(), json!(dec_text(stop.value())));
            // A buy stop waits for the price to rise to it, a sell stop for it to fall.
            b.insert("triggerDirection".into(), json!(if o.side == Side::Buy { 1 } else { 2 }));
            b.insert("triggerBy".into(), json!("LastPrice"));
        }
        if self.category == Category::Spot && kind == "Market" {
            b.insert("marketUnit".into(), json!("baseCoin"));
        }
        if self.category == Category::Linear {
            let idx = match p.position {
                PositionSide::Net => 0,
                PositionSide::Long => 1,
                PositionSide::Short => 2,
            };
            b.insert("positionIdx".into(), json!(idx));
            if o.opts.reduce_only {
                b.insert("reduceOnly".into(), json!(true));
                // A reduce-only stop (a protective stop loss): "a triggered conditional order is
                // cancelled if the margin is insufficient"; `closeOnTrigger` makes it reduce the
                // position regardless, at the cost of cancelling or shrinking other active orders
                // of similar contracts when margin is short (create-order, closeOnTrigger).
                if trigger {
                    b.insert("closeOnTrigger".into(), json!(true));
                }
            }
        } else if o.opts.reduce_only {
            return Err(ConnectorError::unsupported(format!("{} takes no reduce-only order: a spot sale of what is held is a plain sell", self.caps.label)));
        }
        // A dry run carries no client id: the venue must not see the live order's before it.
        if !p.client_id.is_empty() {
            b.insert("orderLinkId".into(), json!(p.client_id));
        }
        Ok(Value::Object(b))
    }

    fn order_row(&self, r: OrderRow) -> Result<VenueOrder, ConnectorError> {
        let limit = if r.order_type == "Limit" { positive(r.price) } else { None };
        let venue_id = if self.category == Category::Spot { spot_order_id(r.order_id, &r.stop_order_type) } else { r.order_id };
        Ok(VenueOrder {
            venue_id,
            client_id: Some(r.order_link_id).filter(|c| !c.is_empty()),
            symbol: r.symbol.to_ascii_uppercase(),
            side: side_of(&r.side),
            order_type: r.order_type,
            status: self.status_of(&r.order_status),
            qty: r.qty.0.unwrap_or_default(),
            filled_qty: r.cum_exec_qty.0.unwrap_or_default(),
            limit,
            stop: positive(r.trigger_price),
            avg_price: positive(r.avg_price),
            reduce_only: r.reduce_only,
            position: position_of(r.position_idx.unwrap_or(0))?,
            created_ms: r.created_time.map(|t| t.0),
            trigger_pending: r.order_status == "Untriggered",
            parent_client_id: Some(r.parent_order_link_id).filter(|c| !c.is_empty()),
            // Enum `stopOrderType`: the exits a position's TP/SL creates.
            exit: match r.stop_order_type.as_str() {
                "TakeProfit" | "PartialTakeProfit" => Some(ExitKind::TakeProfit),
                "StopLoss" | "PartialStopLoss" => Some(ExitKind::StopLoss),
                _ => None,
            },
            successor_id: None,
        })
    }

    /// Enum `execType`: `Trade` fills an order; `Funding` moves cash only and is left out; every
    /// other type (`AdlTrade`, `BustTrade`, `Delivery`, `Settle`, `BlockTrade`, `MovePosition`,
    /// the stock split types, and any new one) moves a position and is kept, marked
    /// `position_only`, never dropped.
    fn fill_row(r: FillRow) -> Result<Option<VenueFill>, ConnectorError> {
        if r.exec_type == "Funding" {
            return Ok(None);
        }
        let position_only = !matches!(r.exec_type.as_str(), "Trade" | "") || r.order_id.is_empty();
        let extra_fees = extra_fees(&r.extra_fees);
        Ok(Some(VenueFill {
            position_only,
            exec_id: r.exec_id,
            venue_order_id: r.order_id,
            client_id: Some(r.order_link_id).filter(|c| !c.is_empty()),
            symbol: r.symbol.to_ascii_uppercase(),
            side: side_of(&r.side),
            qty: r.exec_qty.0.unwrap_or_default(),
            price: r.exec_price.0.unwrap_or_default(),
            fee: r.exec_fee.0.unwrap_or_default(),
            fee_currency: r.fee_currency,
            at_ms: r.exec_time.0,
            extra_fees,
            fee_pending: false,
        }))
    }

    fn rule_row(&self, r: RuleRow) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(self.venue(), &r.symbol.to_ascii_uppercase()));
        let p = r.price_filter.unwrap_or_default();
        let l = r.lot_size_filter.unwrap_or_default();
        v.tick = positive(p.tick_size);
        v.min_price = positive(p.min_price);
        v.max_price = positive(p.max_price);
        if self.category == Category::Spot {
            // Spot: `basePrecision` is the size step; `minOrderQty` and `maxOrderAmt` are no
            // longer checked, `minOrderAmt` and the per-type maximums are.
            v.lot_step = positive(l.base_precision);
            v.min_notional = positive(l.min_order_amt);
            v.max_qty = positive(l.max_limit_order_qty).or(positive(l.max_order_qty));
            v.market_max_qty = positive(l.max_market_order_qty);
        } else {
            v.lot_step = positive(l.qty_step);
            v.min_qty = positive(l.min_order_qty);
            v.max_qty = positive(l.max_order_qty);
            v.market_max_qty = positive(l.max_mkt_order_qty);
            v.min_notional = positive(l.min_notional_value);
        }
        v.status = match r.status.as_str() {
            "Trading" => TradingStatus::Trading,
            // `Delivering` and every other word: no new order. The status enum gives
            // `Delivering` no meaning, and no page says a reduce-only order is accepted in it
            // (error 110042 names a "pre-delivery status" without tying it to this word), so it
            // is halted: cancels still go out.
            other => TradingStatus::Closed(other.to_string()),
        };
        v
    }
}

impl TradeMapper for Bybit {
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(36)
    }

    fn is_sandbox(&self, settings: &Settings) -> bool {
        settings.get("site").is_some_and(|s| s.trim().to_ascii_lowercase().ends_with("testnet"))
    }

    fn place(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let body = self.order_body(o, p)?;
        self.post("order_create", settings, "/v5/order/create", body, p.corr)
    }

    /// Linear, `tpslMode=Full` only: "entire position for TP/SL", each fired as a market order
    /// (`tpOrderType`/`slOrderType` `Market`: "For tpslMode=Full, it only supports
    /// tpOrderType=Market"), triggered on the last price (`tpTriggerBy`/`slTriggerBy` "default:
    /// `LastPrice`"). "When reduceOnly is true, take profit/stop loss cannot be set": refused. A
    /// conditional entry is refused too: the doc shows TP/SL on market and limit entries only.
    fn place_with_exits(&self, settings: &Settings, entry: &OrderRequest, exits: &[(ExitKind, Decimal)], p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if self.category != Category::Linear {
            return Err(ConnectorError::unsupported(format!("{} takes no attached take profit or stop loss", self.caps.label)));
        }
        if entry.opts.reduce_only || !matches!(entry.order_type, OrderType::Market | OrderType::Limit) {
            return Err(ConnectorError::unsupported(format!("{} attaches a take profit or stop loss to a market or limit entry that is not reduce-only", self.caps.label)));
        }
        let mut b = self.order_body(entry, p)?;
        if let Value::Object(m) = &mut b {
            m.insert("tpslMode".into(), json!("Full"));
            for (kind, price) in exits {
                let (field, order) = match kind {
                    ExitKind::TakeProfit => ("takeProfit", "tpOrderType"),
                    ExitKind::StopLoss => ("stopLoss", "slOrderType"),
                };
                m.insert(field.into(), json!(dec_text(*price)));
                m.insert(order.into(), json!("Market"));
            }
        }
        self.post("order_create", settings, "/v5/order/create", b, p.corr)
    }

    /// Set Trading Stop: the position's take profit or stop loss, `tpslMode=Full`; "0 means
    /// cancel" it.
    fn change_exit(&self, settings: &Settings, x: &ExitChange<'_>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let (symbol, position, kind, price) = (x.symbol, x.position, x.kind, x.price);
        if self.category != Category::Linear {
            return Err(ConnectorError::unsupported(format!("{} takes no attached take profit or stop loss", self.caps.label)));
        }
        let idx = match position {
            PositionSide::Net => 0,
            PositionSide::Long => 1,
            PositionSide::Short => 2,
        };
        let value = price.map(dec_text).unwrap_or_else(|| "0".into());
        let mut b = Map::new();
        b.insert("category".into(), json!("linear"));
        b.insert("symbol".into(), json!(symbol));
        b.insert("tpslMode".into(), json!("Full"));
        b.insert("positionIdx".into(), json!(idx));
        match kind {
            ExitKind::TakeProfit => {
                b.insert("takeProfit".into(), json!(value));
                b.insert("tpOrderType".into(), json!("Market"));
            }
            ExitKind::StopLoss => {
                b.insert("stopLoss".into(), json!(value));
                b.insert("slOrderType".into(), json!("Market"));
            }
        }
        self.post("trading_stop", settings, "/v5/position/trading-stop", Value::Object(b), corr)
    }

    /// The last 500 closed orders ("`openOnly`=1"): where an exit that ended is found.
    fn recent_orders(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        Ok(vec![self.get("order_realtime", settings, "/v5/order/realtime", corr)?.query("symbol", symbol).query("openOnly", 1).query("limit", 50)])
    }

    /// A spot conditional order (`cond:<orderFilter>:<orderId>`) names its `orderFilter`
    /// ("Spot trading only ... If not passed, `Order` by default").
    fn cancel(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.trades()?;
        let (id, filter) = spot_split(venue_id);
        let mut b = json!({ "category": self.category.name(), "symbol": symbol, "orderId": id });
        if let (Some(f), Category::Spot) = (filter, self.category) {
            b["orderFilter"] = json!(f);
        }
        self.post("order_cancel", settings, "/v5/order/cancel", b, corr)
    }

    fn cancel_all(&self, settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        let mut out = Vec::new();
        for sym in symbols {
            if self.category == Category::Spot {
                // Spot cancels plain orders only unless told which kind (cancel-all, `orderFilter`).
                for filter in ["Order", "tpslOrder", "StopOrder", "OcoOrder", "BidirectionalTpslOrder"] {
                    out.push(self.post("order_cancel_all", settings, "/v5/order/cancel-all", json!({ "category": "spot", "symbol": sym, "orderFilter": filter }), corr)?);
                }
            } else {
                out.push(self.post("order_cancel_all", settings, "/v5/order/cancel-all", json!({ "category": self.category.name(), "symbol": sym }), corr)?);
            }
        }
        Ok(out)
    }

    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        Ok(vec![
            self.get("order_realtime", settings, "/v5/order/realtime", corr)?.query("symbol", symbol).query("orderLinkId", client_id),
            self.get("order_history", settings, "/v5/order/history", corr)?.query("symbol", symbol).query("orderLinkId", client_id),
        ])
    }

    fn query_by_venue_id(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        let (venue_id, _) = spot_split(venue_id);
        Ok(vec![
            self.get("order_realtime", settings, "/v5/order/realtime", corr)?.query("symbol", symbol).query("orderId", venue_id),
            self.get("order_history", settings, "/v5/order/history", corr)?.query("symbol", symbol).query("orderId", venue_id),
        ])
    }

    fn open_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        let first = || self.get("order_realtime", settings, "/v5/order/realtime", corr).map(|h| h.query("limit", 50));
        Ok(match (symbol, self.category) {
            (Some(s), _) => vec![first()?.query("symbol", s)],
            (None, Category::Spot) => vec![first()?],
            // Linear lists the whole book by settle coin only.
            (None, _) => vec![first()?.query("settleCoin", "USDT"), first()?.query("settleCoin", "USDC")],
        })
    }

    /// "If both are passed, the rule is endTime - startTime <= 7 days" (Get Trade History).
    fn fills_since(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.trades()?;
        Ok(self.get("execution_list", settings, "/v5/execution/list", corr)?.query("symbol", symbol).query("startTime", from_ms).query("endTime", to_ms).query("limit", 100))
    }

    fn fills_window_ms(&self) -> Option<i64> {
        Some(7 * 86_400_000)
    }

    /// "Market order will always use IOC" (create-order): a market or stop (market once
    /// triggered) order asked fill-or-kill would be worked IOC, so it is refused. GTC is the
    /// value "used by default" when none is passed and IOC what it works as: both are taken.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        if matches!(o.order_type, OrderType::Market | OrderType::Stop) && o.tif == TimeInForce::Fok {
            return Err(ConnectorError::unsupported(format!(
                "{} works every market order immediate-or-cancel, never fill-or-kill: send it IOC, or as a limit order FOK",
                self.caps.label
            )));
        }
        Ok(())
    }

    /// Cancel All answers `success`: "1" success, "0" fail (inverse on UTA1.0 sends none).
    fn decode_cancel_all(&self, body: &[u8]) -> Result<(), ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("cancel-all", e))?;
        match v["result"]["success"].as_str() {
            Some("0") => Err(ConnectorError::new(otw_connector_model::ErrorKind::Unknown, "bybit cancel-all: success \"0\"")),
            _ => Ok(()),
        }
    }

    /// Linear lists `Trading` and `PendingOpen` by default; asked by status it lists the
    /// others (`PreLaunch`, `Delivering`, `Closed`). Spot "has `Trading` only": nothing more.
    fn instrument_rules_any_status(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        if self.category == Category::Spot {
            return Ok(Vec::new());
        }
        ["PreLaunch", "Delivering", "Closed"].iter().map(|st| self.instrument_rules(settings, Some(symbol), corr).map(|h| h.query("status", st))).collect()
    }

    fn instrument_rules(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.trades()?;
        let mut h = HttpRequest::get("instruments", self.base(settings)?, "/v5/market/instruments-info", corr).query("category", self.category.name());
        if let Some(s) = symbol {
            h = h.query("symbol", s);
        }
        if self.category != Category::Spot {
            h = h.query("limit", 1000);
        }
        Ok(h)
    }

    fn positions(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.trades()?;
        if self.category == Category::Spot {
            return Ok(None);
        }
        Ok(Some(self.get("position_list", settings, "/v5/position/list", corr)?.query("symbol", symbol)))
    }

    /// Get Account Info: `marginMode` `ISOLATED_MARGIN`, `REGULAR_MARGIN` (cross) or
    /// `PORTFOLIO_MARGIN`, account wide (the position's `tradeMode` is "Deprecated, always 0").
    fn margin_mode(&self, settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.trades()?;
        if self.category == Category::Spot {
            return Ok(None);
        }
        Ok(Some(HttpRequest::get("account_info", self.base(settings)?, "/v5/account/info", corr).signed()))
    }

    fn decode_margin_mode(&self, body: &[u8]) -> Result<Option<String>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("account info", e))?;
        Ok(match v["result"]["marginMode"].as_str() {
            Some("ISOLATED_MARGIN") => Some("isolated".into()),
            Some("REGULAR_MARGIN") => Some("cross".into()),
            Some("PORTFOLIO_MARGIN") => Some("portfolio".into()),
            Some(other) if !other.is_empty() => Some(other.to_ascii_lowercase()),
            _ => None,
        })
    }

    /// Linear: every held position, by settle coin ("If `symbol`=null and `settleCoin` specified,
    /// it returns position size greater than zero").
    fn all_positions(&self, settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.trades()?;
        if self.category == Category::Spot {
            return Ok(Vec::new());
        }
        let page = |coin: &str| self.get("position_list", settings, "/v5/position/list", corr).map(|h| h.query("settleCoin", coin).query("limit", 200));
        Ok(vec![page("USDT")?, page("USDC")?])
    }

    /// The doc names no code for an order whose `positionIdx` does not match the account's mode;
    /// a parameter error (`10001`) on an order is the one it can be, so the mode is read again
    /// before the next order (the refused one is not sent again).
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.category == Category::Linear && e.code.as_deref() == Some("10001")
    }

    /// Linear only, and never for a conditional order (pre-check: "Conditional order is not
    /// supported").
    fn dry_run(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<Option<HttpRequest>, ConnectorError> {
        if self.category != Category::Linear || o.order_type.triggered() {
            return Ok(None);
        }
        let body = self.order_body(o, p)?;
        self.post("order_pre_check", settings, "/v5/order/pre-check", body, p.corr).map(Some)
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let a: Answer<AckRow> = serde_json::from_slice(body).map_err(|e| decode_err("order ack", e))?;
        if a.result.order_id.is_empty() {
            return Err(ConnectorError::decode("bybit order ack: no orderId"));
        }
        Ok(Ack { venue_id: a.result.order_id, client_id: a.result.order_link_id.filter(|c| !c.is_empty()) })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let a: Answer<List<OrderRow>> = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
        let rows = a.result.list.into_iter().map(|r| self.order_row(r)).collect::<Result<Vec<_>, _>>()?;
        Ok((rows, a.result.next_page_cursor.filter(|c| !c.is_empty())))
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let a: Answer<List<FillRow>> = serde_json::from_slice(body).map_err(|e| decode_err("executions", e))?;
        let rows = a.result.list.into_iter().map(Self::fill_row).collect::<Result<Vec<_>, _>>()?;
        Ok((rows.into_iter().flatten().collect(), a.result.next_page_cursor.filter(|c| !c.is_empty())))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let a: Answer<List<RuleRow>> = serde_json::from_slice(body).map_err(|e| decode_err("instruments-info", e))?;
        let next = if self.category == Category::Spot { None } else { a.result.next_page_cursor.filter(|c| !c.is_empty()) };
        Ok((a.result.list.into_iter().filter(|r| !r.symbol.is_empty()).map(|r| self.rule_row(r)).collect(), next))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let a: Answer<List<PositionRow>> = serde_json::from_slice(body).map_err(|e| decode_err("positions", e))?;
        a.result
            .list
            .into_iter()
            .map(|r| {
                Ok(VenuePosition {
                    symbol: r.symbol.to_ascii_uppercase(),
                    side: position_of(r.position_idx)?,
                    direction: side_of(&r.side).known().copied(),
                    qty: r.size.0.unwrap_or_default().abs(),
                    leverage: positive(r.leverage),
                    margin: None,
                })
            })
            .collect()
    }

    fn decode_dry_run(&self, body: &[u8]) -> Result<String, ConnectorError> {
        let a: Answer<PreCheck> = serde_json::from_slice(body).map_err(|e| decode_err("pre-check", e))?;
        // Rates "keep four decimal places": 30 is 0.30%.
        let pct = |d: OptDec| d.0.map(|v| format!("{}%", (v / Decimal::ONE_HUNDRED).normalize())).unwrap_or_else(|| "?".into());
        let r = a.result;
        Ok(format!(
            "initial margin rate {} to {}, maintenance margin rate {} to {}",
            pct(r.pre_imr_e4),
            pct(r.post_imr_e4),
            pct(r.pre_mmr_e4),
            pct(r.post_mmr_e4)
        ))
    }

    /// Enum `orderStatus`. `Untriggered` (a conditional order created, waiting) is `New` with
    /// `trigger_pending` set on the order; `Triggered` ("instantaneous state ... from
    /// Untriggered to New") is `New`; `Deactivated` (cancelled before it triggered) and spot's
    /// `PartiallyFilledCanceled` are `Cancelled`.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "New" | "Untriggered" | "Triggered" => OrderStatus::New,
            "PartiallyFilled" => OrderStatus::PartiallyFilled,
            "Filled" => OrderStatus::Filled,
            "Cancelled" | "PartiallyFilledCanceled" | "Deactivated" => OrderStatus::Cancelled,
            "Rejected" => OrderStatus::Rejected,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    /// `wss://<site stream host>/v5/private`, the book's categorised topics (`order.spot`,
    /// `execution.linear`, `position.linear`), a `ping` every 20 s (websocket Connect page). A
    /// site the page names no stream host for (`bytick`, `ae`) has none: polled only.
    fn private_socket(&self, settings: &Settings) -> Option<PrivateSocket> {
        if self.trades().is_err() {
            return None;
        }
        let site = settings.get("site").map(|s| s.trim().to_ascii_lowercase()).unwrap_or_default();
        let base = match site.as_str() {
            "" | "global" => super::WS,
            "testnet" => "ws-testnet",
            "tr" => "ws-tr",
            "id" => "ws-id",
            "kz" => "ws-kz",
            "ge" => "ws-ge",
            "jp" => "ws-jp",
            "hk" => "ws-hk",
            _ => return None,
        };
        let c = self.category.name();
        let mut topics = vec![format!("order.{c}"), format!("execution.{c}")];
        if self.category == Category::Linear {
            topics.push(format!("position.{c}"));
        }
        Some(PrivateSocket::new(
            base,
            "/v5/private",
            json!({ "op": "subscribe", "args": topics }).to_string().into_bytes(),
            json!({ "op": "ping" }).to_string().into_bytes(),
            PING_EVERY_MS,
        ))
    }

    /// The private `order`, `execution` and `position` topics, as pushed (stream examples), and
    /// the answers to `auth`, `subscribe` and `ping`.
    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let push = serde_json::from_slice::<Push>(payload).map_err(|e| decode_err("private frame", e))?;
        if let Some(op) = push.op.clone().filter(|_| push.topic.is_empty()) {
            // A pong carries no `success`; an auth or subscribe answer does.
            let ok = op == "pong" || push.success.unwrap_or(false) || push.ret_msg.as_deref() == Some("pong");
            return Ok(vec![PrivateEvent::Answer { op, ok, message: push.ret_msg.unwrap_or_default() }]);
        }
        let mut out = Vec::new();
        let topic = push.topic.split('.').next().unwrap_or_default();
        for row in push.data {
            let category = row.get("category").and_then(Value::as_str).unwrap_or_default();
            if category != self.category.name() {
                continue;
            }
            match topic {
                "order" => {
                    let r: OrderRow = serde_json::from_value(row).map_err(|e| decode_err("order push", e))?;
                    out.push(PrivateEvent::Order(self.order_row(r)?));
                }
                "execution" => {
                    let r: FillRow = serde_json::from_value(row).map_err(|e| decode_err("execution push", e))?;
                    if let Some(f) = Self::fill_row(r)? {
                        out.push(PrivateEvent::Fill(f));
                    }
                }
                "position" => {
                    let r: PositionRow = serde_json::from_value(row).map_err(|e| decode_err("position push", e))?;
                    out.push(PrivateEvent::Position(VenuePosition {
                        symbol: r.symbol.to_ascii_uppercase(),
                        side: position_of(r.position_idx)?,
                        direction: side_of(&r.side).known().copied(),
                        qty: r.size.0.unwrap_or_default().abs(),
                        leverage: positive(r.leverage),
                        margin: None,
                    }));
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
