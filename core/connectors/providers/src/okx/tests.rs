//! OKX, offline. Synthetic fixtures written from the documented v5 schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, AssetClass, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn btc() -> InstrumentId {
    InstrumentId::new(VENUE, "BTC-USDT")
}

fn hist(cursor: Option<&str>, tf: Timeframe, from: i64, to: i64) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: btc(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(from), ts(to)), cursor: cursor.map(str::to_string) }
}

fn row(open_ms: i64, confirm: &str) -> String {
    format!(r#"["{open_ms}","100.5","101","99.25","100.75","12.5","1250","1250","{confirm}"]"#)
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    let m = Okx::new().unwrap();
    assert_eq!(m.capabilities().history.candles.as_ref().map(|h| h.max_items_per_call), Some(300));
    assert_undeclared_is_unsupported(&m, &btc());
}

#[test]
fn history_reads_backwards_from_the_window_end_with_utc_days() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &InstrumentCache::new(), &hist(None, Timeframe::D1, 1_700_000_000, 1_710_000_000), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    let q: Vec<(&str, &str)> = h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(q, [("instId", "BTC-USDT"), ("bar", "1Dutc"), ("after", "1710000000000"), ("limit", "300")]);
}

#[test]
fn a_page_is_sorted_filtered_and_continues_from_its_oldest_bar() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let from = 1_699_999_980; // a minute boundary
    let to = from + 400 * 60;
    encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, from, to), ts(0)).unwrap();
    // 300 rows newest first, the newest still forming.
    let rows: Vec<String> = (0..300).map(|i| row((to - 60 - i * 60) * 1000, if i == 0 { "0" } else { "1" })).collect();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "historyCandles", 200, &format!(r#"{{"code":"0","msg":"","data":[{}]}}"#, rows.join(","))), ts(0)).unwrap();
    let c = chunks(&evs);
    let (HistoryItems::Candles(cs), next, done) = &c[0] else { panic!() };
    assert_eq!(cs.len(), 300);
    assert!(cs.windows(2).all(|w| w[0].open_ts < w[1].open_ts), "ascending");
    assert!(!cs.last().unwrap().is_final, "confirm 0 is forming");
    assert_eq!((next.as_deref(), *done), (Some(((to - 300 * 60) * 1000).to_string().as_str()), false));
    assert_history_sound(&[c[0].0.clone()]);
}

#[test]
fn instruments_carry_contract_terms_and_list_order() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(4) }, ts(0)).unwrap();
    assert_eq!(reqs.len(), 3, "spot, swap, futures");
    let swap = json(
        r#"{"code":"0","msg":"","data":[
        {"instId":"BTC-USDT-SWAP","baseCcy":"","quoteCcy":"","settleCcy":"USDT","ctVal":"0.01","ctMult":"1","ctValCcy":"BTC","ctType":"linear","state":"live","tickSz":"0.1","lotSz":"1","minSz":"1","expTime":""},
        {"instId":"BTC-USD-SWAP","baseCcy":"","quoteCcy":"","settleCcy":"BTC","ctVal":"100","ctMult":"1","ctValCcy":"USD","ctType":"inverse","state":"live"},
        {"instId":"OLD-USDT-SWAP","settleCcy":"USDT","ctVal":"1","ctValCcy":"OLD","state":"suspend"}]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "instrumentsSwap", 200, &swap), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some(spec.as_ref().clone()), _ => None }).collect();
    assert_eq!(specs.len(), 3);
    assert_eq!((specs[0].symbol.0.as_str(), specs[0].asset_class, specs[0].multiplier.to_string()), ("BTC/USDT", AssetClass::Perp, "0.01".to_string()));
    assert_eq!(specs[0].settlement_ccy.as_deref(), Some("USDT"));
    assert!(specs[1].inverse && specs[1].settlement_ccy.as_deref() == Some("BTC"));
    assert!(!specs[2].active);
    assert_eq!(m.search_exchange(&specs[0]), "OKX SWAP");
}

#[test]
fn an_envelope_refusal_is_an_error_even_with_status_200() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, 1_700_000_000, 1_700_003_600), ts(0)).unwrap();
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "historyCandles", 200, r#"{"code":"51001","msg":"Instrument ID does not exist","data":[]}"#), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
}

fn fills(cursor: Option<String>) -> Command {
    Command::QueryAccount { req_id: CorrId(7), kind: AccountKind::Fills, symbols: vec![], range: Some(TimeRange::new(ts(1_700_000_000), ts(1_700_086_400))), cursor, account: None }
}

#[test]
fn fills_walk_every_book_and_page_on_the_last_bill_id() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let mut cursor = None;
    let mut asked = Vec::new();
    let mut total = 0;
    for _ in 0..20 {
        let reqs = encode_once(&m, &mut s, &cache, &fills(cursor.clone()), ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        let p = |k: &str| h.query.iter().find(|(q, _)| q == k).map(|(_, v)| v.clone());
        asked.push((p("instType").unwrap(), p("after")));
        // Spot answers a full page then 20 rows; every other book answers nothing.
        let body = if p("instType").as_deref() == Some("SPOT") {
            let (start, n) = if p("after").is_none() { (1, 100) } else { (101, 20) };
            let rows: Vec<String> = (start..start + n)
                .map(|i| format!(r#"{{"instId":"BTC-USDT","tradeId":"{i}","ordId":"o","billId":"{i}","side":"buy","fillSz":"0.5","fillPx":"30000","fee":"-0.1","feeCcy":"usdt","ts":"{}"}}"#, 1_700_000_000_000_i64 + i))
                .collect();
            format!(r#"{{"code":"0","data":[{}]}}"#, rows.join(","))
        } else {
            r#"{"code":"0","data":[]}"#.to_string()
        };
        let evs = decode_once(&m, &mut s, &cache, &http_frame(7, "fillsHistory", 200, &body), ts(0)).unwrap();
        let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor, .. } = &evs[0].event else { panic!() };
        if let Some(x) = f.first() {
            assert_eq!(x.fee.to_string(), "0.1", "a charge is a positive cost");
            assert_eq!(x.quote_ccy.as_deref(), Some("USDT"));
        }
        total += f.len();
        match next_cursor {
            Some(n) => cursor = Some(n.clone()),
            None => break,
        }
    }
    assert_eq!(total, 120);
    let types: Vec<&str> = asked.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(types, ["SPOT", "SPOT", "MARGIN", "SWAP", "FUTURES", "OPTION"]);
    assert_eq!(asked[1].1.as_deref(), Some("100"), "the last row's bill id");
}

#[test]
fn a_pull_that_never_ends_is_stopped_with_a_sentence() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let c = FillCursorProbe::at(500);
    let e = encode_once(&m, &mut s, &InstrumentCache::new(), &fills(Some(c)), ts(0)).unwrap_err();
    assert!(e.message.contains("shorter period"));
}

struct FillCursorProbe;
impl FillCursorProbe {
    fn at(pages: u32) -> String {
        format!("0:9:{pages}")
    }
}

fn sub(id: u64, inst: &str, tf: Timeframe) -> Command {
    Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(VENUE, inst), data_type: DataType::Candles { tf } }
}

fn ws(p: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("business".into()), payload: p.to_string().into() }
}

fn ctx_at<'a>(cache: &'a InstrumentCache, c: &'a otw_connector_model::ConnectorId, secs: i64) -> MapCtx<'a> {
    MapCtx { received_ts: ts(secs), instruments: cache, connector: c }
}

#[test]
fn the_socket_subscribes_routes_dedups_and_keeps_alive() {
    let m = Okx::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let out = m.encode_batch(&[sub(1, "BTC-USDT", Timeframe::M1), sub(2, "ETH-USDT-SWAP", Timeframe::D1)], &mut s, &ctx_at(&cache, &c, 100)).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path == "/ws/v5/business"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"op":"subscribe","args":[{"channel":"candle1m","instId":"BTC-USDT"},{"channel":"candle1Dutc","instId":"ETH-USDT-SWAP"}]}));
    let data = |open: i64, confirm: &str| format!(r#"{{"arg":{{"channel":"candle1m","instId":"BTC-USDT"}},"data":[{}]}}"#, row(open, confirm));
    let evs = decode_once(&m, &mut s, &cache, &ws(&data(1_700_000_040_000, "0")), ts(0)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(1)));
    let evs = decode_once(&m, &mut s, &cache, &ws(&data(1_700_000_100_000, "1")), ts(0)).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!() };
    assert!(cd.is_final);
    assert!(decode_once(&m, &mut s, &cache, &ws(&data(1_700_000_040_000, "1")), ts(0)).unwrap().is_empty(), "an older replayed row is not news");
    assert!(decode_once(&m, &mut s, &cache, &ws("pong"), ts(0)).unwrap().is_empty());
    // Refusals: routed when the sentence names the pane, a socket error otherwise.
    let refused = r#"{"event":"error","msg":"Subscribe failed, wrong URL or channel:candle1Dutc,instId:ETH-USDT-SWAP doesn't exist.","code":"60018"}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(refused), ts(0)).unwrap();
    assert!(matches!(&evs[0].event, Event::SubscriptionFail { error } if error.kind == ErrorKind::NotFound));
    assert_eq!(evs[0].corr_id, Some(CorrId(2)));
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"event":"error","msg":"Wrong URL","code":"60012"}"#), ts(0)).is_err());
    // Keepalive: nothing before 20 quiet seconds, the bare word after.
    assert!(m.keepalive(&mut s, &ctx_at(&cache, &c, 110)).is_empty());
    let ping = m.keepalive(&mut s, &ctx_at(&cache, &c, 121));
    assert!(matches!(&ping[..], [NativeRequest::WsSend { payload, .. }] if &payload[..] == b"ping"));
}

#[test]
fn the_best_bid_and_ask_rides_the_public_socket() {
    let m = Okx::new().unwrap();
    assert_eq!(m.socket_class(&btc(), &DataType::Quote), "public");
    assert_eq!(m.socket_class(&btc(), &DataType::Candles { tf: Timeframe::M1 }), "");
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let q = Command::Subscribe { sub_id: CorrId(5), instrument: btc(), data_type: DataType::Quote };
    let out = m.encode_batch(&[q], &mut s, &ctx_at(&cache, &c, 100)).unwrap();
    assert!(matches!(&out[0], NativeRequest::WsOpen { path, .. } if path == "/ws/v5/public"));
    let NativeRequest::WsSend { payload, .. } = &out[1] else { panic!() };
    let v: serde_json::Value = serde_json::from_slice(payload).unwrap();
    assert_eq!(v, serde_json::json!({"op":"subscribe","args":[{"channel":"bbo-tbt","instId":"BTC-USDT"}]}));
    // Recorded from the live public socket.
    let ack = r#"{"event":"subscribe","arg":{"channel":"bbo-tbt","instId":"BTC-USDT"},"connId":"79890619"}"#;
    assert!(matches!(&decode_once(&m, &mut s, &cache, &ws(ack), ts(0)).unwrap()[0].event, Event::SubscriptionAck));
    let tick = r#"{"arg":{"channel":"bbo-tbt","instId":"BTC-USDT"},"data":[{"asks":[["82245.5","0.34755996","0","8"]],"bids":[["82245.4","0.95365391","0","17"]],"ts":"1792409306102","seqId":82592368691}]}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(tick), ts(1_792_409_306)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(5)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_409_306_102));
    let Event::Quote(q) = &evs[0].event else { panic!() };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82245.4".to_string(), "82245.5".to_string()));
    otw_connector_testkit::assert_events_sound(&evs);
    let refused = r#"{"event":"error","msg":"Subscribe failed, wrong URL or channel:bbo-tbt,instId:BTC-USDT doesn't exist.","code":"60018"}"#;
    assert_eq!(decode_once(&m, &mut s, &cache, &ws(refused), ts(0)).unwrap()[0].corr_id, Some(CorrId(5)));
    let gone = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(5) }], &mut s, &ctx_at(&cache, &c, 101)).unwrap();
    let NativeRequest::WsSend { payload, .. } = &gone[0] else { panic!() };
    assert!(std::str::from_utf8(payload).unwrap().contains(r#""channel":"bbo-tbt""#));
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..3) {
        let m = Okx::new().unwrap();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let frame = match which {
            0 => {
                encode_once(&m, &mut s, &cache, &hist(None, Timeframe::M1, 1_700_000_000, 1_700_003_600), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(1), endpoint_id: "historyCandles", status: 200, headers: vec![], body: bytes.into() }
            }
            1 => {
                encode_once(&m, &mut s, &cache, &fills(None), ts(0)).unwrap();
                RawFrame::Http { corr_id: CorrId(7), endpoint_id: "fillsHistory", status: 200, headers: vec![], body: bytes.into() }
            }
            _ => RawFrame::Ws { conn_key: ConnKey("business".into()), payload: bytes.into() },
        };
        let _ = decode_once(&m, &mut s, &cache, &frame, ts(0));
    }
}
