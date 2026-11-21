//! Coinbase Advanced Trade signs a JWT per request, valid two minutes, whose `uri` claim
//! names the very call it authorises (`GET api.coinbase.com/api/v3/brokerage/accounts`, query
//! string excluded). A token is built for each call and never reused. A socket frame carries one
//! too, in its `jwt` field, built without `uri` ("WebSocket JWTs (vs those for REST API) are not
//! built with a request method or request path"), a new one per frame ("you must generate a
//! different JWT for each websocket message sent").
//!
//! Two kinds of key, told apart by their form: an Ed25519 key (the one Coinbase recommends,
//! 64 base64 bytes: a 32-byte seed then the public key) signs `EdDSA`; an ECDSA P-256 key (a
//! PEM block, `EC PRIVATE KEY` as Coinbase issues it, or `PRIVATE KEY`) signs `ES256`.

use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use otw_connector_sdk::signer::SignableRequest;
use otw_connector_sdk::{Method, Signer};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, Ed25519KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
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

/// A Coinbase key, as its form says.
pub enum SigningKey {
    /// Ed25519: signs `EdDSA`.
    Ed25519(Ed25519KeyPair),
    /// ECDSA on P-256 with SHA-256: signs `ES256`.
    Es256(EcdsaKeyPair),
}

impl std::fmt::Debug for SigningKey {
    /// Never the key itself: its kind only.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.alg())
    }
}

impl SigningKey {
    /// The JWT `alg` this key signs with.
    pub fn alg(&self) -> &'static str {
        match self {
            SigningKey::Ed25519(_) => "EdDSA",
            SigningKey::Es256(_) => "ES256",
        }
    }

    /// The signature of `msg`: 64 bytes either way (ES256 as `r || s`, as a JWT carries it).
    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, ConnectorError> {
        match self {
            SigningKey::Ed25519(k) => Ok(k.sign(msg).as_ref().to_vec()),
            SigningKey::Es256(k) => k.sign(&SystemRandom::new(), msg).map(|s| s.as_ref().to_vec()).map_err(|_| auth("the Coinbase ECDSA key could not sign")),
        }
    }

    /// The public key: 32 bytes (Ed25519), or the 65-byte uncompressed point (P-256).
    #[cfg(test)]
    pub fn public(&self) -> Vec<u8> {
        use ring::signature::KeyPair as _;
        match self {
            SigningKey::Ed25519(k) => k.public_key().as_ref().to_vec(),
            SigningKey::Es256(k) => k.public_key().as_ref().to_vec(),
        }
    }
}

/// A DER length.
fn der_len(n: usize) -> Vec<u8> {
    match n {
        0..=0x7f => vec![u8::try_from(n).unwrap_or(0)],
        0x80..=0xff => vec![0x81, u8::try_from(n).unwrap_or(0)],
        _ => vec![0x82, u8::try_from(n >> 8).unwrap_or(0), u8::try_from(n & 0xff).unwrap_or(0)],
    }
}

/// A SEC1 `ECPrivateKey` (what `-----BEGIN EC PRIVATE KEY-----` holds) inside the PKCS#8
/// envelope the signer loads: algorithm `id-ecPublicKey` on `prime256v1`.
fn sec1_to_pkcs8(sec1: &[u8]) -> Vec<u8> {
    const ALG: [u8; 21] = [0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
    let mut inner = vec![0x02, 0x01, 0x00];
    inner.extend_from_slice(&ALG);
    inner.push(0x04);
    inner.extend(der_len(sec1.len()));
    inner.extend_from_slice(sec1);
    let mut out = vec![0x30];
    out.extend(der_len(inner.len()));
    out.extend(inner);
    out
}

/// The DER inside a PEM block of `label`, where the text is one.
fn pem_body(text: &str, label: &str) -> Option<Result<Vec<u8>, ConnectorError>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = text.find(&begin)? + begin.len();
    let stop = text[start..].find(&end)? + start;
    let b64: String = text[start..stop].chars().filter(|c| !c.is_whitespace()).collect();
    Some(STANDARD.decode(b64).map_err(|_| auth(format!("the Coinbase {label} block is not base64"))))
}

/// The key Coinbase issues, by its form: a PEM block is an ECDSA P-256 key (`EC PRIVATE KEY`,
/// SEC1, as Coinbase issues it; or `PRIVATE KEY`, PKCS#8); base64 of 64 bytes (a seed then its
/// public key, a bare 32-byte seed accepted too) is an Ed25519 key.
pub fn signing_key(secret: &str) -> Result<SigningKey, ConnectorError> {
    // A key pasted from Coinbase's JSON keeps its line breaks escaped.
    let text = secret.trim().replace("\\n", "\n");
    let ec = |pkcs8: &[u8]| {
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8, &SystemRandom::new())
            .map(SigningKey::Es256)
            .map_err(|e| auth(format!("the Coinbase ECDSA key was rejected ({e}): it must be a P-256 key with its public key, as Coinbase issues it")))
    };
    if let Some(der) = pem_body(&text, "EC PRIVATE KEY") {
        return ec(&sec1_to_pkcs8(&der?));
    }
    if let Some(der) = pem_body(&text, "PRIVATE KEY") {
        return ec(&der?);
    }
    if text.starts_with("-----BEGIN") {
        return Err(auth("this PEM block is not a Coinbase private key: paste the whole EC PRIVATE KEY block Coinbase issued, or an Ed25519 key"));
    }
    signing_ed25519(&text).map(SigningKey::Ed25519)
}

fn signing_ed25519(trimmed: &str) -> Result<Ed25519KeyPair, ConnectorError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
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

    /// `{kid, nonce}` header, `{sub, iss, nbf, exp, uri}` payload, EdDSA or ES256 by the key.
    pub fn jwt(&self, method: Method, path: &str, now: Timestamp) -> Result<String, ConnectorError> {
        let verb = match method {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        };
        self.token(Some(format!("{verb} {HOST}{path}")), now)
    }

    /// A socket frame's JWT: the same claims without `uri` (the SDK's `build_ws_jwt`).
    pub fn ws_jwt(&self, now: Timestamp) -> Result<String, ConnectorError> {
        self.token(None, now)
    }

    fn token(&self, uri: Option<String>, now: Timestamp) -> Result<String, ConnectorError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let t = now.secs();
        let key = signing_key(self.key.expose_secret())?;
        let header = serde_json::json!({ "alg": key.alg(), "typ": "JWT", "kid": self.key_name, "nonce": nonce(now) });
        let mut payload = serde_json::json!({ "sub": self.key_name, "iss": "cdp", "nbf": t, "exp": t + JWT_TTL_SECS });
        if let Some(uri) = uri {
            payload["uri"] = serde_json::json!(uri);
        }
        let enc = |v: &serde_json::Value| -> Result<String, ConnectorError> {
            serde_json::to_vec(v).map(|b| URL_SAFE_NO_PAD.encode(b)).map_err(|e| auth(format!("encoding the token: {e}")))
        };
        let input = format!("{}.{}", enc(&header)?, enc(&payload)?);
        let sig = key.sign(input.as_bytes())?;
        Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig)))
    }
}

impl Signer for CoinbaseSigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let token = self.jwt(req.method, &req.path, now)?;
        req.headers.push(("Authorization".into(), format!("Bearer {token}")));
        Ok(())
    }

    /// A subscribe frame with its own JWT in `jwt` (WebSocket overview: "To subscribe to any
    /// channel, provide a `channel` name and `jwt`").
    fn sign_frame(&self, frame: bytes::Bytes, now: Timestamp) -> Result<bytes::Bytes, ConnectorError> {
        let mut v: serde_json::Value = serde_json::from_slice(&frame).map_err(|e| auth(format!("a Coinbase socket frame is not JSON: {e}")))?;
        let obj = v.as_object_mut().ok_or_else(|| auth("a Coinbase socket frame is not a JSON object"))?;
        obj.insert("jwt".into(), serde_json::json!(self.ws_jwt(now)?));
        serde_json::to_vec(&v).map(bytes::Bytes::from).map_err(|e| auth(format!("encoding the socket frame: {e}")))
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
        assert_eq!(key.alg(), "EdDSA");
        assert_eq!(
            key.sign(b"").unwrap(),
            hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")
        );
        assert!(signing_key(&STANDARD.encode(hex(SEED))).is_ok(), "a bare seed is the same key");
        assert!(signing_key("-----BEGIN EC PRIVATE KEY-----\nMHc=\n-----END EC PRIVATE KEY-----").unwrap_err().message.contains("P-256"));
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

    /// A socket frame carries its own JWT, with no `uri` claim, and keeps every field it had.
    #[test]
    fn a_socket_frame_carries_a_jwt_without_uri() {
        let s = CoinbaseSigner::new("organizations/o/apiKeys/k".into(), SecretString::from(STANDARD.encode(hex(SEED)))).unwrap();
        let now = Timestamp::from_secs(1_700_000_000).unwrap();
        let signed = s.sign_frame(bytes::Bytes::from_static(br#"{"type":"subscribe","channel":"user"}"#), now).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&signed).unwrap();
        assert_eq!((v["type"].as_str(), v["channel"].as_str()), (Some("subscribe"), Some("user")));
        let token = v["jwt"].as_str().unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let payload: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert!(payload.get("uri").is_none(), "{payload}");
        assert_eq!((payload["iss"].as_str(), payload["exp"].as_i64()), (Some("cdp"), Some(1_700_000_120)));
        let public = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, hex(PUBLIC));
        assert!(public.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).is_ok());
        // A new token per frame: the nonce differs.
        let again: serde_json::Value = serde_json::from_slice(&s.sign_frame(bytes::Bytes::from_static(br#"{"type":"subscribe"}"#), now).unwrap()).unwrap();
        assert_ne!(again["jwt"], v["jwt"]);
        assert!(s.sign_frame(bytes::Bytes::from_static(b"[1]"), now).is_err());
    }

    // An ECDSA P-256 key made by an independent implementation (OpenSSL 4.1:
    // `openssl ecparam -name prime256v1 -genkey -noout`), a test key made for this vector and
    // nothing else; its public point (`openssl ec -pubout`) and OpenSSL's own signature
    // (`openssl dgst -sha256 -sign`) of `otw-es256-vector`.
    const EC_PEM: &str = "-----BEGIN EC PRIVATE KEY-----\nMHcCAQEEIBKH1nf2ddI/0tzLrlK/jM1uXxdyVUAX77qyIZv0nFBdoAoGCCqGSM49\nAwEHoUQDQgAEWVzQwU6khSrZLiO4KvjKW81z0z+kv3AbDhOivvXc9o0BpE06oE+h\notnbBVsR5eMOd3FkYyq+0D25aklOzCJKew==\n-----END EC PRIVATE KEY-----\n";
    const EC_PUBLIC: &str = "04595cd0c14ea4852ad92e23b82af8ca5bcd73d33fa4bf701b0e13a2bef5dcf68d01a44d3aa04fa1a2d9db055b11e5e30e777164632abed03db96a494ecc224a7b";
    const EC_OPENSSL_SIG: &str = "304402201e60e8c0694a6b7f92c1d64ec2ec9843b121882d3230bfd199d90989d19633d00220574e41c622020968b76a7e481ad96d3f9cfde1a192c1159469fa49d0355b1428";

    /// The SEC1 PEM loads as an ES256 key whose public point is the one OpenSSL derived, and
    /// OpenSSL's signature verifies against it (both sides agree on the curve and the hash).
    #[test]
    fn an_ec_key_from_openssl_is_the_same_key() {
        let key = signing_key(EC_PEM).unwrap();
        assert_eq!(key.alg(), "ES256");
        assert_eq!(key.public(), hex(EC_PUBLIC));
        let public = ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_ASN1, hex(EC_PUBLIC));
        assert!(public.verify(b"otw-es256-vector", &hex(EC_OPENSSL_SIG)).is_ok());
        // As pasted from Coinbase's JSON, line breaks escaped.
        assert_eq!(signing_key(&EC_PEM.replace('\n', "\\n")).unwrap().public(), hex(EC_PUBLIC));
    }

    /// An ES256 JWT: `alg` ES256, a 64-byte `r || s` signature that verifies against the public
    /// point OpenSSL derived from the key.
    #[test]
    fn an_es256_jwt_verifies_against_the_openssl_public_key() {
        let s = CoinbaseSigner::new("organizations/o/apiKeys/k".into(), SecretString::from(EC_PEM.to_string())).unwrap();
        let token = s.jwt(Method::Post, "/api/v3/brokerage/orders", Timestamp::from_secs(1_700_000_000).unwrap()).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let header: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"].as_str(), Some("ES256"));
        let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        assert_eq!(sig.len(), 64);
        let public = ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, hex(EC_PUBLIC));
        assert!(public.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).is_ok());
    }
}
