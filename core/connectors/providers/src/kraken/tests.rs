//! Kraken, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{
    AccountItems, AccountKind, Command, CorrId, DataType, ErrorKind, Event, HistoryItems,
    InstrumentId, TimeRange, Timeframe,
};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, Method, NativeRequest, RawFrame};
use otw_connector_testkit::{
    assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once,
    http_frame, json, ts,
};

use super::*;

fn xbt() -> InstrumentId {
    InstrumentId::new(VENUE, "XBTUSD")
}

fn hist(tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory {
        req_id: CorrId(1),
        instrument: xbt(),
        data_type: DataType::Candles { tf },
        range: TimeRange::new(ts(from), ts(to)),
        cursor: None,
    }
}

const PAIRS: &str = r#"{"error":[],"result":{
    "XXBTZUSD":{"altname":"XBTUSD","wsname":"XBT/USD","base":"XXBT","quote":"ZUSD","lot_decimals":8,"ordermin":"0.00005","tick_size":"0.1","status":"online"},
    "XETHZEUR":{"altname":"ETHEUR","wsname":"ETH/EUR","base":"XETH","quote":"ZEUR","status":"online"},
    "SOLUSD":{"altname":"SOLUSD","wsname":"SOL/USD","base":"SOL","quote":"ZUSD","status":"cancel_only"}}}"#;

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = Kraken::new().unwrap();
    assert_eq!(
        m.capabilities()
            .history
            .candles
            .as_ref()
            .map(|h| h.max_items_per_call),
        Some(720)
    );
    assert!(m.capabilities().account.display_symbols);
    assert_undeclared_is_unsupported(&m, &xbt());
}

#[test]
fn history_is_one_page_filtered_to_the_window_with_the_forming_bar_open() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = 1_700_000_017; // unaligned
    let to = 1_700_003_600 * 2;
    let reqs = encode_once(&m, &mut s, &cache, &hist(Timeframe::H1, from, to), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else {
        panic!()
    };
    let q: Vec<(&str, &str)> = h
        .query
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        q,
        [
            ("pair", "XBTUSD"),
            ("interval", "60"),
            ("since", "1700000017")
        ]
    );
    // Bars every hour from before the window; `last` names the newest committed one.
    let rows: Vec<String> = (0..5)
        .map(|i| {
            let t = 1_699_995_600 + i * 3600;
            format!(r#"[{t},"30000.1","30010.0","29990.5","30005.25","30001.0","1.50000000",{i}]"#)
        })
        .collect();
    let body = format!(
        r#"{{"error":[],"result":{{"XXBTZUSD":[{}],"last":1700006400}}}}"#,
        rows.join(",")
    );
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(1, "ohlc", 200, &body),
        ts(0),
    )
    .unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else {
        panic!()
    };
    assert_eq!((next.as_deref(), *done), (None, true), "one page");
    // 1699995600 and 1699999200 open before `from`: dropped.
    assert_eq!(
        cs.iter().map(|c| c.open_ts.secs()).collect::<Vec<_>>(),
        [1_700_002_800, 1_700_006_400, 1_700_010_000]
    );
    assert_eq!(
        cs.iter().map(|c| c.is_final).collect::<Vec<_>>(),
        [true, true, false],
        "the row after `last` is forming"
    );
    assert_eq!(cs[0].volume.to_string(), "1.50000000");
    assert_eq!(cs[0].trades, Some(2));
    assert_history_sound(&[c[0].0.clone()]);
}

#[test]
fn an_error_array_is_an_error_even_with_status_200() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(
        &m,
        &mut s,
        &cache,
        &hist(Timeframe::M1, 1_700_000_000, 1_700_003_600),
        ts(0),
    )
    .unwrap();
    let e = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(1, "ohlc", 200, r#"{"error":["EQuery:Unknown asset pair"]}"#),
        ts(0),
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    assert!(e.message.contains("bad symbol"));
}

#[test]
fn pairs_are_listed_by_altname_and_named_by_wsname() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(
        &m,
        &mut s,
        &cache,
        &Command::LoadInstruments { req_id: CorrId(4) },
        ts(0),
    )
    .unwrap();
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(4, "assetPairs", 200, &json(PAIRS)),
        ts(0),
    )
    .unwrap();
    let specs: Vec<_> = evs
        .iter()
        .filter_map(|e| match &e.event {
            Event::Instrument { spec } => Some(spec.as_ref().clone()),
            _ => None,
        })
        .collect();
    // In the venue's key order: SOLUSD, XETHZEUR, XXBTZUSD.
    let ids: Vec<&str> = specs.iter().map(|s| s.id.native.as_str()).collect();
    assert_eq!(ids, ["SOLUSD", "ETHEUR", "XBTUSD"]);
    let x = &specs[2];
    assert_eq!(
        (x.symbol.0.as_str(), x.base.as_deref(), x.quote.as_str()),
        ("XBT/USD", Some("XBT"), "USD")
    );
    assert_eq!(
        x.lot_size.map(|q| q.to_string()).as_deref(),
        Some("0.00000001")
    );
    assert!(!specs[0].active, "cancel_only is not open for trading");
    let mut cache = InstrumentCache::new();
    for sp in specs {
        cache.insert(sp);
    }
    assert!(cache.round_trip_failures().is_empty());
}

#[test]
fn the_board_is_keyed_by_wsname_through_the_pair_list() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(
        &m,
        &mut s,
        &cache,
        &Command::RequestBoard { req_id: CorrId(5) },
        ts(100),
    )
    .unwrap();
    assert_eq!(reqs.len(), 2, "the pair list first, then every ticker");
    assert!(decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(5, "pairNames", 200, &json(PAIRS)),
        ts(100)
    )
    .unwrap()
    .is_empty());
    let tickers = json(
        r#"{"error":[],"result":{"XXBTZUSD":{"a":["30001.0","1","1.000"],"b":["30000.0","2","2.000"],"c":["30000.5","0.1"]},"ETHEUR":{"c":["2000.0","1"]},"UNKNOWN":{"c":["1","1"]}}}"#,
    );
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(5, "tickers", 200, &tickers),
        ts(100),
    )
    .unwrap();
    let Event::Board(b) = &evs[0].event else {
        panic!()
    };
    let mut got: Vec<(String, String)> = b
        .iter()
        .map(|p| (p.instrument.native.clone(), p.price.to_string()))
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("ETH/EUR".to_string(), "2000.0".to_string()),
            ("XBT/USD".to_string(), "30000.5".to_string())
        ]
    );
    // Names are kept: a second board within the hour asks for the tickers only.
    assert_eq!(
        encode_once(
            &m,
            &mut s,
            &cache,
            &Command::RequestBoard { req_id: CorrId(6) },
            ts(200)
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(m.pair_symbol("xbt", "usd").as_deref(), Some("XBT/USD"));
}

#[test]
fn a_quote_takes_the_one_row_answered() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = Command::RequestSnapshot {
        req_id: CorrId(2),
        instrument: xbt(),
        data_type: DataType::Quote,
    };
    encode_once(&m, &mut s, &cache, &cmd, ts(0)).unwrap();
    let body = json(
        r#"{"error":[],"result":{"XXBTZUSD":{"a":["30001.0","1","1.500"],"b":["30000.0","2","2.000"],"c":["30000.5","0.1"]}}}"#,
    );
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(2, "ticker", 200, &body),
        ts(0),
    )
    .unwrap();
    let Event::Quote(q) = &evs[0].event else {
        panic!()
    };
    assert_eq!(
        (
            q.bid.to_string(),
            q.ask.to_string(),
            q.ask_qty.map(|x| x.to_string())
        ),
        ("30000.0".into(), "30001.0".into(), Some("1.500".into()))
    );
}

fn fills(cursor: Option<String>) -> Command {
    Command::QueryAccount {
        req_id: CorrId(7),
        kind: AccountKind::Fills,
        symbols: vec![],
        range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_086_400))),
        cursor,
        account: None,
    }
}

#[test]
fn fills_page_by_offset_in_a_signed_post_and_carry_the_wsname() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &fills(None), ts(0)).unwrap();
    let [NativeRequest::Http(names), NativeRequest::Http(h)] = reqs.as_slice() else {
        panic!()
    };
    assert_eq!(names.endpoint_id, "pairNames");
    assert_eq!(
        (h.method, h.path.as_str(), h.signed),
        (Method::Post, "/0/private/TradesHistory", true)
    );
    assert_eq!(
        h.body.as_deref(),
        Some(&b"type=all&trades=false&start=1700000000&end=1700086400&ofs=0"[..])
    );
    decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(7, "pairNames", 200, &json(PAIRS)),
        ts(0),
    )
    .unwrap();
    let rows: Vec<String> = (0..50)
        .map(|i| format!(r#""T{i:03}":{{"ordertxid":"O1","pair":"XXBTZUSD","time":1700000100.{i:04},"type":"buy","ordertype":"limit","price":"30000.0","cost":"15000","fee":"24.0","vol":"0.5"}}"#))
        .collect();
    let body = format!(
        r#"{{"error":[],"result":{{"trades":{{{}}},"count":51}}}}"#,
        rows.join(",")
    );
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(7, "tradesHistory", 200, &body),
        ts(0),
    )
    .unwrap();
    let Event::AccountChunk {
        items: AccountItems::Fills(f),
        next_cursor,
        ..
    } = &evs[0].event
    else {
        panic!()
    };
    assert_eq!(next_cursor.as_deref(), Some("50"));
    assert_eq!(f.len(), 50);
    assert_eq!(f[1].instrument.native, "XBT/USD");
    assert_eq!(
        (
            f[1].fee.to_string(),
            f[1].fee_asset.as_deref(),
            f[1].quote_ccy.as_deref()
        ),
        ("24.0".into(), Some("USD"), Some("USD"))
    );
    assert_eq!(
        f[1].ts.millis(),
        1_700_000_100_000,
        "1700000100.0001 keeps its fraction"
    );
    assert_eq!(f[12].ts.millis(), 1_700_000_100_001);
    // The next page: names are kept, the offset moves.
    let reqs = encode_once(&m, &mut s, &cache, &fills(Some("50".into())), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else {
        panic!()
    };
    assert!(String::from_utf8_lossy(h.body.as_ref().unwrap()).ends_with("ofs=50"));
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(
            7,
            "tradesHistory",
            200,
            r#"{"error":[],"result":{"trades":{},"count":50}}"#,
        ),
        ts(0),
    )
    .unwrap();
    assert!(matches!(
        &evs[0].event,
        Event::AccountChunk {
            next_cursor: None,
            done: true,
            ..
        }
    ));
    // A pull that never ends is stopped with a sentence.
    let e = encode_once(&m, &mut s, &cache, &fills(Some("20000".into())), ts(0)).unwrap_err();
    assert!(e.message.contains("shorter period"));
}

#[test]
fn balances_are_named_by_the_asset_list_never_by_cutting_a_prefix() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let q = Command::QueryAccount {
        req_id: CorrId(8),
        kind: AccountKind::Balances,
        symbols: vec![],
        range: None,
        cursor: None,
        account: None,
    };
    let reqs = encode_once(&m, &mut s, &cache, &q, ts(0)).unwrap();
    assert_eq!(reqs.len(), 2);
    let assets = json(
        r#"{"error":[],"result":{"XXBT":{"altname":"XBT"},"XXLM":{"altname":"XLM"},"ZUSD":{"altname":"USD"}}}"#,
    );
    decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(8, "assets", 200, &assets),
        ts(0),
    )
    .unwrap();
    let bal =
        json(r#"{"error":[],"result":{"XXBT":"0.5","XXLM":"100","ZUSD":"0.0000","ETH2.S":"1.2"}}"#);
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(8, "balance", 200, &bal),
        ts(0),
    )
    .unwrap();
    let Event::AccountChunk {
        items: AccountItems::Balances(b),
        ..
    } = &evs[0].event
    else {
        panic!()
    };
    let names: Vec<&str> = b.iter().map(|b| b.asset.as_str()).collect();
    assert_eq!(
        names,
        ["ETH2.S", "XBT", "XLM"],
        "an unknown code reads as itself, zero is not held"
    );
    // Without the asset list, balances still read.
    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &q, ts(0)).unwrap();
    assert!(decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(8, "assets", 503, "down"),
        ts(0)
    )
    .unwrap()
    .is_empty());
    assert!(decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(8, "balance", 200, &bal),
        ts(0)
    )
    .is_ok());
}

#[test]
fn open_orders_name_their_altname_pair_by_wsname() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let q = Command::QueryAccount {
        req_id: CorrId(9),
        kind: AccountKind::OpenOrders,
        symbols: vec![],
        range: None,
        cursor: None,
        account: None,
    };
    encode_once(&m, &mut s, &cache, &q, ts(0)).unwrap();
    decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(9, "pairNames", 200, &json(PAIRS)),
        ts(0),
    )
    .unwrap();
    let body = json(
        r#"{"error":[],"result":{"open":{
        "OA":{"opentm":1700000000.5,"vol":"1.0","vol_exec":"0.25","descr":{"pair":"XBTUSD","type":"sell","ordertype":"stop-loss","price":"0","price2":"29000.0"}},
        "OB":{"opentm":1700000001,"vol":"2","vol_exec":"0","descr":{"pair":"NEWPAIR","type":"buy","ordertype":"limit","price":"1.5","price2":"0"}}}}}"#,
    );
    let evs = decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(9, "openOrders", 200, &body),
        ts(0),
    )
    .unwrap();
    let Event::AccountChunk {
        items: AccountItems::Orders(o),
        ..
    } = &evs[0].event
    else {
        panic!()
    };
    assert_eq!(
        (o[0].instrument.native.as_str(), o[0].quote_ccy.as_deref()),
        ("XBT/USD", Some("USD"))
    );
    assert_eq!(
        (o[0].limit_price, o[0].stop_price.map(|p| p.to_string())),
        (None, Some("29000.0".into()))
    );
    assert_eq!(o[0].placed_at.map(|t| t.millis()), Some(1_700_000_000_500));
    assert_eq!(
        (o[1].instrument.native.as_str(), o[1].quote_ccy.as_deref()),
        ("NEWPAIR", None)
    );
}

#[test]
fn positions_are_not_a_spot_account_s() {
    let m = Kraken::new().unwrap();
    let q = Command::QueryAccount {
        req_id: CorrId(9),
        kind: AccountKind::Positions,
        symbols: vec![],
        range: None,
        cursor: None,
        account: None,
    };
    assert_eq!(
        encode_once(
            &m,
            &mut Session::default(),
            &InstrumentCache::new(),
            &q,
            ts(0)
        )
        .unwrap_err()
        .kind,
        ErrorKind::Unsupported
    );
}

fn sub(id: u64, inst: &str, tf: Timeframe) -> Command {
    Command::Subscribe {
        sub_id: CorrId(id),
        instrument: InstrumentId::new(VENUE, inst),
        data_type: DataType::Candles { tf },
    }
}

fn ws(p: &str) -> RawFrame {
    RawFrame::Ws {
        conn_key: ConnKey("ohlc".into()),
        payload: p.to_string().into(),
    }
}

fn ctx<'a>(cache: &'a InstrumentCache, c: &'a otw_connector_model::ConnectorId) -> MapCtx<'a> {
    MapCtx {
        received_ts: ts(0),
        instruments: cache,
        connector: c,
    }
}

fn ohlc(symbol: &str, begin: &str, close: &str, interval: u32) -> String {
    format!(
        r#"{{"channel":"ohlc","type":"update","data":[{{"symbol":"{symbol}","open":1.5,"high":2.25,"low":0.5,"close":{close},"trades":3,"volume":3.125,"vwap":1.1,"interval_begin":"{begin}","interval":{interval},"timestamp":"2026-09-05T10:00:30.123456Z"}}]}}"#
    )
}

#[test]
fn the_socket_carries_one_interval_and_closes_on_rollover() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    assert_eq!(
        m.socket_class(&xbt(), &DataType::Candles { tf: Timeframe::H1 }),
        "60"
    );
    let out = m
        .encode_batch(
            &[
                sub(1, "BTC/USD", Timeframe::M1),
                sub(2, "ETH/USD", Timeframe::M1),
            ],
            &mut s,
            &ctx(&cache, &c),
        )
        .unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path == "/v2"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else {
        panic!()
    };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"method":"subscribe","params":{"channel":"ohlc","symbol":["BTC/USD","ETH/USD"],"interval":1}})
    );
    assert!(
        m.encode_batch(
            &[sub(3, "SOL/USD", Timeframe::H1)],
            &mut s,
            &ctx(&cache, &c)
        )
        .is_err(),
        "another interval is another socket"
    );

    // No closed flag: every candle goes out forming, with the venue's time; the engine
    // closes it on the rollover.
    let first = decode_once(
        &m,
        &mut s,
        &cache,
        &ws(&ohlc(
            "BTC/USD",
            "2026-09-05T10:00:00.000000000Z",
            "1.75",
            1,
        )),
        ts(0),
    )
    .unwrap();
    assert!(
        matches!(&first[..], [e] if matches!(&e.event, Event::Candle(c) if !c.is_final) && e.corr_id == Some(CorrId(1)) && e.exchange_ts.is_some())
    );
    let roll = decode_once(
        &m,
        &mut s,
        &cache,
        &ws(&ohlc("BTC/USD", "2026-09-05T10:01:00.000000000Z", "1.8", 1)),
        ts(0),
    )
    .unwrap();
    assert!(
        matches!(&roll[..], [e] if matches!(&e.event, Event::Candle(c) if !c.is_final && c.open_ts.secs() == 1_788_602_460))
    );
    assert!(!m.capabilities().live.candles_final_flag);
    // A row of another interval is not this socket's.
    assert!(decode_once(
        &m,
        &mut s,
        &cache,
        &ws(&ohlc("BTC/USD", "2026-09-05T11:00:00.000000000Z", "1", 60)),
        ts(0)
    )
    .unwrap()
    .is_empty());
    assert!(
        decode_once(&m, &mut s, &cache, &ws(r#"{"channel":"heartbeat"}"#), ts(0))
            .unwrap()
            .is_empty()
    );
    // Refusals: routed when they name a pane's pair, a socket error when they name nothing.
    let refused = r#"{"method":"subscribe","success":false,"error":"Currency pair not supported ETH/USD","symbol":"ETH/USD","time_in":"x","time_out":"y"}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(refused), ts(0)).unwrap();
    assert!(
        matches!(&evs[0].event, Event::SubscriptionFail { error } if error.kind == ErrorKind::NotFound)
    );
    assert_eq!(evs[0].corr_id, Some(CorrId(2)));
    assert!(decode_once(
        &m,
        &mut s,
        &cache,
        &ws(r#"{"method":"subscribe","success":false,"error":"Malformed request"}"#),
        ts(0)
    )
    .is_err());
    let ack = r#"{"method":"subscribe","result":{"channel":"ohlc","symbol":"BTC/USD","interval":1,"snapshot":true},"success":true}"#;
    assert!(matches!(
        &decode_once(&m, &mut s, &cache, &ws(ack), ts(0)).unwrap()[0].event,
        Event::SubscriptionAck
    ));
    // Leaving: one unsubscribe frame on the same interval.
    let out = m
        .encode_batch(
            &[Command::Unsubscribe { sub_id: CorrId(2) }],
            &mut s,
            &ctx(&cache, &c),
        )
        .unwrap();
    let [NativeRequest::WsSend { payload, .. }] = out.as_slice() else {
        panic!()
    };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"method":"unsubscribe","params":{"channel":"ohlc","symbol":["ETH/USD"],"interval":1}})
    );
}

#[test]
fn live_subscription_uses_catalog_wsname_but_keeps_altname_identity() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());

    encode_once(
        &m,
        &mut s,
        &cache,
        &Command::LoadInstruments { req_id: CorrId(4) },
        ts(0),
    )
    .unwrap();
    let pairs = r#"{"error":[],"result":{"SOLETH":{"altname":"SOLETH","wsname":"SOL/ETH","base":"SOL","quote":"XETH","status":"online"}}}"#;
    decode_once(
        &m,
        &mut s,
        &cache,
        &http_frame(4, "assetPairs", 200, pairs),
        ts(0),
    )
    .unwrap();

    let out = m
        .encode_batch(&[sub(7, "SOLETH", Timeframe::M1)], &mut s, &ctx(&cache, &c))
        .unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[1] else {
        panic!()
    };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"method":"subscribe","params":{"channel":"ohlc","symbol":["SOL/ETH"],"interval":1}})
    );

    let ack = r#"{"method":"subscribe","result":{"channel":"ohlc","symbol":"SOL/ETH","interval":1,"snapshot":true},"success":true}"#;
    let events = decode_once(&m, &mut s, &cache, &ws(ack), ts(0)).unwrap();
    assert!(
        matches!(&events[..], [e] if e.corr_id == Some(CorrId(7)) && matches!(&e.event, Event::SubscriptionAck))
    );

    let candle = decode_once(
        &m,
        &mut s,
        &cache,
        &ws(&ohlc(
            "SOL/ETH",
            "2026-09-05T10:00:00.000000000Z",
            "1.75",
            1,
        )),
        ts(0),
    )
    .unwrap();
    assert!(
        matches!(&candle[..], [e] if matches!(&e.event, Event::Candle(c) if c.instrument.native == "SOLETH"))
    );

    let out = m
        .encode_batch(
            &[Command::Unsubscribe { sub_id: CorrId(7) }],
            &mut s,
            &ctx(&cache, &c),
        )
        .unwrap();
    let [NativeRequest::WsSend { payload, .. }] = out.as_slice() else {
        panic!()
    };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"method":"unsubscribe","params":{"channel":"ohlc","symbol":["SOL/ETH"],"interval":1}})
    );
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..4) {
        let m = Kraken::new().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let frame = match which {
            0 => {
                encode_once(&m, &mut s, &cache, &hist(Timeframe::M1, 1_700_000_000, 1_700_003_600), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(1), endpoint_id: "ohlc", status: 200, headers: vec![], body: bytes.into() }
            }
            1 => {
                encode_once(&m, &mut s, &cache, &fills(None), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(7), endpoint_id: "tradesHistory", status: 200, headers: vec![], body: bytes.into() }
            }
            2 => {
                encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(4) }, ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(4), endpoint_id: "assetPairs", status: 200, headers: vec![], body: bytes.into() }
            }
            _ => RawFrame::Ws { conn_key: ConnKey("ohlc".into()), payload: bytes.into() },
        };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}

#[test]
fn the_best_bid_and_ask_is_a_socket_of_its_own() {
    let m = Kraken::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    assert_eq!(m.socket_class(&xbt(), &DataType::Quote), "");
    let q = |id: u64, inst: &str| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(VENUE, inst), data_type: DataType::Quote };
    let out = m.encode_batch(&[q(4, "BTC/USD"), q(5, "ZZZ/NOPE")], &mut s, &ctx(&cache, &c)).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path == "/v2"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"method":"subscribe","params":{"channel":"ticker","symbol":["BTC/USD","ZZZ/NOPE"],"event_trigger":"bbo"}}));
    assert!(m.encode_batch(&[sub(6, "BTC/USD", Timeframe::M1)], &mut s, &ctx(&cache, &c)).is_err(), "candles ride their own interval's socket");
    // Recorded from the live v2 socket.
    let no = r#"{"error":"Currency pair not supported ZZZ/NOPE","method":"subscribe","success":false,"symbol":"ZZZ/NOPE","time_in":"2026-10-19T11:44:00.154714Z","time_out":"2026-10-19T11:44:00.154768Z"}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(no), ts(0)).unwrap()[..], [e] if e.corr_id == Some(CorrId(5)) && matches!(e.event, Event::SubscriptionFail { .. })));
    let snap = r#"{"channel":"ticker","type":"snapshot","data":[{"symbol":"BTC/USD","bid":82184.0,"bid_qty":0.56410902,"ask":82184.1,"ask_qty":0.62676483,"last":82185.5,"volume":1811.69790341,"vwap":81935.5,"low":80567.9,"high":83100.0,"change":269.9,"change_pct":0.33,"trades":123657,"timestamp":"2026-10-19T11:44:00.003176Z"}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(snap), ts(1_792_410_241)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(4)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_410_240_003));
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82184.0".to_string(), "82184.1".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(4) }], &mut s, &ctx(&cache, &c)).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v["params"]["channel"], "ticker");
}
