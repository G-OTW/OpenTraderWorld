//! KuCoin's signature: base64 HMAC-SHA256 of `timestamp(ms) + METHOD + path?query + body`,
//! the query written unencoded, plus the passphrase chosen with the key, itself sent as the
//! base64 HMAC-SHA256 of the passphrase under the secret (key versions 2 and 3).

use std::collections::HashMap;

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{hmac_sha256, SignableRequest};
use otw_connector_sdk::{Method, Signer};
use secrecy::{ExposeSecret, SecretString};

pub struct KucoinSigner {
    key: SecretString,
    secret: SecretString,
    passphrase: SecretString,
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// What the signature covers. The query is joined as written, not URL-encoded: KuCoin signs
/// the raw form (its own SDKs build it with `key=value&...`).
pub fn prehash(ts_ms: i64, method: Method, path: &str, query: &[(String, String)], body: &str) -> String {
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };
    let raw: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    if raw.is_empty() {
        format!("{ts_ms}{verb}{path}{body}")
    } else {
        format!("{ts_ms}{verb}{path}?{}{body}", raw.join("&"))
    }
}

impl KucoinSigner {
    pub fn from_secrets(secrets: &HashMap<String, String>) -> Result<Self, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| SecretString::from(v.to_string()))
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
        };
        Ok(KucoinSigner { key: get("api_key")?, secret: get("api_secret")?, passphrase: get("api_passphrase")? })
    }
}

impl Signer for KucoinSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let ts = now.millis();
        let body = req.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let secret = self.secret.expose_secret().as_bytes();
        let sign = b64(&hmac_sha256(secret, prehash(ts, req.method, &req.path, &req.query, &body).as_bytes())?);
        let passphrase = b64(&hmac_sha256(secret, self.passphrase.expose_secret().as_bytes())?);
        req.headers.push(("KC-API-KEY".into(), self.key.expose_secret().to_string()));
        req.headers.push(("KC-API-SIGN".into(), sign));
        req.headers.push(("KC-API-TIMESTAMP".into(), ts.to_string()));
        req.headers.push(("KC-API-PASSPHRASE".into(), passphrase));
        // What KuCoin's own SDKs send; a version 2 key signs the same way.
        req.headers.push(("KC-API-KEY-VERSION".into(), "3".into()));
        req.headers.push(("Content-Type".into(), "application/json".into()));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_raw_query_and_encrypts_the_passphrase() {
        let q = vec![("symbol".to_string(), "BTC-USDT".to_string()), ("limit".to_string(), "100".to_string())];
        assert_eq!(prehash(1_729_577_515_473, Method::Get, "/api/v1/hf/fills", &q, ""), "1729577515473GET/api/v1/hf/fills?symbol=BTC-USDT&limit=100");
        // KuCoin's documented example: the signed form keeps `!@#` unencoded.
        let odd = vec![("passphrase".to_string(), "abc!@#11".to_string())];
        assert_eq!(prehash(1, Method::Get, "/api/v1/sub/api-key", &odd, ""), "1GET/api/v1/sub/api-key?passphrase=abc!@#11");
        let secrets: HashMap<String, String> =
            [("api_key", "k"), ("api_secret", "secret-test"), ("api_passphrase", "my passphrase")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let s = KucoinSigner::from_secrets(&secrets).unwrap();
        let mut req = SignableRequest { method: Method::Get, path: "/api/v1/hf/fills".into(), query: q, headers: vec![], body: None };
        s.sign(&mut req, Timestamp::from_millis(1_729_577_515_473).unwrap()).unwrap();
        let h = |k: &str| req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        // References computed with an independent HMAC implementation (Python's hmac module).
        assert_eq!(h("KC-API-SIGN").as_deref(), Some("RDdVk1gnR4CIPfci5Iil20DBPuhLBUGZ8sLBoiMdS0Q="));
        assert_eq!(h("KC-API-PASSPHRASE").as_deref(), Some("P8cVhjaE79U2PEBTveX/ADltg1yXB8Wp2Aewuoslz5w="));
        assert_eq!(h("KC-API-TIMESTAMP").as_deref(), Some("1729577515473"));
        assert_eq!(h("KC-API-KEY-VERSION").as_deref(), Some("3"));
    }

    #[test]
    fn a_missing_passphrase_is_named() {
        let secrets: HashMap<String, String> = [("api_key", "k"), ("api_secret", "s")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let e = KucoinSigner::from_secrets(&secrets).err().unwrap();
        assert!(e.message.contains("api_passphrase"));
    }
}
