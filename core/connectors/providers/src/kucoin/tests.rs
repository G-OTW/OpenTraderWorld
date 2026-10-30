//! KuCoin, offline. Market-data fixtures are answers recorded from the public endpoints on
//! 2026-10-19; account fixtures follow the examples of KuCoin's own SDK specification.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn hist(m: &Kucoin, native: &str, tf: Timeframe, from: i64, to: i64, cursor: Option<String>) -> Command {
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

const XBT_CONTRACT: &str = r#"{"code":"200000","data":{"symbol":"XBTUSDTM","rootSymbol":"USDT","type":"FFWCSX","baseCurrency":"XBT","quoteCurrency":"USDT","settleCurrency":"USDT","multiplier":0.001,"lotSize":1,"tickSize":0.1,"isInverse":false,"expireDate":null,"status":"Open"}}"#;

#[test]
fn each_platform_declares_only_what_it_serves() {
    for m in [Kucoin::spot().unwrap(), Kucoin::futures().unwrap()] {
        let native = if m.book() == Book::Spot { "BTC-USDT" } else { "XBTUSDTM" };
        assert_undeclared_is_unsupported(&m, &InstrumentId::new(m.book().venue(), native));
    }
    assert_eq!(Kucoin::spot().unwrap().capabilities().provider, "kucoin");
    assert_eq!(Kucoin::futures().unwrap().capabilities().provider, "kucoin_futures");
    assert!(!Kucoin::spot().unwrap().capabilities().native_timeframes.contains(&Timeframe::W1), "spot weeks open on Thursday");
}

#[test]
fn spot_reads_backwards_and_its_columns_are_open_close_high_low() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_390_020, 1_792_390_320);
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTC-USDT", Timeframe::M1, from, to, None), ts(0)).unwrap();
    let h = one(&reqs);
    assert_eq!(h.path, "/api/v1/market/candles");
    assert_eq!((q(h, "type").as_deref(), q(h, "startAt").as_deref(), q(h, "endAt").as_deref()), (Some("1min"), Some("1792390020"), Some("1792390320")));
    // Recorded: newest first, the last minute still forming.
    let body = json(
        r#"{"code":"200000","data":[["1792390260","82764","82771","82771","82764","0.00742513","614.54729451"],["1792390200","82796.8","82764.1","82796.8","82764","0.1132435","9374.532328428"],["1792390140","82778.6","82796.8","82801.1","82778.6","0.42820594","35454.685727195"],["1792390080","82792.1","82778.6","82792.1","82778.6","0.91267382","75557.277385543"],["1792390020","82789","82792.1","82792.1","82782","0.15787339","13069.488810728"]]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, &body), ts(1_792_390_292)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else { panic!() };
    assert_eq!((next.as_ref(), *done), (None, true), "a short page is the last");
    assert_eq!(cs.len(), 5);
    let b = &cs[3];
    assert_eq!((b.open.to_string(), b.close.to_string(), b.high.to_string(), b.low.to_string()), ("82796.8".into(), "82764.1".into(), "82796.8".into(), "82764".into()));
    assert!(!cs[4].is_final, "the forming minute");
    assert!(cs[3].is_final, "closed 32 s ago, past the settle window");
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);
}

#[test]
fn a_full_spot_page_asks_below_its_oldest_bar() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_700_000_000, 1_700_000_000 + 3 * 86_400);
    encode_once(&m, &mut s, &cache, &hist(&m, "BTC-USDT", Timeframe::M1, from, to, None), ts(0)).unwrap();
    let newest = to - 60 - to % 60;
    let rows: Vec<String> = (0..1500).map(|i| format!(r#"["{}","1","1.5","2","0.5","10","15"]"#, newest - i * 60)).collect();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candles", 200, &format!(r#"{{"code":"200000","data":[{}]}}"#, rows.join(","))), ts(to + 3600)).unwrap();
    let (_, next, done) = &chunks(&evs)[0];
    assert!(!done);
    assert_eq!(next.as_deref(), Some((newest - 1499 * 60).to_string().as_str()), "endAt is exclusive: the oldest bar's open");
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTC-USDT", Timeframe::M1, from, to, next.clone()), ts(0)).unwrap();
    assert_eq!(q(one(&reqs), "endAt"), next.clone());
}

#[test]
fn a_futures_page_asks_the_contract_once_and_counts_coins() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_389_600, 1_792_389_780);
    let first = encode_once(&m, &mut s, &cache, &hist(&m, "XBTUSDTM", Timeframe::M1, from, to, None), ts(0)).unwrap();
    assert_eq!(one(&first).path, "/api/v1/contracts/XBTUSDTM", "the contract's size before its candles");
    assert!(decode_once(&m, &mut s, &cache, &http_frame(1, "contract", 200, XBT_CONTRACT), ts(0)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(1), &mut s);
    let h = one(&next);
    assert_eq!(h.path, "/api/v1/kline/query");
    assert_eq!((q(h, "granularity").as_deref(), q(h, "from").as_deref(), q(h, "to").as_deref()), (Some("1"), Some("1792389600000"), Some("1792389779999")));
    // Recorded: oldest first, numbers, volume in contracts of 0.001 XBT.
    let body = r#"{"code":"200000","data":[[1792389600000,82714.9,82714.9,82696.0,82714.8,198,16377.3566],[1792389660000,82724.5,82741.0,82724.5,82741.0,1381,114246.8932],[1792389720000,82750.0,82769.4,82750.0,82762.8,277,22925.8483]]}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, body), ts(to + 3600)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs.len(), next.as_ref(), *done), (3, None, true));
    let b = &cs[1];
    assert_eq!((b.open.to_string(), b.high.to_string(), b.low.to_string(), b.close.to_string()), ("82724.5".into(), "82741.0".into(), "82724.5".into(), "82741.0".into()));
    assert_eq!(b.volume.to_string(), "1.381", "1381 contracts of 0.001 XBT");
    assert_eq!(b.quote_volume.unwrap().to_string(), "114246.8932");
    // The size is kept: the next page goes straight to the candles.
    let again = encode_once(&m, &mut s, &cache, &hist(&m, "XBTUSDTM", Timeframe::M1, from, to, None), ts(0)).unwrap();
    assert_eq!(one(&again).path, "/api/v1/kline/query");
}

#[test]
fn a_full_futures_page_reads_on_after_its_newest_bar() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    s.sizes.insert("XBTUSDTM".into(), Decimal::new(1, 3));
    let cache = InstrumentCache::new();
    let (from, to) = (1_700_000_040, 1_700_000_040 + 86_400);
    encode_once(&m, &mut s, &cache, &hist(&m, "XBTUSDTM", Timeframe::M1, from, to, None), ts(0)).unwrap();
    let rows: Vec<String> = (0..200).map(|i| format!("[{},1,2,0.5,1.5,10,15]", (from + i * 60) * 1000)).collect();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, &format!(r#"{{"code":"200000","data":[{}]}}"#, rows.join(","))), ts(to + 3600)).unwrap();
    let (_, next, _) = &chunks(&evs)[0];
    assert_eq!(next.as_deref(), Some(((from + 200 * 60) * 1000).to_string().as_str()));
}

#[test]
fn an_unknown_contract_or_pair_is_refused_by_name() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist(&m, "ZZZNOPE", Timeframe::M1, 0, 60, None), ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "contract", 404, r#"{"msg":"The contract information you requested does not exist.","code":"404000"}"#), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    assert!(e.message.contains("ZZZNOPE"), "{}", e.message);
    let spot = Kucoin::spot().unwrap();
    let mut s = Session::default();
    encode_once(&spot, &mut s, &cache, &hist(&spot, "ZZZ-NOPE", Timeframe::M1, 0, 60, None), ts(0)).unwrap();
    let e = decode_once(&spot, &mut s, &cache, &http_frame(1, "candles", 200, r#"{"msg":"Unsupported trading pair.","code":"400100"}"#), ts(0)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("ZZZ-NOPE"), "{e:?}");
    // The spot best bid/ask answers `null` for a pair it does not list.
    let snap = Command::RequestSnapshot { req_id: CorrId(2), instrument: InstrumentId::new("KUCOIN", "ZZZ-NOPE"), data_type: DataType::Quote };
    encode_once(&spot, &mut s, &cache, &snap, ts(0)).unwrap();
    assert_eq!(decode_once(&spot, &mut s, &cache, &http_frame(2, "level1", 200, r#"{"code":"200000","data":null}"#), ts(0)).unwrap_err().kind, ErrorKind::NotFound);
}

#[test]
fn an_inverse_contract_is_refused_with_the_fix() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist(&m, "XBTUSDM", Timeframe::M1, 0, 60, None), ts(0)).unwrap();
    let body = r#"{"code":"200000","data":{"symbol":"XBTUSDM","type":"FFWCSX","baseCurrency":"XBT","quoteCurrency":"USD","settleCurrency":"XBT","multiplier":-1.0,"isInverse":true,"status":"Open"}}"#;
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "contract", 200, body), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    assert!(e.message.contains("XBTUSDTM"), "{}", e.message);
}

#[test]
fn the_contract_list_leaves_inverse_out_and_counts_lots_in_coins() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(0)).unwrap();
    let body = json(
        r#"{"code":"200000","data":[
        {"symbol":"XBTUSDTM","type":"FFWCSX","baseCurrency":"XBT","quoteCurrency":"USDT","settleCurrency":"USDT","multiplier":0.001,"lotSize":1,"tickSize":0.1,"isInverse":false,"expireDate":null,"status":"Open"},
        {"symbol":"XBTMZ26","type":"FFICSX","baseCurrency":"XBT","quoteCurrency":"USD","settleCurrency":"XBT","multiplier":-1.0,"lotSize":1,"tickSize":0.1,"isInverse":true,"expireDate":1798185600000,"status":"Open"},
        {"symbol":"ETHUSDCM","type":"FFWCSX","baseCurrency":"ETH","quoteCurrency":"USDC","settleCurrency":"USDC","multiplier":0.001,"lotSize":1,"tickSize":0.01,"isInverse":false,"expireDate":null,"status":"Open"}]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(3, "contracts", 200, &body), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!(specs.iter().map(|s| s.id.native.as_str()).collect::<Vec<_>>(), ["XBTUSDTM", "ETHUSDCM"]);
    assert_eq!((specs[0].multiplier.to_string(), specs[0].lot_size.unwrap().to_string()), ("0.001".into(), "0.001".into()));
    assert_eq!(specs[1].settlement_ccy.as_deref(), Some("USDC"));
    assert_eq!(s.sizes.get("ETHUSDCM").map(|d| d.to_string()).as_deref(), Some("0.001"));
}

#[test]
fn a_futures_quote_is_in_coins() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    s.sizes.insert("XBTUSDTM".into(), Decimal::new(1, 3));
    let cache = InstrumentCache::new();
    let snap = Command::RequestSnapshot { req_id: CorrId(4), instrument: InstrumentId::new("KUCOIN_FUTURES", "XBTUSDTM"), data_type: DataType::Quote };
    encode_once(&m, &mut s, &cache, &snap, ts(0)).unwrap();
    let body = r#"{"code":"200000","data":{"sequence":1748562815412,"symbol":"XBTUSDTM","side":"sell","size":1,"tradeId":"1948163575237","price":"82743.5","bestBidPrice":"82738.8","bestBidSize":770,"bestAskPrice":"82738.9","bestAskSize":100,"ts":1792390310213000000}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "ticker", 200, body), ts(0)).unwrap();
    let Event::Quote(qt) = &evs[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.bid_qty.unwrap().to_string(), qt.ask_qty.unwrap().to_string()), ("82738.8".into(), "0.770".into(), "0.100".into()));
}

#[test]
fn a_29_digit_turnover_reads() {
    // Recorded on the spot daily bar: more digits than a decimal holds.
    let row: Vec<Box<RawValue>> = serde_json::from_str(r#"["1792281600","81480.8","82111.6","82281.1","80690.5","1759.04020442297886410673","143602014.87539541598870307914"]"#).unwrap();
    let r = CandleRow::read(&row, Layout::Spot, Decimal::ONE).expect("row reads");
    assert!(r.turnover.is_some());
    assert!(r.vol.to_string().starts_with("1759.040204"));
}

fn bullet(m: &Kucoin, s: &mut Session, cache: &InstrumentCache) {
    let h = m.socket_bootstrap(CorrId(90), s).unwrap();
    assert_eq!((h.method, h.path.as_str()), (Method::Post, "/api/v1/bullet-public"));
    let body = r#"{"code":"200000","data":{"token":"tok.en==","instanceServers":[{"endpoint":"wss://ws-api-spot.kucoin.com/","encrypt":true,"protocol":"websocket","pingInterval":18000,"pingTimeout":10000}]}}"#;
    assert!(decode_once(m, s, cache, &http_frame(90, "bullet", 200, body), ts(0)).unwrap().is_empty());
}

fn ws(payload: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("public".into()), payload: payload.to_string().into() }
}

#[test]
fn the_socket_opens_where_the_bullet_points_and_routes_by_topic() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    cache.insert(InstrumentSpec::spot(InstrumentId::new("KUCOIN", "BTC-USDT"), "BTC", "USDT"));
    bullet(&m, &mut s, &cache);
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(1_792_390_380), instruments: &cache, connector: &c };
    let subs = [
        Command::Subscribe { sub_id: CorrId(7), instrument: InstrumentId::new("KUCOIN", "BTC-USDT"), data_type: DataType::Candles { tf: Timeframe::M1 } },
        Command::Subscribe { sub_id: CorrId(8), instrument: InstrumentId::new("KUCOIN", "ZZZ-NOPE"), data_type: DataType::Candles { tf: Timeframe::M1 } },
    ];
    let out = m.encode_batch(&subs, &mut s, &ctx).unwrap();
    let NativeRequest::WsOpenUrl { url, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!(url, "wss://ws-api-spot.kucoin.com/?token=tok.en==");
    assert_eq!(out.len(), 2, "the unlisted pair sends nothing");
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"id":"7","type":"subscribe","topic":"/market/candles:BTC-USDT_1min","privateChannel":false,"response":true}));
    // The welcome carries the refusal of the pair the venue would have acknowledged.
    let evs = decode_once(&m, &mut s, &cache, &ws(r#"{"id":"otw1","type":"welcome"}"#), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(8)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("ZZZ-NOPE"))));
    let ack = decode_once(&m, &mut s, &cache, &ws(r#"{"id":"7","type":"ack"}"#), ts(0)).unwrap();
    assert!(matches!(&ack[0].event, Event::SubscriptionAck));
    // Recorded.
    let msg = r#"{"topic":"/market/candles:BTC-USDT_1min","type":"message","subject":"trade.candles.update","data":{"symbol":"BTC-USDT","candles":["1792390380","82784.1","82785","82785.1","82784.1","0.05661426","4686.805781898"],"time":1792390395756151470}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(msg), ts(0)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!() };
    assert_eq!((cd.close.to_string(), cd.high.to_string(), cd.is_final), ("82785".into(), "82785.1".into(), false));
    assert_eq!(evs[0].exchange_ts.unwrap().millis(), 1_792_390_395_756);
}

#[test]
fn a_futures_socket_candle_swaps_volume_and_turnover() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    let mut spec = InstrumentSpec::spot(InstrumentId::new("KUCOIN_FUTURES", "XBTUSDTM"), "XBT", "USDT");
    spec.multiplier = Decimal::new(1, 3);
    cache.insert(spec);
    bullet(&m, &mut s, &cache);
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(9), instrument: InstrumentId::new("KUCOIN_FUTURES", "XBTUSDTM"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert!(String::from_utf8_lossy(payload).contains("/contractMarket/limitCandle:XBTUSDTM_1min"));
    // Recorded: turnover before volume, volume in contracts, time in milliseconds.
    let msg = r#"{"topic":"/contractMarket/limitCandle:XBTUSDTM_1min","type":"message","data":{"symbol":"XBTUSDTM","candles":["1792390380","82757.1","82765.1","82765.1","82750.1","11421.1508","138"],"time":1792390408661},"subject":"candle.stick"}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(msg), ts(0)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!() };
    assert_eq!((cd.volume.to_string(), cd.quote_volume.unwrap().to_string()), ("0.138".into(), "11421.1508".into()));
    assert_eq!(evs[0].exchange_ts.unwrap().millis(), 1_792_390_408_661);
}

#[test]
fn the_ticker_is_the_quote_on_both_books() {
    // Spot: `/market/ticker`, prices and sizes as strings, time in milliseconds.
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    cache.insert(InstrumentSpec::spot(InstrumentId::new("KUCOIN", "BTC-USDT"), "BTC", "USDT"));
    bullet(&m, &mut s, &cache);
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(5), instrument: InstrumentId::new("KUCOIN", "BTC-USDT"), data_type: DataType::Quote };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert!(String::from_utf8_lossy(payload).contains(r#""topic":"/market/ticker:BTC-USDT""#));
    // Recorded.
    let msg = r#"{"topic":"/market/ticker:BTC-USDT","type":"message","subject":"trade.ticker","data":{"bestAsk":"82198.4","bestAskSize":"0.05753788","bestBid":"82198.3","bestBidSize":"1.42923","price":"82198.4","sequence":"38908751166","size":"0.0009628","time":1792409853888}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(msg), ts(1_792_409_854)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(5)));
    assert_eq!(evs[0].exchange_ts.unwrap().millis(), 1_792_409_853_888);
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82198.3".to_string(), "82198.4".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);

    // Futures: `/contractMarket/tickerV2`, sizes in contracts, time in nanoseconds.
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    let mut spec = InstrumentSpec::spot(InstrumentId::new("KUCOIN_FUTURES", "XBTUSDTM"), "XBT", "USDT");
    spec.multiplier = Decimal::new(1, 3);
    cache.insert(spec);
    bullet(&m, &mut s, &cache);
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(6), instrument: InstrumentId::new("KUCOIN_FUTURES", "XBTUSDTM"), data_type: DataType::Quote };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    assert!(String::from_utf8_lossy(payload).contains("/contractMarket/tickerV2:XBTUSDTM"));
    let msg = r#"{"topic":"/contractMarket/tickerV2:XBTUSDTM","type":"message","subject":"tickerV2","sn":1748569350084,"data":{"symbol":"XBTUSDTM","sequence":1748569350084,"bestBidSize":1108,"bestBidPrice":"82185.9","bestAskPrice":"82186","bestAskSize":104,"ts":1792409856176000000}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(msg), ts(1_792_409_857)).unwrap();
    assert_eq!(evs[0].exchange_ts.unwrap().millis(), 1_792_409_856_176);
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82185.9".to_string(), "82186".to_string()));
    assert_eq!(q.bid_qty.map(|x| x.to_string()), Some("1.108".to_string()), "contracts of 0.001 XBT");
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(6) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    assert!(String::from_utf8_lossy(payload).contains(r#""type":"unsubscribe""#));
}

#[test]
fn a_ping_goes_out_before_the_venue_interval() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    cache.insert(InstrumentSpec::spot(InstrumentId::new("KUCOIN", "BTC-USDT"), "BTC", "USDT"));
    bullet(&m, &mut s, &cache);
    let c = otw_connector_model::ConnectorId("t".into());
    let at = |t: i64| MapCtx { received_ts: ts(t), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(7), instrument: InstrumentId::new("KUCOIN", "BTC-USDT"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    m.encode_batch(&[sub], &mut s, &at(100)).unwrap();
    assert!(m.keepalive(&mut s, &at(110)).is_empty());
    let ping = m.keepalive(&mut s, &at(113));
    let [NativeRequest::WsSend { payload, .. }] = ping.as_slice() else { panic!("{ping:?}") };
    assert!(String::from_utf8_lossy(payload).contains(r#""type":"ping""#));
}

fn account(kind: AccountKind, symbols: &[&str], cursor: Option<String>, days: i64) -> Command {
    Command::QueryAccount {
        req_id: CorrId(5),
        kind,
        symbols: symbols.iter().map(|s| InstrumentId::new("KUCOIN", s)).collect(),
        range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_000_000 + days * 86_400))),
        cursor,
        account: None,
    }
}

#[test]
fn spot_fills_walk_each_pair_a_week_at_a_time_without_overlap() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let mut cursor = None;
    let mut seen = Vec::new();
    loop {
        let reqs = encode_once(&m, &mut s, &cache, &account(AccountKind::Fills, &["BTC-USDT", "ETH-USDT"], cursor.clone(), 10), ts(0)).unwrap();
        let h = one(&reqs);
        assert!(h.signed);
        let n = |k: &str| q(h, k).unwrap().parse::<i64>().unwrap();
        seen.push((q(h, "symbol").unwrap(), n("startAt"), n("endAt")));
        let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "hfFills", 200, r#"{"code":"200000","data":{"items":[],"lastId":0}}"#), ts(0)).unwrap();
        let Event::AccountChunk { next_cursor, .. } = &evs[0].event else { panic!() };
        match next_cursor {
            Some(c) => cursor = Some(c.clone()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 4, "two slices per pair: {seen:?}");
    let (first, end) = (1_700_000_000_000_i64, (1_700_000_000 + 10 * 86_400) * 1000 - 1);
    for pair in [&seen[..2], &seen[2..]] {
        assert_eq!(pair[0].2, end, "from the period's end");
        assert_eq!(pair[1].2, pair[0].1 - 1, "slices tile without overlap");
        assert_eq!(pair[1].1, first, "down to its start");
        assert!(pair.iter().all(|(_, a, b)| b - a < 7 * 86_400_000));
    }
    assert_eq!((seen[0].0.as_str(), seen[2].0.as_str()), ("BTC-USDT", "ETH-USDT"));
}

#[test]
fn a_full_spot_fill_page_reads_below_its_last_id() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &account(AccountKind::Fills, &["BTC-USDT"], None, 3), ts(0)).unwrap();
    let row = r#"{"id":19814995255305,"orderId":"6717422bd51c29000775ea03","tradeId":11029373945659392,"symbol":"BTC-USDT","side":"buy","liquidity":"taker","type":"limit","price":"67717.6","size":"0.00001","funds":"0.677176","fee":"0.000677176","feeRate":"0.001","feeCurrency":"USDT","tradeType":"TRADE","createdAt":1729577515473}"#;
    let rows = vec![row; 100].join(",");
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "hfFills", 200, &format!(r#"{{"code":"200000","data":{{"items":[{rows}],"lastId":19814995255305}}}}"#)), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!((f[0].id.as_str(), f[0].fee.to_string(), f[0].fee_asset.as_deref()), ("11029373945659392", "0.000677176".into(), Some("USDT")));
    let reqs = encode_once(&m, &mut s, &cache, &account(AccountKind::Fills, &["BTC-USDT"], next_cursor.clone(), 3), ts(0)).unwrap();
    assert_eq!(q(one(&reqs), "lastId").as_deref(), Some("19814995255305"));
    let e = encode_once(&m, &mut s, &cache, &account(AccountKind::Fills, &[], None, 3), ts(0)).unwrap_err();
    assert!(e.message.contains("one pair at a time"));
}

#[test]
fn spot_orders_are_asked_for_each_symbol_the_venue_names() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &account(AccountKind::OpenOrders, &[], None, 1), ts(0)).unwrap();
    assert_eq!(one(&reqs).path, "/api/v1/hf/orders/active/symbols");
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "activeSymbols", 200, r#"{"code":"200000","data":{"symbols":["ETH-USDT","BTC-USDT"]}}"#), ts(0)).unwrap();
    let Event::AccountChunk { next_cursor: Some(c), .. } = &evs[0].event else { panic!() };
    let reqs = encode_once(&m, &mut s, &cache, &account(AccountKind::OpenOrders, &[], Some(c.clone()), 1), ts(0)).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "symbol").as_deref()), ("/api/v1/hf/orders/active", Some("ETH-USDT")));
    let body = r#"{"code":"200000","data":[{"id":"67120bbef094e200070976f6","symbol":"ETH-USDT","type":"limit","side":"buy","price":"50000","size":"0.00001","dealSize":"0","createdAt":1729235902748}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "activeOrders", 200, body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].limit_price.unwrap().to_string(), next_cursor.as_deref()), ("50000".into(), Some("BTC-USDT")));
}

#[test]
fn spot_balances_add_funding_and_trading_accounts() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &account(AccountKind::Balances, &[], None, 1), ts(0)).unwrap();
    let body = r#"{"code":"200000","data":[{"id":"1","currency":"USDT","type":"trade","balance":"26.66759503","available":"26.66759503","holds":"0"},{"id":"2","currency":"USDT","type":"main","balance":"0.01","available":"0.01","holds":"0"},{"id":"3","currency":"BTC","type":"margin","balance":"1","available":"1","holds":"0"}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "accounts", 200, body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Balances(b), .. } = &evs[0].event else { panic!() };
    assert_eq!(b.len(), 1, "the margin account is not read");
    assert_eq!((b[0].asset.as_str(), b[0].total.to_string()), ("USDT", "26.67759503".into()));
}

#[test]
fn the_key_test_says_when_it_could_trade() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &account(AccountKind::Summary, &[], None, 1), ts(0)).unwrap();
    let body = r#"{"code":"200000","data":{"remark":"account1","apiKey":"6705f5c311545b000157d3eb","apiVersion":3,"permission":"General,Spot","createdAt":1728443843000,"uid":165111215,"isMaster":true}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "apiKey", 200, body), ts(0)).unwrap();
    let Event::AccountSummary(a) = &evs[0].event else { panic!() };
    assert_eq!(a.can_trade, Some(true));
    assert!(!a.detail.as_deref().unwrap().contains("165111215"), "the uid is not echoed");
}

#[test]
fn futures_fills_page_inside_each_week_and_skip_inverse_rows() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = |c: Option<String>| Command::QueryAccount { req_id: CorrId(6), kind: AccountKind::Fills, symbols: vec![], range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_000_000 + 9 * 86_400))), cursor: c, account: None };
    let reqs = encode_once(&m, &mut s, &cache, &cmd(None), ts(0)).unwrap();
    assert_eq!(q(one(&reqs), "currentPage").as_deref(), Some("1"));
    let row = |id: &str, settle: &str| format!(r#"{{"symbol":"XBTUSDTM","tradeId":"{id}","orderId":"o","side":"buy","price":"86275.1","size":1,"value":"86.2751","fee":"0.05176506","feeCurrency":"{settle}","settleCurrency":"{settle}","tradeType":"trade","tradeTime":1740640088244000000,"createdAt":1740640088427}}"#);
    let body = format!(r#"{{"code":"200000","data":{{"currentPage":1,"pageSize":1000,"totalNum":2,"totalPage":2,"items":[{},{}]}}}}"#, row("1828954878212", "USDT"), row("9", "XBT"));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(6, "fills", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!(f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1828954878212"], "a coin-margined fill is not read");
    assert_eq!((f[0].qty.to_string(), f[0].quote_ccy.as_deref()), ("1".into(), Some("USDT")), "contracts, valued by the instrument list");
    let reqs = encode_once(&m, &mut s, &cache, &cmd(next_cursor.clone()), ts(0)).unwrap();
    assert_eq!(q(one(&reqs), "currentPage").as_deref(), Some("2"), "the same week's next page");
    let last = r#"{"code":"200000","data":{"currentPage":2,"pageSize":1000,"totalNum":2,"totalPage":2,"items":[]}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(6, "fills", 200, last), ts(0)).unwrap();
    let Event::AccountChunk { next_cursor, .. } = &evs[0].event else { panic!() };
    let reqs = encode_once(&m, &mut s, &cache, &cmd(next_cursor.clone()), ts(0)).unwrap();
    let h = one(&reqs);
    assert_eq!(q(h, "currentPage").as_deref(), Some("1"));
    assert_eq!(q(h, "startAt").unwrap().parse::<i64>().unwrap(), 1_700_000_000_000, "the second week reaches the period's start");
}

#[test]
fn futures_positions_read_each_settlement_coin() {
    let m = Kucoin::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = |c: Option<String>| Command::QueryAccount { req_id: CorrId(6), kind: AccountKind::Positions, symbols: vec![], range: None, cursor: c, account: None };
    let reqs = encode_once(&m, &mut s, &cache, &cmd(None), ts(0)).unwrap();
    assert_eq!(q(one(&reqs), "currency").as_deref(), Some("USDT"));
    let body = r#"{"code":"200000","data":[{"symbol":"XBTUSDTM","currentQty":-3,"avgEntryPrice":82000.5,"unrealisedPnl":-1.2,"markPrice":82100.1,"settleCurrency":"USDT","isInverse":false}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(6, "positions", 200, body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!((p[0].qty.to_string(), p[0].avg_price.unwrap().to_string()), ("-3".into(), "82000.5".into()));
    let reqs = encode_once(&m, &mut s, &cache, &cmd(next_cursor.clone()), ts(0)).unwrap();
    assert_eq!(q(one(&reqs), "currency").as_deref(), Some("USDC"));
}

#[test]
fn a_refused_passphrase_names_the_fix() {
    let m = Kucoin::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &account(AccountKind::Balances, &[], None, 1), ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(5, "accounts", 401, r#"{"code":"400004","msg":"Invalid KC-API-PASSPHRASE"}"#), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Auth);
    assert!(e.message.contains("passphrase"), "{}", e.message);
}
