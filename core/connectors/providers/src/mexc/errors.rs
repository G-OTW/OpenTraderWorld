//! MEXC refusals. Spot answers a 4xx status with `{"code":-1121,"msg":"Invalid symbol."}`;
//! the contract API answers HTTP 200 with `{"success":false,"code":1001,"message":...}`. A
//! success is the bare payload on spot and `{"success":true,"code":0,"data":...}` on
//! futures. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn kind_of(code: i64) -> ErrorKind {
    match code {
        // Rate limits: spot 429, contract 510.
        429 | 510 => ErrorKind::RateLimited,
        // Missing or bad key, bad signature, expired key, clock outside the window (spot
        // `recvWindow`, contract 10 s), unknown account.
        401 | 402 | 602 | 513 | 1000 | 10072 | 10073 | 700001 | 700002 | 700003 | 10001 => ErrorKind::Auth,
        // IP not on the key's list.
        406 | 700006 => ErrorKind::Auth,
        // The key lacks the permission.
        403 | 511 | 701 | 703 | 700007 => ErrorKind::Permission,
        // Unknown symbol or contract (measured: -1121 on spot, 1001 on the contract API).
        -1121 | 10007 | 30014 | 730001 | 1001 => ErrorKind::NotFound,
        500 | 501 | 503 | 504 | 801 | 604 | 9999 => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

fn code_of(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "mexc") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    let ok = (200..300).contains(status);
    // A success body that is a list (spot) never parses as a refusal.
    if ok && !body.trim_ascii_start().starts_with(b"{") {
        return None;
    }
    match serde_json::from_slice::<Refusal>(body) {
        Ok(r) => {
            // A 2xx body is a refusal only when it says so.
            if ok && r.success != Some(false) {
                return None;
            }
            let code = r.code.as_ref().and_then(code_of);
            let kind = match (*status, code) {
                (429, _) => ErrorKind::RateLimited,
                (_, Some(c)) => kind_of(c),
                _ => return http::classify(frame, "mexc"),
            };
            let code = code.unwrap_or_default();
            let msg = r.msg.or(r.message).filter(|m| !m.trim().is_empty()).unwrap_or_else(|| format!("code {code}"));
            let mut e = ConnectorError::new(kind, format!("mexc {endpoint_id}: {} ({code})", msg.trim())).with_code(code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        Err(_) if ok => None,
        Err(_) => http::classify(frame, "mexc"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_code_decides_on_both_apis() {
        assert!(from_frame(&http_frame(1, "x", 200, "[]")).is_none());
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"success":true,"code":0,"data":1792398963229}"#)).is_none());
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"serverTime":1792396656322}"#)).is_none());
        // Recorded answers, 2026-10-19.
        let e = from_frame(&http_frame(1, "x", 400, r#"{"msg":"Invalid symbol.","code":-1121,"_extend":null}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::NotFound, Some("-1121")));
        let e = from_frame(&http_frame(1, "x", 200, r#"{"success":false,"code":1001,"message":"Contract does not exist"}"#)).unwrap();
        assert_eq!((e.kind, e.message.contains("Contract does not exist")), (ErrorKind::NotFound, true));
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"code":10072,"msg":"Api key info invalid"}"#)).unwrap().kind, ErrorKind::Auth);
        // 400 is also "the start time cannot be earlier than 90 days": not a key problem.
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"code":400,"msg":"api key required"}"#)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"success":false,"code":600,"message":"Parameter error"}"#)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"success":false,"code":"510","message":"Requests are too frequent"}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"code":700003,"msg":"Timestamp for this request is outside of the recvWindow."}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>bad gateway</html>")).unwrap().kind, ErrorKind::Transient);
    }
}
