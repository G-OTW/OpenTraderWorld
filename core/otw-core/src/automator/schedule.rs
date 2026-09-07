//! Recurrence rules: when a workflow runs next.
//!
//! A rule is stored in the user's own timezone (an IANA name) and resolved to a UTC
//! instant, so the claim query stays a plain index scan on `next_run_at` and a rule keeps
//! meaning "07:30 in Paris" across a DST change rather than drifting by an hour.
//!
//! Two DST cases are decided here rather than left to chance:
//! - a local time that **does not exist** (the spring-forward gap) runs at the next valid
//!   instant, which is the first moment after the gap;
//! - a local time that **happens twice** (the fall-back overlap) runs on the first of the
//!   two, so a daily rule never fires twice in one day.
//!
//! Everything walks forward minute by minute from a candidate date rather than doing
//! offset arithmetic: at most a few hundred cheap iterations, and no class of "one hour
//! off twice a year" bug.

use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time, Weekday};
use time_tz::{OffsetDateTimeExt, PrimitiveDateTimeExt, TimeZone, Tz};

use otw_store::automator::Schedule;

pub const KINDS: &[&str] = &["interval", "daily", "weekly", "monthly", "once"];

/// How far ahead a rule is searched before giving up (a 31st-of-the-month rule in February
/// has to skip a month or two).
const MAX_DAYS_AHEAD: i64 = 400;

pub fn timezone(name: &str) -> Option<&'static Tz> {
    time_tz::timezones::get_by_name(name)
}

/// Every timezone name the UI may offer.
pub fn timezone_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = time_tz::timezones::iter().map(|tz| tz.name()).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Validate a rule and say what is wrong in the user's terms.
pub fn validate(s: &Schedule) -> Result<(), String> {
    if !KINDS.contains(&s.kind.as_str()) {
        return Err(format!("unknown schedule type \"{}\"", s.kind));
    }
    if timezone(&s.timezone).is_none() {
        return Err(format!("unknown timezone \"{}\"", s.timezone));
    }
    match s.kind.as_str() {
        "interval" => {
            let every = s.every_minutes.unwrap_or(0);
            if !(1..=10080).contains(&every) {
                return Err("the interval must be between 1 minute and 7 days".into());
            }
        }
        "daily" | "weekly" | "monthly" => {
            if s.at_hour.is_none() || s.at_minute.is_none() {
                return Err("pick a time of day".into());
            }
            if s.kind == "weekly" && s.weekdays.unwrap_or(0) == 0 {
                return Err("pick at least one day of the week".into());
            }
            if s.kind == "monthly" && !(1..=31).contains(&s.day_of_month.unwrap_or(0)) {
                return Err("pick a day of the month".into());
            }
        }
        "once" => {
            if s.run_at.is_none() {
                return Err("pick a date and time".into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// The first occurrence strictly after `after`, or `None` when the rule is exhausted
/// (a `once` rule whose instant has passed).
pub fn next_after(s: &Schedule, after: OffsetDateTime) -> Option<OffsetDateTime> {
    let tz = timezone(&s.timezone)?;
    match s.kind.as_str() {
        "once" => s.run_at.filter(|at| *at > after),
        "interval" => {
            let every = Duration::minutes(s.every_minutes.unwrap_or(60).max(1) as i64);
            // Anchored on the *planned* instant, not on when the run actually started: a
            // tick fires up to a minute late, and anchoring on that would push every
            // occurrence later than the last, drifting an hourly rule by half an hour a day.
            // `last_run_at` is the fallback for a schedule that has no plan left.
            let mut next = s.next_run_at.or(s.last_run_at).unwrap_or(after);
            if next > after {
                return Some(next);
            }
            loop {
                next += every;
                if next > after {
                    return Some(next);
                }
            }
        }
        _ => {
            let hour = s.at_hour.unwrap_or(0).clamp(0, 23) as u8;
            let minute = s.at_minute.unwrap_or(0).clamp(0, 59) as u8;
            let local = after.to_timezone(tz);
            let mut day = local.date();
            for _ in 0..MAX_DAYS_AHEAD {
                if matches_day(s, day) {
                    if let Some(instant) = at_local(tz, day, hour, minute) {
                        if instant > after {
                            return Some(instant);
                        }
                    }
                }
                day = day.next_day()?;
            }
            None
        }
    }
}

/// The next occurrence from now.
pub fn next_from_now(s: &Schedule) -> Option<OffsetDateTime> {
    next_after(s, OffsetDateTime::now_utc())
}

/// Occurrences inside a window, for the agenda view. Capped so a one-minute interval over
/// a year cannot ask the server to materialize half a million rows.
pub fn occurrences(
    s: &Schedule,
    from: OffsetDateTime,
    to: OffsetDateTime,
    cap: usize,
) -> Vec<OffsetDateTime> {
    let mut out = Vec::new();
    let mut cursor = from;
    while out.len() < cap {
        let Some(next) = next_after(s, cursor) else { break };
        if next > to {
            break;
        }
        out.push(next);
        cursor = next;
    }
    out
}

fn matches_day(s: &Schedule, day: Date) -> bool {
    match s.kind.as_str() {
        "daily" => true,
        "weekly" => {
            let mask = s.weekdays.unwrap_or(0);
            // Monday = bit 0, matching the RemindMe/Routines convention.
            mask == 0 || mask & (1 << weekday_index(day.weekday())) != 0
        }
        "monthly" => {
            let wanted = s.day_of_month.unwrap_or(1);
            let last = days_in_month(day);
            // A 31st rule in a 30-day month fires on the last day rather than skipping the
            // month: "the end of the month" is what the user meant.
            day.day() as i32 == wanted.min(last)
        }
        _ => false,
    }
}

fn weekday_index(w: Weekday) -> u8 {
    match w {
        Weekday::Monday => 0,
        Weekday::Tuesday => 1,
        Weekday::Wednesday => 2,
        Weekday::Thursday => 3,
        Weekday::Friday => 4,
        Weekday::Saturday => 5,
        Weekday::Sunday => 6,
    }
}

fn days_in_month(day: Date) -> i32 {
    time::util::days_in_month(day.month(), day.year()) as i32
}

/// A local wall-clock time as a UTC instant, resolving the two DST edge cases.
fn at_local(tz: &Tz, day: Date, hour: u8, minute: u8) -> Option<OffsetDateTime> {
    let time = Time::from_hms(hour, minute, 0).ok()?;
    let naive = PrimitiveDateTime::new(day, time);
    match naive.assume_timezone(tz) {
        time_tz::OffsetResult::Some(t) => Some(t),
        // Ambiguous: the clock went back and this wall time happened twice. Fire once, on
        // the first of the two.
        time_tz::OffsetResult::Ambiguous(first, _) => Some(first),
        // Invalid: the clock jumped forward over this wall time. Walk to the first minute
        // that does exist, which is the instant the gap ends.
        time_tz::OffsetResult::None => {
            let mut probe = naive;
            for _ in 0..(4 * 60) {
                probe += Duration::minutes(1);
                if let time_tz::OffsetResult::Some(t) = probe.assume_timezone(tz) {
                    return Some(t);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use uuid::Uuid;

    fn rule(kind: &str, tz: &str) -> Schedule {
        Schedule {
            id: Uuid::nil(),
            workflow_id: Uuid::nil(),
            version_id: None,
            kind: kind.into(),
            timezone: tz.into(),
            every_minutes: None,
            at_hour: Some(7),
            at_minute: Some(30),
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
    fn daily_keeps_local_time_across_dst() {
        let s = rule("daily", "Europe/Paris");
        // Winter: Paris is UTC+1, so 07:30 local is 06:30 UTC.
        let winter = next_after(&s, datetime!(2026-03-28 12:00 UTC)).unwrap();
        assert_eq!(winter, datetime!(2026-03-29 05:30 UTC));
        // Summer: UTC+2, so 07:30 local is 05:30 UTC. Same rule, different instant.
        let summer = next_after(&s, datetime!(2026-06-01 12:00 UTC)).unwrap();
        assert_eq!(summer, datetime!(2026-06-02 05:30 UTC));
    }

    #[test]
    fn skipped_local_hour_runs_at_the_end_of_the_gap() {
        // 02:30 does not exist in Paris on 2026-03-29 (clocks jump 02:00 to 03:00).
        let mut s = rule("daily", "Europe/Paris");
        s.at_hour = Some(2);
        s.at_minute = Some(30);
        let hit = next_after(&s, datetime!(2026-03-29 00:00 UTC)).unwrap();
        assert_eq!(hit, datetime!(2026-03-29 01:00 UTC));
    }

    #[test]
    fn doubled_local_hour_runs_once() {
        // 02:30 happens twice in Paris on 2026-10-25; the first is 00:30 UTC.
        let mut s = rule("daily", "Europe/Paris");
        s.at_hour = Some(2);
        s.at_minute = Some(30);
        let hit = next_after(&s, datetime!(2026-10-24 12:00 UTC)).unwrap();
        assert_eq!(hit, datetime!(2026-10-25 00:30 UTC));
        // And the next occurrence is the following day, not the second 02:30.
        let after = next_after(&s, hit).unwrap();
        assert_eq!(after, datetime!(2026-10-26 01:30 UTC));
    }

    #[test]
    fn weekly_picks_the_masked_days() {
        let mut s = rule("weekly", "UTC");
        s.weekdays = Some(1 << 2); // Wednesday
        let hit = next_after(&s, datetime!(2026-08-21 12:00 UTC)).unwrap();
        assert_eq!(hit, datetime!(2026-08-26 07:30 UTC));
    }

    #[test]
    fn monthly_31st_lands_on_the_last_day_of_a_short_month() {
        let mut s = rule("monthly", "UTC");
        s.day_of_month = Some(31);
        let hit = next_after(&s, datetime!(2026-09-15 12:00 UTC)).unwrap();
        assert_eq!(hit, datetime!(2026-09-30 07:30 UTC));
    }

    #[test]
    fn interval_keeps_its_phase_from_the_last_run() {
        let mut s = rule("interval", "UTC");
        s.every_minutes = Some(15);
        s.last_run_at = Some(datetime!(2026-08-21 10:00 UTC));
        let hit = next_after(&s, datetime!(2026-08-21 10:07 UTC)).unwrap();
        assert_eq!(hit, datetime!(2026-08-21 10:15 UTC));
    }

    #[test]
    fn interval_does_not_drift_when_a_run_starts_late() {
        // Planned 10:00, the tick actually fired at 10:00:40. The next occurrence is 10:15,
        // not 10:15:40, so an hourly rule stays on the hour forever.
        let mut s = rule("interval", "UTC");
        s.every_minutes = Some(15);
        s.next_run_at = Some(datetime!(2026-08-21 10:00 UTC));
        s.last_run_at = Some(datetime!(2026-08-21 10:00:40 UTC));
        let hit = next_after(&s, datetime!(2026-08-21 10:00:40 UTC)).unwrap();
        assert_eq!(hit, datetime!(2026-08-21 10:15 UTC));
    }

    #[test]
    fn once_is_exhausted_after_its_instant() {
        let mut s = rule("once", "UTC");
        s.run_at = Some(datetime!(2026-08-21 10:00 UTC));
        assert!(next_after(&s, datetime!(2026-08-21 11:00 UTC)).is_none());
    }
}
