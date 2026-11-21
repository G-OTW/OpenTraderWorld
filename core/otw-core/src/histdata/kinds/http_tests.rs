//! The `http` kind: settings, the three answer shapes, timestamp units, close stamps, daily
//! anchoring, private-host refusal, and a window walked against a local server.

use std::collections::HashMap;
use std::net::IpAddr;

use serde_json::json;
use time::macros::datetime;
use time::OffsetDateTime;

use super::http::{at_path, opened_at, parse_bars, refused, render, uncovered, Enc, Spec, TsUnit, Vars};
use otw_store::histdata::Bar;

fn cfg(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A complete set of settings, with `extra` over it.
fn settings(extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut m = cfg(&[
        ("url", "https://data.example.com/bars?s={symbol}&i={tf}&from={from}&to={to}&n={limit}"),
        ("tf_map", r#"{"1h": "60", "1d": "D", "1w": "W"}"#),
        ("columns", r#"{"ts": 0, "open": 1, "high": 2, "low": 3, "close": 4, "volume": 5}"#),
        ("ts_unit", "s"),
    ]);
    m.extend(cfg(extra));
    m
}

fn spec(extra: &[(&str, &str)]) -> Spec {
    Spec::parse(&settings(extra)).expect("valid settings")
}

fn err(extra: &[(&str, &str)]) -> String {
    match Spec::parse(&settings(extra)) {
        Ok(_) => panic!("settings {extra:?} should be refused"),
        Err(e) => e.to_string(),
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[test]
fn tf_map_keys_must_be_chart_timeframes() {
    assert!(err(&[("tf_map", r#"{"1H": "60"}"#)]).contains("\"1H\" is not a chart timeframe"));
    assert!(err(&[("tf_map", r#"{"2h": "120"}"#)]).contains("not a chart timeframe"));
    assert!(err(&[("tf_map", r#"{}"#)]).contains("maps no timeframe"));
    assert!(err(&[("tf_map", r#"["1h"]"#)]).contains("must be a JSON object"));
    assert!(err(&[("tf_map", "1h=60")]).contains("not valid JSON"));
    assert!(err(&[("tf_map", r#"{"1h": ""}"#)]).contains("empty"));
    // A number token is read as its text.
    assert_eq!(spec(&[("tf_map", r#"{"1h": 60}"#)]).tf_map["1h"], "60");
}

#[test]
fn the_url_names_the_symbol_and_only_known_placeholders() {
    assert!(err(&[("url", "https://x.example.com/bars?i={tf}")]).contains("{symbol}"));
    assert!(err(&[("url", "https://x.example.com/{symbol}?start={start}")]).contains("{start} is not a placeholder"));
    assert!(err(&[("url", "ftp://x.example.com/{symbol}")]).contains("http://"));
    assert!(err(&[("ts_unit", "seconds")]).contains("use s, ms or iso"));
    assert!(err(&[("stamp", "middle")]).contains("open or close"));
    assert!(err(&[("daily_tz", "Mars/Olympus")]).contains("IANA"));
    assert!(err(&[("columns", r#"{"ts": 0}"#)]).contains("close is required"));
    assert!(err(&[("columns", r#"{"ts": 0, "close": 1, "price": 2}"#)]).contains("not a bar field"));
    assert!(err(&[("auth_header", "X-Key"), ("auth_query", "key")]).contains("not both"));
    assert!(err(&[("body", "{\"s\": \"{symbol}\"}")]).contains("method is GET"));
    assert!(err(&[("allow_private_hosts", "maybe")]).contains("true or false"));
    // Every required field is named when it is missing.
    let mut bare = settings(&[]);
    bare.remove("ts_unit");
    assert!(Spec::parse(&bare).unwrap_err().to_string().contains("Timestamp unit is required"));
}

#[test]
fn a_connector_declares_what_its_settings_map() {
    let c = super::build(
        "http-0000cafe",
        "My feed",
        settings(&[("asset_types", "Crypto, fx"), ("symbols", "BTCUSD,ETHUSD"), ("auth_header", "X-Key")]),
    )
    .unwrap();
    let cap = c.capability();
    assert_eq!(cap.provider, "http-0000cafe");
    assert_eq!(cap.label, "My feed");
    assert_eq!(cap.timeframes, ["1h", "1d", "1w"]);
    assert_eq!(cap.asset_types, ["crypto", "fx"]);
    assert_eq!(cap.required_secrets, [super::http::AUTH_SECRET]);
    assert!(cap.searchable && cap.testable);
    assert_eq!(cap.website, "https://data.example.com");
    // Settings that do not parse still give a listed connector that says what is wrong.
    let broken = super::build("http-0000dead", "Broken", cfg(&[("url", "https://x.example.com/")])).unwrap();
    assert!(broken.capability().timeframes.is_empty());
    assert!(broken.capability().rate_limit.contains("Settings incomplete"));
    // validate_config is the parse itself.
    assert!(broken.validate_config(&settings(&[])).is_ok());
    assert!(broken.validate_config(&cfg(&[("url", "https://x.example.com/")])).is_err());
}

#[test]
fn placeholders_are_filled_and_encoded_for_where_they_go() {
    let v = Vars {
        symbol: "BTC/USD",
        tf: "60",
        asset_type: "crypto",
        limit: 500,
        from: datetime!(2024-01-01 0:00 UTC),
        to: datetime!(2024-01-02 0:00 UTC),
    };
    let url = render("https://x/{symbol}?i={tf}&a={from}&b={to_ms}&c={from_iso}&n={limit}", &v, TsUnit::Secs, Enc::Url);
    assert_eq!(url, "https://x/BTC%2FUSD?i=60&a=1704067200&b=1704153600000&c=2024-01-01T00%3A00%3A00Z&n=500");
    let body = render(r#"{"s": "{symbol}", "f": "{from}"}"#, &Vars { symbol: "A\"B", ..v }, TsUnit::Iso, Enc::Json);
    assert_eq!(body, r#"{"s": "A\"B", "f": "2024-01-01T00:00:00Z"}"#);
}

// ── Shapes ───────────────────────────────────────────────────────────────────

fn closes(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| b.close).collect()
}

#[test]
fn a_list_of_lists_is_read_by_position() {
    let s = spec(&[("rows_path", "result.candles")]);
    let body = json!({"result": {"candles": [
        [1704070800, "2", "3", "1", "2.5", 10],
        [1704067200, 1.0, 2.0, 0.5, 1.5, 5],
    ]}});
    let bars = parse_bars(&s, &body, "1h").unwrap();
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].ts, datetime!(2024-01-01 0:00 UTC), "sorted by time");
    assert_eq!(closes(&bars), [1.5, 2.5]);
    assert_eq!(bars[1].volume, 10.0);
}

#[test]
fn a_list_of_objects_is_read_by_key() {
    let s = spec(&[
        ("rows_path", "data"),
        ("columns", r#"{"ts": "t", "open": "o", "high": "h", "low": "l", "close": "c"}"#),
    ]);
    let body = json!({"data": [{"t": 1704067200, "o": 1, "h": 2, "l": 0.5, "c": "1.5e-7"}]});
    let bars = parse_bars(&s, &body, "1h").unwrap();
    // An exponent is read as a number, not as its digits.
    assert_eq!(bars[0].close, 1.5e-7);
    // No volume column: zero, not an error.
    assert_eq!(bars[0].volume, 0.0);
}

#[test]
fn one_object_of_parallel_lists_is_read_by_key() {
    let s = spec(&[("columns", r#"{"ts": "t", "open": "o", "high": "h", "low": "l", "close": "c", "volume": "v"}"#)]);
    let body = json!({"s": "ok", "t": [1704067200, 1704070800], "o": [1, 2], "h": [2, 3], "l": [0.5, 1], "c": [1.5, 2.5], "v": [5, 6]});
    let bars = parse_bars(&s, &body, "1h").unwrap();
    assert_eq!(closes(&bars), [1.5, 2.5]);
    let ragged = json!({"t": [1704067200, 1704070800], "o": [1], "h": [2, 3], "l": [0.5, 1], "c": [1.5, 2.5], "v": [5, 6]});
    assert!(parse_bars(&s, &ragged, "1h").unwrap_err().to_string().contains("holds 1 values"));
}

#[test]
fn a_mapping_that_does_not_fit_the_shape_is_named() {
    // Keys against lists, positions against objects.
    let by_key = spec(&[("columns", r#"{"ts": "t", "close": "c"}"#)]);
    assert!(parse_bars(&by_key, &json!([[1704067200, 1.0]]), "1h").unwrap_err().to_string().contains("give its position"));
    let by_pos = spec(&[]);
    assert!(parse_bars(&by_pos, &json!([{"t": 1704067200}]), "1h").unwrap_err().to_string().contains("give its key"));
    // A missing path lists what the answer does hold.
    let e = at_path(&json!({"data": [], "status": "ok"}), "result").unwrap_err().to_string();
    assert!(e.contains("data") && e.contains("status"), "{e}");
    // An empty cell is an error naming the row, never a guessed price.
    let e = parse_bars(&by_pos, &json!([[1704067200, 1, 2, 0.5, null, 1]]), "1h").unwrap_err().to_string();
    assert!(e.contains("Row 1") && e.contains("close"), "{e}");
    // No rows is no bars, not an error: the window may predate the series.
    assert!(parse_bars(&by_pos, &json!([]), "1h").unwrap().is_empty());
}

// ── Timestamps ───────────────────────────────────────────────────────────────

#[test]
fn every_timestamp_unit_reads_the_same_instant() {
    let row = |ts: serde_json::Value| json!([[ts, 1, 2, 0.5, 1.5, 0]]);
    let want = datetime!(2024-01-01 1:00 UTC);
    let secs = parse_bars(&spec(&[("ts_unit", "s")]), &row(json!(1704070800)), "1h").unwrap();
    let millis = parse_bars(&spec(&[("ts_unit", "ms")]), &row(json!(1704070800000_i64)), "1h").unwrap();
    let iso = parse_bars(&spec(&[("ts_unit", "iso")]), &row(json!("2024-01-01T01:00:00Z")), "1h").unwrap();
    let offset = parse_bars(&spec(&[("ts_unit", "iso")]), &row(json!("2024-01-01T02:00:00+01:00")), "1h").unwrap();
    let naive = parse_bars(&spec(&[("ts_unit", "iso")]), &row(json!("2024-01-01 01:00:00")), "1h").unwrap();
    for bars in [secs, millis, iso, offset, naive] {
        assert_eq!(bars[0].ts, want);
    }
    // Milliseconds read as seconds land in the year 55000: refused, not stored.
    let e = parse_bars(&spec(&[("ts_unit", "s")]), &row(json!(1704070800000_i64)), "1h").unwrap_err().to_string();
    assert!(e.contains("could not be read"), "{e}");
}

#[test]
fn a_close_stamp_moves_back_one_bar() {
    let s = spec(&[("stamp", "close")]);
    let bars = parse_bars(&s, &json!([[1704070800, 1, 2, 0.5, 1.5, 0]]), "1h").unwrap();
    assert_eq!(bars[0].ts, datetime!(2024-01-01 0:00 UTC));
    let utc = time_tz::timezones::db::UTC;
    assert_eq!(opened_at(datetime!(2024-01-01 0:15 UTC), "15m", true, utc).unwrap(), datetime!(2024-01-01 0:00 UTC));
    assert_eq!(opened_at(datetime!(2024-01-01 0:15 UTC), "15m", false, utc).unwrap(), datetime!(2024-01-01 0:15 UTC));
}

#[test]
fn daily_bars_sit_at_their_days_midnight_in_the_daily_zone() {
    let ny = time_tz::timezones::db::america::NEW_YORK;
    let utc = time_tz::timezones::db::UTC;
    // A session closing at 16:00 New York is that day's bar, at New York midnight.
    assert_eq!(opened_at(datetime!(2024-01-05 21:00 UTC), "1d", true, ny).unwrap(), datetime!(2024-01-05 5:00 UTC));
    // In summer the same midnight is an hour earlier in UTC.
    assert_eq!(opened_at(datetime!(2024-07-05 20:00 UTC), "1d", true, ny).unwrap(), datetime!(2024-07-05 4:00 UTC));
    // A crypto day closing at midnight UTC stays on the day it closed.
    assert_eq!(opened_at(datetime!(2024-01-06 0:00 UTC), "1d", true, utc).unwrap(), datetime!(2024-01-05 0:00 UTC));
    // A date with no time is that day, and an open stamp keeps its day.
    let s = spec(&[("ts_unit", "iso"), ("daily_tz", "America/New_York")]);
    let bars = parse_bars(&s, &json!([["2024-01-05", 1, 2, 0.5, 1.5, 0]]), "1d").unwrap();
    assert_eq!(bars[0].ts, datetime!(2024-01-05 5:00 UTC));
    // A week closing on Friday is stored on that week's Monday.
    assert_eq!(opened_at(datetime!(2024-01-05 21:00 UTC), "1w", true, ny).unwrap(), datetime!(2024-01-01 5:00 UTC));
}

// ── Paging ───────────────────────────────────────────────────────────────────

fn bar(ts: OffsetDateTime) -> Bar {
    Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 0.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None }
}

#[test]
fn a_page_short_of_its_window_asks_for_the_rest() {
    let h = time::Duration::hours(1);
    let from = datetime!(2024-01-01 0:00 UTC);
    let to = from + h * 10;
    // Forward pager: the first three hours came back.
    let got: Vec<Bar> = (0..3).map(|i| bar(from + h * i)).collect();
    assert_eq!(uncovered(&got, from, to, h), vec![(from + h * 3, to)]);
    // Backward pager: the last three.
    let got: Vec<Bar> = (7..10).map(|i| bar(from + h * i)).collect();
    assert_eq!(uncovered(&got, from, to, h), vec![(from, from + h * 7)]);
    // Covered, or short by the forming bar only: nothing more to ask.
    let got: Vec<Bar> = (0..10).map(|i| bar(from + h * i)).collect();
    assert!(uncovered(&got, from, to, h).is_empty());
    let now = from + h * 9 + time::Duration::minutes(30);
    let got: Vec<Bar> = (0..9).map(|i| bar(from + h * i)).collect();
    assert!(uncovered(&got, from, now, h).is_empty());
    // A whole closed bar missing at the end is asked for.
    assert_eq!(uncovered(&got, from, to, h), vec![(from + h * 9, to)]);
}

// ── Where the request may go ─────────────────────────────────────────────────

#[test]
fn private_and_loopback_addresses_are_refused() {
    for ip in [
        "10.1.2.3", "172.16.0.1", "172.31.255.255", "192.168.1.10", "127.0.0.1", "169.254.169.254", "0.0.0.0", "::1",
        "fc00::1", "fd12:3456::1", "fe80::1", "::ffff:127.0.0.1", "::ffff:10.0.0.1", "::",
    ] {
        assert!(refused(ip.parse::<IpAddr>().unwrap()), "{ip} should be refused");
    }
    for ip in ["8.8.8.8", "172.32.0.1", "1.1.1.1", "2606:4700::1111"] {
        assert!(!refused(ip.parse::<IpAddr>().unwrap()), "{ip} is public");
    }
}

#[tokio::test]
async fn a_private_host_is_refused_by_name_before_any_request() {
    use super::http::check_host;
    for (url, named) in [
        ("http://127.0.0.1:9/x", "127.0.0.1"),
        ("http://[::1]:9/x", "::1"),
        ("http://169.254.169.254/latest/meta-data", "169.254.169.254"),
        // localhost answers ::1 or 127.0.0.1 depending on the resolver: either is named.
        ("http://localhost:9/x", "resolves to "),
    ] {
        let u = reqwest::Url::parse(url).unwrap();
        let e = check_host(&u, false).await.unwrap_err().to_string();
        assert!(e.contains(named) && e.contains("allow_private_hosts"), "{url}: {e}");
        if url.contains("localhost") {
            assert!(e.contains("127.0.0.1") || e.contains("::1"), "{e}");
        }
        // The per-connector override lets it through.
        assert!(check_host(&u, true).await.is_ok(), "{url}");
    }
}

// ── A window walked against a local server ───────────────────────────────────

/// A one-route HTTP server on loopback answering at most `page` hourly bars from `start`.
async fn serve_pages(page: i64) -> u16 {
    serve_rows(page, 3600).await
}

/// [`serve_pages`] with rows `step` seconds apart.
async fn serve_rows(page: i64, step: i64) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let q = |k: &str| {
                path.split(['?', '&']).find_map(|p| p.strip_prefix(&format!("{k}="))).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0)
            };
            let (start, end) = (q("start"), q("end"));
            let rows: Vec<serde_json::Value> = (0..page)
                .map(|i| start + i * step)
                .filter(|t| *t < end)
                .map(|t| json!([t, 1.0, 2.0, 0.5, 1.5, 1.0]))
                .collect();
            let body = json!({ "rows": rows }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    port
}

fn local(port: u16, extra: &[(&str, &str)]) -> HashMap<String, String> {
    let url = format!("http://127.0.0.1:{port}/bars?s={{symbol}}&start={{from}}&end={{to}}");
    let mut m = settings(&[("url", url.as_str()), ("rows_path", "rows"), ("min_interval_ms", "0")]);
    m.extend(cfg(extra));
    m
}

#[tokio::test]
async fn a_window_is_walked_until_covered_and_a_private_host_needs_the_override() {
    let port = serve_pages(3).await;
    let client = reqwest::Client::new();
    let from = datetime!(2024-01-01 0:00 UTC);
    let to = from + time::Duration::hours(10);

    // Loopback without the override: refused, and nothing was asked.
    let refused_src = super::build("http-0000f00d", "Local", local(port, &[])).unwrap();
    let e = refused_src
        .fetch_chunk(&client, &HashMap::new(), "BTC", "other", "1h", from, to)
        .await
        .err()
        .expect("loopback is refused by default")
        .to_string();
    assert!(e.contains("127.0.0.1"), "{e}");

    // With it: a three-bar page is walked to the whole ten-hour window.
    let src = super::build("http-0000f00d", "Local", local(port, &[("allow_private_hosts", "true")])).unwrap();
    let chunk = src.fetch_chunk(&client, &HashMap::new(), "BTC", "other", "1h", from, to).await.unwrap();
    assert_eq!(chunk.bars.len(), 10);
    assert_eq!(chunk.bars[0].ts, from);
    assert_eq!(chunk.bars[9].ts, from + time::Duration::hours(9));

    // The test reads a sample through the same path and returns the parsed bars.
    let hourly = super::build(
        "http-0000f00d",
        "Local",
        local(port, &[("allow_private_hosts", "true"), ("tf_map", r#"{"1h": "60"}"#), ("test_symbol", "BTC")]),
    )
    .unwrap();
    let (detail, sample) = hourly.test_sample(&client, &HashMap::new()).await.unwrap();
    assert!(detail.contains("bars of BTC at 1h"), "{detail}");
    let sample = sample.expect("a custom source shows what it read");
    assert!(!sample["bars"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_value_series_is_walked_to_now_and_a_short_page_is_named() {
    let port = serve_rows(3, 86_400).await;
    let value = [
        ("allow_private_hosts", "true"),
        ("shape", "value"),
        ("columns", r#"{"period_start": 0, "value": 4}"#),
        ("frequency", "1d"),
    ];
    let src = super::http::HttpSource::new("http-0000beef", "Local", local(port, &value));
    let today = OffsetDateTime::now_utc().date();
    let since = today - time::Duration::days(9);

    // Three observations per page, ten days asked: walked to today, not the first page only.
    let (obs, missing) = src.fetch_observations(&HashMap::new(), "BTC", Some(since), None).await.unwrap();
    assert_eq!(missing, 0);
    assert_eq!(obs.len(), 10);
    assert_eq!(obs[0].period_start, since);
    assert_eq!(obs[9].period_start, today);
    assert!(obs.iter().all(|o| o.value == 1.5));

    // Nothing stored: the default start is 5 years back at 1d, 600 pages of three. The walk
    // stops at its cap and says which setting to change, it never stores a truncated series.
    let e = src.fetch_observations(&HashMap::new(), "BTC", None, None).await.unwrap_err().to_string();
    assert!(e.contains("64 requests") && e.contains("max_bars"), "{e}");

    // The code check before a series is stored is one request.
    assert_eq!(src.probe_observations(&HashMap::new(), "BTC").await.unwrap().len(), 3);
}

#[test]
fn a_value_series_with_nothing_stored_starts_by_its_frequency() {
    let now = datetime!(2026-01-01 0:00 UTC);
    let years = |f: &str| (now - super::http::value_history_start(f, now)).whole_days() / 365;
    assert_eq!((years("D"), years("W"), years("M"), years("A"), years("I")), (5, 10, 30, 30, 30));
}
