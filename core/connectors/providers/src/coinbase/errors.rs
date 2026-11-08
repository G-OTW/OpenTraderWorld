//! Coinbase answers a refusal with its HTTP status and a short body (`{"message":"NotFound"}`
//! on the Exchange API, `{"error":..,"message":..}` on Advanced Trade): the status decides.
//! Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};

/// The error a frame carries, if any. A 404 on a product path is an unknown product.
pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "coinbase") {
        return Some(e);
    }
    let mut e = http::classify(frame, "coinbase")?;
    if e.kind == ErrorKind::NotFound {
        // "bad symbol" is the phrase OTW's notice classifier reads as a symbol problem.
        e.message = format!("{}: bad symbol", e.message);
    }
    Some(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_status_decides() {
        assert!(from_frame(&http_frame(1, "x", 200, "[]")).is_none());
        let e = from_frame(&http_frame(1, "candles", 404, r#"{"message":"NotFound"}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.message.contains("bad symbol"));
        assert_eq!(from_frame(&http_frame(1, "x", 429, r#"{"message":"Slow down"}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 401, r#"{"error":"UNAUTHENTICATED"}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 403, r#"{"error":"PERMISSION_DENIED"}"#)).unwrap().kind, ErrorKind::Permission);
        let e = from_frame(&http_frame(1, "x", 400, r#"{"message":"granularity too small for the requested time range"}"#)).unwrap();
        assert!(e.kind == ErrorKind::InvalidRequest && e.message.contains("granularity"));
        assert_eq!(from_frame(&http_frame(1, "x", 503, "down")).unwrap().kind, ErrorKind::Transient);
    }
}
