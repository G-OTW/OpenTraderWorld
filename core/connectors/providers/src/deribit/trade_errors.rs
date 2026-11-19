//! Deribit's order error table (onboarding spec 8), from the "Complete RPC Error Codes
//! Reference" of https://docs.deribit.com/articles/errors (snapshot read 2026-11-12): every code
//! of that table, mapped to one canonical kind, plus `10050 lock_time_exceeded`, which only the
//! Order Management article names. The "Common Error Codes" table at the top of the same page
//! contradicts the complete one (10001, 10002, 10004, 11000 to 11009, 13009 with other words):
//! only the complete table is used, so its 11000 to 11009 stay unmapped (unknown, and they alert).
//! Mirrored in PROVIDER.md ("Trading", "Errors"); a test keeps the two in step.
//!
//! A JSON-RPC refusal is `{"jsonrpc":"2.0","error":{"code","message","data":{"reason",
//! "param"}}}`, answered with HTTP 400 (measured on the read path). The code decides.
//!
//! The read path ([`super::errors`]) maps 10001, 10040, 11094, 13028 and 13888 as transient
//! failures, a read being safe to repeat. On the order path they are answers that do not say
//! whether the order went through: [`Canon::Unknown`], looked up, never sent again.

use otw_connector_model::ErrorKind;
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, PostOnly, Price, VenueRule};

/// `(code, canonical, the doc's short message)`, in the doc's order.
pub(super) const ERRORS: &[ErrorRow] = &[
    ("10000", Auth, "authorization_required"),
    ("10001", Unknown, "error"),
    ("10002", Rejected(VenueRule), "qty_too_low"),
    ("10003", Rejected(Other), "order_overlap"),
    ("10004", Rejected(NotFound), "order_not_found"),
    ("10005", Rejected(Price), "price_too_low <Limit>"),
    ("10006", Rejected(Price), "price_too_low4idx <Limit>"),
    ("10007", Rejected(Price), "price_too_high <Limit>"),
    ("10009", Rejected(Funds), "not_enough_funds"),
    ("10010", Rejected(Final), "already_closed"),
    ("10011", Rejected(Price), "price_not_allowed"),
    ("10012", Rejected(MarketClosed), "book_closed"),
    ("10013", Rejected(Other), "pme_max_total_open_orders <Limit>"),
    ("10014", Rejected(Other), "pme_max_future_open_orders <Limit>"),
    ("10015", Rejected(Other), "pme_max_option_open_orders <Limit>"),
    ("10016", Rejected(Other), "pme_max_future_open_orders_size <Limit>"),
    ("10017", Rejected(Other), "pme_max_option_open_orders_size <Limit>"),
    ("10018", Rejected(Other), "non_pme_max_future_position_size <Limit>"),
    ("10019", Rejected(MarketClosed), "locked_by_admin"),
    ("10020", Rejected(NotFound), "invalid_or_unsupported_instrument"),
    ("10021", Rejected(VenueRule), "invalid_amount"),
    ("10022", InvalidRequest, "invalid_quantity"),
    ("10023", InvalidRequest, "invalid_price"),
    ("10024", InvalidRequest, "invalid_max_show"),
    ("10025", InvalidRequest, "invalid_order_id"),
    ("10026", Rejected(VenueRule), "price_precision_exceeded"),
    ("10027", Rejected(VenueRule), "non_integer_contract_amount"),
    ("10028", RateLimited, "too_many_requests"),
    ("10029", Rejected(NotFound), "not_owner_of_order"),
    ("10030", InvalidRequest, "must_be_websocket_request"),
    ("10031", InvalidRequest, "invalid_args_for_instrument"),
    ("10032", Rejected(VenueRule), "whole_cost_too_low"),
    ("10033", InvalidRequest, "not_implemented"),
    ("10034", Rejected(Price), "trigger_price_too_high"),
    ("10035", Rejected(Price), "trigger_price_too_low"),
    ("10036", InvalidRequest, "invalid_max_show_amount"),
    ("10037", Rejected(Other), "non_pme_total_short_options_positions_size <Limit>"),
    ("10038", Rejected(Other), "pme_max_risk_reducing_orders <Limit>"),
    ("10039", Rejected(Funds), "not_enough_funds_in_currency <Currency>"),
    ("10040", Unknown, "retry"),
    ("10041", Rejected(MarketClosed), "settlement_in_progress"),
    ("10043", Rejected(VenueRule), "price_wrong_tick"),
    ("10044", Rejected(VenueRule), "trigger_price_wrong_tick"),
    ("10045", Rejected(Other), "can_not_cancel_liquidation_order"),
    ("10046", Rejected(Other), "can_not_edit_liquidation_order"),
    ("10047", RateLimited, "matching_engine_queue_full"),
    ("10048", InvalidRequest, "not_on_this_server"),
    ("10049", InvalidRequest, "cancel_on_disconnect_failed"),
    // Order Management article only: "If a request cannot complete the lock procedure within
    // 5 seconds, it times out".
    ("10050", Unknown, "lock_time_exceeded"),
    ("10066", RateLimited, "too_many_concurrent_requests"),
    ("10072", Rejected(Other), "disabled_while_position_lock"),
    ("11008", Rejected(Final), "already_filled"),
    ("11013", Rejected(Other), "max_spot_open_orders"),
    ("11021", Rejected(PostOnly), "post_only_price_modification_not_possible"),
    ("11022", Rejected(Other), "max_spot_order_quantity"),
    ("11029", InvalidRequest, "invalid_arguments"),
    ("11030", Rejected(Other), "other_reject <Reason>"),
    ("11031", Unknown, "other_error <Error>"),
    ("11035", Rejected(Other), "no_more_triggers <Limit>"),
    ("11036", Rejected(Price), "invalid_trigger_price"),
    ("11037", Rejected(MarketClosed), "outdated_instrument_for_IV_order"),
    ("11038", InvalidRequest, "no_adv_for_futures"),
    ("11039", InvalidRequest, "no_adv_postonly"),
    ("11041", InvalidRequest, "not_adv_order"),
    ("11042", Permission, "permission_denied"),
    ("11043", InvalidRequest, "bad_argument"),
    ("11044", Rejected(Final), "not_open_order"),
    ("11045", InvalidRequest, "invalid_event"),
    ("11046", Rejected(MarketClosed), "outdated_instrument"),
    ("11047", InvalidRequest, "unsupported_arg_combination"),
    ("11048", InvalidRequest, "wrong_max_show_for_option"),
    ("11049", InvalidRequest, "bad_arguments"),
    ("11050", InvalidRequest, "bad_request"),
    ("11051", Rejected(MarketClosed), "system_maintenance"),
    ("11052", InvalidRequest, "subscribe_error_unsubscribed"),
    ("11053", InvalidRequest, "transfer_not_found"),
    ("11054", Rejected(PostOnly), "post_only_reject"),
    ("11055", InvalidRequest, "post_only_not_allowed"),
    ("11056", Rejected(Other), "unauthenticated_public_requests_temporarily_disabled"),
    ("11059", InvalidRequest, "iceberg_not_allowed"),
    ("11060", InvalidRequest, "not_supported_for_coinbase_routed_spot"),
    ("11090", InvalidRequest, "invalid_addr"),
    ("11091", InvalidRequest, "invalid_transfer_address"),
    ("11092", InvalidRequest, "address_already_exist"),
    ("11093", InvalidRequest, "max_addr_count_exceeded"),
    ("11094", Unknown, "internal_server_error"),
    ("11095", InvalidRequest, "disabled_deposit_address_creation"),
    ("11096", InvalidRequest, "address_belongs_to_user"),
    ("11097", InvalidRequest, "no_deposit_address"),
    ("11098", Permission, "account_locked"),
    ("12001", InvalidRequest, "too_many_subaccounts"),
    ("12002", InvalidRequest, "wrong_subaccount_name"),
    ("12003", Auth, "login_over_limit"),
    ("12004", InvalidRequest, "registration_over_limit"),
    ("12005", Permission, "country_is_banned"),
    ("12100", InvalidRequest, "transfer_not_allowed"),
    ("12998", Auth, "security_key_authorization_over_limit"),
    ("13004", Auth, "invalid_credentials"),
    ("13005", Auth, "pwd_match_error"),
    ("13006", Auth, "security_error"),
    ("13007", Auth, "user_not_found"),
    // "invalid input or internal failure": it may have been the venue's own failure.
    ("13008", Unknown, "request_failed"),
    ("13009", Auth, "unauthorized"),
    ("13010", InvalidRequest, "value_required"),
    ("13011", InvalidRequest, "value_too_short"),
    ("13012", Permission, "unavailable_in_subaccount"),
    ("13013", InvalidRequest, "invalid_phone_number"),
    ("13014", InvalidRequest, "cannot_send_sms"),
    ("13015", InvalidRequest, "invalid_sms_code"),
    ("13016", InvalidRequest, "invalid_input"),
    ("13018", InvalidRequest, "invalid_content_type"),
    ("13019", Rejected(MarketClosed), "orderbook_closed"),
    ("13020", Rejected(NotFound), "not_found"),
    ("13021", Permission, "forbidden"),
    ("13025", Rejected(Other), "method_switched_off_by_admin"),
    ("13028", Unknown, "temporarily_unavailable"),
    ("13030", Rejected(Other), "mmp_trigger"),
    ("13031", Permission, "verification_required"),
    ("13032", InvalidRequest, "non_unique_order_label"),
    ("13034", InvalidRequest, "no_more_security_keys_allowed"),
    ("13035", Rejected(Other), "active_combo_limit_reached"),
    ("13036", Rejected(Other), "unavailable_for_combo_books"),
    ("13037", Permission, "incomplete_KYC_data"),
    ("13040", Permission, "mmp_required"),
    ("13042", InvalidRequest, "cod_not_enabled"),
    ("13043", Rejected(Other), "quotes_frozen"),
    ("13403", Permission, "scope_exceeded"),
    ("13503", Rejected(Other), "unavailable"),
    ("13666", Rejected(Other), "request_cancelled_by_user"),
    // An edit superseded by another: which one stands is read from the order.
    ("13777", Unknown, "replaced"),
    ("13778", Auth, "raw_subscriptions_not_available_for_unauthorized"),
    ("13780", RateLimited, "move_positions_over_limit"),
    ("13781", InvalidRequest, "coupon_already_used"),
    ("13791", InvalidRequest, "KYC_transfer_already_initiated"),
    ("13792", Permission, "incomplete_KYC_data"),
    ("13793", Rejected(Other), "KYC_data_inaccessible"),
    ("13888", Unknown, "timed_out"),
    ("13901", Rejected(Other), "no_more_oto_orders"),
    ("13902", Permission, "mass_quotes_disabled"),
    ("13903", RateLimited, "too_many_quotes"),
    ("13904", Permission, "security_key_setup_required"),
    ("13905", Rejected(Other), "too_many_quotes_per_block_rfq"),
    ("13906", Rejected(Other), "too_many_quotes_per_block_rfq_side"),
    // The doc lists 13907 twice (`not_fully_filled`, `too_many_open_block_rfqs`): block RFQ
    // only, a refusal either way.
    ("13907", Rejected(Other), "not_fully_filled / too_many_open_block_rfqs"),
    ("13910", Rejected(Other), "quote_crossed"),
    ("13911", Rejected(Other), "max_broker_client_count"),
    ("13912", InvalidRequest, "broker_cannot_be_client"),
    ("13913", InvalidRequest, "broker_already_linked"),
    ("13914", InvalidRequest, "user_is_a_broker_client"),
    ("13915", InvalidRequest, "user_is_not_a_broker"),
    ("13916", InvalidRequest, "app_registered_to_broker"),
    ("13917", Rejected(Other), "account_quote_limit_crossed"),
    ("13918", Rejected(Other), "inverse_future_cross_trading"),
    ("13919", InvalidRequest, "client_of_main_account"),
    ("13922", InvalidRequest, "linked_order_type_not_supported_for_csr_spot"),
    ("-32602", InvalidRequest, "Invalid params"),
    ("-32600", InvalidRequest, "request entity too large"),
    ("-32601", InvalidRequest, "Method not found"),
    ("-32700", InvalidRequest, "Parse error"),
    ("-32000", InvalidRequest, "Missing params"),
];

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

fn row(code: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|(c, _, _)| *c == code)
}

/// The error a code means, in Deribit's words (`message`, then the `data` it names).
pub(super) fn of(code: i64, msg: &str, param: Option<&str>) -> VenueError {
    let shown = code.to_string();
    // "Invalid params" on the instrument: the venue lists no such instrument (measured on the
    // read path: `instrument not found`, `wrong format`).
    if code == -32602 && param == Some("instrument_name") {
        return VenueError::new(Rejected(NotFound), Some(shown.clone()), format!("Deribit refused it: {msg} (code {shown})"));
    }
    // "Invalid params" on the currency of a lookup by label or of the trigger history: nothing
    // is filed under that currency, so the order is not there; the lookup goes on with its
    // other calls.
    if code == -32602 && param == Some("currency") {
        return VenueError::new(Rejected(NotFound), Some(shown.clone()), format!("Deribit lists nothing under that currency: {msg} (code {shown})"));
    }
    match row(&shown) {
        Some((_, Unknown, _)) => VenueError::new(Unknown, Some(shown.clone()), format!("Deribit did not say whether it went through: {msg} (code {shown})")),
        Some((_, canon, _)) => VenueError::new(*canon, Some(shown.clone()), format!("Deribit refused it: {msg} (code {shown})")),
        None => VenueError::unmapped(Some(shown.clone()), format!("Deribit answered code {shown} ({msg}), which OpenTraderWorld does not map")),
    }
}

/// The error an answer carries, mapped; `None` for a success. A timeout, a closed socket, a 5xx
/// without a refusal and an unreadable refusal are [`Canon::Unknown`].
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, status, .. } = frame else {
        let why = http::timeout(frame, "deribit").map(|e| e.message).unwrap_or_else(|| "deribit: no answer".into());
        return Some(VenueError::new(Unknown, None, why));
    };
    match serde_json::from_slice::<Answer>(body) {
        Ok(Answer { error: Some(err) }) => {
            let data = err.data.as_ref();
            let param = data.and_then(|d| d.get("param")).and_then(|p| p.as_str());
            let reason = data.and_then(|d| d.get("reason").or_else(|| d.get("invalid"))).and_then(|r| r.as_str());
            let base = Some(err.message.trim()).filter(|m| !m.is_empty()).unwrap_or("refused");
            let msg = match (reason, param) {
                (Some(r), Some(p)) => format!("{base} ({p}: {r})"),
                (Some(r), None) => format!("{base} ({r})"),
                (None, Some(p)) => format!("{base} ({p})"),
                (None, None) => base.to_string(),
            };
            let mut e = of(err.code, &msg, param);
            // The doc states no wait: a `Retry-After` is read where the answer carries one.
            if e.canon == RateLimited {
                e.retry_after_ms = http::retry_after_ms(frame);
            }
            Some(e)
        }
        // A body with no `error` is an answer, whatever the status says.
        Ok(Answer { error: None }) if (200..300).contains(status) => None,
        _ => {
            // No JSON-RPC refusal: the HTTP status alone. A 2xx that is not JSON is left to the
            // decoder, which then fails to read it (an answer nobody mapped).
            let e = http::classify(frame, "deribit")?;
            let canon = match e.kind {
                ErrorKind::RateLimited => RateLimited,
                ErrorKind::Auth => Auth,
                ErrorKind::Permission => Permission,
                ErrorKind::InvalidRequest | ErrorKind::NotFound if *status < 500 => {
                    return Some(VenueError::unmapped(e.provider_code.clone(), format!("Deribit answered HTTP {status} without a JSON-RPC error, which OpenTraderWorld does not map ({})", e.message)));
                }
                // A 5xx, a gateway: the venue may have acted.
                _ => Unknown,
            };
            let mut v = VenueError::new(canon, e.provider_code.clone(), e.message);
            v.retry_after_ms = e.retry_after_ms;
            Some(v)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    fn word(c: &Canon) -> &'static str {
        match c {
            Rejected(VenueRule) => "Rejected(venue_rule)",
            Rejected(Funds) => "Rejected(funds)",
            Rejected(Price) => "Rejected(price)",
            Rejected(MarketClosed) => "Rejected(market closed)",
            Rejected(NotFound) => "Rejected(not found)",
            Rejected(Final) => "Rejected(final)",
            Rejected(Refusal::ReduceOnly) => "Rejected(reduce only)",
            Rejected(PostOnly) => "Rejected(post only)",
            Rejected(Other) => "Rejected(other)",
            Unknown => "Unknown",
            RateLimited => "RateLimited",
            Auth => "Auth",
            Permission => "Permission",
            InvalidRequest => "InvalidRequest",
        }
    }

    /// PROVIDER.md's order error table lists the same codes with the same kinds.
    #[test]
    fn the_table_matches_provider_md() {
        let doc = include_str!("PROVIDER.md");
        let start = doc.find("### Errors").unwrap();
        let table = &doc[start..start + doc[start..].find("### Rate limits for orders").unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in table.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            for c in cells[1].split('`').skip(1).step_by(2) {
                seen.push((c.to_string(), cells[3].to_string()));
            }
        }
        let mut want: Vec<(String, String)> = ERRORS.iter().map(|(c, k, _)| (c.to_string(), word(k).to_string())).collect();
        seen.sort();
        want.sort();
        assert_eq!(seen, want);
    }

    #[test]
    fn no_code_is_listed_twice() {
        let mut codes: Vec<&str> = ERRORS.iter().map(|r| r.0).collect();
        codes.sort_unstable();
        let n = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), n);
    }

    #[test]
    fn a_lost_answer_is_unknown_and_never_a_refusal() {
        for code in ["10001", "10040", "10050", "11031", "11094", "13028", "13888"] {
            let body = format!(r#"{{"jsonrpc":"2.0","error":{{"code":{code},"message":"x"}}}}"#);
            let e = decode(&http_frame(1, "buy", 400, &body)).unwrap();
            assert!(e.canon == Unknown && e.mapped, "{code}: {e:?}");
            assert!(e.message.contains("did not say whether it went through"), "{}", e.message);
        }
        assert_eq!(decode(&http_frame(1, "buy", 502, "<html>bad gateway</html>")).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "buy" };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
    }

    #[test]
    fn a_refusal_names_the_broker_code_and_text() {
        let e = decode(&http_frame(1, "buy", 400, r#"{"jsonrpc":"2.0","id":1,"error":{"code":10009,"message":"not_enough_funds"},"testnet":true}"#)).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Rejected(Funds), Some("10009")));
        assert_eq!(e.message, "Deribit refused it: not_enough_funds (code 10009)");
        // Recorded on the read path, 2026-10-19: a stale timestamp is a key refusal.
        let e = decode(&http_frame(1, "buy", 400, r#"{"jsonrpc":"2.0","error":{"code":13004,"data":{"invalid":"timestamp"},"message":"invalid_credentials"}}"#)).unwrap();
        assert!(e.canon == Auth && e.message.contains("timestamp"), "{e:?}");
        let e = decode(&http_frame(1, "openOrdersByInstrument", 400, r#"{"jsonrpc":"2.0","error":{"message":"Invalid params","data":{"reason":"instrument not found","param":"instrument_name"},"code":-32602}}"#)).unwrap();
        assert_eq!(e.canon, Rejected(NotFound));
        let e = decode(&http_frame(1, "buy", 400, r#"{"jsonrpc":"2.0","error":{"message":"Invalid params","data":{"reason":"must be a number","param":"amount"},"code":-32602}}"#)).unwrap();
        assert!(e.canon == InvalidRequest && e.message.contains("amount: must be a number"), "{e:?}");
        // The "Common Error Codes" table's 11000 to 11009 are not in the complete one: unmapped.
        let e = decode(&http_frame(1, "buy", 400, r#"{"jsonrpc":"2.0","error":{"code":11004,"message":"order_not_found"}}"#)).unwrap();
        assert!(!e.mapped && e.canon == Unknown && e.message == "Deribit answered code 11004 (order_not_found), which OpenTraderWorld does not map", "{e:?}");
        // Success.
        assert!(decode(&http_frame(1, "buy", 200, r#"{"jsonrpc":"2.0","id":1,"result":{"order":{},"trades":[]}}"#)).is_none());
    }

    #[test]
    fn a_rate_limit_carries_the_wait_only_when_the_answer_states_one() {
        let e = decode(&http_frame(1, "buy", 400, r#"{"jsonrpc":"2.0","error":{"code":10028,"message":"too_many_requests"}}"#)).unwrap();
        assert_eq!((e.canon, e.retry_after_ms), (RateLimited, None));
        assert_eq!(decode(&http_frame(1, "buy", 429, "Too Many Requests")).unwrap().canon, RateLimited);
    }
}
