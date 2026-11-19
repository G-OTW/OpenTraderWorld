//! NinjaTrader, offline: the chained calls run through the driver against a local server
//! answering like the Tradovate platform. Synthetic fixtures written from the documented
//! entity schema.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, Event, InstrumentId, Mapped, Side, TimeRange};
use otw_connector_testkit::{assert_undeclared_is_unsupported, driver_against, ts, ManualClock};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::*;

fn m() -> NinjaTrader {
    NinjaTrader::new().unwrap()
}

fn secrets(user: &str) -> HashMap<String, String> {
    [("username", user), ("password", "pw"), ("cid", "7"), ("sec", "s3c")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn ok(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(body.to_string())
}

/// 2024-01-01 00:00 UTC.
const T0: i64 = 1_704_067_200;

fn iso(t: i64) -> String {
    format!("2024-01-01T{:02}:{:02}:{:02}.000Z", (t - T0) / 3600, (t - T0) % 3600 / 60, (t - T0) % 60)
}

/// One login per user name: `one` carries account 42, `multi` carries 42 and 43, and a
/// closed 44. Two contracts: ESZ5 (50 USD a point) and a spread with no currency answer.
struct Venue;

impl Respond for Venue {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let q = |k: &str| req.url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string()).unwrap_or_default();
        let p = req.url.path().to_string();
        if p == "/auth/accesstokenrequest" {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!((body["cid"].as_i64(), body["appId"].as_str()), (Some(7), Some("OpenTraderWorld")));
            if body["password"] != "pw" {
                return ResponseTemplate::new(401).set_body_string(r#"{"errorText":"Incorrect username or password"}"#);
            }
            if body["name"] == "terms" {
                return ok(r#"{"errorText":"Please accept the terms","p-ticket":""}"#);
            }
            return ok(&format!(r#"{{"accessToken":"tok-{}","expirationTime":"2024-01-01T01:30:00.000Z"}}"#, body["name"].as_str().unwrap()));
        }
        let auth = req.headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let Some(user) = auth.strip_prefix("Bearer tok-") else {
            return ResponseTemplate::new(401).set_body_string("Access is denied");
        };
        match p.as_str() {
            "/account/list" => ok(if user == "multi" {
                r#"[{"id":42,"name":"APEX-1"},{"id":43,"name":"DEMO"},{"id":44,"name":"OLD","closed":true}]"#
            } else {
                r#"[{"id":42,"name":"APEX-1"}]"#
            }),
            "/order/list" => ok(&format!(
                r#"[{{"id":9,"accountId":42,"action":"Buy","contractId":77,"ordStatus":"Filled","timestamp":"{}"}},
                   {{"id":10,"accountId":42,"action":"Sell","contractId":77,"ordStatus":"Working","timestamp":"{}"}},
                   {{"id":11,"accountId":43,"action":"Buy","contractId":78,"ordStatus":"PendingNew","timestamp":"{}"}},
                   {{"id":12,"accountId":42,"action":"Buy","contractId":78,"ordStatus":"Canceled"}}]"#,
                iso(T0 + 60),
                iso(T0 + 120),
                iso(T0 + 180)
            )),
            "/orderVersion/list" => ok(r#"[{"id":100,"orderId":10,"orderType":"Limit","orderQty":1,"price":5010.25},
                                           {"id":101,"orderId":10,"orderType":"Limit","orderQty":2,"price":5011.5},
                                           {"id":102,"orderId":11,"orderType":"Stop","orderQty":3,"stopPrice":"99.5"}]"#),
            "/fillFee/list" => ok(r#"[{"id":1,"clearingFee":0.1,"exchangeFee":0.2,"nfaFee":0.02,"brokerageFee":0.5,"ipFee":0.05,"commission":-1.0,"orderRoutingFee":0.03}]"#),
            "/fill/list" => ok(&format!(
                r#"[{{"id":1,"orderId":9,"contractId":77,"timestamp":"{}","action":"Buy","qty":2,"price":5000.0}},
                   {{"id":2,"orderId":10,"contractId":77,"timestamp":"{}","action":"Sell","qty":2,"price":5010.25}},
                   {{"id":3,"orderId":11,"contractId":78,"timestamp":"{}","action":"Buy","qty":1,"price":1.5}},
                   {{"id":4,"orderId":9,"contractId":77,"timestamp":"{}","action":"Buy","qty":1,"price":4990}}]"#,
                iso(T0 + 60),
                iso(T0 + 120),
                iso(T0 + 180),
                iso(T0 + 7200)
            )),
            "/position/list" => ok(r#"[{"accountId":42,"contractId":77,"netPos":-2,"netPrice":5010.25},
                                       {"accountId":42,"contractId":78,"netPos":0,"netPrice":0},
                                       {"accountId":43,"contractId":78,"netPos":3,"netPrice":1.25}]"#),
            "/contract/item" => match q("id").as_str() {
                "77" => ok(r#"{"id":77,"name":"ESZ5","contractMaturityId":88}"#),
                "78" => ok(r#"{"id":78,"name":"CLF6-CLG6","contractMaturityId":89}"#),
                _ => ResponseTemplate::new(404),
            },
            "/contractMaturity/item" => match q("id").as_str() {
                "88" => ok(r#"{"id":88,"productId":99}"#),
                "89" => ok(r#"{"id":89,"productId":98}"#),
                _ => ResponseTemplate::new(404),
            },
            "/product/item" => match q("id").as_str() {
                "99" => ok(r#"{"id":99,"productType":"Futures","valuePerPoint":50.0,"currencyId":1}"#),
                "98" => ok(r#"{"id":98,"productType":"Spread","valuePerPoint":1000,"currencyId":2}"#),
                _ => ResponseTemplate::new(404),
            },
            "/currency/item" if q("id") == "1" => ok(r#"{"id":1,"name":"usd"}"#),
            _ => ResponseTemplate::new(404),
        }
    }
}

async fn venue() -> (MockServer, otw_connector_sdk::Driver<NinjaTrader>) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(Venue).mount(&server).await;
    let d = driver_against(m(), &server, ManualClock::at(ts(T0)));
    (server, d)
}

fn q(kind: AccountKind, account: Option<&str>, range: Option<TimeRange>) -> Command {
    Command::QueryAccount { req_id: CorrId(1), kind, symbols: vec![], range, cursor: None, account: account.map(str::to_string) }
}

#[test]
fn capabilities_parse_and_enums_map() {
    assert_undeclared_is_unsupported(&m(), &InstrumentId::new(VENUE, "ESZ5"));
    assert_eq!(
        ["Futures", "Continuous", "CommonStock", "Cryptocurrency", "Options", "MarketInternals", "Spread", "Swap"].map(class_of),
        [AssetClass::Future, AssetClass::Future, AssetClass::Equity, AssetClass::Spot, AssetClass::Option, AssetClass::Index, AssetClass::Other, AssetClass::Other]
    );
    assert_eq!((side("Buy"), side("sell"), side("Hold")), (Mapped::Known(Side::Buy), Mapped::Known(Side::Sell), Mapped::Unknown("Hold".into())));
    let fee: Row = serde_json::from_str(r#"{"id":7,"clearingFee":0.11,"exchangeFee":1.33,"nfaFee":"0.02","brokerageFee":0.5,"commission":-0.25}"#).unwrap();
    assert_eq!(total_fee(&fee).to_string(), "2.21");
    assert!(m().check_settings(&[("account_id".to_string(), "abc".to_string())].into()).is_err());
    assert!(m().check_settings(&[("account_id".to_string(), "42".to_string())].into()).is_ok());
    let e = m().signer(&[("username".to_string(), "u".to_string())].into()).err().unwrap();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("password"));
}

#[tokio::test]
async fn the_account_is_picked_never_guessed() {
    let (_server, d) = venue().await;
    let one = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let evs = d.execute_as(&q(AccountKind::Summary, None, None), Some(one.as_ref())).await.unwrap();
    let Event::AccountSummary(s) = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(s.detail.as_deref(), Some("sign-in accepted, reading account APEX-1 (42) of [APEX-1 (42)]"));
    let multi = d.mapper().signer(&secrets("multi")).unwrap().unwrap();
    let e = d.execute_as(&q(AccountKind::Positions, None, None), Some(multi.as_ref())).await.unwrap_err();
    assert!(e.message.contains("APEX-1 (42)") && e.message.contains("DEMO (43)") && !e.message.contains("OLD"), "{}", e.message);
    let e = d.execute_as(&q(AccountKind::Positions, Some("44"), None), Some(multi.as_ref())).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound, "a closed account is not carried");
    let evs = d.execute_as(&q(AccountKind::Positions, Some("43"), None), Some(multi.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    assert_eq!(p.len(), 1);
    // A spread: filed as other, its own point value, no currency answered.
    assert_eq!((p[0].instrument.native.as_str(), p[0].class, p[0].multiplier.map(|m| m.to_string()), p[0].currency.as_deref()), ("CLF6-CLG6", Some(AssetClass::Other), Some("1000".into()), Some("")));
}

#[tokio::test]
async fn fills_add_every_fee_line_and_walk_each_contract_once() {
    let (server, d) = venue().await;
    let one = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let range = TimeRange::new(ts(T0), ts(T0 + 3600));
    let evs = d.execute_as(&q(AccountKind::Fills, None, Some(range)), Some(one.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!("{evs:?}") };
    // Fill 3 belongs to account 43 (through its order); fill 4 is outside the period.
    assert_eq!(f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1", "2"]);
    let first = &f[0];
    assert_eq!(
        (first.fee.0.to_string(), first.multiplier.map(|m| m.to_string()), first.instrument.native.as_str(), first.quote_ccy.as_deref(), first.class, first.account.as_deref()),
        ("1.90".into(), Some("50.0".into()), "ESZ5", Some("USD"), Some(AssetClass::Future), Some("42"))
    );
    assert_eq!((first.side.clone(), first.qty.to_string(), first.price.to_string(), f[1].fee.0.is_zero()), (Mapped::Known(Side::Buy), "2".into(), "5000.0".into(), true));
    let lookups = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/contract/item").count();
    assert_eq!(lookups, 1, "two fills on one contract: walked once");
    let e = d.execute_as(&q(AccountKind::Fills, None, Some(TimeRange::new(ts(T0), ts(T0)))), Some(one.as_ref())).await.unwrap_err();
    assert!(e.message.contains("ends before it starts"));
}

#[tokio::test]
async fn working_orders_read_their_latest_version() {
    let (_server, d) = venue().await;
    let one = d.mapper().signer(&secrets("one")).unwrap().unwrap();
    let evs = d.execute_as(&q(AccountKind::OpenOrders, None, None), Some(one.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), .. } = &evs[0].event else { panic!() };
    assert_eq!(o.len(), 1, "filled and cancelled orders are history");
    assert_eq!(
        (o[0].id.as_str(), o[0].order_type.as_str(), o[0].qty.to_string(), o[0].limit_price.map(|p| p.to_string()), o[0].stop_price, o[0].placed_at),
        ("10", "Limit", "2".into(), Some("5011.5".into()), None, Some(ts(T0 + 120)))
    );
    let multi = d.mapper().signer(&secrets("multi")).unwrap().unwrap();
    let evs = d.execute_as(&q(AccountKind::OpenOrders, Some("43"), None), Some(multi.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Orders(o), .. } = &evs[0].event else { panic!() };
    assert_eq!((o[0].stop_price.map(|p| p.to_string()), o[0].limit_price), (Some("99.5".into()), None));
}

#[tokio::test]
async fn the_token_is_minted_once_and_dropped_when_refused() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/auth/accesstokenrequest")).and(body_string_contains("\"name\":\"solo\""))
        .respond_with(ok(r#"{"accessToken":"t-solo","expirationTime":"2024-01-01T01:30:00Z"}"#)).expect(2).mount(&server).await;
    Mock::given(method("GET")).and(path("/account/list")).and(wiremock::matchers::header("authorization", "Bearer t-solo"))
        .respond_with(ok(r#"[{"id":5,"name":"A"}]"#)).up_to_n_times(2).mount(&server).await;
    Mock::given(method("GET")).and(path("/account/list")).respond_with(ResponseTemplate::new(401).set_body_string("expired")).mount(&server).await;
    let d = driver_against(m(), &server, ManualClock::at(ts(T0)));
    let signer = d.mapper().signer(&secrets("solo")).unwrap().unwrap();
    d.execute_as(&q(AccountKind::Summary, None, None), Some(signer.as_ref())).await.unwrap();
    d.execute_as(&q(AccountKind::Summary, None, None), Some(signer.as_ref())).await.unwrap();
    let e = d.execute_as(&q(AccountKind::Summary, None, None), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("two sessions per login"), "{}", e.message);
    // The 401 dropped the token: the next call signs in again.
    let _ = d.execute_as(&q(AccountKind::Summary, None, None), Some(signer.as_ref())).await;
}

#[tokio::test]
async fn a_refused_sign_in_names_the_fix() {
    let (_server, d) = venue().await;
    let mut wrong = secrets("nobody");
    wrong.insert("password".into(), "bad".into());
    wrong.insert("env".into(), "demo".into());
    let s = d.mapper().signer(&wrong).unwrap().unwrap();
    let e = d.execute_as(&q(AccountKind::Summary, None, None), Some(s.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::Auth && e.message.contains("refused the sign-in") && e.message.contains("demo environment"), "{}", e.message);
    // A 200 with no token: the login has terms to accept.
    let s = d.mapper().signer(&secrets("terms")).unwrap().unwrap();
    let e = d.execute_as(&q(AccountKind::Summary, None, None), Some(s.as_ref())).await.unwrap_err();
    assert!(e.message.contains("accept new terms") && e.message.contains("Please accept the terms"), "{}", e.message);
}

/// Decoding never panics, whatever the platform answers.
mod fuzz {
    use super::*;
    use otw_connector_sdk::InstrumentCache;
    use otw_connector_testkit::http_frame;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn decode_never_panics(body in ".{0,200}", step in 0usize..10, status in prop_oneof![Just(200u16), Just(401), Just(404), Just(500)]) {
            let endpoints = ["accounts", "orders", "fees", "fills", "positions", "versions", "contract", "maturity", "product", "currency"];
            let mapper = m();
            for kind in [AccountKind::Fills, AccountKind::Positions, AccountKind::OpenOrders, AccountKind::Summary] {
                let mut s = Session::default();
                let cache = InstrumentCache::new();
                let id = otw_connector_model::ConnectorId("t".into());
                let ctx = MapCtx { received_ts: ts(T0), instruments: &cache, connector: &id };
                let cmd = q(kind, None, Some(TimeRange::new(ts(T0), ts(T0 + 60))));
                mapper.encode(&cmd, &mut s, &ctx).unwrap();
                if let Some(j) = s.jobs.get_mut(&CorrId(1)) {
                    j.expect = endpoints[step];
                    j.account = "42".into();
                    j.hop = Some((77, Contract::default()));
                }
                let _ = mapper.decode(&http_frame(1, endpoints[step], status, &body), &mut s, &ctx);
            }
        }
    }
}
