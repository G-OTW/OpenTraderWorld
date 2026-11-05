//! Binance USDⓈ-M, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, AssetClass, Command, CorrId, DataType, ErrorKind, Event, InstrumentId, TimeRange, Timeframe};
use otw_connector_sdk::{InstrumentCache, Mapper, NativeRequest};
use otw_connector_testkit::{assert_undeclared_is_unsupported, decode_once, encode_once, http_frame, json, ts};

use super::*;

fn btc() -> InstrumentId {
    InstrumentId::new(market::USDM.venue, "BTCUSDT")
}

#[test]
fn capabilities_and_paths_are_the_futures_ones() {
    let m = Binance::usdm().unwrap();
    assert_eq!(m.capabilities().provider, "binance_futures");
    let mut s = Session::default();
    let cmd = Command::RequestHistory { req_id: CorrId(1), instrument: btc(), data_type: DataType::Candles { tf: Timeframe::H1 }, range: TimeRange::new(ts(0), ts(3_600_000)), cursor: None };
    let reqs = encode_once(&m, &mut s, &InstrumentCache::new(), &cmd, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.path.as_str(), h.weight), ("/fapi/v1/klines", 10));
    assert!(h.query.contains(&("limit".into(), "1500".into())));
    assert_undeclared_is_unsupported(&m, &btc());
}

#[test]
fn contracts_carry_their_type_and_expiry() {
    let m = Binance::usdm().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(2) }, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert!(h.query.is_empty(), "no spot permission filter on futures");
    let body = json(
        r#"{"symbols":[
        {"symbol":"BTCUSDT","status":"TRADING","baseAsset":"BTC","quoteAsset":"USDT","marginAsset":"USDT","contractType":"PERPETUAL","deliveryDate":4133404800000,"filters":[{"filterType":"PRICE_FILTER","tickSize":"0.10"}]},
        {"symbol":"ETHUSDT_250926","status":"TRADING","baseAsset":"ETH","quoteAsset":"USDT","marginAsset":"USDT","contractType":"CURRENT_QUARTER","deliveryDate":1758873600000,"filters":[]}]}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(2, "exchangeInfo", 200, &body), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some(spec.as_ref().clone()), _ => None }).collect();
    assert_eq!((specs[0].asset_class, specs[0].expiry), (AssetClass::Perp, None));
    assert_eq!(specs[1].asset_class, AssetClass::Future);
    assert_eq!(specs[1].expiry.map(|t| t.millis()), Some(1_758_873_600_000));
    assert_eq!(m.search_exchange(&specs[0]), "Binance PERPETUAL");
    assert!(m.instrument_id("ethusdt_250926").is_ok());
    assert!(Binance::spot().unwrap().instrument_id("ETHUSDT_250926").is_err(), "underscore is a futures spelling");
}

fn fills_cmd(cursor: Option<String>, from: i64, to: i64, symbols: &[&str]) -> Command {
    Command::QueryAccount {
        req_id: CorrId(9),
        kind: AccountKind::Fills,
        symbols: symbols.iter().map(|s| InstrumentId::new(market::USDM.venue, s)).collect(),
        range: Some(TimeRange::new(ts(from), ts(to))),
        cursor,
        account: None,
    }
}

fn start_end(r: &NativeRequest) -> (i64, i64) {
    let NativeRequest::Http(h) = r else { panic!() };
    let get = |k: &str| h.query.iter().find(|(q, _)| q == k).and_then(|(_, v)| v.parse().ok()).unwrap();
    (get("startTime"), get("endTime"))
}

#[test]
fn a_period_is_walked_a_week_at_a_time_then_symbol_by_symbol() {
    let m = Binance::usdm().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_700_000_000, 1_700_000_000 + 20 * 86_400);
    let mut cursor = None;
    let mut windows = Vec::new();
    loop {
        let reqs = encode_once(&m, &mut s, &cache, &fills_cmd(cursor.clone(), from, to, &["BTCUSDT", "ETHUSDT"]), ts(0)).unwrap();
        windows.push(start_end(&reqs[0]));
        let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "userTrades", 200, "[]"), ts(0)).unwrap();
        let Event::AccountChunk { next_cursor, .. } = &evs[0].event else { panic!() };
        match next_cursor {
            Some(n) => cursor = Some(n.clone()),
            None => break,
        }
    }
    assert_eq!(windows.len(), 6, "three weeks per symbol, two symbols: {windows:?}");
    for pair in windows[..3].windows(2) {
        assert_eq!(pair[0].1, pair[1].0, "windows tile the period");
    }
    assert_eq!(windows[0].0, from * 1000);
    assert_eq!(windows[2].1, to * 1000);
    assert_eq!(windows[3].0, from * 1000, "the next symbol starts the period again");
}

#[test]
fn a_full_page_continues_one_millisecond_past_its_last_fill() {
    let m = Binance::usdm().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_700_000_000, 1_700_000_000 + 3 * 86_400);
    let reqs = encode_once(&m, &mut s, &cache, &fills_cmd(None, from, to, &["BTCUSDT"]), ts(0)).unwrap();
    let (start, _) = start_end(&reqs[0]);
    let rows: Vec<String> = (0..1000)
        .map(|i| format!(r#"{{"id":{i},"orderId":1,"symbol":"BTCUSDT","side":"BUY","qty":"1","price":"30000","commission":"0.5","commissionAsset":"usdt","time":{}}}"#, start + i))
        .collect();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(9, "userTrades", 200, &format!("[{}]", rows.join(","))), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), next_cursor: Some(next), .. } = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(f.len(), 1000);
    assert_eq!(f[0].fee_asset.as_deref(), Some("USDT"));
    let reqs = encode_once(&m, &mut s, &cache, &fills_cmd(Some(next.clone()), from, to, &["BTCUSDT"]), ts(0)).unwrap();
    assert_eq!(start_end(&reqs[0]).0, start + 1000);
}

#[test]
fn positions_read_the_side_from_hedge_mode_or_the_sign() {
    let m = Binance::usdm().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let q = Command::QueryAccount { req_id: CorrId(5), kind: AccountKind::Positions, symbols: vec![], range: None, cursor: None, account: None };
    encode_once(&m, &mut s, &cache, &q, ts(0)).unwrap();
    let body = json(
        r#"[{"symbol":"BTCUSDT","positionAmt":"0.010","entryPrice":"30000.5","positionSide":"LONG","unRealizedProfit":"1.2"},
            {"symbol":"ETHUSDT","positionAmt":"-2","entryPrice":"2000","positionSide":"SHORT"},
            {"symbol":"SOLUSDT","positionAmt":"-3","entryPrice":"0","positionSide":"BOTH"},
            {"symbol":"XRPUSDT","positionAmt":"0","entryPrice":"0","positionSide":"BOTH"}]"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "positionRisk", 200, &body), ts(0)).unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    let got: Vec<(String, String, Option<String>)> = p.iter().map(|p| (p.instrument.native.clone(), p.qty.to_string(), p.avg_price.map(|x| x.to_string()))).collect();
    assert_eq!(
        got,
        vec![
            ("BTCUSDT".into(), "0.010".into(), Some("30000.5".into())),
            ("ETHUSDT".into(), "-2".into(), Some("2000".into())),
            ("SOLUSDT".into(), "-3".into(), None),
        ]
    );
}

#[test]
fn an_empty_or_inverted_period_is_refused() {
    let m = Binance::usdm().unwrap();
    let mut s = Session::default();
    let e = encode_once(&m, &mut s, &InstrumentCache::new(), &fills_cmd(None, 10, 10, &["BTCUSDT"]), ts(0)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::InvalidRequest);
}

#[test]
fn the_futures_book_ticker_opens_on_the_public_route_with_the_venue_clock() {
    let m = Binance::usdm().unwrap();
    assert_eq!(m.socket_class(&btc(), &DataType::Quote), "public");
    assert_eq!(m.socket_class(&btc(), &DataType::Candles { tf: Timeframe::M1 }), "");
    let mut s = Session::default();
    let c = otw_connector_model::ConnectorId("t".into());
    let cache = InstrumentCache::new();
    let ctx = otw_connector_sdk::MapCtx { received_ts: ts(0), instruments: &cache, connector: &c };
    let out = m.encode_batch(&[Command::Subscribe { sub_id: CorrId(3), instrument: btc(), data_type: DataType::Quote }], &mut s, &ctx).unwrap();
    assert_eq!(out[0], NativeRequest::WsOpen { conn_key: otw_connector_sdk::ConnKey("main".into()), base: "ws_public", path: "/ws/btcusdt@bookTicker".into() });
    assert!(m.base_urls().contains(&("ws_public", "wss://fstream.binance.com/public")));
    // Recorded from `wss://fstream.binance.com/public`.
    let text = r#"{"e":"bookTicker","u":11860787184197,"s":"BTCUSDT","ps":"BTCUSDT","b":"82238.50","B":"4.852","a":"82238.60","A":"2.651","T":1792408158974,"E":1792408158974,"st":1}"#;
    let frame = otw_connector_sdk::RawFrame::Ws { conn_key: otw_connector_sdk::ConnKey("main".into()), payload: text.to_string().into() };
    let evs = decode_once(&m, &mut s, &cache, &frame, ts(1_792_408_159)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(3)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_408_158_974));
    let Event::Quote(q) = &evs[0].event else { panic!() };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82238.50".to_string(), "82238.60".to_string()));
}
