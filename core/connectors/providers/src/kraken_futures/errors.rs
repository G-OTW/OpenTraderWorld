//! Kraken Futures refuses in three shapes, none of which the status alone decides:
//!
//!   - `/derivatives/api/v3`: HTTP 200 with `{"result":"error","error":"<word>"}` (or an
//!     `errors` array of the same words);
//!   - the same host's 403: `{"result":"error","errors":[{"code":95,"message":".."}]}`;
//!   - `/api/history/v3`: HTTP 401 `{"status":"unauthorized","reason":".."}`, HTTP 429 as
//!     plain text with `rate-limit-reset` (seconds).
//!
//! `"result":"success"` only says the request was read: an order's own outcome lives in its
//! status word, which is the order path's business, not this table's. Table mirrored in
//! PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    errors: Option<Vec<ErrorItem>>,
    /// The history API's refusal.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorItem {
    Word(String),
    Coded {
        #[serde(default)]
        code: Option<serde_json::Value>,
        #[serde(default)]
        message: Option<String>,
    },
}

impl ErrorItem {
    fn text(&self) -> String {
        match self {
            ErrorItem::Word(w) => w.clone(),
            ErrorItem::Coded { code, message } => match (code, message) {
                (Some(c), Some(m)) => format!("{m} (code {c})"),
                (None, Some(m)) => m.clone(),
                (Some(c), None) => format!("code {c}"),
                (None, None) => String::new(),
            },
        }
    }

    fn code(&self) -> Option<String> {
        match self {
            ErrorItem::Word(w) => Some(w.clone()),
            ErrorItem::Coded { code: Some(c), .. } => Some(c.to_string().trim_matches('"').to_string()),
            ErrorItem::Coded { .. } => None,
        }
    }
}

/// A documented error word to its kind (the `error` enum of every `/derivatives` answer).
pub fn kind_of(word: &str) -> ErrorKind {
    match word {
        "authenticationError" | "nonceBelowThreshold" | "nonceDuplicate" => ErrorKind::Auth,
        "apiLimitExceeded" => ErrorKind::RateLimited,
        "accountInactive" | "cashAccountTypeAccessForbidden" | "95" => ErrorKind::Permission,
        "notFound" | "accountNotFound" | "cashAccountTypeNotFound" => ErrorKind::NotFound,
        "Server Error" | "Unavailable" | "unknownError" | "marketUnavailable" => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

/// What the user does about a refusal, where the word alone does not say it.
fn advice(word: &str) -> Option<&'static str> {
    match word {
        "authenticationError" => Some("check the key and its private key, and that the key belongs to this environment (live or demo)"),
        "nonceBelowThreshold" | "nonceDuplicate" => {
            Some("another application signs with this key using a larger nonce: give OpenTraderWorld a key of its own")
        }
        _ => None,
    }
}

/// The error a frame carries, if any.
pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "kraken_futures") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    let env = serde_json::from_slice::<Envelope>(body).ok();
    let words: Vec<(String, Option<String>)> = match &env {
        Some(e) if e.result.as_deref() == Some("error") || e.error.is_some() || e.errors.as_ref().is_some_and(|v| !v.is_empty()) => {
            let mut w: Vec<(String, Option<String>)> = e.error.iter().map(|x| (x.clone(), Some(x.clone()))).collect();
            w.extend(e.errors.iter().flatten().map(|i| (i.text(), i.code())));
            if w.is_empty() {
                w.push(("error".into(), None));
            }
            w
        }
        Some(e) if e.status.is_some() && !(200..300).contains(status) => {
            let s = e.status.clone().unwrap_or_default();
            let reason = e.reason.clone().unwrap_or_default();
            let kind = match s.as_str() {
                "unauthorized" => ErrorKind::Auth,
                _ => http::classify(frame, "kraken_futures").map(|c| c.kind).unwrap_or(ErrorKind::Unknown),
            };
            let mut err = ConnectorError::new(kind, format!("kraken_futures {endpoint_id}: {s}: {reason}")).with_code(s);
            err.retry_after_ms = reset_ms(frame);
            return Some(err);
        }
        _ => {
            return http::classify(frame, "kraken_futures").map(|mut e| {
                if e.retry_after_ms.is_none() {
                    e.retry_after_ms = reset_ms(frame);
                }
                e
            });
        }
    };
    let (first, code) = words.first().cloned().unwrap_or_default();
    let kind = code.as_deref().map(kind_of).unwrap_or(ErrorKind::InvalidRequest);
    let said = words.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join("; ");
    let message = match code.as_deref().and_then(advice) {
        Some(a) => format!("kraken_futures {endpoint_id}: {said}: {a}"),
        None => format!("kraken_futures {endpoint_id}: {said}"),
    };
    let mut e = ConnectorError::new(kind, message).with_code(code.unwrap_or(first));
    e.retry_after_ms = http::retry_after_ms(frame).or_else(|| reset_ms(frame));
    Some(e)
}

/// The history API's `rate-limit-reset`, in seconds.
fn reset_ms(frame: &RawFrame) -> Option<u64> {
    frame.header("rate-limit-reset").and_then(|v| v.trim().parse::<u64>().ok()).map(|s| s.saturating_mul(1000))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::CorrId;
    use otw_connector_testkit::http_frame;

    /// Each answer verbatim from the doc, source tagged on the line.
    #[test]
    fn the_result_word_decides_not_the_status() {
        // https://docs.kraken.com/exchange/guides/futures/rest (Calls and Returns)
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"result":"success","serverTime":"2016-02-25T09:45:53.818Z","sendStatus":{"receivedTime":"2016-02-25T09:45:53.601Z","status":"placed","order_id":"c18f0c17-9971-40e6-8e5b10df05d422f0"}}"#)).is_none());
        // https://docs.kraken.com/exchange/guides/futures/ratelimits (Example)
        let e = from_frame(&http_frame(1, "x", 200, r#"{"result":"error","serverTime":"2016-02-25T09:45:53.818Z","error":"apiLimitExceeded"}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::RateLimited, Some("apiLimitExceeded")));
        // Measured on the live host with no key, 2026-11-12 (synthetic in so far as it is not in the doc).
        let e = from_frame(&http_frame(1, "accounts", 200, r#"{"result":"error","error":"authenticationError","serverTime":"2026-11-12T10:18:41.833Z"}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::Auth);
        assert!(e.message.contains("live or demo"), "{}", e.message);
        let e = from_frame(&http_frame(1, "x", 200, r#"{"result":"error","error":"nonceBelowThreshold","serverTime":"2016-02-25T09:45:53.818Z"}"#)).unwrap();
        assert!(e.kind == ErrorKind::Auth && e.message.contains("key of its own"));
        for (w, k) in [
            ("Server Error", ErrorKind::Transient),
            ("unknownError", ErrorKind::Transient),
            ("Unavailable", ErrorKind::Transient),
            ("accountInactive", ErrorKind::Permission),
            ("invalidArgument", ErrorKind::InvalidRequest),
            ("requiredArgumentMissing", ErrorKind::InvalidRequest),
            ("notFound", ErrorKind::NotFound),
        ] {
            let body = format!(r#"{{"result":"error","serverTime":"2016-02-25T09:45:53.818Z","error":"{w}"}}"#);
            assert_eq!(from_frame(&http_frame(1, "x", 200, &body)).unwrap().kind, k, "{w}");
        }
    }

    #[test]
    fn the_coded_403_and_the_history_refusals_are_read() {
        // Shape from the OpenAPI `ContractAccessForbidden` response (code 95); synthetic message.
        let e = from_frame(&http_frame(1, "x", 403, r#"{"result":"error","serverTime":"2026-11-12T00:00:00Z","errors":[{"code":95,"message":"Contract access forbidden"}]}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::Permission, Some("95")));
        // Measured on the live history host with no key, 2026-11-12.
        let e = from_frame(&http_frame(1, "executions", 401, r#"{"status":"unauthorized","reason":"You are not authorized to access this endpoint."}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::Auth);
        // 429 as plain text with `rate-limit-reset` (history OpenAPI); synthetic body.
        let frame = RawFrame::Http {
            corr_id: CorrId(1),
            endpoint_id: "executions",
            status: 429,
            headers: vec![("rate-limit-reset".into(), "12".into())],
            body: bytes::Bytes::from_static(b"Too many requests"),
        };
        let e = from_frame(&frame).unwrap();
        assert_eq!((e.kind, e.retry_after_ms), (ErrorKind::RateLimited, Some(12_000)));
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>")).unwrap().kind, ErrorKind::Transient);
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"accountUid":"a","len":0,"elements":[]}"#)).is_none());
    }
}
