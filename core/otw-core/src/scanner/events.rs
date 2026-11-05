//! Events: something that happened on a closed bar.
//!
//! A threshold event fires on the bar that crosses it, not on every bar past it: "up 1 %
//! from the open" is news once per session, and the cooldown is for the rest. The
//! detectors that are events by nature (a new high, a gap, a spike) fire on their bar.
//!
//! - `move_from_open{pct, direction}`: the close against the open of the session the bar
//!   belongs to (a market session when one is named, else the day in `timezone`; from 1d up,
//!   the bar's own open). A window that starts mid-session does not know that open, so it
//!   says nothing rather than measure from the wrong bar.
//! - `move{pct, bars, direction}`: the close against the close `bars` bars before.
//! - `new_high{bars}` / `new_low{bars}`: past the highest high (lowest low) of the bars before.
//! - `gap{pct, direction}`: the open against the previous close.
//! - `volume_spike{factor, period}`: volume at least `factor` times the mean of the `period`
//!   bars before.
//! - `range_expansion{factor, period}`: range at least `factor` times the mean range of the
//!   `period` bars before.
//! - `calendar{kind, within_hours}`: refused for now, with the reason (no module stores the
//!   dates it would read).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Direction, Ohlcv};

/// Longest window an event may ask for.
pub const MAX_BARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    MoveFromOpen {
        pct: f64,
        #[serde(default)]
        direction: Direction,
        /// A market session (`/api/market-sessions`) whose open is the anchor. Absent = the
        /// day in `timezone`.
        #[serde(default)]
        session_id: Option<Uuid>,
        /// IANA name; the day boundary when no session is named. Default UTC.
        #[serde(default)]
        timezone: Option<String>,
    },
    Move {
        pct: f64,
        bars: usize,
        #[serde(default)]
        direction: Direction,
    },
    NewHigh {
        bars: usize,
    },
    NewLow {
        bars: usize,
    },
    Gap {
        pct: f64,
        #[serde(default)]
        direction: Direction,
    },
    VolumeSpike {
        factor: f64,
        period: usize,
    },
    RangeExpansion {
        factor: f64,
        period: usize,
    },
    Calendar {
        /// `earnings` | `economic` (named `calendar`: `kind` is the event's own tag).
        calendar: String,
        within_hours: f64,
    },
}

/// Why a calendar event cannot be judged yet. Said in full rather than approximated: an
/// alert that guessed an earnings date would be worse than none.
pub const CALENDAR_REFUSAL: &str = "A calendar event cannot be judged yet: no module stores \
     earnings or economic release dates (the Calendar module shows them as embedded widgets, \
     and Fundamentals keeps past earnings only). Use a time signal for a known hour instead.";

impl Event {
    pub fn validate(&self) -> Result<(), String> {
        let pct = |p: f64| {
            if p.is_finite() && p > 0.0 {
                Ok(())
            } else {
                Err("the percent must be a number above 0".to_string())
            }
        };
        let bars = |b: usize, what: &str| {
            if (1..=MAX_BARS).contains(&b) {
                Ok(())
            } else {
                Err(format!("{what} goes from 1 to {MAX_BARS} bars"))
            }
        };
        let factor = |f: f64| {
            if f.is_finite() && f > 0.0 {
                Ok(())
            } else {
                Err("the factor must be a number above 0".to_string())
            }
        };
        match self {
            Event::MoveFromOpen { pct: p, timezone, .. } => {
                pct(*p)?;
                if let Some(tz) = timezone.as_deref().filter(|t| !t.trim().is_empty()) {
                    if crate::automator::schedule::timezone(tz.trim()).is_none() {
                        return Err(format!("unknown timezone \"{}\"", tz.trim()));
                    }
                }
                Ok(())
            }
            Event::Move { pct: p, bars: b, .. } => {
                pct(*p)?;
                bars(*b, "the move window")
            }
            Event::NewHigh { bars: b } | Event::NewLow { bars: b } => bars(*b, "the lookback"),
            Event::Gap { pct: p, .. } => pct(*p),
            Event::VolumeSpike { factor: f, period } | Event::RangeExpansion { factor: f, period } => {
                factor(*f)?;
                bars(*period, "the average")
            }
            Event::Calendar { .. } => Err(CALENDAR_REFUSAL.into()),
        }
    }

    /// Closed bars needed before the newest can be judged. `day_bars` is how many bars of
    /// this timeframe a day holds, the window a move from the open must see.
    pub fn lookback(&self, day_bars: usize) -> usize {
        match self {
            Event::MoveFromOpen { .. } => day_bars + 1,
            Event::Move { bars, .. } => bars + 1,
            Event::NewHigh { bars } | Event::NewLow { bars } => bars + 1,
            Event::Gap { .. } => 2,
            Event::VolumeSpike { period, .. } | Event::RangeExpansion { period, .. } => period + 1,
            Event::Calendar { .. } => 1,
        }
    }

    /// The measure the event compares, per bar (a percent, a ratio), when it has one.
    /// `anchor` gives, per bar, the index of the first bar of its session (`None` = outside
    /// a session or the open is not in the window); only `move_from_open` reads it.
    pub fn measure(&self, s: &Ohlcv, anchor: &[Option<usize>]) -> Vec<Option<f64>> {
        let n = s.len();
        let pct = |a: f64, b: f64| if b != 0.0 { Some((a / b - 1.0) * 100.0) } else { None };
        (0..n)
            .map(|i| match self {
                Event::MoveFromOpen { .. } => anchor.get(i).copied().flatten().and_then(|a| pct(s.close[i], s.open[a])),
                Event::Move { bars, .. } => (i >= *bars).then(|| pct(s.close[i], s.close[i - bars])).flatten(),
                Event::NewHigh { .. } | Event::NewLow { .. } => Some(s.close[i]),
                Event::Gap { .. } => (i >= 1).then(|| pct(s.open[i], s.close[i - 1])).flatten(),
                Event::VolumeSpike { period, .. } => mean_before(&s.volume, i, *period)
                    .and_then(|m| (m > 0.0).then(|| s.volume[i] / m)),
                Event::RangeExpansion { period, .. } => {
                    let ranges: Vec<f64> = (0..n).map(|k| s.range(k)).collect();
                    mean_before(&ranges, i, *period).and_then(|m| (m > 0.0).then(|| ranges[i] / m))
                }
                Event::Calendar { .. } => None,
            })
            .collect()
    }

    /// Whether the event happened on each bar.
    pub fn judge(&self, s: &Ohlcv, anchor: &[Option<usize>]) -> Vec<bool> {
        let n = s.len();
        let m = self.measure(s, anchor);
        (0..n)
            .map(|i| match self {
                Event::MoveFromOpen { pct, direction, .. } => {
                    let Some(now) = m[i] else { return false };
                    if !direction.reaches(now, *pct) {
                        return false;
                    }
                    // The first bar of the session has nothing before it in the session.
                    let first = anchor[i] == Some(i);
                    first || i == 0 || anchor[i - 1] != anchor[i] || !m[i - 1].is_some_and(|p| direction.reaches(p, *pct))
                }
                Event::Move { pct, direction, .. } => match (i.checked_sub(1).and_then(|p| m[p]), m[i]) {
                    (prev, Some(now)) => {
                        direction.reaches(now, *pct) && !prev.is_some_and(|p| direction.reaches(p, *pct))
                    }
                    _ => false,
                },
                Event::NewHigh { bars } => {
                    i >= *bars && s.high[i] > (i - bars..i).map(|k| s.high[k]).fold(f64::MIN, f64::max)
                }
                Event::NewLow { bars } => {
                    i >= *bars && s.low[i] < (i - bars..i).map(|k| s.low[k]).fold(f64::MAX, f64::min)
                }
                Event::Gap { pct, direction } => m[i].is_some_and(|g| direction.reaches(g, *pct)),
                Event::VolumeSpike { factor, .. } | Event::RangeExpansion { factor, .. } => {
                    m[i].is_some_and(|r| r >= *factor)
                }
                Event::Calendar { .. } => false,
            })
            .collect()
    }

    pub fn describe(&self) -> String {
        match self {
            Event::MoveFromOpen { pct, direction, .. } => format!("{pct}% from the open, {}", direction.word()),
            Event::Move { pct, bars, direction } => format!("{pct}% over {bars} bars, {}", direction.word()),
            Event::NewHigh { bars } => format!("new {bars} bar high"),
            Event::NewLow { bars } => format!("new {bars} bar low"),
            Event::Gap { pct, direction } => format!("gap of {pct}%, {}", direction.word()),
            Event::VolumeSpike { factor, period } => format!("volume {factor}x its {period} bar average"),
            Event::RangeExpansion { factor, period } => format!("range {factor}x its {period} bar average"),
            Event::Calendar { calendar, within_hours } => format!("{calendar} within {within_hours} h"),
        }
    }
}

fn mean_before(v: &[f64], i: usize, period: usize) -> Option<f64> {
    if period == 0 || i < period {
        return None;
    }
    Some(v[i - period..i].iter().sum::<f64>() / period as f64)
}

/// Per bar, the index of the first bar of its session, from a session key per bar (`None` =
/// the bar is outside any session). The open of a session that started before the window is
/// unknown, so its bars get `None`: `first_is_open` says whether bar 0 is the first bar of
/// its session.
pub fn anchors(keys: &[Option<i64>], first_is_open: bool) -> Vec<Option<usize>> {
    let mut out = Vec::with_capacity(keys.len());
    let mut current: Option<(i64, Option<usize>)> = None;
    for (i, k) in keys.iter().enumerate() {
        match k {
            None => out.push(None),
            Some(k) => {
                let a = match current {
                    Some((ck, a)) if ck == *k => a,
                    // A new session: bar i opens it, unless it is bar 0 and the session
                    // started before the window did.
                    _ => {
                        if i == 0 && !first_is_open {
                            None
                        } else {
                            Some(i)
                        }
                    }
                };
                current = Some((*k, a));
                out.push(a);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::fixtures::{bars, closes};
    use crate::scanner::Ohlcv;

    fn on(e: &Event, s: &Ohlcv) -> Vec<usize> {
        let keys: Vec<Option<i64>> = vec![Some(0); s.len()];
        let a = anchors(&keys, true);
        e.judge(s, &a).iter().enumerate().filter(|(_, h)| **h).map(|(i, _)| i).collect()
    }

    #[test]
    fn scanner_event_move_from_open_fires_once_on_the_crossing_bar() {
        // One session: open 100, then 100.5, 101.2 (crosses 1 %), 101.5 (still past), 99.
        let s = bars(&[
            (100.0, 100.6, 99.8, 100.5),
            (100.5, 101.3, 100.4, 101.2),
            (101.2, 101.6, 101.0, 101.5),
            (101.5, 101.5, 98.8, 99.0),
        ]);
        let e = Event::MoveFromOpen { pct: 1.0, direction: Direction::Up, session_id: None, timezone: None };
        assert_eq!(on(&e, &s), vec![1]);
        let down = Event::MoveFromOpen { pct: 1.0, direction: Direction::Down, session_id: None, timezone: None };
        assert_eq!(on(&down, &s), vec![3]);
    }

    #[test]
    fn scanner_event_move_from_open_restarts_at_a_session_open() {
        // Two sessions: the second one opens at 110 and its first bar is already up 1.5 %.
        let s = bars(&[
            (100.0, 101.5, 99.9, 101.2),
            (101.2, 101.4, 101.0, 101.3),
            (110.0, 111.8, 109.9, 111.65),
            (111.65, 111.9, 111.5, 111.8),
        ]);
        let keys = vec![Some(1), Some(1), Some(2), Some(2)];
        let a = anchors(&keys, true);
        assert_eq!(a, vec![Some(0), Some(0), Some(2), Some(2)]);
        let e = Event::MoveFromOpen { pct: 1.0, direction: Direction::Up, session_id: None, timezone: None };
        let hits: Vec<usize> = e.judge(&s, &a).iter().enumerate().filter(|(_, h)| **h).map(|(i, _)| i).collect();
        // Bar 0 crosses in session 1, bar 2 crosses in session 2 (measured from 110, not 100).
        assert_eq!(hits, vec![0, 2]);
        let m = e.measure(&s, &a);
        assert!((m[2].unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn scanner_event_move_from_open_says_nothing_when_the_open_is_not_in_the_window() {
        // The window starts mid-session: its first bar is not the open.
        let keys = vec![Some(7), Some(7), None, Some(8)];
        let a = anchors(&keys, false);
        assert_eq!(a, vec![None, None, None, Some(3)]);
        let s = closes(&[100.0, 105.0, 106.0, 107.0]);
        let e = Event::MoveFromOpen { pct: 1.0, direction: Direction::Either, session_id: None, timezone: None };
        let held = e.judge(&s, &a);
        assert!(!held[0] && !held[1] && !held[2]);
    }

    #[test]
    fn scanner_event_gap() {
        let s = bars(&[(100.0, 101.0, 99.0, 100.0), (102.5, 103.0, 102.0, 102.8), (102.7, 103.0, 102.0, 102.5)]);
        let up = Event::Gap { pct: 2.0, direction: Direction::Up };
        assert_eq!(on(&up, &s), vec![1]);
        assert!(on(&Event::Gap { pct: 2.0, direction: Direction::Down }, &s).is_empty());
        let s = bars(&[(100.0, 101.0, 99.0, 100.0), (97.0, 98.0, 96.0, 97.5)]);
        assert_eq!(on(&Event::Gap { pct: 2.0, direction: Direction::Either }, &s), vec![1]);
    }

    #[test]
    fn scanner_event_move_over_bars_fires_on_the_crossing() {
        let s = closes(&[100.0, 101.0, 103.5, 104.0, 104.5]);
        let e = Event::Move { pct: 3.0, bars: 2, direction: Direction::Up };
        // Bar 2: +3.5 % over 2 bars (first past 3 %). Bar 3: +2.97 %. Bar 4: +0.97 %.
        assert_eq!(on(&e, &s), vec![2]);
    }

    #[test]
    fn scanner_event_new_high_and_low() {
        let s = bars(&[
            (10.0, 11.0, 9.0, 10.0),
            (10.0, 10.5, 9.5, 10.2),
            (10.2, 11.5, 10.0, 11.2),
            (11.2, 11.3, 8.5, 9.0),
        ]);
        assert_eq!(on(&Event::NewHigh { bars: 2 }, &s), vec![2]);
        assert_eq!(on(&Event::NewLow { bars: 3 }, &s), vec![3]);
    }

    #[test]
    fn scanner_event_volume_spike_and_range_expansion() {
        let mut s = closes(&[10.0, 10.1, 10.0, 10.1, 10.0]);
        s.volume = vec![100.0, 100.0, 100.0, 350.0, 100.0];
        let spike = Event::VolumeSpike { factor: 3.0, period: 3 };
        assert_eq!(on(&spike, &s), vec![3]);
        let mut s = bars(&[
            (10.0, 10.5, 9.5, 10.0),
            (10.0, 10.5, 9.5, 10.1),
            (10.1, 10.6, 9.6, 10.0),
            (10.0, 12.0, 9.0, 11.5),
        ]);
        s.volume = vec![1.0; 4];
        assert_eq!(on(&Event::RangeExpansion { factor: 2.5, period: 3 }, &s), vec![3]);
        assert!(on(&Event::RangeExpansion { factor: 4.0, period: 3 }, &s).is_empty());
    }

    #[test]
    fn scanner_event_calendar_is_refused_with_the_module_named() {
        let e = Event::Calendar { calendar: "earnings".into(), within_hours: 24.0 };
        let why = e.validate().unwrap_err();
        assert!(why.contains("Calendar module"), "{why}");
        assert!(Event::Gap { pct: 0.0, direction: Direction::Up }.validate().is_err());
        assert!(Event::Move { pct: 1.0, bars: 0, direction: Direction::Up }.validate().is_err());
        assert!(Event::MoveFromOpen { pct: 1.0, direction: Direction::Up, session_id: None, timezone: Some("Mars/Base".into()) }
            .validate()
            .is_err());
    }
}
