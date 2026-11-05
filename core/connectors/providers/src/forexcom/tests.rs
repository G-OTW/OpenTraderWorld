//! FOREX.com, offline: the chained calls run through the driver against a local server
//! answering like StoneX. Synthetic fixtures written from the documented schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_testkit::{assert_undeclared_is_unsupported, driver_against, ts, ManualClock};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::*;

fn m() -> ForexCom {
    ForexCom::new().unwrap()
}

fn secrets(user: &str) -> HashMap<String, String> {
    [("username", user), ("password", "pw"), ("app_key", "ak")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn ok(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(body.to_string())
}

/// StoneX: a session per login, an account tree, a market search and three sides of bars.
struct Venue;

impl Respond for Venue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let q = |k: &str| req.url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string()).unwrap_or_default();
        let p = req.url.path().to_string();
        if p == "/session" {
            let body = String::from_utf8_lossy(&req.body).to_string();
            return if body.contains("\"Password\":\"pw\"") { ok(r#"{"Session":"sess-1","StatusCode":1}"#) } else { ResponseTemplate::new(401).set_body_string(r#"{"ErrorMessage":"Invalid Username or Password"}"#) };
        }
        if req.headers.get("session").and_then(|v| v.to_str().ok()) != Some("sess-1") {
            return ResponseTemplate::new(401).set_body_string("{}");
        }
        let multi = req.headers.get("username").and_then(|v| v.to_str().ok()) == Some("multi");
        match p.as_str() {
            "/useraccount/ClientAndTradingAccount" => {
                if multi {
                    ok(r#"{"ClientAccountId":12345.0,"TradingAccounts":[{"TradingAccountId":400.0,"TradingAccountCode":"CFD"},{"TradingAccountId":401.0,"TradingAccountCode":"SB"}]}"#)
                } else {
                    ok(r#"{"ClientAccountId":12345.0,"TradingAccounts":[{"TradingAccountId":400.0,"TradingAccountCode":"CFD"}]}"#)
                }
            }
            "/market/search" => {
                assert_eq!(q("ClientAccountId"), "12345");
                let all = [("401484347", "EUR/USD"), ("401484348", "EUR/USD Mini"), ("400616150", "UK 100"), ("1", "GOLD"), ("2", "GOLD")];
                let rows: Vec<String> = all.iter().filter(|(_, n)| n.to_ascii_lowercase().starts_with(&q("query").to_ascii_lowercase())).map(|(i, n)| format!(r#"{{"MarketId":{i}.0,"Name":"{n}"}}"#)).collect();
                ok(&format!(r#"{{"Markets":[{}]}}"#, rows.join(",")))
            }
            "/market/401484347/barhistorybetween" | "/market/400616150/barhistorybetween" => {
                let d = match q("priceType").as_str() {
                    "BID" => -2,
                    "ASK" => 2,
                    _ => 0,
                };
                let rows: Vec<String> = (0..3).map(|i| {
                    let t = 1_704_204_000_000_i64 + i * 3_600_000;
                    format!(r#"{{"BarDate":"/Date({t})/","Open":1.0{}{},"High":1.09,"Low":1.07,"Close":1.08}}"#, 8 + i, 5 + d)
                }).collect();
                ok(&format!(r#"{{"PriceBars":[{}],"PartialPriceBar":{{"BarDate":"/Date(1704214800000)/","Open":1,"High":1,"Low":1,"Close":1}}}}"#, rows.join(",")))
            }
            "/order/tradehistory" => {
                // 250 trades a minute apart from 1 704 067 200; "changed since" repeats the
                // boundary row on the next page.
                let from = q("from").parse::<i64>().unwrap();
                let rows: Vec<String> = (0..250)
                    .map(|i| (i, 1_704_067_200 + i * 60))
                    .filter(|(_, t)| *t >= from)
                    .take(200)
                    .map(|(i, t)| format!(r#"{{"OrderId":{}.0,"MarketName":"{}","Direction":"{}","OriginalQuantity":{}.0,"Quantity":5.0,"Price":1.08{i:02},"Currency":"USD","ExecutedDateTimeUtc":"/Date({}000)/"}}"#, 9000 + i, if i % 2 == 0 { "EUR/USD" } else { "UK 100" }, if i % 3 == 0 { "sell" } else { "buy" }, 1000 + i, t))
                    .collect();
                ok(&format!(r#"{{"TradeHistory":[{}]}}"#, rows.join(",")))
            }
            "/order/openpositions" => ok(r#"{"OpenPositions":[{"MarketName":"EUR/USD","Quantity":10000.0,"Direction":"sell","Price":1.0812,"Currency":"USD"},{"MarketName":"UK 100","Quantity":2.0,"Direction":"buy","Price":7600.5,"Currency":"GBP"}]}"#),
            "/order/activestoplimitorders" => ok(r#"{"ActiveStopLimitOrders":[{"OrderId":77.0,"MarketName":"EUR/USD","Type":2,"TriggerPrice":1.05,"Direction":"buy","Quantity":1000.0,"Currency":"USD","CreatedDateTimeUTC":"/Date(1704204000000)/"},{"OrderId":78.0,"MarketName":"UK 100","Type":1,"TriggerPrice":7500,"Direction":"sell","Quantity":1.0,"Currency":"GBP"}]}"#),
            _ => ResponseTemplate::new(404),
        }
    }
}

async fn venue() -> (MockServer, otw_connector_sdk::Driver<ForexCom>) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(Venue).mount(&server).await;
    let d = driver_against(m(), &server, ManualClock::at(ts(1_704_067_200)));
    (server, d)
}

#[test]
fn capabilities_parse_and_dates_and_classes() {
    assert_undeclared_is_unsupported(&m(), &InstrumentId::new(VENUE, "EUR/USD"));
    assert_eq!(wcf("/Date(1289231327280)/"), Timestamp::from_millis(1_289_231_327_280));
    assert_eq!(wcf("/Date(1289231327280+0000)/"), Timestamp::from_millis(1_289_231_327_280));
    assert!(wcf("2010-11-08T13:08:47Z").is_none() && wcf("/Date(x)/").is_none());
    assert_eq!((class_of("EUR/USD"), class_of("UK 100"), class_of("APPLE INC/CFD")), (AssetClass::Forex, AssetClass::Cfd, AssetClass::Cfd));
    assert_eq!(int_text(&OptDec(Some(Decimal::new(4014843470, 1)))).as_deref(), Some("401484347"));
    assert!(m().check_settings(&[("trading_account_id".to_string(), "abc".to_string())].into()).is_err());
}

#[tokio::test]
async fn bars_resolve_the_name_then_read_three_sides() {
    let (_server, d) = venue().await;
    let signer = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let cmd = |native: &str| Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(VENUE, native),
        data_type: DataType::Candles { tf: Timeframe::H1 },
        range: TimeRange::new(ts(1_704_200_000), ts(1_704_300_000)),
        cursor: None,
    };
    for native in ["EUR/USD", "401484347"] {
        let evs = d.execute_as(&cmd(native), Some(signer.as_ref())).await.unwrap();
        let Event::HistoryChunk { items: HistoryItems::Candles(cs), .. } = &evs[0].event else { panic!("{evs:?}") };
        assert_eq!(cs.len(), 3, "{native}: the partial bar is left out");
        let s = cs[0].sides.unwrap();
        assert_eq!((cs[0].open.to_string(), s.bid.open.to_string(), s.ask.open.to_string()), ("1.085".into(), "1.083".into(), "1.087".into()));
        assert!(cs[0].is_final && cs[0].volume.0.is_zero());
    }
    // A name two markets carry is refused with both ids.
    let e = d.execute_as(&cmd("GOLD"), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.message.contains("GOLD = 1") && e.message.contains("GOLD = 2"), "{}", e.message);
    let e = d.execute_as(&cmd("NOPE"), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"));
}

#[tokio::test]
async fn search_lines_carry_the_market_id() {
    let (_server, d) = venue().await;
    let signer = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let evs = d.execute_as(&Command::SearchInstruments { req_id: CorrId(2), query: "eur".into(), asset_class: None }, Some(signer.as_ref())).await.unwrap();
    let names: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some((spec.id.native.clone(), spec.symbol.0.clone(), spec.asset_class)), _ => None }).collect();
    assert_eq!(names, [("EUR/USD".into(), "EUR/USD (market 401484347)".into(), AssetClass::Forex), ("EUR/USD MINI".into(), "EUR/USD Mini (market 401484348)".into(), AssetClass::Forex)]);
}

#[tokio::test]
async fn the_account_is_picked_never_guessed() {
    let (_server, d) = venue().await;
    let q = |kind, account: Option<&str>| Command::QueryAccount { req_id: CorrId(3), kind, symbols: vec![], range: Some(TimeRange::new(ts(1_704_067_200), ts(1_704_100_000))), cursor: None, account: account.map(str::to_string) };
    let one = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let evs = d.execute_as(&q(AccountKind::Summary, None), Some(one.as_ref())).await.unwrap();
    let Event::AccountSummary(s) = &evs[0].event else { panic!() };
    assert_eq!(s.detail.as_deref(), Some("signed in as client 12345, reading trading account 400 of [CFD (400)]"));
    let multi = d.mapper().signer(&secrets("multi")).unwrap().unwrap();
    let e = d.execute_as(&q(AccountKind::Positions, None), Some(multi.as_ref())).await.unwrap_err();
    assert!(e.message.contains("CFD (400)") && e.message.contains("SB (401)"));
    assert!(d.execute_as(&q(AccountKind::Positions, Some("999")), Some(multi.as_ref())).await.is_err());
    let evs = d.execute_as(&q(AccountKind::Positions, Some("401")), Some(multi.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    assert_eq!(p.iter().map(|p| (p.qty.to_string(), p.class, p.account.clone())).collect::<Vec<_>>(), [("-10000.0".to_string(), Some(AssetClass::Forex), Some("401".to_string())), ("2.0".to_string(), Some(AssetClass::Cfd), Some("401".to_string()))]);
    let evs = d.execute_as(&q(AccountKind::OpenOrders, None), Some(one.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].order_type.as_str(), o[0].limit_price.map(|p| p.to_string()), o[1].order_type.as_str(), o[1].stop_price.map(|p| p.to_string())), ("limit", Some("1.05".into()), "stop", Some("7500".into())));
}

#[tokio::test]
async fn the_trade_walk_moves_past_the_newest_row() {
    let (server, d) = venue().await;
    let one = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let cmd = Command::QueryAccount { req_id: CorrId(4), kind: AccountKind::Fills, symbols: vec![], range: Some(TimeRange::new(ts(1_704_067_200), ts(1_704_067_200 + 249 * 60))), cursor: None, account: None };
    let evs = d.execute_as(&cmd, Some(one.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!() };
    assert_eq!(f.len(), 250, "two pages, the repeated boundary row read once");
    assert_eq!((f[0].side.clone(), f[0].qty.to_string(), f[0].fee.0.is_zero(), f[1].class), (Mapped::Known(Side::Sell), "1000.0".into(), true, Some(AssetClass::Cfd)));
    let trades = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/order/tradehistory").count();
    assert_eq!(trades, 2);
}

#[tokio::test]
async fn the_session_is_opened_once_and_dropped_when_ended() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/session")).and(body_string_contains("\"UserName\":\"solo\""))
        .respond_with(ok(r#"{"Session":"s-solo"}"#)).expect(2).mount(&server).await;
    Mock::given(method("GET")).and(path("/useraccount/ClientAndTradingAccount")).and(header("session", "s-solo")).and(header("username", "solo"))
        .respond_with(ok(r#"{"ClientAccountId":1.0,"TradingAccounts":[{"TradingAccountId":5.0,"TradingAccountCode":"A"}]}"#)).up_to_n_times(2).mount(&server).await;
    Mock::given(method("GET")).and(path("/useraccount/ClientAndTradingAccount")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let d = driver_against(m(), &server, ManualClock::at(ts(1_704_067_200)));
    let signer = d.mapper().signer(&secrets("solo")).unwrap().unwrap();
    let q = |id| Command::QueryAccount { req_id: CorrId(id), kind: AccountKind::Summary, symbols: vec![], range: None, cursor: None, account: None };
    d.execute_as(&q(1), Some(signer.as_ref())).await.unwrap();
    d.execute_as(&q(2), Some(signer.as_ref())).await.unwrap();
    let e = d.execute_as(&q(3), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.message.contains("ended this session"));
    let _ = d.execute_as(&q(4), Some(signer.as_ref())).await;
    let wrong = [("username", "solo2"), ("password", "x"), ("app_key", "ak")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Mock::given(method("POST")).and(path("/session")).respond_with(ResponseTemplate::new(401).set_body_string("{}")).mount(&server).await;
    let s2 = d.mapper().signer(&wrong).unwrap().unwrap();
    let e = d.execute_as(&q(5), Some(s2.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("refused the sign-in"));
}
