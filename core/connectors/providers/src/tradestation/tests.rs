//! TradeStation, offline. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_sdk::{InstrumentCache, NativeRequest, RawFrame};
use otw_connector_testkit::{assert_undeclared_is_unsupported, chunks, decode_once, driver_against, encode_once, http_frame, ts, ManualClock};

use super::*;

fn m() -> TradeStation {
    TradeStation::new().unwrap()
}

fn spy() -> InstrumentId {
    InstrumentId::new(VENUE, "SPY")
}

fn q(h: &HttpRequest) -> Vec<(&str, &str)> {
    h.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

fn account(kind: AccountKind, cursor: Option<&str>) -> Command {
    Command::QueryAccount { req_id: CorrId(7), kind, symbols: vec![], range: Some(TimeRange::new(ts(1_704_067_200), ts(1_704_326_400))), cursor: cursor.map(str::to_string), account: None }
}

fn items(evs: &[Envelope]) -> (AccountItems, Option<String>) {
    match &evs[0].event {
        Event::AccountChunk { items, next_cursor, .. } => (items.clone(), next_cursor.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn capabilities_parse_and_undeclared_is_refused() {
    assert_undeclared_is_unsupported(&m(), &spy());
    assert_eq!(m().capabilities().auth, otw_connector_model::AuthKind::OAuth2Refresh);
}

#[test]
fn an_occ_option_is_written_the_tradestation_way() {
    let a = m();
    assert_eq!(a.instrument_for("AAPL251219C00150000", Some(AssetClass::Option)).unwrap().native, "AAPL 251219C150");
    assert_eq!(a.instrument_for("SPY220916P00412500", Some(AssetClass::Option)).unwrap().native, "SPY 220916P412.5");
    assert_eq!(a.instrument_for("aapl 251219c150", Some(AssetClass::Option)).unwrap().native, "AAPL 251219C150");
    assert_eq!(a.instrument_for("@es", Some(AssetClass::Future)).unwrap().native, "@ES");
    assert!(a.instrument_id("A,B").is_err() && a.instrument_id(" ").is_err());
    assert_eq!(segment("AAPL 251219C150"), "AAPL%20251219C150");
    assert_eq!(segment("$SPX.X,@ES"), "%24SPX.X%2C%40ES");
}

#[test]
fn bars_are_stamped_at_their_open() {
    let a = m();
    let cache = InstrumentCache::new();
    let hist = |tf| Command::RequestHistory { req_id: CorrId(1), instrument: spy(), data_type: DataType::Candles { tf }, range: TimeRange::new(ts(1_704_067_200), ts(1_704_326_400)), cursor: None };
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &hist(Timeframe::M5), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert!(h.signed);
    assert_eq!(h.path, "/v3/marketdata/barcharts/SPY");
    assert_eq!(q(h), [("unit", "Minute"), ("interval", "5"), ("firstdate", "2024-01-01T00:00:00Z"), ("lastdate", "2024-01-04T00:00:00Z")]);
    let body = r#"{"Bars":[
        {"High":"475.2","Low":"474.9","Open":"475.0","Close":"475.1","TimeStamp":"2024-01-02T14:35:00Z","TotalVolume":"120034","BarStatus":"Closed"},
        {"High":"475.3","Low":"475.0","Open":"475.1","Close":"475.2","TimeStamp":"2024-01-02T14:40:00Z","TotalVolume":"9","BarStatus":"Open"}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "bars", 200, body), ts(0)).unwrap();
    let (HistoryItems::Candles(cs), ..) = &chunks(&evs)[0] else { panic!() };
    assert_eq!((cs[0].open_ts, cs[0].is_final, cs[1].is_final), (ts(1_704_205_800), true, false), "a five-minute bar stamped 14:35 opened at 14:30");
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &hist(Timeframe::D1), ts(0)).unwrap();
    let body = r#"{"Bars":[{"High":"1","Low":"1","Open":"1","Close":"1","TimeStamp":"2024-01-02T21:00:00Z","TotalVolume":"1","BarStatus":"Closed"}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(1, "bars", 200, body), ts(0)).unwrap();
    let (HistoryItems::Candles(cs), ..) = &chunks(&evs)[0] else { panic!() };
    assert_eq!(cs[0].open_ts, ts(1_704_153_600), "a session is its date");
    let e = encode_once(&a, &mut Session::default(), &cache, &hist(Timeframe::H1), ts(0)).unwrap_err();
    assert!(e.message.contains("15m"));
}

#[test]
fn symbol_details_carry_the_point_value() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let ids = vec![InstrumentId::new(VENUE, "ESH26"), InstrumentId::new(VENUE, "AAPL 251219C150"), InstrumentId::new(VENUE, "NOPE")];
    let reqs = encode_once(&a, &mut s, &cache, &Command::ResolveInstruments { req_id: CorrId(3), ids }, ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(h.path, "/v3/marketdata/symbols/ESH26%2CAAPL%20251219C150%2CNOPE");
    let body = r#"{"Symbols":[
        {"AssetType":"FUTURE","Currency":"USD","Exchange":"CME","Description":"E-Mini S&P 500","Symbol":"ESH26","PriceFormat":{"Format":"Decimal","PointValue":"50"}},
        {"AssetType":"STOCKOPTION","Currency":"USD","Exchange":"OPRA","Description":"AAPL Dec 2025 150 Call","Symbol":"AAPL 251219C150","PriceFormat":{"PointValue":"100"}},
        {"AssetType":"SOMETHINGNEW","Symbol":"ODD"}],
        "Errors":[{"Symbol":"NOPE","Message":"Invalid symbol."}]}"#;
    let evs = decode_once(&a, &mut s, &cache, &http_frame(3, "symbols", 200, body), ts(0)).unwrap();
    let specs: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some(spec.as_ref().clone()), _ => None }).collect();
    let got: Vec<_> = specs.iter().map(|s| (s.id.native.as_str(), s.asset_class, s.multiplier.to_string(), s.quote.as_str(), a.search_exchange(s))).collect();
    assert_eq!(got, [("ESH26", AssetClass::Future, "50".to_string(), "USD", "CME".to_string()), ("AAPL 251219C150", AssetClass::Option, "100".to_string(), "USD", "OPRA".to_string())]);
    // No free-text search: an empty query asks nothing.
    assert!(encode_once(&a, &mut Session::default(), &cache, &Command::SearchInstruments { req_id: CorrId(4), query: " ".into(), asset_class: None }, ts(0)).unwrap().is_empty());
}

#[test]
fn a_snapshot_is_the_quote_or_the_last_trade() {
    let a = m();
    let cache = InstrumentCache::new();
    let body = r#"{"Quotes":[{"Symbol":"SPY","Bid":"475.10","Ask":"475.12","BidSize":"300","AskSize":"200","Last":"475.11","LastSize":"100","TradeTime":"2024-01-02T15:00:00Z"}]}"#;
    for (dt, last) in [(DataType::Quote, false), (DataType::Trades, true)] {
        let mut s = Session::default();
        let reqs = encode_once(&a, &mut s, &cache, &Command::RequestSnapshot { req_id: CorrId(2), instrument: spy(), data_type: dt }, ts(0)).unwrap();
        let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
        assert_eq!(h.path, "/v3/marketdata/quotes/SPY");
        let evs = decode_once(&a, &mut s, &cache, &http_frame(2, "quotes", 200, body), ts(0)).unwrap();
        match (&evs[0].event, last) {
            (Event::Trade(t), true) => assert_eq!(t.price.to_string(), "475.11"),
            (Event::Quote(q), false) => assert_eq!((q.bid.to_string(), q.ask.to_string()), ("475.10".into(), "475.12".into())),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn fills_are_executed_legs_inside_the_period() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    let reqs = encode_once(&a, &mut s, &cache, &account(AccountKind::Fills, None), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!((h.path.as_str(), q(h)), ("/v3/brokerage/accounts/{account}/historicalorders", vec![("since", "2024-01-01"), ("pageSize", "600")]));
    let body = r#"{"Orders":[
        {"OrderID":"100","Currency":"USD","CommissionFee":"-1.5","ClosedDateTime":"2024-01-02T15:00:00Z","Legs":[
            {"Symbol":"AAPL","BuyOrSell":"SellShort","ExecQuantity":"10","ExecutionPrice":"187.2"},
            {"Symbol":"AAPL 251219C150","BuyOrSell":"BuyToOpen","ExecQuantity":"1","ExecutionPrice":"3.1"},
            {"Symbol":"MSFT","BuyOrSell":"Buy","ExecQuantity":"0"}]},
        {"OrderID":"101","ClosedDateTime":"2023-12-20T15:00:00Z","Legs":[{"Symbol":"OLD","BuyOrSell":"Buy","ExecQuantity":"1"}]}],
        "NextToken":"tok2","Errors":[]}"#;
    let (rows, next) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "fills", 200, body), ts(0)).unwrap());
    let AccountItems::Fills(f) = rows else { panic!() };
    let got: Vec<_> = f.iter().map(|x| (x.id.as_str(), x.side.clone(), x.fee.to_string(), x.fee_asset.as_deref())).collect();
    assert_eq!(got, [("100-0", Mapped::Known(Side::Sell), "1.5".to_string(), Some("USD")), ("100-1", Mapped::Known(Side::Buy), "0".to_string(), Some("USD"))]);
    assert_eq!(next.as_deref(), Some("1:tok2"));
    let reqs = encode_once(&a, &mut s, &cache, &account(AccountKind::Fills, Some("1:tok2")), ts(0)).unwrap();
    let [NativeRequest::Http(h)] = reqs.as_slice() else { panic!() };
    assert_eq!(q(h)[2], ("nextToken", "tok2"));
    let refusal = r#"{"Orders":[],"Errors":[{"AccountID":"123","Error":"Forbidden","Message":"no access"}]}"#;
    let e = decode_once(&a, &mut s, &cache, &http_frame(7, "fills", 200, refusal), ts(0)).unwrap_err();
    assert!(e.kind == ErrorKind::Permission && e.message.contains("123"));
}

#[test]
fn positions_and_orders() {
    let a = m();
    let cache = InstrumentCache::new();
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &account(AccountKind::Positions, None), ts(0)).unwrap();
    let body = r#"{"Positions":[{"Symbol":"AAPL","Quantity":"10","LongShort":"Long","AveragePrice":"180.5","AssetType":"STOCK"},{"Symbol":"ESH26","Quantity":"2","LongShort":"Short","AveragePrice":"4800","AssetType":"FUTURE"}],"Errors":[]}"#;
    let (rows, _) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "positions", 200, body), ts(0)).unwrap());
    let AccountItems::Positions(p) = rows else { panic!() };
    assert_eq!(p.iter().map(|p| p.qty.to_string()).collect::<Vec<_>>(), ["10", "-2"]);
    let mut s = Session::default();
    encode_once(&a, &mut s, &cache, &account(AccountKind::OpenOrders, None), ts(0)).unwrap();
    let body = r#"{"Orders":[{"OrderID":"7","OrderType":"Limit","LimitPrice":"170","OpenedDateTime":"2024-01-02T15:00:00Z","Legs":[{"Symbol":"AAPL","BuyOrSell":"Buy","QuantityOrdered":"5","ExecQuantity":"1"}]},{"OrderID":"8","Legs":[]}]}"#;
    let (rows, _) = items(&decode_once(&a, &mut s, &cache, &http_frame(7, "orders", 200, body), ts(0)).unwrap());
    let AccountItems::Orders(o) = rows else { panic!() };
    assert_eq!((o.len(), o[0].qty.to_string(), o[0].filled_qty.to_string(), o[0].limit_price.map(|p| p.to_string())), (1, "5".into(), "1".into(), Some("170".into())));
}

/// The driver mints the access token once, reuses it, and forgets it on a 401.
#[tokio::test]
async fn the_access_token_is_minted_once_and_forgotten_when_refused() {
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/oauth/token")).and(body_string_contains("grant_type=refresh_token")).and(body_string_contains("refresh_token=r-unique-1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"access_token":"acc-1","expires_in":1200,"token_type":"Bearer"}"#))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/v3/marketdata/quotes/SPY")).and(header("authorization", "Bearer acc-1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"Quotes":[{"Symbol":"SPY","Bid":"1","Ask":"2"}]}"#))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/v3/marketdata/quotes/SPY")).respond_with(ResponseTemplate::new(401).set_body_string("{}")).mount(&server).await;
    let clock = ManualClock::at(ts(1_704_067_200));
    let d = driver_against(m(), &server, clock);
    let secrets: HashMap<String, String> = [("client_id", "cid"), ("client_secret", "cs"), ("refresh_token", "r-unique-1")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let signer = d.mapper().signer(&secrets).unwrap().unwrap();
    let snap = |id| Command::RequestSnapshot { req_id: CorrId(id), instrument: spy(), data_type: DataType::Quote };
    assert!(matches!(d.execute_as(&snap(1), Some(signer.as_ref())).await.unwrap()[0].event, Event::Quote(_)));
    assert!(matches!(d.execute_as(&snap(2), Some(signer.as_ref())).await.unwrap()[0].event, Event::Quote(_)));
    // Refused: the token is dropped and the next call mints again.
    assert!(d.execute_as(&snap(3), Some(signer.as_ref())).await.is_err());
    let _ = d.execute_as(&snap(4), Some(signer.as_ref())).await;
    // A refused refresh token names the fix.
    let other: HashMap<String, String> = [("client_id", "cid"), ("client_secret", "cs"), ("refresh_token", "r-other")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Mock::given(method("POST")).and(path("/oauth/token")).and(body_string_contains("refresh_token=r-other"))
        .respond_with(ResponseTemplate::new(401).set_body_string(r#"{"error":"invalid_grant"}"#))
        .mount(&server)
        .await;
    let s2 = d.mapper().signer(&other).unwrap().unwrap();
    let e = d.execute_as(&snap(5), Some(s2.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("Sign in again"));
}

proptest::proptest! {
    #[test]
    fn decode_survives_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512), which in 0u8..3) {
        let a = m();
        let mut s = Session::default();
        let cache = InstrumentCache::new();
        let (cmd, corr, endpoint) = match which {
            0 => (Command::RequestHistory { req_id: CorrId(1), instrument: spy(), data_type: DataType::Candles { tf: Timeframe::M1 }, range: TimeRange::new(ts(0), ts(60)), cursor: None }, 1, "bars"),
            1 => (account(AccountKind::Fills, None), 7, "fills"),
            _ => (account(AccountKind::Positions, None), 7, "positions"),
        };
        encode_once(&a, &mut s, &cache, &cmd, ts(0)).unwrap();
        let frame = RawFrame::Http { corr_id: CorrId(corr), endpoint_id: endpoint, status: 200, headers: vec![], body: bytes.into() };
        let _ = decode_once(&a, &mut s, &cache, &frame, ts(0));
    }
}
