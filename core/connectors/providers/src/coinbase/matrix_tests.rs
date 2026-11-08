//! Coinbase Exchange candles through the error matrix (testkit `matrix`): a mock venue
//! answers, the real driver reads. Rows are synthetic, `[time, low, high, open, close,
//! volume]` newest first as documented.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Coinbase> {
    Probe::new(
        || Coinbase::new().unwrap(),
        InstrumentId::new(VENUE, "BTC-USD"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
}

fn row(day: i64, open: &str) -> String {
    format!("[{},65000.5,67000.25,{open},66000.75,12.5]", MON + day * DAY)
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
async fn nominal_rows_come_back_oldest_first_and_sound() {
    let o = probe().serve(rows(&[row(1, "66500"), row(0, "66000")])).await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert_eq!((cs[0].low.to_string(), cs[0].high.to_string()), ("65000.5".into(), "67000.25".into()));
    assert_history_sound(o.result.as_ref().unwrap());
}

#[tokio::test]
async fn an_unknown_product_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(404).set_body_string(r#"{"message":"NotFound"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

#[tokio::test]
async fn a_short_row_is_decode() {
    let o = probe().serve(rows(&[format!("[{MON},65000.5,67000.25]")])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_price_is_refused_never_a_zero() {
    let o = probe().serve(rows(&[row(0, "null")])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

/// An object where the rows belong (an error envelope with a 200) is not a history.
#[tokio::test]
async fn an_object_instead_of_rows_is_decode() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"message":"maintenance"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(rows(&[row(3, "66500"), row(0, "66000")])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(rows(&[])).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}
