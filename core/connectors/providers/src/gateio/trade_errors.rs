//! Gate.io's order error table (onboarding spec 8), from the API v4 overview's label list
//! (https://www.gate.com/docs/developers/apiv4/en/#label-list, read 2026-11-12 through the doc
//! reader, the site being behind Cloudflare): every label of its request, authentication,
//! spot and margin trading, futures and server groups that an order call (place, cancel,
//! cancel all, amend, look up) can answer, plus the wallet group's client id labels and the
//! three Flash Convert labels the survey names, each mapped to one canonical kind. Loan,
//! repayment, risk limit and margin transfer labels are left out: no order call answers them.
//! The position and margin labels a futures account answers (`ORDER_PENDING`,
//! `POSITION_HOLDING`, `POSITION_CROSS_MARGIN`) and `NO_MERGEABLE_ORDERS` are listed as clean
//! refusals all the same, so a venue that answers one to an order call does not alert. A label
//! not listed is `Unknown` and alerts. Mirrored in PROVIDER.md ("Trading", "Errors"); a test
//! keeps the two in step.

#![deny(clippy::float_arithmetic)]

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, PostOnly, Price, ReduceOnly, VenueRule};

/// `(label, canonical, the doc's words)`.
pub(super) const ERRORS: &[ErrorRow] = &[
    // Request parameter or format related.
    ("INVALID_PARAM_VALUE", InvalidRequest, "invalid parameter value"),
    ("INVALID_PROTOCOL", InvalidRequest, "invalid parameter value"),
    ("INVALID_ARGUMENT", InvalidRequest, "invalid argument"),
    ("INVALID_REQUEST_BODY", InvalidRequest, "invalid request body"),
    ("MISSING_REQUIRED_PARAM", InvalidRequest, "missing required parameter"),
    ("BAD_REQUEST", InvalidRequest, "invalid request"),
    ("INVALID_CONTENT_TYPE", InvalidRequest, "invalid Content-Type header"),
    ("NOT_ACCEPTABLE", InvalidRequest, "invalid Accept header"),
    ("METHOD_NOT_ALLOWED", InvalidRequest, "request method is not allowed"),
    ("NOT_FOUND", InvalidRequest, "request URL not exists"),
    // Authentication related.
    ("INVALID_CREDENTIALS", Auth, "invalid credentials provided"),
    ("INVALID_KEY", Auth, "invalid API key"),
    ("IP_FORBIDDEN", Auth, "request IP not in whitelist"),
    ("READ_ONLY", Permission, "API key is read-only"),
    ("INVALID_SIGNATURE", Auth, "invalid signature"),
    ("MISSING_REQUIRED_HEADER", Auth, "missing required authentication header"),
    ("REQUEST_EXPIRED", Auth, "request Timestamp is far from the server time"),
    ("ACCOUNT_LOCKED", Auth, "account is locked"),
    ("FORBIDDEN", Permission, "account has no permission to request operation"),
    // Wallet group: the client order id labels.
    ("DUPLICATE_REQUEST", Unknown, "duplicate request"),
    ("ORDER_EXISTS", Unknown, "order already exists, do not resubmit"),
    ("INVALID_CLIENT_ORDER_ID", InvalidRequest, "the client_order_id is invalid"),
    // Spot and margin trading related.
    ("INVALID_PRECISION", Rejected(VenueRule), "invalid precision"),
    ("INVALID_CURRENCY", InvalidRequest, "invalid currency"),
    ("INVALID_CURRENCY_PAIR", InvalidRequest, "invalid currency pair"),
    ("POC_FILL_IMMEDIATELY", Rejected(PostOnly), "order would match and take immediately so it's cancelled"),
    ("ORDER_NOT_FOUND", Rejected(NotFound), "order not found"),
    ("ORDER_CLOSED", Rejected(Final), "order already closed"),
    ("ORDER_CANCELLED", Rejected(Final), "order already cancelled"),
    ("QUANTITY_NOT_ENOUGH", Rejected(VenueRule), "amount is not enough"),
    ("BALANCE_NOT_ENOUGH", Rejected(Funds), "balance is not enough"),
    ("MARGIN_NOT_SUPPORTED", InvalidRequest, "currency pair doesn't provide margin trading"),
    ("MARGIN_BALANCE_NOT_ENOUGH", Rejected(Funds), "margin balance is not enough"),
    ("AMOUNT_TOO_LITTLE", Rejected(VenueRule), "amount does not reach minimum required"),
    ("AMOUNT_TOO_MUCH", Rejected(VenueRule), "amount exceeds maximum allowed"),
    // A second create of the same order: the first may stand.
    ("REPEATED_CREATION", Unknown, "repeated creation"),
    ("NO_CHANGE", InvalidRequest, "no change is made"),
    ("TOO_MANY_CURRENCY_PAIRS", InvalidRequest, "too many currency pairs in batch orders creation"),
    // Spot: too many orders of one pair in a batch; futures: too many open orders.
    ("TOO_MANY_ORDERS", Rejected(Other), "too many orders"),
    ("MIXED_ACCOUNT_TYPE", InvalidRequest, "more than one account type in a batch"),
    ("AUTO_BORROW_TOO_MUCH", Rejected(Funds), "auto borrow exceeds maximum allowed"),
    ("TRADE_RESTRICTED", Rejected(Other), "trading is restricted due to high debt ratio"),
    ("FOK_NOT_FILL", Rejected(Other), "FOK order cannot be filled completely"),
    ("INITIAL_MARGIN_TOO_LOW", Rejected(Funds), "total initial margin rate is too low"),
    ("ORDER_BOOK_NOT_FOUND", Rejected(Other), "insufficient liquidity"),
    ("NO_MERGEABLE_ORDERS", Rejected(Other), "orders can be merged not found"),
    // The venue may have placed the order: looked up, never sent again.
    ("FAILED_RETRIEVE_ASSETS", Unknown, "failed to retrieve account assets"),
    // A cancel that may or may not have gone through: the order is read again.
    ("CANCEL_FAIL", Unknown, "order cancel failed"),
    // Futures related.
    ("USER_NOT_FOUND", Permission, "user has no futures account"),
    ("CONTRACT_NO_COUNTER", Rejected(Other), "no counter order found"),
    ("CONTRACT_NOT_FOUND", InvalidRequest, "contract not found"),
    ("RISK_LIMIT_EXCEEDED", Rejected(Other), "risk limit exceeded"),
    ("INSUFFICIENT_AVAILABLE", Rejected(Funds), "balance is not enough"),
    ("LIQUIDATE_IMMEDIATELY", Rejected(Funds), "operation may cause liquidation"),
    ("LEVERAGE_TOO_HIGH", Rejected(Other), "leverage too high"),
    ("LEVERAGE_TOO_LOW", Rejected(Other), "leverage too low"),
    ("ORDER_NOT_OWNED", Rejected(NotFound), "order not owned"),
    ("ORDER_FINISHED", Rejected(Final), "order already finished"),
    ("POSITION_IN_LIQUIDATION", Rejected(Other), "position is being liquidated"),
    ("POSITION_IN_CLOSE", Rejected(Other), "position is closing"),
    ("POSITION_EMPTY", Rejected(ReduceOnly), "position is empty"),
    ("PRICE_TOO_DEVIATED", Rejected(Price), "order price deviates too much from mark price"),
    ("SIZE_TOO_LARGE", Rejected(VenueRule), "order size exceeds maximum"),
    ("SIZE_TOO_SMALL", Rejected(VenueRule), "order size does not reach minimum"),
    ("PRICE_OVER_LIQUIDATION", Rejected(Price), "price to increase position can not exceed liquidation price"),
    ("PRICE_OVER_BANKRUPT", Rejected(Price), "price to decrease position cannot exceed bankrupting price"),
    ("ORDER_POC_IMMEDIATE", Rejected(PostOnly), "POC order will be finished immediately"),
    ("INCREASE_POSITION", Rejected(ReduceOnly), "POC order will increase position"),
    ("CONTRACT_IN_DELISTING", Rejected(MarketClosed), "contract is delisting, only reduce-only order or close order is allowed"),
    ("POSITION_NOT_FOUND", Rejected(ReduceOnly), "position not found"),
    // Sent for the other position mode: the mode is read again (`invalidates_position_mode`).
    ("POSITION_DUAL_MODE", InvalidRequest, "operation forbidden in dual-mode"),
    ("REDUCE_EXCEEDED", Rejected(ReduceOnly), "reduce order would exceed position in dual-mode"),
    ("AMEND_WITH_STOP", InvalidRequest, "amend forbidden with stop order"),
    ("ORDER_FOK", Rejected(Other), "killed for FOK"),
    ("ORDER_PENDING", Rejected(Other), "operation forbidden with pending orders"),
    ("POSITION_HOLDING", Rejected(Other), "operation forbidden with holding position"),
    ("POSITION_CROSS_MARGIN", Rejected(Other), "margin updating is not allowed in cross margin"),
    // Server errors: the venue may have acted.
    ("INTERNAL", Unknown, "internal server error"),
    ("SERVER_ERROR", Unknown, "internal server error"),
    ("INTERNAL_SERVER_ERROR", Unknown, "operation failed, please try again later"),
    ("TOO_BUSY", Unknown, "server is too busy at the moment"),
    // Listed under Flash Convert, shared label namespace.
    ("TOO_MANY_REQUESTS", RateLimited, "request rate exceeds limits"),
    ("SERVER_TIMEOUT", Unknown, "service timeout"),
    ("REQUEST_FORBIDDEN", Rejected(Other), "asset management product is under liquidation; only USDT purchases are allowed"),
];

/// The calls that read or cancel one order by its id: a 404 with no label there is the order
/// the venue does not know, so a lookup moves on to its next call and a cancel reads it back.
const BY_ID: &[&str] = &["spot_order", "futures_order", "spot_cancel", "futures_cancel", "spot_price_order", "futures_price_order", "spot_price_cancel", "futures_price_cancel"];

/// The account mode asked before the spot price-triggered orders are listed (`GET
/// /unified/unified_mode`): a key refused it reads the classic account's list, as the account
/// read does.
pub(super) const PRICE_MODE: &str = "spot_price_mode";

#[derive(Deserialize)]
struct Refused {
    label: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

/// One row of a batch answer (`succeeded`, `label`, `message`).
#[derive(Deserialize)]
struct BatchRow {
    #[serde(default)]
    succeeded: Option<bool>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    message: String,
}

fn row(label: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|(c, _, _)| *c == label)
}

/// A label with the venue's words, mapped.
pub(super) fn mapped(label: &str, msg: &str) -> VenueError {
    let msg = if msg.trim().is_empty() { label } else { msg.trim() };
    match row(label) {
        Some((_, canon, _)) => VenueError::new(*canon, Some(label.to_string()), format!("Gate.io refused it: {msg} (code {label})")),
        None => VenueError::unmapped(Some(label.to_string()), format!("Gate.io answered code {label} ({msg}), which OpenTraderWorld does not map")),
    }
}

/// When the venue's limit resets, epoch milliseconds (`X-Gate-RateLimit-Reset-Timestamp`: the
/// overview names the header, not its unit, so a value under 10^12 is read as seconds).
fn reset_at(frame: &RawFrame) -> Option<i64> {
    let v = frame.header("x-gate-ratelimit-reset-timestamp")?.trim().parse::<i64>().ok()?;
    Some(if v < 1_000_000_000_000 { v.saturating_mul(1000) } else { v })
}

/// The error an order call's answer carries, mapped; `None` for a success. `batch` names an
/// endpoint whose 2xx answer is a list of per-row results (`succeeded`, `label`): its first
/// failed row is the call's error. The account mode asked before the spot price-triggered
/// orders ([`PRICE_MODE`]) refused for want of permission, as invalid or as not found is no
/// error: the classic account's list is read.
pub(super) fn decode(frame: &RawFrame, batch: bool) -> Option<VenueError> {
    let e = refusal(frame, batch)?;
    let mode_refused = matches!(frame, RawFrame::Http { endpoint_id, .. } if *endpoint_id == PRICE_MODE) && matches!(e.canon, Permission | InvalidRequest | Rejected(NotFound));
    (!mode_refused).then_some(e)
}

fn refusal(frame: &RawFrame, batch: bool) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        return Some(VenueError::new(Unknown, None, format!("gateio: {}", http::timeout(frame, "gateio").map(|e| e.message).unwrap_or_else(|| "no answer".into()))));
    };
    let status = *status;
    if status == 202 {
        // "Request accepted by the server, but processing is not done yet".
        return Some(VenueError::new(Unknown, Some("202".into()), format!("gateio {endpoint_id}: accepted, not processed yet (HTTP 202)")));
    }
    if (200..300).contains(&status) {
        if batch {
            let rows: Vec<BatchRow> = serde_json::from_slice(body).ok()?;
            let bad = rows.into_iter().find(|r| r.succeeded == Some(false))?;
            return Some(if bad.label.is_empty() { VenueError::unmapped(None, format!("Gate.io answered a failed row with no label ({})", bad.message)) } else { mapped(&bad.label, &bad.message) });
        }
        return None;
    }
    if let Ok(r) = serde_json::from_slice::<Refused>(body) {
        let msg = r.message.or(r.detail).unwrap_or_default();
        let mut e = mapped(&r.label, &msg);
        if status == 429 && e.canon != RateLimited && e.mapped {
            e.canon = RateLimited;
        }
        if e.canon == RateLimited {
            e.retry_after_ms = http::retry_after_ms(frame);
            e.retry_at_ms = reset_at(frame);
        }
        return Some(e);
    }
    // No label: the HTTP status alone.
    let e: ConnectorError = http::classify(frame, "gateio")?;
    let canon = match e.kind {
        ErrorKind::NotFound | ErrorKind::InvalidRequest if status == 404 && BY_ID.contains(endpoint_id) => Rejected(NotFound),
        ErrorKind::RateLimited => RateLimited,
        ErrorKind::Auth => Auth,
        ErrorKind::Permission => Permission,
        ErrorKind::InvalidRequest | ErrorKind::NotFound => InvalidRequest,
        // A 5xx: the venue may have acted.
        _ => Unknown,
    };
    let mut v = VenueError::new(canon, e.provider_code.clone(), e.message);
    v.retry_after_ms = e.retry_after_ms;
    if canon == RateLimited {
        v.retry_at_ms = reset_at(frame);
    }
    Some(v)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn every_listed_label_maps_and_an_unlisted_one_alerts() {
        for (label, canon, _) in ERRORS {
            let body = format!(r#"{{"label":"{label}","message":"x"}}"#);
            let e = decode(&http_frame(1, "spot_create", 400, &body), false).unwrap();
            assert_eq!((e.canon, e.mapped), (*canon, true), "{label}");
            assert_eq!(e.message, format!("Gate.io refused it: x (code {label})"));
        }
        let e = decode(&http_frame(1, "spot_create", 400, r#"{"label":"SOMETHING_NEW","message":"new"}"#), false).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        assert_eq!(e.message, "Gate.io answered code SOMETHING_NEW (new), which OpenTraderWorld does not map");
        assert!(decode(&http_frame(1, "spot_create", 201, r#"{"id":"1"}"#), false).is_none());
        assert!(decode(&http_frame(1, "spot_cancel", 204, ""), false).is_none());
    }

    /// The PROVIDER.md table ("Trading", "Errors") names exactly the labels mapped here, each
    /// under the canonical kind it maps to.
    #[test]
    fn the_provider_table_matches_the_code() {
        let doc = include_str!("PROVIDER.md");
        let start = doc.find("### Errors").unwrap();
        let table = &doc[start..start + doc[start..].find("### Rate limits for orders").unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in table.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let (labels, canon) = (cells[1], cells[3]);
            for l in labels.split('`').skip(1).step_by(2) {
                seen.push((l.to_string(), canon.to_string()));
            }
        }
        let word = |c: &Canon| match c {
            Rejected(VenueRule) => "Rejected(venue_rule)",
            Rejected(Funds) => "Rejected(funds)",
            Rejected(Price) => "Rejected(price)",
            Rejected(MarketClosed) => "Rejected(market closed)",
            Rejected(NotFound) => "Rejected(not found)",
            Rejected(Final) => "Rejected(final)",
            Rejected(PostOnly) => "Rejected(post-only)",
            Rejected(ReduceOnly) => "Rejected(reduce-only)",
            Rejected(_) => "Rejected(other)",
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
    fn no_label_is_listed_twice() {
        let mut labels: Vec<&str> = ERRORS.iter().map(|r| r.0).collect();
        labels.sort_unstable();
        let n = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), n);
    }

    #[test]
    fn a_5xx_a_202_or_a_timeout_is_unknown_and_a_rate_limit_holds() {
        assert_eq!(decode(&http_frame(1, "spot_create", 502, "<html>bad gateway</html>"), false).unwrap().canon, Unknown);
        assert_eq!(decode(&http_frame(1, "spot_create", 202, ""), false).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "spot_create" };
        assert_eq!(decode(&t, false).unwrap().canon, Unknown);
        let RawFrame::Http { corr_id, endpoint_id, body, .. } = http_frame(1, "spot_create", 429, r#"{"label":"TOO_MANY_REQUESTS","message":"Request Rate limit Exceeded"}"#) else { unreachable!() };
        let limited = RawFrame::Http { corr_id, endpoint_id, status: 429, headers: vec![("x-gate-ratelimit-reset-timestamp".into(), "1767225630".into())], body };
        let e = decode(&limited, false).unwrap();
        assert_eq!((e.canon, e.retry_at_ms), (RateLimited, Some(1_767_225_630_000)));
        assert_eq!(decode(&http_frame(1, "spot_create", 401, ""), false).unwrap().canon, Auth);
    }

    #[test]
    fn a_batch_answer_carries_its_rows_failure() {
        let ok = r#"[{"order_id":"1","succeeded":true,"label":"","message":""}]"#;
        assert!(decode(&http_frame(1, "spot_amend_batch", 200, ok), true).is_none());
        let bad = r#"[{"order_id":"1","succeeded":false,"label":"ORDER_NOT_FOUND","message":"Order not found"}]"#;
        let e = decode(&http_frame(1, "spot_amend_batch", 200, bad), true).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Rejected(NotFound), Some("ORDER_NOT_FOUND")));
    }
}
