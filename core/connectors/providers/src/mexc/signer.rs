//! MEXC signatures, one scheme per API.
//!
//!   - Spot (`api.mexc.com`): Binance's scheme. Lowercase hex HMAC-SHA256 of the query
//!     string (with `recvWindow` and `timestamp` in milliseconds), appended as `signature`;
//!     the key in `X-MEXC-APIKEY`. The SDK's [`HmacQuerySigner`].
//!   - Contract (`contract.mexc.com`): lowercase hex HMAC-SHA256 of
//!     `key + Request-Time + query`, the query's parameters sorted by name, sent as the
//!     `ApiKey`, `Request-Time` (milliseconds) and `Signature` headers. The parameters travel
//!     in the order signed.

use std::collections::HashMap;

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{encode_query, hmac_sha256, HmacQuerySigner, SignableRequest};
use otw_connector_sdk::Signer;
use secrecy::{ExposeSecret, SecretString};

/// Spot `recvWindow`: how long after its timestamp a call is accepted. The venue allows
/// 60 s at most.
const RECV_WINDOW_MS: u64 = 10_000;

fn secret(secrets: &HashMap<String, String>, name: &str) -> Result<SecretString, ConnectorError> {
    secrets
        .get(name)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| SecretString::from(v.to_string()))
        .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
}

pub fn spot(secrets: &HashMap<String, String>) -> Result<HmacQuerySigner, ConnectorError> {
    Ok(HmacQuerySigner {
        api_key: secret(secrets, "api_key")?,
        secret: secret(secrets, "api_secret")?,
        key_header: "X-MEXC-APIKEY",
        timestamp_param: "timestamp",
        signature_param: "signature",
        recv_window: Some(("recvWindow", RECV_WINDOW_MS)),
    })
}

pub struct ContractSigner {
    key: SecretString,
    secret: SecretString,
}

impl ContractSigner {
    pub fn from_secrets(secrets: &HashMap<String, String>) -> Result<Self, ConnectorError> {
        Ok(ContractSigner { key: secret(secrets, "api_key")?, secret: secret(secrets, "api_secret")? })
    }
}

/// What the contract signature covers on a GET or DELETE: the key, the time, the sorted query.
pub fn contract_prehash(key: &str, ts_ms: i64, query: &[(String, String)]) -> String {
    format!("{key}{ts_ms}{}", encode_query(query))
}

/// What it covers on a POST: the key, the time, the JSON body as sent ("For POST requests, the
/// parameters to sign are the JSON string (no dictionary sorting required)").
pub fn contract_body_prehash(key: &str, ts_ms: i64, body: &[u8]) -> String {
    format!("{key}{ts_ms}{}", String::from_utf8_lossy(body))
}

/// The plan (trigger) order list, whose `start_time` and `end_time` are required ("Start time,
/// Unix millisecond timestamp", no maximum span stated).
pub const PLAN_LIST_PATH: &str = "/api/v1/private/planorder/list/orders";

/// The plan list's window, back from the signing time: a plan order lives 7 days at most
/// (`executeCycle` "1: 24 hours, 2: 7 days"), and a day more covers the two clocks.
pub const PLAN_WINDOW_MS: i64 = 8 * 86_400_000;

/// The plan list's window, written at signing time (the one place the clock is at hand) when
/// the call names none.
fn plan_window(req: &mut SignableRequest, ts_ms: i64) {
    if req.path != PLAN_LIST_PATH || req.query.iter().any(|(k, _)| k == "start_time" || k == "end_time") {
        return;
    }
    req.query.push(("start_time".into(), (ts_ms - PLAN_WINDOW_MS).to_string()));
    req.query.push(("end_time".into(), ts_ms.to_string()));
}

impl Signer for ContractSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let ts = now.millis();
        let key = self.key.expose_secret();
        plan_window(req, ts);
        let prehash = match &req.body {
            // A write: the exact bytes sent. Its parameters travel in the body only.
            Some(b) => contract_body_prehash(key, ts, b),
            None => {
                req.query.sort();
                contract_prehash(key, ts, &req.query)
            }
        };
        let sig = hex::encode(hmac_sha256(self.secret.expose_secret().as_bytes(), prehash.as_bytes())?);
        req.headers.push(("ApiKey".into(), key.to_string()));
        req.headers.push(("Request-Time".into(), ts.to_string()));
        req.headers.push(("Signature".into(), sig));
        // The one Content-Type of the call: the mapper writes none.
        req.headers.push(("Content-Type".into(), "application/json".into()));
        Ok(())
    }

    /// The futures socket's login: "The signature target string is: accessKey + timestamp",
    /// HMAC-SHA256 hex, sent as `{"method":"login","param":{"apiKey","reqTime","signature"}}`.
    fn ws_login(&self, now: Timestamp) -> Result<Option<bytes::Bytes>, ConnectorError> {
        let ts = now.millis();
        let key = self.key.expose_secret();
        let sig = hex::encode(hmac_sha256(self.secret.expose_secret().as_bytes(), login_prehash(key, ts).as_bytes())?);
        let frame = serde_json::json!({ "method": "login", "param": { "apiKey": key, "reqTime": ts.to_string(), "signature": sig } });
        Ok(Some(bytes::Bytes::from(frame.to_string())))
    }
}

/// The futures socket login's signed string: the key, then the time in milliseconds.
pub fn login_prehash(key: &str, ts_ms: i64) -> String {
    format!("{key}{ts_ms}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_sdk::Method;

    fn secrets(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn header(req: &SignableRequest, k: &str) -> Option<String> {
        req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    #[test]
    fn spot_signs_like_the_docs_example() {
        // MEXC's own worked example (spot v3 docs, "SIGNED Endpoint Examples", example 2),
        // with their key, secret and expected signature. Example 1 prints another signature
        // for the same string; Python's hmac agrees with example 2.
        let s = HmacQuerySigner { recv_window: Some(("recvWindow", 5000)), ..spot(&secrets(&[("api_key", "mx0aBYs33eIilxBWC5"), ("api_secret", "45d0b3c26f2644f19bfb98b07741b2f5")])).unwrap() }; // gitleaks:allow
        let q = [("symbol", "BTCUSDT"), ("side", "BUY"), ("type", "LIMIT"), ("quantity", "1"), ("price", "11")];
        let mut req = SignableRequest { method: Method::Post, path: "/api/v3/order".into(), query: q.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), headers: vec![], body: None };
        s.sign(&mut req, Timestamp::from_millis(1_644_489_390_087).unwrap()).unwrap();
        let sig = req.query.iter().find(|(k, _)| k == "signature").map(|(_, v)| v.as_str());
        assert_eq!(sig, Some("fd3e4e8543c5188531eb7279d68ae7d26a573d0fc5ab0d18eb692451654d837a"));
        assert_eq!(header(&req, "X-MEXC-APIKEY").as_deref(), Some("mx0aBYs33eIilxBWC5"));
    }

    #[test]
    fn contract_signs_the_sorted_query_after_key_and_time() {
        let s = ContractSigner::from_secrets(&secrets(&[("api_key", "mx0key"), ("api_secret", "secret-test")])).unwrap();
        let q = vec![("symbol".to_string(), "BTC_USDT".to_string()), ("page_size".to_string(), "100".to_string()), ("page_num".to_string(), "1".to_string())];
        let mut req = SignableRequest { method: Method::Get, path: "/api/v1/private/order/list/order_deals".into(), query: q, headers: vec![], body: None };
        s.sign(&mut req, Timestamp::from_millis(1_792_392_000_000).unwrap()).unwrap();
        assert_eq!(req.query.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["page_num", "page_size", "symbol"], "sent as signed");
        assert_eq!(contract_prehash("mx0key", 1_792_392_000_000, &req.query), "mx0key1792392000000page_num=1&page_size=100&symbol=BTC_USDT");
        // Reference computed with an independent implementation (Python's hmac and hashlib,
        // the way ccxt's mexc.sign builds it).
        assert_eq!(header(&req, "Signature").as_deref(), Some("df387a5b63ddba33e06c87e9ed603738ba0e1eaebdd440f4b4c58a36bcec4693"));
        assert_eq!(header(&req, "Request-Time").as_deref(), Some("1792392000000"));
    }

    #[test]
    fn spot_signs_a_query_and_a_body_concatenated_like_example_3() {
        // Spot v3 docs, "SIGNED Endpoint Examples", Example 3 (mixed query string and request
        // body): totalParams is the query string then the body, "There is no & between
        // "LIMIT" and "quantity=1"". The doc's own key, secret and signature.
        let total = "symbol=BTCUSDT&side=BUY&type=LIMITquantity=1&price=11&recvWindow=5000&timestamp=1644489390087";
        let sig = hex::encode(hmac_sha256(b"45d0b3c26f2644f19bfb98b07741b2f5", total.as_bytes()).unwrap()); // gitleaks:allow
        assert_eq!(sig, "d1a676610ceb39174c8039b3f548357994b2a34139a8addd33baadba65684592");
        // The signer builds totalParams the same way: query (with its window and time), then
        // the body, nothing between.
        let s = HmacQuerySigner { recv_window: None, ..spot(&secrets(&[("api_key", "mx0aBYs33eIilxBWC5"), ("api_secret", "45d0b3c26f2644f19bfb98b07741b2f5")])).unwrap() }; // gitleaks:allow
        let mut req = SignableRequest {
            method: Method::Post,
            path: "/api/v3/order".into(),
            query: vec![],
            headers: vec![],
            body: Some(bytes::Bytes::from_static(b"symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=1&price=11")),
        };
        s.sign(&mut req, Timestamp::from_millis(1_644_489_390_087).unwrap()).unwrap();
        let want = hex::encode(hmac_sha256(b"45d0b3c26f2644f19bfb98b07741b2f5", b"timestamp=1644489390087symbol=BTCUSDT&side=BUY&type=LIMIT&quantity=1&price=11").unwrap()); // gitleaks:allow
        assert_eq!(req.query.iter().find(|(k, _)| k == "signature").map(|(_, v)| v.clone()), Some(want));
    }

    #[test]
    fn contract_signs_a_post_body_as_sent_with_one_content_type() {
        let s = ContractSigner::from_secrets(&secrets(&[("api_key", "mx0key"), ("api_secret", "secret-test")])).unwrap();
        let body = bytes::Bytes::from_static(b"[739113577038255616]");
        let mut req = SignableRequest { method: Method::Post, path: "/api/v1/private/order/cancel".into(), query: vec![], headers: vec![], body: Some(body) };
        s.sign(&mut req, Timestamp::from_millis(1_792_392_000_000).unwrap()).unwrap();
        assert_eq!(contract_body_prehash("mx0key", 1_792_392_000_000, b"[739113577038255616]"), "mx0key1792392000000[739113577038255616]");
        // Python's hmac and hashlib over the same string.
        assert_eq!(header(&req, "Signature").as_deref(), Some("396a24e6e693ae3a891505f68cd03acd5c86913af1c2ea704dacbc778af0d872"));
        assert_eq!(req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).count(), 1, "one Content-Type");
        assert!(req.query.is_empty());
    }

    #[test]
    fn the_futures_socket_login_signs_key_and_time() {
        // Futures websocket docs, "Signature string": the target for key `mx0aBYs33eIilxBW5C`
        // at 1657186536762 is "mx0aBYs33eIilxBW5C1657186536762" (no signature is printed).
        assert_eq!(login_prehash("mx0aBYs33eIilxBW5C", 1_657_186_536_762), "mx0aBYs33eIilxBW5C1657186536762");
        let s = ContractSigner::from_secrets(&secrets(&[("api_key", "mx0key"), ("api_secret", "secret-test")])).unwrap();
        let login = s.ws_login(Timestamp::from_millis(1_792_392_000_000).unwrap()).unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&login).unwrap();
        assert_eq!(v["method"], "login");
        assert_eq!((v["param"]["apiKey"].as_str(), v["param"]["reqTime"].as_str()), (Some("mx0key"), Some("1792392000000")));
        // Python's hmac and hashlib over "mx0key1792392000000".
        assert_eq!(v["param"]["signature"].as_str(), Some("fb009d2e8ad457efe8b992e4f981981d2ca94290e4913c7f11c88c4693ea5e60"));
    }

    /// The plan order list gets its required window, the last 8 days to the signing time,
    /// signed with the rest; a window the call names is kept, and no other call gets one.
    #[test]
    fn the_plan_list_is_signed_with_its_required_window() {
        let s = ContractSigner::from_secrets(&secrets(&[("api_key", "mx0key"), ("api_secret", "secret-test")])).unwrap();
        let now = 1_792_392_000_000_i64;
        let q = |pairs: &[(&str, &str)]| pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        let mut req = SignableRequest { method: Method::Get, path: PLAN_LIST_PATH.into(), query: q(&[("states", "1"), ("page_num", "1"), ("page_size", "100")]), headers: vec![], body: None };
        s.sign(&mut req, Timestamp::from_millis(now).unwrap()).unwrap();
        let get = |r: &SignableRequest, k: &str| r.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!((get(&req, "start_time"), get(&req, "end_time")), (Some((now - 8 * 86_400_000).to_string()), Some(now.to_string())));
        assert_eq!(contract_prehash("mx0key", now, &req.query), format!("mx0key{now}end_time={now}&page_num=1&page_size=100&start_time={}&states=1", now - 8 * 86_400_000));
        let mut named = SignableRequest { method: Method::Get, path: PLAN_LIST_PATH.into(), query: q(&[("start_time", "5"), ("end_time", "6")]), headers: vec![], body: None };
        s.sign(&mut named, Timestamp::from_millis(now).unwrap()).unwrap();
        assert_eq!(named.query.len(), 2);
        let mut other = SignableRequest { method: Method::Get, path: "/api/v1/private/stoporder/open_orders".into(), query: vec![], headers: vec![], body: None };
        s.sign(&mut other, Timestamp::from_millis(now).unwrap()).unwrap();
        assert!(other.query.is_empty());
    }

    #[test]
    fn a_missing_secret_is_named() {
        let e = ContractSigner::from_secrets(&secrets(&[("api_key", "k")])).err().unwrap();
        assert!(e.message.contains("api_secret"));
    }
}
