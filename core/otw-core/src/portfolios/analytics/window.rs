//! The window an analytics request covers.

use time::{Date, Month, OffsetDateTime};

/// A named or explicit date range. Both ends inclusive; `None` means "as far as the data goes".
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub from: Option<Date>,
    pub to: Option<Date>,
    /// The label the caller asked for, echoed back so the screen can say which window it is
    /// looking at without re-deriving it.
    pub label: &'static str,
}

impl Window {
    pub fn inception() -> Self {
        Self { from: None, to: None, label: "inception" }
    }

    /// Resolve `?window=` against today. An unknown name is inception rather than an error:
    /// a window is a view, and refusing to draw the page over a typo helps nobody.
    pub fn named(name: &str, today: Date) -> Self {
        let back = |years: i32| today.replace_year(today.year() - years).ok();
        match name {
            "ytd" => Self {
                from: Date::from_calendar_date(today.year(), Month::January, 1).ok(),
                to: None,
                label: "ytd",
            },
            "1m" => Self { from: today.checked_sub(time::Duration::days(30)), to: None, label: "1m" },
            "3m" => Self { from: today.checked_sub(time::Duration::days(91)), to: None, label: "3m" },
            "6m" => Self { from: today.checked_sub(time::Duration::days(182)), to: None, label: "6m" },
            "1y" => Self { from: back(1), to: None, label: "1y" },
            "3y" => Self { from: back(3), to: None, label: "3y" },
            "5y" => Self { from: back(5), to: None, label: "5y" },
            _ => Self::inception(),
        }
    }

    pub fn explicit(from: Option<Date>, to: Option<Date>) -> Self {
        Self { from, to, label: "custom" }
    }

    pub fn today() -> Date {
        OffsetDateTime::now_utc().date()
    }
}

/// Every named window the API offers, for the picker and for the multi-window performance grid.
pub const WINDOWS: &[&str] = &["1m", "3m", "6m", "ytd", "1y", "3y", "5y", "inception"];

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn day(y: i32, m: Month, d: u8) -> Date {
        Date::from_calendar_date(y, m, d).unwrap()
    }

    #[test]
    fn ytd_starts_on_the_first_of_january() {
        let w = Window::named("ytd", day(2026, Month::September, 4));
        assert_eq!(w.from, Some(day(2026, Month::January, 1)));
        assert_eq!(w.to, None);
    }

    #[test]
    fn a_year_back_keeps_the_day() {
        let w = Window::named("1y", day(2026, Month::September, 4));
        assert_eq!(w.from, Some(day(2025, Month::September, 4)));
    }

    /// A leap day cannot be subtracted a year into a non-leap year, and the window must not
    /// vanish because of it: the whole span is the honest fallback.
    #[test]
    fn a_leap_day_falls_back_to_inception() {
        let w = Window::named("1y", day(2024, Month::February, 29));
        assert_eq!(w.from, None);
    }

    #[test]
    fn an_unknown_name_is_the_whole_history() {
        let w = Window::named("since the dawn of time", day(2026, Month::September, 4));
        assert_eq!(w.label, "inception");
        assert_eq!(w.from, None);
    }
}
