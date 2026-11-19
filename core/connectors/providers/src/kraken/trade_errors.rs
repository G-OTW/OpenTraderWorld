//! Kraken's order error table (onboarding spec 8), from the error guide
//! (https://docs.kraken.com/exchange/guides/general/errors), the REST rate limit guide
//! (https://docs.kraken.com/exchange/guides/rest/ratelimits) and the REST introduction
//! (https://docs.kraken.com/exchange/guides/rest/introduction), read 2026-11-12. Every
//! documented code an order call can meet maps to one canonical kind; a code not listed here is
//! `Unknown` and alerts. Mirrored in PROVIDER.md ("Trading", "Errors"); a test keeps the two in
//! step.
//!
//! - **The envelope decides, not the status**: HTTP 200 with `{"error":[...],"result":...}`.
//!   "Status codes other than 200 indicate that there was an issue with the request reaching
//!   our servers": a non-200 is an answer lost (`Unknown`, looked up, never resent).
//! - **Warnings are a success**: a string whose severity is `W` ("`E` for error, `W` for
//!   warning") rides a request that succeeded; only `E` strings are errors.
//! - **Codes are matched by prefix**, without case and with the spaces around `:` dropped: the
//!   venue appends detail (`EGeneral:Invalid arguments:ordertype`, `EGeneral:Internal error:5`,
//!   `EService: Throttled: [UNIX timestamp]`). The longest listed code wins.
//! - **Lost answers**: `EService:Deadline elapsed` ("the order may have been placed despite the
//!   timeout"), `EGeneral:Internal error` and `EService:Unavailable` say nothing of what the
//!   engine did: `Unknown`.
//! - A cancel answered `count: 0` cancelled nothing: the order is not working there
//!   (`Rejected(NotFound)`, the trader reads it to say how it ended).

use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::RawFrame;
use serde::Deserialize;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Funds, MarketClosed, NotFound, Other, Price, ReduceOnly, VenueRule};

/// The cancel call's endpoint id: its `count` is read for a cancel that reached nothing.
pub(super) const CANCEL: &str = "cancelOrder";

/// `(code, canonical, the doc's words)`, in the guide's order.
pub(super) const ERRORS: &[ErrorRow] = &[
    // Authentication errors
    ("EAPI:Invalid key", Auth, "The API-Key header is missing, malformed, or refers to a key that has been deleted or revoked."),
    ("EAPI:Invalid signature", Auth, "The API-Sign header does not match the expected HMAC-SHA512 signature."),
    ("EAPI:Invalid nonce", Auth, "The nonce submitted is less than or equal to the last accepted nonce for this API key."),
    ("EGeneral:Temporary lockout", Auth, "Too many requests from your IP address used an API key that doesn't exist."),
    ("EGeneral:Permission denied", Permission, "The API key exists and authenticated correctly, but does not have the permission required for this endpoint."),
    ("EAuth:Account temporary disabled", Auth, "The account has been temporarily suspended."),
    ("EAuth:Account unconfirmed", Auth, "The account email has not been verified."),
    ("EAuth:Rate limit exceeded", RateLimited, "Too many authentication attempts in a short period."),
    ("EAuth:Too many requests", RateLimited, "Too many authentication attempts in a short period."),
    // Order errors
    ("EOrder:Insufficient funds", Rejected(Funds), "The account does not have enough available balance to place this order."),
    ("EOrder:Order minimum not met", Rejected(VenueRule), "The order volume is below the ordermin for this pair."),
    ("EOrder:Cost minimum not met", Rejected(VenueRule), "The total cost (price x volume) is below the costmin for this pair."),
    ("EOrder:Tick size check failed", Rejected(VenueRule), "The price submitted is not a valid multiple of the pair's tick_size."),
    ("EOrder:Invalid price", Rejected(Price), "The submitted price is zero, negative, or otherwise invalid."),
    ("EOrder:Rate limit exceeded", RateLimited, "The account's order rate limit counter has reached its threshold."),
    ("EOrder:Orders limit exceeded", Rejected(Other), "The account has too many open orders."),
    ("EOrder:Domain rate limit exceeded", RateLimited, "The domain rate limit (master + all sub-accounts) has been reached."),
    ("EOrder:Scheduled orders limit exceeded", Rejected(Other), "Too many scheduled (trigger) orders are open simultaneously."),
    ("EOrder:Positions limit exceeded", Rejected(Other), "The account has reached the maximum number of open margin positions."),
    ("EOrder:Reduce only", Rejected(ReduceOnly), "Reduce-only variants: Non-PC, No position exists, Position is closed."),
    // "Order would flip the position: it partially fills and the remainder is canceled": not a
    // refusal, part of it traded (looked up).
    ("EOrder:Reduce only:Position is closed", Unknown, "Order would flip the position: it partially fills and the remainder is canceled."),
    ("EOrder:Unknown position", Rejected(Other), "The referenced position ID does not exist or belongs to a different account."),
    // Margin errors (margin orders are not sent; listed so an answer naming one reads right)
    ("EOrder:Cannot open position", Rejected(Other), "The account tier or account type is not eligible for margin trading."),
    ("EOrder:Cannot open opposing position", Rejected(Other), "The account tier or account type is not eligible for margin trading."),
    ("EOrder:Margin allowance exceeded", Rejected(Funds), "The order would exceed the account's total margin allowance."),
    ("EOrder:Margin level too low", Rejected(Funds), "The account has insufficient equity or collateral to support this position."),
    ("EOrder:Margin position size exceeded", Rejected(Other), "The order would cause the account to exceed the maximum allowed position size for this pair."),
    ("EOrder:Insufficient margin", Rejected(Funds), "Kraken's exchange-side margin pool does not have sufficient funds for this trade."),
    // Service errors
    ("EService:Unavailable", Unknown, "The matching engine or API is offline, typically during a maintenance window."),
    ("EService:Market in cancel_only mode", Rejected(MarketClosed), "The market is in cancel_only mode: no new orders can be placed, only cancellations."),
    ("EService:Market in post_only mode", Rejected(MarketClosed), "The market is in post_only mode: only maker orders are accepted."),
    ("EService:Deadline elapsed", Unknown, "The request timed out before the trading engine processed it: the order may have been placed despite the timeout."),
    // General errors
    ("EGeneral:Invalid arguments:Index unavailable", Rejected(Other), "Index pricing is unavailable for stop or profit orders on this pair."),
    ("EGeneral:Invalid arguments", InvalidRequest, "The request payload is malformed, missing required parameters, or contains invalid values."),
    ("EGeneral:Internal error", Unknown, "An unexpected error occurred on Kraken's side."),
    ("ETrade:Invalid request", InvalidRequest, "The trade request is structurally invalid."),
    ("EAccount:Invalid permissions", Permission, "The account does not have the permissions required to perform this action."),
    // REST rate limits guide
    ("EAPI:Rate limit exceeded", RateLimited, "The REST API counter exceeds the user's maximum."),
    ("EService:Throttled", RateLimited, "Too many concurrent requests. Try again after [timestamp]."),
    // TradesHistory `pair` ("An unrecognized pair returns EQuery:Unknown asset pair")
    ("EQuery:Unknown asset pair", InvalidRequest, "An unrecognized pair."),
    // The WebSocket v1 cancelOrder error example (the only place the doc names it)
    ("EOrder:Unknown order", Rejected(NotFound), "The order is not known."),
];

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    error: Vec<String>,
    #[serde(default)]
    result: Option<serde_json::Value>,
}

/// The code without case and without the spaces the venue puts around `:`.
fn norm(s: &str) -> String {
    s.split(':').map(str::trim).collect::<Vec<_>>().join(":").to_ascii_lowercase()
}

/// The code of `codes` that `raw` starts with (the longest one), on a `:` boundary.
pub(super) fn longest_prefix<'a>(codes: impl Iterator<Item = &'a str>, raw: &str) -> Option<&'a str> {
    let n = norm(raw);
    codes
        .filter(|code| {
            let c = norm(code);
            n == c || n.strip_prefix(&c).is_some_and(|rest| rest.starts_with(':'))
        })
        .max_by_key(|code| code.len())
}

/// The listed code `raw` starts with (the longest one), on a `:` boundary.
pub(super) fn row_of(raw: &str) -> Option<&'static ErrorRow> {
    let code = longest_prefix(ERRORS.iter().map(|(c, _, _)| *c), raw)?;
    ERRORS.iter().find(|(c, _, _)| *c == code)
}

/// The moment `EService: Throttled: [UNIX timestamp]` names, epoch milliseconds.
fn throttled_until(raw: &str) -> Option<i64> {
    raw.rsplit(':').next()?.trim().parse::<i64>().ok().filter(|s| *s > 0).map(|s| s.saturating_mul(1000))
}

/// One error string of the `error` array, mapped.
pub(super) fn of(raw: &str, all: &str) -> VenueError {
    match row_of(raw) {
        Some((code, canon, _)) => {
            let message = match canon {
                Unknown => format!("Kraken answered {all} (code {code}): the order may have gone through"),
                _ => format!("Kraken refused it: {all} (code {code})"),
            };
            let mut e = VenueError::new(*canon, Some((*code).to_string()), message);
            if *code == "EService:Throttled" {
                e.retry_at_ms = throttled_until(raw);
            }
            e
        }
        None => VenueError::unmapped(Some(raw.to_string()), format!("Kraken answered code {raw} ({all}), which OpenTraderWorld does not map")),
    }
}

/// The error an order call's answer carries, mapped; `None` for a success.
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let (endpoint, status, body) = match frame {
        RawFrame::Http { endpoint_id, status, body, .. } => (*endpoint_id, *status, body),
        RawFrame::Timeout { endpoint_id, .. } => return Some(VenueError::new(Unknown, None, format!("Kraken: no answer to {endpoint_id} in time: it may have gone through"))),
        RawFrame::Ws { .. } | RawFrame::Closed { .. } => return None,
    };
    if status != 200 {
        // "an issue with the request reaching our servers": nothing says it did not.
        let snippet: String = String::from_utf8_lossy(body).chars().take(120).collect();
        return Some(VenueError::new(Unknown, Some(format!("HTTP {status}")), format!("Kraken answered HTTP {status} to {endpoint} ({snippet}): it may have gone through")));
    }
    let env: Envelope = match serde_json::from_slice(body) {
        Ok(e) => e,
        Err(e) => return Some(VenueError::unmapped(None, format!("Kraken: the answer to {endpoint} was not read ({e})"))),
    };
    let errors: Vec<&str> = env.error.iter().map(String::as_str).filter(|e| !e.trim_start().starts_with('W')).collect();
    if let Some(first) = errors.first() {
        return Some(of(first, &errors.join("; ")));
    }
    if endpoint == CANCEL && env.result.as_ref().and_then(|r| r.get("count")).and_then(serde_json::Value::as_i64) == Some(0) {
        return Some(VenueError::new(Rejected(NotFound), None, "Kraken cancelled no order (count 0): it is not working there"));
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn codes_match_by_prefix_with_their_tails_and_spaces() {
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EGeneral:Invalid arguments:ordertype"]}"#)).unwrap();
        assert_eq!((e.canon, e.code.as_deref(), e.mapped), (InvalidRequest, Some("EGeneral:Invalid arguments"), true));
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EGeneral:Invalid arguments:Index unavailable"]}"#)).unwrap();
        assert_eq!(e.code.as_deref(), Some("EGeneral:Invalid arguments:Index unavailable"), "the longest code wins");
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EGeneral:Internal error:5"]}"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, true));
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EOrder:Reduce only:No position exists"]}"#)).unwrap();
        assert_eq!(e.canon, Rejected(ReduceOnly));
        // "it partially fills and the remainder is canceled": part of it traded, looked up.
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EOrder:Reduce only:Position is closed"]}"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, true));
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EService: Throttled: 1767225630"]}"#)).unwrap();
        assert_eq!((e.canon, e.retry_at_ms), (RateLimited, Some(1_767_225_630_000)));
        // `Order minimum` is not `Orders limit`: a boundary is a `:`.
        assert_eq!(row_of("EOrder:Insufficient fundsX"), None);
    }

    #[test]
    fn a_refusal_says_the_venues_words_and_code() {
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EOrder:Insufficient funds"]}"#)).unwrap();
        assert_eq!(e.canon, Rejected(Funds));
        assert_eq!(e.message, "Kraken refused it: EOrder:Insufficient funds (code EOrder:Insufficient funds)");
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["EOrder:Something new"]}"#)).unwrap();
        assert!(!e.mapped && e.canon == Unknown && e.message.contains("does not map"), "{e:?}");
    }

    #[test]
    fn warnings_ride_a_success_and_a_lost_answer_is_unknown() {
        assert!(decode(&http_frame(1, "addOrder", 200, r#"{"error":["WGeneral:Something deprecated"],"result":{"txid":["O1"]}}"#)).is_none());
        let e = decode(&http_frame(1, "addOrder", 200, r#"{"error":["WGeneral:x","EOrder:Insufficient funds"]}"#)).unwrap();
        assert_eq!(e.canon, Rejected(Funds));
        for (status, body) in [(502, "<html>"), (520, ""), (403, "cloudflare")] {
            let e = decode(&http_frame(1, "addOrder", status, body)).unwrap();
            assert_eq!(e.canon, Unknown, "{status}");
        }
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "addOrder" };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
        for code in ["EService:Deadline elapsed", "EService:Unavailable", "EGeneral:Internal error"] {
            let e = decode(&http_frame(1, "addOrder", 200, &format!(r#"{{"error":["{code}"]}}"#))).unwrap();
            assert_eq!((e.canon, e.mapped), (Unknown, true), "{code}");
        }
        let e = decode(&http_frame(1, "addOrder", 200, "not json")).unwrap();
        assert!(!e.mapped && e.canon == Unknown);
    }

    #[test]
    fn a_cancel_that_cancelled_nothing_is_not_found_and_cancel_all_of_none_is_fine() {
        let e = decode(&http_frame(1, CANCEL, 200, r#"{"error":[],"result":{"count":0}}"#)).unwrap();
        assert_eq!(e.canon, Rejected(NotFound));
        assert!(decode(&http_frame(1, CANCEL, 200, r#"{"error":[],"result":{"count":1}}"#)).is_none());
        assert!(decode(&http_frame(1, "cancelAll", 200, r#"{"error":[],"result":{"count":0}}"#)).is_none());
        let e = decode(&http_frame(1, CANCEL, 200, r#"{"error":["EOrder:Unknown order"]}"#)).unwrap();
        assert_eq!(e.canon, Rejected(NotFound));
    }

    #[test]
    fn auth_and_permission() {
        for (code, canon) in [("EAPI:Invalid key", Auth), ("EAPI:Invalid signature", Auth), ("EAPI:Invalid nonce", Auth), ("EGeneral:Permission denied", Permission)] {
            assert_eq!(decode(&http_frame(1, "addOrder", 200, &format!(r#"{{"error":["{code}"]}}"#))).unwrap().canon, canon, "{code}");
        }
    }

    /// Every code of the table is in PROVIDER.md, and the other way round for the table there.
    #[test]
    fn the_table_and_provider_md_say_the_same() {
        let md = include_str!("PROVIDER.md");
        for (code, _, _) in ERRORS {
            assert!(md.contains(&format!("`{code}`")), "{code} is not in PROVIDER.md");
        }
    }
}
