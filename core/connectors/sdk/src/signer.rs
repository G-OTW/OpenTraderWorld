//! Request signing. Lives on the transport side so a mapper never touches a secret; secrets
//! are `SecretString`, never logged, never formatted.

use hmac::{Hmac, KeyInit, Mac};
use otw_connector_model::{ConnectorError, ErrorKind, Timestamp};
use secrecy::{ExposeSecret, SecretString};

use crate::native::Method;

/// The parts of a request a signature covers, before it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignableRequest {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Option<bytes::Bytes>,
}

pub trait Signer: Send + Sync {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError>;

    /// The frame that logs a socket in, for a venue that authenticates its socket with a
    /// message rather than a signed URL. `None`: this venue's sockets take no login.
    fn ws_login(&self, _now: Timestamp) -> Result<Option<bytes::Bytes>, ConnectorError> {
        Ok(None)
    }

    /// For a credential exchanged for a short-lived token (OAuth refresh): the call that
    /// mints one, when none is held. `None`: a token is held, or this signer needs none.
    fn token_request(&self, _now: Timestamp) -> Option<TokenRequest> {
        None
    }

    /// The minting call's answer, kept for the calls that follow. Header names are lower
    /// case (a venue that hands its session back in headers, not the body).
    fn accept_token(&self, _status: u16, _headers: &[(String, String)], _body: &[u8], _now: Timestamp) -> Result<(), ConnectorError> {
        Ok(())
    }

    /// A socket frame the venue authenticates per message (a session token on every
    /// subscribe), completed with the credentials before it is sent. The mapper writes the
    /// frame without them.
    fn sign_frame(&self, frame: bytes::Bytes, _now: Timestamp) -> Result<bytes::Bytes, ConnectorError> {
        Ok(frame)
    }

    /// The venue refused the token: the next call mints a new one rather than replaying it.
    fn forget_token(&self) {}
}

/// The exact query string the driver sends, so what is signed is what travels.
pub fn encode_query(pairs: &[(String, String)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k, v);
    }
    s.finish()
}

pub fn hmac_sha256(key: &[u8], payload: &[u8]) -> Result<Vec<u8>, ConnectorError> {
    let mut mac = <Hmac<sha2::Sha256>>::new_from_slice(key)
        .map_err(|_| ConnectorError::new(ErrorKind::Auth, "secret is not usable as an HMAC key"))?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().to_vec())
}

pub fn hmac_sha512(key: &[u8], payload: &[u8]) -> Result<Vec<u8>, ConnectorError> {
    let mut mac = <Hmac<sha2::Sha512>>::new_from_slice(key)
        .map_err(|_| ConnectorError::new(ErrorKind::Auth, "secret is not usable as an HMAC key"))?;
    mac.update(payload);
    Ok(mac.finalize().into_bytes().to_vec())
}

pub fn sha256(payload: &[u8]) -> Vec<u8> {
    use sha2::Digest as _;
    sha2::Sha256::digest(payload).to_vec()
}

/// A key sent as a query parameter (`apikey=...`): the venues that authenticate data calls
/// that way. Added at send time, so the mapper never writes it.
pub struct ApiKeyQuerySigner {
    pub param: &'static str,
    pub key: SecretString,
}

impl Signer for ApiKeyQuerySigner {
    fn sign(&self, req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        req.query.push((self.param.to_string(), self.key.expose_secret().to_string()));
        Ok(())
    }
}

/// Keys sent as headers (`APCA-API-KEY-ID`, `X-API-KEY`...).
pub struct HeaderKeySigner {
    pub headers: Vec<(&'static str, SecretString)>,
}

impl Signer for HeaderKeySigner {
    fn sign(&self, req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        for (name, value) in &self.headers {
            req.headers.push((name.to_string(), value.expose_secret().to_string()));
        }
        Ok(())
    }
}

/// HMAC-SHA256 over the query string, hex signature appended as a query parameter, key in a
/// header. Binance and the many venues that copied its scheme (MEXC, BingX, ...).
pub struct HmacQuerySigner {
    pub api_key: SecretString,
    pub secret: SecretString,
    pub key_header: &'static str,
    pub timestamp_param: &'static str,
    pub signature_param: &'static str,
    pub recv_window: Option<(&'static str, u64)>,
}

impl Signer for HmacQuerySigner {
    fn sign(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        if let Some((name, ms)) = self.recv_window {
            req.query.push((name.to_string(), ms.to_string()));
        }
        req.query.push((self.timestamp_param.to_string(), now.millis().to_string()));
        let mut payload = encode_query(&req.query);
        if let Some(b) = &req.body {
            payload.push_str(&String::from_utf8_lossy(b));
        }
        let sig = hmac_sha256(self.secret.expose_secret().as_bytes(), payload.as_bytes())?;
        req.query.push((self.signature_param.to_string(), hex::encode(sig)));
        req.headers.push((self.key_header.to_string(), self.api_key.expose_secret().to_string()));
        Ok(())
    }
}

/// A call that mints an access token (an OAuth grant, a session log-on). The driver asks
/// again after each answer, so a sign-in in two steps (log on, then pick the account) is two
/// of these.
pub struct TokenRequest {
    /// The base URL key of the sign-in host.
    pub base: &'static str,
    pub method: Method,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: TokenBody,
}

impl TokenRequest {
    pub fn post(base: &'static str, path: impl Into<String>, body: TokenBody) -> Self {
        TokenRequest { base, method: Method::Post, path: path.into(), headers: Vec::new(), body }
    }
}

pub enum TokenBody {
    /// `application/x-www-form-urlencoded`.
    Form(Vec<(String, String)>),
    /// `application/json`.
    Json(String),
}

/// Access tokens and sessions minted from a long-lived credential, shared by every signer of
/// the same credential for the life of the process. Keyed by a hash of the credential, never
/// the credential.
static TOKENS: std::sync::Mutex<Option<std::collections::HashMap<String, (SecretString, i64)>>> = std::sync::Mutex::new(None);

fn with_tokens<T>(f: impl FnOnce(&mut std::collections::HashMap<String, (SecretString, i64)>) -> T) -> T {
    let mut guard = TOKENS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(Default::default))
}

/// A stable, non-reversible key for a credential.
pub fn credential_key(parts: &[&str]) -> String {
    let mut material = Vec::new();
    for part in parts {
        material.extend_from_slice(part.as_bytes());
        // A separator, so two different splits of the same characters are two keys.
        material.push(0);
    }
    hex::encode(sha256(&material))
}

/// The token kept for this credential, while it is still good at `now`.
pub fn cached_token(key: &str, now: Timestamp) -> Option<SecretString> {
    with_tokens(|c| c.get(key).filter(|(_, until)| *until > now.nanos()).map(|(t, _)| t.clone()))
}

/// Keep a token for `ttl_secs` from `now`, spent a minute early.
pub fn keep_token(key: &str, token: String, now: Timestamp, ttl_secs: i64) {
    let ttl_ns = ttl_secs.saturating_mul(1_000_000_000);
    let until = now.nanos().saturating_add(ttl_ns.saturating_sub(TOKEN_MARGIN_NS).max(0));
    with_tokens(|c| c.insert(key.to_string(), (SecretString::from(token), until)));
}

pub fn drop_token(key: &str) {
    with_tokens(|c| c.remove(key));
}

/// Spent this long before its stated expiry, so a call never leaves with a token about to die.
const TOKEN_MARGIN_NS: i64 = 60_000_000_000;

/// OAuth 2 refresh-token grant: the refresh token is exchanged at the sign-in host for an
/// access token, kept until shortly before it expires and sent as a bearer.
pub struct OAuthRefresh {
    label: &'static str,
    key: String,
    base: &'static str,
    path: String,
    client_id: SecretString,
    client_secret: SecretString,
    refresh_token: SecretString,
    default_ttl_secs: i64,
}

impl OAuthRefresh {
    /// `label` names the venue in a refusal; `default_ttl_secs` is used when the answer states
    /// no lifetime.
    pub fn new(label: &'static str, base: &'static str, path: &str, client_id: &str, client_secret: &str, refresh_token: &str, default_ttl_secs: i64) -> Self {
        OAuthRefresh {
            label,
            key: credential_key(&[label, client_id, refresh_token]),
            base,
            path: path.to_string(),
            client_id: SecretString::from(client_id.to_string()),
            client_secret: SecretString::from(client_secret.to_string()),
            refresh_token: SecretString::from(refresh_token.to_string()),
            default_ttl_secs,
        }
    }

    /// The access token in force, if one is.
    pub fn token(&self, now: Timestamp) -> Option<SecretString> {
        cached_token(&self.key, now)
    }

    pub fn request(&self, now: Timestamp) -> Option<TokenRequest> {
        if self.token(now).is_some() {
            return None;
        }
        Some(TokenRequest::post(
            self.base,
            self.path.clone(),
            TokenBody::Form(vec![
                ("grant_type".into(), "refresh_token".into()),
                ("client_id".into(), self.client_id.expose_secret().to_string()),
                ("client_secret".into(), self.client_secret.expose_secret().to_string()),
                ("refresh_token".into(), self.refresh_token.expose_secret().to_string()),
            ]),
        ))
    }

    pub fn accept(&self, status: u16, body: &[u8], now: Timestamp) -> Result<(), ConnectorError> {
        #[derive(serde::Deserialize)]
        struct Answer {
            #[serde(default)]
            access_token: Option<String>,
            #[serde(default)]
            expires_in: Option<i64>,
        }
        let refused = |why: String| {
            ConnectorError::new(
                ErrorKind::Auth,
                format!("{} refused the refresh token ({why}). Sign in again to issue a new one, and make sure the scopes include offline_access.", self.label),
            )
        };
        if !(200..300).contains(&status) {
            let snippet: String = String::from_utf8_lossy(body).chars().take(200).collect();
            return Err(refused(format!("{status}: {snippet}")));
        }
        let a: Answer = serde_json::from_slice(body).map_err(|e| refused(format!("unreadable answer: {e}")))?;
        let token = a.access_token.filter(|t| !t.is_empty()).ok_or_else(|| refused("no access token in the answer".into()))?;
        keep_token(&self.key, token, now, a.expires_in.filter(|s| *s > 0).unwrap_or(self.default_ttl_secs));
        Ok(())
    }

    pub fn forget(&self) {
        drop_token(&self.key);
    }

    /// `Authorization: Bearer <token>`, or a refusal when none is held (the driver mints one
    /// before signing, so this means the minting failed).
    pub fn bearer(&self, req: &mut SignableRequest, now: Timestamp) -> Result<(), ConnectorError> {
        let t = self.token(now).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("no {} access token: the sign-in did not answer", self.label)))?;
        req.headers.push(("Authorization".into(), format!("Bearer {}", t.expose_secret())));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The worked example from Binance's own API documentation (a published sample key, not a
    /// credential).
    #[test]
    fn hmac_query_signature_matches_the_documented_example() {
        let signer = HmacQuerySigner {
            api_key: SecretString::from("k".to_string()),
            secret: SecretString::from("NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j".to_string()),
            key_header: "X-MBX-APIKEY",
            timestamp_param: "timestamp",
            signature_param: "signature",
            recv_window: Some(("recvWindow", 5000)),
        };
        let q = |k: &str, v: &str| (k.to_string(), v.to_string());
        let mut req = SignableRequest {
            method: Method::Post,
            path: "/api/v3/order".into(),
            query: vec![
                q("symbol", "LTCBTC"),
                q("side", "BUY"),
                q("type", "LIMIT"),
                q("timeInForce", "GTC"),
                q("quantity", "1"),
                q("price", "0.1"),
            ],
            headers: vec![],
            body: None,
        };
        signer.sign(&mut req, Timestamp::from_millis(1_499_827_319_559).unwrap()).unwrap();
        let sig = req.query.iter().find(|(k, _)| k == "signature").map(|(_, v)| v.as_str());
        assert_eq!(sig, Some("c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"));
        assert_eq!(req.headers, vec![("X-MBX-APIKEY".to_string(), "k".to_string())]);
    }
}
