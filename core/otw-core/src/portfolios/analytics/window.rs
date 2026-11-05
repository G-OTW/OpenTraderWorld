//! The window an analytics request covers.

use time::{Date, Month, OffsetDateTime};

use crate::util::vocab::UnknownValue;

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

    /// Resolve `?window=` against today: case-insensitive, `max` reads as inception. An
    /// unknown name is refused with the list: answering it as inception made `YTD` or
    /// `12m` look like a whole-history figure under the wrong label.
    pub fn named(name: &str, today: Date) -> Result<Self, UnknownValue> {
        Ok(Self::of(WindowName::parse(name)?, today))
    }

    /// A window from its parsed name.
    pub fn of(name: WindowName, today: Date) -> Self {
        let back = |years: i32| today.replace_year(today.year() - years).ok();
        let label = name.as_str();
        match name {
            WindowName::Ytd => Self {
                from: Date::from_calendar_date(today.year(), Month::January, 1).ok(),
                to: None,
                label,
            },
            WindowName::M1 => Self { from: today.checked_sub(time::Duration::days(30)), to: None, label },
            WindowName::M3 => Self { from: today.checked_sub(time::Duration::days(91)), to: None, label },
            WindowName::M6 => Self { from: today.checked_sub(time::Duration::days(182)), to: None, label },
            WindowName::Y1 => Self { from: back(1), to: None, label },
            WindowName::Y3 => Self { from: back(3), to: None, label },
            WindowName::Y5 => Self { from: back(5), to: None, label },
            WindowName::Inception => Self::inception(),
        }
    }

    pub fn explicit(from: Option<Date>, to: Option<Date>) -> Self {
        Self { from, to, label: "custom" }
    }

    pub fn today() -> Date {
        OffsetDateTime::now_utc().date()
    }
}

crate::util::vocab::vocab! {
    /// A named portfolio analytics window.
    pub WindowName("window") {
        M1 = "1m",
        M3 = "3m",
        M6 = "6m",
        Ytd = "ytd",
        Y1 = "1y",
        Y3 = "3y",
        Y5 = "5y",
        Inception = "inception",
    }
    aliases { "max" => Inception }
}

/// Every named window the API offers, for the picker and for the multi-window performance grid.
pub const WINDOWS: &[&str] = WindowName::VALUES;

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn day(y: i32, m: Month, d: u8) -> Date {
        Date::from_calendar_date(y, m, d).unwrap()
    }

    #[test]
    fn ytd_starts_on_the_first_of_january() {
        let w = Window::named("ytd", day(2026, Month::September, 4)).unwrap();
        assert_eq!(w.from, Some(day(2026, Month::January, 1)));
        assert_eq!(w.to, None);
    }

    #[test]
    fn a_year_back_keeps_the_day() {
        let w = Window::named("1y", day(2026, Month::September, 4)).unwrap();
        assert_eq!(w.from, Some(day(2025, Month::September, 4)));
    }

    /// A leap day cannot be subtracted a year into a non-leap year, and the window must not
    /// vanish because of it: the whole span is the honest fallback.
    #[test]
    fn a_leap_day_falls_back_to_inception() {
        let w = Window::named("1y", day(2024, Month::February, 29)).unwrap();
        assert_eq!(w.from, None);
    }

    #[test]
    fn a_name_is_case_insensitive_and_max_is_inception() {
        let today = day(2026, Month::September, 4);
        assert_eq!(Window::named("YTD", today).unwrap().label, "ytd");
        assert_eq!(Window::named("1Y", today).unwrap().label, "1y");
        let w = Window::named("max", today).unwrap();
        assert_eq!((w.label, w.from), ("inception", None));
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_list() {
        let e = Window::named("12m", day(2026, Month::September, 4)).unwrap_err().to_string();
        assert_eq!(e, r#"unknown window "12m"; valid: 1m, 3m, 6m, ytd, 1y, 3y, 5y, inception (aliases: max = inception)"#);
    }
}
