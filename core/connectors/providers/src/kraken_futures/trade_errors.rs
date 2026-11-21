//! Kraken Futures' order error table (onboarding spec 8), from the derivatives OpenAPI
//! (https://docs.kraken.com/openapi/futures-rest.yaml, snapshot read 2026-11-12, last-modified
//! Mon, 09 Nov 2026 11:24:12 GMT): every word of its `Error` enum, every refusal word of
//! `SendOrderJson.status` (enum and description), `EditOrderStatus.status` and
//! `CancelOrderStatusJson.status`, and the coded 403 (`95`) and the 409 ("The venue is
//! cancel-only"), each mapped to one canonical kind. A word not listed here is `Unknown` and
//! alerts. Mirrored in PROVIDER.md ("Trading", "Errors"); a test keeps the two in step.
//!
//! Kraken Futures refuses in two places. The envelope (`"result":"error"`, the word in `error`
//! or `errors`) says the request itself was refused. A request it read answers
//! `"result":"success"`, and "this merely means that the request has been received and assessed
//! successfully ... Details on the operation's status are returned in a status key" (REST
//! guide, Calls and Returns): the order's refusal is that status word (`sendStatus`,
//! `editStatus`, `cancelStatus`), read here for the order calls.

use otw_connector_model::ErrorKind;
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;
use serde_json::Value;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, PostOnly, Price, ReduceOnly, VenueRule};

/// The placing, changing and cancelling calls, whose answer carries the order's status word.
pub(super) const SEND: &str = "sendorder";
pub(super) const EDIT: &str = "editorder";
pub(super) const CANCEL: &str = "cancelorder";
pub(super) const CANCEL_ALL: &str = "cancelallorders";

/// `(word, canonical, the doc's words)`: the `Error` enum first, then the status words. A word
/// the doc lists without words of its own (the enum only) carries none.
pub(super) const ERRORS: &[ErrorRow] = &[
    // `Error` (every `/derivatives` answer's `error`).
    ("accountInactive", Permission, "The Futures account the request refers to is inactive"),
    ("accountNotFound", InvalidRequest, "The account the transfer request refers to could not be found"),
    ("apiLimitExceeded", RateLimited, "The API limit for the calling IP address has been exceeded"),
    ("authenticationError", Auth, "The request could not be authenticated"),
    ("cancelOnly", Rejected(MarketClosed), "The venue is in cancel-only mode and accepts only cancellations until it is lifted"),
    ("cannotTransferBonusCurrency", InvalidRequest, "Deprecated and no longer returned, since bonus currency has been removed"),
    ("cashAccountTypeAccessForbidden", Permission, "The account's platform does not give access to the requested wallet"),
    ("cashAccountTypeNotFound", InvalidRequest, "The requested wallet does not exist"),
    ("insufficientFunds", Rejected(Funds), "The amount requested for transfer is below the amount of funds available"),
    ("invalidAccount", InvalidRequest, "The Futures account the transfer request refers to is invalid"),
    ("invalidAmount", InvalidRequest, "The amount the transfer request refers to is invalid"),
    ("invalidArgument", InvalidRequest, "One or more arguments provided are invalid"),
    ("invalidCurrency", InvalidRequest, "The currency cannot be transferred between the requested wallets"),
    ("invalidUnit", InvalidRequest, "The unit the transfer request refers to is invalid"),
    ("Json Parse Error", InvalidRequest, "The request failed to pass valid JSON as an argument"),
    ("marketUnavailable", Unknown, "The market is currently unavailable"),
    ("nonceBelowThreshold", Auth, "The provided nonce is below the threshold"),
    ("nonceDuplicate", Auth, "The provided nonce is a duplicate as it has been used in a previous request"),
    ("notFound", Rejected(NotFound), "The requested information could not be found"),
    ("requiredArgumentMissing", InvalidRequest, "One or more required arguments are missing"),
    ("Server Error", Unknown, "There was an error processing the request"),
    ("transferFailed", Rejected(Other), "The transfer was refused for a reason the venue does not disclose"),
    ("Unavailable", Unknown, "The endpoint being called is unavailable"),
    ("unknownError", Unknown, "An unknown error has occurred"),
    // HTTP 403 `ContractAccessForbidden`, error `code` 95.
    ("95", Permission, "Account is not permitted to perform the requested trading operation"),
    // `sendStatus.status` (Send order), refusals only.
    ("marketSuspended", Rejected(VenueRule), "the order was not placed because the market is suspended"),
    ("marketInactive", Rejected(VenueRule), "the order was not placed because the market is inactive"),
    ("invalidPrice", Rejected(VenueRule), "the order was not placed because limitPrice and/or stopPrice are invalid"),
    ("invalidSize", Rejected(VenueRule), "the order was not placed because size is invalid"),
    ("tooManySmallOrders", Rejected(Other), "the order was not placed because the number of small open orders would exceed the permissible limit"),
    ("insufficientAvailableFunds", Rejected(Funds), "the order was not placed because available funds are insufficient"),
    ("wouldCauseLiquidation", Rejected(Funds), "a new order would fill at a worse price than the mark price, causing the portfolio value to fall below maintenance margin and triggering a liquidation"),
    ("clientOrderIdAlreadyExist", Unknown, "the specified client id already exist"),
    ("clientOrderIdTooBig", InvalidRequest, ""),
    ("clientOrderIdTooLong", InvalidRequest, "the client id is longer than the permissible limit"),
    ("clientOrderIdInvalid", InvalidRequest, ""),
    ("maxPositionViolation", Rejected(Other), "Order would cause you to exceed your maximum position in this contract."),
    ("outsidePriceCollar", Rejected(Price), "the order would have executed outside of the price collar for its order type"),
    ("wouldIncreasePriceDislocation", Rejected(Price), ""),
    ("orderForEditNotAStop", InvalidRequest, "The supplied stopPrice cannot be applied because order is not a stop order"),
    ("orderForEditNotFound", Rejected(NotFound), "The requested order for edit has not been found"),
    ("postWouldExecute", Rejected(PostOnly), "the post-only order would be filled upon placement, thus is cancelled"),
    ("iocWouldNotExecute", Rejected(Other), "the immediate-or-cancel order would not execute"),
    ("selfFill", Rejected(Other), "the order was not placed because it would be filled against an existing order belonging to the same account"),
    ("wouldNotReducePosition", Rejected(ReduceOnly), "the reduce only order would not reduce position"),
    ("marketIsPostOnly", Rejected(VenueRule), ""),
    ("tooManyOrders", Rejected(Other), ""),
    ("makerProtectionOrderLimitExceeded", Rejected(Other), "the order was not placed because accepting it would exceed the maker protection live-hold limit"),
    ("fixedLeverageTooHigh", Rejected(Other), ""),
    ("cannotEditTriggerPriceOfTrailingStop", InvalidRequest, ""),
    ("cannotEditLimitPriceOfTrailingStop", InvalidRequest, ""),
    ("wouldProcessAfterSpecifiedTime", Rejected(Other), "order would be processed after the time specified in processBefore parameter"),
    ("invalidDomainConfiguration", Permission, "the order was not placed because the account's domain is not configured to permit it"),
    ("editAlreadyHeld", Rejected(Other), "the edit was refused because an earlier edit of the order is still held for the market's maker protection window"),
    ("marketIsCancelOnly", Rejected(MarketClosed), "the request was refused because the venue is in cancel-only mode: only cancellations are accepted until it is lifted"),
    ("invalidOrderType", InvalidRequest, "the order was not placed because orderType is invalid"),
    ("invalidSide", InvalidRequest, "the order was not placed because side is invalid"),
    // `cancelStatus.status` (Cancel order): `filled` is a refusal there only.
    ("filled", Rejected(Final), "The order was found completely filled and could not be cancelled"),
];

/// The status words that say the call went through, per order call.
fn accepted(endpoint: &str, word: &str) -> bool {
    match endpoint {
        // `cancelled` and `filled` on a placement: an immediate order that ran its course.
        SEND => matches!(word, "placed" | "partiallyFilled" | "filled" | "cancelled" | "edited"),
        // "`filled` - The edit caused the order to fill completely".
        EDIT => matches!(word, "edited" | "filled"),
        CANCEL => word == "cancelled",
        // "cancelled - successful cancellation", "noOrdersToCancel - no open orders for
        // cancellation"; trigger orders it cancelled are in `orderEvents` either way.
        CANCEL_ALL => matches!(word, "cancelled" | "noOrdersToCancel"),
        _ => true,
    }
}

/// Edit order refusals that may have taken the order off the book: `postWouldExecute` ("thus
/// is cancelled"), `iocWouldNotExecute` ("the part that did not fill was removed from the
/// book"), and `selfFill`, `insufficientAvailableFunds`, `invalidSize`, whose words ("The order
/// was not placed") do not say whether the order still works. The order's state is unknown:
/// it is read again before anything else is done to it.
const EDIT_MAY_END: &[&str] = &["postWouldExecute", "iocWouldNotExecute", "selfFill", "insufficientAvailableFunds", "invalidSize"];

fn row(word: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|(c, _, _)| *c == word)
}

/// The error a word means, in Kraken's words (`said`: the answer's own text, where it gives
/// one beside the word).
pub(super) fn of(word: &str, said: Option<&str>) -> VenueError {
    let words = |doc: &str| {
        let w = said.filter(|s| !s.is_empty() && *s != word).unwrap_or(doc);
        if w.is_empty() {
            String::new()
        } else {
            format!(": {w}")
        }
    };
    match row(word) {
        Some((_, Unknown, doc)) => VenueError::new(Unknown, Some(word.to_string()), format!("Kraken Futures did not say whether it went through{} (code {word})", words(doc))),
        Some((_, canon, doc)) => VenueError::new(*canon, Some(word.to_string()), format!("Kraken Futures refused it{} (code {word})", words(doc))),
        None => VenueError::unmapped(Some(word.to_string()), format!("Kraken Futures answered code {word} ({}), which OpenTraderWorld does not map", said.unwrap_or(word))),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    errors: Vec<Value>,
    /// The history API's refusal (`{"status":"unauthorized","reason":".."}`).
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    send_status: Option<Status>,
    #[serde(default)]
    edit_status: Option<Status>,
    #[serde(default)]
    cancel_status: Option<Status>,
}

#[derive(Deserialize, Default)]
struct Status {
    #[serde(default)]
    status: String,
}

/// The first word an `errors` array names: a bare word, or `{code, message}` (the 403).
fn first_error(items: &[Value]) -> Option<(String, Option<String>)> {
    items.iter().find_map(|i| match i {
        Value::String(w) => Some((w.clone(), None)),
        Value::Object(m) => {
            let code = m.get("code").map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string()));
            let msg = m.get("message").and_then(Value::as_str).map(str::to_string);
            code.map(|c| (c, msg))
        }
        _ => None,
    })
}

/// The error an order call's answer carries, mapped; `None` for a success.
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        let why = http::timeout(frame, "Kraken Futures").map(|e| e.message).unwrap_or_else(|| "Kraken Futures: no answer".into());
        return Some(VenueError::new(Unknown, None, format!("{why}: it may have gone through")));
    };
    let status = *status;
    // A 5xx: the venue may have acted, whatever the body says.
    if status >= 500 {
        let snippet: String = String::from_utf8_lossy(body).chars().take(120).collect();
        return Some(VenueError::new(Unknown, Some(status.to_string()), format!("Kraken Futures did not say whether it went through: HTTP {status} {snippet}")));
    }
    let env = serde_json::from_slice::<Envelope>(body).ok();
    let mut e = match env {
        Some(env) if env.result.as_deref() == Some("error") || env.error.is_some() || !env.errors.is_empty() => {
            let (word, said) = env.error.clone().map(|w| (w, None)).or_else(|| first_error(&env.errors)).unwrap_or_else(|| ("error".into(), None));
            of(&word, said.as_deref())
        }
        Some(env) if !(200..300).contains(&status) && env.status.is_some() => {
            // The history API: `{"status":"unauthorized","reason":".."}`, a 429.
            let word = env.status.as_ref().and_then(Value::as_str).unwrap_or_default().to_string();
            let reason = env.reason.unwrap_or_default();
            let canon = match (status, word.as_str()) {
                (_, "unauthorized") | (401, _) => Auth,
                (429, _) => RateLimited,
                (403, _) => Permission,
                _ => InvalidRequest,
            };
            VenueError::new(canon, Some(word.clone()), format!("Kraken Futures refused it: {reason} (code {word})"))
        }
        Some(env) if (200..300).contains(&status) => {
            let st = match *endpoint_id {
                SEND => env.send_status,
                EDIT => env.edit_status,
                CANCEL | CANCEL_ALL => env.cancel_status,
                _ => return None,
            };
            let word = st.map(|s| s.status).unwrap_or_default();
            if word.is_empty() {
                // A success with no status word: nobody can tell what it did.
                return Some(VenueError::unmapped(None, format!("Kraken Futures answered {endpoint_id} with no status word, which OpenTraderWorld does not map")));
            }
            if accepted(endpoint_id, &word) {
                return None;
            }
            if *endpoint_id == EDIT && EDIT_MAY_END.contains(&word.as_str()) {
                let doc = row(&word).map(|r| r.2).unwrap_or_default();
                return Some(VenueError::new(
                    Unknown,
                    Some(word.clone()),
                    format!("Kraken Futures refused the change: {doc} (code {word}); this refusal can take the order off the book"),
                ));
            }
            of(&word, None)
        }
        _ => {
            // No envelope: the HTTP status alone.
            let c = http::classify(frame, "Kraken Futures")?;
            let canon = match (status, c.kind) {
                (409, _) => Rejected(MarketClosed),
                (_, ErrorKind::RateLimited) => RateLimited,
                (_, ErrorKind::Auth) => Auth,
                (_, ErrorKind::Permission) => Permission,
                (_, ErrorKind::InvalidRequest | ErrorKind::NotFound) => InvalidRequest,
                _ => Unknown,
            };
            let mut v = VenueError::new(canon, c.provider_code.clone(), c.message);
            v.retry_after_ms = c.retry_after_ms;
            v
        }
    };
    if e.canon == RateLimited && e.retry_after_ms.is_none() {
        // The doc states no wait for `apiLimitExceeded`; a `Retry-After`, or the history
        // API's `rate-limit-reset` (seconds), is read where the answer carries one.
        e.retry_after_ms = http::retry_after_ms(frame).or_else(|| frame.header("rate-limit-reset").and_then(|v| v.trim().parse::<u64>().ok()).map(|s| s.saturating_mul(1000)));
    }
    Some(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    /// PROVIDER.md's error table lists the same words with the same kinds.
    #[test]
    fn the_table_matches_provider_md() {
        let doc = include_str!("PROVIDER.md");
        let start = doc.find("### Errors").unwrap();
        let table = &doc[start..start + doc[start..].find("### Rate limits for orders").unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in table.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let canon = cells[2];
            for c in cells[1].split('`').skip(1).step_by(2) {
                seen.push((c.to_string(), canon.to_string()));
            }
        }
        let word = |c: &Canon| match c {
            Rejected(VenueRule) => "Rejected(venue_rule)",
            Rejected(Funds) => "Rejected(funds)",
            Rejected(Price) => "Rejected(price)",
            Rejected(MarketClosed) => "Rejected(market closed)",
            Rejected(NotFound) => "Rejected(not found)",
            Rejected(Final) => "Rejected(final)",
            Rejected(ReduceOnly) => "Rejected(reduce only)",
            Rejected(PostOnly) => "Rejected(post only)",
            Rejected(Other) => "Rejected(other)",
            Unknown => "Unknown",
            RateLimited => "RateLimited",
            Auth => "Auth",
            Permission => "Permission",
            InvalidRequest => "InvalidRequest",
        };
        let mut want: Vec<(String, String)> = ERRORS.iter().map(|(c, k, _)| (c.to_string(), word(k).to_string())).collect();
        seen.sort();
        want.sort();
        assert_eq!(seen, want);
    }

    #[test]
    fn no_word_is_listed_twice() {
        let mut codes: Vec<&str> = ERRORS.iter().map(|r| r.0).collect();
        codes.sort_unstable();
        let n = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), n);
    }

    /// Every refusal word of the doc's three status enums is in the table, and every success
    /// word is not.
    #[test]
    fn every_documented_status_word_is_mapped() {
        let send = [
            "marketSuspended", "marketInactive", "invalidPrice", "invalidSize", "tooManySmallOrders", "insufficientAvailableFunds", "wouldCauseLiquidation",
            "clientOrderIdAlreadyExist", "clientOrderIdTooBig", "maxPositionViolation", "outsidePriceCollar", "wouldIncreasePriceDislocation", "notFound",
            "orderForEditNotAStop", "orderForEditNotFound", "postWouldExecute", "iocWouldNotExecute", "selfFill", "wouldNotReducePosition", "marketIsPostOnly",
            "tooManyOrders", "makerProtectionOrderLimitExceeded", "fixedLeverageTooHigh", "clientOrderIdInvalid", "cannotEditTriggerPriceOfTrailingStop",
            "cannotEditLimitPriceOfTrailingStop", "wouldProcessAfterSpecifiedTime", "invalidDomainConfiguration", "editAlreadyHeld", "marketIsCancelOnly",
        ];
        for w in send {
            assert!(row(w).is_some(), "{w}");
            let body = format!(r#"{{"result":"success","serverTime":"2026-11-12T00:00:00Z","sendStatus":{{"status":"{w}"}}}}"#);
            let e = decode(&http_frame(1, SEND, 200, &body)).unwrap();
            assert!(e.mapped, "{w}");
        }
        for w in ["placed", "partiallyFilled", "filled", "cancelled", "edited"] {
            let body = format!(r#"{{"result":"success","sendStatus":{{"order_id":"x","status":"{w}"}}}}"#);
            assert!(decode(&http_frame(1, SEND, 200, &body)).is_none(), "{w}");
        }
        let edit = ["invalidSize", "invalidPrice", "insufficientAvailableFunds", "makerProtectionOrderLimitExceeded", "selfFill", "tooManySmallOrders", "outsidePriceCollar", "postWouldExecute", "iocWouldNotExecute", "wouldNotReducePosition", "orderForEditNotFound", "orderForEditNotAStop", "editAlreadyHeld", "marketIsCancelOnly"];
        for w in edit {
            let body = format!(r#"{{"result":"success","editStatus":{{"status":"{w}","orderEvents":[]}}}}"#);
            assert!(decode(&http_frame(1, EDIT, 200, &body)).unwrap().mapped, "{w}");
        }
        for w in ["edited", "filled"] {
            let body = format!(r#"{{"result":"success","editStatus":{{"status":"{w}","orderEvents":[]}}}}"#);
            assert!(decode(&http_frame(1, EDIT, 200, &body)).is_none(), "{w}");
        }
    }

    /// An edit refusal whose doc words say (or leave open) that the order left the book is
    /// unknown, so the order is read again; the same word on a placement is a refusal.
    #[test]
    fn an_edit_refusal_that_may_end_the_order_is_unknown() {
        for w in EDIT_MAY_END {
            let body = format!(r#"{{"result":"success","editStatus":{{"status":"{w}","orderEvents":[]}}}}"#);
            let e = decode(&http_frame(1, EDIT, 200, &body)).unwrap();
            assert!(e.canon == Unknown && e.mapped && e.code.as_deref() == Some(*w) && e.message.contains("off the book"), "{w}: {e:?}");
            let body = format!(r#"{{"result":"success","sendStatus":{{"status":"{w}"}}}}"#);
            assert!(matches!(decode(&http_frame(1, SEND, 200, &body)).unwrap().canon, Rejected(_) | InvalidRequest), "{w}");
        }
        // Every other edit refusal leaves the order as it was: refused.
        let body = r#"{"result":"success","editStatus":{"status":"invalidPrice","orderEvents":[]}}"#;
        assert_eq!(decode(&http_frame(1, EDIT, 200, body)).unwrap().canon, Rejected(VenueRule));
    }

    /// The doc's answers, verbatim where it gives them.
    #[test]
    fn the_status_word_decides_inside_a_success() {
        // Send order, `rejected` example (futures-rest.yaml, RejectedOrderExample).
        let rejected = include_str!("fixtures/trade/doc_sendorder_rejected.json");
        let e = decode(&http_frame(1, SEND, 200, rejected)).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Rejected(Other), Some("iocWouldNotExecute")));
        assert!(e.message.starts_with("Kraken Futures refused it: the immediate-or-cancel order would not execute (code iocWouldNotExecute)"), "{}", e.message);
        // The REST guide's "desired operation was not performed" answer
        // (https://docs.kraken.com/exchange/guides/futures/rest, Returns).
        let guide = r#"{"result":"success","serverTime":"2016-02-25T09:45:53.818Z","sendStatus":{"receivedTime":"2016-02-25T09:45:53.601Z","status":"insufficientAvailableFunds"}}"#;
        assert_eq!(decode(&http_frame(1, SEND, 200, guide)).unwrap().canon, Rejected(Funds));
        assert!(decode(&http_frame(1, SEND, 200, include_str!("fixtures/trade/doc_sendorder_placed.json"))).is_none());
        assert!(decode(&http_frame(1, CANCEL, 200, include_str!("fixtures/trade/doc_cancelorder.json"))).is_none());
        assert!(decode(&http_frame(1, EDIT, 200, include_str!("fixtures/trade/doc_editorder.json"))).is_none());
        assert!(decode(&http_frame(1, CANCEL_ALL, 200, include_str!("fixtures/trade/doc_cancelallorders.json"))).is_none());
        // Synthetic: the cancel's other two documented words.
        let c = |w: &str| decode(&http_frame(1, CANCEL, 200, &format!(r#"{{"result":"success","cancelStatus":{{"status":"{w}","order_id":"x","receivedTime":"t","orderEvents":[]}}}}"#)));
        assert_eq!(c("filled").unwrap().canon, Rejected(Final));
        assert_eq!(c("notFound").unwrap().canon, Rejected(NotFound));
        // Changelog 2026-11-12: only trigger orders cancelled still says `noOrdersToCancel`.
        assert!(decode(&http_frame(1, CANCEL_ALL, 200, r#"{"result":"success","cancelStatus":{"status":"noOrdersToCancel","cancelOnly":"all","cancelledOrders":[],"orderEvents":[],"receivedTime":"t"}}"#)).is_none());
        // A read call's success carries no status word to check.
        assert!(decode(&http_frame(1, "openorders", 200, include_str!("fixtures/doc_openorders.json"))).is_none());
    }

    #[test]
    fn a_lost_answer_is_unknown_and_an_unlisted_word_alerts() {
        for w in ["Server Error", "unknownError", "Unavailable", "marketUnavailable"] {
            let body = format!(r#"{{"result":"error","serverTime":"2016-02-25T09:45:53.818Z","error":"{w}"}}"#);
            let e = decode(&http_frame(1, SEND, 200, &body)).unwrap();
            assert!(e.canon == Unknown && e.mapped && e.message.contains("did not say whether"), "{w}: {e:?}");
        }
        assert_eq!(decode(&http_frame(1, SEND, 502, "<html>")).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: SEND };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
        let e = decode(&http_frame(1, SEND, 200, r#"{"result":"success","sendStatus":{"status":"somethingNew"}}"#)).unwrap();
        assert!(!e.mapped && e.canon == Unknown && e.message == "Kraken Futures answered code somethingNew (somethingNew), which OpenTraderWorld does not map", "{e:?}");
        // A duplicate client id: looked up, never resent.
        let e = decode(&http_frame(1, SEND, 200, r#"{"result":"success","sendStatus":{"status":"clientOrderIdAlreadyExist"}}"#)).unwrap();
        assert_eq!(e.canon, Unknown);
    }

    #[test]
    fn the_envelope_refusals_and_the_http_ones_are_read() {
        // Rate limits guide, Example (verbatim): no wait stated.
        let e = decode(&http_frame(1, SEND, 200, r#"{"result":"error","serverTime":"2016-02-25T09:45:53.818Z","error":"apiLimitExceeded"}"#)).unwrap();
        assert_eq!((e.canon, e.retry_after_ms), (RateLimited, None));
        let e = decode(&http_frame(1, SEND, 200, r#"{"result":"error","serverTime":"2026-11-12T10:18:41.833Z","error":"authenticationError"}"#)).unwrap();
        assert_eq!(e.canon, Auth);
        assert_eq!(decode(&http_frame(1, SEND, 200, r#"{"result":"error","error":"nonceBelowThreshold"}"#)).unwrap().canon, Auth);
        // The coded 403 (OpenAPI `ContractAccessForbidden`); synthetic message.
        let e = decode(&http_frame(1, SEND, 403, r#"{"result":"error","serverTime":"2026-11-12T00:00:00Z","errors":[{"code":95,"message":"Contract access forbidden"}]}"#)).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Permission, Some("95")));
        // 409 "The venue is cancel-only" (no body documented).
        assert_eq!(decode(&http_frame(1, SEND, 409, "")).unwrap().canon, Rejected(MarketClosed));
        // The history API (measured on the live host with no key, 2026-11-12).
        let e = decode(&http_frame(1, "executions", 401, r#"{"status":"unauthorized","reason":"You are not authorized to access this endpoint."}"#)).unwrap();
        assert_eq!(e.canon, Auth);
        let frame = RawFrame::Http {
            corr_id: otw_connector_model::CorrId(1),
            endpoint_id: "executions",
            status: 429,
            headers: vec![("rate-limit-reset".into(), "12".into())],
            body: bytes::Bytes::from_static(b"Too many requests"),
        };
        let e = decode(&frame).unwrap();
        assert_eq!((e.canon, e.retry_after_ms), (RateLimited, Some(12_000)));
        // A history answer is no error.
        assert!(decode(&http_frame(1, "executions", 200, r#"{"accountUid":"a","len":0,"elements":[]}"#)).is_none());
    }
}
