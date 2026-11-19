//! KuCoin's order error tables (onboarding spec 8), one per platform, since the same code means
//! different things on spot and futures (`200001`, `200002`, `200003`). From the error pages
//! (https://www.kucoin.com/docs-new/error-code/spot, .../futures, .../http and the gateway codes
//! of .../websocket, read 2026-11-11): every code an order call (place, test, cancel, cancel
//! all, look up, fills, position and margin reads, the private token) can answer, mapped to one
//! canonical kind. A code not listed is `Unknown` and alerts. Mirrored in PROVIDER.md
//! ("Trading", "Errors"); a test keeps the two in step.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::trade::{Canon, ErrorRow, Refusal, VenueError};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

use super::Book;
use Canon::{Auth, InvalidRequest, Permission, RateLimited, Rejected, Unknown};
use Refusal::{Final, Funds, MarketClosed, NotFound, Other, Price, ReduceOnly, VenueRule};

/// Futures `1015`: "cloudflare frequency limit according to IP, block 30s".
const CLOUDFLARE_BLOCK_MS: u64 = 30_000;
/// Futures `200002`: "KuCoin business layer request frequency limit, block 10s".
const BUSINESS_BLOCK_MS: u64 = 10_000;

/// The gateway's codes, the same on both platforms (error-code/websocket, "GATEWAY ERROR
/// CODE"; error-code/spot and error-code/futures for the `4000xx` ones).
macro_rules! gateway_rows {
    () => {
        [
            ("400001", Auth, "a KC-API header is missing"),
            ("400002", Auth, "KC-API-TIMESTAMP invalid (more than 5 s off)"),
            ("400003", Auth, "KC-API-KEY does not exist"),
            ("400004", Auth, "KC-API-PASSPHRASE error"),
            ("400005", Auth, "signature error"),
            ("400006", Auth, "IP not on the key's whitelist"),
            ("400007", Permission, "access denied, the key lacks the permission"),
            ("400008", Auth, "V1 and V2 keys no longer supported: a V3 key is needed"),
            ("400009", Auth, "invalid KC-API-KEY-VERSION"),
            ("400010", Permission, "UID access denied"),
            ("400011", Auth, "session verification failed"),
            ("400012", Auth, "session verification timed out"),
            ("400101", InvalidRequest, "invalid request data"),
            ("400102", InvalidRequest, "check the parameters of the request"),
            ("400201", Auth, "invalid KC-API-PARTNER-SIGN"),
            ("400202", Auth, "invalid partner request IP"),
            ("400301", Permission, "restricted by the laws of the country or region"),
            ("400302", Permission, "services unavailable in this region (IP)"),
            ("400303", Permission, "identity verification required"),
            ("400304", Permission, "identity verification on the master account required"),
            ("400400", Auth, "invalid authorization token"),
            ("400401", Auth, "authorization required"),
            ("404000", InvalidRequest, "URL not found"),
            ("411100", Permission, "user is frozen"),
            ("411200", Permission, "URL on the user blacklist"),
            ("415000", InvalidRequest, "Content-Type must be application/json"),
            ("429000", RateLimited, "too many requests (UID limit, or server overload)"),
            ("429001", RateLimited, "too many total requests (system limit)"),
            ("429002", RateLimited, "too many requests on this connection"),
            ("500000", Unknown, "internal server error"),
            ("501000", Unknown, "the service has stopped running"),
            ("503000", Unknown, "server is busy"),
            ("504000", Unknown, "gateway timeout"),
            ("505000", Unknown, "unknown error"),
        ]
    };
}

const GATEWAY: [ErrorRow; 34] = gateway_rows!();

/// Codes the changelog added (2026.09.04, 2026.09.21) without naming a platform: mapped on both.
const CHANGELOG: [ErrorRow; 2] = [
    ("110188", RateLimited, "too many requests right now, the system is busy"),
    ("116241", Rejected(Other), "positions in associated accounts reached the system risk limit: close or reduce only"),
];

/// Spot (error-code/spot), the gateway rows first.
pub(super) const SPOT_ERRORS: &[ErrorRow] = &{
    const OWN: [ErrorRow; 21] = [
        // `400100` is "Parameter Error" and "account.available.amount: Insufficient balance"
        // at once: the message decides (see `decode`); the row is the parameter error.
        ("400100", InvalidRequest, "parameter error (or insufficient balance, by its message)"),
        ("102411", InvalidRequest, "parameter error"),
        ("102421", Rejected(Funds), "insufficient account balance"),
        ("102426", Unknown, "duplicate user-defined unique order id"),
        ("102427", Rejected(Other), "number of pending orders exceeds the threshold"),
        ("102428", Unknown, "order being processed, cannot be canceled"),
        ("102429", Rejected(Price), "price below the protection price"),
        ("102434", Rejected(VenueRule), "exceeds the maximum order size"),
        ("102435", Rejected(VenueRule), "below the minimum order size"),
        ("200001", Rejected(MarketClosed), "order creation for this pair suspended"),
        ("200002", Rejected(MarketClosed), "order cancel for this pair suspended"),
        ("200003", Rejected(Other), "number of orders breached the limit"),
        ("200004", Rejected(Funds), "balance insufficient"),
        ("200009", Permission, "KYC verification required before trading"),
        ("230005", Unknown, "system is busy"),
        ("400200", Permission, "forbidden to place an order"),
        ("400500", Permission, "country or region not supported for this token"),
        ("400600", Rejected(MarketClosed), "the trading pair has not started trading"),
        ("400700", Permission, "transaction restricted, risk problem on the account"),
        ("400800", Rejected(Other), "leverage order failed"),
        ("600203", Rejected(MarketClosed), "symbol cannot be traded (not enabled)"),
    ];
    const MORE: [ErrorRow; 3] = [("900001", InvalidRequest, "symbol does not exist"), CHANGELOG[0], CHANGELOG[1]];
    concat_rows::<34, 21, 3, 58>(GATEWAY, OWN, MORE)
};

/// Futures (error-code/futures), the gateway rows first.
pub(super) const FUTURES_ERRORS: &[ErrorRow] = &{
    const OWN: [ErrorRow; 34] = [
        ("400100", InvalidRequest, "parameter error (or insufficient balance, by its message)"),
        ("1015", RateLimited, "cloudflare IP frequency limit, blocked 30 s"),
        ("40010", Permission, "country or region restricted"),
        ("100001", InvalidRequest, "invalid parameters"),
        ("100002", Unknown, "systemConfigError"),
        ("100003", InvalidRequest, "contract parameter invalid"),
        ("100004", Rejected(Final), "order is in not cancelable state"),
        ("100005", Rejected(Other), "contract risk limit does not exist"),
        ("200001", InvalidRequest, "Level 2 query scope too large"),
        // `200002` is the 10 s frequency block and a Level 3 query error at once: the message
        // decides (see `decode`); the row is the frequency block.
        ("200002", RateLimited, "too many requests, blocked 10 s (or a Level 3 query error, by its message)"),
        ("200003", InvalidRequest, "symbol parameter invalid"),
        ("200005", Rejected(Funds), "insufficient balance (risk limit change)"),
        ("300000", InvalidRequest, "request parameter illegal"),
        ("300001", Rejected(Other), "active order quantity limit exceeded"),
        ("300002", Rejected(MarketClosed), "order placement or cancellation suspended"),
        ("300003", Rejected(Funds), "balance not enough"),
        ("300004", Rejected(Other), "stop order quantity limit exceeded"),
        ("300005", Rejected(Other), "order would exceed the maximum risk limit"),
        ("300006", Rejected(Price), "close price must be above the bankruptcy price"),
        ("300007", Rejected(Price), "price worse than the liquidation price"),
        ("300008", Rejected(Other), "no contra order in the market"),
        ("300009", Rejected(ReduceOnly), "position size 0, unable to close"),
        ("300010", Rejected(Other), "failed to close the position"),
        ("300011", Rejected(Price), "order price above the maximum allowed"),
        ("300012", Rejected(Price), "order price below the minimum allowed"),
        ("300013", Rejected(Other), "no contra order in the order book"),
        ("300014", Rejected(MarketClosed), "position being liquidated"),
        ("300015", Rejected(MarketClosed), "settlement in progress"),
        ("300016", Rejected(Other), "leverage above the maximum"),
        ("300017", Rejected(Other), "position is for Futures Brawl"),
        ("300018", Unknown, "clientOid repeated"),
        ("330005", InvalidRequest, "switch the margin mode to cross and try again"),
        ("330011", InvalidRequest, "switch and confirm the position mode before trading"),
        ("400200", Permission, "forbidden (unknown partner)"),
    ];
    concat_rows::<34, 34, 2, 70>(GATEWAY, OWN, CHANGELOG)
};

const fn concat_rows<const A: usize, const B: usize, const C: usize, const N: usize>(a: [ErrorRow; A], b: [ErrorRow; B], c: [ErrorRow; C]) -> [ErrorRow; N] {
    let mut out: [ErrorRow; N] = [("", Unknown, ""); N];
    let mut i = 0;
    while i < A {
        out[i] = a[i];
        i += 1;
    }
    let mut j = 0;
    while j < B {
        out[A + j] = b[j];
        j += 1;
    }
    let mut k = 0;
    while k < C {
        out[A + B + k] = c[k];
        k += 1;
    }
    out
}

pub(super) fn table(book: Book) -> &'static [ErrorRow] {
    match book {
        Book::Spot => SPOT_ERRORS,
        Book::Futures => FUTURES_ERRORS,
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
}

/// Whether the call cancels one order by its id (a `400100` there is looked up: see `decode`).
fn is_cancel(endpoint: &str) -> bool {
    matches!(endpoint, "spot_cancel" | "fut_cancel")
}

/// The error an answer carries, mapped; `None` for a success (`code` `200000`).
pub(super) fn decode(book: Book, frame: &RawFrame) -> Option<VenueError> {
    let label = book.label();
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else {
        // A timeout or a closed socket: the call may have reached the venue.
        return Some(VenueError::new(Unknown, None, format!("{label}: {}", http::timeout(frame, "kucoin").map(|e| e.message).unwrap_or_else(|| "no answer".into()))));
    };
    let status = *status;
    let env = serde_json::from_slice::<Envelope>(body).ok();
    let code = env.as_ref().and_then(|e| e.code.as_ref()).map(|c| match c {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    // "gw-ratelimit-reset: countdown to the next quota reset in milliseconds" (Rate Limit Rule).
    let reset = frame.header("gw-ratelimit-reset").and_then(|v| v.trim().parse::<u64>().ok());
    match code.as_deref() {
        Some("200000") if (200..300).contains(&status) => None,
        Some(code) if code != "200000" => {
            let msg = env.as_ref().and_then(|e| e.msg.clone()).unwrap_or_default();
            let mut e = match table(book).iter().find(|(c, _, _)| *c == code) {
                Some((_, canon, _)) => VenueError::new(*canon, Some(code.to_string()), format!("{label} refused it: {msg} (code {code})")),
                None => VenueError::unmapped(Some(code.to_string()), format!("{label} answered code {code} ({msg}), which OpenTraderWorld does not map")),
            };
            let lower = msg.to_ascii_lowercase();
            match code {
                // Both meanings of `400100` share the code; the balance one names itself.
                "400100" if lower.contains("account.available.amount") || lower.contains("insufficient balance") => e.canon = Rejected(Funds),
                // The doc names no code for an unknown or ended order on a cancel: a parameter
                // error on a cancel by id is looked up, and the order's real state answered.
                "400100" if is_cancel(endpoint_id) => e.canon = Rejected(NotFound),
                "200002" if book == Book::Futures && lower.contains("level 3") => e.canon = InvalidRequest,
                _ => {}
            }
            if e.canon == RateLimited {
                e.retry_after_ms = reset.or_else(|| http::retry_after_ms(frame)).or(match (book, code) {
                    (Book::Futures, "1015") => Some(CLOUDFLARE_BLOCK_MS),
                    (Book::Futures, "200002") => Some(BUSINESS_BLOCK_MS),
                    _ => None,
                });
            }
            Some(e)
        }
        _ => {
            if status == 403 {
                // HTTP page: "Forbidden or Too Many Requests: the request is forbidden or
                // Access limit breached": refused either way, never unknown.
                let mut e = VenueError::new(RateLimited, Some("403".into()), format!("{label} refused it with HTTP 403 (forbidden, or too many requests)"));
                e.retry_after_ms = reset.or_else(|| http::retry_after_ms(frame));
                return Some(e);
            }
            // No envelope: the HTTP status alone (error-code/http).
            let e: ConnectorError = http::classify(frame, "kucoin")?;
            let canon = match e.kind {
                ErrorKind::RateLimited => RateLimited,
                ErrorKind::Auth => Auth,
                ErrorKind::Permission => Permission,
                ErrorKind::InvalidRequest | ErrorKind::NotFound => InvalidRequest,
                // A 5xx: the venue may have acted.
                _ => Unknown,
            };
            let mut v = VenueError::new(canon, e.provider_code.clone(), format!("{label}: {}", e.message));
            v.retry_after_ms = reset.or(e.retry_after_ms);
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
        for book in [Book::Spot, Book::Futures] {
            for (code, canon, _) in table(book) {
                let body = format!(r#"{{"code":"{code}","msg":"x"}}"#);
                let e = decode(book, &http_frame(1, "spot_place", 200, &body)).unwrap();
                assert_eq!((e.canon, e.mapped), (*canon, true), "{book:?} {code}");
                assert!(e.message.contains("refused it") && e.message.contains(code), "{}", e.message);
            }
            let e = decode(book, &http_frame(1, "spot_place", 200, r#"{"code":"999999","msg":"new"}"#)).unwrap();
            assert_eq!((e.canon, e.mapped), (Unknown, false));
            assert!(e.message.contains("answered code 999999 (new), which OpenTraderWorld does not map"), "{}", e.message);
            assert!(decode(book, &http_frame(1, "spot_place", 200, r#"{"code":"200000","data":{}}"#)).is_none());
        }
    }

    #[test]
    fn the_same_code_means_what_its_platform_says() {
        let c = |book, code: &str| decode(book, &http_frame(1, "x", 200, &format!(r#"{{"code":"{code}","msg":"m"}}"#))).unwrap().canon;
        assert_eq!(c(Book::Spot, "200003"), Rejected(Other));
        assert_eq!(c(Book::Futures, "200003"), InvalidRequest);
        assert_eq!(c(Book::Spot, "200002"), Rejected(MarketClosed));
        assert_eq!(c(Book::Futures, "200002"), RateLimited);
        let lvl3 = decode(Book::Futures, &http_frame(1, "x", 200, r#"{"code":"200002","msg":"The query scope for Level 3 cannot exceed 100"}"#)).unwrap();
        assert_eq!(lvl3.canon, InvalidRequest);
    }

    /// The changelog's codes (2026.09.04, 2026.09.21): a busy system holds new orders, the risk
    /// limit refuses the order with its reason, on both platforms.
    #[test]
    fn the_changelog_codes_map_on_both_platforms() {
        for book in [Book::Spot, Book::Futures] {
            let c = |code: &str, msg: &str| decode(book, &http_frame(1, "x", 200, &format!(r#"{{"code":"{code}","msg":"{msg}"}}"#))).unwrap();
            assert_eq!(c("110188", "Too many requests right now. The system is busy. Please try again late.").canon, RateLimited);
            let risk = c("116241", "Positions in your associated accounts have reached the system risk limit.");
            assert_eq!(risk.canon, Rejected(Other));
            assert!(risk.message.contains("risk limit"), "{}", risk.message);
        }
    }

    #[test]
    fn code_400100_reads_its_message_and_its_call() {
        let e = |endpoint: &'static str, msg: &str| decode(Book::Spot, &http_frame(1, endpoint, 400, &format!(r#"{{"code":"400100","msg":"{msg}"}}"#))).unwrap().canon;
        assert_eq!(e("spot_place", "account.available.amount: Insufficient balance"), Rejected(Funds));
        assert_eq!(e("spot_place", "Parameter Error"), InvalidRequest);
        assert_eq!(e("spot_cancel", "Parameter Error"), Rejected(NotFound), "a cancel is looked up");
    }

    #[test]
    fn rate_limits_say_when_to_retry() {
        let f = otw_connector_sdk::RawFrame::Http {
            corr_id: otw_connector_model::CorrId(1),
            endpoint_id: "spot_place",
            status: 429,
            headers: vec![("gw-ratelimit-reset".into(), "489".into())],
            body: bytes::Bytes::from_static(br#"{"code":"429000","msg":"Too Many Requests"}"#),
        };
        let e = decode(Book::Spot, &f).unwrap();
        assert_eq!((e.canon, e.retry_after_ms), (RateLimited, Some(489)));
        let cf = decode(Book::Futures, &http_frame(1, "fut_place", 200, r#"{"code":"1015","msg":"cloudflare frequency limit"}"#)).unwrap();
        assert_eq!(cf.retry_after_ms, Some(30_000));
        let ban = decode(Book::Futures, &http_frame(1, "fut_place", 403, "Forbidden")).unwrap();
        assert_eq!(ban.canon, RateLimited);
    }

    #[test]
    fn a_5xx_or_a_timeout_is_unknown() {
        assert_eq!(decode(Book::Spot, &http_frame(1, "spot_place", 502, "bad gateway")).unwrap().canon, Unknown);
        let t = RawFrame::Timeout { corr_id: otw_connector_model::CorrId(1), endpoint_id: "spot_place" };
        assert_eq!(decode(Book::Futures, &t).unwrap().canon, Unknown);
        assert_eq!(decode(Book::Spot, &http_frame(1, "spot_place", 500, r#"{"code":"500000","msg":"Internal Server Error"}"#)).unwrap().canon, Unknown);
        assert_eq!(decode(Book::Spot, &http_frame(1, "spot_place", 200, r#"{"code":"230005","msg":"busy"}"#)).unwrap().canon, Unknown);
        assert_eq!(decode(Book::Futures, &http_frame(1, "fut_place", 200, r#"{"code":"300018","msg":"clientOid parameter repeated"}"#)).unwrap().canon, Unknown);
    }

    #[test]
    fn no_code_is_listed_twice_per_platform() {
        for book in [Book::Spot, Book::Futures] {
            let mut codes: Vec<&str> = table(book).iter().map(|r| r.0).collect();
            codes.sort_unstable();
            let n = codes.len();
            codes.dedup();
            assert_eq!(codes.len(), n, "{book:?}");
        }
    }

    /// The PROVIDER.md tables ("Trading", "Errors") name exactly the codes mapped here, each
    /// under the canonical kind it maps to.
    #[test]
    fn the_provider_tables_match_the_code() {
        let doc = include_str!("PROVIDER.md");
        let word = |c: &Canon| match c {
            Rejected(VenueRule) => "Rejected(venue_rule)",
            Rejected(Funds) => "Rejected(funds)",
            Rejected(Price) => "Rejected(price)",
            Rejected(MarketClosed) => "Rejected(market closed)",
            Rejected(NotFound) => "Rejected(not found)",
            Rejected(Final) => "Rejected(final)",
            Rejected(ReduceOnly) => "Rejected(reduce only)",
            Rejected(_) => "Rejected(other)",
            Unknown => "Unknown",
            RateLimited => "RateLimited",
            Auth => "Auth",
            Permission => "Permission",
            InvalidRequest => "InvalidRequest",
        };
        for (book, head, next) in [(Book::Spot, "#### Spot and gateway codes", "#### Futures codes"), (Book::Futures, "#### Futures codes", "### Rate limits for orders")] {
            let start = doc.find(head).unwrap();
            let table_text = &doc[start..start + doc[start..].find(next).unwrap()];
            let mut seen: Vec<(String, String)> = Vec::new();
            for line in table_text.lines().filter(|l| l.starts_with("| `")) {
                let cells: Vec<&str> = line.split('|').map(str::trim).collect();
                let (codes, canon) = (cells[1], cells[3]);
                for c in codes.split('`').skip(1).step_by(2) {
                    seen.push((c.to_string(), canon.to_string()));
                }
            }
            // The gateway rows are listed once, under spot, and apply to both platforms.
            if book == Book::Futures {
                let start = doc.find("#### Spot and gateway codes").unwrap();
                let spot_text = &doc[start..start + doc[start..].find("#### Futures codes").unwrap()];
                for line in spot_text.lines().filter(|l| l.starts_with("| `") && l.contains("(gateway)")) {
                    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
                    for c in cells[1].split('`').skip(1).step_by(2) {
                        seen.push((c.to_string(), cells[3].to_string()));
                    }
                }
            }
            let mut want: Vec<(String, String)> = table(book).iter().map(|(c, k, _)| (c.to_string(), word(k).to_string())).collect();
            seen.sort();
            seen.dedup();
            want.sort();
            assert_eq!(seen, want, "{book:?}");
        }
    }
}
