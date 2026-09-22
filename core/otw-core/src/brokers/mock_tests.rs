//! The connectors driven end to end against a local server that answers like the venue.
//!
//! A unit test covers one payload and the network tests cover the half of a request that
//! travels; what neither can reach is the **loop**: the cursor a second page is asked with,
//! the week a venue's history is sliced into, the page it stops on, the refusal folded into
//! a 200, the session token it is supposed to reuse instead of minting again. Those need a
//! server that answers, and a funded account would not exercise them any better: it would
//! have to happen to hold a history long enough to page.
//!
//! [`super::at`] is what makes it possible: every connector builds its URL through it, so a
//! test points the whole connector at a `wiremock` server and nothing else changes.
//!
//! These run offline, in the ordinary `cargo test`.

use std::collections::HashMap;

use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::{broker_for, client, serve_from, ExecQuery};

fn creds(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn q(days: i64, symbols: &[&str]) -> ExecQuery {
    let to = OffsetDateTime::now_utc();
    ExecQuery {
        from: to - Duration::days(days),
        to,
        symbols: symbols.iter().map(|s| s.to_string()).collect(),
    }
}

/// A query parameter of a request the mock server recorded.
fn param(req: &Request, key: &str) -> Option<String> {
    req.url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.to_string())
}

// ── OKX: a cursor, a short page, and a ceiling ───────────────────────────────

/// Pages until the venue answers a short one, carrying the previous page's last `billId`
/// forward as `after`. A cursor taken from the wrong row, or a loop that stops on the first
/// page, both show up here as a fill count.
#[tokio::test]
async fn okx_pages_on_the_cursor_it_was_handed() {
    let server = MockServer::start().await;
    struct Fills;
    impl Respond for Fills {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            // Only spot answers anything; the other instrument types come back empty, which
            // is also what closes their loop.
            if param(req, "instType").as_deref() != Some("SPOT") {
                return ResponseTemplate::new(200).set_body_json(json!({"code":"0","data":[]}));
            }
            let after: i64 = param(req, "after").and_then(|s| s.parse().ok()).unwrap_or(0);
            // 100 rows is a full page, so a second one is asked for; the second answers 20.
            let (start, count) = if after == 0 { (1_i64, 100) } else { (101, 20) };
            let data: Vec<Value> = (start..start + count)
                .map(|i| {
                    json!({
                        "instId": "BTC-USDT",
                        "tradeId": i.to_string(),
                        "billId": i.to_string(),
                        "ordId": "o1",
                        "side": "buy",
                        "fillSz": "0.5",
                        "fillPx": "30000",
                        "fee": "-0.1",
                        "feeCcy": "USDT",
                        "ts": (1_700_000_000_000_i64 + i).to_string(),
                    })
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"code":"0","data":data}))
        }
    }
    Mock::given(method("GET"))
        .and(path("/api/v5/trade/fills-history"))
        .respond_with(Fills)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v5/public/instruments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":"0","data":[]})))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let out = broker_for("okx")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[("api_key", "k"), ("api_secret", "s"), ("api_passphrase", "p")]),
            &q(3, &[]),
        )
        .await
        .unwrap();
    assert_eq!(out.len(), 120, "both pages should be read");
    // The second page was asked for with the first page's last bill id, not its first.
    let asked: Vec<Option<String>> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v5/trade/fills-history")
        .filter(|r| param(r, "instType").as_deref() == Some("SPOT"))
        .map(|r| param(r, "after"))
        .collect();
    assert_eq!(asked, vec![None, Some("100".into())], "the cursor is the last row's bill id");
    // A charge is filed as a positive cost and the fills come back in time order.
    assert!(out.windows(2).all(|w| w[0].at <= w[1].at), "fills are sorted by time");
    assert_eq!(out[0].fee, 0.1);
}

/// A venue that keeps answering full pages is stopped by the connector's own ceiling, with
/// a sentence the user can act on, rather than paging until the request times out.
#[tokio::test]
async fn okx_stops_on_a_history_it_cannot_page_to_the_end_of() {
    let server = MockServer::start().await;
    struct Endless;
    impl Respond for Endless {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            if param(req, "instType").as_deref() != Some("SPOT") {
                return ResponseTemplate::new(200).set_body_json(json!({"code":"0","data":[]}));
            }
            let base: i64 = param(req, "after").and_then(|s| s.parse().ok()).unwrap_or(0);
            let data: Vec<Value> = (1..=100)
                .map(|i| {
                    json!({
                        "instId": "BTC-USDT",
                        "billId": (base + i).to_string(),
                        "tradeId": (base + i).to_string(),
                        "side": "buy",
                        "fillSz": "1",
                        "fillPx": "1",
                        "fee": "0",
                        "ts": "1700000000000",
                    })
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"code":"0","data":data}))
        }
    }
    Mock::given(method("GET"))
        .and(path("/api/v5/trade/fills-history"))
        .respond_with(Endless)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v5/public/instruments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":"0","data":[]})))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let err = broker_for("okx")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[("api_key", "k"), ("api_secret", "s"), ("api_passphrase", "p")]),
            &q(3, &[]),
        )
        .await
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("shorter period"), "{msg}");
}

// ── Binance USDⓈ-M: a week at a time, and a millisecond past the last fill ────

/// Binance signs against *its* clock, so every pull starts by asking for it.
async fn mock_binance_clock(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/fapi/v1/time"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "serverTime": (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
        })))
        .mount(server)
        .await;
}

/// Binance answers seven days at most, so a longer period has to be walked. The windows
/// have to tile it: a gap is a hole in the book nobody would notice.
#[tokio::test]
async fn binance_futures_walks_the_period_a_week_at_a_time() {
    let server = MockServer::start().await;
    mock_binance_clock(&server).await;
    Mock::given(method("GET"))
        .and(path("/fapi/v1/userTrades"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let query = q(20, &["BTCUSDT"]);
    broker_for("binance_futures")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[("api_key", "k"), ("api_secret", "s")]),
            &query,
        )
        .await
        .unwrap();

    let windows: Vec<(i64, i64)> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/fapi/v1/userTrades")
        .map(|r| {
            (
                param(r, "startTime").unwrap().parse().unwrap(),
                param(r, "endTime").unwrap().parse().unwrap(),
            )
        })
        .collect();
    assert_eq!(windows.len(), 3, "20 days is three windows of at most seven: {windows:?}");
    let week = 7 * 86_400_000_i64;
    for (a, b) in &windows {
        assert!(b - a <= week, "a window wider than a week: {a}..{b}");
    }
    // They start where the period does, end where it does, and leave no gap between them.
    assert_eq!(windows[0].0, (query.from.unix_timestamp_nanos() / 1_000_000) as i64);
    assert_eq!(windows[2].1, (query.to.unix_timestamp_nanos() / 1_000_000) as i64);
    for pair in windows.windows(2) {
        assert_eq!(pair[0].1, pair[1].0, "a gap between two windows: {pair:?}");
    }
}

/// Inside a window the walk moves on the clock, and it has to move *past* the last fill:
/// starting again on its millisecond replays it, and starting a second later drops whatever
/// traded in between.
#[tokio::test]
async fn binance_futures_opens_the_next_page_one_millisecond_past_the_last_fill() {
    let server = MockServer::start().await;
    // One full page of a thousand fills, then nothing: enough to prove where the walk
    // resumed. The stamps are relative to the window the connector actually asked for.
    struct Trades(std::sync::atomic::AtomicUsize);
    impl Respond for Trades {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let start: i64 = param(req, "startTime").unwrap().parse().unwrap();
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
                return ResponseTemplate::new(200).set_body_json(json!([]));
            }
            let data: Vec<Value> = (0..1000)
                .map(|i| {
                    json!({
                        "id": i,
                        "orderId": 1,
                        "symbol": "BTCUSDT",
                        "side": "BUY",
                        "qty": "1",
                        "price": "30000",
                        "commission": "0.5",
                        "commissionAsset": "USDT",
                        "time": start + i,
                        "marginAsset": "USDT",
                    })
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(json!(data))
        }
    }
    mock_binance_clock(&server).await;
    Mock::given(method("GET"))
        .and(path("/fapi/v1/userTrades"))
        .respond_with(Trades(std::sync::atomic::AtomicUsize::new(0)))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let out = broker_for("binance_futures")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[("api_key", "k"), ("api_secret", "s")]),
            &q(3, &["BTCUSDT"]),
        )
        .await
        .unwrap();
    assert_eq!(out.len(), 1000);
    let starts: Vec<i64> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/fapi/v1/userTrades")
        .map(|r| param(r, "startTime").unwrap().parse().unwrap())
        .collect();
    // The page's last fill is at start+999, so the next page opens at start+1000: one
    // millisecond past it, never on it (a replay) and never a second later (a hole).
    assert!(starts.len() >= 2, "the walk stopped on the first page: {starts:?}");
    assert_eq!(starts[1], starts[0] + 1000, "{starts:?}");
}

/// The venue answers one instrument at a time, and a pull that names none would silently
/// read nothing at all. It is refused instead, naming what to type.
#[tokio::test]
async fn binance_futures_refuses_a_pull_that_names_no_instrument() {
    let err = broker_for("binance_futures")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[("api_key", "k"), ("api_secret", "s")]),
            &q(3, &[]),
        )
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("one instrument at a time"));
}

// ── Bitget: two books, two window widths, one cursor ─────────────────────────

/// Spot is served ninety days at a time and futures seven, each on its own endpoint, and
/// the two are asked for separately. Both must cover the whole period.
#[tokio::test]
async fn bitget_slices_each_book_into_the_window_it_serves() {
    let server = MockServer::start().await;
    for p in ["/api/v2/spot/trade/fills", "/api/v2/mix/order/fill-history"] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"code":"00000","data":[]})),
            )
            .mount(&server)
            .await;
    }
    serve_from(&server.uri());
    broker_for("bitget")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[
                ("api_key", "k"),
                ("api_secret", "s"),
                ("api_passphrase", "p"),
                ("books", "spot,usdt-futures"),
            ]),
            &q(100, &[]),
        )
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let widths = |p: &str| -> Vec<i64> {
        reqs.iter()
            .filter(|r| r.url.path() == p)
            .map(|r| {
                let a: i64 = param(r, "startTime").unwrap().parse().unwrap();
                let b: i64 = param(r, "endTime").unwrap().parse().unwrap();
                b - a
            })
            .collect()
    };
    let spot = widths("/api/v2/spot/trade/fills");
    let futures = widths("/api/v2/mix/order/fill-history");
    assert!(!spot.is_empty() && !futures.is_empty(), "both books are asked");
    assert!(spot.iter().all(|w| *w <= 90 * 86_400_000), "a spot slice wider than 90 days");
    assert!(futures.iter().all(|w| *w <= 7 * 86_400_000), "a futures slice wider than a week");
    // A hundred days of futures at a week each: the period is covered, not sampled.
    assert!(futures.len() >= 14, "only {} futures windows for 100 days", futures.len());
}

// ── OANDA: the pages the venue itself hands back ─────────────────────────────

/// OANDA answers an index of page URLs rather than a cursor, and each one is fetched. Only
/// the id pair is taken from those URLs (a unit test covers that); what is checked here is
/// that every page in the index is actually read.
#[tokio::test]
async fn oanda_reads_every_page_of_the_index() {
    let server = MockServer::start().await;
    let uri = server.uri();
    Mock::given(method("GET"))
        .and(path("/v3/accounts/001-004-1234567-001/transactions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pages": [
                format!("{uri}/v3/accounts/X/transactions/idrange?from=1&to=2"),
                format!("{uri}/v3/accounts/X/transactions/idrange?from=3&to=4"),
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v3/accounts/001-004-1234567-001/transactions/idrange"))
        .respond_with(|req: &Request| {
            let from = param(req, "from").unwrap();
            ResponseTemplate::new(200).set_body_json(json!({
                "transactions": [{
                    "id": from,
                    "type": "ORDER_FILL",
                    "instrument": "EUR_USD",
                    "units": "1000",
                    "price": "1.1",
                    "commission": "0.1",
                    "financing": "0",
                    "time": "2026-01-02T03:04:05.000000000Z",
                    "accountID": "001-004-1234567-001",
                }]
            }))
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v3/accounts/001-004-1234567-001/instruments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"instruments":[]})))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let out = broker_for("oanda")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[("api_token", "t"), ("account_id", "001-004-1234567-001")]),
            &q(3, &[]),
        )
        .await
        .unwrap();
    assert_eq!(out.len(), 2, "one fill per page in the index");
    let ranges: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/idrange"))
        .map(|r| param(r, "from").unwrap())
        .collect();
    assert_eq!(ranges, vec!["1", "3"], "both pages were fetched");
}

// ── Capital.com: one session, reused ─────────────────────────────────────────

/// The sign-in endpoint allows one request a second and the tokens come back in headers, so
/// a session is minted once and carried on every later call. A connector that signed in per
/// request would be rate-limited off the venue within a pull.
#[tokio::test]
async fn capitalcom_signs_in_once_and_carries_the_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("CST", "cst-token")
                .insert_header("X-SECURITY-TOKEN", "sec-token")
                .set_body_json(json!({})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/accounts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "accounts": [{"accountId": "1", "accountName": "Main", "balance": {"balance": 10.0}}]
        })))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let http = client().unwrap();
    // A credential of this test's own, so the process-wide session cache is not one another
    // test already filled.
    let secrets = creds(&[
        ("api_key", "capital-one-session"),
        ("identifier", "nobody@example.invalid"),
        ("api_password", "p"),
    ]);
    let b = broker_for("capitalcom").unwrap();
    b.test(&http, &secrets).await.unwrap();
    b.test(&http, &secrets).await.unwrap();

    let reqs = server.received_requests().await.unwrap();
    let logins = reqs.iter().filter(|r| r.method == wiremock::http::Method::POST).count();
    assert_eq!(logins, 1, "the session was minted twice");
    let carried: Vec<_> = reqs
        .iter()
        .filter(|r| r.url.path() == "/api/v1/accounts")
        .map(|r| {
            (
                r.headers.get("CST").map(|v| v.to_str().unwrap().to_string()),
                r.headers.get("X-SECURITY-TOKEN").map(|v| v.to_str().unwrap().to_string()),
            )
        })
        .collect();
    assert_eq!(carried.len(), 2);
    for (cst, sec) in carried {
        assert_eq!(cst.as_deref(), Some("cst-token"), "the session token is not carried");
        assert_eq!(sec.as_deref(), Some("sec-token"));
    }
}

// ── TradeStation: a refusal wrapped in a 200 ─────────────────────────────────

/// TradeStation answers 200 with a per-account `Errors` array when it will not serve one of
/// them. Reading that as an empty history would file "you traded nothing" into the journal,
/// so it has to fail instead, and the token call must not be repeated for each page.
#[tokio::test]
async fn tradestation_refuses_a_per_account_error_inside_a_200() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "tok", "expires_in": 1200})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Orders": [],
            "Errors": [{"AccountID": "SIM123", "Error": "Forbidden", "Message": "no access"}]
        })))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let err = broker_for("tradestation")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[
                ("client_id", "ts-inside-200"),
                ("client_secret", "s"),
                ("refresh_token", "r"),
                ("account_id", "SIM123"),
            ]),
            &q(3, &[]),
        )
        .await
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("SIM123"), "the refusal names the account: {msg}");
    assert!(msg.to_lowercase().contains("no access"), "it repeats what the venue said: {msg}");
}

// ── FOREX.com: no cursor, so the walk must not repeat or stall ───────────────

/// StoneX has no page cursor: the walk asks for everything changed since the newest row it
/// has read, so a row lands on two pages by design and the order id is what tells them
/// apart. A page that does not move the clock ends the walk rather than repeating forever.
#[tokio::test]
async fn forexcom_does_not_file_a_row_twice_or_walk_forever() {
    let server = MockServer::start().await;
    let at = OffsetDateTime::now_utc() - Duration::hours(1);
    let at_ms = (at.unix_timestamp_nanos() / 1_000_000) as i64;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"Session": "sess"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/useraccount/ClientAndTradingAccount"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ClientAccountId": 1,
            "TradingAccounts": [{"TradingAccountId": 7, "TradingAccountCode": "ACC"}]
        })))
        .mount(&server)
        .await;
    // Two hundred rows is a full page, so a second is asked for; it repeats the last row of
    // the first (same OrderId) and adds one more, then the third page is short.
    struct History {
        at_ms: i64,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Respond for History {
        fn respond(&self, _req: &Request) -> ResponseTemplate {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let row = |id: i64, ms: i64| {
                json!({
                    "OrderId": id,
                    "MarketName": "EUR/USD",
                    "Direction": "buy",
                    "Quantity": 1000.0,
                    "Price": 1.1,
                    "ExecutedDateTimeUtc": format!("/Date({ms})/"),
                })
            };
            let rows: Vec<Value> = match n {
                0 => (1..=200).map(|i| row(i, self.at_ms + i)).collect(),
                1 => vec![row(200, self.at_ms + 200), row(201, self.at_ms + 201)],
                _ => vec![],
            };
            ResponseTemplate::new(200).set_body_json(json!({ "TradeHistory": rows }))
        }
    }
    Mock::given(method("GET"))
        .and(path("/order/tradehistory"))
        .respond_with(History { at_ms, calls: std::sync::atomic::AtomicUsize::new(0) })
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let out = broker_for("forexcom")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[
                ("username", "fx-walk"),
                ("password", "p"),
                ("app_key", "a"),
                ("trading_account_id", "7"),
            ]),
            &q(1, &[]),
        )
        .await
        .unwrap();
    assert_eq!(out.len(), 201, "the repeated row was filed twice or a page was skipped");
    let mut ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), out.len(), "two fills share an id");
    // The second page was asked for from the newest row of the first, not from the start.
    let froms: Vec<i64> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/order/tradehistory")
        .map(|r| param(r, "from").unwrap().parse().unwrap())
        .collect();
    assert!(froms[1] > froms[0], "the walk did not advance: {froms:?}");
}

// ── NinjaTrader: every fee line, and one lookup per contract ─────────────────

/// A NinjaTrader fill carries no cost of its own: the platform books clearing, exchange,
/// NFA, brokerage, IP, routing and commission separately, and a trade showing only the
/// commission understates what it cost. The contract behind it takes three hops, which are
/// asked once and remembered rather than repeated per fill.
#[tokio::test]
async fn ninjatrader_adds_up_every_fee_line_and_looks_a_contract_up_once() {
    let server = MockServer::start().await;
    let at = OffsetDateTime::now_utc() - Duration::hours(2);
    let stamp = at.format(&time::format_description::well_known::Rfc3339).unwrap();
    Mock::given(method("POST"))
        .and(path("/auth/accesstokenrequest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"accessToken": "tok"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/account/list"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{"id": 42, "name": "DEMO1"}])),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/order/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 9, "accountId": 42},
            {"id": 10, "accountId": 42}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fillFee/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": 1,
            "clearingFee": 0.1,
            "exchangeFee": 0.2,
            "nfaFee": 0.02,
            "brokerageFee": 0.5,
            "ipFee": 0.05,
            "commission": 1.0,
            "orderRoutingFee": 0.03,
        }])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fill/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "orderId": 9, "contractId": 77, "timestamp": stamp, "action": "Buy",
             "qty": 2, "price": 5000.0},
            {"id": 2, "orderId": 10, "contractId": 77, "timestamp": stamp, "action": "Sell",
             "qty": 2, "price": 5010.0}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/contract/item"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 77, "name": "ESZ5", "contractMaturityId": 88
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/contractMaturity/item"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": 88, "productId": 99})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/product/item"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 99, "productType": "Futures", "valuePerPoint": 50.0, "currencyId": 1,
            "name": "ES"
        })))
        .mount(&server)
        .await;

    serve_from(&server.uri());
    let out = broker_for("ninjatrader")
        .unwrap()
        .executions(
            &client().unwrap(),
            &creds(&[
                ("username", "nt-fees"),
                ("password", "p"),
                ("cid", "1"),
                ("sec", "s"),
                ("account_id", "42"),
            ]),
            &q(1, &[]),
        )
        .await
        .unwrap();
    assert_eq!(out.len(), 2);
    // Seven lines, added up as one positive cost, on the fill they belong to.
    let first = out.iter().find(|e| e.id == "1").unwrap();
    assert!((first.fee - 1.90).abs() < 1e-9, "fee lines were not all counted: {}", first.fee);
    // A fill with no fee row is a zero, never an invented one.
    assert_eq!(out.iter().find(|e| e.id == "2").unwrap().fee, 0.0);
    // The point value is the venue's, not an assumption.
    assert_eq!(first.multiplier, 50.0);
    assert_eq!(first.symbol, "ESZ5");
    // Two fills on one contract: the three hops are walked once, not twice.
    let lookups = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/contract/item")
        .count();
    assert_eq!(lookups, 1, "the contract was looked up per fill");
}
