//! Gate.io answers a refusal with a 4xx or 5xx status and a `label` in the body
//! (`{"label":"INVALID_KEY","message":"..."}`); a success is the bare payload. Table
//! mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Refusal {
    label: String,
    #[serde(default)]
    message: Option<String>,
}

fn kind_of(label: &str) -> ErrorKind {
    match label {
        "TOO_MANY_REQUESTS" => ErrorKind::RateLimited,
        // Bad key, signature, timestamp outside 60 s, missing header, IP not on the key's list,
        // account locked.
        "INVALID_KEY" | "INVALID_SIGNATURE" | "INVALID_CREDENTIALS" | "REQUEST_EXPIRED" | "MISSING_REQUIRED_HEADER" | "IP_FORBIDDEN" | "ACCOUNT_LOCKED" => {
            ErrorKind::Auth
        }
        // The key lacks the permission, or is read-only for a write.
        "FORBIDDEN" | "READ_ONLY" => ErrorKind::Permission,
        "INVALID_CURRENCY_PAIR" | "INVALID_CURRENCY" | "CONTRACT_NOT_FOUND" => ErrorKind::NotFound,
        "SERVER_ERROR" | "INTERNAL" | "TOO_BUSY" => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "gateio") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    if (200..300).contains(status) {
        return None;
    }
    match serde_json::from_slice::<Refusal>(body) {
        Ok(r) => {
            let kind = if *status == 429 { ErrorKind::RateLimited } else { kind_of(&r.label) };
            let msg = r.message.filter(|m| !m.is_empty()).unwrap_or_else(|| r.label.clone());
            let mut e = ConnectorError::new(kind, format!("gateio {endpoint_id}: {msg} ({})", r.label)).with_code(r.label);
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        Err(_) => http::classify(frame, "gateio"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_label_decides() {
        assert!(from_frame(&http_frame(1, "x", 200, "[]")).is_none());
        // Recorded answers, 2026-10-19.
        let e = from_frame(&http_frame(1, "x", 400, r#"{"label":"INVALID_CURRENCY_PAIR","message":"Invalid currency pair ZZZ_NOPE"}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::NotFound, Some("INVALID_CURRENCY_PAIR")));
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"label":"CONTRACT_NOT_FOUND"}"#)).unwrap().kind, ErrorKind::NotFound);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"label":"INVALID_CURRENCY","message":"Invalid currency NOPE"}"#)).unwrap().kind, ErrorKind::NotFound);
        assert_eq!(from_frame(&http_frame(1, "x", 401, r#"{"label":"INVALID_KEY","message":"Invalid key provided"}"#)).unwrap().kind, ErrorKind::Auth);
        let late = r#"{"label":"REQUEST_EXPIRED","message":"gap between request Timestamp and server time exceeds 60"}"#;
        assert_eq!(from_frame(&http_frame(1, "x", 403, late)).unwrap().kind, ErrorKind::Auth);
        let wide = r#"{"label":"INVALID_PARAM_VALUE","message":"Candlestick range too broad. Maximum 1000 data points are allowed per request"}"#;
        assert_eq!(from_frame(&http_frame(1, "x", 400, wide)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 429, r#"{"label":"TOO_MANY_REQUESTS","message":"Request Rate limit Exceeded"}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>bad gateway</html>")).unwrap().kind, ErrorKind::Transient);
    }
}
