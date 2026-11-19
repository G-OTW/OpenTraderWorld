//! Kraken spot orders, written from the documentation alone: no account, no key, no sandbox
//! (Kraken has none for spot). Every field below is the doc's; each claim is sourced in
//! PROVIDER.md ("Trading"). The order path itself is the shared trader's; this file only says
//! what the calls are and what the answers mean.
//!
//! - Place: `AddOrder`, a signed form POST. `market` (sized in the base coin), `limit`,
//!   `stop-loss`, `stop-loss-limit`, `take-profit`, `take-profit-limit` on the last price;
//!   `timeinforce` GTC (sent as the default), IOC and FOK on a limit, GTD with `expiretm`;
//!   `oflags=post` for post-only. Every order carries `oflags=fciq` (the fee in the quote
//!   currency, the venue's default on a buy only), so a fill's fee is always in the quote.
//! - Client id: `cl_ord_id`, free text of 18 characters at most, or a UUID. OTW's key goes as
//!   is when it fits; a longer one (a robot's 24 characters) is packed into a UUID and read
//!   back from it ([`pack`], [`unpack`]), whatever spelling of the UUID the venue echoes.
//! - Change in place (`AmendOrder`: limit, trigger, size; `post_only` kept on a post-only
//!   order's new limit), cancel by `txid`, cancel all in one
//!   call (`CancelAll`, the whole account). Lookup by client id in the open orders, then the
//!   closed ones; by venue id with `QueryOrders`.
//! - The pair: orders take its key or altname; answers name it by altname (orders), key
//!   (fills) or `BTC/USD` (the socket). The trader reads the whole pair list and hands its
//!   name table to the decoders ([`TradeMapper::uses_symbol_names`]); rows are named by the
//!   wsname, as the account read names them.
//! - Private stream: `executions` on `ws-auth.kraken.com/v2`, its token from
//!   `GetWebSocketsToken` before each connection. Its frames carry only what changed, so each
//!   one is an "order changed, read it again" ([`PrivateEvent::OrderChanged`]).

#![deny(clippy::float_arithmetic)]

use std::collections::BTreeMap;

use otw_connector_model::{ConnectorError, CorrId, Decimal, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, SizeUnit, TimeInForce, TriggerKind};
use otw_connector_sdk::parse::{Dec, OptDec};
use otw_connector_sdk::signer::encode_query;
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, ClientIdRule, ContractSizes, DecodeCtx, ErrorRow, Page, Placing, PositionSide, PrivateEvent, PrivateSocket, Settings, TradeMapper, TradingStatus, VenueError,
    VenueFill, VenueOrder, VenueRules,
};
use otw_connector_sdk::{HttpRequest, Method, RawFrame};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{base_qty, has_flag, order_prices, DescrPrice, Kraken, HISTORY_COST, REST, VENUE, WS_AUTH};

/// Rows per list page: OpenOrders "with_cursor ... values above `100` are rejected",
/// TradesHistory "Number of trades returned per page (1-100)".
const PAGE: usize = 100;
/// The private socket's heartbeat: the server "closes any open websocket connection within
/// approximately one minute of inactivity"; any request, a `ping` for one, keeps it alive.
const PING_EVERY_MS: u64 = 30_000;
const PRIVATE_PATH: &str = "/v2";
/// "Free format ascii text up to 18 characters".
const FREE_TEXT_MAX: usize = 18;
/// The longest OTW key packed into a UUID: 24 characters of `[a-z0-9]` (a robot's key).
const PACKED_MAX: usize = 24;

fn dec_text(d: Decimal) -> String {
    d.normalize().to_string()
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kraken {what}: {e}"))
}

/// A signed private call: a form POST whose body the signer puts the nonce in front of.
fn private(endpoint: &'static str, method: &str, params: &BTreeMap<&str, String>, corr: CorrId) -> HttpRequest {
    let mut h = HttpRequest::get(endpoint, REST, format!("/0/private/{method}"), corr).signed();
    h.method = Method::Post;
    let pairs: Vec<(String, String)> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    h.body = Some(encode_query(&pairs).into());
    h
}

// ── Client ids ──

fn base36(c: char) -> Option<u128> {
    c.to_digit(36).filter(|_| c.is_ascii_digit() || c.is_ascii_lowercase()).map(u128::from)
}

/// The version nibble of a UUID OTW packs (the 13th hex digit, `xxxxxxxx-xxxx-Axxx-...`): no
/// UUID version (1 to 8) uses it, so a UUID placed elsewhere (a v4 has `4` there) is never read
/// as an OTW key.
const MARK: u128 = 0xA;
/// The packed number's bits: 128 less the marker's 4.
const PAYLOAD_BITS: u32 = 124;
/// The bits under the marker (the UUID's last 19 hex digits).
const LOW_BITS: u32 = 76;

/// How many keys are shorter than `len` (from 19 characters): where keys of `len` start.
fn offset(len: usize) -> Option<u128> {
    (FREE_TEXT_MAX + 1..len).try_fold(0u128, |acc, l| acc.checked_add(36u128.checked_pow(u32::try_from(l).ok()?)?))
}

/// An OTW key of 19 to 24 characters of `[a-z0-9]` as a long UUID: the key read as a base-36
/// number past every shorter key (so its length is read back), in 124 bits, with [`MARK`] as
/// its version nibble. Reversible ([`unpack`]), leading zeros included. `None` for a key that
/// does not fit: a 24-character one from about `x` up (a robot's starts `ots`).
pub(super) fn pack(key: &str) -> Option<String> {
    let len = key.chars().count();
    if !(FREE_TEXT_MAX + 1..=PACKED_MAX).contains(&len) {
        return None;
    }
    let mut n: u128 = 0;
    for c in key.chars() {
        n = n.checked_mul(36)?.checked_add(base36(c)?)?;
    }
    let p = offset(len)?.checked_add(n)?;
    if p >> PAYLOAD_BITS != 0 {
        return None;
    }
    let v = ((p >> LOW_BITS) << (LOW_BITS + 4)) | (MARK << LOW_BITS) | (p & ((1u128 << LOW_BITS) - 1));
    let h = format!("{v:032x}");
    Some(format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..]))
}

/// The OTW key a packed UUID carries; `None` for a UUID OTW did not pack (no [`MARK`]).
pub(super) fn unpack(uuid: &str) -> Option<String> {
    let hex: String = uuid.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let v = u128::from_str_radix(&hex, 16).ok()?;
    if (v >> LOW_BITS) & 0xF != MARK {
        return None;
    }
    let p = ((v >> (LOW_BITS + 4)) << LOW_BITS) | (v & ((1u128 << LOW_BITS) - 1));
    let len = (FREE_TEXT_MAX + 1..=PACKED_MAX).rev().find(|l| offset(*l).is_some_and(|o| o <= p))?;
    let mut n = p - offset(len)?;
    let mut digits = Vec::with_capacity(len);
    for _ in 0..len {
        let d = u32::try_from(n % 36).ok()?;
        digits.push(char::from_digit(d, 36)?);
        n /= 36;
    }
    if n != 0 {
        return None;
    }
    digits.reverse();
    Some(digits.into_iter().collect())
}

/// A `cl_ord_id` as OTW spells it: a UUID (long or short, any case: "32 hex characters"
/// with or without "4 dashes") in its long lower-case form, free text as it is.
pub(super) fn spelled(raw: &str) -> String {
    let t = raw.trim();
    let hex: String = t.chars().filter(|c| *c != '-').collect();
    let uuid_shape = (t.len() == 32 || (t.len() == 36 && t.chars().filter(|c| *c == '-').count() == 4)) && hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit());
    if !uuid_shape {
        return t.to_string();
    }
    let h = hex.to_ascii_lowercase();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

// ── Answers ──

#[derive(Deserialize)]
struct Answer<T> {
    result: T,
}

#[derive(Deserialize, Default)]
struct Descr {
    #[serde(default)]
    pair: String,
    #[serde(rename = "type", default)]
    side: String,
    #[serde(default)]
    ordertype: String,
    #[serde(default)]
    price: DescrPrice,
    #[serde(default)]
    price2: DescrPrice,
    #[serde(default)]
    order: String,
}

/// An order as OpenOrders, ClosedOrders and QueryOrders write it (schemas `open`, `closed`).
#[derive(Deserialize)]
struct OrderRow {
    #[serde(default)]
    cl_ord_id: Option<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    opentm: Option<Dec>,
    #[serde(default)]
    descr: Descr,
    #[serde(default)]
    vol: OptDec,
    #[serde(default)]
    vol_exec: OptDec,
    /// "Average price (quote currency)".
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    reduce_only: Option<bool>,
    #[serde(default)]
    misc: String,
    #[serde(default)]
    oflags: String,
}

#[derive(Deserialize, Default)]
struct Cursor {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
struct TradesPage {
    #[serde(default)]
    trades: BTreeMap<String, TradeRow>,
    #[serde(default)]
    cursor: Option<Cursor>,
}

#[derive(Deserialize)]
struct TradeRow {
    #[serde(default)]
    ordertxid: String,
    #[serde(default)]
    pair: String,
    time: Dec,
    #[serde(rename = "type", default)]
    side: String,
    #[serde(default)]
    price: OptDec,
    #[serde(default)]
    fee: OptDec,
    #[serde(default)]
    vol: OptDec,
}

#[derive(Deserialize)]
struct PairRow {
    altname: String,
    #[serde(default)]
    wsname: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    pair_decimals: Option<u32>,
    #[serde(default)]
    lot_decimals: Option<u32>,
    #[serde(default)]
    lot_multiplier: OptDec,
    #[serde(default)]
    ordermin: OptDec,
    #[serde(default)]
    costmin: OptDec,
    #[serde(default)]
    cost_decimals: Option<u32>,
}

#[derive(Deserialize)]
struct AckResult {
    #[serde(default)]
    txid: Vec<String>,
    #[serde(default)]
    descr: Descr,
}

fn side_of(word: &str) -> Mapped<Side> {
    match word {
        "buy" => Mapped::Known(Side::Buy),
        "sell" => Mapped::Known(Side::Sell),
        other => Mapped::Unknown(other.to_string()),
    }
}

fn step(decimals: Option<u32>) -> Option<Decimal> {
    decimals.and_then(|d| Decimal::try_from_i128_with_scale(1, d).ok())
}

fn positive(d: OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

/// Seconds with a fraction to epoch milliseconds.
fn millis(secs: Decimal) -> Option<i64> {
    i64::try_from(secs.checked_mul(Decimal::ONE_THOUSAND)?.trunc()).ok()
}

/// The quote currency a fill's fee is in: the wsname's second half (`XBT/USD`).
fn quote_of(symbol: &str) -> String {
    symbol.split_once('/').map(|(_, q)| q.to_string()).unwrap_or_default()
}

/// The triggered order types (stop-loss, take-profit and their limit and trailing forms).
fn triggered(ordertype: &str) -> bool {
    ordertype.starts_with("stop-loss") || ordertype.starts_with("take-profit") || ordertype.starts_with("trailing-stop")
}

impl Kraken {
    /// The pair as an order names it: the altname or the key as given ("Asset pair `id` or
    /// `altname`"); a wsname (`XBT/USD`, how the account read names pairs) through the pair
    /// list read last. One the list does not give is refused, never guessed.
    fn pair_of(&self, native: &str) -> Result<String, ConnectorError> {
        if !native.contains('/') {
            return Ok(native.to_string());
        }
        let held = self.altnames.lock().unwrap_or_else(|e| e.into_inner());
        held.get(&native.to_ascii_uppercase()).cloned().ok_or_else(|| {
            ConnectorError::invalid(format!("Kraken: {native} is not a pair of the list read, so nothing was sent: name it by its altname (XBTUSD)")).with_code("venue_rule")
        })
    }

    /// The AddOrder fields of `o` (sorted, as the doc's examples write them).
    fn order_params(&self, o: &OrderRequest, p: &Placing) -> Result<BTreeMap<&'static str, String>, ConnectorError> {
        let no = |what: &str| Err(ConnectorError::unsupported(format!("Kraken takes no {what} from OpenTraderWorld")));
        if o.opts.reduce_only {
            return no("reduce-only order: a spot sale of what is held is a plain sell");
        }
        if o.opts.display_qty.is_some() {
            return no("iceberg order");
        }
        if o.opts.close_position {
            return no("close-position order");
        }
        if o.opts.size_unit == SizeUnit::Quote {
            return no("size in the quote currency: give it in the base coin");
        }
        if o.opts.trigger.is_some_and(|t| t != TriggerKind::Last) {
            return no("trigger other than the last price");
        }
        let ordertype = match o.order_type {
            OrderType::Market => "market",
            OrderType::Limit => "limit",
            OrderType::Stop => "stop-loss",
            OrderType::StopLimit => "stop-loss-limit",
            OrderType::Mit => "take-profit",
            OrderType::Lit => "take-profit-limit",
            OrderType::TrailingStop => return no("trailing stop"),
        };
        let mut b = BTreeMap::new();
        b.insert("pair", self.pair_of(&o.instrument.native)?);
        b.insert("type", if o.side == Side::Buy { "buy" } else { "sell" }.to_string());
        b.insert("ordertype", ordertype.to_string());
        // "Order quantity in terms of the base asset".
        b.insert("volume", dec_text(o.qty.value()));
        let price = |p: Option<otw_connector_model::Price>, what: &str| p.map(|v| dec_text(v.value())).ok_or_else(|| ConnectorError::invalid(format!("this order needs its {what} price")));
        match o.order_type {
            OrderType::Limit => {
                b.insert("price", price(o.limit, "limit")?);
            }
            OrderType::Stop | OrderType::Mit => {
                b.insert("price", price(o.stop, "trigger")?);
            }
            OrderType::StopLimit | OrderType::Lit => {
                b.insert("price", price(o.stop, "trigger")?);
                b.insert("price2", price(o.limit, "limit")?);
            }
            _ => {}
        }
        if o.order_type.triggered() {
            b.insert("trigger", "last".to_string());
        }
        match o.tif {
            // GTC "is default if the parameter is omitted"; a market order fills at once.
            TimeInForce::Gtc => {}
            TimeInForce::Ioc if o.order_type == OrderType::Market => {}
            TimeInForce::Ioc => {
                b.insert("timeinforce", "IOC".to_string());
            }
            TimeInForce::Fok => {
                b.insert("timeinforce", "FOK".to_string());
            }
            TimeInForce::Gtd(until) => {
                b.insert("timeinforce", "GTD".to_string());
                b.insert("expiretm", until.secs().to_string());
            }
            TimeInForce::Day => return no("day order"),
        }
        b.insert("oflags", if o.opts.post_only { "post,fciq" } else { "fciq" }.to_string());
        if !p.client_id.is_empty() {
            b.insert("cl_ord_id", p.client_id.clone());
        }
        Ok(b)
    }

    /// One order row in OTW's terms, under the venue's `txid`. A pair the list read does not
    /// name is an error, never a guess: the book is read again.
    fn order_row(&self, ctx: &DecodeCtx<'_>, txid: String, r: OrderRow) -> Result<VenueOrder, ConnectorError> {
        let symbol = ctx.native("Kraken", &r.descr.pair)?.to_string();
        let filled = r.vol_exec.0.unwrap_or_default();
        // A market buy sized in the quote currency (`viqc`, placed elsewhere): its `vol` is not
        // a size in the base coin, so its size is what filled.
        let qty = base_qty(&r.oflags, r.vol.0, Some(filled));
        let status = match self.status_of(&r.status) {
            Mapped::Known(OrderStatus::New) if filled > Decimal::ZERO => Mapped::Known(OrderStatus::PartiallyFilled),
            // "CLOSED (fully filled from partial, or canceled partial)": a closed order that did
            // not fill whole was cancelled with what it filled.
            Mapped::Known(OrderStatus::Filled) if filled < qty => Mapped::Known(OrderStatus::Cancelled),
            other => other,
        };
        let (limit, stop) = order_prices(&r.descr.ordertype, r.descr.price.0, r.descr.price2.0);
        let working = matches!(status, Mapped::Known(OrderStatus::New | OrderStatus::PartiallyFilled | OrderStatus::PendingNew));
        // `misc` "`stopped` triggered by stop price, `touched` triggered by touch price": a
        // triggered type working without either is still waiting.
        let fired = r.misc.split(',').any(|m| matches!(m.trim(), "stopped" | "touched"));
        {
            let mut post = self.post_only.lock().unwrap_or_else(|e| e.into_inner());
            if working && has_flag(&r.oflags, "post") {
                post.insert(txid.clone());
            } else {
                post.remove(&txid);
            }
        }
        Ok(VenueOrder {
            venue_id: txid,
            client_id: r.cl_ord_id.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(spelled),
            symbol,
            side: side_of(&r.descr.side),
            order_type: r.descr.ordertype.clone(),
            status,
            qty,
            filled_qty: filled,
            limit,
            stop,
            avg_price: positive(r.price),
            reduce_only: r.reduce_only.unwrap_or(false),
            position: PositionSide::Net,
            created_ms: r.opentm.and_then(|t| millis(t.0)),
            trigger_pending: triggered(&r.descr.ordertype) && working && !fired,
            parent_client_id: None,
            exit: None,
            successor_id: None,
        })
    }

    /// The rules of one pair under `native`, its other names its aliases.
    fn rule_row(native: &str, r: &PairRow, names: &[&str]) -> VenueRules {
        let mut v = VenueRules::new(InstrumentId::new(VENUE, &native.to_ascii_uppercase()));
        v.aliases = names.iter().filter(|n| !n.eq_ignore_ascii_case(native)).map(|n| n.to_string()).collect();
        v.tick = positive(r.tick_size).or_else(|| step(r.pair_decimals));
        v.lot_step = step(r.lot_decimals);
        v.min_qty = positive(r.ordermin);
        v.min_notional = positive(r.costmin);
        v.quote_step = step(r.cost_decimals);
        v.status = match r.status.as_deref() {
            None | Some("online") => TradingStatus::Trading,
            Some(w @ "post_only") => TradingStatus::PostOnly(w.to_string()),
            Some(w @ "limit_only") => TradingStatus::LimitOnly(w.to_string()),
            // `cancel_only`, `reduce_only` (a margin position's state), `delisted`,
            // `maintenance`, `work_in_progress`, and any new word.
            Some(w) => TradingStatus::Closed(w.to_string()),
        };
        // "Amount to multiply lot volume by to get currency volume": a pair whose volume is
        // not the base coin is never sized by a guess.
        if r.lot_multiplier.0.is_some_and(|m| m != Decimal::ONE) {
            v.status = TradingStatus::Closed(format!("lot_multiplier {}: sized in lots, not taken by OpenTraderWorld", dec_text(r.lot_multiplier.0.unwrap_or_default())));
        }
        v
    }

    /// One fill in OTW's terms. A pair the list read does not name, or one named without its
    /// quote (no wsname), is an error, never a guess: the fee is in the quote currency.
    fn fill_row(ctx: &DecodeCtx<'_>, txid: String, r: TradeRow) -> Result<VenueFill, ConnectorError> {
        let symbol = ctx.native("Kraken", &r.pair)?.to_string();
        let fee_currency = quote_of(&symbol);
        if fee_currency.is_empty() {
            return Err(ConnectorError::decode(format!("Kraken: pair {} has no wsname in the list read, so the currency of its fill's fee is not known", r.pair)));
        }
        Ok(VenueFill {
            exec_id: txid,
            fee_currency,
            position_only: r.ordertxid.is_empty(),
            venue_order_id: r.ordertxid,
            client_id: None,
            symbol,
            side: side_of(&r.side),
            qty: r.vol.0.unwrap_or_default(),
            price: r.price.0.unwrap_or_default(),
            // "Total fee (quote currency)", positive as a cost in the doc's examples.
            fee: r.fee.0.unwrap_or_default(),
            at_ms: millis(r.time.0).unwrap_or_default(),
            extra_fees: Vec::new(),
            fee_pending: false,
        })
    }

    /// The orders of a list or lookup answer: OpenOrders and ClosedOrders wrap them under
    /// `open` / `closed` with a `cursor`; QueryOrders answers them keyed by `txid` directly.
    fn orders_of(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let a: Answer<Value> = serde_json::from_slice(body).map_err(|e| decode_err("orders", e))?;
        let Value::Object(mut result) = a.result else { return Err(ConnectorError::decode("kraken orders: the result is not an object")) };
        let wrapped = result.contains_key("open") || result.contains_key("closed");
        let mut rows: Vec<(String, Value)> = Vec::new();
        let mut next = None;
        if wrapped {
            for key in ["open", "closed"] {
                if let Some(Value::Object(m)) = result.remove(key) {
                    rows.extend(m);
                }
            }
            next = result.get("cursor").and_then(|c| c.get("next")).and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
        } else {
            rows.extend(result);
        }
        let mut out = Vec::with_capacity(rows.len());
        for (txid, v) in rows {
            let r: OrderRow = serde_json::from_value(v).map_err(|e| decode_err("order row", e))?;
            out.push(self.order_row(ctx, txid, r)?);
        }
        Ok((out, next))
    }

    fn subscribe_frame(token: &str) -> Vec<u8> {
        // No snapshot: the book is read on its own when the stream (re)connects.
        json!({ "method": "subscribe", "params": { "channel": "executions", "token": token, "snap_orders": false, "snap_trades": false, "order_status": true } })
            .to_string()
            .into_bytes()
    }
}

impl TradeMapper for Kraken {
    fn venue_name(&self) -> &str {
        "Kraken"
    }

    /// Free text of up to 18 characters; OTW's longer keys (19 to 24 of `[a-z0-9]`) go as a
    /// UUID ([`TradeMapper::client_id`]).
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule { max_len: FREE_TEXT_MAX, charset: "letters, digits, - and _", allowed: |c| c.is_ascii_alphanumeric() || c == '-' || c == '_' }
    }

    /// The key as is when it is free text Kraken takes (18 characters at most), else packed
    /// into a long UUID ([`pack`]); anything else refused naming the rule.
    fn client_id(&self, otw_id: &str) -> Result<String, ConnectorError> {
        if self.client_id_rule().check(self.venue_name(), otw_id).is_ok() {
            return Ok(otw_id.to_string());
        }
        pack(otw_id).ok_or_else(|| {
            ConnectorError::invalid(format!(
                "Kraken takes client order ids of 1 to {FREE_TEXT_MAX} letters, digits, - and _, or a UUID that OpenTraderWorld packs from 19 to {PACKED_MAX} lower-case letters and digits (a 24-character one from about `x` up does not fit; {otw_id:?} is neither)"
            ))
        })
    }

    /// A packed UUID back to OTW's key; free text, and a UUID OTW did not pack, as they are.
    fn otw_id(&self, venue_client_id: &str) -> String {
        let s = spelled(venue_client_id);
        unpack(&s).unwrap_or(s)
    }

    fn place(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        Ok(private("addOrder", "AddOrder", &self.order_params(o, p)?, p.corr))
    }

    /// AmendOrder: "The order identifiers assigned by Kraken and/or client will stay the same";
    /// `limit_price`, `trigger_price`, `order_qty` ("The new order quantity in terms of the base
    /// asset"). The time in force is not amendable. A post-only order's new limit goes with
    /// `post_only=true` ("the limit price change will be rejected if the order cannot be posted
    /// passively in the book"): it stays a maker order.
    fn amend(&self, _settings: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        if change.tif.is_some() {
            return Err(ConnectorError::unsupported("Kraken changes a working order's limit price, trigger price and size only: cancel it and send a new one for another time in force"));
        }
        let mut b = BTreeMap::new();
        b.insert("txid", cur.venue_id.clone());
        // By the order's type, not the prices read back: a stop-limit's limit may read as `0`
        // (the doc's examples), and it is still there to change.
        let t = cur.order_type.as_str();
        if let Some(px) = change.limit {
            if !(cur.limit.is_some() || matches!(t, "limit" | "iceberg") || t.ends_with("-limit")) {
                return Err(ConnectorError::invalid(format!("order {} ({t}) has no limit price to change", cur.venue_id)));
            }
            b.insert("limit_price", dec_text(px.value()));
            if self.post_only.lock().unwrap_or_else(|e| e.into_inner()).contains(&cur.venue_id) {
                b.insert("post_only", "true".to_string());
            }
        }
        if let Some(px) = change.stop {
            if !triggered(t) {
                return Err(ConnectorError::invalid(format!("order {} ({t}) has no trigger price to change", cur.venue_id)));
            }
            b.insert("trigger_price", dec_text(px.value()));
        }
        if let Some(q) = change.qty {
            b.insert("order_qty", dec_text(q.value()));
        }
        Ok(private("private:amendOrder", "AmendOrder", &b, p.corr))
    }

    /// CancelOrder by `txid` (no pair needed).
    fn cancel(&self, _settings: &Settings, _symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let b = BTreeMap::from([("txid", venue_id.to_string())]);
        Ok(private(super::trade_errors::CANCEL, "CancelOrder", &b, corr))
    }

    /// "Cancel all open orders": every pair of the account, one call.
    fn cancel_all(&self, _settings: &Settings, _symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![private("private:cancelAll", "CancelAll", &BTreeMap::new(), corr)])
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::Book
    }

    /// OpenOrders then ClosedOrders, each "Restrict results to given client order id".
    fn query_by_client_id(&self, _settings: &Settings, _symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let b = BTreeMap::from([("cl_ord_id", client_id.to_string())]);
        Ok(vec![private("private:openOrders", "OpenOrders", &b, corr), private("private:closedOrders", "ClosedOrders", &b, corr)])
    }

    /// QueryOrders by `txid` (open or closed).
    fn query_by_venue_id(&self, _settings: &Settings, _symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let b = BTreeMap::from([("txid", venue_id.to_string())]);
        Ok(vec![private("private:queryOrders", "QueryOrders", &b, corr)])
    }

    /// OpenOrders of the whole account (it takes no pair), paged by cursor, 100 a page.
    fn open_orders(&self, _settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let b = BTreeMap::from([("with_cursor", "true".to_string()), ("limit", PAGE.to_string())]);
        Ok(vec![private("private:openOrders", "OpenOrders", &b, corr)])
    }

    /// TradesHistory over the window, the whole account (`pair` takes spellings the doc does
    /// not list), paged by cursor. `start` is exclusive and in whole seconds: one second
    /// earlier, so nothing at the window's first second is missed (a duplicate is dropped by
    /// its id).
    fn fills_since(&self, _settings: &Settings, _symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let start = from_ms.div_euclid(1000) - 1;
        let end = to_ms.div_euclid(1000) + i64::from(to_ms.rem_euclid(1000) > 0);
        let b = BTreeMap::from([
            ("type", "all".to_string()),
            ("start", start.to_string()),
            ("end", end.to_string()),
            ("with_cursor", "true".to_string()),
            ("limit", PAGE.to_string()),
        ]);
        Ok(private("private:tradesHistory", "TradesHistory", &b, corr).weight(HISTORY_COST))
    }

    /// The whole pair list (`GET /0/public/AssetPairs`): a pair is found by any of its names.
    fn instrument_rules(&self, _settings: &Settings, _symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(HttpRequest::get("tradeRules", REST, "/0/public/AssetPairs", corr))
    }

    /// What the doc limits beyond the declared capabilities: FOK "Available for `limit` orders
    /// only"; IOC on a limit (a market order fills at once; the doc says nothing of IOC on a
    /// triggered order); GTD not on a market order.
    fn check_order(&self, o: &OrderRequest) -> Result<(), ConnectorError> {
        match (o.order_type, o.tif) {
            (OrderType::Limit, _) | (_, TimeInForce::Gtc) | (OrderType::Market, TimeInForce::Ioc) => Ok(()),
            (OrderType::Market, TimeInForce::Gtd(_)) => Err(ConnectorError::unsupported("Kraken: a market order fills at once, it takes no expiry date")),
            (_, TimeInForce::Gtd(_)) => Ok(()),
            (_, TimeInForce::Fok) => Err(ConnectorError::unsupported("Kraken takes fill-or-kill on a limit order only")),
            (t, tif) => Err(ConnectorError::unsupported(format!("Kraken takes no {t:?} order {:?}: GTC or GTD only", tif.kind()))),
        }
    }

    /// The next page: `cursor` in the body ("Opaque pagination token from `cursor.next` of the
    /// previous page"), the rest unchanged.
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        let body = req.body.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let mut kept: Vec<String> = body.split('&').filter(|kv| !kv.is_empty() && !kv.starts_with("cursor=")).map(str::to_string).collect();
        kept.push(encode_query(&[("cursor".to_string(), cursor.to_string())]));
        next.body = Some(kept.join("&").into());
        next
    }

    /// AddOrder `validate`: "the order will be validated only, it will not trade in the
    /// matching engine". Sent without the client id.
    fn dry_run(&self, _settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<Option<HttpRequest>, ConnectorError> {
        let mut b = self.order_params(o, &Placing { client_id: String::new(), ..p.clone() })?;
        b.insert("validate", "true".to_string());
        Ok(Some(private("addOrderValidate", "AddOrder", &b, p.corr)))
    }

    fn decode_dry_run(&self, body: &[u8]) -> Result<String, ConnectorError> {
        let a: Answer<AckResult> = serde_json::from_slice(body).map_err(|e| decode_err("AddOrder validate", e))?;
        Ok(if a.result.descr.order.is_empty() { "validated".into() } else { format!("validated ({})", a.result.descr.order) })
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        super::trade_errors::decode(frame)
    }

    /// `txid[0]`; the REST answer echoes no client id.
    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError> {
        let a: Answer<AckResult> = serde_json::from_slice(body).map_err(|e| decode_err("AddOrder", e))?;
        Ok(match a.result.txid.into_iter().find(|t| !t.is_empty()) {
            Some(id) => Ack { venue_id: id, client_id: None },
            None => Ack::without_id(None),
        })
    }

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.orders_of(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.orders_of(ctx, body)
    }

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.decode_fills_in(&DecodeCtx::new("", &ContractSizes::default()), body)
    }

    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        let a: Answer<TradesPage> = serde_json::from_slice(body).map_err(|e| decode_err("TradesHistory", e))?;
        let next = a.result.cursor.and_then(|c| c.next).filter(|c| !c.is_empty());
        let fills = a.result.trades.into_iter().map(|(txid, r)| Self::fill_row(ctx, txid, r)).collect::<Result<Vec<_>, _>>()?;
        Ok((fills, next))
    }

    /// Each pair under its altname (what market data names it, `XBTUSD`), then again under its
    /// wsname (`XBT/USD`, what the account read names it) where it has one: an order from
    /// either side finds its rules. Each names the other, and the key, as aliases; the name
    /// table maps every name to the wsname, so a decoded row is named as the account read
    /// names it. The altname table an order is sent with is kept ([`Kraken::pair_of`]).
    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let a: Answer<BTreeMap<String, PairRow>> = serde_json::from_slice(body).map_err(|e| decode_err("AssetPairs", e))?;
        let mut first = Vec::with_capacity(a.result.len());
        let mut second = Vec::new();
        {
            let mut held = self.altnames.lock().unwrap_or_else(|e| e.into_inner());
            for (key, r) in &a.result {
                if r.altname.is_empty() {
                    continue;
                }
                let ws = r.wsname.as_deref().filter(|w| !w.is_empty());
                let names: Vec<&str> = [Some(key.as_str()), Some(r.altname.as_str()), ws].into_iter().flatten().collect();
                for n in &names {
                    held.insert(n.to_ascii_uppercase(), r.altname.clone());
                }
                first.push(Self::rule_row(&r.altname, r, &names));
                if let Some(w) = ws.filter(|w| !w.eq_ignore_ascii_case(&r.altname)) {
                    second.push(Self::rule_row(w, r, &names));
                }
            }
        }
        first.extend(second);
        Ok((first, None))
    }

    /// REST `status`: `pending` "order pending book entry", `open`, `closed`, `canceled`,
    /// `expired`. `open` with a fill and `closed` short of its size are told apart in
    /// [`Kraken::order_row`], by `vol_exec`.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "pending" => OrderStatus::PendingNew,
            "open" => OrderStatus::New,
            "closed" => OrderStatus::Filled,
            "canceled" => OrderStatus::Cancelled,
            "expired" => OrderStatus::Expired,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    fn uses_symbol_names(&self) -> bool {
        true
    }

    /// `executions` on the authenticated host, its subscribe frame written by the bootstrap
    /// (the token goes in it), an application `ping` every 30 s.
    fn private_socket(&self, _settings: &Settings) -> Option<PrivateSocket> {
        Some(PrivateSocket::new(WS_AUTH, PRIVATE_PATH, Vec::new(), json!({ "method": "ping" }).to_string().into_bytes(), PING_EVERY_MS))
    }

    /// `GetWebSocketsToken` before each connection: "The token should be used within 15
    /// minutes of creation".
    fn private_socket_bootstrap(&self, _settings: &Settings, _socket: &PrivateSocket, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(private("private:wsToken", "GetWebSocketsToken", &BTreeMap::new(), corr)))
    }

    fn decode_private_socket_bootstrap(&self, mut socket: PrivateSocket, body: &[u8]) -> Result<PrivateSocket, ConnectorError> {
        let v: Value = serde_json::from_slice(body).map_err(|e| decode_err("GetWebSocketsToken", e))?;
        let token = v["result"]["token"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| ConnectorError::decode("kraken GetWebSocketsToken: no token"))?;
        socket.subscribe = Self::subscribe_frame(token);
        Ok(socket)
    }

    fn decode_private(&self, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.decode_private_in(&DecodeCtx::new(PRIVATE_PATH, &ContractSizes::default()), payload)
    }

    /// The answers to `subscribe` and `ping`, and `executions`: each order a frame names is
    /// read again by its `txid`, since "the fields included in the message is dependant on the
    /// `exec_type`" (an update may carry only the status). Heartbeats and status frames say
    /// nothing of the account.
    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let v: Value = serde_json::from_slice(payload).map_err(|e| decode_err("private frame", e))?;
        if let Some(method) = v.get("method").and_then(Value::as_str) {
            let ok = v.get("success").and_then(Value::as_bool).unwrap_or(true);
            let message = v.get("error").and_then(Value::as_str).unwrap_or_default().to_string();
            return Ok(vec![PrivateEvent::Answer { op: method.to_string(), ok, message }]);
        }
        if v.get("channel").and_then(Value::as_str) != Some("executions") {
            return Ok(Vec::new());
        }
        let mut out: Vec<PrivateEvent> = Vec::new();
        for row in v.get("data").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = row.get("order_id").and_then(Value::as_str).filter(|i| !i.is_empty()) else { continue };
            // "received and validated but the order is not live yet": its `new` (or its
            // `canceled`) follows, and each read costs the key's call counter.
            if row.get("exec_type").and_then(Value::as_str) == Some("pending_new") {
                continue;
            }
            if out.iter().any(|e| matches!(e, PrivateEvent::OrderChanged { venue_id, .. } if venue_id == id)) {
                continue;
            }
            // The socket's `BTC/USD` is named when the list gives it; the read needs no pair.
            let symbol = row.get("symbol").and_then(Value::as_str).and_then(|s| ctx.native("Kraken", s).ok()).map(str::to_string);
            out.push(PrivateEvent::OrderChanged { symbol, venue_id: id.to_string() });
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
