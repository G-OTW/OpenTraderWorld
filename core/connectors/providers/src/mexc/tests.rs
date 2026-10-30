//! MEXC, offline. Market-data fixtures are answers recorded from the public endpoints and
//! sockets on 2026-10-19; account fixtures follow the samples of MEXC's own API docs (spot
//! v3, contract v1) and ccxt's.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

/// The moment the recorded answers were taken: Monday 2026-10-19 08:04:26 UTC.
const NOW: i64 = 1_792_399_466;

fn hist(m: &Mexc, native: &str, tf: Timeframe, from: i64, to: i64, cursor: Option<String>) -> Command {
    Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(m.book().venue(), native),
        data_type: DataType::Candles { tf },
        range: TimeRange::new(ts(from), ts(to)),
        cursor,
    }
}

fn q(h: &HttpRequest, k: &str) -> Option<String> {
    h.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}

fn one(reqs: &[NativeRequest]) -> &HttpRequest {
    let [NativeRequest::Http(h)] = reqs else { panic!("{reqs:?}") };
    h
}

/// Recorded (trimmed).
const BTC_CONTRACT: &str = r#"{"symbol":"BTC_USDT","displayNameEn":"BTC_USDT PERPETUAL","baseCoin":"BTC","quoteCoin":"USDT","futureType":1,"settleCoin":"USDT","contractSize":0.0001,"priceScale":1,"volScale":0,"priceUnit":0.1,"volUnit":1,"minVol":1,"maxVol":400000,"state":0,"apiAllowed":true}"#;

fn btc_cache() -> InstrumentCache {
    let mut cache = InstrumentCache::new();
    let row: ContractRow = serde_json::from_str(BTC_CONTRACT).unwrap();
    cache.insert(Mexc::contract_spec(&row).unwrap());
    cache
}

#[test]
fn each_book_declares_only_what_it_serves() {
    for (m, native) in [(Mexc::spot().unwrap(), "BTCUSDT"), (Mexc::futures().unwrap(), "BTC_USDT")] {
        assert_undeclared_is_unsupported(&m, &InstrumentId::new(m.book().venue(), native));
        assert_eq!(m.capabilities().provider, "mexc");
    }
}

#[test]
fn spot_pages_in_half_open_millisecond_windows() {
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_399_260, 1_792_399_440);
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTCUSDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let h = one(&reqs);
    assert_eq!(h.path, "/api/v3/klines");
    assert_eq!(
        (q(h, "symbol").as_deref(), q(h, "interval").as_deref(), q(h, "startTime").as_deref(), q(h, "endTime").as_deref()),
        (Some("BTCUSDT"), Some("1m"), Some("1792399260000"), Some("1792399440000")),
        "`endTime` is exclusive on the open time"
    );
    // Recorded: `[open ms, open, high, low, close, base volume, close ms, quote volume]`.
    let body = r#"[[1792399260000,"82911.48","82944.42","82911.48","82921","1.64033067",1792399320000,"136041.7"],[1792399320000,"82921","82939.67","82882.89","82887.99","1.97786167",1792399380000,"163985.77"],[1792399380000,"82887.99","82887.99","82865.6","82877.45","2.12964302",1792399440000,"176492.01"]]"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs.len(), next.as_ref(), *done), (3, None, true));
    let b = &cs[1];
    assert_eq!((b.open.to_string(), b.high.to_string(), b.low.to_string(), b.close.to_string()), ("82921".into(), "82939.67".into(), "82882.89".into(), "82887.99".into()));
    assert_eq!((b.volume.to_string(), b.quote_volume.unwrap().to_string()), ("1.97786167".into(), "163985.77".into()));
    assert!(cs.iter().all(|c| c.is_final), "closed for more than the 10 s settle window");
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);
    // Twenty seconds earlier the last minute was still settling.
    encode_once(&m, &mut s, &cache, &hist(&m, "BTCUSDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, body), ts(NOW - 20)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, true, false]);
}

#[test]
fn a_long_range_starts_at_the_archives_floor_and_walks_forwards() {
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // Sixty days of minutes: the archive keeps thirty, and answers nothing at all for a
    // window that starts before them (measured).
    let (from, to) = (NOW - 60 * 86_400, NOW - NOW % 60);
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTCUSDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let start: i64 = q(one(&reqs), "startTime").unwrap().parse::<i64>().unwrap() / 1000;
    let end: i64 = q(one(&reqs), "endTime").unwrap().parse::<i64>().unwrap() / 1000;
    assert!((NOW - 30 * 86_400..=NOW - 30 * 86_400 + 3_660).contains(&start), "starts at the floor: {start}");
    assert_eq!((end - start) / 60, 500, "a page is 500 bars");
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, "[]"), ts(NOW)).unwrap();
    let (_, next, done) = &chunks(&evs)[0];
    assert!(!done);
    assert_eq!(next.as_deref(), Some(end.to_string().as_str()));
    let mut cursor = next.clone();
    let mut pages = 1;
    let mut last_end = end;
    while let Some(c) = cursor {
        let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTCUSDT", Timeframe::M1, from, to, Some(c)), ts(NOW)).unwrap();
        last_end = q(one(&reqs), "endTime").unwrap().parse::<i64>().unwrap() / 1000;
        let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, "[]"), ts(NOW)).unwrap();
        cursor = chunks(&evs)[0].1.clone();
        pages += 1;
    }
    assert_eq!(last_end, to, "the walk ends at the range's end");
    assert_eq!(pages, ((to - start) / 60 + 499) / 500);
}

#[test]
fn a_range_wholly_older_than_the_archive_is_refused_with_the_date() {
    let m = Mexc::futures().unwrap();
    let mut s = Session::default();
    s.sizes.insert("BTC_USDT".into(), Decimal::new(1, 4));
    let cache = InstrumentCache::new();
    let e = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, 1_700_000_000, 1_700_003_600, None), ts(NOW)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    assert!(e.message.contains("30 days") && e.message.contains("2026-09-19"), "{}", e.message);
    // Hours reach back to the listing.
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::H1, 1_591_228_800, 1_600_000_000, None), ts(NOW)).unwrap();
    assert_eq!(q(one(&reqs), "start").as_deref(), Some("1591228800"));
}

#[test]
fn a_futures_page_asks_the_contract_once_and_counts_coins() {
    let m = Mexc::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_399_260, 1_792_399_440);
    let first = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let h = one(&first);
    assert_eq!((h.path.as_str(), q(h, "symbol").as_deref()), ("/api/v1/contract/detail", Some("BTC_USDT")), "the contract's size before its candles");
    let detail = format!(r#"{{"success":true,"code":0,"data":{BTC_CONTRACT}}}"#);
    assert!(decode_once(&m, &mut s, &cache, &http_frame(1, "contract", 200, &detail), ts(NOW)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(1), &mut s);
    let h = one(&next);
    assert_eq!(h.path, "/api/v1/contract/kline/BTC_USDT");
    assert_eq!(
        (q(h, "interval").as_deref(), q(h, "start").as_deref(), q(h, "end").as_deref()),
        (Some("Min1"), Some("1792399260"), Some("1792399439")),
        "seconds, both ends inclusive"
    );
    // Recorded: columns, volume a number of contracts of 0.0001 BTC.
    let body = json(
        r#"{"success":true,"code":0,"data":{"time":[1792399260,1792399320,1792399380],"open":[82895.2,82897.6,82861.1],"close":[82897.6,82861.1,82849.6],"high":[82922.6,82912.9,82861.1],"low":[82890.1,82852.0,82841.4],"vol":[181036.0,283016.0,284198.0],"amount":[1500882.95202,2345929.40512,2354545.45023],"realOpen":[82892.9,82897.5,82861.0],"realClose":[82897.6,82861.1,82849.6],"realHigh":[82922.6,82912.9,82861.1],"realLow":[82890.1,82852.0,82841.4]}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, &body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), _, done) = &chunks(&evs)[0] else { panic!() };
    assert!(*done);
    assert_eq!((cs[0].open.to_string(), cs[0].volume.to_string()), ("82895.2".into(), "18.10360".into()), "181036 contracts of 0.0001 BTC");
    assert_eq!(cs[0].quote_volume.unwrap().to_string(), "1500882.95202");
    // Futures bars settle for 60 s after their close.
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, true, false]);
    let again = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    assert_eq!(one(&again).path, "/api/v1/contract/kline/BTC_USDT", "the size is kept");
}

#[test]
fn each_archive_speaks_its_own_tokens() {
    assert_eq!(
        [Timeframe::H1, Timeframe::W1, Timeframe::MO1].map(|tf| archive_interval(Book::Spot, tf)),
        [Some("60m"), Some("1W"), Some("1M")]
    );
    assert_eq!([Timeframe::H1, Timeframe::W1, Timeframe::MO1].map(|tf| archive_interval(Book::Futures, tf)), [Some("Min60"), Some("Week1"), Some("Month1")]);
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // A month is not a fixed length: one call over the whole range.
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTCUSDT", Timeframe::MO1, 1_767_225_600, NOW, None), ts(NOW)).unwrap();
    assert_eq!((q(one(&reqs), "interval").as_deref(), q(one(&reqs), "startTime").as_deref()), (Some("1M"), Some("1767225600000")));
    let body = r#"[[1788220800000,"80000","90000","70000","82000","1000",1790812800000,"82000000"],[1790812800000,"82000","84000","80000","82800","100",1793491200000,"8280000"]]"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "klines", 200, body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, false], "September is over, October is not");
}

#[test]
fn an_unknown_pair_or_contract_is_refused_by_name_with_the_books_spelling() {
    let spot = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&spot, &mut s, &cache, &hist(&spot, "BTC_USDT", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    let e = decode_once(&spot, &mut s, &cache, &http_frame(1, "klines", 400, r#"{"msg":"Invalid symbol.","code":-1121,"_extend":null}"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("BTC_USDT") && e.message.contains("(BTCUSDT)"), "{e:?}");
    let fut = Mexc::futures().unwrap();
    let mut s = Session::default();
    encode_once(&fut, &mut s, &cache, &hist(&fut, "NOPE_USDT", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    let e = decode_once(&fut, &mut s, &cache, &http_frame(1, "contract", 200, r#"{"success":false,"code":1001,"message":"Contract does not exist"}"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("NOPE_USDT"), "{e:?}");
    assert!(spot.instrument_id("BTC-USDT").is_err(), "a dash is not MEXC's");
}

#[test]
fn the_lists_read_steps_and_contract_sizes() {
    let spot = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&spot, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(NOW)).unwrap();
    // Recorded (trimmed).
    let body = r#"{"timezone":"CST","serverTime":1792396700000,"symbols":[{"symbol":"BTCUSDT","status":"1","baseAsset":"BTC","baseAssetPrecision":8,"quoteAsset":"USDT","quotePrecision":2,"quoteAssetPrecision":2,"isSpotTradingAllowed":true,"quoteAmountPrecision":"1","baseSizePrecision":"0.000001","permissions":["SPOT"]},{"symbol":"OLDUSDT","status":"1","baseAsset":"OLD","quoteAsset":"USDT","quotePrecision":4,"isSpotTradingAllowed":false,"baseSizePrecision":"0.01"}]}"#;
    let evs = decode_once(&spot, &mut s, &cache, &http_frame(3, "exchangeInfo", 200, body), ts(NOW)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!((specs[0].tick_size.unwrap().to_string(), specs[0].lot_size.unwrap().to_string()), ("0.01".into(), "0.000001".into()));
    assert!(specs[0].active && !specs[1].active);
    assert!(matches!(evs.last().unwrap().event, Event::InstrumentsEnd));

    let fut = Mexc::futures().unwrap();
    let mut s = Session::default();
    encode_once(&fut, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(NOW)).unwrap();
    let body = format!(
        r#"{{"success":true,"code":0,"data":[{BTC_CONTRACT},{{"symbol":"ETH_USDC","baseCoin":"ETH","quoteCoin":"USDC","settleCoin":"USDC","futureType":1,"contractSize":0.01,"priceUnit":0.01,"volUnit":1,"minVol":1,"state":0}},{{"symbol":"BTC_USD","baseCoin":"BTC","quoteCoin":"USD","settleCoin":"BTC","futureType":1,"contractSize":100,"priceUnit":0.1,"volUnit":1,"minVol":1,"state":0}}]}}"#
    );
    let evs = decode_once(&fut, &mut s, &cache, &http_frame(3, "contracts", 200, &body), ts(NOW)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!(specs.iter().map(|s| s.id.native.as_str()).collect::<Vec<_>>(), ["BTC_USDT", "ETH_USDC"], "the coin-settled contract is left out");
    assert_eq!((specs[0].multiplier.to_string(), specs[0].lot_size.unwrap().to_string(), specs[0].tick_size.unwrap().to_string()), ("0.0001".into(), "0.0001".into(), "0.1".into()));
    assert_eq!((specs[1].settlement_ccy.as_deref(), specs[1].quote.as_str()), (Some("USDC"), "USDC"));
    assert_eq!(s.sizes.get("ETH_USDC").map(|d| d.to_string()).as_deref(), Some("0.01"));
}

#[test]
fn quotes_the_board_and_the_clock() {
    let spot = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let snap = |venue: &str, native: &str| Command::RequestSnapshot { req_id: CorrId(4), instrument: InstrumentId::new(venue, native), data_type: DataType::Quote };
    let reqs = encode_once(&spot, &mut s, &cache, &snap("MEXC", "BTCUSDT"), ts(NOW)).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "symbol").as_deref()), ("/api/v3/ticker/bookTicker", Some("BTCUSDT")));
    let body = r#"{"symbol":"BTCUSDT","bidPrice":"82740.31","bidQty":"7.99649105","askPrice":"82740.32","askQty":"0.29691141"}"#;
    let Event::Quote(qt) = &decode_once(&spot, &mut s, &cache, &http_frame(4, "bookTicker", 200, body), ts(NOW)).unwrap()[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.bid_qty.unwrap().to_string(), qt.ask.to_string()), ("82740.31".into(), "7.99649105".into(), "82740.32".into()));

    let fut = Mexc::futures().unwrap();
    let mut s = Session::default();
    let reqs = encode_once(&fut, &mut s, &cache, &snap("MEXC_FUTURES", "BTC_USDT"), ts(NOW)).unwrap();
    assert_eq!(one(&reqs).path, "/api/v1/contract/ticker");
    let body = r#"{"success":true,"code":0,"data":{"contractId":10,"symbol":"BTC_USDT","lastPrice":82853.9,"bid1":82853.9,"ask1":82854,"volume24":285346971,"fairPrice":82855.7,"timestamp":1792399466000}}"#;
    let Event::Quote(qt) = &decode_once(&fut, &mut s, &cache, &http_frame(4, "ticker", 200, body), ts(NOW)).unwrap()[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.ask.to_string(), qt.bid_qty), ("82853.9".into(), "82854".into(), None), "the contract ticker carries no sizes");

    let mut s = Session::default();
    encode_once(&spot, &mut s, &cache, &Command::RequestBoard { req_id: CorrId(5) }, ts(NOW)).unwrap();
    let body = r#"[{"symbol":"METALUSDT","price":"0.11017"},{"symbol":"NEXUSDT","price":"0.0000006897"}]"#;
    let Event::Board(board) = &decode_once(&spot, &mut s, &cache, &http_frame(5, "tickerPrice", 200, body), ts(NOW)).unwrap()[0].event else { panic!() };
    assert_eq!(board.iter().map(|p| (p.instrument.native.as_str(), p.price.to_string())).collect::<Vec<_>>(), [("METALUSDT", "0.11017".to_string()), ("NEXUSDT", "0.0000006897".to_string())]);

    for (m, body) in [(&spot, r#"{"serverTime":1792399467686}"#), (&fut, r#"{"success":true,"code":0,"data":1792399467686}"#)] {
        let mut s = Session::default();
        encode_once(m, &mut s, &cache, &Command::SyncTime { req_id: CorrId(6) }, ts(NOW)).unwrap();
        let Event::ServerTime(t) = &decode_once(m, &mut s, &cache, &http_frame(6, "time", 200, body), ts(NOW)).unwrap()[0].event else { panic!() };
        assert_eq!(t.millis(), 1_792_399_467_686);
    }
}

fn ws(payload: &[u8]) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("public".into()), payload: bytes::Bytes::copy_from_slice(payload) }
}

fn sub(id: u64, venue: &str, native: &str, tf: Timeframe) -> Command {
    Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(venue, native), data_type: DataType::Candles { tf } }
}

#[test]
fn the_spot_socket_reads_protobuf_and_names_each_pane() {
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let out = m.encode_batch(&[sub(7, "MEXC", "BTCUSDT", Timeframe::M1), sub(8, "MEXC", "NOPEUSDT", Timeframe::M1)], &mut s, &ctx).unwrap();
    let NativeRequest::WsOpen { base, path, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!((*base, path.as_str()), (WS, "/ws"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"method":"SUBSCRIPTION","params":["spot@public.kline.v3.api.pb@BTCUSDT@Min1"],"id":7}));
    // Recorded answers.
    let ok = br#"{"id":7,"code":0,"msg":"spot@public.kline.v3.api.pb@BTCUSDT@Min1"}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(ok), ts(0)).unwrap()[..], [e] if e.corr_id == Some(CorrId(7)) && matches!(e.event, Event::SubscriptionAck)));
    let fail = r#"{"id":8,"code":0,"msg":"Not Subscribed successfully! [spot@public.kline.v3.api.pb@NOPEUSDT@Min1].  Reason： Blocked! "}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(fail.as_bytes()), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(8)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("NOPEUSDT"))));
    // A recorded update: protobuf, no closed flag.
    let pb = hex::decode("0a2873706f74407075626c69632e6b6c696e652e76332e6170692e70624042544355534454404d696e311a0742544355534454222032666239343231353465663434613461623265663938633861666236613461372890b5b89a9534a2134f0a044d696e3110f0acd7d6061a0738323734302e34220838323736322e30382a0838323736322e3039320838323734302e33313a0a302e3432313731383334420833343839382e373248acadd7d606").unwrap();
    let evs = decode_once(&m, &mut s, &cache, &ws(&pb), ts(0)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(
        (cd.open_ts.secs(), cd.open.to_string(), cd.close.to_string(), cd.volume.to_string(), cd.quote_volume.unwrap().to_string(), cd.is_final),
        (1_792_398_960, "82740.4".into(), "82762.08".into(), "0.42171834".into(), "34898.72".into(), false)
    );
    assert_eq!(evs[0].exchange_ts.unwrap().millis(), 1_792_398_989_968);
    // A pong, and the unsubscribe answer: nothing to route.
    assert!(decode_once(&m, &mut s, &cache, &ws(br#"{"id":0,"code":0,"msg":"PONG"}"#), ts(0)).unwrap().is_empty());
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(7) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap()["method"], "UNSUBSCRIPTION");
}

#[test]
fn a_spot_socket_refuses_a_31st_pane() {
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let panes: Vec<Command> = (0..31).map(|i| sub(100 + i, "MEXC", &format!("C{i}USDT"), Timeframe::M1)).collect();
    let out = m.encode_batch(&panes, &mut s, &ctx).unwrap();
    assert_eq!(out.len(), 31, "the socket and 30 subscriptions");
    let evs = decode_once(&m, &mut s, &cache, &ws(br#"{"id":0,"code":0,"msg":"PONG"}"#), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(130)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("30 panes"))));
}

#[test]
fn the_futures_socket_answers_in_order_and_counts_contracts_into_coins() {
    let m = Mexc::futures().unwrap();
    let mut s = Session::default();
    let cache = btc_cache();
    let c = otw_connector_model::ConnectorId("t".into());
    let at = |t: i64| MapCtx { received_ts: ts(t), instruments: &cache, connector: &c };
    let panes = [sub(7, "MEXC_FUTURES", "BTC_USDT", Timeframe::M1), sub(9, "MEXC_FUTURES", "BTC_USDT", Timeframe::W1)];
    let out = m.encode_batch(&panes, &mut s, &at(NOW)).unwrap();
    let NativeRequest::WsOpen { base, path, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!((*base, path.as_str()), (FUT_WS, "/edge"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"method":"sub.kline","param":{"symbol":"BTC_USDT","interval":"Min1"}}));
    // An unlisted contract is refused before anything goes out.
    let out = m.encode_batch(&[sub(8, "MEXC_FUTURES", "NOPE_USDT", Timeframe::M1)], &mut s, &at(NOW)).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsSend { payload, .. }] if payload.as_ref() == br#"{"method":"ping"}"#));
    // Recorded answers, in the order asked.
    let ack = |t: &str| format!(r#"{{"channel":"rs.sub.kline","data":"success","ts":{t}}}"#);
    let evs = decode_once(&m, &mut s, &cache, &ws(ack("1792396949259").as_bytes()), ts(0)).unwrap();
    assert!(matches!(&evs[0], e if e.corr_id == Some(CorrId(8)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("NOPE_USDT"))));
    assert!(matches!(&evs[1], e if e.corr_id == Some(CorrId(7)) && matches!(e.event, Event::SubscriptionAck)));
    let err = br#"{"channel":"rs.error","data":"Contract [BTC_USDT] not exists","ts":1792396949260}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(err), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(9)) && matches!(e.event, Event::SubscriptionFail { .. })));
    let push = br#"{"symbol":"BTC_USDT","data":{"symbol":"BTC_USDT","interval":"Min1","t":1792396920,"o":82712.7,"c":82730.7,"h":82732,"l":82712.6,"a":1052258.4551,"q":127196,"ro":82712.6,"rc":82730.7,"rh":82732,"rl":82712.6},"channel":"push.kline","ts":1792396949339}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(push), ts(0)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((cd.volume.to_string(), cd.quote_volume.unwrap().to_string(), cd.is_final), ("12.7196".into(), "1052258.4551".into(), false));
    assert_eq!((cd.open_ts.secs(), evs[0].exchange_ts.unwrap().millis()), (1_792_396_920, 1_792_396_949_339));
    // Unsubscribing names the interval: the contract's other panes keep streaming.
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(7) }], &mut s, &at(NOW)).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"method":"unsub.kline","param":{"symbol":"BTC_USDT","interval":"Min1"}}));
    // The contract socket drops a client that has not pinged for a minute.
    assert!(m.keepalive(&mut s, &at(NOW + 10)).is_empty());
    assert!(matches!(&m.keepalive(&mut s, &at(NOW + 16))[..], [NativeRequest::WsSend { payload, .. }] if payload.as_ref() == br#"{"method":"ping"}"#));
}

#[test]
fn the_best_bid_and_ask_on_both_books() {
    // Spot: the aggregated book ticker, protobuf.
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let q = Command::Subscribe { sub_id: CorrId(4), instrument: InstrumentId::new("MEXC", "BTCUSDT"), data_type: DataType::Quote };
    let out = m.encode_batch(&[q], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"method":"SUBSCRIPTION","params":["spot@public.aggre.bookTicker.v3.api.pb@100ms@BTCUSDT"],"id":4}));
    // Recorded.
    let ok = br#"{"id":4,"code":0,"msg":"spot@public.aggre.bookTicker.v3.api.pb@100ms@BTCUSDT"}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(ok), ts(0)).unwrap()[0].event, Event::SubscriptionAck));
    let pb = hex::decode("0a3473706f74407075626c69632e61676772652e626f6f6b5469636b65722e76332e6170692e7062403130306d7340425443555344541a074254435553445430f8e2d49f9534da13400a0838323236312e3433120a382e33373338353037361a0838323236312e3434220a302e30303032303135312a0b383430343234373133323630efe2d49f9534").unwrap();
    let evs = decode_once(&m, &mut s, &cache, &ws(&pb), ts(1_792_409_940)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(4)));
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82261.43".to_string(), "82261.44".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);

    // Contracts: the ticker's `bid1` / `ask1`, answered in order like a candle.
    let m = Mexc::futures().unwrap();
    let mut s = Session::default();
    let cache = btc_cache();
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let q = Command::Subscribe { sub_id: CorrId(5), instrument: InstrumentId::new("MEXC_FUTURES", "BTC_USDT"), data_type: DataType::Quote };
    let out = m.encode_batch(&[q], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"method":"sub.ticker","param":{"symbol":"BTC_USDT"}}));
    let ack = br#"{"channel":"rs.sub.ticker","data":"success","ts":1792409941325}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(ack), ts(0)).unwrap()[..], [e] if e.corr_id == Some(CorrId(5)) && matches!(e.event, Event::SubscriptionAck)));
    let push = br#"{"symbol":"BTC_USDT","data":{"symbol":"BTC_USDT","lastPrice":82239.5,"fairPrice":82239.4,"timestamp":1792409941829,"bid1":82239.4,"ask1":82239.5,"holdVol":453851187},"channel":"push.ticker","ts":1792409941830}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(push), ts(1_792_409_942)).unwrap();
    assert_eq!(evs[0].exchange_ts.unwrap().millis(), 1_792_409_941_829);
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82239.4".to_string(), "82239.5".to_string()));
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(5) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"method":"unsub.ticker","param":{"symbol":"BTC_USDT"}}));
}

fn account(m: &Mexc, s: &mut Session, kind: AccountKind, symbols: &[&str], cursor: Option<String>, range: Option<TimeRange>) -> Vec<NativeRequest> {
    let symbols = symbols.iter().map(|n| InstrumentId::new(m.book().venue(), n)).collect();
    let cmd = Command::QueryAccount { req_id: CorrId(20), kind, symbols, range, cursor, account: None };
    encode_once(m, s, &InstrumentCache::new(), &cmd, ts(NOW)).unwrap()
}

fn account_chunk(evs: &[otw_connector_model::Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn spot_balances_summary_and_open_orders() {
    let m = Mexc::spot().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let h = one(&account(&m, &mut s, AccountKind::Balances, &[], None, None)).clone();
    assert!(h.signed && h.path == "/api/v3/account");
    let body = r#"{"canTrade":true,"canWithdraw":true,"canDeposit":true,"updateTime":null,"accountType":"SPOT","balances":[{"asset":"BTC","free":"0.5","locked":"0.1"},{"asset":"MX","free":"0","locked":"0"}],"permissions":["SPOT"]}"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "account", 200, body), ts(NOW)).unwrap());
    let AccountItems::Balances(b) = items else { panic!() };
    assert_eq!(b.iter().map(|b| (b.asset.as_str(), b.total.0.to_string(), b.available.0.to_string())).collect::<Vec<_>>(), [("BTC", "0.6".to_string(), "0.5".to_string())]);
    account(&m, &mut s, AccountKind::Summary, &[], None, None);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(20, "account", 200, body), ts(NOW)).unwrap();
    let Event::AccountSummary(sm) = &evs[0].event else { panic!() };
    assert_eq!((sm.can_trade, sm.assets_held, sm.account_type.as_deref()), (Some(true), 1, Some("SPOT")));
    // Open orders of every pair: no symbol.
    let h = one(&account(&m, &mut s, AccountKind::OpenOrders, &[], None, None)).clone();
    assert!(h.query.is_empty(), "{:?}", h.query);
    let body = r#"[{"symbol":"LTCBTC","orderId":1,"orderListId":-1,"clientOrderId":"myOrder1","price":"0.1","origQty":"1.0","executedQty":"0.25","cummulativeQuoteQty":"0.0","status":"NEW","timeInForce":"GTC","type":"LIMIT","side":"BUY","stopPrice":"0.0","icebergQty":"0.0","time":1499827319559,"updateTime":1499827319559,"isWorking":true,"origQuoteOrderQty":"0.000000"}]"#;
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "openOrders", 200, body), ts(NOW)).unwrap());
    let AccountItems::Orders(o) = items else { panic!() };
    assert_eq!((o[0].id.as_str(), o[0].filled_qty.0.to_string(), o[0].order_type.as_str(), o[0].stop_price, next), ("1", "0.25".to_string(), "limit", None, None));
    // A refused key says what to check.
    account(&m, &mut s, AccountKind::Balances, &[], None, None);
    let e = decode_once(&m, &mut s, &cache, &http_frame(20, "account", 400, r#"{"code":700006,"msg":"IP [1.2.3.4] not in the ip white list"}"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("IP addresses"), "{e:?}");
}

#[test]
fn each_book_reads_the_names_spelled_its_way() {
    let range = TimeRange::new(ts(NOW - 3 * 86_400), ts(NOW));
    let spot = Mexc::spot().unwrap();
    let mut s = Session::default();
    let reqs = account(&spot, &mut s, AccountKind::Fills, &["BTC_USDT", "ETHUSDT"], None, Some(range));
    assert_eq!(q(one(&reqs), "symbol").as_deref(), Some("ETHUSDT"));
    let fut = Mexc::futures().unwrap();
    let mut s = Session::default();
    assert!(account(&fut, &mut s, AccountKind::Fills, &["ETHUSDT"], None, Some(range)).is_empty(), "no contract named: no call");
    assert!(s.pending.is_empty());
}

#[test]
fn spot_fills_halve_a_full_slice_and_move_from_pair_to_pair() {
    let m = Mexc::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = NOW - 10 * 86_400;
    let range = TimeRange::new(ts(from), ts(NOW));
    let pairs = ["BTCUSDT", "ETHUSDT"];
    let window = |h: &HttpRequest| -> (i64, i64) { (q(h, "startTime").unwrap().parse().unwrap(), q(h, "endTime").unwrap().parse().unwrap()) };
    let h = one(&account(&m, &mut s, AccountKind::Fills, &pairs, None, Some(range))).clone();
    assert_eq!((h.path.as_str(), q(&h, "symbol").as_deref(), q(&h, "limit").as_deref()), ("/api/v3/myTrades", Some("BTCUSDT"), Some("100")));
    assert_eq!(window(&h), (NOW * 1000 - 7 * 86_400_000, NOW * 1000 - 1), "a week first, from the period's end");
    let row = r#"{"symbol":"BTCUSDT","id":"fad2af9e942049b6adbda1a271f990c6","orderId":"bb41e5663e124046bd9497a3f5692f39","orderListId":-1,"price":"82000.5","qty":"0.01","quoteQty":"820.005","commission":"0.41","commissionAsset":"USDT","time":1792390000123,"isBuyer":false,"isMaker":true,"isBestMatch":true,"isSelfTrade":false,"clientOrderId":null}"#;
    let full = format!("[{}]", vec![row; 100].join(","));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "myTrades", 200, &full), ts(NOW)).unwrap());
    assert!(matches!(items, AccountItems::Fills(ref f) if f.is_empty()), "a full page is not kept: MEXC pages no further");
    let h = one(&account(&m, &mut s, AccountKind::Fills, &pairs, next, Some(range))).clone();
    let (a, b) = window(&h);
    assert_eq!((b - a + 1, b), (7 * 86_400_000 / 2, NOW * 1000 - 1), "the same end, half the width");
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "myTrades", 200, &format!("[{row}]")), ts(NOW)).unwrap());
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!((f[0].id.as_str(), f[0].ts.millis(), f[0].fee.0.to_string(), f[0].fee_asset.as_deref()), ("fad2af9e942049b6adbda1a271f990c6", 1_792_390_000_123, "0.41".to_string(), Some("USDT")));
    assert!(matches!(f[0].side, Mapped::Known(Side::Sell)));
    // A quiet slice: the next one below it is a week wide again, cut at the period's start.
    let h = one(&account(&m, &mut s, AccountKind::Fills, &pairs, next, Some(range))).clone();
    assert_eq!(window(&h), (from * 1000, a - 1));
    let (_, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "myTrades", 200, "[]"), ts(NOW)).unwrap());
    // The pair is done: the next one, from the period's end.
    let h = one(&account(&m, &mut s, AccountKind::Fills, &pairs, next, Some(range))).clone();
    assert_eq!((q(&h, "symbol").as_deref(), window(&h).1), (Some("ETHUSDT"), NOW * 1000 - 1));
}

#[test]
fn futures_positions_orders_and_fills_count_contracts() {
    let m = Mexc::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let h = one(&account(&m, &mut s, AccountKind::Positions, &[], None, None)).clone();
    assert_eq!(h.path, "/api/v1/private/position/open_positions");
    let body = r#"{"success":true,"code":0,"data":[{"positionId":1394650,"symbol":"ETH_USDT","positionType":2,"openType":1,"state":1,"holdVol":25,"frozenVol":0,"closeVol":0,"holdAvgPrice":1217.3,"openAvgPrice":1217.3,"liquidatePrice":1211.2,"leverage":100,"createTime":1609991676000}]}"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "openPositions", 200, body), ts(NOW)).unwrap());
    let AccountItems::Positions(p) = items else { panic!() };
    assert_eq!((p[0].instrument.native.as_str(), p[0].qty.0.to_string(), p[0].avg_price.unwrap().to_string(), p[0].currency.as_deref()), ("ETH_USDT", "-25".to_string(), "1217.3".to_string(), Some("USDT")));

    let h = one(&account(&m, &mut s, AccountKind::OpenOrders, &[], None, None)).clone();
    assert_eq!((h.path.as_str(), q(&h, "page_num").as_deref(), q(&h, "page_size").as_deref()), ("/api/v1/private/order/list/open_orders", Some("1"), Some("100")));
    let body = r#"{"success":true,"code":0,"data":[{"orderId":"265307163526610432","symbol":"BTC_USDT","positionId":0,"price":90000,"vol":10,"leverage":20,"side":3,"category":1,"orderType":2,"dealAvgPrice":0,"dealVol":4,"state":2,"createTime":1792300000250}]}"#;
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "openOrders", 200, body), ts(NOW)).unwrap());
    let AccountItems::Orders(o) = items else { panic!() };
    assert_eq!((o[0].id.as_str(), o[0].qty.0.to_string(), o[0].filled_qty.0.to_string(), o[0].order_type.as_str(), next), ("265307163526610432", "10".to_string(), "4".to_string(), "post only", None));
    assert!(matches!(o[0].side, Mapped::Known(Side::Sell)), "opening a short sells");

    let range = TimeRange::new(ts(NOW - 40 * 86_400), ts(NOW));
    let h = one(&account(&m, &mut s, AccountKind::Fills, &["BTC_USDT"], None, Some(range))).clone();
    assert_eq!(h.path, "/api/v1/private/order/list/order_deals");
    assert_eq!((q(&h, "start_time"), q(&h, "page_num").as_deref()), (Some((NOW * 1000 - 30 * 86_400_000).to_string()), Some("1")), "30 days a slice");
    // ccxt's recorded sample, ids and numbers as strings.
    let row = r#"{"id":"299444585","symbol":"BTC_USDT","side":"4","vol":"3","price":"82000.5","feeCurrency":"USDT","fee":"0.0147","timestamp":"1792390000500","profit":"0","category":"1","orderId":"265307163526610432","positionMode":"1","taker":true}"#;
    let full = format!(r#"{{"success":true,"code":0,"data":[{}]}}"#, vec![row; 100].join(","));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "orderDeals", 200, &full), ts(NOW)).unwrap());
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!((f.len(), f[0].qty.0.to_string(), f[0].ts.millis(), f[0].quote_ccy.as_deref()), (100, "3".to_string(), 1_792_390_000_500, Some("USDT")));
    assert!(matches!(f[0].side, Mapped::Known(Side::Sell)), "closing a long sells");
    let h = one(&account(&m, &mut s, AccountKind::Fills, &["BTC_USDT"], next, Some(range))).clone();
    assert_eq!(q(&h, "page_num").as_deref(), Some("2"), "a full page pages on");
    let (_, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "orderDeals", 200, r#"{"success":true,"code":0,"data":[]}"#), ts(NOW)).unwrap());
    let h = one(&account(&m, &mut s, AccountKind::Fills, &["BTC_USDT"], next, Some(range))).clone();
    assert_eq!((q(&h, "start_time"), q(&h, "page_num").as_deref()), (Some((NOW * 1000 - 40 * 86_400_000).to_string()), Some("1")), "the older ten days");
}
