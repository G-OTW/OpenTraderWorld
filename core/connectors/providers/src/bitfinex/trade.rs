//! Bitfinex (API v2) orders, the exchange (spot cash) book and the perpetual derivatives,
//! written from the documentation alone: no account, no paper key. Every field below is the
//! doc's; each claim is sourced in PROVIDER.md ("Trading"). The order path itself is the shared
//! trader's; this file only says what the calls are and what the answers mean.
//!
//! - Orders go over REST (`POST /v2/auth/w/order/submit`, `update`, `cancel`, `cancel/multi`),
//!   the account socket only pushes: an order sent over the socket carries no request id to
//!   match its answer by.
//! - The book is the order type: `EXCHANGE LIMIT` and the like on a spot pair (the exchange
//!   wallet), the same types unprefixed on a perpetual (`tBTCF0:USTF0`). The margin book (the
//!   unprefixed types on a spot pair) is not traded.
//! - Client id: `cid`, an integer of at most 45 bits "unique in the day (UTC+0)", looked up and
//!   cancelled with its `cid_date`. OTW mints it ([`TradeMapper::mints_client_ids`]): the
//!   milliseconds it was made at, as the doc's own examples do (`cid: Date.now()`), so its date
//!   is read from it.
//! - Derivatives: one net position per instrument; reduce-only is a flag; the leverage an order
//!   carries (`lev`) is the one a working order on the instrument states (`META.$F33`), a
//!   change the order's own; none when no working order states one (the venue then applies its
//!   own default). A position's `LEVERAGE` is a measured ratio, never sent.
//! - Prices take at most 5 significant digits and 8 decimals, amounts 8 decimals: both checked
//!   before anything leaves, never rounded.

#![deny(clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, TimeInForce};
use otw_connector_sdk::parse::{Dec, Int};
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, ClientIdRule, ClientRef, DecodeCtx, ErrorRow, LookupAt, Page, Placing, PositionMode, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper, TradingStatus,
    VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde_json::value::RawValue;
use serde_json::{json, Map, Value};

use super::{currency, Bitfinex, Book, AUTH, REST, WS_AUTH};

/// The largest client id the venue takes: an int45 ("(int45)", Submit Order).
pub(super) const INT45_MAX: i64 = (1 << 45) - 1;
/// Rows per ended-orders read ("max. 2500", Orders History).
const HIST_LIMIT: i64 = 2_500;
/// Rows per fills read: within the doc's "max. 10000" (Trades by symbol).
pub(super) const FILLS_LIMIT: usize = 2_500;
/// How many ids one batch cancel names. The doc states no maximum for `cancel/multi`; 75 is the
/// most operations its Order Multi-OP takes, used by analogy. What a batch leaves is cancelled
/// one by one by the trader.
const CANCEL_BATCH: usize = 75;
/// How near midnight UTC an order's mint time must be for its `cid_date` to be in doubt: the
/// doc does not say whether the date is the caller's or the venue's creation time.
const DAY_EDGE_MS: i64 = 5 * 60_000;
/// The window of creation times an ended order is looked for in, around its mint time.
const HIST_BEFORE_MS: i64 = 5 * 60_000;
const HIST_AFTER_MS: i64 = 60 * 60_000;
const DAY_MS: i64 = 86_400_000;
/// The server's heartbeat comes "every 15 seconds" (ws-general); OTW pings as often.
const PING_EVERY_MS: u64 = 15_000;
/// Flags (https://docs.bitfinex.com/docs/flag-values).
const REDUCE_ONLY: i64 = 1024;
const POST_ONLY: i64 = 4096;
/// The endpoint id of the ended-orders read: `PARTIALLY FILLED` there is a cancelled remainder.
const HIST: &str = "ordersHist";
/// The endpoint id of a positions read for one instrument: `positions:<symbol>` ([`positions_endpoint`]).
const POSITIONS_OF: &str = "positions:";
/// The most instruments a positions read is held for ([`positions_endpoint`]).
const POSITION_IDS_KEPT: usize = 512;
/// How many pages of ended orders a client id lookup reads: [`HIST_LIMIT`] rows each.
const LOOKUP_PAGES: usize = 4;
/// The most orders whose leverage is held ([`order_leverage`]): past it, the list starts over.
const ORDER_LEVS_KEPT: usize = 4096;

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: impl std::fmt::Display) -> ConnectorError {
    ConnectorError::decode(format!("bitfinex {what}: {e}"))
}

/// A signed POST with a JSON body, the way every authenticated call goes.
fn post(endpoint: &'static str, path: impl Into<String>, body: Value, corr: CorrId) -> HttpRequest {
    let mut h = HttpRequest::get(endpoint, AUTH, path, corr).signed();
    h.method = Method::Post;
    h.body = Some(body.to_string().into());
    h
}

/// A venue order id: an integer. Anything else never leaves.
fn order_id(id: &str) -> Result<i64, ConnectorError> {
    id.trim().parse::<i64>().ok().filter(|v| *v > 0).ok_or_else(|| ConnectorError::invalid(format!("{id:?} is not a Bitfinex order id")))
}

/// A minted client id: an integer of at most 45 bits.
fn cid_of(id: &str) -> Result<i64, ConnectorError> {
    id.trim()
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0 && *v <= INT45_MAX)
        .ok_or_else(|| ConnectorError::invalid(format!("Bitfinex takes a client order id that is a whole number below 2^45 ({id:?} is not one)")))
}

/// `YYYY-MM-DD`, UTC, of an epoch millisecond.
pub(super) fn date_of(ms: i64) -> Result<String, ConnectorError> {
    let d = time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).map_err(|e| ConnectorError::invalid(format!("Bitfinex: no date for {ms}: {e}")))?.date();
    Ok(format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()))
}

/// The dates an order made at `ms` may be keyed under: its own, and the day next to it when it
/// was made within [`DAY_EDGE_MS`] of midnight UTC.
pub(super) fn cid_dates(ms: i64) -> Result<Vec<String>, ConnectorError> {
    let mut out = vec![date_of(ms)?];
    let into = ms.rem_euclid(DAY_MS);
    if into < DAY_EDGE_MS {
        out.push(date_of(ms - DAY_MS)?);
    } else if DAY_MS - into <= DAY_EDGE_MS {
        out.push(date_of(ms + DAY_MS)?);
    }
    Ok(out)
}

/// `2020-01-15 10:45:23`, UTC: the time in force of a good-till-date order. The doc's format
/// has whole seconds: a deadline between two is refused with both.
fn tif_of(ms: i64) -> Result<String, ConnectorError> {
    if ms.rem_euclid(1000) != 0 {
        let lo = ms.div_euclid(1000);
        return Err(ConnectorError::invalid(format!("Bitfinex takes an expiry in whole seconds: {} or {} (UTC epoch seconds)", lo, lo + 1)).with_code("venue_rule"));
    }
    let t = time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).map_err(|e| ConnectorError::invalid(format!("Bitfinex: no date for {ms}: {e}")))?;
    Ok(format!("{} {:02}:{:02}:{:02}", date_of(ms)?, t.hour(), t.minute(), t.second()))
}

/// 10 to the `exp`.
fn pow10(exp: i64) -> Decimal {
    if exp >= 0 {
        Decimal::from(10_i64.pow(u32::try_from(exp).unwrap_or(0)))
    } else {
        Decimal::new(1, u32::try_from(-exp).unwrap_or(0))
    }
}

/// The step prices move in at `p`'s magnitude: 5 significant digits, never under 8 decimals
/// ("All pairs on Bitfinex use up to 5 significant digits and up to 8 decimals", introduction).
pub(super) fn price_step(p: Decimal) -> Decimal {
    let n = p.normalize();
    let digits = i64::try_from(n.mantissa().unsigned_abs().to_string().len()).unwrap_or(1);
    let lead = digits - 1 - i64::from(n.scale());
    pow10((lead - 4).max(-8))
}

/// A price the venue would cut ("Prices submit with a precision larger than 5 will be cut by
/// the API") refused before anything leaves, with the two nearest it takes.
pub(super) fn check_price(what: &str, p: Decimal) -> Result<(), ConnectorError> {
    if p <= Decimal::ZERO {
        return Err(ConnectorError::invalid(format!("Bitfinex takes no {what} price of {}", dec_text(p))));
    }
    let step = price_step(p);
    if (p % step).is_zero() {
        return Ok(());
    }
    let (lo, hi) = ((p / step).floor() * step, (p / step).ceil() * step);
    Err(ConnectorError::invalid(format!(
        "Bitfinex takes prices of at most 5 significant digits and 8 decimals: the {what} price {} is not one ({} or {})",
        dec_text(p),
        dec_text(lo),
        dec_text(hi)
    ))
    .with_code("venue_rule"))
}

/// A leverage the venue states as an order can carry it: a whole number from 1 to 100 (`lev`,
/// Submit Order). Any other value (the doc writes a position's leverage as a float) is not one.
fn lev_of(l: Decimal) -> Option<i64> {
    let n = l.normalize();
    (n.fract().is_zero() && n >= Decimal::ONE && n <= Decimal::from(100)).then(|| n.mantissa()).and_then(|m| i64::try_from(m).ok())
}

/// The leverage an order row states: `META.$F33` ("Leverage (int)", Retrieve Orders), when it
/// is one an order can carry (1 to 100).
fn meta_lev(r: &Row) -> Option<i64> {
    let meta: Value = serde_json::from_str(r.get(31)?).ok()?;
    let v = meta.get("$F33")?;
    let lev = v.as_i64().or_else(|| v.as_str().and_then(|t| t.trim().parse::<i64>().ok()))?;
    (1..=100).contains(&lev).then_some(lev)
}

/// The leverage each order read last stated, by venue order id (unique across accounts): a
/// change carries its order's own ("Mandatory for derivative order updates", Update Order). The
/// trader reads the order just before it changes it, so the value is the venue's of the moment.
fn order_levs() -> std::sync::MutexGuard<'static, HashMap<i64, i64>> {
    static LEVS: OnceLock<Mutex<HashMap<i64, i64>>> = OnceLock::new();
    LEVS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn hold_order_lev(id: i64, lev: Option<i64>) {
    let mut held = order_levs();
    match lev {
        Some(l) => {
            if held.len() >= ORDER_LEVS_KEPT && !held.contains_key(&id) {
                held.clear();
            }
            held.insert(id, l);
        }
        None => {
            held.remove(&id);
        }
    }
}

/// The leverage the last read of order `id` stated.
fn order_leverage(id: i64) -> Option<i64> {
    order_levs().get(&id).copied()
}

/// The endpoint id a positions read for `symbol` goes out under, so its answer, which lists
/// every position of the account ("Retrieve Positions" takes no symbol), is read for that
/// instrument only. The decoders know a request by its endpoint id alone; one id per
/// instrument is held for the life of the process, at most [`POSITION_IDS_KEPT`].
fn positions_endpoint(symbol: &str) -> Result<&'static str, ConnectorError> {
    static IDS: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut held = IDS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(id) = held.get(symbol) {
        return Ok(id);
    }
    if held.len() >= POSITION_IDS_KEPT {
        return Err(ConnectorError::invalid(format!("Bitfinex: positions read for more than {POSITION_IDS_KEPT} instruments: {symbol} is not read")));
    }
    let id: &'static str = Box::leak(format!("{POSITIONS_OF}{symbol}").into_boxed_str());
    held.insert(symbol.to_string(), id);
    Ok(id)
}

/// A row of positional fields; new fields may be appended at any time (introduction).
struct Row(Vec<Box<RawValue>>);

impl Row {
    fn get(&self, i: usize) -> Option<&str> {
        self.0.get(i).map(|v| v.get().trim()).filter(|t| *t != "null")
    }

    fn text(&self, i: usize) -> Option<String> {
        serde_json::from_str::<String>(self.get(i)?).ok()
    }

    fn dec(&self, i: usize) -> Option<Decimal> {
        serde_json::from_str::<Dec>(self.get(i)?).ok().map(|d| d.0)
    }

    fn int(&self, i: usize) -> Option<i64> {
        serde_json::from_str::<Int>(self.get(i)?).ok().map(|d| d.0)
    }
}

fn rows(body: &[u8], what: &str) -> Result<Vec<Row>, ConnectorError> {
    let raw: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err(what, e))?;
    Ok(raw.into_iter().map(Row).collect())
}

fn side_of(amount: Decimal) -> Mapped<Side> {
    if amount > Decimal::ZERO {
        Mapped::Known(Side::Buy)
    } else if amount < Decimal::ZERO {
        Mapped::Known(Side::Sell)
    } else {
        Mapped::Unknown("0".into())
    }
}

/// The head of a status: `EXECUTED @ 107.6(-0.2)`, `CANCELED was: PARTIALLY FILLED @ ...`,
/// `RSN_DUST was: ACTIVE (note: POSCLOSE)` are read on their first word(s).
pub(super) fn status_head(word: &str) -> &str {
    let w = word.trim();
    let w = w.split(" @ ").next().unwrap_or(w);
    w.split(" was").next().unwrap_or(w).trim()
}

/// The order's type without its book prefix, and which price is a limit and which a trigger:
/// `PRICE` is the limit of a limit order and the trigger of a stop; a stop limit's limit is
/// `PRICE_AUX_LIMIT` (Submit Order). `None` for a type the doc does not list.
fn price_roles(kind: &str, price: Option<Decimal>, aux: Option<Decimal>) -> Option<(Option<Decimal>, Option<Decimal>)> {
    let positive = |d: Option<Decimal>| d.filter(|v| *v > Decimal::ZERO);
    let (price, aux) = (positive(price), positive(aux));
    Some(match kind.trim().strip_prefix("EXCHANGE ").unwrap_or(kind.trim()) {
        "LIMIT" | "FOK" | "IOC" => (price, None),
        "MARKET" => (None, None),
        "STOP" | "TRAILING STOP" => (None, price),
        "STOP LIMIT" => (aux, price),
        _ => return None,
    })
}

impl Bitfinex {
    fn venue(&self) -> &'static str {
        self.book().label()
    }

    fn on_derivatives(&self) -> bool {
        self.book() == Book::Derivatives
    }

    /// A symbol of this book, or the refusal naming the other one.
    fn own(&self, symbol: &str) -> Result<(), ConnectorError> {
        if self.book().owns(symbol) {
            return Ok(());
        }
        Err(ConnectorError::invalid(match self.book() {
            Book::Spot => format!("{symbol} is a Bitfinex derivatives contract: send it on the derivatives book"),
            Book::Derivatives => format!("{symbol} is a Bitfinex spot pair: derivatives are written <BASE>F0:<QUOTE>F0 (tBTCF0:USTF0)"),
        }))
    }

    /// The venue's type word for `o` on this book ("To place exchange orders, add the EXCHANGE
    /// prefix", Submit Order): IOC and FOK are limit types of their own; a good-till-date order
    /// is a plain type with `tif`. Anything the types cannot say is refused with the fix.
    fn type_word(&self, o: &OrderRequest) -> Result<String, ConnectorError> {
        let base = match (o.order_type, o.tif) {
            // A market order fills at once: as immediate-or-cancel it is the same order.
            (OrderType::Market, TimeInForce::Gtc | TimeInForce::Ioc) => "MARKET",
            (OrderType::Limit, TimeInForce::Gtc | TimeInForce::Gtd(_)) => "LIMIT",
            (OrderType::Limit, TimeInForce::Ioc) => "IOC",
            (OrderType::Limit, TimeInForce::Fok) => "FOK",
            (OrderType::Stop, TimeInForce::Gtc | TimeInForce::Gtd(_)) => "STOP",
            (OrderType::StopLimit, TimeInForce::Gtc | TimeInForce::Gtd(_)) => "STOP LIMIT",
            (OrderType::Market, _) => return Err(ConnectorError::invalid("Bitfinex takes a market order as it is: immediate, with no time in force")),
            (OrderType::Stop | OrderType::StopLimit, _) => {
                return Err(ConnectorError::invalid("Bitfinex takes a stop good till cancelled or till a date only: immediate-or-cancel and fill-or-kill are limit orders there"))
            }
            (t, tif) => return Err(ConnectorError::unsupported(format!("Bitfinex takes no {t:?} order with {tif:?} from OpenTraderWorld"))),
        };
        Ok(match self.book() {
            Book::Spot => format!("EXCHANGE {base}"),
            Book::Derivatives => base.to_string(),
        })
    }

    /// The order body, every field the doc's (Submit Order): `type`, `symbol`, `amount` (signed,
    /// positive to buy), `price`, `price_aux_limit`, `flags`, `tif`, `cid`, `lev`.
    fn order_body(&self, o: &OrderRequest, p: &Placing) -> Result<Value, ConnectorError> {
        let symbol = o.instrument.native.as_str();
        self.own(symbol)?;
        self.check(o)?;
        let qty = o.qty.value();
        if qty <= Decimal::ZERO {
            return Err(ConnectorError::invalid(format!("Bitfinex takes no order of {}", dec_text(qty))));
        }
        let amount = if o.side == Side::Buy { qty } else { -qty };
        let mut b = Map::new();
        b.insert("type".into(), json!(self.type_word(o)?));
        b.insert("symbol".into(), json!(symbol));
        b.insert("amount".into(), json!(dec_text(amount)));
        let (limit, stop) = (o.limit.map(|v| v.value()), o.stop.map(|v| v.value()));
        match o.order_type {
            // The socket's page: "Price (Not required for market orders)". The REST schema lists
            // `price` as required but states no value for a market order; ccxt sends none.
            OrderType::Market => {}
            OrderType::Limit => {
                b.insert("price".into(), json!(dec_text(limit.ok_or_else(|| ConnectorError::invalid("a limit order needs its price"))?)));
            }
            OrderType::Stop => {
                b.insert("price".into(), json!(dec_text(stop.ok_or_else(|| ConnectorError::invalid("a stop needs its trigger price"))?)));
            }
            OrderType::StopLimit => {
                b.insert("price".into(), json!(dec_text(stop.ok_or_else(|| ConnectorError::invalid("a stop limit needs its trigger price"))?)));
                b.insert("price_aux_limit".into(), json!(dec_text(limit.ok_or_else(|| ConnectorError::invalid("a stop limit needs its limit price"))?)));
            }
            other => return Err(ConnectorError::unsupported(format!("Bitfinex takes no {other:?} order from OpenTraderWorld"))),
        }
        let mut flags = 0;
        if o.opts.post_only {
            flags += POST_ONLY;
        }
        if o.opts.reduce_only {
            if !self.on_derivatives() {
                return Err(ConnectorError::unsupported("Bitfinex spot holds balances, not positions: an order there cannot be reduce-only"));
            }
            flags += REDUCE_ONLY;
        }
        if flags != 0 {
            b.insert("flags".into(), json!(flags));
        }
        if let TimeInForce::Gtd(at) = o.tif {
            b.insert("tif".into(), json!(tif_of(at.millis())?));
        }
        if !p.client_id.is_empty() {
            b.insert("cid".into(), json!(cid_of(&p.client_id)?));
        }
        if self.on_derivatives() {
            if let Some(l) = p.leverage.and_then(lev_of) {
                b.insert("lev".into(), json!(l));
            }
        }
        Ok(Value::Object(b))
    }

    /// What the doc's words cannot carry as asked, refused before anything leaves.
    fn check(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        self.type_word(o)?;
        if o.opts.post_only && !(o.order_type == OrderType::Limit && matches!(o.tif, TimeInForce::Gtc | TimeInForce::Gtd(_))) {
            return Err(ConnectorError::invalid("Bitfinex takes post-only on a limit order that rests (good till cancelled or till a date), not on this one"));
        }
        if let Some(p) = o.limit {
            check_price("limit", p.value())?;
        }
        if let Some(p) = o.stop {
            check_price("stop", p.value())?;
        }
        if let TimeInForce::Gtd(at) = o.tif {
            tif_of(at.millis())?;
        }
        Ok(())
    }

    /// The working orders filtered by client id and date (Retrieve Orders, `cid` + `cid_date`),
    /// one call per date the order may be keyed under, then the ended orders on the symbol
    /// created around the mint time (Orders History has no client id filter: its rows are
    /// matched by their `CID`).
    fn lookups(&self, symbol: &str, client_id: &str, created_ms: Option<i64>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.own(symbol)?;
        let cid = cid_of(client_id)?;
        // A minted id is its own mint time: the date and the window are read from it when the
        // order's time is not held.
        let at = created_ms.unwrap_or(cid);
        let mut out = Vec::new();
        for d in cid_dates(at)? {
            out.push(post("orders", "/v2/auth/r/orders", json!({ "cid": cid, "cid_date": d }), corr));
        }
        out.push(post(HIST, format!("/v2/auth/r/orders/{symbol}/hist"), json!({ "start": at - HIST_BEFORE_MS, "end": at + HIST_AFTER_MS, "limit": HIST_LIMIT }), corr));
        Ok(out)
    }

    /// One order row (Retrieve Orders: `[ID, GID, CID, SYMBOL, MTS_CREATE, MTS_UPDATE, AMOUNT,
    /// AMOUNT_ORIG, ORDER_TYPE, TYPE_PREV, MTS_TIF, _, FLAGS, STATUS, _, _, PRICE, PRICE_AVG,
    /// PRICE_TRAILING, PRICE_AUX_LIMIT, ...]`). `AMOUNT` is what is left, signed like the order.
    fn order_row(&self, r: &Row, ended: bool) -> Result<VenueOrder, ConnectorError> {
        let id = r.int(0).ok_or_else(|| ConnectorError::decode("bitfinex order: no ID"))?;
        let symbol = r.text(3).ok_or_else(|| ConnectorError::decode("bitfinex order: no SYMBOL"))?;
        let left = r.dec(6).unwrap_or_default();
        let orig = r.dec(7).unwrap_or(left);
        let qty = orig.abs();
        let filled = (qty - left.abs()).max(Decimal::ZERO);
        let kind = r.text(8).unwrap_or_default();
        let word = r.text(13).unwrap_or_default();
        let roles = price_roles(&kind, r.dec(16), r.dec(19));
        let status = match roles {
            // A type the doc does not list: its prices cannot be told apart, so its state is
            // not claimed either.
            None => otw_connector_sdk::enums::unknown("bitfinex", "order type", &kind),
            Some(_) => match self.status_of(&word) {
                Mapped::Known(OrderStatus::New) if filled > Decimal::ZERO => Mapped::Known(OrderStatus::PartiallyFilled),
                // Orders History: "the remainder may have been canceled".
                Mapped::Known(OrderStatus::PartiallyFilled) if ended => Mapped::Known(OrderStatus::Cancelled),
                s => s,
            },
        };
        let (limit, stop) = roles.unwrap_or((None, None));
        hold_order_lev(id, meta_lev(r));
        Ok(VenueOrder {
            venue_id: id.to_string(),
            client_id: r.int(2).filter(|c| *c > 0).map(|c| c.to_string()),
            symbol,
            side: side_of(if orig.is_zero() { left } else { orig }),
            order_type: kind,
            status,
            qty,
            filled_qty: filled,
            limit,
            stop,
            avg_price: r.dec(17).filter(|v| *v > Decimal::ZERO),
            reduce_only: r.int(12).is_some_and(|f| f & REDUCE_ONLY != 0),
            position: PositionSide::Net,
            created_ms: r.int(4),
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        })
    }

    /// A list of order rows, this book's only (one account lists every book in one answer).
    fn orders_of(&self, list: Vec<Row>, ended: bool) -> Result<Vec<VenueOrder>, ConnectorError> {
        let mut out = Vec::new();
        for r in &list {
            if r.text(3).is_some_and(|s| self.book().owns(&s)) {
                out.push(self.order_row(r, ended)?);
            }
        }
        Ok(out)
    }

    /// One trade row (Trades: `[ID, SYMBOL, MTS, ORDER_ID, EXEC_AMOUNT, EXEC_PRICE, ORDER_TYPE,
    /// ORDER_PRICE, MAKER, FEE, FEE_CURRENCY, CID]`). The fee is negative when paid (every
    /// example), so its cost is its negation. A stream `te` carries no fee yet.
    fn fill_row(&self, r: &Row) -> Result<VenueFill, ConnectorError> {
        let (Some(id), Some(symbol), Some(mts), Some(amount)) = (r.int(0), r.text(1), r.int(2), r.dec(4)) else {
            return Err(ConnectorError::decode("bitfinex trade: row shape"));
        };
        let fee = r.dec(9);
        Ok(VenueFill {
            exec_id: id.to_string(),
            venue_order_id: r.int(3).map(|o| o.to_string()).unwrap_or_default(),
            client_id: r.int(11).filter(|c| *c > 0).map(|c| c.to_string()),
            symbol,
            side: side_of(amount),
            qty: amount.abs(),
            price: r.dec(5).unwrap_or_default(),
            fee: fee.map(|f| -f).unwrap_or_default(),
            fee_currency: r.text(10).map(|c| currency(&c)).unwrap_or_default(),
            at_ms: mts,
            position_only: false,
            extra_fees: Vec::new(),
            fee_pending: fee.is_none(),
        })
    }

    /// One position row (Retrieve Positions: `[SYMBOL, STATUS, AMOUNT, BASE_PRICE, FUNDING,
    /// FUNDING_TYPE, PL, PL_PERC, PRICE_LIQ, LEVERAGE, _, POSITION_ID, MTS_CREATE, MTS_UPDATE, _,
    /// TYPE, ...]`): one net position, positive long, negative short, 0 closed. `None` for a row
    /// of another book, or a listed one no longer active. A stream frame (`pn`, `pu`, `pc`) whose
    /// status is `CLOSED` or whose amount is 0 says the instrument is flat; `pc` always does.
    /// `LEVERAGE` is not read: it is the position's measured ratio (`1.409...`, `3.067...` in the
    /// doc's examples), not a leverage an order carries.
    fn position_row(&self, r: &Row, from: PosFrame) -> Option<VenuePosition> {
        let symbol = r.text(0)?;
        if !self.on_derivatives() || Book::of(&symbol) != Book::Derivatives {
            return None;
        }
        let amount = r.dec(2).unwrap_or_default();
        let status = r.text(1).unwrap_or_default();
        let flat = match from {
            PosFrame::Listed => {
                if !status.eq_ignore_ascii_case("ACTIVE") {
                    return None;
                }
                false
            }
            PosFrame::Update => {
                let closed = status.eq_ignore_ascii_case("CLOSED") || amount.is_zero();
                if !closed && !status.eq_ignore_ascii_case("ACTIVE") {
                    return None;
                }
                closed
            }
            PosFrame::Close => true,
        };
        let amount = if flat { Decimal::ZERO } else { amount };
        Some(VenuePosition {
            symbol,
            side: PositionSide::Net,
            direction: match side_of(amount) {
                Mapped::Known(s) => Some(s),
                Mapped::Unknown(_) => None,
            },
            qty: amount.abs(),
            leverage: None,
            margin: None,
        })
    }

    fn positions_in(&self, body: &[u8], symbol: Option<&str>) -> Result<Vec<VenuePosition>, ConnectorError> {
        Ok(rows(body, "positions")?.iter().filter_map(|r| self.position_row(r, PosFrame::Listed)).filter(|p| symbol.is_none_or(|s| p.symbol.eq_ignore_ascii_case(s))).collect())
    }

    /// The venue order id of a change or a cancel, and the order's book.
    fn change_target(&self, cur: &VenueOrder) -> Result<i64, ConnectorError> {
        self.own(&cur.symbol)?;
        order_id(&cur.venue_id)
    }
}

/// Where a position row comes from: a list (Retrieve Positions, the stream's `ps` snapshot),
/// a stream update (`pn`, `pu`) or a stream close (`pc`).
#[derive(Clone, Copy)]
enum PosFrame {
    Listed,
    Update,
    Close,
}

/// What a notification carries as its order(s): `DATA` is "An array containing only the new
/// order" on a submit, the order itself on a change or a cancel.
fn notified_orders(body: &[u8]) -> Result<Vec<Row>, ConnectorError> {
    let parts: Vec<Box<RawValue>> = serde_json::from_slice(body).map_err(|e| decode_err("notification", e))?;
    let Some(data) = parts.get(4).map(|d| d.get().trim()).filter(|d| *d != "null") else { return Ok(Vec::new()) };
    if let Ok(list) = serde_json::from_str::<Vec<Vec<Box<RawValue>>>>(data) {
        return Ok(list.into_iter().map(Row).collect());
    }
    let one: Vec<Box<RawValue>> = serde_json::from_str(data).map_err(|e| decode_err("notification data", e))?;
    Ok(if one.is_empty() { Vec::new() } else { vec![Row(one)] })
}

/// The next ended-orders page when this one is full ([`HIST_LIMIT`] rows): the doc states no
/// sort, so the page's own order says which bound moves. Newest first: the window ends at the
/// last row's `MTS_CREATE`; oldest first: it starts there (rows at that millisecond are read
/// again).
fn hist_cursor(list: &[Row]) -> Option<String> {
    if list.len() < usize::try_from(HIST_LIMIT).unwrap_or(usize::MAX) {
        return None;
    }
    let (first, last) = (list.first()?.int(4)?, list.last()?.int(4)?);
    Some(if first >= last { format!("end:{last}") } else { format!("start:{last}") })
}

impl TradeMapper for Bitfinex {
    fn venue_name(&self) -> &str {
        self.venue()
    }

    /// "Client Order ID; should be unique in the day (UTC+0). (int45)" (Submit Order): digits
    /// only. OTW mints it ([`TradeMapper::mint_client_id`]); the rule is what a typed one meets.
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule { max_len: 14, charset: "digits (a whole number below 2^45)", allowed: |c| c.is_ascii_digit() }
    }

    fn mints_client_ids(&self) -> bool {
        true
    }

    /// The milliseconds the order is made at, as the doc's examples mint it (`cid: Date.now()`):
    /// strictly increasing across the process, so unique in its day, and its own date.
    fn mint_client_id(&self, stamp_ms: i64) -> Result<String, ConnectorError> {
        if stamp_ms <= 0 || stamp_ms > INT45_MAX {
            return Err(ConnectorError::invalid(format!("Bitfinex: no client id can be minted at {stamp_ms} (it must stay below 2^45)")));
        }
        Ok(stamp_ms.to_string())
    }

    fn place(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        Ok(post("orderSubmit", "/v2/auth/w/order/submit", self.order_body(o, p)?, p.corr))
    }

    /// Update Order, in place, by the order's id: its prices only. A size change is refused (the
    /// doc does not say whether `amount` is the new total or what is left, and `AMOUNT_ORIG`
    /// keeps the size "before any update", so what filled could no longer be read); a time in
    /// force "can be updated but not removed", so it is left as placed. A derivatives order
    /// change must carry the leverage ("Mandatory for derivative order updates").
    fn amend(&self, _settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let id = self.change_target(cur)?;
        if change.qty.is_some() {
            return Err(ConnectorError::invalid("Bitfinex changes an order's prices in place, not its size: cancel it and place a new one"));
        }
        if change.tif.is_some() {
            return Err(ConnectorError::invalid("Bitfinex changes an order's prices in place, not its time in force: cancel it and place a new one"));
        }
        let mut b = Map::new();
        b.insert("id".into(), json!(id));
        let kind = cur.order_type.trim().strip_prefix("EXCHANGE ").unwrap_or(cur.order_type.trim()).to_string();
        let (limit, stop) = (change.limit.map(|v| v.value()), change.stop.map(|v| v.value()));
        match kind.as_str() {
            "LIMIT" | "IOC" | "FOK" => {
                if stop.is_some() {
                    return Err(ConnectorError::invalid("a limit order has no trigger price to change"));
                }
                let l = limit.ok_or_else(|| ConnectorError::invalid("nothing to change on this limit order but its price"))?;
                check_price("limit", l)?;
                b.insert("price".into(), json!(dec_text(l)));
            }
            "STOP" => {
                if limit.is_some() {
                    return Err(ConnectorError::invalid("a stop has no limit price to change"));
                }
                let s = stop.ok_or_else(|| ConnectorError::invalid("nothing to change on this stop but its trigger price"))?;
                check_price("stop", s)?;
                b.insert("price".into(), json!(dec_text(s)));
            }
            "STOP LIMIT" => {
                if limit.is_none() && stop.is_none() {
                    return Err(ConnectorError::invalid("nothing to change on this stop limit but its prices"));
                }
                if let Some(s) = stop {
                    check_price("stop", s)?;
                    b.insert("price".into(), json!(dec_text(s)));
                }
                if let Some(l) = limit {
                    check_price("limit", l)?;
                    b.insert("price_aux_limit".into(), json!(dec_text(l)));
                }
            }
            other => return Err(ConnectorError::unsupported(format!("Bitfinex: a {other} order is not changed from OpenTraderWorld: cancel it and place a new one"))),
        }
        if self.on_derivatives() {
            // The order's own (`META.$F33` of the read just made), else the one a working order
            // on the instrument states.
            let lev = order_leverage(id).or_else(|| p.leverage.and_then(lev_of)).ok_or_else(|| {
                ConnectorError::invalid(format!(
                    "Bitfinex needs the leverage on every change to a derivatives order, and states none for order {id} on {} (a whole number from 1 to 100): cancel it and place a new one",
                    cur.symbol
                ))
            })?;
            b.insert("lev".into(), json!(lev));
        }
        Ok(post("orderUpdate", "/v2/auth/w/order/update", Value::Object(b), p.corr))
    }

    /// Cancel Order by its id ("Users can cancel their margin, exchange and derivative orders by
    /// providing their IDs").
    fn cancel(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.own(symbol)?;
        Ok(post("orderCancel", "/v2/auth/w/order/cancel", json!({ "id": order_id(venue_id)? }), corr))
    }

    /// By the venue's id when the trader has it; by the client id and its date otherwise ("Users
    /// must also provide a `cid_date` to cancel an order through its CID"), every date it may be
    /// keyed under in one Cancel Orders (multiple) call.
    fn cancel_order(&self, settings: &Settings, symbol: &str, venue_id: &str, client: Option<&ClientRef>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        if !venue_id.trim().is_empty() {
            return self.cancel(settings, symbol, venue_id, corr);
        }
        let c = client.ok_or_else(|| ConnectorError::invalid("Bitfinex: an order with neither its id nor its client id cannot be cancelled"))?;
        self.own(symbol)?;
        let cid = cid_of(&c.id)?;
        let pairs: Vec<Value> = cid_dates(c.at_ms.unwrap_or(cid))?.into_iter().map(|d| json!([cid, d])).collect();
        Ok(post("orderCancelMulti", "/v2/auth/w/order/cancel/multi", json!({ "cid": pairs }), corr))
    }

    /// The kill switch names the orders it cancels (Cancel Orders (multiple), `id`): `all: 1`
    /// "cancels all open orders (both trading and derivative)", other books and programs alike.
    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::ByIds { max: CANCEL_BATCH }
    }

    fn cancel_ids(&self, _settings: &Settings, orders: &[VenueOrder], corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let ids = orders.iter().map(|o| order_id(&o.venue_id)).collect::<Result<Vec<_>, _>>()?;
        Ok(post("orderCancelMulti", "/v2/auth/w/order/cancel/multi", json!({ "id": ids }), corr))
    }

    /// Orders History states no sort and keeps [`HIST_LIMIT`] rows a page: a full page is
    /// followed ([`hist_cursor`]).
    fn lookup_pages(&self) -> usize {
        LOOKUP_PAGES
    }

    fn query_by_client_id(&self, _settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.lookups(symbol, client_id, None, corr)
    }

    fn query_by_client_id_at(&self, _settings: &Settings, symbol: &str, client_id: &str, at: &LookupAt, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.lookups(symbol, client_id, at.created_ms, corr)
    }

    /// The working orders on the symbol by id (Retrieve Orders by symbol, `id` an array), then
    /// the ended ones (Orders History by symbol, `id` an array).
    fn query_by_venue_id(&self, _settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.own(symbol)?;
        let id = order_id(venue_id)?;
        Ok(vec![
            post("ordersSymbol", format!("/v2/auth/r/orders/{symbol}"), json!({ "id": [id] }), corr),
            post(HIST, format!("/v2/auth/r/orders/{symbol}/hist"), json!({ "id": [id] }), corr),
        ])
    }

    /// Retrieve Orders: every working order of the account, every book (filtered to this one
    /// when read), or those on one symbol.
    fn open_orders(&self, _settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![match symbol {
            Some(s) => {
                self.own(s)?;
                post("ordersSymbol", format!("/v2/auth/r/orders/{s}"), json!({}), corr)
            }
            None => post("orders", "/v2/auth/r/orders", json!({}), corr),
        }])
    }

    /// Trades (by symbol), oldest first, a page at a time from the window's start.
    fn fills_since(&self, _settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.own(symbol)?;
        Ok(post("tradesSymbolHist", format!("/v2/auth/r/trades/{symbol}/hist"), json!({ "start": from_ms, "end": to_ms, "limit": FILLS_LIMIT, "sort": 1 }), corr))
    }

    /// The next page of fills starts at the last one read (ties within a millisecond are read
    /// again and dropped by their id). The next page of ended orders moves the window's bound
    /// the page reached ([`hist_cursor`]): `end:<ms>` or `start:<ms>`.
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        let (field, at) = match cursor.split_once(':') {
            Some(("end", t)) => ("end", t),
            Some(("start", t)) => ("start", t),
            Some(_) => return next,
            None => ("start", cursor),
        };
        let Ok(at) = at.parse::<i64>() else { return next };
        let mut body: Value = req.body.as_deref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_else(|| json!({}));
        body[field] = json!(at);
        next.body = Some(body.to_string().into());
        next
    }

    /// `pub:info:pair` (spot) or `pub:info:pair:futures`: `[[[PAIR, [_, _, _, MIN_ORDER_SIZE,
    /// MAX_ORDER_SIZE, ...]], ...]]` (Configs).
    fn instrument_rules(&self, _settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let path = match self.book() {
            Book::Spot => "/v2/conf/pub:info:pair",
            Book::Derivatives => "/v2/conf/pub:info:pair:futures",
        };
        Ok(HttpRequest::get("conf", REST, path, corr))
    }

    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        self.check(o)
    }

    fn positions(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.on_derivatives() {
            return Ok(None);
        }
        self.own(symbol)?;
        Ok(Some(post(positions_endpoint(symbol)?, "/v2/auth/r/positions", json!({}), corr)))
    }

    fn all_positions(&self, _settings: &Settings, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(if self.on_derivatives() { vec![post("positions", "/v2/auth/r/positions", json!({}), corr)] } else { Vec::new() })
    }

    /// One net position per instrument ("Positive for long and negative for short positions",
    /// Retrieve Positions): the mode is the account's, answered with no position open. Read
    /// from the positions call, whose rows have no other mode to say.
    fn account_position_mode(&self, _settings: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(self.on_derivatives().then(|| post("positions", "/v2/auth/r/positions", json!({}), corr)))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        rows(body, "positions")?;
        Ok(Some(PositionMode::OneWay))
    }

    /// The leverage an order carries on `symbol`: the one the working orders on it state
    /// (`META.$F33`, Retrieve Orders by symbol), read before an order when none is held.
    fn symbol_leverage(&self, _settings: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        if !self.on_derivatives() {
            return Ok(None);
        }
        self.own(symbol)?;
        Ok(Some(post("ordersSymbol", format!("/v2/auth/r/orders/{symbol}"), json!({}), corr)))
    }

    /// The `$F33` every working order on `symbol` states; none when none states one, or when
    /// they state different ones (no one of them is chosen).
    fn decode_symbol_leverage(&self, symbol: &str, _side: PositionSide, body: &[u8]) -> Result<Option<Decimal>, ConnectorError> {
        let mut levs: Vec<i64> = rows(body, "orders")?.iter().filter(|r| r.text(3).is_some_and(|s| s.eq_ignore_ascii_case(symbol))).filter_map(meta_lev).collect();
        levs.sort_unstable();
        levs.dedup();
        Ok(match levs.as_slice() {
            [one] => Some(Decimal::from(*one)),
            _ => None,
        })
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    /// The submit's notification: `DATA` holds the new order, its `ID` and `CID`.
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let list = notified_orders(body)?;
        let Some(r) = list.first() else { return Ok(Ack::without_id(None)) };
        Ok(Ack { venue_id: r.int(0).map(|v| v.to_string()).unwrap_or_default(), client_id: r.int(2).filter(|c| *c > 0).map(|c| c.to_string()) })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        Ok((self.orders_of(rows(body, "orders")?, false)?, None))
    }

    /// An ended-orders page as full as the doc's "max. 2500" may leave orders out of the window:
    /// it names the next one ([`hist_cursor`]).
    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let list = rows(body, "orders")?;
        let next = if ctx.endpoint == HIST { hist_cursor(&list) } else { None };
        Ok((self.orders_of(list, ctx.endpoint == HIST)?, next))
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let list = rows(body, "trades")?;
        let n = list.len();
        let mut out = Vec::new();
        let mut last = None;
        for r in &list {
            let f = self.fill_row(r)?;
            last = Some(f.at_ms);
            if self.book().owns(&f.symbol) {
                out.push(f);
            }
        }
        let next = if n >= FILLS_LIMIT { last.map(|t| t.to_string()) } else { None };
        Ok((out, next))
    }

    /// `pub:info:pair` and its derivatives twin. Sizes count the base coin on both books (the
    /// amount is "Amount of order"); the amount step is 8 decimals ("The amount field allows up
    /// to 8 decimals"); prices are checked against 5 significant digits by [`check_price`]. The
    /// list states no status: a listed pair trades. Paper-trading pairs (`TEST...`) are left out.
    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        type Pair = (String, Vec<Box<RawValue>>);
        let outer: Vec<Vec<Pair>> = serde_json::from_slice(body).map_err(|e| decode_err("pair info", e))?;
        let mut out = Vec::new();
        for (pair, info) in outer.into_iter().flatten() {
            let pair = pair.trim().to_ascii_uppercase();
            if pair.is_empty() || pair.starts_with("TEST") {
                continue;
            }
            let native = format!("t{pair}");
            if !self.book().owns(&native) {
                continue;
            }
            let size = |i: usize| info.get(i).and_then(|v| serde_json::from_str::<Dec>(v.get()).ok()).map(|d| d.0).filter(|d| *d > Decimal::ZERO);
            out.push(VenueRules {
                lot_step: Some(Decimal::new(1, 8)),
                min_qty: size(3),
                max_qty: size(4),
                status: TradingStatus::Trading,
                ..VenueRules::new(InstrumentId::new(self.book().venue(), &native))
            });
        }
        Ok((out, None))
    }

    fn decode_positions(&self, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.positions_in(body, None)
    }

    /// A positions read for one instrument (`positions:<symbol>`) answers that instrument only.
    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.positions_in(body, ctx.endpoint.strip_prefix(POSITIONS_OF))
    }

    /// The order statuses of the glossary ("Order Status"), read on their head word.
    /// `RSN_DUST` (a dust position closed by a market order) says nothing of a fill: unknown.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        match status_head(word) {
            "ACTIVE" => Mapped::Known(OrderStatus::New),
            "PARTIALLY FILLED" => Mapped::Known(OrderStatus::PartiallyFilled),
            "EXECUTED" | "FORCED EXECUTED" => Mapped::Known(OrderStatus::Filled),
            "CANCELED"
            | "POSTONLY CANCELED"
            | "FILLORKILL CANCELED"
            | "IOC CANCELED"
            | "INSUFFICIENT BALANCE (G1)"
            | "INSUFFICIENT BALANCE (U1)"
            | "INSUFFICIENT MARGIN"
            | "RSN_POS_REDUCE_INCR"
            | "RSN_POS_REDUCE_FLIP"
            | "RSN_POS_NOTFOUND"
            | "RSN_BOOK_SLIP"
            | "RSN_PAUSE" => Mapped::Known(OrderStatus::Cancelled),
            _ => otw_connector_sdk::enums::unknown("bitfinex", "order status", word),
        }
    }

    /// The account socket (`wss://api.bitfinex.com/ws/2`): the login subscribes it ("Subscribing
    /// to the Account Info channel is automatically done when you authenticate"), so nothing
    /// else is sent but a ping as often as the server's heartbeat.
    fn private_socket(&self, _settings: &Settings) -> Option<PrivateSocket> {
        Some(PrivateSocket::new(WS_AUTH, "/ws/2", Vec::new(), br#"{"event":"ping","cid":1}"#.to_vec(), PING_EVERY_MS))
    }

    /// Channel 0 frames (Account Info): orders `os` (a list), `on`/`ou`/`oc` (one), positions
    /// `ps`/`pn`/`pu`/`pc`, trades `te`/`tu`; the login's answer; `info` 20051 and 20061 ask to
    /// reconnect (an answer refused, so the stream is opened again). `info` 20060 ("maintenance
    /// starting", "pause any activity") announces maintenance: new orders are refused until
    /// 20061 ("maintenance over") or a reconnect. Every other frame (heartbeat, wallets,
    /// notifications) says nothing here. Rows of the other book are left out.
    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let text = payload.trim_ascii_start();
        if text.starts_with(b"{") {
            let v: Value = serde_json::from_slice(text).map_err(|e| decode_err("socket event", e))?;
            let code = v.get("code").map(|c| c.to_string().trim_matches('"').to_string()).unwrap_or_default();
            let msg = v.get("msg").and_then(Value::as_str).unwrap_or_default();
            return Ok(match v.get("event").and_then(Value::as_str) {
                Some("auth") => {
                    let ok = v.get("status").and_then(Value::as_str) == Some("OK");
                    vec![PrivateEvent::Answer { op: "auth".into(), ok, message: if ok { String::new() } else { format!("Bitfinex refused the login (code {code}) {msg}").trim().to_string() } }]
                }
                Some("info") if code == "20060" => {
                    vec![PrivateEvent::Maintenance { on: true, message: format!("code 20060 {msg}").trim().to_string() }]
                }
                Some("info") if code == "20061" => vec![
                    PrivateEvent::Maintenance { on: false, message: format!("code 20061 {msg}").trim().to_string() },
                    PrivateEvent::Answer { op: "info".into(), ok: false, message: "Bitfinex asked to reconnect (code 20061)".into() },
                ],
                Some("info") if code == "20051" => {
                    vec![PrivateEvent::Answer { op: "info".into(), ok: false, message: format!("Bitfinex asked to reconnect (code {code})") }]
                }
                Some("error") => vec![PrivateEvent::Answer { op: "error".into(), ok: false, message: format!("Bitfinex: {msg} (code {code})") }],
                _ => Vec::new(),
            });
        }
        let parts: Vec<Box<RawValue>> = serde_json::from_slice(text).map_err(|e| decode_err("socket frame", e))?;
        let (Some(kind), Some(data)) = (parts.get(1).and_then(|k| serde_json::from_str::<String>(k.get()).ok()), parts.get(2)) else { return Ok(Vec::new()) };
        let list = |what: &str| -> Result<Vec<Row>, ConnectorError> { rows(data.get().as_bytes(), what) };
        let one = |what: &str| -> Result<Row, ConnectorError> { serde_json::from_str::<Vec<Box<RawValue>>>(data.get()).map(Row).map_err(|e| decode_err(what, e)) };
        let mut out = Vec::new();
        match kind.as_str() {
            "os" => out.extend(self.orders_of(list("order snapshot")?, false)?.into_iter().map(PrivateEvent::Order)),
            "on" | "ou" | "oc" => out.extend(self.orders_of(vec![one("order")?], false)?.into_iter().map(PrivateEvent::Order)),
            "ps" => out.extend(list("position snapshot")?.iter().filter_map(|r| self.position_row(r, PosFrame::Listed)).map(PrivateEvent::Position)),
            "pn" | "pu" => out.extend(self.position_row(&one("position")?, PosFrame::Update).map(PrivateEvent::Position)),
            "pc" => out.extend(self.position_row(&one("position")?, PosFrame::Close).map(PrivateEvent::Position)),
            "te" | "tu" => {
                let f = self.fill_row(&one("trade")?)?;
                if self.book().owns(&f.symbol) {
                    out.push(PrivateEvent::Fill(f));
                }
            }
            _ => {}
        }
        Ok(out)
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        super::trade_errors::ERRORS
    }
}
