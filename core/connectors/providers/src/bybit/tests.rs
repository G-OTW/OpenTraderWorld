//! Bybit, offline. Market fixtures are trimmed from answers of the live public endpoints
//! (2026-10-18); account fixtures are written from the documented v5 schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, OptionRight, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn hist(m: &Bybit, tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(m.venue(), "BTCUSDT"),
        data_type: DataType::Candles { tf },
        range: TimeRange::new(ts(from), ts(to)),
        cursor: None,
    }
}

fn query(h: &HttpRequest, k: &str) -> Option<String> {
    h.query.iter().find(|(q, _)| q == k).map(|(_, v)| v.clone())
}

#[test]
fn each_category_declares_only_what_it_serves() {
    for m in [Bybit::spot().unwrap(), Bybit::linear().unwrap(), Bybit::inverse().unwrap(), Bybit::option().unwrap(), Bybit::eu_spot().unwrap()] {
        assert!(m.capabilities().provider.starts_with("bybit"));
        assert_undeclared_is_unsupported(&m, &InstrumentId::new(m.venue(), "BTCUSDT"));
    }
    assert!(Bybit::spot().unwrap().capabilities().account.balances);
    assert!(Bybit::linear().unwrap().capabilities().account.positions);
}

#[test]
fn the_category_and_interval_are_named_on_every_page() {
    for (m, cat) in [(Bybit::spot().unwrap(), "spot"), (Bybit::linear().unwrap(), "linear")] {
        let mut s = Session::default();
        let reqs = encode_once(&m, &mut s, &InstrumentCache::new(), &hist(&m, Timeframe::D1, 1_700_000_000, 1_700_864_000), ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        assert_eq!(h.path, "/v5/market/kline");
        assert_eq!(query(h, "category").as_deref(), Some(cat));
        assert_eq!(query(h, "interval").as_deref(), Some("D"));
        assert_eq!(query(h, "start").as_deref(), Some("1700000000000"));
        assert_eq!(query(h, "end").as_deref(), Some("1700864000000"));
    }
}

#[test]
fn history_reads_newest_first_and_pages_backwards() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // Recorded rows (spot BTCUSDT 1h), newest first as the venue sends them.
    let from = 1_792_342_800;
    encode_once(&m, &mut s, &cache, &hist(&m, Timeframe::H1, from, from + 3 * 3600), ts(0)).unwrap();
    let body = json(
        r#"{"retCode":0,"retMsg":"OK","result":{"category":"spot","symbol":"BTCUSDT","list":[
        ["1792350000000","81194","81194","81142.4","81163.6","41.630194","3379022.321037"],
        ["1792346400000","81190.6","81282.3","81082.7","81194","206.384593","16751422.8754134"],
        ["1792342800000","81077.5","81228.1","81077.5","81190.6","273.99199","22239360.1990811"]]},"retExtInfo":{},"time":1792350863569}"#,
    );
    // "Now" is the recording time: the newest bar is still forming.
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, &body), ts(1_792_350_863)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.len(), 3);
    assert!(cs.windows(2).all(|w| w[0].open_ts < w[1].open_ts), "sorted oldest first");
    assert!(cs[0].is_final && cs[1].is_final);
    assert!(!cs[2].is_final, "the hour that opened at 18:20 is still forming");
    assert_eq!(cs[0].quote_volume.unwrap().to_string(), "22239360.1990811");
    assert!(next.is_none() && *done, "a short page is the last one");
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);

    // A full page whose oldest bar is still after the start asks again below it.
    let mut s = Session::default();
    let start = 1_700_000_000 / 60 * 60;
    encode_once(&m, &mut s, &cache, &hist(&m, Timeframe::M1, start, start + 2000 * 60), ts(0)).unwrap();
    let rows: Vec<String> = (0..1000).rev().map(|i| format!(r#"["{}","1","2","0.5","1.5","10","15"]"#, (start + 1000 * 60 + i * 60) * 1000)).collect();
    let body = format!(r#"{{"retCode":0,"retMsg":"OK","result":{{"list":[{}]}}}}"#, rows.join(","));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, &body), ts(start + 5000 * 60)).unwrap();
    let (_, next, done) = &chunks(&evs)[0];
    assert_eq!(next.as_deref(), Some(((start + 1000 * 60) * 1000 - 1).to_string().as_str()));
    assert!(!done);
}

#[test]
fn a_month_closes_on_the_calendar() {
    // 2026-10-01 opens October, which closes on 2026-11-01; December rolls the year.
    assert_eq!(close_ms(1_790_812_800_000, Timeframe::MO1), Some(1_793_491_200_000));
    assert_eq!(close_ms(1_796_083_200_000, Timeframe::MO1), Some(1_798_761_600_000));
    assert_eq!(close_ms(0, Timeframe::H1), Some(3_600_000));
}

#[test]
fn the_contract_list_is_paged_and_typed() {
    let m = Bybit::linear().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(4) }, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(query(h, "category").as_deref(), Some("linear"));
    // Trimmed from the live list: a USDT perpetual, a USDC perpetual and a dated future.
    let body = json(
        r#"{"retCode":0,"retMsg":"OK","result":{"category":"linear","list":[
        {"symbol":"0GUSDT","contractType":"LinearPerpetual","status":"Trading","baseCoin":"0G","quoteCoin":"USDT","deliveryTime":"0","priceFilter":{"tickSize":"0.0001"},"lotSizeFilter":{"minOrderQty":"0.1","qtyStep":"0.1"},"settleCoin":"USDT"},
        {"symbol":"AAVEPERP","contractType":"LinearPerpetual","status":"Trading","baseCoin":"AAVE","quoteCoin":"USDC","deliveryTime":"0","settleCoin":"USDC"},
        {"symbol":"BTCUSDT-06NOV26","contractType":"LinearFutures","status":"Trading","baseCoin":"BTC","quoteCoin":"USDT","deliveryTime":"1793952000000","settleCoin":"USDT"}],
        "nextPageCursor":"first%3D0GUSDT%26last%3DBTCUSDT-06NOV26"},"retExtInfo":{},"time":1792350864252}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "instruments", 200, &body), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec) } else { None }).collect();
    assert_eq!(specs.len(), 3);
    assert_eq!(specs[0].tick_size.unwrap().to_string(), "0.0001");
    assert_eq!(specs[1].quote, "USDC");
    assert_eq!(specs[2].asset_class, AssetClass::Future);
    assert_eq!(specs[2].expiry.unwrap().millis(), 1_793_952_000_000);
    assert!(!evs.iter().any(|e| matches!(e.event, Event::InstrumentsEnd)), "a cursor means more pages");
    let next = m.follow_ups(CorrId(4), &mut s);
    let [NativeRequest::Http(h)] = next.as_slice() else { panic!() };
    assert_eq!(query(h, "cursor").as_deref(), Some("first%3D0GUSDT%26last%3DBTCUSDT-06NOV26"));
    let last = decode_once(&m, &mut s, &cache, &http_frame(4, "instruments", 200, r#"{"retCode":0,"retMsg":"OK","result":{"list":[],"nextPageCursor":""}}"#), ts(0)).unwrap();
    assert!(matches!(last.last().map(|e| &e.event), Some(Event::InstrumentsEnd)));
    assert!(m.follow_ups(CorrId(4), &mut s).is_empty());
}

#[test]
fn a_symbol_the_venue_does_not_list_is_named() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist(&m, Timeframe::H1, 0, 3600), ts(0)).unwrap();
    // Recorded from the live venue.
    let err = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, r#"{"retCode":10001,"retMsg":"Not supported symbols","result":{},"retExtInfo":{},"time":1792350864714}"#), ts(0)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
    assert!(m.instrument_id("BTC/USDT").is_err());
    assert_eq!(m.instrument_id("btcusdt-06nov26").unwrap().native, "BTCUSDT-06NOV26");
}

#[test]
fn the_socket_subscribes_per_topic_and_reads_the_confirm_flag() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let subs = [
        Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new("BYBIT", "BTCUSDT"), data_type: DataType::Candles { tf: Timeframe::M1 } },
        Command::Subscribe { sub_id: CorrId(2), instrument: InstrumentId::new("BYBIT", "ZZZNOPE"), data_type: DataType::Candles { tf: Timeframe::M1 } },
    ];
    let out = m.encode_batch(&subs, &mut s, &ctx).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path == "/v5/public/spot"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"op":"subscribe","req_id":"kline.1.BTCUSDT","args":["kline.1.BTCUSDT"]}));
    let ws = |p: &str| RawFrame::Ws { conn_key: ConnKey("public".into()), payload: p.to_string().into() };
    // Frames recorded from the live spot socket.
    let ack = decode_once(&m, &mut s, &cache, &ws(r#"{"success":true,"ret_msg":"subscribe","conn_id":"d9avt8sb86tr31f6ej90-imuih","req_id":"kline.1.BTCUSDT","op":"subscribe"}"#), ts(0)).unwrap();
    assert!(matches!(&ack[0].event, Event::SubscriptionAck) && ack[0].corr_id == Some(CorrId(1)));
    let refused = decode_once(&m, &mut s, &cache, &ws(r#"{"success":false,"ret_msg":"Invalid symbol :[kline.1.ZZZNOPE]","conn_id":"d9avt8sb86tr31f6ej90-imuih","req_id":"kline.1.ZZZNOPE","op":"subscribe"}"#), ts(0)).unwrap();
    assert!(matches!(&refused[0].event, Event::SubscriptionFail { .. }) && refused[0].corr_id == Some(CorrId(2)));
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"success":true,"ret_msg":"pong","conn_id":"d9avt8sb86tr31f6ej90-imuih","op":"ping"}"#), ts(0)).unwrap().is_empty());
    let forming = decode_once(&m, &mut s, &cache, &ws(r#"{"type":"snapshot","topic":"kline.1.BTCUSDT","data":[{"start":1792350840000,"end":1792350899999,"interval":"1","open":"81163.4","close":"81163.5","high":"81168.4","low":"81163.4","volume":"1.104455","turnover":"89642.7039616","confirm":false,"timestamp":1792350899562}],"ts":1792350899562}"#), ts(0)).unwrap();
    assert!(matches!(&forming[0].event, Event::Candle(c) if !c.is_final));
    let closed = decode_once(&m, &mut s, &cache, &ws(r#"{"type":"snapshot","topic":"kline.1.BTCUSDT","data":[{"start":1792350840000,"end":1792350899999,"interval":"1","open":"81163.4","close":"81163.5","high":"81168.4","low":"81163.4","volume":"1.104455","turnover":"89642.7039616","confirm":true,"timestamp":1792350900562}],"ts":1792350900562}"#), ts(0)).unwrap();
    assert!(matches!(&closed[0].event, Event::Candle(c) if c.is_final && c.close.to_string() == "81163.5"));
    assert_eq!(closed[0].exchange_ts.map(|t| t.millis()), Some(1_792_350_900_562));
}

#[test]
fn the_linear_socket_is_its_own_path() {
    let m = Bybit::linear().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new("BYBIT_LINEAR", "BTCUSDT"), data_type: DataType::Candles { tf: Timeframe::D1 } };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path == "/v5/public/linear"));
    assert!(matches!(&out[1], NativeRequest::WsSend { payload, .. } if std::str::from_utf8(payload).unwrap().contains("kline.D.BTCUSDT")));
}

#[test]
fn the_level_one_book_is_the_quote() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(4), instrument: InstrumentId::new("BYBIT", "BTCUSDT"), data_type: DataType::Quote };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    assert!(matches!(&out[1], NativeRequest::WsSend { payload, .. } if std::str::from_utf8(payload).unwrap().contains(r#""args":["orderbook.1.BTCUSDT"]"#)));
    let ws = |p: &str| RawFrame::Ws { conn_key: ConnKey("public".into()), payload: p.to_string().into() };
    // Recorded from the live spot socket.
    let snap = r#"{"topic":"orderbook.1.BTCUSDT","ts":1792409229279,"type":"snapshot","data":{"s":"BTCUSDT","b":[["82250.9","1.121701"]],"a":[["82251","0.501505"]],"u":126379082,"seq":115722715034},"cts":1792409229276}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(snap), ts(1_792_409_230)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(4)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_409_229_279));
    let Event::Quote(q) = &evs[0].event else { panic!("{:?}", evs[0].event) };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82250.9".to_string(), "82251".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);
    // A delta naming one side keeps the other; a zero size empties it, and half a book is
    // no quote.
    let delta = r#"{"topic":"orderbook.1.BTCUSDT","ts":1792409229300,"type":"delta","data":{"s":"BTCUSDT","b":[["82250.5","2"]],"a":[],"u":126379083}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(delta), ts(0)).unwrap();
    let Event::Quote(q) = &evs[0].event else { panic!() };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82250.5".to_string(), "82251".to_string()));
    let gone = r#"{"topic":"orderbook.1.BTCUSDT","ts":1792409229310,"type":"delta","data":{"s":"BTCUSDT","b":[],"a":[["82251","0"]],"u":126379084}}"#;
    assert!(decode_once(&m, &mut s, &cache, &ws(gone), ts(0)).unwrap().is_empty());
}

fn fills(cursor: Option<String>, days: i64) -> Command {
    fills_on(cursor, days, None)
}

fn fills_on(cursor: Option<String>, days: i64, site: Option<&str>) -> Command {
    Command::QueryAccount {
        req_id: CorrId(7),
        kind: AccountKind::Fills,
        symbols: vec![],
        range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_000_000 + days * 86_400))),
        cursor,
        account: site.map(str::to_string),
    }
}

#[test]
fn fills_are_walked_a_week_at_a_time_backwards_following_the_page_cursor() {
    let m = Bybit::linear().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let mut cursor = None;
    let mut asked = Vec::new();
    let mut first_page = true;
    loop {
        let reqs = encode_once(&m, &mut s, &cache, &fills(cursor.clone(), 20), ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        assert!(h.signed);
        let p = |k: &str| query(h, k).and_then(|v| v.parse::<i64>().ok()).unwrap();
        asked.push((p("startTime"), p("endTime"), query(h, "cursor")));
        // The first slice has a second page; every other answer is short.
        let body = if first_page { r#"{"retCode":0,"result":{"list":[],"nextPageCursor":"page2"}}"# } else { r#"{"retCode":0,"result":{"list":[],"nextPageCursor":""}}"# };
        first_page = false;
        let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "executionList", 200, body), ts(0)).unwrap();
        let Event::AccountChunk { next_cursor, .. } = &evs[0].event else { panic!() };
        match next_cursor {
            Some(n) => cursor = Some(n.clone()),
            None => break,
        }
    }
    assert_eq!(asked.len(), 4, "{asked:?}");
    assert_eq!(asked[0].1, (1_700_000_000 + 20 * 86_400) * 1000, "from the period's end");
    assert_eq!((asked[1].0, asked[1].1, asked[1].2.as_deref()), (asked[0].0, asked[0].1, Some("page2")), "same slice, next page");
    assert_eq!(asked[3].0, 1_700_000_000 * 1000, "down to its start");
    assert_eq!(asked[1].0, asked[2].1, "slices tile the period");
}

#[test]
fn a_fill_keeps_its_fee_sign_and_funding_is_not_a_fill() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &fills(None, 3), ts(0)).unwrap();
    let body = json(
        r#"{"retCode":0,"retMsg":"OK","result":{"nextPageCursor":"","category":"spot","list":[
        {"symbol":"BTCUSDT","orderId":"o1","execId":"e1","side":"Buy","execPrice":"81000.5","execQty":"0.01","execFee":"0.00001","feeCurrency":"BTC","execType":"Trade","execTime":"1700000100000"},
        {"symbol":"BTCUSDT","orderId":"o2","execId":"e2","side":"Sell","execPrice":"81010","execQty":"0.01","execFee":"-0.0162","feeCurrency":"USDT","execType":"Trade","execTime":"1700000200000"},
        {"symbol":"BTCUSDT","orderId":"","execId":"e3","side":"Sell","execPrice":"81010","execQty":"0.01","execFee":"0.5","execType":"Funding","execTime":"1700000300000"}]}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "executionList", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!() };
    assert_eq!(f.len(), 2);
    assert_eq!((f[0].fee.to_string().as_str(), f[0].fee_asset.as_deref()), ("0.00001", Some("BTC")));
    assert_eq!(f[1].fee.to_string(), "-0.0162", "a maker rebate stays a negative cost");
}

#[test]
fn positions_walk_both_settle_coins() {
    let m = Bybit::linear().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let q = |cursor: Option<String>| Command::QueryAccount { req_id: CorrId(8), kind: AccountKind::Positions, symbols: vec![], range: None, cursor, account: None };
    let reqs = encode_once(&m, &mut s, &cache, &q(None), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(query(h, "settleCoin").as_deref(), Some("USDT"));
    let body = json(
        r#"{"retCode":0,"result":{"nextPageCursor":"","list":[
        {"symbol":"BTCUSDT","side":"Sell","size":"0.5","avgPrice":"80000","markPrice":"81000","unrealisedPnl":"-500"},
        {"symbol":"ETHUSDT","side":"","size":"0","avgPrice":"0","markPrice":"3000","unrealisedPnl":"0"}]}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(8, "positionList", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].qty.to_string(), "-0.5");
    assert_eq!(p[0].currency.as_deref(), Some("USDT"));
    let reqs = encode_once(&m, &mut s, &cache, &q(next_cursor.clone()), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(query(h, "settleCoin").as_deref(), Some("USDC"));
    let evs = decode_once(&m, &mut s, &cache, &http_frame(8, "positionList", 200, r#"{"retCode":0,"result":{"nextPageCursor":"","list":[]}}"#), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::AccountChunk { done: true, .. }));
}

#[test]
fn balances_count_what_open_orders_lock() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let q = Command::QueryAccount { req_id: CorrId(9), kind: AccountKind::Balances, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &q, ts(0)).unwrap();
    let body = json(
        r#"{"retCode":0,"result":{"list":[{"accountType":"UNIFIED","totalEquity":"1234.5","coin":[
        {"coin":"BTC","walletBalance":"0.1","locked":"0.02"},{"coin":"USDT","walletBalance":"0","locked":"0"}]}]}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "walletBalance", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Balances(b), .. } = &evs[0].event else { panic!() };
    assert_eq!(b.len(), 1);
    assert_eq!((b[0].total.to_string().as_str(), b[0].available.to_string().as_str()), ("0.1", "0.08"));
}

fn account(kind: AccountKind, cursor: Option<String>, site: Option<&str>) -> Command {
    Command::QueryAccount { req_id: CorrId(9), kind, symbols: vec![], range: None, cursor, account: site.map(str::to_string) }
}

fn one_http(reqs: &[NativeRequest]) -> &HttpRequest {
    let [NativeRequest::Http(h)] = reqs else { panic!("{reqs:?}") };
    h
}

#[test]
fn a_key_is_read_on_the_site_that_issued_it() {
    let cache = InstrumentCache::new();
    for (site, base) in [(None, "rest"), (Some("global"), "rest"), (Some("tr"), "tr"), (Some("JP"), "jp"), (Some("testnet"), "testnet")] {
        let m = Bybit::spot().unwrap();
        let mut s = Session::default();
        let reqs = encode_once(&m, &mut s, &cache, &fills_on(None, 3, site), ts(0)).unwrap();
        assert_eq!(one_http(&reqs).base, base, "{site:?}");
    }
    let m = Bybit::spot().unwrap();
    let e = encode_once(&m, &mut Session::default(), &cache, &fills_on(None, 3, Some("eu")), ts(0)).unwrap_err();
    assert!(e.message.contains("Bybit EU account"), "{}", e.message);
    assert!(encode_once(&m, &mut Session::default(), &cache, &fills_on(None, 3, Some("mars")), ts(0)).is_err());
    let eu = Bybit::eu_spot().unwrap();
    assert_eq!(one_http(&encode_once(&eu, &mut Session::default(), &cache, &fills_on(None, 3, None), ts(0)).unwrap()).base, "eu");
    assert!(encode_once(&eu, &mut Session::default(), &cache, &fills_on(None, 3, Some("tr")), ts(0)).is_err(), "an EU book never reads a bybit.com key");
    let settings: std::collections::HashMap<String, String> = [("site".to_string(), "eu".to_string())].into();
    assert!(m.check_settings(&settings).is_err() && eu.check_settings(&settings).is_ok());
}

#[test]
fn a_refused_key_names_the_host_it_was_sent_to() {
    let m = Bybit::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &fills_on(None, 3, Some("kz")), ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(7, "executionList", 401, ""), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Auth);
    assert!(e.message.contains("https://api.bybit.kz"), "{}", e.message);
}

#[test]
fn the_summary_reads_the_key_then_the_wallet() {
    let m = Bybit::linear().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &account(AccountKind::Summary, None, None), ts(0)).unwrap();
    assert_eq!(one_http(&reqs).path, "/v5/user/query-api");
    let key = json(r#"{"retCode":0,"retMsg":"","result":{"readOnly":1,"uta":1,"permissions":{"ContractTrade":[],"Spot":[],"Wallet":["AccountTransfer"]}}}"#);
    assert!(decode_once(&m, &mut s, &cache, &http_frame(9, "queryApi", 200, &key), ts(0)).unwrap().is_empty());
    let next = m.follow_ups(CorrId(9), &mut s);
    assert_eq!(one_http(&next).path, "/v5/account/wallet-balance");
    let wallet = json(r#"{"retCode":0,"result":{"list":[{"accountType":"UNIFIED","totalEquity":"1234.567","coin":[{"coin":"BTC","walletBalance":"0.1"}]}]}}"#);
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "walletBalance", 200, &wallet), ts(0)).unwrap();
    let Event::AccountSummary(sum) = &evs[0].event else { panic!() };
    assert_eq!(sum.can_trade, Some(false));
    assert_eq!(sum.permissions, vec!["Wallet: AccountTransfer".to_string()]);
    assert!(sum.detail.as_deref().unwrap().contains("1234.57 USD"));

    // A classic account is refused by name.
    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &account(AccountKind::Summary, None, None), ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(9, "queryApi", 200, r#"{"retCode":0,"result":{"readOnly":1,"uta":0}}"#), ts(0)).unwrap_err();
    assert!(e.message.contains("classic"), "{}", e.message);
}

#[test]
fn an_inverse_candle_swaps_volume_and_turnover() {
    let m = Bybit::inverse().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = 1_792_350_000;
    let mut cmd = hist(&m, Timeframe::H1, from, from + 3600);
    if let Command::RequestHistory { instrument, .. } = &mut cmd {
        *instrument = InstrumentId::new("BYBIT_INVERSE", "BTCUSD");
    }
    let reqs = encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
    assert_eq!(query(one_http(&reqs), "category").as_deref(), Some("inverse"));
    // Recorded from the live venue: volume in one-USD contracts, turnover in BTC.
    let body = r#"{"retCode":0,"retMsg":"OK","result":{"symbol":"BTCUSD","category":"inverse","list":[["1792350000000","81059.1","81059.1","80679","80716","3000332","37.11319016"]]},"retExtInfo":{},"time":1792352709437}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, body), ts(1_792_360_000)).unwrap();
    let (HistoryItems::Candles(cs), ..) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs[0].volume.to_string(), "37.11319016");
    assert_eq!(cs[0].quote_volume.unwrap().to_string(), "3000332");
}

#[test]
fn inverse_positions_and_orders_are_counted_in_coins_and_fills_are_refused() {
    let m = Bybit::inverse().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &cache, &account(AccountKind::Positions, None, None), ts(0)).unwrap();
    assert!(query(one_http(&reqs), "settleCoin").is_none(), "inverse answers every position without a coin");
    let body = r#"{"retCode":0,"result":{"nextPageCursor":"","list":[{"symbol":"BTCUSD","side":"Sell","size":"8000","avgPrice":"80000","markPrice":"81000","unrealisedPnl":"-0.0012"}]}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "positionList", 200, body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    assert_eq!(p[0].qty.to_string(), "-0.1", "8000 USD at 80000 is 0.1 BTC, short");
    assert_eq!(p[0].currency.as_deref(), Some("USD"));
    assert!(p[0].unrealized_pnl.is_none(), "the profit is in BTC, not in the position's currency");

    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &account(AccountKind::OpenOrders, None, None), ts(0)).unwrap();
    let body = r#"{"retCode":0,"result":{"nextPageCursor":"","list":[{"symbol":"BTCUSD","orderId":"o1","side":"Buy","orderType":"Limit","qty":"1000","cumExecQty":"500","price":"50000","triggerPrice":"0","createdTime":"1700000000000"}]}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "orderRealtime", 200, body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].qty.to_string().as_str(), o[0].filled_qty.to_string().as_str()), ("0.02", "0.01"));

    let e = encode_once(&m, &mut Session::default(), &cache, &fills(None, 3), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    assert!(e.message.contains("1/entry"), "{}", e.message);
}

#[test]
fn the_option_list_is_typed_and_has_no_candles() {
    let m = Bybit::option().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(4) }, ts(0)).unwrap();
    assert_eq!(query(one_http(&reqs), "baseCoin").as_deref(), Some("All"));
    // Trimmed from the live list.
    let body = r#"{"retCode":0,"result":{"category":"option","nextPageCursor":"","list":[{"symbol":"ETH-24SEP27-7000-P-USDT","status":"Trading","baseCoin":"ETH","quoteCoin":"USDT","settleCoin":"USDT","optionsType":"Put","launchTime":"1791446400000","deliveryTime":"1821772800000","priceFilter":{"tickSize":"0.1"},"lotSizeFilter":{"minOrderQty":"0.1","qtyStep":"0.1"}}]}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "instruments", 200, body), ts(0)).unwrap();
    let Event::Instrument { spec } = &evs[0].event else { panic!() };
    assert_eq!((spec.asset_class, spec.right, spec.expiry.unwrap().millis()), (AssetClass::Option, Some(OptionRight::Put), 1_821_772_800_000));
    let e = encode_once(&m, &mut Session::default(), &cache, &hist(&m, Timeframe::H1, 0, 3600), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
}

#[test]
fn option_fills_are_walked_per_base_coin() {
    let m = Bybit::option().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let mut cursor = None;
    let mut asked = Vec::new();
    let mut fills_seen = Vec::new();
    loop {
        let reqs = encode_once(&m, &mut s, &cache, &fills(cursor.clone(), 10), ts(0)).unwrap();
        let h = one_http(&reqs).clone();
        let body = if h.path == "/v5/market/instruments-info" {
            assert!(!h.signed);
            if query(&h, "cursor").is_none() {
                r#"{"retCode":0,"result":{"list":[{"baseCoin":"BTC"},{"baseCoin":"ETH"},{"baseCoin":"BTC"}],"nextPageCursor":"p2"}}"#.to_string()
            } else {
                r#"{"retCode":0,"result":{"list":[{"baseCoin":"NVDA"}],"nextPageCursor":""}}"#.to_string()
            }
        } else {
            assert!(h.signed);
            let coin = query(&h, "baseCoin").unwrap();
            let end = query(&h, "endTime").unwrap();
            format!(r#"{{"retCode":0,"result":{{"nextPageCursor":"","list":[{{"symbol":"{coin}-X-C","orderId":"o","execId":"{coin}{end}","side":"Buy","execPrice":"10","execQty":"1","execFee":"0.1","feeCurrency":"USDT","execType":"Trade","execTime":"{end}"}}]}}}}"#)
        };
        asked.push((h.path.clone(), query(&h, "baseCoin"), query(&h, "endTime")));
        let evs = decode_once(&m, &mut s, &cache, &http_frame(7, h.endpoint_id, 200, &body), ts(0)).unwrap();
        let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor, .. } = &evs[0].event else { panic!() };
        fills_seen.extend(f.iter().map(|f| (f.instrument.native.clone(), f.class)));
        match next_cursor {
            Some(n) => cursor = Some(n.clone()),
            None => break,
        }
    }
    let per_coin = |c: &str| asked.iter().filter(|(_, b, _)| b.as_deref() == Some(c)).count();
    // Two list pages, then ten days in two weekly slices for each of three coins.
    assert_eq!(asked.len(), 2 + 3 * 2, "{asked:?}");
    assert_eq!((per_coin("BTC"), per_coin("ETH"), per_coin("NVDA")), (2, 2, 2));
    assert!(fills_seen.iter().all(|(_, c)| *c == Some(AssetClass::Option)));
}

#[test]
fn the_eu_book_is_its_own_venue_on_its_own_hosts() {
    let m = Bybit::eu_spot().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &cache, &hist(&m, Timeframe::H1, 0, 3600), ts(0)).unwrap();
    assert_eq!((one_http(&reqs).base, m.venue()), ("eu", "BYBIT_EU"));
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new("BYBIT_EU", "BTCUSDT"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { base, path, .. } if *base == "ws-eu" && path == "/v5/public/spot"));
}

#[test]
fn a_minute_is_final_ten_seconds_after_its_close() {
    let m = Bybit::linear().unwrap();
    let cache = InstrumentCache::new();
    let open = 1_792_350_000;
    let body = format!(r#"{{"retCode":0,"result":{{"list":[["{}","1","2","0.5","1.5","10","15"]]}}}}"#, open * 1000);
    let at = |secs_after_close: i64| {
        let mut s = Session::default();
        encode_once(&m, &mut s, &cache, &hist(&m, Timeframe::M1, open, open + 60), ts(0)).unwrap();
        let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "kline", 200, &body), ts(open + 60 + secs_after_close)).unwrap();
        let (HistoryItems::Candles(cs), ..) = &chunks(&evs)[0] else { panic!() };
        cs[0].is_final
    };
    // Bybit was seen rewriting a closed minute up to 2.2 s after its close.
    assert!(!at(3));
    assert!(at(11));
}
