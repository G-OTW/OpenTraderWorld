//! Alpaca authenticates with two plain headers on REST and an `auth` message on the
//! socket: no signature, but the key never leaves the signer.

use otw_connector_model::{ConnectorError, Timestamp};
use otw_connector_sdk::signer::SignableRequest;
use otw_connector_sdk::Signer;
use secrecy::{ExposeSecret, SecretString};

pub struct AlpacaSigner {
    pub key: SecretString,
    pub secret: SecretString,
}

impl Signer for AlpacaSigner {
    fn sign(&self, req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        req.headers.push(("APCA-API-KEY-ID".into(), self.key.expose_secret().to_string()));
        req.headers.push(("APCA-API-SECRET-KEY".into(), self.secret.expose_secret().to_string()));
        Ok(())
    }

    fn ws_login(&self, _now: Timestamp) -> Result<Option<bytes::Bytes>, ConnectorError> {
        let frame = serde_json::json!({ "action": "auth", "key": self.key.expose_secret(), "secret": self.secret.expose_secret() });
        Ok(Some(frame.to_string().into()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_sdk::Method;

    #[test]
    fn headers_on_rest_a_message_on_the_socket() {
        let s = AlpacaSigner { key: SecretString::from("k".to_string()), secret: SecretString::from("s".to_string()) };
        let mut r = SignableRequest { method: Method::Get, path: "/v2/account".into(), query: vec![], headers: vec![], body: None };
        s.sign(&mut r, Timestamp::from_nanos(0)).unwrap();
        assert_eq!(r.headers, [("APCA-API-KEY-ID".to_string(), "k".to_string()), ("APCA-API-SECRET-KEY".to_string(), "s".to_string())]);
        let login: serde_json::Value = serde_json::from_slice(&s.ws_login(Timestamp::from_nanos(0)).unwrap().unwrap()).unwrap();
        assert_eq!(login, serde_json::json!({"action":"auth","key":"k","secret":"s"}));
    }
}
