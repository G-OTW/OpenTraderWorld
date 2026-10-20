//! KuCoin answers with a code in the envelope (`200000` = success), with HTTP 200 for some
//! refusals and 4xx for others. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
}

fn kind_of(code: &str) -> ErrorKind {
    match code {
        "429000" => ErrorKind::RateLimited,
        // Missing header, timestamp, key, passphrase, signature, IP not whitelisted, frozen.
        "400001" | "400002" | "400003" | "400004" | "400005" | "400006" | "411100" => ErrorKind::Auth,
        // The key lacks the permission this call needs.
        "400007" => ErrorKind::Permission,
        // Trading pair or contract does not exist, invalid symbol.
        "900001" | "404000" | "200003" => ErrorKind::NotFound,
        "500000" | "503000" => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "kucoin") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    let env = serde_json::from_slice::<Envelope>(body).ok();
    let code = env.as_ref().and_then(|e| e.code.as_ref()).map(|c| match c {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    match code.as_deref() {
        Some("200000") => None,
        Some(code) => {
            let msg = env.as_ref().and_then(|e| e.msg.clone()).unwrap_or_default();
            let kind = if *status == 429 { ErrorKind::RateLimited } else { kind_of(code) };
            let mut e = ConnectorError::new(kind, format!("kucoin {endpoint_id}: {msg} (code {code})")).with_code(code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        None => http::classify(frame, "kucoin").or_else(|| Some(ConnectorError::decode(format!("kucoin {endpoint_id}: no envelope code")))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_envelope_code_decides() {
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"code":"200000","data":[]}"#)).is_none());
        // Recorded answers, 2026-10-19.
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"msg":"Trading pair ZZZ-NOPE does not exist.","code":"900001"}"#)).unwrap().kind, ErrorKind::NotFound);
        assert_eq!(from_frame(&http_frame(1, "x", 404, r#"{"msg":"The contract information you requested does not exist.","code":"404000"}"#)).unwrap().kind, ErrorKind::NotFound);
        let e = from_frame(&http_frame(1, "x", 400, r#"{"code":"400001","msg":"Please check the header of your request for KC-API-KEY, KC-API-SIGN, KC-API-TIMESTAMP, KC-API-PASSPHRASE."}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::Auth, Some("400001")));
        assert_eq!(from_frame(&http_frame(1, "x", 429, r#"{"code":"429000","msg":"Too Many Requests"}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 503, "busy")).unwrap().kind, ErrorKind::Transient);
    }
}
