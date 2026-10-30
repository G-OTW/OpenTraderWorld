//! Binance spot, offline. Fixtures are synthetic, written from the documented schema
//! (developers.binance.com, spot REST market-data endpoints); nothing here was captured.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::str::FromStr;

use otw_connector_model::{
    Command, ConnectorError, CorrId, DataType, Decimal, ErrorKind, Event, HistoryItems, InstrumentId, Maturity, Price,
    TimeRange, Timeframe,
};
use otw_connector_sdk::{InstrumentCache, NativeRequest};
use otw_connector_testkit::wiremock::matchers::{method, path, query_param};
use otw_connector_testkit::wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use otw_connector_testkit::{
    assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, driver_against, encode_once, http_frame, json,
    ts, ManualClock,
};

use super::*;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn btc() -> InstrumentId {
    InstrumentId::new(VENUE, "BTCUSDT")
}

/// One kline row as Binance documents it: times as numbers, prices as strings.
fn row(open_ms: i64, o: &str, h: &str, l: &str, c: &str, v: &str, step_ms: i64) -> String {
    format!(r#"[{open_ms},"{o}","{h}","{l}","{c}","{v}",{},"{}",42,"0","0","0"]"#, open_ms + step_ms - 1, "1000.5")
}

fn history_cmd(req: u64, tf: Timeframe, from: i64, to: i64, cursor: Option<&str>) -> Command {
    Command::RequestHistory {
        req_id: CorrId(req),
        instrument: btc(),
        data_type: DataType::Candles { tf },
        range: TimeRange::new(ts(from), ts(to)),
        cursor: cursor.map(str::to_string),
    }
}

#[test]
fn capabilities_parse_and_match_the_legacy_connector_set() {
    let m = Binance::spot().unwrap();
    let c = m.capabilities();
    assert_eq!(c.provider, "binance");
    assert_eq!(c.maturity, Maturity::M2);
    let tfs: Vec<String> = c.history_timeframes().iter().map(|t| t.to_string()).collect();
    assert_eq!(tfs, ["1m", "5m", "15m", "1h", "4h", "1d", "1w"]);
}

#[test]
fn klines_request_is_built_exactly() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &InstrumentCache::new(), &history_cmd(7, Timeframe::H1, 1_700_000_000, 1_700_360_000, None), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!("one HTTP request") };
    assert_eq!((h.endpoint_id, h.path.as_str(), h.weight, h.signed), ("klines", "/api/v3/klines", 2, false));
    let q: Vec<(&str, &str)> = h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(
        q,
        [
            ("symbol", "BTCUSDT"),
            ("interval", "1h"),
            ("startTime", "1700000000000"),
            ("endTime", "1700360000000"),
            ("limit", "1000")
        ]
    );
    let with_cursor = encode_once(&m, &mut s, &InstrumentCache::new(), &history_cmd(8, Timeframe::H1, 1_700_000_000, 1_700_360_000, Some("1700003600000")), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = with_cursor.as_slice() else { panic!() };
    assert!(h.query.contains(&("startTime".into(), "1700003600000".into())));
}

#[test]
fn klines_decode_exact_decimals_range_and_forming_bar() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let step = 3_600_000;
    let t0 = 1_699_999_200_000; // one bar before the range
    encode_once(&m, &mut s, &cache, &history_cmd(1, Timeframe::H1, 1_700_002_800, 1_700_013_600, None), ts(0)).unwrap();
    let body = json(&format!(
        "[{},{},{},{}]",
        row(t0, "1", "1", "1", "1", "1", step),
        row(1_700_002_800_000, "37000.10", "37100.00", "36950.5", "37050.01", "12.34567891", step),
        row(1_700_006_400_000, "37050.01", "37060", "37000", "37001", "0.1", step),
        row(1_700_010_000_000, "37001", "37002", "37000", "37000.5", "0.2", step),
    ));
    // "now" is inside the last bar: it is still forming.
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, &body), ts(1_700_011_000)).unwrap();
    let c = chunks(&evs);
    assert_eq!(c.len(), 1);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else { panic!() };
    assert_eq!(cs.len(), 3, "the bar before the range is dropped");
    assert_eq!(cs[0].open, Price(d("37000.10")));
    assert_eq!(cs[0].low.to_string(), "36950.5");
    assert_eq!(cs[0].volume.to_string(), "12.34567891");
    assert_eq!(cs[0].trades, Some(42));
    assert!(cs[0].is_final && cs[1].is_final);
    assert!(!cs[2].is_final, "the bar whose closeTime has not passed is forming");
    assert_eq!((next, done), (&None, &true), "a short page ends the walk");
    assert_history_sound(&[c[0].0.clone()]);
}

#[test]
fn a_full_page_hands_back_the_next_start_and_repeats_are_dropped() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let step = 60_000;
    let from = 1_700_000_000;
    encode_once(&m, &mut s, &cache, &history_cmd(1, Timeframe::M1, from, from + 3 * 86_400, None), ts(0)).unwrap();
    let rows: Vec<String> = (0..1000).map(|i| row(from * 1000 + i * step, "1", "2", "1", "2", "1", step)).collect();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, &format!("[{}]", rows.join(","))), ts(from + 4 * 86_400)).unwrap();
    let c = chunks(&evs);
    let expected = (from * 1000 + 1000 * step).to_string();
    assert_eq!((c[0].1.as_deref(), c[0].2), (Some(expected.as_str()), false));

    // Second page starts at the cursor; a venue repeating the previous bar is not believed.
    encode_once(&m, &mut s, &cache, &history_cmd(1, Timeframe::M1, from, from + 3 * 86_400, Some(&expected)), ts(0)).unwrap();
    let repeat = row(from * 1000 + 999 * step, "1", "2", "1", "2", "1", step);
    let next = row(from * 1000 + 1000 * step, "1", "2", "1", "2", "1", step);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, &format!("[{repeat},{next}]")), ts(from + 4 * 86_400)).unwrap();
    let (HistoryItems::Candles(cs), ..) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.len(), 1);
}

#[test]
fn instruments_carry_filters_status_and_round_trip() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(0)).unwrap();
    let body = json(
        r#"{"timezone":"UTC","symbols":[
        {"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","filters":[
            {"filterType":"PRICE_FILTER","minPrice":"0.01","maxPrice":"1000000","tickSize":"0.01000000"},
            {"filterType":"LOT_SIZE","minQty":"0.00001000","maxQty":"9000","stepSize":"0.00001000"},
            {"filterType":"NOTIONAL","minNotional":"5"}]},
        {"symbol":"OLDBTC","status":"BREAK","baseAsset":"OLD","quoteAsset":"BTC","filters":[]}]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(3, "exchangeInfo", 200, &body), ts(0)).unwrap();
    let specs: Vec<_> = evs
        .iter()
        .filter_map(|e| match &e.event {
            Event::Instrument { spec } => Some(spec.as_ref().clone()),
            _ => None,
        })
        .collect();
    assert!(matches!(evs.last().map(|e| &e.event), Some(Event::InstrumentsEnd)));
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].symbol.0, "BTC/USDT");
    assert_eq!(specs[0].tick_size.map(|p| p.0), Some(d("0.01")));
    assert_eq!(specs[0].min_qty.map(|q| q.0), Some(d("0.00001")));
    // The trading rules beyond tick and lot ride on the instrument (spec 4.1).
    let r = &specs[0].rules;
    assert_eq!((r.min_price, r.max_price, r.max_qty), (Some(d("0.01")), Some(d("1000000")), Some(d("9000"))));
    assert_eq!(r.min_notional.as_ref().map(|n| (n.value, n.filter.as_str())), Some((d("5"), "NOTIONAL")));
    assert!(specs[1].rules == Default::default(), "a symbol with no filters checks nothing");
    assert!(specs[0].active && !specs[1].active);
    let mut c = InstrumentCache::new();
    for sp in specs {
        c.insert(sp);
    }
    assert!(c.round_trip_failures().is_empty());
}

#[test]
fn spot_reads_one_symbols_terms_without_the_list() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let one = Command::ResolveInstruments { req_id: CorrId(4), ids: vec![InstrumentId::new(VENUE, "BTCUSDT")] };
    let reqs = encode_once(&m, &mut s, &cache, &one, ts(0)).unwrap();
    let NativeRequest::Http(h) = &reqs[0] else { panic!() };
    assert_eq!(h.path.as_str(), "/api/v3/exchangeInfo");
    assert_eq!(h.query, vec![("symbol".to_string(), "BTCUSDT".to_string())]);
    let body = json(r#"{"timezone":"UTC","symbols":[{"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","filters":[{"filterType":"LOT_SIZE","minQty":"0.00001","maxQty":"9000","stepSize":"0.00001"}]}]}"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "exchangeInfo", 200, &body), ts(0)).unwrap();
    let lot = evs.iter().find_map(|e| match &e.event {
        Event::Instrument { spec } => spec.lot_size.map(|q| q.0),
        _ => None,
    });
    assert_eq!(lot, Some(d("0.00001")));
    let two = Command::ResolveInstruments { req_id: CorrId(5), ids: vec![InstrumentId::new(VENUE, "BTCUSDT"), InstrumentId::new(VENUE, "ETHBTC")] };
    let NativeRequest::Http(h) = &encode_once(&m, &mut s, &cache, &two, ts(0)).unwrap()[0] else { panic!() };
    assert_eq!(h.query, vec![("symbols".to_string(), r#"["BTCUSDT","ETHBTC"]"#.to_string())]);
}

#[test]
fn quote_snapshot_and_a_mismatched_answer() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = Command::RequestSnapshot { req_id: CorrId(4), instrument: btc(), data_type: DataType::Quote };
    encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
    let body = json(r#"{"symbol":"BTCUSDT","bidPrice":"37000.01","bidQty":"1.5","askPrice":"37000.02","askQty":"0.25"}"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "bookTicker", 200, &body), ts(0)).unwrap();
    let Event::Quote(q) = &evs[0].event else { panic!() };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("37000.01".into(), "37000.02".into()));
    encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
    let other = json(r#"{"symbol":"ETHUSDT","bidPrice":"1","bidQty":"1","askPrice":"2","askQty":"1"}"#);
    assert!(decode_once(&m, &mut s, &cache, &http_frame(4, "bookTicker", 200, &other), ts(0)).is_err());
}

#[test]
fn symbols_are_exact_or_refused() {
    let m = Binance::spot().unwrap();
    assert_eq!(m.instrument_id("btcusdt").unwrap(), btc());
    for bad in ["", "BTC/USDT", "BTC-USDT", "BTC USDT"] {
        let e = m.instrument_id(bad).unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotFound, "{bad}");
    }
}

#[test]
fn undeclared_capabilities_are_refused() {
    let m = Binance::spot().unwrap();
    assert_undeclared_is_unsupported(&m, &btc());
}

#[test]
fn an_answer_nobody_asked_for_is_a_decode_error() {
    let m = Binance::spot().unwrap();
    let e = decode_once(&m, &mut Session::default(), &InstrumentCache::new(), &http_frame(99, "klines", 200, "[]"), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Decode);
}

// ── Mock venue: the driver end to end ─────────────────────────────────────────

/// Serves 1m klines for `[startTime, endTime]` from a fixed synthetic tape of `total` bars.
struct Tape {
    first_ms: i64,
    total: i64,
}

impl Respond for Tape {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let p = |k: &str| req.url.query_pairs().find(|(q, _)| q == k).and_then(|(_, v)| v.parse::<i64>().ok());
        let (start, end, limit) = (p("startTime").unwrap_or(0), p("endTime").unwrap_or(i64::MAX), p("limit").unwrap_or(500));
        let rows: Vec<String> = (0..self.total)
            .map(|i| self.first_ms + i * 60_000)
            .filter(|t| *t >= start && *t <= end)
            .take(limit as usize)
            .map(|t| row(t, "10", "11", "9", "10.5", "3", 60_000))
            .collect();
        ResponseTemplate::new(200).set_body_string(format!("[{}]", rows.join(",")))
    }
}

#[tokio::test]
async fn driver_walks_every_page_without_gap_or_repeat() {
    let server = MockServer::start().await;
    let from = 1_700_000_040; // a minute boundary
    Mock::given(method("GET")).and(path("/api/v3/klines")).respond_with(Tape { first_ms: from * 1000, total: 2_345 }).mount(&server).await;
    let clock = ManualClock::at(ts(from + 10 * 86_400));
    let drv = driver_against(Binance::spot().unwrap(), &server, clock);
    let range = TimeRange::new(ts(from), ts(from + 2_345 * 60));
    let got = drv.history(CorrId(1), &btc(), &DataType::Candles { tf: Timeframe::M1 }, range).await.unwrap();
    assert_eq!(got.len(), 3, "1000 + 1000 + 345");
    assert_eq!(got.iter().map(|c| c.len()).sum::<usize>(), 2_345);
    assert_history_sound(&got);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn driver_reports_the_venue_errors_by_kind() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v3/klines"))
        .and(query_param("symbol", "LIMITED"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "7").set_body_string(r#"{"code":-1003,"msg":"Too many requests"}"#))
        .mount(&server)
        .await;
    Mock::given(path("/api/v3/klines"))
        .and(query_param("symbol", "DOWN"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .mount(&server)
        .await;
    Mock::given(path("/api/v3/klines"))
        .and(query_param("symbol", "NOPE"))
        .respond_with(ResponseTemplate::new(400).set_body_string(r#"{"code":-1121,"msg":"Invalid symbol."}"#))
        .mount(&server)
        .await;
    Mock::given(path("/api/v3/klines"))
        .and(query_param("symbol", "GARBLED"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[[1700000000000,\"1\""))
        .mount(&server)
        .await;
    Mock::given(path("/api/v3/klines"))
        .and(query_param("symbol", "EMPTY"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let drv = driver_against(Binance::spot().unwrap(), &server, ManualClock::at(ts(1_800_000_000)));
    let range = TimeRange::new(ts(1_700_000_000), ts(1_700_086_400));
    let run = |sym: &'static str| {
        let drv = &drv;
        async move {
            drv.history(CorrId(1), &InstrumentId::new(VENUE, sym), &DataType::Candles { tf: Timeframe::M1 }, range).await
        }
    };
    let e: ConnectorError = run("LIMITED").await.unwrap_err();
    assert_eq!((e.kind, e.retry_after_ms), (ErrorKind::RateLimited, Some(7000)));
    assert_eq!(run("DOWN").await.unwrap_err().kind, ErrorKind::Transient);
    assert_eq!(run("NOPE").await.unwrap_err().kind, ErrorKind::NotFound);
    assert_eq!(run("GARBLED").await.unwrap_err().kind, ErrorKind::Decode);
    let empty = run("EMPTY").await.unwrap();
    assert_eq!((empty.len(), empty[0].len()), (1, 0), "an empty answer is one empty, finished chunk");
}

#[tokio::test]
async fn driver_times_out_instead_of_hanging() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v3/klines"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]").set_delay(std::time::Duration::from_secs(3)))
        .mount(&server)
        .await;
    let mut cfg = otw_connector_sdk::DriverConfig::new("t").base("rest", &server.uri());
    cfg.timeout = std::time::Duration::from_millis(200);
    let drv = otw_connector_sdk::Driver::new(Binance::spot().unwrap(), cfg, reqwest::Client::new());
    let range = TimeRange::new(ts(1_700_000_000), ts(1_700_086_400));
    let e = drv.history(CorrId(1), &btc(), &DataType::Candles { tf: Timeframe::M1 }, range).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Transient);
}

#[test]
fn a_book_ticker_rides_the_kline_socket_and_lands_on_its_quote() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let quote = Command::Subscribe { sub_id: CorrId(7), instrument: btc(), data_type: DataType::Quote };
    let out = batch(&m, &mut s, &[sub(1, "BTCUSDT", Timeframe::M1), quote]).unwrap();
    assert_eq!(out[0], NativeRequest::WsOpen { conn_key: otw_connector_sdk::ConnKey("main".into()), base: "ws", path: "/ws/btcusdt@kline_1m".into() });
    assert_eq!(sent(&out[1])["params"], serde_json::json!(["btcusdt@kline_1m", "btcusdt@bookTicker"]));
    assert_eq!(m.socket_class(&btc(), &DataType::Quote), "", "spot carries both on one socket");
    // Recorded from the spot stream: no event type, no event time.
    let text = r#"{"u":101628173415,"s":"BTCUSDT","b":"82280.00000000","B":"0.40541000","a":"82280.01000000","A":"3.91138000"}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(text), ts(1_700_000_031)).unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].corr_id, Some(CorrId(7)));
    assert_eq!(evs[0].exchange_ts, None, "spot sends no event time: stamped on receipt");
    let Event::Quote(q) = &evs[0].event else { panic!("{:?}", evs[0].event) };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82280.00000000".to_string(), "82280.01000000".to_string()));
    assert_eq!(q.bid_qty.map(|x| x.to_string()), Some("0.40541000".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);
    // An empty side is no quote, and a symbol nobody holds is dropped.
    let empty = text.replace("82280.00000000", "0.00000000");
    assert!(decode_once(&m, &mut s, &cache, &ws(&empty), ts(0)).unwrap().is_empty());
    assert!(decode_once(&m, &mut s, &cache, &ws(&text.replace("BTCUSDT", "ETHUSDT")), ts(0)).unwrap().is_empty());
    let gone = batch(&m, &mut s, &[Command::Unsubscribe { sub_id: CorrId(7) }]).unwrap();
    assert_eq!(sent(&gone[0])["params"], serde_json::json!(["btcusdt@bookTicker"]));
}

proptest::proptest! {
    /// `decode` never panics, whatever the bytes.
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..3) {
        let m = Binance::spot().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let cmd = match which {
            0 => history_cmd(1, Timeframe::M1, 1_700_000_000, 1_700_086_400, None),
            1 => Command::LoadInstruments { req_id: CorrId(1) },
            _ => Command::RequestSnapshot { req_id: CorrId(1), instrument: btc(), data_type: DataType::Quote },
        };
        encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
        let frame = otw_connector_sdk::RawFrame::Http { corr_id: CorrId(1), endpoint_id: "x", status: 200, headers: vec![], body: bytes.into() };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}

// ── Socket ────────────────────────────────────────────────────────────────────

fn sub(id: u64, native: &str, tf: Timeframe) -> Command {
    Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(VENUE, native), data_type: DataType::Candles { tf } }
}

fn ws(payload: &str) -> otw_connector_sdk::RawFrame {
    otw_connector_sdk::RawFrame::Ws { conn_key: otw_connector_sdk::ConnKey("main".into()), payload: payload.to_string().into() }
}

fn sent(r: &NativeRequest) -> serde_json::Value {
    match r {
        NativeRequest::WsSend { payload, .. } => serde_json::from_slice(payload).unwrap(),
        other => panic!("not a frame: {other:?}"),
    }
}

fn batch(m: &BinanceSpot, s: &mut Session, cmds: &[Command]) -> Result<Vec<NativeRequest>, ConnectorError> {
    let c = otw_connector_model::ConnectorId("t".into());
    let cache = InstrumentCache::new();
    let ctx = otw_connector_sdk::MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    m.encode_batch(cmds, s, &ctx)
}

#[test]
fn panes_joining_together_open_one_socket_and_send_one_frame() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let out = batch(&m, &mut s, &[sub(1, "BTCUSDT", Timeframe::M1), sub(2, "ETHUSDT", Timeframe::H1)]).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0], NativeRequest::WsOpen { conn_key: otw_connector_sdk::ConnKey("main".into()), base: "ws", path: "/ws/btcusdt@kline_1m".into() });
    assert_eq!(sent(&out[1]), serde_json::json!({"method":"SUBSCRIBE","params":["btcusdt@kline_1m","ethusdt@kline_1h"],"id":2}));
    // A later pane is one more frame on the same socket.
    let more = batch(&m, &mut s, &[sub(3, "SOLUSDT", Timeframe::M5)]).unwrap();
    assert_eq!(more.len(), 1);
    assert_eq!(sent(&more[0])["id"], 3);
    let gone = batch(&m, &mut s, &[Command::Unsubscribe { sub_id: CorrId(2) }]).unwrap();
    assert_eq!(sent(&gone[0]), serde_json::json!({"method":"UNSUBSCRIBE","params":["ethusdt@kline_1h"],"id":4}));
    assert!(batch(&m, &mut s, &[Command::Unsubscribe { sub_id: CorrId(2) }]).unwrap().is_empty(), "twice is nothing");
}

#[test]
fn a_kline_lands_on_its_subscription_with_the_venue_clock() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    batch(&m, &mut s, &[sub(1, "BTCUSDT", Timeframe::M1), sub(2, "BTCUSDT", Timeframe::H1)]).unwrap();
    let text = r#"{"e":"kline","E":1700000030123,"s":"BTCUSDT","k":{"t":1699999200000,"T":1700002799999,"s":"BTCUSDT","i":"1h",
        "o":"37000.10","c":"37010.5","h":"37020","l":"36990.01","v":"12.5","n":321,"x":false,"q":"462500.1"}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(text), ts(1_700_000_031)).unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].corr_id, Some(CorrId(2)), "routed by symbol and interval");
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_700_000_030_123));
    let Event::Candle(c) = &evs[0].event else { panic!() };
    assert_eq!((c.tf, c.open.to_string(), c.is_final, c.trades), (Timeframe::H1, "37000.10".to_string(), false, Some(321)));
    otw_connector_testkit::assert_events_sound(&evs);
    // Nobody holds ETH: dropped, not misrouted.
    let eth = text.replace("BTCUSDT", "ETHUSDT");
    assert!(decode_once(&m, &mut s, &cache, &ws(&eth), ts(0)).unwrap().is_empty());
}

#[test]
fn acks_and_refusals_answer_the_frame_they_belong_to() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    batch(&m, &mut s, &[sub(1, "BTCUSDT", Timeframe::M1), sub(2, "ETHUSDT", Timeframe::M1)]).unwrap();
    let acks = decode_once(&m, &mut s, &cache, &ws(r#"{"result":null,"id":2}"#), ts(0)).unwrap();
    assert_eq!(acks.len(), 2);
    assert!(acks.iter().all(|e| matches!(e.event, Event::SubscriptionAck)));
    batch(&m, &mut s, &[sub(3, "XXXUSDT", Timeframe::M1)]).unwrap();
    let no = decode_once(&m, &mut s, &cache, &ws(r#"{"code":2,"msg":"Invalid request: unknown stream","id":3}"#), ts(0)).unwrap();
    assert!(matches!(&no[0].event, Event::SubscriptionFail { .. }));
    assert_eq!(no[0].corr_id, Some(CorrId(3)));
}

#[test]
fn a_broken_frame_is_a_decode_error() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    batch(&m, &mut s, &[sub(1, "BTCUSDT", Timeframe::M1)]).unwrap();
    assert_eq!(decode_once(&m, &mut s, &cache, &ws("{not json"), ts(0)).unwrap_err().kind, ErrorKind::Decode);
    let missing = r#"{"e":"kline","s":"BTCUSDT","k":{"t":1700000000000,"i":"1m","o":"1","h":"1","l":"1","c":"1"}}"#;
    assert_eq!(decode_once(&m, &mut s, &cache, &ws(missing), ts(0)).unwrap_err().kind, ErrorKind::Decode);
}

#[test]
fn only_declared_streams_are_accepted() {
    let m = Binance::spot().unwrap();
    let mut s = Session::default();
    let odd: Timeframe = "3m".parse().unwrap();
    assert_eq!(batch(&m, &mut s, &[sub(1, "BTCUSDT", odd)]).unwrap_err().kind, ErrorKind::Unsupported);
    let trades = Command::Subscribe { sub_id: CorrId(2), instrument: btc(), data_type: DataType::Trades };
    assert_eq!(batch(&m, &mut s, &[trades]).unwrap_err().kind, ErrorKind::Unsupported);
    // 1s is a socket-only resolution: streamed, never downloaded.
    assert!(batch(&m, &mut s, &[sub(3, "BTCUSDT", "1s".parse().unwrap())]).is_ok());
    assert!(!m.capabilities().history_timeframes().contains(&"1s".parse().unwrap()));
}

proptest::proptest! {
    #[test]
    fn socket_decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512)) {
        let m = Binance::spot().unwrap();
        let mut s = Session::default();
        batch(&m, &mut s, &[sub(1, "BTCUSDT", Timeframe::M1)]).unwrap();
        let frame = otw_connector_sdk::RawFrame::Ws { conn_key: otw_connector_sdk::ConnKey("main".into()), payload: bytes.into() };
        let _ = decode_once(&m, &mut s, &InstrumentCache::new(), &frame, ts(0));
    }
}
