//! Bitfinex's order error table (onboarding spec 8), from the glossary's "Error/Info Codes"
//! (https://docs.bitfinex.com/docs/abbreviations-glossary, read 2026-11-12) and the rate-limit
//! answer of https://docs.bitfinex.com/docs/requirements-and-limitations: every code an order
//! call (place, change, cancel, batch cancel, the reads the order path makes) can answer, mapped
//! to one canonical kind. A code not listed here is `Unknown` and alerts. Mirrored in
//! PROVIDER.md ("Trading", "Errors"); a test keeps the two in step.
//!
//! A refusal comes in one of three shapes, so the body is read before the HTTP status:
//! - `["error", CODE, "message"]`, with HTTP 500 (the doc's own example on the collateral page;
//!   every refusal OTW measured on the read path came so). A 500 is no lost answer when it
//!   carries this array: the code decides.
//! - A notification with HTTP 200, `[MTS, TYPE, MESSAGE_ID, null, DATA, CODE, STATUS, TEXT]`,
//!   whose `STATUS` is not `SUCCESS` ("SUCCESS, ERROR, FAILURE, ...", Submit Order). `ERROR` and
//!   `FAILURE` are refusals in the venue's words; the doc lists no code for them (`CODE` is
//!   "W.I.P."). Any other status word is an answer nobody mapped.
//! - `{"error": "ERR_RATE_LIMIT"}`: the IP "is blocked for 60 seconds".
//!
//! The read path keeps its own mapping (`errors.rs`): there 10000, 10008, 11000 and 20060 are a
//! transient read failure to retry. On the order path they say the venue may have acted, so they
//! are `Unknown` here: looked up, never sent again.

use otw_connector_model::ErrorKind;
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde_json::value::RawValue;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};

/// How long a rate-limited IP is blocked when the answer states no wait ("the IP is blocked for
/// 60 seconds", requirements-and-limitations).
pub(super) const RATE_BLOCK_MS: u64 = 60_000;

/// `(code, canonical, the doc's words)`.
pub(super) const ERRORS: &[ErrorRow] = &[
    // The venue may have acted: looked up by the client id, never sent again.
    ("10000", Unknown, "ERR_UNK: unknown error"),
    ("10001", Unknown, "ERR_GENERIC: generic error"),
    ("10008", Unknown, "ERR_CONCURRENCY: concurrency error"),
    ("11000", Unknown, "ERR_READY: not ready, try again later"),
    ("20051", Unknown, "EVT_STOP: websocket server stopping, please reconnect later"),
    ("20060", Unknown, "EVT_RESYNC_START: maintenance, please reconnect later"),
    ("20061", Unknown, "EVT_RESYNC_STOP: maintenance ended, please reconnect"),
    ("5000", Unknown, "EVT_INFO: info message"),
    // The request itself was refused: nothing was done.
    ("10020", InvalidRequest, "ERR_PARAMS: request parameters error"),
    ("10050", InvalidRequest, "ERR_CONF_FAIL: configuration setup failed"),
    ("10200", InvalidRequest, "ERR_UNAUTH_FAIL: error in un-authentication request"),
    ("10300", InvalidRequest, "ERR_SUB_FAIL: failed channel subscription"),
    ("10301", InvalidRequest, "ERR_SUB_MULTI: already subscribed"),
    ("10302", InvalidRequest, "ERR_SUB_UNK: unknown channel"),
    ("10305", InvalidRequest, "ERR_SUB_LIMIT: reached limit of open channels"),
    ("10400", InvalidRequest, "ERR_UNSUB_FAIL: channel not found"),
    ("10401", InvalidRequest, "ERR_UNSUB_NOT: not subscribed"),
    // The key: checked before the call is processed.
    ("10100", Auth, "ERR_AUTH_FAIL: failed authentication"),
    ("10111", Auth, "ERR_AUTH_PAYLOAD: error in authentication request payload"),
    ("10112", Auth, "ERR_AUTH_SIG: error in authentication request signature"),
    ("10113", Auth, "ERR_AUTH_HMAC: error in authentication request encryption"),
    ("10114", Auth, "ERR_AUTH_NONCE: error in authentication request nonce"),
    // The rate limit (requirements-and-limitations; 11010 `ratelimit: error` is the array form,
    // the read path maps it so too).
    ("ERR_RATE_LIMIT", RateLimited, "rate limited: the IP is blocked for 60 seconds"),
    ("11010", RateLimited, "ratelimit: error"),
];

fn row(code: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|r| r.0 == code)
}

/// A coded answer, mapped. 10001 is the generic code; two of its texts are refusals, not a lost
/// answer: a key without the right (`api_key: permission invalid`, ccxt's sample, the read path
/// reads it so too), and `Invalid order: ...`, the words an order refusal carries (ccxt).
pub(super) fn mapped(code: &str, msg: &str) -> VenueError {
    let msg = msg.trim();
    if code == "10001" && msg.to_ascii_lowercase().contains("permission") {
        return VenueError::new(Permission, Some(code.to_string()), format!("Bitfinex refused it: {msg} (code {code}): the key needs write access to Orders"));
    }
    if code == "10001" && msg.to_ascii_lowercase().starts_with("invalid order") {
        return VenueError::new(Rejected(Refusal::Other), Some(code.to_string()), format!("Bitfinex refused it: {msg} (code {code})"));
    }
    match row(code) {
        Some((_, Unknown, _)) => VenueError::new(Unknown, Some(code.to_string()), format!("Bitfinex answered: {msg} (code {code}), so the order may have gone through")),
        Some((_, canon, _)) => VenueError::new(*canon, Some(code.to_string()), format!("Bitfinex refused it: {msg} (code {code})")),
        None => VenueError::unmapped(Some(code.to_string()), format!("Bitfinex answered code {code} ({msg}), which OpenTraderWorld does not map")),
    }
}

/// `["error", code, "message"]`.
fn error_array(body: &[u8]) -> Option<(String, String)> {
    let parts: Vec<Box<RawValue>> = serde_json::from_slice(body).ok()?;
    let tag: String = serde_json::from_str(parts.first()?.get()).ok()?;
    if tag != "error" {
        return None;
    }
    let code = parts.get(1).map(|c| c.get().trim().trim_matches('"').to_string()).unwrap_or_default();
    let msg = parts.get(2).and_then(|m| serde_json::from_str::<String>(m.get()).ok()).unwrap_or_default();
    Some((code, msg))
}

/// A notification's type, code, status and text: `[MTS, TYPE, MESSAGE_ID, null, DATA, CODE,
/// STATUS, TEXT]`.
pub(super) struct Notification {
    pub kind: String,
    pub code: Option<String>,
    pub status: String,
    pub text: String,
}

pub(super) fn notification(body: &[u8]) -> Option<Notification> {
    let parts: Vec<Box<RawValue>> = serde_json::from_slice(body).ok()?;
    if parts.len() < 8 {
        return None;
    }
    let kind: String = serde_json::from_str(parts[1].get()).ok()?;
    let status: String = serde_json::from_str(parts[6].get()).ok()?;
    let code = Some(parts[5].get().trim().trim_matches('"').to_string()).filter(|c| !c.is_empty() && c != "null");
    let text = serde_json::from_str::<String>(parts[7].get()).unwrap_or_default();
    Some(Notification { kind, code, status, text })
}

/// What a notification whose status is not `SUCCESS` means. `ERROR` and `FAILURE` refuse, in
/// the venue's words. On a cancel (`oc-req`, `oc_multi-req`) the refusal is reconciled by
/// reading the order (already final: no error; still working: the refusal is shown).
fn refused(n: &Notification) -> VenueError {
    let words = if n.text.trim().is_empty() { n.status.clone() } else { n.text.trim().to_string() };
    let message = match &n.code {
        Some(c) => format!("Bitfinex refused it: {words} (code {c})"),
        None => format!("Bitfinex refused it: {words}"),
    };
    match n.status.as_str() {
        "ERROR" | "FAILURE" if n.kind.starts_with("oc") => VenueError::new(Rejected(Refusal::Final), n.code.clone(), message),
        "ERROR" | "FAILURE" => VenueError::new(Rejected(Refusal::Other), n.code.clone(), message),
        other => VenueError::unmapped(n.code.clone(), format!("Bitfinex answered {other:?} to the {} request ({words}), which OpenTraderWorld does not map", n.kind)),
    }
}

/// The error an order call's answer carries, mapped; `None` for a success.
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        let what = http::timeout(frame, "bitfinex").map(|e| e.message).unwrap_or_else(|| "no answer".into());
        return Some(VenueError::new(Unknown, None, format!("Bitfinex: {what}")));
    };
    let status = *status;
    let text = body.trim_ascii_start();
    let mut e = if let Some((code, msg)) = error_array(text) {
        mapped(&code, &msg)
    } else if let Some(n) = notification(text).filter(|n| n.status != "SUCCESS") {
        refused(&n)
    } else if let Some(err) = text.starts_with(b"{").then(|| serde_json::from_slice::<serde_json::Value>(text).ok()).flatten().and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string)) {
        match err.as_str() {
            "ERR_RATE_LIMIT" => VenueError::new(RateLimited, Some(err.clone()), "Bitfinex refused it: rate limited, the IP is blocked for 60 seconds (code ERR_RATE_LIMIT)"),
            // An outage object (`temporarily_unavailable`) or any other word: the venue may
            // have acted.
            _ => VenueError::unmapped(Some(err.clone()), format!("Bitfinex answered code {err} (HTTP {status}), which OpenTraderWorld does not map")),
        }
    } else {
        let c = http::classify(frame, "bitfinex")?;
        let canon = match c.kind {
            ErrorKind::RateLimited => RateLimited,
            ErrorKind::Auth => Auth,
            ErrorKind::Permission => Permission,
            // A path or a request the venue refused before reading it.
            ErrorKind::InvalidRequest | ErrorKind::NotFound if (400..500).contains(&status) => InvalidRequest,
            // A 5xx without the error array, or an answer that cannot be read: the venue may
            // have acted.
            _ => Unknown,
        };
        VenueError::new(canon, c.provider_code.clone(), format!("bitfinex {endpoint_id}: {}", c.message))
    };
    if status == 429 && e.canon != RateLimited {
        e = VenueError::new(RateLimited, e.code.clone(), e.message.clone());
    }
    if e.canon == RateLimited {
        e.retry_after_ms = Some(http::retry_after_ms(frame).unwrap_or(RATE_BLOCK_MS));
    }
    Some(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn every_listed_code_maps_and_an_unlisted_one_alerts() {
        for (code, canon, _) in ERRORS.iter().filter(|r| r.0.parse::<i64>().is_ok()) {
            let e = decode(&http_frame(1, "orderSubmit", 500, &format!(r#"["error",{code},"x"]"#))).unwrap();
            assert_eq!((e.canon, e.mapped), (*canon, true), "{code}");
        }
        let e = decode(&http_frame(1, "orderSubmit", 500, r#"["error",99999,"new"]"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        assert_eq!(e.message, "Bitfinex answered code 99999 (new), which OpenTraderWorld does not map");
    }

    /// The read path retries these; the order path must not (the venue may have acted).
    #[test]
    fn the_codes_the_read_path_retries_are_unknown_on_an_order() {
        for code in ["10000", "10008", "11000", "20060"] {
            let body = format!(r#"["error",{code},"m"]"#);
            assert_eq!(decode(&http_frame(1, "orderSubmit", 500, &body)).unwrap().canon, Unknown, "{code}");
            assert_eq!(super::super::errors::from_frame(&http_frame(1, "x", 500, &body)).unwrap().kind, ErrorKind::Transient, "reads unchanged");
        }
    }

    #[test]
    fn a_refusal_names_the_venue_code_and_words() {
        let e = decode(&http_frame(1, "orderSubmit", 500, r#"["error",10020,"amount: invalid"]"#)).unwrap();
        assert_eq!((e.canon, e.message.as_str(), e.code.as_deref()), (InvalidRequest, "Bitfinex refused it: amount: invalid (code 10020)", Some("10020")));
        assert_eq!(decode(&http_frame(1, "orderSubmit", 500, r#"["error",10114,"nonce: small"]"#)).unwrap().canon, Auth);
        let p = decode(&http_frame(1, "orderSubmit", 500, r#"["error",10001,"api_key: permission invalid"]"#)).unwrap();
        assert_eq!(p.canon, Permission);
        let r = decode(&http_frame(1, "orderSubmit", 500, r#"["error",10001,"Invalid order: not enough exchange balance for 0.1 BTCUSD at 10000"]"#)).unwrap();
        assert_eq!((r.canon, r.message.as_str()), (Rejected(Refusal::Other), "Bitfinex refused it: Invalid order: not enough exchange balance for 0.1 BTCUSD at 10000 (code 10001)"));
        assert_eq!(decode(&http_frame(1, "orderSubmit", 500, r#"["error",10001,"something"]"#)).unwrap().canon, Unknown, "any other generic answer");
        let rl = decode(&http_frame(1, "orderSubmit", 500, r#"["error",11010,"ratelimit: error"]"#)).unwrap();
        assert_eq!((rl.canon, rl.retry_after_ms), (RateLimited, Some(60_000)));
    }

    /// A 200 notification refuses by its status, on a submit and on a cancel alike; `SUCCESS`
    /// is none. (`synthetic` bodies: the documented notification with its status changed.)
    #[test]
    fn a_notification_refuses_by_its_status() {
        let ok = r#"[1678987199,"on-req",null,null,[[1747566428,null,1678987199446,"tBTCUSD",1678987199446,1678987199448,0.1,0.1,"EXCHANGE LIMIT",null,null,null,0,"ACTIVE",null,null,10000,0,0,0,null,null,null,0,0,null,null,null,"API>BFX",null,null,null]],null,"SUCCESS","Submitting exchange limit buy order for 0.1 BTC."]"#;
        assert!(decode(&http_frame(1, "orderSubmit", 200, ok)).is_none());
        let no = r#"[1678987199,"on-req",null,null,[],null,"ERROR","Invalid order: not enough exchange balance for 0.1 BTCUSD at 10000"]"#;
        let e = decode(&http_frame(1, "orderSubmit", 200, no)).unwrap();
        assert_eq!((e.canon, e.message.as_str()), (Rejected(Refusal::Other), "Bitfinex refused it: Invalid order: not enough exchange balance for 0.1 BTCUSD at 10000"));
        let failure = no.replace("\"ERROR\"", "\"FAILURE\"");
        assert_eq!(decode(&http_frame(1, "orderSubmit", 200, &failure)).unwrap().canon, Rejected(Refusal::Other));
        let cancel = r#"[1678989283488,"oc-req",null,null,[],null,"ERROR","Order not found."]"#;
        assert_eq!(decode(&http_frame(1, "orderCancel", 200, cancel)).unwrap().canon, Rejected(Refusal::Final), "a refused cancel is reconciled");
        let odd = no.replace("\"ERROR\"", "\"PENDING\"");
        let e = decode(&http_frame(1, "orderSubmit", 200, &odd)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
    }

    #[test]
    fn a_5xx_or_a_timeout_is_unknown_and_a_rate_limit_holds_a_minute() {
        assert_eq!(decode(&http_frame(1, "orderSubmit", 502, "<html>bad gateway</html>")).unwrap().canon, Unknown);
        let down = r#"{"code":503,"error":"temporarily_unavailable","error_description":"Sorry, the service is temporarily unavailable."}"#;
        assert_eq!(decode(&http_frame(1, "orderSubmit", 503, down)).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "orderSubmit" };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
        let r = decode(&http_frame(1, "orderSubmit", 429, r#"{"error":"ERR_RATE_LIMIT"}"#)).unwrap();
        assert_eq!((r.canon, r.retry_after_ms), (RateLimited, Some(60_000)));
        assert_eq!(decode(&http_frame(1, "orderSubmit", 404, "Cannot POST /api/v2/auth/w/nope")).unwrap().canon, InvalidRequest);
        assert!(decode(&http_frame(1, "orders", 200, "[]")).is_none());
    }

    /// PROVIDER.md ("Trading", "Errors") names every code of the table.
    #[test]
    fn provider_md_names_every_code() {
        let md = include_str!("PROVIDER.md");
        let errors = md.split("### Errors").nth(1).unwrap();
        for (code, _, _) in ERRORS {
            assert!(errors.contains(code), "{code} not in PROVIDER.md");
        }
    }

    #[test]
    fn no_code_is_listed_twice() {
        let mut codes: Vec<&str> = ERRORS.iter().map(|r| r.0).collect();
        codes.sort_unstable();
        let n = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), n);
    }
}
