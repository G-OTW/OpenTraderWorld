//! Gate.io APIv4 signature: lowercase hex HMAC-SHA512 of
//! `METHOD\n/api/v4/path\nquery\nhex(SHA512(body))\ntimestamp`, the query written unencoded
//! and the timestamp in seconds, sent as `KEY`, `Timestamp` and `SIGN` headers. The same
//! form as Gate.io's own SDK (`gate_api/api_client.py`, `gen_sign`).

use std::collections::HashMap;

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{hmac_sha512, SignableRequest};
use otw_connector_sdk::{Method, Signer};
use secrecy::{ExposeSecret, SecretString};

pub struct GateSigner {
    key: SecretString,
    secret: SecretString,
}

/// What the signature covers. The query is joined as written: the SDK signs
/// `unquote_plus(urlencode(query))`.
pub fn prehash(method: Method, path: &str, query: &[(String, String)], body: &[u8], ts_secs: i64) -> String {
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };
    let raw: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let hashed = hex::encode(ring::digest::digest(&ring::digest::SHA512, body));
    format!("{verb}\n{path}\n{}\n{hashed}\n{ts_secs}", raw.join("&"))
}

impl GateSigner {
    pub fn from_secrets(secrets: &HashMap<String, String>) -> Result<Self, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| SecretString::from(v.to_string()))
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
        };
        Ok(GateSigner { key: get("api_key")?, secret: get("api_secret")? })
    }
}

impl Signer for GateSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let ts = now.secs();
        let body = req.body.as_deref().unwrap_or_default();
        let sign = hex::encode(hmac_sha512(self.secret.expose_secret().as_bytes(), prehash(req.method, &req.path, &req.query, body, ts).as_bytes())?);
        req.headers.push(("KEY".into(), self.key.expose_secret().to_string()));
        req.headers.push(("Timestamp".into(), ts.to_string()));
        req.headers.push(("SIGN".into(), sign));
        req.headers.push(("Accept".into(), "application/json".into()));
        req.headers.push(("Content-Type".into(), "application/json".into()));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn signs_like_the_sdk() {
        let q = vec![("currency_pair".to_string(), "BTC_USDT".to_string()), ("limit".to_string(), "1000".to_string())];
        let empty = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
        assert_eq!(prehash(Method::Get, "/api/v4/spot/my_trades", &q, b"", 1_792_392_000), format!("GET\n/api/v4/spot/my_trades\ncurrency_pair=BTC_USDT&limit=1000\n{empty}\n1792392000"));
        let secrets: HashMap<String, String> = [("api_key", "k"), ("api_secret", "secret-test")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let s = GateSigner::from_secrets(&secrets).unwrap();
        let mut req = SignableRequest { method: Method::Get, path: "/api/v4/spot/my_trades".into(), query: q, headers: vec![], body: None };
        s.sign(&mut req, Timestamp::from_secs(1_792_392_000).unwrap()).unwrap();
        let h = |k: &str| req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        // Reference computed with an independent implementation (Python's hmac and hashlib,
        // the way gate_api's gen_sign builds it).
        assert_eq!(
            h("SIGN").as_deref(),
            Some("1afe4b5887bd1752f91495fa083aefc69765096eb95add03fd618911d54dccd3c9aa8ba4d8cceaa9f0474281b4296cd81a8389abf353b74444602145006c287e")
        );
        assert_eq!(h("Timestamp").as_deref(), Some("1792392000"));
        assert_eq!(h("KEY").as_deref(), Some("k"));
    }

    #[test]
    fn a_missing_secret_is_named() {
        let secrets: HashMap<String, String> = [("api_key", "k")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let e = GateSigner::from_secrets(&secrets).err().unwrap();
        assert!(e.message.contains("api_secret"));
    }
}
