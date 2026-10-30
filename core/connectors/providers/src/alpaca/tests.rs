//! Alpaca, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, AssetClass, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn m() -> Alpaca {
    Alpaca::new(Feed::Iex).unwrap()
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
    assert!(a.capabilities().history.quotes_at.contains(&AssetClass::Equity));
}

#[test]
fn the_market_rides_the_id_and_an_option_must_be_occ() {
    let a = m();
    assert_eq!(a.instrument_for("aapl", Some(AssetClass::Equity)).unwrap(), InstrumentId::new(VENUE, "AAPL"));
    assert_eq!(a.instrument_for("btc/usd", Some(AssetClass::Spot)).unwrap(), InstrumentId::new(VENUE_CRYPTO, "BTC/USD"));
    assert_eq!(a.instrument_for("SPY251219C00650000", Some(AssetClass::Option)).unwrap().venue.0, VENUE_OPTIONS);
    assert!(a.instrument_for("SPY", Some(AssetClass::Option)).is_err());
    assert!(a.instrument_for("ES", Some(AssetClass::Future)).is_err());
    // Alpaca's own notation when no market is named.
    assert_eq!(a.instrument_id("ETH/USD").unwrap().venue.0, VENUE_CRYPTO);
    assert_eq!(a.instrument_id("AAPL240119C00150000").unwrap().venue.0, VENUE_OPTIONS);
}

#[test]
fn bars_go_to_the_market_s_endpoint() {
    let a = m();
    let cache = InstrumentCache::new();
    let cases = [
        (InstrumentId::new(VENUE, "AAPL"), "/v2/stocks/AAPL/bars", None),
        (InstrumentId::new(VENUE_CRYPTO, "BTC/USD"), "/v1beta3/crypto/us/bars", Some("BTC/USD")),
        (InstrumentId::new(VENUE_OPTIONS, "SPY251219C00650000"), "/v1beta1/options/bars", Some("SPY251219C00650000")),
    ];
    for (id, path, symbols) in cases {
        let mut s = Session::default();
        let reqs = encode_once(&a, &mut s, &cache, &hist(id.clone(), Timeframe::H1), ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        assert!(h.signed && h.path == path, "{path}");
        assert_eq!(h.query.iter().find(|(k, _)| k == "symbols").map(|(_, v)| v.as_str()), symbols);
        let tail: Vec<_> = q(h).into_iter().filter(|(k, _)| *k != "symbols").collect();
        assert_eq!(tail, [("timeframe", "1Hour"), ("start", "2024-01-01T00:00:00Z"), ("end", "2024-01-04T00:00:00Z"), ("limit", "10000")]);
        let row = r#"{"t":"2024-01-02T15:00:00Z","o":187.15,"h":188.44,"l":186.9,"c":187.2,"v":12345,"n":321,"vw":187.1}"#;
        let body = match symbols {
            None => format!(r#"{{"bars":[{row}],"symbol":"AAPL","next_page_token":"tok"}}"#),
            Some(sym) => format!(r#"{{"bars":{{"{sym}":[{row}]}},"next_page_token":null}}"#),
        };
        let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "bars", 200, &body), ts(0)).unwrap();
        let c = chunks(&evs);
        let (HistoryItems::Candles(cs), next, _) = &c[0] else { panic!() };
        assert_eq!((cs.len(), cs[0].trades, cs[0].is_final), (1, Some(321), false));
        assert_eq!(next.is_some(), symbols.is_none(), "the page token is the cursor");
    }
}

#[test]
fn quotes_latest_and_at_a_moment() {
    let a = m();
    let cache = InstrumentCache::new();
    let aapl = InstrumentId::new(VENUE, "AAPL");
    let mut s = Session::default();
    let at = Command::RequestQuoteAt { req_id: CorrId(2), instrument: aapl.clone(), at: ts(1_704_205_800), before: true };
    let reqs = encode_once(&a, &mut s, &cache, &at, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.path.as_str(), q(h)), ("/v2/stocks/AAPL/quotes", vec![("end", "2024-01-02T14:30:00Z"), ("sort", "desc"), ("limit", "1")]));
    let evs = decode_once(&a, &mut s, &cache, &http_frame(2, "quote", 200, r#"{"quotes":[{"t":"2024-01-02T14:29:59.9Z","bp":187.1,"bs":2,"ap":187.2,"as":3}],"symbol":"AAPL"}"#), ts(0)).unwrap();
    let Event::Quote(qt) = &evs[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.ask.to_string()), ("187.1".into(), "187.2".into()));
    assert!(evs[0].exchange_ts.is_some());
    // Crypto, latest, nested by symbol.
    let mut s = Session::default();
    let btc = InstrumentId::new(VENUE_CRYPTO, "BTC/USD");
    encode_once(&a, &mut s, &cache, &Command::RequestSnapshot { req_id: CorrId(3), instrument: btc, data_type: DataType::Quote }, ts(0)).unwrap();
    let evs = decode_once(&a, &mut s, &cache, &http_frame(3, "quote", 200, r#"{"quotes":{"BTC/USD":{"bp":30000,"ap":30001,"bs":1,"as":1}}}"#), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::Quote(_)));
    // Options keep no past quotes.
    let opt = Command::RequestQuoteAt { req_id: CorrId(4), instrument: InstrumentId::new(VENUE_OPTIONS, "SPY251219C00650000"), at: ts(0), before: false };
    assert_eq!(encode_once(&a, &mut Session::default(), &cache, &opt, ts(0)).unwrap_err().kind, ErrorKind::Unsupported);
}

#[test]
fn the_asset_list_carries_both_markets() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&a, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(5) }, ts(0)).unwrap();
    assert_eq!(reqs.len(), 2);
    let eq = json(r#"[{"symbol":"AAPL","name":"Apple Inc.","exchange":"NASDAQ","class":"us_equity","tradable":true},{"symbol":"OLD","name":"Old","exchange":"NYSE","class":"us_equity","tradable":false}]"#);
    let evs = decode_once(&a, &mut s, &cache, &http_frame(5, "assetsEquity", 200, &eq), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some(spec.as_ref().clone()), _ => None }).collect();
    assert_eq!((specs[0].id.clone(), specs[0].symbol.0.as_str(), a.search_exchange(&specs[0]), specs[1].active), (InstrumentId::new(VENUE, "AAPL"), "Apple Inc.", "NASDAQ".to_string(), false));
    let cr = json(r#"[{"symbol":"BTC/USD","name":"Bitcoin","exchange":"CRYPTO","class":"crypto","tradable":true}]"#);
    let evs = decode_once(&a, &mut s, &cache, &http_frame(5, "assetsCrypto", 200, &cr), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::Instrument { spec } if spec.asset_class == AssetClass::Spot && spec.quote == "USD"));
    assert!(matches!(evs.last().map(|e| &e.event), Some(Event::InstrumentsEnd)));
}

#[test]
fn a_lookup_asks_the_asset_and_an_option_s_contract() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let ids = vec![InstrumentId::new(VENUE, "AAPL"), InstrumentId::new(VENUE_OPTIONS, "SPY251219C00650000"), InstrumentId::new(VENUE_CRYPTO, "BTC/USDT"), InstrumentId::new(VENUE, "GONE")];
    let reqs = encode_once(&a, &mut s, &cache, &Command::ResolveInstruments { req_id: CorrId(6), ids }, ts(0)).unwrap();
    let paths: Vec<String> = reqs.iter().filter_map(|r| match r { NativeRequest::Http(h) => Some(h.path.clone()), _ => None }).collect();
    assert_eq!(paths, ["/v2/account", "/v2/assets/AAPL", "/v2/assets/SPY251219C00650000", "/v2/options/contracts/SPY251219C00650000", "/v2/assets/BTC%2FUSDT", "/v2/assets/GONE"]);
    let mut specs = Vec::new();
    for (ep, status, body) in [
        ("currency", 200, r#"{"account_number":"1","status":"ACTIVE","currency":"EUR"}"#),
        ("asset0", 200, r#"{"symbol":"AAPL","name":"Apple Inc.","exchange":"NASDAQ","class":"us_equity"}"#),
        ("asset1", 404, r#"{"message":"asset not found"}"#),
        ("contract1", 200, r#"{"symbol":"SPY251219C00650000","name":"SPY Dec 19 2025 650 Call","multiplier":"100"}"#),
        ("asset2", 200, r#"{"symbol":"BTC/USDT","name":"Bitcoin / USD Tether","exchange":"CRYPTO","class":"crypto"}"#),
        ("asset3", 404, r#"{"message":"asset not found"}"#),
    ] {
        for e in decode_once(&a, &mut s, &cache, &http_frame(6, ep, status, body), ts(0)).unwrap() {
            if let Event::Instrument { spec } = e.event {
                specs.push(*spec);
            }
        }
    }
    let got: Vec<_> = specs.iter().map(|s| (s.id.native.as_str(), s.asset_class, s.quote.as_str(), s.multiplier.to_string(), s.exchange.clone())).collect();
    assert_eq!(
        got,
        [
            ("AAPL", AssetClass::Equity, "EUR", "1".to_string(), Some("NASDAQ".to_string())),
            ("SPY251219C00650000", AssetClass::Option, "EUR", "100".to_string(), None),
            ("BTC/USDT", AssetClass::Spot, "USDT", "1".to_string(), Some("CRYPTO".to_string())),
        ]
    );
}

#[test]
fn fills_page_by_the_last_row_id_and_a_short_sale_is_a_sell() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let cmd = |cursor: Option<&str>| Command::QueryAccount { req_id: CorrId(7), kind: AccountKind::Fills, symbols: vec![], range: Some(TimeRange::new(ts(1_704_067_200), ts(1_706_745_600))), cursor: cursor.map(str::to_string), account: None };
    let reqs = encode_once(&a, &mut s, &cache, &cmd(None), ts(0)).unwrap();
    let [NativeRequest::Http(acct), NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(acct.path, "/v2/account", "the account's currency first");
    decode_once(&a, &mut s, &cache, &http_frame(7, "currency", 200, r#"{"currency":"USD"}"#), ts(0)).unwrap();
    assert_eq!((h.path.as_str(), q(h)), ("/v2/account/activities/FILL", vec![("after", "2024-01-01T00:00:00Z"), ("until", "2024-02-01T00:00:00Z"), ("direction", "asc"), ("page_size", "100")]));
    let rows: Vec<String> = (0..100)
        .map(|i| format!(r#"{{"id":"20240102{i:03}::x","activity_type":"FILL","transaction_time":"2024-01-02T15:00:{:02}.123Z","type":"fill","price":"187.5","qty":"2","side":"{}","symbol":"{}","order_id":"o{i}"}}"#, i % 60, if i == 0 { "sell_short" } else { "buy" }, if i == 1 { "BTC/USD" } else { "AAPL" }))
        .collect();
    let evs = decode_once(&a, &mut s, &cache, &http_frame(7, "fills", 200, &format!("[{}]", rows.join(","))), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor, .. } = &evs[0].event else { panic!() };
    assert_eq!(next_cursor.as_deref(), Some("1:20240102099::x"));
    assert_eq!(f[0].side, otw_connector_model::Mapped::Known(otw_connector_model::Side::Sell));
    assert_eq!((f[1].instrument.venue.0.as_str(), f[1].quote_ccy.as_deref()), (VENUE_CRYPTO, Some("USD")));
    assert_eq!((f[2].fee.to_string(), f[2].ts.millis() % 1000), ("0".into(), 123));
    let reqs = encode_once(&a, &mut s, &cache, &cmd(Some("1:20240102099::x")), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.query.last(), Some(&("page_token".to_string(), "20240102099::x".to_string())));
}

#[test]
fn positions_carry_their_mark_and_exchange() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&a, &mut s, &cache, &Command::QueryAccount { req_id: CorrId(8), kind: AccountKind::Positions, symbols: vec![], range: None, cursor: None, account: None }, ts(0)).unwrap();
    decode_once(&a, &mut s, &cache, &http_frame(8, "currency", 503, "down"), ts(0)).unwrap();
    let body = json(r#"[{"symbol":"AAPL","qty":"10","side":"long","avg_entry_price":"150.5","current_price":"187.2","exchange":"NASDAQ","asset_class":"us_equity","unrealized_pl":"367"},{"symbol":"TSLA","qty":"-3","side":"short","avg_entry_price":"240","current_price":"0","exchange":"NASDAQ","asset_class":"us_equity"}]"#);
    let evs = decode_once(&a, &mut s, &cache, &http_frame(8, "positions", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    assert_eq!((p[0].qty.to_string(), p[0].mark_price.map(|x| x.to_string()), p[0].venue.as_deref()), ("10".into(), Some("187.2".into()), Some("NASDAQ")));
    assert_eq!((p[1].qty.to_string(), p[1].mark_price), ("-3".into(), None));
    assert_eq!(p[0].currency.as_deref(), Some("USD"), "the account's currency, USD when it cannot be read");
}

fn ws(p: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("bars".into()), payload: p.to_string().into() }
}

#[test]
fn the_socket_logs_in_before_it_subscribes() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let sub = |id: u64, sym: &str| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(VENUE, sym), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let out = a.encode_batch(&[sub(1, "AAPL"), sub(2, "MSFT")], &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsOpen { path, .. }] if path == "/v2/iex"), "nothing goes out before the login");
    assert!(decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"success","msg":"connected"}]"#), ts(0)).unwrap().is_empty());
    assert!(matches!(&a.keepalive(&mut s, &ctx)[..], [NativeRequest::WsLogin { .. }]));
    decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"success","msg":"authenticated"}]"#), ts(0)).unwrap();
    let sent = a.keepalive(&mut s, &ctx);
    let [NativeRequest::WsSend { payload, .. }] = sent.as_slice() else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"action":"subscribe","bars":["AAPL","MSFT"]}));
    // After the login a pane joins straight away.
    let out = a.encode_batch(&[sub(3, "TSLA")], &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsSend { .. }]));
    let evs = decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"b","S":"MSFT","t":"2026-09-05T14:31:00Z","o":1,"h":2,"l":0.5,"c":1.5,"v":9},{"T":"u","S":"MSFT"},{"T":"b","S":"NVDA","t":"2026-09-05T14:31:00Z","o":1,"h":1,"l":1,"c":1,"v":1}]"#), ts(0)).unwrap();
    assert!(matches!(&evs[..], [e] if e.corr_id == Some(CorrId(2)) && matches!(&e.event, Event::Candle(c) if c.is_final)));
    let e = decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"error","code":409,"msg":"insufficient subscription"}]"#), ts(0)).unwrap_err();
    assert!(e.kind == ErrorKind::Permission && e.message.contains("IEX"));
    assert_eq!(a.socket_class(&InstrumentId::new(VENUE_CRYPTO, "BTC/USD"), &DataType::Candles { tf: Timeframe::M1 }), "crypto");
    let sip = Alpaca::new(Feed::Sip).unwrap();
    let out = sip.encode_batch(&[Command::Subscribe { sub_id: CorrId(9), instrument: InstrumentId::new(VENUE_OPTIONS, "SPY251219C00650000"), data_type: DataType::Candles { tf: Timeframe::M1 } }], &mut Session::default(), &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsOpen { path, .. }] if path == "/v1beta1/opra"));
}

#[test]
fn quotes_wait_for_the_login() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let q = |id: u64, venue: &str, sym: &str| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(venue, sym), data_type: DataType::Quote };
    let bars = Command::Subscribe { sub_id: CorrId(1), instrument: InstrumentId::new(VENUE, "AMD"), data_type: DataType::Candles { tf: Timeframe::M1 } };
    let out = a.encode_batch(&[bars, q(2, VENUE, "AMD")], &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsOpen { path, .. }] if path == "/v2/iex"));
    decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"success","msg":"connected"}]"#), ts(0)).unwrap();
    a.keepalive(&mut s, &ctx);
    decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"success","msg":"authenticated"}]"#), ts(0)).unwrap();
    let sent: Vec<serde_json::Value> = a.keepalive(&mut s, &ctx).iter().map(|r| match r {
        NativeRequest::WsSend { payload, .. } => serde_json::from_slice(payload).unwrap(),
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(sent, vec![serde_json::json!({"action":"subscribe","bars":["AMD"]}), serde_json::json!({"action":"subscribe","quotes":["AMD"]})]);
    // The documented quote.
    let msg = r#"[{"T":"q","S":"AMD","bx":"U","bp":87.66,"bs":1,"ax":"Q","ap":87.68,"as":4,"t":"2021-02-22T15:51:45.335689322Z","c":["R"],"z":"C"}]"#;
    let evs = decode_once(&a, &mut s, &cache, &ws(msg), ts(1_614_009_106)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(2)));
    let Event::Quote(qt) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((qt.bid.to_string(), qt.ask.to_string(), qt.bid_qty), ("87.66".into(), "87.68".into(), None), "round lots are not shares");
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_614_009_105_335));
}

#[test]
fn an_option_socket_is_msgpack_and_folds_its_candles_from_prints() {
    let a = m();
    let opt = InstrumentId::new(VENUE_OPTIONS, "SPXW240327P04925000");
    // No `bars` channel there (410), and JSON is refused (412).
    assert!(a.live_trades_only(&opt));
    assert!(!a.live_trades_only(&InstrumentId::new(VENUE, "AAPL")));
    assert!(a.ws_msgpack("/v1beta1/indicative") && a.ws_msgpack("/v1beta1/opra"));
    assert!(!a.ws_msgpack("/v2/iex") && !a.ws_msgpack("/v1beta3/crypto/us"));
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let trades = Command::Subscribe { sub_id: CorrId(5), instrument: opt.clone(), data_type: DataType::Trades };
    let quote = Command::Subscribe { sub_id: CorrId(6), instrument: opt.clone(), data_type: DataType::Quote };
    let out = a.encode_batch(&[trades, quote], &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [NativeRequest::WsOpen { path, .. }] if path == "/v1beta1/indicative"));
    decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"success","msg":"connected"}]"#), ts(0)).unwrap();
    a.keepalive(&mut s, &ctx);
    decode_once(&a, &mut s, &cache, &ws(r#"[{"T":"success","msg":"authenticated"}]"#), ts(0)).unwrap();
    let sent: Vec<serde_json::Value> = a.keepalive(&mut s, &ctx).iter().map(|r| match r {
        NativeRequest::WsSend { payload, .. } => serde_json::from_slice(payload).unwrap(),
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(sent, vec![serde_json::json!({"action":"subscribe","trades":["SPXW240327P04925000"]}), serde_json::json!({"action":"subscribe","quotes":["SPXW240327P04925000"]})]);
    // The documented messages, as the transport hands them over once transcoded to JSON.
    let trade = r#"[{"T":"t","S":"SPXW240327P04925000","t":"2024-03-12T11:59:38.897261568Z","p":9.5,"s":2,"x":"C","c":"I"}]"#;
    let evs = decode_once(&a, &mut s, &cache, &ws(trade), ts(1_710_244_779)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(5)));
    let Event::Trade(t) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((t.price.to_string(), t.qty.to_string()), ("9.5".into(), "2".into()));
    let quote = r#"[{"T":"q","S":"SPXW240327P04925000","t":"2024-03-12T11:59:38.897261568Z","bx":"C","bp":9.46,"bs":53,"ax":"C","ap":9.66,"as":38,"c":"A"}]"#;
    let evs = decode_once(&a, &mut s, &cache, &ws(quote), ts(1_710_244_779)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(6)));
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string(), q.bid_qty), ("9.46".into(), "9.66".into(), None), "contracts are not carried");
    let out = a.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(5) }], &mut s, &ctx).unwrap();
    let NativeRequest::WsSend { payload, .. } = &out[0] else { panic!() };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(payload).unwrap(), serde_json::json!({"action":"unsubscribe","trades":["SPXW240327P04925000"]}));
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..3) {
        let a = m();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let frame = match which {
            0 => {
                encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE_CRYPTO, "BTC/USD"), Timeframe::M1), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(1), endpoint_id: "bars", status: 200, headers: vec![], body: bytes.into() }
            }
            1 => {
                encode_once(&a, &mut s, &cache, &Command::QueryAccount { req_id: CorrId(8), kind: AccountKind::Positions, symbols: vec![], range: None, cursor: None, account: None }, ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(8), endpoint_id: "positions", status: 200, headers: vec![], body: bytes.into() }
            }
            _ => RawFrame::Ws { conn_key: ConnKey("bars".into()), payload: bytes.into() },
        };
        let _ = decode_once(&a, &mut s, &cache, &frame, ts(0));
    }
}

#[test]
fn a_crypto_window_with_no_bar_is_an_empty_chunk() {
    let a = m();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&a, &mut s, &cache, &hist(InstrumentId::new(VENUE_CRYPTO, "BTC/USD"), Timeframe::H1), ts(0)).unwrap();
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "bars", 200, r#"{"bars":{},"next_page_token":null}"#), ts(0)).unwrap();
    assert!(matches!(&chunks(&evs)[0].0, HistoryItems::Candles(c) if c.is_empty()));
}
