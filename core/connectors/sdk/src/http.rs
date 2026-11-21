//! Default reading of an HTTP status. A mapper checks its venue's own error codes first and
//! falls back to this.

use otw_connector_model::{ConnectorError, ErrorKind};

use crate::native::RawFrame;

/// `Retry-After` in milliseconds: delta-seconds only (HTTP dates are not used by venues).
pub fn retry_after_ms(frame: &RawFrame) -> Option<u64> {
    frame.header("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).map(|s| s.saturating_mul(1000))
}

/// A request that got no answer in time: transient by definition.
pub fn timeout(frame: &RawFrame, provider: &str) -> Option<ConnectorError> {
    match frame {
        RawFrame::Timeout { endpoint_id, .. } => {
            Some(ConnectorError::new(ErrorKind::Transient, format!("{provider} {endpoint_id}: no answer in time")))
        }
        _ => None,
    }
}

/// `None` for a 2xx. Otherwise the error a status means on its own.
pub fn classify(frame: &RawFrame, provider: &str) -> Option<ConnectorError> {
    let RawFrame::Http { status, body, endpoint_id, .. } = frame else {
        return None;
    };
    let status = *status;
    if (200..300).contains(&status) {
        return None;
    }
    let snippet: String = String::from_utf8_lossy(body).chars().take(200).collect();
    let (kind, what) = match status {
        429 | 418 => (ErrorKind::RateLimited, "rate limited"),
        401 => (ErrorKind::Auth, "credentials refused"),
        403 => (ErrorKind::Permission, "not allowed"),
        404 => (ErrorKind::NotFound, "not found"),
        408 => (ErrorKind::Transient, "request timeout"),
        400..=499 => (ErrorKind::InvalidRequest, "request rejected"),
        500..=599 => (ErrorKind::Transient, "venue error"),
        _ => (ErrorKind::Unknown, "unexpected status"),
    };
    let mut e = ConnectorError::new(kind, format!("{provider} {endpoint_id}: {what} (HTTP {status}): {snippet}"))
        .with_code(status.to_string());
    if let Some(ms) = retry_after_ms(frame) {
        e = e.with_retry_after_ms(ms);
    }
    Some(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use otw_connector_model::CorrId;

    fn frame(status: u16, headers: &[(&str, &str)]) -> RawFrame {
        RawFrame::Http {
            corr_id: CorrId(1),
            endpoint_id: "x",
            status,
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: Bytes::from_static(b"{}"),
        }
    }

    #[test]
    fn statuses_map_to_kinds() {
        assert!(classify(&frame(200, &[]), "p").is_none());
        let rl = classify(&frame(429, &[("Retry-After", "3")]), "p").unwrap();
        assert_eq!((rl.kind, rl.retry_after_ms), (ErrorKind::RateLimited, Some(3000)));
        assert_eq!(classify(&frame(418, &[]), "p").unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(classify(&frame(401, &[]), "p").unwrap().kind, ErrorKind::Auth);
        assert_eq!(classify(&frame(403, &[]), "p").unwrap().kind, ErrorKind::Permission);
        assert_eq!(classify(&frame(404, &[]), "p").unwrap().kind, ErrorKind::NotFound);
        assert_eq!(classify(&frame(400, &[]), "p").unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(classify(&frame(503, &[]), "p").unwrap().kind, ErrorKind::Transient);
    }
}
