//! OKX history candles through the error matrix (testkit `matrix`): a mock venue answers,
//! the real driver reads. Rows are synthetic, from the documented
//! `/api/v5/market/history-candles` schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Okx> {
    Probe::new(
        || Okx::new().unwrap(),
        InstrumentId::new(VENUE, "BTC-USDT"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
}

fn row(day: i64, open: &str) -> String {
    format!(r#"["{}",{open},"67000.0","65000.0","66000.0","12.5","825000","825000","1"]"#, (MON + day * DAY) * 1000)
}

fn data(r: &[String]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(r#"{{"code":"0","msg":"","data":[{}]}}"#, r.join(",")))
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
async fn nominal_rows_come_back_oldest_first_and_sound() {
    let o = probe().serve(data(&[row(1, r#""66500.0""#), row(0, r#""66000.0""#)])).await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert_history_sound(o.result.as_ref().unwrap());
}

/// OKX answers 200 and code 51001 for an instrument it does not list.
#[tokio::test]
async fn an_unknown_instrument_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"code":"51001","msg":"Instrument ID does not exist","data":[]}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

/// The throttle comes as a 429 with the envelope code 50011: the wait is kept.
#[tokio::test]
async fn the_envelope_throttle_keeps_the_retry_after() {
    let t = ResponseTemplate::new(429).insert_header("Retry-After", "2").set_body_string(r#"{"code":"50011","msg":"Too Many Requests","data":[]}"#);
    let o = probe().serve(t).await;
    assert_eq!((o.kind(), o.err().retry_after_ms), (ErrorKind::RateLimited, Some(2_000)));
}

#[tokio::test]
async fn a_short_row_is_decode() {
    let o = probe().serve(data(&[format!(r#"["{}","66000.0"]"#, MON * 1000)])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_price_is_refused_never_a_zero() {
    let o = probe().serve(data(&[row(0, "null")])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(data(&[row(3, r#""66500.0""#), row(0, r#""66000.0""#)])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(data(&[])).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}
