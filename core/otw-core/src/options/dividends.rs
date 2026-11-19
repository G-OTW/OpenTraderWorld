//! The cash dividends a chain is priced with: those a provider declared, then the rest of
//! the option horizon projected on the stock's own pattern. A projection says so: nobody has
//! announced it, and the user can drop the whole schedule.

use serde::Serialize;
use time::{Date, Duration};

/// Past payments a projection reads: two years of a quarterly payer.
const LOOKBACK: usize = 8;
/// A pattern needs this many past payments to exist.
const MIN_PAST: usize = 3;
/// A payer quiet for this many of its own gaps has stopped.
const QUIET_GAPS: f64 = 1.75;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Upcoming {
    pub ex_date: Date,
    pub amount: f64,
    /// Not announced: the last payments carried forward.
    pub projected: bool,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied()
}

/// What goes ex after `today` up to `until`, from a history of (ex-date, amount).
pub fn upcoming(history: &[(Date, f64)], today: Date, until: Date) -> Vec<Upcoming> {
    let mut all: Vec<(Date, f64)> = history.iter().copied().filter(|(_, a)| a.is_finite() && *a > 0.0).collect();
    all.sort_by_key(|(d, _)| *d);
    all.dedup_by_key(|(d, _)| *d);
    let declared: Vec<Upcoming> = all
        .iter()
        .filter(|(d, _)| *d > today && *d <= until)
        .map(|(d, a)| Upcoming { ex_date: *d, amount: *a, projected: false })
        .collect();
    let mut past: Vec<(Date, f64)> = all.iter().copied().filter(|(d, _)| *d <= today).collect();
    past.drain(..past.len().saturating_sub(LOOKBACK));
    let mut out = declared;
    if past.len() < MIN_PAST {
        return out;
    }
    let Some(gap) = median(past.windows(2).map(|w| (w[1].0 - w[0].0).whole_days() as f64).collect()).filter(|g| *g >= 7.0) else {
        return out;
    };
    let Some(&(last, _)) = past.last() else { return out };
    if (today - last).whole_days() as f64 > gap * QUIET_GAPS {
        return out;
    }
    // The median: a one-off special dividend is not the pattern.
    let Some(amount) = median(past.iter().rev().take(4).map(|(_, a)| *a).collect()) else { return out };
    let from = out.last().map_or(last, |u| u.ex_date);
    let step = Duration::days(gap.round() as i64);
    let mut next = from + step;
    while next <= today {
        next += step;
    }
    while next <= until {
        out.push(Upcoming { ex_date: next, amount, projected: true });
        next += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const TODAY: Date = date!(2026 - 11 - 04);
    const UNTIL: Date = date!(2027 - 06 - 30);

    fn quarterly() -> Vec<(Date, f64)> {
        vec![
            (date!(2025 - 11 - 10), 0.25),
            (date!(2026 - 02 - 09), 0.26),
            (date!(2026 - 05 - 11), 0.26),
            (date!(2026 - 08 - 10), 0.26),
        ]
    }

    #[test]
    fn a_regular_payer_is_carried_forward_and_says_so() {
        let u = upcoming(&quarterly(), TODAY, UNTIL);
        assert_eq!(u.len(), 3);
        assert!(u.iter().all(|x| x.projected && x.amount == 0.26));
        assert_eq!(u[0].ex_date, date!(2026 - 11 - 09));
    }

    #[test]
    fn a_declared_dividend_is_kept_and_the_pattern_goes_on_from_it() {
        let mut h = quarterly();
        h.push((date!(2026 - 11 - 12), 0.3));
        let u = upcoming(&h, TODAY, UNTIL);
        assert_eq!(u[0], Upcoming { ex_date: date!(2026 - 11 - 12), amount: 0.3, projected: false });
        assert!(u[1].projected && u[1].ex_date > date!(2027 - 02 - 01));
    }

    #[test]
    fn a_special_dividend_does_not_set_the_amount() {
        let mut h = quarterly();
        h[3].1 = 5.0;
        assert!(upcoming(&h, TODAY, UNTIL).iter().all(|x| x.amount < 1.0));
    }

    #[test]
    fn no_pattern_means_no_projection() {
        assert!(upcoming(&[], TODAY, UNTIL).is_empty());
        assert!(upcoming(&quarterly()[..2], TODAY, UNTIL).is_empty(), "too few payments");
        // Stopped: nothing for over a year on a quarterly payer.
        let stale: Vec<(Date, f64)> = quarterly().iter().map(|(d, a)| (*d - Duration::days(500), *a)).collect();
        assert!(upcoming(&stale, TODAY, UNTIL).is_empty());
    }
}
