//! Gate.io APIv4 signature: lowercase hex HMAC-SHA512 of
//! `METHOD\n/api/v4/path\nquery\nhex(SHA512(body))\ntimestamp`, the query written unencoded
//! and the timestamp in seconds, sent as `KEY`, `Timestamp` and `SIGN` headers. The same
//! form as Gate.io's own SDK (`gate_api/api_client.py`, `gen_sign`).
//!
//! The private sockets authenticate each subscribe frame on its own: `auth{method: "api_key",
//! KEY, SIGN}` with SIGN the hex HMAC-SHA512 of `channel=<c>&event=<e>&time=<t>`, `time` the
//! frame's own, in seconds (spot and futures WebSocket v4 pages, "Authentication").

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

/// What a socket frame's signature covers.
pub fn frame_prehash(channel: &str, event: &str, time_secs: i64) -> String {
    format!("channel={channel}&event={event}&time={time_secs}")
}

impl Signer for GateSigner {
    /// A `subscribe` or `unsubscribe` frame of a private channel, stamped with `now` and signed.
    /// Any other frame (a ping) goes as written.
    fn sign_frame(&self, frame: bytes::Bytes, now: Timestamp) -> Result<bytes::Bytes, ConnectorError> {
        let Ok(serde_json::Value::Object(mut m)) = serde_json::from_slice::<serde_json::Value>(&frame) else { return Ok(frame) };
        let channel = m.get("channel").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let event = m.get("event").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        if channel.is_empty() || !matches!(event.as_str(), "subscribe" | "unsubscribe") {
            return Ok(frame);
        }
        let t = now.secs();
        let sign = hex::encode(hmac_sha512(self.secret.expose_secret().as_bytes(), frame_prehash(&channel, &event, t).as_bytes())?);
        m.insert("time".into(), serde_json::json!(t));
        m.insert("auth".into(), serde_json::json!({ "method": "api_key", "KEY": self.key.expose_secret(), "SIGN": sign }));
        let out = serde_json::to_vec(&serde_json::Value::Object(m)).map_err(|e| ConnectorError::invalid(format!("gateio frame: {e}")))?;
        Ok(bytes::Bytes::from(out))
    }

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

    /// The overview's worked example (key `key`, secret `secret`, timestamp 1541993715), the
    /// full SIGN values read 2026-11-12 from https://www.gate.com/docs/developers/apiv4/en/
    /// ("API Signature string generation", examples 1 and 2), byte for byte.
    #[test]
    fn signs_the_documented_examples() {
        let secrets: HashMap<String, String> = [("api_key", "key"), ("api_secret", "secret")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let s = GateSigner::from_secrets(&secrets).unwrap();
        let at = Timestamp::from_secs(1_541_993_715).unwrap();
        let h = |req: &SignableRequest, k: &str| req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let q = vec![("contract".to_string(), "BTC_USD".to_string()), ("status".to_string(), "finished".to_string()), ("limit".to_string(), "50".to_string())];
        let mut get = SignableRequest { method: Method::Get, path: "/api/v4/futures/orders".into(), query: q, headers: vec![], body: None };
        s.sign(&mut get, at).unwrap();
        assert_eq!(
            h(&get, "SIGN").as_deref(),
            Some("55f84ea195d6fe57ce62464daaa7c3c02fa9d1dde954e4c898289c9a2407a3d6fb3faf24deff16790d726b66ac9f74526668b13bd01029199cc4fcc522418b8a")
        );
        let body = r#"{"contract":"BTC_USD","type":"limit","size":100,"price":6800,"time_in_force":"gtc"}"#;
        let mut post = SignableRequest { method: Method::Post, path: "/api/v4/futures/orders".into(), query: vec![], headers: vec![], body: Some(bytes::Bytes::from(body)) };
        s.sign(&mut post, at).unwrap();
        assert_eq!(
            h(&post, "SIGN").as_deref(),
            Some("eae42da914a590ddf727473aff25fc87d50b64783941061f47a3fdb92742541fc4c2c14017581b4199a1418d54471c269c03a38d788d802e2c306c37636389f0")
        );
    }

    /// A private subscribe frame is stamped and signed; the doc gives no worked value, so the
    /// reference is an independent HMAC-SHA512 (Python's `hmac`, computed once and pinned).
    #[test]
    fn a_subscribe_frame_is_signed_and_a_ping_is_not() {
        let secrets: HashMap<String, String> = [("api_key", "key"), ("api_secret", "secret")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let s = GateSigner::from_secrets(&secrets).unwrap();
        let at = Timestamp::from_secs(1_611_541_000).unwrap();
        assert_eq!(frame_prehash("spot.orders", "subscribe", 1_611_541_000), "channel=spot.orders&event=subscribe&time=1611541000");
        let frame = bytes::Bytes::from(r#"{"time":0,"channel":"spot.orders","event":"subscribe","payload":["!all"]}"#);
        let v: serde_json::Value = serde_json::from_slice(&s.sign_frame(frame, at).unwrap()).unwrap();
        assert_eq!(v["time"], 1_611_541_000);
        assert_eq!((v["auth"]["method"].as_str(), v["auth"]["KEY"].as_str()), (Some("api_key"), Some("key")));
        assert_eq!(
            v["auth"]["SIGN"].as_str(),
            Some("833a44bb4cd083e47bc1fec22b5c12e8b98c30fc7a9f4a1e52800dcfdff9a57240d7c10f1e9553dee020011fa5d02cdc7d1155a380cc4dcbd45f8fb525e41f4b")
        );
        let ping = bytes::Bytes::from(r#"{"time":0,"channel":"spot.ping"}"#);
        assert_eq!(s.sign_frame(ping.clone(), at).unwrap(), ping);
    }

    #[test]
    fn a_missing_secret_is_named() {
        let secrets: HashMap<String, String> = [("api_key", "k")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let e = GateSigner::from_secrets(&secrets).err().unwrap();
        assert!(e.message.contains("api_secret"));
    }
}
