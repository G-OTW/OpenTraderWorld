//! Alpha Vantage, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AssetClass, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{InstrumentCache, NativeRequest, RawFrame, SignableRequest};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn ibm() -> InstrumentId {
    InstrumentId::new(VENUE, "IBM")
}

fn hist(tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: ibm(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(from), ts(to)), cursor: None }
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = AlphaVantage::new().unwrap();
    assert_undeclared_is_unsupported(&m, &ibm());
    assert!(m.instrument_id("TSCO.LON").is_ok() && m.instrument_id("A/../B").is_err());
}

#[test]
fn the_key_rides_the_query_through_the_signer() {
    let m = AlphaVantage::new().unwrap();
    assert!(m.signer(&HashMap::new()).err().is_some_and(|e| e.message.contains("api_key")));
    let signer = m.signer(&[("api_key".to_string(), "demo".to_string())].into()).unwrap().unwrap();
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &InstrumentCache::new(), &hist(Timeframe::D1, 0, 1), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert!(h.signed && !h.query.iter().any(|(k, _)| k == "apikey"), "the mapper never writes the key");
    let mut r = SignableRequest { method: h.method, path: h.path.clone(), query: h.query.clone(), headers: vec![], body: None };
    signer.sign(&mut r, ts(0)).unwrap();
    assert_eq!(r.query.last(), Some(&("apikey".to_string(), "demo".to_string())));
    let q: Vec<(&str, &str)> = h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(q, [("function", "TIME_SERIES_DAILY"), ("symbol", "IBM"), ("outputsize", "compact")]);
}

#[test]
fn a_series_is_read_sorted_and_filtered_to_the_window() {
    let m = AlphaVantage::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &hist(Timeframe::M5, 1_704_200_000, 1_704_300_000), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert!(h.query.contains(&("interval".to_string(), "5min".to_string())));
    let body = json(
        r#"{"Meta Data":{"1. Information":"Intraday (5min)","6. Time Zone":"US/Eastern"},
        "Time Series (5min)":{"2024-01-02 19:55:00":{"1. open":"161.1000","2. high":"161.2000","3. low":"161.0000","4. close":"161.1500","5. volume":"1203"},
        "2024-01-02 19:50:00":{"1. open":"161.0000","2. high":"161.1000","3. low":"160.9000","4. close":"161.1000","5. volume":"900"},
        "2023-12-29 19:55:00":{"1. open":"1","2. high":"1","3. low":"1","4. close":"1","5. volume":"1"}}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "series", 200, &body), ts(0)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), ..) = &c[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(), [1_704_243_000, 1_704_243_300], "New York time read as UTC instants, sorted, in the window");
    assert_eq!((cs[1].close.to_string(), cs[1].volume.to_string(), cs[1].is_final), ("161.1500".into(), "1203".into(), false));
    assert_history_sound(&[c[0].0.clone()]);
}

#[test]
fn refusals_are_read_from_the_body() {
    let m = AlphaVantage::new().unwrap();
    let cache = InstrumentCache::new();
    let case = |body: &str| {
        let mut s = Session::default();
        encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 0, 1), ts(0)).unwrap();
        decode_once(&m, &mut s, &cache, &http_frame(1, "series", 200, body), ts(0)).unwrap_err()
    };
    assert_eq!(case(r#"{"Note":"Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day."}"#).kind, ErrorKind::RateLimited);
    assert_eq!(case(r#"{"Information":"Thank you for using Alpha Vantage! This is a premium endpoint."}"#).kind, ErrorKind::Permission);
    let e = case(r#"{"Error Message":"Invalid API call. Please retry or visit the documentation."}"#);
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"));
    assert_eq!(case(r#"{"Meta Data":{}}"#).kind, ErrorKind::NotFound);
}

#[test]
fn search_reads_best_matches_in_order() {
    let m = AlphaVantage::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = Command::SearchInstruments { req_id: CorrId(3), query: "tesco".into(), asset_class: None };
    encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
    let body = json(r#"{"bestMatches":[{"1. symbol":"TSCO.LON","2. name":"Tesco PLC","3. type":"Equity","4. region":"United Kingdom"},{"1. symbol":"TSCDF","2. name":"Tesco plc","3. type":"ETF","4. region":"United States"}]}"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(3, "search", 200, &body), ts(0)).unwrap();
    let got: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some((spec.id.native.clone(), spec.asset_class, m.search_exchange(spec))), _ => None }).collect();
    assert_eq!(got, [("TSCO.LON".into(), AssetClass::Equity, "United Kingdom".into()), ("TSCDF".into(), AssetClass::Etf, "United States".into())]);
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512)) {
        let m = AlphaVantage::new().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 0, 1), ts(0)).unwrap();
        let frame = RawFrame::Http { corr_id: CorrId(1), endpoint_id: "series", status: 200, headers: vec![], body: bytes.into() };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}

#[test]
fn intraday_follows_new_york_s_clock_and_a_day_stays_a_date() {
    let zone = time_tz::timezones::get_by_name("US/Eastern").unwrap();
    // Summer (EDT, UTC-4) and winter (EST, UTC-5).
    assert_eq!(stamp("2024-07-01 09:30:00", Some(zone)).unwrap().secs(), 1_719_840_600);
    assert_eq!(stamp("2024-01-02 09:30:00", Some(zone)).unwrap().secs(), 1_704_205_800);
    assert_eq!(stamp("2024-01-02", None).unwrap().secs(), 1_704_153_600);
    // An intraday series without its zone is refused rather than guessed.
    let m = AlphaVantage::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist(Timeframe::M5, 0, 1), ts(0)).unwrap();
    let body = r#"{"Time Series (5min)":{"2024-01-02 19:55:00":{"1. open":"1","2. high":"1","3. low":"1","4. close":"1","5. volume":"1"}}}"#;
    assert!(decode_once(&m, &mut s, &cache, &http_frame(1, "series", 200, body), ts(0)).unwrap_err().message.contains("time zone"));
}
