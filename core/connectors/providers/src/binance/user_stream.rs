//! Binance account stream (the user data stream), spot and USDⓈ-M: where it is, how it is
//! opened, and what its frames say. Pure: the caller opens the socket named here, sends the
//! frames built here, and hands every text frame back to [`decode`].
//!
//! - **Spot subscribes on the WebSocket API** (`userDataStream.subscribe.signature`): the REST
//!   listen key is gone (`POST /api/v3/userDataStream` answers `410`). The subscription is
//!   signed as a REST call is (`apiKey`, `recvWindow`, `timestamp`, in name order), so the
//!   account's HMAC key serves: Binance takes any key type on this form (changelog
//!   2025-08-12), only `userDataStream.subscribe` after a logon wants Ed25519.
//! - **USDⓈ-M reads a listen key** (`POST /fapi/v1/listenKey`, the API key alone, no
//!   signature) and opens `/private/ws/<listenKey>`. The key lives 60 min unless kept alive
//!   (`PUT`); a POST on an account that has one answers the same key.
//! - **An order event is the order whole**, in the shape the REST reads answer
//!   ([`VenueOrder`]): the stream and the book name it alike (`SYMBOL:client id`,
//!   `SYMBOL:algo:client id` for a futures stop, which reports on `ALGO_UPDATE`).
//! - **The end of a stream is said.** An expired listen key, a terminated subscription and a
//!   server about to shut down are [`Update::Ended`]: the caller connects again and reads the
//!   book again.

use otw_connector_model::{ConnectorError, CorrId, Decimal, ErrorKind, Side, Timestamp, TriggerKind};
use otw_connector_sdk::parse::OptDec;
use otw_connector_sdk::{HttpRequest, Method, RawFrame, SignableRequest, Signer};
use serde::Deserialize;
use serde_json::Value;

use super::market::{Kind, Market};
use super::trade::{status_of, OrderRef, VenueOrder};
use super::REST;

/// Spot WebSocket API, live and Spot Demo Mode.
const SPOT_LIVE: &str = "wss://ws-api.binance.com:443/ws-api/v3";
const SPOT_DEMO: &str = "wss://demo-ws-api.binance.com/ws-api/v3";
/// USDⓈ-M user data, live and testnet: the `/private` route carries account events, the
/// legacy root no longer does.
const USDM_LIVE: &str = "wss://fstream.binance.com/private/ws/";
const USDM_TESTNET: &str = "wss://stream.binancefuture.com/private/ws/";

const LISTEN_KEY: &str = "/fapi/v1/listenKey";
const SUBSCRIBE: &str = "userDataStream.subscribe.signature";

/// The socket the account's stream comes on. USDⓈ-M names its listen key in the path.
pub fn socket_url(m: &Market, testnet: bool, listen_key: Option<&str>) -> Result<String, ConnectorError> {
    match m.kind {
        Kind::Spot => Ok(if testnet { SPOT_DEMO } else { SPOT_LIVE }.to_string()),
        Kind::UsdM => {
            let key = listen_key.map(str::trim).filter(|k| !k.is_empty()).ok_or_else(|| ConnectorError::invalid("binance futures: the account stream needs a listen key"))?;
            Ok(format!("{}{key}", if testnet { USDM_TESTNET } else { USDM_LIVE }))
        }
    }
}

fn listen_key_call(method: Method, api_key: &str, corr: CorrId) -> HttpRequest {
    let mut h = HttpRequest { method, ..HttpRequest::get("listenKey", REST, LISTEN_KEY, corr) }.weight(1);
    h.headers.push(("X-MBX-APIKEY".into(), api_key.trim().to_string()));
    h
}

/// `POST /fapi/v1/listenKey`: the account's listen key, the active one when it has one.
pub fn start(api_key: &str, corr: CorrId) -> HttpRequest {
    listen_key_call(Method::Post, api_key, corr)
}

/// `PUT /fapi/v1/listenKey`: another 60 min for the account's listen key. `-1125`: it is gone,
/// start a new one.
pub fn keepalive(api_key: &str, corr: CorrId) -> HttpRequest {
    listen_key_call(Method::Put, api_key, corr)
}

pub fn decode_listen_key(body: &[u8]) -> Result<String, ConnectorError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Answer {
        listen_key: String,
    }
    let a: Answer = serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("binance listenKey: {e}")))?;
    if a.listen_key.trim().is_empty() {
        return Err(ConnectorError::decode("binance listenKey: empty key"));
    }
    Ok(a.listen_key)
}

/// The listen key Binance no longer knows (`-1125`).
pub fn listen_key_gone(e: &ConnectorError) -> bool {
    e.provider_code.as_deref() == Some("-1125")
}

/// The spot frame that subscribes this socket to the account of `api_key`, signed by its
/// signer at `now` (the venue clock). Request `id` comes back on the answer.
pub fn subscribe(id: u64, api_key: &str, signer: &dyn Signer, now: Timestamp) -> Result<String, ConnectorError> {
    let mut req = SignableRequest { method: Method::Get, path: String::new(), query: vec![("apiKey".into(), api_key.trim().to_string())], headers: Vec::new(), body: None };
    signer.sign(&mut req, now)?;
    // The WebSocket API checks the signature over the parameters sorted by name: the signer
    // must have written them so.
    let signed: Vec<&str> = req.query.iter().map(|(k, _)| k.as_str()).filter(|k| *k != "signature").collect();
    if !signed.windows(2).all(|w| w[0] < w[1]) || !req.query.iter().any(|(k, _)| k == "signature") {
        return Err(ConnectorError::new(ErrorKind::Auth, "binance: this key's signature cannot subscribe to the account stream"));
    }
    let mut params = serde_json::Map::new();
    for (k, v) in req.query {
        let value = match k.as_str() {
            "timestamp" | "recvWindow" => v.parse::<i64>().map(Value::from).unwrap_or(Value::String(v)),
            _ => Value::String(v),
        };
        params.insert(k, value);
    }
    Ok(serde_json::json!({ "id": id, "method": SUBSCRIBE, "params": params }).to_string())
}

/// An order as the stream now says it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderUpdate {
    pub order: VenueOrder,
    /// This event is a fill (`x = TRADE`).
    pub trade: bool,
    /// Why it stopped working, when Binance says: a rejection, an expiry.
    pub reason: Option<String>,
}

/// A USDⓈ-M position as it now stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionUpdate {
    pub symbol: String,
    /// `BOTH` (one-way mode: the sign says the side), `LONG` or `SHORT` (hedge mode).
    pub side: String,
    /// Signed; zero is flat.
    pub amount: Decimal,
    pub entry: Option<Decimal>,
    pub unrealized: Option<Decimal>,
    /// `isolated` or `cross`.
    pub margin_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Order(Box<OrderUpdate>),
    Position(PositionUpdate),
    /// The WebSocket API's answer to request `id`: the subscription is live, or refused.
    Answer { id: u64, result: Result<(), ConnectorError> },
    /// The stream stopped sending, for the reason given: connect again and read the book.
    Ended(String),
    /// What the book holds no row for: balances, order lists, leverage, margin calls.
    Other,
}

fn decode_err(what: &str, e: impl std::fmt::Display) -> ConnectorError {
    ConnectorError::decode(format!("binance account stream {what}: {e}"))
}

fn side_of(word: &str) -> Result<Side, ConnectorError> {
    match word {
        "BUY" => Ok(Side::Buy),
        "SELL" => Ok(Side::Sell),
        other => Err(decode_err("side", format!("{other:?}"))),
    }
}

/// USDⓈ-M `wt`: the price a conditional order triggers on.
fn trigger_of(wt: Option<&str>) -> Option<TriggerKind> {
    match wt {
        Some("MARK_PRICE") => Some(TriggerKind::Mark),
        Some("CONTRACT_PRICE") => Some(TriggerKind::Last),
        _ => None,
    }
}

fn positive(d: &OptDec) -> Option<Decimal> {
    d.0.filter(|v| *v > Decimal::ZERO)
}

/// `executionReport` (spot).
#[derive(Deserialize)]
struct SpotReport {
    s: String,
    c: String,
    /// The order a cancel names: a cancel report carries the cancel's own id in `c`.
    #[serde(rename = "C", default)]
    orig: Option<String>,
    #[serde(rename = "S")]
    side: String,
    o: String,
    #[serde(default)]
    f: Option<String>,
    q: OptDec,
    p: OptDec,
    #[serde(rename = "P")]
    stop: OptDec,
    #[serde(default)]
    g: Option<i64>,
    x: String,
    #[serde(rename = "X")]
    status: String,
    #[serde(default)]
    r: Option<String>,
    i: i64,
    z: OptDec,
    #[serde(rename = "O", default)]
    created: Option<i64>,
    #[serde(rename = "eR", default)]
    expiry: Option<String>,
}

/// Binance's reject reasons (`r`), in the words of its error messages.
fn spot_reject(r: &str) -> Option<String> {
    Some(
        match r {
            "NONE" | "" => return None,
            "INSUFFICIENT_BALANCES" => "Account has insufficient balance for requested action.",
            "STOP_PRICE_WOULD_TRIGGER_IMMEDIATELY" => "Order would trigger immediately.",
            "WOULD_MATCH_IMMEDIATELY" => "Order would immediately match and take.",
            "OCO_BAD_PRICES" => "The relationship of the prices for the orders is not correct.",
            other => other,
        }
        .to_string(),
    )
}

/// Binance's expiry reasons (`eR`).
fn spot_expiry(e: &str) -> Option<String> {
    Some(
        match e {
            "NONE" | "" => return None,
            "REJECTED" => "a contingent order was rejected when placed on the book",
            "EXCHANGE_CANCELED" => "cancelled by Binance",
            "OCO_TRIGGER" => "the other order of its OCO started working",
            "OTO_PHASE_ONE_EXPIRED" => "the working order of its list expired",
            "UNFILLED_IOC_QUANTITY_EXPIRED" => "IOC not fully filled",
            "UNFILLED_FOK_ORDER_EXPIRED" => "FOK not fully filled",
            "INSUFFICIENT_LIQUIDITY" => "not enough orders in the book to match",
            "EXECUTION_RULE_PRICE_RANGE_EXCEEDED" => "outside the price range execution rule",
            other => other,
        }
        .to_string(),
    )
}

fn spot_order(v: Value) -> Result<OrderUpdate, ConnectorError> {
    let r: SpotReport = serde_json::from_value(v).map_err(|e| decode_err("executionReport", e))?;
    let client_id = r.orig.filter(|c| !c.is_empty()).unwrap_or(r.c);
    let reason = match r.status.as_str() {
        "REJECTED" => r.r.as_deref().and_then(spot_reject),
        "EXPIRED" | "EXPIRED_IN_MATCH" => r.expiry.as_deref().and_then(spot_expiry).or_else(|| (r.x == "TRADE_PREVENTION").then(|| "self-trade prevention".to_string())),
        _ => None,
    };
    Ok(OrderUpdate {
        order: VenueOrder {
            r: OrderRef { symbol: r.s, client_id, algo: false },
            venue_id: r.i.to_string(),
            fills_as: Some(r.i.to_string()),
            status: status_of(&r.status),
            side: side_of(&r.side)?,
            kind: r.o,
            qty: r.q.0.unwrap_or_default(),
            filled: r.z.0.unwrap_or_default(),
            limit: positive(&r.p),
            stop: positive(&r.stop),
            trail_start: None,
            trail_rate: None,
            tif: r.f,
            good_till: None,
            placed_ms: r.created,
            reduce_only: false,
            close_position: false,
            position_side: None,
            trigger: None,
            list: r.g.filter(|g| *g >= 0),
        },
        trade: r.x == "TRADE",
        reason,
    })
}

/// `ORDER_TRADE_UPDATE.o` (USDⓈ-M).
#[derive(Deserialize)]
struct FuturesReport {
    s: String,
    c: String,
    #[serde(rename = "S")]
    side: String,
    o: String,
    #[serde(default)]
    f: Option<String>,
    q: OptDec,
    p: OptDec,
    #[serde(default)]
    sp: OptDec,
    /// Activation price of a trailing stop, whose `sp` means nothing.
    #[serde(rename = "AP", default)]
    activation: OptDec,
    x: String,
    #[serde(rename = "X")]
    status: String,
    i: i64,
    z: OptDec,
    #[serde(rename = "T", default)]
    at: Option<i64>,
    #[serde(rename = "R", default)]
    reduce_only: bool,
    #[serde(default)]
    cp: bool,
    #[serde(default)]
    ps: Option<String>,
    #[serde(default)]
    wt: Option<String>,
    #[serde(default)]
    gtd: Option<i64>,
    #[serde(default)]
    er: Option<String>,
}

/// USDⓈ-M expiry reasons (`er`).
fn futures_expiry(code: &str) -> Option<String> {
    Some(
        match code {
            "0" | "" => return None,
            "1" => "self-trade prevention",
            "2" => "IOC not fully filled",
            "3" => "IOC not fully filled, self-trade prevention",
            "4" => "knocked out by a higher priority reduce-only order, or it would reverse the position",
            "5" => "the account was liquidated",
            "6" => "GTE condition not met",
            "7" => "the symbol was delisted",
            "8" => "the initial order expired after the stop triggered",
            "9" => "market order not fully filled",
            other => return Some(format!("expiry reason {other}")),
        }
        .to_string(),
    )
}

fn futures_order(v: Value) -> Result<OrderUpdate, ConnectorError> {
    let r: FuturesReport = serde_json::from_value(v).map_err(|e| decode_err("ORDER_TRADE_UPDATE", e))?;
    let stop = if r.o == "TRAILING_STOP_MARKET" { positive(&r.activation) } else { positive(&r.sp) };
    let reason = match r.status.as_str() {
        "EXPIRED" | "EXPIRED_IN_MATCH" => r.er.as_deref().and_then(futures_expiry),
        _ => None,
    };
    Ok(OrderUpdate {
        order: VenueOrder {
            r: OrderRef { symbol: r.s, client_id: r.c, algo: false },
            venue_id: r.i.to_string(),
            fills_as: Some(r.i.to_string()),
            status: status_of(&r.status),
            side: side_of(&r.side)?,
            kind: r.o,
            qty: r.q.0.unwrap_or_default(),
            filled: r.z.0.unwrap_or_default(),
            limit: positive(&r.p),
            stop,
            trail_start: None,
            trail_rate: None,
            tif: r.f,
            good_till: r.gtd.filter(|t| *t > 0),
            // The event of a new order is its placing.
            placed_ms: r.at.filter(|_| r.x == "NEW"),
            reduce_only: r.reduce_only,
            close_position: r.cp,
            position_side: r.ps,
            trigger: trigger_of(r.wt.as_deref()),
            list: None,
        },
        trade: r.x == "TRADE",
        reason,
    })
}

/// `ALGO_UPDATE.o` (USDⓈ-M conditional orders).
#[derive(Deserialize)]
struct AlgoReport {
    caid: String,
    aid: i64,
    o: String,
    s: String,
    #[serde(rename = "S")]
    side: String,
    #[serde(default)]
    ps: Option<String>,
    #[serde(default)]
    f: Option<String>,
    q: OptDec,
    #[serde(rename = "X")]
    status: String,
    #[serde(default)]
    aq: OptDec,
    #[serde(default)]
    tp: OptDec,
    #[serde(default)]
    p: OptDec,
    #[serde(default)]
    cp: bool,
    #[serde(rename = "R", default)]
    reduce_only: bool,
    #[serde(default)]
    wt: Option<String>,
    #[serde(default)]
    gtd: Option<i64>,
    #[serde(default)]
    rm: Option<String>,
}

fn algo_order(v: Value, at: Option<i64>) -> Result<OrderUpdate, ConnectorError> {
    let r: AlgoReport = serde_json::from_value(v).map_err(|e| decode_err("ALGO_UPDATE", e))?;
    let reason = match r.status.as_str() {
        "CANCELED" | "REJECTED" | "EXPIRED" => r.rm.filter(|m| !m.trim().is_empty()),
        _ => None,
    };
    Ok(OrderUpdate {
        order: VenueOrder {
            r: OrderRef { symbol: r.s, client_id: r.caid, algo: true },
            venue_id: r.aid.to_string(),
            fills_as: None,
            status: status_of(&r.status),
            side: side_of(&r.side)?,
            kind: r.o,
            qty: r.q.0.unwrap_or_default(),
            filled: r.aq.0.unwrap_or_default(),
            limit: positive(&r.p),
            stop: positive(&r.tp),
            trail_start: None,
            trail_rate: None,
            tif: r.f,
            good_till: r.gtd.filter(|t| *t > 0),
            placed_ms: at.filter(|_| r.status == "NEW"),
            reduce_only: r.reduce_only,
            close_position: r.cp,
            position_side: r.ps,
            trigger: trigger_of(r.wt.as_deref()),
            list: None,
        },
        trade: false,
        reason,
    })
}

/// `ACCOUNT_UPDATE.a.P[]` (USDⓈ-M).
#[derive(Deserialize)]
struct FuturesPosition {
    s: String,
    pa: OptDec,
    #[serde(default)]
    ep: OptDec,
    #[serde(default)]
    up: OptDec,
    #[serde(default)]
    mt: Option<String>,
    #[serde(default)]
    ps: Option<String>,
}

fn positions(v: &Value) -> Result<Vec<Update>, ConnectorError> {
    let Some(list) = v.get("a").and_then(|a| a.get("P")) else { return Ok(Vec::new()) };
    let rows: Vec<FuturesPosition> = serde_json::from_value(list.clone()).map_err(|e| decode_err("ACCOUNT_UPDATE", e))?;
    Ok(rows
        .into_iter()
        .map(|p| {
            Update::Position(PositionUpdate {
                symbol: p.s,
                side: p.ps.unwrap_or_else(|| "BOTH".into()),
                amount: p.pa.0.unwrap_or_default(),
                entry: positive(&p.ep),
                unrealized: p.up.0,
                margin_type: p.mt.filter(|m| !m.is_empty()),
            })
        })
        .collect())
}

/// A WebSocket API answer: `{"id", "status", "result"}` or `{"id", "status", "error"}`.
fn answer(m: &Market, v: &Value) -> Option<Update> {
    let id = v.get("id")?.as_u64()?;
    let status = v.get("status").and_then(Value::as_u64).and_then(|s| u16::try_from(s).ok()).unwrap_or(200);
    let result = match v.get("error") {
        None if status < 400 => Ok(()),
        err => {
            let body = err.map(Value::to_string).unwrap_or_default();
            let frame = RawFrame::Http { corr_id: CorrId(id), endpoint_id: SUBSCRIBE, status: status.max(400), headers: Vec::new(), body: body.into() };
            Err(super::errors::from_frame(&frame, m.name).unwrap_or_else(|| ConnectorError::new(ErrorKind::Unknown, format!("{}: {SUBSCRIBE} refused", m.name))))
        }
    };
    Some(Update::Answer { id, result })
}

/// One text frame of the account socket.
pub fn decode(m: &Market, text: &str) -> Result<Vec<Update>, ConnectorError> {
    let v: Value = serde_json::from_str(text).map_err(|e| decode_err("frame", e))?;
    if v.get("e").is_none() && v.get("event").is_none() {
        return Ok(answer(m, &v).into_iter().collect());
    }
    // The WebSocket API wraps each event (`{"subscriptionId", "event"}`); the futures stream
    // sends it bare.
    let ev = match v.get("event") {
        Some(inner) if inner.is_object() => inner.clone(),
        _ => v,
    };
    let kind = ev.get("e").and_then(Value::as_str).unwrap_or_default().to_string();
    let at = ev.get("T").and_then(Value::as_i64);
    Ok(match kind.as_str() {
        "executionReport" => vec![Update::Order(Box::new(spot_order(ev)?))],
        "ORDER_TRADE_UPDATE" => {
            let o = ev.get("o").cloned().ok_or_else(|| decode_err("ORDER_TRADE_UPDATE", "no order"))?;
            vec![Update::Order(Box::new(futures_order(o)?))]
        }
        "ALGO_UPDATE" => {
            let o = ev.get("o").cloned().ok_or_else(|| decode_err("ALGO_UPDATE", "no order"))?;
            vec![Update::Order(Box::new(algo_order(o, at)?))]
        }
        "ACCOUNT_UPDATE" => positions(&ev)?,
        "listenKeyExpired" => vec![Update::Ended("the listen key expired".into())],
        "eventStreamTerminated" => vec![Update::Ended("Binance ended the subscription".into())],
        "serverShutdown" => vec![Update::Ended("the Binance server is shutting down".into())],
        _ => vec![Update::Other],
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::binance::market::{SPOT, USDM};
    use crate::binance::Binance;
    use otw_connector_model::{Mapped, OrderStatus};
    use otw_connector_sdk::Mapper;
    use std::collections::HashMap;

    fn d(s: &str) -> Decimal {
        Decimal::from_str_exact(s).unwrap()
    }

    fn one(m: &Market, text: &str) -> Update {
        let mut u = decode(m, text).unwrap();
        assert_eq!(u.len(), 1, "{u:?}");
        u.remove(0)
    }

    fn order(u: Update) -> OrderUpdate {
        match u {
            Update::Order(o) => *o,
            other => panic!("not an order: {other:?}"),
        }
    }

    #[test]
    fn hosts_follow_the_environment() {
        assert_eq!(socket_url(&SPOT, false, None).unwrap(), "wss://ws-api.binance.com:443/ws-api/v3");
        assert_eq!(socket_url(&SPOT, true, None).unwrap(), "wss://demo-ws-api.binance.com/ws-api/v3");
        assert_eq!(socket_url(&USDM, false, Some("XaEAKTsQ")).unwrap(), "wss://fstream.binance.com/private/ws/XaEAKTsQ");
        assert_eq!(socket_url(&USDM, true, Some("XaEAKTsQ")).unwrap(), "wss://stream.binancefuture.com/private/ws/XaEAKTsQ");
        assert!(socket_url(&USDM, false, None).is_err());
    }

    #[test]
    fn the_listen_key_is_asked_with_the_api_key_alone() {
        let h = start(" k1 ", CorrId(1));
        assert_eq!((h.method, h.path.as_str(), h.signed, h.weight), (Method::Post, "/fapi/v1/listenKey", false, 1));
        assert_eq!(h.headers, vec![("X-MBX-APIKEY".to_string(), "k1".to_string())]);
        assert_eq!(keepalive("k1", CorrId(2)).method, Method::Put);
        // Source: USDⓈ-M REST spec, startUserDataStream response example.
        let k = decode_listen_key(br#"{"listenKey":"pqia91ma19a5s61cv6a81va65sdf19v8a65a1a5s61cv6a81va65sdf19v8a65a1"}"#).unwrap();
        assert_eq!(k, "pqia91ma19a5s61cv6a81va65sdf19v8a65a1a5s61cv6a81va65sdf19v8a65a1");
        assert!(decode_listen_key(br#"{"listenKey":""}"#).is_err());
    }

    #[test]
    fn the_spot_subscription_is_signed_over_its_sorted_parameters() {
        // Keys from the WebSocket API's HMAC example; the account's own signer (recvWindow 20000).
        let s = HashMap::from([
            ("api_key".to_string(), "vmPUZE6mv9SD5VNHk4HlWFsOr6aKE2zvsw0MuIgwCIPy6utIco14y7Ju91duEh8A".to_string()),
            ("api_secret".to_string(), "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j".to_string()),
        ]);
        let signer = Binance::spot().unwrap().signer(&s).unwrap().unwrap();
        let f = subscribe(7, "vmPUZE6mv9SD5VNHk4HlWFsOr6aKE2zvsw0MuIgwCIPy6utIco14y7Ju91duEh8A", signer.as_ref(), Timestamp::from_millis(1_645_423_376_532).unwrap()).unwrap();
        let v: Value = serde_json::from_str(&f).unwrap();
        assert_eq!(v["method"], "userDataStream.subscribe.signature");
        assert_eq!(v["id"], 7);
        assert_eq!(v["params"]["timestamp"], 1_645_423_376_532_i64);
        assert_eq!(v["params"]["recvWindow"], 20000);
        // openssl dgst -sha256 -hmac <secret> over
        // apiKey=vmPUZE6...&recvWindow=20000&timestamp=1645423376532
        assert_eq!(v["params"]["signature"], "7d75b5f025c8ed2203265489a3dcc501f33fa1472688a1740b678cb4892a63fe");
        assert!(v["params"].get("X-MBX-APIKEY").is_none());
    }

    #[test]
    fn subscription_answers() {
        // Source: web-socket-api.md, userDataStream.subscribe.signature response.
        let ok = r#"{"id":"d3df8a22-98ea-4fe0-9f4e-0fcea5d418b7","status":200,"result":{"subscriptionId":0}}"#;
        assert!(decode(&SPOT, ok).unwrap().is_empty(), "an answer to a request this client did not number is nobody's");
        assert_eq!(one(&SPOT, r#"{"id":3,"status":200,"result":{"subscriptionId":0}}"#), Update::Answer { id: 3, result: Ok(()) });
        // Source: web-socket-api.md, error response shape (-2015).
        let refused = r#"{"id":3,"status":401,"error":{"code":-2015,"msg":"Invalid API-key, IP, or permissions for action."}}"#;
        match one(&SPOT, refused) {
            Update::Answer { id: 3, result: Err(e) } => {
                assert_eq!(e.kind, ErrorKind::Auth);
                assert_eq!(e.provider_code.as_deref(), Some("-2015"));
                assert!(e.message.contains("Invalid API-key"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn spot_execution_report() {
        // Source: user-data-stream.md, Order Update payload (comments removed).
        let text = r#"{"subscriptionId":0,"event":{"e":"executionReport","E":1499405658658,"s":"ETHBTC","c":"mUvoqJxFIILMdfAW5iGSOW","S":"BUY","o":"LIMIT","f":"GTC","q":"1.00000000","p":"0.10264410","P":"0.00000000","F":"0.00000000","g":-1,"C":"","x":"NEW","X":"NEW","r":"NONE","i":4293153,"l":"0.00000000","z":"0.00000000","L":"0.00000000","n":"0","N":null,"T":1499405658657,"t":-1,"v":3,"I":8641984,"w":true,"m":false,"M":false,"O":1499405658657,"Z":"0.00000000","Y":"0.00000000","Q":"0.00000000","W":1499405658657,"V":"NONE","d":4,"D":1668680518494,"j":1,"J":1000000,"A":"3.000000","B":"3.000000","u":1,"U":37,"Cs":"BTCUSDT","pl":"2.123456","pL":"0.10000001","pY":"0.21234562","b":"ONE_PARTY_TRADE_REPORT","a":1234,"k":"SOR","uS":true,"gP":"PRIMARY_PEG","gOT":"PRICE_LEVEL","gOV":5,"gp":"1.00000000","eR":"INSUFFICIENT_LIQUIDITY"}}"#;
        let u = order(one(&SPOT, text));
        assert_eq!(u.order.r.id(), "ETHBTC:mUvoqJxFIILMdfAW5iGSOW");
        assert_eq!(u.order.status, Mapped::Known(OrderStatus::New));
        assert_eq!((u.order.side, u.order.kind.as_str(), u.order.venue_id.as_str()), (Side::Buy, "LIMIT", "4293153"));
        assert_eq!((u.order.qty, u.order.filled, u.order.limit, u.order.stop), (d("1"), d("0"), Some(d("0.1026441")), None));
        assert_eq!((u.order.tif.as_deref(), u.order.placed_ms, u.order.list), (Some("GTC"), Some(1499405658657), None));
        assert!(!u.trade);
        assert_eq!(u.reason, None, "a working order has no end reason");
    }

    #[test]
    fn a_spot_cancel_names_the_order_it_cancelled() {
        // The doc's sample as a cancel answers it: `c` is the cancel's own id, `C` the order's.
        let text = r#"{"subscriptionId":0,"event":{"e":"executionReport","E":1499405658658,"s":"ETHBTC","c":"cancel-9","S":"BUY","o":"LIMIT","f":"GTC","q":"1.00000000","p":"0.10264410","P":"0.00000000","g":-1,"C":"mUvoqJxFIILMdfAW5iGSOW","x":"CANCELED","X":"CANCELED","r":"NONE","i":4293153,"z":"0.00000000","O":1499405658657}}"#;
        let u = order(one(&SPOT, text));
        assert_eq!(u.order.r.id(), "ETHBTC:mUvoqJxFIILMdfAW5iGSOW");
        assert_eq!(u.order.status, Mapped::Known(OrderStatus::Cancelled));
        let fill = text.replace(r#""c":"cancel-9""#, r#""c":"k1""#).replace(r#""C":"mUvoqJxFIILMdfAW5iGSOW","x":"CANCELED","X":"CANCELED""#, r#""C":"","x":"TRADE","X":"PARTIALLY_FILLED""#).replace(r#""z":"0.00000000""#, r#""z":"0.40000000""#);
        let u = order(one(&SPOT, &fill));
        assert_eq!((u.order.r.id().as_str(), u.trade, u.order.filled), ("ETHBTC:k1", true, d("0.4")));
        let rejected = text.replace(r#""x":"CANCELED","X":"CANCELED","r":"NONE""#, r#""x":"REJECTED","X":"REJECTED","r":"INSUFFICIENT_BALANCES""#);
        assert_eq!(order(one(&SPOT, &rejected)).reason.as_deref(), Some("Account has insufficient balance for requested action."));
        let expired = text.replace(r#""x":"CANCELED","X":"CANCELED""#, r#""x":"EXPIRED","X":"EXPIRED","eR":"UNFILLED_IOC_QUANTITY_EXPIRED""#);
        assert_eq!(order(one(&SPOT, &expired)).reason.as_deref(), Some("IOC not fully filled"));
    }

    #[test]
    fn spot_events_the_book_holds_no_row_for() {
        // Source: user-data-stream.md payloads (comments removed).
        for text in [
            r#"{"subscriptionId":0,"event":{"e":"outboundAccountPosition","E":1564034571105,"u":1564034571073,"B":[{"a":"ETH","f":"10000.000000","l":"0.000000"}]}}"#,
            r#"{"subscriptionId":0,"event":{"e":"balanceUpdate","E":1573200697110,"a":"BTC","d":"100.00000000","T":1573200697068}}"#,
            r#"{"subscriptionId":0,"event":{"e":"listStatus","E":1564035303637,"s":"ETHBTC","g":2,"c":"OCO","l":"EXEC_STARTED","L":"EXECUTING","r":"NONE","C":"F4QN4G8DlFATFlIUQ0cjdD","T":1564035303625,"O":[{"s":"ETHBTC","i":17,"c":"AJYsMjErWJesZvqlJCTUgL"},{"s":"ETHBTC","i":18,"c":"bfYPSQdLoqAJeNrOr9adzq"}]}}"#,
            r#"{"subscriptionId":0,"event":{"e":"externalLockUpdate","E":1581557507324,"a":"NEO","d":"10.00000000","T":1581557507268}}"#,
        ] {
            assert_eq!(one(&SPOT, text), Update::Other, "{text}");
        }
        // Source: user-data-stream.md (eventStreamTerminated), web-socket-api.md (serverShutdown).
        assert!(matches!(one(&SPOT, r#"{"subscriptionId":0,"event":{"e":"eventStreamTerminated","E":1728973001334}}"#), Update::Ended(_)));
        assert!(matches!(one(&SPOT, r#"{"event":{"e":"serverShutdown","E":1770123456789}}"#), Update::Ended(_)));
    }

    #[test]
    fn futures_order_trade_update() {
        // Source: USDⓈ-M ws-streams schema `orderTradeUpdate`, its example values.
        let text = r#"{"e":"ORDER_TRADE_UPDATE","E":1568879465651,"T":1568879465650,"o":{"s":"BTCUSDT","c":"TEST","S":"SELL","o":"TRAILING_STOP_MARKET","f":"GTC","q":"0.001","p":"0","ap":"0","sp":"7103.04","x":"NEW","X":"NEW","i":8886774,"l":"0","z":"0","L":"0","N":"USDT","n":"0","T":1568879465650,"t":0,"b":"0","a":"9.91","m":false,"R":false,"wt":"CONTRACT_PRICE","ot":"TRAILING_STOP_MARKET","ps":"LONG","cp":false,"AP":"7476.89","cr":"5.0","pP":false,"si":0,"ss":0,"rp":"0","V":"EXPIRE_TAKER","pm":"OPPONENT","gtd":0,"er":"0"}}"#;
        let u = order(one(&USDM, text));
        assert_eq!(u.order.r.id(), "BTCUSDT:TEST");
        assert_eq!((u.order.status.clone(), u.order.side, u.order.kind.as_str()), (Mapped::Known(OrderStatus::New), Side::Sell, "TRAILING_STOP_MARKET"));
        assert_eq!((u.order.qty, u.order.limit, u.order.stop), (d("0.001"), None, Some(d("7476.89"))), "a trailing stop is shown at its activation price");
        assert_eq!((u.order.placed_ms, u.order.good_till, u.order.position_side.as_deref()), (Some(1568879465650), None, Some("LONG")));
        assert!(!u.trade && !u.order.reduce_only && !u.order.close_position);
        assert_eq!(u.order.trigger, Some(TriggerKind::Last));
        let filled = text.replace(r#""x":"NEW","X":"NEW""#, r#""x":"TRADE","X":"FILLED""#).replace(r#""z":"0""#, r#""z":"0.001""#);
        let u = order(one(&USDM, &filled));
        assert_eq!((u.trade, u.order.status, u.order.filled, u.order.placed_ms), (true, Mapped::Known(OrderStatus::Filled), d("0.001"), None));
        let expired = text.replace(r#""x":"NEW","X":"NEW""#, r#""x":"EXPIRED","X":"EXPIRED""#).replace(r#""er":"0""#, r#""er":"2""#);
        assert_eq!(order(one(&USDM, &expired)).reason.as_deref(), Some("IOC not fully filled"));
    }

    #[test]
    fn futures_algo_update() {
        // Source: USDⓈ-M ws-streams schema `algoUpdate`, its example values.
        let text = r#"{"e":"ALGO_UPDATE","T":1750515742297,"E":1750515742303,"o":{"caid":"Q5xaq5EGKgXXa0fD7fs0Ip","aid":2148719,"at":"CONDITIONAL","o":"TAKE_PROFIT","s":"BNBUSDT","S":"SELL","ps":"BOTH","f":"GTC","q":"0.01","X":"CANCELED","ai":"","ap":"0.00000","aq":"0.00000","act":"0","tp":"750","p":"750","V":"EXPIRE_MAKER","wt":"CONTRACT_PRICE","pm":"NONE","cp":false,"pP":false,"R":false,"tt":0,"gtd":0,"rm":"Reduce Only reject","ia":false}}"#;
        let u = order(one(&USDM, text));
        assert_eq!(u.order.r.id(), "BNBUSDT:algo:Q5xaq5EGKgXXa0fD7fs0Ip", "a stop is named in the algo book");
        assert_eq!((u.order.status.clone(), u.order.venue_id.as_str()), (Mapped::Known(OrderStatus::Cancelled), "2148719"));
        assert_eq!((u.order.limit, u.order.stop, u.order.qty), (Some(d("750")), Some(d("750")), d("0.01")));
        assert_eq!(u.reason.as_deref(), Some("Reduce Only reject"));
        let new = text.replace(r#""X":"CANCELED""#, r#""X":"NEW""#);
        let u = order(one(&USDM, &new));
        assert_eq!((u.order.status, u.order.placed_ms, u.reason), (Mapped::Known(OrderStatus::New), Some(1750515742297), None));
        let fired = text.replace(r#""X":"CANCELED""#, r#""X":"TRIGGERED""#);
        assert_eq!(order(one(&USDM, &fired)).order.status, Mapped::Known(OrderStatus::Filled), "as the REST read says it: what fired is an order of its own");
    }

    #[test]
    fn futures_account_update_and_expiry() {
        // Source: USDⓈ-M ws-streams schema `accountUpdate`, its example values.
        let text = r#"{"e":"ACCOUNT_UPDATE","E":1564745798939,"T":1564745798938,"a":{"m":"ORDER","B":[{"a":"USDT","wb":"122624.12345678","cw":"100.12345678","bc":"50.12345678"}],"P":[{"s":"BTCUSDT","pa":"0","ep":"0.00000","bep":"0","cr":"200","up":"0","mt":"isolated","iw":"0.00000000","ps":"BOTH"}]}}"#;
        let p = match one(&USDM, text) {
            Update::Position(p) => p,
            other => panic!("{other:?}"),
        };
        assert_eq!(p, PositionUpdate { symbol: "BTCUSDT".into(), side: "BOTH".into(), amount: d("0"), entry: None, unrealized: Some(d("0")), margin_type: Some("isolated".into()) });
        let two = text.replace(r#""ps":"BOTH"}]"#, r#""ps":"BOTH"},{"s":"ETHUSDT","pa":"-0.5","ep":"2500.5","up":"-1.25","mt":"cross","ps":"BOTH"}]"#);
        assert_eq!(decode(&USDM, &two).unwrap().len(), 2);
        // A funding fee on a cross position: a balance, no position.
        let fee = r#"{"e":"ACCOUNT_UPDATE","E":1564745798939,"T":1564745798938,"a":{"m":"FUNDING_FEE","B":[{"a":"USDT","wb":"122624.12345678","cw":"100.12345678","bc":"0"}],"S":"BTCUSDT"}}"#;
        assert!(decode(&USDM, fee).unwrap().is_empty());
        // Source: USDⓈ-M ws-streams schema `listenKeyExpired`, its example values.
        let expired = r#"{"e":"listenKeyExpired","E":1736996475556,"listenKey":"test-listen-key"}"#;
        assert!(matches!(one(&USDM, expired), Update::Ended(_)));
        assert_eq!(one(&USDM, r#"{"e":"MARGIN_CALL","E":1587727187525}"#), Update::Other);
        assert!(decode(&USDM, "not json").is_err());
    }
}
