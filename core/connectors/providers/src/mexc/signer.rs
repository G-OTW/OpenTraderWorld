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

/// What the contract signature covers: the key, the time, the sorted query (a body for a
/// write, which this connector never sends).
pub fn contract_prehash(key: &str, ts_ms: i64, query: &[(String, String)]) -> String {
    format!("{key}{ts_ms}{}", encode_query(query))
}

impl Signer for ContractSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        req.query.sort();
        let ts = now.millis();
        let key = self.key.expose_secret();
        let sig = hex::encode(hmac_sha256(self.secret.expose_secret().as_bytes(), contract_prehash(key, ts, &req.query).as_bytes())?);
        req.headers.push(("ApiKey".into(), key.to_string()));
        req.headers.push(("Request-Time".into(), ts.to_string()));
        req.headers.push(("Signature".into(), sig));
        req.headers.push(("Content-Type".into(), "application/json".into()));
        Ok(())
    }
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
    fn a_missing_secret_is_named() {
        let e = ContractSigner::from_secrets(&secrets(&[("api_key", "k")])).err().unwrap();
        assert!(e.message.contains("api_secret"));
    }
}
