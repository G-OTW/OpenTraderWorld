//! Massive, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AssetClass, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, ts};

use super::*;

fn m() -> Massive {
    Massive::new().unwrap()
}

fn hist(id: InstrumentId, tf: Timeframe) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: id, data_type: DataType::Candles { tf }, range: TimeRange::new(ts(1_704_067_200), ts(1_704_326_400)), cursor: None }
}

fn q(h: &HttpRequest) -> Vec<(&str, &str)> {
    h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let a = m();
    assert_undeclared_is_unsupported(&a, &InstrumentId::new(VENUE, "AAPL"));
    assert!(!a.capabilities().history.quotes_at.contains(&AssetClass::Spot));
}

#[test]
fn the_market_prefix_is_added_and_rides_the_venue() {
    let a = m();
    let cases = [
        (Some(AssetClass::Equity), "aapl", "MASSIVE", "AAPL"),
        (Some(AssetClass::Option), "SPY251219C00650000", "MASSIVE-OPTIONS", "O:SPY251219C00650000"),
        (Some(AssetClass::Spot), "X:btcusd", "MASSIVE-CRYPTO", "X:BTCUSD"),
        (Some(AssetClass::Forex), "EURUSD", "MASSIVE-FX", "C:EURUSD"),
        (Some(AssetClass::Index), "I:SPX", "MASSIVE-INDICES", "I:SPX"),
        (Some(AssetClass::Future), "esz5", "MASSIVE-FUTURES", "ESZ5"),
    ];
    for (class, sym, venue, native) in cases {
        assert_eq!(a.instrument_for(sym, class).unwrap(), InstrumentId::new(venue, native), "{sym}");
    }
    assert_eq!(a.instrument_id("C:GBPJPY").unwrap().venue.0, "MASSIVE-FX");
    assert!(a.instrument_for("T 4.5 2030", None).is_err());
    assert!(a.instrument_for("X", Some(AssetClass::Bond)).is_err());
}

#[test]
fn aggregates_are_unadjusted_with_an_inclusive_end() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE, "AAPL"), Timeframe::H1), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert!(h.signed);
    assert_eq!(h.path, "/v2/aggs/ticker/AAPL/range/1/hour/1704067200000/1704326399999");
    assert_eq!(q(h), [("adjusted", "false"), ("sort", "asc"), ("limit", "50000")]);
    let body = r#"{"ticker":"AAPL","status":"OK","results":[{"t":1704204000000,"o":187.15,"h":188.44,"l":186.9,"c":187.2,"v":12345,"n":321,"vw":187.1},{"t":1704326400000,"o":1,"h":1,"l":1,"c":1,"v":1}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "aggs", 200, body), ts(0)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else { panic!() };
    assert_eq!((cs.len(), cs[0].trades, cs[0].is_final, next.is_none(), *done), (1, Some(321), false, true, true), "a bar opening at the window's end is the next one's");
    assert_eq!(cs[0].open.to_string(), "187.15");
    // The index endpoint takes no `adjusted`.
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &hist(InstrumentId::new("MASSIVE-INDICES", "I:SPX"), Timeframe::D1), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h), [("sort", "asc"), ("limit", "50000")]);
    // An OK answer with no results is an empty window.
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "aggs", 200, r#"{"status":"OK","resultsCount":0}"#), ts(0)).unwrap();
    assert!(matches!(&chunks(&evs)[0].0, HistoryItems::Candles(c) if c.is_empty()));
}

#[test]
fn futures_ride_their_own_service_in_nanoseconds() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &hist(InstrumentId::new("MASSIVE-FUTURES", "ESZ5"), Timeframe::D1), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.path, "/futures/v1/aggs/ESZ5");
    assert_eq!(
        q(h),
        [("resolution", "1session"), ("window_start.gte", "1704067200000000000"), ("window_start.lt", "1704326400000000000"), ("sort", "window_start.asc"), ("limit", "50000")]
    );
    let body = r#"{"status":"OK","results":[{"window_start":1704114000000000000,"open":4800.25,"high":4810,"low":4790.5,"close":4805,"volume":1200}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "futuresAggs", 200, body), ts(0)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs[0].open_ts, cs[0].close.to_string()), (ts(1_704_114_000), "4805".into()));
}

#[test]
fn an_envelope_status_names_the_fault() {
    let a = m();
    let cache = InstrumentCache::new();
    let cases = [
        (r#"{"status":"NOT_AUTHORIZED","message":"Your plan doesn't include this data timeframe."}"#, ErrorKind::Permission),
        (r#"{"status":"ERROR","error":"You've exceeded the maximum requests per minute"}"#, ErrorKind::RateLimited),
        (r#"{"status":"NOT_FOUND","message":"ticker not found"}"#, ErrorKind::NotFound),
        (r#"{"status":"ERROR","error":"bad timespan"}"#, ErrorKind::InvalidRequest),
    ];
    for (body, kind) in cases {
        let mut s = Session::default();
        encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE, "AAPL"), Timeframe::D1), ts(0)).unwrap();
        let e = decode_once(&a, &mut s, &cache, &http_frame(1, "aggs", 200, body), ts(0)).unwrap_err();
        assert_eq!(e.kind, kind, "{body}");
    }
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE, "AAPL"), Timeframe::D1), ts(0)).unwrap();
    let e = decode_once(&a, &mut s, &cache, &http_frame(1, "aggs", 429, "{}"), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::RateLimited);
    // A delayed (free) answer is served.
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE, "AAPL"), Timeframe::D1), ts(0)).unwrap();
    assert!(decode_once(&a, &mut s, &cache, &http_frame(1, "aggs", 200, r#"{"status":"DELAYED","results":[]}"#), ts(0)).is_ok());
}

#[test]
fn quotes_latest_and_at_a_moment() {
    let a = m();
    let cache = InstrumentCache::new();
    let aapl = InstrumentId::new(VENUE, "AAPL");
    let mut s = Session::default();
    let at = Command::RequestQuoteAt { req_id: CorrId(2), instrument: aapl.clone(), at: ts(1_704_205_800), before: false };
    let reqs = encode_once(&a, &mut s, &cache, &at, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(
        (h.path.as_str(), q(h)),
        ("/v3/quotes/AAPL", vec![("timestamp.gte", "1704205800000000000"), ("order", "asc"), ("sort", "timestamp"), ("limit", "1")])
    );
    let body = r#"{"status":"OK","results":[{"bid_price":187.1,"bid_size":2,"ask_price":187.2,"ask_size":3,"sip_timestamp":1704205800100000000}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(2, "quote", 200, body), ts(0)).unwrap();
    let Event::Quote(qt) = &evs[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.ask.to_string()), ("187.1".into(), "187.2".into()));
    assert!(evs[0].exchange_ts.is_some());
    // A one-sided book is no quote.
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &at, ts(0)).unwrap();
    assert!(decode_once(&a, &mut s, &cache, &http_frame(2, "quote", 200, r#"{"status":"OK","results":[{"bid_price":187.1,"ask_price":0}]}"#), ts(0)).unwrap().is_empty());
    // The latest quote is the last before now, on any market.
    let mut s = Session::default();
    let btc = InstrumentId::new("MASSIVE-CRYPTO", "X:BTCUSD");
    let reqs = encode_once(&a, &mut s, &cache, &Command::RequestSnapshot { req_id: CorrId(3), instrument: btc.clone(), data_type: DataType::Quote }, ts(1_704_205_800)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h)[..2], [("timestamp.lt", "1704205800000000000"), ("order", "desc")]);
    // Crypto keeps no past quotes.
    let past = Command::RequestQuoteAt { req_id: CorrId(4), instrument: btc, at: ts(0), before: false };
    assert_eq!(encode_once(&a, &mut Session::default(), &cache, &past, ts(0)).unwrap_err().kind, ErrorKind::Unsupported);
}

#[test]
fn a_search_is_narrowed_to_a_market() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let cmd = Command::SearchInstruments { req_id: CorrId(5), query: "bitcoin".into(), asset_class: Some(AssetClass::Spot) };
    let reqs = encode_once(&a, &mut s, &cache, &cmd, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h), [("search", "bitcoin"), ("market", "crypto"), ("active", "true"), ("limit", "50")]);
    let body = r#"{"status":"OK","results":[{"ticker":"X:BTCUSD","name":"Bitcoin - United States Dollar","market":"crypto"},{"name":"no ticker"}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(5, "search", 200, body), ts(0)).unwrap();
    let Event::Instrument { spec } = &evs[0].event else { panic!() };
    assert_eq!((spec.id.clone(), spec.asset_class, spec.symbol.0.as_str()), (InstrumentId::new("MASSIVE-CRYPTO", "X:BTCUSD"), AssetClass::Spot, "Bitcoin - United States Dollar"));
    assert_eq!(evs.len(), 2);
    assert!(matches!(evs[1].event, Event::InstrumentsEnd));
}

#[test]
fn the_socket_channel_follows_the_cluster_s_spelling() {
    let cases = [
        (InstrumentId::new(VENUE, "AAPL"), "AM.AAPL"),
        (InstrumentId::new("MASSIVE-INDICES", "I:SPX"), "AM.I:SPX"),
        (InstrumentId::new("MASSIVE-OPTIONS", "O:SPY251219C00650000"), "AM.O:SPY251219C00650000"),
        (InstrumentId::new("MASSIVE-CRYPTO", "X:BTCUSDT"), "XA.BTC-USDT"),
        (InstrumentId::new("MASSIVE-CRYPTO", "X:ETHUSD"), "XA.ETH-USD"),
        (InstrumentId::new("MASSIVE-FX", "C:GBPJPY"), "CA.GBP/JPY"),
        (InstrumentId::new("MASSIVE-FUTURES", "ESZ5"), "AM.ESZ5"),
    ];
    for (id, ch) in cases {
        assert_eq!(stream::channel(&id), ch);
    }
}

fn ws(p: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("aggs".into()), payload: p.to_string().into() }
}

#[test]
fn the_socket_logs_in_before_it_subscribes() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = |id: u64, venue: &str, sym: &str| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(venue, sym), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let out = a.encode_batch(&[sub(1, "MASSIVE-CRYPTO", "X:BTCUSD"), sub(2, "MASSIVE-CRYPTO", "X:ETHUSD")], &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsOpen { path, .. }] if path == "/crypto"), "nothing goes out before the login");
    assert!(decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"connected","message":"Connected Successfully"}]"#), ts(0)).unwrap().is_empty());
    assert!(matches!(&a.keepalive(&mut s, &ctx)[..], [NativeRequest::WsLogin { .. }]));
    decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"auth_success","message":"authenticated"}]"#), ts(0)).unwrap();
    let sent = a.keepalive(&mut s, &ctx);
    let [NativeRequest::WsSend { payload, .. }] = sent.as_slice() else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"action":"subscribe","params":"XA.BTC-USD,XA.ETH-USD"}));
    let out = a.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(1) }], &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsSend { payload, .. }] if std::str::from_utf8(payload).unwrap().contains("unsubscribe")));
    let evs = decode_once(
        &a,
        &mut s,
        &cache,
        &ws(r#"[{"ev":"XA","pair":"ETH-USD","s":1757082660000,"e":1757082720000,"o":1,"h":2,"l":0.5,"c":1.5,"v":9},{"ev":"XA","pair":"BTC-USD","s":1757082660000,"o":1,"h":1,"l":1,"c":1,"v":1}]"#),
        ts(0),
    )
    .unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(2)) && matches!(&e.event, Event::Candle(c) if c.is_final && c.open_ts == ts(1_757_082_660))));
    let e = decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"auth_failed","message":"Your plan doesn't include websocket access."}]"#), ts(0)).unwrap_err();
    assert!(e.kind == ErrorKind::Permission && e.message.contains("paid"));
    let e = decode_once(&a, &mut s, &cache, &ws(r#"{"ev":"status","status":"max_connections","message":"Maximum number of websocket connections exceeded."}"#), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::RateLimited);
    assert_eq!(a.socket_class(&InstrumentId::new("MASSIVE-FX", "C:EURUSD"), &DataType::Candles { tf: Timeframe::M1 }), "forex");
    // Only minute aggregates are on the wire.
    assert_eq!(a.encode_batch(&[Command::Subscribe { sub_id: CorrId(9), instrument: InstrumentId::new(VENUE, "AAPL"), data_type: DataType::Candles { tf: Timeframe::H1 } }], &mut Session::default(), &ctx).unwrap_err().kind, ErrorKind::Unsupported);
}

#[test]
fn the_quote_channel_follows_the_cluster_s_spelling() {
    let cases = [
        (InstrumentId::new(VENUE, "AAPL"), Some("Q.AAPL")),
        (InstrumentId::new("MASSIVE-OPTIONS", "O:SPY251219C00650000"), Some("Q.O:SPY251219C00650000")),
        (InstrumentId::new("MASSIVE-CRYPTO", "X:BTCUSD"), Some("XQ.BTC-USD")),
        (InstrumentId::new("MASSIVE-FX", "C:EURUSD"), Some("C.EUR/USD")),
        (InstrumentId::new("MASSIVE-FUTURES", "ESZ5"), Some("Q.ESZ5")),
        (InstrumentId::new("MASSIVE-INDICES", "I:SPX"), None),
    ];
    for (id, ch) in cases {
        assert_eq!(stream::quote_channel(&id).as_deref(), ch);
    }
}

fn logged_in(a: &Massive, s: &mut Session, cache: &InstrumentCache, ctx: &MapCtx<'_>) -> Vec<serde_json::Value> {
    decode_once(a, s, cache, &ws(r#"[{"ev":"status","status":"connected","message":"Connected Successfully"}]"#), ts(0)).unwrap();
    a.keepalive(s, ctx);
    decode_once(a, s, cache, &ws(r#"[{"ev":"status","status":"auth_success","message":"authenticated"}]"#), ts(0)).unwrap();
    a.keepalive(s, ctx)
        .iter()
        .map(|r| match r {
            NativeRequest::WsSend { payload, .. } => serde_json::from_slice(payload).unwrap(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn quotes_go_in_their_own_frame_and_a_refusal_fails_only_them() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let bars = Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new(VENUE, "MSFT"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let quote = Command::Subscribe { sub_id: CorrId(2), instrument: InstrumentId::new(VENUE, "MSFT"), data_type: DataType::Quote };
    a.encode_batch(&[bars, quote], &mut s, &ctx).unwrap();
    assert_eq!(
        logged_in(&a, &mut s, &cache, &ctx),
        vec![serde_json::json!({"action":"subscribe","params":"AM.MSFT"}), serde_json::json!({"action":"subscribe","params":"Q.MSFT"})],
        "the quote in a frame of its own"
    );
    // The documented quote: sizes in shares.
    let msg = r#"[{"ev":"Q","sym":"MSFT","bx":4,"bp":114.125,"bs":100,"ax":7,"ap":114.128,"as":160,"c":0,"i":[604],"t":1536036818784,"q":50385480,"z":3}]"#;
    let evs = decode_once(&a, &mut s, &cache, &ws(msg), ts(1_536_036_819)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(2)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_536_036_818_784));
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string(), q.bid_qty.map(|x| x.to_string())), ("114.125".into(), "114.128".into(), Some("100".into())));
    otw_connector_testkit::assert_events_sound(&evs);
    // A one-sided market omits a side: no quote.
    assert!(decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"Q","sym":"MSFT","ap":114.2,"as":10,"t":1536036818790}]"#), ts(0)).unwrap().is_empty());

    // A plan without quotes: the refusal fails the quote pane, the socket and the bars go on.
    let mut s = Session::default();
    let bars = Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new(VENUE, "MSFT"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let quote = Command::Subscribe { sub_id: CorrId(2), instrument: InstrumentId::new(VENUE, "MSFT"), data_type: DataType::Quote };
    a.encode_batch(&[bars, quote], &mut s, &ctx).unwrap();
    logged_in(&a, &mut s, &cache, &ctx);
    let ok = decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"success","message":"subscribed to: AM.MSFT"}]"#), ts(0)).unwrap();
    assert!(ok.is_empty(), "a bar channel's confirmation is not a quote's");
    let refused = decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"error","message":"not authorized"}]"#), ts(0)).unwrap();
    assert!(matches!(&refused[..], [e] if e.corr_id == Some(CorrId(2)) && matches!(&e.event, Event::SubscriptionFail { error } if error.kind == ErrorKind::Permission)));
    let evs = decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"AM","sym":"MSFT","s":1757082660000,"o":1,"h":2,"l":0.5,"c":1.5,"v":9}]"#), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(1))), "the bars still stream");
    // Once confirmed, a later error is the socket's again, as before.
    let mut s = Session::default();
    let quote = Command::Subscribe { sub_id: CorrId(3), instrument: InstrumentId::new("MASSIVE-CRYPTO", "X:BTCUSD"), data_type: DataType::Quote };
    a.encode_batch(&[quote], &mut s, &ctx).unwrap();
    logged_in(&a, &mut s, &cache, &ctx);
    let ack = decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"success","message":"subscribed to: XQ.BTC-USD"}]"#), ts(0)).unwrap();
    assert!(matches!(&ack[..], [e] if e.corr_id == Some(CorrId(3)) && matches!(e.event, Event::SubscriptionAck)));
    let crypto = r#"[{"ev":"XQ","pair":"BTC-USD","bp":33052.79,"bs":0.48,"ap":33073.19,"as":0.601,"t":1610462411115,"x":1,"r":1610462411128}]"#;
    assert!(matches!(&decode_once(&a, &mut s, &cache, &ws(crypto), ts(1_610_462_412)).unwrap()[0].event, Event::Quote(_)));
    assert!(decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"status","status":"error","message":"something else"}]"#), ts(0)).is_err());
    // Forex quotes name their pair in `p` and their prices `b` / `a`; an index has none.
    let mut s = Session::default();
    let fx = Command::Subscribe { sub_id: CorrId(4), instrument: InstrumentId::new("MASSIVE-FX", "C:EURUSD"), data_type: DataType::Quote };
    let index = Command::Subscribe { sub_id: CorrId(5), instrument: InstrumentId::new("MASSIVE-INDICES", "I:SPX"), data_type: DataType::Quote };
    a.encode_batch(&[fx, index], &mut s, &ctx).unwrap();
    logged_in(&a, &mut s, &cache, &ctx);
    let evs = decode_once(&a, &mut s, &cache, &ws(r#"[{"ev":"C","p":"EUR/USD","x":"44","a":1.08366,"b":1.08363,"t":1536036818784}]"#), ts(1_536_036_819)).unwrap();
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string(), q.bid_qty), ("1.08363".into(), "1.08366".into(), None));
}

#[test]
fn the_signer_puts_the_key_in_the_query_and_the_login() {
    let a = m();
    assert_eq!(a.signer(&HashMap::new()).err().map(|e| e.kind), Some(ErrorKind::Auth));
    let signer = a.signer(&HashMap::from([("api_key".to_string(), " k1 ".to_string())])).unwrap().unwrap();
    let login = signer.ws_login(ts(0)).unwrap().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&login).unwrap(), serde_json::json!({"action":"auth","params":"k1"}));
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..3) {
        let a = m();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let frame = match which {
            0 => {
                encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE, "AAPL"), Timeframe::M1), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(1), endpoint_id: "aggs", status: 200, headers: vec![], body: bytes.into() }
            }
            1 => {
                encode_once(&a, &mut s, &cache, &hist(InstrumentId::new("MASSIVE-FUTURES", "ESZ5"), Timeframe::M1), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(1), endpoint_id: "futuresAggs", status: 200, headers: vec![], body: bytes.into() }
            }
            _ => RawFrame::Ws { conn_key: ConnKey("aggs".into()), payload: bytes.into() },
        };
        let _ = decode_once(&a, &mut s, &cache, &frame, ts(0));
    }
}
