//! Kraken answers most refusals with HTTP 200 and an `error` array of `Category:Message`
//! strings: success is an array with no error in it, not the status. A string whose severity
//! is `W` is a warning on a request that succeeded ("`result` and `error` for successful
//! requests or those with warnings", REST introduction), never an error. Table mirrored in
//! PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    error: Option<Vec<String>>,
}

/// The error guide's codes a read can meet, by kind
/// (https://docs.kraken.com/exchange/guides/general/errors). Matched by prefix, as the trade
/// path does: the venue appends detail (`EGeneral:Internal error:5`).
const KINDS: &[(&str, ErrorKind)] = &[
    ("EAPI:Invalid key", ErrorKind::Auth),
    ("EAPI:Invalid signature", ErrorKind::Auth),
    ("EAPI:Invalid nonce", ErrorKind::Auth),
    ("EGeneral:Temporary lockout", ErrorKind::Auth),
    ("EAuth:Account temporary disabled", ErrorKind::Auth),
    ("EAuth:Account unconfirmed", ErrorKind::Auth),
    ("EGeneral:Permission denied", ErrorKind::Permission),
    ("EAccount:Invalid permissions", ErrorKind::Permission),
    ("EQuery:Unknown asset pair", ErrorKind::NotFound),
    ("EService:Unavailable", ErrorKind::Transient),
    ("EService:Deadline elapsed", ErrorKind::Transient),
    ("EGeneral:Internal error", ErrorKind::Transient),
];

fn kind_of(e: &str) -> ErrorKind {
    let lower = e.to_ascii_lowercase();
    // "EService: Throttled: [UNIX timestamp]" (too many concurrent requests), spaces and all.
    if lower.contains("rate limit") || lower.contains("too many requests") || lower.replace(' ', "").starts_with("eservice:throttled") {
        return ErrorKind::RateLimited;
    }
    super::trade_errors::longest_prefix(KINDS.iter().map(|(c, _)| *c), e)
        .and_then(|code| KINDS.iter().find(|(c, _)| *c == code))
        .map_or(ErrorKind::InvalidRequest, |(_, k)| *k)
}

/// The error a frame carries, if any: the envelope's `error` array first, then the status.
pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "kraken") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, .. } = frame else { return None };
    let env = serde_json::from_slice::<Envelope>(body).ok();
    // Warnings (`W...`) ride a success: only the errors are read.
    let errors = env.as_ref().and_then(|e| e.error.as_ref()).map(|all| all.iter().filter(|e| !e.trim_start().starts_with('W')).cloned().collect::<Vec<String>>());
    match errors.as_ref() {
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

    #[test]
    fn a_warning_rides_a_success_and_a_throttle_is_a_rate_limit() {
        // Synthetic: the REST introduction's `W` severity on a request that succeeded.
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"error":["WGeneral:Deprecated field"],"result":{}}"#)).is_none());
        let e = from_frame(&http_frame(1, "x", 200, r#"{"error":["WGeneral:Deprecated field","EGeneral:Invalid arguments"]}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::InvalidRequest, Some("EGeneral:Invalid arguments")));
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"error":["EService: Throttled: 1700000000"]}"#)).unwrap().kind, ErrorKind::RateLimited);
    }

    /// Codes carry detail after them ("The error may include a numeric code (e.g.
    /// `EGeneral:Internal error:5`)"): matched by prefix on a `:`, as the trade path does.
    #[test]
    fn a_code_with_its_detail_reads_as_the_code() {
        let kind = |e: &str| from_frame(&http_frame(1, "x", 200, &format!(r#"{{"error":["{e}"]}}"#))).unwrap().kind;
        assert_eq!(kind("EGeneral:Internal error:5"), ErrorKind::Transient);
        assert_eq!(kind("EService: Unavailable"), ErrorKind::Transient);
        assert_eq!(kind("EGeneral:Temporary lockout"), ErrorKind::Auth);
        assert_eq!(kind("EAccount:Invalid permissions"), ErrorKind::Permission);
        assert_eq!(kind("EQuery:Unknown asset pair"), ErrorKind::NotFound);
        // Not on a `:` boundary: not the code.
        assert_eq!(kind("EAPI:Invalid keys"), ErrorKind::InvalidRequest);
    }
}
