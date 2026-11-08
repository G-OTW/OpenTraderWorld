//! Loan schedule: a liability whose balance follows the amortization formula.
//!
//! A loan is principal, a yearly rate, a term in months and a start date; the payment is the
//! annuity that clears it at term unless the owner states the real one (a bank rounding, an
//! overpayment). Payments fall due monthly from one month after the start, on the start's day
//! of the month (clamped to the month's length). The balance after `k` payments is the closed
//! form `P(1+r)^k - A((1+r)^k - 1)/r`, never below zero.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Iso8601, Date, Month};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Loan {
    pub principal: f64,
    /// Nominal yearly rate in percent (4.2 = 4.2 %).
    pub rate_pct: f64,
    pub term_months: u32,
    /// `YYYY-MM-DD`, the day the money was lent.
    pub start: String,
    /// Monthly payment as the lender states it; None = the annuity that clears it at term.
    #[serde(default)]
    pub payment: Option<f64>,
}

/// Longest term accepted: 50 years.
const MAX_TERM: u32 = 600;

impl Loan {
    /// Read and check stored terms. The error names the field to fix.
    pub fn parse(v: &Value) -> Result<Loan, String> {
        let loan: Loan = serde_json::from_value(v.clone()).map_err(|e| format!("loan terms: {e}"))?;
        loan.check()?;
        Ok(loan)
    }

    pub fn check(&self) -> Result<(), String> {
        if !(self.principal.is_finite() && self.principal > 0.0) {
            return Err("loan principal must be above zero".into());
        }
        if !(self.rate_pct.is_finite() && (0.0..100.0).contains(&self.rate_pct)) {
            return Err("loan rate must be between 0 and 100 %".into());
        }
        if self.term_months == 0 || self.term_months > MAX_TERM {
            return Err(format!("loan term must be 1 to {MAX_TERM} months"));
        }
        self.start_date()?;
        if let Some(p) = self.payment {
            if !(p.is_finite() && p > 0.0) {
                return Err("loan payment must be above zero".into());
            }
            // A payment that does not cover the first month's interest grows the debt: the
            // schedule would never end, and drawing it would be a fiction.
            if p <= self.principal * self.monthly_rate() {
                return Err("the loan payment does not cover the interest, so the balance would never fall".into());
            }
        }
        Ok(())
    }

    pub fn start_date(&self) -> Result<Date, String> {
        Date::parse(&self.start, &Iso8601::DATE).map_err(|_| "loan start must be a YYYY-MM-DD date".to_string())
    }

    fn monthly_rate(&self) -> f64 {
        self.rate_pct / 100.0 / 12.0
    }

    /// The monthly payment: the stated one, else the annuity.
    pub fn payment(&self) -> f64 {
        self.payment
            .unwrap_or_else(|| annuity(self.principal, self.rate_pct, self.term_months))
    }

    /// Balance after `k` payments.
    pub fn balance_after(&self, k: u32) -> f64 {
        balance_after(self.principal, self.rate_pct, self.payment(), k.min(self.term_months))
    }

    /// Balance on `date`: None before the loan exists.
    pub fn balance_on(&self, date: Date) -> Option<f64> {
        let start = self.start_date().ok()?;
        if date < start {
            return None;
        }
        Some(self.balance_after(payments_made(start, date)))
    }

    /// One (due date, balance) per month from the start to `today`: the start itself at the
    /// full principal, then each payment that has fallen due. What the module writes as
    /// revisions, so the curve is exact on every due date.
    pub fn history(&self, today: Date) -> Vec<(Date, f64)> {
        let Ok(start) = self.start_date() else {
            return Vec::new();
        };
        if today < start {
            return Vec::new();
        }
        let made = payments_made(start, today).min(self.term_months);
        let mut out = Vec::with_capacity(made as usize + 1);
        for k in 0..=made {
            let bal = self.balance_after(k);
            out.push((add_months(start, k), bal));
            if bal <= 0.0 {
                break;
            }
        }
        out
    }

    /// The payments still to come after `today`, each split into interest and principal.
    pub fn schedule(&self, today: Date) -> Vec<ScheduleRow> {
        let Ok(start) = self.start_date() else {
            return Vec::new();
        };
        let r = self.monthly_rate();
        let payment = self.payment();
        let first = if today < start { 0 } else { payments_made(start, today) };
        let mut balance = self.balance_after(first);
        let mut out = Vec::new();
        for k in (first + 1)..=self.term_months {
            if balance <= 0.0 {
                break;
            }
            let interest = balance * r;
            // The last payment clears what is left, so the table ends at zero exactly.
            let pay = if k == self.term_months { balance + interest } else { payment.min(balance + interest) };
            let principal = pay - interest;
            balance = (balance - principal).max(0.0);
            out.push(ScheduleRow { n: k, date: add_months(start, k), payment: pay, interest, principal, balance });
        }
        out
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduleRow {
    /// Payment number, from 1.
    pub n: u32,
    #[serde(with = "date_iso")]
    pub date: Date,
    pub payment: f64,
    pub interest: f64,
    pub principal: f64,
    /// Balance once this payment is made.
    pub balance: f64,
}

mod date_iso {
    use serde::Serializer;
    use time::{format_description::well_known::Iso8601, Date};
    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.format(&Iso8601::DATE).map_err(serde::ser::Error::custom)?)
    }
}

/// The payment that clears `principal` in `n` months at `rate_pct` a year.
pub fn annuity(principal: f64, rate_pct: f64, n: u32) -> f64 {
    let r = rate_pct / 100.0 / 12.0;
    if n == 0 {
        return principal;
    }
    if r == 0.0 {
        return principal / n as f64;
    }
    principal * r / (1.0 - (1.0 + r).powi(-(n as i32)))
}

/// Balance after `k` payments of `payment`.
pub fn balance_after(principal: f64, rate_pct: f64, payment: f64, k: u32) -> f64 {
    let r = rate_pct / 100.0 / 12.0;
    let b = if r == 0.0 {
        principal - payment * k as f64
    } else {
        let g = (1.0 + r).powi(k as i32);
        principal * g - payment * (g - 1.0) / r
    };
    // A rounding residue of a cent below zero is a paid-off loan, not a credit.
    if b < 1e-6 {
        0.0
    } else {
        b
    }
}

/// Payments due in (start, at]: the first falls one month after the start.
pub fn payments_made(start: Date, at: Date) -> u32 {
    if at <= start {
        return 0;
    }
    let months = (at.year() - start.year()) * 12 + (at.month() as i32 - start.month() as i32);
    let mut k = months.max(0) as u32;
    // This month's payment is due on the start's day (clamped), not on the first.
    if k > 0 && at < add_months(start, k) {
        k -= 1;
    }
    k
}

/// `d` plus `n` months, the day clamped to the target month's length (Jan 31 + 1 = Feb 28).
pub fn add_months(d: Date, n: u32) -> Date {
    let total = d.year() * 12 + (d.month() as i32 - 1) + n as i32;
    let year = total.div_euclid(12);
    let month = Month::try_from((total.rem_euclid(12) + 1) as u8).unwrap_or(Month::January);
    let last = month.length(year);
    Date::from_calendar_date(year, month, d.day().min(last)).unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: Month, day: u8) -> Date {
        Date::from_calendar_date(y, m, day).unwrap()
    }

    fn mortgage() -> Loan {
        Loan { principal: 200_000.0, rate_pct: 6.0, term_months: 360, start: "2020-01-15".into(), payment: None }
    }

    /// The textbook case: 200 000 at 6 % over 30 years is 1 199.10 a month.
    #[test]
    fn annuity_matches_the_known_case() {
        assert!((mortgage().payment() - 1199.10).abs() < 0.01);
        // After one year: 197 543.98 still owed (standard amortization table).
        assert!((mortgage().balance_after(12) - 197_543.98).abs() < 0.05);
        // Zero rate: straight division.
        assert!((annuity(1200.0, 0.0, 12) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn balance_reaches_zero_at_term() {
        let m = mortgage();
        assert_eq!(m.balance_after(360), 0.0);
        assert!(m.balance_after(359) > 0.0);
        // Past the term the balance stays at zero.
        assert_eq!(m.balance_on(d(2060, Month::June, 1)), Some(0.0));
        // The remaining schedule ends at zero exactly, on the last due date.
        let rows = m.schedule(d(2049, Month::June, 20));
        let last = rows.last().unwrap();
        assert_eq!(last.n, 360);
        assert_eq!(last.balance, 0.0);
        assert_eq!(last.date, d(2050, Month::January, 15));
    }

    #[test]
    fn payments_fall_on_the_start_day() {
        let start = d(2020, Month::January, 15);
        assert_eq!(payments_made(start, d(2020, Month::January, 31)), 0);
        assert_eq!(payments_made(start, d(2020, Month::February, 14)), 0);
        assert_eq!(payments_made(start, d(2020, Month::February, 15)), 1);
        assert_eq!(payments_made(start, d(2021, Month::January, 15)), 12);
        // A month-end start clamps rather than skipping February.
        assert_eq!(add_months(d(2021, Month::January, 31), 1), d(2021, Month::February, 28));
        assert_eq!(payments_made(d(2021, Month::January, 31), d(2021, Month::February, 28)), 1);
    }

    #[test]
    fn history_is_one_point_per_due_date() {
        let m = mortgage();
        let h = m.history(d(2020, Month::April, 20));
        assert_eq!(h.len(), 4);
        assert_eq!(h[0], (d(2020, Month::January, 15), 200_000.0));
        assert_eq!(h[3].0, d(2020, Month::April, 15));
        assert!((h[3].1 - m.balance_after(3)).abs() < 1e-9);
        assert!(m.history(d(2019, Month::December, 1)).is_empty());
    }

    #[test]
    fn interest_and_principal_add_up_to_the_payment() {
        let rows = mortgage().schedule(d(2020, Month::January, 15));
        assert_eq!(rows[0].n, 1);
        assert!((rows[0].interest - 1000.0).abs() < 1e-6);
        for r in &rows[..359] {
            assert!((r.interest + r.principal - r.payment).abs() < 1e-9);
        }
    }

    #[test]
    fn terms_that_cannot_amortize_are_refused() {
        let mut l = mortgage();
        l.payment = Some(900.0); // interest alone is 1 000 a month
        assert!(l.check().is_err());
        l.payment = Some(1500.0);
        assert!(l.check().is_ok());
        l.start = "15/01/2020".into();
        assert!(l.check().is_err());
        assert!(Loan::parse(&serde_json::json!({ "principal": 0, "rate_pct": 1, "term_months": 12, "start": "2020-01-01" })).is_err());
    }
}
