//! Binance error codes to canonical kinds. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};

use super::dto::ApiError;

fn kind_of(code: i64) -> ErrorKind {
    match code {
        // UNKNOWN, DISCONNECTED, TIMEOUT: the server side failed us.
        -1000 | -1001 | -1007 => ErrorKind::Transient,
        // TOO_MANY_REQUESTS; TOO_MANY_ORDERS (the order buckets).
        -1003 | -1015 => ErrorKind::RateLimited,
        // UNAUTHORIZED, INVALID_TIMESTAMP, INVALID_SIGNATURE, bad key format, rejected key.
        -1002 | -1021 | -1022 | -2014 | -2015 => ErrorKind::Auth,
        // BAD_SYMBOL.
        -1121 => ErrorKind::NotFound,
        // Malformed or illegal parameters (-11xx).
        -1199..=-1100 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Unknown,
    }
}

/// The error a frame carries, if any: Binance's own code first, the HTTP status second.
pub fn from_frame(frame: &RawFrame, name: &str) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, name) {
        return Some(e);
    }
    let generic = http::classify(frame, name)?;
    let RawFrame::Http { body, endpoint_id, .. } = frame else {
        return Some(generic);
    };
    let Ok(api) = serde_json::from_slice::<ApiError>(body) else {
        return Some(generic);
    };
    let kind = match kind_of(api.code) {
        // An unknown code keeps what the status said.
        ErrorKind::Unknown => generic.kind,
        k => k,
    };
    // "bad symbol" is the phrase OTW's notice classifier reads as a symbol problem.
    let message = if api.code == -1121 {
        format!("{name} {endpoint_id}: bad symbol ({})", api.msg)
    } else {
        format!("{name} {endpoint_id}: {} (code {})", api.msg, api.code)
    };
    let mut e = ConnectorError::new(kind, message).with_code(api.code.to_string());
    e.retry_after_ms = generic.retry_after_ms;
    Some(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    fn from_frame_t(f: &RawFrame) -> Option<ConnectorError> {
        from_frame(f, "binance")
    }

    #[test]
    fn codes_win_over_status() {
        let e = from_frame_t(&http_frame(1, "klines", 400, r#"{"code":-1121,"msg":"Invalid symbol."}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.message.contains("bad symbol"));
        let e = from_frame_t(&http_frame(1, "klines", 400, r#"{"code":-1100,"msg":"Illegal characters"}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::InvalidRequest);
        let e = from_frame_t(&http_frame(1, "klines", 429, r#"{"code":-1003,"msg":"Too many"}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::RateLimited);
        let e = from_frame_t(&http_frame(1, "order", 400, r#"{"code":-1015,"msg":"Too many new orders; current limit is 50 orders per 10 SECOND."}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::RateLimited, "the order buckets");
        let e = from_frame_t(&http_frame(1, "klines", 400, r#"{"code":-9999,"msg":"new"}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::InvalidRequest, "unknown code keeps the status");
        let e = from_frame_t(&http_frame(1, "klines", 502, "<html>bad gateway</html>")).unwrap();
        assert_eq!(e.kind, ErrorKind::Transient);
        assert!(from_frame_t(&http_frame(1, "klines", 200, "[]")).is_none());
    }
}
