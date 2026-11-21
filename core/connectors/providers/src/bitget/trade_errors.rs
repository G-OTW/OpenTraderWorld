//! Bitget's order error table (onboarding spec 8), from the Classic REST error page
//! (https://www.bitget.com/docs/classic/error-code/restapi, read 2026-11-11): every code of its
//! flat list that an order call (place, cancel, cancel all, look up, the reads the order path
//! makes) can answer, mapped to one canonical kind. A code not listed here is `Unknown` and
//! alerts. Mirrored in PROVIDER.md ("Trading", "Errors"); a test keeps the two in step.
//!
//! The read path keeps its own mapping (`errors.rs`): there `40010`, `40015` and `45001` are a
//! transient read failure to retry. On the order path they say the venue may have acted, so they
//! are `Unknown` here: looked up, never sent again.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;
use serde_json::Value;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, PostOnly, Price, ReduceOnly, VenueRule};

/// How long a rate-limit refusal holds new orders when the answer states no wait: one window of
/// Bitget's per-second order limits ("10 requests/second/UID", rate-limit page).
const RATE_WINDOW_MS: u64 = 1_000;

/// `(code, canonical, the doc's words)`.
pub(super) const ERRORS: &[ErrorRow] = &[
    ("429", RateLimited, "too many requests"),
    // Request and parameters.
    ("00171", InvalidRequest, "parameter verification failed"),
    ("00172", InvalidRequest, "parameter verification failed"),
    ("01001", InvalidRequest, "a value must be greater than 0"),
    ("01002", Rejected(VenueRule), "precision must be less than or equal to the limit"),
    ("01003", InvalidRequest, "duplicate data exists"),
    // Futures orders (22xxx).
    ("22001", Rejected(NotFound), "no order to cancel"),
    ("22002", Rejected(ReduceOnly), "no position to close"),
    ("22004", Rejected(MarketClosed), "this symbol does not support API trade"),
    ("22005", InvalidRequest, "this symbol does not support cross mode"),
    ("22006", Rejected(Price), "limit price above the risk price"),
    ("22007", Rejected(Price), "limit price below the risk price"),
    ("22008", Rejected(Price), "market price above the risk price"),
    ("22009", Rejected(Price), "market price below the risk price"),
    ("22010", Permission, "bind an IP whitelist address"),
    ("22029", Rejected(Funds), "risk control: maximum position open"),
    ("22030", Permission, "not available in demo trading mode"),
    ("22034", Rejected(VenueRule), "less than the minimum order amount"),
    ("22035", RateLimited, "demo account opens positions too frequently"),
    ("22038", Rejected(VenueRule), "quantity must be an integral multiple"),
    ("22039", Rejected(MarketClosed), "opening limited as delivery approaches"),
    ("22040", Rejected(MarketClosed), "open orders limited as delivery approaches"),
    ("22041", Rejected(MarketClosed), "close orders limited as delivery approaches"),
    ("22042", Rejected(ReduceOnly), "one-way position held: a trigger order cannot only reduce"),
    ("22043", Rejected(MarketClosed), "ADL processing: close limited"),
    ("22044", Rejected(MarketClosed), "ADL processing: no flash close"),
    ("22045", Rejected(Other), "insufficient liquidity in the market"),
    ("22067", Rejected(MarketClosed), "ADL processing: the symbol is forbidden"),
    // Common (40xxx): authentication, permissions, request.
    ("40001", Auth, "ACCESS_KEY cannot be empty"),
    ("40002", Auth, "ACCESS_SIGN cannot be empty"),
    ("40003", Auth, "signature cannot be empty"),
    ("40005", Auth, "invalid ACCESS_TIMESTAMP"),
    ("40006", Auth, "invalid ACCESS_KEY"),
    ("40007", InvalidRequest, "invalid Content_Type"),
    ("40008", Auth, "request timestamp expired"),
    ("40009", Auth, "sign signature error"),
    ("40010", Unknown, "request timed out"),
    ("40011", Auth, "ACCESS_PASSPHRASE cannot be empty"),
    ("40012", Auth, "apikey or password is incorrect"),
    ("40013", Permission, "user status is abnormal"),
    ("40014", Permission, "incorrect permissions"),
    ("40015", Unknown, "system is abnormal, try again later"),
    ("40016", Permission, "bind a phone or Google first"),
    ("40017", InvalidRequest, "parameter verification failed"),
    ("40018", Auth, "invalid IP"),
    ("40019", InvalidRequest, "a parameter cannot be empty"),
    ("40020", InvalidRequest, "parameter error"),
    ("40022", Permission, "the business of this account is restricted"),
    ("40023", Permission, "API service restricted"),
    ("40024", Permission, "account frozen"),
    ("40025", Permission, "the user does not have this permission"),
    ("40026", Permission, "user is disabled"),
    ("40031", Permission, "account cancelled"),
    ("40034", InvalidRequest, "a parameter does not exist"),
    ("40035", Permission, "complete KYC first"),
    ("40036", Auth, "passphrase is wrong"),
    ("40037", Auth, "apikey does not exist"),
    ("40038", Auth, "IP not in the apikey whitelist"),
    ("40040", Permission, "api key permission setting error"),
    ("40041", Auth, "the user's apikey does not exist"),
    ("40053", InvalidRequest, "value range verification failed"),
    ("40057", InvalidRequest, "a parameter does not meet the specification"),
    ("40058", InvalidRequest, "a parameter allows a maximum"),
    ("40059", InvalidRequest, "a parameter should be less than its limit"),
    ("40064", InvalidRequest, "parameter verification failed"),
    ("40072", InvalidRequest, "symbol invalid or not supported for futures trade"),
    ("40078", Auth, "timestamp outside the receiveWindow"),
    ("40079", InvalidRequest, "receiveWindow must be under 60 s"),
    ("40081", Permission, "spot demo accounts reach spot order endpoints only"),
    ("40100", Permission, "Hong Kong IPs need identity verification"),
    ("40101", Permission, "complete KYC"),
    ("40102", InvalidRequest, "symbol does not exist"),
    ("40103", Permission, "region restricted"),
    ("40104", Permission, "region restricted"),
    ("40105", Permission, "spot demo trading does not support this feature"),
    ("40109", Rejected(NotFound), "the order cannot be found"),
    ("40110", Permission, "unavailable in demo trading"),
    ("40111", Permission, "restricted country: complete KYC"),
    ("40199", Permission, "traders are prohibited from calling the API"),
    ("40200", Unknown, "server upgrade, try again later"),
    ("40301", Permission, "permission not obtained"),
    ("40304", InvalidRequest, "clientOid longer than 50"),
    ("40305", InvalidRequest, "clientOid longer than 64 or with invalid characters"),
    ("40309", Rejected(MarketClosed), "the contract has been removed"),
    ("40402", InvalidRequest, "orderId or clientOid format error"),
    ("40404", InvalidRequest, "request URL not found"),
    // Futures order and position checks (407xx to 409xx).
    ("40706", Rejected(Price), "wrong order price"),
    ("40710", Permission, "account frozen"),
    ("40715", Rejected(Other), "delegate count above the open count"),
    ("40716", InvalidRequest, "this pair does not support cross margin"),
    ("40717", Rejected(ReduceOnly), "closed size cannot exceed the size held"),
    ("40718", Rejected(Price), "close-long price below the liquidation price"),
    ("40719", Rejected(Price), "close-short price above the liquidation price"),
    ("40721", Rejected(MarketClosed), "market orders not allowed at present"),
    ("40722", Rejected(Funds), "market order cost insufficient after price moves"),
    ("40723", Rejected(Other), "too many unexecuted orders"),
    ("40724", InvalidRequest, "parameter is empty"),
    ("40725", Unknown, "service returned an error"),
    ("40752", Permission, "disabled for this business"),
    ("40753", Permission, "contract trading disabled"),
    ("40754", Rejected(Funds), "balance not enough"),
    ("40755", Rejected(ReduceOnly), "not enough open positions available"),
    ("40756", Rejected(Funds), "balance lock insufficient"),
    ("40757", Rejected(ReduceOnly), "not enough position available"),
    ("40758", Rejected(ReduceOnly), "position lock insufficient"),
    ("40760", Permission, "account status abnormal"),
    ("40761", Rejected(Other), "too many unfilled orders"),
    ("40762", Rejected(Funds), "order size above the max open size"),
    ("40764", Rejected(Other), "order remaining amount below the traded volume"),
    ("40765", Rejected(ReduceOnly), "position remaining volume below the traded volume"),
    ("40766", Rejected(Other), "open orders below this traded volume"),
    ("40778", InvalidRequest, "this pair does not take this margin coin"),
    ("40798", Rejected(Funds), "insufficient contract account balance"),
    ("40800", Rejected(Funds), "insufficient margin"),
    ("40804", Rejected(ReduceOnly), "closed size cannot exceed the positions held"),
    ("40805", InvalidRequest, "unsupported operation"),
    ("40808", Unknown, "parameter verification exception (a release window, FAQ Q13)"),
    ("40811", InvalidRequest, "a parameter should not be null"),
    ("40812", InvalidRequest, "a condition is not met"),
    ("40813", InvalidRequest, "a parameter must have a value"),
    ("40815", Rejected(Price), "order price above the highest bid"),
    ("40816", Rejected(Price), "order price below the lowest ask"),
    ("40820", Rejected(Price), "close-long price below the liquidation price"),
    ("40821", Rejected(Price), "closing price above the liquidation price"),
    ("40822", InvalidRequest, "contract configuration does not exist"),
    ("40823", Rejected(MarketClosed), "no traded or mark price"),
    ("40824", Rejected(MarketClosed), "market orders not allowed now"),
    ("40825", Rejected(MarketClosed), "no opposite depth"),
    ("40826", Rejected(Funds), "market order cost insufficient"),
    ("40828", Permission, "market making accounts cannot place orders by hand"),
    ("40829", Rejected(Price), "long take profit not above the average open price"),
    ("40830", Rejected(Price), "long take profit not above the current price"),
    ("40831", Rejected(Price), "short take profit not below the average open price"),
    ("40832", Rejected(Price), "short take profit not below the current price"),
    ("40833", Rejected(Price), "long stop loss not below the average open price"),
    ("40834", Rejected(Price), "long stop loss not below the current price"),
    ("40835", Rejected(Price), "short stop loss not above the average open price"),
    ("40836", Rejected(Price), "short stop loss not above the current price"),
    ("40837", Rejected(ReduceOnly), "no position: no take profit or stop loss"),
    ("40844", Rejected(MarketClosed), "contract under temporary maintenance"),
    ("40845", Rejected(MarketClosed), "contract removed"),
    ("40846", Rejected(Other), "status verification abnormal"),
    ("40847", Rejected(Other), "the operation cannot be performed"),
    ("40859", Rejected(Other), "this order is being closed"),
    ("40862", Rejected(Other), "cancel the current order first"),
    ("40863", Rejected(Other), "cancel the current plan first"),
    ("40865", Rejected(Other), "order being commissioned: no close now"),
    ("40878", Rejected(MarketClosed), "index data abnormal, try again later"),
    ("40879", Rejected(Other), "risk being processed"),
    ("40887", Rejected(VenueRule), "above the single flash open maximum"),
    ("40888", Rejected(VenueRule), "above the single flash close maximum"),
    ("40889", Rejected(Other), "plan orders of this contract at the upper limit"),
    ("40890", Rejected(Other), "take profit and stop loss orders at the upper limit"),
    ("40891", Rejected(ReduceOnly), "insufficient position for a take profit or stop loss"),
    ("40908", Unknown, "concurrent operation failed"),
    ("40910", Unknown, "operation timed out"),
    ("40912", InvalidRequest, "a single cancel takes at most 50"),
    ("40913", InvalidRequest, "one of the parameters must be passed"),
    ("40915", Rejected(Price), "long take profit not above the mark price"),
    ("40916", Permission, "futures services restricted for this account"),
    ("40917", Rejected(Price), "long stop not below the mark price"),
    ("40918", RateLimited, "positions opened too frequently"),
    ("40919", InvalidRequest, "function not open yet"),
    ("40921", Rejected(VenueRule), "order size above the position level maximum"),
    ("40922", Rejected(Final), "only working orders can be modified"),
    ("40923", InvalidRequest, "size and price unchanged"),
    ("40924", InvalidRequest, "orderId or clientOid required"),
    ("40925", InvalidRequest, "price and size must be passed together"),
    ("40928", Rejected(Funds), "risk control: maximum open size"),
    ("40930", Rejected(PostOnly), "post-only quantity limit"),
    ("40931", Rejected(Other), "risk control prohibits closing"),
    ("40939", Rejected(ReduceOnly), "reduce-only would exceed the position: cancel or change the pending order"),
    // Spot and futures orders (43xxx).
    ("43001", Rejected(NotFound), "the order does not exist"),
    ("43002", Rejected(Other), "pending order failed"),
    ("43003", Rejected(Other), "pending order failed"),
    ("43004", Rejected(NotFound), "there is no order to cancel"),
    ("43005", Rejected(Other), "maximum number of orders exceeded"),
    ("43006", Rejected(VenueRule), "quantity below the minimum"),
    ("43007", Rejected(VenueRule), "quantity above the maximum"),
    ("43008", Rejected(Price), "price below the limit"),
    ("43009", Rejected(Price), "price above the limit"),
    ("43010", Rejected(VenueRule), "amount below the minimum"),
    ("43011", InvalidRequest, "a parameter does not meet the specification"),
    ("43012", Rejected(Funds), "insufficient balance"),
    ("43013", Rejected(Price), "take profit must be above the current price"),
    ("43014", Rejected(Price), "take profit must be below the current price"),
    ("43015", Rejected(Price), "stop loss must be below the current price"),
    ("43016", Rejected(Price), "stop loss must be above the current price"),
    ("43020", Rejected(NotFound), "take profit or stop loss order does not exist"),
    ("43021", Rejected(Final), "take profit or stop loss order already closed"),
    ("43023", Rejected(ReduceOnly), "insufficient position for a take profit or stop loss"),
    ("43024", Rejected(Other), "take profit or stop loss on an existing order"),
    ("43025", Rejected(NotFound), "plan order does not exist"),
    ("43026", Rejected(Final), "plan order already closed"),
    ("43027", Rejected(VenueRule), "minimum order value not met"),
    ("43028", Rejected(VenueRule), "price must be an integer multiple of the step"),
    ("43029", Rejected(ReduceOnly), "size above the maximum that can be closed"),
    ("43030", Rejected(Other), "take profit order already exists"),
    ("43031", Rejected(Other), "stop loss order already exists"),
    ("43034", Rejected(Price), "trigger price must be at or below the market"),
    ("43035", Rejected(Price), "trigger price must be at or above the market"),
    ("43037", Rejected(VenueRule), "below the minimum order amount"),
    ("43038", Rejected(VenueRule), "above the maximum order amount"),
    ("43039", Rejected(Price), "above the maximum price limit"),
    ("43040", Rejected(Price), "below the minimum price limit"),
    ("43041", Rejected(VenueRule), "above the maximum transaction amount"),
    ("43042", Rejected(VenueRule), "below the minimum transaction amount"),
    ("43043", Rejected(ReduceOnly), "there is no position"),
    ("43048", InvalidRequest, "the symbol is null"),
    ("43049", InvalidRequest, "margin coin not allowed"),
    ("43050", Rejected(Other), "leverage outside the effective range"),
    ("43058", InvalidRequest, "parameter verification failed"),
    ("43059", Unknown, "request failed, try again"),
    ("43071", Rejected(Other), "trigger order limit for this pair"),
    ("43075", InvalidRequest, "position mode mismatch"),
    ("43118", Unknown, "clientOrderId duplicate"),
    ("43128", Rejected(Other), "maximum number of orders for the pair"),
    // Futures (45xxx).
    ("45001", Unknown, "unknown error"),
    ("45002", Rejected(Funds), "insufficient asset"),
    ("45003", Rejected(ReduceOnly), "insufficient position"),
    ("45004", Rejected(Funds), "insufficient locked asset"),
    ("45005", Rejected(ReduceOnly), "insufficient available positions"),
    ("45006", Rejected(ReduceOnly), "insufficient position"),
    ("45007", Rejected(ReduceOnly), "insufficient locked position"),
    ("45008", Rejected(Funds), "no assets"),
    ("45009", Rejected(Other), "account at risk: no trade for now"),
    ("45011", Rejected(Other), "order remaining volume below the traded volume"),
    ("45012", Rejected(ReduceOnly), "position remaining volume below the traded volume"),
    ("45013", Rejected(Other), "open orders below the traded volume"),
    ("45014", Rejected(Other), "position does not exist during opening"),
    ("45017", InvalidRequest, "settlement or transaction coin configuration not found"),
    ("45018", InvalidRequest, "netting position: no liquidation order"),
    ("45019", Permission, "account does not exist"),
    ("45020", InvalidRequest, "liquidation only under two-way positions"),
    ("45021", InvalidRequest, "one-way position: the order must be of one-way type"),
    ("45023", Unknown, "error creating order"),
    ("45024", Unknown, "cancel order error"),
    ("45025", InvalidRequest, "this pair does not take the currency as margin"),
    ("45026", InvalidRequest, "wrong delegateType"),
    ("45031", Rejected(Final), "the order is finalized"),
    ("45034", Unknown, "clientOid duplicate"),
    ("45035", Rejected(VenueRule), "price step mismatch"),
    ("45043", Rejected(MarketClosed), "trade suspended for settlement or maintenance"),
    ("45051", InvalidRequest, "execution price verification abnormal"),
    ("45052", InvalidRequest, "trigger price verification abnormal"),
    ("45055", Rejected(Final), "the order status cannot be cancelled"),
    ("45056", InvalidRequest, "this order type cannot be cancelled"),
    ("45057", Rejected(NotFound), "the order does not exist"),
    ("45060", Rejected(Price), "long take profit not above the current price"),
    ("45061", Rejected(Price), "short take profit not below the current price"),
    ("45062", Rejected(Price), "long stop loss not below the current price"),
    ("45064", Rejected(Price), "long take profit not above the order price"),
    ("45065", Rejected(Price), "short take profit not below the order price"),
    ("45066", Rejected(Price), "long stop loss not below the order price"),
    ("45067", Rejected(Price), "short stop loss not above the order price"),
    ("45068", Rejected(ReduceOnly), "no position: no take profit or stop loss"),
    ("45103", Rejected(VenueRule), "above the single flash open maximum"),
    ("45104", Rejected(VenueRule), "above the single flash close maximum"),
    ("45107", Permission, "API restricted from opening positions"),
    ("45108", Permission, "API restricted from closing positions"),
    ("45109", InvalidRequest, "the account is in two-way position mode"),
    ("45110", Rejected(VenueRule), "below the minimum amount in USDT"),
    ("45111", Rejected(VenueRule), "below the minimum order quantity"),
    ("45112", Rejected(VenueRule), "above the maximum order quantity"),
    ("45113", Rejected(VenueRule), "maximum order value limit"),
    ("45114", Rejected(VenueRule), "minimum order requirement not met"),
    ("45115", Rejected(VenueRule), "price must be a multiple of the step"),
    ("45116", Rejected(Other), "position count above the maximum"),
    ("45118", Rejected(Other), "order count at the upper limit"),
    ("45119", Rejected(MarketClosed), "this symbol does not support opening"),
    ("45120", Rejected(VenueRule), "size above the max open size"),
    ("45121", Rejected(Price), "mark price deviates too much from the market"),
    ("45122", Rejected(Price), "short stop loss not above the mark price"),
    ("45123", Rejected(MarketClosed), "only market orders can be placed now"),
    ("45124", InvalidRequest, "edit and submit again"),
    ("45127", Rejected(Other), "position brackets disable take profit and stop loss"),
    ("45128", Rejected(Other), "position brackets disable a change of size"),
    ("45129", RateLimited, "the same orderId can be cancelled once a second"),
    // Elsewhere in the list, reachable from an order call.
    ("49040", Unknown, "unknown error"),
    ("50031", Unknown, "system error"),
    ("50060", Unknown, "duplicated clientOid"),
    ("50062", Rejected(Final), "the order is cancelled or filled"),
    ("59044", RateLimited, "operations too frequent"),
    ("70227", Permission, "the user is not allowed to place orders"),
    ("70228", RateLimited, "operation too frequent"),
    ("80002", Unknown, "system error"),
    // Spot buy and sell quotas, liquidity and order counts; futures close sizes and the busy
    // close (independent doc review, 2026-11-12).
    ("12002", Rejected(Other), "limit on the net sell value of this coin"),
    ("12003", Rejected(Other), "limit on the net buy value of this coin"),
    ("13007", Rejected(Other), "24-hour limit on the net buy value of this coin"),
    ("13009", Rejected(Other), "24-hour limit on the net sell value of this coin"),
    ("40746", Rejected(ReduceOnly), "more than the position that can be closed now"),
    ("40748", Rejected(Price), "the order price is above the highest bid"),
    ("40867", Rejected(ReduceOnly), "more than the position that can be closed now"),
    ("42016", Rejected(MarketClosed), "the symbol is invalid or not traded on spot"),
    ("46013", Rejected(Other), "this symbol limits the amount sold"),
    ("50066", RateLimited, "the position is closing, try again later"),
    ("70220", Rejected(Other), "not enough liquidity in the market"),
    ("70221", Rejected(Other), "limit on the net buy value of this coin"),
    ("70222", Rejected(Other), "limit on the net buy quantity of this coin"),
    ("70223", Rejected(Other), "more than the maximum number of orders"),
    ("70224", Rejected(Other), "selling limit of this coin"),
    ("70225", Rejected(Other), "buying limit of this coin"),
];

/// The endpoints whose single-order answer carries the refusal inside a success
/// (`failureList`): a futures plan order, or a position's take profit or stop loss, cancelled
/// by its id.
pub(super) const ROW_ANSWERS: &[&str] = &["mix_plan_cancel", "mix_tpsl_cancel"];

/// The endpoint whose success answers `data.result`, "success or failure" (spot Cancel Plan
/// Order): no list, no code.
pub(super) const RESULT_ANSWERS: &[&str] = &["spot_plan_cancel"];

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<Value>,
}

fn row(code: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|(c, _, _)| *c == code)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// One code and message, mapped: the table's kind, or unknown and unmapped.
pub(super) fn mapped(code: &str, msg: &str) -> VenueError {
    match row(code) {
        Some((_, Unknown, _)) => VenueError::new(Unknown, Some(code.to_string()), format!("Bitget answered: {msg} (code {code}), so the order may have gone through")),
        Some((_, canon, _)) => VenueError::new(*canon, Some(code.to_string()), format!("Bitget refused it: {msg} (code {code})")),
        None => VenueError::unmapped(Some(code.to_string()), format!("Bitget answered code {code} ({msg}), which OpenTraderWorld does not map")),
    }
}

/// A row of a `failureList`: its code when the doc gives one (`errorCode`), else its words only,
/// which no table maps.
pub(super) fn failure_row(r: &Value) -> VenueError {
    let msg = text(&r["errorMsg"]);
    match r.get("errorCode").map(text).filter(|c| !c.is_empty()) {
        Some(code) => mapped(&code, &msg),
        None => VenueError::unmapped(None, format!("Bitget answered {msg:?} for the order, which OpenTraderWorld does not map")),
    }
}

/// The error an order call's answer carries, mapped; `None` for a success (`code` `00000`).
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        return Some(VenueError::new(Unknown, None, format!("bitget: {}", http::timeout(frame, "bitget").map(|e| e.message).unwrap_or_else(|| "no answer".into()))));
    };
    let status = *status;
    let env = serde_json::from_slice::<Envelope>(body).ok();
    let code = env.as_ref().and_then(|e| e.code.as_ref()).map(text).filter(|c| !c.is_empty());
    let mut e = match code.as_deref() {
        Some("00000") if (200..300).contains(&status) => {
            // A plan order cancelled by id answers its refusal inside the success.
            if ROW_ANSWERS.contains(endpoint_id) {
                let failed = env.as_ref().and_then(|e| e.data.as_ref()).and_then(|d| d.get("failureList")).and_then(Value::as_array).and_then(|l| l.first().cloned());
                return failed.map(|r| failure_row(&r));
            }
            if RESULT_ANSWERS.contains(endpoint_id) {
                let result = env.as_ref().and_then(|e| e.data.as_ref()).and_then(|d| d.get("result")).map(text).unwrap_or_default();
                // Anything but `success` is no cancel: not mapped, looked up by its id.
                return (!result.eq_ignore_ascii_case("success")).then(|| {
                    VenueError::unmapped(None, format!("Bitget answered {result:?} to the plan order cancel, which OpenTraderWorld does not map"))
                });
            }
            return None;
        }
        Some(code) => {
            let msg = env.as_ref().and_then(|e| e.msg.clone()).unwrap_or_default();
            let mut e = mapped(code, &msg);
            if status == 429 && e.canon != RateLimited {
                e = VenueError::new(RateLimited, Some(code.to_string()), format!("Bitget refused it: too many requests ({msg}, code {code})"));
            }
            e
        }
        None => {
            // No envelope: the HTTP status alone.
            let c: ConnectorError = http::classify(frame, "bitget").unwrap_or_else(|| ConnectorError::decode(format!("bitget {endpoint_id}: no envelope code")));
            let canon = match c.kind {
                ErrorKind::RateLimited => RateLimited,
                ErrorKind::Auth => Auth,
                ErrorKind::Permission => Permission,
                ErrorKind::InvalidRequest | ErrorKind::NotFound if (400..500).contains(&status) => InvalidRequest,
                // A 5xx or an answer that cannot be read: the venue may have acted.
                _ => Unknown,
            };
            VenueError::new(canon, c.provider_code.clone(), format!("bitget {endpoint_id}: {}", c.message))
        }
    };
    if e.canon == RateLimited {
        e.retry_after_ms = Some(http::retry_after_ms(frame).unwrap_or(RATE_WINDOW_MS));
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
        for (code, canon, _) in ERRORS {
            let body = format!(r#"{{"code":"{code}","msg":"x","requestTime":1,"data":null}}"#);
            let e = decode(&http_frame(1, "spot_place", 400, &body)).unwrap();
            assert_eq!((e.canon, e.mapped), (*canon, true), "{code}");
        }
        let e = decode(&http_frame(1, "spot_place", 400, r#"{"code":"99999","msg":"new"}"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        assert!(e.message.contains("which OpenTraderWorld does not map"), "{}", e.message);
        assert!(decode(&http_frame(1, "spot_place", 200, r#"{"code":"00000","msg":"success","data":{"orderId":"1"}}"#)).is_none());
    }

    #[test]
    fn a_refusal_names_the_venue_code_and_words() {
        let e = decode(&http_frame(1, "mix_place", 400, r#"{"code":"40754","msg":"balance not enough"}"#)).unwrap();
        assert_eq!(e.message, "Bitget refused it: balance not enough (code 40754)");
        assert_eq!(e.code.as_deref(), Some("40754"));
    }

    /// The read path retries these; the order path must not (the venue may have acted).
    #[test]
    fn a_timeout_or_system_error_code_on_an_order_is_unknown() {
        for code in ["40010", "40015", "45001", "40808", "40725", "43118", "45034", "50060"] {
            let e = decode(&http_frame(1, "mix_place", 400, &format!(r#"{{"code":"{code}","msg":"m"}}"#))).unwrap();
            assert_eq!(e.canon, Unknown, "{code}");
        }
        assert_eq!(super::super::errors::from_frame(&http_frame(1, "x", 400, r#"{"code":"40015","msg":"m"}"#)).unwrap().kind, ErrorKind::Transient, "reads unchanged");
    }

    #[test]
    fn a_5xx_or_a_timeout_is_unknown_and_a_rate_limit_holds() {
        assert_eq!(decode(&http_frame(1, "spot_place", 502, "bad gateway")).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "spot_place" };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
        let r = decode(&http_frame(1, "spot_place", 429, r#"{"code":"429","msg":"Too Many Requests"}"#)).unwrap();
        assert_eq!((r.canon, r.retry_after_ms), (RateLimited, Some(1_000)));
        assert_eq!(decode(&http_frame(1, "spot_place", 401, "")).unwrap().canon, Auth);
    }

    #[test]
    fn a_plan_cancel_refused_inside_a_success_is_read() {
        let ok = r#"{"code":"00000","data":{"successList":[{"orderId":"1","clientOid":""}],"failureList":[]},"msg":"success"}"#;
        assert!(decode(&http_frame(1, "mix_plan_cancel", 200, ok)).is_none());
        let no = r#"{"code":"00000","data":{"successList":[],"failureList":[{"orderId":"3","clientOid":"123","errorMsg":"notExistend"}]},"msg":"success"}"#;
        let e = decode(&http_frame(1, "mix_plan_cancel", 200, no)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        // A batch reads its rows elsewhere: the call itself succeeded.
        assert!(decode(&http_frame(1, "mix_plan_cancel_all", 200, no)).is_none());
        let coded = r#"{"code":"00000","data":{"failureList":[{"orderId":"3","errorMsg":"notExistend","errorCode":"43001"}]}}"#;
        assert_eq!(decode(&http_frame(1, "mix_tpsl_cancel", 200, coded)).unwrap().canon, Rejected(NotFound));
    }

    /// Spot Cancel Plan Order answers `data.result`, "success or failure": a failure is no
    /// cancel (it was read as a success before).
    #[test]
    fn a_spot_plan_cancel_answering_failure_is_not_a_cancel() {
        let ok = r#"{"code":"00000","msg":"success","requestTime":1668134497496,"data":{"result":"success"}}"#;
        assert!(decode(&http_frame(1, "spot_plan_cancel", 200, ok)).is_none());
        let no = r#"{"code":"00000","msg":"success","requestTime":1668134497496,"data":{"result":"failure"}}"#;
        let e = decode(&http_frame(1, "spot_plan_cancel", 200, no)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        assert!(decode(&http_frame(1, "spot_plan_cancel", 200, r#"{"code":"00000","data":null}"#)).is_some(), "no result is no cancel");
    }

    /// Codes the first table missed (independent doc review): refusals carry their reason, the
    /// busy close holds new orders.
    #[test]
    fn the_review_codes_map_to_their_kind() {
        for (code, want) in [
            ("42016", Rejected(MarketClosed)),
            ("46013", Rejected(Other)),
            ("70220", Rejected(Other)),
            ("70223", Rejected(Other)),
            ("12003", Rejected(Other)),
            ("40748", Rejected(Price)),
            ("40746", Rejected(ReduceOnly)),
            ("40867", Rejected(ReduceOnly)),
            ("50066", RateLimited),
        ] {
            let e = decode(&http_frame(1, "mix_place", 400, &format!(r#"{{"code":"{code}","msg":"m"}}"#))).unwrap();
            assert_eq!((e.canon, e.mapped), (want, true), "{code}");
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

    /// The PROVIDER.md table ("Trading", "Errors") names exactly the codes mapped here, each
    /// under the canonical kind it maps to.
    #[test]
    fn the_provider_table_matches_the_code() {
        let doc = include_str!("PROVIDER.md");
        let start = doc.find("### Errors").unwrap();
        let table = &doc[start..start + doc[start..].find("### Rate limits for orders").unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in table.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let (codes, canon) = (cells[1], cells[2]);
            for c in codes.split('`').skip(1).step_by(2) {
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
}
