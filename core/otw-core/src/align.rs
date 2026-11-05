//! Cross-asset alignment: one merged clock for N price series, whatever stamped them.
//!
//! Every module that reads more than one series shares this. Before it existed, the quant
//! endpoint intersected raw timestamp strings and the backtest engine unioned them, so a
//! crypto series stamped `00:00Z` and an equity series stamped `05:00Z` never met: the
//! intersection came back empty and the union produced a clock where the two assets simply
//! never coexisted on a row.
//!
//! Three decisions hold this together:
//!
//! 1. **Normalize on read, never on write.** A stored bar keeps the provider's own timestamp
//!    (re-download idempotence, `ON CONFLICT (dataset_id, ts)`, auditability). Alignment is a
//!    property of the question being asked, not of the data.
//! 2. **A row's timestamp is the latest real stamp in its bucket**, not the bucket's opening
//!    instant. That keeps a single-asset or single-provider clock *bit-identical* to what it
//!    was before this module existed (nothing downstream shifts), it stays parseable for
//!    everything that reads the clock as an instant (funding accrual, session filters, the
//!    daily-loss baseline), and it is the instant at which the row's information is complete.
//! 3. **Intraday is refused, not approximated.** See [`Timeframe::is_intraday`].

use std::collections::BTreeMap;

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::timeframe::{Timeframe, DAILY, WEEKLY};

/// One asset's timestamps, in ascending order.
pub struct Series<'a> {
    pub label: &'a str,
    pub ts: &'a [String],
}

/// Which period granularity the rows are keyed on.
pub enum Grain {
    /// Timestamps must match to the second (the pre-alignment behaviour).
    Exact,
    /// Read the granularity off the data (median spacing of each series).
    Infer,
    /// A caller-declared timeframe (the datasets' own, or a coarser one to resample to).
    At(Timeframe),
}

/// Rows kept: every period any asset has (`Union`), or only the periods every asset has
/// (`Intersection`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Join {
    Union,
    Intersection,
}

pub struct Aligned {
    /// The series' labels, in input order (the index every `maps` and `collapsed` entry uses).
    pub labels: Vec<String>,
    /// One representative RFC3339 timestamp per row, ascending.
    pub clock: Vec<String>,
    /// Per asset: row → that asset's bar index, `None` where it has no bar in the period.
    pub maps: Vec<Vec<Option<usize>>>,
    /// The granularity rows were keyed on; `None` when timestamps had to match exactly.
    pub grain: Option<Timeframe>,
    /// Per asset: bars that fell into a period another of its own bars already occupied
    /// (the later one wins). Non-zero means the series is finer than `grain`, i.e. it was
    /// resampled, or its data is dirty.
    pub collapsed: Vec<usize>,
}

impl Aligned {
    pub fn rows(&self) -> usize {
        self.clock.len()
    }
    pub fn collapsed_total(&self) -> usize {
        self.collapsed.iter().sum()
    }
    /// Rows where every asset has a bar (the fully-aligned span).
    pub fn overlap_rows(&self) -> Vec<usize> {
        (0..self.clock.len())
            .filter(|&r| self.maps.iter().all(|m| m[r].is_some()))
            .collect()
    }
}

/// One row's identity. Borrowed and integer-keyed: the merge runs once per backtest trial,
/// and the optimizer runs a trial up to six figures of times, so this allocates nothing.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
enum Key<'a> {
    /// A period's opening instant, in Unix seconds.
    At(i64),
    /// The raw stamp, when it is its own key: exact matching, a daily bucket read straight
    /// off an RFC3339 date prefix, or a timestamp nothing could parse.
    Raw(&'a str),
}

/// Is this an RFC3339 UTC stamp, i.e. one whose first ten characters *are* its UTC date?
/// Everything the store produces is (`timestamptz` → `OffsetDateTime` → RFC3339), which is
/// what lets the daily bucket be a string slice instead of a parse and a format.
fn utc_rfc3339(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 20 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[b.len() - 1] == b'Z'
}

/// The period a bar belongs to.
fn key_of<'a>(raw: &'a str, tf: Option<Timeframe>, by_date: bool) -> Key<'a> {
    match tf {
        None => Key::Raw(raw),
        // `by_date` guarantees the stamp is RFC3339-in-UTC, so its first ten bytes are its
        // UTC date: the daily bucket without a parse or an allocation.
        Some(_) if by_date => Key::Raw(&raw[..10]),
        Some(tf) => match OffsetDateTime::parse(raw, &Rfc3339) {
            Ok(dt) => Key::At(tf.bucket(dt).unix_timestamp()),
            // An unreadable timestamp keys on itself: never dropped, never merged.
            Err(_) => Key::Raw(raw),
        },
    }
}

/// Merge N series onto one clock.
///
/// Rows are ordered, and a row's representative timestamp chosen, by **lexical** comparison
/// of RFC3339 stamps, which the engine has always relied on (RFC3339 is sort-safe) and which
/// keeps this off the parsing path.
///
/// Series are expected in ascending time order (everything that reads bars returns them that
/// way), which makes this a linear merge-join rather than three balanced trees. An unsorted
/// series is detected and takes an order-independent path instead of producing a broken clock.
pub fn merge(series: &[Series<'_>], grain: Grain, join: Join) -> Aligned {
    let tf = match grain {
        Grain::Exact => None,
        Grain::At(tf) => Some(tf),
        Grain::Infer => infer_grain(series),
    }
    // Bucketing an intraday period would pair bars up to a period apart (a 09:30 session bar
    // with an 08:00 epoch-grid one). Exact matching is the honest answer; the API layer
    // refuses the mix rather than pretending it aligned.
    .filter(|tf: &Timeframe| !tf.is_intraday());

    // A daily bucket is the UTC date, so it is the stamp's own first ten characters, as long
    // as every stamp in the request is RFC3339-in-UTC. One mixed-in oddity and the whole merge
    // takes the parsing path instead, rather than keying two conventions differently.
    let by_date =
        tf == Some(DAILY) && series.iter().all(|s| s.ts.iter().all(|t| utc_rfc3339(t)));

    // Per asset: its periods, ascending, each with the last bar that falls in it.
    let mut per_asset: Vec<Vec<(Key<'_>, usize)>> = Vec::with_capacity(series.len());
    let mut collapsed = vec![0usize; series.len()];
    for (ai, s) in series.iter().enumerate() {
        let mut v: Vec<(Key<'_>, usize)> = Vec::with_capacity(s.ts.len());
        let mut ascending = true;
        for (bi, raw) in s.ts.iter().enumerate() {
            let key = key_of(raw, tf, by_date);
            match v.last_mut() {
                Some((k, idx)) if *k == key => {
                    *idx = bi;
                    collapsed[ai] += 1;
                }
                Some((k, _)) if key < *k => {
                    ascending = false;
                    v.push((key, bi));
                }
                _ => v.push((key, bi)),
            }
        }
        if !ascending {
            // Out-of-order input: group through an ordered map, where the result no longer
            // depends on the order bars arrived in. Still last-bar-wins per period.
            let mut m: BTreeMap<Key<'_>, usize> = BTreeMap::new();
            collapsed[ai] = 0;
            for (bi, raw) in s.ts.iter().enumerate() {
                if m.insert(key_of(raw, tf, by_date), bi).is_some() {
                    collapsed[ai] += 1;
                }
            }
            v = m.into_iter().collect();
        }
        per_asset.push(v);
    }

    // Merge-join the per-asset period lists into one clock.
    let n = series.len();
    let mut cur = vec![0usize; n];
    let mut clock: Vec<String> = Vec::new();
    let mut maps: Vec<Vec<Option<usize>>> = vec![Vec::new(); n];
    loop {
        let head = |ai: usize, cur: &[usize]| per_asset[ai].get(cur[ai]).copied();
        let mut min: Option<Key<'_>> = None;
        for ai in 0..n {
            if let Some((k, _)) = head(ai, &cur) {
                if min.is_none_or(|m| k < m) {
                    min = Some(k);
                }
            }
        }
        let Some(key) = min else { break };
        let complete =
            (0..n).all(|ai| head(ai, &cur).is_some_and(|(k, _)| k == key));
        if join == Join::Union || complete {
            // The row's timestamp: the latest stamp among the bars it holds. With one asset,
            // or several sharing a stamping convention, this is the input timestamp itself.
            let mut best: Option<&str> = None;
            for ai in 0..n {
                let bar = head(ai, &cur).filter(|(k, _)| *k == key).map(|(_, bi)| bi);
                maps[ai].push(bar);
                if let Some(bi) = bar {
                    let raw = series[ai].ts[bi].as_str();
                    if best.is_none_or(|b| raw > b) {
                        best = Some(raw);
                    }
                }
            }
            clock.push(match best {
                Some(raw) => raw.to_string(),
                None => String::new(),
            });
        }
        for ai in 0..n {
            if head(ai, &cur).is_some_and(|(k, _)| k == key) {
                cur[ai] += 1;
            }
        }
    }

    let labels = series.iter().map(|s| s.label.to_string()).collect();
    Aligned { labels, clock, maps, grain: tf, collapsed }
}

/// The finest granularity the data itself exhibits, or `None` when no series is regular
/// enough to name one. Read from the **median** gap between consecutive bars, which is immune
/// to the weekend and holiday gaps that would skew a mean.
///
/// Sampled from each end of a series rather than over all of it: this runs on every backtest
/// trial, and a price series does not change its bar length halfway through (if one somehow
/// did, the collapse counter is what catches it).
fn infer_grain(series: &[Series<'_>]) -> Option<Timeframe> {
    /// Consecutive gaps read from each end of the series.
    const SAMPLE: usize = 128;
    let mut finest: Option<i64> = None;
    for s in series {
        let n = s.ts.len();
        if n < 3 {
            continue;
        }
        let head = 0..SAMPLE.min(n);
        let tail = n.saturating_sub(SAMPLE).max(head.end)..n;
        let mut gaps: Vec<i64> = Vec::with_capacity(2 * SAMPLE);
        for range in [head, tail] {
            let stamps: Vec<Option<OffsetDateTime>> = s.ts[range]
                .iter()
                .map(|t| OffsetDateTime::parse(t, &Rfc3339).ok())
                .collect();
            gaps.extend(stamps.windows(2).filter_map(|w| match (w[0], w[1]) {
                (Some(a), Some(b)) => Some(b.unix_timestamp() - a.unix_timestamp()),
                _ => None,
            }));
        }
        gaps.retain(|g| *g > 0);
        if gaps.len() < 2 {
            continue;
        }
        gaps.sort_unstable();
        let median = gaps[gaps.len() / 2];
        finest = Some(finest.map_or(median, |f: i64| f.min(median)));
    }
    finest.and_then(Timeframe::from_secs)
}

/// Observations per year, **measured on the clock** rather than assumed from the timeframe.
///
/// This is the annualization factor: it must match the sampling frequency of the series being
/// annualized, not the calendar of the assets in it. A daily equity clock yields ~252, a daily
/// crypto clock ~365, a merged one ~365, a weekly one ~52, with no table to maintain and no
/// choice to make for a mixed basket (where picking either constant is wrong: what is fixed is
/// the *annual* variance, not the per-row variance).
///
/// Falls back to `nominal` when the sample is too short to measure. The estimate counts
/// observations over the calendar span, so a long data hole reads as a lower frequency: that
/// divergence from `nominal` is a data-quality signal worth surfacing, not smoothing away.
pub fn observed_ppy(clock: &[String], nominal: f64) -> f64 {
    const MIN_OBS: usize = 30;
    if clock.len() < MIN_OBS {
        return nominal;
    }
    let (Some(first), Some(last)) = (
        OffsetDateTime::parse(&clock[0], &Rfc3339).ok(),
        OffsetDateTime::parse(&clock[clock.len() - 1], &Rfc3339).ok(),
    ) else {
        return nominal;
    };
    let span_days = (last - first).as_seconds_f64() / 86_400.0;
    if span_days <= 0.0 {
        return nominal;
    }
    ((clock.len() - 1) as f64 * 365.25 / span_days).max(1.0)
}

/// Share of a series' bars falling on a Saturday or Sunday.
pub fn weekend_share(ts: &[String]) -> f64 {
    if ts.is_empty() {
        return 0.0;
    }
    let n = ts
        .iter()
        .filter_map(|t| OffsetDateTime::parse(t, &Rfc3339).ok())
        .filter(|d| {
            matches!(d.weekday(), time::Weekday::Saturday | time::Weekday::Sunday)
        })
        .count();
    n as f64 / ts.len() as f64
}

/// Does this set mix a 24/7 calendar with an exchange-hours one? A continuous market puts
/// ~2/7 of its bars on a weekend; an exchange puts none there (a stray row is a data artifact,
/// hence the band rather than a zero test).
pub fn mixed_calendars(series: &[Series<'_>]) -> bool {
    if series.len() < 2 {
        return false;
    }
    let shares: Vec<f64> = series.iter().map(|s| weekend_share(s.ts)).collect();
    let hi = shares.iter().cloned().fold(f64::MIN, f64::max);
    let lo = shares.iter().cloned().fold(f64::MAX, f64::min);
    hi > 0.15 && lo < 0.05
}

/// Why a set of series is being measured at a coarser granularity than it was stored at.
pub struct Resample {
    pub from: Timeframe,
    pub to: Timeframe,
    pub reason: &'static str,
}

/// The granularity a multi-asset **statistical** measure (covariance, correlation, risk
/// parity) should be computed at.
///
/// Aligning a 24/7 series with an exchange one on a daily clock leaves an artefact no
/// annualization factor can repair: the weekend move of the continuous asset lands on the
/// same row as Monday's move of the other, which spreads their covariance across rows and
/// understates the portfolio's volatility. The standard remedy is to measure at a frequency
/// that swallows the mismatch, and for a weekend it is one week.
pub fn measure_grain(stored: Timeframe, series: &[Series<'_>]) -> (Timeframe, Option<Resample>) {
    // Daily or finer: a week already swallows the weekend mismatch, and a month has no
    // fixed length to compare (`secs()` is None), so neither is ever resampled.
    let daily_or_finer =
        matches!((stored.secs(), DAILY.secs()), (Some(a), Some(b)) if a <= b);
    if daily_or_finer && mixed_calendars(series) {
        return (
            WEEKLY,
            Some(Resample {
                from: stored,
                to: WEEKLY,
                reason: "mixed trading calendars (a 24/7 market with an exchange-hours one): \
                         measured weekly so the weekend gap cannot understate covariance",
            }),
        );
    }
    (stored, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    /// `n` consecutive calendar days from `start`, stamped at `hh:mm`, optionally skipping
    /// weekends (an exchange series) or not (a 24/7 one).
    fn stamps(start: &str, n: usize, hour: u8, weekends: bool) -> Vec<String> {
        let mut d = OffsetDateTime::parse(&format!("{start}T00:00:00Z"), &Rfc3339).unwrap()
            + Duration::hours(hour as i64);
        let mut out = Vec::new();
        while out.len() < n {
            let wd = d.weekday();
            let weekend =
                matches!(wd, time::Weekday::Saturday | time::Weekday::Sunday);
            if weekends || !weekend {
                out.push(d.format(&Rfc3339).unwrap());
            }
            d += Duration::days(1);
        }
        out
    }

    fn series<'a>(label: &'a str, ts: &'a [String]) -> Series<'a> {
        Series { label, ts }
    }

    #[test]
    fn crypto_and_equity_daily_bars_meet_on_the_same_rows() {
        // The exact bug the module exists for: Binance stamps 00:00Z, Alpaca 05:00Z.
        let btc = stamps("2015-01-05", 10, 0, false);
        let spy = stamps("2015-01-05", 10, 5, false);
        let s = [series("BTC", &btc), series("SPY", &spy)];

        let exact = merge(&s, Grain::Exact, Join::Intersection);
        assert_eq!(exact.rows(), 0, "raw stamps share nothing, which is the bug");

        let a = merge(&s, Grain::Infer, Join::Intersection);
        assert_eq!(a.grain, Some(DAILY));
        assert_eq!(a.rows(), 10);
        assert_eq!(a.collapsed_total(), 0);
        for r in 0..a.rows() {
            assert!(a.maps[0][r].is_some() && a.maps[1][r].is_some());
        }
        // The row is stamped at the latest real bar it holds: 05:00Z, not an invented midnight.
        assert_eq!(a.clock[0], spy[0]);
    }

    /// The real stamps this was written for: Binance dates a daily candle at its open
    /// (00:00 UTC), Yahoo at the New York session open, which moves with US daylight saving
    /// (14:30 UTC in winter, 13:30 in summer). Same days, three different stamps, and the
    /// switch happens mid-series.
    #[test]
    fn real_provider_stamps_across_a_daylight_saving_switch() {
        let sessions = [
            ("2024-03-06", "14:30"),
            ("2024-03-07", "14:30"),
            ("2024-03-08", "14:30"),
            ("2024-03-11", "13:30"),
            ("2024-03-12", "13:30"),
            ("2024-03-13", "13:30"),
            ("2024-03-14", "13:30"),
            ("2024-03-15", "13:30"),
        ];
        let spy: Vec<String> =
            sessions.iter().map(|(d, t)| format!("{d}T{t}:00Z")).collect();
        // The crypto series runs every calendar day, weekends included.
        let btc: Vec<String> = (6..=15)
            .map(|d| format!("2024-03-{d:02}T00:00:00Z"))
            .collect();

        let s = [series("BTCUSDT", &btc), series("SPY", &spy)];
        assert_eq!(merge(&s, Grain::Exact, Join::Intersection).rows(), 0, "the bug, verbatim");

        let a = merge(&s, Grain::Infer, Join::Intersection);
        assert_eq!(a.grain, Some(DAILY));
        assert_eq!(a.rows(), sessions.len(), "one row per session, DST switch included");
        assert_eq!(a.clock[0], "2024-03-06T14:30:00Z");
        assert_eq!(a.clock[3], "2024-03-11T13:30:00Z");

        // The union keeps the two weekend days the exchange does not trade.
        let u = merge(&s, Grain::Infer, Join::Union);
        assert_eq!(u.rows(), 10);
        assert_eq!(u.overlap_rows().len(), 8);
    }

    #[test]
    fn a_single_series_clock_is_bit_identical() {
        let spy = stamps("2020-01-01", 40, 5, false);
        let a = merge(&[series("SPY", &spy)], Grain::Infer, Join::Union);
        assert_eq!(a.clock, spy);
        assert_eq!(a.maps[0], (0..spy.len()).map(Some).collect::<Vec<_>>());
        assert_eq!(a.collapsed_total(), 0);
    }

    #[test]
    fn one_stamping_convention_is_left_exactly_as_it_was() {
        let a1 = stamps("2020-01-01", 30, 5, false);
        let a2 = stamps("2020-01-01", 30, 5, false);
        let s = [series("A", &a1), series("B", &a2)];
        let bucketed = merge(&s, Grain::Infer, Join::Union);
        let exact = merge(&s, Grain::Exact, Join::Union);
        assert_eq!(bucketed.clock, exact.clock);
        assert_eq!(bucketed.maps, exact.maps);
    }

    #[test]
    fn union_keeps_the_weekend_rows_of_the_continuous_asset() {
        let btc = stamps("2015-01-05", 14, 0, true); // two full weeks
        let spy = stamps("2015-01-05", 10, 5, false); // ten sessions
        let s = [series("BTC", &btc), series("SPY", &spy)];
        let a = merge(&s, Grain::Infer, Join::Union);
        assert_eq!(a.rows(), 14);
        assert_eq!(a.overlap_rows().len(), 10);
        // Weekend rows carry the crypto bar only, and are stamped by it.
        let sat = a.clock.iter().position(|t| t.starts_with("2015-01-10")).unwrap();
        assert!(a.maps[0][sat].is_some() && a.maps[1][sat].is_none());
        assert_eq!(a.clock[sat], "2015-01-10T00:00:00Z");
    }

    #[test]
    fn intersection_drops_the_periods_one_asset_is_missing() {
        let btc = stamps("2015-01-05", 14, 0, true);
        let spy = stamps("2015-01-05", 10, 5, false);
        let s = [series("BTC", &btc), series("SPY", &spy)];
        let a = merge(&s, Grain::Infer, Join::Intersection);
        assert_eq!(a.rows(), 10);
        assert!(a.maps.iter().all(|m| m.iter().all(|b| b.is_some())));
    }

    #[test]
    fn resampling_to_weekly_keeps_the_last_close_of_each_week() {
        // Four ISO weeks of sessions: 20 equity bars → 4 rows, 16 collapsed.
        let spy = stamps("2015-01-05", 20, 5, false);
        let a = merge(&[series("SPY", &spy)], Grain::At(WEEKLY), Join::Union);
        assert_eq!(a.rows(), 4);
        assert_eq!(a.collapsed[0], 16);
        // Each row points at that week's Friday, and is stamped with it.
        assert_eq!(a.clock[0], "2015-01-09T05:00:00Z");
        assert_eq!(a.maps[0][0], Some(4));
        assert_eq!(a.clock[3], "2015-01-30T05:00:00Z");
    }

    #[test]
    fn weekly_pairs_a_sunday_crypto_close_with_the_friday_equity_close() {
        let btc = stamps("2015-01-05", 28, 0, true);
        let spy = stamps("2015-01-05", 20, 5, false);
        let s = [series("BTC", &btc), series("SPY", &spy)];
        let a = merge(&s, Grain::At(WEEKLY), Join::Intersection);
        assert_eq!(a.rows(), 4);
        // Week 1: BTC's last bar is Sunday the 11th, SPY's is Friday the 9th.
        assert_eq!(btc[a.maps[0][0].unwrap()], "2015-01-11T00:00:00Z");
        assert_eq!(spy[a.maps[1][0].unwrap()], "2015-01-09T05:00:00Z");
        assert_eq!(a.clock[0], "2015-01-11T00:00:00Z");
    }

    #[test]
    fn duplicate_bars_in_one_period_collapse_to_the_last_and_are_counted() {
        let ts: Vec<String> = vec![
            "2020-01-01T05:00:00Z".into(),
            "2020-01-01T21:00:00Z".into(),
            "2020-01-02T05:00:00Z".into(),
        ];
        let a = merge(&[series("X", &ts)], Grain::At(DAILY), Join::Union);
        assert_eq!(a.rows(), 2);
        assert_eq!(a.collapsed[0], 1);
        assert_eq!(a.maps[0][0], Some(1));
        assert_eq!(a.clock[0], "2020-01-01T21:00:00Z");
    }

    #[test]
    fn an_out_of_order_series_still_produces_an_ascending_clock() {
        let ts: Vec<String> = vec![
            "2020-01-03T00:00:00Z".into(),
            "2020-01-01T00:00:00Z".into(),
            "2020-01-02T00:00:00Z".into(),
        ];
        let a = merge(&[series("X", &ts)], Grain::At(DAILY), Join::Union);
        assert_eq!(a.clock, vec![ts[1].clone(), ts[2].clone(), ts[0].clone()]);
        assert_eq!(a.maps[0], vec![Some(1), Some(2), Some(0)]);
        assert_eq!(a.collapsed[0], 0);
    }

    #[test]
    fn stamps_written_in_another_offset_land_in_the_same_period() {
        // The same three days, one series in UTC and one in New York time. The date-prefix
        // shortcut cannot read the second, so the whole merge parses instead.
        let utc: Vec<String> = (2..5).map(|d| format!("2020-01-0{d}T00:00:00Z")).collect();
        let ny: Vec<String> = (1..4).map(|d| format!("2020-01-0{d}T19:00:00-05:00")).collect();
        let a = merge(&[series("A", &utc), series("B", &ny)], Grain::At(DAILY), Join::Intersection);
        assert_eq!(a.rows(), 3);
        assert!(a.maps.iter().all(|m| m.iter().all(|b| b.is_some())));
        assert_eq!(a.clock[0], "2020-01-02T00:00:00Z");
    }

    #[test]
    fn an_unreadable_timestamp_keys_on_itself_instead_of_vanishing() {
        let ts: Vec<String> =
            vec!["2020-01-01T05:00:00Z".into(), "not a date".into(), "2020-01-02T05:00:00Z".into()];
        let a = merge(&[series("X", &ts)], Grain::At(DAILY), Join::Union);
        assert_eq!(a.rows(), 3);
        assert_eq!(a.collapsed_total(), 0);
        assert!(a.clock.contains(&"not a date".to_string()));
    }

    #[test]
    fn intraday_is_never_bucketed() {
        let ts: Vec<String> = (0..40)
            .map(|i| {
                (OffsetDateTime::parse("2020-01-01T00:00:00Z", &Rfc3339).unwrap()
                    + Duration::hours(i))
                .format(&Rfc3339)
                .unwrap()
            })
            .collect();
        let a = merge(&[series("X", &ts)], Grain::Infer, Join::Union);
        assert_eq!(a.grain, None, "an hourly series must not be bucketed");
        assert_eq!(a.clock, ts);
        // Even asked for explicitly.
        let forced = merge(
            &[series("X", &ts)],
            Grain::At(Timeframe::parse("4h").unwrap()),
            Join::Union,
        );
        assert_eq!(forced.grain, None);
        assert_eq!(forced.rows(), ts.len());
    }

    #[test]
    fn grain_inference_reads_the_data() {
        let daily = stamps("2020-01-01", 40, 0, true);
        let weekly: Vec<String> = (0..30)
            .map(|i| {
                (OffsetDateTime::parse("2020-01-06T00:00:00Z", &Rfc3339).unwrap()
                    + Duration::weeks(i))
                .format(&Rfc3339)
                .unwrap()
            })
            .collect();
        assert_eq!(merge(&[series("D", &daily)], Grain::Infer, Join::Union).grain, Some(DAILY));
        assert_eq!(merge(&[series("W", &weekly)], Grain::Infer, Join::Union).grain, Some(WEEKLY));
        // Mixed: the finest granularity present wins, so nothing is merged that shouldn't be.
        let mixed = merge(
            &[series("D", &daily), series("W", &weekly)],
            Grain::Infer,
            Join::Union,
        );
        assert_eq!(mixed.grain, Some(DAILY));
        // Too short to read.
        let two = vec![daily[0].clone(), daily[1].clone()];
        assert_eq!(merge(&[series("S", &two)], Grain::Infer, Join::Union).grain, None);
    }

    #[test]
    fn observed_ppy_counts_what_the_clock_holds() {
        let equity = stamps("2015-01-05", 252, 5, false);
        let crypto = stamps("2015-01-05", 365, 0, true);
        let ppy_e = observed_ppy(&equity, 252.0);
        let ppy_c = observed_ppy(&crypto, 252.0);
        // Weekdays with no holidays: 365.25 × 5/7 = 260.9. Real exchange data, which is what
        // this reads in production, lands near 252 for exactly the same reason: the estimate
        // follows the calendar the series actually has instead of a constant.
        assert!((ppy_e - 260.9).abs() < 1.0, "weekday clock ≈ 261, got {ppy_e}");
        assert!((ppy_c - 365.25).abs() < 1.0, "crypto daily ≈ 365, got {ppy_c}");

        let weekly: Vec<String> = (0..104)
            .map(|i| {
                (OffsetDateTime::parse("2015-01-05T00:00:00Z", &Rfc3339).unwrap()
                    + Duration::weeks(i))
                .format(&Rfc3339)
                .unwrap()
            })
            .collect();
        let ppy_w = observed_ppy(&weekly, 52.0);
        assert!((ppy_w - 52.18).abs() < 0.5, "weekly ≈ 52, got {ppy_w}");
    }

    #[test]
    fn observed_ppy_falls_back_on_a_sample_too_short_to_measure() {
        let short = stamps("2020-01-01", 10, 0, true);
        assert_eq!(observed_ppy(&short, 252.0), 252.0);
        assert_eq!(observed_ppy(&[], 52.0), 52.0);
        // A degenerate span (every stamp identical) cannot be measured either.
        let same = vec!["2020-01-01T00:00:00Z".to_string(); 40];
        assert_eq!(observed_ppy(&same, 12.0), 12.0);
    }

    #[test]
    fn calendars_are_told_apart_by_their_weekends() {
        let crypto = stamps("2020-01-01", 70, 0, true);
        let equity = stamps("2020-01-01", 50, 5, false);
        assert!((weekend_share(&crypto) - 2.0 / 7.0).abs() < 0.02);
        assert_eq!(weekend_share(&equity), 0.0);

        assert!(mixed_calendars(&[series("BTC", &crypto), series("SPY", &equity)]));
        let e2 = stamps("2020-01-01", 50, 5, false);
        assert!(!mixed_calendars(&[series("SPY", &equity), series("EFA", &e2)]));
        let c2 = stamps("2020-01-01", 70, 0, true);
        assert!(!mixed_calendars(&[series("BTC", &crypto), series("ETH", &c2)]));
        assert!(!mixed_calendars(&[series("SPY", &equity)]));
    }

    #[test]
    fn measure_grain_switches_a_mixed_basket_to_weekly() {
        let crypto = stamps("2020-01-01", 70, 0, true);
        let equity = stamps("2020-01-01", 50, 5, false);
        let (tf, note) =
            measure_grain(DAILY, &[series("BTC", &crypto), series("SPY", &equity)]);
        assert_eq!(tf, WEEKLY);
        assert!(note.is_some());

        let e2 = stamps("2020-01-01", 50, 5, false);
        let (tf, note) = measure_grain(DAILY, &[series("SPY", &equity), series("EFA", &e2)]);
        assert_eq!(tf, DAILY);
        assert!(note.is_none());

        // Already weekly or coarser: nothing to resample.
        let (tf, note) =
            measure_grain(WEEKLY, &[series("BTC", &crypto), series("SPY", &equity)]);
        assert_eq!(tf, WEEKLY);
        assert!(note.is_none());
    }

    /// The clock builder as it was before this module: a BTreeSet of raw stamps plus a
    /// HashMap lookup per bar. Kept in the bench so the cost of alignment is measured against
    /// what it replaced, not against nothing.
    fn legacy_merged_clock(ts: &[&Vec<String>]) -> (Vec<String>, Vec<Vec<Option<usize>>>) {
        let mut set: std::collections::BTreeSet<&str> = Default::default();
        for a in ts {
            for t in a.iter() {
                set.insert(t.as_str());
            }
        }
        let clock: Vec<String> = set.into_iter().map(|s| s.to_string()).collect();
        let row_of: std::collections::HashMap<&str, usize> =
            clock.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();
        let maps: Vec<Vec<Option<usize>>> = ts
            .iter()
            .map(|a| {
                let mut m = vec![None; clock.len()];
                for (bi, t) in a.iter().enumerate() {
                    if let Some(&r) = row_of.get(t.as_str()) {
                        m[r] = Some(bi);
                    }
                }
                m
            })
            .collect();
        (clock, maps)
    }

    #[test]
    #[ignore]
    fn bench_merge_cost() {
        use std::time::Instant;
        fn mk(n: usize, hour: u8) -> Vec<String> {
            let base = OffsetDateTime::parse("2015-01-01T00:00:00Z", &Rfc3339).unwrap()
                + Duration::hours(hour as i64);
            (0..n)
                .map(|i| (base + Duration::days(i as i64)).format(&Rfc3339).unwrap())
                .collect()
        }
        for (assets, bars) in [(1usize, 2800usize), (5, 2800), (5, 50_000)] {
            let data: Vec<Vec<String>> = (0..assets).map(|a| mk(bars, (a * 5) as u8)).collect();
            let s: Vec<Series<'_>> = data.iter().map(|d| Series { label: "x", ts: d }).collect();
            let refs: Vec<&Vec<String>> = data.iter().collect();
            let reps = 20;
            let t0 = Instant::now();
            for _ in 0..reps {
                std::hint::black_box(merge(&s, Grain::Infer, Join::Union).rows());
            }
            let new = t0.elapsed() / reps;
            let t1 = Instant::now();
            for _ in 0..reps {
                std::hint::black_box(legacy_merged_clock(&refs).0.len());
            }
            let old = t1.elapsed() / reps;
            println!("{assets} × {bars}: merge {new:?} vs legacy {old:?}");
        }
    }

    #[test]
    fn no_series_is_an_empty_clock_not_a_panic() {
        let a = merge(&[], Grain::Infer, Join::Intersection);
        assert_eq!(a.rows(), 0);
        assert!(a.maps.is_empty());
        let b = merge(&[], Grain::Infer, Join::Union);
        assert_eq!(b.rows(), 0);
    }
}
