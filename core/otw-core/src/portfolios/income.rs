//! Dividend calendar: forecast annual income, yield on cost, and the next dates.
//!
//! Every input is user entered on the asset (per-share amount, frequency, next ex and pay
//! dates). The quote sources this module prices with do not serve dividend data, so a
//! holding without an amount is listed as "no estimate", never as a zero, and the booked
//! dividends of the last twelve months stand next to the forecast as the measured figure.

use serde::Serialize;
use time::Date;
use uuid::Uuid;

use otw_store::portfolios::payments_per_year;

/// One holding's forecast, portfolio currency.
#[derive(Debug, Clone, Serialize)]
pub struct Forecast {
    pub asset_id: Uuid,
    pub symbol: String,
    pub quantity: f64,
    pub frequency: String,
    /// Per share per payment, converted to the portfolio currency.
    pub per_share: Option<f64>,
    pub annual: Option<f64>,
    /// Forecast annual income over the cost basis, percent.
    pub yield_on_cost_pct: Option<f64>,
    /// Dividends booked over the last twelve months.
    pub trailing_12m: f64,
    pub next_ex: Option<String>,
    pub next_pay: Option<String>,
    /// What the next payment should be: units held × per share.
    pub next_amount: Option<f64>,
}

/// What one holding needs to be forecast.
pub struct Holding {
    pub asset_id: Uuid,
    pub symbol: String,
    pub quantity: f64,
    pub cost_basis: f64,
    pub per_share_pf: Option<f64>,
    pub frequency: String,
    pub next_ex: Option<Date>,
    pub next_pay: Option<Date>,
    pub trailing_12m: f64,
}

/// Forecast one holding. No amount, or a frequency of `none`, is no estimate.
pub fn forecast(h: &Holding) -> Forecast {
    let per_year = payments_per_year(&h.frequency);
    let annual = h.per_share_pf.filter(|_| per_year > 0.0).map(|ps| h.quantity * ps * per_year);
    Forecast {
        asset_id: h.asset_id,
        symbol: h.symbol.clone(),
        quantity: h.quantity,
        frequency: h.frequency.clone(),
        per_share: h.per_share_pf,
        annual,
        yield_on_cost_pct: annual.filter(|_| h.cost_basis > 0.0).map(|a| a / h.cost_basis * 100.0),
        trailing_12m: h.trailing_12m,
        next_ex: h.next_ex.map(|d| d.to_string()),
        next_pay: h.next_pay.map(|d| d.to_string()),
        next_amount: h.per_share_pf.map(|ps| h.quantity * ps),
    }
}

/// One dated event of the calendar.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub date: String,
    /// `ex` or `pay`.
    pub kind: &'static str,
    pub asset_id: Uuid,
    pub symbol: String,
    pub amount: Option<f64>,
}

/// The upcoming ex and pay dates on or after `today`, soonest first.
pub fn upcoming(rows: &[Forecast], today: Date) -> Vec<Event> {
    let today = today.to_string();
    let mut out = Vec::new();
    for f in rows {
        for (date, kind) in [(&f.next_ex, "ex"), (&f.next_pay, "pay")] {
            if let Some(d) = date.as_ref().filter(|d| d.as_str() >= today.as_str()) {
                out.push(Event {
                    date: d.clone(),
                    kind,
                    asset_id: f.asset_id,
                    symbol: f.symbol.clone(),
                    amount: f.next_amount,
                });
            }
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.kind.cmp(b.kind)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn h(per: Option<f64>, freq: &str) -> Holding {
        Holding {
            asset_id: Uuid::nil(),
            symbol: "KO".into(),
            quantity: 100.0,
            cost_basis: 5000.0,
            per_share_pf: per,
            frequency: freq.into(),
            next_ex: None,
            next_pay: None,
            trailing_12m: 0.0,
        }
    }

    #[test]
    fn annual_income_is_units_times_amount_times_payments() {
        let f = forecast(&h(Some(0.5), "quarterly"));
        assert!((f.annual.unwrap() - 200.0).abs() < 1e-9);
        assert!((f.yield_on_cost_pct.unwrap() - 4.0).abs() < 1e-9);
        assert!((f.next_amount.unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn every_frequency_has_its_multiplier() {
        for (freq, n) in [("monthly", 12.0), ("quarterly", 4.0), ("semi", 2.0), ("annual", 1.0)] {
            let f = forecast(&h(Some(1.0), freq));
            assert!((f.annual.unwrap() - 100.0 * n).abs() < 1e-9, "{freq}");
        }
    }

    #[test]
    fn a_holding_without_data_has_no_estimate() {
        assert!(forecast(&h(None, "quarterly")).annual.is_none());
        assert!(forecast(&h(Some(1.0), "none")).annual.is_none());
        assert!(forecast(&h(None, "none")).yield_on_cost_pct.is_none());
    }

    #[test]
    fn past_dates_leave_the_calendar() {
        let today = Date::from_calendar_date(2026, Month::June, 1).unwrap();
        let mut a = h(Some(1.0), "annual");
        a.next_ex = Some(Date::from_calendar_date(2026, Month::May, 1).unwrap());
        a.next_pay = Some(Date::from_calendar_date(2026, Month::June, 20).unwrap());
        let ev = upcoming(&[forecast(&a)], today);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, "pay");
    }
}
