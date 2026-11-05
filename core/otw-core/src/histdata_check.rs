//! Integrity check of a stored dataset: where candles are missing, and how a download would
//! ask for them again.
//!
//! "Missing" needs to know when the market trades, and no calendar is asked for that: the
//! series tells it itself.
//!
//! - **Round the clock**: filed as crypto, or holding Saturday candles (no exchange-hours
//!   market prints on a Saturday). Every step without a candle is one missing.
//! - **Sessions**: anything else. A slot of the week (Tuesday 14:00 for hourly bars) is
//!   expected when most of the neighbouring weeks hold a candle there. Nights, weekends and
//!   the daily break of a future are never expected, and a session that moves with daylight
//!   saving moves its expectation along, because the vote is local (four weeks either side)
//!   rather than taken over the whole history.
//!
//! What is missing is then sorted by how likely a download is to find it:
//!
//! - `holiday`: on an exchange-hours market, one or two sessions gone whole, or a session
//!   ending early (at most half of it): the shape of a public holiday or a half day;
//! - `quiet`: a silence under half an hour beyond the step, what an illiquid instrument
//!   leaves when nobody trades (many providers publish no candle for that minute);
//! - `missing`: everything else.

use std::collections::HashMap;

use serde::Serialize;

const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;
/// Weeks start on Saturday 00:00 UTC (1970-01-03), when no exchange-hours market trades, so a
/// session never straddles two of them.
const ORIGIN: i64 = 2 * DAY;
/// Neighbouring weeks on each side that vote on a slot.
const VOTE_RADIUS: i64 = 4;
/// A silence shorter than this beyond the step is `quiet`.
const QUIET_SECS: i64 = 30 * 60;
/// The least a half day takes off a session.
const EARLY_CLOSE_SECS: i64 = 2 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Missing,
    Holiday,
    Quiet,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "missing" => Some(Self::Missing),
            "holiday" => Some(Self::Holiday),
            "quiet" => Some(Self::Quiet),
            _ => None,
        }
    }
}

/// One hole: the bars either side of it (Unix seconds) and the candles expected in between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finding {
    pub before: i64,
    pub after: i64,
    pub bars: i64,
    pub kind: Kind,
}

#[derive(Debug)]
pub struct Scan {
    pub around_the_clock: bool,
    pub findings: Vec<Finding>,
}

/// The holes of a series of bar stamps (Unix seconds, oldest first) of `step` seconds.
pub fn scan(epochs: &[i64], step: i64, crypto: bool) -> Scan {
    // A share, not any(): a provider that stamps a few Friday closes on Saturday must not turn
    // an equity into a round-the-clock market. A 24/7 series has one bar in seven there.
    let saturdays = epochs.iter().filter(|&&e| weekday(e) == 6).count();
    let around_the_clock = crypto || saturdays * 20 > epochs.len();
    let mut findings = Vec::new();
    if epochs.len() < 2 || step <= 0 {
        return Scan { around_the_clock, findings };
    }
    if around_the_clock || step >= WEEK {
        for w in epochs.windows(2) {
            let (a, b) = (w[0], w[1]);
            // Rounded, so a bar stamped a few seconds late is not a hole.
            let bars = (b - a + step / 2) / step - 1;
            if bars <= 0 {
                continue;
            }
            let kind = if around_the_clock && b - a - step < QUIET_SECS { Kind::Quiet } else { Kind::Missing };
            findings.push(Finding { before: a, after: b, bars, kind });
        }
        return Scan { around_the_clock, findings };
    }

    let mut profile = Profile::new(epochs, step);
    for w in epochs.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut bars = 0;
        // Sessions the hole runs to the end of, and whether every missing slot belongs to one.
        // Only the session the hole opens in can have started: that is an early close.
        let mut sessions: Vec<(i64, usize)> = Vec::new();
        let mut whole = true;
        let (mut tail, mut tail_len) = (0i64, 0i64);
        // Slot by slot, not second by second: a bar stamped off the grid (a half hour, a
        // partial last candle) must not shift which slots lie between `a` and `b`.
        let n = profile.nslots as i64;
        let slot_of = |t: i64| {
            let (wk, s) = profile.locate(t);
            wk * n + s as i64
        };
        let (ga, gb) = (slot_of(a), slot_of(b));
        for g in ga + 1..gb {
            let (wk, s) = (g.div_euclid(n), g.rem_euclid(n) as usize);
            let week = profile.week(wk);
            if !week.expected[s] {
                continue;
            }
            bars += 1;
            let (r0, r1) = (wk * n + week.run_start[s] as i64, wk * n + week.run_end[s] as i64);
            if r1 < gb {
                if !sessions.contains(&(wk, week.run_start[s])) {
                    sessions.push((wk, week.run_start[s]));
                }
                if r0 <= ga {
                    tail += 1;
                    tail_len = r1 - r0 + 1;
                }
            } else {
                whole = false;
            }
        }
        if bars == 0 {
            continue;
        }
        // An early close cuts hours, at most half the session: one candle short of a break
        // is a candle missing.
        let early_close = tail == 0 || (tail * 2 <= tail_len && tail * step >= EARLY_CLOSE_SECS);
        let kind = if b - a - step < QUIET_SECS {
            Kind::Quiet
        } else if whole && early_close && (1..=2).contains(&sessions.len()) {
            Kind::Holiday
        } else {
            Kind::Missing
        };
        findings.push(Finding { before: a, after: b, bars, kind });
    }
    Scan { around_the_clock, findings }
}

/// What a session market is expected to print, learnt week by week from the series.
struct Profile {
    step: i64,
    nslots: usize,
    first_week: i64,
    /// Per week since `first_week`: which slots hold a candle.
    present: Vec<Vec<bool>>,
    active: Vec<bool>,
    /// Candles per slot over every active week: the fallback vote of a week without enough
    /// active neighbours. Built on first use.
    global: Option<(Vec<u32>, usize)>,
    weeks: HashMap<i64, Week>,
}

struct Week {
    expected: Vec<bool>,
    /// For each expected slot, the first and last slot of its session.
    run_start: Vec<usize>,
    run_end: Vec<usize>,
}

impl Profile {
    fn new(epochs: &[i64], step: i64) -> Self {
        let nslots = ((WEEK + step - 1) / step) as usize;
        let week_of = |e: i64| (e - ORIGIN).div_euclid(WEEK);
        let first_week = week_of(epochs[0]);
        let nweeks = (week_of(epochs[epochs.len() - 1]) - first_week + 1).max(1) as usize;
        let mut p = Self {
            step,
            nslots,
            first_week,
            present: vec![Vec::new(); nweeks],
            active: vec![false; nweeks],
            global: None,
            weeks: HashMap::new(),
        };
        for &e in epochs {
            let (wk, s) = p.locate(e);
            let i = (wk - first_week) as usize;
            if !p.active[i] {
                p.active[i] = true;
                p.present[i] = vec![false; nslots];
            }
            p.present[i][s] = true;
        }
        p
    }

    fn locate(&self, t: i64) -> (i64, usize) {
        let off = t - ORIGIN;
        let s = (off.rem_euclid(WEEK) / self.step) as usize;
        (off.div_euclid(WEEK), s.min(self.nslots - 1))
    }

    fn week(&mut self, wk: i64) -> &Week {
        if !self.weeks.contains_key(&wk) {
            let w = self.build(wk);
            self.weeks.insert(wk, w);
        }
        &self.weeks[&wk]
    }

    fn build(&mut self, wk: i64) -> Week {
        let idx = wk - self.first_week;
        let n = self.present.len() as i64;
        // The week itself never votes: a week that lost its Tuesday must not vote it away.
        let mut votes = vec![0u32; self.nslots];
        let mut voters = 0usize;
        for i in (idx - VOTE_RADIUS).max(0)..=(idx + VOTE_RADIUS).min(n - 1) {
            if i == idx || !self.active[i as usize] {
                continue;
            }
            voters += 1;
            for (v, &p) in votes.iter_mut().zip(&self.present[i as usize]) {
                *v += p as u32;
            }
        }
        if voters < 2 {
            let (all, count) = self.global.get_or_insert_with(|| {
                let mut all = vec![0u32; self.nslots];
                let mut count = 0;
                for (i, p) in self.present.iter().enumerate() {
                    if self.active[i] {
                        count += 1;
                        for (v, &x) in all.iter_mut().zip(p) {
                            *v += x as u32;
                        }
                    }
                }
                (all, count)
            });
            votes = all.clone();
            voters = *count;
            if (0..n).contains(&idx) && self.active[idx as usize] {
                voters -= 1;
                for (v, &p) in votes.iter_mut().zip(&self.present[idx as usize]) {
                    *v -= p as u32;
                }
            }
        }
        // Strict majority: across a daylight-saving change the old and new hours tie for a
        // week, and a tie expects neither.
        let expected: Vec<bool> = if voters < 2 {
            vec![false; self.nslots]
        } else {
            votes.iter().map(|&v| v as usize * 2 > voters).collect()
        };
        // A daily slot is its own session; intraday, a session is a run of expected slots.
        let mut run_start: Vec<usize> = (0..self.nslots).collect();
        let mut run_end: Vec<usize> = (0..self.nslots).collect();
        if self.step < DAY {
            for s in 1..self.nslots {
                if expected[s] && expected[s - 1] {
                    run_start[s] = run_start[s - 1];
                }
            }
            for s in (0..self.nslots - 1).rev() {
                if expected[s] && expected[s + 1] {
                    run_end[s] = run_end[s + 1];
                }
            }
        }
        Week { expected, run_start, run_end }
    }
}

/// One download that fills one or more holes: `[from, to)` in Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub from: i64,
    pub to: i64,
    pub holes: Vec<(i64, i64)>,
}

/// Provider requests a window of `span` seconds costs, paging `page` seconds at a time.
pub fn chunks(span: i64, page: i64) -> i64 {
    let page = page.max(1);
    ((span.max(0) + page - 1) / page).max(1)
}

/// The downloads that fill these holes (bar before, bar after), at most `max_jobs` of them.
///
/// Two neighbouring holes share a download whenever that costs no more requests than asking
/// for each: refetching the stored bars between them is free when they sit on the same page.
/// Past `max_jobs`, the narrowest separations are bridged too, which costs requests but keeps
/// the queue readable.
pub fn plan(holes: &[(i64, i64)], step: i64, page: i64, max_jobs: usize) -> Vec<Window> {
    let mut sorted = holes.to_vec();
    sorted.sort_unstable();
    let mut out: Vec<Window> = Vec::new();
    for (a, b) in sorted {
        let (from, to) = (a + step, b);
        if from >= to {
            continue;
        }
        if let Some(w) = out.last_mut() {
            let joined = chunks(to.max(w.to) - w.from, page);
            if joined <= chunks(w.to - w.from, page) + chunks(to - from, page) {
                w.to = w.to.max(to);
                w.holes.push((a, b));
                continue;
            }
        }
        out.push(Window { from, to, holes: vec![(a, b)] });
    }
    let max_jobs = max_jobs.max(1);
    if out.len() > max_jobs {
        let mut seps: Vec<(i64, usize)> =
            out.windows(2).enumerate().map(|(i, w)| (w[1].from - w[0].to, i)).collect();
        seps.sort_unstable();
        let mut join = vec![false; out.len()];
        for &(_, i) in &seps[..out.len() - max_jobs] {
            join[i + 1] = true;
        }
        let mut merged: Vec<Window> = Vec::with_capacity(max_jobs);
        for (i, w) in out.into_iter().enumerate() {
            match merged.last_mut() {
                Some(last) if join[i] => {
                    last.to = last.to.max(w.to);
                    last.holes.extend(w.holes);
                }
                _ => merged.push(w),
            }
        }
        out = merged;
    }
    out
}

/// Day of the week of an epoch second, UTC: 0 = Sunday … 6 = Saturday.
fn weekday(e: i64) -> i64 {
    // 1970-01-01 was a Thursday.
    (e.div_euclid(DAY) + 4).rem_euclid(7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{format_description::well_known::Rfc3339, OffsetDateTime, Weekday::*};

    fn at(s: &str) -> i64 {
        OffsetDateTime::parse(s, &Rfc3339).unwrap().unix_timestamp()
    }

    fn series(start: &str, step: i64, n: usize, skip: impl Fn(OffsetDateTime) -> bool) -> Vec<i64> {
        let mut t = at(start);
        let mut out = Vec::new();
        while out.len() < n {
            if !skip(OffsetDateTime::from_unix_timestamp(t).unwrap()) {
                out.push(t);
            }
            t += step;
        }
        out
    }

    fn kinds(s: &Scan) -> Vec<(Kind, i64)> {
        s.findings.iter().map(|f| (f.kind, f.bars)).collect()
    }

    #[test]
    fn crypto_counts_every_missing_step_and_calls_a_lull_quiet() {
        let mut ts = series("2026-01-01T00:00:00Z", 60, 5000, |_| false);
        ts.drain(100..105); // five minutes without a trade
        ts.drain(2000..2600); // ten hours lost
        let s = scan(&ts, 60, true);
        assert!(s.around_the_clock);
        assert_eq!(kinds(&s), [(Kind::Quiet, 5), (Kind::Missing, 600)]);
        // A missing weekend day on daily bars, found without being told it is crypto.
        let mut d = series("2026-01-01T00:00:00Z", DAY, 90, |_| false);
        d.retain(|&e| e != at("2026-02-07T00:00:00Z"));
        assert_eq!(kinds(&scan(&d, DAY, false)), [(Kind::Missing, 1)]);
    }

    #[test]
    fn equity_nights_and_weekends_are_not_holes() {
        let hourly = series("2026-01-05T14:00:00Z", 3600, 1200, |t| {
            matches!(t.weekday(), Saturday | Sunday) || !(14..=20).contains(&t.hour())
        });
        let s = scan(&hourly, 3600, false);
        assert!(!s.around_the_clock);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        let daily = series("2026-01-05T00:00:00Z", DAY, 200, |t| matches!(t.weekday(), Saturday | Sunday));
        assert!(scan(&daily, DAY, false).findings.is_empty());
    }

    #[test]
    fn a_future_missing_a_weekday_afternoon_is_found() {
        // Sunday 23:00 to Friday 22:00 UTC with a daily break at 22:00.
        let open = |t: OffsetDateTime| match t.weekday() {
            Saturday => false,
            Sunday => t.hour() == 23,
            Friday => t.hour() < 22,
            _ => t.hour() != 22,
        };
        let mut fut = series("2026-01-04T23:00:00Z", 3600, 2000, |t| !open(t));
        let s = scan(&fut, 3600, false);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        // Wednesday 2026-02-11, 12:00 to 17:00 gone, and a lone quarter hour on 15m bars.
        fut.retain(|&e| !(at("2026-02-11T12:00:00Z")..at("2026-02-11T17:00:00Z")).contains(&e));
        assert_eq!(kinds(&scan(&fut, 3600, false)), [(Kind::Missing, 5)]);
        let mut q = series("2026-01-04T23:00:00Z", 900, 8000, |t| !open(t));
        q.retain(|&e| e != at("2026-02-11T09:15:00Z"));
        assert_eq!(kinds(&scan(&q, 900, false)), [(Kind::Quiet, 1)]);
    }

    #[test]
    fn a_whole_closed_day_reads_as_a_holiday() {
        let mut daily = series("2026-01-05T00:00:00Z", DAY, 200, |t| matches!(t.weekday(), Saturday | Sunday));
        daily.retain(|&e| e != at("2026-02-16T00:00:00Z"));
        assert_eq!(kinds(&scan(&daily, DAY, false)), [(Kind::Holiday, 1)]);
        // A whole week gone is data loss, not a holiday.
        daily.retain(|&e| !(at("2026-03-02T00:00:00Z")..at("2026-03-07T00:00:00Z")).contains(&e));
        assert_eq!(kinds(&scan(&daily, DAY, false)), [(Kind::Holiday, 1), (Kind::Missing, 5)]);
        // Intraday: a session missing whole is a holiday, half of one is not.
        let mut hourly = series("2026-01-05T14:00:00Z", 3600, 1200, |t| {
            matches!(t.weekday(), Saturday | Sunday) || !(14..=20).contains(&t.hour())
        });
        hourly.retain(|&e| !(at("2026-02-16T00:00:00Z")..at("2026-02-17T00:00:00Z")).contains(&e));
        hourly.retain(|&e| !(at("2026-02-18T17:00:00Z")..at("2026-02-18T21:00:00Z")).contains(&e));
        // A half day: the session closes three hours early.
        hourly.retain(|&e| !(at("2026-02-20T18:00:00Z")..at("2026-02-20T21:00:00Z")).contains(&e));
        assert_eq!(kinds(&scan(&hourly, 3600, false)), [(Kind::Holiday, 7), (Kind::Missing, 4), (Kind::Holiday, 3)]);
    }

    #[test]
    fn a_session_that_moves_with_daylight_saving_is_followed() {
        // 14:00 to 20:00 UTC in winter, 13:00 to 19:00 from 2026-03-09.
        let switch = at("2026-03-09T00:00:00Z");
        let ts = series("2026-01-05T13:00:00Z", 3600, 1500, |t| {
            let h = t.hour();
            let hours = if t.unix_timestamp() < switch { 14..=20 } else { 13..=19 };
            matches!(t.weekday(), Saturday | Sunday) || !hours.contains(&h)
        });
        let s = scan(&ts, 3600, false);
        assert!(s.findings.is_empty(), "{:?}", s.findings);
    }

    #[test]
    fn neighbouring_holes_share_a_download_when_it_is_free() {
        let step = 60;
        let page = 1000 * step;
        // Two holes on the same page, one far away.
        let holes = [(0, 10 * step), (50 * step, 60 * step), (100_000 * step, 100_010 * step)];
        let w = plan(&holes, step, page, 200);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].from, w[0].to, w[0].holes.len()), (step, 60 * step, 2));
        assert_eq!(chunks(w[1].to - w[1].from, page), 1);
        // Capped at one job: everything is bridged.
        let one = plan(&holes, step, page, 1);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].holes.len(), 3);
    }
}
