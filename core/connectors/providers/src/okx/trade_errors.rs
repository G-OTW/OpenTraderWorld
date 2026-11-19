//! OKX's order error table (onboarding spec 8), from the v5 error code page
//! (https://www.okx.com/docs-v5/en/#error-code-rest-api-public, snapshot read 2026-11-11): every
//! code of its General, API and Trade classes (50000 to 51699, 51820 to 51825, 54000 to 54109),
//! of its Data class (`52000`), and `51784` of its Finance class (its words name a client order
//! id in flight), mapped to one canonical kind; a sub-code (`51008_1000`) maps as its code. A code
//! not listed here is `Unknown` and alerts. Mirrored in PROVIDER.md ("Trading", "Errors"); a test
//! keeps the two in step.
//!
//! OKX answers most refusals with HTTP 200. A call on one order carries the order's own result
//! in `data[0].sCode` / `sMsg` / `subCode` beside the envelope's `code` (`1` "Operation failed"
//! when the item failed): the item's code is the one read. A batch answer's rows are read one by
//! one ([`super::trade`] `decode_cancel_rows`), so its envelope is not an error of its own.

use otw_connector_model::ErrorKind;
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, PostOnly, Price, ReduceOnly, VenueRule};

/// `(code, canonical, the doc's words)`, in the doc's order of codes.
pub(super) const ERRORS: &[ErrorRow] = &[
    ("1", Unknown, "Operation failed."),
    ("2", Unknown, "Bulk operation partially succeeded."),
    ("50000", InvalidRequest, "Body for POST request cannot be empty."),
    ("50001", Unknown, "Service temporarily unavailable. Please try again later."),
    ("50002", InvalidRequest, "JSON syntax error"),
    ("50004", Unknown, "API endpoint request timeout. (does not mean that the request was successful or failed, please check the request result)."),
    ("50005", InvalidRequest, "API endpoint is inactive or unavailable."),
    ("50006", InvalidRequest, "Invalid Content-Type. Please use \"application/JSON\"."),
    ("50007", Rejected(Other), "User blocked."),
    ("50008", Rejected(Other), "User doesn't exist."),
    ("50009", Rejected(Other), "Account is frozen due to stop-out."),
    ("50010", Rejected(Other), "User ID cannot be empty."),
    ("50011", RateLimited, "Rate limit reached. Please refer to API documentation and throttle requests accordingly."),
    ("50012", Rejected(Other), "Account status invalid. Check account status"),
    ("50013", Unknown, "Systems are busy. Please try again later."),
    ("50014", InvalidRequest, "Parameter {param0} can not be empty."),
    ("50015", InvalidRequest, "Either parameter {param0} or {param1} is required"),
    ("50016", InvalidRequest, "Parameter {param0} does not match parameter {param1}"),
    ("50017", Rejected(Other), "Position frozen and related operations restricted due to auto-deleveraging (ADL). Please try again later."),
    ("50018", Rejected(Other), "Currency {param0} is frozen due to ADL. Operation restricted."),
    ("50019", Rejected(Other), "Account frozen and related operations restricted due to auto-deleveraging (ADL). Please try again later."),
    ("50020", Rejected(Other), "Position frozen and related operations restricted due to forced liquidation. Please try again later."),
    ("50021", Rejected(Other), "Currency {param0} is frozen due to liquidation. Operation restricted."),
    ("50022", Rejected(Other), "Account frozen and related operations restricted due to forced liquidation. Please try again later."),
    ("50023", Rejected(Other), "Funding fees frozen and related operations are restricted. Please try again later."),
    ("50024", InvalidRequest, "Parameter {param0} and {param1} can not exist at the same time."),
    ("50025", InvalidRequest, "Parameter {param0} count exceeds the limit {param1}."),
    ("50026", Unknown, "System error. Try again later"),
    ("50027", Rejected(Other), "This account is restricted from trading. Please contact customer support for assistance."),
    ("50028", Rejected(Other), "Unable to place the order. Please contact the customer service for details."),
    ("50029", Rejected(Other), "Your account has triggered OKX risk control and is temporarily restricted from conducting transactions. Please check your email registered with OKX for contact from our customer support team."),
    ("50030", Permission, "You don't have permission to use this API endpoint"),
    ("50032", Rejected(Other), "Your account has been set to prohibit transactions in this currency. Please confirm and try again"),
    ("50033", Rejected(Other), "Instrument blocked. Please verify trading this instrument is allowed under account settings and try again."),
    ("50035", Permission, "This endpoint requires that APIKey must be bound to IP"),
    ("50036", InvalidRequest, "The expTime can't be earlier than the current system time. Please adjust the expTime and try again."),
    ("50037", Rejected(Other), "Order expired."),
    ("50038", Permission, "This feature is unavailable in demo trading"),
    ("50039", InvalidRequest, "Parameter \"before\" isn't supported for timestamp pagination"),
    ("50040", RateLimited, "Too frequent operations, please try again later"),
    ("50041", Permission, "Your user ID hasn’t been allowlisted. Please contact customer service for assistance."),
    ("50042", Unknown, "Repeated request"),
    ("50044", Rejected(Other), "Must select one broker type"),
    ("50045", Rejected(Other), "simPos should be empty because simulated positions cannot be counted when Position Builder is calculating under Spot-Derivatives risk offset mode"),
    ("50046", Rejected(Other), "This feature is temporarily unavailable while we make some improvements to it. Please try again later."),
    ("50047", Rejected(Other), "{param0} has already settled. To check the relevant candlestick data, please use {param1}"),
    ("50048", Rejected(Other), "Switching risk unit may lead position risk increases and be forced liquidated. Please adjust position size, make sure margin is in a safe status."),
    ("50049", Rejected(Other), "No information on the position tier. The current instrument doesn’t support margin trading."),
    ("50050", Rejected(Other), "You’ve already activated options trading. Please don’t activate it again."),
    ("50051", Rejected(Other), "Due to compliance restrictions in your country or region, you cannot use this feature."),
    ("50052", Rejected(Other), "Due to local laws and regulations, you cannot trade with your chosen crypto."),
    ("50053", Rejected(Other), "This feature is only available in demo trading."),
    ("50055", Rejected(Other), "Reset unsuccessful. Assets can only be reset up to 5 times per day."),
    ("50056", Rejected(Other), "You have pending orders or open positions with this currency. Please reset after canceling all the pending orders/closing all the open positions."),
    ("50057", Rejected(Other), "Reset unsuccessful. Try again later."),
    ("50058", Rejected(Other), "This crypto is not supported in an asset reset."),
    ("50059", Rejected(Other), "Before you continue, you'll need to complete additional steps as required by your local regulators. Please visit the website or app for more details."),
    ("50060", Rejected(Other), "For security and compliance purposes, please complete the identity verification process to continue using our services."),
    ("50061", RateLimited, "You've reached the maximum order rate limit for this account."),
    ("50062", Rejected(Other), "This feature is currently unavailable."),
    ("50063", Rejected(Other), "You can't activate the credits as they might have expired or are already activated."),
    ("50064", Rejected(Other), "The borrowing system is unavailable. Try again later."),
    ("50067", Rejected(Other), "The API doesn't support cross site trading feature"),
    ("50069", Rejected(Other), "Margin ratio verification for this risk unit failed."),
    ("50071", Unknown, "{param} already exists / e.g. clOrdId already exists"),
    ("50072", InvalidRequest, "Isolated margin mode is no longer supported in multi-currency margin mode and portfolio margin mode."),
    ("50075", Rejected(Other), "The token is restricted in Islamic sub-account"),
    ("50076", Rejected(Other), "The time interval between the start date and end date cannot exceed {param0} days"),
    ("50077", Rejected(Other), "The time interval between the start date and end date cannot exceed {param0} months"),
    ("50100", Auth, "API frozen, please contact customer service."),
    ("50101", Auth, "APIKey does not match current environment."),
    ("50102", Auth, "Timestamp request expired."),
    ("50103", Auth, "Request header \"OK-ACCESS-KEY\" cannot be empty."),
    ("50104", Auth, "Request header \"OK-ACCESS-PASSPHRASE\" cannot be empty."),
    ("50105", Auth, "Request header \"OK-ACCESS-PASSPHRASE\" incorrect."),
    ("50106", Auth, "Request header \"OK-ACCESS-SIGN\" cannot be empty."),
    ("50107", Auth, "Request header \"OK-ACCESS-TIMESTAMP\" cannot be empty."),
    ("50108", Auth, "Exchange ID does not exist."),
    ("50109", Auth, "Exchange domain does not exist."),
    ("50110", Permission, "Your IP {param0} is not included in your API key's IP whitelist."),
    ("50111", Auth, "Invalid OK-ACCESS-KEY."),
    ("50112", Auth, "Invalid OK-ACCESS-TIMESTAMP."),
    ("50113", Auth, "Invalid signature."),
    ("50114", Auth, "Invalid authorization."),
    ("50115", Rejected(Other), "Invalid request method."),
    ("50116", Rejected(Other), "Fast API is allowed to create only one API key"),
    ("50118", Rejected(Other), "To link the app using your API key, your broker needs to share their IP to be whitelisted"),
    ("50119", Auth, "API key doesn't exist"),
    ("50120", Permission, "This API key doesn't have permission to use this function"),
    ("50121", Permission, "You can't access our services through the IP address ({param0})"),
    ("50122", Rejected(VenueRule), "Order amount must exceed minimum amount"),
    ("50123", Permission, "This API Key does not have trading permission for the Crypto."),
    ("51000", InvalidRequest, "Parameter {param0} error"),
    ("51001", InvalidRequest, "Instrument ID, Instrument ID code or Spread ID doesn't exist."),
    ("51002", InvalidRequest, "Instrument ID doesn't match underlying index."),
    ("51003", InvalidRequest, "Either client order ID or order ID is required."),
    ("51004", Rejected(Other), "Order failed. For isolated long/short mode of {param0}, the sum of current order size, position quantity in the same direction, and pending orders in the same direction can’t be more than {param1}(contracts) which is the maximum position amount under current leverage. Please lower the leverage or use a new sub-account to place the order again (current leverage: {param2}×, current order size: {param3} contracts, position quantity in the same direction: {param4} contracts, pending orders in the same direction: {param5} contracts)."),
    ("51005", Rejected(VenueRule), "Your order amount exceeds the max order amount."),
    ("51006", Rejected(Price), "Order price is not within the price limit (max buy price: {param0} , min sell price: {param1} )"),
    ("51007", Rejected(VenueRule), "Order failed. Please place orders of at least 1 contract or more."),
    ("51008", Rejected(Funds), "Order failed. Insufficient {param0} balance in account"),
    ("51009", Rejected(Other), "Order blocked. Please contact customer support for assistance."),
    ("51010", InvalidRequest, "Request unsupported under current account mode"),
    ("51011", Unknown, "Order ID already exists."),
    ("51012", Rejected(Other), "Token doesn't exist."),
    ("51014", Rejected(Other), "Index doesn't exist."),
    ("51015", InvalidRequest, "Instrument ID doesn't match instrument type."),
    ("51016", Unknown, "Client order ID already exists."),
    ("51017", Rejected(Funds), "Loan amount exceeds borrowing limit."),
    ("51018", Rejected(Other), "Users with options accounts cannot hold net short positions."),
    ("51019", Rejected(Other), "No net long positions can be held under cross margin mode in options."),
    ("51020", Rejected(VenueRule), "Your order should meet or exceed the minimum order amount."),
    ("51021", Rejected(MarketClosed), "The pair or contract is not yet listed"),
    ("51022", Rejected(MarketClosed), "Contract suspended."),
    ("51023", Rejected(ReduceOnly), "Position doesn't exist."),
    ("51024", Rejected(Other), "Trading account is blocked."),
    ("51025", Rejected(Other), "Order count exceeds the limit."),
    ("51026", InvalidRequest, "Instrument type doesn't match underlying index."),
    ("51027", Rejected(MarketClosed), "Contract expired."),
    ("51028", Rejected(MarketClosed), "Contract under delivery."),
    ("51029", Rejected(MarketClosed), "Contract is being settled."),
    ("51030", Rejected(MarketClosed), "Funding fee is being settled."),
    ("51031", Rejected(Price), "This order price is not within the closing price range."),
    ("51032", Rejected(Other), "Closing all the positions at the market price."),
    ("51033", Rejected(VenueRule), "The total amount per order for this pair has reached the upper limit."),
    ("51034", Rejected(Other), "Fill rate exceeds the limit that you've set. Please reset the market maker protection to inactive for new trades."),
    ("51035", Rejected(Other), "This account doesn't have permission to submit MM quote order."),
    ("51036", Rejected(Other), "Only options instrument of the PM account supports MMP orders."),
    ("51037", Rejected(Other), "This account only supports placing IOC orders to reduce account risk."),
    ("51038", Rejected(Other), "IOC order already exists under the current risk module."),
    ("51039", Rejected(Other), "Leverage cannot be adjusted for the cross positions of Expiry Futures and Perpetual Futures under the PM account."),
    ("51040", Rejected(Other), "Cannot adjust margins for long isolated options positions"),
    ("51041", InvalidRequest, "Portfolio margin account only supports the Buy/Sell mode."),
    ("51042", Rejected(Other), "Under the Portfolio margin account, users can only place market maker protection orders in cross margin mode in Options."),
    ("51043", Rejected(Other), "This isolated position doesn't exist."),
    ("51044", Rejected(Other), "The order type {param0}, {param1} is not allowed to set stop loss and take profit"),
    ("51046", Rejected(Price), "The take profit trigger price must be higher than the order price"),
    ("51047", Rejected(Price), "The stop loss trigger price must be lower than the order price"),
    ("51048", Rejected(Price), "The take profit trigger price must be lower than the order price"),
    ("51049", Rejected(Price), "The stop loss trigger price must be higher than the order price"),
    ("51050", Rejected(Price), "The take profit trigger price must be higher than the best ask price"),
    ("51051", Rejected(Price), "The stop loss trigger price must be lower than the best ask price"),
    ("51052", Rejected(Price), "The take profit trigger price must be lower than the best bid price"),
    ("51053", Rejected(Price), "The stop loss trigger price must be higher than the best bid price"),
    ("51054", Unknown, "Request timed out. Please try again."),
    ("51055", Rejected(Other), "Futures Grid is not available in Portfolio Margin mode"),
    ("51056", Rejected(Other), "Action not allowed"),
    ("51057", Rejected(Other), "This bot isn’t available in current account mode. Switch mode in Settings > Account mode to continue."),
    ("51058", Rejected(Other), "No available position for this algo order"),
    ("51059", Rejected(Other), "Strategy for the current state does not support this operation"),
    ("51063", Rejected(NotFound), "OrdId does not exist"),
    ("51065", Rejected(Other), "algoClOrdId already exists."),
    ("51066", Rejected(Other), "Market orders unavailable for options trading. Place a limit order to close position."),
    ("51068", Rejected(Other), "{param0} already exists within algoClOrdId and attachAlgoClOrdId."),
    ("51069", Rejected(Other), "The option contracts related to current {param0} do not exist"),
    ("51070", Rejected(Other), "You do not meet the requirements for switching to this account mode. Please upgrade the account mode on the OKX website or App"),
    ("51071", Rejected(Other), "You've reached the maximum limit for tag level cancel all after timers."),
    ("51072", Rejected(Other), "As a spot lead trader, you need to set tdMode to spot_isolated when buying the configured lead trade pairs."),
    ("51073", Rejected(Other), "As a spot lead trader, you need to use '/copytrading/close-subposition' for selling assets through lead trades"),
    ("51074", Rejected(Other), "Only the tdMode for lead trade pairs configured by spot lead traders can be set to 'spot_isolated'"),
    ("51075", Rejected(Other), "Order modification failed. You can only modify the price of sell orders in spot copy trading."),
    ("51076", Rejected(Other), "TP/SL orders in Split TPs only support one-way TP/SL. You can't use slTriggerPx&slOrdPx and tpTriggerPx&tpOrdPx at the same time."),
    ("51077", Rejected(Other), "Setting multiple TP and cost-price SL orders isn’t supported for spot and margin trading."),
    ("51078", Rejected(Other), "You are a lead trader. Split TPs are not supported."),
    ("51079", Rejected(Other), "The number of TP orders with Split TPs attached in a same order cannot exceed {param0}"),
    ("51080", Rejected(Other), "Take-profit trigger price types (tpTriggerPxType) must be the same in an order with Split TPs attached"),
    ("51081", Rejected(Other), "Take-profit trigger prices (tpTriggerPx) cannot be the same in an order with Split TPs attached"),
    ("51082", Rejected(Other), "TP trigger prices (tpOrdPx) in one order with multiple TPs must be market prices."),
    ("51083", Rejected(Other), "The total size of TP orders with Split TPs attached in a same order should equal the size of this order"),
    ("51084", Rejected(Other), "The number of SL orders with Split TPs attached in a same order cannot exceed {param0}"),
    ("51085", Rejected(Other), "The number of TP orders cannot be less than 2 when cost-price SL is enabled (amendPxOnTriggerType set as 1) for Split TPs"),
    ("51086", Rejected(Other), "The number of orders with Split TPs attached in a same order cannot exceed {param0}"),
    ("51087", Rejected(MarketClosed), "Listing canceled for this crypto"),
    ("51088", Rejected(Other), "You can only place 1 TP/SL order to close an entire position"),
    ("51089", Rejected(Other), "The size of the TP order among split TPs attached cannot be empty"),
    ("51090", Rejected(Other), "You can't modify the amount of an SL order placed with a TP limit order."),
    ("51091", Rejected(Other), "All TP orders in one order must be of the same type."),
    ("51092", Rejected(Other), "TP order prices (tpOrdPx) in one order must be different."),
    ("51093", Rejected(Other), "TP limit order prices (tpOrdPx) in one order can't be -1 (market price)."),
    ("51094", Rejected(Other), "You can't place TP limit orders in spot, margin, or options trading."),
    ("51095", Rejected(Other), "To place TP limit orders at this endpoint, you must place an SL order at the same time."),
    ("51096", Rejected(Other), "cxlOnClosePos needs to be true to place a TP limit order"),
    ("51098", Rejected(Other), "You can't add a new TP order to an SL order placed with a TP limit order."),
    ("51099", Rejected(Other), "You can't place TP limit orders as a lead trader."),
    ("51100", Rejected(Other), "Unable to place order. Take profit/Stop loss conditions cannot be added to reduce-only orders."),
    ("51101", Rejected(VenueRule), "Order failed, the sz of the current order can’t be more than {param0} (contracts)."),
    ("51102", Rejected(Other), "Order failed, the number of pending orders for this instId can’t be more than {param0} (orders)"),
    ("51103", Rejected(Other), "Order failed, the number of pending orders across all instIds under the {param0} current instFamily can’t be more than {param1} (orders)"),
    ("51104", Rejected(Other), "Order failed, the aggregated contract quantity for all pending orders across all instIds under the {param0} current instFamily can’t be more than {param1} (contracts)"),
    ("51105", Rejected(Other), "Order failed, the maximal sum of position quantity and pending orders quantity with the same direction for current instId can’t be more than {param0} (contracts)"),
    ("51106", Rejected(Other), "Order failed, the maximal sum of position quantity and pending orders quantity with the same direction across all instIds under the {param0} current instFamily can’t be more than {param1} (contracts)"),
    ("51107", Rejected(Other), "Order failed, the maximal sum of position quantity and pending orders quantity in both directions across all instIds under the {param0} current instFamily can’t be more than {param1} (contracts)"),
    ("51108", Rejected(Other), "Positions exceed the limit for closing out with the market price."),
    ("51109", Rejected(Other), "No available offers."),
    ("51110", Rejected(MarketClosed), "You can only place a limit order after Call Auction has started."),
    ("51111", InvalidRequest, "Maximum {param0} orders can be placed in bulk."),
    ("51112", Rejected(ReduceOnly), "Close order size exceeds available size for this position."),
    ("51113", RateLimited, "Market-price liquidation requests too frequent."),
    ("51115", Rejected(Other), "Cancel all pending close-orders before liquidation."),
    ("51116", Rejected(Price), "Order price or trigger price exceeds {param0}."),
    ("51117", Rejected(Other), "Pending close-orders count exceeds limit."),
    ("51120", Rejected(VenueRule), "Order amount is less than {param0}, please try again."),
    ("51121", Rejected(VenueRule), "Order quantity must be a multiple of the lot size."),
    ("51122", Rejected(Price), "Order price should higher than the min price {param0}"),
    ("51123", Rejected(Other), "Min price increment is null."),
    ("51124", Rejected(MarketClosed), "You can only place limit orders during call auction."),
    ("51125", Rejected(ReduceOnly), "Currently there are pending reduce + reverse position orders in margin trading. Please cancel all pending reduce + reverse position orders and continue."),
    ("51126", Rejected(ReduceOnly), "Currently there are pending reduce only orders in margin trading. Please cancel all pending reduce only orders and continue."),
    ("51127", Rejected(Funds), "Available balance is 0."),
    ("51128", Rejected(Other), "Multi-currency margin accounts cannot do cross-margin trading."),
    ("51129", Rejected(Other), "The value of the position and buy order has reached the position limit. No further buying is allowed."),
    ("51130", Rejected(Other), "Fixed margin currency error."),
    ("51131", Rejected(Funds), "Insufficient balance."),
    ("51132", Rejected(Other), "Your position amount is negative and less than the minimum trading amount."),
    ("51133", Rejected(ReduceOnly), "Reduce-only feature is unavailable for spot transactions in multi-currency margin accounts."),
    ("51134", Rejected(ReduceOnly), "Closing failed. Please check your margin holdings and pending orders. Turn off the Reduce-only to continue."),
    ("51135", Rejected(Price), "Your closing price has triggered the limit price, and the max buy price is {param0}."),
    ("51136", Rejected(Price), "Your closing price has triggered the limit price, and the min sell price is {param0}."),
    ("51137", Rejected(Price), "The highest price limit for buy orders is {param0}."),
    ("51138", Rejected(Price), "The lowest price limit for sell orders is {param0}."),
    ("51139", Rejected(ReduceOnly), "Reduce-only feature is unavailable for the spot transactions by spot mode."),
    ("51140", Rejected(Other), "Purchase failed due to insufficient sell orders, please try later."),
    ("51142", Rejected(Other), "There is no valid quotation in the market, and the order cannot be filled in USDT mode, please try to switch to currency mode"),
    ("51143", Rejected(Funds), "Insufficient conversion amount"),
    ("51144", Rejected(Other), "Please use {param0} for closing."),
    ("51147", Rejected(Other), "To trade options, make sure you have more than 10,000 USD worth of assets in your trading account first, then activate options trading"),
    ("51148", Rejected(ReduceOnly), "Failed to place order. The new order may execute an opposite trading direction of your existing reduce-only positions. Cancel or edit pending orders to continue order"),
    ("51149", Unknown, "Order timed out. Please try again. (does not mean that the request was successful or failed, please check the request result)"),
    ("51150", Rejected(VenueRule), "The precision of the number of trades or the price exceeds the limit."),
    ("51152", Rejected(Other), "Unable to place an order that mixes automatic buy with automatic repayment or manual operation in Quick margin mode."),
    ("51153", Rejected(Other), "Unable to borrow manually in Quick margin mode. The amount you entered exceeds the upper limit."),
    ("51154", Rejected(Other), "Unable to repay manually in Quick margin mode. The amount you entered exceeds your available balance."),
    ("51155", Rejected(Other), "Trading of this pair or contract is restricted due to local compliance requirements."),
    ("51156", Rejected(Other), "You're leading trades in long/short mode and can't use this API endpoint to close positions"),
    ("51158", Rejected(Other), "Manual transfer unavailable. To proceed, please switch to Quick margin mode (isoMode = quick_margin)"),
    ("51159", Rejected(Other), "You're leading trades in buy/sell mode. If you want to place orders using this API endpoint, the orders must be in the same direction as your existing positions and open orders."),
    ("51162", Rejected(Other), "You have {instrument} open orders. Cancel these orders and try again"),
    ("51163", Rejected(Other), "You hold {instrument} positions. Close these positions and try again"),
    ("51164", Rejected(Other), "As lead trader, you can't switch to portfolio margin mode."),
    ("51165", Rejected(Other), "The number of {instrument} reduce-only orders reached the upper limit of {upLimit}. Cancel some orders to proceed."),
    ("51166", Rejected(Other), "Currently, we don't support leading trades with this instrument"),
    ("51167", Rejected(Other), "Failed. You have block trading open order(s), please proceed after canceling existing order(s)."),
    ("51168", Rejected(Other), "Failed. You have reduce-only type of open order(s), please proceed after canceling existing order(s)"),
    ("51169", Rejected(ReduceOnly), "Order failed because you don't have any positions in this direction for this contract to reduce or close."),
    ("51170", Rejected(ReduceOnly), "Failed to place order. A reduce-only order can’t be the same trading direction as your existing positions."),
    ("51171", Rejected(ReduceOnly), "Failed to edit order. The edited order may execute an opposite trading direction of your existing reduce-only positions. Cancel or edit pending orders to continue."),
    ("51173", Rejected(Other), "Unable to close all at market price. Your current positions don't have any liabilities."),
    ("51174", Rejected(Other), "Order failed, number of pending orders for {param0} exceed the limit of {param1}."),
    ("51175", Rejected(Other), "Parameters {param0} {param1} and {param2} cannot be empty at the same time"),
    ("51176", Rejected(Other), "Only one parameter can be filled among Parameters {param0} {param1} and {param2}"),
    ("51177", Rejected(Other), "Unavailable to amend {param1} because the price type of the current options order is {param0}"),
    ("51178", Rejected(Other), "tpTriggerPx&tpOrdPx or slTriggerPx&slOrdPx can't be empty when using attachAlgoClOrdId."),
    ("51179", Rejected(Other), "Unavailable to place options orders using {param0} in simple mode"),
    ("51180", Rejected(Other), "The range of {param0} should be ({param1}~{param2})"),
    ("51181", Rejected(Other), "ordType must be limit while placing {param0} orders"),
    ("51182", Rejected(Other), "The total number of pending orders under price types pxUsd and pxVol for the current account cannot exceed {param0}"),
    ("51185", Rejected(VenueRule), "The maximum value allowed per order is {maxOrderValue} USD"),
    ("51186", Rejected(Other), "Order failed. The leverage for {param0} in your current margin mode is {param1}x, which exceeds the platform limit of {param2}x."),
    ("51187", Rejected(Other), "Order failed. For {param0} {param1} in your current margin mode, the sum of your current order amount, position sizes, and open orders is {param2} contracts, which exceeds the platform limit of {param3} contracts. Reduce your order amount, cancel orders, or close positions."),
    ("51192", Rejected(Other), "The {param0} price corresponding to the IV level you entered is lower than the minimum allowed selling price of {param1} {param2}. Enter a higher IV level."),
    ("51193", Rejected(Other), "The {param0} price corresponding to the IV level you entered is higher than the maximum allowed buying price of {param1} {param2}. Enter a lower IV level."),
    ("51194", Rejected(Other), "The {param0} price corresponding to the USD price you entered is lower than the minimum allowed selling price of {param1} {param2}. Enter a higher USD price."),
    ("51195", Rejected(Other), "The {param0} price corresponding to the USD price you entered is higher than the maximum allowed buying price of {param1} {param2}. Enter a lower USD price."),
    ("51196", Rejected(MarketClosed), "You can only place limit orders during the pre-quote phase."),
    ("51197", Rejected(MarketClosed), "You can only place limit orders after the pre-quote phase begins."),
    ("51201", Rejected(VenueRule), "The value of a market order can't exceed {param0}."),
    ("51202", Rejected(VenueRule), "Market order amount exceeds the maximum amount."),
    ("51203", Rejected(VenueRule), "Order amount exceeds the limit {param0}."),
    ("51204", Rejected(Other), "The price for the limit order cannot be empty."),
    ("51205", Rejected(ReduceOnly), "Reduce Only is not available."),
    ("51206", Rejected(ReduceOnly), "Please cancel the Reduce Only order before placing the current {param0} order to avoid opening a reverse position."),
    ("51207", Rejected(Other), "Trading amount exceeds the limit, and can't be all closed at the market price. You can try closing the position manually in batches."),
    ("51209", Rejected(Other), "Order failed because your current status only allows placing delta-reducing orders. Increase your account equity or place an order that would reduce delta when filled to proceed."),
    ("51210", Rejected(Other), "Order failed because your current status only allows placing one order per underlying. Cancel other orders in the underlying to proceed."),
    ("51211", Rejected(Other), "Order failed because your current status only allows placing delta-reducing orders. Increase your account equity or place an order that would reduce delta when filled to proceed."),
    ("51212", Rejected(Other), "Order failed because your current status doesn’t support batch orders. Place a single order to proceed."),
    ("51213", Rejected(Other), "Order failed because your current status only allows placing one order per underlying. Cancel other orders in the underlying to proceed."),
    ("51214", Rejected(Other), "Failed to modify order because your current status only allows placing delta-reducing orders. Increase your account equity or place an order that would reduce delta when filled to proceed."),
    ("51220", Rejected(Other), "Lead and follow bots only support “Sell” or “Close all positions” when bot stops"),
    ("51221", Rejected(Other), "The profit-sharing ratio must be between 0% and 30%"),
    ("51222", Rejected(Other), "Profit sharing isn’t supported for this type of bot"),
    ("51223", Rejected(Other), "Only lead bot creators can set profit-sharing ratio"),
    ("51224", Rejected(Other), "Profit sharing isn’t supported for this crypto pair"),
    ("51225", Rejected(Other), "Instant trigger isn’t available for follow bots"),
    ("51226", Rejected(Other), "Editing parameters isn’t available for follow bots"),
    ("51250", Rejected(Price), "Algo order price is out of the available range."),
    ("51251", Rejected(Other), "Bot order type error occurred when placing iceberg order"),
    ("51252", Rejected(Other), "Algo order amount is out of the available range."),
    ("51253", Rejected(Other), "Average amount exceeds the limit of per iceberg order."),
    ("51254", Rejected(Other), "Iceberg average amount error occurred."),
    ("51255", Rejected(Other), "Limit of per iceberg order: Total amount/1000"),
    ("51256", Rejected(Other), "Iceberg order price variance error."),
    ("51257", Rejected(Other), "Trailing stop order callback rate error. The callback rate should be {min}< x<={max}%."),
    ("51258", Rejected(Price), "Trailing stop order placement failed. The trigger price of a sell order must be higher than the last transaction price."),
    ("51259", Rejected(Price), "Trailing stop order placement failed. The trigger price of a buy order must be lower than the last transaction price."),
    ("51260", Rejected(Other), "Maximum of {param0} pending trailing stop orders can be held at the same time."),
    ("51261", Rejected(Other), "Each user can hold up to {param0} pending stop orders at the same time."),
    ("51262", Rejected(Other), "Maximum {param0} pending iceberg orders can be held at the same time."),
    ("51263", Rejected(Other), "Maximum {param0} pending time-weighted orders can be held at the same time."),
    ("51264", Rejected(Other), "Average amount exceeds the limit of per time-weighted order."),
    ("51265", Rejected(Other), "Time-weighted order limit error."),
    ("51267", Rejected(Other), "Time-weighted order strategy initiative rate error."),
    ("51268", Rejected(Other), "Time-weighted order strategy initiative range error."),
    ("51269", Rejected(Other), "Time-weighted order interval error. Interval must be {%min}<= x<={%max}."),
    ("51270", Rejected(Other), "The limit of time-weighted order price variance is 0 < x <= 1%."),
    ("51271", Rejected(Other), "Sweep ratio must be 0 < x <= 100%."),
    ("51272", Rejected(Other), "Price variance must be 0 < x <= 1%."),
    ("51273", Rejected(Other), "Total amount must be greater than {param0}."),
    ("51274", Rejected(Other), "Total quantity of time-weighted order must be larger than single order limit."),
    ("51275", Rejected(Other), "The amount of single stop-market order cannot exceed the upper limit."),
    ("51276", Rejected(Other), "Prices cannot be specified for stop market orders."),
    ("51277", Rejected(Price), "TP trigger price cannot be higher than the last price."),
    ("51278", Rejected(Price), "SL trigger price cannot be lower than the last price."),
    ("51279", Rejected(Price), "TP trigger price cannot be lower than the last price."),
    ("51280", Rejected(Price), "SL trigger price cannot be higher than the last price."),
    ("51281", Rejected(Other), "Trigger order do not support the tgtCcy parameter."),
    ("51282", Rejected(Other), "The range of Price variance is {param0}~{param1}"),
    ("51283", Rejected(Other), "The range of Time interval is {param0}~{param1}"),
    ("51284", Rejected(Other), "The range of Average amount is {param0}~{param1}"),
    ("51285", Rejected(Other), "The range of Total amount is {param0}~{param1}"),
    ("51286", Rejected(Other), "The total amount should not be less than {param0}"),
    ("51287", Rejected(Other), "This bot doesn't support current instrument"),
    ("51288", Rejected(Other), "Bot is currently stopping. Do not make multiple attempts to stop."),
    ("51289", Rejected(Other), "Bot configuration does not exist. Please try again later"),
    ("51290", Rejected(Other), "The Bot engine is being upgraded. Please try again later"),
    ("51291", Rejected(Other), "This Bot does not exist or has been stopped"),
    ("51292", Rejected(Other), "This Bot type does not exist"),
    ("51293", Rejected(Other), "This Bot does not exist"),
    ("51294", Rejected(Other), "This Bot cannot be created temporarily. Please try again later"),
    ("51295", Rejected(Other), "Portfolio margin account does not support ordType {param0} in Trading bot mode"),
    ("51298", Rejected(Other), "Trigger orders are not available in the net mode of Expiry Futures and Perpetual Futures"),
    ("51299", Rejected(Other), "Order did not go through. You can hold a maximum of {param0} orders of this type."),
    ("51300", Rejected(Price), "TP trigger price cannot be higher than the mark price"),
    ("51302", Rejected(Price), "SL trigger price cannot be lower than the mark price"),
    ("51303", Rejected(Price), "TP trigger price cannot be lower than the mark price"),
    ("51304", Rejected(Price), "SL trigger price cannot be higher than the mark price"),
    ("51305", Rejected(Price), "TP trigger price cannot be higher than the index price"),
    ("51306", Rejected(Price), "SL trigger price cannot be lower than the index price"),
    ("51307", Rejected(Price), "TP trigger price cannot be lower than the index price"),
    ("51308", Rejected(Price), "SL trigger price cannot be higher than the index price"),
    ("51309", Rejected(Other), "Cannot create trading bot during call auction"),
    ("51310", Rejected(Other), "Strategic orders with Iceberg and TWAP order type are not supported when margins are self-transferred in isolated mode."),
    ("51311", Rejected(Other), "Failed to place trailing stop order. Callback rate should be within {min}<x<={max}"),
    ("51312", Rejected(Other), "Failed to place trailing stop order. Order amount should be within {min}<x<={max}"),
    ("51313", Rejected(Other), "Manual transfer in isolated mode does not support bot trading"),
    ("51317", Rejected(Other), "Trigger orders are not available by margin"),
    ("51320", Rejected(Other), "The range of coin percentage is {0}%-{1}%"),
    ("51321", Rejected(Other), "You're leading trades. Currently, we don't support leading trades with arbitrage, iceberg, or TWAP bots"),
    ("51322", Rejected(Other), "You're leading trades that have been filled at market price. We've canceled your open stop orders to close your positions"),
    ("51323", Rejected(Other), "You're already leading trades with take profit or stop loss settings. Cancel your existing stop orders to proceed"),
    ("51324", Rejected(Other), "As a lead trader, you hold positions in {instrument}. To close your positions, place orders in the amount that equals the available amount for closing"),
    ("51325", Rejected(Other), "As a lead trader, you must use market price when placing stop orders"),
    ("51326", Rejected(Other), "As a lead trader, you must use market price when placing orders with take profit or stop loss settings"),
    ("51327", Rejected(Other), "closeFraction is only available for Expiry Futures and Perpetual Futures"),
    ("51328", Rejected(Other), "closeFraction is only available for reduceOnly orders"),
    ("51329", Rejected(Other), "closeFraction is only available in NET mode"),
    ("51330", Rejected(Other), "closeFraction is only available for stop market orders"),
    ("51331", Rejected(Other), "closeFraction is only available for close position orders"),
    ("51332", Rejected(Other), "closeFraction is not applicable to Portfolio Margin"),
    ("51333", Rejected(Other), "Close position order in hedge-mode or reduce-only order in one-way mode cannot attach TPSL"),
    ("51340", Rejected(Other), "Used margin must be greater than {0}{1}"),
    ("51341", Rejected(Other), "Position closing not allowed"),
    ("51342", Rejected(Other), "Closing order already exists. Please try again later"),
    ("51343", Rejected(Other), "TP price must be less than the lower price"),
    ("51344", Rejected(Other), "SL price must be greater than the upper price"),
    ("51345", Rejected(Other), "Policy type is not grid policy"),
    ("51346", Rejected(Other), "The highest price cannot be lower than the lowest price"),
    ("51347", Rejected(Other), "No profit available"),
    ("51348", Rejected(Other), "Stop loss price must be less than the lower price in the range."),
    ("51349", Rejected(Other), "Take profit price must be greater than the highest price in the range."),
    ("51350", Rejected(Other), "No recommended parameters"),
    ("51351", Rejected(Other), "Single income must be greater than 0"),
    ("51352", Rejected(Other), "You can have {0} to {1} trading pairs"),
    ("51353", Rejected(Other), "Trading pair {0} already exists"),
    ("51354", Rejected(Other), "The percentages of all trading pairs should add up to 100%"),
    ("51355", Rejected(Other), "Select a date within {0} - {1}"),
    ("51356", Rejected(Other), "Select a time within {0} - {1}"),
    ("51357", Rejected(Other), "Select a time zone within {0} - {1}"),
    ("51358", Rejected(Other), "The investment amount of each crypto must be greater than {amount}"),
    ("51359", Rejected(Other), "Recurring buy not supported for the selected crypto {0}"),
    ("51370", Rejected(Other), "The range of lever is {0}~{1}"),
    ("51380", Rejected(Other), "Market conditions do not meet the strategy running configuration. You can try again later or adjust your tp/sl configuration."),
    ("51381", Rejected(Other), "Per grid profit ratio must be larger than 0.1% and less or equal to 10%"),
    ("51382", Rejected(Other), "Stop triggerAction is not supported by the current strategy"),
    ("51383", Rejected(Other), "The min_price is lower than the last price"),
    ("51384", Rejected(Other), "The trigger price must be greater than the min price"),
    ("51385", Rejected(Other), "The take profit price needs to be greater than the min price"),
    ("51386", Rejected(Other), "The min price needs to be greater than 1/2 of the last price"),
    ("51387", Rejected(Other), "Stop loss price must be less than the bottom price"),
    ("51388", Rejected(Other), "This Bot is in running status"),
    ("51389", Rejected(Other), "Trigger price should be lower than {0}"),
    ("51390", Rejected(Other), "Trigger price should be lower than the TP price"),
    ("51391", Rejected(Other), "Trigger price should be higher than the SL price"),
    ("51392", Rejected(Other), "TP price should be higher than the trigger price"),
    ("51393", Rejected(Other), "SL price should be lower than the trigger price"),
    ("51394", Rejected(Other), "Trigger price should be higher than the TP price"),
    ("51395", Rejected(Other), "Trigger price should be lower than the SL price"),
    ("51396", Rejected(Other), "TP price should be lower than the trigger price"),
    ("51397", Rejected(Other), "SL price should be higher than the trigger price"),
    ("51398", Rejected(Other), "Current market meets the stop condition. The bot cannot be created."),
    ("51399", Rejected(Other), "Max margin under current leverage: {amountLimit} {quoteCurrency}. Enter a smaller amount and try again."),
    ("51400", Rejected(Final), "Order cancellation failed as the order has been filled, canceled or does not exist."),
    ("51401", Rejected(Final), "Cancellation failed as the order is already canceled. (Only applicable to Nitro Spread)"),
    ("51402", Rejected(Final), "Cancellation failed as the order is already completed. (Only applicable to Nitro Spread)"),
    ("51403", Rejected(Other), "Cancellation failed as the order type doesn't support cancellation."),
    ("51404", Rejected(MarketClosed), "Order cancellation unavailable during the second phase of call auction."),
    ("51405", Rejected(Other), "Cancellation failed as you don't have any pending orders."),
    ("51406", InvalidRequest, "Canceled - order count exceeds the limit {param0}."),
    ("51407", InvalidRequest, "Either order ID or client order ID is required."),
    ("51408", Rejected(Other), "Pair ID or name doesn't match the order info."),
    ("51409", InvalidRequest, "Either pair ID or pair name ID is required."),
    ("51410", Rejected(Final), "Cancellation failed as the order is already in canceling status or pending settlement."),
    ("51411", Permission, "Account does not have permission for mass cancellation."),
    ("51412", Unknown, "Cancellation timed out, please try again later."),
    ("51413", Rejected(Other), "Cancellation failed as the order type is not supported by endpoint."),
    ("51415", Rejected(Other), "Unable to place order. Spot trading only supports using the last price as trigger price. Please select \"Last\" and try again."),
    ("51416", Rejected(Other), "Order has been triggered and can't be canceled"),
    ("51500", InvalidRequest, "You must enter a price, quantity, or TP/SL"),
    ("51501", InvalidRequest, "Maximum {param0} orders can be modified."),
    ("51502", Rejected(Funds), "Unable to edit order: insufficient balance or margin."),
    ("51503", Rejected(Final), "Your order has already been filled or canceled."),
    ("51505", Rejected(Other), "{instId} is not in call auction"),
    ("51506", Rejected(Other), "Order modification unavailable for the order type."),
    ("51507", Rejected(MarketClosed), "You can only place market orders for this crypto at least 5 minutes after its listing."),
    ("51508", Rejected(MarketClosed), "Orders are not allowed to be modified during the call auction."),
    ("51509", Rejected(Final), "Modification failed as the order has been canceled. (Only applicable to Nitro Spread)"),
    ("51510", Rejected(Final), "Modification failed as the order has been completed. (Only applicable to Nitro Spread)"),
    ("51511", Rejected(PostOnly), "Modification failed as the order price did not meet the requirement for Post Only."),
    ("51512", InvalidRequest, "Failed to amend orders in batches. You cannot have duplicate orders in the same amend-batch-orders request."),
    ("51513", RateLimited, "Number of modification requests that are currently in progress for an order cannot exceed 3 times."),
    ("51514", InvalidRequest, "Order modification failed. The price length must be 32 characters or shorter."),
    ("51521", Rejected(ReduceOnly), "Failed to edit. Unable to edit reduce-only order because you don't have any positions of this contract."),
    ("51522", Rejected(ReduceOnly), "Failed to edit. A reduce-only order can't be in the same trading direction as your existing positions."),
    ("51523", Rejected(Other), "Unable to modify the order price of a stop order that closes an entire position. Please modify the trigger price instead."),
    ("51524", Rejected(Other), "Unable to modify the order quantity of a stop order that closes an entire position. Please modify the trigger price instead."),
    ("51525", Rejected(Other), "Stop order modification is not available for quick margin"),
    ("51526", Rejected(Other), "Order modification unsuccessful. Take profit/Stop loss conditions cannot be added to or removed from stop orders."),
    ("51527", Rejected(Other), "Order modification failed. At least 1 of the attached TP/SL orders does not exist."),
    ("51528", Rejected(Other), "Unable to modify trigger price type"),
    ("51529", Rejected(Other), "Order modification unsuccessful. Stop order modification only applies to Expiry Futures and Perpetual Futures."),
    ("51530", Rejected(Other), "Order modification unsuccessful. Take profit/Stop loss conditions cannot be added to or removed from reduce-only orders."),
    ("51531", Rejected(Other), "Order modification unsuccessful. The stop order must have either take profit or stop loss attached."),
    ("51532", Rejected(Other), "Your TP/SL can't be modified because it was partially triggered"),
    ("51536", Rejected(Other), "Unable to modify the size of the options order if the price type is pxUsd or pxVol."),
    ("51537", Rejected(Other), "pxUsd or pxVol are not supported by non-options instruments"),
    ("51538", Rejected(Other), "You need to use attachAlgoOrds if you used attachAlgoOrds when placing an order. attachAlgoOrds is not supported if you did not use attachAlgoOrds when placing this order."),
    ("51539", Rejected(Other), "attachAlgoId or attachAlgoClOrdId cannot be identical when modifying any TP/SL within your split TPs order"),
    ("51543", Rejected(Other), "When modifying take-profit or stop-loss orders for spot or margin trading, you can only adjust the price and quantity. Cancel the order and place a new one for other actions."),
    ("51600", InvalidRequest, "Status not found."),
    ("51601", InvalidRequest, "Order status and order id cannot exist at the same time."),
    ("51602", InvalidRequest, "Either order status or order ID is required."),
    ("51603", Rejected(NotFound), "Order does not exist."),
    ("51604", Rejected(Other), "Initiate a download request before obtaining the hyperlink"),
    ("51605", Rejected(Other), "You can only download transaction data from the past 2 years"),
    ("51606", Rejected(Other), "Transaction data for the current quarter is not available"),
    ("51607", Rejected(Other), "Your previous download request is still being processed"),
    ("51608", Rejected(Other), "No transaction data found for the current quarter"),
    ("51610", Rejected(Other), "You can't download billing statements for the current quarter."),
    ("51611", Rejected(Other), "You can't download billing statements for the current quarter."),
    ("51620", Rejected(Other), "Only affiliates can perform this action"),
    ("51621", Rejected(Other), "The user isn’t your invitee"),
    ("51625", Rejected(Other), "One-click repay requires auto-borrow to be enabled. Please enable auto-borrow, or repay the debt currency via Easy Convert or spot trading."),
    ("51784", Unknown, "Client order ID is being processed"),
    ("51820", Rejected(Other), "Request failed"),
    ("51821", Rejected(Other), "The payment method is not supported"),
    ("51822", Rejected(Other), "Quote expired"),
    ("51823", Rejected(Other), "Parameter {param} of buy/sell trading is inconsistent with the quotation"),
    ("51824", Rejected(Other), "You must bind a phone number to your account before using this feature"),
    ("51825", Rejected(Other), "You must bind an email to your account before using this feature"),
    ("52000", Rejected(Other), "No market data found."),
    ("54000", Rejected(Other), "Margin trading is not supported."),
    ("54001", Rejected(Other), "Only Multi-currency margin account can be set to borrow coins automatically."),
    ("54004", Rejected(Other), "Order placement or modification failed because one of the orders in the batch failed."),
    ("54005", Rejected(Other), "Switch to isolated margin mode to trade pre-market expiry futures."),
    ("54006", Rejected(Other), "Pre-market expiry future position limit is {posLimit}. Please cancel order or close position"),
    ("54007", Rejected(Other), "Instrument {instId} is not supported"),
    ("54008", Rejected(Other), "This operation is disabled by the 'mass cancel order' endpoint. Please enable it using this endpoint."),
    ("54009", InvalidRequest, "The range of {param0} should be [{param1}, {param2}]."),
    ("54011", Rejected(ReduceOnly), "Pre-market trading contracts are only allowed to reduce the number of positions within 1 hour before delivery. Please modify or cancel the order."),
    ("54012", Rejected(Other), "Due to insufficient order book depth, we are now taking measures to protect your positions. Currently, you can only cancel orders, add margin to your positions, and place post-only orders. Your positions will not be liquidated. Trade will resume once order book depth returns to a safe level."),
    ("54018", Rejected(VenueRule), "Buy limit of {param0} USD exceeded. Your remaining limit is {param1} USD. (During the call auction)"),
    ("54019", Rejected(VenueRule), "Buy limit of {param0} USD exceeded. Your remaining limit is {param1} USD. (After the call auction)"),
    ("54024", Rejected(Other), "Your order failed because you must enable {ccy} as collateral to trade expiry futures, perpetual futures, and options in cross-margin mode."),
    ("54025", Rejected(Other), "Your order failed because you must enable {ccy} as collateral to trade margin, expiry futures, perpetual futures, and options in isolated margin mode."),
    ("54026", Rejected(Other), "Your order failed because you must enable {ccy} and {ccy1} as collateral to trade the margin pair in isolated margin mode."),
    ("54027", Rejected(Other), "Your order failed because you must enable {ccy} as collateral to trade options."),
    ("54028", Rejected(Other), "Your order failed because you must enable {ccy} as collateral to trade spot in isolated margin mode."),
    ("54029", InvalidRequest, "{param0} doesn't exist within {param1}."),
    ("54030", Rejected(Other), "Order failed. Your total value of same-direction {param0} open positions and orders can't exceed {param1} USD or {param2} of the platform's open interest."),
    ("54031", Rejected(Other), "Order failed. The open interest of {param0} has reached the platform open interest limit. Positions opening orders cannot be placed. Please try again later."),
    ("54035", Rejected(ReduceOnly), "Order failed. The platform has reached the collateral limit for this crypto, so you can only place reduce-only orders."),
    ("54036", Rejected(Other), "You can't place fill or kill orders when self-trade prevention is set to both maker and taker orders."),
    ("54039", Rejected(Other), "RPI orders can't be reduce-only orders."),
    ("54040", Rejected(Other), "RPI orders can't be used with TP/SL settings."),
    ("54041", Rejected(Other), "RPI orders aren't supported for {param0}."),
    ("54042", Rejected(Other), "You don't have permission to place RPI orders for {param0}."),
    ("54043", Rejected(Other), "You can only place up to {param1} RPI orders for {param0}. Cancel some of your orders and try again."),
    ("54044", Rejected(Other), "RPI is not enabled for {param0}. You can't place orders that take RPI liquidity of it."),
    ("54045", Rejected(Other), "OpenAPI users can only place IOC orders that take ELP liquidity. (Deprecated)"),
    ("54046", Rejected(Other), "You can't place orders to take RPI liquidity."),
    ("54047", RateLimited, "You can't amend this order because an order with the same order ID or client order ID is in speed bump."),
    ("54048", RateLimited, "You can't cancel the order because an order with the same order ID or client order ID is in speed bump."),
    ("54049", RateLimited, "API users can't place orders that take RPI liquidity now because system is busy. To proceed, set rpiTakerAccess:false."),
    ("54051", Rejected(Other), "RPI order rejected. The order value is below the minimum required for RPI orders ({param0} USD)."),
    ("54070", Rejected(Other), "The current function is not supported. Please update to the latest app version if using the app, or use the attachAlgoOrds array to place orders via Open API."),
    ("54071", Rejected(Other), "Due to the platform system upgrade, this order no longer supports modifications. It is recommended to cancel and place a new order."),
    ("54072", Rejected(MarketClosed), "This contract is currently view-only and not tradable."),
    ("54073", Rejected(Other), "Couldn't place order, as {param0} is at risk of depegging. Switch settlement currencies and try again."),
    ("54074", Rejected(Other), "Your settings failed as you have positions, bot or open orders for USD contracts."),
    ("54075", Rejected(Other), "Cross-crypto contracts are currently not supported in this account mode."),
    ("54078", Rejected(Other), "If the main order is a buy order, the TP ratio must be greater than 0, while the SL ratio must be between -1 and 0. If the main order is a sell order, the TP ratio must be between -1 and 0, and the SL ratio must be greater than 0. Additionally, the ratio must be multiples of 0.0001."),
    ("54079", Rejected(Other), "Dynamic change is available only for futures trading in futures mode or multi-currency mode. Note that when selecting dynamic change, the trigger price can only be calculated using the last price."),
    ("54092", Rejected(Other), "Action Required: Please accept the TradFi Perps disclaimer on Web or App by attempting to place a TradFi Perp trade via the frontend. Each account, including sub-accounts, must separately accept the disclaimer before API trading is enabled"),
    ("54094", Rejected(Other), "Order rejected. The cool-off period is active for the current instId."),
    ("54109", Rejected(Other), "You haven't activated trading for this pair. Log in to the OKX App or website and click \"Trade\" on this pair's trading page to activate it, or activate it via the designated API endpoint, then try again."),
];

/// The endpoint ids whose answer is a list of per-order results (read row by row).
pub(super) const BATCH_ENDPOINTS: &[&str] = &["cancel_batch", "cancel_algos_batch"];

/// The lookups of one order: "Instrument ID ... doesn't exist" (`51001`, an expiry future
/// delivered and delisted) is that the venue no longer knows the order there.
const LOOKUPS: &[&str] = &["order_get"];

/// The algo order list of one `ordType`, read beside the plain orders.
pub(super) const ALGO_LIST: &str = "orders_algo_pending";

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: serde_json::Value,
}

/// One order's result inside `data` (place, cancel, amend): `sCode` "0" is success.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct Item {
    #[serde(default)]
    pub s_code: Option<String>,
    #[serde(default)]
    pub s_msg: String,
    #[serde(default)]
    pub sub_code: String,
}

fn row(code: &str) -> Option<&'static ErrorRow> {
    let base = code.split('_').next().unwrap_or(code);
    ERRORS.iter().find(|(c, _, _)| *c == base)
}

/// The error a code (and its sub-code, where OKX gives one) means, in OKX's words.
pub(super) fn of(code: &str, sub: &str, msg: &str) -> VenueError {
    let shown = if sub.is_empty() { code.to_string() } else { format!("{code}_{sub}") };
    match row(code) {
        Some((_, Unknown, _)) => VenueError::new(Unknown, Some(shown.clone()), format!("OKX did not say whether it went through: {msg} (code {shown})")),
        Some((_, canon, _)) => VenueError::new(*canon, Some(shown.clone()), format!("OKX refused it: {msg} (code {shown})")),
        None => VenueError::unmapped(Some(shown.clone()), format!("OKX answered code {shown} ({msg}), which OpenTraderWorld does not map")),
    }
}

/// The per-order results an answer carries in `data` (empty for a list answer).
pub(super) fn items(data: &serde_json::Value) -> Vec<Item> {
    data.as_array()
        .map(|rows| rows.iter().filter(|r| r.get("sCode").is_some()).filter_map(|r| serde_json::from_value::<Item>(r.clone()).ok()).collect())
        .unwrap_or_default()
}

/// The error an order call's answer carries, mapped; `None` for a success.
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        let why = http::timeout(frame, "okx").map(|e| e.message).unwrap_or_else(|| "okx: no answer".into());
        return Some(VenueError::new(Unknown, None, why));
    };
    let Some(env) = serde_json::from_slice::<Envelope>(body).ok().filter(|e| e.code.is_some()) else {
        // No envelope: the HTTP status alone. A 2xx without one is left to the decoder, which
        // then fails to read it (an answer nobody mapped).
        let e = http::classify(frame, "okx")?;
        let canon = match e.kind {
            ErrorKind::RateLimited => RateLimited,
            ErrorKind::Auth => Auth,
            ErrorKind::Permission => Permission,
            ErrorKind::InvalidRequest | ErrorKind::NotFound => InvalidRequest,
            // A 5xx: the venue may have acted.
            _ => Unknown,
        };
        let mut v = VenueError::new(canon, e.provider_code.clone(), e.message);
        v.retry_after_ms = e.retry_after_ms;
        return Some(v);
    };
    let code = env.code.as_deref().unwrap_or_default();
    // The instrument list asked by id: "Instrument ID ... doesn't exist" is that it lists none.
    if *endpoint_id == "instruments" && code == "51001" {
        return None;
    }
    let rows = items(&env.data);
    if BATCH_ENDPOINTS.contains(endpoint_id) && !rows.is_empty() {
        return None;
    }
    let mut e = match rows.iter().find(|i| i.s_code.as_deref().is_some_and(|c| c != "0")) {
        Some(i) => of(i.s_code.as_deref().unwrap_or_default(), &i.sub_code, &i.s_msg),
        None if code == "0" => return None,
        None if LOOKUPS.contains(endpoint_id) && code == "51001" => {
            return Some(VenueError::new(Rejected(NotFound), Some(code.to_string()), format!("OKX no longer lists the order's instrument: {} (code {code})", env.msg)));
        }
        None => of(code, "", &env.msg),
    };
    // One algo order type the venue refuses to list (`chase` outside FUTURES and SWAP, a type
    // the account cannot hold): that list is empty, the others are still read. A rate limit, a
    // key refusal or a lost answer still fails the read.
    if *endpoint_id == ALGO_LIST && matches!(e.canon, InvalidRequest | Permission | Rejected(_)) {
        tracing::warn!("okx: the algo order list was refused ({}), read as empty", e.message);
        return None;
    }
    if e.canon == RateLimited {
        // The doc states no reset time: a `Retry-After` is read where the answer carries one.
        e.retry_after_ms = http::retry_after_ms(frame);
    }
    Some(e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    /// PROVIDER.md's error table lists the same codes with the same kinds.
    #[test]
    fn the_table_matches_provider_md() {
        let doc = include_str!("PROVIDER.md");
        let start = doc.find("### Errors").unwrap();
        let table = &doc[start..start + doc[start..].find("### Rate limits for orders").unwrap()];
        let mut seen: Vec<(String, String)> = Vec::new();
        for line in table.lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let canon = cells[3];
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
    fn no_code_is_listed_twice() {
        let mut codes: Vec<&str> = ERRORS.iter().map(|r| r.0).collect();
        codes.sort_unstable();
        let n = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), n);
    }

    #[test]
    fn the_item_code_decides_and_a_lost_answer_is_unknown() {
        // Place-order with the item refused: envelope `1`, the item's code is the one read.
        let refused = r#"{"code":"1","msg":"Operation failed.","data":[{"clOrdId":"k1","ordId":"","sCode":"51008","sMsg":"Order failed. Insufficient USDT balance in account","subCode":"1000","tag":"","ts":"1695190491421"}],"inTime":"1","outTime":"2"}"#;
        let e = decode(&http_frame(1, "order", 200, refused)).unwrap();
        assert_eq!((e.canon, e.code.as_deref()), (Rejected(Funds), Some("51008_1000")));
        assert!(e.message.starts_with("OKX refused it: Order failed. Insufficient USDT balance in account (code 51008_1000)"), "{}", e.message);
        // The envelope alone.
        let e = decode(&http_frame(1, "order", 401, r#"{"code":"50113","msg":"Invalid signature","data":[]}"#)).unwrap();
        assert_eq!(e.canon, Auth);
        let e = decode(&http_frame(1, "order", 200, r#"{"code":"1","msg":"Operation failed.","data":[{"sCode":"51149","sMsg":"Order timed out. Please try again.","subCode":""}]}"#)).unwrap();
        assert_eq!(e.canon, Unknown);
        assert!(e.mapped);
        // A 5xx, a timeout: the order may have gone through.
        assert_eq!(decode(&http_frame(1, "order", 502, "<html>")).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "order" };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
        // A code nobody mapped alerts.
        let e = decode(&http_frame(1, "order", 200, r#"{"code":"1","msg":"","data":[{"sCode":"59999","sMsg":"new","subCode":""}]}"#)).unwrap();
        assert!(!e.mapped && e.canon == Unknown && e.message.contains("does not map"), "{e:?}");
        // Success.
        assert!(decode(&http_frame(1, "order", 200, r#"{"code":"0","msg":"","data":[{"sCode":"0","sMsg":"","ordId":"1"}]}"#)).is_none());
    }

    #[test]
    fn a_batch_answer_is_read_row_by_row_and_an_unlisted_instrument_is_an_empty_list() {
        let partial = r#"{"code":"2","msg":"Bulk operation partially succeeded.","data":[{"ordId":"1","sCode":"0","sMsg":""},{"ordId":"2","sCode":"51400","sMsg":"Order cancellation failed"}]}"#;
        assert!(decode(&http_frame(1, "cancel_batch", 200, partial)).is_none());
        assert!(decode(&http_frame(1, "instruments", 200, r#"{"code":"51001","msg":"Instrument ID doesn't exist","data":[]}"#)).is_none());
    }

    #[test]
    fn a_rate_limit_carries_the_wait_only_when_the_answer_states_one() {
        let e = decode(&http_frame(1, "order", 429, r#"{"code":"50011","msg":"Too Many Requests"}"#)).unwrap();
        assert_eq!((e.canon, e.retry_after_ms), (RateLimited, None));
    }
}
