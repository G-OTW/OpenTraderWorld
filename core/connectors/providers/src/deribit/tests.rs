//! Deribit, offline. Market-data fixtures are answers recorded from the public endpoints and
//! the socket on 2026-10-19 (trimmed); account fixtures follow the samples of Deribit's own
//! API reference and ccxt's.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_sdk::{ConnKey, InstrumentCache, MapCtx, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_history_sound, assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, json, ts};

use super::*;

/// The moment the recorded answers were taken: Monday 2026-10-19 09:59:45.521 UTC.
const NOW_MS: i64 = 1_792_403_985_521;

fn now() -> Timestamp {
    Timestamp::from_millis(NOW_MS).unwrap()
}

fn at_ms(ms: i64) -> Timestamp {
    Timestamp::from_millis(ms).unwrap()
}

fn id(native: &str) -> InstrumentId {
    InstrumentId::new(VENUE, native)
}

fn hist(native: &str, tf: Timeframe, from_ms: i64, to_ms: i64, cursor: Option<String>) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: id(native), data_type: DataType::Candles { tf }, range: TimeRange::new(at_ms(from_ms), at_ms(to_ms)), cursor }
}

fn q(h: &HttpRequest, k: &str) -> Option<String> {
    h.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}

fn qi(h: &HttpRequest, k: &str) -> i64 {
    q(h, k).unwrap().parse().unwrap()
}

fn one(reqs: &[NativeRequest]) -> &HttpRequest {
    let [NativeRequest::Http(h)] = reqs else { panic!("{reqs:?}") };
    h
}

/// A spot pair Deribit matches itself, known to the session.
fn session_knowing(native: &str, routed: bool) -> Session {
    let mut s = Session::default();
    s.routed.insert(native.into(), routed);
    s
}

/// Recorded: BTC-PERPETUAL, four minutes; `volume` in BTC, `cost` in USD.
const BTC_PERP_1M: &str = r#"{"usOut":1792403985589740,"usIn":1792403985587032,"usDiff":2708,"testnet":false,"result":{"volume":[3.77014441,10.26913517,16.93499071,26.47085279],"ticks":[1792403700000,1792403760000,1792403820000,1792403880000],"status":"ok","open":[82903.0,82898.0,82865.5,82827.0],"low":[82898.5,82853.5,82808.0,82701.0],"high":[82920.0,82908.0,82872.0,82830.0],"cost":[312580.0,851130.0,1402930.0,2190510.0],"close":[82898.5,82868.0,82825.0,82732.5]},"jsonrpc":"2.0"}"#;

#[test]
fn declares_only_what_it_serves() {
    let m = Deribit::new().unwrap();
    assert_undeclared_is_unsupported(&m, &id("BTC-PERPETUAL"));
    assert_eq!((m.capabilities().provider.as_str(), m.capabilities().venue.as_str()), ("deribit", "DERIBIT"));
}

#[test]
fn a_name_says_what_it_is() {
    let n = Name::of("BTC-PERPETUAL");
    assert_eq!((n.kind, n.inverse(), n.quote().as_deref(), n.class()), (Kind::Perp, true, Some("USD"), Some(AssetClass::Perp)));
    let n = Name::of("BTC_USDC-PERPETUAL");
    assert_eq!((n.kind, n.inverse(), n.quote().as_deref()), (Kind::Perp, false, Some("USDC")));
    let n = Name::of("BTC-30OCT26");
    assert_eq!((n.kind, n.inverse(), n.expiry_ms()), (Kind::Future, true, Some(1_793_347_200_000)), "08:00 UTC on its date, as the list says");
    let n = Name::of("BTC-25DEC26-100000-C");
    assert_eq!((n.kind, n.inverse(), n.quote().as_deref()), (Kind::Option, false, Some("BTC")), "a coin-settled option is priced in the coin");
    assert_eq!(Name::of("AVAX_USDC-20OCT26-11d2-C").quote().as_deref(), Some("USDC"));
    let n = Name::of("ETH_BTC");
    assert_eq!((n.kind, n.quote().as_deref()), (Kind::Spot, Some("BTC")));
    for combo in ["BTC-FS-20OCT26_PERP", "BTC-PBUT-20OCT26-80000_81000_82000", "BTC"] {
        assert_eq!(Name::of(combo).kind, Kind::Combo, "{combo}");
    }
    let m = Deribit::new().unwrap();
    assert_eq!(m.instrument_id(" btc-perpetual ").unwrap().native, "BTC-PERPETUAL");
    assert_eq!(m.instrument_id("xrp_usdc-25dec26-1d75-c").unwrap().native, "XRP_USDC-25DEC26-1d75-C", "the strike's fraction mark stays lower case");
    assert!(m.instrument_id("BTC/USD").is_err());
    assert_eq!(m.pair_symbol("btc", "usdc").as_deref(), Some("BTC_USDC"));
}

#[test]
fn a_page_asks_bar_opens_both_included_and_settles_for_ten_seconds() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_403_700_000, 1_792_403_940_000);
    let reqs = encode_once(&m, &mut s, &cache, &hist("BTC-PERPETUAL", Timeframe::M1, from, to, None), now()).unwrap();
    let h = one(&reqs);
    assert_eq!(h.path, "/api/v2/public/get_tradingview_chart_data");
    assert_eq!(
        (q(h, "instrument_name").as_deref(), q(h, "resolution").as_deref(), qi(h, "start_timestamp"), qi(h, "end_timestamp")),
        (Some("BTC-PERPETUAL"), Some("1"), from, to - 60_000),
        "the last bar asked opens before the range's end"
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "chart", 200, BTC_PERP_1M), now()).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs.len(), next.as_ref(), *done), (4, None, true));
    let b = &cs[1];
    assert_eq!((b.open.to_string(), b.high.to_string(), b.low.to_string(), b.close.to_string()), ("82898.0".into(), "82908.0".into(), "82853.5".into(), "82868.0".into()));
    assert_eq!((b.volume.to_string(), b.quote_volume.unwrap().to_string()), ("10.26913517".into(), "851130.0".into()), "BTC traded, USD turnover");
    assert!(cs.iter().all(|c| c.is_final), "closed for more than the 10 s settle window");
    assert_history_sound(&[HistoryItems::Candles(cs.clone())]);
    // Forty seconds earlier the last minute was still settling.
    encode_once(&m, &mut s, &cache, &hist("BTC-PERPETUAL", Timeframe::M1, from, to, None), now()).unwrap();
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "chart", 200, BTC_PERP_1M), at_ms(NOW_MS - 40_000)).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, true, true, false]);
}

#[test]
fn a_linear_perpetual_counts_coins_and_usdc() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist("BTC_USDC-PERPETUAL", Timeframe::M1, 1_792_403_700_000, 1_792_403_940_000, None), now()).unwrap();
    // Recorded: `volume` in BTC, `cost` in USDC.
    let body = r#"{"usOut":1792403985660898,"usIn":1792403985658498,"usDiff":2400,"testnet":false,"result":{"volume":[16.0986,13.5055,28.1598,51.9468],"ticks":[1792403700000,1792403760000,1792403820000,1792403880000],"status":"ok","open":[82904.0,82900.8,82860.0,82824.3],"low":[82901.2,82850.8,82811.0,82695.6],"high":[82918.9,82902.1,82870.6,82824.3],"cost":[1334725.80201,1119338.56338,2332774.19252,4298329.42],"close":[82901.2,82859.0,82827.5,82732.8]},"jsonrpc":"2.0"}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "chart", 200, body), now()).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs[3].volume.to_string(), cs[3].quote_volume.unwrap().to_string()), ("51.9468".into(), "4298329.42".into()));
}

#[test]
fn a_long_range_walks_backwards_and_stops_at_the_listing() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // Ten days of minutes: 14400 bars, three pages of at most 5000.
    let to = NOW_MS - NOW_MS % 60_000;
    let from = to - 10 * 86_400_000;
    let page = |s: &mut Session, cursor: Option<String>| {
        let reqs = encode_once(&m, s, &cache, &hist("BTC-PERPETUAL", Timeframe::M1, from, to, cursor), now()).unwrap();
        let h = one(&reqs).clone();
        (qi(&h, "start_timestamp"), qi(&h, "end_timestamp"))
    };
    let answer = |s: &mut Session, body: &str| {
        let evs = decode_once(&m, s, &cache, &http_frame(1, "chart", 200, body), now()).unwrap();
        chunks(&evs)[0].clone()
    };
    let ok_from = |t: i64| format!(r#"{{"jsonrpc":"2.0","result":{{"status":"ok","ticks":[{t}],"open":[1],"high":[1],"low":[1],"close":[1],"volume":[0],"cost":[0]}}}}"#);
    let (a, b) = page(&mut s, None);
    assert_eq!((b, (b - a) / 60_000 + 1), (to - 60_000, 5000), "the newest 5000 bars first");
    let (_, next, done) = answer(&mut s, &ok_from(a));
    assert!(!done);
    assert_eq!(next.as_deref(), Some(format!("{a}:1").as_str()));
    let (a2, b2) = page(&mut s, next);
    assert_eq!(b2, a - 60_000, "no bar asked twice");
    // The listing falls inside this window: its first bar opens after the window's start.
    let (_, next, done) = answer(&mut s, &ok_from(a2 + 3_600_000));
    assert!(done && next.is_none(), "nothing older than the listing is asked");
    // Before the listing the archive answers `no_data` (recorded): once bars were seen, the
    // walk ends there; before any, it goes on (a contract delisted inside the range).
    let no_data = r#"{"usOut":1792403985735703,"usIn":1792403985733412,"usDiff":2291,"testnet":false,"result":{"volume":[],"ticks":[],"status":"no_data","open":[],"low":[],"high":[],"cost":[],"close":[]},"jsonrpc":"2.0"}"#;
    page(&mut s, Some(format!("{a}:1")));
    assert!(answer(&mut s, no_data).2);
    page(&mut s, None);
    let (_, next, done) = answer(&mut s, no_data);
    assert!(!done && next.as_deref() == Some(format!("{a}:0").as_str()));
    let (a3, _) = page(&mut s, Some(format!("{a2}:0")));
    assert_eq!(a3, from, "the last page starts at the range's start");
    let (_, next, done) = answer(&mut s, &ok_from(a3));
    assert!(done && next.is_none());
}

#[test]
fn days_open_at_eight_and_a_dated_contract_stops_at_its_expiry() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    // From a midnight: the first daily bar asked is that day's 08:00 one.
    let midnight = 1_792_108_800_000;
    let reqs = encode_once(&m, &mut s, &cache, &hist("BTC-PERPETUAL", Timeframe::D1, midnight, NOW_MS, None), now()).unwrap();
    let h = one(&reqs);
    assert_eq!((q(h, "resolution").as_deref(), qi(h, "start_timestamp"), qi(h, "end_timestamp")), (Some("1D"), midnight + 8 * 3_600_000, 1_792_396_800_000));
    // Recorded.
    let body = r#"{"usOut":1792403985871886,"usIn":1792403985869409,"usDiff":2477,"testnet":false,"result":{"volume":[14360.25298085,6155.8887642,14083.55795897,1342.28976698],"ticks":[1792137600000,1792224000000,1792310400000,1792396800000],"status":"ok","open":[82220.0,81630.5,81605.0,82659.5],"low":[80725.5,80735.0,80600.0,82513.0],"high":[82859.5,81983.5,83200.0,83042.0],"cost":[1173457030.0,501296480.0,1151132420.0,111145510.0],"close":[81628.0,81605.0,82658.5,82718.0]},"jsonrpc":"2.0"}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(1, "chart", 200, body), now()).unwrap();
    let (HistoryItems::Candles(cs), _, _) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs.iter().map(|c| c.open_ts.millis() % 86_400_000).collect::<Vec<_>>(), [28_800_000; 4], "08:00 UTC");
    assert_eq!(cs.iter().map(|c| c.is_final).collect::<Vec<_>>(), [true, true, true, false]);
    // A dated contract is asked up to its expiry, and refused wholly after it.
    let reqs = encode_once(&m, &mut s, &cache, &hist("BTC-27DEC24", Timeframe::H1, 1_734_000_000_000, 1_740_000_000_000, None), now()).unwrap();
    assert_eq!(qi(one(&reqs), "end_timestamp"), 1_735_286_400_000 - 3_600_000);
    let e = encode_once(&m, &mut s, &cache, &hist("BTC-27DEC24", Timeframe::H1, 1_740_000_000_000, 1_750_000_000_000, None), now()).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("2024-12-27"), "{e:?}");
    // 3h opens on the 08:00 grid, off the epoch: not declared.
    let three: Timeframe = "3h".parse().unwrap();
    assert_eq!(encode_once(&m, &mut s, &cache, &hist("BTC-PERPETUAL", three, midnight, NOW_MS, None), now()).unwrap_err().kind, ErrorKind::Unsupported);
}

#[test]
fn a_spot_pair_is_looked_up_once_and_one_matched_on_coinbase_is_refused() {
    let m = Deribit::new().unwrap();
    let cache = InstrumentCache::new();
    let (from, to) = (1_792_403_700_000, 1_792_403_940_000);
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &cache, &hist("BTC_USDC", Timeframe::M1, from, to, None), now()).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "instrument_name").as_deref()), ("/api/v2/public/get_instrument", Some("BTC_USDC")));
    // Recorded (trimmed).
    let routed = r#"{"jsonrpc":"2.0","result":{"state":"open","kind":"spot","instrument_name":"BTC_USDC","instrument_type":"linear","is_active":true,"contract_size":1.0e-8,"tick_size":0.01,"is_cbe_routed":true,"min_trade_amount":1.0e-8,"base_currency":"BTC","quote_currency":"USDC"},"usIn":1792403986259997,"usOut":1792403986260330,"usDiff":333,"testnet":false}"#;
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "instrument", 200, routed), now()).unwrap_err();
    assert!(e.kind == ErrorKind::Unsupported && e.message.contains("Coinbase") && e.message.contains("BTC-USDC"), "{e:?}");
    assert!(m.follow_ups(CorrId(1), &mut s).is_empty());
    // Known now: refused before any call.
    assert_eq!(encode_once(&m, &mut s, &cache, &hist("BTC_USDC", Timeframe::M1, from, to, None), now()).unwrap_err().kind, ErrorKind::Unsupported);
    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &hist("BTC_USDT", Timeframe::M1, from, to, None), now()).unwrap();
    let own = r#"{"jsonrpc":"2.0","result":{"state":"open","kind":"spot","instrument_name":"BTC_USDT","instrument_type":"linear","is_active":true,"contract_size":0.0001,"tick_size":1.0,"min_trade_amount":0.0001,"base_currency":"BTC","quote_currency":"USDT"},"usIn":1792403986337520,"usOut":1792403986337804,"usDiff":284,"testnet":false}"#;
    assert!(decode_once(&m, &mut s, &cache, &http_frame(1, "instrument", 200, own), now()).unwrap().is_empty());
    let next = m.follow_ups(CorrId(1), &mut s);
    assert_eq!((one(&next).path.as_str(), q(one(&next), "instrument_name").as_deref()), ("/api/v2/public/get_tradingview_chart_data", Some("BTC_USDT")));
    let mut s = session_knowing("ETH_BTC", false);
    let reqs = encode_once(&m, &mut s, &cache, &hist("ETH_BTC", Timeframe::M1, from, to, None), now()).unwrap();
    assert_eq!(one(&reqs).path, "/api/v2/public/get_tradingview_chart_data", "known: no lookup");
}

#[test]
fn an_unknown_instrument_is_refused_by_name_with_deribits_spelling() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    encode_once(&m, &mut s, &cache, &hist("NOPE-PERPETUAL", Timeframe::M1, NOW_MS - 600_000, NOW_MS, None), now()).unwrap();
    // Recorded.
    let body = r#"{"usOut":1792403985799342,"usIn":1792403985799330,"usDiff":12,"testnet":false,"jsonrpc":"2.0","error":{"message":"Invalid params","data":{"reason":"instrument not found","param":"instrument_name"},"code":-32602}}"#;
    let e = decode_once(&m, &mut s, &cache, &http_frame(1, "chart", 400, body), now()).unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("NOPE-PERPETUAL") && e.message.contains("BTC_USDC-PERPETUAL"), "{e:?}");
}

#[test]
fn the_list_reads_every_kind_and_leaves_combos_out() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let reqs = encode_once(&m, &mut s, &cache, &Command::LoadInstruments { req_id: CorrId(3) }, now()).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "currency").as_deref(), q(one(&reqs), "expired").as_deref()), ("/api/v2/public/get_instruments", Some("any"), Some("false")));
    // Recorded (trimmed).
    let body = json(
        r#"{"jsonrpc":"2.0","result":[
{"instrument_name":"BTC-PERPETUAL","kind":"future","instrument_type":"reversed","base_currency":"BTC","quote_currency":"USD","settlement_currency":"BTC","settlement_period":"perpetual","tick_size":0.5,"min_trade_amount":10.0,"contract_size":10.0,"expiration_timestamp":32503708800000,"is_active":true,"state":"open","creation_timestamp":1534242287000},
{"instrument_name":"BTC_USDC-PERPETUAL","kind":"future","instrument_type":"linear","base_currency":"BTC","quote_currency":"USDC","settlement_currency":"USDC","settlement_period":"perpetual","tick_size":0.1,"min_trade_amount":0.0001,"contract_size":0.0001,"expiration_timestamp":32503708800000,"is_active":true,"state":"open","creation_timestamp":1646824342000},
{"instrument_name":"BTC-30OCT26","kind":"future","instrument_type":"reversed","base_currency":"BTC","quote_currency":"USD","settlement_currency":"BTC","settlement_period":"month","tick_size":2.5,"min_trade_amount":10.0,"contract_size":10.0,"expiration_timestamp":1793347200000,"is_active":true,"state":"open","creation_timestamp":1785398413000},
{"instrument_name":"BTC-25DEC26-100000-C","kind":"option","instrument_type":"reversed","base_currency":"BTC","quote_currency":"BTC","settlement_currency":"BTC","settlement_period":"month","tick_size":0.0001,"min_trade_amount":0.1,"contract_size":1.0,"expiration_timestamp":1798185600000,"strike":100000.0,"option_type":"call","is_active":true,"state":"open","creation_timestamp":1766649615000},
{"instrument_name":"AVAX_USDC-20OCT26-11d2-C","kind":"option","instrument_type":"linear","base_currency":"AVAX","quote_currency":"USDC","settlement_currency":"USDC","settlement_period":"day","tick_size":0.0005,"min_trade_amount":100,"contract_size":100,"expiration_timestamp":1792483200000,"strike":11.2,"option_type":"call","is_active":true,"state":"open","creation_timestamp":1792314480000},
{"instrument_name":"BTC_USDC","kind":"spot","instrument_type":"linear","base_currency":"BTC","quote_currency":"USDC","tick_size":0.01,"min_trade_amount":1e-08,"contract_size":1e-08,"expiration_timestamp":32503708800000,"is_active":true,"state":"open","is_cbe_routed":true,"creation_timestamp":1682341202000},
{"instrument_name":"ETH_BTC","kind":"spot","instrument_type":"linear","base_currency":"ETH","quote_currency":"BTC","tick_size":0.0001,"min_trade_amount":0.0001,"contract_size":0.0001,"expiration_timestamp":32503708800000,"is_active":true,"state":"open","creation_timestamp":1682341237000},
{"instrument_name":"BTC-FS-20OCT26_PERP","kind":"future_combo","instrument_type":"reversed","base_currency":"BTC","quote_currency":"USD","settlement_currency":"BTC","settlement_period":"day","tick_size":0.5,"min_trade_amount":10.0,"contract_size":10.0,"expiration_timestamp":1792483200000,"is_active":true,"state":"open","creation_timestamp":1792137996000}
],"usIn":1,"usOut":2,"usDiff":1,"testnet":false}"#,
    );
    let evs = decode_once(&m, &mut s, &cache, &http_frame(3, "instruments", 200, &body), now()).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| if let Event::Instrument { spec } = &e.event { Some(spec.clone()) } else { None }).collect();
    assert!(matches!(evs.last().unwrap().event, Event::InstrumentsEnd));
    assert_eq!(specs.len(), 7, "the combo is left out");
    let perp = &specs[0];
    assert_eq!((perp.asset_class, perp.inverse, perp.quote.as_str(), perp.settlement_ccy.as_deref(), perp.expiry), (AssetClass::Perp, true, "USD", Some("BTC"), None));
    assert_eq!((perp.min_qty.unwrap().to_string(), perp.multiplier.to_string(), perp.note.as_deref()), ("10.0".into(), "1".into(), Some("inverse: one contract is 10 USD, settled in BTC")));
    let linear = &specs[1];
    assert_eq!((linear.inverse, linear.quote.as_str(), linear.min_qty.unwrap().to_string()), (false, "USDC", "0.0001".into()));
    assert_eq!((specs[2].asset_class, specs[2].expiry.map(|t| t.millis())), (AssetClass::Future, Some(Name::of("BTC-30OCT26").expiry_ms().unwrap())));
    let call = &specs[3];
    assert_eq!((call.asset_class, call.right, call.strike.unwrap().to_string(), call.quote.as_str()), (AssetClass::Option, Some(OptionRight::Call), "100000.0".into(), "BTC"));
    assert_eq!((specs[4].id.native.as_str(), specs[4].strike.unwrap().to_string()), ("AVAX_USDC-20OCT26-11d2-C", "11.2".into()));
    let routed = &specs[5];
    assert_eq!((routed.symbol.0.as_str(), routed.active, routed.note.as_deref()), ("BTC/USDC", false, Some(ROUTED_NOTE)), "matched on Coinbase: no candles to offer");
    assert!(specs[6].active && s.routed.get("BTC_USDC") == Some(&true) && s.routed.get("ETH_BTC") == Some(&false));
}

#[test]
fn quotes_the_board_and_the_clock() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let snap = |native: &str| Command::RequestSnapshot { req_id: CorrId(4), instrument: id(native), data_type: DataType::Quote };
    let reqs = encode_once(&m, &mut s, &cache, &snap("BTC-PERPETUAL"), now()).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "instrument_name").as_deref()), ("/api/v2/public/ticker", Some("BTC-PERPETUAL")));
    // Recorded (trimmed): an inverse contract's sizes are USD.
    let body = r#"{"jsonrpc":"2.0","result":{"timestamp":1792403985999,"state":"open","index_price":82706.41,"instrument_name":"BTC-PERPETUAL","last_price":82718.0,"mark_price":82728.09,"best_ask_price":82721.0,"best_bid_price":82720.5,"best_ask_amount":45250.0,"best_bid_amount":1.681e5},"usIn":1792403986032880,"usOut":1792403986033103,"usDiff":223,"testnet":false}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(4, "ticker", 200, body), now()).unwrap();
    let Event::Quote(qt) = &evs[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.ask.to_string(), qt.bid_qty, evs[0].exchange_ts.unwrap().millis()), ("82720.5".into(), "82721.0".into(), None, 1_792_403_985_999));
    encode_once(&m, &mut s, &cache, &snap("ETH_USDT"), now()).unwrap();
    let body = r#"{"jsonrpc":"2.0","result":{"timestamp":1792403985451,"state":"open","instrument_name":"ETH_USDT","last_price":2440.2,"mark_price":2437.939,"best_ask_price":2443.9,"best_bid_price":2431.3,"best_ask_amount":1.2305,"best_bid_amount":0.4104},"usIn":1792403986106617,"usOut":1792403986106864,"usDiff":247,"testnet":false}"#;
    let Event::Quote(qt) = &decode_once(&m, &mut s, &cache, &http_frame(4, "ticker", 200, body), now()).unwrap()[0].event else { panic!() };
    assert_eq!((qt.bid_qty.unwrap().to_string(), qt.ask_qty.unwrap().to_string()), ("0.4104".into(), "1.2305".into()), "spot sizes are coins");

    // The board: the spot summaries under four currencies, one after another.
    let mut s = Session::default();
    let reqs = encode_once(&m, &mut s, &cache, &Command::RequestBoard { req_id: CorrId(5) }, now()).unwrap();
    assert_eq!((one(&reqs).path.as_str(), q(one(&reqs), "currency").as_deref(), q(one(&reqs), "kind").as_deref()), ("/api/v2/public/get_book_summary_by_currency", Some("USDC"), Some("spot")));
    let usdc = r#"{"jsonrpc":"2.0","result":[{"high":0.3613,"low":0.3274,"last":0.3546,"instrument_name":"CRV_USDC","bid_price":0.3544,"ask_price":0.3547,"mark_price":0.3546,"base_currency":"CRV","quote_currency":"USDC"},{"high":0.99897,"low":0.99864,"last":0.9986,"instrument_name":"USDT_USDC","mark_price":0.999,"base_currency":"USDT","quote_currency":"USDC"}]}"#;
    assert!(decode_once(&m, &mut s, &cache, &http_frame(5, "bookSummary", 200, usdc), now()).unwrap().is_empty());
    let mut seen = vec![];
    for (i, body) in [
        r#"{"jsonrpc":"2.0","result":[{"last":0.9986,"instrument_name":"USDT_USDC","mark_price":0.999},{"last":83060.0,"instrument_name":"BTC_USDT","mark_price":82793.9338}]}"#,
        r#"{"jsonrpc":"2.0","result":[{"last":null,"instrument_name":"BTC_USDE","mark_price":82790.5}]}"#,
        r#"{"jsonrpc":"2.0","result":[{"last":0.0295,"instrument_name":"ETH_BTC","mark_price":0.02951}]}"#,
    ]
    .iter()
    .enumerate()
    {
        let next = m.follow_ups(CorrId(5), &mut s);
        seen.push(q(one(&next), "currency").unwrap());
        let evs = decode_once(&m, &mut s, &cache, &http_frame(5, "bookSummary", 200, body), now()).unwrap();
        assert_eq!(evs.is_empty(), i < 2);
        if let Some(Event::Board(board)) = evs.first().map(|e| &e.event) {
            let rows: Vec<(String, String)> = board.iter().map(|p| (p.instrument.native.clone(), p.price.to_string())).collect();
            assert_eq!(
                rows,
                [("CRV_USDC", "0.3546"), ("USDT_USDC", "0.9986"), ("BTC_USDT", "83060.0"), ("BTC_USDE", "82790.5"), ("ETH_BTC", "0.0295")].map(|(a, b)| (a.to_string(), b.to_string())),
                "each pair once, the mark when no trade printed"
            );
        }
    }
    assert_eq!(seen, ["USDT", "BTC", "ETH"]);
    assert!(m.follow_ups(CorrId(5), &mut s).is_empty());

    let mut s = Session::default();
    encode_once(&m, &mut s, &cache, &Command::SyncTime { req_id: CorrId(6) }, now()).unwrap();
    let body = r#"{"jsonrpc":"2.0","result":1792403985955,"usIn":1792403985955292,"usOut":1792403985955357,"usDiff":65,"testnet":false}"#;
    let Event::ServerTime(t) = &decode_once(&m, &mut s, &cache, &http_frame(6, "time", 200, body), now()).unwrap()[0].event else { panic!() };
    assert_eq!(t.millis(), 1_792_403_985_955);
}

fn ws(payload: &str) -> RawFrame {
    RawFrame::Ws { conn_key: ConnKey("public".into()), payload: bytes::Bytes::copy_from_slice(payload.as_bytes()) }
}

fn sub(n: u64, native: &str, tf: Timeframe) -> Command {
    Command::Subscribe { sub_id: CorrId(n), instrument: id(native), data_type: DataType::Candles { tf } }
}

fn sent(r: &NativeRequest) -> serde_json::Value {
    let NativeRequest::WsSend { payload, .. } = r else { panic!("{r:?}") };
    serde_json::from_slice(payload).unwrap()
}

#[test]
fn the_socket_takes_panes_together_and_reads_the_ack_list() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: now(), instruments: &cache, connector: &c };
    let out = m.encode_batch(&[sub(7, "BTC-PERPETUAL", Timeframe::M1), sub(8, "NOPE-PERPETUAL", Timeframe::M1), sub(9, "ETH-PERPETUAL", Timeframe::H1)], &mut s, &ctx).unwrap();
    let NativeRequest::WsOpen { base, path, .. } = &out[0] else { panic!("{out:?}") };
    assert_eq!((*base, path.as_str()), (WS, "/ws/api/v2"));
    assert_eq!(sent(&out[1]), serde_json::json!({"jsonrpc":"2.0","id":1,"method":"public/set_heartbeat","params":{"interval":30}}));
    assert_eq!(
        sent(&out[2]),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"public/subscribe","params":{"channels":["chart.trades.BTC-PERPETUAL.1","chart.trades.NOPE-PERPETUAL.1","chart.trades.ETH-PERPETUAL.60"]}})
    );
    assert_eq!(out.len(), 3, "one frame for the three panes");
    // Recorded answers (ids aside): the heartbeat's "ok", then the channels taken; an
    // unknown instrument is simply missing from the list.
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"jsonrpc":"2.0","id":1,"result":"ok","usIn":1792403850713182,"usOut":1792403850713216,"usDiff":34,"testnet":false}"#), now()).unwrap().is_empty());
    let ack = r#"{"jsonrpc":"2.0","id":2,"result":["chart.trades.BTC-PERPETUAL.1","chart.trades.ETH-PERPETUAL.60"],"usIn":1792403850711903,"usOut":1792403850712159,"usDiff":256,"testnet":false}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(ack), now()).unwrap();
    let got: Vec<(u64, bool)> = evs.iter().map(|e| (e.corr_id.unwrap().0, matches!(e.event, Event::SubscriptionAck))).collect();
    assert_eq!(got, [(7, true), (8, false), (9, true)]);
    assert!(matches!(&evs[1].event, Event::SubscriptionFail { error } if error.message.contains("NOPE-PERPETUAL")));
    // Recorded updates: no closed flag, `tick` the bar's open.
    let push = r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"chart.trades.BTC-PERPETUAL.1","data":{"close":82872.0,"high":82872.0,"low":82863.0,"open":82865.5,"tick":1792403820000,"cost":3.602e5,"volume":4.34683738}}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(push), now()).unwrap();
    let Event::Candle(cd) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(
        (evs[0].corr_id, cd.open_ts.millis(), cd.close.to_string(), cd.volume.to_string(), cd.quote_volume.unwrap().to_string(), cd.is_final),
        (Some(CorrId(7)), 1_792_403_820_000, "82872.0".into(), "4.34683738".into(), "360200.0".into(), false)
    );
    let refused = r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"chart.trades.NOPE-PERPETUAL.1","data":{"close":1,"high":1,"low":1,"open":1,"tick":1792403820000,"cost":0,"volume":0}}}"#;
    assert!(decode_once(&m, &mut s, &cache, &ws(refused), now()).unwrap().is_empty(), "a refused pane routes nothing");
    // The venue's heartbeat asks for a `public/test`, sent at the next keepalive only.
    assert!(m.keepalive(&mut s, &ctx).is_empty());
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"params":{"type":"test_request"},"method":"heartbeat","jsonrpc":"2.0"}"#), now()).unwrap().is_empty());
    let ka = m.keepalive(&mut s, &ctx);
    assert_eq!(sent(&ka[0])["method"], "public/test");
    assert!(m.keepalive(&mut s, &ctx).is_empty());
    assert!(decode_once(&m, &mut s, &cache, &ws(r#"{"jsonrpc":"2.0","id":3,"result":{"version":"1.2.26"}}"#), now()).unwrap().is_empty());
    // Unsubscribing names the channel; the socket stays.
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(7) }], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[0])["params"], serde_json::json!({"channels":["chart.trades.BTC-PERPETUAL.1"]}));
    assert!(decode_once(&m, &mut s, &cache, &ws(push), now()).unwrap().is_empty());
    // A frame the venue could not read (recorded) is an error tied to nothing.
    let e = decode_once(&m, &mut s, &cache, &ws(r#"{"jsonrpc":"2.0","error":{"code":11050,"data":"invalid json","message":"bad_request"},"usIn":1792403915406151,"usOut":1792403915406163,"usDiff":12}"#), now()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Transient);
}

#[test]
fn the_quote_channel_is_the_best_bid_and_ask() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: now(), instruments: &cache, connector: &c };
    let q = |n: u64, native: &str| Command::Subscribe { sub_id: CorrId(n), instrument: id(native), data_type: DataType::Quote };
    let out = m.encode_batch(&[q(4, "BTC-PERPETUAL"), q(5, "ZZZ-NOPE")], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[2])["params"], serde_json::json!({"channels":["quote.BTC-PERPETUAL","quote.ZZZ-NOPE"]}));
    // Recorded: an unknown instrument is missing from the list taken.
    let ack = r#"{"jsonrpc":"2.0","id":2,"result":["quote.BTC-PERPETUAL"],"usIn":1792410176818892,"usOut":1792410176819096,"usDiff":204,"testnet":false}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(ack), now()).unwrap();
    assert!(evs.iter().any(|e| e.corr_id == Some(CorrId(4)) && matches!(e.event, Event::SubscriptionAck)));
    assert!(evs.iter().any(|e| e.corr_id == Some(CorrId(5)) && matches!(e.event, Event::SubscriptionFail { .. })));
    let push = r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"quote.BTC-PERPETUAL","data":{"timestamp":1792410176810,"instrument_name":"BTC-PERPETUAL","best_ask_price":82205.5,"best_bid_price":82205.0,"best_ask_amount":8050.0,"best_bid_amount":15540.0}}}"#;
    let evs = decode_once(&m, &mut s, &cache, &ws(push), at_ms(1_792_410_177_000)).unwrap();
    assert_eq!(evs[0].corr_id, Some(CorrId(4)));
    assert_eq!(evs[0].exchange_ts.map(|t| t.millis()), Some(1_792_410_176_810));
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("82205.0".to_string(), "82205.5".to_string()));
    assert_eq!(q.bid_qty, None, "an inverse perpetual's amounts are USD, not coins");
    otw_connector_testkit::assert_events_sound(&evs);
    let out = m.encode_batch(&[Command::Unsubscribe { sub_id: CorrId(4) }], &mut s, &ctx).unwrap();
    assert_eq!(sent(&out[0])["params"], serde_json::json!({"channels":["quote.BTC-PERPETUAL"]}));
}

#[test]
fn a_refused_subscribe_fails_its_panes_and_a_large_batch_is_split() {
    let m = Deribit::new().unwrap();
    let mut s = Session::default();
    let cache = InstrumentCache::new();
    let c = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: now(), instruments: &cache, connector: &c };
    let panes: Vec<Command> = (0..600).map(|i| sub(100 + i, &format!("BTC-25DEC26-{}-C", 10_000 + i * 1_000), Timeframe::M1)).collect();
    let out = m.encode_batch(&panes, &mut s, &ctx).unwrap();
    let frames: Vec<serde_json::Value> = out[2..].iter().map(sent).collect();
    assert!(frames.len() >= 2, "600 channels do not fit one 16 kB frame");
    assert!(out[2..].iter().all(|r| matches!(r, NativeRequest::WsSend { payload, .. } if payload.len() < 20_000)));
    assert_eq!(frames.iter().map(|f| f["params"]["channels"].as_array().unwrap().len()).sum::<usize>(), 600);
    let rid = frames[0]["id"].as_u64().unwrap();
    let n = frames[0]["params"]["channels"].as_array().unwrap().len();
    let err = format!(r#"{{"jsonrpc":"2.0","id":{rid},"error":{{"code":10028,"message":"too_many_requests"}}}}"#);
    let evs = decode_once(&m, &mut s, &cache, &ws(&err), now()).unwrap();
    assert_eq!(evs.len(), n);
    assert!(evs.iter().all(|e| matches!(&e.event, Event::SubscriptionFail { error } if error.kind == ErrorKind::RateLimited)));
    // Undeclared live data is refused before anything goes out.
    let e = m.encode_batch(&[Command::Subscribe { sub_id: CorrId(1), instrument: id("BTC-PERPETUAL"), data_type: DataType::Trades }], &mut s, &ctx).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
}

fn account(m: &Deribit, s: &mut Session, kind: AccountKind, symbols: &[&str], cursor: Option<String>, range: Option<TimeRange>) -> Vec<NativeRequest> {
    let symbols = symbols.iter().map(|n| id(n)).collect();
    let cmd = Command::QueryAccount { req_id: CorrId(20), kind, symbols, range, cursor, account: None };
    encode_once(m, s, &InstrumentCache::new(), &cmd, now()).unwrap()
}

fn account_chunk(evs: &[otw_connector_model::Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn summary_and_balances_read_every_currency() {
    let m = Deribit::new().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let h = one(&account(&m, &mut s, AccountKind::Summary, &[], None, None)).clone();
    assert!(h.signed && h.path == "/api/v2/private/get_account_summaries" && q(&h, "extended").as_deref() == Some("true"));
    // Deribit's reference sample (trimmed).
    let body = r#"{"jsonrpc":"2.0","id":2515,"result":{"id":10,"username":"user","type":"main","summaries":[{"currency":"BTC","available_funds":301.38059622,"balance":302.60065765,"equity":302.61869214,"available_withdrawal_funds":301.35396172},{"currency":"ETH","available_funds":99.999598,"balance":100,"equity":100},{"currency":"USDC","available_funds":0,"balance":0,"equity":0}]}}"#;
    let evs = decode_once(&m, &mut s, &cache, &http_frame(20, "accountSummaries", 200, body), now()).unwrap();
    let Event::AccountSummary(sm) = &evs[0].event else { panic!() };
    assert_eq!((sm.assets_held, sm.account_type.as_deref()), (2, Some("main")));
    assert_eq!(sm.detail.as_deref(), Some("Deribit main account: 302.61869214 BTC equity, 100 ETH equity"));
    account(&m, &mut s, AccountKind::Balances, &[], None, None);
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "accountSummaries", 200, body), now()).unwrap());
    let AccountItems::Balances(b) = items else { panic!() };
    assert_eq!(
        b.iter().map(|b| (b.asset.as_str(), b.total.0.to_string(), b.available.0.to_string())).collect::<Vec<_>>(),
        [("BTC", "302.60065765".to_string(), "301.38059622".to_string()), ("ETH", "100".to_string(), "99.999598".to_string())]
    );
    assert!(next.is_none());
    // A refused key says what to check (recorded refusal).
    account(&m, &mut s, AccountKind::Balances, &[], None, None);
    let e = decode_once(&m, &mut s, &cache, &http_frame(20, "accountSummaries", 400, r#"{"jsonrpc":"2.0","error":{"code":13004,"data":{"invalid":"timestamp"},"message":"invalid_credentials"}}"#), now()).unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("NTP"), "{e:?}");
}

#[test]
fn positions_and_orders_count_coins_and_inverse_ones_are_converted() {
    let m = Deribit::new().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let h = one(&account(&m, &mut s, AccountKind::Positions, &[], None, None)).clone();
    assert_eq!((h.path.as_str(), q(&h, "currency").as_deref()), ("/api/v2/private/get_positions", Some("any")));
    // Deribit's reference sample, with a short option and a linear perpetual next to it.
    let body = r#"{"jsonrpc":"2.0","id":2236,"result":[
        {"average_price":7440.18,"delta":0.006687487,"direction":"buy","floating_profit_loss":0,"index_price":7466.79,"instrument_name":"BTC-PERPETUAL","kind":"future","mark_price":7476.65,"size":50,"size_currency":0.006687487,"total_profit_loss":0.000032781},
        {"average_price":0.0105,"direction":"sell","floating_profit_loss":-0.0004,"instrument_name":"BTC-25DEC26-100000-C","kind":"option","mark_price":0.0106,"size":-2.5},
        {"average_price":82000.5,"direction":"buy","floating_profit_loss":12.5,"instrument_name":"BTC_USDC-PERPETUAL","kind":"future","mark_price":82125.5,"size":8212.55,"size_currency":0.1},
        {"average_price":0,"direction":"zero","instrument_name":"ETH-PERPETUAL","kind":"future","mark_price":2400,"size":0,"size_currency":0}]}"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "positions", 200, &json(body)), now()).unwrap());
    let AccountItems::Positions(p) = items else { panic!() };
    let rows: Vec<_> = p.iter().map(|p| (p.instrument.native.as_str(), p.qty.0.round_dp(9).to_string(), p.currency.as_deref(), p.unrealized_pnl.map(|x| x.0.to_string()), p.class)).collect();
    assert_eq!(
        rows,
        [
            ("BTC-PERPETUAL", "0.006720268".to_string(), Some("USD"), None, Some(AssetClass::Perp)),
            ("BTC-25DEC26-100000-C", "-2.5".to_string(), Some("BTC"), Some("-0.0004".to_string()), Some(AssetClass::Option)),
            ("BTC_USDC-PERPETUAL", "0.1".to_string(), Some("USDC"), Some("12.5".to_string()), Some(AssetClass::Perp)),
        ],
        "50 USD at the 7440.18 entry; a zero position is skipped"
    );
    assert!(p[0].name.as_deref().unwrap().contains("50 USD contracts"));

    let h = one(&account(&m, &mut s, AccountKind::OpenOrders, &[], None, None)).clone();
    assert!(h.path == "/api/v2/private/get_open_orders" && h.query.is_empty());
    // Deribit's reference sample, an inverse limit order and an untriggered market stop.
    let body = r#"{"jsonrpc":"2.0","id":1953,"result":[
        {"time_in_force":"good_til_cancelled","reduce_only":false,"price":0.0028,"post_only":false,"order_type":"limit","order_state":"open","order_id":"146062","instrument_name":"BTC-15FEB19-3250-P","filled_amount":0,"direction":"buy","creation_timestamp":1550050597036,"average_price":0,"api":true,"amount":10},
        {"price":80000,"order_type":"limit","order_state":"open","order_id":"BTC-1","instrument_name":"BTC-PERPETUAL","filled_amount":400,"direction":"sell","creation_timestamp":1792400000000,"amount":800},
        {"price":"market_price","trigger_price":70000,"order_type":"stop_market","order_state":"untriggered","order_id":"BTC-2","instrument_name":"BTC-PERPETUAL","filled_amount":0,"direction":"sell","creation_timestamp":1792400000001,"amount":700}]}"#;
    let (items, _) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "openOrders", 200, &json(body)), now()).unwrap());
    let AccountItems::Orders(o) = items else { panic!() };
    let rows: Vec<_> = o.iter().map(|o| (o.id.as_str(), o.qty.0.to_string(), o.filled_qty.0.to_string(), o.limit_price.map(|p| p.to_string()), o.stop_price.map(|p| p.to_string()), o.quote_ccy.as_deref())).collect();
    assert_eq!(
        rows,
        [
            ("146062", "10".to_string(), "0".to_string(), Some("0.0028".to_string()), None, Some("BTC")),
            ("BTC-1", "0.01".to_string(), "0.005".to_string(), Some("80000".to_string()), None, Some("USD")),
            ("BTC-2", "0.01".to_string(), "0".to_string(), None, Some("70000".to_string()), Some("USD")),
        ]
    );
    assert!(matches!(o[1].side, Mapped::Known(Side::Sell)) && o[2].order_type == "stop_market");
}

/// A fill row shaped like Deribit's reference sample.
fn trade(id: &str, name: &str, ts: i64) -> String {
    format!(
        r#"{{"timestamp":{ts},"state":"filled","price":0.0105,"direction":"buy","index_price":82706.41,"instrument_name":"{name}","trade_seq":1,"api":false,"amount":0.5,"mark_price":0.0106,"order_id":"BTC-77","matching_id":null,"tick_direction":3,"fee":0.00015,"profit_loss":null,"contracts":5,"trade_id":"{id}","fee_currency":"BTC","order_type":"limit","liquidity":"M"}}"#
    )
}

#[test]
fn fills_walk_the_groups_page_by_millisecond_and_hand_each_fill_out_once() {
    let m = Deribit::new().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let from = NOW_MS - 3 * 86_400_000;
    let range = TimeRange::new(at_ms(from), now());
    let h = one(&account(&m, &mut s, AccountKind::Fills, &[], None, Some(range))).clone();
    assert_eq!(h.path, "/api/v2/private/get_user_trades_by_currency_and_time");
    assert_eq!(
        (q(&h, "currency").as_deref(), q(&h, "kind").as_deref(), qi(&h, "start_timestamp"), qi(&h, "end_timestamp"), q(&h, "count").as_deref(), q(&h, "sorting").as_deref(), q(&h, "historical").as_deref()),
        (Some("BTC"), Some("option"), from, NOW_MS - 1, Some("1000"), Some("asc"), Some("true"))
    );
    // A full page: the next one starts at its last millisecond.
    let rows: Vec<String> = (0..1000).map(|i| trade(&format!("BTC-{i}"), "BTC-25DEC26-100000-C", from + 1_000 + i)).collect();
    let body = format!(r#"{{"jsonrpc":"2.0","result":{{"trades":[{}],"has_more":true}}}}"#, rows.join(","));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "userTrades", 200, &body), now()).unwrap());
    let AccountItems::Fills(f) = items else { panic!() };
    assert_eq!(f.len(), 1000);
    assert_eq!((f[0].qty.0.to_string(), f[0].price.to_string(), f[0].fee.0.to_string(), f[0].quote_ccy.as_deref(), f[0].class), ("0.5".into(), "0.0105".into(), "0.00015".into(), Some("BTC"), Some(AssetClass::Option)));
    assert!(matches!(f[0].side, Mapped::Known(Side::Buy)));
    let h = one(&account(&m, &mut s, AccountKind::Fills, &[], next, Some(range))).clone();
    assert_eq!(qi(&h, "start_timestamp"), from + 1_999);
    // The page repeats the last fill of the one before: it is not handed out twice.
    let body = format!(r#"{{"jsonrpc":"2.0","result":{{"trades":[{},{}],"has_more":false}}}}"#, trade("BTC-999", "BTC-25DEC26-100000-C", from + 1_999), trade("BTC-1000", "BTC-25DEC26-100000-C", from + 2_500));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "userTrades", 200, &body), now()).unwrap());
    assert!(matches!(items, AccountItems::Fills(ref f) if f.len() == 1 && f[0].id == "BTC-1000"));
    // Then the recent store of the same group, over the last day.
    let h = one(&account(&m, &mut s, AccountKind::Fills, &[], next, Some(range))).clone();
    assert_eq!((q(&h, "currency").as_deref(), q(&h, "historical").as_deref(), qi(&h, "start_timestamp")), (Some("BTC"), Some("false"), NOW_MS - 86_400_000));
    let body = format!(r#"{{"jsonrpc":"2.0","result":{{"trades":[{}],"has_more":false}}}}"#, trade("BTC-1000", "BTC-25DEC26-100000-C", from + 2_500));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "userTrades", 200, &body), now()).unwrap());
    assert!(matches!(items, AccountItems::Fills(ref f) if f.is_empty()), "seen in the historical store already");
    // Then the next group, from the period's start.
    let h = one(&account(&m, &mut s, AccountKind::Fills, &[], next, Some(range))).clone();
    assert_eq!((q(&h, "currency").as_deref(), q(&h, "kind").as_deref(), q(&h, "historical").as_deref(), qi(&h, "start_timestamp")), (Some("ETH"), Some("option"), Some("true"), from));
}

#[test]
fn a_coinbase_routed_spot_group_is_passed_over_and_inverse_fills_are_refused_by_name() {
    let m = Deribit::new().unwrap();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let range = TimeRange::new(ts(1_700_000_000), ts(1_700_086_400));
    let h = one(&account(&m, &mut s, AccountKind::Fills, &["ETH_USDC", "BTC_USDC-PERPETUAL"], None, Some(range))).clone();
    assert_eq!((q(&h, "currency").as_deref(), q(&h, "kind").as_deref()), (Some("USDC"), Some("spot")));
    let e = r#"{"jsonrpc":"2.0","error":{"code":11060,"message":"not_supported_for_coinbase_routed_spot"}}"#;
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "userTrades", 400, e), now()).unwrap());
    assert!(matches!(items, AccountItems::Fills(ref f) if f.is_empty()));
    let h = one(&account(&m, &mut s, AccountKind::Fills, &["ETH_USDC", "BTC_USDC-PERPETUAL"], next, Some(range))).clone();
    assert_eq!((q(&h, "currency").as_deref(), q(&h, "kind").as_deref()), (Some("USDC"), Some("future")), "an old period: no recent store");
    let body = format!(r#"{{"jsonrpc":"2.0","result":{{"trades":[{}],"has_more":false}}}}"#, trade("USDC-1", "BTC_USDC-PERPETUAL", 1_700_000_500_000));
    let (items, next) = account_chunk(&decode_once(&m, &mut s, &cache, &http_frame(20, "userTrades", 200, &body), now()).unwrap());
    assert!(matches!(items, AccountItems::Fills(ref f) if f[0].quote_ccy.as_deref() == Some("USDC") && f[0].class == Some(AssetClass::Perp)));
    assert!(next.is_none(), "both named groups read");
    assert!(s.seen_fills.is_empty(), "the walk's ids are dropped at its end");
    // A key refused on a fill page says what to check.
    account(&m, &mut s, AccountKind::Fills, &["ETH_USDC"], None, Some(range));
    let e = decode_once(&m, &mut s, &cache, &http_frame(20, "userTrades", 400, r#"{"jsonrpc":"2.0","error":{"code":13021,"message":"forbidden"}}"#), now()).unwrap_err();
    assert!(e.kind == ErrorKind::Permission && e.message.contains("trade:read"), "{e:?}");
    let cmd = Command::QueryAccount { req_id: CorrId(21), kind: AccountKind::Fills, symbols: vec![id("BTC-PERPETUAL")], range: Some(range), cursor: None, account: None };
    let e = encode_once(&m, &mut Session::default(), &cache, &cmd, now()).unwrap_err();
    assert!(e.kind == ErrorKind::Unsupported && e.message.contains("inverse"), "{e:?}");
}
