//! Exclusion periods: when a robot takes no new entry.
//!
//! `{ weekdays, hours: [{from, to}], dates: [{from, to}], leave_market }`, read in the robot's
//! timezone. `weekdays` is a bitmask, Monday = bit 0, a whole day excluded. An hour range is
//! `HH:MM` to `HH:MM`, the end excluded, wrapping past midnight when it ends before it starts.
//! A date range is `YYYY-MM-DD` to `YYYY-MM-DD`, both days included. Any mix, none required.
//!
//! Inside one the robot keeps ticking and managing its exits; it opens nothing. With
//! `leave_market` it also closes what it holds on the way in, and trades again on the way out.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::macros::format_description;
use time::{Date, OffsetDateTime, Time};
use time_tz::{OffsetDateTimeExt, Tz};

const MAX_RANGES: usize = 20;
const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusions {
    #[serde(default)]
    pub weekdays: i16,
    #[serde(default)]
    pub hours: Vec<Range>,
    #[serde(default)]
    pub dates: Vec<Range>,
    #[serde(default)]
    pub leave_market: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub from: String,
    pub to: String,
}

fn time_of(s: &str) -> Option<Time> {
    Time::parse(s.trim(), format_description!("[hour]:[minute]")).ok()
}

fn date_of(s: &str) -> Option<Date> {
    Date::parse(s.trim(), format_description!("[year]-[month]-[day]")).ok()
}

/// Parse and check the stored value; each refusal names the fix.
pub fn validate(v: &Value) -> Result<Exclusions, String> {
    if v.is_null() || v.as_object().is_some_and(|o| o.is_empty()) {
        return Ok(Exclusions::default());
    }
    let e: Exclusions = serde_json::from_value(v.clone()).map_err(|e| format!("exclusions: {e}"))?;
    if !(0..=127).contains(&e.weekdays) {
        return Err("exclusions: weekdays is a bitmask from 0 to 127 (Monday = 1)".into());
    }
    if e.hours.len() > MAX_RANGES || e.dates.len() > MAX_RANGES {
        return Err(format!("exclusions: at most {MAX_RANGES} hour ranges and {MAX_RANGES} date ranges"));
    }
    for r in &e.hours {
        let (Some(a), Some(b)) = (time_of(&r.from), time_of(&r.to)) else {
            return Err(format!("exclusions: {:?} to {:?} is not an hour range, write it as 15:30 to 16:00", r.from, r.to));
        };
        if a == b {
            return Err(format!("exclusions: the hour range {} to {} is empty", r.from, r.to));
        }
    }
    for r in &e.dates {
        let (Some(a), Some(b)) = (date_of(&r.from), date_of(&r.to)) else {
            return Err(format!("exclusions: {:?} to {:?} is not a date range, write it as 2026-12-24 to 2027-01-02", r.from, r.to));
        };
        if b < a {
            return Err(format!("exclusions: the date range {} to {} ends before it starts", r.from, r.to));
        }
    }
    Ok(e)
}

impl Exclusions {
    pub fn of(v: &Value) -> Self {
        validate(v).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.weekdays == 0 && self.hours.is_empty() && self.dates.is_empty()
    }

    /// Why `now` is excluded, read in `tz`; `None` when it is not.
    pub fn holds(&self, now: OffsetDateTime, tz: &Tz) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let local = now.to_timezone(tz);
        let day = local.weekday().number_days_from_monday() as usize;
        if self.weekdays & (1 << day) != 0 {
            return Some(format!("excluded on {}", DAYS[day]));
        }
        let date = local.date();
        for r in &self.dates {
            if let (Some(a), Some(b)) = (date_of(&r.from), date_of(&r.to)) {
                if a <= date && date <= b {
                    return Some(format!("excluded from {} to {}", r.from, r.to));
                }
            }
        }
        let t = local.time();
        for r in &self.hours {
            if let (Some(a), Some(b)) = (time_of(&r.from), time_of(&r.to)) {
                let inside = if a < b { a <= t && t < b } else { t >= a || t < b };
                if inside {
                    return Some(format!("excluded from {} to {}", r.from, r.to));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::macros::datetime;

    fn tz() -> &'static Tz {
        crate::automator::schedule::timezone("Europe/Zurich").unwrap()
    }

    #[test]
    fn weekdays_hours_and_dates_are_read_in_the_robot_timezone() {
        let e = validate(&json!({
            "weekdays": 16,
            "hours": [{ "from": "15:30", "to": "16:00" }, { "from": "22:00", "to": "01:00" }],
            "dates": [{ "from": "2026-12-24", "to": "2027-01-02" }]
        }))
        .unwrap();
        // Friday 2026-11-13, any hour: Friday is bit 4.
        assert_eq!(e.holds(datetime!(2026-11-13 10:00 UTC), tz()).as_deref(), Some("excluded on Friday"));
        // Thursday 15:45 Zurich = 14:45 UTC (winter, UTC+1).
        assert!(e.holds(datetime!(2026-11-12 14:45 UTC), tz()).is_some());
        // 16:00 Zurich is the end, not inside.
        assert!(e.holds(datetime!(2026-11-12 15:00 UTC), tz()).is_none());
        // A range that wraps midnight: 00:30 Zurich on Thursday.
        assert!(e.holds(datetime!(2026-11-11 23:30 UTC), tz()).is_some());
        // Both days of a date range are included.
        assert!(e.holds(datetime!(2027-01-02 22:00 UTC), tz()).is_some());
        assert!(e.holds(datetime!(2027-01-03 12:00 UTC), tz()).is_none());
    }

    #[test]
    fn nothing_set_excludes_nothing() {
        assert!(Exclusions::of(&json!({})).holds(datetime!(2026-11-13 10:00 UTC), tz()).is_none());
        assert!(Exclusions::of(&json!({ "leave_market": true })).is_empty());
    }

    #[test]
    fn a_bad_range_is_refused_with_the_fix() {
        assert!(validate(&json!({ "hours": [{ "from": "9h", "to": "10:00" }] })).unwrap_err().contains("15:30"));
        assert!(validate(&json!({ "hours": [{ "from": "10:00", "to": "10:00" }] })).is_err());
        assert!(validate(&json!({ "dates": [{ "from": "2027-01-02", "to": "2026-12-24" }] })).is_err());
        assert!(validate(&json!({ "weekdays": 128 })).is_err());
        assert!(validate(&json!({ "days": 1 })).is_err());
    }
}
