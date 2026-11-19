//! Bitget answers with a code in the envelope (`00000` = success), often with HTTP 400 for a
//! refusal. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn kind_of(code: &str) -> ErrorKind {
    match code {
        "429" => ErrorKind::RateLimited,
        // Key does not exist, invalid key, signature, passphrase, timestamp, IP.
        "40037" | "40006" | "40009" | "40012" | "40008" | "40018" => ErrorKind::Auth,
        "40014" => ErrorKind::Permission,
        // Parameter (symbol) does not exist.
        "40034" => ErrorKind::NotFound,
        "40010" | "40015" | "45001" => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "bitget") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    let env = serde_json::from_slice::<Envelope>(body).ok();
    let code = env.as_ref().and_then(|e| e.code.as_ref()).map(|c| match c {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    match code.as_deref() {
        Some("00000") => None,
        Some(code) => {
            let msg = env.as_ref().and_then(|e| e.msg.clone().or_else(|| e.message.clone())).unwrap_or_default();
            let kind = if *status == 429 { ErrorKind::RateLimited } else { kind_of(code) };
            let message = if kind == ErrorKind::NotFound {
                format!("bitget {endpoint_id}: bad symbol ({code} {msg})")
            } else {
                format!("bitget {endpoint_id}: {msg} (code {code})")
            };
            let mut e = ConnectorError::new(kind, message).with_code(code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        None => http::classify(frame, "bitget").or_else(|| Some(ConnectorError::decode(format!("bitget {endpoint_id}: no envelope code")))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_envelope_code_decides() {
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"code":"00000","msg":"success","data":[]}"#)).is_none());
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"code":"40034","msg":"Parameter does not exist"}"#)).unwrap().kind, ErrorKind::NotFound);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"code":"40037","msg":"Apikey does not exist"}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 429, r#"{"code":"429","msg":"Too Many Requests"}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 503, "busy")).unwrap().kind, ErrorKind::Transient);
    }
}
