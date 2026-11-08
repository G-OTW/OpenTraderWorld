//! EODHD end-of-day bars through the error matrix (testkit `matrix`): a mock venue answers,
//! the real driver reads. Rows are synthetic, from the documented `/api/eod` schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Eodhd> {
    Probe::new(
        || Eodhd::new().unwrap(),
        InstrumentId::new(VENUE, "AAPL.US"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
    .secrets(&[("api_key", "demo")])
}

fn row(date: &str, open: &str) -> String {
    format!(r#"{{"date":"{date}","open":{open},"high":11.0,"low":9.5,"close":10.5,"adjusted_close":10.4,"volume":1000}}"#)
}

fn rows(r: &[String]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!("[{}]", r.join(",")))
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
    let o = probe().serve(rows(&[row("2026-03-03", "10.2"), row("2026-03-02", "10.0")])).await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert!(cs[0].adjusted.is_some());
    assert_history_sound(o.result.as_ref().unwrap());
}

#[tokio::test]
async fn an_unknown_ticker_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(404).set_body_string("Ticker Not Found.")).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

/// A 422 naming the ticker is an unknown instrument, never a best-effort match.
#[tokio::test]
async fn a_422_naming_the_ticker_is_not_found() {
    let o = probe().serve(ResponseTemplate::new(422).set_body_string(r#"{"message":"Ticker not found or invalid"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"), "{}", o.err());
}

/// Any other 422 is the request's fault: a rejected request carrying EODHD's message.
#[tokio::test]
async fn a_422_on_a_bad_parameter_is_invalid_with_the_providers_message() {
    let o = probe().serve(ResponseTemplate::new(422).set_body_string(r#"{"errors":{"from":["The from does not match the format Y-m-d."]}}"#)).await;
    assert_eq!(o.kind(), ErrorKind::InvalidRequest);
    assert!(o.err().message.contains("The from does not match the format Y-m-d."), "{}", o.err());
    assert!(!o.err().message.contains("bad symbol"), "{}", o.err());
}

/// 402: the plan does not cover this data.
#[tokio::test]
async fn payment_required_is_permission() {
    let o = probe().serve(ResponseTemplate::new(402).set_body_string("Payment Required")).await;
    assert_eq!(o.kind(), ErrorKind::Permission);
}

#[tokio::test]
async fn a_row_without_its_date_is_decode() {
    let o = probe().serve(rows(&[r#"{"open":10.0,"high":11.0,"low":9.5,"close":10.5}"#.to_string()])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_price_is_never_a_zero() {
    let o = probe().serve(rows(&[row("2026-03-02", "null")])).await;
    match &o.result {
        Err(e) => assert_eq!(e.kind, ErrorKind::Decode),
        Ok(_) => assert!(o.candles().iter().all(|c| !c.open.0.is_zero()), "a null open became 0"),
    }
}

/// The row with the null open, and one with the close left out, are holes: the rows around
/// them are kept as served.
#[tokio::test]
async fn a_row_missing_a_price_is_a_hole_and_the_others_are_kept() {
    let missing = row("2026-03-04", "10.3").replace(r#","close":10.5"#, "");
    let o = probe().serve(rows(&[row("2026-03-02", "10.0"), row("2026-03-03", "null"), missing, row("2026-03-05", "10.4")])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(rows(&[row("2026-03-02", "10.0"), row("2026-03-05", "10.4")])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(rows(&[])).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}
