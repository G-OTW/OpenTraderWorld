//! OANDA, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, ErrorKind, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_sdk::{InstrumentCache, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_undeclared_is_unsupported, chunks, decode_once, encode_once, http_frame, ts};

use super::*;

fn m() -> Oanda {
    Oanda::new().unwrap()
}

fn eur() -> InstrumentId {
    InstrumentId::new(VENUE, "EUR_USD")
}

fn hist(tf: Timeframe) -> Command {
    Command::RequestHistory { req_id: CorrId(1), instrument: eur(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(1_704_067_200), ts(1_704_326_400)), cursor: None }
}

fn q(h: &HttpRequest) -> Vec<(&str, &str)> {
    h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

fn account(kind: AccountKind, cursor: Option<&str>) -> Command {
    Command::QueryAccount { req_id: CorrId(7), kind, symbols: vec![], range: Some(TimeRange::new(ts(1_600_000_000), ts(1_700_000_000))), cursor: cursor.map(str::to_string), account: None }
}

fn secrets() -> HashMap<String, String> {
    [("api_token", " tok "), ("account_id", "001-004-1234567-001")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    assert_undeclared_is_unsupported(&m(), &eur());
}

#[test]
fn the_signer_writes_the_account_into_the_path_and_the_token_into_a_header() {
    let a = m();
    let signer = a.signer(&secrets()).unwrap().unwrap();
    let mut req = SignableRequest { method: otw_connector_sdk::native::Method::Get, path: acct("/summary"), query: vec![], headers: vec![], body: None };
    signer.sign(&mut req, ts(0)).unwrap();
    assert_eq!(req.path, "/v3/accounts/001-004-1234567-001/summary");
    assert!(req.headers.contains(&("Authorization".into(), "Bearer tok".into())));
    assert!(req.headers.contains(&("Accept-Datetime-Format".into(), "RFC3339".into())));
    let mut no_account = secrets();
    no_account.remove("account_id");
    assert_eq!(a.signer(&no_account).err().map(|e| e.kind), Some(ErrorKind::Auth));
    let mut bad = secrets();
    bad.insert("account_id".into(), "my-oanda-token".into());
    assert!(a.signer(&bad).is_err());
    assert!(a.check_settings(&bad).is_err() && a.check_settings(&secrets()).is_ok());
}

#[test]
fn candles_carry_mid_bid_and_ask_and_the_complete_flag() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &hist(Timeframe::H4), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert!(h.signed);
    assert_eq!(h.path, "/v3/accounts/{account}/instruments/EUR_USD/candles");
    assert_eq!(
        q(h),
        [
            ("price", "MBA"),
            ("granularity", "H4"),
            ("from", "2024-01-01T00:00:00Z"),
            ("to", "2024-01-04T00:00:00Z"),
            ("dailyAlignment", "0"),
            ("alignmentTimezone", "UTC"),
            ("weeklyAlignment", "Monday")
        ]
    );
    let body = r#"{"instrument":"EUR_USD","granularity":"H4","candles":[
        {"complete":true,"volume":4120,"time":"2024-01-02T04:00:00.000000000Z","bid":{"o":"1.09410","h":"1.09500","l":"1.09380","c":"1.09450"},"mid":{"o":"1.09417","h":"1.09507","l":"1.09387","c":"1.09457"},"ask":{"o":"1.09424","h":"1.09514","l":"1.09394","c":"1.09464"}},
        {"complete":false,"volume":12,"time":"2024-01-02T08:00:00.000000000Z","mid":{"o":"1.09457","h":"1.09460","l":"1.09450","c":"1.09455"}},
        {"complete":true,"volume":1,"time":"2024-01-04T00:00:00.000000000Z","mid":{"o":"1","h":"1","l":"1","c":"1"}}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "candles", 200, body), ts(0)).unwrap();
    let (HistoryItems::Candles(cs), next, done) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs.len(), next.is_none(), *done), (2, true, true), "the bar at the window's end is the next window's");
    assert!(cs[0].is_final && !cs[1].is_final);
    assert_eq!((cs[0].open.to_string(), cs[0].volume.to_string()), ("1.09417".into(), "4120".into()));
    let sides = cs[0].sides.unwrap();
    assert_eq!((sides.bid.close.to_string(), sides.ask.high.to_string()), ("1.09450".into(), "1.09514".into()));
    assert!(cs[1].sides.is_none(), "one side missing is no sides");
    assert_eq!(encode_once(&a, &mut Session::default(), &cache, &hist(Timeframe::M30), ts(0)).unwrap_err().kind, ErrorKind::Unsupported);
}

#[test]
fn refusals_name_the_fault() {
    let a = m();
    let cache = InstrumentCache::new();
    let case = |status: u16, body: &str| {
        let mut s = Session::default();
        encode_once(&a, &mut s, &cache, &hist(Timeframe::D1), ts(0)).unwrap();
        decode_once(&a, &mut s, &cache, &http_frame(1, "candles", status, body), ts(0)).unwrap_err()
    };
    let e = case(400, r#"{"errorMessage":"Invalid value specified for 'instrument'"}"#);
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"));
    assert_eq!(case(400, r#"{"errorMessage":"Invalid value specified for 'to'. Time is in the future"}"#).kind, ErrorKind::InvalidRequest);
    assert_eq!(case(401, r#"{"errorMessage":"Insufficient authorization to perform request."}"#).kind, ErrorKind::Auth);
    assert_eq!(case(429, "{}").kind, ErrorKind::RateLimited);
}

#[test]
fn the_latest_price_is_the_closeout_pair() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &Command::RequestSnapshot { req_id: CorrId(2), instrument: eur(), data_type: DataType::Quote }, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.path.as_str(), q(h)), ("/v3/accounts/{account}/pricing", vec![("instruments", "EUR_USD")]));
    let body = r#"{"prices":[{"type":"PRICE","instrument":"EUR_USD","time":"2024-01-02T14:30:00.123456789Z","bids":[{"price":"1.09410","liquidity":1000000}],"asks":[{"price":"1.09424","liquidity":1000000}],"closeoutBid":"1.09405","closeoutAsk":"1.09429","tradeable":true}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(2, "pricing", 200, body), ts(0)).unwrap();
    let Event::Quote(qt) = &evs[0].event else { panic!() };
    assert_eq!((qt.bid.to_string(), qt.ask.to_string()), ("1.09405".into(), "1.09429".into()));
    assert!(evs[0].exchange_ts.is_some());
}

#[test]
fn the_instrument_list_files_each_kind() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let ids = vec![eur(), InstrumentId::new(VENUE, "XAU_USD")];
    let reqs = encode_once(&a, &mut s, &cache, &Command::ResolveInstruments { req_id: CorrId(3), ids }, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h), [("instruments", "EUR_USD,XAU_USD")]);
    let body = r#"{"instruments":[{"name":"EUR_USD","type":"CURRENCY","displayName":"EUR/USD"},{"name":"SPX500_USD","type":"CFD","displayName":"US SPX 500"},{"name":"XAU_USD","type":"METAL","displayName":"Gold"}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(3, "instruments", 200, body), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some(spec.as_ref().clone()), _ => None }).collect();
    let kinds: Vec<_> = specs.iter().map(|s| (s.id.native.as_str(), s.asset_class, s.quote.as_str(), a.search_exchange(s))).collect();
    assert_eq!(
        kinds,
        [
            ("EUR_USD", AssetClass::Forex, "USD", "CURRENCY".to_string()),
            ("SPX500_USD", AssetClass::Cfd, "USD", "CFD".to_string()),
            ("XAU_USD", AssetClass::Commodity, "USD", "METAL".to_string())
        ]
    );
    assert!(matches!(evs.last().map(|e| &e.event), Some(Event::InstrumentsEnd)));
}

fn items(evs: &[Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn fills_walk_a_year_at_a_time_then_each_id_range() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    // 1 600 000 000 to 1 700 000 000 is over three years: the first slice is 365 days.
    let reqs = encode_once(&a, &mut s, &cache, &account(AccountKind::Fills, None), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.path, "/v3/accounts/{account}/transactions");
    assert_eq!(q(h), [("from", "2020-09-13T12:26:40Z"), ("to", "2021-09-13T12:26:40Z"), ("pageSize", "1000"), ("type", "ORDER_FILL")]);
    let index = r#"{"from":"x","to":"y","pageSize":1000,"count":3,"pages":["https://api-fxtrade.oanda.com/v3/accounts/X/transactions/idrange?from=6409&to=6412","https://evil.example/steal?from=abc&to=1","https://api-fxtrade.oanda.com/v3/accounts/X/transactions/idrange?from=6413&to=6500"]}"#;
    let (fills, next) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "transactions", 200, index), ts(0)).unwrap());
    assert!(matches!(fills, AccountItems::Fills(f) if f.is_empty()));
    let next = next.unwrap();
    assert_eq!(next, "0:1631536000000:6409-6412,6413-6500", "the bad URL is dropped, its id pair never trusted");
    let reqs = encode_once(&a, &mut s, &cache, &account(AccountKind::Fills, Some(&next)), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.path.as_str(), q(h)), ("/v3/accounts/{account}/transactions/idrange", vec![("from", "6409"), ("to", "6412"), ("type", "ORDER_FILL")]));
    let page = r#"{"transactions":[
        {"type":"ORDER_FILL","id":"6410","orderID":"6409","accountID":"001-004-1234567-001","instrument":"EUR_USD","units":"-100","price":"1.13031","fullVWAP":"1.13030","commission":"-0.40000","guaranteedExecutionFee":"0.10000","time":"2016-06-22T18:41:52.655959788Z"},
        {"type":"ORDER_FILL","id":"6411","orderID":"6411","instrument":"","units":"5"},
        {"type":"DAILY_FINANCING","id":"6412"}]}"#;
    let (fills, next) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "idrange", 200, page), ts(0)).unwrap());
    let AccountItems::Fills(f) = fills else { panic!() };
    assert_eq!(f.len(), 1);
    assert_eq!((f[0].side.clone(), f[0].qty.to_string(), f[0].price.to_string(), f[0].fee.to_string()), (Mapped::Known(Side::Sell), "100".into(), "1.13030".into(), "0.50000".into()));
    assert_eq!((f[0].quote_ccy.as_deref(), f[0].account.as_deref(), f[0].ts.nanos() % 1_000_000_000), (Some("USD"), Some("001-004-1234567-001"), 655_959_788));
    assert_eq!(next.as_deref(), Some("1:1631536000000:6413-6500"));
    // The last slice ends the walk.
    let reqs = encode_once(&a, &mut s, &cache, &account(AccountKind::Fills, Some("2:1694000000000:")), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h)[1], ("to", "2023-11-14T22:13:20Z"));
    let (_, next) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "transactions", 200, r#"{"pages":[]}"#), ts(0)).unwrap());
    assert!(next.is_none());
    assert!(encode_once(&a, &mut Session::default(), &cache, &account(AccountKind::Fills, Some("garbage")), ts(0)).is_err());
}

#[test]
fn positions_are_one_row_per_side_and_orders_keep_their_trigger() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &account(AccountKind::Positions, None), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.path, "/v3/accounts/{account}/openPositions");
    let body = r#"{"positions":[{"instrument":"EUR_USD","long":{"units":"1000","averagePrice":"1.09000"},"short":{"units":"-500","averagePrice":"1.10000"}},{"instrument":"SPX500_USD","long":{"units":"0"},"short":{"units":"-2","averagePrice":"5000.1"}}],"lastTransactionID":"9"}"#;
    let (rows, _) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "positions", 200, body), ts(0)).unwrap());
    let AccountItems::Positions(p) = rows else { panic!() };
    let got: Vec<_> = p.iter().map(|p| (p.instrument.native.as_str(), p.qty.to_string(), p.avg_price.map(|x| x.to_string()), p.currency.as_deref())).collect();
    assert_eq!(
        got,
        [
            ("EUR_USD", "1000".to_string(), Some("1.09000".to_string()), Some("USD")),
            ("EUR_USD", "-500".to_string(), Some("1.10000".to_string()), Some("USD")),
            ("SPX500_USD", "-2".to_string(), Some("5000.1".to_string()), Some("USD"))
        ]
    );
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &account(AccountKind::OpenOrders, None), ts(0)).unwrap();
    let body = r#"{"orders":[{"id":"1","type":"LIMIT","instrument":"EUR_USD","units":"-100","price":"1.2","createTime":"2024-01-02T10:00:00.000000000Z"},{"id":"2","type":"STOP","instrument":"EUR_USD","units":"100","price":"1.3"},{"id":"3","type":"TAKE_PROFIT","tradeID":"7","price":"1.4"}]}"#;
    let (rows, _) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "orders", 200, body), ts(0)).unwrap());
    let AccountItems::Orders(o) = rows else { panic!() };
    assert_eq!(o.len(), 2, "a take-profit names no instrument");
    assert_eq!((o[0].side.clone(), o[0].limit_price.map(|p| p.to_string()), o[0].stop_price), (Mapped::Known(Side::Sell), Some("1.2".into()), None));
    assert_eq!((o[1].limit_price, o[1].stop_price.map(|p| p.to_string())), (None, Some("1.3".into())));
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &account(AccountKind::Summary, None), ts(0)).unwrap();
    let body = r#"{"account":{"id":"001-004-1234567-001","alias":"Main","balance":"1000.50","currency":"EUR","openTradeCount":2,"pendingOrderCount":1}}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(7, "summary", 200, body), ts(0)).unwrap();
    let Event::AccountSummary(sum) = &evs[0].event else { panic!() };
    assert_eq!(sum.detail.as_deref(), Some("account 001-004-1234567-001 (Main), balance 1000.50 EUR, 2 open trade(s) and 1 pending order(s)"));
    assert_eq!(encode_once(&a, &mut Session::default(), &cache, &account(AccountKind::Balances, None), ts(0)).unwrap_err().kind, ErrorKind::Unsupported);
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..3) {
        let a = m();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let (cmd, corr, endpoint) = match which {
            0 => (hist(Timeframe::M1), 1, "candles"),
            1 => (account(AccountKind::Fills, Some("0:end:1-2")), 7, "idrange"),
            _ => (account(AccountKind::Positions, None), 7, "positions"),
        };
        encode_once(&a, &mut s, &cache, &cmd, ts(0)).unwrap();
        let frame = RawFrame::Http { corr_id: CorrId(corr), endpoint_id: endpoint, status: 200, headers: vec![], body: bytes.into() };
        let _ = decode_once(&a, &mut s, &cache, &frame, ts(0));
    }
}
