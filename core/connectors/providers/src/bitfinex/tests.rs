//! Bitfinex, offline. Market-data fixtures are answers recorded from the public endpoints and
//! socket on 2026-10-19 (trimmed); account fixtures follow the samples of Bitfinex's own API
//! docs (v2) and ccxt's, no key being available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, AssetClass, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, Method, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

/// The moment the recorded answers were taken: Monday 2026-10-19 10:05:00 UTC.
const NOW: i64 = 1_792_404_300;

fn hist(native: &str, tf: Timeframe, from: i64, to: i64, cursor: Option<String>) -> Command {
    Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(Book::of(native).venue(), native),
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

#[test]
fn each_book_declares_only_what_it_serves() {
    for (m, native) in [(Bitfinex::spot().unwrap(), "tBTCUSD"), (Bitfinex::derivatives().unwrap(), "tBTCF0:USTF0")] {
        assert_undeclared_is_unsupported(&m, &InstrumentId::new(m.book().venue(), native));
        assert_eq!(m.capabilities().provider, "bitfinex");
    }
    // The account of both books is read once, on the spot adapter.
    assert!(Bitfinex::spot().unwrap().capabilities().account.fills_history);
    assert!(!Bitfinex::derivatives().unwrap().capabilities().account.balances);
}

#[test]
fn names_are_written_the_bitfinex_way() {
    let m = Bitfinex::spot().unwrap();
    let id = |s: &str| m.instrument_id(s).map(|i| (i.venue.0.clone(), i.native.clone()));
    assert_eq!(id("BTCUSD").unwrap(), ("BITFINEX".to_string(), "tBTCUSD".to_string()));
    assert_eq!(id("tBTCUSD").unwrap().1, "tBTCUSD");
    assert_eq!(id(" avax:usd ").unwrap().1, "tAVAX:USD");
    assert_eq!(id("TRXUSD").unwrap().1, "tTRXUSD", "an upper-case T is the pair's own");
    assert_eq!(id("BTCF0:USTF0").unwrap(), ("BITFINEX_PERP".to_string(), "tBTCF0:USTF0".to_string()));
    for bad in ["BTC/USD", "BTCUSDT", "AVAX:", "", "BTC-USD"] {
        let e = m.instrument_id(bad).unwrap_err();
        assert!(e.kind == ErrorKind::NotFound && e.message.contains("AVAX:USD"), "{bad}: {e:?}");
    }
    assert_eq!(m.pair_symbol("btc", "UST").as_deref(), Some("tBTCUST"));
    assert_eq!(m.pair_symbol("AVAX", "USD").as_deref(), Some("tAVAX:USD"));
    assert_eq!(m.pair_symbol("BTC", "XAUT").as_deref(), Some("tBTC:XAUT"));
    assert_eq!(Bitfinex::derivatives().unwrap().pair_symbol("BTC", "UST"), None);
    assert_eq!(split_pair("tBTCF0:USTF0"), Some(("BTC".to_string(), "UST".to_string())));
    assert_eq!(split_pair("tETHBTC"), Some(("ETH".to_string(), "BTC".to_string())));
}

/// Recorded: `[MTS, OPEN, CLOSE, HIGH, LOW, VOLUME]`, close before high and low.
const BTC_1M: &str = r#"[[1792399260000,82920,82921,82967,82920,0.58929448],[1792399320000,82929,82891,82951,82886,0.81036486],[1792399380000,82881,82891,82891,82878,0.02166692],[1792399440000,82888,82891,82897,82880,0.12133376]]"#;

#[test]
fn a_page_is_a_window_inclusive_at_both_ends() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_399_260, 1_792_399_500);
    let reqs = encode_once(&m, &mut s, &cache, &hist("tBTCUSD", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let h = one(&reqs);
    assert_eq!(h.path, "/v2/candles/trade:1m:tBTCUSD/hist");
    assert_eq!(
        (q(h, "start").as_deref(), q(h, "end").as_deref(), q(h, "limit").as_deref(), q(h, "sort").as_deref()),
        (Some("1792399260000"), Some("1792399499999"), Some("10000"), Some("1")),
        "`end` is inclusive on the open time"
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, BTC_1M), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs.len(), next.as_ref(), *done), (4, None, true));
    let b = &cs[1];
    assert_eq!((b.open.to_string(), b.high.to_string(), b.low.to_string(), b.close.to_string()), ("82929".into(), "82951".into(), "82886".into(), "82891".into()));
    assert_eq!((b.volume.to_string(), b.quote_volume), ("0.81036486".into(), None));
    assert!(cs.iter().all(|c| c.is_final), "closed for more than the 60 s settle window");
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);
    // Half a minute after the last bar closed, it was still settling.
    encode_once(&m, &mut s, &cache, &hist("tBTCUSD", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, BTC_1M), ts(1_792_399_530)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, true, true, false]);
}

#[test]
fn a_long_range_walks_forwards_ten_thousand_bars_a_page() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    s.known.insert("tBTCUSD".into());
    let cache = InstrumentCache::new();
    // Thirty days of minutes, from the listing's era: every timeframe reaches back to it.
    let (from, to) = (NOW - 30 * 86_400, NOW);
    let mut cursor = None;
    let mut windows = Vec::new();
    loop {
        let reqs = encode_once(&m, &mut s, &cache, &hist("tBTCUSD", Timeframe::M1, from, to, cursor.clone()), ts(NOW)).unwrap();
        let h = one(&reqs);
        windows.push((q(h, "start").unwrap().parse::<i64>().unwrap(), q(h, "end").unwrap().parse::<i64>().unwrap()));
        // A quiet window of a pair known to trade is an empty page, nothing more.
        let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, "[]"), ts(NOW)).unwrap();
        let (_, next, done) = &chunks(&evs)[0];
        if *done {
            break;
        }
        cursor = next.clone();
    }
    assert_eq!(windows.len(), 5, "43200 minutes, 10000 a page");
    assert_eq!(windows[0], (from * 1000, from * 1000 + 10_000 * 60_000 - 1));
    assert!(windows.windows(2).all(|w| w[1].0 == w[0].1 + 1), "no gap, no overlap: {windows:?}");
    assert_eq!(windows.last().unwrap().1, to * 1000 - 1);
    // The first bar reaches back to 2013 (measured): an old range is asked as is.
    let reqs = encode_once(&m, &mut s, &cache, &hist("tBTCUSD", Timeframe::M1, 1_388_534_400, 1_388_538_000, None), ts(NOW)).unwrap();
    assert_eq!(q(one(&reqs), "start").as_deref(), Some("1388534400000"));
}

#[test]
fn an_empty_page_of_a_name_never_traded_is_refused_by_name() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist("tNOPEUSD", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    // Recorded: an unknown pair answers an empty list, like a quiet one.
    assert!(decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, "[]"), ts(NOW)).unwrap().is_empty(), "held until the check answers");
    let next = m.follow_ups(CorrId(1), &mut s);
    let h = one(&next);
    assert_eq!((h.path.as_str(), q(h, "limit").as_deref()), ("/v2/candles/trade:1M:tNOPEUSD/hist", Some("1")));
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, "[]"), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("tNOPEUSD") && e.message.contains("AVAX:USD"), "{e:?}");

    // A delisted pair keeps its bars (recorded): an empty period of it is only quiet.
    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &hist("tBSVUSD", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, "[]"), ts(NOW)).unwrap();
    assert_eq!(m.follow_ups(CorrId(1), &mut s).len(), 1);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, "[[1669852800000,43.697,42.287,51.878,40.275,92601.55831865]]"), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert!(cs.is_empty() && next.is_none() && *done);
    // Known from now on: the next empty page asks nothing more.
    encode_once(&m, &mut s, &cache, &hist("tBSVUSD", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    assert_eq!(chunks(&decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, "[]"), ts(NOW)).unwrap()).len(), 1);
    assert!(m.follow_ups(CorrId(1), &mut s).is_empty());
}

#[test]
fn each_book_reads_its_own_names() {
    let fut = Bitfinex::derivatives().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&fut, &mut s, &cache, &hist("tBTCF0:USTF0", Timeframe::M1, 1_792_399_260, 1_792_399_380, None), ts(NOW)).unwrap();
    assert_eq!(one(&reqs).path, "/v2/candles/trade:1m:tBTCF0:USTF0/hist");
    // Recorded: the volume counts coins on the derivatives book too.
    let body = r#"[[1792399260000,82987,82991,83014,82987,0.0039864900000000005],[1792399320000,82991,82941,83001,82932,0.09368153]]"#;
    let evs = decode_once(&fut, &mut s, &cache, &http_frame(1, "candles", 200, body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs[0].volume.to_string(), cs[0].instrument.venue.0.as_str()), ("0.0039864900000000005".into(), "BITFINEX_PERP"));

    let spot = Bitfinex::spot().unwrap();
    let e = encode_once(&spot, &mut Session::default(), &cache, &hist("tBTCF0:USTF0", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("derivatives"), "{e:?}");
    let e = encode_once(&fut, &mut Session::default(), &cache, &hist("tBTCUSD", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("BTCF0:USTF0"), "{e:?}");
}

#[test]
fn tokens_and_the_calendar_month() {
    assert_eq!(
        [Timeframe::M1, Timeframe::H1, "3h".parse().unwrap(), Timeframe::H4, Timeframe::D1, Timeframe::W1, Timeframe::MO1].map(interval),
        [Some("1m"), Some("1h"), Some("3h"), Some("4h"), Some("1D"), Some("1W"), Some("1M")]
    );
    assert_eq!(interval("2h".parse().unwrap()), None);
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // A month is not a fixed length: one call over the whole range.
    let reqs = encode_once(&m, &mut s, &cache, &hist("tBTCUSD", Timeframe::MO1, 1_788_220_800, NOW, None), ts(NOW)).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "start").as_deref()), ("/v2/candles/trade:1M:tBTCUSD/hist", Some("1788220800000")));
    let body = r#"[[1788220800000,78627,83607,87392,74985,32720.1682823],[1790812800000,83604,82717,87197,79821,10959.08280245]]"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, false], "September is over, October is not");
}

#[test]
fn the_lists_read_pairs_and_perpetuals() {
    let cache = InstrumentCache::new();
    let spot = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let reqs = encode_once(&spot, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(NOW)).unwrap();
    assert_eq!(one(&reqs).path, "/v2/conf/pub:info:pair");
    // Recorded (trimmed).
    let body = r#"[[["AVAX:USD",[1600862465185,null,null,"0.08","50000.0",null,null,null,0.3,0.15,null,null]],["BTCUSD",[1358182043000,null,null,"0.00004","2000.0",null,null,null,0.1,0.05,null,null]],["TESTBTC:TESTUSD",[1592991523941,null,null,"0.00006","100.0",null,null,null,null,null,null,null]]]]"#;
    let evs = decode_once(&spot, &mut s, &cache, &http_frame(3, "conf", 200, body), ts(NOW)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!(specs.iter().map(|s| s.id.native.as_str()).collect::<Vec<_>>(), ["tAVAX:USD", "tBTCUSD"], "the paper-trading pair is left out");
    assert_eq!((specs[0].base.as_deref(), specs[0].quote.as_str(), specs[1].min_qty.unwrap().to_string()), (Some("AVAX"), "USD", "0.00004".to_string()));
    assert!(matches!(evs.last().unwrap().event, Event::InstrumentsEnd));

    let fut = Bitfinex::derivatives().unwrap();
    let mut s = Session::default();
    let reqs = encode_once(&fut, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(NOW)).unwrap();
    assert_eq!(one(&reqs).path, "/v2/conf/pub:info:pair:futures");
    let body = r#"[[["BTCF0:USTF0",[1562164542332,null,null,"0.00004","100.0",null,null,null,0.01,0.005]],["ETHF0:BTCF0",[1600000000000,null,null,"0.002","500.0",null,null,null,0.01,0.005]]]]"#;
    let evs = decode_once(&fut, &mut s, &cache, &http_frame(3, "conf", 200, body), ts(NOW)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!((specs[0].id.venue.0.as_str(), specs[0].base.as_deref(), specs[0].quote.as_str()), ("BITFINEX_PERP", Some("BTC"), "UST"));
    assert_eq!((specs[0].asset_class, specs[0].multiplier, specs[1].settlement_ccy.as_deref()), (AssetClass::Perp, Decimal::ONE, Some("BTC")));
}

#[test]
fn quotes_and_the_board() {
    let cache = InstrumentCache::new();
    let spot = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let snap = |native: &str| Command::RequestSnapshot { req_id: CorrId(4), instrument: InstrumentId::new(Book::of(native).venue(), native), data_type: DataType::Quote };
    let reqs = encode_once(&spot, &mut s, &cache, &snap("tBTCUSD"), ts(NOW)).unwrap();
    assert_eq!(one(&reqs).path, "/v2/ticker/tBTCUSD");
    // Recorded: `[BID, BID_SIZE, ASK, ASK_SIZE, ...]`.
    let body = "[82911,2.21958591,82929,1.18023683,679,0.00825773,82905,611.7627325,83170,80700,1358182043000]";
    let Event::Quote(qt) = &decode_once(&spot, &mut s, &cache, &http_frame(4, "ticker", 200, body), ts(NOW)).unwrap()[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.bid_qty.unwrap().to_string(), qt.ask.to_string(), qt.ask_qty.unwrap().to_string()), ("82911".into(), "2.21958591".into(), "82929".into(), "1.18023683".into()));
    // Recorded: an unknown pair.
    encode_once(&spot, &mut s, &cache, &snap("tNOPEUSD"), ts(NOW)).unwrap();
    let e = decode_once(&spot, &mut s, &cache, &http_frame(4, "ticker", 500, r#"["error",10020,"symbol: invalid"]"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("tNOPEUSD"), "{e:?}");

    let reqs = encode_once(&spot, &mut s, &cache, &Command::RequestBoard { req_id: CorrId(5) }, ts(NOW)).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "symbols").as_deref()), ("/v2/tickers", Some("ALL")));
    // Recorded (trimmed): funding, perpetual and paper-trading rows are not pairs to value at.
    let body = json(
        r#"[["tBTCUSD",82775,2.61395066,82797,1.63755666,507,0.00616308,82771,612.02712513,83170,80700,1358182043000],["tUSTUSD",0.99992,3467183.63564041,0.99996,28923706.47614756,-0.00014,-0.00013999,0.99996,45296181.21049837,1.0006,0.99988,1543330803000],["tTESTBTC:TESTUSD",82755,56.30262385,82884,264.27713576,611,0.00742649,82884,23.89743147,83206,80747,1592991523941],["tAVAX:USD",9.7515,2723.58913731,9.7589,2867.03700597,-0.0454,-0.00462011,9.7812,10278.34810712,9.8395,9.2547,1600862465185],["tBTCF0:USTF0",82839,7.88163289,82848,9.74195335,545,0.00662155,82852,191.15049452,83222,80694,1562164542332],["fUSD",0.0003020821917808219,0.000273972602739726,120,20671440.67750387,0.000178178082,7,51182.73817035,-0.00011043,-0.3826,0.00017818,190647902.18222517,0.000336,3.22e-06,null,null,297708548.60204273,1469734163000]]"#,
    );
    let Event::Board(board) = &decode_once(&spot, &mut s, &cache, &http_frame(5, "tickers", 200, &body), ts(NOW)).unwrap()[0].event else { panic!() };
    assert_eq!(
        board.iter().map(|p| (p.instrument.native.as_str(), p.price.to_string())).collect::<Vec<_>>(),
        [("tBTCUSD", "82771".to_string()), ("tUSTUSD", "0.99996".to_string()), ("tAVAX:USD", "9.7812".to_string())]
    );
    // No clock to read: the nonce is a counter.
    let e = encode_once(&spot, &mut s, &cache, &Command::SyncTime { req_id: CorrId(6) }, ts(NOW)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
}

fn ws(payload: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("public".into()), payload: bytes::Bytes::copy_from_slice(payload.as_bytes()) }
}

fn sub(id: u64, native: &str, tf: Timeframe) -> Command {
    Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(Book::of(native).venue(), native), data_type: DataType::Candles { tf } }
}

fn sent(r: &NativeRequest) -> serde_json::Value {
    let NativeRequest::WsSend { payload, .. } = r else { panic!("{r:?}") };
    serde_json::from_slice(payload).unwrap()
}

#[test]
fn the_socket_routes_by_channel_and_refuses_an_empty_snapshot() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let out = m.encode_batch(&[sub(7, "tBTCUSD", Timeframe::M1), sub(8, "tNOPEUSD", Timeframe::M1)], &mut s, &ctx).unwrap();
    let NativeRequest::WsOpen { base, path, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!((*base, path.as_str()), (WS, "/ws/2"));
    assert_eq!(sent(&out[1]), serde_json::json!({"event":"subscribe","channel":"candles","key":"trade:1m:tBTCUSD"}));
    // Recorded frames.
    let info = r#"{"event":"info","version":2,"serverId":"4d9fad5a-f0b9-4753-9cab-3f91e4aa8a51","platform":{"status":1}}"#;
    assert!(decode_once(&m, &mut s, &cache, &ws(info), ts(NOW)).unwrap().is_empty());
    for f in [r#"{"event":"subscribed","channel":"candles","chanId":264666,"key":"trade:1m:tBTCUSD"}"#, r#"{"event":"subscribed","channel":"candles","chanId":264667,"key":"trade:1m:tNOPEUSD"}"#] {
        assert!(decode_once(&m, &mut s, &cache, &ws(f), ts(NOW)).unwrap().is_empty(), "acknowledged by its snapshot");
    }
    // An unknown pair is accepted with an empty snapshot.
    let evs = decode_once(&m, &mut s, &cache, &ws("[264667,[]]"), ts(NOW)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(8)) && matches!(&e.event, Event::SubscriptionFail { error } if error.kind == ErrorKind::NotFound && error.message.contains("tNOPEUSD"))));
    // Its channel is let go on the next keepalive.
    assert_eq!(sent(&m.keepalive(&mut s, &ctx)[0]), serde_json::json!({"event":"unsubscribe","chanId":264667}));
    // The snapshot, newest first: the newest bar is the forming one.
    let snap = "[264666,[[1792403880000,82872,82790,82872,82757,0.19681827999999998],[1792403820000,82911,82890,82924,82857,0.42763879],[1792389540000,82732,82768,82768,82732,0.02159552]]]";
    let evs = decode_once(&m, &mut s, &cache, &ws(snap), ts(NOW)).unwrap();
    assert!(matches!(evs[0].event, Event::SubscriptionAck) && evs[0].corr_id == Some(CorrId(7)));
    let Event::Candle(cd) = &evs[1].event else { panic!("{evs:?}") };
    assert_eq!((cd.open_ts.millis(), cd.close.to_string(), cd.high.to_string(), cd.is_final), (1_792_403_880_000, "82790".into(), "82872".into(), false));
    assert_eq!(evs.len(), 2, "the history is not replayed");
    // An update repeats the previous bar next to the forming one; both go out as forming.
    let evs = decode_once(&m, &mut s, &cache, &ws("[264666,[1792403940000,82797,82803,82803,82797,0.04058534]]"), ts(NOW)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((cd.open_ts.millis(), cd.volume.to_string(), cd.is_final), (1_792_403_940_000, "0.04058534".into(), false));
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"[264666,"hb"]"#), ts(NOW)).unwrap().is_empty());
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"event":"pong","cid":1,"ts":1792403937975}"#), ts(NOW)).unwrap().is_empty());
    // Unsubscribing names the channel.
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(7) }], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[0]), serde_json::json!({"event":"unsubscribe","chanId":264666}));
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"event":"unsubscribed","status":"OK","chanId":264666}"#), ts(NOW)).unwrap().is_empty());
    assert!(decode_once(&m, &mut s, &cache, &ws("[264666,[1792404000000,1,1,1,1,1]]"), ts(NOW)).unwrap().is_empty(), "nobody listens any more");
}

#[test]
fn the_one_level_book_is_the_quote() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let q = |id: u64, native: &str| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(Book::of(native).venue(), native), data_type: DataType::Quote };
    let out = m.encode_batch(&[q(5, "tBTCUSD"), q(6, "tZZZNOPE")], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[1]), serde_json::json!({"event":"subscribe","channel":"book","symbol":"tBTCUSD","prec":"P0","freq":"F0","len":"1"}));
    // Recorded frames: the answer names the channel and the symbol, not a key.
    let ok = r#"{"event":"subscribed","channel":"book","chanId":191942,"symbol":"tBTCUSD","prec":"P0","freq":"F0","len":"1","pair":"BTCUSD"}"#;
    assert!(decode_once(&m, &mut s, &cache, &ws(ok), ts(NOW)).unwrap().is_empty());
    let no = r#"{"channel":"book","symbol":"tZZZNOPE","event":"error","msg":"symbol: invalid","code":10300,"pair":"ZZZNOPE"}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(no), ts(NOW)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(6)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("tZZZNOPE"))));
    let mut quote = |f: &str| -> Option<(String, String)> {
        let evs = decode_once(&m, &mut s, &cache, &ws(f), ts(NOW)).unwrap();
        evs.first().map(|e| {
            assert_eq!(e.corr_id, Some(CorrId(5)));
            let Event::Quote(q) = &e.event else { panic!("{evs:?}") };
            (q.bid.to_string(), q.ask.to_string())
        })
    };
    // Snapshot: bid (amount > 0) and ask (amount < 0).
    assert_eq!(quote("[191942,[[82231,2,23.32300331],[82247,1,-0.00022]]]"), Some(("82231".into(), "82247".into())));
    assert_eq!(quote("[191942,[82247,2,-0.00048383]]"), Some(("82231".into(), "82247".into())));
    // The top ask leaves: no quote until the next level arrives.
    assert_eq!(quote("[191942,[82247,0,-1]]"), None);
    assert_eq!(quote("[191942,[82246,1,-0.06079359]]"), Some(("82231".into(), "82246".into())));
    assert_eq!(quote("[191942,\"hb\"]"), None);
    // Closing the pane unsubscribes its channel.
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(5) }], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[0]), serde_json::json!({"event":"unsubscribe","chanId":191942}));
}

#[test]
fn refusals_restarts_and_late_acknowledgements() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    m.encode_batch(&[sub(7, "tBTCUSD", Timeframe::M1), sub(9, "tETHUSD", Timeframe::M1)], &mut s, &ctx).unwrap();
    // Recorded: the limit refusal names the key.
    let limit = r#"{"channel":"candles","key":"trade:1m:tETHUSD","event":"error","msg":"subscribe: limit","code":10305}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(limit), ts(NOW)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(9)) && matches!(&e.event, Event::SubscriptionFail { error } if error.kind == ErrorKind::RateLimited)));
    // Recorded: a second subscription to a key already carried; the first stands.
    let dup = r#"{"channel":"candles","key":"trade:1m:tBTCUSD","event":"error","msg":"subscribe: dup","code":10301,"chanId":264666}"#;
    assert!(decode_once(&m, &mut s, &cache, &ws(dup), ts(NOW)).unwrap().is_empty());
    // A pane closed before its channel id arrived is let go when it does.
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(7) }], &mut s, &ctx).unwrap();
    assert!(out.is_empty());
    decode_once(&m, &mut s, &cache, &ws(r#"{"event":"subscribed","channel":"candles","chanId":264666,"key":"trade:1m:tBTCUSD"}"#), ts(NOW)).unwrap();
    assert_eq!(sent(&m.keepalive(&mut s, &ctx)[0]), serde_json::json!({"event":"unsubscribe","chanId":264666}));
    assert!(m.keepalive(&mut s, &ctx).is_empty());
    // A refusal naming no pane is reported and the stream goes on (recorded).
    let evs = decode_once(&m, &mut s, &cache, &ws(r#"{"event":"error","msg":"unsubscribe: invalid","chanId":999999,"code":10400}"#), ts(NOW)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id.is_none() && matches!(e.event, Event::Error(_))));
    // A restart or the end of a maintenance ends the socket: the hub reconnects and subscribes again.
    for code in [20051, 20061] {
        let f = format!(r#"{{"event":"info","code":{code},"msg":"x"}}"#);
        assert_eq!(decode_once(&m, &mut s, &cache, &ws(&f), ts(NOW)).unwrap_err().kind, ErrorKind::Transient);
    }
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"event":"info","code":20060,"msg":"x"}"#), ts(NOW)).unwrap().is_empty());
}

#[test]
fn a_socket_refuses_a_31st_pane_and_the_other_books_names() {
    let m = Bitfinex::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(NOW), instruments: &cache, connector: &c };
    let mut panes: Vec<Command> = (0..31).map(|i| sub(100 + i, &format!("tC{i:02}USD"), Timeframe::M1)).collect();
    panes.push(sub(200, "tBTCF0:USTF0", Timeframe::M1));
    let out = m.encode_batch(&panes, &mut s, &ctx).unwrap();
    assert_eq!(out.len(), 31, "the socket and 30 subscriptions");
    let evs = decode_once(&m, &mut s, &cache, &ws(r#"{"event":"pong","cid":1,"ts":1}"#), ts(NOW)).unwrap();
    assert!(matches!(&evs[0], e if e.corr_id == Some(CorrId(130)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("30 panes"))));
    assert!(matches!(&evs[1], e if e.corr_id == Some(CorrId(200)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("derivatives"))));
    // Nothing else to send: a ping draws the frame that reports a refusal.
    let mut s = Session::default();
    let out = m.encode_batch(&[sub(300, "tBTCF0:USTF0", Timeframe::M1)], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[1]), serde_json::json!({"event":"ping","cid":1}));
}

fn account(m: &Bitfinex, s: &mut Session, kind: AccountKind, cursor: Option<String>, range: Option<TimeRange>) -> Vec<NativeRequest> {
    let cmd = Command::QueryAccount { req_id: CorrId(20), kind, symbols: vec![], range, cursor, account: None };
    encode_once(m, s, &InstrumentCache::new(), &cmd, ts(NOW)).unwrap()
}

fn account_chunk(evs: &[otw_connector_model::Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("{other:?}"),
    }
}

fn body_of(h: &HttpRequest) -> String {
    String::from_utf8(h.body.clone().unwrap().to_vec()).unwrap()
}

#[test]
fn the_summary_reads_the_permissions_then_the_wallets() {
    let m = Bitfinex::spot().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let h = one(&account(&m, &mut s, AccountKind::Summary, None, None)).clone();
    assert!(h.signed && h.method == Method::Post && h.base == AUTH && h.path == "/v2/auth/r/permissions" && body_of(&h) == "{}");
    // The docs sample (their quotes made JSON).
    let perms = r#"[["account",1,0],["history",1,0],["orders",1,1],["positions",1,0],["funding",0,0],["settings",0,0],["wallets",1,0],["withdraw",0,0],["ui_withdraw",0,0],["bfxpay",0,0]]"#;
    assert!(decode_once(&m, &mut s, &cache, &http_frame(20, "permissions", 200, perms), ts(NOW)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(20), &mut s);
    assert_eq!(one(&next).path, "/v2/auth/r/wallets");
    let wallets = r#"[["exchange","UST",19788.6529257,0,19788.6529257,"Exchange 2.0 UST for USD @ 11.696",{"reason":"TRADE"}],["margin","USTF0",500,0,null,null,null],["funding","USD",0,0,0,null,null]]"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(20, "wallets", 200, wallets), ts(NOW)).unwrap();
    let Event::AccountSummary(sm) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((sm.can_trade, sm.assets_held), (Some(true), 1));
    assert!(sm.permissions.contains(&"orders: read, write".to_string()) && sm.permissions.contains(&"wallets: read".to_string()));
    assert!(sm.detail.as_deref().unwrap().contains("exchange, margin"), "{:?}", sm.detail);
    // A key that may not read the wallets is summarised without asking them.
    let mut s = Session::default();
    account(&m, &mut s, AccountKind::Summary, None, None);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(20, "permissions", 200, r#"[["wallets",0,0],["orders",1,0]]"#), ts(NOW)).unwrap();
    assert!(matches!(&evs[0].event, Event::AccountSummary(sm) if sm.can_trade == Some(false)));
    assert!(m.follow_ups(CorrId(20), &mut s).is_empty());
}

#[test]
fn balances_sum_the_wallets_and_name_the_collateral_by_its_currency() {
    let m = Bitfinex::spot().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    account(&m, &mut s, AccountKind::Balances, None, None);
    let wallets = r#"[["exchange","UST",19788.6529257,0,19788.6529257,"Exchange 2.0 UST for USD @ 11.696",{"reason":"TRADE"}],["margin","USTF0",500,0,null,null,null],["exchange","BTC",0.5,0,0.25,null,null],["funding","USD",100,0,40,null,null],["exchange","ETH",0,0,0,null,null]]"#;
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "wallets", 200, wallets), ts(NOW)).unwrap());
    let AccountItems::Balances(b) = items else { panic!() };
    assert_eq!(
        b.iter().map(|b| (b.asset.as_str(), b.total.0.to_string(), b.available.0.to_string(), b.fiat)).collect::<Vec<_>>(),
        [("BTC", "0.5".to_string(), "0.25".to_string(), false), ("USD", "100".to_string(), "40".to_string(), true), ("UST", "20288.6529257".to_string(), "20288.6529257".to_string(), false)]
    );
    assert!(next.is_none());
    // A refused key says what to check.
    account(&m, &mut s, AccountKind::Balances, None, None);
    let e = decode_once(&m, &mut s, &cache, &http_frame(20, "wallets", 500, r#"["error",10114,"nonce: small"]"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("key of its own"), "{e:?}");
    account(&m, &mut s, AccountKind::Balances, None, None);
    let e = decode_once(&m, &mut s, &cache, &http_frame(20, "wallets", 500, r#"["error",10100,"apikey: digest invalid"]"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("Check the key"), "{e:?}");
}

#[test]
fn positions_and_orders_of_both_books() {
    let m = Bitfinex::spot().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    assert_eq!(one(&account(&m, &mut s, AccountKind::Positions, None, None)).path, "/v2/auth/r/positions");
    // The docs sample, and a derivatives one beside it.
    let body = r#"[["tBTCUSD","ACTIVE",0.0195,8565.0267019,0,0,-0.33455568705000516,-0.0003117550117425625,7045.876419249083,3.0673001895895604,null,142355652,1574002216000,1574002216000,null,0,null,0,0,{"reason":"TRADE","order_id":34271018124}],["tETHF0:USTF0","ACTIVE",-1.5,2400.5,0,0,12.25,0.003,3100,5,null,142355653,1792300000000,1792300000000,null,1,null,720,360,null]]"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "positions", 200, body), ts(NOW)).unwrap());
    let AccountItems::Positions(p) = items else { panic!() };
    assert_eq!((p[0].instrument.venue.0.as_str(), p[0].qty.0.to_string(), p[0].avg_price.unwrap().to_string(), p[0].currency.as_deref(), p[0].class), ("BITFINEX", "0.0195".to_string(), "8565.0267019".to_string(), Some("USD"), None));
    assert_eq!((p[1].instrument.venue.0.as_str(), p[1].qty.0.to_string(), p[1].unrealized_pnl.unwrap().0.to_string(), p[1].currency.as_deref(), p[1].class), ("BITFINEX_PERP", "-1.5".to_string(), "12.25".to_string(), Some("UST"), Some(AssetClass::Perp)));

    assert_eq!(one(&account(&m, &mut s, AccountKind::OpenOrders, None, None)).path, "/v2/auth/r/orders");
    // The docs sample (a margin sell, half left), and an exchange stop limit.
    let body = r#"[[33950998275,null,1573476747887,"tETHUSD",1573476748000,1573476748000,-0.25,-0.5,"LIMIT",null,null,null,0,"ACTIVE",null,null,220,0,0,0,null,null,null,0,0,null,null,null,"BFX",null,null,null],[33950998276,null,null,"tBTCUST",1792300000000,1792300000000,0.1,0.1,"EXCHANGE STOP LIMIT",null,null,null,0,"ACTIVE",null,null,90000,0,0,90500,null,null,null,0,0,null,null,null,"API>BFX",null,null,{}]]"#;
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "orders", 200, body), ts(NOW)).unwrap());
    let AccountItems::Orders(o) = items else { panic!() };
    assert_eq!((o[0].id.as_str(), o[0].qty.0.to_string(), o[0].filled_qty.0.to_string(), o[0].limit_price.unwrap().to_string(), o[0].order_type.as_str()), ("33950998275", "0.5".to_string(), "0.25".to_string(), "220".to_string(), "limit"));
    assert!(matches!(o[0].side, Mapped::Known(Side::Sell)));
    assert_eq!((o[1].limit_price.unwrap().to_string(), o[1].stop_price.unwrap().to_string(), o[1].quote_ccy.as_deref()), ("90500".to_string(), "90000".to_string(), Some("UST")));
    assert!(matches!(o[1].side, Mapped::Known(Side::Buy)) && next.is_none());
}

#[test]
fn fills_of_every_instrument_page_back_from_the_periods_end() {
    let m = Bitfinex::spot().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let from = NOW - 30 * 86_400;
    let range = TimeRange::new(ts(from), ts(NOW));
    let h = one(&account(&m, &mut s, AccountKind::Fills, None, Some(range))).clone();
    assert_eq!(h.path, "/v2/auth/r/trades/hist");
    assert_eq!(body_of(&h), format!(r#"{{"start":{},"end":{},"limit":1000,"sort":-1}}"#, from * 1000, NOW * 1000 - 1), "no symbol: every instrument at once");
    // The docs sample row (a sell, fee negative when paid), the newest first. A full page
    // ends on two fills of one millisecond.
    let row = |id: i64, mts: i64| format!(r#"[{id},"tETHUST",{mts},34938060782,-0.2,153.57,"MARKET",0,-1,-0.061668,"UST",1234]"#);
    let mut rows: Vec<String> = (0..998).map(|i| row(500_000 + i, 1_792_390_000_000 - i)).collect();
    rows.push(row(400_001, 1_792_380_000_000));
    rows.push(row(400_000, 1_792_380_000_000));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "tradesHist", 200, &format!("[{}]", rows.join(","))), ts(NOW)).unwrap());
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!(f.len(), 1000);
    assert_eq!((f[0].id.as_str(), f[0].qty.0.to_string(), f[0].price.0.to_string(), f[0].fee.0.to_string(), f[0].fee_asset.as_deref(), f[0].quote_ccy.as_deref()), ("500000", "0.2".to_string(), "153.57".to_string(), "0.061668".to_string(), Some("UST"), Some("UST")));
    assert!(matches!(f[0].side, Mapped::Known(Side::Sell)) && f[0].order_id.as_deref() == Some("34938060782"));
    let next = next.unwrap();
    // The next page ends at the oldest millisecond read, leaving out what was read of it.
    let h = one(&account(&m, &mut s, AccountKind::Fills, Some(next), Some(range))).clone();
    assert!(body_of(&h).contains(r#""end":1792380000000"#), "{}", body_of(&h));
    let body = format!("[{},{},{}]", row(400_002, 1_792_380_000_000), row(400_001, 1_792_380_000_000), row(399_000, 1_792_370_000_000));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "tradesHist", 200, &body), ts(NOW)).unwrap());
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!(f.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["400002", "399000"], "the boundary fill is not read twice");
    assert!(next.is_none(), "a short page ends the walk");
    let cmd = Command::QueryAccount { req_id: CorrId(20), kind: AccountKind::Fills, symbols: vec![], range: None, cursor: None, account: None };
    assert_eq!(encode_once(&m, &mut s, &cache, &cmd, ts(NOW)).unwrap_err().kind, ErrorKind::InvalidRequest);
}
