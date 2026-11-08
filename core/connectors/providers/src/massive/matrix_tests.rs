//! Massive stock aggregates through the error matrix (testkit `matrix`): a mock venue
//! answers, the real driver reads. Bodies are synthetic, from the documented v2 aggregates
//! schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Massive> {
    Probe::new(
        || Massive::new().unwrap(),
        InstrumentId::new(VENUE, "AAPL"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
    .secrets(&[("api_key", "demo")])
}

/// A daily aggregate opens at midnight New York (05:00 UTC in winter), in milliseconds.
fn agg(day: i64, open: &str) -> String {
    format!(r#"{{"t":{},"o":{open},"h":11.0,"l":9.5,"c":10.5,"v":1000,"n":10}}"#, (MON + day * DAY + 5 * 3600) * 1000)
}

fn results(r: &[String], extra: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(
        r#"{{"ticker":"AAPL","status":"OK","adjusted":false,"resultsCount":{},"results":[{}]{extra}}}"#,
        r.len(),
        r.join(",")
    ))
}

otw_connector_testkit::history_error_matrix! { probe();
    bad_request_is_invalid,
    unauthorized_is_auth,
    forbidden_is_permission,
    rate_limit_carries_retry_after,
    server_error_is_transient,
    gateway_json_outage_is_transient,
    timeout_is_transient,
    malformed_json_is_decode,
    empty_body_is_decode,
    html_page_is_decode,
}

#[tokio::test]
async fn nominal_aggregates_come_back_sorted_and_sound() {
    let o = probe().serve(results(&[agg(0, "10.0"), agg(1, "10.2")], "")).await;
    let cs = o.candles();
    assert_eq!(cs.len(), 2);
    assert!(cs[0].open_ts < cs[1].open_ts);
    assert_history_sound(o.result.as_ref().unwrap());
}

#[tokio::test]
async fn an_unknown_ticker_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"status":"NOT_FOUND","message":"ticker not found"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

#[tokio::test]
async fn an_aggregate_without_its_time_is_decode() {
    let o = probe().serve(results(&[r#"{"o":1,"h":1,"l":1,"c":1,"v":1}"#.to_string()], "")).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_price_is_never_a_zero() {
    let o = probe().serve(results(&[agg(0, "null")], "")).await;
    match &o.result {
        Err(e) => assert_eq!(e.kind, ErrorKind::Decode),
        Ok(_) => assert!(o.candles().iter().all(|c| !c.open.0.is_zero()), "a null open became 0"),
    }
}

/// The aggregate with the null open, and one with the close left out, are holes: the bars
/// around them are kept as served.
#[tokio::test]
async fn an_aggregate_missing_a_price_is_a_hole_and_the_others_are_kept() {
    let missing = agg(2, "10.3").replace(r#","c":10.5"#, "");
    let o = probe().serve(results(&[agg(0, "10.0"), agg(1, "null"), missing, agg(3, "10.4")], "")).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(results(&[agg(0, "10.0"), agg(3, "10.4")], "")).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

/// `resultsCount: 0` and no `results`: the window held no bars.
#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"ticker":"AAPL","status":"OK","resultsCount":0}"#)).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}

/// A truncated answer names the rest in `next_url`. Taking the first page as the whole
/// window is a silent partial history: the walk must continue or say it stopped.
#[tokio::test]
async fn a_next_url_is_never_dropped_silently() {
    let next = r#","next_url":"https://api.massive.com/v2/aggs/ticker/AAPL/range/1/day/1772409600000/1773014399999?cursor=bGltaXQ9MiZzb3J0PWFzYw""#;
    let o = probe().serve(results(&[agg(0, "10.0")], next)).await;
    match &o.result {
        Err(_) => {}
        Ok(_) => assert!(o.requests > 1, "the next page was never asked for"),
    }
}

/// An answer cut at the limit names its next page: it is asked for on the connector's own
/// base, with its cursor, and the history is both pages.
#[tokio::test]
async fn a_next_url_is_followed_until_the_last_page() {
    use otw_connector_testkit::wiremock::matchers::{any, query_param};
    use otw_connector_testkit::wiremock::{Mock, MockServer};
    let server = MockServer::start().await;
    Mock::given(query_param("cursor", "bGltaXQ9MiZzb3J0PWFzYw=="))
        .respond_with(results(&[agg(1, "10.2")], ""))
        .mount(&server)
        .await;
    let next = r#","next_url":"https://api.massive.com/v2/aggs/ticker/AAPL/range/1/day/1772409600000/1773014399999?cursor=bGltaXQ9MiZzb3J0PWFzYw%3D%3D""#;
    Mock::given(any()).respond_with(results(&[agg(0, "10.0")], next)).with_priority(10).mount(&server).await;
    let secrets: std::collections::HashMap<String, String> = [("api_key", "demo")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let m = Massive::new().unwrap();
    let signer = m.signer(&secrets).unwrap().unwrap();
    let drv = otw_connector_testkit::driver_against(m, &server, otw_connector_testkit::ManualClock::at(ts(1_774_915_200))).with_signer(signer);
    let got = drv.history(CorrId(1), &InstrumentId::new(VENUE, "AAPL"), &DataType::Candles { tf: Timeframe::D1 }, TimeRange::new(ts(MON), ts(MON + 7 * DAY))).await.unwrap();
    let days: Vec<i64> = got
        .iter()
        .flat_map(|i| match i {
            otw_connector_model::HistoryItems::Candles(c) => c.clone(),
            _ => Vec::new(),
        })
        .map(|c| (c.open_ts.secs() - MON) / DAY)
        .collect();
    assert_eq!(days, [0, 1]);
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[1].url.path(), "/v2/aggs/ticker/AAPL/range/1/day/1772409600000/1773014399999");
    assert!(reqs[1].url.query_pairs().any(|(k, v)| k == "apiKey" && v == "demo"), "the next page is signed: {}", reqs[1].url);
}
