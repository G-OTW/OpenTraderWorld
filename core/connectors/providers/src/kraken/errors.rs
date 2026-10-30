//! Kraken answers most refusals with HTTP 200 and an `error` array of `Category:Message`
//! strings: success is an empty array, not the status. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    error: Option<Vec<String>>,
}

fn kind_of(e: &str) -> ErrorKind {
    let lower = e.to_ascii_lowercase();
    if lower.contains("rate limit") || lower.contains("too many requests") {
        return ErrorKind::RateLimited;
    }
    match e {
        "EAPI:Invalid key" | "EAPI:Invalid signature" | "EAPI:Invalid nonce" | "EAPI:Bad request" => ErrorKind::Auth,
        "EGeneral:Permission denied" | "EAPI:Feature disabled" => ErrorKind::Permission,
        "EQuery:Unknown asset pair" | "EQuery:Unknown asset" => ErrorKind::NotFound,
        "EService:Unavailable" | "EService:Busy" | "EService:Deadline elapsed" | "EGeneral:Internal error" => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

/// The error a frame carries, if any: the envelope's `error` array first, then the status.
pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "kraken") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, .. } = frame else { return None };
    let env = serde_json::from_slice::<Envelope>(body).ok();
    match env.as_ref().and_then(|e| e.error.as_ref()) {
        Some(errs) if errs.is_empty() => http::classify(frame, "kraken"),
        Some(errs) => {
            let first = errs.first().cloned().unwrap_or_default();
            let kind = kind_of(&first);
            // "bad symbol" is the phrase OTW's notice classifier reads as a symbol problem.
            let message = if kind == ErrorKind::NotFound {
                format!("kraken {endpoint_id}: bad symbol ({})", errs.join("; "))
            } else {
                format!("kraken {endpoint_id}: {}", errs.join("; "))
            };
            let mut e = ConnectorError::new(kind, message).with_code(first);
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        None => http::classify(frame, "kraken").or_else(|| Some(ConnectorError::decode(format!("kraken {endpoint_id}: no error array")))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_error_array_decides_not_the_status() {
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"error":[],"result":{}}"#)).is_none());
        let e = from_frame(&http_frame(1, "x", 200, r#"{"error":["EQuery:Unknown asset pair"]}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.message.contains("bad symbol"));
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EAPI:Rate limit exceeded"]}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EGeneral:Too many requests"]}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EAPI:Invalid key"]}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EGeneral:Permission denied"]}"#)).unwrap().kind, ErrorKind::Permission);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EService:Unavailable"]}"#)).unwrap().kind, ErrorKind::Transient);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EGeneral:Invalid arguments"]}"#)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>")).unwrap().kind, ErrorKind::Transient);
        assert_eq!(from_frame(&http_frame(1, "x", 200, "[]")).unwrap().kind, ErrorKind::Decode);
    }
}
