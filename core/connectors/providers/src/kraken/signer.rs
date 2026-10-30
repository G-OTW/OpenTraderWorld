//! Kraken signs a POST body rather than a query string: `API-Sign` is
//! `HMAC-SHA512(base64-decoded secret, path ++ SHA256(nonce ++ body))`, which is why the
//! nonce appears twice: once inside the body that is hashed, once as the prefix of that
//! hash. It must also strictly increase, so it comes from a process-wide counter rather than
//! from the clock alone: two calls in the same millisecond would otherwise be refused with
//! `EAPI:Invalid nonce`, an error that reads like a bad key.

use std::sync::atomic::{AtomicU64, Ordering};

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::{hmac_sha512, sha256, SignableRequest};
use otw_connector_sdk::Signer;
use secrecy::{ExposeSecret, SecretString};

pub struct KrakenSigner {
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

impl KrakenSigner {
    /// Refuses a secret that is not the base64 string Kraken issued, before any call.
    pub fn new(key: SecretString, secret: SecretString) -> Result<Self, ConnectorError> {
        if b64_decode(secret.expose_secret()).is_none() {
            return Err(ConnectorError::new(ErrorKind::Auth, "the Kraken private key is not the base64 string Kraken issued"));
        }
        Ok(KrakenSigner { key, secret })
    }
}

/// Strictly increasing, whatever the clock does: Kraken refuses a nonce it has already seen.
pub fn nonce(now_ms: u64) -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    // `fetch_update` hands back the previous value, so the nonce used is recomputed from it.
    let prev = LAST.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |p| Some(now_ms.max(p + 1))).unwrap_or(0);
    now_ms.max(prev + 1)
}

/// `HMAC-SHA512(secret, path ++ SHA256(nonce ++ body))`, base64.
pub fn sign(secret: &[u8], path: &str, nonce: &str, body: &str) -> Result<String, ConnectorError> {
    let mut payload = path.as_bytes().to_vec();
    payload.extend(sha256(format!("{nonce}{body}").as_bytes()));
    Ok(b64(&hmac_sha512(secret, &payload)?))
}

impl Signer for KrakenSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let secret = b64_decode(self.secret.expose_secret())
            .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "the Kraken private key is not the base64 string Kraken issued"))?;
        let n = nonce(u64::try_from(now.millis()).unwrap_or(0)).to_string();
        let rest = req.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let body = if rest.is_empty() { format!("nonce={n}") } else { format!("nonce={n}&{rest}") };
        let signature = sign(&secret, &req.path, &n, &body)?;
        req.body = Some(body.into());
        req.headers.push(("API-Key".into(), self.key.expose_secret().to_string()));
        req.headers.push(("API-Sign".into(), signature));
        req.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_sdk::Method;

    /// Kraken's own worked example (docs: "Authentication", a published sample secret).
    #[test]
    fn signs_the_documented_example() {
        let secret = b64_decode("kQH5HW/8p1uGOVjbgWA7FunAmGO8lsSUXNsu3eow76sz84Q18fWxnyRzBHCd3pd5nE9qa99HAZtuZuj6F1huXg==").unwrap();
        let sig = sign(
            &secret,
            "/0/private/AddOrder",
            "1616492376594",
            "nonce=1616492376594&ordertype=limit&pair=XBTUSD&price=37500&type=buy&volume=1.25",
        )
        .unwrap();
        assert_eq!(sig, "4/dpxb3iT4tp/ZCVEwSnEsLxx0bqyhLpdfOpc6fn7OR8+UClSV5n9E6aSS8MPtnRfp32bAb0nmbRn6H8ndwLUQ==");
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

    #[test]
    fn the_nonce_leads_the_body_and_the_key_rides_a_header() {
        let s = KrakenSigner::new(SecretString::from("k".to_string()), SecretString::from("aGVsbG8=".to_string())).unwrap();
        let mut req = SignableRequest {
            method: Method::Post,
            path: "/0/private/TradesHistory".into(),
            query: vec![],
            headers: vec![],
            body: Some("type=all&ofs=0".into()),
        };
        s.sign(&mut req, Timestamp::from_millis(1_700_000_000_000).unwrap()).unwrap();
        let body = String::from_utf8(req.body.unwrap().to_vec()).unwrap();
        assert!(body.starts_with("nonce=") && body.ends_with("&type=all&ofs=0"), "{body}");
        assert!(req.headers.iter().any(|(k, v)| k == "API-Key" && v == "k"));
        assert!(KrakenSigner::new(SecretString::from("k".to_string()), SecretString::from("not base64!".to_string())).is_err());
    }
}
