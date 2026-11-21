//! Trading hours on the 2026 clock changes, the weeks US and EU offsets disagree, weekends,
//! holidays and half days, read from the embedded calendar. Expected instants come from the
//! venues' published hours: NYSE 09:30 to 16:00 New York, Euronext Paris 09:00 to 17:30,
//! London 08:00 to 16:30, CME Globex 17:00 Chicago the evening before to 16:00.

use time::macros::{date, datetime};

use super::*;

fn venue(mic: &str) -> Exchange {
    Calendar::parse(EMBEDDED).unwrap().find(mic).cloned().unwrap()
}

fn first_open(e: &Exchange, label: Date) -> OffsetDateTime {
    e.intervals(label).first().map(|(a, _)| *a).unwrap_or_else(|| panic!("{} trades on {label}", e.mic))
}

#[test]
fn nyse_opens_an_hour_earlier_in_utc_after_the_us_spring_change() {
    let nyse = venue("XNYS");
    assert_eq!(first_open(&nyse, date!(2026 - 03 - 06)), datetime!(2026-03-06 14:30 UTC));
    assert_eq!(first_open(&nyse, date!(2026 - 03 - 09)), datetime!(2026-03-09 13:30 UTC));
    // And back after 1 November.
    assert_eq!(first_open(&nyse, date!(2026 - 10 - 30)), datetime!(2026-10-30 13:30 UTC));
    assert_eq!(first_open(&nyse, date!(2026 - 11 - 02)), datetime!(2026-11-02 14:30 UTC));
}

#[test]
fn paris_opens_an_hour_earlier_in_utc_after_the_eu_spring_change() {
    let par = venue("XPAR");
    assert_eq!(first_open(&par, date!(2026 - 03 - 27)), datetime!(2026-03-27 08:00 UTC));
    assert_eq!(first_open(&par, date!(2026 - 03 - 30)), datetime!(2026-03-30 07:00 UTC));
    assert_eq!(first_open(&par, date!(2026 - 10 - 23)), datetime!(2026-10-23 07:00 UTC));
    assert_eq!(first_open(&par, date!(2026 - 10 - 26)), datetime!(2026-10-26 08:00 UTC));
}

/// Between 8 and 29 March (and between 25 October and 1 November) New York is five hours
/// behind Paris: the overlap of the two sessions is an hour longer.
#[test]
fn the_paris_new_york_overlap_widens_while_the_clocks_disagree() {
    let (par, nyse) = (venue("XPAR"), venue("XNYS"));
    let overlap = |d: Date| {
        let p = par.intervals(d)[0].1;
        let n = nyse.intervals(d)[0].0;
        (p - n).whole_minutes()
    };
    assert_eq!(overlap(date!(2026 - 03 - 02)), 120, "17:30 Paris is 11:30 New York");
    assert_eq!(overlap(date!(2026 - 03 - 16)), 180);
    assert_eq!(overlap(date!(2026 - 04 - 07)), 120);
    assert_eq!(overlap(date!(2026 - 10 - 27)), 180);
    assert_eq!(overlap(date!(2026 - 11 - 03)), 120);
}

#[test]
fn a_weekend_points_at_monday_s_open_on_the_new_offset() {
    let nyse = venue("XNYS");
    // Saturday 7 March, the clocks change on Sunday: Monday opens at 13:30 UTC.
    let s = nyse.status(datetime!(2026-03-07 15:00 UTC));
    assert!(!s.open);
    assert_eq!(s.next_change, Some(datetime!(2026-03-09 13:30 UTC)));
}

#[test]
fn good_friday_closes_new_york_and_paris_and_easter_monday_only_paris() {
    let (par, nyse, lon) = (venue("XPAR"), venue("XNYS"), venue("XLON"));
    for e in [&par, &nyse, &lon] {
        assert!(e.intervals(date!(2026 - 04 - 03)).is_empty(), "{} trades on Good Friday", e.mic);
    }
    assert!(par.intervals(date!(2026 - 04 - 06)).is_empty());
    assert!(lon.intervals(date!(2026 - 04 - 06)).is_empty());
    assert_eq!(first_open(&nyse, date!(2026 - 04 - 06)), datetime!(2026-04-06 13:30 UTC));
    // Thursday 2 April Paris is open; the next session after it is Tuesday 7 April.
    let s = par.status(datetime!(2026-04-02 16:00 UTC));
    assert!(!s.open);
    assert_eq!(s.next_change, Some(datetime!(2026-04-07 07:00 UTC)));
}

#[test]
fn christmas_eve_is_a_half_day_on_every_side_of_the_atlantic() {
    // NYSE 13:00 New York, Paris 14:05, London 12:30, all on winter time.
    let close = |mic: &str| venue(mic).intervals(date!(2026 - 12 - 24)).last().map(|(_, b)| *b).unwrap();
    assert_eq!(close("XNYS"), datetime!(2026-12-24 18:00 UTC));
    assert_eq!(close("XPAR"), datetime!(2026-12-24 13:05 UTC));
    assert_eq!(close("XLON"), datetime!(2026-12-24 12:30 UTC));
    // Christmas Day: nobody trades.
    for mic in ["XNYS", "XPAR", "XLON", "XCME"] {
        assert!(venue(mic).intervals(date!(2026 - 12 - 25)).is_empty(), "{mic}");
    }
}

/// CME's Monday session opens Sunday 17:00 Chicago: 22:00 UTC on the Sunday the US clocks
/// change in March, 23:00 UTC on the Sunday they change back.
#[test]
fn cme_sunday_evening_open_follows_the_change_made_that_morning() {
    let cme = venue("XCME");
    assert_eq!(first_open(&cme, date!(2026 - 03 - 09)), datetime!(2026-03-08 22:00 UTC));
    assert_eq!(first_open(&cme, date!(2026 - 03 - 02)), datetime!(2026-03-01 23:00 UTC));
    assert_eq!(first_open(&cme, date!(2026 - 11 - 02)), datetime!(2026-11-01 23:00 UTC));
}

/// A Globex session runs through UTC midnight: at 01:00 UTC Tuesday it is Monday evening in
/// Chicago and Tuesday's session is trading.
#[test]
fn a_session_crossing_utc_midnight_is_open_on_both_sides() {
    let cme = venue("XCME");
    assert!(cme.status(datetime!(2026-07-13 23:30 UTC)).open);
    let s = cme.status(datetime!(2026-07-14 01:00 UTC));
    assert!(s.open);
    assert_eq!(s.next_change, Some(datetime!(2026-07-14 21:00 UTC)), "16:00 Chicago on summer time");
}

#[test]
fn the_cme_thanksgiving_half_day_closes_at_noon_chicago() {
    let cme = venue("XCME");
    let (_, close) = *cme.intervals(date!(2026 - 11 - 27)).last().unwrap();
    assert_eq!(close, datetime!(2026-11-27 18:00 UTC));
}
