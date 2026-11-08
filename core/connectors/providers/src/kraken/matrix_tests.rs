//! Kraken OHLC through the error matrix (testkit `matrix`): a mock venue answers, the real
//! driver reads. Bodies are synthetic, from the documented `/0/public/OHLC` schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Kraken> {
    Probe::new(
        || Kraken::new().unwrap(),
        InstrumentId::new(VENUE, "XBTUSD"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
}

fn row(day: i64, open: &str) -> String {
    format!(r#"[{},{open},"67000.0","65000.0","66000.0","66100.0","12.5",420]"#, MON + day * DAY)
}

fn ohlc(r: &[String], last: i64) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(r#"{{"error":[],"result":{{"XXBTZUSD":[{}],"last":{last}}}}}"#, r.join(",")))
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
async fn nominal_rows_come_back_sorted_final_and_sound() {
    let o = probe().serve(ohlc(&[row(0, r#""66000.0""#), row(1, r#""66500.0""#)], MON + DAY)).await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert!(cs.iter().all(|c| c.is_final), "both open at or before `last`");
    assert_history_sound(o.result.as_ref().unwrap());
}

/// Kraken answers 200 with an error array for a pair it does not list.
#[tokio::test]
async fn an_unknown_pair_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"error":["EQuery:Unknown asset pair"]}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

/// Kraken's own throttle code, with a 200.
#[tokio::test]
async fn the_venue_throttle_code_is_a_rate_limit() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"error":["EGeneral:Too many requests"]}"#)).await;
    assert_eq!(o.kind(), ErrorKind::RateLimited);
}

#[tokio::test]
async fn a_busy_venue_is_transient() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"error":["EService:Unavailable"]}"#)).await;
    assert_eq!(o.kind(), ErrorKind::Transient);
}

#[tokio::test]
async fn a_short_row_is_decode() {
    let o = probe().serve(ohlc(&[format!(r#"[{MON},"66000.0","67000.0"]"#)], MON)).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_price_is_refused_never_a_zero() {
    let o = probe().serve(ohlc(&[row(0, "null")], MON)).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(ohlc(&[row(0, r#""66000.0""#), row(3, r#""66500.0""#)], MON + 3 * DAY)).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_series_is_one_empty_finished_chunk() {
    let o = probe().serve(ohlc(&[], MON)).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}

/// A result holding nothing but `last` names no series: the pair is unknown.
#[tokio::test]
async fn a_result_without_a_series_is_not_found() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"error":[],"result":{"last":1772409600}}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
}
