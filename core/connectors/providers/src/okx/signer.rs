//! OKX's signature: base64 HMAC-SHA256 of `timestamp + METHOD + path?query + body`, the
//! timestamp in ISO-8601 to the millisecond, plus the passphrase chosen with the key. Demo
//! trading is the same host with one more header.

use otw_connector_model::{ConnectorError, Timestamp};
use otw_connector_sdk::signer::{encode_query, hmac_sha256, SignableRequest};
use otw_connector_sdk::{Method, Signer};
use secrecy::{ExposeSecret, SecretString};

pub struct OkxSigner {
    pub key: SecretString,
    pub secret: SecretString,
    pub passphrase: SecretString,
    pub demo: bool,
}

/// `2020-12-08T09:08:57.715Z`.
pub fn iso_ms(now: Timestamp) -> Result<String, ConnectorError> {
    let t = now.to_offset_datetime().ok_or_else(|| ConnectorError::invalid("clock out of range"))?;
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.millisecond()
    ))
}

pub fn prehash(ts: &str, method: Method, path: &str, query: &str, body: &str) -> String {
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };
    if query.is_empty() {
        format!("{ts}{verb}{path}{body}")
    } else {
        format!("{ts}{verb}{path}?{query}{body}")
    }
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

impl Signer for OkxSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let ts = iso_ms(now)?;
        let body = req.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let pre = prehash(&ts, req.method, &req.path, &encode_query(&req.query), &body);
        let sign = b64(&hmac_sha256(self.secret.expose_secret().as_bytes(), pre.as_bytes())?);
        req.headers.push(("OK-ACCESS-KEY".into(), self.key.expose_secret().to_string()));
        req.headers.push(("OK-ACCESS-SIGN".into(), sign));
        req.headers.push(("OK-ACCESS-TIMESTAMP".into(), ts));
        req.headers.push(("OK-ACCESS-PASSPHRASE".into(), self.passphrase.expose_secret().to_string()));
        req.headers.push(("Content-Type".into(), "application/json".into()));
        if self.demo {
            req.headers.push(("x-simulated-trading".into(), "1".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_path_with_its_query_in_iso_time() {
        let now = Timestamp::from_millis(1_607_418_537_715).unwrap();
        assert_eq!(iso_ms(now).unwrap(), "2020-12-08T09:08:57.715Z");
        assert_eq!(prehash("T", Method::Get, "/api/v5/trade/fills-history", "", ""), "TGET/api/v5/trade/fills-history");
        let s = OkxSigner {
            key: SecretString::from("k".to_string()),
            secret: SecretString::from("22582BD0CFF14C41EDBF1AB98506286D".to_string()),
            passphrase: SecretString::from("p".to_string()),
            demo: true,
        };
        let mut req = SignableRequest {
            method: Method::Get,
            path: "/api/v5/account/balance".into(),
            query: vec![("ccy".into(), "BTC".into())],
            headers: vec![],
            body: None,
        };
        s.sign(&mut req, now).unwrap();
        let h = |k: &str| req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        // Reference computed with an independent HMAC implementation (Python's hmac module).
        assert_eq!(h("OK-ACCESS-SIGN").as_deref(), Some("HiZhvSfMtWJA3uUIVXV3a/bSXNPCWvYFXoGCVS8V4zY="));
        assert_eq!(h("OK-ACCESS-TIMESTAMP").as_deref(), Some("2020-12-08T09:08:57.715Z"));
        assert_eq!(h("x-simulated-trading").as_deref(), Some("1"));
    }
}
