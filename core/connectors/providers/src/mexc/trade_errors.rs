//! MEXC's order error tables (onboarding spec 8), one per book, from the docs' error code
//! pages read 2026-11-12: spot https://www.mexc.com/api-docs/spot-v3/error-code, futures
//! https://www.mexc.com/api-docs/futures/error-code. Every code about orders, keys and the
//! request is mapped to one canonical kind; the futures page's campaign, grid-bot and Earn
//! codes (8002 to 8202, 8900 to 8911, 8925 to 8929, 8933 to 8946, 9065 to 9067, 9501 to 9533,
//! 9901 to 9918) concern no order call and stay unmapped. A code not listed is `Unknown` and
//! alerts. Mirrored in PROVIDER.md ("Trading", "Errors"); a test keeps the two in step.
//!
//! Spot refuses with an HTTP 4xx and `{"code","msg"}`. Futures answer HTTP 200 with
//! `{"success":false,"code","message"}`; a cancel by ids answers `success:true` with a row per
//! order (`orderId`, `errorCode`, `errorMsg`), read here for a single cancel.
//!
//! Lost answers (`Unknown`, looked up, never sent again): a timeout, an HTTP 5xx ("the execution
//! status is UNKNOWN and could have been a success"), spot 500, 503, 504, 20002 and 11444,
//! futures 500, 501, 603, 604, 801, 2042, 3016 to 3019, 9999 and 11444. Futures 6008 ("Order
//! placed successfully") is an acceptance, read by the ack.

use otw_connector_model::ErrorKind;
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;
use serde_json::Value;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, Price, ReduceOnly, VenueRule};

/// The futures code that says the order was placed.
pub(super) const PLACED: &str = "6008";

/// Spot: `(code, canonical, the doc's words)`.
pub(super) const SPOT_ERRORS: &[ErrorRow] = &[
    ("-2011", Rejected(NotFound), "Unknown order sent"),
    ("26", Rejected(Other), "operation not allowed"),
    ("400", Auth, "api key required"),
    ("401", Auth, "No authority"),
    ("403", Permission, "Access Denied"),
    ("429", RateLimited, "Too Many Requests"),
    ("500", Unknown, "Internal error"),
    ("503", Unknown, "service not available, please try again"),
    ("504", Unknown, "Gateway Time-out"),
    ("602", Auth, "Signature verification failed"),
    ("10001", Auth, "user does not exist"),
    ("10007", InvalidRequest, "bad symbol"),
    ("10072", Auth, "invalid access key"),
    ("10073", Auth, "invalid Request-Time"),
    ("10095", InvalidRequest, "amount cannot be null"),
    ("10096", Rejected(VenueRule), "amount decimal places is too long"),
    ("10097", Rejected(VenueRule), "amount is error"),
    ("10098", Rejected(Other), "risk control system detected abnormal"),
    ("10101", Rejected(Funds), "Insufficient balance"),
    ("10102", InvalidRequest, "amount cannot be zero or negative"),
    ("11444", Unknown, "This feature is temporarily unavailable. For details, please refer to our official announcement"),
    ("20002", Unknown, "Internal system error please contact support"),
    ("22222", Rejected(NotFound), "record does not exist"),
    ("30000", Rejected(MarketClosed), "suspended transaction for the symbol"),
    ("30001", Rejected(MarketClosed), "The current transaction direction is not allowed to place an order"),
    ("30002", Rejected(VenueRule), "The minimum transaction volume cannot be less than :"),
    ("30003", Rejected(VenueRule), "The maximum transaction volume cannot be greater than :"),
    ("30004", Rejected(Funds), "Insufficient position"),
    ("30005", Rejected(Funds), "Oversold"),
    ("30010", Rejected(Price), "no valid trade price"),
    ("30014", InvalidRequest, "invalid symbol"),
    ("30016", Rejected(MarketClosed), "trading disabled"),
    ("30018", Rejected(MarketClosed), "market order is disabled"),
    ("30019", Rejected(MarketClosed), "api market order is disabled"),
    ("30020", Permission, "no permission for the symbol"),
    ("30021", InvalidRequest, "invalid symbol"),
    ("30025", Rejected(Price), "no exist opponent order"),
    ("30026", Rejected(NotFound), "invalid order ids"),
    ("30027", Rejected(MarketClosed), "The currency has reached the maximum position limit, the buying is suspended"),
    ("30028", Rejected(MarketClosed), "The currency triggered the platform risk control, the selling is suspended"),
    ("30029", Rejected(VenueRule), "Cannot exceed the maximum order limit"),
    ("30032", Rejected(VenueRule), "Cannot exceed the maximum position"),
    ("30035", Rejected(Other), "You have triggered the order limit, please contact us by submitting a ticket."),
    ("30041", Rejected(VenueRule), "current order type can not place order"),
    ("33333", InvalidRequest, "param is error"),
    ("44444", InvalidRequest, "param cannot be null"),
    ("60005", Permission, "your account is abnormal"),
    ("70011", Permission, "Pair user ban trade apikey"),
    ("700001", Auth, "API-key format invalid"),
    ("700002", Auth, "Signature for this request is not valid"),
    ("700003", Auth, "Timestamp for this request is outside of the recvWindow"),
    ("700004", InvalidRequest, "Param 'origClientOrderId' or 'orderId' must be sent, but both were empty/null"),
    ("700005", InvalidRequest, "recvWindow must less than 60000"),
    ("700006", Auth, "IP non white list"),
    ("700007", Permission, "No permission to access the endpoint"),
    ("700008", InvalidRequest, "Illegal characters found in parameter"),
    ("700013", InvalidRequest, "Invalid content Type"),
    ("730001", InvalidRequest, "Pair not found"),
    ("730002", InvalidRequest, "Your input param is invalid"),
];

/// Futures: `(code, canonical, the doc's words)`.
pub(super) const FUT_ERRORS: &[ErrorRow] = &[
    ("401", Auth, "Not logged in or login has expired"),
    ("402", Auth, "API Key expired, please apply again"),
    ("406", Auth, "Accessing IP is not in the whitelist"),
    ("500", Unknown, "Internal error. Please try again."),
    ("501", Unknown, "System busy, try again later"),
    ("506", Permission, "Unknown source of request"),
    ("510", RateLimited, "Requests are too frequent, please try again later"),
    ("511", Permission, "You do not have access to the interface"),
    ("513", Auth, "Invalid request, please try again later"),
    ("600", InvalidRequest, "Parameter error"),
    ("601", InvalidRequest, "Data parsing error"),
    ("602", Auth, "Confirming signature failed"),
    ("603", Unknown, "Repeated request"),
    ("604", Unknown, "Sorry, this feature is under maintenance"),
    ("701", Permission, "Please enable API Key read access"),
    ("702", Permission, "Please enable API Key write access"),
    ("703", Permission, "Trading information read access is required"),
    ("704", Permission, "Please enable API Key trading information write access"),
    ("801", Unknown, "System under maintenance"),
    ("1000", Auth, "Account does not exist"),
    ("1001", InvalidRequest, "Contract does not exist"),
    ("1002", Rejected(MarketClosed), "Contract not activated"),
    ("1003", Rejected(Other), "Risk limit level error, please try again after adjustment"),
    ("1004", InvalidRequest, "Amount error"),
    ("2001", InvalidRequest, "Order direction error"),
    ("2002", InvalidRequest, "Position type error"),
    ("2003", Rejected(Price), "Exceeded the maximum buying price"),
    ("2004", Rejected(Price), "Selling price is lower than the minimum price"),
    ("2005", Rejected(Funds), "Balance insufficient"),
    ("2006", Rejected(Other), "Leverage multiplier must be within the upper limit"),
    ("2007", Rejected(Price), "Order price error"),
    ("2008", Rejected(ReduceOnly), "Insufficient number of positions can be closed"),
    ("2009", Rejected(ReduceOnly), "Position is nonexistent or closed"),
    ("2011", Rejected(VenueRule), "Order quantity error"),
    ("2013", InvalidRequest, "Number of cancelled orders exceeds {count}"),
    ("2014", InvalidRequest, "Orders in batches shall not be greater than {count}"),
    ("2015", Rejected(VenueRule), "Price or quantity precision error, please enter again"),
    ("2016", Rejected(VenueRule), "Trigger order exceeds the max. number of {count}"),
    ("2018", Rejected(Funds), "Cannot exceed the maximum reducible margin {vol}"),
    ("2019", Rejected(Other), "Leverage adjustment unavailable while orders are open"),
    ("2021", Rejected(Other), "Order leverage is inconsistent with the existing position leverage"),
    ("2022", InvalidRequest, "Position type error"),
    ("2023", Rejected(Other), "Risk limit cannot be modified at the moment due to leverage"),
    ("2024", Rejected(Other), "There's order with leverage greater than the maximum leverage"),
    ("2025", Rejected(Other), "Your current position amount is greater than the allowable amount"),
    ("2026", Rejected(Other), "Leverage adjustment is unavailable for cross margin"),
    ("2027", Rejected(Other), "Cross and isolated position of the same direction are alternative."),
    ("2028", Rejected(VenueRule), "Maximum order quantity of {count} exceeded"),
    ("2029", InvalidRequest, "Order type error"),
    ("2030", InvalidRequest, "External order ID too long (max. 32 digit)"),
    ("2031", Rejected(Other), "Current risk limit and maximum allowable number of positions"),
    ("2032", Rejected(Price), "Order price is lower than the liquidation price of the position"),
    ("2033", Rejected(Price), "Order price is higher than the liquidation price of the position"),
    ("2034", InvalidRequest, "Batch query quantity exceeds the limit of {count}"),
    ("2035", Rejected(VenueRule), "Market order level not supported"),
    ("2036", Rejected(Other), "You've reached today's order limit"),
    ("2037", RateLimited, "Trading too frequent, please try again later"),
    ("2038", Rejected(Other), "Position limit reached for your current leverage tier"),
    ("2039", Rejected(Other), "Counterparty's order amount shall not be greater than that of"),
    ("2040", Rejected(NotFound), "Order does not exist"),
    ("2041", Rejected(Final), "Current order status does not allow cancelation"),
    ("2042", Unknown, "Duplicate order ID"),
    ("2043", Rejected(Other), "The order does not match the position"),
    ("2044", Rejected(Funds), "Insufficient margin to adjust leverage"),
    ("2045", Rejected(Other), "Minimum ratio allowed is {number}"),
    ("2046", Rejected(Other), "Maximum ratio allowed is {number}"),
    ("2047", Rejected(Other), "Minimum variance allowed is {number}"),
    ("2048", Rejected(Other), "Maximum variance allowed is {number}"),
    ("2049", Rejected(Price), "Liquidation may occur after the order is filled at the specified price"),
    ("2051", Rejected(VenueRule), "Exceeds the maximum order amount allowed for a single order"),
    ("2052", Rejected(Other), "Partially filled orders do not support price or quantity editing"),
    ("2053", Rejected(Other), "One-way mode does not support limit order editing currently"),
    ("2054", Rejected(Other), "Unable to place a new order as the system has not found any successfully canceled orders"),
    ("2055", Rejected(Other), "System does not support limit order editing currently"),
    ("2056", Rejected(Other), "Market close order is in progress due to market volatility."),
    ("2057", Rejected(VenueRule), "Not meeting the minimum order amount required for a single order"),
    ("2058", Rejected(Other), "Chase Order Failed: The system does not currently support chase orders"),
    ("2059", Rejected(Other), "Chase Order Failed: Your order has been filled or canceled"),
    ("2060", Rejected(Other), "Chase Order Failed: TP price must be higher than the best bid price"),
    ("2061", Rejected(Other), "Chase Order Failed: SL price must be lower than the best bid price"),
    ("2062", Rejected(Other), "Chase Order Failed: TP price must be lower than the best ask price"),
    ("2063", Rejected(Other), "Chase Order Failed: SL price must be higher than the best ask price"),
    ("2064", Rejected(Other), "Unable to place chase orders in one-way position mode"),
    ("2065", Rejected(Funds), "Chase Order Failed: Insufficient assets"),
    ("2066", Rejected(Other), "Chase Order Failed: This action will result in liquidation"),
    ("2068", Rejected(Other), "MEXC has disabled certain SMS services"),
    ("2069", Rejected(Other), "Your current Cross Margin position is showing a loss."),
    ("2070", Rejected(Price), "Price distance cannot exceed 5% of the current bid1/ask1"),
    ("2071", Rejected(Other), "Max. chase distance cannot exceed 10%"),
    ("2072", Rejected(Other), "You can have up to {count} active chase limit orders per account"),
    ("2073", Rejected(Other), "A chase limit order already exists for this Futures pair"),
    ("2074", Rejected(Other), "Chase limit orders cannot be edited"),
    ("2075", Rejected(Other), "This is not a chase limit order"),
    ("2076", Rejected(Other), "This order is a chase limit order"),
    ("2077", Rejected(Price), "Long: Fill price is below the liquidation price. Use a limit order instead."),
    ("2078", Rejected(Price), "Short: Fill price exceeds the liquidation price. Use a limit order instead."),
    ("2079", Rejected(Other), "Loss Coverage does not exist"),
    ("2080", Rejected(Other), "Loss Coverage used"),
    ("2081", Rejected(Other), "Loss Coverage expired"),
    ("2082", Rejected(Other), "Loss Coverage unavailable"),
    ("3001", InvalidRequest, "Trigger order price type error"),
    ("3002", InvalidRequest, "Trigger order type error"),
    ("3003", InvalidRequest, "Execution cycle error"),
    ("3004", Rejected(Price), "Trigger price error"),
    ("3006", Rejected(Price), "Order prices can only be edited for orders that have not been triggered"),
    ("3007", Rejected(Price), "Take-profit price of a trigger-market long order should be higher"),
    ("3008", Rejected(Price), "Take-profit price of a trigger-limit long order should be higher"),
    ("3009", Rejected(Price), "Stop-loss price of a trigger-market long order should be lower"),
    ("3010", Rejected(Price), "Stop-loss price of a trigger-limit long order should be lower"),
    ("3011", Rejected(Price), "Take-profit price of a trigger-market short order should be lower"),
    ("3012", Rejected(Price), "Take-profit price of a trigger-limit short order should be lower"),
    ("3013", Rejected(Price), "Stop-loss price of a short position trigger-market order should be higher"),
    ("3014", Rejected(Price), "Stop-loss price of a short position trigger-limit order should be higher"),
    ("3015", Rejected(Price), "Pre-set TP/SL prices can only be edited for orders that have not been filled"),
    ("3016", Unknown, "{symbol} order timed out"),
    ("3017", Unknown, "{symbol} trigger order timed out"),
    ("3018", Unknown, "{symbol} trailing stop order timed out"),
    ("3019", Unknown, "{symbol} chase limit order timed out"),
    ("4001", Rejected(Other), "Unsupported cryptocurrency"),
    ("5001", Rejected(Other), "Stop profit price and stop loss price cannot both be empty"),
    ("5002", Rejected(NotFound), "Stop-limit order is nonexistent or closed"),
    ("5003", Rejected(Price), "The price of stop-limit order error"),
    ("5004", Rejected(Other), "Total amount of the stop-limit order is greater than the closable amount"),
    ("5005", Rejected(Other), "Order failed as there is already a position TP/SL order."),
    ("5006", Rejected(Other), "You currently have maker orders or positions in the opposite direction"),
    ("6001", Rejected(MarketClosed), "Trading is forbidden"),
    ("6002", Rejected(MarketClosed), "Position opening is forbidden. Please contact Customer Service"),
    ("6003", InvalidRequest, "Incorrect time range, you can select up to 90 days only"),
    ("6004", InvalidRequest, "The trading pair and status should be filled in"),
    ("6005", Rejected(MarketClosed), "The trading pair is not available"),
    ("6006", Rejected(Other), "The current version does not support Cross Margin function"),
    ("6007", Rejected(Other), "Switching from Cross Margin mode to Isolated Margin Mode is not supported"),
    ("6008", Unknown, "Order placed successfully (The estimated liquidation price may not show accurately on the current version"),
    ("6009", Permission, "Position opening is currently unavailable for your account."),
    ("6010", Rejected(Other), "Multi-asset mode isn't available for your account"),
    ("6011", Rejected(Other), "Unable to switch to Multi-Asset mode as there are currently active orders"),
    ("6012", Rejected(Other), "Sub-accounts cannot use Multi-Asset mode"),
    ("6013", Rejected(Other), "Traders in Copy Trade cannot use Multi-Asset mode"),
    ("6014", Rejected(Other), "Your account has liabilities and cannot switch to Multi-Asset mode"),
    ("6015", Rejected(Other), "The account has liabilities and cannot switch to Single-Asset mode"),
    ("6016", Rejected(Other), "Unable to switch to Single-Asset mode as there are currently active orders"),
    ("6017", Rejected(Other), "The current price has expired. Please request the price again."),
    ("6018", Rejected(Other), "The current exchange rate is unfavorable, and this action may"),
    ("6019", Rejected(Other), "Coin-margined trading is not supported in Multi-Asset mode"),
    ("6020", Rejected(Other), "Only cross margin is allowed in Multi-Asset mode"),
    ("6021", Rejected(Other), "There is an issue with the liability crypto. Please try again later."),
    ("6022", Rejected(Other), "Trading error. Please try again later."),
    ("6023", Rejected(Other), "Market maker accounts cannot use Multi-Asset mode"),
    ("6024", Rejected(Other), "Event Futures trading is not available in Multi-Asset mode"),
    ("6025", Rejected(Other), "Position airdrops are not available in Multi-Asset mode"),
    ("6026", Permission, "Position opening is unavailable until risk control verification"),
    ("6027", Rejected(MarketClosed), "Due to risk controls, opening new positions for this pair is temporarily"),
    ("6028", Rejected(Other), "After the platform's risk review, you are no longer allowed to"),
    ("6029", Rejected(Other), "Bonuses cannot be used to open Prediction Futures positions"),
    ("6030", Permission, "Position opening is unavailable until risk control verification"),
    ("7001", Rejected(Other), "Please cancel active orders or close positions before modifying position mode"),
    ("7002", Rejected(Other), "Position mode mismatch, please switch position mode"),
    ("7003", Rejected(Other), "The new order type must be consistent with the existing order"),
    ("7004", Rejected(Other), "The new order leverage must be consistent with the existing order"),
    ("7005", Rejected(ReduceOnly), "The position amount for closing is insufficient"),
    ("7008", Rejected(VenueRule), "Cannot be less than the minimum order amount {value} USDT"),
    ("7009", Rejected(Other), "Your leverage ratio (position value/wallet balance) is too high"),
    ("8012", Rejected(Funds), "Insufficient available margin"),
    ("8815", Rejected(Other), "Current positions exceeded the maximum number of positions"),
    ("8816", Rejected(Other), "Exceeded the maximum number of positions {amount}{currency} that can be held"),
    ("8817", Rejected(Other), "Risk limiting mechanism has been upgraded."),
    ("8818", Rejected(Other), "Current positions exceeded the maximum number of contracts"),
    ("8819", Rejected(Other), "Exceeded the maximum number of contracts {amount} that can be held"),
    ("8820", Rejected(Other), "Due to risk control measures, your account has reached the maximum"),
    ("8821", Rejected(Other), "The current position quantity exceeds the maximum position quantity"),
    ("8823", Rejected(MarketClosed), "This Futures pair will be delisted. New positions cannot be opened"),
    ("8912", Rejected(Other), "Automatically cancelled as position has been closed"),
    ("8913", Rejected(Other), "Manually cancelled by user"),
    ("8915", Rejected(Other), "Leverage mode cannot be changed as there is an active order"),
    ("8917", Rejected(ReduceOnly), "The reduce-only order would lead to insufficient position margin"),
    ("8918", Rejected(Other), "Queries for market makers or particular users are not currently supported"),
    ("8919", Rejected(Other), "Exceeds the maximum value limit for position and open order value"),
    ("8920", Rejected(MarketClosed), "Due to market risk controls, only closing positions is allowed for this pair"),
    ("8921", Rejected(MarketClosed), "Due to market risk controls, only opening positions is allowed for this pair"),
    ("8922", Rejected(MarketClosed), "Trading for this pair is suspended. Wait for trading to resume."),
    ("8930", Rejected(MarketClosed), "Not yet listed, unable to place an order"),
    ("8931", Rejected(Other), "Your leverage exceeds the maximum limit of {max}x in your country"),
    ("8932", Rejected(MarketClosed), "Due to risk controls, opening new positions for this pair is temporarily"),
    ("8934", Rejected(MarketClosed), "Position opening is prohibited"),
    ("8948", Rejected(MarketClosed), "Trading for this pair is about to close for the selected time period"),
    ("8949", Rejected(MarketClosed), "This trading pair is outside trading hours. Placing orders is disabled."),
    ("8950", Rejected(MarketClosed), "For risk management reasons, opening positions for this trading pair"),
    ("9068", Rejected(MarketClosed), "Trading unavailable due to market closure. Only order cancellation"),
    ("9069", Rejected(Other), "This trading pair only supports TP/SL triggered by fair price."),
    ("9070", Rejected(Other), "This pair supports Isolated Margin mode only"),
    ("9071", Rejected(Other), "This pair supports Cross Margin mode only"),
    ("9073", Rejected(MarketClosed), "Stock Futures trading is suspended during market closure."),
    ("9074", Rejected(Other), "Guaranteed Stop-Loss is currently unavailable"),
    ("9075", Rejected(Other), "Stop-Loss trigger price can only be set to the last price"),
    ("9998", Rejected(MarketClosed), "Only position closing is supported, as the platform does not support"),
    ("9999", Unknown, "Network error. Please try again."),
    ("11444", Unknown, "This feature is temporarily unavailable."),
    ("200005", Permission, "To further enhance the security of user accounts and assets"),
    ("200006", Permission, "Please complete KYC verification to access platform services"),
    ("300000", Permission, "Please complete KYC verification to access platform services"),
    ("300001", Permission, "To further enhance the security of user accounts and assets"),
    ("300002", Permission, "Due to the relevant regulations, this service is not supported in your region"),
];

/// The fix a permission refusal names.
fn fix(spot: bool, code: &str) -> &'static str {
    match code {
        "70011" | "30020" => " Add the pair to the API key's trading pairs at MEXC (My API Key, Trading Pairs), or use a key that trades it.",
        "300000" | "300001" | "200005" | "200006" => " Complete KYC at MEXC: futures order placing needs it.",
        _ if spot => " The key needs the spot trading permission (SPOT_DEAL_WRITE) to send orders; a read-only key still reads.",
        _ => " The key needs the futures Order Placing permission to send orders; a read-only key still reads.",
    }
}

/// A code mapped in the book's table, in MEXC's words.
pub(super) fn mapped(spot: bool, code: &str, msg: &str) -> VenueError {
    let table = if spot { SPOT_ERRORS } else { FUT_ERRORS };
    let msg = msg.trim();
    match table.iter().find(|(c, _, _)| *c == code) {
        Some((_, Unknown, _)) => VenueError::new(Unknown, Some(code.to_string()), format!("MEXC did not say whether it went through: {msg} (code {code})")),
        Some((_, Permission, _)) => VenueError::new(Permission, Some(code.to_string()), format!("MEXC refused it: {msg} (code {code}).{}", fix(spot, code))),
        Some((_, canon, _)) => VenueError::new(*canon, Some(code.to_string()), format!("MEXC refused it: {msg} (code {code})")),
        None => VenueError::unmapped(Some(code.to_string()), format!("MEXC answered code {code} ({msg}), which OpenTraderWorld does not map")),
    }
}

fn code_text(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    code: Option<Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    data: Value,
}

/// One row of a futures cancel by ids.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CancelItem {
    #[serde(default)]
    pub error_code: Option<Value>,
    #[serde(default)]
    pub error_msg: String,
}

impl CancelItem {
    /// The row's refusal, `None` for `errorCode` 0.
    pub fn error(&self) -> Option<VenueError> {
        let code = self.error_code.as_ref().and_then(code_text)?;
        (code != "0").then(|| mapped(false, &code, &self.error_msg))
    }
}

/// The status alone, where no body names a code: a timeout or a 5xx is lost, a 429 or 418 a wait.
fn by_status(frame: &RawFrame) -> Option<VenueError> {
    let e = http::classify(frame, "mexc")?;
    let canon = match e.kind {
        ErrorKind::RateLimited => RateLimited,
        ErrorKind::Auth => Auth,
        ErrorKind::Permission => Permission,
        ErrorKind::InvalidRequest | ErrorKind::NotFound => InvalidRequest,
        // A 5xx: "the execution status is UNKNOWN and could have been a success".
        _ => Unknown,
    };
    let mut v = VenueError::new(canon, e.provider_code.clone(), e.message);
    v.retry_after_ms = e.retry_after_ms;
    Some(v)
}

/// The error a frame carries, mapped; `None` for a success.
pub(super) fn decode(spot: bool, frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, status, endpoint_id, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        let why = http::timeout(frame, "mexc").map(|e| e.message).unwrap_or_else(|| "mexc: no answer".into());
        return Some(VenueError::new(Unknown, None, why));
    };
    let ok = (200..300).contains(status);
    if *status >= 500 {
        return by_status(frame);
    }
    // A list (spot rows, a cancel-all answer) is never an error.
    if ok && !body.trim_ascii_start().starts_with(b"{") {
        return None;
    }
    let Ok(env) = serde_json::from_slice::<Envelope>(body) else {
        return if ok { None } else { by_status(frame) };
    };
    let code = env.code.as_ref().and_then(code_text);
    let msg = env.msg.or(env.message).unwrap_or_default();
    let mut e = if spot {
        match code {
            // A spot success carries no `code`.
            _ if ok && env.success != Some(false) => return None,
            Some(c) => mapped(true, &c, &msg),
            None => return by_status(frame),
        }
    } else {
        match (env.success, code) {
            (Some(false), Some(c)) if c == PLACED => return None,
            (Some(false), Some(c)) => mapped(false, &c, &msg),
            (Some(false), None) => VenueError::unmapped(None, format!("MEXC refused it with no code ({msg}), which OpenTraderWorld does not map")),
            // A single cancel answers a row per id: the row's refusal is the call's.
            _ if ok && *endpoint_id == super::trade::FUT_CANCEL => {
                let rows: Vec<CancelItem> = serde_json::from_value(env.data).unwrap_or_default();
                return rows.iter().find_map(CancelItem::error);
            }
            _ if ok => return None,
            (_, Some(c)) => mapped(false, &c, &msg),
            (_, None) => return by_status(frame),
        }
    };
    // One list of working conditional orders the venue refuses (a parameter it reads its own
    // way, a key without that read): that list is empty, the account's other orders are still
    // read. A rate limit, a key refusal or a lost answer still fails the read.
    let cond_list = [super::trade::PLAN_OPEN, super::trade::STOP_OPEN, super::trade::TRACK_OPEN].contains(endpoint_id);
    if !spot && cond_list && matches!(e.canon, InvalidRequest | Permission | Rejected(_)) {
        tracing::warn!("mexc: the {endpoint_id} list was refused ({}), read as empty", e.message);
        return None;
    }
    if e.canon == RateLimited {
        // 429 and 418 carry `Retry-After` in seconds.
        e.retry_after_ms = http::retry_after_ms(frame);
    }
    Some(e)
}
