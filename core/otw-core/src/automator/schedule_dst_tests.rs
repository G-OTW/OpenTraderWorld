//! Recurrence rules on the 2026 clock changes (US: 8 March and 1 November; EU: 29 March and
//! 25 October), the weeks the two disagree, weekends, and a local evening that is already
//! tomorrow in UTC. Expected instants are worked out from the published offsets, not from
//! the code: New York is UTC-5 in winter and UTC-4 in summer, Paris UTC+1 and UTC+2.

use time::macros::datetime;
use uuid::Uuid;

use super::*;

fn rule(kind: &str, tz: &str, hour: i32, minute: i32) -> Schedule {
    Schedule {
        id: Uuid::nil(),
        workflow_id: Uuid::nil(),
        version_id: None,
        kind: kind.into(),
        timezone: tz.into(),
        every_minutes: None,
        at_hour: Some(hour),
        at_minute: Some(minute),
        weekdays: None,
        day_of_month: None,
        run_at: None,
        catch_up: false,
        active: true,
        next_run_at: None,
        last_run_at: None,
        created_at: OffsetDateTime::UNIX_EPOCH,
        updated_at: OffsetDateTime::UNIX_EPOCH,
    }
}

#[test]
fn a_new_york_morning_rule_follows_the_us_clock_changes() {
    let s = rule("daily", "America/New_York", 9, 30);
    // Friday 6 March: EST.
    assert_eq!(next_after(&s, datetime!(2026-03-06 00:00 UTC)), Some(datetime!(2026-03-06 14:30 UTC)));
    // Monday 9 March: EDT since Sunday 02:00.
    assert_eq!(next_after(&s, datetime!(2026-03-09 00:00 UTC)), Some(datetime!(2026-03-09 13:30 UTC)));
    // Friday 30 October: still EDT. Monday 2 November: EST again.
    assert_eq!(next_after(&s, datetime!(2026-10-30 00:00 UTC)), Some(datetime!(2026-10-30 13:30 UTC)));
    assert_eq!(next_after(&s, datetime!(2026-11-02 00:00 UTC)), Some(datetime!(2026-11-02 14:30 UTC)));
}

#[test]
fn the_us_spring_gap_runs_at_its_end_and_the_autumn_overlap_runs_once() {
    // 02:30 does not exist in New York on 8 March 2026: clocks go 02:00 EST to 03:00 EDT,
    // which is 07:00 UTC.
    let s = rule("daily", "America/New_York", 2, 30);
    assert_eq!(next_after(&s, datetime!(2026-03-08 00:00 UTC)), Some(datetime!(2026-03-08 07:00 UTC)));
    // 01:30 happens twice on 1 November 2026: first at 05:30 UTC (EDT), then 06:30 (EST).
    let s = rule("daily", "America/New_York", 1, 30);
    let first = next_after(&s, datetime!(2026-11-01 00:00 UTC)).unwrap();
    assert_eq!(first, datetime!(2026-11-01 05:30 UTC));
    assert_eq!(next_after(&s, first), Some(datetime!(2026-11-02 06:30 UTC)), "not the second 01:30");
}

/// The same 09:00 in Paris and New York is six hours apart, except between the US and the
/// EU switches, when it is five.
#[test]
fn paris_and_new_york_are_five_hours_apart_while_their_clocks_disagree() {
    let paris = rule("daily", "Europe/Paris", 9, 0);
    let ny = rule("daily", "America/New_York", 9, 0);
    let gap = |day: OffsetDateTime| (next_after(&ny, day).unwrap() - next_after(&paris, day).unwrap()).whole_hours();
    assert_eq!(gap(datetime!(2026-03-02 00:00 UTC)), 6);
    assert_eq!(gap(datetime!(2026-03-16 00:00 UTC)), 5, "US on summer time, Paris not yet");
    assert_eq!(gap(datetime!(2026-04-06 00:00 UTC)), 6);
    assert_eq!(gap(datetime!(2026-10-27 00:00 UTC)), 5, "Paris back on winter time, the US not yet");
    assert_eq!(gap(datetime!(2026-11-03 00:00 UTC)), 6);
}

/// The agenda across a clock change: one occurrence a day, none doubled, none missing.
#[test]
fn the_agenda_holds_one_run_a_day_across_both_eu_changes() {
    let s = rule("daily", "Europe/Paris", 2, 30);
    for (from, to) in [
        (datetime!(2026-03-27 00:00 UTC), datetime!(2026-03-31 23:59 UTC)),
        (datetime!(2026-10-23 00:00 UTC), datetime!(2026-10-27 23:59 UTC)),
    ] {
        let runs = occurrences(&s, from, to, 100);
        assert_eq!(runs.len(), 5, "{runs:?}");
        let days: Vec<_> = runs.iter().map(|r| r.to_timezone(timezone("Europe/Paris").unwrap()).date()).collect();
        let mut distinct = days.clone();
        distinct.dedup();
        assert_eq!(days, distinct, "one run per local day");
    }
}

/// A fixed interval is a fixed length of time: across the autumn overlap it keeps sixty
/// minutes between runs, so the local wall clock reads 02:00 twice.
#[test]
fn an_interval_keeps_its_length_across_the_overlap() {
    let mut s = rule("interval", "Europe/Paris", 0, 0);
    s.every_minutes = Some(60);
    s.next_run_at = Some(datetime!(2026-10-24 23:00 UTC));
    let runs = occurrences(&s, datetime!(2026-10-24 23:00 UTC), datetime!(2026-10-25 03:00 UTC), 10);
    assert_eq!(runs, [datetime!(2026-10-25 00:00 UTC), datetime!(2026-10-25 01:00 UTC), datetime!(2026-10-25 02:00 UTC), datetime!(2026-10-25 03:00 UTC)]);
}

#[test]
fn a_weekday_rule_skips_the_weekend() {
    let mut s = rule("weekly", "America/New_York", 9, 30);
    s.weekdays = Some(0b0011111); // Monday to Friday
    // Friday 6 March after the open: next is Monday 9 March, already on summer time.
    assert_eq!(next_after(&s, datetime!(2026-03-06 15:00 UTC)), Some(datetime!(2026-03-09 13:30 UTC)));
}

/// 20:00 in New York is 00:00 or 01:00 UTC the next day: the instant moves, the local date
/// is the one the user picked.
#[test]
fn an_evening_rule_lands_on_the_next_utc_day() {
    let s = rule("daily", "America/New_York", 20, 0);
    assert_eq!(next_after(&s, datetime!(2026-06-30 12:00 UTC)), Some(datetime!(2026-07-01 00:00 UTC)));
    assert_eq!(next_after(&s, datetime!(2026-12-30 12:00 UTC)), Some(datetime!(2026-12-31 01:00 UTC)));
}

#[test]
fn a_monthly_31st_lands_on_the_last_day_of_february() {
    let mut s = rule("monthly", "Europe/Paris", 7, 30);
    s.day_of_month = Some(31);
    assert_eq!(next_after(&s, datetime!(2026-02-01 00:00 UTC)), Some(datetime!(2026-02-28 06:30 UTC)));
    // 31 March in Paris is on summer time.
    assert_eq!(next_after(&s, datetime!(2026-03-01 00:00 UTC)), Some(datetime!(2026-03-31 05:30 UTC)));
}
