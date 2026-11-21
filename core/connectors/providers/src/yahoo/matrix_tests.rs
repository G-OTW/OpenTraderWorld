//! Yahoo through the error matrix (testkit `matrix`): a mock venue answers, the real driver
//! reads. Bodies are synthetic, written from the observed chart v8 schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

/// 2026-03-02 14:30 UTC, the cash open: Yahoo stamps a daily bar there.
const MON: i64 = 1_772_461_800;
const DAY: i64 = 86_400;

fn probe() -> Probe<Yahoo> {
    Probe::new(
        || Yahoo::new().unwrap(),
        InstrumentId::new(VENUE, "AAPL"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(1_772_409_600), ts(1_773_014_400)),
        ts(1_774_915_200),
    )
}

fn chart(stamps: &str, quote: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(
        r#"{{"chart":{{"result":[{{"meta":{{}},"timestamp":[{stamps}],"indicators":{{"quote":[{quote}]}}}}],"error":null}}}}"#
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
async fn nominal_bars_come_back_sorted_and_sound() {
    let o = probe()
        .serve(chart(
            &format!("{},{}", MON + DAY, MON),
            r#"{"open":[11.5,10.0],"high":[12.0,11.0],"low":[11.0,9.5],"close":[11.8,10.5],"volume":[200,100]}"#,
        ))
        .await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert_eq!(cs[0].close.to_string(), "10.5");
    assert_history_sound(o.result.as_ref().unwrap());
}

#[tokio::test]
async fn an_unknown_symbol_is_not_found_never_a_best_effort_match() {
    let body = r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found, symbol may be delisted"}}}"#;
    let o = probe().serve(ResponseTemplate::new(404).set_body_string(body)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"), "{}", o.err());
}

#[tokio::test]
async fn an_answer_without_its_chart_envelope_is_decode() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"finance":{"result":null}}"#)).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

/// A null price is a gap: the row is skipped, never read as a zero price.
#[tokio::test]
async fn a_null_or_missing_price_is_a_gap_not_a_zero() {
    let o = probe()
        .serve(chart(
            &format!("{MON},{},{}", MON + DAY, MON + 2 * DAY),
            r#"{"open":[10.0,null,12.0],"high":[11.0,11.5,13.0],"low":[9.5,10.5],"close":[10.5,11.0,12.5],"volume":[100,null,300]}"#,
        ))
        .await;
    let cs = o.candles();
    // Day 2 has a null open, day 3 has no low at all: only day 1 is whole.
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].open_ts.secs(), MON);
    assert!(cs.iter().all(|c| !c.open.0.is_zero() && !c.low.0.is_zero()));
}

/// A hole in the series stays a hole: no bar is invented across it.
#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe()
        .serve(chart(
            &format!("{MON},{}", MON + 3 * DAY),
            r#"{"open":[10.0,12.0],"high":[11.0,13.0],"low":[9.5,11.5],"close":[10.5,12.5],"volume":[100,300]}"#,
        ))
        .await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"chart":{"result":[{"meta":{},"indicators":{"quote":[{}]}}],"error":null}}"#)).await;
    let items = o.result.as_ref().unwrap();
    assert_eq!(items.len(), 1);
    assert!(o.candles().is_empty());
}
