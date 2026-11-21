//! Goals and FIRE: where the curve is heading and when it reaches the target.
//!
//! - **Growth** is measured from the book itself, month by month: what the assets already
//!   held were revalued to, over the month's opening net worth. Adding an asset to the book is
//!   not growth, so additions and removals are left out. The monthly returns are chained and
//!   annualized over at least 12 months. Fewer is refused: two points a month apart say
//!   nothing about a year.
//! - **Projection** compounds today's net worth at that rate until the goal; a rate at or
//!   below zero never gets there, and the answer says so rather than giving a date.
//! - **Savings** over the last 12 months is the change in net worth minus what markets did
//!   (revaluation of priced holdings) and minus assumed drift. The savings rate is that
//!   saving over income, with income taken as saving plus spending (12 × monthly expenses).
//! - **FIRE number** is 25 × yearly expenses: the 4 % rule.

use serde::Serialize;
use time::Date;

use super::loan::add_months;

/// The history needed before a growth rate means anything.
pub const MIN_MONTHS: i32 = 12;
/// Past this the projection is not a date, it is a wish.
const MAX_YEARS: f64 = 100.0;

#[derive(Debug, Serialize, PartialEq)]
pub struct Projection {
    /// `reached` | `projected` | `unreachable` | `no_goal` | `insufficient_history`.
    pub state: &'static str,
    #[serde(with = "date_opt")]
    pub date: Option<Date>,
    pub months: Option<f64>,
}

mod date_opt {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?),
            None => s.serialize_none(),
        }
    }
}

/// Yearly growth chained from monthly organic returns `(month_end, return)`: each return is
/// the revaluation of what was already held over the month's opening net worth, additions and
/// removals excluded. `(1 + r1)(1 + r2)...^(12 / months) - 1`. Err names why there is none.
pub fn growth_from_returns(returns: &[(Date, f64)]) -> Result<(f64, i32), &'static str> {
    let months = returns.len() as i32;
    if months < MIN_MONTHS {
        return Err("insufficient_history");
    }
    let total: f64 = returns.iter().map(|(_, r)| 1.0 + r).product();
    if total <= 0.0 {
        return Err("negative_equity");
    }
    Ok((total.powf(12.0 / months as f64) - 1.0, months))
}

/// When `net_worth` growing at `growth` a year reaches `goal`, counted from `today`.
pub fn project(net_worth: f64, goal: Option<f64>, growth: Result<f64, &'static str>, today: Date) -> Projection {
    let none = |state| Projection { state, date: None, months: None };
    let Some(goal) = goal.filter(|g| *g > 0.0) else {
        return none("no_goal");
    };
    if net_worth >= goal {
        return Projection { state: "reached", date: Some(today), months: Some(0.0) };
    }
    let Ok(g) = growth else {
        return none("insufficient_history");
    };
    if g <= 0.0 || net_worth <= 0.0 {
        return none("unreachable");
    }
    let months = (goal / net_worth).ln() / (1.0 + g).ln() * 12.0;
    if !months.is_finite() || months > MAX_YEARS * 12.0 {
        return none("unreachable");
    }
    Projection { state: "projected", date: Some(add_months(today, months.ceil() as u32)), months: Some(months) }
}

/// 25 × yearly expenses.
pub fn fire_number(monthly_expenses: Option<f64>) -> Option<f64> {
    monthly_expenses.filter(|m| *m > 0.0).map(|m| m * 12.0 * 25.0)
}

/// Saving over income, in percent, income being saving plus a year of spending.
pub fn savings_rate(savings_12m: f64, monthly_expenses: Option<f64>) -> Option<f64> {
    let spent = monthly_expenses.filter(|m| *m > 0.0)? * 12.0;
    let income = savings_12m + spent;
    (income > 0.0).then(|| savings_12m / income * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn d(y: i32, m: Month, day: u8) -> Date {
        Date::from_calendar_date(y, m, day).unwrap()
    }

    /// A curve growing 10 % a year reaches 1.21 × today in two years, to the month.
    #[test]
    fn projection_with_constant_growth() {
        let monthly = 1.1f64.powf(1.0 / 12.0) - 1.0;
        let returns: Vec<(Date, f64)> = (1..=24u32)
            .map(|k| (add_months(d(2024, Month::January, 31), k), monthly))
            .collect();
        let (g, months) = growth_from_returns(&returns).unwrap();
        assert_eq!(months, 24);
        assert!((g - 0.10).abs() < 1e-9);
        let today = d(2026, Month::January, 31);
        let nw = 100_000.0 * 1.1f64.powf(2.0);
        let p = project(nw, Some(nw * 1.21), Ok(g), today);
        assert_eq!(p.state, "projected");
        assert!((p.months.unwrap() - 24.0).abs() < 1e-6);
        assert_eq!(p.date, Some(d(2028, Month::January, 31)));
    }

    #[test]
    fn insufficient_history_is_refused() {
        let short: Vec<(Date, f64)> =
            (1..=8u32).map(|k| (add_months(d(2026, Month::March, 31), k), 0.05)).collect();
        assert_eq!(growth_from_returns(&short).unwrap_err(), "insufficient_history");
        assert_eq!(growth_from_returns(&[]).unwrap_err(), "insufficient_history");
        let p = project(1500.0, Some(5000.0), growth_from_returns(&short).map(|x| x.0), d(2026, Month::November, 6));
        assert_eq!(p.state, "insufficient_history");
        assert_eq!(p.date, None);
    }

    /// Twelve flat months chain to zero growth whatever was added to the book meanwhile: the
    /// caller feeds revaluation over opening net worth, never the raw change.
    #[test]
    fn flat_returns_are_zero_growth() {
        let flat: Vec<(Date, f64)> = (1..=12u32).map(|k| (add_months(d(2025, Month::January, 31), k), 0.0)).collect();
        let (g, months) = growth_from_returns(&flat).unwrap();
        assert_eq!(months, 12);
        assert!(g.abs() < 1e-12);
    }

    #[test]
    fn no_growth_never_reaches_and_a_reached_goal_says_so() {
        let today = d(2026, Month::November, 6);
        assert_eq!(project(100.0, Some(200.0), Ok(0.0), today).state, "unreachable");
        assert_eq!(project(100.0, Some(200.0), Ok(-0.05), today).state, "unreachable");
        assert_eq!(project(300.0, Some(200.0), Err("insufficient_history"), today).state, "reached");
        assert_eq!(project(300.0, None, Ok(0.1), today).state, "no_goal");
    }

    #[test]
    fn fire_and_savings_rate() {
        assert_eq!(fire_number(Some(3_000.0)), Some(900_000.0));
        assert_eq!(fire_number(None), None);
        // Saved 12 000 while spending 36 000: a quarter of income.
        assert!((savings_rate(12_000.0, Some(3_000.0)).unwrap() - 25.0).abs() < 1e-9);
        assert_eq!(savings_rate(12_000.0, None), None);
        assert_eq!(savings_rate(-50_000.0, Some(1_000.0)), None);
    }
}
