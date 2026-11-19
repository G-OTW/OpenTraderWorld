//! OANDA v20 candles through the error matrix (testkit `matrix`): a mock venue answers, the
//! real driver reads. Bodies are synthetic, from the documented `/instruments/{id}/candles`
//! schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Oanda> {
    Probe::new(
        || Oanda::new().unwrap(),
        InstrumentId::new(VENUE, "EUR_USD"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
    .secrets(&[("api_token", "tok"), ("account_id", "001-004-1234567-001")])
}

fn candle(date: &str, mid: &str) -> String {
    format!(r#"{{"complete":true,"volume":4200,"time":"{date}T00:00:00.000000000Z","mid":{mid}}}"#)
}

const MID: &str = r#"{"o":"1.08000","h":"1.08500","l":"1.07500","c":"1.08200"}"#;

fn candles(c: &[String]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(r#"{{"instrument":"EUR_USD","granularity":"D","candles":[{}]}}"#, c.join(",")))
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
async fn nominal_candles_come_back_sorted_and_sound() {
    let o = probe().serve(candles(&[candle("2026-03-03", MID), candle("2026-03-02", MID)])).await;
    let cs = o.candles();
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [MON, MON + DAY]);
    assert!(cs.iter().all(|c| c.is_final));
    assert_history_sound(o.result.as_ref().unwrap());
}

/// OANDA answers 400 naming the instrument when it lists no such pair.
#[tokio::test]
async fn an_unknown_instrument_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(400).set_body_string(r#"{"errorMessage":"Invalid value specified for 'instrument'"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"));
}

#[tokio::test]
async fn an_answer_without_its_candle_list_is_decode() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"instrument":"EUR_USD","granularity":"D"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn a_null_midpoint_price_is_refused_never_a_zero() {
    let o = probe().serve(candles(&[candle("2026-03-02", r#"{"o":null,"h":"1.08500","l":"1.07500","c":"1.08200"}"#)])).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(candles(&[candle("2026-03-02", MID), candle("2026-03-05", MID)])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(candles(&[])).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}
