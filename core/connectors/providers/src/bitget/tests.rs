//! Bitget, offline. Synthetic fixtures written from the documented v2 schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, Mapper, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn hist(m: &Bitget, tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(m.book().venue(), "BTCUSDT"),
        data_type: DataType::Candles { tf },
        range: TimeRange::new(ts(from), ts(to)),
        cursor: None,
    }
}

#[test]
fn each_book_declares_only_what_it_serves() {
    for m in [Bitget::spot().unwrap(), Bitget::usdt_futures().unwrap()] {
        assert_eq!(m.capabilities().provider, "bitget");
        assert_undeclared_is_unsupported(&m, &InstrumentId::new(m.book().venue(), "BTCUSDT"));
    }
    assert!(Bitget::spot().unwrap().capabilities().account.balances);
    assert!(Bitget::usdt_futures().unwrap().capabilities().account.positions);
}

#[test]
fn the_books_spell_their_granularities_and_paths_differently() {
    let q = |m: &Bitget| {
        let mut s = Session::default();
        let reqs = encode_once(m, &mut s, &InstrumentCache::new(), &hist(m, Timeframe::H1, 1_700_000_000, 1_700_360_000), ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        (h.path.clone(), h.query.clone())
    };
    let (p, sq) = q(&Bitget::spot().unwrap());
    assert_eq!(p, "/api/v2/spot/market/history-candles");
    assert!(sq.contains(&("granularity".into(), "1h".into())));
    let (p, fq) = q(&Bitget::usdt_futures().unwrap());
    assert_eq!(p, "/api/v2/mix/market/history-candles");
    assert!(fq.contains(&("granularity".into(), "1H".into())) && fq.contains(&("productType".into(), "USDT-FUTURES".into())));
}

#[test]
fn a_bar_is_final_only_once_it_has_settled() {
    let m = Bitget::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = 1_699_999_980;
    encode_once(&m, &mut s, &cache, &hist(&m, Timeframe::M1, from, from + 600), ts(0)).unwrap();
    let rows: Vec<String> = (0..10).map(|i| format!(r#"["{}","1","2","0.5","1.5","10","15"]"#, (from + i * 60) * 1000)).collect();
    // "Now" is 30 s after the last bar closed: inside the settle window.
    let now = from + 600 + 30;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "historyCandles", 200, &format!(r#"{{"code":"00000","msg":"success","data":[{}]}}"#, rows.join(","))), ts(now)).unwrap();
    let (HistoryItems::Candles(cs), ..) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.len(), 10);
    assert!(!cs[9].is_final, "closed 30 s ago: Bitget may still rewrite it");
    assert!(cs[7].is_final);
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);
}

#[test]
fn the_socket_closes_a_candle_on_rollover() {
    let m = Bitget::spot().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new("BITGET", "BTCUSDT"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"op":"subscribe","args":[{"instType":"SPOT","channel":"candle1m","instId":"BTCUSDT"}]}));
    let row = |open: i64, close: &str| {
        RawFrame::Ws {
            conn_key: ConnKey("public".into()),
            payload: format!(r#"{{"action":"update","arg":{{"instType":"SPOT","channel":"candle1m","instId":"BTCUSDT"}},"data":[["{open}","1","2","0.5","{close}","10","900","900"]],"ts":"1790014542099"}}"#).into(),
        }
    };
    let a = decode_once(&m, &mut s, &cache, &row(1_790_014_500_000, "10"), ts(0)).unwrap();
    assert_eq!(a.len(), 1);
    let b = decode_once(&m, &mut s, &cache, &row(1_790_014_560_000, "11"), ts(0)).unwrap();
    assert_eq!(b.len(), 2, "the previous candle closes, the new one forms");
    assert!(matches!(&b[0].event, Event::Candle(c) if c.is_final && c.close.to_string() == "10"));
    assert!(decode_once(&m, &mut s, &cache, &row(1_790_014_500_000, "9"), ts(0)).unwrap().is_empty(), "an older row is history");
    let refused = RawFrame::Ws {
        conn_key: ConnKey("public".into()),
        payload: r#"{"event":"error","arg":{"instType":"SPOT","channel":"candle1m","instId":"BTCUSDT"},"code":30001,"msg":"instId:BTCUSDT doesn't exist"}"#.to_string().into(),
    };
    assert!(matches!(&decode_once(&m, &mut s, &cache, &refused, ts(0)).unwrap()[0].event, Event::SubscriptionFail { .. }));
}

#[test]
fn the_one_level_book_is_the_quote_on_the_same_socket() {
    let m = Bitget::usdt_futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let inst = InstrumentId::new(m.book().venue(), "BTCUSDT");
    assert_eq!(m.socket_class(&inst, &DataType::Quote), m.socket_class(&inst, &DataType::Candles { tf: Timeframe::M1 }));
    let sub = Command::Subscribe { sub_id: CorrId(9), instrument: inst, data_type: DataType::Quote };
    let out = m.encode_batch(&[sub], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"op":"subscribe","args":[{"instType":"USDT-FUTURES","channel":"books1","instId":"BTCUSDT"}]}));
    // Recorded from the live public socket.
    let ws = |p: &str| RawFrame::Ws { conn_key: ConnKey("public".into()), payload: p.to_string().into() };
    let tick = r#"{"action":"snapshot","arg":{"instType":"USDT-FUTURES","channel":"books1","instId":"BTCUSDT"},"data":[{"asks":[["82133.1","7.5378"]],"bids":[["82133","2.7873"]],"ts":"1792409709053","seq":1121125465261,"pseq":0}],"ts":1792409709054}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(tick), ts(1_792_409_710)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(9)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_409_709_053));
    let Event::Quote(q) = &evs[0].event else { panic!() };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82133".to_string(), "82133.1".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);
    let refused = r#"{"event":"error","arg":{"instType":"USDT-FUTURES","channel":"books1","instId":"BTCUSDT"},"code":30001,"msg":"instType:USDT-FUTURES,channel:books1,instId:BTCUSDT,precision:null doesn't exist","op":"subscribe"}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(refused), ts(0)).unwrap()[0].event, Event::SubscriptionFail { .. }));
}

fn fills(cursor: Option<String>, days: i64) -> Command {
    Command::QueryAccount {
        req_id: CorrId(7),
        kind: AccountKind::Fills,
        symbols: vec![],
        range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_000_000 + days * 86_400))),
        cursor,
        account: None,
    }
}

#[test]
fn futures_fills_are_walked_a_week_at_a_time_backwards() {
    let m = Bitget::usdt_futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let mut cursor = None;
    let mut windows = Vec::new();
    loop {
        let reqs = encode_once(&m, &mut s, &cache, &fills(cursor.clone(), 20), ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        let p = |k: &str| h.query.iter().find(|(q, _)| q == k).and_then(|(_, v)| v.parse::<i64>().ok()).unwrap();
        windows.push((p("startTime"), p("endTime")));
        let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "fillHistory", 200, r#"{"code":"00000","data":{"fillList":[],"endId":null}}"#), ts(0)).unwrap();
        let Event::AccountChunk { next_cursor, .. } = &evs[0].event else { panic!() };
        match next_cursor {
            Some(n) => cursor = Some(n.clone()),
            None => break,
        }
    }
    assert_eq!(windows.len(), 3, "{windows:?}");
    assert_eq!(windows[0].1, (1_700_000_000 + 20 * 86_400) * 1000, "from the period's end");
    assert_eq!(windows[2].0, 1_700_000_000 * 1000, "down to its start");
    for w in windows.windows(2) {
        assert_eq!(w[0].0, w[1].1, "slices tile the period");
    }
}

#[test]
fn a_futures_fill_adds_up_its_fee_lines_as_a_cost() {
    let m = Bitget::usdt_futures().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &fills(None, 3), ts(0)).unwrap();
    let body = json(
        r#"{"code":"00000","data":{"fillList":[{"tradeId":"x1","symbol":"ETHUSDT","marginCoin":"USDT","orderId":"o1","price":"1801.33","baseVolume":"0.02","side":"sell","cTime":"1698730804882",
        "feeDetail":[{"feeCoin":"USDT","totalFee":"-0.02161596"},{"feeCoin":"USDT","totalFee":"-0.001"}]}],"endId":"x1"}}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "fillHistory", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!() };
    assert_eq!(f[0].fee.to_string(), "0.02261596");
    assert_eq!(f[0].quote_ccy.as_deref(), Some("USDT"));
}
