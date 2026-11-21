//! Kraken Futures orders on the multi-collateral contracts (`flexible_futures`: `PF_`
//! perpetuals, `FF_` dated), written from the documentation alone: no account, no demo key.
//! Every field below is the doc's; each claim is sourced in PROVIDER.md ("Trading"). The order
//! path itself is the shared trader's; this file only says what the calls are and what the
//! answers mean.
//!
//! - Place `POST /sendorder`, the parameters in a form body (REST guide: "Calls to endpoints
//!   that do change the state of the server should be submitted with request type POST ... with
//!   request parameters submitted in the request body"), the body hashed as `postData`.
//!   `orderType`: `mkt`, `lmt`, `post`, `ioc`, `fok`, `stp` (a stop, with or without a limit),
//!   `take_profit` (with or without a limit). Sizes in the base coin (`contractSize` 1).
//! - Inverse and vanilla contracts (`PI_`, `FI_`, sized in USD contracts) and options are
//!   refused before I/O.
//! - Client id `cliOrdId` (at most 100 characters), the OTW key unchanged.
//! - A refusal comes inside an HTTP 200 `"result":"success"`: the status word
//!   ([`super::trade_errors`]).
//! - Change `POST /editorder` by `orderId`, `qtyMode=ABSOLUTE` always (the default, RELATIVE,
//!   would read the size as what is left). Cancel `POST /cancelorder` by `order_id`; cancel
//!   all `POST /cancelallorders` (trigger orders included). Limit and trigger orders share one
//!   id space: no id is tagged.
//! - Lookup: `POST /orders/status` knows open orders and those ended in the last 5 seconds
//!   only; older ones are found in the history API: their cancel or rejection (`/orders`),
//!   then their executions (`/executions`), from the order's time on. Every history read
//!   counts in the history pool (endpoint ids `executions:...`).
//! - One position per contract (`openpositions`, `side` long or short): every order is
//!   one-way. No private socket (its login is a challenge round trip the shared layer cannot
//!   make): polled only.

#![deny(clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, TimeInForce, TriggerKind};
use otw_connector_sdk::parse::{ts_rfc3339, Int, OptDec};
use otw_connector_sdk::signer::encode_query;
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, LookupAt, Page, Placing, PositionMode, PositionSide, Settings, TradeMapper, TradingStatus, VenueError,
    VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;

use super::trade_errors::{CANCEL, CANCEL_ALL, EDIT, SEND};
use super::{KrakenFutures, API, HISTORY, REST, VENUE};

const STATUS: &str = "orders_status";
const OPEN: &str = "openorders";
const POSITIONS: &str = "openpositions";
const MODE: &str = "position_mode";
const RULES: &str = "instruments";
/// Fills: the history API's executions over a period.
const FILLS: &str = "executions";
/// A lookup in the history API: the order's cancel or rejection, then its executions. Both ids
/// count in the history pool declared under `executions` (the part before the `:`).
const LOOKUP_FILLS: &str = "executions:lookup_executions";
const LOOKUP_ORDERS: &str = "executions:lookup_orders";
/// History rows per page ("silently capped at 1000").
const PAGE: u32 = 1000;
/// How far before the order's own time a history lookup starts (the venue's clock and OTW's
/// may differ a little).
const LOOKUP_SLACK_MS: i64 = 60_000;
/// The contract types orders are sent on: multi-collateral, sized in the base coin.
const TRADED: &str = "flexible_futures";
/// Inverse contracts: sized in USD contracts, settled in the coin. Never sent an order; their
/// open orders are read in coins.
const INVERSE: &str = "futures_inverse";

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kraken_futures {what}: {e}"))
}

/// A contract OTW sends orders on: `PF_` (perpetual) and `FF_` (dated), the multi-collateral
/// ones. Every other prefix (`PI_`, `FI_` inverse, `FV_`, `PV_` vanilla, `OF_` options) is sized
/// in USD contracts or is not a future.
pub(super) fn traded(symbol: &str) -> bool {
    let s = symbol.to_ascii_uppercase();
    s.starts_with("PF_") || s.starts_with("FF_")
}

fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn side_of(word: &str) -> Mapped<Side> {
    match word.to_ascii_lowercase().as_str() {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        _ => Mapped::Unknown(word.to_string()),
    }
}

fn rfc_ms(t: &str) -> Option<i64> {
    ts_rfc3339(t).ok().map(|ts| ts.millis())
}

/// The endpoint id of a read asked for one contract (`openpositions/PF_XBTUSD`): Kraken's list
/// answers every contract, and the decoder keeps the one asked, told by the id the answer is
/// read with ([`DecodeCtx::endpoint`]). One id per contract, kept for the process.
fn on_symbol(base: &str, symbol: &str) -> &'static str {
    static IDS: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let key = format!("{base}/{}", symbol.to_ascii_uppercase());
    let mut ids = IDS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(id) = ids.get(&key) {
        return id;
    }
    let id: &'static str = Box::leak(key.clone().into_boxed_str());
    ids.insert(key, id);
    id
}

/// The contract a read's endpoint id names ([`on_symbol`]), if any.
fn asked_symbol<'a>(endpoint: &'a str, base: &str) -> Option<&'a str> {
    endpoint.strip_prefix(base)?.strip_prefix('/')
}

// ── Answers ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SendAnswer {
    #[serde(rename = "sendStatus", default)]
    send_status: Option<SendStatus>,
}

#[derive(Deserialize)]
struct SendStatus {
    #[serde(default)]
    order_id: String,
    #[serde(rename = "cliOrdId", default)]
    cli_ord_id: Option<String>,
}

/// `POST /orders/status`.
#[derive(Deserialize)]
struct StatusAnswer {
    #[serde(default)]
    orders: Vec<StatusRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusRow {
    order: CachedOrder,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedOrder {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    cli_ord_id: Option<String>,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    quantity: OptDec,
    #[serde(default)]
    filled: OptDec,
    #[serde(default)]
    limit_price: OptDec,
    #[serde(default)]
    reduce_only: bool,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    price_trigger_options: Option<TriggerOptions>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TriggerOptions {
    #[serde(default)]
    trigger_price: OptDec,
}

/// `GET /openorders`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAnswer {
    #[serde(default)]
    open_orders: Vec<OpenRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenRow {
    #[serde(rename = "order_id")]
    order_id: String,
    #[serde(default)]
    cli_ord_id: Option<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    order_type: String,
    symbol: String,
    #[serde(default)]
    limit_price: OptDec,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    filled_size: OptDec,
    #[serde(default)]
    unfilled_size: OptDec,
    #[serde(default)]
    reduce_only: bool,
    #[serde(default)]
    received_time: Option<String>,
}

/// The history API's pages (`/executions`, `/orders`): rows under `elements`, the next page's
/// token in `continuationToken` (or the `Next-Continuation-Token` header).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPage<E> {
    #[serde(default = "Vec::new")]
    elements: Vec<E>,
    #[serde(default)]
    continuation_token: Option<String>,
}

/// The header the history API hands the next page's token in.
const NEXT_TOKEN: &str = "Next-Continuation-Token";

impl<E> HistoryPage<E> {
    /// The next page's token: the body's `continuationToken`, else the answer's
    /// `Next-Continuation-Token` header (`header`); none after an empty page.
    fn next(&self, header: Option<&str>) -> Option<String> {
        if self.elements.is_empty() {
            return None;
        }
        self.continuation_token.clone().filter(|t| !t.is_empty()).or_else(|| header.filter(|t| !t.is_empty()).map(str::to_string))
    }
}

#[derive(Deserialize)]
struct ExecElement {
    #[serde(default)]
    event: ExecEvent,
}

#[derive(Deserialize, Default)]
struct ExecEvent {
    #[serde(default)]
    execution: Option<ExecDetails>,
}

#[derive(Deserialize)]
struct ExecDetails {
    execution: Execution,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Execution {
    uid: String,
    order: HistOrder,
    timestamp: Int,
    #[serde(default)]
    quantity: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    order_data: Option<OrderData>,
}

#[derive(Deserialize)]
struct OrderData {
    #[serde(default)]
    fee: OptDec,
}

/// `HistoricalOrder`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistOrder {
    #[serde(default)]
    uid: String,
    #[serde(default)]
    tradeable: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    quantity: OptDec,
    #[serde(default)]
    filled: OptDec,
    #[serde(default)]
    timestamp: Option<Int>,
    #[serde(default)]
    limit_price: OptDec,
    #[serde(default)]
    order_type: String,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    reduce_only: bool,
}

#[derive(Deserialize)]
struct OrderElement {
    #[serde(default)]
    event: OrderEvent,
}

/// `HistoricalOrderEvent`: only its cancel and its rejection settle an order; the others
/// (`OrderPlaced`, `OrderUpdated`, `OrderEditRejected`, `OrderNotFound`) are not read.
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct OrderEvent {
    #[serde(default)]
    order_cancelled: Option<WithOrder>,
    #[serde(default)]
    order_rejected: Option<WithOrder>,
}

#[derive(Deserialize)]
struct WithOrder {
    order: HistOrder,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionsAnswer {
    #[serde(default)]
    open_positions: Vec<PositionRow>,
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
    max_fixed_leverage: OptDec,
    #[serde(default)]
    cross_max_leverage: OptDec,
}

#[derive(Deserialize)]
struct RulesAnswer {
    #[serde(default)]
    instruments: Vec<RuleRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleRow {
    symbol: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    contract_size: OptDec,
    #[serde(default)]
    contract_value_trade_precision: Option<i64>,
    #[serde(default)]
    suspended: Option<bool>,
    #[serde(default)]
    post_only: Option<bool>,
    #[serde(default)]
    is_expired: Option<bool>,
    #[serde(default)]
    tradeable: Option<bool>,
}

impl RuleRow {
    /// `10^-contractValueTradePrecision`: "a trade precision of 2 means order quantities are not
    /// allowed to be more precise than the hundredth decimal place (0.01). These values can be
    /// negative to specify quantity increments of 10 (-1), 100 (-2), etc."
    fn lot(&self) -> Option<Decimal> {
        let p = self.contract_value_trade_precision?;
        if p >= 0 {
            Decimal::try_from_i128_with_scale(1, u32::try_from(p).ok()?).ok()
        } else {
            10i64.checked_pow(u32::try_from(p.checked_neg()?).ok()?).map(Decimal::from)
        }
    }

    fn rules(&self) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(VENUE, &self.symbol.to_ascii_uppercase()));
        v.tick = positive(self.tick_size);
        v.lot_step = self.lot();
        // No smallest size is published (`minimumTradeSize` "Not populated for any markets")
        // and `maxPositionSize` bounds a position, not an order: the venue checks both.
        v.status = if self.kind != TRADED || !traded(&self.symbol) {
            TradingStatus::Closed(format!("{}: sized in USD contracts or not a future, not taken by OpenTraderWorld", if self.kind.is_empty() { "unknown type" } else { &self.kind }))
        } else if positive(self.contract_size) != Some(Decimal::ONE) {
            // A multi-collateral contract is one base coin; any other size is not read as one.
            TradingStatus::Closed(format!("contract size {} is not one coin", self.contract_size.0.map(dec_text).unwrap_or_else(|| "none".into())))
        } else if self.suspended.unwrap_or(false) {
            TradingStatus::Closed("suspended".into())
        } else if self.is_expired.unwrap_or(false) {
            TradingStatus::Closed("expired".into())
        } else if !self.tradeable.unwrap_or(true) {
            TradingStatus::Closed("not tradeable".into())
        } else if self.post_only.unwrap_or(false) {
            TradingStatus::PostOnly("postOnly".into())
        } else {
            TradingStatus::Trading
        };
        // An inverse contract is never traded (closed above); its size in the quote currency
        // (`contractSize`, USD) is carried so its open orders read in coins, as the account read
        // states them ([`KrakenFutures::open_row`]).
        if self.kind == INVERSE {
            v.contract_size = positive(self.contract_size);
        }
        v
    }
}

// ── Requests ─────────────────────────────────────────────────────────────────

impl KrakenFutures {
    fn demo(s: &Settings) -> bool {
        s.get("env").is_some_and(|v| v.trim().eq_ignore_ascii_case("demo"))
    }

    /// A state-changing call: its parameters in a form body, the body signed as `postData`.
    fn post(&self, endpoint: &'static str, path: &str, params: &[(String, String)], weight: u32, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, REST, format!("{API}{path}"), corr).signed().weight(weight);
        h.method = Method::Post;
        if !params.is_empty() {
            h.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
            h.body = Some(encode_query(params).into_bytes().into());
        }
        h
    }

    fn get(&self, endpoint: &'static str, path: String, weight: u32, corr: CorrId) -> HttpRequest {
        HttpRequest::get(endpoint, REST, path, corr).signed().weight(weight)
    }

    /// A history read of `symbol`, newest first, from `since` when known.
    fn history(&self, endpoint: &'static str, what: &str, symbol: &str, since: Option<i64>, corr: CorrId) -> HttpRequest {
        let mut h = self.get(endpoint, format!("{HISTORY}/{what}"), 1, corr).query("tradeable", symbol).query("sort", "desc").query("count", PAGE);
        if let Some(t) = since {
            h = h.query("since", t);
        }
        h
    }

    /// The venue's `orderType` for `o`; a combination its words cannot carry is refused.
    fn order_type(o: &OrderRequest) -> Result<&'static str, ConnectorError> {
        Ok(match (o.order_type, o.tif, o.opts.post_only) {
            // "an immediate-or-cancel order with 1% price protection": no TIF of its own.
            (OrderType::Market, TimeInForce::Gtc | TimeInForce::Ioc, false) => "mkt",
            (OrderType::Limit, TimeInForce::Gtc, true) => "post",
            (OrderType::Limit, TimeInForce::Gtc, false) => "lmt",
            (OrderType::Limit, TimeInForce::Ioc, false) => "ioc",
            (OrderType::Limit, TimeInForce::Fok, false) => "fok",
            // "If no limitPrice is provided the stp or take_profit order will trigger a market
            // order"; with one, "limitPrice denotes the worst price at which the stp or
            // take_profit order can get filled at".
            (OrderType::Stop | OrderType::StopLimit, TimeInForce::Gtc, false) => "stp",
            (OrderType::Mit | OrderType::Lit, TimeInForce::Gtc, false) => "take_profit",
            (t, tif, post) => {
                return Err(ConnectorError::unsupported(format!(
                    "Kraken Futures takes no {t:?} order {:?}{}: a market order goes as is, a limit good till cancelled, immediate or fill or kill, a stop or take profit good till cancelled",
                    tif.kind(),
                    if post { " post-only" } else { "" }
                )))
            }
        })
    }

    fn trigger_word(t: TriggerKind) -> Result<&'static str, ConnectorError> {
        match t {
            TriggerKind::Mark => Ok("mark"),
            TriggerKind::Last => Ok("last"),
            TriggerKind::Index => Ok("index"),
            other => Err(ConnectorError::unsupported(format!("Kraken Futures triggers on the mark, the index or the last trade, not on {other:?}"))),
        }
    }

    fn refuse_untraded(symbol: &str) -> Result<(), ConnectorError> {
        if traded(symbol) {
            return Ok(());
        }
        Err(ConnectorError::unsupported(format!(
            "OpenTraderWorld sends Kraken Futures orders on the multi-collateral contracts (PF_, FF_) only: {symbol} is sized in USD contracts or is not a future, nothing was sent"
        )))
    }

    /// The Send order parameters for `o`, in the doc's order.
    fn order_params(o: &OrderRequest, p: &Placing) -> Result<Vec<(String, String)>, ConnectorError> {
        let kind = Self::order_type(o)?;
        let mut b: Vec<(String, String)> = vec![
            ("orderType".into(), kind.into()),
            ("symbol".into(), o.instrument.native.clone()),
            ("side".into(), if o.side == Side::Buy { "buy" } else { "sell" }.into()),
            // A multi-collateral contract is one base coin (`contractSize` 1): the size as is.
            ("size".into(), dec_text(o.qty.value())),
        ];
        if let Some(px) = o.limit {
            b.push(("limitPrice".into(), dec_text(px.value())));
        }
        if let Some(px) = o.stop {
            b.push(("stopPrice".into(), dec_text(px.value())));
        }
        if let Some(t) = o.opts.trigger {
            b.push(("triggerSignal".into(), Self::trigger_word(t)?.into()));
        }
        if o.opts.reduce_only {
            b.push(("reduceOnly".into(), "true".into()));
        }
        if !p.client_id.is_empty() {
            b.push(("cliOrdId".into(), p.client_id.clone()));
        }
        Ok(b)
    }

    fn status_row(r: StatusRow) -> VenueOrder {
        let o = r.order;
        let qty = o.quantity.0.unwrap_or_default();
        let filled = o.filled.0.unwrap_or_default();
        let trigger = o.kind == "TRIGGER_ORDER";
        let status = match r.status.as_str() {
            "ENTERED_BOOK" if filled > Decimal::ZERO => Mapped::Known(OrderStatus::PartiallyFilled),
            other => status_word(other),
        };
        VenueOrder {
            venue_id: o.order_id,
            client_id: o.cli_ord_id.filter(|c| !c.is_empty()),
            symbol: o.symbol.to_ascii_uppercase(),
            side: side_of(&o.side),
            order_type: o.kind,
            trigger_pending: trigger && r.status == "TRIGGER_PLACED",
            status,
            qty,
            filled_qty: filled,
            limit: positive(o.limit_price),
            stop: o.price_trigger_options.and_then(|t| positive(t.trigger_price)),
            avg_price: None,
            reduce_only: o.reduce_only,
            position: PositionSide::Net,
            created_ms: o.timestamp.as_deref().and_then(rfc_ms),
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }
    }

    /// A working order. An inverse contract's row (USD contracts) is stated in coins at its own
    /// price (its limit, else its trigger), `size × contractSize / price`, as the account read
    /// states it; a row that cannot be (no size read, no price) is kept as the venue states it,
    /// so cancel all still sees it.
    fn open_row(r: OpenRow, sizes: &ContractSizes) -> VenueOrder {
        let mut filled = r.filled_size.0.unwrap_or_default();
        let mut qty = filled + r.unfilled_size.0.unwrap_or_default();
        if !traded(&r.symbol) {
            let at = positive(r.limit_price).or(positive(r.stop_price));
            if let Some((cs, px)) = sizes.get(&r.symbol).zip(at) {
                let coins = |n: Decimal| (n * cs).checked_div(px);
                if let (Some(q), Some(f)) = (coins(qty), coins(filled)) {
                    (qty, filled) = (q, f);
                }
            }
        }
        VenueOrder {
            venue_id: r.order_id,
            client_id: r.cli_ord_id.filter(|c| !c.is_empty()),
            symbol: r.symbol.to_ascii_uppercase(),
            side: side_of(&r.side),
            // "`lmt` - limit order; `stp` - stop order; `take_profit` - take profit order"
            // (the enum also says `stop`).
            trigger_pending: r.order_type != "lmt",
            status: status_word(&r.status),
            qty,
            filled_qty: filled,
            limit: positive(r.limit_price),
            stop: positive(r.stop_price),
            avg_price: None,
            reduce_only: r.reduce_only,
            position: PositionSide::Net,
            created_ms: r.received_time.as_deref().and_then(rfc_ms),
            order_type: r.order_type,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }
    }

    /// An order the history API holds, in the state `status`.
    fn hist_row(o: HistOrder, status: OrderStatus, filled: Decimal, avg: Option<Decimal>) -> VenueOrder {
        VenueOrder {
            venue_id: o.uid,
            client_id: o.client_id.filter(|c| !c.is_empty()),
            symbol: o.tradeable.to_ascii_uppercase(),
            side: side_of(&o.direction),
            order_type: o.order_type,
            status: Mapped::Known(status),
            qty: o.quantity.0.unwrap_or_default(),
            filled_qty: filled,
            limit: positive(o.limit_price),
            stop: None,
            avg_price: avg,
            reduce_only: o.reduce_only,
            position: PositionSide::Net,
            created_ms: o.timestamp.map(|t| t.0),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        }
    }

    /// A page of `/executions` as fills; `header`: the answer's `Next-Continuation-Token`.
    fn fills_page(body: &[u8], header: Option<&str>) -> Result<Page<VenueFill>, ConnectorError> {
        let page: HistoryPage<ExecElement> = serde_json::from_slice(body).map_err(|e| decode_err("executions", e))?;
        let next = page.next(header);
        let mut out = Vec::with_capacity(page.elements.len());
        for x in page.elements.into_iter().filter_map(|e| e.event.execution.map(|d| d.execution)) {
            // An inverse contract's fill is in USD contracts: not read here (never asked: the
            // fills are read per contract OTW trades).
            if !traded(&x.order.tradeable) {
                continue;
            }
            // Executions an order of the account did not make move the position all the same.
            let position_only = matches!(
                x.order.order_type.as_str(),
                "Liquidation" | "PartialLiquidation" | "CoveredLiquidation" | "Assignment" | "HedgeAssignment" | "Unwind"
            );
            out.push(VenueFill {
                exec_id: x.uid,
                venue_order_id: x.order.uid,
                client_id: x.order.client_id.filter(|c| !c.is_empty()),
                symbol: x.order.tradeable.to_ascii_uppercase(),
                side: side_of(&x.order.direction),
                qty: x.quantity.0.unwrap_or_default(),
                price: x.price.0.unwrap_or_default(),
                // `orderData.fee`, as written: the history doc states neither its currency nor
                // its sign (the socket's `fills` feed signs a rebate negative).
                fee: x.order_data.and_then(|d| d.fee.0).unwrap_or_default(),
                fee_currency: String::new(),
                at_ms: x.timestamp.0,
                position_only,
                extra_fees: Vec::new(),
                fee_pending: false,
            });
        }
        Ok((out, next))
    }


    /// The orders a page of executions settles. An order read here was not open at the venue
    /// (the live lookup, asked first, did not list it) and had no cancel or rejection in the
    /// order history (read before): what it filled is the most of its executions on this page
    /// added up and the order's own `filled` as an execution carries it, since its executions
    /// may run over several pages. Its whole size filled is `Filled`, part of it `Cancelled`
    /// with what filled. The average price is the page's only when the page holds every fill.
    fn executed_orders(page: HistoryPage<ExecElement>, header: Option<&str>) -> Page<VenueOrder> {
        let next = page.next(header);
        // Per order: the order, its executions' sum and value on this page, its own `filled`.
        let mut by_order: Vec<(HistOrder, Decimal, Decimal, Decimal)> = Vec::new();
        for x in page.elements.into_iter().filter_map(|e| e.event.execution.map(|d| d.execution)) {
            let q = x.quantity.0.unwrap_or_default();
            let px = x.price.0.unwrap_or_default();
            let own = x.order.filled.0.unwrap_or_default();
            match by_order.iter_mut().find(|(o, _, _, _)| o.uid == x.order.uid) {
                Some((_, sum, value, most)) => {
                    *sum += q;
                    *value += q * px;
                    *most = (*most).max(own);
                }
                None => by_order.push((x.order, q, q * px, own)),
            }
        }
        let rows = by_order
            .into_iter()
            .map(|(o, sum, value, own)| {
                let filled = sum.max(own);
                let whole = o.quantity.0.is_some_and(|q| filled >= q);
                let avg = value.checked_div(sum).filter(|_| sum > Decimal::ZERO && sum == filled);
                Self::hist_row(o, if whole { OrderStatus::Filled } else { OrderStatus::Cancelled }, filled, avg)
            })
            .collect();
        (rows, next)
    }
}

/// `orders/status` and `openorders` status words.
fn status_word(word: &str) -> Mapped<OrderStatus> {
    Mapped::Known(match word {
        "ENTERED_BOOK" | "TRIGGER_PLACED" | "untouched" => OrderStatus::New,
        "partiallyFilled" => OrderStatus::PartiallyFilled,
        "FULLY_EXECUTED" => OrderStatus::Filled,
        "CANCELLED" => OrderStatus::Cancelled,
        "REJECTED" | "TRIGGER_ACTIVATION_FAILURE" => OrderStatus::Rejected,
        other => return Mapped::Unknown(other.to_string()),
    })
}

impl TradeMapper for KrakenFutures {
    fn venue_name(&self) -> &str {
        "Kraken Futures"
    }

    /// `cliOrdId`: "maxLength: 100", "It must be globally unique". The REST doc states no
    /// charset (doc gap); letters, digits and `-` (a UUID's) are sent.
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule { max_len: 100, charset: "letters, digits and -", allowed: |c| c.is_ascii_alphanumeric() || c == '-' }
    }

    fn is_sandbox(&self, settings: &Settings) -> bool {
        Self::demo(settings)
    }

    fn place(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        Self::refuse_untraded(&o.instrument.native)?;
        let params = Self::order_params(o, p)?;
        Ok(self.post(SEND, "/sendorder", &params, 10, p.corr))
    }

    /// Edit order by `orderId`: `size` with `qtyMode=ABSOLUTE` ("the total order size,
    /// including past fills, is set to size"), `limitPrice`, `stopPrice` ("Required ... not a
    /// stop order" refuses it). A size at or under what filled is refused before I/O.
    fn amend(&self, _settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if change.tif.is_some() {
            return Err(ConnectorError::unsupported("Kraken Futures changes a working order's size and prices only"));
        }
        let mut b: Vec<(String, String)> = vec![("orderId".into(), cur.venue_id.clone())];
        if let Some(q) = change.qty {
            if q.value() <= cur.filled_qty {
                return Err(ConnectorError::invalid(format!("{} of order {} already filled: a new size must be above it", dec_text(cur.filled_qty), cur.venue_id)));
            }
            b.push(("size".into(), dec_text(q.value())));
        }
        if let Some(px) = change.limit {
            if cur.limit.is_none() {
                return Err(ConnectorError::invalid(format!("order {} has no limit price to change", cur.venue_id)));
            }
            b.push(("limitPrice".into(), dec_text(px.value())));
        }
        if let Some(px) = change.stop {
            if cur.stop.is_none() {
                return Err(ConnectorError::invalid(format!("order {} has no trigger price to change", cur.venue_id)));
            }
            b.push(("stopPrice".into(), dec_text(px.value())));
        }
        if b.len() == 1 {
            return Err(ConnectorError::invalid("nothing to change"));
        }
        // The default (`RELATIVE`) would read the size as what is left: never relied on.
        b.push(("qtyMode".into(), "ABSOLUTE".into()));
        Ok(self.post(EDIT, "/editorder", &b, 10, p.corr))
    }

    /// Cancel order by `order_id`: a limit or a trigger order alike.
    fn cancel(&self, _settings: &Settings, _symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(self.post(CANCEL, "/cancelorder", &[("order_id".into(), venue_id.to_string())], 10, corr))
    }

    /// Cancel all orders: "If no arguments are specified all open orders will be cancelled",
    /// trigger orders included (changelog 2026-11-12); `symbol` for one contract.
    fn cancel_all(&self, _settings: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if symbols.is_empty() {
            return Ok(vec![self.post(CANCEL_ALL, "/cancelallorders", &[], 25, corr)]);
        }
        Ok(symbols.iter().map(|s| self.post(CANCEL_ALL, "/cancelallorders", &[("symbol".into(), s.clone())], 25, corr)).collect())
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::Book
    }

    /// `cancelStatus.status` `cancelled` or `noOrdersToCancel`, both a success (what it
    /// cancelled is in `orderEvents`); what is left working is cancelled one by one after.
    fn decode_cancel_all(&self, body: &[u8]) -> Result<(), ConnectorError> {
        let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| decode_err("cancelallorders", e))?;
        match v.pointer("/cancelStatus/status").and_then(serde_json::Value::as_str) {
            Some("cancelled" | "noOrdersToCancel") => Ok(()),
            other => Err(ConnectorError::decode(format!("kraken_futures cancelallorders: status {other:?}"))),
        }
    }

    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.query_by_client_id_at(settings, symbol, client_id, &LookupAt { created_ms: None, now_ms: 0 }, corr)
    }

    /// `orders/status` by `cliOrdIds` (open, or ended in the last 5 seconds); then the history
    /// API from the order's time (newest first): its cancel or rejection (with what filled),
    /// then its executions (an order filled whole has no cancel).
    fn query_by_client_id_at(&self, _settings: &Settings, symbol: &str, client_id: &str, at: &LookupAt, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let since = at.created_ms.map(|t| t - LOOKUP_SLACK_MS);
        Ok(vec![
            self.post(STATUS, "/orders/status", &[("cliOrdIds".into(), client_id.to_string())], 1, corr),
            self.history(LOOKUP_ORDERS, "orders", symbol, since, corr),
            self.history(LOOKUP_FILLS, "executions", symbol, since, corr),
        ])
    }

    /// The same, by `orderIds`; the history API's newest page of the contract.
    fn query_by_venue_id(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![
            self.post(STATUS, "/orders/status", &[("orderIds".into(), venue_id.to_string())], 1, corr),
            self.history(LOOKUP_ORDERS, "orders", symbol, None, corr),
            self.history(LOOKUP_FILLS, "executions", symbol, None, corr),
        ])
    }

    /// Up to three pages (3000 history rows) of each history read.
    fn lookup_pages(&self) -> usize {
        3
    }

    /// Get open orders: every contract's limit and trigger orders in one list (the rows of
    /// `symbol` kept, when one is asked).
    fn open_orders(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let endpoint = symbol.map(|s| on_symbol(OPEN, s)).unwrap_or(OPEN);
        Ok(vec![self.get(endpoint, format!("{API}/openorders"), 2, corr)])
    }

    /// The history API's executions of `symbol` over the period, oldest first.
    fn fills_since(&self, _settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(self
            .get(FILLS, format!("{HISTORY}/executions"), 1, corr)
            .query("since", from_ms)
            .query("before", to_ms)
            .query("sort", "asc")
            .query("count", PAGE)
            .query("tradeable", symbol))
    }

    /// The public list of the contracts, every type: the multi-collateral ones are traded, the
    /// inverse ones carry their size for their open orders ([`RuleRow::rules`]).
    fn instrument_rules(&self, _settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(HttpRequest::get(RULES, REST, format!("{API}/instruments"), corr).weight(0))
    }

    /// The contract list is read for the inverse contracts' sizes ([`DecodeCtx::sizes`]): a
    /// multi-collateral contract carries none and is sized in the base coin.
    fn counts_contracts(&self) -> bool {
        true
    }

    /// The history API pages by `continuation_token` ("Opaque token from the
    /// `Next-Continuation-Token` header", also in the body as `continuationToken`): the
    /// order path's decoders read the body's, else the header's.
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        next.query.retain(|(k, _)| k != "continuation_token");
        next.query.push(("continuation_token".into(), cursor.to_string()));
        next
    }

    /// Before I/O: a contract OTW trades, and a type and time in force the venue's words carry.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        Self::refuse_untraded(&o.instrument.native)?;
        Self::order_type(o)?;
        if let Some(t) = o.opts.trigger {
            Self::trigger_word(t)?;
        }
        Ok(())
    }

    /// Get open positions answers every contract: the rows of `symbol` are kept.
    fn positions(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(self.get(on_symbol(POSITIONS, symbol), format!("{API}/openpositions"), 2, corr)))
    }

    fn all_positions(&self, _settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![self.get(POSITIONS, format!("{API}/openpositions"), 2, corr)])
    }

    /// One position per contract (`side` long or short; the doc has no hedge mode): read once
    /// per account with Get open positions, which also proves the key reads.
    fn account_position_mode(&self, _settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(self.get(MODE, format!("{API}/openpositions"), 2, corr)))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        let _: PositionsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("openpositions", e))?;
        Ok(Some(PositionMode::OneWay))
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    /// `sendStatus.order_id` (a refusing status was read as an error before).
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let a: SendAnswer = serde_json::from_slice(body).map_err(|e| decode_err("sendorder", e))?;
        let s = a.send_status.ok_or_else(|| ConnectorError::decode("kraken_futures sendorder: no sendStatus"))?;
        let client_id = s.cli_ord_id.filter(|c| !c.is_empty());
        if s.order_id.is_empty() {
            return Ok(Ack::without_id(client_id));
        }
        Ok(Ack { venue_id: s.order_id, client_id })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_orders_in(&DecodeCtx::new(OPEN, &ContractSizes::default()), body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        match ctx.endpoint {
            STATUS => {
                let a: StatusAnswer = serde_json::from_slice(body).map_err(|e| decode_err("orders/status", e))?;
                Ok((a.orders.into_iter().map(Self::status_row).collect(), None))
            }
            LOOKUP_FILLS => {
                let page: HistoryPage<ExecElement> = serde_json::from_slice(body).map_err(|e| decode_err("executions", e))?;
                Ok(Self::executed_orders(page, ctx.header(NEXT_TOKEN)))
            }
            LOOKUP_ORDERS => {
                let page: HistoryPage<OrderElement> = serde_json::from_slice(body).map_err(|e| decode_err("orders history", e))?;
                let next = page.next(ctx.header(NEXT_TOKEN));
                let rows = page
                    .elements
                    .into_iter()
                    .filter_map(|e| match (e.event.order_cancelled, e.event.order_rejected) {
                        (Some(c), _) => {
                            let filled = c.order.filled.0.unwrap_or_default();
                            Some(Self::hist_row(c.order, OrderStatus::Cancelled, filled, None))
                        }
                        (None, Some(r)) => {
                            let filled = r.order.filled.0.unwrap_or_default();
                            Some(Self::hist_row(r.order, OrderStatus::Rejected, filled, None))
                        }
                        (None, None) => None,
                    })
                    .collect();
                Ok((rows, next))
            }
            other => {
                let a: OpenAnswer = serde_json::from_slice(body).map_err(|e| decode_err("openorders", e))?;
                let only = asked_symbol(other, OPEN);
                Ok((a.open_orders.into_iter().filter(|r| only.is_none_or(|s| r.symbol.eq_ignore_ascii_case(s))).map(|r| Self::open_row(r, ctx.sizes)).collect(), None))
            }
        }
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        Self::fills_page(body, None)
    }

    /// [`Self::fills_page`] with the answer's `Next-Continuation-Token` header.
    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        Self::fills_page(body, ctx.header(NEXT_TOKEN))
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let a: RulesAnswer = serde_json::from_slice(body).map_err(|e| decode_err("instruments", e))?;
        Ok((a.instruments.iter().filter(|r| !r.symbol.is_empty()).map(RuleRow::rules).collect(), None))
    }

    /// One row per contract held: `side` long or short, `size` in the base coin on a
    /// multi-collateral contract; the leverage set on it (`maxFixedLeverage`, isolated, or
    /// `crossMaxLeverage`, cross), read only. Neither is cross with no leverage cap
    /// (`crossMaxLeverage` "Absent when the position is uncapped or isolated"). Inverse rows
    /// (USD contracts) are not read here.
    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let a: PositionsAnswer = serde_json::from_slice(body).map_err(|e| decode_err("openpositions", e))?;
        let mut out = Vec::new();
        for r in a.open_positions.into_iter().filter(|r| traded(&r.symbol)) {
            let qty = r.size.0.unwrap_or_default().abs();
            let direction = match r.side.as_str() {
                _ if qty.is_zero() => None,
                "long" => Some(Side::Buy),
                "short" => Some(Side::Sell),
                other => return Err(ConnectorError::decode(format!("kraken_futures openpositions: side {other:?} on {} is not long or short", r.symbol))),
            };
            let (leverage, margin) = match (positive(r.max_fixed_leverage), positive(r.cross_max_leverage)) {
                (Some(l), _) => (Some(l), Some(otw_connector_sdk::trade::MarginMode::isolated())),
                (None, Some(l)) => (Some(l), Some(otw_connector_sdk::trade::MarginMode::cross())),
                (None, None) => (None, Some(otw_connector_sdk::trade::MarginMode::cross())),
            };
            out.push(VenuePosition { symbol: r.symbol.to_ascii_uppercase(), side: PositionSide::Net, direction, qty, leverage, margin });
        }
        Ok(out)
    }

    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        let rows = self.decode_positions(body)?;
        Ok(match asked_symbol(ctx.endpoint, POSITIONS) {
            Some(s) => rows.into_iter().filter(|r| r.symbol.eq_ignore_ascii_case(s)).collect(),
            None => rows,
        })
    }

    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        status_word(word)
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        super::trade_errors::ERRORS
    }
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod tests;
