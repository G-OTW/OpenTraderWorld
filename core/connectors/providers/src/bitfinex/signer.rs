//! Bitfinex signs every authenticated call the same way (REST v2, all on POST):
//! `bfx-signature` is the lowercase hex HMAC-SHA384, keyed with the API secret, of
//! `/api` + path + nonce + body, where the body is the JSON exactly as sent (`{}` when the
//! call takes no parameter). The key rides `bfx-apikey`, the nonce `bfx-nonce`.
//!
//! The nonce is not a clock check: Bitfinex only asks that it strictly increase per key and
//! stay below 2^53 (`MAX_SAFE_INTEGER`). It is microseconds, as Bitfinex's JavaScript example
//! and its own libraries write it, so a key once used by a client counting milliseconds
//! still works here; a process-wide counter keeps two calls in one microsecond apart.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::{SignableRequest, Signer};
use secrecy::{ExposeSecret, SecretString};

/// What Bitfinex accepts as a nonce at most (2^53 - 1).
const MAX_NONCE: i64 = 9_007_199_254_740_991;

fn secret(secrets: &HashMap<String, String>, name: &str) -> Result<SecretString, ConnectorError> {
    secrets
        .get(name)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| SecretString::from(v.to_string()))
        .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
}

/// Strictly increasing whatever the clock does: Bitfinex refuses a nonce not above the last
/// one it saw for the key (10114, "nonce: small").
pub fn nonce(now_us: i64) -> i64 {
    static LAST: AtomicI64 = AtomicI64::new(0);
    // `fetch_update` hands back the previous value, so the nonce used is recomputed from it.
    let prev = LAST.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |p| Some(now_us.max(p.saturating_add(1)))).unwrap_or(0);
    now_us.max(prev.saturating_add(1))
}

/// What the signature covers.
pub fn prehash(path: &str, nonce: &str, body: &str) -> String {
    format!("/api{path}{nonce}{body}")
}

/// Lowercase hex HMAC-SHA384.
pub fn sign(secret: &[u8], payload: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA384, secret);
    hex::encode(ring::hmac::sign(&key, payload.as_bytes()).as_ref())
}

pub struct BitfinexSigner {
    key: SecretString,
    secret: SecretString,
}

impl BitfinexSigner {
    pub fn from_secrets(secrets: &HashMap<String, String>) -> Result<Self, ConnectorError> {
        Ok(BitfinexSigner { key: secret(secrets, "api_key")?, secret: secret(secrets, "api_secret")? })
    }
}

impl Signer for BitfinexSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let n = nonce(now.nanos().div_euclid(1_000));
        if n > MAX_NONCE {
            return Err(ConnectorError::new(ErrorKind::Auth, "Bitfinex: the nonce went past the largest the venue accepts"));
        }
        let n = n.to_string();
        let body = match req.body.as_ref().filter(|b| !b.is_empty()) {
            Some(b) => String::from_utf8_lossy(b).into_owned(),
            None => "{}".to_string(),
        };
        let signature = sign(self.secret.expose_secret().as_bytes(), &prehash(&req.path, &n, &body));
        req.body = Some(body.into());
        req.headers.push(("bfx-nonce".into(), n));
        req.headers.push(("bfx-apikey".into(), self.key.expose_secret().to_string()));
        req.headers.push(("bfx-signature".into(), signature));
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
    fn signs_path_nonce_and_body_with_hmac_sha384() {
        // References computed with an independent implementation (Python's hmac and
        // hashlib, the way Bitfinex's own Python example builds the message).
        let body = r#"{"start":1791000000000,"end":1792399999999,"limit":1000,"sort":-1}"#;
        let msg = prehash("/v2/auth/r/trades/hist", "1792400000000000", body);
        assert_eq!(msg, format!("/api/v2/auth/r/trades/hist1792400000000000{body}"));
        assert_eq!(sign(b"bfx-test-secret", &msg), "579ff261c966b144654e75e029bdc33534d2dc4176e1777fa2df6fb3d66c8e303bd179d4500517c28324665c1155cf78");
        assert_eq!(
            sign(b"bfx-test-secret", &prehash("/v2/auth/r/wallets", "1792400000000000", "{}")),
            "541dbeeda6f19f59931bb7bd47b690c117792a36968a304b5c2c7ee249f870550157936dd80158e8bb4563975fedfed9"
        );
    }

    #[test]
    fn a_call_without_parameters_signs_and_sends_an_empty_object() {
        let s = BitfinexSigner::from_secrets(&secrets(&[("api_key", "bfx-key"), ("api_secret", "bfx-test-secret")])).unwrap();
        let mut req = SignableRequest { method: Method::Post, path: "/v2/auth/r/wallets".into(), query: vec![], headers: vec![], body: None };
        s.sign(&mut req, Timestamp::from_millis(1_792_400_000_000).unwrap()).unwrap();
        assert_eq!(req.body.as_deref(), Some(&b"{}"[..]));
        let n = header(&req, "bfx-nonce").unwrap();
        assert!(n.parse::<i64>().unwrap() >= 1_792_400_000_000_000, "microseconds: {n}");
        assert_eq!(header(&req, "bfx-signature").unwrap(), sign(b"bfx-test-secret", &prehash("/v2/auth/r/wallets", &n, "{}")));
        assert_eq!(header(&req, "bfx-apikey").as_deref(), Some("bfx-key"));
        assert_eq!(header(&req, "Content-Type").as_deref(), Some("application/json"));
    }

    #[test]
    fn a_nonce_never_repeats_even_inside_one_microsecond() {
        let mut last = 0;
        for _ in 0..1000 {
            let n = nonce(1_792_400_000_000_000);
            assert!(n > last);
            last = n;
        }
    }

    #[test]
    fn a_missing_secret_is_named() {
        let e = BitfinexSigner::from_secrets(&secrets(&[("api_key", "k")])).err().unwrap();
        assert!(e.message.contains("api_secret"));
    }
}
