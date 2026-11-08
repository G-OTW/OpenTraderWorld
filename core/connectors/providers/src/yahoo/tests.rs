//! Yahoo, offline. Synthetic fixtures written from the observed schema (Yahoo publishes no
//! reference; see PROVIDER.md).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AssetClass, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{InstrumentCache, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn aapl() -> InstrumentId {
    InstrumentId::new(VENUE, "AAPL")
}

fn hist(tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: aapl(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(from), ts(to)), cursor: None }
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = Yahoo::new().unwrap();
    assert!(!m.capabilities().history.candles.as_ref().unwrap().final_flag);
    assert_undeclared_is_unsupported(&m, &aapl());
    assert!(m.instrument_id("^GSPC").is_ok() && m.instrument_id("EURUSD=X").is_ok() && m.instrument_id("brk-b").unwrap().native == "brk-b");
    assert!(m.instrument_id("AAPL/../x").is_err());
}

#[test]
fn history_clamps_to_the_depth_yahoo_keeps_and_reads_columns() {
    let m = Yahoo::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let to = 1_704_499_200;
    // 1h keeps 730 days: an older start is pulled in to 729 days before the end.
    let reqs = encode_once(&m, &mut s, &cache, &hist(Timeframe::H1, 1_500_000_000, to), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    let q: Vec<(&str, &str)> = h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(h.path, "/v8/finance/chart/AAPL");
    assert_eq!(q, [("interval", "1h"), ("period1", (to - 729 * 86_400).to_string().as_str()), ("period2", to.to_string().as_str())]);
    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 1_704_067_200, 1_704_292_200), ts(0)).unwrap();
    let body = json(
        r#"{"chart":{"result":[{"meta":{},"timestamp":[1704205800,1704292200,1704378600],
        "indicators":{"quote":[{"open":[187.14999389648438,null,182.15],"high":[188.44,185.88,183.09],"low":[183.89,183.43,180.88],"close":[185.64,184.25,181.91],"volume":[82488700,null,71983600]}],
        "adjclose":[{"adjclose":[183.40402221679688,182.03,180.0]}]}}],"error":null}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "chart", 200, &body), ts(0)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else { panic!() };
    assert_eq!((next.as_deref(), *done), (None, true));
    // The null open is a gap; the bar at period2 is outside [from, to).
    assert_eq!(cs.len(), 1);
    // A 32-bit float's expansion is read back as the price it stands for; a plain decimal
    // (188.44) is kept as sent.
    assert_eq!((cs[0].open.to_string(), cs[0].high.to_string(), cs[0].adjusted.as_ref().and_then(|a| a.close).map(|p| p.to_string())), ("187.15".into(), "188.44".into(), Some("183.40402".into())));
    assert!(!cs[0].is_final, "the engine finalizes");
    assert_history_sound(&[c[0].0.clone()]);
}

#[test]
fn refusals_come_in_the_body_whatever_the_status() {
    let m = Yahoo::new().unwrap();
    let cache = InstrumentCache::new();
    let case = |status: u16, body: &str| {
        let mut s = Session::default();
        encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 1_704_067_200, 1_704_292_200), ts(0)).unwrap();
        decode_once(&m, &mut s, &cache, &http_frame(1, "chart", status, body), ts(0)).unwrap_err()
    };
    let e = case(404, r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found, symbol may be delisted"}}}"#);
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"));
    let e = case(422, r#"{"chart":{"result":null,"error":{"code":"Unprocessable Entity","description":"1h data not available. The requested range must be within the last 730 days."}}}"#);
    assert!(e.kind == ErrorKind::InvalidRequest && e.message.contains("730 days"));
    assert_eq!(case(429, "Edge: Too Many Requests").kind, ErrorKind::RateLimited);
    assert_eq!(case(200, r#"{"chart":{"result":[],"error":null}}"#).kind, ErrorKind::NotFound);
}

#[test]
fn search_keeps_the_venue_order_and_filters_by_market() {
    let m = Yahoo::new().unwrap();
    let cache = InstrumentCache::new();
    let body = json(
        r#"{"quotes":[{"exchange":"NMS","shortname":"Apple Inc.","quoteType":"EQUITY","symbol":"AAPL","exchDisp":"NASDAQ"},
        {"exchange":"CME","longname":"Apple futures","quoteType":"FUTURE","symbol":"SAAPL=F"},
        {"quoteType":"EQUITY"},{"symbol":"APLE","quoteType":"MUTUALFUND","shortname":"Fund"}]}"#,
    );
    let run = |market: Option<AssetClass>| {
        let mut s = Session::default();
        let cmd = Command::SearchInstruments { req_id: CorrId(3), query: "apple inc".into(), asset_class: market };
        let reqs = encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        assert_eq!(h.query[0], ("q".to_string(), "apple inc".to_string()));
        decode_once(&m, &mut s, &cache, &http_frame(3, "search", 200, &body), ts(0))
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.event {
                Event::Instrument { spec } => Some((spec.id.native.clone(), spec.symbol.0.clone(), spec.asset_class, m.search_exchange(&spec))),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let all = run(None);
    assert_eq!(all.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["AAPL", "SAAPL=F", "APLE"]);
    assert_eq!(all[0], ("AAPL".into(), "Apple Inc.".into(), AssetClass::Equity, "NASDAQ".into()));
    assert_eq!((all[1].1.as_str(), all[1].2, all[1].3.as_str()), ("Apple futures", AssetClass::Future, "CME"));
    assert_eq!(all[2].2, AssetClass::Etf);
    assert_eq!(run(Some(AssetClass::Equity)).len(), 1);
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), status in 200u16..600) {
        let m = Yahoo::new().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        encode_once(&m, &mut s, &cache, &hist(Timeframe::D1, 1_704_067_200, 1_704_292_200), ts(0)).unwrap();
        let frame = RawFrame::Http { corr_id: CorrId(1), endpoint_id: "chart", status, headers: vec![], body: bytes.into() };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}
