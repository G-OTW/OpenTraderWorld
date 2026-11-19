//! A Flex statement read whole, then damaged: every fill it holds comes back, or the read
//! fails naming why, never a statement silently shortened. Fixture in
//! `fixtures/imports/ibkr/` (synthetic, see its README).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use otw_connector_model::{AccountItems, AccountKind, CorrId, Event, Mapped, Side, TimeRange};
use otw_connector_testkit::ts;

use super::*;

const STATEMENT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../fixtures/imports/ibkr/flex_trades.xml"));

/// March 2026, the fixture's period.
fn march() -> TimeRange {
    TimeRange::new(ts(1_772_323_200), ts(1_775_001_600))
}

fn read(kind: AccountKind, xml: &str, offset_min: i32) -> Result<AccountItems, ConnectorError> {
    read_with(kind, xml, offset_min, None, march())
}

fn read_with(kind: AccountKind, xml: &str, offset_min: i32, zone: Option<&'static Tz>, range: TimeRange) -> Result<AccountItems, ConnectorError> {
    let cache = otw_connector_sdk::InstrumentCache::new();
    let id = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(1_775_001_600), instruments: &cache, connector: &id };
    let job = Job {
        kind,
        range: Some(range),
        symbols: vec![],
        offset: UtcOffset::from_whole_seconds(offset_min * 60).unwrap(),
        zone,
        reference: "REF".into(),
        attempt: 1,
    };
    let evs = IbkrFlex::emit(&job, xml, CorrId(1), &ctx)?;
    match evs.into_iter().next().map(|e| e.event) {
        Some(Event::AccountChunk { items, .. }) => Ok(items),
        other => panic!("not a chunk: {other:?}"),
    }
}

fn fills(xml: &str, offset_min: i32) -> Result<Vec<Fill>, ConnectorError> {
    match read(AccountKind::Fills, xml, offset_min)? {
        AccountItems::Fills(f) => Ok(f),
        other => panic!("not fills: {other:?}"),
    }
}

#[test]
fn the_fixture_statement_reads_every_fill() {
    let f = fills(STATEMENT, 0).unwrap();
    assert_eq!(f.len(), 3);
    assert_eq!(f.iter().map(|x| x.side.clone()).collect::<Vec<_>>(), [Mapped::Known(Side::Buy), Mapped::Known(Side::Sell), Mapped::Known(Side::Sell)]);
    assert_eq!(f[2].multiplier.map(|m| m.to_string()).as_deref(), Some("5"));
    assert_eq!(f[0].fee.0.to_string(), "1.00", "a commission is a cost whatever its sign");
    assert_eq!(f[0].ts, ts(1_772_443_800), "20260302;093000 at UTC");
}

/// New York is UTC-5 until 8 March 2026: a 09:30 fill on 2 March is 14:30 UTC.
#[test]
fn the_query_offset_places_a_fill_in_utc() {
    let f = fills(STATEMENT, -300).unwrap();
    assert_eq!(f[0].ts, ts(1_772_461_800));
}

#[test]
fn the_fixture_statement_reads_its_open_position() {
    match read(AccountKind::Positions, STATEMENT, 0).unwrap() {
        AccountItems::Positions(p) => {
            assert_eq!(p.len(), 1);
            assert_eq!(p[0].qty.0.to_string(), "-1");
            assert_eq!(p[0].multiplier.map(|m| m.to_string()).as_deref(), Some("5"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_empty_trades_section_is_no_fills_not_an_error() {
    let xml = r#"<FlexQueryResponse><FlexStatements count="1"><FlexStatement accountId="U0000000"><Trades/></FlexStatement></FlexStatements></FlexQueryResponse>"#;
    assert!(fills(xml, 0).unwrap().is_empty());
}

#[test]
fn a_trade_with_an_unreadable_date_names_the_format_to_set() {
    let xml = STATEMENT.replace(r#"dateTime="20260303;154500" tradeDate="20260303""#, r#"dateTime="03/03/2026 15:45" tradeDate="""#);
    let e = fills(&xml, 0).unwrap_err();
    assert!(e.message.contains("yyyyMMdd"), "{e}");
}

/// A statement cut short (a dropped connection, a disk full) must not read as a shorter
/// period: the fills after the cut would vanish without a word.
#[test]
fn a_truncated_statement_is_an_error_not_fewer_fills() {
    let cut = &STATEMENT[..STATEMENT.find(r#"tradeID="1002""#).unwrap()];
    assert!(fills(cut, 0).is_err(), "a cut statement read as a complete one");
}

/// A quantity the statement wrote but nobody can read is not a zero: skipping the row drops
/// a real fill from the position.
#[test]
fn an_unreadable_quantity_is_an_error_not_a_skipped_fill() {
    let xml = STATEMENT.replace(r#"quantity="-10""#, r#"quantity="ten""#);
    assert!(fills(&xml, 0).is_err(), "the fill with quantity=ten disappeared");
}

#[test]
fn an_unreadable_price_is_an_error_not_a_zero_price() {
    let xml = STATEMENT.replace(r#"tradePrice="160.50""#, r#"tradePrice="n/a""#);
    match fills(&xml, 0) {
        Err(_) => {}
        Ok(f) => assert!(f.iter().all(|x| !x.price.0.is_zero()), "a fill was booked at price 0"),
    }
}

/// The error names the trade and the field, so the user knows which row to look at.
#[test]
fn an_unreadable_number_names_the_trade_and_the_field() {
    for bad in ["ten", "", "--", "N/A"] {
        let xml = STATEMENT.replace(r#"quantity="-10""#, &format!(r#"quantity="{bad}""#));
        let e = fills(&xml, 0).unwrap_err();
        assert!(e.message.contains("trade 1002") && e.message.contains("quantity"), "{bad:?}: {e}");
    }
    let xml = STATEMENT.replace(r#"tradePrice="160.50""#, r#"tradePrice="n/a""#);
    let e = fills(&xml, 0).unwrap_err();
    assert!(e.message.contains("trade 1002") && e.message.contains("tradePrice") && e.message.contains("n/a"), "{e}");
    let xml = STATEMENT.replace(r#"ibCommission="-0.62""#, r#"ibCommission="--""#);
    let e = fills(&xml, 0).unwrap_err();
    assert!(e.message.contains("trade 1003") && e.message.contains("ibCommission"), "{e}");
    let xml = STATEMENT.replace(r#"position="-1""#, r#"position="x""#);
    let e = read(AccountKind::Positions, &xml, 0).unwrap_err();
    assert!(e.message.contains("MESM6") && e.message.contains("position"), "{e}");
}

/// A field IBKR leaves out when the query does not select it reads as before: no commission
/// is no fee, no point value is one.
#[test]
fn a_field_the_query_left_out_keeps_its_default() {
    let xml = STATEMENT.replace(r#" ibCommission="-1.00""#, "").replace(r#" multiplier="1""#, "");
    let f = fills(&xml, 0).unwrap();
    assert_eq!(f.len(), 3);
    assert_eq!((f[0].fee.0.to_string(), f[0].multiplier.map(|m| m.to_string())), ("0".to_string(), Some("1".to_string())));
}

/// A statement cut between two elements is as short as one cut inside a tag.
#[test]
fn a_statement_cut_between_elements_is_an_error() {
    let cut = &STATEMENT[..STATEMENT.find("</Trades>").unwrap()];
    let e = fills(cut, 0).unwrap_err();
    assert!(e.message.contains("truncated") && e.message.contains("download it again"), "{e}");
    for kind in [AccountKind::Positions, AccountKind::Summary] {
        assert!(read(kind, cut, 0).is_err(), "{kind:?}");
    }
}

// ── Named zones across the 2026 daylight saving changes ──────────────────────

/// One trade per stamp, all inside 2026.
fn statement(stamps: &[&str]) -> String {
    let rows: String = stamps
        .iter()
        .enumerate()
        .map(|(i, s)| format!(r#"<Trade tradeID="{}" symbol="AAPL" assetCategory="STK" currency="USD" dateTime="{s}" quantity="1" tradePrice="100" levelOfDetail="EXECUTION" />"#, 9000 + i))
        .collect();
    format!("<FlexQueryResponse><FlexStatements><FlexStatement><Trades>{rows}</Trades></FlexStatement></FlexStatements></FlexQueryResponse>")
}

fn zone_fills(zone: &str, stamps: &[&str]) -> Result<Vec<Timestamp>, ConnectorError> {
    let year = TimeRange::new(ts(1_767_225_600), ts(1_798_761_600));
    let tz = time_tz::timezones::get_by_name(zone).unwrap();
    match read_with(AccountKind::Fills, &statement(stamps), 0, Some(tz), year)? {
        AccountItems::Fills(f) => Ok(f.iter().map(|x| x.ts).collect()),
        other => panic!("{other:?}"),
    }
}

/// 2026-03-06 14:30Z, 2026-03-09 13:30Z, 2026-10-30 13:30Z, 2026-11-02 14:30Z.
#[test]
fn new_york_follows_its_march_8_and_november_1_changes() {
    let got = zone_fills("America/New_York", &["20260306;093000", "20260309;093000", "20261030;093000", "20261102;093000"]).unwrap();
    assert_eq!(got, [1_772_807_400, 1_773_063_000, 1_793_367_000, 1_793_629_800].map(ts));
}

/// 2026-03-27 08:00Z, 2026-03-30 07:00Z, 2026-10-23 07:00Z, 2026-10-26 08:00Z.
#[test]
fn paris_follows_its_march_29_and_october_25_changes() {
    let got = zone_fills("Europe/Paris", &["20260327;090000", "20260330;090000", "20261023;090000", "20261026;090000"]).unwrap();
    assert_eq!(got, [1_774_598_400, 1_774_854_000, 1_792_738_800, 1_793_001_600].map(ts));
}

/// The hour the clock jumps over never happened: no fill can carry it.
#[test]
fn a_wall_time_in_the_spring_gap_is_an_error() {
    for (zone, stamp) in [("America/New_York", "20260308;023000"), ("Europe/Paris", "20260329;023000")] {
        let e = zone_fills(zone, &[stamp]).unwrap_err();
        assert!(e.message.contains("trade 9000") && e.message.contains("does not exist") && e.message.contains(zone), "{e}");
    }
}

/// The autumn hour shown twice reads as its first occurrence, still on summer time:
/// 01:30 EDT on 2026-11-01 is 05:30Z, 02:30 CEST on 2026-10-25 is 00:30Z.
#[test]
fn a_wall_time_in_the_autumn_overlap_reads_as_the_earlier_instant() {
    assert_eq!(zone_fills("America/New_York", &["20261101;013000"]).unwrap(), [ts(1_793_511_000)]);
    assert_eq!(zone_fills("Europe/Paris", &["20261025;023000"]).unwrap(), [ts(1_792_888_200)]);
}

/// The zone setting rides in the selector, wins over the fixed offset, and an unknown name
/// is refused when saved and when used.
#[test]
fn the_timezone_setting_reaches_the_job() {
    let m = IbkrFlex::new().unwrap();
    let settings = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
    assert_eq!(m.account_selector(&settings(&[("query_id", "123"), ("tz_offset", "60"), ("timezone", "Europe/Paris")])).as_deref(), Some("123|Europe/Paris"));
    assert_eq!(m.account_selector(&settings(&[("query_id", "123"), ("tz_offset", "60"), ("timezone", " ")])).as_deref(), Some("123|60"));
    assert!(m.check_settings(&settings(&[("timezone", "America/New_York")])).is_ok());
    assert!(m.check_settings(&settings(&[("timezone", "Mars/Olympus")])).is_err());

    let cache = otw_connector_sdk::InstrumentCache::new();
    let id = otw_connector_model::ConnectorId("t".into());
    let ctx = MapCtx { received_ts: ts(0), instruments: &cache, connector: &id };
    let query = |selector: &str| otw_connector_model::Command::QueryAccount {
        req_id: CorrId(1),
        kind: AccountKind::Fills,
        symbols: vec![],
        range: Some(march()),
        cursor: None,
        account: Some(selector.to_string()),
    };
    let mut s = Session::default();
    m.encode(&query("123|Europe/Paris"), &mut s, &ctx).unwrap();
    assert_eq!(s.jobs[&CorrId(1)].zone.map(|z| z.name()), Some("Europe/Paris"));
    let mut s = Session::default();
    m.encode(&query("123|-300"), &mut s, &ctx).unwrap();
    let job = &s.jobs[&CorrId(1)];
    assert_eq!((job.zone.is_none(), job.offset.whole_minutes()), (true, -300));
    let e = m.encode(&query("123|Mars/Olympus"), &mut Session::default(), &ctx).unwrap_err();
    assert!(e.message.contains("Mars/Olympus"), "{e}");
}
