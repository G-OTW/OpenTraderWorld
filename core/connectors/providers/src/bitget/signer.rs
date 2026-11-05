//! Bitget's signature (the OKX family with a millisecond epoch): base64 HMAC-SHA256 of
//! `timestamp + METHOD + path?query + body`, plus the passphrase chosen with the key.

use otw_connector_model::{ConnectorError, Timestamp};
use otw_connector_sdk::signer::{encode_query, hmac_sha256, SignableRequest};
use otw_connector_sdk::{Method, Signer};
use secrecy::{ExposeSecret, SecretString};

pub struct BitgetSigner {
    pub key: SecretString,
    pub secret: SecretString,
    pub passphrase: SecretString,
}

pub fn prehash(ts_ms: i64, method: Method, path: &str, query: &str, body: &str) -> String {
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };
    if query.is_empty() {
        format!("{ts_ms}{verb}{path}{body}")
    } else {
        format!("{ts_ms}{verb}{path}?{query}{body}")
    }
}

impl Signer for BitgetSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        use base64::Engine as _;
        let ts = now.millis();
        let body = req.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let pre = prehash(ts, req.method, &req.path, &encode_query(&req.query), &body);
        let sign = base64::engine::general_purpose::STANDARD.encode(hmac_sha256(self.secret.expose_secret().as_bytes(), pre.as_bytes())?);
        req.headers.push(("ACCESS-KEY".into(), self.key.expose_secret().to_string()));
        req.headers.push(("ACCESS-SIGN".into(), sign));
        req.headers.push(("ACCESS-TIMESTAMP".into(), ts.to_string()));
        req.headers.push(("ACCESS-PASSPHRASE".into(), self.passphrase.expose_secret().to_string()));
        req.headers.push(("locale".into(), "en-US".into()));
        req.headers.push(("Content-Type".into(), "application/json".into()));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_documented_prehash() {
        // Bitget's own worked example, from the REST authentication page.
        assert_eq!(
            prehash(16273667805456, Method::Get, "/api/mix/v2/market/depth", "limit=20&symbol=BTCUSDT", ""),
            "16273667805456GET/api/mix/v2/market/depth?limit=20&symbol=BTCUSDT"
        );
        let s = BitgetSigner {
            key: SecretString::from("k".to_string()),
            secret: SecretString::from("secret-test".to_string()),
            passphrase: SecretString::from("p".to_string()),
        };
        let mut req = SignableRequest {
            method: Method::Get,
            path: "/api/mix/v2/market/depth".into(),
            query: vec![("limit".into(), "20".into()), ("symbol".into(), "BTCUSDT".into())],
            headers: vec![],
            body: None,
        };
        s.sign(&mut req, Timestamp::from_millis(1_627_366_780_545).unwrap()).unwrap();
        let h = |k: &str| req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        // Reference computed with an independent HMAC implementation (Python's hmac module).
        assert_eq!(h("ACCESS-SIGN").as_deref(), Some("WSb9iSrOn8HhQMGgyzInt4CI+q3V/+Plc3ntdpVRn5A="));
        assert_eq!(h("ACCESS-TIMESTAMP").as_deref(), Some("1627366780545"));
    }
}
