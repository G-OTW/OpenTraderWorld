//! Gate.io, offline. Market-data fixtures are answers recorded from the public endpoints on
//! 2026-10-19; account fixtures follow the models of Gate.io's own SDK (gateapi-python,
//! `docs/*.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

/// The moment the recorded answers were taken: Monday 2026-10-19 06:48 UTC.
const NOW: i64 = 1_792_392_490;

fn hist(m: &Gateio, native: &str, tf: Timeframe, from: i64, to: i64, cursor: Option<String>) -> Command {
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

const BTC_CONTRACT: &str = r#"{"name":"BTC_USDT","type":"direct","quanto_multiplier":"0.0001","order_price_round":"0.1","order_size_min":1,"in_delisting":false,"status":"trading","contract_type":""}"#;

#[test]
fn each_book_declares_only_what_it_serves() {
    for m in [Gateio::spot().unwrap(), Gateio::futures().unwrap()] {
        assert_undeclared_is_unsupported(&m, &InstrumentId::new(m.book().venue(), "BTC_USDT"));
        assert_eq!(m.capabilities().provider, "gateio");
    }
    assert!(!Gateio::futures().unwrap().capabilities().native_timeframes.contains(&Timeframe::MO1), "the futures 30d bar is not a month");
}

#[test]
fn spot_reads_its_columns_and_its_closed_flag() {
    let m = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_392_360, 1_792_392_540);
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let h = one(&reqs);
    assert_eq!(h.path, "/api/v4/spot/candlesticks");
    assert_eq!(
        (q(h, "currency_pair").as_deref(), q(h, "interval").as_deref(), q(h, "from").as_deref(), q(h, "to").as_deref()),
        (Some("BTC_USDT"), Some("1m"), Some("1792392360"), Some("1792392480")),
        "both ends inclusive: the last bar opening before `to`"
    );
    // Recorded: `[t, quote volume, close, high, low, open, base volume, closed]`.
    let body = json(
        r#"[["1792392360","439886.01858520","82818","82818","82791.6","82791.6","5.31218000","true"],["1792392420","420447.27052520","82791.5","82826.2","82791.5","82817.9","5.07663200","true"],["1792392480","32071.12884230","82791.5","82791.6","82791.5","82791.6","0.38737200","false"]]"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candlesticks", 200, &body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs.len(), next.as_ref(), *done), (3, None, true));
    let b = &cs[1];
    assert_eq!((b.open.to_string(), b.high.to_string(), b.low.to_string(), b.close.to_string()), ("82817.9".into(), "82826.2".into(), "82791.5".into(), "82791.5".into()));
    assert_eq!((b.volume.to_string(), b.quote_volume.unwrap().to_string()), ("5.07663200".into(), "420447.27052520".into()));
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, true, false], "the venue's own flag");
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);
}

#[test]
fn a_long_range_walks_back_a_page_at_a_time_and_stops_at_the_venues_floor() {
    let m = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // Thirty days of minutes: more than the 10000 bars the venue keeps.
    let (from, to) = (NOW - 30 * 86_400, NOW - NOW % 60);
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    let h = one(&reqs);
    let (a, b): (i64, i64) = (q(h, "from").unwrap().parse().unwrap(), q(h, "to").unwrap().parse().unwrap());
    assert_eq!((b - a) / 60 + 1, 1000, "a page is 1000 bars, both ends counted");
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candlesticks", 200, "[]"), ts(NOW)).unwrap();
    let (_, next, done) = &chunks(&evs)[0];
    assert!(!done);
    assert_eq!(next.as_deref(), Some((a - 60).to_string().as_str()));
    // Walk to the end: the last page stops at the oldest bar the venue still answers.
    let mut cursor = next.clone();
    let mut oldest = a;
    let mut pages = 1;
    while let Some(c) = cursor {
        let reqs = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, Some(c)), ts(NOW)).unwrap();
        oldest = q(one(&reqs), "from").unwrap().parse().unwrap();
        let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candlesticks", 200, "[]"), ts(NOW)).unwrap();
        cursor = chunks(&evs)[0].1.clone();
        pages += 1;
    }
    assert_eq!(pages, 10, "9980 bars in pages of 1000");
    assert!(oldest >= NOW - 10_000 * 60, "asked {oldest}, past the 10000-bar floor");
}

#[test]
fn a_range_wholly_older_than_the_venue_keeps_is_refused_with_the_date() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    s.sizes.insert("BTC_USDT".into(), Decimal::new(1, 4));
    let cache = InstrumentCache::new();
    let e = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, 1_700_000_000, 1_700_003_600, None), ts(NOW)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    assert!(e.message.contains("10000") && e.message.contains("2026-10-12"), "{}", e.message);
    // Days reach back far enough for any listing.
    assert!(encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::D1, 1_574_035_200, 1_600_000_000, None), ts(NOW)).is_ok());
}

#[test]
fn a_futures_page_asks_the_contract_once_and_counts_coins() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_392_360, 1_792_392_540);
    let first = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    assert_eq!(one(&first).path, "/api/v4/futures/usdt/contracts/BTC_USDT", "the contract's size before its candles");
    assert!(decode_once(&m, &mut s, &cache, &http_frame(1, "contract", 200, BTC_CONTRACT), ts(NOW)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(1), &mut s);
    let h = one(&next);
    assert_eq!(h.path, "/api/v4/futures/usdt/candlesticks");
    assert_eq!((q(h, "contract").as_deref(), q(h, "interval").as_deref()), (Some("BTC_USDT"), Some("1m")));
    // Recorded: objects, volume a number of contracts of 0.0001 BTC, `sum` the turnover.
    let body = json(
        r#"[{"o":"82739.1","v":145034,"t":1792392360,"c":"82775.8","l":"82739.1","h":"82775.8","sum":"1200352.26399"},{"o":"82775.7","v":101865,"t":1792392420,"c":"82743.3","l":"82743.3","h":"82787.3","sum":"843234.3238"},{"o":"82743.4","v":19513,"t":1792392480,"c":"82743.4","l":"82743.3","h":"82743.4","sum":"161457.02314"}]"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "candlesticks", 200, &body), ts(NOW)).unwrap();
    let (HistoryItems::Candles(cs), _, done) = &chunks(&evs)[0] else { panic!() };
    assert!(*done);
    assert_eq!(cs[0].volume.to_string(), "14.5034", "145034 contracts of 0.0001 BTC");
    assert_eq!(cs[0].quote_volume.unwrap().to_string(), "1200352.26399");
    // No flag on the futures archive: final once the period plus the settle window is over.
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, false, false]);
    let again = encode_once(&m, &mut s, &cache, &hist(&m, "BTC_USDT", Timeframe::M1, from, to, None), ts(NOW)).unwrap();
    assert_eq!(one(&again).path, "/api/v4/futures/usdt/candlesticks", "the size is kept");
}

#[test]
fn weeks_and_months_use_each_books_own_token() {
    let spot = Gateio::spot().unwrap();
    let fut = Gateio::futures().unwrap();
    assert_eq!((interval(Book::Spot, Timeframe::W1), interval(Book::Futures, Timeframe::W1)), (Some("7d"), Some("1w")));
    assert_eq!((interval(Book::Spot, Timeframe::MO1), interval(Book::Futures, Timeframe::MO1)), (Some("30d"), None));
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // A month is not a fixed length: one call over the whole range.
    let reqs = encode_once(&spot, &mut s, &cache, &hist(&spot, "BTC_USDT", Timeframe::MO1, 1_767_225_600, NOW, None), ts(NOW)).unwrap();
    assert_eq!((q(one(&reqs), "interval").as_deref(), q(one(&reqs), "from").as_deref()), (Some("30d"), Some("1767225600")));
    let mut s = Session::default();
    s.sizes.insert("BTC_USDT".into(), Decimal::new(1, 4));
    let reqs = encode_once(&fut, &mut s, &cache, &hist(&fut, "BTC_USDT", Timeframe::W1, 1_767_225_600, NOW, None), ts(NOW)).unwrap();
    assert_eq!(q(one(&reqs), "interval").as_deref(), Some("1w"));
}

#[test]
fn an_unknown_pair_or_contract_is_refused_by_name() {
    let spot = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&spot, &mut s, &cache, &hist(&spot, "ZZZ_NOPE", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    let e = decode_once(&spot, &mut s, &cache, &http_frame(1, "candlesticks", 400, r#"{"label":"INVALID_CURRENCY_PAIR","message":"Invalid currency pair ZZZ_NOPE"}"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("ZZZ_NOPE"), "{e:?}");
    let fut = Gateio::futures().unwrap();
    let mut s = Session::default();
    encode_once(&fut, &mut s, &cache, &hist(&fut, "ZZZ_NOPE", Timeframe::M1, NOW - 600, NOW, None), ts(NOW)).unwrap();
    let e = decode_once(&fut, &mut s, &cache, &http_frame(1, "contract", 400, r#"{"label":"CONTRACT_NOT_FOUND"}"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("ZZZ_NOPE"), "{e:?}");
    assert!(spot.instrument_id("BTC-USDT").is_err(), "a dash is not Gate.io's separator");
}

#[test]
fn the_lists_read_precisions_and_contract_sizes() {
    let spot = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&spot, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(NOW)).unwrap();
    // Recorded (trimmed).
    let body = json(
        r#"[{"id":"BTC_USDT","base":"BTC","base_name":"Bitcoin","quote":"USDT","quote_name":"Tether","fee":"0.2","min_base_amount":"0.000001","min_quote_amount":"3","amount_precision":6,"precision":1,"trade_status":"tradable","sell_start":0,"buy_start":0,"type":"normal"},
        {"id":"OLD_USDT","base":"OLD","quote":"USDT","amount_precision":2,"precision":4,"trade_status":"untradable"}]"#,
    );
    let evs = decode_once(&spot, &mut s, &cache, &http_frame(3, "currency_pairs", 200, &body), ts(NOW)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!((specs[0].tick_size.unwrap().to_string(), specs[0].lot_size.unwrap().to_string(), specs[0].min_qty.unwrap().to_string()), ("0.1".into(), "0.000001".into(), "0.000001".into()));
    assert!(specs[0].active && !specs[1].active);
    assert!(matches!(evs.last().unwrap().event, Event::InstrumentsEnd));

    let fut = Gateio::futures().unwrap();
    let mut s = Session::default();
    encode_once(&fut, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, ts(NOW)).unwrap();
    let body = format!(
        r#"[{BTC_CONTRACT},{{"name":"AAPLX_USDT","type":"direct","quanto_multiplier":"0.01","order_price_round":"0.01","order_size_min":1,"in_delisting":false,"status":"trading","contract_type":"stocks"}},{{"name":"BTC_USD","type":"inverse","quanto_multiplier":"0","status":"trading"}}]"#
    );
    let evs = decode_once(&fut, &mut s, &cache, &http_frame(3, "contracts", 200, &body), ts(NOW)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert_eq!(specs.iter().map(|s| s.id.native.as_str()).collect::<Vec<_>>(), ["BTC_USDT", "AAPLX_USDT"], "inverse left out");
    assert_eq!((specs[0].multiplier.to_string(), specs[0].lot_size.unwrap().to_string(), specs[0].min_qty.unwrap().to_string()), ("0.0001".into(), "0.0001".into(), "0.0001".into()));
    assert_eq!((specs[0].base.as_deref(), specs[0].quote.as_str()), (Some("BTC"), "USDT"));
    assert_eq!(specs[1].note.as_deref(), Some("stocks"));
    assert_eq!(s.sizes.get("AAPLX_USDT").map(|d| d.to_string()).as_deref(), Some("0.01"));
}

#[test]
fn quotes_and_the_board() {
    let fut = Gateio::futures().unwrap();
    let mut s = Session::default();
    s.sizes.insert("BTC_USDT".into(), Decimal::new(1, 4));
    let cache = InstrumentCache::new();
    let snap = Command::RequestSnapshot { req_id: CorrId(4), instrument: InstrumentId::new("GATEIO_FUTURES", "BTC_USDT"), data_type: DataType::Quote };
    let reqs = encode_once(&fut, &mut s, &cache, &snap, ts(NOW)).unwrap();
    assert_eq!(q(one(&reqs), "contract").as_deref(), Some("BTC_USDT"));
    // Recorded (trimmed): sizes in contracts.
    let body = r#"[{"last":"82744.9","contract":"BTC_USDT","highest_bid":"82744.9","highest_size":"49928","lowest_ask":"82745","lowest_size":"29800","quanto_multiplier":"0.0001"}]"#;
    let evs = decode_once(&fut, &mut s, &cache, &http_frame(4, "tickers", 200, body), ts(NOW)).unwrap();
    let Event::Quote(qt) = &evs[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.bid_qty.unwrap().to_string(), qt.ask_qty.unwrap().to_string()), ("82744.9".into(), "4.9928".into(), "2.9800".into()));

    let spot = Gateio::spot().unwrap();
    let mut s = Session::default();
    encode_once(&spot, &mut s, &cache, &Command::RequestBoard { req_id: CorrId(5) }, ts(NOW)).unwrap();
    let body = r#"[{"currency_pair":"BTC_USDT","last":"82783.7","lowest_ask":"82783.7","lowest_size":"0.012453","highest_bid":"82783.6","highest_size":"5.142886"},{"currency_pair":"MSFTON_USDT","last":"534.14"}]"#;
    let evs = decode_once(&spot, &mut s, &cache, &http_frame(5, "tickers", 200, body), ts(NOW)).unwrap();
    let Event::Board(board) = &evs[0].event else { panic!() };
    assert_eq!(board.iter().map(|p| (p.instrument.native.as_str(), p.price.to_string())).collect::<Vec<_>>(), [("BTC_USDT", "82783.7".to_string()), ("MSFTON_USDT", "534.14".to_string())]);
}

fn ws(payload: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("public".into()), payload: payload.to_string().into() }
}

#[test]
fn the_spot_socket_names_each_pane_and_routes_by_name() {
    let m = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(1_792_392_666), instruments: &cache, connector: &c };
    let subs = [
        Command::Subscribe { sub_id: CorrId(7), instrument: InstrumentId::new("GATEIO", "BTC_USDT"), data_type: DataType::Candles { tf: Timeframe::M1 } },
        Command::Subscribe { sub_id: CorrId(8), instrument: InstrumentId::new("GATEIO", "ZZZ_NOPE"), data_type: DataType::Candles { tf: Timeframe::M1 } },
    ];
    let out = m.encode_batch(&subs, &mut s, &ctx).unwrap();
    let NativeRequest::WsOpen { base, path, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!((*base, path.as_str()), (WS, "/ws/v4/"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"time":1792392666,"id":7,"channel":"spot.candlesticks","event":"subscribe","payload":["1m","BTC_USDT"]}));
    // Recorded answers.
    let ok = r#"{"time":1792392666,"time_ms":1792392666937,"id":7,"conn_id":"896616a1ce9c7404","channel":"spot.candlesticks","event":"subscribe","payload":["1m","BTC_USDT"],"result":{"status":"success"}}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(ok), ts(0)).unwrap()[..], [e] if e.corr_id == Some(CorrId(7)) && matches!(e.event, Event::SubscriptionAck)));
    let fail = r#"{"time":1792392666,"time_ms":1792392666938,"id":8,"channel":"spot.candlesticks","event":"subscribe","payload":["1m","ZZZ_NOPE"],"error":{"code":2,"message":"unknown currency pair: ZZZ_NOPE"},"result":{"status":"fail"}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(fail), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(8)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("ZZZ_NOPE"))));
    let forming = r#"{"time":1792392668,"time_ms":1792392668530,"channel":"spot.candlesticks","event":"update","result":{"t":"1792392660","v":"31634.3642413","c":"82781.5","h":"82783.3","l":"82781.5","o":"82783.3","n":"1m_BTC_USDT","a":"0.382141","w":false}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(forming), ts(0)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!() };
    assert_eq!((cd.volume.to_string(), cd.quote_volume.unwrap().to_string(), cd.is_final), ("0.382141".into(), "31634.3642413".into(), false), "a is the base, v the quote");
    assert_eq!((cd.open_ts.secs(), evs[0].exchange_ts.unwrap().millis()), (1_792_392_660, 1_792_392_668_530));
    let closed = r#"{"time":1792392720,"time_ms":1792392720498,"channel":"spot.candlesticks","event":"update","result":{"t":"1792392660","v":"542325.1334564","c":"82772.4","h":"82789.3","l":"82772.3","o":"82783.3","n":"1m_BTC_USDT","a":"6.551148","w":true}}"#;
    let Event::Candle(cd) = &decode_once(&m, &mut s, &cache, &ws(closed), ts(0)).unwrap()[0].event else { panic!() };
    assert!(cd.is_final);
    // Unsubscribing names the same pair and interval.
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(7) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!((v["event"].as_str(), v["payload"].clone()), (Some("unsubscribe"), serde_json::json!(["1m", "BTC_USDT"])));
}

#[test]
fn a_futures_socket_counts_contracts_into_coins_and_refuses_an_unlisted_contract() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    let row: ContractRow = serde_json::from_str(BTC_CONTRACT).unwrap();
    cache.insert(Gateio::contract_spec(&row).unwrap());
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(1_792_392_695), instruments: &cache, connector: &c };
    let subs = [
        Command::Subscribe { sub_id: CorrId(7), instrument: InstrumentId::new("GATEIO_FUTURES", "BTC_USDT"), data_type: DataType::Candles { tf: Timeframe::W1 } },
        Command::Subscribe { sub_id: CorrId(9), instrument: InstrumentId::new("GATEIO_FUTURES", "ZZZ_NOPE"), data_type: DataType::Candles { tf: Timeframe::M1 } },
    ];
    let out = m.encode_batch(&subs, &mut s, &ctx).unwrap();
    let NativeRequest::WsOpen { base, path, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!((*base, path.as_str()), (FX_WS, "/v4/ws/usdt"));
    assert_eq!(out.len(), 2, "the unlisted contract sends nothing");
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v["payload"], serde_json::json!(["1w", "BTC_USDT"]), "the Monday week");
    // Recorded: a list of rows, `v` contracts and `a` the quote volume.
    let msg = r#"{"time":1792392998,"time_ms":1792392998649,"channel":"futures.candlesticks","event":"update","result":[{"t":1792368000,"c":"82771.3","h":"82876.5","l":"81972.3","o":"82064.6","a":"1078613366.28917","n":"1w_BTC_USDT","w":false,"v":130738962}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(msg), ts(0)).unwrap();
    assert!(matches!(&evs[0], e if e.corr_id == Some(CorrId(9)) && matches!(&e.event, Event::SubscriptionFail { error } if error.message.contains("ZZZ_NOPE"))));
    let Event::Candle(cd) = &evs[1].event else { panic!("{evs:?}") };
    assert_eq!((cd.volume.to_string(), cd.quote_volume.unwrap().to_string()), ("13073.8962".into(), "1078613366.28917".into()));
    assert_eq!(cd.open_ts.secs(), 1_792_368_000);
}

#[test]
fn the_book_ticker_rides_the_candle_socket_and_counts_contracts_into_coins() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    let row: ContractRow = serde_json::from_str(BTC_CONTRACT).unwrap();
    cache.insert(Gateio::contract_spec(&row).unwrap());
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(1_792_409_783), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(8), instrument: InstrumentId::new("GATEIO_FUTURES", "BTC_USDT"), data_type: DataType::Quote };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"time":1792409783,"id":8,"channel":"futures.book_ticker","event":"subscribe","payload":["BTC_USDT"]}));
    // Recorded from the live futures socket: sizes are numbers, in contracts.
    let ok = r#"{"time":1792409783,"time_ms":1792409783802,"id":8,"conn_id":"49191151d290e67f","channel":"futures.book_ticker","event":"subscribe","payload":["BTC_USDT"],"result":{"status":"success"}}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(ok), ts(0)).unwrap()[0].event, Event::SubscriptionAck));
    let tick = r#"{"time":1792409784,"time_ms":1792409784620,"channel":"futures.book_ticker","event":"update","result":{"t":1792409784615,"u":128337967372,"s":"BTC_USDT","b":"82141","B":39923,"a":"82141.1","A":2458}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(tick), ts(1_792_409_785)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(8)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_409_784_615));
    let Event::Quote(q) = &evs[0].event else { panic!() };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82141".to_string(), "82141.1".to_string()));
    assert_eq!(q.bid_qty.map(|x| x.to_string()), Some("3.9923".to_string()), "contracts of 0.0001 BTC");
    otw_connector_testkit::assert_events_sound(&evs);
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(8) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!((v["channel"].as_str(), v["event"].as_str()), (Some("futures.book_ticker"), Some("unsubscribe")));
}

#[test]
fn a_quiet_socket_is_pinged_on_its_own_channel() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    let mut cache = InstrumentCache::new();
    let row: ContractRow = serde_json::from_str(BTC_CONTRACT).unwrap();
    cache.insert(Gateio::contract_spec(&row).unwrap());
    let c = otw_connector_model::ConnectorId("t".into());
    let at = |t: i64| MapCtx { received_ts: ts(t), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(7), instrument: InstrumentId::new("GATEIO_FUTURES", "BTC_USDT"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    m.encode_batch(&[sub], &mut s, &at(100)).unwrap();
    assert!(m.keepalive(&mut s, &at(110)).is_empty());
    let out = m.keepalive(&mut s, &at(121));
    let [NativeRequest::WsSend { payload, .. }] = &out[..] else { panic!("{out:?}") };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"time":121,"channel":"futures.ping"}));
}

fn account(m: &Gateio, s: &mut Session, kind: AccountKind, cursor: Option<String>, range: Option<TimeRange>) -> HttpRequest {
    let cmd = Command::QueryAccount { req_id: CorrId(20), kind, symbols: vec![], range, cursor, account: None };
    one(&encode_once(m, s, &InstrumentCache::new(), &cmd, ts(NOW)).unwrap()).clone()
}

fn account_chunk(evs: &[otw_connector_model::Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn balances_follow_the_accounts_mode() {
    let m = Gateio::spot().unwrap();
    let cache = InstrumentCache::new();
    // A unified account: its balances net of what it borrowed.
    let mut s = Session::default();
    let h = account(&m, &mut s, AccountKind::Balances, None, None);
    assert!(h.signed && h.path == "/api/v4/unified/unified_mode");
    assert!(decode_once(&m, &mut s, &cache, &http_frame(20, "unifiedMode", 200, r#"{"mode":"multi_currency","settings":{}}"#), ts(NOW)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(20), &mut s);
    assert_eq!(one(&next).path, "/api/v4/unified/accounts");
    let body = r#"{"user_id":1,"balances":{"BTC":{"available":"0.5","freeze":"0.1","borrowed":"0"},"USDT":{"available":"100","freeze":"0","borrowed":"300"},"ETH":{"available":"0","freeze":"0","borrowed":"0"}}}"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "unifiedAccounts", 200, body), ts(NOW)).unwrap());
    let AccountItems::Balances(b) = items else { panic!() };
    assert_eq!(b.iter().map(|b| (b.asset.as_str(), b.total.0.to_string())).collect::<Vec<_>>(), [("BTC", "0.6".to_string())], "a net borrowing is no holding");
    // A key without unified access: the classic spot account is read instead.
    let mut s = Session::default();
    account(&m, &mut s, AccountKind::Balances, None, None);
    assert!(decode_once(&m, &mut s, &cache, &http_frame(20, "unifiedMode", 403, r#"{"label":"FORBIDDEN","message":"Forbidden"}"#), ts(NOW)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(20), &mut s);
    assert_eq!(one(&next).path, "/api/v4/spot/accounts");
    let body = r#"[{"currency":"ETH","available":"1.5","locked":"0.5","update_id":7},{"currency":"DOGE","available":"0","locked":"0"}]"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "spotAccounts", 200, body), ts(NOW)).unwrap());
    let AccountItems::Balances(b) = items else { panic!() };
    assert_eq!(b.iter().map(|b| (b.asset.as_str(), b.total.0.to_string(), b.available.0.to_string())).collect::<Vec<_>>(), [("ETH", "2.0".to_string(), "1.5".to_string())]);
    // A refused key is not papered over.
    let mut s = Session::default();
    account(&m, &mut s, AccountKind::Balances, None, None);
    let e = decode_once(&m, &mut s, &cache, &http_frame(20, "unifiedMode", 401, r#"{"label":"INVALID_KEY","message":"Invalid key provided"}"#), ts(NOW)).unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("Check the key"), "{e:?}");
}

#[test]
fn spot_fills_read_their_fees_in_whatever_paid_them() {
    let m = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let range = TimeRange::new(ts(NOW - 3 * 86_400), ts(NOW));
    let h = account(&m, &mut s, AccountKind::Fills, None, Some(range));
    assert_eq!(h.path, "/api/v4/spot/my_trades");
    assert_eq!((q(&h, "from").as_deref(), q(&h, "to").as_deref(), q(&h, "page").as_deref()), (Some((NOW - 3 * 86_400).to_string().as_str()), Some((NOW - 1).to_string().as_str()), Some("1")));
    let body = json(
        r#"[{"id":"1232893232","create_time":"1792390000","create_time_ms":"1792390000123.456","currency_pair":"BTC_USDT","side":"sell","role":"taker","amount":"0.01","price":"82000","order_id":"12345","fee":"0.82","fee_currency":"USDT","point_fee":"0","gt_fee":"0"},
        {"id":"1232893233","create_time_ms":"1792390001000","currency_pair":"ETH_USDT","side":"buy","amount":"1","price":"2400","order_id":"12346","fee":"0","fee_currency":"ETH","point_fee":"0","gt_fee":"0.05"}]"#,
    );
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "myTrades", 200, &body), ts(NOW)).unwrap());
    assert_eq!(next, None, "three days fit one slice");
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!((f[0].id.as_str(), f[0].ts.millis(), f[0].fee.0.to_string(), f[0].fee_asset.as_deref(), f[0].quote_ccy.as_deref()), ("1232893232", 1_792_390_000_123, "0.82".to_string(), Some("USDT"), Some("USDT")));
    assert!(matches!(f[0].side, Mapped::Known(otw_connector_model::Side::Sell)));
    assert_eq!((f[1].fee.0.to_string(), f[1].fee_asset.as_deref()), ("0.05".to_string(), Some("GT")));
}

#[test]
fn a_busy_slice_is_read_again_narrower_before_anything_is_kept() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let range = TimeRange::new(ts(NOW - 30 * 86_400), ts(NOW));
    let h = account(&m, &mut s, AccountKind::Fills, None, Some(range));
    assert_eq!(h.path, "/api/v4/futures/usdt/my_trades_timerange");
    assert_eq!(q(&h, "offset").as_deref(), Some("0"));
    let row = r#"{"trade_id":"1","create_time":1792390000.5,"contract":"BTC_USDT","order_id":"9","size":-3,"close_size":0,"price":"82000","role":"maker","fee":"-0.01","point_fee":"0"}"#;
    let full = format!("[{}]", vec![row; 1000].join(","));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "myTradesTimerange", 200, &full), ts(NOW)).unwrap());
    assert!(matches!(items, AccountItems::Fills(ref f) if f.is_empty()), "the full page of a week is not kept");
    let h = account(&m, &mut s, AccountKind::Fills, next, Some(range));
    let (a, b): (i64, i64) = (q(&h, "from").unwrap().parse().unwrap(), q(&h, "to").unwrap().parse().unwrap());
    assert_eq!(b - a + 1, 7 * 86_400 / 2);
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "myTradesTimerange", 200, &format!("[{row}]")), ts(NOW)).unwrap());
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!((f[0].qty.0.to_string(), f[0].ts.millis(), f[0].fee.0.to_string()), ("3".to_string(), 1_792_390_000_500, "-0.01".to_string()), "contracts, a maker rebate negative");
    assert!(matches!(f[0].side, Mapped::Known(otw_connector_model::Side::Sell)));
}

#[test]
fn positions_and_orders_count_contracts() {
    let m = Gateio::futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let h = account(&m, &mut s, AccountKind::Positions, None, None);
    assert_eq!((q(&h, "holding").as_deref(), q(&h, "offset").as_deref()), (Some("true"), Some("0")));
    let body = r#"[{"user":1,"contract":"BTC_USDT","size":-25,"leverage":"10","entry_price":"82000","mark_price":"82100.5","unrealised_pnl":"-0.25","mode":"single"},{"contract":"ETH_USDT","size":0,"entry_price":"0"}]"#;
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "positions", 200, body), ts(NOW)).unwrap());
    assert_eq!(next, None);
    let AccountItems::Positions(p) = items else { panic!() };
    assert_eq!(p.iter().map(|p| (p.instrument.native.as_str(), p.qty.0.to_string())).collect::<Vec<_>>(), [("BTC_USDT", "-25".to_string())]);
    account(&m, &mut s, AccountKind::OpenOrders, None, None);
    let body = r#"[{"id":58828142,"contract":"BTC_USDT","size":-10,"left":-4,"price":"90000","tif":"gtc","status":"open","create_time":1792300000.25}]"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "orders", 200, body), ts(NOW)).unwrap());
    let AccountItems::Orders(o) = items else { panic!() };
    assert_eq!((o[0].id.as_str(), o[0].qty.0.to_string(), o[0].filled_qty.0.to_string(), o[0].order_type.as_str()), ("58828142", "10".to_string(), "6".to_string(), "limit gtc"));
    assert!(matches!(o[0].side, Mapped::Known(otw_connector_model::Side::Sell)));
}

#[test]
fn spot_open_orders_page_while_a_pair_fills_one() {
    let m = Gateio::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    account(&m, &mut s, AccountKind::OpenOrders, None, None);
    let order = r#"{"id":"1","currency_pair":"BTC_USDT","side":"buy","type":"limit","amount":"0.1","left":"0.1","filled_amount":"0","price":"70000","create_time_ms":1792300000123}"#;
    let body = format!(r#"[{{"currency_pair":"BTC_USDT","total":100,"orders":[{}]}}]"#, vec![order; 100].join(","));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "openOrders", 200, &body), ts(NOW)).unwrap());
    assert_eq!(next.as_deref(), Some("2"));
    let AccountItems::Orders(o) = items else { panic!() };
    assert_eq!((o.len(), o[0].limit_price.unwrap().to_string(), o[0].quote_ccy.as_deref()), (100, "70000".to_string(), Some("USDT")));
}
