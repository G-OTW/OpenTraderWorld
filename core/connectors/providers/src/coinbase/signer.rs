//! Coinbase Advanced Trade signs a JWT per request, valid two minutes, whose `uri` claim
//! names the very call it authorises (`GET api.coinbase.com/api/v3/brokerage/accounts`, query
//! string excluded). A token is built for each call and never reused.
//!
//! Ed25519 keys only: Coinbase recommends them and issues them as 64 base64 bytes (32-byte
//! seed then the public key). The older ECDSA keys arrive as a PEM block and are refused with
//! a message naming the fix rather than half-supported.

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::SignableRequest;
use otw_connector_sdk::{Method, Signer};
use ring::signature::Ed25519KeyPair;
use secrecy::{ExposeSecret, SecretString};

/// The host the `uri` claim names (the brokerage API's).
pub const HOST: &str = "api.coinbase.com";
/// A token is accepted for two minutes.
pub const JWT_TTL_SECS: i64 = 120;

pub struct CoinbaseSigner {
    key_name: String,
    key: SecretString,
}

fn auth(msg: impl Into<String>) -> ConnectorError {
    ConnectorError::new(ErrorKind::Auth, msg)
}

/// The Ed25519 key Coinbase issues: base64 of 64 bytes, a seed then its public key (a bare
/// 32-byte seed is accepted too).
pub fn signing_key(secret: &str) -> Result<Ed25519KeyPair, ConnectorError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let trimmed = secret.trim();
    if trimmed.starts_with("-----BEGIN") {
        return Err(auth(
            "this is an ECDSA key in PEM form, which this connector does not sign with. Create a new CDP API key of type Ed25519, the one Coinbase recommends.",
        ));
    }
    let raw = STANDARD.decode(trimmed).map_err(|_| auth("the Coinbase private key is not the base64 string Coinbase issued"))?;
    let pair = match raw.len() {
        64 => Ed25519KeyPair::from_seed_and_public_key(&raw[..32], &raw[32..]),
        32 => Ed25519KeyPair::from_seed_unchecked(&raw),
        n => return Err(auth(format!("a Coinbase Ed25519 key decodes to 64 bytes (32 seed + 32 public), this one has {n}"))),
    };
    pair.map_err(|e| auth(format!("the Coinbase private key was rejected: {e}")))
}

/// A fresh random value per token (replay protection).
fn nonce(now: Timestamp) -> String {
    let mut bytes = [0u8; 16];
    // A failed draw must not produce a constant: the clock still varies, and the token only
    // lives two minutes.
    if getrandom::fill(&mut bytes).is_err() {
        bytes.copy_from_slice(&i128::from(now.nanos()).to_le_bytes());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl CoinbaseSigner {
    /// Checks the key loads, so a wrong one is refused before any call.
    pub fn new(key_name: String, key: SecretString) -> Result<Self, ConnectorError> {
        signing_key(key.expose_secret())?;
        Ok(CoinbaseSigner { key_name, key })
    }

    /// `{kid, nonce}` header, `{sub, iss, nbf, exp, uri}` payload, EdDSA.
    pub fn jwt(&self, method: Method, path: &str, now: Timestamp) -> Result<String, ConnectorError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let verb = match method {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        };
        let t = now.secs();
        let header = serde_json::json!({ "alg": "EdDSA", "typ": "JWT", "kid": self.key_name, "nonce": nonce(now) });
        let payload = serde_json::json!({
            "sub": self.key_name, "iss": "cdp", "nbf": t, "exp": t + JWT_TTL_SECS, "uri": format!("{verb} {HOST}{path}"),
        });
        let enc = |v: &serde_json::Value| -> Result<String, ConnectorError> {
            serde_json::to_vec(v).map(|b| URL_SAFE_NO_PAD.encode(b)).map_err(|e| auth(format!("encoding the token: {e}")))
        };
        let input = format!("{}.{}", enc(&header)?, enc(&payload)?);
        let sig = signing_key(self.key.expose_secret())?.sign(input.as_bytes());
        Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref())))
    }
}

impl Signer for CoinbaseSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let token = self.jwt(req.method, &req.path, now)?;
        req.headers.push(("Authorization".into(), format!("Bearer {token}")));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    const SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    /// RFC 8032 test vector 1: a known seed, the empty message, a known signature.
    #[test]
    fn signs_the_rfc8032_vector() {
        let mut raw = hex(SEED);
        raw.extend(hex(PUBLIC));
        let key = signing_key(&STANDARD.encode(&raw)).unwrap();
        assert_eq!(
            key.sign(b"").as_ref().to_vec(),
            hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")
        );
        assert!(signing_key(&STANDARD.encode(hex(SEED))).is_ok(), "a bare seed is the same key");
        assert!(signing_key("-----BEGIN EC PRIVATE KEY-----\nMHc=\n-----END EC PRIVATE KEY-----").unwrap_err().message.contains("Ed25519"));
        assert!(signing_key(&STANDARD.encode([1u8; 10])).unwrap_err().message.contains("has 10"));
    }

    #[test]
    fn the_jwt_names_the_call_it_authorises() {
        let s = CoinbaseSigner::new("organizations/o/apiKeys/k".into(), SecretString::from(STANDARD.encode(hex(SEED)))).unwrap();
        let now = Timestamp::from_secs(1_700_000_000).unwrap();
        let mut req = SignableRequest { method: Method::Get, path: "/api/v3/brokerage/accounts".into(), query: vec![("limit".into(), "250".into())], headers: vec![], body: None };
        s.sign(&mut req, now).unwrap();
        let token = req.headers[0].1.strip_prefix("Bearer ").unwrap().to_string();
        let parts: Vec<&str> = token.split('.').collect();
        let decode = |p: &str| -> serde_json::Value { serde_json::from_slice(&URL_SAFE_NO_PAD.decode(p).unwrap()).unwrap() };
        let (header, payload) = (decode(parts[0]), decode(parts[1]));
        assert_eq!(payload["uri"], "GET api.coinbase.com/api/v3/brokerage/accounts", "the query string is not signed");
        assert_eq!((payload["iss"].as_str(), payload["sub"].as_str()), (Some("cdp"), Some("organizations/o/apiKeys/k")));
        assert_eq!((payload["nbf"].as_i64(), payload["exp"].as_i64()), (Some(1_700_000_000), Some(1_700_000_120)));
        assert_eq!((header["alg"].as_str(), header["kid"].as_str()), (Some("EdDSA"), Some("organizations/o/apiKeys/k")));
        assert!(header["nonce"].as_str().is_some_and(|n| n.len() == 32));
        // The signature verifies against the public key.
        let public = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, hex(PUBLIC));
        assert!(public.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).is_ok());
    }
}
