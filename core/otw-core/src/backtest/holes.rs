//! Holes in a bar series: stretches where the market traded but no candle was stored.
//!
//! A hole is not a closed market. A weekend in an equity series or the nightly break of a
//! future is the market saying nothing; a day missing from a crypto series is the *data*
//! saying nothing. The engine cannot carry a position across the second kind: whatever the
//! price did in there is unknown, so the trade's exit, and every indicator read on the far
//! side until it has warmed up again, would be made up.
//!
//! No calendar is needed to tell the two apart well enough, only two facts the series gives
//! away about itself:
//!
//! - **its bar length**, the median spacing (immune to the gaps it is looking for);
//! - **whether it trades around the clock**: a series holding Saturday candles does. No
//!   exchange-hours market prints on a Saturday (futures and forex reopen on Sunday evening),
//!   so this splits crypto from everything else without being told the asset type.
//!
//! A round-the-clock series has a hole wherever more than half a bar is missing beyond the
//! normal step and at least 30 minutes went by, so a quiet minute on an illiquid coin (some
//! providers publish nothing for a minute without trades) is not one. Any other series only
//! has a hole when the silence outlasts the longest closure a market takes on its own: a
//! long holiday weekend is four days, so five is the bar.

/// What a series must go without, beyond its own step, before a round-the-clock gap is a hole.
const MIN_SILENCE_SECS: i64 = 30 * 60;
/// The longest gap an exchange-hours market leaves on its own (Friday close to Tuesday open
/// over a holiday Monday, plus slack).
const CLOSED_MARKET_SECS: i64 = 5 * 86_400;

/// One hole: the bar before it and the bar after it, as indices into the series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hole {
    /// Last bar before the silence.
    pub before: usize,
    /// First bar after it.
    pub after: usize,
}

/// The holes in a series of RFC3339 stamps, oldest first. Unparseable stamps never make one.
pub fn find(ts: &[String]) -> Vec<Hole> {
    let epochs: Vec<Option<i64>> = ts.iter().map(|t| epoch(t)).collect();
    let mut steps: Vec<i64> = epochs
        .windows(2)
        .filter_map(|w| match (w[0], w[1]) {
            (Some(a), Some(b)) if b > a => Some(b - a),
            _ => None,
        })
        .collect();
    if steps.len() < 2 {
        return Vec::new();
    }
    steps.sort_unstable();
    let step = steps[steps.len() / 2];
    let around_the_clock = epochs.iter().flatten().any(|&e| weekday(e) == 6);
    let threshold = if around_the_clock {
        (step + step / 2).max(step + MIN_SILENCE_SECS)
    } else {
        CLOSED_MARKET_SECS.max(step + step / 2)
    };
    epochs
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| match (w[0], w[1]) {
            (Some(a), Some(b)) if b - a >= threshold => Some(Hole { before: i, after: i + 1 }),
            _ => None,
        })
        .collect()
}

/// Per bar: true when the bar is the last one before a hole.
pub fn before_mask(n: usize, holes: &[Hole]) -> Vec<bool> {
    let mut m = vec![false; n];
    for h in holes {
        if h.before < n {
            m[h.before] = true;
        }
    }
    m
}

/// Per bar: true while no entry may fill there because the strategy's inputs straddle a hole.
///
/// An order fills at a bar's open on what the previous closes said, so the first bar after a
/// hole is always cold (its signal was read before the silence), and so is every bar until
/// `warmup` clean bars have closed on the far side: an indicator over 50 periods needs 50
/// periods of data that actually followed each other.
pub fn cold_mask(n: usize, holes: &[Hole], warmup: usize) -> Vec<bool> {
    let mut m = vec![false; n];
    for h in holes {
        let end = (h.after + warmup.max(1)).min(n);
        for v in &mut m[h.after.min(n)..end] {
            *v = true;
        }
    }
    m
}

/// Seconds since the epoch for `YYYY-MM-DDTHH:MM:SS…Z` without a full parse (this runs on
/// every bar of every optimizer trial), falling back to the RFC3339 parser for any other shape.
fn epoch(t: &str) -> Option<i64> {
    let b = t.as_bytes();
    let fast = b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[b.len() - 1] == b'Z';
    if fast {
        let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
        let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
        let (h, mi, s) = (n(11..13)?, n(14..16)?, n(17..19)?);
        return Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s);
    }
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    OffsetDateTime::parse(t, &Rfc3339).ok().map(|d| d.unix_timestamp())
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Day of the week of an epoch second, UTC: 0 = Sunday … 6 = Saturday.
fn weekday(e: i64) -> i64 {
    // 1970-01-01 was a Thursday.
    (e.div_euclid(86_400) + 4).rem_euclid(7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

    fn series(start: &str, step_secs: i64, n: usize, skip: impl Fn(OffsetDateTime) -> bool) -> Vec<String> {
        let mut t = OffsetDateTime::parse(start, &Rfc3339).unwrap();
        let mut out = Vec::new();
        while out.len() < n {
            if !skip(t) {
                out.push(t.format(&Rfc3339).unwrap());
            }
            t += Duration::seconds(step_secs);
        }
        out
    }

    #[test]
    fn fast_epoch_matches_the_parser() {
        for s in ["2026-09-06T14:06:00Z", "1999-12-31T23:59:59Z", "2024-02-29T00:00:00Z"] {
            let slow = OffsetDateTime::parse(s, &Rfc3339).unwrap().unix_timestamp();
            assert_eq!(epoch(s), Some(slow), "{s}");
        }
        assert_eq!(epoch("2026-09-06T14:06:00+02:00"), Some(OffsetDateTime::parse("2026-09-06T14:06:00+02:00", &Rfc3339).unwrap().unix_timestamp()));
        assert_eq!(weekday(OffsetDateTime::parse("2026-10-03T00:00:00Z", &Rfc3339).unwrap().unix_timestamp()), 6);
    }

    #[test]
    fn crypto_minutes_find_the_missing_month_but_not_a_quiet_minute() {
        let mut ts = series("2026-09-05T00:00:00Z", 60, 3000, |_| false);
        // A five-minute lull: not a hole.
        ts.drain(100..105);
        // The paper session's 27 days.
        ts.push("2026-10-03T16:01:00Z".into());
        ts.push("2026-10-03T16:02:00Z".into());
        let holes = find(&ts);
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].after, ts.len() - 2);
    }

    #[test]
    fn crypto_days_flag_a_single_missing_day() {
        let ts = series("2026-01-01T00:00:00Z", 86_400, 60, |t| t.date().day() == 15 && t.month() as u8 == 1);
        assert_eq!(find(&ts).len(), 1);
    }

    #[test]
    fn equity_weekends_and_nights_are_not_holes() {
        use time::Weekday::*;
        // Daily bars, weekdays only.
        let daily = series("2026-01-05T00:00:00Z", 86_400, 200, |t| matches!(t.weekday(), Saturday | Sunday));
        assert!(find(&daily).is_empty());
        // Hourly bars 14:00–20:00, weekdays only: nights and weekends.
        let hourly = series("2026-01-05T14:00:00Z", 3600, 600, |t| {
            matches!(t.weekday(), Saturday | Sunday) || !(14..=20).contains(&t.hour())
        });
        assert!(find(&hourly).is_empty());
        // A futures-style Sunday-evening reopen is still not round the clock.
        let mut fut = series("2026-01-04T23:00:00Z", 3600, 800, |t| t.weekday() == Saturday);
        assert!(find(&fut).is_empty());
        // Two missing weeks are a hole even there.
        fut.drain(150..150 + 24 * 12);
        assert_eq!(find(&fut).len(), 1);
    }

    #[test]
    fn masks_cover_the_bar_before_and_the_warmup_after() {
        let holes = [Hole { before: 4, after: 5 }];
        assert_eq!(before_mask(8, &holes), [false, false, false, false, true, false, false, false]);
        assert_eq!(cold_mask(8, &holes, 0), [false, false, false, false, false, true, false, false]);
        assert_eq!(cold_mask(8, &holes, 2), [false, false, false, false, false, true, true, false]);
        assert_eq!(cold_mask(8, &holes, 50), [false, false, false, false, false, true, true, true]);
    }
}
