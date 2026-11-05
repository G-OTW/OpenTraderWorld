//! Bybit v5 request signature over `timestamp + api_key + recv_window + (query string |
//! body)`, sent in headers with the key, the millisecond timestamp and the receive window.
//!
//! Two kinds of key: a key Bybit generates signs with HMAC-SHA256 (lowercase hex); a key
//! the user generated signs with RSA-SHA256 PKCS#1 v1.5 (base64), the private key pasted as
//! the secret in PEM (PKCS#8 `PRIVATE KEY` or PKCS#1 `RSA PRIVATE KEY`).

use std::collections::HashMap;

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{encode_query, hmac_sha256, SignableRequest};
use otw_connector_sdk::{Method, Signer};
use secrecy::{ExposeSecret, SecretString};

/// How long after its timestamp Bybit still accepts a request (its documented default).
pub const RECV_WINDOW_MS: u64 = 5000;

enum Key {
    Hmac(SecretString),
    Rsa(Box<ring::signature::RsaKeyPair>),
}

pub struct BybitSigner {
    key: SecretString,
    secret: Key,
}

pub fn prehash(ts_ms: i64, key: &str, recv_window: u64, method: Method, query: &str, body: &str) -> String {
    // A GET signs its query string, anything else its JSON body.
    let payload = if method == Method::Get { query } else { body };
    format!("{ts_ms}{key}{recv_window}{payload}")
}

fn auth(msg: impl Into<String>) -> ConnectorError {
    ConnectorError::new(ErrorKind::Auth, msg)
}

/// The DER inside a PEM block, and whether it is PKCS#1 (`RSA PRIVATE KEY`).
fn pem_der(pem: &str) -> Result<(Vec<u8>, bool), ConnectorError> {
    use base64::Engine as _;
    let pkcs1 = pem.contains("BEGIN RSA PRIVATE KEY");
    if !pkcs1 && !pem.contains("BEGIN PRIVATE KEY") {
        return Err(auth("the Bybit RSA secret must be the whole private key in PEM, its BEGIN and END lines included"));
    }
    let body: String = pem.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(body).map_err(|_| auth("the Bybit RSA private key is not valid PEM (base64)"))?;
    Ok((der, pkcs1))
}

impl BybitSigner {
    pub fn from_secrets(secrets: &HashMap<String, String>) -> Result<Self, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .ok_or_else(|| auth(format!("{name} is not set on this broker account")))
        };
        let key = SecretString::from(get("api_key")?);
        let raw = get("api_secret")?;
        let secret = if raw.contains("-----BEGIN") {
            let (der, pkcs1) = pem_der(&raw)?;
            let pair = if pkcs1 { ring::signature::RsaKeyPair::from_der(&der) } else { ring::signature::RsaKeyPair::from_pkcs8(&der) };
            Key::Rsa(Box::new(pair.map_err(|e| auth(format!("the Bybit RSA private key is refused ({e}): it must be an RSA key of 2048 to 4096 bits")))?))
        } else {
            Key::Hmac(SecretString::from(raw))
        };
        Ok(BybitSigner { key, secret })
    }

    fn signature(&self, pre: &str) -> Result<String, ConnectorError> {
        match &self.secret {
            Key::Hmac(s) => Ok(hex::encode(hmac_sha256(s.expose_secret().as_bytes(), pre.as_bytes())?)),
            Key::Rsa(pair) => {
                use base64::Engine as _;
                let mut sig = vec![0; pair.public().modulus_len()];
                // PKCS#1 v1.5 is deterministic: the generator is never drawn from.
                pair.sign(&ring::signature::RSA_PKCS1_SHA256, &ring::rand::SystemRandom::new(), pre.as_bytes(), &mut sig)
                    .map_err(|_| auth("the Bybit RSA key could not sign the request"))?;
                Ok(base64::engine::general_purpose::STANDARD.encode(sig))
            }
        }
    }
}

impl Signer for BybitSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let ts = now.millis();
        let key = self.key.expose_secret();
        let body = req.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let pre = prehash(ts, key, RECV_WINDOW_MS, req.method, &encode_query(&req.query), &body);
        let sign = self.signature(&pre)?;
        req.headers.push(("X-BAPI-API-KEY".into(), key.to_string()));
        req.headers.push(("X-BAPI-TIMESTAMP".into(), ts.to_string()));
        req.headers.push(("X-BAPI-RECV-WINDOW".into(), RECV_WINDOW_MS.to_string()));
        req.headers.push(("X-BAPI-SIGN".into(), sign));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn signed(secret: &str) -> SignableRequest {
        let secrets: HashMap<String, String> = [("api_key", "XXXXXXXXXX"), ("api_secret", secret)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let s = BybitSigner::from_secrets(&secrets).unwrap();
        let mut req = SignableRequest {
            method: Method::Get,
            path: "/v5/order/realtime".into(),
            query: vec![("category".into(), "option".into()), ("symbol".into(), "BTC-29JUL22-25000-C".into())],
            headers: vec![],
            body: None,
        };
        s.sign(&mut req, Timestamp::from_millis(1_658_384_314_791).unwrap()).unwrap();
        req
    }

    fn header(req: &SignableRequest, k: &str) -> Option<String> {
        req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    #[test]
    fn signs_the_documented_plain_text() {
        // Bybit's own worked example, from the v5 integration guide.
        assert_eq!(
            prehash(1_658_384_314_791, "XXXXXXXXXX", 5000, Method::Get, "category=option&symbol=BTC-29JUL22-25000-C", ""),
            "1658384314791XXXXXXXXXX5000category=option&symbol=BTC-29JUL22-25000-C"
        );
        assert_eq!(prehash(1_658_385_579_423, "XXXXXXXXXX", 5000, Method::Post, "", r#"{"category": "option"}"#), r#"1658385579423XXXXXXXXXX5000{"category": "option"}"#);
        let req = signed("secret-test");
        // Reference computed by Bybit's official Python SDK (pybit `generate_signature`).
        assert_eq!(header(&req, "X-BAPI-SIGN").as_deref(), Some("1e6e27b1200d5619b6be23711859e6cd2af138a024f487e93d78a3c81945e5c9"));
        assert_eq!(header(&req, "X-BAPI-TIMESTAMP").as_deref(), Some("1658384314791"));
        assert_eq!(header(&req, "X-BAPI-RECV-WINDOW").as_deref(), Some("5000"));
    }

    /// A throwaway RSA key generated for this test only: it signs nothing anywhere else.
    const TEST_RSA_PKCS8: &str = include_str!("fixtures/test_rsa_pkcs8.txt");
    const TEST_RSA_PKCS1: &str = include_str!("fixtures/test_rsa_pkcs1.txt");
    /// The same plain text signed by pybit with that key (RSA-SHA256 PKCS#1 v1.5, base64).
    const RSA_REFERENCE: &str = include_str!("fixtures/test_rsa_signature.txt");

    #[test]
    fn a_self_generated_key_signs_with_rsa_like_the_official_sdk() {
        for pem in [TEST_RSA_PKCS8, TEST_RSA_PKCS1] {
            let req = signed(pem);
            assert_eq!(header(&req, "X-BAPI-SIGN").as_deref(), Some(RSA_REFERENCE.trim()));
        }
    }

    #[test]
    fn a_broken_rsa_secret_is_named() {
        let secrets: HashMap<String, String> =
            [("api_key", "k"), ("api_secret", "-----BEGIN PRIVATE KEY-----\nnot base64!\n-----END PRIVATE KEY-----")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(); // gitleaks:allow
        let e = BybitSigner::from_secrets(&secrets).err().unwrap();
        assert_eq!(e.kind, ErrorKind::Auth);
        assert!(e.message.contains("PEM"), "{}", e.message);
    }
}
