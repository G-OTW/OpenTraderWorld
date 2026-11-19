//! Coinbase Advanced Trade's order error table (onboarding spec 8), from the OpenAPI document
//! every REST reference page embeds
//! (https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/advanced-trade-spec.yaml,
//! read 2026-11-12, sha256 `e1edecbc619a5058...`): every value of its four failure enums
//! (`NewOrderFailureReason`, `PreviewFailureReason`, `CancelOrderFailureReason`,
//! `EditOrderFailureReason`; no value is shared between them) mapped to one canonical kind. A
//! reason not listed here is `Unknown` and alerts. Mirrored in PROVIDER.md ("Trading",
//! "Errors"); a test keeps the two in step.
//!
//! Where a refusal travels: a place, a preview, a cancel and an edit answer HTTP 200 with the
//! refusal in the body (`success: false` and `error_response`, `errs[]`, a `results[]` row with
//! `success: false`, `errors[]`); anything else is the gRPC gateway's body
//! `{error, code, message, details}` under an HTTP status, which alone decides. `success` decides,
//! never the reason field ("`success: true` along with `UNKNOWN_FAILURE_REASON` ... means that
//! your order was successfully accepted", FAQ).

use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;
use serde_json::Value;

use Canon::{InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, PostOnly, Price, ReduceOnly, VenueRule};

/// The endpoint ids of the order calls (`trade.rs`), by which an answer is read.
pub(super) const CREATE: &str = "order_create";
pub(super) const PREVIEW: &str = "order_preview";
pub(super) const CANCEL: &str = "order_cancel";
/// A batch cancel of the kill switch: its rows are read one by one
/// ([`otw_connector_sdk::trade::TradeMapper::decode_cancel_rows`]), never as one refusal.
pub(super) const CANCEL_BATCH: &str = "order_cancel_batch";
pub(super) const EDIT: &str = "order_edit";
pub(super) const GET_ORDER: &str = "order_get";

/// `(reason, canonical, the enum it comes from and its words)`.
pub(super) const ERRORS: &[ErrorRow] = &[
    ("UNKNOWN_FAILURE_REASON", Unknown, "new order: unknown failure reason"),
    ("UNSUPPORTED_ORDER_CONFIGURATION", InvalidRequest, "new order: unsupported order configuration"),
    ("INVALID_SIDE", InvalidRequest, "new order: invalid side"),
    ("INVALID_PRODUCT_ID", InvalidRequest, "new order: invalid product id"),
    ("INVALID_SIZE_PRECISION", Rejected(VenueRule), "new order: invalid size precision"),
    ("INVALID_PRICE_PRECISION", Rejected(VenueRule), "new order: invalid price precision"),
    ("INSUFFICIENT_FUND", Rejected(Funds), "new order: insufficient fund"),
    ("INVALID_LEDGER_BALANCE", Rejected(Funds), "new order: invalid ledger balance"),
    ("ORDER_ENTRY_DISABLED", Rejected(MarketClosed), "new order: order entry disabled"),
    ("INELIGIBLE_PAIR", Permission, "new order: ineligible pair"),
    ("INVALID_LIMIT_PRICE_POST_ONLY", Rejected(PostOnly), "new order: invalid limit price post only"),
    ("INVALID_LIMIT_PRICE", Rejected(Price), "new order: invalid limit price"),
    ("INVALID_NO_LIQUIDITY", Rejected(Other), "new order: invalid no liquidity"),
    ("INVALID_REQUEST", InvalidRequest, "new order: invalid request"),
    ("COMMANDER_REJECTED_NEW_ORDER", Unknown, "new order: commander rejected new order"),
    ("INSUFFICIENT_FUNDS", Rejected(Funds), "new order: insufficient funds"),
    ("IN_LIQUIDATION", Rejected(Funds), "new order: in liquidation"),
    ("INVALID_MARGIN_TYPE", InvalidRequest, "new order: invalid margin type"),
    ("INVALID_LEVERAGE", InvalidRequest, "new order: invalid leverage"),
    ("UNTRADABLE_PRODUCT", Rejected(MarketClosed), "new order: untradable product"),
    ("INVALID_FCM_TRADING_SESSION", Rejected(MarketClosed), "new order: invalid fcm trading session"),
    ("GEOFENCING_RESTRICTION", Permission, "new order: geofencing restriction"),
    ("QUOTE_SIZE_NOT_ALLOWED_FOR_BRACKET", InvalidRequest, "new order: quote size not allowed for bracket"),
    ("INVALID_BRACKET_PRICES", Rejected(Price), "new order: invalid bracket prices"),
    ("MISSING_MARKET_TRADE_DATA", Rejected(Other), "new order: missing market trade data"),
    ("INVALID_BRACKET_LIMIT_PRICE", Rejected(Price), "new order: invalid bracket limit price"),
    ("INVALID_BRACKET_STOP_TRIGGER_PRICE", Rejected(Price), "new order: invalid bracket stop trigger price"),
    ("BRACKET_LIMIT_PRICE_OUT_OF_BOUNDS", Rejected(Price), "new order: bracket limit price out of bounds"),
    ("STOP_TRIGGER_PRICE_OUT_OF_BOUNDS", Rejected(Price), "new order: stop trigger price out of bounds"),
    ("BRACKET_ORDER_NOT_SUPPORTED", InvalidRequest, "new order: bracket order not supported"),
    ("FOK_DISABLED", InvalidRequest, "new order: fok disabled"),
    ("FOK_ONLY_ALLOWED_ON_LIMIT_ORDERS", InvalidRequest, "new order: fok only allowed on limit orders"),
    ("POST_ONLY_NOT_ALLOWED_WITH_FOK", InvalidRequest, "new order: post only not allowed with fok"),
    ("UBO_HIGH_LEVERAGE_QUANTITY_BREACHED", Rejected(Funds), "new order: ubo high leverage quantity breached"),
    ("END_TIME_TOO_FAR_IN_FUTURE", InvalidRequest, "new order: end time too far in future"),
    ("LIMIT_PRICE_TOO_FAR_FROM_MARKET", Rejected(Price), "new order: limit price too far from market"),
    ("OPEN_BRACKET_ORDERS", Rejected(Other), "new order: open bracket orders"),
    ("FUTURES_AFTER_HOUR_INVALID_ORDER_TYPE", Rejected(MarketClosed), "new order: futures after hour invalid order type"),
    ("FUTURES_AFTER_HOUR_INVALID_TIME_IN_FORCE", Rejected(MarketClosed), "new order: futures after hour invalid time in force"),
    ("INVALID_ATTACHED_TAKE_PROFIT_PRICE", Rejected(Price), "new order: invalid attached take profit price"),
    ("INVALID_ATTACHED_STOP_LOSS_PRICE", Rejected(Price), "new order: invalid attached stop loss price"),
    ("INVALID_ATTACHED_TAKE_PROFIT_PRICE_PRECISION", Rejected(VenueRule), "new order: invalid attached take profit price precision"),
    ("INVALID_ATTACHED_STOP_LOSS_PRICE_PRECISION", Rejected(VenueRule), "new order: invalid attached stop loss price precision"),
    ("INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS", Rejected(Price), "new order: invalid attached take profit price out of bounds"),
    ("INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS", Rejected(Price), "new order: invalid attached stop loss price out of bounds"),
    ("INVALID_ATTACHED_TAKE_PROFIT_PRICE_EXCEEDS_MAX_DISTANCE_FROM_ORIGINATING_PRICE", Rejected(Price), "new order: invalid attached take profit price exceeds max distance from originating price"),
    ("INVALID_ATTACHED_TAKE_PROFIT_SIZE_BELOW_MIN", Rejected(VenueRule), "new order: invalid attached take profit size below min"),
    ("ATTACHED_ORDER_SIZE_MUST_BE_NIL", InvalidRequest, "new order: attached order size must be nil"),
    ("INVALID_SETTLEMENT_CURRENCY", InvalidRequest, "new order: invalid settlement currency"),
    ("DURATION_TOO_SMALL", InvalidRequest, "new order: duration too small"),
    ("INTX_FOK_ONLY_ALLOWED_ON_LIMIT_AND_MARKET_ORDERS", InvalidRequest, "new order: intx fok only allowed on limit and market orders"),
    ("BUCKET_SIZE_SMALLER_THAN_QUOTE_MIN", Rejected(VenueRule), "new order: bucket size smaller than quote min"),
    ("BUCKET_SIZE_SMALLER_THAN_BASE_MIN", Rejected(VenueRule), "new order: bucket size smaller than base min"),
    ("END_TIME_AFTER_CONTRACT_EXPIRATION", InvalidRequest, "new order: end time after contract expiration"),
    ("START_TIME_MUST_BE_SPECIFIED", InvalidRequest, "new order: start time must be specified"),
    ("ICEBERG_ORDERS_NOT_SUPPORTED", InvalidRequest, "new order: iceberg orders not supported"),
    ("END_TIME_IS_IN_THE_PAST", InvalidRequest, "new order: end time is in the past"),
    ("GTD_ORDERS_MUST_HAVE_END_TIME", InvalidRequest, "new order: gtd orders must have end time"),
    ("ATTACHED_ORDER_MUST_HAVE_POSITIVE_PRICES", Rejected(Price), "new order: attached order must have positive prices"),
    ("INVALID_ORDER_SIDE_FOR_ATTACHED_TPSL", InvalidRequest, "new order: invalid order side for attached tpsl"),
    ("ATTACHED_ORDERS_ONLY_ALLOWED_ON_MARKET_LIMIT", InvalidRequest, "new order: attached orders only allowed on market limit"),
    ("INVALID_ORDER_TYPE_FOR_ATTACHED", InvalidRequest, "new order: invalid order type for attached"),
    ("PRICE_NOT_ALLOWED_FOR_MARKET_ORDERS", InvalidRequest, "new order: price not allowed for market orders"),
    ("REDUCE_ONLY_NOT_ALLOWED_ON_VENUE", InvalidRequest, "new order: reduce only not allowed on venue"),
    ("DURATION_TOO_LARGE", InvalidRequest, "new order: duration too large"),
    ("REDUCE_ONLY_NOT_ALLOWED_ON_SPOT_PRODUCTS", InvalidRequest, "new order: reduce only not allowed on spot products"),
    ("LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_BUY", Rejected(Price), "new order: limit order price exceeds price band on buy"),
    ("LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_SELL", Rejected(Price), "new order: limit order price exceeds price band on sell"),
    ("INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER", Rejected(Price), "new order: invalid attached take profit price out of bounds on aggressive order"),
    ("INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER", Rejected(Price), "new order: invalid attached stop loss price out of bounds on aggressive order"),
    ("STOP_ALREADY_TRIGGERED", Rejected(Price), "new order: stop already triggered"),
    ("REPLACE_NOT_SUPPORTED", InvalidRequest, "new order: replace not supported"),
    ("ORDER_IS_PENDING_CANCEL", Rejected(Final), "new order: order is pending cancel"),
    ("POSITION_SIZE_INCREASE_REJECT", Rejected(ReduceOnly), "new order: position size increase reject"),
    ("ASSET_BALANCE_INCREASE_REJECT", Rejected(ReduceOnly), "new order: asset balance increase reject"),
    ("TOO_MANY_PENDING_REPLACES", Unknown, "new order: too many pending replaces"),
    ("INVALID_RFQ_BASE_SIZE_TOO_SMALL", Rejected(VenueRule), "new order: invalid rfq base size too small"),
    ("INVALID_RFQ_BASE_SIZE_TOO_LARGE", Rejected(VenueRule), "new order: invalid rfq base size too large"),
    ("INVALID_RFQ_QUOTE_SIZE_TOO_SMALL", Rejected(VenueRule), "new order: invalid rfq quote size too small"),
    ("INVALID_RFQ_QUOTE_SIZE_TOO_LARGE", Rejected(VenueRule), "new order: invalid rfq quote size too large"),
    ("INVALID_UNSUPPORTED_INSTRUMENT", InvalidRequest, "new order: invalid unsupported instrument"),
    ("REDUCE_ONLY_INCREASED_POSITION_SIZE", Rejected(ReduceOnly), "new order: reduce only increased position size"),
    ("SCALED_PARAM_INFEASIBLE", Rejected(Other), "new order: scaled param infeasible"),
    ("SCALED_MIN_ORDER_VIOLATION", Rejected(Other), "new order: scaled min order violation"),
    ("SCALED_MAX_ORDER_VIOLATION", Rejected(Other), "new order: scaled max order violation"),
    ("POST_ONLY_NOT_ALLOWED_WITH_PEG", InvalidRequest, "new order: post only not allowed with peg"),
    ("INVALID_PEG_OFFSET", InvalidRequest, "new order: invalid peg offset"),
    ("INVALID_PEG_WIG_LEVEL", InvalidRequest, "new order: invalid peg wig level"),
    ("INVALID_PEG_VENUE_OPTIONS", InvalidRequest, "new order: invalid peg venue options"),
    ("PEG_INVALID_ORDER_TYPE", InvalidRequest, "new order: peg invalid order type"),
    ("SINGLE_LEGGED_TPSL_NOT_ALLOWED", InvalidRequest, "new order: single legged tpsl not allowed"),
    ("FRACTIONAL_ORDERS_NOT_ALLOWED_FOR_PRODUCT", Rejected(VenueRule), "new order: fractional orders not allowed for product"),
    ("QUOTE_ORDERS_NOT_ALLOWED_FOR_PRODUCT", Rejected(VenueRule), "new order: quote orders not allowed for product"),
    ("NBBO_NOT_PROVIDED", InvalidRequest, "new order: nbbo not provided"),
    ("INVALID_NBBO_BID_PRICE", Rejected(Price), "new order: invalid nbbo bid price"),
    ("INVALID_NBBO_ASK_PRICE", Rejected(Price), "new order: invalid nbbo ask price"),
    ("NOTIONAL_SIZE_BREACHES_FRACTIONAL_MINIMUM", Rejected(VenueRule), "new order: notional size breaches fractional minimum"),
    ("MARKET_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION", Rejected(MarketClosed), "new order: market orders prohibited during non core session"),
    ("NOTIONAL_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION", Rejected(MarketClosed), "new order: notional orders prohibited during non core session"),
    ("MAX_NOTIONAL_PER_ORDER_BREACHED_15C35_CHECK", Rejected(Other), "new order: max notional per order breached 15c35 check"),
    ("MAX_SHARES_PER_ORDER_BREACHED_15C35_CHECK", Rejected(Other), "new order: max shares per order breached 15c35 check"),
    ("INVALID_EQUITY_TRADING_SESSION", Rejected(MarketClosed), "new order: invalid equity trading session"),
    ("PRODUCT_TRADING_HALTED", Rejected(MarketClosed), "new order: product trading halted"),
    ("TRADING_DISABLED", Rejected(MarketClosed), "new order: trading disabled"),
    ("CANNOT_CLOSE_ZERO_POSITION", Rejected(Other), "new order: cannot close zero position"),
    ("SCALED_PARAM_DISCREPANCY", Rejected(Other), "new order: scaled param discrepancy"),
    ("STOP_LOSS_PRICE_TOO_LOW", Rejected(Price), "new order: stop loss price too low"),
    ("ATTACHED_STOP_LOSS_PRICE_TOO_LOW", Rejected(Price), "new order: attached stop loss price too low"),
    ("BREACHED_RISK_LIMIT", Rejected(Funds), "new order: breached risk limit"),
    ("STOP_LOSS_PRICE_TOO_HIGH", Rejected(Price), "new order: stop loss price too high"),
    ("ATTACHED_STOP_LOSS_PRICE_TOO_HIGH", Rejected(Price), "new order: attached stop loss price too high"),
    ("TAKE_PROFIT_PRICE_TOO_HIGH", Rejected(Price), "new order: take profit price too high"),
    ("ATTACHED_TAKE_PROFIT_PRICE_TOO_HIGH", Rejected(Price), "new order: attached take profit price too high"),
    ("TAKE_PROFIT_PRICE_TOO_LOW", Rejected(Price), "new order: take profit price too low"),
    ("ATTACHED_TAKE_PROFIT_PRICE_TOO_LOW", Rejected(Price), "new order: attached take profit price too low"),
    ("MODIFY_POSITION_WORKFLOW_ALREADY_RUNNING", Unknown, "new order: modify position workflow already running"),
    ("DUPLICATE_CLIENT_ORDER_ID", Unknown, "new order: duplicate client order id"),
    ("RFQ_QUOTE_EXPIRED", Rejected(Other), "new order: rfq quote expired"),
    ("RFQ_TOO_MANY_OPEN_QUOTES", Rejected(Other), "new order: rfq too many open quotes"),
    ("RFQ_RATE_LIMITED", RateLimited, "new order: rfq rate limited"),
    ("RFQ_INVALID_COMBO_LEGS", InvalidRequest, "new order: rfq invalid combo legs"),
    ("RFQ_VALIDATION_FAILURE", Rejected(Other), "new order: rfq validation failure"),
    ("RFQ_VENUE_REJECTED", Rejected(Other), "new order: rfq venue rejected"),
    ("UNKNOWN_PREVIEW_FAILURE_REASON", Unknown, "preview: unknown preview failure reason"),
    ("PREVIEW_MISSING_COMMISSION_RATE", InvalidRequest, "preview: preview missing commission rate"),
    ("PREVIEW_INVALID_SIDE", InvalidRequest, "preview: preview invalid side"),
    ("PREVIEW_INVALID_ORDER_CONFIG", InvalidRequest, "preview: preview invalid order config"),
    ("PREVIEW_INVALID_PRODUCT_ID", InvalidRequest, "preview: preview invalid product id"),
    ("PREVIEW_INVALID_SIZE_PRECISION", Rejected(VenueRule), "preview: preview invalid size precision"),
    ("PREVIEW_INVALID_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid price precision"),
    ("PREVIEW_MISSING_PRODUCT_PRICE_BOOK", Rejected(MarketClosed), "preview: preview missing product price book"),
    ("PREVIEW_INVALID_LEDGER_BALANCE", Rejected(Funds), "preview: preview invalid ledger balance"),
    ("PREVIEW_INSUFFICIENT_LEDGER_BALANCE", Rejected(Funds), "preview: preview insufficient ledger balance"),
    ("PREVIEW_INVALID_LIMIT_PRICE_POST_ONLY", Rejected(PostOnly), "preview: preview invalid limit price post only"),
    ("PREVIEW_INVALID_LIMIT_PRICE", Rejected(Price), "preview: preview invalid limit price"),
    ("PREVIEW_INVALID_NO_LIQUIDITY", Rejected(Other), "preview: preview invalid no liquidity"),
    ("PREVIEW_INSUFFICIENT_FUND", Rejected(Funds), "preview: preview insufficient fund"),
    ("PREVIEW_INVALID_COMMISSION_CONFIGURATION", InvalidRequest, "preview: preview invalid commission configuration"),
    ("PREVIEW_INVALID_STOP_PRICE", Rejected(Price), "preview: preview invalid stop price"),
    ("PREVIEW_INVALID_BASE_SIZE_TOO_LARGE", Rejected(VenueRule), "preview: preview invalid base size too large"),
    ("PREVIEW_INVALID_BASE_SIZE_TOO_SMALL", Rejected(VenueRule), "preview: preview invalid base size too small"),
    ("PREVIEW_INVALID_QUOTE_SIZE_PRECISION", Rejected(VenueRule), "preview: preview invalid quote size precision"),
    ("PREVIEW_INVALID_QUOTE_SIZE_TOO_LARGE", Rejected(VenueRule), "preview: preview invalid quote size too large"),
    ("PREVIEW_INVALID_PRICE_TOO_LARGE", Rejected(Price), "preview: preview invalid price too large"),
    ("PREVIEW_INVALID_QUOTE_SIZE_TOO_SMALL", Rejected(VenueRule), "preview: preview invalid quote size too small"),
    ("PREVIEW_INSUFFICIENT_FUNDS_FOR_FUTURES", Rejected(Funds), "preview: preview insufficient funds for futures"),
    ("PREVIEW_BREACHED_PRICE_LIMIT", Rejected(Price), "preview: preview breached price limit"),
    ("PREVIEW_BREACHED_ACCOUNT_POSITION_LIMIT", Rejected(Other), "preview: preview breached account position limit"),
    ("PREVIEW_BREACHED_COMPANY_POSITION_LIMIT", Rejected(Other), "preview: preview breached company position limit"),
    ("PREVIEW_INVALID_MARGIN_HEALTH", Rejected(Funds), "preview: preview invalid margin health"),
    ("PREVIEW_RISK_PROXY_FAILURE", Unknown, "preview: preview risk proxy failure"),
    ("PREVIEW_UNTRADABLE_FCM_ACCOUNT_STATUS", Permission, "preview: preview untradable fcm account status"),
    ("PREVIEW_IN_LIQUIDATION", Rejected(Funds), "preview: preview in liquidation"),
    ("PREVIEW_INVALID_MARGIN_TYPE", InvalidRequest, "preview: preview invalid margin type"),
    ("PREVIEW_INVALID_LEVERAGE", InvalidRequest, "preview: preview invalid leverage"),
    ("PREVIEW_UNTRADABLE_PRODUCT", Rejected(MarketClosed), "preview: preview untradable product"),
    ("PREVIEW_INVALID_FCM_TRADING_SESSION", Rejected(MarketClosed), "preview: preview invalid fcm trading session"),
    ("PREVIEW_NOT_ALLOWED_BY_MARKET_STATE", Rejected(MarketClosed), "preview: preview not allowed by market state"),
    ("PREVIEW_BREACHED_OPEN_INTEREST_LIMIT", Rejected(Other), "preview: preview breached open interest limit"),
    ("PREVIEW_GEOFENCING_RESTRICTION", Permission, "preview: preview geofencing restriction"),
    ("PREVIEW_INVALID_END_TIME", InvalidRequest, "preview: preview invalid end time"),
    ("PREVIEW_OPPOSITE_MARGIN_TYPE_EXISTS", Rejected(Other), "preview: preview opposite margin type exists"),
    ("PREVIEW_QUOTE_SIZE_NOT_ALLOWED_FOR_BRACKET", InvalidRequest, "preview: preview quote size not allowed for bracket"),
    ("PREVIEW_INVALID_BRACKET_PRICES", Rejected(Price), "preview: preview invalid bracket prices"),
    ("PREVIEW_MISSING_MARKET_TRADE_DATA", Rejected(Other), "preview: preview missing market trade data"),
    ("PREVIEW_INVALID_BRACKET_LIMIT_PRICE", Rejected(Price), "preview: preview invalid bracket limit price"),
    ("PREVIEW_INVALID_BRACKET_STOP_TRIGGER_PRICE", Rejected(Price), "preview: preview invalid bracket stop trigger price"),
    ("PREVIEW_BRACKET_LIMIT_PRICE_OUT_OF_BOUNDS", Rejected(Price), "preview: preview bracket limit price out of bounds"),
    ("PREVIEW_STOP_TRIGGER_PRICE_OUT_OF_BOUNDS", Rejected(Price), "preview: preview stop trigger price out of bounds"),
    ("PREVIEW_BRACKET_ORDER_NOT_SUPPORTED", InvalidRequest, "preview: preview bracket order not supported"),
    ("PREVIEW_INVALID_STOP_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid stop price precision"),
    ("PREVIEW_STOP_PRICE_ABOVE_LIMIT_PRICE", Rejected(Price), "preview: preview stop price above limit price"),
    ("PREVIEW_STOP_PRICE_BELOW_LIMIT_PRICE", Rejected(Price), "preview: preview stop price below limit price"),
    ("PREVIEW_STOP_PRICE_ABOVE_LAST_TRADE_PRICE", Rejected(Price), "preview: preview stop price above last trade price"),
    ("PREVIEW_STOP_PRICE_BELOW_LAST_TRADE_PRICE", Rejected(Price), "preview: preview stop price below last trade price"),
    ("PREVIEW_FOK_DISABLED", InvalidRequest, "preview: preview fok disabled"),
    ("PREVIEW_FOK_ONLY_ALLOWED_ON_LIMIT_ORDERS", InvalidRequest, "preview: preview fok only allowed on limit orders"),
    ("PREVIEW_POST_ONLY_NOT_ALLOWED_WITH_FOK", InvalidRequest, "preview: preview post only not allowed with fok"),
    ("PREVIEW_UBO_HIGH_LEVERAGE_QUANTITY_BREACHED", Rejected(Funds), "preview: preview ubo high leverage quantity breached"),
    ("PREVIEW_ECOSYSTEM_LEVERAGE_UTILIZATION_BREACHED", Rejected(Funds), "preview: preview ecosystem leverage utilization breached"),
    ("PREVIEW_CLOSE_ONLY_FAILURE", Rejected(MarketClosed), "preview: preview close only failure"),
    ("PREVIEW_UBO_HIGH_LEVERAGE_NOTIONAL_BREACHED", Rejected(Funds), "preview: preview ubo high leverage notional breached"),
    ("PREVIEW_END_TIME_TOO_FAR_IN_FUTURE", InvalidRequest, "preview: preview end time too far in future"),
    ("PREVIEW_LIMIT_PRICE_TOO_FAR_FROM_MARKET", Rejected(Price), "preview: preview limit price too far from market"),
    ("PREVIEW_FUTURES_AFTER_HOUR_INVALID_ORDER_TYPE", Rejected(MarketClosed), "preview: preview futures after hour invalid order type"),
    ("PREVIEW_FUTURES_AFTER_HOUR_INVALID_TIME_IN_FORCE", Rejected(MarketClosed), "preview: preview futures after hour invalid time in force"),
    ("PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE", Rejected(Price), "preview: preview invalid attached take profit price"),
    ("PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE", Rejected(Price), "preview: preview invalid attached stop loss price"),
    ("PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid attached take profit price precision"),
    ("PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid attached stop loss price precision"),
    ("PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS", Rejected(Price), "preview: preview invalid attached take profit price out of bounds"),
    ("PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS", Rejected(Price), "preview: preview invalid attached stop loss price out of bounds"),
    ("PREVIEW_INVALID_BRACKET_ORDER_SIDE", InvalidRequest, "preview: preview invalid bracket order side"),
    ("PREVIEW_BRACKET_ORDER_SIZE_EXCEEDS_POSITION", Rejected(Other), "preview: preview bracket order size exceeds position"),
    ("PREVIEW_ORDER_SIZE_EXCEEDS_BRACKETED_POSITION", Rejected(Other), "preview: preview order size exceeds bracketed position"),
    ("PREVIEW_INVALID_LIMIT_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid limit price precision"),
    ("PREVIEW_INVALID_STOP_TRIGGER_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid stop trigger price precision"),
    ("PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_EXCEEDS_MAX_DISTANCE_FROM_ORIGINATING_PRICE", Rejected(Price), "preview: preview invalid attached take profit price exceeds max distance from originating price"),
    ("PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_SIZE_BELOW_MIN", Rejected(VenueRule), "preview: preview invalid attached take profit size below min"),
    ("PREVIEW_ATTACHED_ORDER_SIZE_MUST_BE_NIL", InvalidRequest, "preview: preview attached order size must be nil"),
    ("PREVIEW_BELOW_MIN_SIZE_FOR_DURATION", Rejected(VenueRule), "preview: preview below min size for duration"),
    ("PREVIEW_MAX_DAILY_VOLUME_NOTIONAL_BREACHED", Rejected(Other), "preview: preview max daily volume notional breached"),
    ("PREVIEW_INVALID_SETTLEMENT_CURRENCY", InvalidRequest, "preview: preview invalid settlement currency"),
    ("PREVIEW_DURATION_TOO_SMALL", InvalidRequest, "preview: preview duration too small"),
    ("PREVIEW_INTX_FOK_ONLY_ALLOWED_ON_LIMIT_AND_MARKET_ORDERS", InvalidRequest, "preview: preview intx fok only allowed on limit and market orders"),
    ("PREVIEW_BUCKET_SIZE_SMALLER_THAN_QUOTE_MIN", Rejected(VenueRule), "preview: preview bucket size smaller than quote min"),
    ("PREVIEW_BUCKET_SIZE_SMALLER_THAN_BASE_MIN", Rejected(VenueRule), "preview: preview bucket size smaller than base min"),
    ("PREVIEW_END_TIME_AFTER_CONTRACT_EXPIRATION", InvalidRequest, "preview: preview end time after contract expiration"),
    ("PREVIEW_START_TIME_MUST_BE_SPECIFIED", InvalidRequest, "preview: preview start time must be specified"),
    ("PREVIEW_ICEBERG_ORDERS_NOT_SUPPORTED", InvalidRequest, "preview: preview iceberg orders not supported"),
    ("PREVIEW_END_TIME_IS_IN_THE_PAST", InvalidRequest, "preview: preview end time is in the past"),
    ("PREVIEW_GTD_ORDERS_MUST_HAVE_END_TIME", InvalidRequest, "preview: preview gtd orders must have end time"),
    ("PREVIEW_ATTACHED_ORDER_MUST_HAVE_POSITIVE_PRICES", Rejected(Price), "preview: preview attached order must have positive prices"),
    ("PREVIEW_INVALID_ORDER_SIDE_FOR_ATTACHED_TPSL", InvalidRequest, "preview: preview invalid order side for attached tpsl"),
    ("PREVIEW_ATTACHED_ORDERS_ONLY_ALLOWED_ON_MARKET_LIMIT", InvalidRequest, "preview: preview attached orders only allowed on market limit"),
    ("PREVIEW_INVALID_ORDER_TYPE_FOR_ATTACHED", InvalidRequest, "preview: preview invalid order type for attached"),
    ("PREVIEW_PRICE_NOT_ALLOWED_FOR_MARKET_ORDERS", InvalidRequest, "preview: preview price not allowed for market orders"),
    ("PREVIEW_REDUCE_ONLY_NOT_ALLOWED_ON_VENUE", InvalidRequest, "preview: preview reduce only not allowed on venue"),
    ("PREVIEW_NON_NUMERIC_ORDER_SIZE", InvalidRequest, "preview: preview non numeric order size"),
    ("PREVIEW_INVALID_INTX_CLIENT_ORDER_ID", InvalidRequest, "preview: preview invalid intx client order id"),
    ("PREVIEW_DURATION_TOO_LARGE", InvalidRequest, "preview: preview duration too large"),
    ("PREVIEW_REDUCE_ONLY_NOT_ALLOWED_ON_SPOT_PRODUCTS", InvalidRequest, "preview: preview reduce only not allowed on spot products"),
    ("PREVIEW_LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_BUY", Rejected(Price), "preview: preview limit order price exceeds price band on buy"),
    ("PREVIEW_LIMIT_ORDER_PRICE_EXCEEDS_PRICE_BAND_ON_SELL", Rejected(Price), "preview: preview limit order price exceeds price band on sell"),
    ("PREVIEW_INVALID_ATTACHED_TAKE_PROFIT_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER", Rejected(Price), "preview: preview invalid attached take profit price out of bounds on aggressive order"),
    ("PREVIEW_INVALID_ATTACHED_STOP_LOSS_PRICE_OUT_OF_BOUNDS_ON_AGGRESSIVE_ORDER", Rejected(Price), "preview: preview invalid attached stop loss price out of bounds on aggressive order"),
    ("PREVIEW_STOP_TRIGGERED", Rejected(Price), "preview: preview stop triggered"),
    ("PREVIEW_REPLACE_NOT_SUPPORTED", InvalidRequest, "preview: preview replace not supported"),
    ("PREVIEW_ORDER_IS_PENDING_CANCEL", Rejected(Final), "preview: preview order is pending cancel"),
    ("PREVIEW_POSITION_SIZE_INCREASE_REJECT", Rejected(ReduceOnly), "preview: preview position size increase reject"),
    ("PREVIEW_ASSET_BALANCE_INCREASE_REJECT", Rejected(ReduceOnly), "preview: preview asset balance increase reject"),
    ("PREVIEW_TOO_MANY_PENDING_REPLACES", Unknown, "preview: preview too many pending replaces"),
    ("PREVIEW_INVALID_RFQ_BASE_SIZE_TOO_SMALL", Rejected(VenueRule), "preview: preview invalid rfq base size too small"),
    ("PREVIEW_INVALID_RFQ_BASE_SIZE_TOO_LARGE", Rejected(VenueRule), "preview: preview invalid rfq base size too large"),
    ("PREVIEW_INVALID_RFQ_QUOTE_SIZE_TOO_SMALL", Rejected(VenueRule), "preview: preview invalid rfq quote size too small"),
    ("PREVIEW_INVALID_RFQ_QUOTE_SIZE_TOO_LARGE", Rejected(VenueRule), "preview: preview invalid rfq quote size too large"),
    ("PREVIEW_REDUCE_ONLY_INCREASED_POSITION_SIZE", Rejected(ReduceOnly), "preview: preview reduce only increased position size"),
    ("PREVIEW_COMPLIANCE_PURCHASE_LIMIT_EXCEEDED", Rejected(Other), "preview: preview compliance purchase limit exceeded"),
    ("PREVIEW_SCALED_PARAM_INFEASIBLE", Rejected(Other), "preview: preview scaled param infeasible"),
    ("PREVIEW_SCALED_MIN_ORDER_VIOLATION", Rejected(Other), "preview: preview scaled min order violation"),
    ("PREVIEW_SCALED_MAX_ORDER_VIOLATION", Rejected(Other), "preview: preview scaled max order violation"),
    ("PREVIEW_POST_ONLY_NOT_ALLOWED_WITH_PEG", InvalidRequest, "preview: preview post only not allowed with peg"),
    ("PREVIEW_INVALID_PEG_OFFSET", InvalidRequest, "preview: preview invalid peg offset"),
    ("PREVIEW_INVALID_PEG_WIG_LEVEL", InvalidRequest, "preview: preview invalid peg wig level"),
    ("PREVIEW_INVALID_PEG_VENUE_OPTIONS", InvalidRequest, "preview: preview invalid peg venue options"),
    ("PREVIEW_PEG_INVALID_ORDER_TYPE", InvalidRequest, "preview: preview peg invalid order type"),
    ("PREVIEW_SINGLE_LEGGED_TPSL_NOT_ALLOWED", InvalidRequest, "preview: preview single legged tpsl not allowed"),
    ("PREVIEW_FRACTIONAL_ORDERS_NOT_ALLOWED_FOR_PRODUCT", Rejected(VenueRule), "preview: preview fractional orders not allowed for product"),
    ("PREVIEW_QUOTE_ORDERS_NOT_ALLOWED_FOR_PRODUCT", Rejected(VenueRule), "preview: preview quote orders not allowed for product"),
    ("PREVIEW_NBBO_NOT_PROVIDED", InvalidRequest, "preview: preview nbbo not provided"),
    ("PREVIEW_INVALID_NBBO_BID_PRICE", Rejected(Price), "preview: preview invalid nbbo bid price"),
    ("PREVIEW_INVALID_NBBO_ASK_PRICE", Rejected(Price), "preview: preview invalid nbbo ask price"),
    ("PREVIEW_NOTIONAL_SIZE_BREACHES_FRACTIONAL_MINIMUM", Rejected(VenueRule), "preview: preview notional size breaches fractional minimum"),
    ("PREVIEW_MARKET_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION", Rejected(MarketClosed), "preview: preview market orders prohibited during non core session"),
    ("PREVIEW_NOTIONAL_ORDERS_PROHIBITED_DURING_NON_CORE_SESSION", Rejected(MarketClosed), "preview: preview notional orders prohibited during non core session"),
    ("PREVIEW_MAX_NOTIONAL_PER_ORDER_BREACHED_15C35_CHECK", Rejected(Other), "preview: preview max notional per order breached 15c35 check"),
    ("PREVIEW_MAX_SHARES_PER_ORDER_BREACHED_15C35_CHECK", Rejected(Other), "preview: preview max shares per order breached 15c35 check"),
    ("PREVIEW_INVALID_EQUITY_TRADING_SESSION", Rejected(MarketClosed), "preview: preview invalid equity trading session"),
    ("PREVIEW_PRODUCT_TRADING_HALTED", Rejected(MarketClosed), "preview: preview product trading halted"),
    ("PREVIEW_TRADING_DISABLED", Rejected(MarketClosed), "preview: preview trading disabled"),
    ("PREVIEW_INVALID_BRACKET_LIMIT_PRICE_PRECISION", Rejected(VenueRule), "preview: preview invalid bracket limit price precision"),
    ("PREVIEW_SCALED_PARAM_DISCREPANCY", Rejected(Other), "preview: preview scaled param discrepancy"),
    ("PREVIEW_STOP_LOSS_PRICE_TOO_LOW", Rejected(Price), "preview: preview stop loss price too low"),
    ("PREVIEW_PREDICTIONS_QUOTE_SIZE_BELOW_MIN_CONTRACT_PRICE", Rejected(VenueRule), "preview: preview predictions quote size below min contract price"),
    ("PREVIEW_PREDICTIONS_HIGH_PRICE_CONTRACTS_BLOCKED", Rejected(Price), "preview: preview predictions high price contracts blocked"),
    ("PREVIEW_ATTACHED_STOP_LOSS_PRICE_TOO_LOW", Rejected(Price), "preview: preview attached stop loss price too low"),
    ("PREVIEW_BREACHED_RISK_LIMIT", Rejected(Funds), "preview: preview breached risk limit"),
    ("PREVIEW_STOP_LOSS_PRICE_TOO_HIGH", Rejected(Price), "preview: preview stop loss price too high"),
    ("PREVIEW_ATTACHED_STOP_LOSS_PRICE_TOO_HIGH", Rejected(Price), "preview: preview attached stop loss price too high"),
    ("PREVIEW_TAKE_PROFIT_PRICE_TOO_HIGH", Rejected(Price), "preview: preview take profit price too high"),
    ("PREVIEW_ATTACHED_TAKE_PROFIT_PRICE_TOO_HIGH", Rejected(Price), "preview: preview attached take profit price too high"),
    ("PREVIEW_TAKE_PROFIT_PRICE_TOO_LOW", Rejected(Price), "preview: preview take profit price too low"),
    ("PREVIEW_ATTACHED_TAKE_PROFIT_PRICE_TOO_LOW", Rejected(Price), "preview: preview attached take profit price too low"),
    ("PREVIEW_INVALID_UNSUPPORTED_INSTRUMENT", InvalidRequest, "preview: preview invalid unsupported instrument"),
    ("UNKNOWN_CANCEL_FAILURE_REASON", Unknown, "cancel: unknown cancel failure reason"),
    ("INVALID_CANCEL_REQUEST", Unknown, "cancel: invalid cancel request"),
    ("UNKNOWN_CANCEL_ORDER", Unknown, "cancel: unknown cancel order"),
    ("COMMANDER_REJECTED_CANCEL_ORDER", Unknown, "cancel: commander rejected cancel order"),
    ("DUPLICATE_CANCEL_REQUEST", Unknown, "cancel: duplicate cancel request"),
    ("INVALID_CANCEL_PRODUCT_ID", InvalidRequest, "cancel: invalid cancel product id"),
    ("INVALID_CANCEL_FCM_TRADING_SESSION", Rejected(MarketClosed), "cancel: invalid cancel fcm trading session"),
    ("NOT_ALLOWED_TO_CANCEL", Permission, "cancel: not allowed to cancel"),
    ("ORDER_IS_FULLY_FILLED", Rejected(Final), "cancel: order is fully filled"),
    ("ORDER_IS_BEING_REPLACED", Unknown, "cancel: order is being replaced"),
    ("UNKNOWN_EDIT_ORDER_FAILURE_REASON", Unknown, "edit: unknown edit order failure reason"),
    ("COMMANDER_REJECTED_EDIT_ORDER", Unknown, "edit: commander rejected edit order"),
    ("CANNOT_EDIT_TO_BELOW_FILLED_SIZE", Rejected(Other), "edit: cannot edit to below filled size"),
    ("ORDER_NOT_FOUND", Rejected(NotFound), "edit: order not found"),
    ("CALLER_ID_MISMATCH", Permission, "edit: caller id mismatch"),
    ("ONLY_LIMIT_ORDER_EDITS_SUPPORTED", InvalidRequest, "edit: only limit order edits supported"),
    ("INVALID_EDITED_SIZE", Rejected(VenueRule), "edit: invalid edited size"),
    ("INVALID_EDITED_PRICE", Rejected(Price), "edit: invalid edited price"),
    ("INVALID_ORIGINAL_SIZE", InvalidRequest, "edit: invalid original size"),
    ("INVALID_ORIGINAL_PRICE", InvalidRequest, "edit: invalid original price"),
    ("EDIT_REQUEST_EQUAL_TO_ORIGINAL_REQUEST", InvalidRequest, "edit: edit request equal to original request"),
    ("ONLY_OPEN_ORDERS_CAN_BE_EDITED", Rejected(Final), "edit: only open orders can be edited"),
    ("SIZE_IN_QUOTE_EDITS_NOT_ALLOWED", InvalidRequest, "edit: size in quote edits not allowed"),
    ("ORDER_IS_ALREADY_BEING_REPLACED", Unknown, "edit: order is already being replaced"),
    ("PORTFOLIO_NOT_ALLOWED_FOR_EDITS", Permission, "edit: portfolio not allowed for edits"),
    ("STRATEGY_NOT_SUPPORTED", InvalidRequest, "edit: strategy not supported"),
    ("FIELD_STOPPRICE_NOT_SUPPORTED", InvalidRequest, "edit: field stopprice not supported"),
    ("FIELD_DISPLAYSIZE_NOT_SUPPORTED", InvalidRequest, "edit: field displaysize not supported"),
    ("FIELD_ENDTIME_NOT_SUPPORTED", InvalidRequest, "edit: field endtime not supported"),
    ("INVALID_ORIGINAL_TIME", InvalidRequest, "edit: invalid original time"),
    ("INVALID_NEW_ENDTIME", InvalidRequest, "edit: invalid new endtime"),
    ("NOT_STARTED", Rejected(Other), "edit: not started"),
    ("CANNOT_EDIT_FUTURES_ORDER", Rejected(Other), "edit: cannot edit futures order"),
    ("FUTURES_VENUE_ORDER_NOT_FOUND", Rejected(NotFound), "edit: futures venue order not found"),
    ("CANNOT_EDIT_STOP_PRICE_FOR_TRIGGERED_ORDER", Rejected(Other), "edit: cannot edit stop price for triggered order"),
    ("CANNOT_EDIT_ORDER_PENDING_CANCEL", Rejected(Final), "edit: cannot edit order pending cancel"),
    ("INSUFFICIENT_TIME_TO_EDIT", Rejected(Other), "edit: insufficient time to edit"),
    ("EXCEEDED_MAX_ALLOWED_EDIT_REQUEST_COUNT", Rejected(Other), "edit: exceeded max allowed edit request count"),
    ("CANNOT_CANCEL_ACTIVE_ATTACHED_ORDER", Rejected(Other), "edit: cannot cancel active attached order"),
    ("ORDER_EDIT_INVALID_ATTACHED_ORDER_REQUEST", InvalidRequest, "edit: order edit invalid attached order request"),
    ("ORDER_EDIT_ORDER_TYPE_NOT_SUPPORTED", InvalidRequest, "edit: order edit order type not supported"),
    ("CANNOT_EDIT_TRIGGERED_ORDER", Rejected(Other), "edit: cannot edit triggered order"),
    ("INVALID_EDITED_STOP_PRICE", Rejected(Price), "edit: invalid edited stop price"),
    ("INVALID_EDITED_STOP_LIMIT_PRICE", Rejected(Price), "edit: invalid edited stop limit price"),
    ("INVALID_ORIGINAL_STOP_PRICE", InvalidRequest, "edit: invalid original stop price"),
    ("INVALID_ORIGINAL_STOP_LIMIT_PRICE", InvalidRequest, "edit: invalid original stop limit price"),
    ("INVALID_ORIGINAL_ATTACHED_ORDER_CONFIG", InvalidRequest, "edit: invalid original attached order config"),
    ("CANNOT_EDIT_ATTACHED_ORDER_CONFIGURATION_AFTER_CREATION", Rejected(Other), "edit: cannot edit attached order configuration after creation"),
    ("CANNOT_CONVERT_SIZE_ASSET", InvalidRequest, "edit: cannot convert size asset"),
    ("CANNOT_CONVERT_DISPLAY_SIZE_ASSET", InvalidRequest, "edit: cannot convert display size asset"),
    ("CANNOT_EDIT_ATTACHED_ORDER_SIZE_WITH_OPEN_ORIGINATING_ORDER", Rejected(Other), "edit: cannot edit attached order size with open originating order"),
    ("INVALID_EDITED_TAKE_PROFIT_PRICE", Rejected(Price), "edit: invalid edited take profit price"),
    ("INVALID_EDITED_STOP_LOSS_PRICE", Rejected(Price), "edit: invalid edited stop loss price"),
    ("INVALID_EDITED_ATTACHED_ORDER_CONFIGURATION", InvalidRequest, "edit: invalid edited attached order configuration"),
    ("CANNOT_EDIT_ATTACHED_SL_ORDER_PRICE", Rejected(Other), "edit: cannot edit attached sl order price"),
    ("CANNOT_ADD_ATTACHED_ORDER_TO_ORDER_TYPE", InvalidRequest, "edit: cannot add attached order to order type"),
    ("CANNOT_CHANGE_SIDE_OF_ORDER", InvalidRequest, "edit: cannot change side of order"),
    ("CANNOT_CHANGE_ORDER_TYPE_OF_ORDER", InvalidRequest, "edit: cannot change order type of order"),
    ("CANNOT_CHANGE_PRODUCT_ID_OF_ORDER", InvalidRequest, "edit: cannot change product id of order"),
    ("PREV_CLIENT_ORDER_ID_REQUIRED_WHEN_CLIENT_ORDER_ID_PROVIDED", InvalidRequest, "edit: prev client order id required when client order id provided"),
    ("CLIENT_ORDER_ID_REQUIRED_WHEN_PREV_CLIENT_ORDER_ID_PROVIDED", InvalidRequest, "edit: client order id required when prev client order id provided"),
    ("CLIENT_ORDER_ID_MUST_BE_DIFFERENT_FROM_PREV_CLIENT_ORDER_ID", InvalidRequest, "edit: client order id must be different from prev client order id"),
    ("PREV_CLIENT_ORDER_ID_MUST_MATCH_CURRENT_CLIENT_ORDER_ID", InvalidRequest, "edit: prev client order id must match current client order id"),
    ("CANNOT_MODIFY_ATTACHED_CONFIG_OF_NON_NEW_ORDER", Rejected(Other), "edit: cannot modify attached config of non new order"),
    ("INVALID_EDITED_ATTACHED_ORDER_CONFIGURATION_NON_MODIFIABLE_VALUE_EDIT", InvalidRequest, "edit: invalid edited attached order configuration non modifiable value edit"),
    ("ATTACHED_CONFIGURATION_IS_EQUAL", InvalidRequest, "edit: attached configuration is equal"),
    ("CANNOT_ADD_LEG_TO_TPSL_ORDER", InvalidRequest, "edit: cannot add leg to tpsl order"),
    ("CANNOT_REMOVE_LEG_FROM_TPSL_ORDER", InvalidRequest, "edit: cannot remove leg from tpsl order"),
    ("SINGLE_LEGGED_BRACKET_ORDER_NOT_ALLOWED", InvalidRequest, "edit: single legged bracket order not allowed"),
    ("SINGLE_LEGGED_ATTACHED_ORDER_CONFIGURATION_NOT_ALLOWED", InvalidRequest, "edit: single legged attached order configuration not allowed"),
    ("FIELD_PEG_OFFSET_NOT_SUPPORTED", InvalidRequest, "edit: field peg offset not supported"),
    ("INVALID_EDITED_PEG_WIG_LEVEL", InvalidRequest, "edit: invalid edited peg wig level"),
    ("INVALID_EDITED_PEG_OFFSET", InvalidRequest, "edit: invalid edited peg offset"),
    ("FIELD_PEG_WIG_LEVEL_NOT_SUPPORTED", InvalidRequest, "edit: field peg wig level not supported"),
];

fn row(code: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|(c, _, _)| *c == code)
}

/// A failure reason with the venue's message, mapped: a listed one in the refusal's words, any
/// other one unknown and alerting.
pub(super) fn of(code: &str, message: &str) -> VenueError {
    let words = |w: &str| if message.trim().is_empty() { w.split_once(": ").map(|(_, t)| t).unwrap_or(w).to_string() } else { message.trim().to_string() };
    match row(code) {
        Some((_, Unknown, w)) => VenueError::new(Unknown, Some(code.to_string()), format!("Coinbase answered {code} ({}): whether it acted is not known", words(w))),
        Some((_, canon, w)) => VenueError::new(*canon, Some(code.to_string()), format!("Coinbase refused it: {} (code {code})", words(w))),
        None => VenueError::unmapped(Some(code.to_string()), format!("Coinbase answered code {code} ({}), which OpenTraderWorld does not map", message.trim())),
    }
}

/// A refusal with no reason at all (`success: false` and nothing that says why): whether the
/// venue acted is not known.
fn no_reason(what: &str) -> VenueError {
    VenueError::new(Unknown, None, format!("Coinbase answered the {what} with success false and no reason: whether it acted is not known"))
}

/// `error_response` of a create (and of a close): the reason, the newest field first. The two
/// older ones (`error`, `preview_failure_reason`) are deprecated but still sent (the sandbox's
/// `INSUFFICIENT_FUND` answer carries only them).
#[derive(Deserialize, Default)]
pub(super) struct ErrorResponse {
    #[serde(default)]
    new_order_failure_reason: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    preview_failure_reason: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error_details: Option<String>,
}

/// The first reason that says something: `UNKNOWN_*` is the enums' default, so it is taken only
/// when nothing else is stated.
fn first_reason<'a>(fields: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    let given: Vec<&str> = fields.into_iter().flatten().filter(|s| !s.trim().is_empty()).collect();
    given.iter().find(|s| !s.starts_with("UNKNOWN_")).or(given.first()).copied()
}

impl ErrorResponse {
    pub(super) fn to_error(&self) -> VenueError {
        let reason = first_reason([self.new_order_failure_reason.as_deref(), self.error.as_deref(), self.preview_failure_reason.as_deref()]);
        let message = [self.error_details.as_deref(), self.message.as_deref()].into_iter().flatten().find(|m| !m.trim().is_empty()).unwrap_or("");
        match reason {
            Some(r) => of(r, message),
            None => no_reason("order"),
        }
    }
}

/// One row of a cancel's `results`, mapped: `None` when it was submitted.
pub(super) fn cancel_row(row: &Value) -> Option<VenueError> {
    if row.get("success").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    match row.get("failure_reason").and_then(Value::as_str).filter(|r| !r.is_empty()) {
        Some(r) => Some(of(r, "")),
        None => Some(no_reason("cancel")),
    }
}

/// The error an answer carries, if any. A timeout or a dropped connection, a 5xx: unknown
/// (the venue may have acted). A 200 is read for the refusals it carries in its body, whatever
/// the call (a body shape the call does not have is simply absent).
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        let what = http::timeout(frame, "coinbase").map(|e| e.message).unwrap_or_else(|| "no answer".into());
        return Some(VenueError::new(Unknown, None, format!("coinbase: {what}")));
    };
    let status = *status;
    if !(200..300).contains(&status) {
        return Some(from_status(frame, endpoint_id, status, body));
    }
    let v: Value = serde_json::from_slice(body).ok()?;
    // Place (and the error shape of a close): `success: false` with `error_response`.
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        if let Some(er) = v.get("error_response").filter(|e| e.is_object()) {
            let er: ErrorResponse = serde_json::from_value(er.clone()).unwrap_or_default();
            return Some(er.to_error());
        }
        // Edit: `errors[]` of `{edit_failure_reason, preview_failure_reason}`.
        if let Some(errors) = v.get("errors").and_then(Value::as_array) {
            let reason = errors.iter().find_map(|e| {
                first_reason([e.get("edit_failure_reason").and_then(Value::as_str), e.get("preview_failure_reason").and_then(Value::as_str)])
            });
            return Some(reason.map(|r| of(r, "")).unwrap_or_else(|| no_reason("change")));
        }
        return Some(no_reason(endpoint_id));
    }
    // Preview: `errs[]`, "potential failure reasons were this order to be submitted".
    if let Some(errs) = v.get("errs").and_then(Value::as_array).filter(|e| !e.is_empty()) {
        let reason = first_reason(errs.iter().map(Value::as_str));
        return Some(reason.map(|r| of(r, "")).unwrap_or_else(|| no_reason("preview")));
    }
    // A single cancel: its one row. A batch's rows are read one by one, by the trader.
    if *endpoint_id != CANCEL_BATCH {
        if let Some(rows) = v.get("results").and_then(Value::as_array) {
            if *endpoint_id == CANCEL && rows.is_empty() {
                return Some(VenueError::unmapped(None, "Coinbase answered the cancel with no result row, which OpenTraderWorld does not read as done"));
            }
            return rows.iter().find_map(cancel_row);
        }
    }
    None
}

/// The gRPC gateway's error body (`grpc.gateway.runtime.Error`): its fields are shown, the HTTP
/// status decides (the doc lists no `code` values).
#[derive(Deserialize, Default)]
struct Gateway {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn from_status(frame: &RawFrame, endpoint: &str, status: u16, body: &[u8]) -> VenueError {
    let g: Gateway = serde_json::from_slice(body).unwrap_or_default();
    let code = g.error.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| status.to_string());
    let text = g.message.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(200).collect());
    let text = text.trim();
    let say = |canon: Canon| VenueError::new(canon, Some(code.clone()), format!("Coinbase refused it: {text} (code {code}, HTTP {status})"));
    match status {
        401 => say(Canon::Auth),
        403 => say(Permission),
        429 => {
            let mut e = say(RateLimited);
            e.retry_after_ms = http::retry_after_ms(frame);
            e
        }
        // A lookup of an order the venue does not have (Get Order, the venue's id in the path).
        404 if endpoint == GET_ORDER => say(Rejected(NotFound)),
        400..=499 => say(InvalidRequest),
        // A 5xx: the venue may have acted.
        _ => VenueError::new(Unknown, Some(code.clone()), format!("Coinbase answered HTTP {status} ({text}): whether it acted is not known")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn every_listed_reason_maps_in_each_shape_and_an_unlisted_one_alerts() {
        for (code, canon, _) in ERRORS {
            let create = format!(r#"{{"success":false,"error_response":{{"new_order_failure_reason":"{code}","message":"m"}}}}"#);
            let e = decode(&http_frame(1, CREATE, 200, &create)).unwrap();
            assert_eq!((e.canon, e.mapped), (*canon, true), "{code}");
            let e = decode(&http_frame(1, PREVIEW, 200, &format!(r#"{{"errs":["{code}"]}}"#))).unwrap();
            assert_eq!((e.canon, e.mapped), (*canon, true), "{code}");
        }
        let e = decode(&http_frame(1, CREATE, 200, r#"{"success":false,"error_response":{"new_order_failure_reason":"SOMETHING_NEW","message":"new"}}"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        assert!(e.message.contains("does not map") && e.message.contains("SOMETHING_NEW"), "{}", e.message);
    }

    #[test]
    fn success_decides_never_the_reason_field() {
        // FAQ: success true with UNKNOWN_FAILURE_REASON is an accepted order.
        let faq = include_str!("fixtures/trade/doc_faq_success_unknown_reason.json");
        assert!(decode(&http_frame(1, CREATE, 200, faq)).is_none());
        // No reason at all: unknown, looked up, never resent.
        let e = decode(&http_frame(1, CREATE, 200, r#"{"success":false,"error_response":{}}"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, true));
        // UNKNOWN_FAILURE_REASON alone is unknown too; a reason beside it says why.
        let e = decode(&http_frame(1, CREATE, 200, r#"{"success":false,"error_response":{"new_order_failure_reason":"UNKNOWN_FAILURE_REASON","preview_failure_reason":"PREVIEW_INSUFFICIENT_FUND"}}"#)).unwrap();
        assert_eq!(e.canon, Rejected(Funds));
    }

    #[test]
    fn refusal_words_name_the_broker_code_and_text() {
        let e = decode(&http_frame(1, CREATE, 200, include_str!("fixtures/trade/sandbox_create_insufficient_fund.json"))).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Rejected(Funds), Some("INSUFFICIENT_FUND")));
        assert_eq!(e.message, "Coinbase refused it: Insufficient balance in source account (code INSUFFICIENT_FUND)");
    }

    #[test]
    fn a_cancel_row_decides_a_single_cancel_and_a_batch_is_left_to_its_rows() {
        let failed = include_str!("fixtures/trade/sandbox_batch_cancel_failure.json");
        let e = decode(&http_frame(1, CANCEL, 200, failed)).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Unknown, Some("UNKNOWN_CANCEL_ORDER")));
        assert!(decode(&http_frame(1, CANCEL_BATCH, 200, failed)).is_none());
        // Submitted, with the enum's default reason beside it: done.
        assert!(decode(&http_frame(1, CANCEL, 200, include_str!("fixtures/trade/sandbox_batch_cancel.json"))).is_none());
        let filled = r#"{"results":[{"success":false,"failure_reason":"ORDER_IS_FULLY_FILLED","order_id":"o"}]}"#;
        assert_eq!(decode(&http_frame(1, CANCEL, 200, filled)).unwrap().canon, Rejected(Final));
        assert!(!decode(&http_frame(1, CANCEL, 200, r#"{"results":[]}"#)).unwrap().mapped);
    }

    #[test]
    fn an_edit_and_a_preview_refuse_in_their_own_shape() {
        let e = decode(&http_frame(1, EDIT, 200, include_str!("fixtures/trade/sandbox_edit_failure.json"))).unwrap();
        assert_eq!(e.canon, Rejected(NotFound));
        assert!(decode(&http_frame(1, EDIT, 200, include_str!("fixtures/trade/sandbox_edit.json"))).is_none());
        let e = decode(&http_frame(1, PREVIEW, 200, include_str!("fixtures/trade/sandbox_preview_insufficient_fund.json"))).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Rejected(Funds), Some("PREVIEW_INSUFFICIENT_FUND")));
        assert!(decode(&http_frame(1, PREVIEW, 200, include_str!("fixtures/trade/sandbox_preview.json"))).is_none());
    }

    #[test]
    fn the_status_decides_outside_a_200() {
        let e = |status: u16, endpoint: &'static str| decode(&http_frame(1, endpoint, status, r#"{"error":"X","code":1,"message":"m"}"#)).unwrap();
        assert_eq!(e(401, CREATE).canon, Canon::Auth);
        assert_eq!(e(403, CREATE).canon, Permission);
        assert_eq!(e(429, CREATE).canon, RateLimited);
        assert_eq!(e(404, GET_ORDER).canon, Rejected(NotFound));
        assert_eq!(e(404, CREATE).canon, InvalidRequest);
        assert_eq!(e(400, CREATE).canon, InvalidRequest);
        assert_eq!(e(502, CREATE).canon, Unknown);
        // The sandbox's answer to an id that is not one.
        let bad = decode(&http_frame(1, GET_ORDER, 400, include_str!("fixtures/trade/sandbox_get_order_invalid_id.json"))).unwrap();
        assert_eq!((bad.canon, bad.code.as_deref()), (InvalidRequest, Some("INVALID_ARGUMENT")));
        let lost = decode(&RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: CREATE }).unwrap();
        assert_eq!(lost.canon, Unknown);
    }

    /// The PROVIDER.md table ("Trading", "Errors") names exactly the reasons mapped here, each
    /// under the canonical kind it maps to.
    #[test]
    fn the_provider_table_matches_the_code() {
        let doc = include_str!("PROVIDER.md");
        let start = doc.find("### Errors").unwrap();
        let table = &doc[start..start + doc[start..].find("### Rate limits for orders").unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in table.lines().filter(|l| l.starts_with("| ")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            if cells.len() < 4 {
                continue;
            }
            let (codes, canon) = (cells[2], cells[1]);
            for code in codes.split('`').skip(1).step_by(2) {
                seen.push((code.to_string(), canon.to_string()));
            }
        }
        let word = |c: &Canon| match c {
            Rejected(VenueRule) => "Rejected(venue_rule)",
            Rejected(Funds) => "Rejected(funds)",
            Rejected(Price) => "Rejected(price)",
            Rejected(PostOnly) => "Rejected(post_only)",
            Rejected(ReduceOnly) => "Rejected(reduce_only)",
            Rejected(MarketClosed) => "Rejected(market closed)",
            Rejected(NotFound) => "Rejected(not found)",
            Rejected(Final) => "Rejected(final)",
            Rejected(Other) => "Rejected(other)",
            Unknown => "Unknown",
            RateLimited => "RateLimited",
            Canon::Auth => "Auth",
            Permission => "Permission",
            InvalidRequest => "InvalidRequest",
        };
        let want: Vec<(String, String)> = ERRORS.iter().map(|(c, k, _)| (c.to_string(), word(k).to_string())).collect();
        for w in &want {
            assert!(seen.contains(w), "PROVIDER.md lacks {w:?}");
        }
        for s in &seen {
            assert!(want.contains(s), "PROVIDER.md names {s:?}, which the table does not map so");
        }
    }
}
