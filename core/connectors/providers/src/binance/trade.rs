//! Binance orders (execution, v2), spot and USDⓈ-M: the request a canonical order is on each
//! market, and what the venue answers. Pure: the caller sends what is built here with the
//! account's key, and hands the answers back.
//!
//! Rules held here:
//!   - **An order is named by its symbol and the caller's client id** (`BTCUSDT:k1`), never by
//!     the venue's number. Binance cancels, queries and (on spot) replaces by client id, and a
//!     spot change is a cancel-replace that keeps the client id but not the number.
//!   - **USDⓈ-M stops live in the algo book** (`/fapi/v1/algoOrder`): the order route answers
//!     `-4120` for them. Their id says so (`BTCUSDT:algo:k1`), so a cancel goes to the right book.
//!   - **No day order.** Binance takes `gtc` and `ioc`, and `gtd` on USDⓈ-M; the capabilities
//!     declare that much and nothing else is sent.
//!   - Stops are the canonical kind: a buy stop above the market, a sell stop below. Spot names
//!     them `STOP_LOSS` / `STOP_LOSS_LIMIT` (the side sets the trigger direction), USDⓈ-M
//!     `STOP_MARKET` / `STOP`. One on the wrong side of the market is the venue's to refuse
//!     ("Stop price would trigger immediately").

use otw_connector_model::{
    ConnectorError, CorrId, Decimal, ErrorKind, InstrumentSpec, Mapped, TradingRules, OrderAmend, OrderRequest, OrderStatus, OrderType, Price, Qty,
    OrderOpts, Side, TimeInForce, Timestamp, Trail, TriggerKind,
};
use std::collections::HashMap;

use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;

use super::market::{Kind, Market};
use super::REST;

/// What marks an id as one of the USDⓈ-M algo book.
const ALGO: &str = "algo:";

/// An order as OTW names it at Binance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRef {
    pub symbol: String,
    pub client_id: String,
    /// A USDⓈ-M conditional order (the algo book).
    pub algo: bool,
}

impl OrderRef {
    pub fn parse(id: &str) -> Result<Self, ConnectorError> {
        let bad = || ConnectorError::invalid(format!("{id:?} is not a Binance order id (SYMBOL:client order id)"));
        let (symbol, rest) = id.trim().split_once(':').ok_or_else(bad)?;
        let (algo, client) = match rest.strip_prefix(ALGO) {
            Some(c) => (true, c),
            None => (false, rest),
        };
        if symbol.is_empty() || client.is_empty() {
            return Err(bad());
        }
        Ok(OrderRef { symbol: symbol.to_string(), client_id: client.to_string(), algo })
    }

    pub fn id(&self) -> String {
        format!("{}:{}{}", self.symbol, if self.algo { ALGO } else { "" }, self.client_id)
    }
}

/// Binance's rule for a client order id: 1 to 36 of `.A-Z:/a-z0-9_-`.
pub fn check_client_id(id: &str) -> Result<(), ConnectorError> {
    let ok = (1..=36).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '/' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(ConnectorError::invalid(format!("Binance takes client order ids of 1 to 36 letters, digits or .:/_- ({id:?} is not one)")))
    }
}

/// The grid values either side of `v`: what a refusal offers instead.
fn either_side(v: Decimal, step: Decimal) -> (Decimal, Decimal) {
    ((v / step).floor() * step, (v / step).ceil() * step)
}

/// An order's price and size against the instrument's tick and lot, before anything leaves.
/// A refusal names the nearest values that pass; nothing is adjusted.
pub fn check_rules(spec: &InstrumentSpec, o: &OrderRequest) -> Result<(), ConnectorError> {
    let on = |v: Decimal, step: Decimal| step.is_zero() || (v % step).is_zero();
    if let Some(tick) = spec.tick_size.map(|t| t.value()) {
        for (what, p) in [("limit", o.limit), ("stop", o.stop)] {
            if let Some(p) = p.map(|p| p.value()) {
                if !on(p, tick) {
                    let (lo, hi) = either_side(p, tick);
                    return Err(ConnectorError::invalid(format!(
                        "{} moves in steps of {} (PRICE_FILTER): the {what} price {} is off that step ({} or {})",
                        o.instrument.native,
                        num(tick),
                        num(p),
                        num(lo),
                        num(hi)
                    )));
                }
            }
        }
    }
    // A close-position stop carries no size: Binance reports it as 0.
    if o.opts.close_position {
        return Ok(());
    }
    if let Some(min) = spec.min_qty {
        if o.qty.value() < min.value() {
            return Err(ConnectorError::invalid(format!("{} takes at least {} per order (LOT_SIZE)", o.instrument.native, min.value().normalize())));
        }
    }
    if let Some(lot) = spec.lot_size.map(|l| l.value()) {
        if !on(o.qty.value(), lot) {
            let (lo, hi) = either_side(o.qty.value(), lot);
            return Err(ConnectorError::invalid(format!(
                "{} trades in steps of {} (LOT_SIZE): {} is off that step ({} or {})",
                o.instrument.native,
                num(lot),
                num(o.qty.value()),
                num(lo),
                num(hi)
            )));
        }
    }
    Ok(())
}

// ── Venue filters (spec 4.1, 4.2) ──

/// A notional bound: price times size, held on the instrument ([`InstrumentSpec::rules`]).
pub use otw_connector_model::NotionalBound as Notional;
/// A price band around the reference price, held on the instrument.
pub use otw_connector_model::PriceBand as Band;

/// The filters an order is checked against: the instrument's tick and lot, and the rules the
/// instrument list states beyond them ([`rules_of`]). A bound the venue sets to 0 or does not
/// list is `None`: nothing is checked against it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    pub tick: Option<Decimal>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
    pub step: Option<Decimal>,
    pub max_qty: Option<Decimal>,
    /// MARKET_LOT_SIZE: a market order's size.
    pub market_min_qty: Option<Decimal>,
    pub market_max_qty: Option<Decimal>,
    pub market_step: Option<Decimal>,
    pub min_notional: Option<Notional>,
    pub max_notional: Option<Notional>,
    pub band: Option<Band>,
    /// MAX_NUM_ORDERS: open orders per account on the symbol, conditional ones included.
    pub max_orders: Option<u32>,
    /// MAX_NUM_ALGO_ORDERS: open conditional orders per account on the symbol.
    pub max_algo_orders: Option<u32>,
}

fn bound(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn whole(d: OptDec) -> Option<u32> {
    d.0.and_then(|v| v.trunc().to_string().parse().ok())
}

/// The rules a symbol's `exchangeInfo` filters state beyond tick and lot, either market:
/// what the instrument list stores on each instrument (spec 4.1).
pub fn trading_rules(m: &Market, filters: &[super::dto::Filter]) -> TradingRules {
    let mut r = TradingRules::default();
    for f in filters {
        match (f.filter_type.as_str(), m.kind) {
            ("PRICE_FILTER", _) => {
                r.min_price = bound(f.min_price);
                r.max_price = bound(f.max_price);
            }
            ("LOT_SIZE", _) => r.max_qty = bound(f.max_qty),
            ("MARKET_LOT_SIZE", _) => {
                r.market_min_qty = bound(f.min_qty);
                r.market_max_qty = bound(f.max_qty);
                r.market_step = bound(f.step_size);
            }
            ("NOTIONAL", Kind::Spot) => {
                let mins = whole(f.avg_price_mins);
                r.min_notional = bound(f.min_notional).map(|value| Notional { value, market: f.apply_min_to_market.unwrap_or(false), avg_mins: mins, filter: "NOTIONAL".into() });
                r.max_notional = bound(f.max_notional).map(|value| Notional { value, market: f.apply_max_to_market.unwrap_or(false), avg_mins: mins, filter: "NOTIONAL".into() });
            }
            ("MIN_NOTIONAL", Kind::Spot) => {
                r.min_notional = bound(f.min_notional).map(|value| Notional { value, market: f.apply_to_market.unwrap_or(false), avg_mins: whole(f.avg_price_mins), filter: "MIN_NOTIONAL".into() });
            }
            // USDⓈ-M values a market order at the mark price.
            ("MIN_NOTIONAL", Kind::UsdM) => {
                r.min_notional = bound(f.notional).map(|value| Notional { value, market: true, avg_mins: None, filter: "MIN_NOTIONAL".into() });
            }
            ("PERCENT_PRICE", Kind::Spot) => {
                let side = (bound(f.multiplier_down), bound(f.multiplier_up));
                r.price_band = Some(Band { buy: side, sell: side, avg_mins: whole(f.avg_price_mins), filter: "PERCENT_PRICE".into() });
            }
            ("PERCENT_PRICE_BY_SIDE", Kind::Spot) => {
                r.price_band = Some(Band {
                    buy: (bound(f.bid_multiplier_down), bound(f.bid_multiplier_up)),
                    sell: (bound(f.ask_multiplier_down), bound(f.ask_multiplier_up)),
                    avg_mins: whole(f.avg_price_mins),
                    filter: "PERCENT_PRICE_BY_SIDE".into(),
                });
            }
            // USDⓈ-M caps a buy above the mark price and floors a sell below it.
            ("PERCENT_PRICE", Kind::UsdM) => {
                r.price_band = Some(Band { buy: (None, bound(f.multiplier_up)), sell: (bound(f.multiplier_down), None), avg_mins: None, filter: "PERCENT_PRICE".into() });
            }
            ("MAX_NUM_ORDERS", Kind::Spot) => r.max_orders = whole(f.max_num_orders),
            ("MAX_NUM_ALGO_ORDERS", Kind::Spot) => r.max_algo_orders = whole(f.max_num_algo_orders),
            ("MAX_NUM_ORDERS", Kind::UsdM) => r.max_orders = whole(f.limit),
            ("MAX_NUM_ALGO_ORDERS", Kind::UsdM) => r.max_algo_orders = whole(f.limit),
            _ => {}
        }
    }
    r
}

/// The filters an order on `spec` is checked against: its tick and lot, and its stored rules.
pub fn rules_of(spec: &InstrumentSpec) -> Rules {
    let t = &spec.rules;
    Rules {
        tick: spec.tick_size.map(|p| p.value()).filter(|v| *v > Decimal::ZERO),
        min_price: t.min_price,
        max_price: t.max_price,
        step: spec.lot_size.map(|q| q.value()).filter(|v| *v > Decimal::ZERO),
        max_qty: t.max_qty,
        market_min_qty: t.market_min_qty,
        market_max_qty: t.market_max_qty,
        market_step: t.market_step,
        min_notional: t.min_notional.clone(),
        max_notional: t.max_notional.clone(),
        band: t.price_band.clone(),
        max_orders: t.max_orders,
        max_algo_orders: t.max_algo_orders,
    }
}

/// What a band, and a market order's notional, is measured against, read just before sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reference {
    /// Spot's reference price, or USDⓈ-M's mark price: what every filter of the symbol reads.
    Venue(Decimal),
    /// Spot's average price over `mins` minutes (`avgPrice`), read when the symbol has no
    /// reference price: it stands in only for a filter averaging over those same minutes.
    Average { mins: u32, price: Decimal },
}

impl Reference {
    fn for_filter(self, avg_mins: Option<u32>) -> Option<Decimal> {
        match self {
            Reference::Venue(p) => Some(p),
            Reference::Average { mins, price } => (avg_mins == Some(mins)).then_some(price),
        }
    }

    fn word(self, m: &Market) -> String {
        match (self, m.kind) {
            (Reference::Venue(_), Kind::Spot) => "reference price".into(),
            (Reference::Venue(_), Kind::UsdM) => "mark price".into(),
            (Reference::Average { mins, .. }, _) => format!("{mins} min average"),
        }
    }
}

/// The price a filter values `o` at: its limit where the venue says so (spot: any order with a
/// price; USDⓈ-M: a plain limit, conditional orders being checked when they fire).
fn valued_at(m: &Market, o: &OrderRequest) -> Option<Decimal> {
    match (m.kind, o.order_type) {
        (Kind::Spot, OrderType::Limit | OrderType::StopLimit | OrderType::Lit) | (Kind::UsdM, OrderType::Limit) => o.limit.map(|p| p.value()),
        _ => None,
    }
}

/// Whether the band or a market order's notional needs a reference price for this order.
pub fn needs_reference(m: &Market, r: &Rules, o: &OrderRequest) -> bool {
    let market = o.order_type == OrderType::Market && (r.min_notional.as_ref().is_some_and(|n| n.market) || r.max_notional.as_ref().is_some_and(|n| n.market));
    market || (r.band.is_some() && valued_at(m, o).is_some())
}

/// `o` against the symbol's filters beyond tick and lot: price range, maximum size, a market
/// order's lot, notional and the price band (the last two when `reference` is at hand: a filter
/// whose reference is not known is the venue's to apply). `reduce_only`: forced on by the
/// caller (a USDⓈ-M exit OTW links), which lets a USDⓈ-M order under the minimum notional. A
/// refusal names the filter and the nearest values that pass; nothing is adjusted.
pub fn check_filters(m: &Market, r: &Rules, o: &OrderRequest, reduce_only: bool, reference: Option<Reference>) -> Result<(), ConnectorError> {
    let sym = &o.instrument.native;
    let refuse = |msg: String| Err(ConnectorError::invalid(msg));
    for (what, p) in [("limit", o.limit), ("stop", o.stop)] {
        let Some(p) = p.map(|p| p.value()) else { continue };
        if let Some(min) = r.min_price.filter(|min| p < *min) {
            return refuse(format!("{sym} takes prices from {} (PRICE_FILTER): the {what} price {} is below it", num(min), num(p)));
        }
        if let Some(max) = r.max_price.filter(|max| p > *max) {
            return refuse(format!("{sym} takes prices up to {} (PRICE_FILTER): the {what} price {} is above it", num(max), num(p)));
        }
    }
    // A close-position stop carries no size.
    if m.kind == Kind::UsdM && o.opts.close_position {
        return Ok(());
    }
    let qty = o.qty.value();
    if let Some(max) = r.max_qty.filter(|max| qty > *max) {
        return refuse(format!("{sym} takes at most {} per order (LOT_SIZE): {} is over it", num(max), num(qty)));
    }
    let market = o.order_type == OrderType::Market;
    if market {
        if let Some(min) = r.market_min_qty.filter(|min| qty < *min) {
            return refuse(format!("a market order on {sym} is at least {} (MARKET_LOT_SIZE): {} is under it", num(min), num(qty)));
        }
        if let Some(max) = r.market_max_qty.filter(|max| qty > *max) {
            return refuse(format!("a market order on {sym} is at most {} (MARKET_LOT_SIZE): {} is over it; split it or use a limit", num(max), num(qty)));
        }
        if let Some(step) = r.market_step.filter(|step| !(qty % *step).is_zero()) {
            let (lo, hi) = either_side(qty, step);
            return refuse(format!("a market order on {sym} moves in steps of {} (MARKET_LOT_SIZE): {} is off that step ({} or {})", num(step), num(qty), num(lo), num(hi)));
        }
    }
    let priced = valued_at(m, o);
    let reducing = m.kind == Kind::UsdM && (reduce_only || o.opts.reduce_only);
    for (b, floor) in [(r.min_notional.as_ref(), true), (r.max_notional.as_ref(), false)] {
        let Some(b) = b else { continue };
        if floor && reducing {
            continue;
        }
        let (price, at) = match priced {
            Some(p) => (p, String::new()),
            None if market && b.market => match reference.and_then(|rf| rf.for_filter(b.avg_mins).map(|p| (p, rf))) {
                Some((p, rf)) => (p, format!(" (the {})", rf.word(m))),
                None => continue,
            },
            None => continue,
        };
        let worth = price * qty;
        if (floor && worth < b.value) || (!floor && worth > b.value) {
            let size = b.value / price;
            let (lo, hi) = r.step.map(|s| either_side(size, s)).unwrap_or((size, size));
            let (bound_word, size_word, near) = if floor { ("at least", "smallest", hi) } else { ("at most", "largest", lo) };
            return refuse(format!(
                "{sym} takes orders worth {bound_word} {} ({}): {} at {}{at} is worth {}; {} is the {size_word} size at that price",
                num(b.value),
                b.filter,
                num(qty),
                num(price),
                num(worth.round_dp(8)),
                num(near.round_dp(8)),
            ));
        }
    }
    if let (Some(b), Some(p), Some(rf)) = (r.band.as_ref(), priced, reference) {
        if let Some(refp) = rf.for_filter(b.avg_mins) {
            let (down, up) = if o.side == Side::Buy { b.buy } else { b.sell };
            let lo = down.map(|d| refp * d);
            let hi = up.map(|u| refp * u);
            if lo.is_some_and(|lo| p < lo) || hi.is_some_and(|hi| p > hi) {
                // The band's edges on the tick grid, inside the band.
                let lo = lo.map(|v| r.tick.map(|t| either_side(v, t).1).unwrap_or(v).round_dp(8));
                let hi = hi.map(|v| r.tick.map(|t| either_side(v, t).0).unwrap_or(v).round_dp(8));
                let range = match (lo, hi) {
                    (Some(lo), Some(hi)) => format!("between {} and {}", num(lo), num(hi)),
                    (Some(lo), None) => format!("from {}", num(lo)),
                    (None, Some(hi)) => format!("up to {}", num(hi)),
                    (None, None) => String::new(),
                };
                let side = if o.side == Side::Buy { "buy" } else { "sell" };
                return refuse(format!("{sym} takes {side} prices {range} right now ({}, {} {}): {} is outside it", b.filter, rf.word(m), num(refp), num(p)));
            }
        }
    }
    Ok(())
}

/// Whether a working order counts as a conditional one (MAX_NUM_ALGO_ORDERS): spot's
/// `STOP_LOSS` and `TAKE_PROFIT` families, USDⓈ-M's algo book.
pub fn is_algo(o: &VenueOrder) -> bool {
    o.r.algo || matches!(o.kind.as_str(), "STOP_LOSS" | "STOP_LOSS_LIMIT" | "TAKE_PROFIT" | "TAKE_PROFIT_LIMIT")
}

/// The open-order caps on a symbol: `working` (of which `working_algo` conditional) work now,
/// the ticket adds `adding` (of which `adding_algo`).
pub fn check_open(r: &Rules, symbol: &str, working: usize, working_algo: usize, adding: usize, adding_algo: usize) -> Result<(), ConnectorError> {
    for (cap, now, add, filter, what) in [(r.max_orders, working, adding, "MAX_NUM_ORDERS", "open orders"), (r.max_algo_orders, working_algo, adding_algo, "MAX_NUM_ALGO_ORDERS", "open conditional orders")] {
        let Some(cap) = cap.map(|c| c as usize) else { continue };
        if add > 0 && now + add > cap {
            return Err(ConnectorError::invalid(format!(
                "Binance holds at most {cap} {what} per account on {symbol} ({filter}): {now} work there now and this ticket adds {add}; cancel {} first",
                now + add - cap
            )));
        }
    }
    Ok(())
}

/// Spot: the symbol's reference price, `-2043` when it never had one.
pub fn reference_price(symbol: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get("referencePrice", REST, "/api/v3/referencePrice", corr).query("symbol", symbol).weight(2)
}

/// Spot: the average price filters fall back on.
pub fn avg_price(symbol: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get("avgPrice", REST, "/api/v3/avgPrice", corr).query("symbol", symbol).weight(2)
}

/// Spot: one symbol's terms, not the whole list (`exchangeInfo?symbol=`).
pub fn symbol_info(symbol: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get("exchangeInfo", REST, "/api/v3/exchangeInfo", corr).query("symbol", symbol).weight(20)
}

/// A symbol's base and quote assets, from [`symbol_info`].
pub fn decode_symbol_assets(body: &[u8]) -> Result<Option<(String, String)>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct S {
        base_asset: String,
        quote_asset: String,
    }
    #[derive(Deserialize)]
    struct I {
        symbols: Vec<S>,
    }
    let i: I = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance exchangeInfo: {e}")))?;
    Ok(i.symbols.into_iter().next().map(|s| (s.base_asset, s.quote_asset)))
}

/// USDⓈ-M: the mark price.
pub fn mark_price(symbol: &str, corr: CorrId) -> HttpRequest {
    HttpRequest::get("premiumIndex", REST, "/fapi/v1/premiumIndex", corr).query("symbol", symbol).weight(1)
}

/// The reference a spot `referencePrice` or a USDⓈ-M `premiumIndex` answer states; `None`
/// when it is null.
pub fn decode_reference(m: &Market, body: &[u8]) -> Result<Option<Reference>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct R {
        #[serde(default)]
        reference_price: OptDec,
        #[serde(default)]
        mark_price: OptDec,
    }
    let r: R = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("{} reference price: {e}", m.name)))?;
    Ok(match m.kind {
        Kind::Spot => r.reference_price.0,
        Kind::UsdM => r.mark_price.0,
    }
    .filter(|p| *p > Decimal::ZERO)
    .map(Reference::Venue))
}

pub fn decode_avg_price(body: &[u8]) -> Result<Reference, ConnectorError> {
    #[derive(Deserialize)]
    struct A {
        mins: u32,
        price: OptDec,
    }
    let a: A = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance avgPrice: {e}")))?;
    let price = a.price.0.ok_or_else(|| ConnectorError::decode("binance avgPrice without a price"))?;
    Ok(Reference::Average { mins: a.mins, price })
}

/// The working orders on one symbol (USDⓈ-M: the order book; the algo book is
/// [`open_algo_on`]).
pub fn open_on(m: &Market, symbol: &str, corr: CorrId) -> HttpRequest {
    let weight = if m.kind == Kind::Spot { 6 } else { 1 };
    call("openOrders", Method::Get, m.open_orders, corr, weight).query("symbol", symbol)
}

/// USDⓈ-M: the working conditional orders on one symbol.
pub fn open_algo_on(symbol: &str, corr: CorrId) -> HttpRequest {
    call("openAlgoOrders", Method::Get, "/fapi/v1/openAlgoOrders", corr, 1).query("symbol", symbol)
}

fn num(d: Decimal) -> String {
    d.normalize().to_string()
}

fn call(endpoint: &'static str, method: Method, path: &'static str, corr: CorrId, weight: u32) -> HttpRequest {
    HttpRequest { method, ..HttpRequest::get(endpoint, REST, path, corr) }.weight(weight).signed()
}

fn side_word(s: Side) -> &'static str {
    match s {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

fn unsupported(m: &Market, what: &str) -> ConnectorError {
    ConnectorError::unsupported(format!("{} takes no {what}", m.name))
}

/// Time in force: the word, and the expiry a `gtd` carries.
fn tif_of(m: &Market, t: TimeInForce) -> Result<(&'static str, Option<i64>), ConnectorError> {
    match (t, m.kind) {
        (TimeInForce::Gtc, _) => Ok(("GTC", None)),
        (TimeInForce::Ioc, _) => Ok(("IOC", None)),
        (TimeInForce::Fok, _) => Ok(("FOK", None)),
        (TimeInForce::Gtd(until), Kind::UsdM) => Ok(("GTD", Some(until.millis()))),
        (TimeInForce::Gtd(_), Kind::Spot) => Err(unsupported(m, "good-till-date order")),
        (TimeInForce::Day, _) => Err(unsupported(m, "day order: choose good-till-cancelled or immediate-or-cancel")),
    }
}

fn with_tif(mut h: HttpRequest, m: &Market, t: TimeInForce) -> Result<HttpRequest, ConnectorError> {
    let (word, until) = tif_of(m, t)?;
    h = h.query("timeInForce", word);
    if let Some(ms) = until {
        h = h.query("goodTillDate", ms);
    }
    Ok(h)
}

fn price_of(p: Option<Price>, what: &str) -> Result<String, ConnectorError> {
    p.map(|p| num(p.value())).ok_or_else(|| ConnectorError::invalid(format!("this order needs a {what} price")))
}

/// A trailing stop's distance as a percentage: Binance takes no amount.
fn trail_pct(m: &Market, o: &OrderRequest) -> Result<Decimal, ConnectorError> {
    match o.opts.trail {
        Some(Trail::Percent(p)) => Ok(p),
        Some(Trail::Amount(_)) => Err(unsupported(m, "trailing distance as an amount: give it as a percentage")),
        None => Err(ConnectorError::invalid("a trailing stop needs its distance")),
    }
}

/// Spot trails in basis points, whole ones: 1.25% is 125.
fn trailing_delta(m: &Market, o: &OrderRequest) -> Result<String, ConnectorError> {
    let bips = trail_pct(m, o)? * Decimal::ONE_HUNDRED;
    if !bips.fract().is_zero() {
        return Err(ConnectorError::invalid("Binance spot trails in steps of 0.01%"));
    }
    Ok(bips.trunc().normalize().to_string())
}

/// One spot order's type and prices, under `prefix` (`""` for a lone order, `above`, `below`,
/// `working`, `pending`, `pendingAbove`… inside an order list, where each name is the prefix
/// followed by the capitalised field).
fn spot_fields(m: &Market, o: &OrderRequest, mut h: HttpRequest, prefix: &str) -> Result<HttpRequest, ConnectorError> {
    let key = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            let mut c = name.chars();
            let first = c.next().map(|f| f.to_ascii_uppercase().to_string()).unwrap_or_default();
            format!("{prefix}{first}{}", c.as_str())
        }
    };
    let x = &o.opts;
    let (word, limit, stop, tif) = match o.order_type {
        OrderType::Market => ("MARKET", false, false, false),
        OrderType::Limit if x.post_only => ("LIMIT_MAKER", true, false, false),
        OrderType::Limit => ("LIMIT", true, false, true),
        OrderType::Stop | OrderType::TrailingStop => ("STOP_LOSS", false, true, false),
        OrderType::StopLimit => ("STOP_LOSS_LIMIT", true, true, true),
        OrderType::Mit => ("TAKE_PROFIT", false, true, false),
        OrderType::Lit => ("TAKE_PROFIT_LIMIT", true, true, true),
    };
    h = h.query(&key("type"), word);
    if limit {
        h = h.query(&key("price"), price_of(o.limit, "limit")?);
    }
    if o.order_type == OrderType::TrailingStop {
        h = h.query(&key("trailingDelta"), trailing_delta(m, o)?);
        // The price it starts trailing at, when given.
        if let Some(p) = o.stop {
            h = h.query(&key("stopPrice"), num(p.value()));
        }
    } else if stop {
        h = h.query(&key("stopPrice"), price_of(o.stop, "stop")?);
    }
    if tif {
        let (word, _) = tif_of(m, o.tif)?;
        h = h.query(&key("timeInForce"), word);
    }
    if let Some(d) = x.display_qty {
        if !(word == "LIMIT_MAKER" || (tif && o.tif == TimeInForce::Gtc)) {
            return Err(ConnectorError::invalid("a Binance iceberg is a good-till-cancelled limit order"));
        }
        h = h.query(&key("icebergQty"), num(d.value()));
    }
    Ok(h)
}

/// The futures position side an order works on in hedge mode: a buy opens the long and a sell
/// the short, and one that only reduces (or closes) works the other side's position.
fn position_side(o: &OrderRequest, reduce_only: bool) -> &'static str {
    match (o.side, reduce_only || o.opts.close_position) {
        (Side::Buy, false) | (Side::Sell, true) => "LONG",
        (Side::Sell, false) | (Side::Buy, true) => "SHORT",
    }
}

/// The request that places `o` on its own. `reduce_only`: forced on (a USDⓈ-M exit OTW links,
/// a close) whatever the order says. `hedge`: the USDⓈ-M account holds long and short apart,
/// so the order names its position side instead of reduce-only.
pub fn place(m: &Market, o: &OrderRequest, reduce_only: bool, hedge: bool, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
    check_client_id(&o.client_order_id)?;
    if o.parent.is_some() || o.oca_group.is_some() {
        return Err(unsupported(m, "attached exit or one-cancels-other group on a single order: they go out as an order list"));
    }
    let reduce = reduce_only || o.opts.reduce_only;
    let x = &o.opts;
    let qty = num(o.qty.value());
    match m.kind {
        Kind::Spot => {
            if reduce || x.close_position {
                return Err(unsupported(m, "reduce-only order: a spot balance is sold, not closed"));
            }
            if x.trigger.is_some() {
                return Err(unsupported(m, "trigger price choice: spot triggers on the last trade"));
            }
            let h = call("order", Method::Post, "/api/v3/order", corr, 1)
                .query("symbol", &o.instrument.native)
                .query("side", side_word(o.side))
                .query("newClientOrderId", &o.client_order_id)
                .query("newOrderRespType", "RESULT")
                .query("quantity", qty);
            spot_fields(m, o, h, "")
        }
        Kind::UsdM => {
            if x.display_qty.is_some() {
                return Err(unsupported(m, "iceberg order"));
            }
            let algo = placed_ref(m, o).algo;
            let mut h = if algo {
                call("algoOrder", Method::Post, "/fapi/v1/algoOrder", corr, 1)
                    .query("symbol", &o.instrument.native)
                    .query("side", side_word(o.side))
                    .query("algoType", "CONDITIONAL")
                    .query("clientAlgoId", &o.client_order_id)
            } else {
                call("order", Method::Post, "/fapi/v1/order", corr, 1)
                    .query("symbol", &o.instrument.native)
                    .query("side", side_word(o.side))
                    .query("newClientOrderId", &o.client_order_id)
                    .query("newOrderRespType", "RESULT")
            };
            let (word, limit, trigger, tif) = match o.order_type {
                OrderType::Market => ("MARKET", false, false, false),
                OrderType::Limit => ("LIMIT", true, false, true),
                OrderType::Stop => ("STOP_MARKET", false, true, false),
                OrderType::StopLimit => ("STOP", true, true, true),
                OrderType::TrailingStop => ("TRAILING_STOP_MARKET", false, false, false),
                OrderType::Mit => ("TAKE_PROFIT_MARKET", false, true, false),
                OrderType::Lit => ("TAKE_PROFIT", true, true, true),
            };
            h = h.query("type", word);
            // A close-position stop closes what is held, whatever its size.
            if !x.close_position {
                h = h.query("quantity", qty);
            }
            if limit {
                h = h.query("price", price_of(o.limit, "limit")?);
            }
            if trigger {
                h = h.query("triggerPrice", price_of(o.stop, "stop")?);
            }
            if o.order_type == OrderType::TrailingStop {
                h = h.query("callbackRate", num(trail_pct(m, o)?));
                if let Some(p) = o.stop {
                    h = h.query("activatePrice", num(p.value()));
                }
            }
            if tif {
                if x.post_only {
                    // GTX: good till crossing, Binance's post-only.
                    h = h.query("timeInForce", "GTX");
                } else {
                    h = with_tif(h, m, o.tif)?;
                }
            }
            match x.trigger {
                None => {}
                Some(TriggerKind::Mark) => h = h.query("workingType", "MARK_PRICE"),
                Some(TriggerKind::Last) => h = h.query("workingType", "CONTRACT_PRICE"),
                Some(t) => return Err(unsupported(m, &format!("{t:?} trigger").to_lowercase())),
            }
            if x.close_position {
                h = h.query("closePosition", "true");
            }
            if hedge {
                h = h.query("positionSide", position_side(o, reduce));
            } else if reduce && !x.close_position {
                // Binance refuses reduce-only beside close-position: closing already only reduces.
                h = h.query("reduceOnly", "true");
            }
            Ok(h)
        }
    }
}

/// Which half of a spot OCO pair an order is: above the market or below it. A sell takes
/// profit above and stops below; a buy the other way round.
fn above(o: &OrderRequest) -> bool {
    let stop_side = matches!(o.order_type, OrderType::Stop | OrderType::StopLimit | OrderType::TrailingStop);
    (o.side == Side::Buy) == stop_side
}

/// A spot order list for orders sent together, when they are one (Binance links them itself):
///   - two orders on one side in one one-cancels-other group: an **OCO**, a take-profit half
///     (limit, if touched) and a stop half (stop, stop-limit, trailing);
///   - a limit entry and one exit: an **OTO**, the exit placed once the entry fills;
///   - a limit entry and two exits: an **OTOCO**, the exits an OCO placed once it fills.
///
/// `None` for a single order. A shape Binance cannot link is refused with what it takes.
pub fn place_list(m: &Market, orders: &[OrderRequest], corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
    if m.kind != Kind::Spot || orders.len() < 2 {
        return Ok(None);
    }
    for o in orders {
        check_client_id(&o.client_order_id)?;
        if o.opts.reduce_only || o.opts.close_position {
            return Err(unsupported(m, "reduce-only order: a spot balance is sold, not closed"));
        }
    }
    let sym = &orders[0].instrument.native;
    let entry = &orders[0];
    let exits: Vec<&OrderRequest> = orders[1..].iter().filter(|o| o.parent.as_deref() == Some(entry.client_order_id.as_str())).collect();
    let pair = |a: &OrderRequest, b: &OrderRequest, prefix: &str, h: HttpRequest| -> Result<HttpRequest, ConnectorError> {
        if a.side != b.side || a.qty != b.qty {
            return Err(ConnectorError::invalid("the two halves of a Binance OCO are on one side, for one size"));
        }
        let (hi, lo) = match (above(a), above(b)) {
            (true, false) => (a, b),
            (false, true) => (b, a),
            _ => return Err(ConnectorError::invalid("a Binance OCO pairs a take-profit (limit, if touched) with a stop (stop, stop-limit, trailing)")),
        };
        let cap = |p: &str| if prefix.is_empty() { p.to_string() } else { format!("{prefix}{}{}", &p[..1].to_ascii_uppercase(), &p[1..]) };
        // A limit half of an OCO only rests: Binance takes it as a limit maker.
        let maker = |o: &OrderRequest| {
            let mut o = o.clone();
            o.opts.post_only |= o.order_type == OrderType::Limit;
            o
        };
        let h = h.query(&(cap("above") + "ClientOrderId"), &hi.client_order_id).query(&(cap("below") + "ClientOrderId"), &lo.client_order_id);
        let h = spot_fields(m, &maker(hi), h, &cap("above"))?;
        spot_fields(m, &maker(lo), h, &cap("below"))
    };
    let working = |h: HttpRequest| -> Result<HttpRequest, ConnectorError> {
        if entry.order_type != OrderType::Limit {
            return Err(ConnectorError::invalid(
                "Binance spot holds exits for a limit entry, or places them once a market entry fills: a stop entry takes no exits",
            ));
        }
        let h = h.query("workingSide", side_word(entry.side)).query("workingClientOrderId", &entry.client_order_id).query("workingQuantity", num(entry.qty.value()));
        spot_fields(m, entry, h, "working")
    };
    let base = |endpoint: &'static str, path: &'static str| call(endpoint, Method::Post, path, corr, 1).query("symbol", sym).query("newOrderRespType", "RESULT");
    match (orders.len(), exits.len()) {
        (2, 0) => {
            let (a, b) = (&orders[0], &orders[1]);
            if a.parent.is_some() || b.parent.is_some() || a.oca_group.is_none() || a.oca_group != b.oca_group {
                return Err(ConnectorError::invalid("orders sent together on Binance spot are an OCO pair or an entry with its exits"));
            }
            let h = base("orderListOco", "/api/v3/orderList/oco").query("side", side_word(a.side)).query("quantity", num(a.qty.value()));
            pair(a, b, "", h).map(Some)
        }
        (2, 1) => {
            let x = exits[0];
            let h = working(base("orderListOto", "/api/v3/orderList/oto"))?
                .query("pendingSide", side_word(x.side))
                .query("pendingClientOrderId", &x.client_order_id)
                .query("pendingQuantity", num(x.qty.value()));
            spot_fields(m, x, h, "pending").map(Some)
        }
        (3, 2) => {
            let h = working(base("orderListOtoco", "/api/v3/orderList/otoco"))?
                .query("pendingSide", side_word(exits[0].side))
                .query("pendingQuantity", num(exits[0].qty.value()));
            pair(exits[0], exits[1], "pending", h).map(Some)
        }
        _ => Err(ConnectorError::invalid("Binance spot links an OCO pair, or one entry with one or two exits")),
    }
}

/// The id OTW names a placed order by.
pub fn placed_ref(m: &Market, o: &OrderRequest) -> OrderRef {
    let algo = m.kind == Kind::UsdM && o.order_type.triggered();
    OrderRef { symbol: o.instrument.native.clone(), client_id: o.client_order_id.clone(), algo }
}

/// One order as Binance reads it back.
pub fn query(m: &Market, r: &OrderRef, corr: CorrId) -> HttpRequest {
    match (m.kind, r.algo) {
        (Kind::Spot, _) => call("order", Method::Get, "/api/v3/order", corr, 4).query("symbol", &r.symbol).query("origClientOrderId", &r.client_id),
        (Kind::UsdM, false) => call("order", Method::Get, "/fapi/v1/order", corr, 1).query("symbol", &r.symbol).query("origClientOrderId", &r.client_id),
        (Kind::UsdM, true) => call("algoOrder", Method::Get, "/fapi/v1/algoOrder", corr, 1).query("clientAlgoId", &r.client_id),
    }
}

pub fn cancel(m: &Market, r: &OrderRef, corr: CorrId) -> HttpRequest {
    match (m.kind, r.algo) {
        (Kind::Spot, _) => call("cancel", Method::Delete, "/api/v3/order", corr, 1).query("symbol", &r.symbol).query("origClientOrderId", &r.client_id),
        (Kind::UsdM, false) => call("cancel", Method::Delete, "/fapi/v1/order", corr, 1).query("symbol", &r.symbol).query("origClientOrderId", &r.client_id),
        (Kind::UsdM, true) => call("cancelAlgo", Method::Delete, "/fapi/v1/algoOrder", corr, 1).query("clientAlgoId", &r.client_id),
    }
}

/// Every working order of the account, all symbols (USDⓈ-M: the order book; the algo book is
/// [`open_algo`]).
pub fn open_orders(m: &Market, corr: CorrId) -> HttpRequest {
    let weight = if m.kind == Kind::Spot { 80 } else { 40 };
    call("openOrders", Method::Get, m.open_orders, corr, weight)
}

/// USDⓈ-M: the working conditional orders, all symbols.
pub fn open_algo(corr: CorrId) -> HttpRequest {
    call("openAlgoOrders", Method::Get, "/fapi/v1/openAlgoOrders", corr, 40)
}

/// What cancels everything working on one symbol: one call on spot, the order book and the
/// algo book on USDⓈ-M.
pub fn cancel_symbol(m: &Market, symbol: &str, corr: CorrId) -> Vec<HttpRequest> {
    match m.kind {
        Kind::Spot => vec![call("cancelAll", Method::Delete, "/api/v3/openOrders", corr, 1).query("symbol", symbol)],
        Kind::UsdM => vec![
            call("cancelAll", Method::Delete, "/fapi/v1/allOpenOrders", corr, 1).query("symbol", symbol),
            call("cancelAllAlgo", Method::Delete, "/fapi/v1/algoOpenOrders", corr, 1).query("symbol", symbol),
        ],
    }
}

/// USDⓈ-M: whether the account holds long and short apart (hedge mode).
pub fn position_mode(corr: CorrId) -> HttpRequest {
    call("positionSideDual", Method::Get, "/fapi/v1/positionSide/dual", corr, 30)
}

/// USDⓈ-M: the position on one symbol.
pub fn position(symbol: &str, corr: CorrId) -> HttpRequest {
    call("positionRisk", Method::Get, "/fapi/v3/positionRisk", corr, 5).query("symbol", symbol)
}

/// USDⓈ-M account figures.
pub fn futures_account(corr: CorrId) -> HttpRequest {
    call("account", Method::Get, "/fapi/v3/account", corr, 5)
}

/// Spot: the client ids of an order list's orders (an OCO, OTO or OTOCO).
pub fn order_list(list_id: i64, corr: CorrId) -> HttpRequest {
    call("orderList", Method::Get, "/api/v3/orderList", corr, 4).query("orderListId", &list_id.to_string())
}

pub fn decode_order_list(body: &[u8]) -> Result<Vec<String>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct O {
        client_order_id: String,
    }
    #[derive(Deserialize)]
    struct L {
        orders: Vec<O>,
    }
    let l: L = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance orderList: {e}")))?;
    Ok(l.orders.into_iter().map(|o| o.client_order_id).collect())
}

/// Spot: cancel a whole order list.
pub fn cancel_list(symbol: &str, list_id: i64, corr: CorrId) -> HttpRequest {
    call("cancelOrderList", Method::Delete, "/api/v3/orderList", corr, 1).query("symbol", symbol).query("orderListId", &list_id.to_string())
}

/// Spot: a working order as the request that places it again under its own client id, with
/// `a` applied (the order the user changed). A trailing stop carries no stop price to place
/// again: refused.
pub fn spot_request(m: &Market, cur: &VenueOrder, a: Option<&OrderAmend>) -> Result<OrderRequest, ConnectorError> {
    let (order_type, post_only) = match cur.kind.as_str() {
        "LIMIT" => (OrderType::Limit, false),
        "LIMIT_MAKER" => (OrderType::Limit, true),
        "STOP_LOSS" if cur.stop.is_some() => (OrderType::Stop, false),
        "STOP_LOSS_LIMIT" => (OrderType::StopLimit, false),
        "TAKE_PROFIT" => (OrderType::Mit, false),
        "TAKE_PROFIT_LIMIT" => (OrderType::Lit, false),
        other => return Err(ConnectorError::unsupported(format!("{} cannot place a {other} order of a list again: cancel it and place a new one", m.name))),
    };
    let tif = match (a.and_then(|a| a.tif), cur.tif.as_deref()) {
        (Some(t), _) => t,
        (None, Some("IOC")) => TimeInForce::Ioc,
        (None, Some("FOK")) => TimeInForce::Fok,
        _ => TimeInForce::Gtc,
    };
    let (limit, stop) = (order_type.prices().0, order_type.prices().1);
    Ok(OrderRequest {
        client_order_id: cur.r.client_id.clone(),
        instrument: otw_connector_model::InstrumentId::new(m.venue, &cur.r.symbol),
        side: cur.side,
        order_type,
        qty: Qty::new(a.and_then(|a| a.qty).map(|q| q.value()).unwrap_or(cur.qty)),
        limit: if limit { a.and_then(|a| a.limit).or(cur.limit.map(Price::new)) } else { None },
        stop: if stop { a.and_then(|a| a.stop).or(cur.stop.map(Price::new)) } else { None },
        tif,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts { post_only, ..Default::default() },
    })
}

/// A change to a working order, where Binance changes it in place or by cancel-replace with
/// the same client id: spot (any type, `cancelReplace`) and a USDⓈ-M limit order (`PUT`). A
/// USDⓈ-M stop has no such call: [`replacement`] is the order that takes its place.
pub fn amend(m: &Market, cur: &VenueOrder, a: &OrderAmend, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
    let carries_limit = cur.limit.is_some() || matches!(cur.kind.as_str(), "LIMIT" | "STOP_LOSS_LIMIT" | "TAKE_PROFIT_LIMIT" | "LIMIT_MAKER" | "STOP");
    let carries_stop = cur.stop.is_some();
    if a.limit.is_some() && !carries_limit {
        return Err(ConnectorError::invalid("this order carries no limit price"));
    }
    if a.stop.is_some() && !carries_stop {
        return Err(ConnectorError::invalid("this order carries no stop price"));
    }
    let total = a.qty.map(|q| q.value()).unwrap_or(cur.qty);
    let left = total - cur.filled;
    if left <= Decimal::ZERO {
        return Err(ConnectorError::invalid(format!("{} of this order has already filled: a size of {} leaves nothing to work", num(cur.filled), num(total))));
    }
    let r = &cur.r;
    match m.kind {
        Kind::Spot => {
            let mut h = call("cancelReplace", Method::Post, "/api/v3/order/cancelReplace", corr, 1)
                .query("symbol", &r.symbol)
                .query("side", side_word(cur.side))
                .query("type", &cur.kind)
                .query("cancelReplaceMode", "STOP_ON_FAILURE")
                .query("cancelOrigClientOrderId", &r.client_id)
                .query("newClientOrderId", &r.client_id)
                .query("quantity", num(left));
            if carries_limit {
                let p = a.limit.map(|p| p.value()).or(cur.limit).ok_or_else(|| ConnectorError::decode("binance answered no price for a limit order"))?;
                h = h.query("price", num(p));
                let tif = match a.tif {
                    Some(t) => tif_of(m, t)?.0.to_string(),
                    None => cur.tif.clone().unwrap_or_else(|| "GTC".into()),
                };
                // A limit maker takes no time in force.
                if cur.kind != "LIMIT_MAKER" {
                    h = h.query("timeInForce", tif);
                }
            }
            if let Some(s) = a.stop.map(|p| p.value()).or(cur.stop) {
                h = h.query("stopPrice", num(s));
            }
            Ok(h)
        }
        Kind::UsdM if !r.algo => {
            if a.tif.is_some() {
                return Err(ConnectorError::unsupported("Binance futures changes an order's price and size in place; a new time in force is a new order"));
            }
            let p = a.limit.map(|p| p.value()).or(cur.limit).ok_or_else(|| ConnectorError::unsupported("Binance futures changes limit orders only in place"))?;
            Ok(call("modify", Method::Put, "/fapi/v1/order", corr, 1)
                .query("symbol", &r.symbol)
                .query("side", side_word(cur.side))
                .query("origClientOrderId", &r.client_id)
                .query("quantity", num(total))
                .query("price", num(p)))
        }
        Kind::UsdM => Err(ConnectorError::unsupported("a Binance futures stop is changed by cancel and replace")),
    }
}

/// The client id a USDⓈ-M stop's replacement goes out under: Binance has no change in place for
/// an algo order, and refuses an id while its first holder works, so the new order takes the
/// next of `k1.r1`, `k1.r2`... (the base cut to keep 36 characters).
pub fn next_client_id(id: &str) -> String {
    let (base, n) = match id.rsplit_once(".r") {
        Some((b, n)) if !b.is_empty() && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => (b, n.parse::<u64>().unwrap_or(0)),
        _ => (id, 0),
    };
    let tail = format!(".r{}", n + 1);
    let keep = 36usize.saturating_sub(tail.len()).min(base.len());
    format!("{}{tail}", &base[..keep])
}

/// Whether a working USDⓈ-M order shrinks its position: flagged so, a close-position stop, or
/// in hedge mode one working the other side's position (a sell on the long).
pub fn reduces(o: &VenueOrder) -> bool {
    o.reduce_only
        || o.close_position
        || matches!((o.side, o.position_side.as_deref()), (Side::Sell, Some("LONG")) | (Side::Buy, Some("SHORT")))
}

/// The canonical order that replaces a USDⓈ-M stop with `a` applied, under `client_id` (see
/// [`next_client_id`]). It keeps what the stop closes: reduce-only, close-position, and in
/// hedge mode the position side it works.
pub fn replacement(m: &Market, cur: &VenueOrder, a: &OrderAmend, client_id: &str) -> Result<OrderRequest, ConnectorError> {
    let order_type = match cur.kind.as_str() {
        "STOP_MARKET" => OrderType::Stop,
        "STOP" => OrderType::StopLimit,
        "TAKE_PROFIT_MARKET" => OrderType::Mit,
        "TAKE_PROFIT" => OrderType::Lit,
        "TRAILING_STOP_MARKET" => return Err(ConnectorError::unsupported(format!("{} changes no trailing stop: cancel it and place a new one", m.name))),
        other => return Err(ConnectorError::unsupported(format!("{} cannot change a {other} order here", m.name))),
    };
    let tif = match (a.tif, cur.tif.as_deref()) {
        (Some(t), _) => t,
        (None, Some("IOC")) => TimeInForce::Ioc,
        (None, Some("GTD")) => TimeInForce::Gtd(cur.good_till.and_then(Timestamp::from_millis).ok_or_else(|| ConnectorError::decode("binance answered a good-till-date order without its date"))?),
        _ => TimeInForce::Gtc,
    };
    let qty = a.qty.map(|q| q.value()).unwrap_or(cur.qty);
    check_client_id(client_id)?;
    Ok(OrderRequest {
        client_order_id: client_id.to_string(),
        instrument: otw_connector_model::InstrumentId::new(m.venue, &cur.r.symbol),
        side: cur.side,
        order_type,
        qty: Qty::new(qty),
        limit: if order_type.prices().0 { a.limit.or(cur.limit.map(Price::new)) } else { None },
        stop: a.stop.or(cur.stop.map(Price::new)),
        tif,
        parent: None,
        oca_group: None,
        account: None,
        opts: OrderOpts { reduce_only: reduces(cur), close_position: cur.close_position, trigger: cur.trigger, ..Default::default() },
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    symbol: String,
    #[serde(default)]
    client_order_id: Option<String>,
    #[serde(default)]
    client_algo_id: Option<String>,
    #[serde(default)]
    order_id: Option<i64>,
    #[serde(default)]
    algo_id: Option<i64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    algo_status: Option<String>,
    side: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    order_type: Option<String>,
    #[serde(default)]
    orig_qty: OptDec,
    #[serde(default)]
    quantity: OptDec,
    #[serde(default)]
    executed_qty: OptDec,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    stop_price: OptDec,
    #[serde(default)]
    trigger_price: OptDec,
    #[serde(default)]
    activate_price: OptDec,
    #[serde(default, alias = "priceRate")]
    callback_rate: OptDec,
    #[serde(default)]
    time_in_force: Option<String>,
    #[serde(default)]
    good_till_date: Option<i64>,
    #[serde(default)]
    reduce_only: Option<bool>,
    #[serde(default)]
    close_position: Option<bool>,
    #[serde(default)]
    position_side: Option<String>,
    #[serde(default)]
    working_type: Option<String>,
    #[serde(default)]
    order_list_id: Option<i64>,
    #[serde(default)]
    time: Option<i64>,
    #[serde(default)]
    transact_time: Option<i64>,
    #[serde(default)]
    create_time: Option<i64>,
}

/// An order as Binance answers it, either book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenueOrder {
    pub r: OrderRef,
    /// Binance's own number for it.
    pub venue_id: String,
    pub status: Mapped<OrderStatus>,
    pub side: Side,
    /// Binance's type word (`LIMIT`, `STOP_LOSS`, `STOP_MARKET`).
    pub kind: String,
    pub qty: Decimal,
    pub filled: Decimal,
    pub limit: Option<Decimal>,
    pub stop: Option<Decimal>,
    /// USDⓈ-M: the price a trailing stop starts trailing at, when it was given one.
    pub trail_start: Option<Decimal>,
    /// USDⓈ-M: how far a trailing stop trails, in percent (`callbackRate`).
    pub trail_rate: Option<Decimal>,
    pub tif: Option<String>,
    pub good_till: Option<i64>,
    pub placed_ms: Option<i64>,
    /// USDⓈ-M: it may only shrink the position.
    pub reduce_only: bool,
    /// USDⓈ-M: a stop that closes the whole position on its trigger.
    pub close_position: bool,
    /// USDⓈ-M hedge mode: the position it works on (`LONG`, `SHORT`; `BOTH` one-way).
    pub position_side: Option<String>,
    /// USDⓈ-M: the price a conditional order triggers on (`workingType`), when it says.
    pub trigger: Option<TriggerKind>,
    /// Spot: the order list it belongs to (an OCO, OTO or OTOCO).
    pub list: Option<i64>,
}

/// Binance's order states, both books. An algo order that fired (`TRIGGERED`, `FINISHED`) has
/// done its work: what it placed is an order of its own.
pub fn status_of(word: &str) -> Mapped<OrderStatus> {
    Mapped::Known(match word {
        "NEW" | "TRIGGERING" => OrderStatus::New,
        "PENDING_NEW" => OrderStatus::PendingNew,
        "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
        "FILLED" | "TRIGGERED" | "FINISHED" => OrderStatus::Filled,
        "PENDING_CANCEL" => OrderStatus::PendingCancel,
        "CANCELED" => OrderStatus::Cancelled,
        "REJECTED" => OrderStatus::Rejected,
        "EXPIRED" | "EXPIRED_IN_MATCH" => OrderStatus::Expired,
        other => return Mapped::Unknown(other.to_string()),
    })
}

fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

fn venue_order(r: Row) -> Result<VenueOrder, ConnectorError> {
    let algo = r.client_algo_id.is_some() || r.algo_id.is_some();
    let client_id = r.client_algo_id.or(r.client_order_id).ok_or_else(|| ConnectorError::decode("binance order without a client id"))?;
    let side = match r.side.as_str() {
        "BUY" => Side::Buy,
        "SELL" => Side::Sell,
        other => return Err(ConnectorError::decode(format!("binance order side {other:?}"))),
    };
    let status = r.algo_status.or(r.status).map(|s| status_of(&s)).unwrap_or(Mapped::Unknown(String::new()));
    Ok(VenueOrder {
        r: OrderRef { symbol: r.symbol, client_id, algo },
        venue_id: r.algo_id.or(r.order_id).map(|n| n.to_string()).unwrap_or_default(),
        status,
        side,
        kind: r.order_type.or(r.kind).unwrap_or_default(),
        qty: r.orig_qty.0.or(r.quantity.0).unwrap_or_default(),
        filled: r.executed_qty.0.unwrap_or_default(),
        limit: positive(r.price),
        stop: positive(r.stop_price).or(positive(r.trigger_price)),
        trail_start: positive(r.activate_price),
        trail_rate: positive(r.callback_rate),
        tif: r.time_in_force,
        good_till: r.good_till_date.filter(|t| *t > 0),
        placed_ms: r.time.or(r.create_time).or(r.transact_time),
        reduce_only: r.reduce_only.unwrap_or(false),
        close_position: r.close_position.unwrap_or(false),
        position_side: r.position_side,
        trigger: match r.working_type.as_deref() {
            Some("MARK_PRICE") => Some(TriggerKind::Mark),
            Some("CONTRACT_PRICE") => Some(TriggerKind::Last),
            _ => None,
        },
        list: r.order_list_id.filter(|id| *id >= 0),
    })
}

pub fn decode_order(body: &[u8]) -> Result<VenueOrder, ConnectorError> {
    venue_order(serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance order: {e}")))?)
}

pub fn decode_orders(body: &[u8]) -> Result<Vec<VenueOrder>, ConnectorError> {
    let rows: Vec<Row> = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance orders: {e}")))?;
    rows.into_iter().map(venue_order).collect()
}

/// The order a spot cancel-replace left working.
pub fn decode_replaced(body: &[u8]) -> Result<VenueOrder, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Answer {
        new_order_response: Row,
    }
    let a: Answer = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance cancelReplace: {e}")))?;
    venue_order(a.new_order_response)
}

/// USDⓈ-M: hedge mode on or off.
pub fn decode_dual(body: &[u8]) -> Result<bool, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Dual {
        dual_side_position: bool,
    }
    let d: Dual = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance position mode: {e}")))?;
    Ok(d.dual_side_position)
}

/// USDⓈ-M: the signed position on a symbol (one-way mode: one row).
pub fn decode_position(body: &[u8]) -> Result<Decimal, ConnectorError> {
    Ok(decode_positions(body)?.iter().map(|(_, amt)| *amt).sum())
}

/// USDⓈ-M: each position on a symbol with its side (`BOTH` one-way; `LONG` and `SHORT` in
/// hedge mode), signed.
pub fn decode_positions(body: &[u8]) -> Result<Vec<(String, Decimal)>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct P {
        position_amt: OptDec,
        #[serde(default)]
        position_side: Option<String>,
    }
    let rows: Vec<P> = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance positionRisk: {e}")))?;
    Ok(rows.into_iter().filter_map(|p| Some((p.position_side.unwrap_or_else(|| "BOTH".into()), p.position_amt.0?))).collect())
}

/// USDⓈ-M account figures, as Binance states them (USD terms).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FuturesFigures {
    pub wallet: Option<Decimal>,
    pub equity: Option<Decimal>,
    pub unrealized: Option<Decimal>,
    pub initial_margin: Option<Decimal>,
    pub maintenance_margin: Option<Decimal>,
    pub available: Option<Decimal>,
}

pub fn decode_futures_account(body: &[u8]) -> Result<FuturesFigures, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        #[serde(default)]
        total_wallet_balance: OptDec,
        #[serde(default)]
        total_margin_balance: OptDec,
        #[serde(default)]
        total_unrealized_profit: OptDec,
        #[serde(default)]
        total_initial_margin: OptDec,
        #[serde(default)]
        total_maint_margin: OptDec,
        #[serde(default)]
        available_balance: OptDec,
    }
    let a: A = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance futures account: {e}")))?;
    Ok(FuturesFigures {
        wallet: a.total_wallet_balance.0,
        equity: a.total_margin_balance.0,
        unrealized: a.total_unrealized_profit.0,
        initial_margin: a.total_initial_margin.0,
        maintenance_margin: a.total_maint_margin.0,
        available: a.available_balance.0,
    })
}

/// USDⓈ-M: each symbol's leverage and margin mode as the account set them (`symbolConfig`,
/// every symbol in one call). Read only: OTW changes neither.
pub fn symbol_config(corr: CorrId) -> HttpRequest {
    call("symbolConfig", Method::Get, "/fapi/v1/symbolConfig", corr, 5)
}

/// A symbol's leverage and margin mode (`cross` or `isolated`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolConfig {
    pub leverage: Option<Decimal>,
    pub margin_mode: Option<String>,
}

pub fn decode_symbol_config(body: &[u8]) -> Result<HashMap<String, SymbolConfig>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct C {
        symbol: String,
        #[serde(default)]
        margin_type: Option<String>,
        #[serde(default)]
        leverage: OptDec,
    }
    let rows: Vec<C> = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance futures symbolConfig: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|c| {
            let margin_mode = match c.margin_type.as_deref().map(str::to_ascii_uppercase).as_deref() {
                Some("CROSSED" | "CROSS") => Some("cross".to_string()),
                Some("ISOLATED") => Some("isolated".to_string()),
                _ => None,
            };
            (c.symbol, SymbolConfig { leverage: c.leverage.0.filter(|l| *l > Decimal::ZERO), margin_mode })
        })
        .collect())
}

/// Spot: the account's balances, zero ones left out.
pub fn spot_account(corr: CorrId) -> HttpRequest {
    call("account", Method::Get, "/api/v3/account", corr, 20).query("omitZeroBalances", "true")
}

/// Spot: one asset's balance, free and locked (zero when the account holds none).
pub fn decode_spot_balance(body: &[u8], asset: &str) -> Result<(Decimal, Decimal), ConnectorError> {
    Ok(decode_spot_balances(body)?.into_iter().find(|(a, _, _)| a == asset).map(|(_, free, locked)| (free, locked)).unwrap_or_default())
}

/// Spot: every balance the account answers, `(asset, free, locked)`.
pub fn decode_spot_balances(body: &[u8]) -> Result<Vec<(String, Decimal, Decimal)>, ConnectorError> {
    #[derive(Deserialize)]
    struct B {
        asset: String,
        free: OptDec,
        locked: OptDec,
    }
    #[derive(Deserialize)]
    struct A {
        balances: Vec<B>,
    }
    let a: A = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance account: {e}")))?;
    Ok(a.balances.into_iter().map(|b| (b.asset, b.free.0.unwrap_or_default(), b.locked.0.unwrap_or_default())).collect())
}

/// Live only (`/sapi`): what each wallet is worth in `quote`, as Binance values it.
pub fn wallet_balance(quote: &str, corr: CorrId) -> HttpRequest {
    HttpRequest { base: super::SAPI, ..call("walletBalance", Method::Get, "/sapi/v1/asset/wallet/balance", corr, 60) }.query("quoteAsset", quote)
}

/// The spot wallet's value in a [`wallet_balance`] answer.
pub fn decode_spot_wallet(body: &[u8]) -> Result<Option<Decimal>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct W {
        wallet_name: String,
        balance: OptDec,
    }
    let rows: Vec<W> = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance wallet balance: {e}")))?;
    Ok(rows.into_iter().find(|w| w.wallet_name == "Spot").and_then(|w| w.balance.0))
}

// ── Live gates (spec 9, 12.2) ──

/// Live only (`/sapi`): what the key is allowed to do.
pub fn api_restrictions(corr: CorrId) -> HttpRequest {
    HttpRequest { base: super::SAPI, ..call("apiRestrictions", Method::Get, "/sapi/v1/account/apiRestrictions", corr, 1) }
}

/// A key's permissions, as `apiRestrictions` states them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRights {
    #[serde(default)]
    pub ip_restrict: bool,
    #[serde(default)]
    pub enable_withdrawals: bool,
    #[serde(default)]
    pub enable_spot_and_margin_trading: bool,
    #[serde(default)]
    pub enable_futures: bool,
}

pub fn decode_rights(body: &[u8]) -> Result<KeyRights, ConnectorError> {
    serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance apiRestrictions: {e}")))
}

/// What a key needs before OTW sends it a live order: trading on for the market, withdrawals
/// off, and access restricted to trusted IPs. A refusal names the setting to change on
/// Binance's API management page.
pub fn check_rights(m: &Market, k: &KeyRights) -> Result<(), ConnectorError> {
    let fix = match m.kind {
        Kind::Spot if !k.enable_spot_and_margin_trading => Some("trading is off on it: tick \"Enable Spot & Margin Trading\""),
        Kind::UsdM if !k.enable_futures => Some("futures trading is off on it: tick \"Enable Futures\""),
        _ if k.enable_withdrawals => Some("it can withdraw: untick \"Enable Withdrawals\", OpenTraderWorld never needs it"),
        _ if !k.ip_restrict => Some("it takes calls from any IP: restrict it to this server's IP (\"Restrict access to trusted IPs only\")"),
        _ => None,
    };
    match fix {
        Some(f) => Err(ConnectorError::new(ErrorKind::Permission, format!("{}: this API key is not fit for live orders, {f} in Binance's API management, then send again", m.name))),
        None => Ok(()),
    }
}

/// The dry run of a placing call: the same order on Binance's test route, which validates it
/// without sending it to the matching engine. `None` where there is none (an order list, a
/// USDⓈ-M conditional order).
pub fn order_test(req: &HttpRequest) -> Option<HttpRequest> {
    let path = match req.path.as_str() {
        "/api/v3/order" => "/api/v3/order/test",
        "/fapi/v1/order" => "/fapi/v1/order/test",
        _ => return None,
    };
    Some(HttpRequest { endpoint_id: "orderTest", path: path.into(), ..req.clone() }.weight(1))
}

/// The account's order buckets (`rateLimit/order`).
pub fn order_limits(m: &Market, corr: CorrId) -> HttpRequest {
    match m.kind {
        Kind::Spot => call("orderLimits", Method::Get, "/api/v3/rateLimit/order", corr, 40),
        Kind::UsdM => call("orderLimits", Method::Get, "/fapi/v1/rateLimit/order", corr, 1),
    }
}

/// One order bucket: at most `limit` new orders per `interval_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderLimit {
    pub interval_ms: i64,
    pub limit: u64,
}

fn interval_ms(num: i64, unit: &str) -> Option<i64> {
    let ms = match unit.to_ascii_uppercase().as_str() {
        "S" | "SECOND" => 1_000,
        "M" | "MINUTE" => 60_000,
        "H" | "HOUR" => 3_600_000,
        "D" | "DAY" => 86_400_000,
        _ => return None,
    };
    Some(num * ms)
}

pub fn decode_order_limits(body: &[u8]) -> Result<Vec<OrderLimit>, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct L {
        rate_limit_type: String,
        interval: String,
        interval_num: i64,
        limit: u64,
    }
    let rows: Vec<L> = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance rateLimit/order: {e}")))?;
    Ok(rows
        .into_iter()
        .filter(|l| l.rate_limit_type == "ORDERS")
        .filter_map(|l| Some(OrderLimit { interval_ms: interval_ms(l.interval_num, &l.interval)?, limit: l.limit }))
        .collect())
}

/// The orders counted so far in each bucket, from an order answer's
/// `X-MBX-ORDER-COUNT-(intervalNum)(intervalLetter)` headers: `(interval, count)`.
pub fn order_counts(frame: &RawFrame) -> Vec<(i64, u64)> {
    let RawFrame::Http { headers, .. } = frame else { return vec![] };
    headers
        .iter()
        .filter_map(|(k, v)| {
            let tag = k.to_ascii_lowercase().strip_prefix("x-mbx-order-count-")?.to_string();
            let split = tag.find(|c: char| !c.is_ascii_digit())?;
            let ms = interval_ms(tag[..split].parse().ok()?, &tag[split..])?;
            Some((ms, v.trim().parse().ok()?))
        })
        .collect()
}

/// Whether `adding` more orders fit the buckets, from the counts last read `age_ms` ago. A
/// count older than its interval may have reset and is not held against the ticket. Refused:
/// `RateLimited`, with the time left in that bucket's interval as when to retry.
pub fn check_budget(m: &Market, limits: &[OrderLimit], counts: &[(i64, u64)], age_ms: i64, adding: u64) -> Result<(), ConnectorError> {
    for l in limits {
        let Some((_, used)) = counts.iter().find(|(ms, _)| *ms == l.interval_ms) else { continue };
        if age_ms < l.interval_ms && used + adding > l.limit {
            let wait = l.interval_ms - age_ms;
            let mut e = ConnectorError::new(
                ErrorKind::RateLimited,
                format!(
                    "{}: the account's order limit is {} per {} s and {used} are counted; this ticket adds {adding}, so it is refused rather than queued: send it again in {} s",
                    m.name,
                    l.limit,
                    l.interval_ms / 1000,
                    (wait + 999) / 1000
                ),
            );
            e.retry_after_ms = u64::try_from(wait).ok();
            return Err(e);
        }
    }
    Ok(())
}

/// USDⓈ-M dead man's switch: cancel every order on `symbol` unless called again within `ms`;
/// 0 stops the countdown.
pub fn countdown(symbol: &str, ms: u64, corr: CorrId) -> HttpRequest {
    call("countdownCancelAll", Method::Post, "/fapi/v1/countdownCancelAll", corr, 10).query("symbol", symbol).query("countdownTime", ms)
}

/// Whether a request places or changes an order: what the order buckets count, and what a
/// rate-limit refusal stops before it is sent.
pub fn places(req: &HttpRequest) -> bool {
    matches!(req.endpoint_id, "order" | "algoOrder" | "orderListOco" | "orderListOto" | "orderListOtoco" | "cancelReplace" | "modify") && req.method != Method::Get && req.method != Method::Delete
}

/// Binance's refusal in a frame, if any. A spot cancel-replace that half failed says which
/// half: the cancel went through and the new order was refused means **the order is gone**.
pub fn refused(frame: &RawFrame, name: &str) -> Option<ConnectorError> {
    let e = super::errors::from_frame(frame, name)?;
    let RawFrame::Http { body, .. } = frame else { return Some(e) };
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Half {
        #[serde(default)]
        msg: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        cancel_result: String,
        new_order_result: String,
        #[serde(default)]
        cancel_response: Option<Half>,
        #[serde(default)]
        new_order_response: Option<Half>,
    }
    #[derive(Deserialize)]
    struct Body {
        data: Data,
    }
    let Ok(b) = serde_json::from_slice::<Body>(body) else { return Some(e) };
    let why = |h: Option<Half>| h.and_then(|h| h.msg).unwrap_or_default();
    let d = b.data;
    let message = match (d.cancel_result.as_str(), d.new_order_result.as_str()) {
        ("SUCCESS", _) => format!("{name}: the order was cancelled but Binance refused its replacement ({}); nothing is working in its place", why(d.new_order_response)),
        (_, "NOT_ATTEMPTED") => format!("{name}: Binance refused the cancel ({}); the order stands unchanged", why(d.cancel_response)),
        _ => format!("{name}: Binance refused the change (cancel: {}; new order: {}); the order stands unchanged", why(d.cancel_response), why(d.new_order_response)),
    };
    Some(ConnectorError { message, ..e })
}

/// Binance does not know this order (`-2011` on a cancel, `-2013` on a query).
pub fn unknown_order(e: &ConnectorError) -> bool {
    matches!(e.provider_code.as_deref(), Some("-2011") | Some("-2013")) || (e.kind == ErrorKind::NotFound && e.message.contains("does not exist"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Every symbol's filters as an order is checked against them: decoded the way the
    /// instrument list stores them (tick and lot on the spec, the rest in its rules).
    fn decode_rules(m: &Market, body: &[u8]) -> Result<HashMap<String, Rules>, ConnectorError> {
        let info: super::super::dto::SymbolFilters = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("exchangeInfo: {e}")))?;
        let mut out = HashMap::new();
        for s in info.symbols {
            let mut spec = InstrumentSpec::spot(otw_connector_model::InstrumentId::new(m.venue, &s.symbol), "B", "Q");
            for f in &s.filters {
                match f.filter_type.as_str() {
                    "PRICE_FILTER" => spec.tick_size = f.tick_size.0.map(Price::new),
                    "LOT_SIZE" => spec.lot_size = f.step_size.0.map(Qty::new),
                    _ => {}
                }
            }
            spec.rules = trading_rules(m, &s.filters);
            out.insert(s.symbol, rules_of(&spec));
        }
        Ok(out)
    }
    use crate::binance::market::{SPOT, USDM};
    use otw_connector_model::InstrumentId;
    use otw_connector_testkit::http_frame;

    fn order(m: &Market, t: OrderType, tif: TimeInForce) -> OrderRequest {
        let (l, s) = t.prices();
        OrderRequest {
            client_order_id: "k1".into(),
            instrument: InstrumentId::new(m.venue, "BTCUSDT"),
            side: Side::Buy,
            order_type: t,
            qty: Qty::new(Decimal::new(20, 4)),
            limit: l.then(|| Price::new(Decimal::new(735000, 1))),
            stop: s.then(|| Price::new(Decimal::new(900000, 1))),
            tif,
            parent: None,
            oca_group: None,
            account: None,
            opts: Default::default(),
        }
    }

    fn q(h: &HttpRequest) -> Vec<(&str, &str)> {
        h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
    }

    #[test]
    fn an_id_names_symbol_client_id_and_book() {
        let r = OrderRef::parse("BTCUSDT:algo:k1").unwrap();
        assert_eq!((r.symbol.as_str(), r.client_id.as_str(), r.algo), ("BTCUSDT", "k1", true));
        assert_eq!(r.id(), "BTCUSDT:algo:k1");
        assert_eq!(OrderRef::parse("ETHUSDT:web_x").unwrap().id(), "ETHUSDT:web_x");
        assert!(OrderRef::parse("76194051130").is_err());
        assert!(check_client_id(&"x".repeat(37)).is_err());
        assert!(check_client_id("a b").is_err());
    }

    #[test]
    fn spot_stops_are_stop_loss_and_carry_their_time_in_force() {
        let h = place(&SPOT, &order(&SPOT, OrderType::StopLimit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap();
        assert_eq!((h.method, h.path.as_str(), h.signed), (Method::Post, "/api/v3/order", true));
        let p = q(&h);
        for kv in [("type", "STOP_LOSS_LIMIT"), ("price", "73500"), ("stopPrice", "90000"), ("timeInForce", "GTC"), ("quantity", "0.002"), ("newClientOrderId", "k1")] {
            assert!(p.contains(&kv), "{kv:?} in {p:?}");
        }
        let m = place(&SPOT, &order(&SPOT, OrderType::Market, TimeInForce::Ioc), false, false, CorrId(1)).unwrap();
        assert!(!q(&m).iter().any(|(k, _)| *k == "timeInForce"), "a market order takes no time in force");
        assert!(place(&SPOT, &order(&SPOT, OrderType::Limit, TimeInForce::Day), false, false, CorrId(1)).is_err());
        assert!(place(&SPOT, &order(&SPOT, OrderType::Market, TimeInForce::Gtc), true, false, CorrId(1)).is_err());
    }

    #[test]
    fn futures_stops_go_to_the_algo_book() {
        let o = order(&USDM, OrderType::Stop, TimeInForce::Gtc);
        let h = place(&USDM, &o, false, false, CorrId(1)).unwrap();
        assert_eq!(h.path, "/fapi/v1/algoOrder");
        let p = q(&h);
        for kv in [("type", "STOP_MARKET"), ("algoType", "CONDITIONAL"), ("triggerPrice", "90000"), ("clientAlgoId", "k1")] {
            assert!(p.contains(&kv), "{kv:?} in {p:?}");
        }
        assert_eq!(placed_ref(&USDM, &o).id(), "BTCUSDT:algo:k1");
        let until = Timestamp::from_millis(1_800_000_000_000).unwrap();
        let g = place(&USDM, &order(&USDM, OrderType::Limit, TimeInForce::Gtd(until)), true, false, CorrId(1)).unwrap();
        let p = q(&g);
        assert_eq!(g.path, "/fapi/v1/order");
        for kv in [("timeInForce", "GTD"), ("goodTillDate", "1800000000000"), ("reduceOnly", "true")] {
            assert!(p.contains(&kv), "{kv:?} in {p:?}");
        }
        assert_eq!(cancel(&USDM, &placed_ref(&USDM, &o), CorrId(2)).path, "/fapi/v1/algoOrder");
    }

    #[test]
    fn an_order_reads_back_from_either_book() {
        let spot = decode_order(br#"{"symbol":"BTCUSDT","orderId":76194051135,"clientOrderId":"k1","price":"71790.00000000","origQty":"0.00200000","executedQty":"0.00000000","status":"NEW","timeInForce":"GTC","type":"LIMIT","side":"BUY","stopPrice":"0.00000000","time":1792766161934}"#).unwrap();
        assert_eq!((spot.r.id(), spot.kind.as_str(), spot.status.clone()), ("BTCUSDT:k1".to_string(), "LIMIT", Mapped::Known(OrderStatus::New)));
        assert_eq!((spot.limit, spot.stop), (Some(Decimal::new(71790, 0)), None));
        let algo = decode_orders(br#"[{"algoId":1000000253740560,"clientAlgoId":"k2","algoType":"CONDITIONAL","orderType":"STOP","symbol":"BTCUSDT","side":"BUY","positionSide":"BOTH","timeInForce":"GTC","quantity":"0.002","algoStatus":"NEW","actualOrderId":"","triggerPrice":"89823.0","price":"91456.0","createTime":1792766166871}]"#).unwrap();
        assert_eq!((algo[0].r.id(), algo[0].kind.as_str()), ("BTCUSDT:algo:k2".to_string(), "STOP"));
        assert_eq!((algo[0].stop, algo[0].limit, algo[0].qty), (Some(Decimal::new(898230, 1)), Some(Decimal::new(914560, 1)), Decimal::new(2, 3)));
    }

    #[test]
    fn a_spot_change_replaces_what_is_left_under_the_same_id() {
        let cur = decode_order(br#"{"symbol":"BTCUSDT","orderId":1,"clientOrderId":"k1","price":"70000","origQty":"0.003","executedQty":"0.001","status":"PARTIALLY_FILLED","timeInForce":"GTC","type":"LIMIT","side":"BUY"}"#).unwrap();
        let h = amend(&SPOT, &cur, &OrderAmend { limit: Some(Price::new(Decimal::new(69000, 0))), ..Default::default() }, CorrId(1)).unwrap();
        let p = q(&h);
        for kv in [("cancelOrigClientOrderId", "k1"), ("newClientOrderId", "k1"), ("quantity", "0.002"), ("price", "69000"), ("cancelReplaceMode", "STOP_ON_FAILURE")] {
            assert!(p.contains(&kv), "{kv:?} in {p:?}");
        }
        assert!(amend(&SPOT, &cur, &OrderAmend { qty: Some(Qty::new(Decimal::new(1, 3))), ..Default::default() }, CorrId(1)).is_err(), "nothing left to work");
        assert!(amend(&SPOT, &cur, &OrderAmend { stop: Some(Price::new(Decimal::ONE)), ..Default::default() }, CorrId(1)).is_err());
    }

    #[test]
    fn a_half_failed_replace_says_the_order_is_gone() {
        let f = http_frame(1, "cancelReplace", 409, r#"{"code":-2021,"msg":"Order cancel-replace partially failed.","data":{"cancelResult":"SUCCESS","newOrderResult":"FAILURE","cancelResponse":{"symbol":"BTCUSDT"},"newOrderResponse":{"code":-2010,"msg":"Account has insufficient balance for requested action."}}}"#);
        let e = refused(&f, "binance").unwrap();
        assert!(e.message.contains("nothing is working in its place") && e.message.contains("insufficient balance"), "{}", e.message);
        let gone = refused(&http_frame(1, "cancel", 400, r#"{"code":-2011,"msg":"Unknown order sent."}"#), "binance").unwrap();
        assert!(unknown_order(&gone));
    }

    #[test]
    fn price_and_size_are_checked_against_tick_and_lot() {
        let mut spec = InstrumentSpec::spot(InstrumentId::new("BINANCE_USDM", "BTCUSDT"), "BTC", "USDT");
        spec.tick_size = Some(Price::new(Decimal::new(1, 1)));
        spec.lot_size = Some(Qty::new(Decimal::new(1, 4)));
        spec.min_qty = Some(Qty::new(Decimal::new(1, 4)));
        let mut o = order(&USDM, OrderType::Limit, TimeInForce::Gtc);
        assert!(check_rules(&spec, &o).is_ok());
        o.limit = Some(Price::new(Decimal::new(7350005, 2)));
        assert!(check_rules(&spec, &o).is_err());
        o.limit = Some(Price::new(Decimal::new(735000, 1)));
        o.qty = Qty::new(Decimal::new(15, 5));
        assert!(check_rules(&spec, &o).is_err());
        o.qty = Qty::new(Decimal::new(5, 5));
        assert!(check_rules(&spec, &o).is_err());
        // A close-position stop reads back with no size: only its prices are checked.
        o.qty = Qty::new(Decimal::ZERO);
        o.opts.close_position = true;
        assert!(check_rules(&spec, &o).is_ok());
        o.limit = Some(Price::new(Decimal::new(7350005, 2)));
        assert!(check_rules(&spec, &o).is_err());
    }

    fn with(mut o: OrderRequest, f: impl FnOnce(&mut OrderRequest)) -> OrderRequest {
        f(&mut o);
        o
    }

    fn has(h: &HttpRequest, kvs: &[(&str, &str)]) {
        let p = q(h);
        for kv in kvs {
            assert!(p.contains(kv), "{kv:?} in {p:?}");
        }
    }

    #[test]
    fn spot_takes_post_only_iceberg_trailing_and_if_touched_orders() {
        let maker = with(order(&SPOT, OrderType::Limit, TimeInForce::Gtc), |o| o.opts.post_only = true);
        let h = place(&SPOT, &maker, false, false, CorrId(1)).unwrap();
        has(&h, &[("type", "LIMIT_MAKER"), ("price", "73500")]);
        assert!(!q(&h).iter().any(|(k, _)| *k == "timeInForce"), "a limit maker takes no time in force");
        let ice = with(order(&SPOT, OrderType::Limit, TimeInForce::Gtc), |o| o.opts.display_qty = Some(Qty::new(Decimal::new(5, 4))));
        has(&place(&SPOT, &ice, false, false, CorrId(1)).unwrap(), &[("icebergQty", "0.0005"), ("timeInForce", "GTC")]);
        let ice_ioc = with(ice.clone(), |o| o.tif = TimeInForce::Ioc);
        assert!(place(&SPOT, &ice_ioc, false, false, CorrId(1)).is_err());
        let trail = with(order(&SPOT, OrderType::TrailingStop, TimeInForce::Gtc), |o| o.opts.trail = Some(Trail::Percent(Decimal::new(125, 2))));
        let h = place(&SPOT, &trail, false, false, CorrId(1)).unwrap();
        has(&h, &[("type", "STOP_LOSS"), ("trailingDelta", "125")]);
        assert!(!q(&h).iter().any(|(k, _)| *k == "stopPrice"), "no start price, trails at once");
        let fine = with(trail.clone(), |o| o.opts.trail = Some(Trail::Percent(Decimal::new(1255, 3))));
        assert!(place(&SPOT, &fine, false, false, CorrId(1)).is_err(), "whole basis points only");
        has(&place(&SPOT, &order(&SPOT, OrderType::Mit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap(), &[("type", "TAKE_PROFIT"), ("stopPrice", "90000")]);
        has(
            &place(&SPOT, &order(&SPOT, OrderType::Lit, TimeInForce::Fok), false, false, CorrId(1)).unwrap(),
            &[("type", "TAKE_PROFIT_LIMIT"), ("price", "73500"), ("stopPrice", "90000"), ("timeInForce", "FOK")],
        );
    }

    #[test]
    fn spot_links_an_oco_pair_and_an_entry_with_its_exits_as_order_lists() {
        let mk = |id: &str, t: OrderType, side: Side, parent: Option<&str>, group: Option<&str>| {
            with(order(&SPOT, t, TimeInForce::Gtc), |o| {
                o.client_order_id = id.into();
                o.side = side;
                o.parent = parent.map(Into::into);
                o.oca_group = group.map(Into::into);
            })
        };
        // A sell OCO: the limit half above, the stop below.
        let tp = with(mk("a", OrderType::Limit, Side::Sell, None, Some("g")), |o| o.limit = Some(Price::new(Decimal::from(95_000))));
        let sl = mk("b", OrderType::Stop, Side::Sell, None, Some("g"));
        let h = place_list(&SPOT, &[tp.clone(), sl.clone()], CorrId(1)).unwrap().unwrap();
        assert_eq!(h.path, "/api/v3/orderList/oco");
        has(&h, &[("side", "SELL"), ("aboveType", "LIMIT_MAKER"), ("abovePrice", "95000"), ("aboveClientOrderId", "a"), ("belowType", "STOP_LOSS"), ("belowStopPrice", "90000"), ("belowClientOrderId", "b")]);
        // Two of a kind are no pair.
        assert!(place_list(&SPOT, &[tp.clone(), with(tp.clone(), |o| o.client_order_id = "c".into())], CorrId(1)).is_err());
        // A limit entry and one exit: OTO.
        let entry = mk("e", OrderType::Limit, Side::Buy, None, None);
        let x = mk("x", OrderType::Stop, Side::Sell, Some("e"), None);
        let h = place_list(&SPOT, &[entry.clone(), x.clone()], CorrId(1)).unwrap().unwrap();
        assert_eq!(h.path, "/api/v3/orderList/oto");
        has(&h, &[("workingType", "LIMIT"), ("workingPrice", "73500"), ("workingTimeInForce", "GTC"), ("workingClientOrderId", "e"), ("pendingType", "STOP_LOSS"), ("pendingStopPrice", "90000"), ("pendingSide", "SELL")]);
        // And two: OTOCO, its exits an OCO.
        let tp2 = with(mk("t", OrderType::Limit, Side::Sell, Some("e"), Some("g")), |o| o.limit = Some(Price::new(Decimal::from(95_000))));
        let sl2 = with(mk("s", OrderType::Stop, Side::Sell, Some("e"), Some("g")), |o| o.stop = Some(Price::new(Decimal::from(70_000))));
        let h = place_list(&SPOT, &[entry.clone(), tp2, sl2], CorrId(1)).unwrap().unwrap();
        assert_eq!(h.path, "/api/v3/orderList/otoco");
        has(&h, &[("pendingAboveType", "LIMIT_MAKER"), ("pendingAbovePrice", "95000"), ("pendingBelowType", "STOP_LOSS"), ("pendingBelowStopPrice", "70000"), ("pendingQuantity", "0.002")]);
        // A stop entry holds no exits; a lone order is no list; futures make none.
        let stop_entry = mk("e", OrderType::Stop, Side::Buy, None, None);
        assert!(place_list(&SPOT, &[stop_entry, x.clone()], CorrId(1)).is_err());
        assert!(place_list(&SPOT, std::slice::from_ref(&entry), CorrId(1)).unwrap().is_none());
        assert!(place_list(&USDM, &[entry, x], CorrId(1)).unwrap().is_none());
    }

    #[test]
    fn futures_take_trailing_if_touched_post_only_mark_triggers_and_close_position() {
        let trail = with(order(&USDM, OrderType::TrailingStop, TimeInForce::Gtc), |o| {
            o.opts.trail = Some(Trail::Percent(Decimal::new(15, 1)));
            o.stop = Some(Price::new(Decimal::from(95_000)));
        });
        let h = place(&USDM, &trail, false, false, CorrId(1)).unwrap();
        assert_eq!(h.path, "/fapi/v1/algoOrder");
        has(&h, &[("type", "TRAILING_STOP_MARKET"), ("callbackRate", "1.5"), ("activatePrice", "95000"), ("quantity", "0.002")]);
        assert!(place(&USDM, &with(trail.clone(), |o| o.opts.trail = Some(Trail::Amount(Price::new(Decimal::TEN)))), false, false, CorrId(1)).is_err());
        has(&place(&USDM, &order(&USDM, OrderType::Mit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap(), &[("type", "TAKE_PROFIT_MARKET"), ("triggerPrice", "90000")]);
        has(&place(&USDM, &order(&USDM, OrderType::Lit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap(), &[("type", "TAKE_PROFIT"), ("price", "73500"), ("timeInForce", "GTC")]);
        let post = with(order(&USDM, OrderType::Limit, TimeInForce::Gtc), |o| o.opts.post_only = true);
        has(&place(&USDM, &post, false, false, CorrId(1)).unwrap(), &[("timeInForce", "GTX")]);
        let mark = with(order(&USDM, OrderType::Stop, TimeInForce::Gtc), |o| o.opts.trigger = Some(TriggerKind::Mark));
        has(&place(&USDM, &mark, false, false, CorrId(1)).unwrap(), &[("workingType", "MARK_PRICE")]);
        let close = with(order(&USDM, OrderType::Stop, TimeInForce::Gtc), |o| {
            o.side = Side::Sell;
            o.opts.close_position = true;
        });
        let h = place(&USDM, &close, false, false, CorrId(1)).unwrap();
        has(&h, &[("closePosition", "true")]);
        assert!(!q(&h).iter().any(|(k, _)| *k == "quantity" || *k == "reduceOnly"), "a close-position stop carries no size");
    }

    #[test]
    fn hedge_mode_names_the_position_side_instead_of_reduce_only() {
        let buy = order(&USDM, OrderType::Market, TimeInForce::Gtc);
        has(&place(&USDM, &buy, false, true, CorrId(1)).unwrap(), &[("positionSide", "LONG")]);
        let sell = with(buy.clone(), |o| o.side = Side::Sell);
        has(&place(&USDM, &sell, false, true, CorrId(1)).unwrap(), &[("positionSide", "SHORT")]);
        // Selling to reduce works the long.
        let h = place(&USDM, &sell, true, true, CorrId(1)).unwrap();
        has(&h, &[("positionSide", "LONG")]);
        assert!(!q(&h).iter().any(|(k, _)| *k == "reduceOnly"), "hedge mode takes no reduce-only flag");
    }

    #[test]
    fn a_futures_stop_replacement_keeps_reduce_only_and_maps_take_profits() {
        let cur = decode_order(br#"{"algoId":1,"clientAlgoId":"k2","algoType":"CONDITIONAL","orderType":"TAKE_PROFIT_MARKET","symbol":"BTCUSDT","side":"SELL","positionSide":"BOTH","timeInForce":"GTC","quantity":"0.002","algoStatus":"NEW","triggerPrice":"99000","reduceOnly":true}"#).unwrap();
        assert!(cur.reduce_only);
        let r = replacement(&USDM, &cur, &OrderAmend { stop: Some(Price::new(Decimal::from(98_000))), ..Default::default() }, "k2.r1").unwrap();
        assert_eq!((r.order_type, r.opts.reduce_only, r.stop), (OrderType::Mit, true, Some(Price::new(Decimal::from(98_000)))));
        assert_eq!(r.client_order_id, "k2.r1");
        let trail = decode_order(br#"{"algoId":2,"clientAlgoId":"k3","orderType":"TRAILING_STOP_MARKET","symbol":"BTCUSDT","side":"SELL","quantity":"0.002","algoStatus":"NEW"}"#).unwrap();
        assert!(replacement(&USDM, &trail, &OrderAmend::default(), "k3.r1").is_err());
    }

    #[test]
    fn a_trailing_stop_reads_where_it_trails_and_where_it_started() {
        let o = decode_order(br#"{"algoId":2,"clientAlgoId":"k3","orderType":"TRAILING_STOP_MARKET","symbol":"BTCUSDT","side":"SELL","quantity":"0.002","algoStatus":"NEW","triggerPrice":"79930.98","activatePrice":"80020","callbackRate":"0.1"}"#).unwrap();
        assert_eq!(o.stop, Some(Decimal::new(7993098, 2)));
        assert_eq!(o.trail_start, Some(Decimal::new(80020, 0)));
        assert_eq!(o.trail_rate, Some(Decimal::new(1, 1)));
    }

    #[test]
    fn a_replaced_stop_takes_the_next_client_id() {
        assert_eq!(next_client_id("k1"), "k1.r1");
        assert_eq!(next_client_id("k1.r1"), "k1.r2");
        assert_eq!(next_client_id("k1.r9"), "k1.r10");
        let long = "x".repeat(36);
        let n = next_client_id(&long);
        assert!(n.len() <= 36 && n.ends_with(".r1") && check_client_id(&n).is_ok(), "{n}");
        assert_eq!(next_client_id(&n), format!("{}.r2", &n[..33]));
    }

    #[test]
    fn a_hedge_mode_stop_replacement_keeps_working_its_position() {
        // A sell stop on the long, mark-price triggered: the venue says reduceOnly false in hedge mode.
        let cur = decode_order(br#"{"algoId":1,"clientAlgoId":"k2","orderType":"STOP_MARKET","symbol":"BTCUSDT","side":"SELL","positionSide":"LONG","workingType":"MARK_PRICE","timeInForce":"GTC","quantity":"0.002","algoStatus":"NEW","triggerPrice":"60000","reduceOnly":false}"#).unwrap();
        assert!(reduces(&cur));
        let r = replacement(&USDM, &cur, &OrderAmend { stop: Some(Price::new(Decimal::from(61_000))), ..Default::default() }, "k2.r1").unwrap();
        assert_eq!(r.opts.trigger, Some(TriggerKind::Mark));
        let h = place(&USDM, &r, false, true, CorrId(1)).unwrap();
        has(&h, &[("positionSide", "LONG"), ("workingType", "MARK_PRICE"), ("clientAlgoId", "k2.r1"), ("triggerPrice", "61000")]);
        // Opening one: a sell on the short side does not reduce.
        let open = decode_order(br#"{"algoId":2,"clientAlgoId":"k4","orderType":"STOP_MARKET","symbol":"BTCUSDT","side":"SELL","positionSide":"SHORT","quantity":"0.002","algoStatus":"NEW","triggerPrice":"60000"}"#).unwrap();
        assert!(!reduces(&open));
    }

    #[test]
    fn a_close_position_exit_sends_no_reduce_only() {
        let close = with(order(&USDM, OrderType::Stop, TimeInForce::Gtc), |o| {
            o.side = Side::Sell;
            o.opts.close_position = true;
        });
        let h = place(&USDM, &close, true, false, CorrId(1)).unwrap();
        has(&h, &[("closePosition", "true")]);
        assert!(!q(&h).iter().any(|(k, _)| *k == "reduceOnly"), "Binance refuses reduceOnly with closePosition");
    }

    const SPOT_INFO: &[u8] = br#"{"timezone":"UTC","symbols":[{"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","filters":[
        {"filterType":"PRICE_FILTER","minPrice":"0.01000000","maxPrice":"1000000.00000000","tickSize":"0.01000000"},
        {"filterType":"LOT_SIZE","minQty":"0.00001000","maxQty":"9000.00000000","stepSize":"0.00001000"},
        {"filterType":"ICEBERG_PARTS","limit":10},
        {"filterType":"MARKET_LOT_SIZE","minQty":"0.00000000","maxQty":"92.50000000","stepSize":"0.00000000"},
        {"filterType":"TRAILING_DELTA","minTrailingAboveDelta":10,"maxTrailingAboveDelta":2000,"minTrailingBelowDelta":10,"maxTrailingBelowDelta":2000},
        {"filterType":"PERCENT_PRICE_BY_SIDE","bidMultiplierUp":"5","bidMultiplierDown":"0.2","askMultiplierUp":"5","askMultiplierDown":"0.2","avgPriceMins":5},
        {"filterType":"NOTIONAL","minNotional":"5.00000000","applyMinToMarket":true,"maxNotional":"9000000.00000000","applyMaxToMarket":false,"avgPriceMins":5},
        {"filterType":"MAX_NUM_ORDERS","maxNumOrders":200},
        {"filterType":"MAX_NUM_ALGO_ORDERS","maxNumAlgoOrders":5}]}]}"#;

    const USDM_INFO: &[u8] = br#"{"symbols":[{"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","contractType":"PERPETUAL","filters":[
        {"filterType":"PRICE_FILTER","minPrice":"556.80","maxPrice":"4529764","tickSize":"0.10"},
        {"filterType":"LOT_SIZE","stepSize":"0.001","maxQty":"1000","minQty":"0.001"},
        {"filterType":"MARKET_LOT_SIZE","stepSize":"0.001","maxQty":"120","minQty":"0.001"},
        {"filterType":"MAX_NUM_ORDERS","limit":200},
        {"filterType":"MAX_NUM_ALGO_ORDERS","limit":10},
        {"filterType":"MIN_NOTIONAL","notional":"100"},
        {"filterType":"PERCENT_PRICE","multiplierUp":"1.0500","multiplierDown":"0.9500","multiplierDecimal":"4"}]}]}"#;

    #[test]
    fn filters_load_from_exchange_info_on_either_market() {
        let spot = decode_rules(&SPOT, SPOT_INFO).unwrap().remove("BTCUSDT").unwrap();
        assert_eq!((spot.tick, spot.min_price, spot.max_qty), (Some(Decimal::new(1, 2)), Some(Decimal::new(1, 2)), Some(Decimal::from(9000))));
        // A 0 bound is no bound.
        assert_eq!((spot.market_min_qty, spot.market_max_qty, spot.market_step), (None, Some(Decimal::new(925, 1)), None));
        let n = spot.min_notional.clone().unwrap();
        assert_eq!((n.value, n.market, n.avg_mins, n.filter.as_str()), (Decimal::from(5), true, Some(5), "NOTIONAL"));
        assert!(!spot.max_notional.clone().unwrap().market);
        let b = spot.band.clone().unwrap();
        assert_eq!((b.buy, b.sell, b.avg_mins, b.filter.as_str()), ((Some(Decimal::new(2, 1)), Some(Decimal::from(5))), (Some(Decimal::new(2, 1)), Some(Decimal::from(5))), Some(5), "PERCENT_PRICE_BY_SIDE"));
        assert_eq!((spot.max_orders, spot.max_algo_orders), (Some(200), Some(5)));
        let fut = decode_rules(&USDM, USDM_INFO).unwrap().remove("BTCUSDT").unwrap();
        assert_eq!((fut.max_orders, fut.max_algo_orders, fut.market_max_qty), (Some(200), Some(10), Some(Decimal::from(120))));
        let n = fut.min_notional.clone().unwrap();
        assert_eq!((n.value, n.market, n.filter.as_str()), (Decimal::from(100), true, "MIN_NOTIONAL"));
        let b = fut.band.clone().unwrap();
        assert_eq!((b.buy, b.sell), ((None, Some(Decimal::new(105, 2))), (Some(Decimal::new(95, 2)), None)));
    }

    fn spot_order(t: OrderType, side: Side, qty: &str, limit: Option<&str>) -> OrderRequest {
        with(order(&SPOT, t, TimeInForce::Gtc), |o| {
            o.side = side;
            o.qty = Qty::new(qty.parse().unwrap());
            o.limit = limit.map(|p| Price::new(p.parse().unwrap()));
            o.stop = None;
        })
    }

    #[test]
    fn spot_notional_band_market_lot_and_range_are_refused_before_sending() {
        let r = decode_rules(&SPOT, SPOT_INFO).unwrap().remove("BTCUSDT").unwrap();
        let at = Some(Reference::Venue(Decimal::from(70_000)));
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Limit, Side::Buy, "0.001", Some("69000")), false, at).is_ok());
        // Worth 3.45: under NOTIONAL's 5, the smallest size at that price named.
        let e = check_filters(&SPOT, &r, &spot_order(OrderType::Limit, Side::Buy, "0.00005", Some("69000")), false, at).unwrap_err();
        assert!(e.message.contains("NOTIONAL") && e.message.contains("at least 5") && e.message.contains("0.00008 is the smallest size"), "{}", e.message);
        // A market order is valued at the reference price (applyMinToMarket).
        let e = check_filters(&SPOT, &r, &spot_order(OrderType::Market, Side::Buy, "0.00005", None), false, at).unwrap_err();
        assert!(e.message.contains("the reference price"), "{}", e.message);
        // An average over other minutes than the filter's stands for nothing: the venue decides.
        let other = Some(Reference::Average { mins: 1, price: Decimal::from(70_000) });
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Market, Side::Buy, "0.00005", None), false, other).is_ok());
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Market, Side::Buy, "0.00005", None), false, None).is_ok(), "no reference at hand");
        let five = Some(Reference::Average { mins: 5, price: Decimal::from(70_000) });
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Market, Side::Buy, "0.00005", None), false, five).is_err());
        // Band: 0.2x to 5x the reference, on the tick grid.
        let e = check_filters(&SPOT, &r, &spot_order(OrderType::Limit, Side::Sell, "0.001", Some("13000")), false, at).unwrap_err();
        assert!(e.message.contains("PERCENT_PRICE_BY_SIDE") && e.message.contains("between 14000 and 350000") && e.message.contains("reference price 70000"), "{}", e.message);
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Limit, Side::Sell, "0.001", Some("13000")), false, None).is_ok(), "no band without a reference");
        // Market lot, maximum size, price range.
        let e = check_filters(&SPOT, &r, &spot_order(OrderType::Market, Side::Sell, "93", None), false, at).unwrap_err();
        assert!(e.message.contains("MARKET_LOT_SIZE") && e.message.contains("at most 92.5"), "{}", e.message);
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Limit, Side::Sell, "9001", Some("70000")), false, at).unwrap_err().message.contains("at most 9000"));
        assert!(check_filters(&SPOT, &r, &spot_order(OrderType::Limit, Side::Buy, "1", Some("2000000")), false, None).unwrap_err().message.contains("up to 1000000 (PRICE_FILTER)"));
        // A stop with no limit price is not valued here.
        let stop = with(spot_order(OrderType::Stop, Side::Sell, "0.00001", None), |o| o.stop = Some(Price::new(Decimal::from(60_000))));
        assert!(check_filters(&SPOT, &r, &stop, false, at).is_ok());
    }

    #[test]
    fn futures_value_at_the_mark_and_let_reduce_only_under_the_minimum() {
        let r = decode_rules(&USDM, USDM_INFO).unwrap().remove("BTCUSDT").unwrap();
        let mark = Some(Reference::Venue(Decimal::from(100_000)));
        let o = |side: Side, t: OrderType, qty: &str, limit: Option<&str>| {
            with(order(&USDM, t, TimeInForce::Gtc), |o| {
                o.side = side;
                o.qty = Qty::new(qty.parse().unwrap());
                o.limit = limit.map(|p| Price::new(p.parse().unwrap()));
            })
        };
        let low = Some(Reference::Venue(Decimal::from(50_000)));
        let e = check_filters(&USDM, &r, &o(Side::Buy, OrderType::Market, "0.001", None), false, low).unwrap_err();
        assert!(e.message.contains("MIN_NOTIONAL") && e.message.contains("the mark price") && e.message.contains("0.002 is the smallest"), "{}", e.message);
        assert!(check_filters(&USDM, &r, &o(Side::Sell, OrderType::Market, "0.001", None), true, low).is_ok(), "reduce-only goes under the minimum");
        // Buys capped above the mark, sells floored below it; nothing else.
        let e = check_filters(&USDM, &r, &o(Side::Buy, OrderType::Limit, "0.01", Some("106000")), false, mark).unwrap_err();
        assert!(e.message.contains("up to 105000") && e.message.contains("mark price 100000"), "{}", e.message);
        assert!(check_filters(&USDM, &r, &o(Side::Buy, OrderType::Limit, "0.01", Some("90000")), false, mark).is_ok());
        assert!(check_filters(&USDM, &r, &o(Side::Sell, OrderType::Limit, "0.01", Some("94000")), false, mark).unwrap_err().message.contains("from 95000"));
        assert!(check_filters(&USDM, &r, &o(Side::Buy, OrderType::Market, "121", None), false, mark).unwrap_err().message.contains("MARKET_LOT_SIZE"));
        // A close-position stop carries no size.
        let close = with(o(Side::Sell, OrderType::Stop, "0", None), |o| o.opts.close_position = true);
        assert!(check_filters(&USDM, &r, &close, false, mark).is_ok());
        assert!(needs_reference(&USDM, &r, &o(Side::Buy, OrderType::Limit, "1", Some("1"))) && !needs_reference(&USDM, &r, &o(Side::Buy, OrderType::Stop, "1", None)));
    }

    #[test]
    fn open_order_caps_count_what_the_ticket_adds() {
        let r = decode_rules(&SPOT, SPOT_INFO).unwrap().remove("BTCUSDT").unwrap();
        assert!(check_open(&r, "BTCUSDT", 199, 0, 1, 0).is_ok());
        let e = check_open(&r, "BTCUSDT", 199, 0, 2, 0).unwrap_err();
        assert!(e.message.contains("MAX_NUM_ORDERS") && e.message.contains("cancel 1 first"), "{}", e.message);
        let e = check_open(&r, "BTCUSDT", 10, 5, 1, 1).unwrap_err();
        assert!(e.message.contains("MAX_NUM_ALGO_ORDERS"), "{}", e.message);
        assert!(check_open(&r, "BTCUSDT", 10, 5, 1, 0).is_ok(), "a limit adds no conditional order");
        // A book already past a cap (orders placed elsewhere) refuses only what adds to it.
        assert!(check_open(&r, "BTCUSDT", 10, 9, 1, 0).is_ok(), "a full conditional book does not refuse a plain limit");
        let algo = decode_orders(br#"[{"symbol":"BTCUSDT","orderId":1,"clientOrderId":"a","side":"SELL","type":"STOP_LOSS_LIMIT","origQty":"1","status":"NEW"},{"symbol":"BTCUSDT","orderId":2,"clientOrderId":"b","side":"SELL","type":"LIMIT","origQty":"1","status":"NEW"}]"#).unwrap();
        assert_eq!(algo.iter().filter(|o| is_algo(o)).count(), 1);
    }

    #[test]
    fn the_reference_is_the_venues_else_the_average() {
        assert_eq!(decode_reference(&SPOT, br#"{"symbol":"BTCUSDT","referencePrice":"70000.10","timestamp":1}"#).unwrap(), Some(Reference::Venue(Decimal::new(7_000_010, 2))));
        assert_eq!(decode_reference(&SPOT, br#"{"symbol":"BTCUSDT","referencePrice":null,"timestamp":1}"#).unwrap(), None);
        assert_eq!(decode_reference(&USDM, br#"{"symbol":"BTCUSDT","markPrice":"11793.63104562","indexPrice":"11781.80495970"}"#).unwrap(), Some(Reference::Venue(Decimal::new(1_179_363_104_562, 8))));
        assert_eq!(decode_avg_price(br#"{"mins":5,"price":"9.35751834","closeTime":1694061154503}"#).unwrap(), Reference::Average { mins: 5, price: Decimal::new(935_751_834, 8) });
    }

    #[test]
    fn a_live_key_must_trade_not_withdraw_and_be_ip_restricted() {
        let k = decode_rights(br#"{"ipRestrict":true,"createTime":1623840271000,"enableReading":true,"enableWithdrawals":false,"enableInternalTransfer":true,"enableMargin":false,"enableFutures":true,"permitsUniversalTransfer":true,"enableVanillaOptions":false,"enableFixApiTrade":false,"enableFixReadOnly":true,"enableSpotAndMarginTrading":true,"enablePortfolioMarginTrading":true}"#).unwrap();
        assert!(check_rights(&SPOT, &k).is_ok() && check_rights(&USDM, &k).is_ok());
        let no_trade = KeyRights { enable_spot_and_margin_trading: false, ..k.clone() };
        assert!(check_rights(&SPOT, &no_trade).unwrap_err().message.contains("Enable Spot & Margin Trading"));
        assert!(check_rights(&USDM, &no_trade).is_ok(), "futures trade under their own flag");
        assert!(check_rights(&USDM, &KeyRights { enable_futures: false, ..k.clone() }).unwrap_err().message.contains("Enable Futures"));
        let e = check_rights(&SPOT, &KeyRights { enable_withdrawals: true, ..k.clone() }).unwrap_err();
        assert!(e.kind == ErrorKind::Permission && e.message.contains("Enable Withdrawals"), "{}", e.message);
        assert!(check_rights(&SPOT, &KeyRights { ip_restrict: false, ..k.clone() }).unwrap_err().message.contains("trusted IPs"));
        assert_eq!(api_restrictions(CorrId(1)).base, "sapi");
    }

    #[test]
    fn the_dry_run_is_the_same_order_on_the_test_route() {
        let h = place(&SPOT, &order(&SPOT, OrderType::Limit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap();
        let t = order_test(&h).unwrap();
        assert_eq!((t.path.as_str(), t.method, t.query.clone(), t.signed), ("/api/v3/order/test", Method::Post, h.query.clone(), true));
        let f = place(&USDM, &order(&USDM, OrderType::Limit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap();
        assert_eq!(order_test(&f).unwrap().path, "/fapi/v1/order/test");
        assert!(order_test(&place(&USDM, &order(&USDM, OrderType::Stop, TimeInForce::Gtc), false, false, CorrId(1)).unwrap()).is_none(), "no test route for the algo book");
    }

    fn with_headers(headers: &[(&str, &str)]) -> RawFrame {
        RawFrame::Http {
            corr_id: CorrId(1),
            endpoint_id: "order",
            status: 200,
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: bytes::Bytes::from_static(b"{}"),
        }
    }

    #[test]
    fn order_buckets_are_read_from_the_headers_and_refuse_rather_than_queue() {
        let limits = decode_order_limits(br#"[{"rateLimitType":"ORDERS","interval":"SECOND","intervalNum":10,"limit":50,"count":0},{"rateLimitType":"ORDERS","interval":"DAY","intervalNum":1,"limit":160000,"count":0}]"#).unwrap();
        assert_eq!(limits, vec![OrderLimit { interval_ms: 10_000, limit: 50 }, OrderLimit { interval_ms: 86_400_000, limit: 160_000 }]);
        let fut = decode_order_limits(br#"[{"rateLimitType":"ORDERS","interval":"SECOND","intervalNum":10,"limit":300},{"rateLimitType":"ORDERS","interval":"MINUTE","intervalNum":1,"limit":1200}]"#).unwrap();
        assert_eq!(fut[1], OrderLimit { interval_ms: 60_000, limit: 1200 });
        let counts = order_counts(&with_headers(&[("x-mbx-order-count-10s", "49"), ("x-mbx-order-count-1d", "120"), ("x-mbx-used-weight-1m", "7")]));
        assert_eq!(counts, vec![(10_000, 49), (86_400_000, 120)]);
        assert!(check_budget(&SPOT, &limits, &counts, 2_000, 1).is_ok());
        let e = check_budget(&SPOT, &limits, &counts, 2_000, 2).unwrap_err();
        assert!(e.kind == ErrorKind::RateLimited && e.retry_after_ms == Some(8_000) && e.message.contains("again in 8 s"), "{e:?}");
        assert!(check_budget(&SPOT, &limits, &counts, 10_000, 2).is_ok(), "a count older than its interval may have reset");
        assert!(places(&place(&SPOT, &order(&SPOT, OrderType::Limit, TimeInForce::Gtc), false, false, CorrId(1)).unwrap()));
        assert!(!places(&cancel(&SPOT, &OrderRef::parse("BTCUSDT:k1").unwrap(), CorrId(1))), "a cancel is always let through");
    }

    #[test]
    fn the_dead_mans_switch_is_armed_per_symbol() {
        let h = countdown("BTCUSDT", 120_000, CorrId(1));
        assert_eq!((h.method, h.path.as_str(), h.signed), (Method::Post, "/fapi/v1/countdownCancelAll", true));
        has(&h, &[("symbol", "BTCUSDT"), ("countdownTime", "120000")]);
    }

    #[test]
    fn futures_leverage_and_margin_mode_are_read_per_symbol() {
        let c = decode_symbol_config(br#"[{"symbol":"BTCUSDT","marginType":"CROSSED","isAutoAddMargin":false,"leverage":21,"maxNotionalValue":"1000000"},{"symbol":"ETHUSDT","marginType":"ISOLATED","isAutoAddMargin":false,"leverage":5,"maxNotionalValue":"100000"}]"#).unwrap();
        assert_eq!(c["BTCUSDT"], SymbolConfig { leverage: Some(Decimal::from(21)), margin_mode: Some("cross".into()) });
        assert_eq!(c["ETHUSDT"].margin_mode.as_deref(), Some("isolated"));
    }

    #[test]
    fn spot_cash_is_the_usdt_balance_and_equity_the_wallet_value() {
        let body = br#"{"canTrade":true,"accountType":"SPOT","balances":[{"asset":"BTC","free":"1.0","locked":"0"},{"asset":"USDT","free":"900.5","locked":"99.5"}],"permissions":["SPOT"]}"#;
        assert_eq!(decode_spot_balance(body, "USDT").unwrap(), (Decimal::new(9005, 1), Decimal::new(995, 1)));
        assert_eq!(decode_spot_balance(body, "EUR").unwrap(), (Decimal::ZERO, Decimal::ZERO));
        let all = decode_spot_balances(body).unwrap();
        assert_eq!(all.iter().map(|(a, _, _)| a.as_str()).collect::<Vec<_>>(), ["BTC", "USDT"]);
        assert_eq!(all[0], ("BTC".to_string(), Decimal::ONE, Decimal::ZERO));
        let info = br#"{"timezone":"UTC","symbols":[{"symbol":"ETHBTC","status":"TRADING","baseAsset":"ETH","quoteAsset":"BTC","filters":[]}]}"#;
        assert_eq!(decode_symbol_assets(info).unwrap(), Some(("ETH".to_string(), "BTC".to_string())));
        let h = symbol_info("ETHBTC", CorrId(1));
        assert_eq!((h.path.as_str(), h.signed), ("/api/v3/exchangeInfo", false));
        let w =br#"[{"activate":true,"balance":"0","walletName":"Funding"},{"activate":true,"balance":"71000.25","walletName":"Spot"}]"#;
        assert_eq!(decode_spot_wallet(w).unwrap(), Some(Decimal::new(7_100_025, 2)));
        let h = wallet_balance("USDT", CorrId(1));
        assert_eq!((h.base, h.path.as_str()), ("sapi", "/sapi/v1/asset/wallet/balance"));
    }
}
