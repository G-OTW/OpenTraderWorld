//! EODHD, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AssetClass, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{InstrumentCache, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn aapl() -> InstrumentId {
    InstrumentId::new(VENUE, "AAPL.US")
}

fn hist(tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: aapl(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(from), ts(to)), cursor: None }
}

fn q(h: &HttpRequest) -> Vec<(&str, &str)> {
    h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = Eodhd::new().unwrap();
    assert_undeclared_is_unsupported(&m, &aapl());
    assert!(m.instrument_id("BMW.XETRA").is_ok() && m.instrument_id("A/B").is_err());
    assert!(m.signer(&HashMap::new()).err().is_some_and(|e| e.message.contains("api_key")));
}

#[test]
fn end_of_day_reads_dates_and_the_adjusted_close() {
    let m = Eodhd::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 1_704_067_200, 1_704_326_400), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.path.as_str(), h.signed), ("/api/eod/AAPL.US", true));
    assert_eq!(q(h), [("from", "2024-01-01"), ("to", "2024-01-04"), ("period", "d"), ("fmt", "json")]);
    let body = json(r#"[{"date":"2024-01-02","open":187.15,"high":188.44,"low":183.885,"close":185.64,"adjusted_close":184.938,"volume":82488700},{"date":"2024-01-04","open":182.15,"high":183.09,"low":180.88,"close":181.91,"adjusted_close":181.2,"volume":71983600}]"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "eod", 200, &body), ts(0)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), ..) = &c[0] else { panic!() };
    assert_eq!(cs.len(), 1, "the `to` day is outside [from, to)");
    assert_eq!((cs[0].open_ts.secs(), cs[0].adjusted.as_ref().and_then(|a| a.close).map(|p| p.to_string())), (1_704_153_600, Some("184.938".into())));
}

#[test]
fn intraday_reads_unix_stamps() {
    let m = Eodhd::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &hist(Timeframe::M5, 1_704_200_000, 1_704_300_000), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h), [("interval", "5m"), ("from", "1704200000"), ("to", "1704300000"), ("fmt", "json")]);
    let body = json(r#"[{"timestamp":1704205800,"gmtoffset":0,"datetime":"2024-01-02 14:30:00","open":187.15,"high":187.5,"low":186.9,"close":187.2,"volume":120345}]"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "intraday", 200, &body), ts(0)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), ..) = &c[0] else { panic!() };
    assert_eq!((cs[0].open_ts.secs(), cs[0].volume.to_string(), cs[0].is_final), (1_704_205_800, "120345".into(), false));
}

#[test]
fn refusals_by_status() {
    let m = Eodhd::new().unwrap();
    let cache = InstrumentCache::new();
    let case = |status: u16, body: &str| {
        let mut s = Session::default();
        encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 0, 86_400), ts(0)).unwrap();
        decode_once(&m, &mut s, &cache, &http_frame(1, "eod", status, body), ts(0)).unwrap_err()
    };
    let e = case(404, "Ticker Not Found.");
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"));
    assert_eq!(case(401, "Unauthenticated").kind, ErrorKind::Auth);
    assert_eq!(case(402, "Payment Required").kind, ErrorKind::Permission);
    assert_eq!(case(429, "Too Many Requests").kind, ErrorKind::RateLimited);
}

#[test]
fn search_joins_code_and_exchange() {
    let m = Eodhd::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = Command::SearchInstruments { req_id: CorrId(3), query: "apple inc".into(), asset_class: None };
    let reqs = encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.path, "/api/search/apple%20inc");
    let body = json(r#"[{"Code":"AAPL","Exchange":"US","Name":"Apple Inc","Type":"Common Stock","Country":"USA"},{"Code":"EURUSD","Exchange":"FOREX","Name":"EUR/USD","Type":"Currency"},{"Code":"X","Name":"No exchange","Type":"ETF"}]"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(3, "search", 200, &body), ts(0)).unwrap();
    let got: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some((spec.id.native.clone(), spec.asset_class, m.search_exchange(spec))), _ => None }).collect();
    assert_eq!(got, [("AAPL.US".into(), AssetClass::Equity, "US".into()), ("EURUSD.FOREX".into(), AssetClass::Forex, "FOREX".into()), ("X.US".into(), AssetClass::Etf, "US".into())]);
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), intraday in proptest::bool::ANY) {
        let m = Eodhd::new().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let (tf, ep) = if intraday { (Timeframe::M5, "intraday") } else { (Timeframe::D1, "eod") };
        encode_once(&m, &mut s, &cache, &hist(tf, 0, 86_400), ts(0)).unwrap();
        let frame = RawFrame::Http { corr_id: CorrId(1), endpoint_id: ep, status: 200, headers: vec![], body: bytes.into() };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}
