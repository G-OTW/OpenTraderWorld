//! IBKR Flex, offline: the request, the collection while IBKR builds the statement, and the
//! statement read, through the driver against a local server. Synthetic fixtures written
//! from the Activity Flex Query reference.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicU32, Ordering};

use otw_connector_model::{AccountItems, AccountKind, Command, CorrId, Event, Mapped, Side, TimeRange};
use otw_connector_testkit::{assert_undeclared_is_unsupported, driver_against, ts, ManualClock};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::*;

fn m() -> IbkrFlex {
    IbkrFlex::new().unwrap()
}

const XML: &str = r#"<FlexQueryResponse queryName="Trades" type="AF">
  <FlexStatements count="1">
    <FlexStatement accountId="U1234567" fromDate="20240101" toDate="20240131">
      <Trades>
        <Trade accountId="U1234567" currency="USD" assetCategory="STK" symbol="AAPL"
               tradeID="7788" ibOrderID="99" dateTime="20240115;103000" quantity="10"
               tradePrice="185.5" ibCommission="-1.05" ibCommissionCurrency="USD"
               buySell="BUY" exchange="NASDAQ" multiplier="1" levelOfDetail="EXECUTION" />
        <Trade accountId="U1234567" currency="USD" assetCategory="STK" symbol="AAPL"
               tradeID="7788" quantity="10" tradePrice="185.5" buySell="BUY"
               levelOfDetail="ORDER" />
        <Trade accountId="U1234567" currency="USD" assetCategory="OPT"
               symbol="SPY   251219C00650000" tradeID="7789" dateTime="20240116;0930"
               quantity="-2" tradePrice="3.2" ibCommission="-1.30" buySell="SELL"
               multiplier="100" listingExchange="CBOE" levelOfDetail="EXECUTION" />
        <Trade accountId="U1234567" currency="USD" assetCategory="STK" symbol="AAPL"
               tradeID="7790" dateTime="20240115;103000" quantity="-10" tradePrice="185.5"
               buySell="BUY (Ca.)" levelOfDetail="EXECUTION" />
        <Trade accountId="U1234567" currency="EUR" assetCategory="BILL" symbol="DE0001"
               tradeID="7791" dateTime="2024-01-20;10:00:00" quantity="1,000" tradePrice="99.1"
               levelOfDetail="EXECUTION" />
      </Trades>
      <OpenPositions>
        <OpenPosition accountId="U1234567" currency="USD" assetCategory="FUT" symbol="MNQH4"
                      description="MNQ 15MAR24" position="-2" costBasisPrice="17000.25"
                      multiplier="2" listingExchange="CME" />
      </OpenPositions>
    </FlexStatement>
  </FlexStatements>
</FlexQueryResponse>"#;

fn settings(query: &str, tz: &str) -> HashMap<String, String> {
    [("flex_token", "tok"), ("query_id", query), ("tz_offset", tz)].iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn q(kind: AccountKind, selector: Option<String>, symbols: &[&str]) -> Command {
    Command::QueryAccount {
        req_id: CorrId(1),
        kind,
        symbols: symbols.iter().map(|s| InstrumentId::new(VENUE, s)).collect(),
        range: Some(TimeRange::new(ts(1_704_067_200), ts(1_706_745_600))),
        cursor: None,
        account: selector,
    }
}

/// SendRequest answers a reference; GetStatement says "in progress" `busy` times first.
struct Flex {
    busy: u32,
    seen: AtomicU32,
}

impl Respond for Flex {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let p = |k: &str| req.url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string()).unwrap_or_default();
        assert_eq!(p("v"), "3");
        match req.url.path() {
            "/SendRequest" => {
                if p("t") != "tok" {
                    return ResponseTemplate::new(200).set_body_string("<FlexStatementResponse><Status>Fail</Status><ErrorCode>1015</ErrorCode><ErrorMessage>Token is invalid.</ErrorMessage></FlexStatementResponse>");
                }
                if p("q") == "999" {
                    return ResponseTemplate::new(200).set_body_string("<FlexStatementResponse><Status>Fail</Status><ErrorCode>1014</ErrorCode><ErrorMessage>Query is invalid.</ErrorMessage></FlexStatementResponse>");
                }
                ResponseTemplate::new(200).set_body_string("<FlexStatementResponse><Status>Success</Status><ReferenceCode>REF42</ReferenceCode><Url>https://example.invalid/GetStatement</Url></FlexStatementResponse>")
            }
            "/GetStatement" => {
                assert_eq!((p("q").as_str(), p("t").as_str()), ("REF42", "tok"));
                if self.seen.fetch_add(1, Ordering::SeqCst) < self.busy {
                    return ResponseTemplate::new(200).set_body_string("<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode><ErrorMessage>Statement generation in progress. Please try again shortly.</ErrorMessage></FlexStatementResponse>");
                }
                ResponseTemplate::new(200).set_body_string(XML)
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

async fn venue(busy: u32) -> (MockServer, otw_connector_sdk::Driver<IbkrFlex>) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any()).respond_with(Flex { busy, seen: AtomicU32::new(0) }).mount(&server).await;
    let d = driver_against(m(), &server, ManualClock::at(ts(1_704_067_200)));
    (server, d)
}

fn rows(xml: &str) -> Vec<Attrs> {
    elements(xml, "Trade")
}

#[test]
fn capabilities_parse_and_rows_read() {
    assert_undeclared_is_unsupported(&m(), &InstrumentId::new(VENUE, "AAPL"));
    let all = rows(XML);
    assert_eq!(execution_level(all.clone()).len(), 4, "the order-level duplicate is dropped");
    assert_eq!((symbol_of(&all[2]), symbol_of(&all[0])), ("SPY251219C00650000".to_string(), "AAPL".to_string()));
    assert_eq!(stamp(&all[0], UtcOffset::UTC), ts(1_705_314_600).into());
    // HHmm without seconds, in the query's zone.
    assert_eq!(stamp(&all[2], UtcOffset::from_whole_seconds(-5 * 3600).unwrap()), ts(1_705_415_400).into());
    assert_eq!((side_of(&all[0]), side_of(&all[3])), (Mapped::Known(Side::Buy), Mapped::Known(Side::Sell)), "a cancellation undoes the buy");
    let unsigned = rows(r#"<Trades><Trade quantity="5" buySell="SELL" /></Trades>"#);
    assert_eq!(side_of(&unsigned[0]), Mapped::Known(Side::Sell));
    assert_eq!(["STK", "fund", "FOP", "FUT", "CFD", "CASH", "CRYPTO", "BILL", "CMDTY", "IOPT"].map(class_of), [
        AssetClass::Equity, AssetClass::Etf, AssetClass::Option, AssetClass::Future, AssetClass::Cfd, AssetClass::Forex, AssetClass::Spot, AssetClass::Bond, AssetClass::Commodity, AssetClass::Other
    ]);
    let fail = "<FlexStatementResponse><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>";
    assert_eq!(flex_error(fail), Some(("1012".into(), "1012 Token has expired.".into())));
    assert_eq!((code_kind("1012"), code_kind("1018"), code_kind("1019"), code_kind("1014")), (ErrorKind::Auth, ErrorKind::RateLimited, ErrorKind::Transient, ErrorKind::InvalidRequest));
    assert!(flex_error("<FlexStatementResponse><Status>Success</Status></FlexStatementResponse>").is_none());
    for (k, v, good) in [("query_id", "abc", false), ("query_id", "123", true), ("tz_offset", "-300", true), ("tz_offset", "900", false), ("tz_offset", "x", false)] {
        assert_eq!(m().check_settings(&[(k.to_string(), v.to_string())].into()).is_ok(), good, "{k} {v}");
    }
    assert_eq!(m().account_selector(&settings("123", "-300")).as_deref(), Some("123|-300"));
    assert_eq!(m().account_selector(&settings("", "60")), None);
}

#[tokio::test]
async fn fills_wait_for_the_statement_then_read_it() {
    let (server, d) = venue(2).await;
    let signer = d.mapper().signer(&settings("123", "")).unwrap().unwrap();
    let sel = d.mapper().account_selector(&settings("123", "0"));
    let evs = d.execute_as(&q(AccountKind::Fills, sel, &[]), Some(signer.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!("{evs:?}") };
    assert_eq!(f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["7788", "7789", "7790", "7791"]);
    let opt = &f[1];
    assert_eq!(
        (opt.instrument.native.as_str(), opt.qty.to_string(), opt.fee.0.to_string(), opt.class, opt.multiplier.map(|m| m.to_string()), opt.venue.as_deref()),
        ("SPY251219C00650000", "2".into(), "1.30".into(), Some(AssetClass::Option), Some("100".into()), Some("CBOE"))
    );
    assert_eq!((f[3].qty.to_string(), f[3].class, f[3].quote_ccy.as_deref(), f[3].multiplier.map(|m| m.to_string())), ("1000".into(), Some(AssetClass::Bond), Some("EUR"), Some("1".into())));
    let gets = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/GetStatement").count();
    assert_eq!(gets, 3, "two answers in progress, then the statement");
    // The zone and the symbol filter.
    let (_s2, d2) = venue(0).await;
    let sel = d2.mapper().account_selector(&settings("123", "60"));
    let evs = d2.execute_as(&q(AccountKind::Fills, sel, &["SPY251219C00650000"]), Some(signer.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Fills(f), .. } = &evs[0].event else { panic!() };
    assert_eq!((f.len(), f[0].ts), (1, ts(1_705_393_800)));
}

#[tokio::test]
async fn a_statement_still_building_after_twelve_tries_is_a_retry_later() {
    let (server, d) = venue(100).await;
    let signer = d.mapper().signer(&settings("123", "")).unwrap().unwrap();
    let e = d.execute_as(&q(AccountKind::Fills, Some("123|0".into()), &[]), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::Transient && e.message.contains("still generating the statement after 60s"), "{e}");
    let gets = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/GetStatement").count();
    assert_eq!(gets, 12);
}

#[tokio::test]
async fn positions_summary_and_refusals() {
    let (_server, d) = venue(0).await;
    let signer = d.mapper().signer(&settings("123", "")).unwrap().unwrap();
    let evs = d.execute_as(&q(AccountKind::Positions, Some("123|0".into()), &[]), Some(signer.as_ref())).await.unwrap();
    let Event::AccountChunk { items: AccountItems::Positions(p), .. } = &evs[0].event else { panic!() };
    assert_eq!(
        (p[0].instrument.native.as_str(), p[0].qty.to_string(), p[0].name.as_deref(), p[0].multiplier.map(|m| m.to_string()), p[0].venue.as_deref(), p[0].class),
        ("MNQH4", "-2".into(), Some("MNQ 15MAR24"), Some("2".into()), Some("CME"), Some(AssetClass::Future))
    );
    let evs = d.execute_as(&q(AccountKind::Summary, Some("123|0".into()), &[]), Some(signer.as_ref())).await.unwrap();
    let Event::AccountSummary(s) = &evs[0].event else { panic!() };
    assert_eq!(s.detail.as_deref(), Some("statement for U1234567 covering 20240101 → 20240131, 5 trade row(s)"));
    let e = d.execute_as(&q(AccountKind::Summary, Some("999|0".into()), &[]), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.kind == ErrorKind::InvalidRequest && e.message.contains("refused the Flex request: 1014 Query is invalid."), "{e}");
    let wrong = d.mapper().signer(&[("flex_token".to_string(), "nope".to_string())].into()).unwrap().unwrap();
    let e = d.execute_as(&q(AccountKind::Summary, Some("123|0".into()), &[]), Some(wrong.as_ref())).await.unwrap_err();
    assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::Auth, Some("1015")));
    let e = d.execute_as(&q(AccountKind::Summary, None, &[]), Some(signer.as_ref())).await.unwrap_err();
    assert!(e.message.contains("query_id is not set"));
    assert!(d.mapper().signer(&HashMap::new()).err().unwrap().message.contains("flex_token"));
}

#[test]
fn a_statement_without_open_positions_says_how_to_add_them() {
    let job = Job { kind: AccountKind::Positions, range: None, symbols: vec![], offset: UtcOffset::UTC, reference: String::new(), attempt: 1 };
    let cache = otw_connector_sdk::InstrumentCache::new();
    let id = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &id };
    let e = IbkrFlex::emit(&job, "<FlexQueryResponse><Trades/></FlexQueryResponse>", CorrId(1), &ctx).unwrap_err();
    assert!(e.message.contains("Add the Open Positions section"));
    let bad = r#"<FlexQueryResponse><Trades><Trade symbol="X" quantity="1" dateTime="15/01/2024" /></Trades></FlexQueryResponse>"#;
    let job = Job { kind: AccountKind::Fills, range: Some(TimeRange::new(ts(0), ts(1))), ..job };
    let e = IbkrFlex::emit(&job, bad, CorrId(1), &ctx).unwrap_err();
    assert!(e.message.contains("yyyyMMdd"), "{e}");
}

mod fuzz {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn statement_reading_never_panics(body in ".{0,300}", tz in -900i32..900) {
            let cache = otw_connector_sdk::InstrumentCache::new();
            let id = otw_connector_model::ConnectorId("t".into());
            let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &id };
            for kind in [AccountKind::Fills, AccountKind::Positions, AccountKind::Summary] {
                let job = Job { kind, range: Some(TimeRange::new(ts(0), ts(1))), symbols: vec![], offset: UtcOffset::from_whole_seconds(tz * 60).unwrap_or(UtcOffset::UTC), reference: "r".into(), attempt: 1 };
                let xml = format!("<FlexQueryResponse><Trade dateTime=\"{body}\" quantity=\"{body}\"/><OpenPosition position=\"{body}\"/></FlexQueryResponse>");
                let _ = IbkrFlex::emit(&job, &xml, CorrId(1), &ctx);
                let _ = flex_error(&body);
            }
        }
    }
}
