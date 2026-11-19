//! OKX answers most refusals with HTTP 200 and a code in the envelope: success is the code,
//! not the status. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    msg: String,
}

fn kind_of(code: &str) -> ErrorKind {
    match code {
        // Rate limits.
        "50011" | "50061" => ErrorKind::RateLimited,
        // Key, signature, timestamp, passphrase.
        "50101" | "50102" | "50103" | "50104" | "50105" | "50111" | "50113" => ErrorKind::Auth,
        // The key may not read this.
        "50114" | "50120" => ErrorKind::Permission,
        // Instrument does not exist.
        "51001" => ErrorKind::NotFound,
        // System busy, timeouts.
        "50001" | "50004" | "50013" | "50026" => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

/// The error a frame carries, if any: the envelope's code first, then the HTTP status.
pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "okx") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, .. } = frame else { return None };
    let env = serde_json::from_slice::<Envelope>(body).ok();
    match env.as_ref().and_then(|e| e.code.as_deref()) {
        Some("0") => None,
        Some(code) => {
            let msg = env.as_ref().map(|e| e.msg.as_str()).unwrap_or_default();
            let kind = kind_of(code);
            // "bad symbol" is the phrase OTW's notice classifier reads as a symbol problem.
            let message = if kind == ErrorKind::NotFound {
                format!("okx {endpoint_id}: bad symbol ({code} {msg})")
            } else {
                format!("okx {endpoint_id}: {msg} (code {code})")
            };
            let mut e = ConnectorError::new(kind, message).with_code(code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        None => http::classify(frame, "okx").or_else(|| Some(ConnectorError::decode(format!("okx {endpoint_id}: no envelope code")))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_code_decides_not_the_status() {
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"code":"0","msg":"","data":[]}"#)).is_none());
        let e = from_frame(&http_frame(1, "x", 200, r#"{"code":"51001","msg":"Instrument ID does not exist","data":[]}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.message.contains("bad symbol"));
        assert_eq!(from_frame(&http_frame(1, "x", 429, r#"{"code":"50011","msg":"Too Many Requests"}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 401, r#"{"code":"50111","msg":"Invalid OK-ACCESS-KEY"}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>")).unwrap().kind, ErrorKind::Transient);
        assert_eq!(from_frame(&http_frame(1, "x", 200, "[]")).unwrap().kind, ErrorKind::Decode);
    }
}
