//! Capital.com, offline: the session, the account switch, candles, quotes, search and the
//! account reads run through the driver against a local server answering like Capital.com;
//! the socket mapping runs on its own. Synthetic fixtures written from the documented schema
//! (the docs' own worked examples where they give one).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, DataType, Event, HistoryItems, InstrumentId, Mapped, Side, TimeRange, Timeframe};
use otw_connector_model::ConnectorId;
use otw_connector_testkit::{assert_undeclared_is_unsupported, driver_against, ts, ManualClock};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::*;

fn m() -> CapitalCom {
    CapitalCom::new().unwrap()
}

fn secrets(login: &str, account: Option<&str>) -> HashMap<String, String> {
    let mut s: HashMap<String, String> = [("api_key", "key-1"), ("identifier", login), ("api_password", "pw")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    if let Some(a) = account {
        s.insert("account_id".into(), a.into());
    }
    s
}

fn ok(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(body.to_string())
}

fn price(t: &str, o: &str, spread: &str) -> String {
    let d = |x: &str| x.parse::<Decimal>().unwrap();
    let (o, spread) = (d(o), d(spread));
    let two = |p: Decimal| format!(r#"{{"bid":{p},"ask":{}}}"#, p + spread);
    format!(
        r#"{{"snapshotTime":"{t}","snapshotTimeUTC":"{t}","openPrice":{},"closePrice":{},"highPrice":{},"lowPrice":{},"lastTradedVolume":187}}"#,
        two(o),
        two(o + d("0.02")),
        two(o + d("0.03")),
        two(o - d("0.01"))
    )
}

/// Capital.com: a session in two headers, a switch to another account, prices, markets and
/// the account reads. Two financial accounts: `A1` (preferred) and `A2`.
struct Venue;

impl Respond for Venue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let q = |k: &str| req.url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string()).unwrap_or_default();
        let h = |k: &str| req.headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let p = req.url.path().to_string();
        if h("x-cap-api-key") != "key-1" {
            return ResponseTemplate::new(401).set_body_string(r#"{"errorCode":"error.invalid.api.key"}"#);
        }
        if p == "/api/v1/session" && req.method.as_str() == "POST" {
            let body = String::from_utf8_lossy(&req.body).to_string();
            if !body.contains("\"password\":\"pw\"") || !body.contains("\"encryptedPassword\":false") {
                return ResponseTemplate::new(401).set_body_string(r#"{"errorCode":"error.invalid.details"}"#);
            }
            return ok(r#"{"currentAccountId":"A1"}"#).insert_header("CST", "cst-1").insert_header("X-SECURITY-TOKEN", "sec-A1");
        }
        if h("cst") != "cst-1" {
            return ResponseTemplate::new(401).set_body_string(r#"{"errorCode":"error.invalid.session.token"}"#);
        }
        if p == "/api/v1/session" && req.method.as_str() == "PUT" {
            let body = String::from_utf8_lossy(&req.body).to_string();
            return match (h("x-security-token").as_str(), body.contains("\"accountId\":\"A2\""), body.contains("\"accountId\":\"A1\"")) {
                ("sec-A1", true, _) => ok(r#"{"trailingStopsEnabled":false}"#).insert_header("X-SECURITY-TOKEN", "sec-A2"),
                ("sec-A1", _, true) => ResponseTemplate::new(400).set_body_string(r#"{"errorCode":"error.not-different.accountId"}"#),
                _ => ResponseTemplate::new(400).set_body_string(r#"{"errorCode":"error.invalid.accountId"}"#),
            };
        }
        let account = match h("x-security-token").as_str() {
            "sec-A1" => "A1",
            "sec-A2" => "A2",
            _ => return ResponseTemplate::new(401).set_body_string("{}"),
        };
        match p.as_str() {
            "/api/v1/accounts" => ok(r#"{"accounts":[{"accountId":"A1","accountName":"USD account","status":"ENABLED","accountType":"CFD","preferred":true,"balance":{"balance":1000.5,"deposit":1000,"profitLoss":0,"available":900},"currency":"USD"},{"accountId":"A2","accountName":"EUR account","preferred":false,"balance":{"balance":250},"currency":"EUR"}]}"#),
            "/api/v1/prices/SILVER" => {
                assert_eq!((q("resolution").as_str(), q("max").as_str()), ("MINUTE", "1000"));
                let rows = [price("2022-04-06T13:18:00", "24.356", "0.02"), price("2022-04-06T13:19:00", "24.40", "0.02"), price("2022-04-06T13:20:00", "24.45", "0.02")];
                ok(&format!(r#"{{"prices":[{}],"instrumentType":"COMMODITIES"}}"#, rows.join(",")))
            }
            "/api/v1/prices/WEEKEND" => ResponseTemplate::new(404).set_body_string(r#"{"errorCode":"error.prices.not-found"}"#),
            "/api/v1/prices/NOPE" => ResponseTemplate::new(404).set_body_string(r#"{"errorCode":"error.not-found.epic"}"#),
            "/api/v1/markets/GOLD" => ok(r#"{"instrument":{"epic":"GOLD","name":"Gold","type":"COMMODITIES","currency":"USD"},"snapshot":{"bid":1950.5,"offer":1951.0}}"#),
            "/api/v1/markets/NOPE" => ResponseTemplate::new(404).set_body_string(r#"{"errorCode":"error.not-found.epic"}"#),
            "/api/v1/markets" if !q("searchTerm").is_empty() => ok(r#"{"markets":[{"epic":"GOLD","instrumentName":"Gold","instrumentType":"COMMODITIES"},{"epic":"EURUSD","instrumentName":"EUR/USD","instrumentType":"CURRENCIES"},{"epic":"US500","instrumentName":"US 500","instrumentType":"INDICES"},{"epic":"BTCUSD","instrumentName":"Bitcoin/USD","instrumentType":"CRYPTOCURRENCIES"}]}"#),
            "/api/v1/markets" => {
                let all = [("GOLD", "Gold", "COMMODITIES", "USD"), ("EURUSD", "EUR/USD", "CURRENCIES", "USD"), ("OIL_CRUDE", "Crude Oil", "COMMODITIES", "USD")];
                let wanted: Vec<String> = q("epics").split(',').map(str::to_string).collect();
                let rows: Vec<String> = all
                    .iter()
                    .filter(|(e, ..)| wanted.iter().any(|w| w == e))
                    .map(|(e, n, t, c)| format!(r#"{{"instrument":{{"epic":"{e}","name":"{n}","type":"{t}","currency":"{c}"}},"snapshot":{{"bid":1,"offer":1.1}}}}"#))
                    .collect();
                ok(&format!(r#"{{"marketDetails":[{}]}}"#, rows.join(",")))
            }
            "/api/v1/positions" => ok(&format!(
                r#"{{"positions":[{{"position":{{"contractSize":1,"dealId":"d1","size":{},"direction":"BUY","level":1.0812,"currency":"USD","upl":3.5}},"market":{{"epic":"EURUSD","instrumentType":"CURRENCIES"}}}},{{"position":{{"contractSize":10,"dealId":"d2","size":2,"direction":"SELL","level":1950.25,"currency":"USD"}},"market":{{"epic":"GOLD","instrumentType":"COMMODITIES"}}}},{{"position":{{"size":0,"direction":"BUY","level":1}},"market":{{"epic":"US500","instrumentType":"INDICES"}}}}]}}"#,
                if account == "A2" { 500 } else { 1000 }
            )),
            "/api/v1/workingorders" => ok(r#"{"workingOrders":[{"workingOrderData":{"dealId":"w1","direction":"BUY","epic":"EURUSD","orderSize":1000,"orderLevel":1.05,"orderType":"LIMIT","currencyCode":"USD","createdDateUTC":"2022-01-03T15:01:34.228"},"marketData":{"instrumentType":"CURRENCIES"}},{"workingOrderData":{"dealId":"w2","direction":"SELL","epic":"GOLD","orderSize":1,"orderLevel":1900,"orderType":"STOP","currencyCode":"USD"},"marketData":{"instrumentType":"COMMODITIES"}}]}"#),
            "/api/v1/history/activity" => {
                assert_eq!((q("detailed").as_str(), q("filter").as_str()), ("true", "type==POSITION;status==ACCEPTED"));
                // One deal per day, opened and closed out, plus a row with no money in it.
                let day = q("from").get(..10).unwrap_or_default().to_string();
                ok(&format!(
                    r#"{{"activities":[
                        {{"dateUTC":"{day}T15:01:34.228","epic":"OIL_CRUDE","dealId":"deal-{day}","source":"USER","type":"POSITION","status":"ACCEPTED","details":{{"currency":"USD","size":1,"direction":"BUY","level":75.31}}}},
                        {{"dateUTC":"{day}T15:01:48.420","epic":"OIL_CRUDE","dealId":"deal-{day}","source":"CLOSE_OUT","type":"POSITION","status":"ACCEPTED","details":{{"currency":"USD","size":1,"direction":"SELL","level":75.2}}}},
                        {{"dateUTC":"{day}T15:01:48.536","epic":"OIL_CRUDE","dealId":"x","type":"POSITION","details":{{"size":0,"direction":"SELL","level":75.31}}}}]}}"#
                ))
            }
            _ => ResponseTemplate::new(404).set_body_string("{}"),
        }
    }
}

async fn venue() -> (MockServer, otw_connector_sdk::Driver<CapitalCom>) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(Venue).mount(&server).await;
    let d = driver_against(m(), &server, ManualClock::at(ts(1_649_250_000)));
    (server, d)
}

fn signer(login: &str, account: Option<&str>) -> Arc<dyn Signer> {
    m().signer(&secrets(login, account)).unwrap().unwrap()
}

fn query(kind: AccountKind, range: Option<TimeRange>, account: Option<&str>) -> Command {
    Command::QueryAccount { req_id: CorrId(3), kind, symbols: vec![], range, cursor: None, account: account.map(str::to_string) }
}

#[test]
fn capabilities_parse_and_stamps_and_classes() {
    assert_undeclared_is_unsupported(&m(), &InstrumentId::new(VENUE, "GOLD"));
    let t = parse_utc("2022-01-17T15:09:47.344").unwrap();
    assert_eq!(t.millis(), 1_642_432_187_344);
    assert_eq!(parse_utc("2022-01-17T15:09:47Z"), parse_utc("2022-01-17T15:09:47"));
    assert!(parse_utc("17/01/2022").is_none());
    assert_eq!(query_stamp(t).unwrap(), "2022-01-17T15:09:47");
    assert_eq!((class_of("CURRENCIES"), class_of("SHARES"), class_of("CRYPTOCURRENCIES")), (AssetClass::Forex, AssetClass::Cfd, AssetClass::Cfd));
    assert_eq!((resolution(Timeframe::H4), resolution(Timeframe::M30)), (Some("HOUR_4"), None));
    assert_eq!(m().instrument_id(" oil_crude ").unwrap().native, "OIL_CRUDE");
    assert!(m().instrument_id("EUR/USD").is_err() && m().instrument_id("").is_err());
}

#[tokio::test]
async fn a_candle_is_the_mid_with_both_sides() {
    let (_server, d) = venue().await;
    let s = signer("one", None);
    let cmd = |epic: &str, to: i64| Command::RequestHistory {
        req_id: CorrId(1),
        instrument: InstrumentId::new(VENUE, epic),
        data_type: DataType::Candles { tf: Timeframe::M1 },
        range: TimeRange::new(ts(1_649_251_080), ts(to)),
        cursor: None,
    };
    let evs = d.execute_as(&cmd("SILVER", 1_649_251_200), Some(s.as_ref())).await.unwrap();
    let Event::HistoryChunk { items: HistoryItems::Candles(cs), .. } = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(cs.len(), 2, "the window is [from, to)");
    // The docs' worked example: open 24.356 / 24.376.
    let c = &cs[0];
    assert_eq!((c.open_ts, c.open.to_string(), c.low.to_string(), c.volume.to_string()), (ts(1_649_251_080), "24.366".into(), "24.356".into(), "187".into()));
    let sides = c.sides.unwrap();
    assert_eq!((sides.bid.open.to_string(), sides.ask.open.to_string()), ("24.356".into(), "24.376".into()));
    assert!(!c.is_final, "the period's end decides");
    // A window with no prices is empty, an unknown epic is a symbol fault.
    let evs = d.execute_as(&cmd("WEEKEND", 1_649_251_200), Some(s.as_ref())).await.unwrap();
    assert!(matches!(&evs[0].event, Event::HistoryChunk { items: HistoryItems::Candles(c), .. } if c.is_empty()));
    let e = d.execute_as(&cmd("NOPE", 1_649_251_200), Some(s.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::NotFound && e.message.contains("bad symbol"), "{}", e.message);
}

#[test]
fn one_side_alone_is_still_the_venues_number() {
    let two = |bid: Option<i64>, ask: Option<i64>| Some(Two { bid: OptDec(bid.map(Decimal::from)), ask: OptDec(ask.map(Decimal::from)) });
    assert_eq!(mid(two(Some(10), None)), Some(Decimal::from(10)));
    assert_eq!(mid(two(None, Some(12))), Some(Decimal::from(12)));
    assert_eq!(mid(two(None, None)), None);
}

#[tokio::test]
async fn quotes_search_and_details() {
    let (_server, d) = venue().await;
    let s = signer("one", None);
    let snap = |epic: &str| Command::RequestSnapshot { req_id: CorrId(2), instrument: InstrumentId::new(VENUE, epic), data_type: DataType::Quote };
    let evs = d.execute_as(&snap("GOLD"), Some(s.as_ref())).await.unwrap();
    let Event::Quote(q) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!((q.bid.to_string(), q.ask.to_string()), ("1950.5".into(), "1951.0".into()));
    assert!(d.execute_as(&snap("NOPE"), Some(s.as_ref())).await.unwrap().is_empty(), "no market, no price");
    let evs = d.execute_as(&Command::SearchInstruments { req_id: CorrId(3), query: "go".into(), asset_class: None }, Some(s.as_ref())).await.unwrap();
    let hits: Vec<_> = evs.iter().filter_map(|e| match &e.event { Event::Instrument { spec } => Some((spec.id.native.clone(), spec.symbol.0.clone(), spec.asset_class, d.mapper().search_exchange(spec))), _ => None }).collect();
    assert_eq!(hits[0], ("GOLD".into(), "Gold".into(), AssetClass::Equity, "Capital.com COMMODITIES".into()));
    assert_eq!(hits.iter().map(|h| h.2).collect::<Vec<_>>(), [AssetClass::Equity, AssetClass::Forex, AssetClass::Index, AssetClass::Spot]);
    let found = d.resolve_instruments(CorrId(4), &[InstrumentId::new(VENUE, "EURUSD"), InstrumentId::new(VENUE, "MISSING")], Some(s.as_ref())).await.unwrap();
    assert_eq!(found.iter().map(|f| (f.id.native.clone(), f.quote.clone(), f.asset_class)).collect::<Vec<_>>(), [("EURUSD".into(), "USD".into(), AssetClass::Forex)]);
}

#[tokio::test]
async fn the_session_switches_to_the_account_asked_for() {
    let (server, d) = venue().await;
    let evs = d.execute_as(&query(AccountKind::Summary, None, None), Some(signer("pref", None).as_ref())).await.unwrap();
    let Event::AccountSummary(a) = &evs[0].event else { panic!() };
    assert_eq!(a.detail.as_deref(), Some("sign-in accepted, 2 account(s); reading USD account (USD), balance 1000.5"));
    // A2: the session moves there and reads it with the fresh security token.
    let s2 = signer("other", Some("A2"));
    let evs = d.execute_as(&query(AccountKind::Summary, None, Some("A2")), Some(s2.as_ref())).await.unwrap();
    let Event::AccountSummary(a) = &evs[0].event else { panic!() };
    assert_eq!(a.detail.as_deref(), Some("sign-in accepted, 2 account(s); reading EUR account (EUR), balance 250"));
    let evs = d.execute_as(&query(AccountKind::Positions, None, Some("A2")), Some(s2.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    assert_eq!(p[0].qty.to_string(), "500", "read on A2");
    // Asking for the account the session is already on is not a refusal.
    d.execute_as(&query(AccountKind::Summary, None, Some("A1")), Some(signer("same", Some("A1")).as_ref())).await.unwrap();
    let e = d.execute_as(&query(AccountKind::Summary, None, Some("ZZ")), Some(signer("bad", Some("ZZ")).as_ref())).await.unwrap_err();
    assert!(e.message.contains("would not switch to account ZZ"), "{}", e.message);
    // One sign-in per login, reused.
    d.execute_as(&query(AccountKind::Summary, None, None), Some(signer("pref", None).as_ref())).await.unwrap();
    let posts = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/api/v1/session" && r.method.as_str() == "POST").count();
    assert_eq!(posts, 4, "pref, other, same, bad");
}

#[tokio::test]
async fn a_refused_sign_in_names_the_fix() {
    let (_server, d) = venue().await;
    let mut wrong = secrets("wrong", None);
    wrong.insert("api_password".into(), "nope".into());
    let s = m().signer(&wrong).unwrap().unwrap();
    let e = d.execute_as(&query(AccountKind::Summary, None, None), Some(s.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("custom password"), "{}", e.message);
    assert!(m().signer(&HashMap::new()).is_err());
}

#[tokio::test]
async fn positions_and_orders() {
    let (_server, d) = venue().await;
    let s = signer("one", None);
    let evs = d.execute_as(&query(AccountKind::Positions, None, None), Some(s.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    let rows: Vec<_> = p.iter().map(|p| (p.instrument.native.clone(), p.qty.to_string(), p.class, p.multiplier.map(|m| m.to_string()))).collect();
    assert_eq!(rows, [("EURUSD".into(), "1000".into(), Some(AssetClass::Forex), Some("1".into())), ("GOLD".into(), "-2".into(), Some(AssetClass::Cfd), Some("10".into()))]);
    let evs = d.execute_as(&query(AccountKind::OpenOrders, None, None), Some(s.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].limit_price.map(|p| p.to_string()), o[0].stop_price, o[0].placed_at.map(|t| t.millis())), (Some("1.05".into()), None, Some(1_641_222_094_228)));
    assert_eq!((o[1].order_type.as_str(), o[1].stop_price.map(|p| p.to_string()), o[1].side.clone()), ("STOP", Some("1900".into()), Mapped::Known(Side::Sell)));
}

#[tokio::test]
async fn the_activity_log_is_walked_a_day_at_a_time() {
    let (server, d) = venue().await;
    let s = signer("one", None);
    // 2022-01-03 00:00 to 2022-01-05 12:00: three windows.
    let range = TimeRange::new(ts(1_641_168_000), ts(1_641_168_000 + 2 * 86_400 + 43_200));
    let evs = d.execute_as(&query(AccountKind::Fills, Some(range), None), Some(s.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!() };
    assert_eq!(f.len(), 6, "two fills a day, the row with no money left out");
    assert_eq!(f[0].id, "deal-2022-01-03:2022-01-03T15:01:34.228:buy");
    assert_ne!(f[0].id, f[1].id, "two fills of one deal are two identities");
    assert_eq!((f[1].side.clone(), f[1].price.to_string(), f[1].quote_ccy.as_deref(), f[1].class), (Mapped::Known(Side::Sell), "75.2".into(), Some("USD"), Some(AssetClass::Cfd)));
    let calls = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/api/v1/history/activity").count();
    assert_eq!(calls, 3);
    let long = TimeRange::new(ts(1_600_000_000), ts(1_600_000_000 + 401 * 86_400));
    let e = d.execute_as(&query(AccountKind::Fills, Some(long), None), Some(s.as_ref())).await.unwrap_err();
    assert!(e.message.contains("400 days or fewer"), "{}", e.message);
}

fn ctx_at(secs: i64) -> (InstrumentCache, ConnectorId, Timestamp) {
    (InstrumentCache::new(), ConnectorId("capitalcom".into()), ts(secs))
}

use otw_connector_sdk::InstrumentCache;

#[test]
fn the_socket_signs_every_frame_and_waits_for_both_sides() {
    let (cache, connector, now) = ctx_at(1_671_714_000);
    let ctx = MapCtx { received_ts: now, instruments: &cache, connector: &connector };
    let mut s = Session::default();
    let sub = |id: u64, epic: &str, tf: Timeframe| Command::Subscribe { sub_id: CorrId(id), instrument: InstrumentId::new(VENUE, epic), data_type: DataType::Candles { tf } };
    let reqs = m().encode_batch(&[sub(1, "GOLD", Timeframe::M5), sub(2, "US500", Timeframe::M5), sub(3, "GOLD", Timeframe::H1)], &mut s, &ctx).unwrap();
    assert!(matches!(&reqs[0], NativeRequest::WsOpen { path, .. } if path == "/connect"));
    let frames: Vec<String> = reqs[1..].iter().map(|r| match r { NativeRequest::WsSigned { payload, .. } => String::from_utf8(payload.to_vec()).unwrap(), other => panic!("{other:?}") }).collect();
    assert_eq!(frames.len(), 2, "one frame per resolution");
    assert!(frames[0].contains("MINUTE_5") && frames[0].contains("GOLD") && frames[0].contains("US500"));
    // The signer writes the tokens on the frame; the mapper never does.
    assert!(!frames[0].contains("cst"));
    let key = credential_key(&["capitalcom-test-frame"]);
    keep_token(&key, "c\ns".into(), now, 600);
    let signer = CapitalComSigner { api_key: "k".to_string().into(), identifier: "i".into(), password: "p".to_string().into(), account: None, key: key.clone(), login_key: key };
    let signed = String::from_utf8(signer.sign_frame(frames[0].clone().into(), now).unwrap().to_vec()).unwrap();
    assert!(signed.contains("\"cst\":\"c\"") && signed.contains("\"securityToken\":\"s\"") && signed.contains("OHLCMarketData.subscribe"));

    let ev = |side: &str, t: i64, c: f64| format!(r#"{{"status":"OK","destination":"ohlc.event","payload":{{"resolution":"MINUTE_5","epic":"GOLD","type":"classic","priceType":"{side}","t":{t},"h":{c},"l":{c},"o":{c},"c":{c}}}}}"#);
    let frame = |text: String| RawFrame::Ws { conn_key: otw_connector_sdk::ConnKey("main".into()), payload: text.into() };
    assert!(m().decode(&frame(ev("bid", 1_671_714_000_000, 134.0)), &mut s, &ctx).unwrap().is_empty(), "the bid alone is half a spread off");
    let out = m().decode(&frame(ev("ask", 1_671_714_000_000, 135.0)), &mut s, &ctx).unwrap();
    let Event::Candle(c) = &out[0].event else { panic!("{out:?}") };
    assert_eq!((out[0].corr_id, c.close.to_string(), c.is_final, c.sides.unwrap().ask.close.to_string()), (Some(CorrId(1)), "134.5".into(), false, "135".into()));
    // A newer period drops the older sides.
    m().decode(&frame(ev("bid", 1_671_714_300_000, 136.0)), &mut s, &ctx).unwrap();
    m().decode(&frame(ev("ask", 1_671_714_300_000, 137.0)), &mut s, &ctx).unwrap();
    assert_eq!(s.live.sides.len(), 1);

    // A refused epic faults its own pane.
    let ack = r#"{"status":"OK","destination":"OHLCMarketData.subscribe","correlationId":"3","payload":{"subscriptions":{"GOLD:MINUTE_5:classic":"PROCESSED","US500:MINUTE_5:classic":"ERROR"}}}"#;
    let out = m().decode(&frame(ack.into()), &mut s, &ctx).unwrap();
    assert!(matches!(&out[..], [e] if e.corr_id == Some(CorrId(2)) && matches!(&e.event, Event::SubscriptionFail { error } if error.kind == ErrorKind::NotFound)));
    // A refused token ends the socket as an auth fault.
    let bad = r#"{"status":"FAILED","destination":"OHLCMarketData.subscribe.error","payload":{"errorCode":"error.invalid.session.token"}}"#;
    assert_eq!(m().decode(&frame(bad.into()), &mut s, &ctx).unwrap_err().kind, ErrorKind::Auth);

    // A ping after four quiet minutes, not before; unsubscribing sends one frame.
    assert!(m().keepalive(&mut s, &ctx).is_empty());
    let later = MapCtx { received_ts: ts(1_671_714_000 + 241), instruments: &cache, connector: &connector };
    assert!(matches!(&m().keepalive(&mut s, &later)[..], [NativeRequest::WsSigned { payload, .. }] if String::from_utf8_lossy(payload).contains("\"destination\":\"ping\"")));
    let reqs = m().encode_batch(&[Command::Unsubscribe { sub_id: CorrId(1) }], &mut s, &ctx).unwrap();
    assert!(matches!(&reqs[..], [NativeRequest::WsSigned { payload, .. }] if String::from_utf8_lossy(payload).contains("OHLCMarketData.unsubscribe")));
}
