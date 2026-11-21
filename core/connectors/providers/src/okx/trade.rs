//! OKX v5 orders on the spot, perpetual swap and expiry futures books, written from the
//! documentation alone: no account, no demo key. Every field below is the doc's; each claim is
//! sourced in PROVIDER.md ("Trading"). The order path itself is the shared trader's; this file
//! only says what the calls are and what the answers mean.
//!
//! - Place: `POST /api/v5/trade/order`. The time in force is the order type (`ordType`):
//!   `market`, `limit` (GTC), `ioc`, `fok`, `post_only`.
//! - Client id: `clOrdId`, at most 32 letters and digits, the OTW key unchanged.
//! - `tdMode` on every order: `cash` on spot (never margin); on a contract, the margin of the
//!   position held (`mgnMode`), else `cross` (the doc's trade mode table lists cross for
//!   FUTURES/SWAP in every account mode). Never chosen beyond that.
//! - Position mode: `posMode` of `GET /api/v5/account/config`, account wide, read once and held;
//!   `posSide` `long`/`short` in long/short mode, omitted in net mode (`reduceOnly` there).
//! - Sizes: contracts at OKX (`ctVal` base coins each, linear), the base coin in OTW.
//! - A spot market order is sized in the base coin (`tgtCcy=base_ccy`) and refused whole when the
//!   balance is short (`banAmend=true`), never shrunk by the venue.
//! - Cancel and amend by `ordId`; cancel-all in batches of 20 ids (`cancel-batch-orders`), the
//!   book's untriggered algo orders (`orders-algo-pending`, `cond:<algoId>`) in batches of 10
//!   (`cancel-algos`); look up by `clOrdId` (`GET /api/v5/trade/order`).
//! - Host by the account's `region` (global, eea, us); demo trading (`env = demo`) is the same
//!   host with `x-simulated-trading: 1` (the signer's), and its own socket hosts.

#![deny(clippy::float_arithmetic)]

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, TimeInForce};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, CancelRow, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, MarginKind, MarginMode, OrderSpace, Page, Placing, PositionMode, PositionSide, PrivateEvent,
    PrivateSocket, Settings, TradeMapper, TradingStatus, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{Book, Okx};

/// Rows per list page (orders-pending, fills-history: "The maximum is 100").
const PAGE: usize = 100;
/// Ids per batch cancel ("Maximum 20 orders can be canceled per request").
const BATCH: usize = 20;
/// Algo ids per Cancel algo order ("A maximum of 10 orders can be canceled per request").
const ALGO_BATCH: usize = 10;
/// The algo order types a book lists, as Get algo order list's `ordType` names them
/// ("conditional and oco both can be used and separated with comma"); `chase` is "only
/// applicable to FUTURES and SWAP".
const ALGO_TYPES_SPOT: &[&str] = &["conditional,oco", "trigger", "move_order_stop", "iceberg", "twap", "smart_iceberg"];
const ALGO_TYPES_CONTRACTS: &[&str] = &["conditional,oco", "trigger", "move_order_stop", "chase", "iceberg", "twap", "smart_iceberg"];
/// The private socket's heartbeat: "send the String 'ping'" before 30 s of silence.
const PING_EVERY_MS: u64 = 20_000;
const PRIVATE_PATH: &str = "/ws/v5/private";

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("okx {what}: {e}"))
}

#[derive(Deserialize)]
struct Answer<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AckRow {
    #[serde(default)]
    ord_id: String,
    #[serde(default)]
    cl_ord_id: String,
}

/// An order as `GET /trade/order`, `orders-pending` and the `orders` channel write it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    #[serde(default)]
    inst_type: String,
    inst_id: String,
    #[serde(default)]
    ord_id: String,
    #[serde(default)]
    cl_ord_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    pos_side: String,
    #[serde(default)]
    ord_type: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    sz: OptDec,
    #[serde(default)]
    acc_fill_sz: OptDec,
    #[serde(default)]
    px: OptDec,
    #[serde(default)]
    avg_px: OptDec,
    /// `"true"` / `"false"` (a string in the doc's examples).
    #[serde(default)]
    reduce_only: Value,
    #[serde(default)]
    tgt_ccy: String,
    #[serde(default)]
    c_time: String,
    #[serde(default)]
    category: String,
    // The `orders` channel's fill of this update.
    #[serde(default)]
    trade_id: String,
    #[serde(default)]
    fill_sz: OptDec,
    #[serde(default)]
    fill_px: OptDec,
    #[serde(default)]
    fill_fee: OptDec,
    #[serde(default)]
    fill_fee_ccy: String,
    #[serde(default)]
    fill_time: String,
}

/// An untriggered algo order as Get algo order list writes it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlgoRow {
    #[serde(default)]
    inst_type: String,
    inst_id: String,
    #[serde(default)]
    algo_id: String,
    #[serde(default)]
    algo_cl_ord_id: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    pos_side: String,
    #[serde(default)]
    ord_type: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    sz: OptDec,
    #[serde(default)]
    tgt_ccy: String,
    #[serde(default)]
    reduce_only: Value,
    #[serde(default)]
    trigger_px: OptDec,
    #[serde(default)]
    sl_trigger_px: OptDec,
    #[serde(default)]
    tp_trigger_px: OptDec,
    #[serde(default)]
    move_trigger_px: OptDec,
    /// "Order price for the trigger order"; `-1` at the market.
    #[serde(default)]
    ord_px: OptDec,
    #[serde(default)]
    sl_ord_px: OptDec,
    #[serde(default)]
    tp_ord_px: OptDec,
    #[serde(default)]
    px_limit: OptDec,
    #[serde(default)]
    c_time: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FillRow {
    #[serde(default)]
    inst_type: String,
    inst_id: String,
    #[serde(default)]
    trade_id: String,
    #[serde(default)]
    ord_id: String,
    #[serde(default)]
    cl_ord_id: String,
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
    #[serde(default)]
    sub_type: String,
    #[serde(default)]
    fill_time: String,
    #[serde(default)]
    ts: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleRow {
    #[serde(default)]
    inst_type: String,
    inst_id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    tick_sz: OptDec,
    #[serde(default)]
    lot_sz: OptDec,
    #[serde(default)]
    min_sz: OptDec,
    #[serde(default)]
    max_lmt_sz: OptDec,
    #[serde(default)]
    max_mkt_sz: OptDec,
    #[serde(default)]
    ct_val: OptDec,
    #[serde(default)]
    ct_type: String,
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
    mgn_mode: String,
    #[serde(default)]
    lever: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigRow {
    #[serde(default)]
    pos_mode: String,
}

/// A socket frame: an event answer (`login`, `subscribe`, `error`, `notice`), or a push.
#[derive(Deserialize)]
struct Push {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    arg: Value,
    #[serde(default)]
    data: Vec<Value>,
}

fn side_of(word: &str) -> Mapped<Side> {
    match word {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

fn position_of(word: &str) -> Result<PositionSide, ConnectorError> {
    match word {
        "" | "net" => Ok(PositionSide::Net),
        "long" => Ok(PositionSide::Long),
        "short" => Ok(PositionSide::Short),
        other => Err(ConnectorError::decode(format!("okx: posSide {other:?} is not net, long or short"))),
    }
}

fn margin_of(word: &str) -> Option<MarginMode> {
    match word {
        "cross" => Some(MarginMode::cross()),
        "isolated" => Some(MarginMode::isolated()),
        _ => None,
    }
}

/// A positive amount, or none: OKX writes `""` (and `"0"` for a lever on spot) for none.
fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn millis(t: &str) -> Option<i64> {
    t.trim().parse::<i64>().ok()
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => s == "true",
        _ => false,
    }
}

/// `subType` of a fill: 1 to 6 are an order's own trades (buy, sell, open and close long and
/// short); every other type (liquidation, ADL, delivery, block trade, move position, conversion,
/// and any new one) moves a position without an order of the account behind it.
fn order_trade(sub_type: &str) -> bool {
    matches!(sub_type, "1" | "2" | "3" | "4" | "5" | "6")
}

impl Okx {
    /// The book's `instType`; the read instance takes no orders.
    fn inst_type(&self) -> Result<&'static str, ConnectorError> {
        self.book.inst_type().ok_or_else(|| ConnectorError::unsupported("OKX's data and account read takes no orders: an order goes to its spot, swap or futures book"))
    }

    fn contracts(&self) -> bool {
        matches!(self.book, Book::Swap | Book::Futures)
    }

    fn base(&self, s: &Settings) -> Result<&'static str, ConnectorError> {
        Self::region_base(s.get("region").map(String::as_str))
    }

    fn demo(s: &Settings) -> bool {
        s.get("env").is_some_and(|v| v.trim().eq_ignore_ascii_case("demo"))
    }

    fn post(&self, endpoint: &'static str, s: &Settings, path: &str, body: Value, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.inst_type()?;
        let bytes = serde_json::to_vec(&body).map_err(|e| ConnectorError::invalid(format!("okx {endpoint}: {e}")))?;
        let mut h = HttpRequest::get(endpoint, self.base(s)?, path, corr).signed();
        h.method = Method::Post;
        // The signer names the body's Content-Type: one header, sent once.
        h.body = Some(bytes.into());
        Ok(h)
    }

    fn get(&self, endpoint: &'static str, s: &Settings, path: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.inst_type()?;
        Ok(HttpRequest::get(endpoint, self.base(s)?, path, corr).signed())
    }

    /// OTW's size (the base coin) as OKX's: the coin on spot, contracts of `ctVal` on a
    /// contract book. A size that is not a whole number of the venue's lots was refused by the
    /// rules before.
    fn venue_size(&self, symbol: &str, qty: Decimal, p: &Placing) -> Result<Decimal, ConnectorError> {
        if !self.contracts() {
            return Ok(qty);
        }
        let size = p.contract_size.filter(|s| *s > Decimal::ZERO).ok_or_else(|| ConnectorError::invalid(format!("OKX: the contract size of {symbol} was not read, so nothing was sent")))?;
        // A quotient that does not end (0.01 / 0.03 runs to the last digit Decimal keeps) is no
        // size OKX can be sent; the lot grid itself was checked by the rules (lotSz x ctVal).
        let n = qty / size;
        if n.normalize().scale() >= 24 || n * size != qty {
            return Err(ConnectorError::invalid(format!("OKX: {} {symbol} is not a whole number of its contract size {}, so nothing was sent", dec_text(qty), dec_text(size))).with_code("venue_rule"));
        }
        Ok(n.normalize())
    }

    /// The place-order body for `o` (place order, request parameters).
    fn order_body(&self, o: &OrderRequest, p: &Placing) -> Result<Value, ConnectorError> {
        let symbol = &o.instrument.native;
        let mut b = Map::new();
        b.insert("instId".into(), json!(symbol));
        b.insert("side".into(), json!(if o.side == Side::Buy { "buy" } else { "sell" }));
        let kind = match (o.order_type, o.tif, o.opts.post_only) {
            (OrderType::Market, TimeInForce::Gtc | TimeInForce::Ioc, false) => "market",
            (OrderType::Limit, TimeInForce::Gtc, true) => "post_only",
            (OrderType::Limit, TimeInForce::Gtc, false) => "limit",
            (OrderType::Limit, TimeInForce::Ioc, false) => "ioc",
            (OrderType::Limit, TimeInForce::Fok, false) => "fok",
            (t, tif, _) => return Err(ConnectorError::unsupported(format!("OKX takes no {t:?} order {:?}{}", tif.kind(), if o.opts.post_only { " post-only" } else { "" }))),
        };
        b.insert("ordType".into(), json!(kind));
        b.insert("sz".into(), json!(dec_text(self.venue_size(symbol, o.qty.value(), p)?)));
        if kind != "market" {
            let px = o.limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?;
            b.insert("px".into(), json!(dec_text(px.value())));
        }
        if self.contracts() {
            let td = p.margin.as_ref().map(MarginMode::word).unwrap_or("cross");
            b.insert("tdMode".into(), json!(td));
            match p.position {
                // Net mode: posSide is omitted ("omit or explicitly pass net").
                PositionSide::Net => {
                    if o.opts.reduce_only {
                        b.insert("reduceOnly".into(), json!(true));
                    }
                }
                // Long/short mode: the side closes or opens; "all closing orders apply the
                // reduce-only feature", so the flag is not sent ("applicable ... in net mode").
                PositionSide::Long => {
                    b.insert("posSide".into(), json!("long"));
                }
                PositionSide::Short => {
                    b.insert("posSide".into(), json!("short"));
                }
            }
        } else {
            if o.opts.reduce_only {
                return Err(ConnectorError::unsupported("OKX takes no reduce-only order on spot: a sale of what is held is a plain sell"));
            }
            b.insert("tdMode".into(), json!("cash"));
            if kind == "market" {
                // The size in the base coin both ways, and the whole order refused when the
                // balance is short rather than shrunk by the venue. `slippagePct` ("Maximum
                // acceptable slippage ... where tgtCcy is the received currency", default 0.00%)
                // is not sent: the doc names no reference price and no outcome past the cap
                // (PROVIDER.md, doc gaps), and OTW invents no value.
                b.insert("tgtCcy".into(), json!("base_ccy"));
                b.insert("banAmend".into(), json!(true));
            }
        }
        if !p.client_id.is_empty() {
            b.insert("clOrdId".into(), json!(p.client_id));
        }
        Ok(Value::Object(b))
    }

    /// One order row in OTW's terms. `None`: a row of another book (an `instType` this one does
    /// not trade).
    fn order_row(&self, ctx: &DecodeCtx<'_>, r: OrderRow) -> Result<Option<VenueOrder>, ConnectorError> {
        if !self.ours(&r.inst_type) {
            return Ok(None);
        }
        let symbol = r.inst_id.to_ascii_uppercase();
        let size = |v: Decimal| self.to_base(ctx.sizes, &symbol, v);
        let filled = size(r.acc_fill_sz.0.unwrap_or_default())?;
        // A spot market order sized in the quote coin (placed elsewhere: OTW sends base_ccy):
        // its size in the base coin is what filled.
        let qty = if !self.contracts() && r.ord_type == "market" && r.tgt_ccy == "quote_ccy" { filled } else { size(r.sz.0.unwrap_or_default())? };
        let limit = if r.ord_type == "market" { None } else { positive(r.px) };
        Ok(Some(VenueOrder {
            venue_id: r.ord_id,
            client_id: Some(r.cl_ord_id).filter(|c| !c.is_empty()),
            symbol,
            side: side_of(&r.side),
            order_type: r.ord_type,
            status: self.status_of(&r.state),
            qty,
            filled_qty: filled,
            limit,
            stop: None,
            avg_price: positive(r.avg_px),
            reduce_only: truthy(&r.reduce_only),
            position: position_of(&r.pos_side)?,
            created_ms: millis(&r.c_time),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }))
    }

    /// One untriggered algo order in OTW's terms, `cond:<algoId>` (the id its cancel names).
    fn algo_row(&self, ctx: &DecodeCtx<'_>, r: AlgoRow) -> Result<Option<VenueOrder>, ConnectorError> {
        if !self.ours(&r.inst_type) || r.algo_id.is_empty() {
            return Ok(None);
        }
        let symbol = r.inst_id.to_ascii_uppercase();
        // A spot algo sized in the quote coin (`tgtCcy=quote_ccy`, the buy default) states no
        // size in the base coin before it triggers: none is shown, never a guess.
        let qty = if !self.contracts() && r.tgt_ccy == "quote_ccy" { Decimal::ZERO } else { self.to_base(ctx.sizes, &symbol, r.sz.0.unwrap_or_default())? };
        let stop = [r.trigger_px, r.sl_trigger_px, r.tp_trigger_px, r.move_trigger_px].into_iter().find_map(positive);
        let limit = [r.ord_px, r.sl_ord_px, r.tp_ord_px, r.px_limit].into_iter().find_map(positive);
        let status = match r.state.as_str() {
            "live" | "pause" => Mapped::Known(OrderStatus::New),
            "partially_effective" => Mapped::Known(OrderStatus::PartiallyFilled),
            "effective" => Mapped::Known(OrderStatus::Filled),
            "canceled" => Mapped::Known(OrderStatus::Cancelled),
            "order_failed" => Mapped::Known(OrderStatus::Rejected),
            other => Mapped::Unknown(other.to_string()),
        };
        let pending = matches!(status, Mapped::Known(OrderStatus::New));
        Ok(Some(VenueOrder {
            venue_id: OrderSpace::Conditional.tag(&r.algo_id),
            client_id: Some(r.algo_cl_ord_id).filter(|c| !c.is_empty()),
            symbol,
            side: side_of(&r.side),
            order_type: r.ord_type,
            status,
            qty,
            filled_qty: Decimal::ZERO,
            limit,
            stop,
            avg_price: None,
            reduce_only: truthy(&r.reduce_only),
            position: position_of(&r.pos_side)?,
            created_ms: millis(&r.c_time),
            trigger_pending: pending,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }))
    }

    fn algo_types(&self) -> &'static [&'static str] {
        if self.contracts() {
            ALGO_TYPES_CONTRACTS
        } else {
            ALGO_TYPES_SPOT
        }
    }

    /// A row's `instType` is this book's (an empty one is taken as the book's).
    fn ours(&self, inst_type: &str) -> bool {
        inst_type.is_empty() || Some(inst_type) == self.book.inst_type()
    }

    /// Contracts in the base coin on a contract book; the coin as is on spot.
    fn to_base(&self, sizes: &ContractSizes, symbol: &str, v: Decimal) -> Result<Decimal, ConnectorError> {
        if self.contracts() {
            sizes.to_base("OKX", symbol, v)
        } else {
            Ok(v)
        }
    }

    fn fill_row(&self, ctx: &DecodeCtx<'_>, r: FillRow) -> Result<Option<VenueFill>, ConnectorError> {
        if !self.ours(&r.inst_type) {
            return Ok(None);
        }
        let symbol = r.inst_id.to_ascii_uppercase();
        let position_only = !order_trade(&r.sub_type) || r.ord_id.is_empty() || r.trade_id.starts_with('-');
        Ok(Some(VenueFill {
            exec_id: r.trade_id,
            venue_order_id: r.ord_id,
            client_id: Some(r.cl_ord_id).filter(|c| !c.is_empty()),
            qty: self.to_base(ctx.sizes, &symbol, r.fill_sz.0.unwrap_or_default())?,
            symbol,
            side: side_of(&r.side),
            price: r.fill_px.0.unwrap_or_default(),
            // OKX signs a charge negative and a rebate positive: a cost is positive here.
            fee: -r.fee.0.unwrap_or_default(),
            fee_currency: r.fee_ccy,
            at_ms: millis(&r.fill_time).or(millis(&r.ts)).unwrap_or_default(),
            position_only,
            extra_fees: Vec::new(),
            fee_pending: false,
        }))
    }

    /// The fill an `orders` channel update carries (`tradeId` present), keyed like a REST fill.
    fn pushed_fill(&self, ctx: &DecodeCtx<'_>, r: &OrderRow) -> Result<Option<VenueFill>, ConnectorError> {
        let qty = r.fill_sz.0.unwrap_or_default();
        if r.trade_id.is_empty() || qty <= Decimal::ZERO {
            return Ok(None);
        }
        let symbol = r.inst_id.to_ascii_uppercase();
        Ok(Some(VenueFill {
            exec_id: r.trade_id.clone(),
            venue_order_id: r.ord_id.clone(),
            client_id: Some(r.cl_ord_id.clone()).filter(|c| !c.is_empty()),
            qty: self.to_base(ctx.sizes, &symbol, qty)?,
            symbol,
            side: side_of(&r.side),
            price: r.fill_px.0.unwrap_or_default(),
            fee: -r.fill_fee.0.unwrap_or_default(),
            fee_currency: r.fill_fee_ccy.clone(),
            at_ms: millis(&r.fill_time).unwrap_or_default(),
            position_only: !matches!(r.category.as_str(), "" | "normal") || r.trade_id.starts_with('-'),
            extra_fees: Vec::new(),
            fee_pending: false,
        }))
    }

    fn position_row(&self, ctx: &DecodeCtx<'_>, r: PositionRow) -> Result<Option<VenuePosition>, ConnectorError> {
        if !self.ours(&r.inst_type) {
            return Ok(None);
        }
        let symbol = r.inst_id.to_ascii_uppercase();
        let side = position_of(&r.pos_side)?;
        let pos = r.pos.0.unwrap_or_default();
        // Net mode: the sign is the direction; long/short mode: the side is.
        let direction = match side {
            PositionSide::Long => Some(Side::Buy),
            PositionSide::Short => Some(Side::Sell),
            PositionSide::Net if pos > Decimal::ZERO => Some(Side::Buy),
            PositionSide::Net if pos < Decimal::ZERO => Some(Side::Sell),
            PositionSide::Net => None,
        };
        Ok(Some(VenuePosition {
            qty: self.to_base(ctx.sizes, &symbol, pos.abs())?,
            symbol,
            side,
            direction,
            leverage: positive(r.lever),
            margin: margin_of(&r.mgn_mode),
        }))
    }

    fn rule_row(&self, r: RuleRow) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(super::VENUE, &r.inst_id.to_ascii_uppercase()));
        v.tick = positive(r.tick_sz);
        v.status = match r.state.as_str() {
            "live" => TradingStatus::Trading,
            // `suspend`, `rebase`, `preopen`, `test`, `settling`, and `post_only` ("only
            // post-only orders are accepted": held closed, a post-only order waits for `live`).
            other => TradingStatus::Closed(other.to_string()),
        };
        if !self.contracts() {
            v.lot_step = positive(r.lot_sz);
            v.min_qty = positive(r.min_sz);
            v.max_qty = positive(r.max_lmt_sz);
            // `maxMktSz` is "the quantity in USDT" on spot: not a size in the base coin.
            return v;
        }
        let Some(size) = positive(r.ct_val) else {
            v.status = TradingStatus::Closed("no contract size listed".into());
            return v;
        };
        v.contract_size = Some(size);
        let coins = |d: OptDec| positive(d).map(|n| n * size);
        v.lot_step = coins(r.lot_sz);
        v.min_qty = coins(r.min_sz);
        v.max_qty = coins(r.max_lmt_sz);
        v.market_max_qty = coins(r.max_mkt_sz);
        if r.ct_type != "linear" {
            // An inverse contract's `ctVal` is in USD: its size is not a coin amount. Listed with
            // its size (its orders and positions are read), never sent an order.
            v.status = TradingStatus::Closed(format!("{} contract, sized in USD: not taken by OpenTraderWorld", if r.ct_type.is_empty() { "unknown" } else { &r.ct_type }));
        }
        v
    }

    fn ws_base(s: &Settings) -> Option<&'static str> {
        let demo = Self::demo(s);
        Some(match Self::region_base(s.get("region").map(String::as_str)).ok()? {
            super::REST_EEA if demo => "ws-eea-demo",
            super::REST_EEA => "ws-eea",
            super::REST_US if demo => "ws-us-demo",
            super::REST_US => "ws-us",
            _ if demo => "ws-demo",
            _ => super::WS,
        })
    }

    fn rows<T: serde::de::DeserializeOwned>(&self, what: &str, body: &[u8]) -> Result<Vec<T>, ConnectorError> {
        let a: Answer<T> = serde_json::from_slice(body).map_err(|e| decode_err(what, e))?;
        Ok(a.data)
    }
}

impl TradeMapper for Okx {
    fn venue_name(&self) -> &str {
        "OKX"
    }

    /// "A combination of case-sensitive alphanumerics, all numbers, or all letters of up to 32
    /// characters".
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule { max_len: 32, charset: "letters and digits", allowed: |c| c.is_ascii_alphanumeric() }
    }

    fn is_sandbox(&self, settings: &Settings) -> bool {
        Self::demo(settings)
    }

    fn place(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        self.inst_type()?;
        let body = self.order_body(o, p)?;
        self.post("order", settings, "/api/v5/trade/order", body, p.corr)
    }

    /// Amend order: `newPx`, `newSz` ("the desired total order size"). A size at or under what
    /// filled is refused ("the order status will be changed to filled"). On a contract book
    /// the new size is sent in contracts of the instrument's contract size.
    fn amend(&self, settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if change.stop.is_some() || change.tif.is_some() {
            return Err(ConnectorError::unsupported("OKX changes a working order's price and size only"));
        }
        let mut b = Map::new();
        b.insert("instId".into(), json!(cur.symbol));
        b.insert("ordId".into(), json!(cur.venue_id));
        if let Some(px) = change.limit {
            if cur.limit.is_none() {
                return Err(ConnectorError::invalid(format!("order {} has no price to change", cur.venue_id)));
            }
            b.insert("newPx".into(), json!(dec_text(px.value())));
        }
        if let Some(q) = change.qty {
            if q.value() <= cur.filled_qty {
                return Err(ConnectorError::invalid(format!(
                    "{} of order {} already filled: a new size must be above it (OKX would end the order as filled)",
                    dec_text(cur.filled_qty),
                    cur.venue_id
                )));
            }
            b.insert("newSz".into(), json!(dec_text(self.venue_size(&cur.symbol, q.value(), p)?)));
        }
        self.post("amend", settings, "/api/v5/trade/amend-order", Value::Object(b), p.corr)
    }

    /// An algo order (`cond:<algoId>`, listed by the account read) goes to Cancel algo order
    /// (`cancel-algos`, an array of `instId` and `algoId`).
    fn cancel(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if let (OrderSpace::Conditional, id) = OrderSpace::of(venue_id) {
            return self.post("cancel_algos", settings, "/api/v5/trade/cancel-algos", json!([{ "instId": symbol, "algoId": id }]), corr);
        }
        self.post("cancel", settings, "/api/v5/trade/cancel-order", json!({ "instId": symbol, "ordId": venue_id }), corr)
    }

    /// No native cancel-all on these books (`mass-cancel` is "Only applicable to Option in
    /// Portfolio Margin mode"): batches of at most 20 ids.
    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::ByIds { max: BATCH }
    }

    /// One batch call per kind: the plain orders of the batch to `cancel-batch-orders` (20 at
    /// most), or, when the batch opens with algo orders, its algo orders to `cancel-algos` (10
    /// at most, as the doc allows). What a call does not carry is still working afterwards, and
    /// the trader cancels it on its own.
    fn cancel_ids(&self, settings: &Settings, orders: &[VenueOrder], corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if orders.is_empty() || orders.len() > BATCH {
            return Err(ConnectorError::invalid(format!("OKX cancels 1 to {BATCH} orders per batch ({} given)", orders.len())));
        }
        let algo = |o: &&VenueOrder| OrderSpace::of(&o.venue_id).0 == OrderSpace::Conditional;
        if orders.first().is_some_and(|o| algo(&o)) {
            let rows: Vec<Value> = orders.iter().filter(algo).take(ALGO_BATCH).map(|o| json!({ "instId": o.symbol, "algoId": OrderSpace::of(&o.venue_id).1 })).collect();
            return self.post("cancel_algos_batch", settings, "/api/v5/trade/cancel-algos", Value::Array(rows), corr);
        }
        let rows: Vec<Value> = orders.iter().filter(|o| !algo(o)).map(|o| json!({ "instId": o.symbol, "ordId": o.venue_id })).collect();
        self.post("cancel_batch", settings, "/api/v5/trade/cancel-batch-orders", Value::Array(rows), corr)
    }

    fn decode_cancel_rows(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("cancel-batch-orders", e))?;
        let rows = v.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        let algos = ctx.endpoint == "cancel_algos_batch";
        Ok(rows
            .iter()
            .map(|r| {
                let item: super::trade_errors::Item = serde_json::from_value(r.clone()).unwrap_or_default();
                let error = item.s_code.as_deref().filter(|c| *c != "0").map(|c| super::trade_errors::of(c, &item.sub_code, &item.s_msg));
                let id = r.get(if algos { "algoId" } else { "ordId" }).and_then(Value::as_str).unwrap_or_default();
                let venue_id = if algos { OrderSpace::Conditional.tag(id) } else { id.to_string() };
                CancelRow { venue_id, error }
            })
            .collect())
    }

    /// `GET /api/v5/trade/order` by `clOrdId` ("only the latest one will be returned").
    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![self.get("order_get", settings, "/api/v5/trade/order", corr)?.query("instId", symbol).query("clOrdId", client_id)])
    }

    /// An algo order is not read back here (its rows are not orders' rows): none.
    fn query_by_venue_id(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if OrderSpace::of(venue_id).0 == OrderSpace::Conditional {
            return Ok(Vec::new());
        }
        Ok(vec![self.get("order_get", settings, "/api/v5/trade/order", corr)?.query("instId", symbol).query("ordId", venue_id)])
    }

    /// The plain working orders, then the book's untriggered algo orders, one list per
    /// `ordType` (each paged `after` its last id): the kill switch reaches both.
    fn open_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let inst = self.inst_type()?;
        let mut h = self.get("orders_pending", settings, "/api/v5/trade/orders-pending", corr)?.query("instType", inst);
        if let Some(s) = symbol {
            h = h.query("instId", s);
        }
        let mut out = vec![h.query("limit", PAGE)];
        for t in self.algo_types() {
            let mut a = self.get(super::trade_errors::ALGO_LIST, settings, "/api/v5/trade/orders-algo-pending", corr)?.query("ordType", *t).query("instType", inst);
            if let Some(s) = symbol {
                a = a.query("instId", s);
            }
            out.push(a.query("limit", PAGE));
        }
        Ok(out)
    }

    /// Transaction details (last 3 months), filtered on `ts` by `begin` and `end`.
    fn fills_since(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(self
            .get("fills_history", settings, "/api/v5/trade/fills-history", corr)?
            .query("instType", self.inst_type()?)
            .query("instId", symbol)
            .query("begin", from_ms)
            .query("end", to_ms)
            .query("limit", PAGE))
    }

    /// The book's whole list (`GET /api/v5/public/instruments?instType=`), public: the symbol is
    /// found in it, so an instrument the book does not list is simply absent.
    fn instrument_rules(&self, settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(HttpRequest::get("instruments", self.base(settings)?, "/api/v5/public/instruments", corr).query("instType", self.inst_type()?))
    }

    /// The next page: `after` the last row's id (`ordId`, `billId`).
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        next.query.retain(|(k, _)| k != "after");
        next.query.push(("after".into(), cursor.to_string()));
        next
    }

    /// A market order has no time in force at OKX: one asked fill-or-kill is refused (a limit
    /// order takes `fok`). GTC and IOC are taken: a market order is not sent one.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        if o.order_type == OrderType::Market && o.tif == TimeInForce::Fok {
            return Err(ConnectorError::unsupported("OKX has no fill-or-kill market order: send it as a limit order FOK"));
        }
        Ok(())
    }

    fn positions(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.contracts() {
            return Ok(None);
        }
        Ok(Some(self.get("positions", settings, "/api/v5/account/positions", corr)?.query("instType", self.inst_type()?).query("instId", symbol)))
    }

    fn all_positions(&self, settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if !self.contracts() {
            return Ok(Vec::new());
        }
        Ok(vec![self.get("positions", settings, "/api/v5/account/positions", corr)?.query("instType", self.inst_type()?)])
    }

    /// `51000` "Parameter {param0} error" is what a `posSide` the mode does not take answers
    /// ("Omitting it ... in long/short mode returns error 51000"); `51041` "Portfolio margin
    /// account only supports the Buy/Sell mode".
    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        self.contracts() && e.code.as_deref().is_some_and(|c| c.starts_with("51000") || c.starts_with("51041"))
    }

    /// The margin of a contract: the held position's `mgnMode`, read from the positions call.
    fn symbol_margin(&self, settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.positions(settings, symbol, corr)
    }

    /// The held position's margin; with none held, cross (the trade mode table lists cross for
    /// FUTURES/SWAP in every account mode).
    fn decode_symbol_margin(&self, symbol: &str, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        self.decode_symbol_margin_on(symbol, PositionSide::Net, body)
    }

    /// In long/short mode each `posSide` row carries its own `mgnMode` (Get positions: one row
    /// per instrument, margin mode and side), so the order's `tdMode` is read per side.
    fn margin_per_side(&self) -> bool {
        self.contracts()
    }

    /// The held position's margin on the side the order acts on (`posSide` `long` / `short`
    /// in long/short mode; any held row for a net order); with none held there, cross.
    fn decode_symbol_margin_on(&self, symbol: &str, side: PositionSide, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        let rows: Vec<PositionRow> = self.rows("positions", body)?;
        let held = rows
            .iter()
            .filter(|r| self.ours(&r.inst_type) && r.inst_id.eq_ignore_ascii_case(symbol) && r.pos.0.is_some_and(|p| !p.is_zero()))
            .filter(|r| side == PositionSide::Net || position_of(&r.pos_side).is_ok_and(|s| s == side))
            .find_map(|r| margin_of(&r.mgn_mode));
        Ok(Some(held.unwrap_or(MarginMode { kind: MarginKind::Cross, coin: None })))
    }

    /// "isolated is not available in multi-currency margin mode and portfolio margin mode"
    /// (`50072`), "This isolated position doesn't exist" (`51043`).
    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        self.contracts() && e.code.as_deref().is_some_and(|c| c.starts_with("50072") || c.starts_with("51043"))
    }

    /// `posMode` of Get account configuration, account wide.
    fn account_position_mode(&self, settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.contracts() {
            return Ok(None);
        }
        Ok(Some(self.get("account_config", settings, "/api/v5/account/config", corr)?))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        let rows: Vec<ConfigRow> = self.rows("account config", body)?;
        match rows.first().map(|r| r.pos_mode.as_str()) {
            Some("net_mode") => Ok(Some(PositionMode::OneWay)),
            Some("long_short_mode") => Ok(Some(PositionMode::Hedge)),
            None | Some("") => Ok(None),
            Some(other) => Err(ConnectorError::decode(format!("okx: posMode {other:?} is not net_mode or long_short_mode"))),
        }
    }

    fn counts_contracts(&self) -> bool {
        self.contracts()
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    /// `ordId` and `clOrdId` of `data[0]` (a refused item was read as an error before).
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let rows: Vec<AckRow> = self.rows("order ack", body)?;
        let r = rows.into_iter().next().ok_or_else(|| ConnectorError::decode("okx order ack: no data"))?;
        let client_id = Some(r.cl_ord_id).filter(|c| !c.is_empty());
        if r.ord_id.is_empty() {
            return Ok(Ack::without_id(client_id));
        }
        Ok(Ack { venue_id: r.ord_id, client_id })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_orders_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        if ctx.endpoint == super::trade_errors::ALGO_LIST {
            let rows: Vec<AlgoRow> = self.rows("orders-algo-pending", body)?;
            let full = rows.len() >= PAGE;
            let last = rows.last().map(|r| r.algo_id.clone());
            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                out.extend(self.algo_row(ctx, r)?);
            }
            return Ok((out, last.filter(|_| full).filter(|c| !c.is_empty())));
        }
        let rows: Vec<OrderRow> = self.rows("orders", body)?;
        let full = rows.len() >= PAGE;
        let last = rows.last().map(|r| r.ord_id.clone());
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.extend(self.order_row(ctx, r)?);
        }
        Ok((out, last.filter(|_| full).filter(|c| !c.is_empty())))
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.decode_fills_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let rows: Vec<FillRow> = self.rows("fills-history", body)?;
        let full = rows.len() >= PAGE;
        let last = rows.last().map(|r| r.bill_id.clone());
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.extend(self.fill_row(ctx, r)?);
        }
        Ok((out, last.filter(|_| full).filter(|c| !c.is_empty())))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let rows: Vec<RuleRow> = self.rows("instruments", body)?;
        Ok((rows.into_iter().filter(|r| !r.inst_id.is_empty() && self.ours(&r.inst_type)).map(|r| self.rule_row(r)).collect(), None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.decode_positions_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let rows: Vec<PositionRow> = self.rows("positions", body)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.extend(self.position_row(ctx, r)?);
        }
        Ok(out)
    }

    /// Order `state`: `live`, `partially_filled`, `filled`, `canceled`, `mmp_canceled`. There
    /// is no rejected state: a refusal is the place answer's `sCode`.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "live" => OrderStatus::New,
            "partially_filled" => OrderStatus::PartiallyFilled,
            "filled" => OrderStatus::Filled,
            "canceled" | "mmp_canceled" => OrderStatus::Cancelled,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    /// The private socket of the account's region (demo on its own hosts): the book's `orders`,
    /// and `positions` on a contract book, a text `ping` every 20 s.
    fn private_socket(&self, settings: &Settings) -> Option<PrivateSocket> {
        let inst = self.book.inst_type()?;
        let base = Self::ws_base(settings)?;
        let sub = |channel: &str| json!({ "op": "subscribe", "args": [{ "channel": channel, "instType": inst }] }).to_string().into_bytes();
        let mut s = PrivateSocket::new(base, PRIVATE_PATH, sub("orders"), b"ping".to_vec(), PING_EVERY_MS);
        if self.contracts() {
            s.more.push(sub("positions"));
        }
        Some(s)
    }

    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.decode_private_in(&DecodeCtx::new(PRIVATE_PATH, &ContractSizes::default()), payload)
    }

    /// The `orders` and `positions` pushes, and the answers to `login`, `subscribe` and the
    /// text `ping`. An `error` event names no operation: it answers whichever is pending.
    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        if payload.trim_ascii() == b"pong" {
            return Ok(vec![PrivateEvent::Answer { op: "pong".into(), ok: true, message: String::new() }]);
        }
        let push = serde_json::from_slice::<Push>(payload).map_err(|e| decode_err("private frame", e))?;
        let code = push.code.clone().unwrap_or_default();
        let answer = |op: &str, ok: bool| PrivateEvent::Answer { op: op.into(), ok, message: if push.msg.is_empty() { code.clone() } else { format!("{} (code {code})", push.msg) } };
        if let Some(event) = push.event.as_deref() {
            return Ok(match event {
                "login" => vec![answer("auth", code.is_empty() || code == "0")],
                "subscribe" | "unsubscribe" => vec![answer("subscribe", true)],
                "error" => vec![answer("auth", false), answer("subscribe", false)],
                // "The connection will soon be closed for a service upgrade. Please reconnect."
                "notice" => vec![answer("notice", code != "64008")],
                "channel-conn-count-error" => vec![answer("subscribe", false)],
                other => vec![answer(other, true)],
            });
        }
        let channel = push.arg.get("channel").and_then(Value::as_str).unwrap_or_default();
        let mut out = Vec::new();
        for row in push.data {
            match channel {
                "orders" => {
                    let r: OrderRow = serde_json::from_value(row).map_err(|e| decode_err("orders push", e))?;
                    if !self.ours(&r.inst_type) {
                        continue;
                    }
                    let fill = self.pushed_fill(ctx, &r)?;
                    if let Some(o) = self.order_row(ctx, r)? {
                        out.push(PrivateEvent::Order(o));
                    }
                    out.extend(fill.map(PrivateEvent::Fill));
                }
                "positions" => {
                    let r: PositionRow = serde_json::from_value(row).map_err(|e| decode_err("positions push", e))?;
                    if let Some(p) = self.position_row(ctx, r)? {
                        out.push(PrivateEvent::Position(p));
                    }
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
