//! Deribit's per-request signature (`deri-hmac-sha256`): no token to mint or keep. Lowercase
//! hex HMAC-SHA256, keyed by the client secret, of
//! `timestamp + "\n" + nonce + "\n" + METHOD + "\n" + URI + "\n" + body + "\n"`, where the
//! URI is the path with its query string exactly as sent (`/api/v2/private/get_positions?
//! currency=any`) and the body is empty on a GET. Sent as
//! `Authorization: deri-hmac-sha256 id=<client id>,ts=<ms>,sig=<hex>,nonce=<nonce>`. The
//! timestamp is accepted for 60 s; the nonce is random per call.

use std::collections::HashMap;

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{encode_query, hmac_sha256, SignableRequest};
use otw_connector_sdk::{Method, Signer};
use secrecy::{ExposeSecret, SecretString};

fn secret(secrets: &HashMap<String, String>, name: &str) -> Result<SecretString, ConnectorError> {
    secrets
        .get(name)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| SecretString::from(v.to_string()))
        .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
}

pub struct DeribitSigner {
    /// Deribit's "Client Id".
    id: SecretString,
    /// Deribit's "Client Secret".
    secret: SecretString,
}

/// What the signature covers.
pub fn prehash(ts_ms: i64, nonce: &str, method: Method, uri: &str, body: &str) -> String {
    let method = match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    };
    format!("{ts_ms}\n{nonce}\n{method}\n{uri}\n{body}\n")
}

/// Sixteen hex characters, random. A failed draw falls back on the clock: the timestamp
/// already varies, and the venue keeps a nonce for one timestamp only.
fn nonce(now: Timestamp) -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        bytes.copy_from_slice(&now.nanos().to_le_bytes());
    }
    hex::encode(bytes)
}

impl DeribitSigner {
    pub fn from_secrets(secrets: &HashMap<String, String>) -> Result<Self, ConnectorError> {
        Ok(DeribitSigner { id: secret(secrets, "api_key")?, secret: secret(secrets, "api_secret")? })
    }

    /// [`Signer::sign`] with a given nonce, so a test can check it.
    pub fn sign_with(&self, req: &mut SignableRequest, now: Timestamp, nonce: &str) -> Result<(), ConnectorError> {
        let ts = now.millis();
        let mut uri = req.path.clone();
        if !req.query.is_empty() {
            uri.push('?');
            uri.push_str(&encode_query(&req.query));
        }
        let body = req.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let sig = hex::encode(hmac_sha256(self.secret.expose_secret().as_bytes(), prehash(ts, nonce, req.method, &uri, &body).as_bytes())?);
        req.headers.push(("Authorization".into(), format!("deri-hmac-sha256 id={},ts={ts},sig={sig},nonce={nonce}", self.id.expose_secret())));
        Ok(())
    }
}

impl Signer for DeribitSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        self.sign_with(req, now, &nonce(now))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn secrets(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn signs_the_uri_with_its_query_as_sent() {
        let s = DeribitSigner::from_secrets(&secrets(&[("api_key", "AMANDA"), ("api_secret", "AMANDASECRECT")])).unwrap();
        let q = vec![("currency".to_string(), "any".to_string())];
        let mut req = SignableRequest { method: Method::Get, path: "/api/v2/private/get_positions".into(), query: q, headers: vec![], body: None };
        s.sign_with(&mut req, Timestamp::from_millis(1_576_074_319_000).unwrap(), "1iqt2wls").unwrap();
        assert_eq!(prehash(1_576_074_319_000, "1iqt2wls", Method::Get, "/api/v2/private/get_positions?currency=any", ""), "1576074319000\n1iqt2wls\nGET\n/api/v2/private/get_positions?currency=any\n\n");
        // Reference computed with an independent implementation (Python's hmac and hashlib,
        // the `calcSignature` of Deribit's authentication guide).
        let auth = req.headers.iter().find(|(k, _)| k == "Authorization").map(|(_, v)| v.as_str());
        assert_eq!(auth, Some("deri-hmac-sha256 id=AMANDA,ts=1576074319000,sig=f71f94f835df0c0c0eda2ef0aa13eb135c6b7976f899bf4260fe69321fca41bc,nonce=1iqt2wls"));
    }

    #[test]
    fn a_nonce_is_fresh_per_call() {
        let s = DeribitSigner::from_secrets(&secrets(&[("api_key", "k"), ("api_secret", "s")])).unwrap();
        let mut a = SignableRequest { method: Method::Get, path: "/api/v2/private/get_open_orders".into(), query: vec![], headers: vec![], body: None };
        let mut b = a.clone();
        let now = Timestamp::from_millis(1_792_400_000_000).unwrap();
        s.sign(&mut a, now).unwrap();
        s.sign(&mut b, now).unwrap();
        assert_ne!(a.headers, b.headers);
    }

    #[test]
    fn a_missing_secret_is_named() {
        let e = DeribitSigner::from_secrets(&secrets(&[("api_key", "k")])).err().unwrap();
        assert!(e.message.contains("api_secret"));
    }
}
