//! Kraken Futures signs every private call with three headers: `APIKey`, `Nonce` and
//! `Authent` = base64 `HMAC-SHA512(base64-decoded secret, SHA256(postData ++ nonce ++
//! endpointPath))`.
//!
//!   - `postData` is the parameter string exactly as it is sent, url-encoded: the query of a
//!     GET, the body of a POST (the doc's "Upcoming Changes": the encoded form is the one
//!     hashed).
//!   - `endpointPath` is the path without the `/derivatives` prefix (the doc's example is
//!     `/api/v3/orderbook` for `.../derivatives/api/v3/orderbook`); a history path
//!     (`/api/history/v3/...`) is hashed as it is.
//!   - The nonce is optional but, when sent, has to increase: it comes from a process-wide
//!     counter seeded by the clock, so two calls in one millisecond never share one.
//!
//! The socket's challenge is signed with the same construction over the challenge alone,
//! which is what lets the documented socket example check the REST signature byte for byte.

use std::sync::atomic::{AtomicU64, Ordering};

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{encode_query, hmac_sha512, sha256, SignableRequest};
use otw_connector_sdk::Signer;
use secrecy::{ExposeSecret, SecretString};

pub struct KrakenFuturesSigner {
    pub key: SecretString,
    /// As Kraken issued it: base64, decoded at signing time.
    pub secret: SecretString,
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

const NOT_BASE64: &str = "the Kraken Futures private key is not the base64 string Kraken issued";

impl KrakenFuturesSigner {
    /// Refuses a secret that is not the base64 string Kraken issued, before any call.
    pub fn new(key: SecretString, secret: SecretString) -> Result<Self, ConnectorError> {
        if b64_decode(secret.expose_secret()).is_none() {
            return Err(ConnectorError::new(ErrorKind::Auth, NOT_BASE64));
        }
        Ok(KrakenFuturesSigner { key, secret })
    }
}

/// Strictly increasing, whatever the clock does.
pub fn nonce(now_ms: u64) -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    // `try_update` hands back the previous value, so the nonce used is recomputed from it.
    let prev = LAST.try_update(Ordering::SeqCst, Ordering::SeqCst, |p| Some(now_ms.max(p + 1))).unwrap_or(0);
    now_ms.max(prev + 1)
}

/// The path Kraken hashes: the request path without its `/derivatives` prefix.
pub fn endpoint_path(path: &str) -> &str {
    path.strip_prefix("/derivatives").filter(|p| p.starts_with('/')).unwrap_or(path)
}

/// base64 `HMAC-SHA512(secret, SHA256(message))`: `Authent` over `postData ++ nonce ++
/// endpointPath`, or the socket's signed challenge over the challenge.
pub fn authent(secret: &[u8], message: &str) -> Result<String, ConnectorError> {
    Ok(b64(&hmac_sha512(secret, &sha256(message.as_bytes()))?))
}

impl Signer for KrakenFuturesSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let secret = b64_decode(self.secret.expose_secret()).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, NOT_BASE64))?;
        let n = nonce(u64::try_from(now.millis()).unwrap_or(0)).to_string();
        let post_data = match &req.body {
            Some(b) if !b.is_empty() => String::from_utf8_lossy(b).into_owned(),
            _ => encode_query(&req.query),
        };
        let signature = authent(&secret, &format!("{post_data}{n}{}", endpoint_path(&req.path)))?;
        req.headers.push(("APIKey".into(), self.key.expose_secret().to_string()));
        req.headers.push(("Nonce".into(), n));
        req.headers.push(("Authent".into(), signature));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_sdk::Method;

    /// The secret of the documented socket example.
    const DOC_SECRET: &str = "7zxMEF5p/Z8l2p2U7Ghv6x14Af+Fx+92tPgUdVQ748FOIrEoT9bgT+bTRfXc5pz8na+hL/QdrCVG7bh9KpT0eMTm"; // gitleaks:allow

    /// Kraken's own worked example, verbatim: https://docs.kraken.com/exchange/guides/futures/websockets
    /// ("Sign challenge", Example). The challenge goes through the same SHA-256 then
    /// HMAC-SHA-512 then base64 as `Authent`.
    #[test]
    fn signs_the_documented_challenge() {
        let secret = b64_decode(DOC_SECRET).unwrap();
        assert_eq!(
            authent(&secret, "c100b894-1729-464d-ace1-52dbce11db42").unwrap(),
            "4JEpF3ix66GA2B+ooK128Ift4XQVtc137N9yeg4Kqsn9PI0Kpzbysl9M1IeCEdjg0zl00wkVqcsnG4bmnlMb3A=="
        );
    }

    /// Synthetic: the REST guide's inputs (`symbol=fi_xbtusd_180615`, nonce `1415957147987`,
    /// path `/api/v3/orderbook`) with the socket example's secret, because the REST guide's
    /// own sample secret is not valid base64 (87 characters). Reference value computed with
    /// Python's hashlib and hmac over the same three steps.
    #[test]
    fn signs_the_rest_guide_inputs() {
        let secret = b64_decode(DOC_SECRET).unwrap();
        assert_eq!(
            authent(&secret, "symbol=fi_xbtusd_1806151415957147987/api/v3/orderbook").unwrap(),
            "JHLjN8OUDYaXjHRGT0z4nUvJorORrXoL9omot4BK5ihtp6jKHPHfzX9MrpVjqAgKqGJejoO3gySwoVCMJQlx/Q=="
        );
        assert!(b64_decode("rttp4AzwRfYEdQ7R7X8Z/04Y4TZPa97pqCypi3xXxAqftygftnI6H9yGV+OcUOOJeFtZkr8mVwbAndU3Kz4Q+eG").is_none());
    }

    #[test]
    fn the_derivatives_prefix_is_not_hashed_and_a_history_path_is_hashed_whole() {
        assert_eq!(endpoint_path("/derivatives/api/v3/accounts"), "/api/v3/accounts");
        assert_eq!(endpoint_path("/api/history/v3/executions"), "/api/history/v3/executions");
        assert_eq!(endpoint_path("/derivativesx"), "/derivativesx");
    }

    #[test]
    fn a_nonce_never_repeats_even_inside_one_millisecond() {
        let mut last = 0;
        for _ in 0..1000 {
            let n = nonce(1_700_000_000_000);
            assert!(n > last);
            last = n;
        }
    }

    /// A signed GET: the query string as sent, the nonce, the trimmed path; three headers.
    /// Synthetic reference computed with Python over `count=1000&since=1604937694000&sort=asc`
    /// ++ `1415957147987` ++ `/api/history/v3/executions`.
    #[test]
    fn a_get_hashes_its_query_and_carries_three_headers() {
        let s = KrakenFuturesSigner::new(SecretString::from("k".to_string()), SecretString::from(DOC_SECRET.to_string())).unwrap();
        let mut req = SignableRequest {
            method: Method::Get,
            path: "/api/history/v3/executions".into(),
            query: vec![("count".into(), "1000".into()), ("since".into(), "1604937694000".into()), ("sort".into(), "asc".into())],
            headers: vec![],
            body: None,
        };
        s.sign(&mut req, Timestamp::from_millis(1_415_957_147_987).unwrap()).unwrap();
        let header = |k: &str| req.headers.iter().find(|(h, _)| h == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(header("APIKey"), "k");
        let n = header("Nonce");
        let secret = b64_decode(DOC_SECRET).unwrap();
        assert!(n.parse::<u64>().unwrap() >= 1_415_957_147_987);
        assert_eq!(header("Authent"), authent(&secret, &format!("count=1000&since=1604937694000&sort=asc{n}/api/history/v3/executions")).unwrap());
        assert_eq!(
            authent(&secret, "count=1000&since=1604937694000&sort=asc1415957147987/api/history/v3/executions").unwrap(),
            "FpM1hV3ucXAyE/mQ+pWdVqXUh+JGTxq35RckQRVWHqTpuInMraSoYJXOxRKArIyVSe08u15hlINfCufFMj3d5w=="
        );
        assert!(req.body.is_none(), "a GET carries no body");
        assert!(KrakenFuturesSigner::new(SecretString::from("k".to_string()), SecretString::from("not base64!".to_string())).is_err());
    }
}
