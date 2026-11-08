//! Alpaca stock bars through the error matrix (testkit `matrix`): a mock venue answers, the
//! real driver reads. Bodies are synthetic, written from Alpaca's documented v2 bars schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{DataType, ErrorKind, InstrumentId, TimeRange, Timeframe};
use otw_connector_testkit::matrix::Probe;
use otw_connector_testkit::wiremock::ResponseTemplate;
use otw_connector_testkit::{assert_history_sound, ts};

use super::*;

const MON: i64 = 1_772_409_600; // 2026-03-02 00:00 UTC
const DAY: i64 = 86_400;

fn probe() -> Probe<Alpaca> {
    Probe::new(
        || Alpaca::new(Feed::Iex).unwrap(),
        InstrumentId::new(VENUE, "AAPL"),
        DataType::Candles { tf: Timeframe::D1 },
        TimeRange::new(ts(MON), ts(MON + 7 * DAY)),
        ts(1_774_915_200),
    )
    .secrets(&[("api_key", "PKTEST"), ("api_secret", "secret")])
}

/// A daily bar as Alpaca stamps it: midnight New York (05:00 UTC in winter).
fn bar(day: i64, o: &str, h: &str, l: &str, c: &str) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp(MON + day * DAY + 5 * 3600).unwrap();
    let t = t.format(&time::format_description::well_known::Rfc3339).unwrap();
    format!(r#"{{"t":"{t}","o":{o},"h":{h},"l":{l},"c":{c},"v":1000,"n":10}}"#)
}

fn page(bars: &[String]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(format!(r#"{{"bars":[{}],"symbol":"AAPL","next_page_token":null}}"#, bars.join(",")))
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
    let o = probe().serve(page(&[bar(1, "11", "12", "10.5", "11.5"), bar(0, "10", "11", "9.5", "10.5")])).await;
    let cs = o.candles();
    assert_eq!(cs.len(), 2);
    assert!(cs[0].open_ts < cs[1].open_ts);
    assert_eq!(cs[1].close.to_string(), "11.5");
    assert_history_sound(o.result.as_ref().unwrap());
}

/// Alpaca answers 422 for a symbol it does not list: a symbol problem, named as such.
#[tokio::test]
async fn an_unknown_symbol_is_not_found_never_a_best_effort_match() {
    let o = probe().serve(ResponseTemplate::new(422).set_body_string(r#"{"code":42210000,"message":"invalid symbol: ZZZZZ"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
    assert!(o.err().message.contains("bad symbol"), "{}", o.err());
}

/// A 422 on a parameter Alpaca refuses is the request's fault, not an unknown symbol: it
/// stays a rejected request and carries Alpaca's own message.
#[tokio::test]
async fn a_422_on_a_bad_parameter_is_invalid_with_the_providers_message() {
    let o = probe().serve(ResponseTemplate::new(422).set_body_string(r#"{"code":42210000,"message":"invalid timeframe"}"#)).await;
    assert_eq!(o.kind(), ErrorKind::InvalidRequest);
    assert!(o.err().message.contains("invalid timeframe"), "{}", o.err());
    assert!(!o.err().message.contains("bad symbol"), "{}", o.err());
}

/// No `bars` key at all: nothing for that symbol.
#[tokio::test]
async fn an_answer_without_bars_is_not_found() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"symbol":"AAPL","next_page_token":null}"#)).await;
    assert_eq!(o.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn a_bar_without_its_time_is_decode() {
    let o = probe().serve(ResponseTemplate::new(200).set_body_string(r#"{"bars":[{"o":1,"h":1,"l":1,"c":1,"v":1}],"next_page_token":null}"#)).await;
    assert_eq!(o.kind(), ErrorKind::Decode);
}

/// A price the venue left null is unknown: it must not reach the chart as a zero.
#[tokio::test]
async fn a_null_price_is_never_a_zero() {
    let o = probe().serve(page(&[bar(0, "10", "11", "9.5", "10.5"), bar(1, "null", "12", "10.5", "11.5")])).await;
    match &o.result {
        Err(e) => assert_eq!(e.kind, ErrorKind::Decode),
        Ok(_) => assert!(o.candles().iter().all(|c| !c.open.0.is_zero()), "a null open became 0"),
    }
}

/// The bar with the null price, and one with a price left out, are holes: the bars around
/// them are kept as served.
#[tokio::test]
async fn a_bar_missing_a_price_is_a_hole_and_the_others_are_kept() {
    let missing = bar(2, "12", "13", "11.5", "12.5").replace(r#","c":12.5"#, "");
    let o = probe().serve(page(&[bar(0, "10", "11", "9.5", "10.5"), bar(1, "null", "12", "10.5", "11.5"), missing, bar(3, "13", "14", "12.5", "13.5")])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn partial_data_keeps_the_hole() {
    let o = probe().serve(page(&[bar(0, "10", "11", "9.5", "10.5"), bar(3, "12", "13", "11.5", "12.5")])).await;
    let days: Vec<i64> = o.candles().iter().map(|c| (c.open_ts.secs() - MON) / DAY).collect();
    assert_eq!(days, [0, 3]);
}

#[tokio::test]
async fn an_empty_window_is_one_empty_finished_chunk() {
    let o = probe().serve(page(&[])).await;
    assert_eq!(o.result.as_ref().unwrap().len(), 1);
    assert!(o.candles().is_empty());
}

/// A page token continues the walk; the second page ends it.
#[tokio::test]
async fn a_next_page_token_is_followed_until_it_runs_out() {
    use otw_connector_testkit::wiremock::matchers::query_param;
    use otw_connector_testkit::wiremock::{Mock, MockServer};
    let server = MockServer::start().await;
    Mock::given(query_param("page_token", "p2"))
        .respond_with(page(&[bar(1, "11", "12", "10.5", "11.5")]))
        .mount(&server)
        .await;
    let first = format!(r#"{{"bars":[{}],"symbol":"AAPL","next_page_token":"p2"}}"#, bar(0, "10", "11", "9.5", "10.5"));
    Mock::given(otw_connector_testkit::wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_string(first))
        .with_priority(10)
        .mount(&server)
        .await;
    let secrets: std::collections::HashMap<String, String> =
        [("api_key", "PKTEST"), ("api_secret", "secret")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let a = Alpaca::new(Feed::Iex).unwrap();
    let signer = a.signer(&secrets).unwrap().unwrap();
    let drv = otw_connector_testkit::driver_against(a, &server, otw_connector_testkit::ManualClock::at(ts(1_774_915_200))).with_signer(signer);
    let got = drv.history(CorrId(1), &InstrumentId::new(VENUE, "AAPL"), &DataType::Candles { tf: Timeframe::D1 }, TimeRange::new(ts(MON), ts(MON + 7 * DAY))).await.unwrap();
    assert_eq!(got.len(), 2);
    assert_history_sound(&got);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
