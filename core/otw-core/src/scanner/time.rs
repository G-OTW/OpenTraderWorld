//! Time signals: a window on the clock.
//!
//! `{sessions, weekdays, from, to, timezone, at}`, every part optional and every part given
//! must hold. A bar is placed in time by the instant it closed (one second before, so a bar
//! closing exactly at 16:00 is still a 16:00 session bar), except `at`, which holds on the
//! bar during which the local clock read that time, the same rule a chart alert's `at_time`
//! fires on.
//!
//! Everything is local wall-clock time in the named timezone (or the session's own), so a
//! DST switch moves the window with the market instead of shifting it by an hour.

use ::time::{Duration, OffsetDateTime};
use serde::{Deserialize, Serialize};
use time_tz::{OffsetDateTimeExt, Tz};
use uuid::Uuid;

use otw_store::market_sessions::Session;

use crate::automator::schedule;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimeSpec {
    /// Market sessions (`/api/market-sessions`); the bar must close inside one of them.
    #[serde(default)]
    pub sessions: Vec<Uuid>,
    /// Days of the week in `timezone`, Monday = bit 0 (31 = Monday to Friday).
    #[serde(default)]
    pub weekdays: Option<i32>,
    /// Local "HH:MM", start of a daily window. With `to`; `to` before `from` runs overnight.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    /// IANA timezone for `weekdays`, `from`, `to` and `at`. Default UTC.
    #[serde(default)]
    pub timezone: Option<String>,
    /// Local "HH:MM": holds on the bar during which the clock reads it.
    #[serde(default)]
    pub at: Option<String>,
}

/// A time signal resolved against its sessions and timezone, ready to judge bars.
#[derive(Debug, Clone)]
pub struct TimeGate {
    sessions: Vec<Session>,
    weekdays: Option<i32>,
    window: Option<(u32, u32)>,
    tz: &'static Tz,
    at: Option<(u8, u8)>,
}

/// "HH:MM" as minutes after midnight.
pub fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl TimeSpec {
    fn tz_name(&self) -> &str {
        self.timezone.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or("UTC")
    }

    /// Why this window cannot be judged, without the session list.
    pub fn validate(&self) -> Result<(), String> {
        if schedule::timezone(self.tz_name()).is_none() {
            return Err(format!("unknown timezone \"{}\"", self.tz_name()));
        }
        if let Some(w) = self.weekdays {
            if !(1..=127).contains(&w) {
                return Err("pick at least one day of the week".into());
            }
        }
        match (self.from.as_deref(), self.to.as_deref()) {
            (None, None) => {}
            (Some(f), Some(t)) => {
                let (Some(f), Some(t)) = (parse_hm(f), parse_hm(t)) else {
                    return Err("from and to are times of day, HH:MM".into());
                };
                if f == t {
                    return Err("a window cannot start and end at the same time".into());
                }
            }
            _ => return Err("a window needs both from and to".into()),
        }
        if let Some(a) = self.at.as_deref() {
            if parse_hm(a).is_none() {
                return Err("at is a time of day, HH:MM".into());
            }
        }
        if self.sessions.is_empty() && self.weekdays.is_none() && self.from.is_none() && self.at.is_none() {
            return Err("a time signal needs a session, days of the week, a window or a time".into());
        }
        Ok(())
    }

    /// Resolve against the sessions it names (already loaded, in any order).
    pub fn gate(&self, sessions: &[Session]) -> Result<TimeGate, String> {
        self.validate()?;
        let mut picked = Vec::with_capacity(self.sessions.len());
        for id in &self.sessions {
            let s = sessions
                .iter()
                .find(|s| s.id == *id)
                .ok_or_else(|| "a session this time signal uses was deleted".to_string())?;
            picked.push(s.clone());
        }
        let window = match (self.from.as_deref(), self.to.as_deref()) {
            (Some(f), Some(t)) => parse_hm(f).zip(parse_hm(t)),
            _ => None,
        };
        let at = self.at.as_deref().and_then(parse_hm).map(|m| ((m / 60) as u8, (m % 60) as u8));
        let tz = schedule::timezone(self.tz_name()).ok_or("unknown timezone")?;
        Ok(TimeGate { sessions: picked, weekdays: self.weekdays, window, tz, at })
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.sessions.is_empty() {
            parts.push("in session".to_string());
        }
        if let (Some(f), Some(t)) = (&self.from, &self.to) {
            parts.push(format!("{f} to {t}"));
        }
        if let Some(a) = &self.at {
            parts.push(format!("at {a}"));
        }
        if self.weekdays.is_some() {
            parts.push("on chosen days".into());
        }
        format!("{} ({})", parts.join(", "), self.tz_name())
    }
}

impl TimeGate {
    /// Whether the bar that opened at `start` and lasts `tf` seconds is in the window.
    pub fn holds(&self, start: OffsetDateTime, tf: i64) -> bool {
        let end = start + Duration::seconds(tf);
        let t = end - Duration::seconds(1);
        if !self.sessions.is_empty()
            && !self.sessions.iter().any(|s| crate::market_sessions_api::occurrence(s, t).is_some())
        {
            return false;
        }
        let local = t.to_timezone(self.tz);
        if let Some(w) = self.weekdays {
            if w & (1 << schedule::weekday_index(local.weekday())) == 0 {
                return false;
            }
        }
        if let Some((from, to)) = self.window {
            let m = local.hour() as u32 * 60 + local.minute() as u32;
            let inside = if from < to { m >= from && m < to } else { m >= from || m < to };
            if !inside {
                return false;
            }
        }
        if let Some((h, mi)) = self.at {
            // The local days the bar touches: the clock may read `at` on either.
            let days = [start.to_timezone(self.tz).date(), t.to_timezone(self.tz).date()];
            let hit = days.iter().any(|d| {
                schedule::at_local(self.tz, *d, h, mi).is_some_and(|inst| start <= inst && inst < end)
            });
            if !hit {
                return false;
            }
        }
        true
    }

    pub fn judge(&self, ts: &[OffsetDateTime], tf: i64) -> Vec<bool> {
        ts.iter().map(|t| self.holds(*t, tf)).collect()
    }
}

/// The day key of each bar in `tz`: the UTC instant of the local midnight the bar closes in.
/// What a move from the open measures against when no market session is named.
pub fn day_keys(ts: &[OffsetDateTime], tf: i64, tz: &Tz) -> Vec<Option<i64>> {
    ts.iter()
        .map(|start| {
            let t = *start + Duration::seconds(tf) - Duration::seconds(1);
            let day = t.to_timezone(tz).date();
            schedule::at_local(tz, day, 0, 0).map(|m| m.unix_timestamp())
        })
        .collect()
}

/// The session key of each bar: the instant its session occurrence opened, `None` outside.
pub fn session_keys(ts: &[OffsetDateTime], tf: i64, s: &Session) -> Vec<Option<i64>> {
    ts.iter()
        .map(|start| {
            let t = *start + Duration::seconds(tf) - Duration::seconds(1);
            crate::market_sessions_api::occurrence(s, t).map(|(open, _)| open.unix_timestamp())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::time::macros::datetime;

    fn us() -> Session {
        Session {
            id: Uuid::nil(),
            name: "US".into(),
            timezone: "America/New_York".into(),
            start_minute: 570,
            end_minute: 960,
            weekdays: 31,
            position: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn spec(f: impl FnOnce(&mut TimeSpec)) -> TimeSpec {
        let mut s = TimeSpec::default();
        f(&mut s);
        s
    }

    #[test]
    fn scanner_time_session_window_follows_dst() {
        let g = spec(|s| s.sessions = vec![Uuid::nil()]).gate(&[us()]).unwrap();
        // Summer (EDT, UTC-4): the 13:30 UTC hourly bar closes 10:30 New York, in session.
        assert!(g.holds(datetime!(2026-10-28 13:30 UTC), 3600));
        // Winter (EST, UTC-5): the same UTC bar closes 09:30 New York, the very open... one
        // second before it, so it is not a session bar.
        assert!(!g.holds(datetime!(2026-11-04 13:30 UTC), 3600));
        // The bar an hour later is.
        assert!(g.holds(datetime!(2026-11-04 14:30 UTC), 3600));
        // A bar closing at 16:00 New York belongs to the session; the next one does not.
        assert!(g.holds(datetime!(2026-11-04 20:00 UTC), 3600));
        assert!(!g.holds(datetime!(2026-11-04 21:00 UTC), 3600));
        // Saturday is not a weekday session.
        assert!(!g.holds(datetime!(2026-11-07 15:00 UTC), 3600));
    }

    #[test]
    fn scanner_time_local_window_and_weekdays_across_dst() {
        let g = spec(|s| {
            s.from = Some("09:00".into());
            s.to = Some("10:00".into());
            s.timezone = Some("Europe/Paris".into());
            s.weekdays = Some(31);
        })
        .gate(&[])
        .unwrap();
        // 15m bars. Before the March switch Paris is UTC+1: 08:00 UTC opens 09:00 Paris.
        assert!(g.holds(datetime!(2026-03-27 08:00 UTC), 900));
        assert!(!g.holds(datetime!(2026-03-27 07:45 UTC), 900));
        // After it Paris is UTC+2: the same window starts at 07:00 UTC.
        assert!(g.holds(datetime!(2026-03-30 07:00 UTC), 900));
        assert!(!g.holds(datetime!(2026-03-30 08:00 UTC), 900));
        // Sunday is off.
        assert!(!g.holds(datetime!(2026-03-29 07:00 UTC), 900));
    }

    #[test]
    fn scanner_time_overnight_window() {
        let g = spec(|s| {
            s.from = Some("22:00".into());
            s.to = Some("02:00".into());
        })
        .gate(&[])
        .unwrap();
        assert!(g.holds(datetime!(2026-09-07 23:00 UTC), 3600));
        assert!(g.holds(datetime!(2026-09-08 00:00 UTC), 3600));
        assert!(!g.holds(datetime!(2026-09-08 12:00 UTC), 3600));
    }

    #[test]
    fn scanner_time_at_holds_on_the_bar_the_clock_reads_it_across_dst() {
        let g = spec(|s| {
            s.at = Some("09:30".into());
            s.timezone = Some("America/New_York".into());
        })
        .gate(&[])
        .unwrap();
        // EDT: 09:30 New York is 13:30 UTC, inside the 13:00 hourly bar.
        assert!(g.holds(datetime!(2026-10-28 13:00 UTC), 3600));
        assert!(!g.holds(datetime!(2026-10-28 14:00 UTC), 3600));
        // EST: 14:30 UTC, inside the 14:00 bar.
        assert!(g.holds(datetime!(2026-11-04 14:00 UTC), 3600));
        assert!(!g.holds(datetime!(2026-11-04 13:00 UTC), 3600));
        // A daily bar holds every day: the clock reads 09:30 once in it.
        assert!(g.holds(datetime!(2026-11-04 00:00 UTC), 86_400));
    }

    #[test]
    fn scanner_time_refuses_what_it_cannot_judge() {
        assert!(TimeSpec::default().validate().is_err());
        assert!(spec(|s| s.from = Some("09:00".into())).validate().is_err());
        assert!(spec(|s| {
            s.from = Some("25:00".into());
            s.to = Some("10:00".into());
        })
        .validate()
        .is_err());
        assert!(spec(|s| {
            s.at = Some("09:00".into());
            s.timezone = Some("Nowhere/City".into());
        })
        .validate()
        .is_err());
        let gone = spec(|s| s.sessions = vec![Uuid::new_v4()]).gate(&[us()]).unwrap_err();
        assert!(gone.contains("deleted"));
    }

    #[test]
    fn scanner_time_day_and_session_keys() {
        let tz = schedule::timezone("America/New_York").unwrap();
        let ts = [datetime!(2026-11-04 03:00 UTC), datetime!(2026-11-04 06:00 UTC)];
        // The 03:00 UTC bar closes 22:59 New York on the 3rd, the 06:00 one 01:59 on the 4th.
        let k = day_keys(&ts, 3600, tz);
        assert_ne!(k[0], k[1]);
        let s = session_keys(&[datetime!(2026-11-04 15:00 UTC), datetime!(2026-11-04 23:00 UTC)], 3600, &us());
        assert!(s[0].is_some() && s[1].is_none());
    }
}
