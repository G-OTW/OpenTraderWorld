//! Bitfinex refusals. The v2 API answers a refusal as `["error", code, "message"]`, with HTTP
//! 500 for nearly everything (a bad key, a bad parameter, an unknown symbol, all measured);
//! the rate limit answers `{"error": "ERR_RATE_LIMIT"}` (docs) and an outage
//! `{"code":503,"error":"temporarily_unavailable",...}` (ccxt's sample). The code alone is
//! coarse (10020 is any bad parameter, the unknown symbol among them), so the message
//! decides where the code does not. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde_json::value::RawValue;

fn kind_of(code: i64, msg: &str) -> ErrorKind {
    let m = msg.to_ascii_lowercase();
    match code {
        // `ratelimit: error`.
        11010 => ErrorKind::RateLimited,
        // Failed authentication (`apikey: invalid`, `apikey: digest invalid`, measured), a
        // malformed payload, signature or HMAC, and the nonce (`nonce: small`).
        10100 | 10111 | 10112 | 10113 | 10114 => ErrorKind::Auth,
        // A generic error; ccxt reads `api_key: permission invalid` here.
        10001 if m.contains("permission") => ErrorKind::Permission,
        // A bad parameter: an unknown symbol is one (`symbol: invalid`, measured).
        10020 if m.contains("symbol: invalid") => ErrorKind::NotFound,
        // Unknown error, concurrency, not ready, maintenance.
        10000 | 10008 | 11000 | 20060 => ErrorKind::Transient,
        _ if m.contains("permission") => ErrorKind::Permission,
        _ if m.contains("nonce") || m.contains("apikey") => ErrorKind::Auth,
        _ => ErrorKind::InvalidRequest,
    }
}

/// `["error", code, "message"]`.
fn error_array(body: &[u8]) -> Option<(i64, String)> {
    let parts: Vec<Box<RawValue>> = serde_json::from_slice(body).ok()?;
    let tag: String = serde_json::from_str(parts.first()?.get()).ok()?;
    if tag != "error" {
        return None;
    }
    let code = parts.get(1).and_then(|c| serde_json::from_str::<i64>(c.get()).ok()).unwrap_or_default();
    let msg = parts.get(2).and_then(|m| serde_json::from_str::<String>(m.get()).ok()).unwrap_or_default();
    Some((code, msg))
}

/// `{"error": "ERR_RATE_LIMIT"}` and the outage object.
#[derive(serde::Deserialize)]
struct ErrorObject {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "bitfinex") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    let text = body.trim_ascii_start();
    if text.starts_with(b"[") {
        if let Some((code, msg)) = error_array(text) {
            let msg = if msg.trim().is_empty() { format!("code {code}") } else { msg.trim().to_string() };
            let kind = if *status == 429 { ErrorKind::RateLimited } else { kind_of(code, &msg) };
            let mut e = ConnectorError::new(kind, format!("bitfinex {endpoint_id}: {msg} ({code})")).with_code(code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            return Some(e);
        }
        // A success is a bare array.
        if (200..300).contains(status) {
            return None;
        }
    }
    if text.starts_with(b"{") {
        if let Ok(o) = serde_json::from_slice::<ErrorObject>(text) {
            if let Some(err) = o.error.filter(|e| !e.is_empty()) {
                let (kind, code) = match err.as_str() {
                    "ERR_RATE_LIMIT" => (ErrorKind::RateLimited, err.clone()),
                    "temporarily_unavailable" => (ErrorKind::Transient, err.clone()),
                    _ if *status == 429 => (ErrorKind::RateLimited, err.clone()),
                    _ => (ErrorKind::Transient, err.clone()),
                };
                let what = o.error_description.filter(|d| !d.is_empty()).unwrap_or_else(|| err.clone());
                let mut e = ConnectorError::new(kind, format!("bitfinex {endpoint_id}: {what}")).with_code(code);
                // A rate-limited IP is blocked for a minute (docs).
                e.retry_after_ms = http::retry_after_ms(frame).or((kind == ErrorKind::RateLimited).then_some(60_000));
                return Some(e);
            }
        }
    }
    http::classify(frame, "bitfinex")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_code_and_message_decide() {
        assert!(from_frame(&http_frame(1, "x", 200, "[]")).is_none());
        assert!(from_frame(&http_frame(1, "x", 200, r#"[[1792403460000,82975,82960,82975,82957,0.36083973]]"#)).is_none());
        assert!(from_frame(&http_frame(1, "x", 200, r#"["tBTCUSD",1]"#)).is_none());
        // Recorded answers, 2026-10-19.
        let e = from_frame(&http_frame(1, "x", 500, r#"["error",10020,"symbol: invalid"]"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::NotFound, Some("10020")));
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",10020,"limit: invalid"]"#)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",10020,"time_interval: invalid"]"#)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",10100,"apikey: digest invalid"]"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",10100,"apikey: invalid"]"#)).unwrap().kind, ErrorKind::Auth);
        let e = from_frame(&http_frame(1, "x", 404, "<html><pre>Cannot POST /api/v2/auth/r/nope</pre></html>")).unwrap();
        assert_eq!(e.kind, ErrorKind::NotFound);
        // Documented and ccxt's samples.
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",10114,"nonce: small"]"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",10001,"api_key: permission invalid"]"#)).unwrap().kind, ErrorKind::Permission);
        assert_eq!(from_frame(&http_frame(1, "x", 500, r#"["error",11010,"ratelimit: error"]"#)).unwrap().kind, ErrorKind::RateLimited);
        let e = from_frame(&http_frame(1, "x", 429, r#"{"error":"ERR_RATE_LIMIT"}"#)).unwrap();
        assert_eq!((e.kind, e.retry_after_ms), (ErrorKind::RateLimited, Some(60_000)));
        let down = r#"{"code":503,"error":"temporarily_unavailable","error_description":"Sorry, the service is temporarily unavailable."}"#;
        assert_eq!(from_frame(&http_frame(1, "x", 503, down)).unwrap().kind, ErrorKind::Transient);
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>bad gateway</html>")).unwrap().kind, ErrorKind::Transient);
    }
}
