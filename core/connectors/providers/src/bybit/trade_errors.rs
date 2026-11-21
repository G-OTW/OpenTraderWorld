//! Bybit's order error table (onboarding spec 8), from the v5 error page
//! (https://bybit-exchange.github.io/docs/v5/error, read 2026-11-11): every code of its UTA and
//! Spot Trade sections that an order call (place, cancel, cancel all, look up, pre-check) can
//! answer, mapped to one canonical kind. A code not listed here is `Unknown` and alerts.
//! Mirrored in PROVIDER.md ("Trading", "Errors"); a test keeps the two in step.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, Price, VenueRule};

/// How long Bybit bans an IP over its limit (rate-limit page: "wait for at least 10 minutes").
const IP_BAN_MS: u64 = 600_000;

/// `(code, canonical, the doc's words)`.
pub(super) const ERRORS: &[ErrorRow] = &[
    // HTTP-level codes in the envelope, and the request itself.
    ("-1", Auth, "request expired"),
    ("429", RateLimited, "high server load, retry"),
    ("10000", Unknown, "server timeout"),
    ("10001", InvalidRequest, "request parameter error"),
    ("10002", Auth, "request time outside the window"),
    ("10003", Auth, "API key is invalid (wrong environment)"),
    ("10004", Auth, "error sign"),
    ("10005", Permission, "API key permission denied"),
    ("10006", RateLimited, "too many visits (API rate limit)"),
    ("10007", Auth, "user authentication failed"),
    ("10008", Permission, "common banned, check the account mode"),
    ("10009", Permission, "region restricted"),
    ("10010", Auth, "unmatched IP"),
    ("10014", Unknown, "invalid duplicate request"),
    ("10016", Unknown, "server error"),
    ("10017", InvalidRequest, "route not found"),
    ("10018", RateLimited, "IP rate limit (deprecated)"),
    ("10024", Permission, "compliance rules triggered"),
    ("10027", Permission, "transactions are banned"),
    ("10028", Permission, "unified account users only"),
    ("10029", InvalidRequest, "symbol not on the whitelist"),
    ("-2015", Auth, "(spot) API key expired"),
    ("33004", Auth, "(derivatives) API key expired"),
    ("100028", Permission, "not for unified account users"),
    ("30133", Rejected(Other), "OTC loan: USDT perpetual symbol not allowed"),
    ("30134", Rejected(Other), "OTC loan: USDC contract symbol not allowed"),
    ("30135", Rejected(Other), "OTC loan: USDT perpetual leverage above the maximum"),
    ("30136", Rejected(Other), "OTC loan: USDC leverage above the maximum"),
    ("30208", Rejected(Price), "price above the maximum buying price"),
    ("30209", Rejected(Price), "price below the minimum selling price"),
    ("30228", Rejected(MarketClosed), "no new positions during delisting"),
    ("30256", Rejected(Funds), "margin limit exceeded (spot)"),
    ("40004", Unknown, "order modified while being replaced"),
    // Derivatives orders.
    ("110001", Rejected(NotFound), "order does not exist"),
    ("110003", Rejected(Price), "price outside the allowable range"),
    ("110004", Rejected(Funds), "wallet balance insufficient"),
    ("110005", Rejected(Other), "position status error"),
    ("110006", Rejected(Funds), "assets cannot cover the position margin"),
    ("110007", Rejected(Funds), "available balance insufficient"),
    ("110008", Rejected(Final), "order already completed or cancelled"),
    ("110009", Rejected(Other), "too many stop orders"),
    ("110010", Rejected(Final), "order already cancelled"),
    ("110011", Rejected(Funds), "liquidation would be triggered"),
    ("110012", Rejected(Funds), "insufficient available balance"),
    ("110017", Rejected(VenueRule), "quantity truncated to zero"),
    ("110018", InvalidRequest, "user id is illegal"),
    ("110019", InvalidRequest, "order id is illegal"),
    ("110020", Rejected(Other), "more than 500 active orders"),
    ("110021", Rejected(Other), "open interest position limit"),
    ("110022", Rejected(Other), "quantity restricted, cannot increase"),
    ("110023", Rejected(MarketClosed), "reduce only on this contract"),
    ("110029", InvalidRequest, "hedge mode not supported for this symbol"),
    ("110032", InvalidRequest, "order is illegal"),
    ("110035", Unknown, "cancellation not completed before liquidation"),
    ("110039", Rejected(Funds), "maintenance margin rate too high"),
    ("110040", Rejected(Funds), "order would trigger liquidation"),
    ("110042", Rejected(MarketClosed), "pre-delivery: reduce only"),
    ("110044", Rejected(Funds), "available margin insufficient"),
    ("110045", Rejected(Funds), "wallet balance insufficient"),
    ("110051", Rejected(Funds), "balance cannot cover the lowest market price"),
    ("110052", Rejected(Funds), "balance insufficient to set the price"),
    ("110053", Rejected(Funds), "balance cannot cover market price and upper limit"),
    ("110057", InvalidRequest, "conditional or limit order with TP/SL params"),
    ("110063", Rejected(MarketClosed), "settlement in progress"),
    ("110066", Rejected(MarketClosed), "trading not allowed now"),
    ("110067", Permission, "unified account not supported"),
    ("110068", Permission, "leveraged trading not allowed"),
    ("110069", Permission, "institutional lending customer cannot trade"),
    ("110070", Rejected(MarketClosed), "ETP symbols cannot be traded"),
    ("110071", Permission, "unified account revamp: upgrades not supported"),
    ("110072", Unknown, "orderLinkId is duplicate"),
    ("110074", Rejected(MarketClosed), "contract is not live"),
    ("110079", Unknown, "order is processing, try again later"),
    ("110080", Rejected(Other), "institutional lending LTV at liquidation"),
    ("110085", Rejected(Other), "risk limit updated, place again"),
    ("110086", Rejected(Other), "leverage above the risk limit tier"),
    ("110087", Rejected(Other), "leverage above the institutional loan maximum"),
    ("110088", Permission, "upgrade to UTA to trade"),
    ("110089", Rejected(Other), "exceeds the maximum risk limit level"),
    ("110090", Rejected(Other), "position may exceed the max limit"),
    ("110092", Rejected(Price), "trigger expects rising but is at or below the price"),
    ("110093", Rejected(Price), "trigger expects falling but is at or above the price"),
    ("110094", Rejected(VenueRule), "notional below the lower limit"),
    ("110095", Rejected(MarketClosed), "pre-market call auction"),
    ("110096", Permission, "pre-market not in portfolio margin"),
    ("110097", Permission, "pre-market needs UTA"),
    ("110098", Rejected(MarketClosed), "only GTC during the call auction"),
    ("110100", Rejected(MarketClosed), "pre-market not in demo trading"),
    ("110101", Rejected(Funds), "settlement asset not enabled as collateral"),
    ("110102", Permission, "inverse contracts not supported for this user"),
    ("110103", Rejected(MarketClosed), "only post-only orders at this stage"),
    ("110104", Rejected(Other), "institutional loan LTV over the limit"),
    ("110105", Rejected(Other), "institutional loan LTV over the limit"),
    ("110106", Rejected(Other), "institutional loan whitelist"),
    ("110107", Rejected(Other), "institutional loan leverage limit"),
    ("110108", InvalidRequest, "slippage tick size out of range"),
    ("110109", InvalidRequest, "slippage percent out of range"),
    ("110118", Rejected(Other), "no market price, low liquidity"),
    ("110119", InvalidRequest, "RPI orders for approved market makers only"),
    ("110120", Rejected(Price), "price under the price limitation"),
    ("110121", Rejected(Price), "price over the price limitation"),
    ("110123", Permission, "agree to the trading terms"),
    ("110124", Permission, "cooling-off period"),
    ("110125", Permission, "agree to the trading terms for this contract"),
    ("110131", Rejected(Funds), "margin limit exceeded (perps)"),
    ("110132", Permission, "product not available in this region"),
    ("110134", Rejected(MarketClosed), "trading on this symbol is suspended"),
    ("110135", Rejected(Other), "event futures position under portfolio margin"),
    ("110136", Permission, "event futures for whitelisted accounts"),
    ("110137", Rejected(MarketClosed), "event futures reduce-only window"),
    ("170346", Rejected(Funds), "settle coin is not a collateral coin"),
    ("3100197", Permission, "temporarily banned during the UTA upgrade"),
    ("3200306", Rejected(Funds), "maintenance margin utilization over the limit"),
    ("3200403", Rejected(Other), "isolated margin cannot create order"),
    ("3400139", Rejected(Other), "positions and orders over the risk limit"),
    // Spot orders.
    ("170001", Unknown, "internal error"),
    ("170005", RateLimited, "too many new orders"),
    ("170007", Unknown, "timeout waiting for the backend"),
    ("170010", Rejected(Other), "leveraged token position limit"),
    ("170011", Rejected(Other), "innovation token position limit"),
    ("170031", Rejected(MarketClosed), "feature suspended"),
    ("170032", Unknown, "network error"),
    ("170033", Rejected(Funds), "margin: insufficient balance"),
    ("170034", Rejected(Funds), "spot leverage liability overflow"),
    ("170035", Unknown, "submitted to the system for processing"),
    ("170036", Permission, "cross margin trading not enabled"),
    ("170037", Rejected(Other), "cross margin not supported for this coin"),
    ("170105", InvalidRequest, "parameter was empty"),
    ("170115", InvalidRequest, "invalid timeInForce"),
    ("170116", InvalidRequest, "invalid orderType"),
    ("170117", InvalidRequest, "invalid side"),
    ("170121", InvalidRequest, "invalid symbol"),
    ("170124", Rejected(VenueRule), "order amount too large"),
    ("170130", InvalidRequest, "data sent for a parameter is not valid"),
    ("170131", Rejected(Funds), "balance insufficient"),
    ("170132", Rejected(Price), "order price too high"),
    ("170133", Rejected(Price), "order price lower than the minimum"),
    ("170134", Rejected(VenueRule), "order price decimal too long"),
    ("170136", Rejected(VenueRule), "order quantity lower than the minimum"),
    ("170137", Rejected(VenueRule), "order volume decimal too long"),
    ("170139", Rejected(Final), "order has been filled"),
    ("170140", Rejected(VenueRule), "order value under the lower limit"),
    ("170141", Unknown, "duplicate clientOrderId"),
    ("170142", Rejected(Final), "order has been cancelled"),
    ("170143", Rejected(NotFound), "cannot be found on the order book"),
    ("170144", Rejected(Other), "order has been locked"),
    ("170145", InvalidRequest, "this order type cannot be cancelled"),
    ("170146", Unknown, "order creation timeout"),
    ("170147", Unknown, "order cancellation timeout"),
    ("170148", Rejected(VenueRule), "market order amount decimal too long"),
    ("170149", Rejected(Other), "create order failed"),
    ("170150", Rejected(Other), "cancel order failed"),
    ("170151", Rejected(MarketClosed), "trading pair not open yet"),
    ("170157", Rejected(MarketClosed), "pair not available for API trading"),
    ("170159", Rejected(MarketClosed), "no market order in a new pair's first minutes"),
    ("170190", Rejected(Final), "cancel already finished"),
    ("170191", Rejected(Other), "cannot cancel now, try again later"),
    ("170192", Rejected(Price), "price above the limit"),
    ("170193", Rejected(Price), "buy price above the limit"),
    ("170194", Rejected(Price), "sell price below the limit"),
    // "Please note that your order may not be filled": the doc does not say it was refused.
    ("170195", Unknown, "ETP buy order may not be filled"),
    ("170196", Unknown, "ETP sell order may not be filled"),
    ("170197", Rejected(VenueRule), "buy quantity too large for the market"),
    ("170198", Rejected(VenueRule), "sell quantity too large for the market"),
    ("170199", Rejected(VenueRule), "buy quantity too large for the nav"),
    ("170200", Rejected(VenueRule), "sell quantity too large for the nav"),
    ("170201", InvalidRequest, "invalid orderFilter"),
    ("170202", InvalidRequest, "enter the TP/SL price"),
    ("170203", Rejected(Price), "trigger price above 110%"),
    ("170204", Rejected(Price), "trigger price below 90%"),
    ("170206", Rejected(MarketClosed), "no stop-limit in a new pair's first minutes"),
    ("170207", Rejected(Funds), "platform loan amount not enough"),
    ("170209", Permission, "pair only available in some regions"),
    ("170210", Rejected(Other), "new order rejected"),
    ("170212", Unknown, "cancel request processing"),
    ("170213", Rejected(NotFound), "order does not exist"),
    ("170215", Rejected(Other), "institutional lending buy restriction"),
    ("170216", Rejected(Other), "institutional lending leverage"),
    ("170217", Rejected(Other), "only LIMIT-MAKER for this pair"),
    ("170218", Rejected(Price), "LIMIT-MAKER rejected: invalid price"),
    ("170219", Permission, "UID not available to this feature"),
    ("170220", Rejected(Other), "institutional lending spot restriction"),
    ("170221", InvalidRequest, "this coin does not exist"),
    ("170222", RateLimited, "too many requests in this time frame"),
    ("170223", Rejected(Other), "institutional lending alert or liquidation"),
    ("170224", Permission, "not an Innovation Zone user"),
    ("170226", Rejected(Other), "spot margin account being liquidated"),
    ("170227", InvalidRequest, "feature not supported"),
    ("170228", Rejected(Funds), "purchase above the estimated maximum"),
    ("170229", Rejected(Funds), "sell above the estimated maximum"),
    ("170230", Rejected(Other), "institutional loan margin deactivated"),
    ("170234", Unknown, "system error"),
    ("170241", Permission, "confirm the project's risk disclosure"),
    ("170341", Rejected(VenueRule), "quantity over the maximum"),
    ("170344", Rejected(Other), "symbol not supported on margin"),
    ("170348", Permission, "use the Turkish site"),
    ("170355", InvalidRequest, "RPI orders for approved market makers only"),
    ("170358", Permission, "site does not support ETP"),
    ("170359", Permission, "site does not support leveraged trading"),
    ("170371", Rejected(Price), "price under the price limitation"),
    ("170372", Rejected(Price), "price over the price limitation"),
    ("170381", Rejected(VenueRule), "order quantity too large"),
    ("170382", Rejected(VenueRule), "order quantity too large"),
    ("170709", Rejected(Other), "OTC loan pair not whitelisted"),
    ("170810", Rejected(Other), "more than 500 conditional, TP/SL and active orders"),
];

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "retCode", default)]
    ret_code: Option<i64>,
    #[serde(rename = "retMsg", default)]
    ret_msg: Option<String>,
}

fn row(code: &str) -> Option<&'static ErrorRow> {
    ERRORS.iter().find(|(c, _, _)| *c == code)
}

/// The error an order call's answer carries, mapped; `None` for a success (`retCode` 0).
pub(super) fn decode(frame: &RawFrame) -> Option<VenueError> {
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        return Some(VenueError::new(Unknown, None, format!("bybit: {}", http::timeout(frame, "bybit").map(|e| e.message).unwrap_or_else(|| "no answer".into()))));
    };
    let retry_at = frame.header("x-bapi-limit-reset-timestamp").and_then(|v| v.trim().parse::<i64>().ok());
    let status = *status;
    if status == 403 {
        let text = String::from_utf8_lossy(body).to_ascii_lowercase();
        // A refused country gets a 403 from the CDN, as its own HTML page; otherwise a 403 is
        // the IP ban (error page, HTTP 403; rate-limit page: ten minutes).
        if text.contains("cloudfront") || text.contains("your country") {
            return Some(VenueError::new(Permission, Some("403".into()), format!("bybit {endpoint_id}: refused from this country (HTTP 403)")));
        }
        let mut e = VenueError::new(RateLimited, Some("403".into()), format!("bybit {endpoint_id}: this IP is over Bybit's limit and banned for ten minutes (HTTP 403)"));
        e.retry_after_ms = Some(http::retry_after_ms(frame).unwrap_or(IP_BAN_MS));
        return Some(e);
    }
    let env = serde_json::from_slice::<Envelope>(body).ok();
    match env.as_ref().and_then(|e| e.ret_code) {
        Some(0) if (200..300).contains(&status) => None,
        Some(code) if code != 0 => {
            let msg = env.as_ref().and_then(|e| e.ret_msg.clone()).unwrap_or_default();
            let code_s = code.to_string();
            let mut e = match row(&code_s) {
                Some((_, canon, _)) => VenueError::new(*canon, Some(code_s.clone()), format!("Bybit refused it: {msg} (code {code})")),
                None => VenueError::unmapped(Some(code_s.clone()), format!("Bybit answered code {code} ({msg}), which OpenTraderWorld does not map")),
            };
            if e.canon == RateLimited {
                e.retry_after_ms = http::retry_after_ms(frame);
                e.retry_at_ms = retry_at;
            }
            Some(e)
        }
        _ => {
            // No envelope: the HTTP status alone (error page "HTTP Code").
            let e: ConnectorError = http::classify(frame, "bybit")?;
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
            v.retry_at_ms = retry_at;
            Some(v)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn every_listed_code_maps_and_an_unlisted_one_alerts() {
        for (code, canon, _) in ERRORS {
            let body = format!(r#"{{"retCode":{code},"retMsg":"x","result":{{}},"retExtInfo":{{}},"time":1}}"#);
            let e = decode(&http_frame(1, "create", 200, &body)).unwrap();
            assert_eq!((e.canon, e.mapped), (*canon, true), "{code}");
        }
        let e = decode(&http_frame(1, "create", 200, r#"{"retCode":999999,"retMsg":"new"}"#)).unwrap();
        assert_eq!((e.canon, e.mapped), (Unknown, false));
        assert!(decode(&http_frame(1, "create", 200, r#"{"retCode":0,"retMsg":"OK","result":{}}"#)).is_none());
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
            let (codes, canon) = (cells[1], cells[3]);
            let ticks: Vec<&str> = codes.split('`').skip(1).step_by(2).collect();
            let mut i = 0;
            while i < ticks.len() {
                // `a` to `b` is a range of codes.
                let between = codes.split(&format!("`{}`", ticks[i])).nth(1).unwrap_or("");
                if between.trim_start().starts_with("to") && i + 1 < ticks.len() {
                    let (a, b): (i64, i64) = (ticks[i].parse().unwrap(), ticks[i + 1].parse().unwrap());
                    seen.extend((a..=b).map(|c| (c.to_string(), canon.to_string())));
                    i += 2;
                } else {
                    seen.push((ticks[i].to_string(), canon.to_string()));
                    i += 1;
                }
            }
        }
        let word = |c: &Canon| match c {
            Rejected(VenueRule) => "Rejected(venue_rule)",
            Rejected(Funds) => "Rejected(funds)",
            Rejected(Price) => "Rejected(price)",
            Rejected(MarketClosed) => "Rejected(market closed)",
            Rejected(NotFound) => "Rejected(not found)",
            Rejected(Final) => "Rejected(final)",
            Rejected(_) => "Rejected(other)",
            Unknown => "Unknown",
            RateLimited => "RateLimited",
            Auth => "Auth",
            Permission => "Permission",
            InvalidRequest => "InvalidRequest",
        };
        let mut want: Vec<(String, String)> = ERRORS.iter().map(|(c, k, _)| (c.to_string(), word(k).to_string())).collect();
        seen.retain(|(c, _)| c != "0");
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
    fn a_5xx_or_a_timeout_is_unknown_and_a_ban_holds() {
        assert_eq!(decode(&http_frame(1, "create", 502, "bad gateway")).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "create" };
        assert_eq!(decode(&t).unwrap().canon, Unknown);
        let ban = decode(&http_frame(1, "create", 403, "access too frequent")).unwrap();
        assert_eq!((ban.canon, ban.retry_after_ms), (RateLimited, Some(600_000)));
        assert_eq!(decode(&http_frame(1, "create", 401, "")).unwrap().canon, Auth);
    }
}
