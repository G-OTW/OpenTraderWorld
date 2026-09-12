//! Canonical timeframe: parsing, annualization and **period bucketing**.
//!
//! One type for a notion the codebase used to re-parse in three places with three different
//! vocabularies (`histdata::timeframe_secs`, `quant::periods_per_year`, and the alignment
//! code). Adding a timeframe is a line here, not a hunt.
//!
//! The reason this module exists at all is [`Timeframe::bucket`]: two providers stamp the
//! same daily close differently (Binance dates the candle at its open, 00:00 UTC; Alpaca and
//! Polygon at the session start, 00:00 New York), so a raw timestamp is *not* an identity for
//! "the same period". The bucket is: the period's opening instant in UTC.

use anyhow::{anyhow, Result};
use time::{Date, Month, OffsetDateTime, Time};

/// Seconds in a day, the anchor for every grid below.
const DAY: i64 = 86_400;
/// 1970-01-01 was a Thursday; the first Monday of the epoch is 1970-01-05.
const FIRST_MONDAY: i64 = 4 * DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

/// A count of units: `15m`, `4h`, `1d`, `1w`, `1M`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeframe {
    pub n: u32,
    pub unit: Unit,
}

impl Timeframe {
    pub const fn new(n: u32, unit: Unit) -> Self {
        Timeframe { n, unit }
    }

    /// Parse a timeframe string. The vocabulary is the one the app already accepted:
    /// `m`/`min` minutes, `h`/`hour`, `d`/`day`, `w`/`wk`/`week`, `mo`/`month`, and a
    /// **capital** `M` suffix for months (charting convention: lowercase `m` is minutes,
    /// so `1M` and `1m` are five orders of magnitude apart and must not be conflated).
    pub fn parse(s: &str) -> Result<Self> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err(anyhow!("empty timeframe"));
        }
        let capital_month = raw.ends_with('M');
        let lower = raw.to_lowercase();
        let split = lower.find(|c: char| c.is_alphabetic()).unwrap_or(lower.len());
        let (num, unit) = lower.split_at(split);
        let n: u32 = if num.is_empty() { 1 } else { num.parse().map_err(|_| anyhow!("bad timeframe count in {raw:?}"))? };
        if n == 0 {
            return Err(anyhow!("timeframe count must be > 0"));
        }
        let unit = if capital_month {
            Unit::Month
        } else {
            match unit {
                "m" | "min" => Unit::Minute,
                "h" | "hour" => Unit::Hour,
                "d" | "day" => Unit::Day,
                "w" | "wk" | "week" => Unit::Week,
                "mo" | "month" => Unit::Month,
                other => return Err(anyhow!("unknown timeframe unit {other:?}")),
            }
        };
        Ok(Timeframe { n, unit })
    }

    /// Seconds in one period, or `None` for months (not a fixed length).
    pub fn secs(&self) -> Option<i64> {
        let n = self.n as i64;
        Some(match self.unit {
            Unit::Minute => 60 * n,
            Unit::Hour => 3_600 * n,
            Unit::Day => DAY * n,
            Unit::Week => 7 * DAY * n,
            Unit::Month => return None,
        })
    }

    /// Recognize a timeframe from a fixed period length in seconds. Exact divisors only, no
    /// tolerance: this reads a clean series' own spacing, and a value that is not a whole
    /// number of a canonical unit is reported as unknown rather than rounded into one.
    /// The 27..=32 day window is the one exception, because a calendar month has no length.
    pub fn from_secs(secs: i64) -> Option<Self> {
        if secs <= 0 {
            return None;
        }
        if (27 * DAY..=32 * DAY).contains(&secs) {
            return Some(Timeframe::new(1, Unit::Month));
        }
        let week = 7 * DAY;
        if secs % week == 0 {
            return Some(Timeframe::new((secs / week) as u32, Unit::Week));
        }
        if secs % DAY == 0 {
            return Some(Timeframe::new((secs / DAY) as u32, Unit::Day));
        }
        if secs % 3_600 == 0 && secs < DAY {
            return Some(Timeframe::new((secs / 3_600) as u32, Unit::Hour));
        }
        if secs % 60 == 0 && secs < 3_600 {
            return Some(Timeframe::new((secs / 60) as u32, Unit::Minute));
        }
        None
    }

    /// Minutes and hours. Intraday periods are **not** alignable by bucketing across
    /// providers: a session-anchored 4h bar (09:30 New York) and an epoch-anchored one
    /// (08:00 UTC) are 90 minutes apart, and pairing them would be a silent lie. Callers
    /// use this to refuse the mix instead.
    pub fn is_intraday(&self) -> bool {
        matches!(self.unit, Unit::Minute | Unit::Hour)
    }

    /// Trading periods per year, the annualization factor of last resort. Prefer
    /// [`crate::align::observed_ppy`], which counts what the series actually contains:
    /// this table cannot know that a daily crypto series has 365 periods and a daily
    /// equity series 252. ~6.5 trading hours a day, 252 trading days a year.
    pub fn periods_per_year(&self) -> f64 {
        let n = self.n as f64;
        let per_year = match self.unit {
            Unit::Minute => 252.0 * 6.5 * 60.0 / n,
            Unit::Hour => 252.0 * 6.5 / n,
            Unit::Day => 252.0 / n,
            Unit::Week => 52.0 / n,
            Unit::Month => 12.0 / n,
        };
        per_year.max(1.0)
    }

    /// The opening instant, in UTC, of the period `ts` belongs to. This is the alignment
    /// identity: two bars are the same period when their buckets are equal.
    ///
    /// - minutes/hours/days: floored on a grid anchored at the Unix epoch (which *is*
    ///   midnight UTC, so `1d` floors to midnight).
    /// - weeks: floored to the ISO Monday, 00:00 UTC (the epoch's own week starts on a
    ///   Thursday, hence the four-day anchor).
    /// - months: the first of the month, 00:00 UTC.
    pub fn bucket(&self, ts: OffsetDateTime) -> OffsetDateTime {
        let ts = ts.to_offset(time::UtcOffset::UTC);
        match self.unit {
            Unit::Month => {
                let months = (ts.year() as i64) * 12 + (ts.month() as u8 as i64 - 1);
                let floored = months.div_euclid(self.n as i64) * self.n as i64;
                let (y, m) = (floored.div_euclid(12), floored.rem_euclid(12) + 1);
                let month = Month::try_from(m as u8).unwrap_or(Month::January);
                let date = Date::from_calendar_date(y as i32, month, 1).unwrap_or(ts.date());
                date.with_time(Time::MIDNIGHT).assume_utc()
            }
            Unit::Week => {
                let step = 7 * DAY * self.n as i64;
                let s = ts.unix_timestamp() - FIRST_MONDAY;
                let floored = s.div_euclid(step) * step + FIRST_MONDAY;
                OffsetDateTime::from_unix_timestamp(floored).unwrap_or(ts)
            }
            _ => {
                let step = self.secs().unwrap_or(DAY);
                let floored = ts.unix_timestamp().div_euclid(step) * step;
                OffsetDateTime::from_unix_timestamp(floored).unwrap_or(ts)
            }
        }
    }

    /// Canonical label (`"1d"`, `"15m"`, `"1M"`), stable enough to send to the frontend.
    pub fn label(&self) -> String {
        let u = match self.unit {
            Unit::Minute => "m",
            Unit::Hour => "h",
            Unit::Day => "d",
            Unit::Week => "w",
            Unit::Month => "M",
        };
        format!("{}{}", self.n, u)
    }
}

/// One calendar day, the granularity everything cross-asset settles on.
pub const DAILY: Timeframe = Timeframe::new(1, Unit::Day);
/// One ISO week: the frequency a mixed-calendar basket's covariance is measured at.
pub const WEEKLY: Timeframe = Timeframe::new(1, Unit::Week);

#[cfg(test)]
mod tests {
    use super::*;
    use time::format_description::well_known::Rfc3339;

    fn dt(s: &str) -> OffsetDateTime {
        OffsetDateTime::parse(s, &Rfc3339).unwrap()
    }
    fn bucket_str(tf: Timeframe, s: &str) -> String {
        tf.bucket(dt(s)).format(&Rfc3339).unwrap()
    }

    #[test]
    fn parses_the_vocabulary() {
        assert_eq!(Timeframe::parse("1d").unwrap(), Timeframe::new(1, Unit::Day));
        assert_eq!(Timeframe::parse("15m").unwrap(), Timeframe::new(15, Unit::Minute));
        assert_eq!(Timeframe::parse("4h").unwrap(), Timeframe::new(4, Unit::Hour));
        assert_eq!(Timeframe::parse("1w").unwrap(), Timeframe::new(1, Unit::Week));
        assert_eq!(Timeframe::parse(" 1D ").unwrap(), Timeframe::new(1, Unit::Day));
        assert_eq!(Timeframe::parse("2wk").unwrap(), Timeframe::new(2, Unit::Week));
        assert_eq!(Timeframe::parse("3month").unwrap(), Timeframe::new(3, Unit::Month));
    }

    #[test]
    fn capital_m_is_months_lowercase_is_minutes() {
        assert_eq!(Timeframe::parse("1M").unwrap(), Timeframe::new(1, Unit::Month));
        assert_eq!(Timeframe::parse("1m").unwrap(), Timeframe::new(1, Unit::Minute));
    }

    #[test]
    fn rejects_nonsense() {
        for bad in ["", "   ", "0d", "d0", "1x", "abc", "-1d", "1.5d"] {
            assert!(Timeframe::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn seconds_and_recognition_round_trip() {
        for s in ["1m", "5m", "15m", "1h", "4h", "1d", "1w"] {
            let tf = Timeframe::parse(s).unwrap();
            let secs = tf.secs().unwrap();
            assert_eq!(Timeframe::from_secs(secs), Some(tf), "{s}");
        }
        assert_eq!(Timeframe::parse("1M").unwrap().secs(), None);
        // A calendar month has no fixed length: every plausible one reads as a month.
        for days in [28, 29, 30, 31] {
            assert_eq!(
                Timeframe::from_secs(days * DAY),
                Some(Timeframe::new(1, Unit::Month)),
                "{days} days"
            );
        }
        // Not a whole number of any unit: unknown, never rounded.
        assert_eq!(Timeframe::from_secs(90_000), None);
        assert_eq!(Timeframe::from_secs(0), None);
        assert_eq!(Timeframe::from_secs(-86_400), None);
    }

    #[test]
    fn daily_bucket_is_midnight_utc_whatever_the_provider_stamps() {
        // Binance stamps the candle open, Alpaca the session start, Polygon 04:00 UTC in
        // winter: same day, three stamps, one bucket.
        for s in ["2015-01-02T00:00:00Z", "2015-01-02T04:00:00Z", "2015-01-02T05:00:00Z", "2015-01-02T23:59:59Z"] {
            assert_eq!(bucket_str(DAILY, s), "2015-01-02T00:00:00Z", "{s}");
        }
    }

    #[test]
    fn weekly_bucket_is_the_iso_monday() {
        // Mon 2020-12-28 → Sun 2021-01-03 is one ISO week, across the year boundary.
        for s in [
            "2020-12-28T00:00:00Z",
            "2020-12-31T21:00:00Z",
            "2021-01-01T14:30:00Z",
            "2021-01-03T23:59:59Z",
        ] {
            assert_eq!(bucket_str(WEEKLY, s), "2020-12-28T00:00:00Z", "{s}");
        }
        assert_eq!(bucket_str(WEEKLY, "2021-01-04T00:00:00Z"), "2021-01-04T00:00:00Z");
    }

    #[test]
    fn intraday_buckets_floor_on_the_epoch_grid() {
        let h4 = Timeframe::parse("4h").unwrap();
        assert_eq!(bucket_str(h4, "2021-03-04T09:30:00Z"), "2021-03-04T08:00:00Z");
        assert_eq!(bucket_str(h4, "2021-03-04T00:00:00Z"), "2021-03-04T00:00:00Z");
        let m15 = Timeframe::parse("15m").unwrap();
        assert_eq!(bucket_str(m15, "2021-03-04T09:44:59Z"), "2021-03-04T09:30:00Z");
    }

    #[test]
    fn monthly_bucket_is_the_first_of_the_month() {
        let m = Timeframe::parse("1M").unwrap();
        assert_eq!(bucket_str(m, "2024-02-29T23:00:00Z"), "2024-02-01T00:00:00Z");
        assert_eq!(bucket_str(m, "2024-01-01T00:00:00Z"), "2024-01-01T00:00:00Z");
        let q = Timeframe::parse("3M").unwrap();
        assert_eq!(bucket_str(q, "2024-05-17T12:00:00Z"), "2024-04-01T00:00:00Z");
    }

    #[test]
    fn bucket_normalizes_the_offset_before_flooring() {
        // Same instant, written in New York time: still the UTC day it belongs to.
        assert_eq!(bucket_str(DAILY, "2015-01-02T00:00:00-05:00"), "2015-01-02T00:00:00Z");
    }

    #[test]
    fn leap_day_and_dst_are_plain_utc_arithmetic() {
        assert_eq!(bucket_str(DAILY, "2024-02-29T13:00:00Z"), "2024-02-29T00:00:00Z");
        // The US DST switch (2021-03-14) shifts a session stamp by an hour; the day is the same.
        assert_eq!(bucket_str(DAILY, "2021-03-15T04:00:00Z"), "2021-03-15T00:00:00Z");
        assert_eq!(bucket_str(DAILY, "2021-03-12T05:00:00Z"), "2021-03-12T00:00:00Z");
    }

    #[test]
    fn labels_round_trip_through_parse() {
        for s in ["1m", "15m", "1h", "4h", "1d", "1w", "1M", "3M"] {
            assert_eq!(Timeframe::parse(s).unwrap().label(), s);
        }
    }

    #[test]
    fn intraday_classification() {
        assert!(Timeframe::parse("1m").unwrap().is_intraday());
        assert!(Timeframe::parse("4h").unwrap().is_intraday());
        assert!(!DAILY.is_intraday());
        assert!(!WEEKLY.is_intraday());
        assert!(!Timeframe::parse("1M").unwrap().is_intraday());
    }
}
