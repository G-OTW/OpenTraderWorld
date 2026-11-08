//! Alpha Vantage daily series through the error matrix (testkit `matrix`): a mock venue
//! answers, the real driver reads. Bodies are synthetic, from the documented
//! TIME_SERIES_DAILY schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<AlphaVantage> {
    Probe::new(
        || AlphaVantage::new().unwrap(),
        InstrumentId::new(VENUE, "IBM"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
    .secrets(&[("api_key", "demo")])
}

fn row(date: &str, o: &str) -> String {
    format!(r#""{date}":{{"1. open":{o},"2. high":"11.0","3. low":"9.5","4. close":"10.5","5. volume":"1000"}}"#)
}

fn series(rows: &[String]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(
        r#"{{"Meta Data":{{"1. Information":"Daily Prices","2. Symbol":"IBM","5. Time Zone":"US/Eastern"}},"Time Series (Daily)":{{{}}}}}"#,
        rows.join(",")
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
async fn nominal_days_come_back_sorted_and_sound() {
    let o = probe().serve(series(&[row("2026-03-03", r#""10.2""#), row("2026-03-02", r#""10.0""#)])).await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert_history_sound(o.result.as_ref().unwrap());
}

/// Alpha Vantage answers 200 and an `Error Message` for a symbol it does not know.
#[tokio::test]
async fn an_unknown_symbol_is_not_found_never_a_best_effort_match() {
    let body = r#"{"Error Message":"Invalid API call. Please retry or visit the documentation (https://www.alphavantage.co/documentation/) for TIME_SERIES_DAILY."}"#;
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(body)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

/// The daily allowance is a throttle, said in the body of a 200.
#[tokio::test]
async fn the_allowance_note_is_a_rate_limit() {
    let body = r#"{"Information":"Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day."}"#;
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(body)).await;
    assert_eq!(o.kind(), ErrorKind::RateLimited);
}

#[tokio::test]
async fn a_row_with_an_unreadable_date_is_decode() {
    let o = probe().serve(series(&[row("03/02/2026", r#""10.0""#)])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_price_is_never_a_zero() {
    let o = probe().serve(series(&[row("2026-03-02", "null")])).await;
    match &o.result {
        Err(e) => assert_eq!(e.kind, ErrorKind::Decode),
        Ok(_) => assert!(o.candles().iter().all(|c| !c.open.0.is_zero()), "a null open became 0"),
    }
}

/// The row with the null open, and one with the close left out, are holes: the rows around
/// them are kept as served.
#[tokio::test]
async fn a_row_missing_a_price_is_a_hole_and_the_others_are_kept() {
    let missing = row("2026-03-04", r#""10.3""#).replace(r#","4. close":"10.5""#, "");
    let o = probe().serve(series(&[row("2026-03-02", r#""10.0""#), row("2026-03-03", "null"), missing, row("2026-03-05", r#""10.4""#)])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(series(&[row("2026-03-02", r#""10.0""#), row("2026-03-05", r#""10.4""#)])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_series_is_one_empty_finished_chunk() {
    let o = probe().serve(series(&[])).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}
