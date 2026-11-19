//! The daily curve, deposit-adjusted.
//!
//! A value series is not a return series. Every deposit reads as a rally and every withdrawal
//! as a crash, so a volatility, a drawdown or a Sharpe computed on stored market value
//! measures how much money was added, not what the investments did. The curve below removes
//! the day's external flow before it takes a return, which is the time-weighted convention
//! the DCA backtest already uses, and every risk measure in this module reads it and nothing
//! else.

use otw_store::portfolios::Snapshot;

use crate::quant;

/// A portfolio's history as something measurable.
#[derive(Debug, Clone)]
pub struct Curve {
    pub currency: String,
    /// ISO dates, oldest first.
    pub dates: Vec<String>,
    /// RFC3339 stamps of the same days, for the annualization measurement.
    clock: Vec<String>,
    /// Positions plus cash, per day.
    pub net_worth: Vec<f64>,
    /// Net external cash of the day, signed.
    pub flow: Vec<f64>,
    pub income: Vec<f64>,
    pub fees: Vec<f64>,
    /// Time-weighted period returns. One shorter than `dates`.
    pub rets: Vec<f64>,
    /// The same returns compounded from 1.0. Same length as `dates`.
    pub equity: Vec<f64>,
}

impl Curve {
    /// Build from stored snapshots, oldest first.
    ///
    /// Leading days worth nothing are dropped: a portfolio created in March has snapshots
    /// from March, and a leading zero would make its first real day an infinite return.
    pub fn build(currency: &str, snaps: &[Snapshot]) -> Curve {
        let mut dates = Vec::new();
        let mut clock = Vec::new();
        let mut net_worth = Vec::new();
        let mut flow = Vec::new();
        let mut income = Vec::new();
        let mut fees = Vec::new();
        let mut started = false;
        for s in snaps {
            let v = s.market_value + s.cash;
            if !started {
                if v.abs() < 1e-9 && s.flow.abs() < 1e-9 {
                    continue;
                }
                started = true;
            }
            let iso = s.snap_date.to_string();
            clock.push(format!("{iso}T00:00:00Z"));
            dates.push(iso);
            net_worth.push(v);
            flow.push(s.flow);
            income.push(s.income);
            fees.push(s.fees);
        }

        // r_t = (V_t − F_t) / V_{t−1} − 1. The flow is removed from the *end* value because
        // it is money that arrived, not money that was earned; a day whose opening value is
        // zero (the first deposit) has no return to speak of and contributes 0.
        // A return needs a positive opening value. `prev.abs()` would let a negative net
        // worth through and hand back a return with the sign inverted: a book that climbed
        // from -1000 to -900 would read as a 10% loss.
        let mut rets = Vec::with_capacity(dates.len().saturating_sub(1));
        for i in 1..net_worth.len() {
            let prev = net_worth[i - 1];
            rets.push(if prev > 1e-9 {
                (net_worth[i] - flow[i]) / prev - 1.0
            } else {
                0.0
            });
        }
        let equity = quant::compound(&rets);
        Curve { currency: currency.to_string(), dates, clock, net_worth, flow, income, fees, rets, equity }
    }

    /// The same curve restricted to a window, with its returns re-derived over it.
    ///
    /// Re-derived, not sliced: the time-weighted return of a sub-window is the compounding
    /// of that window's own returns, and carrying the parent's equity index in would report
    /// the whole history's growth under a one-month label.
    pub fn slice(&self, from: Option<&str>, to: Option<&str>) -> Curve {
        let keep: Vec<usize> = (0..self.dates.len())
            .filter(|i| {
                let d = self.dates[*i].as_str();
                from.is_none_or(|f| d >= f) && to.is_none_or(|t| d <= t)
            })
            .collect();
        let pick = |v: &[f64]| keep.iter().map(|i| v[*i]).collect::<Vec<f64>>();
        let dates: Vec<String> = keep.iter().map(|i| self.dates[*i].clone()).collect();
        let clock: Vec<String> = keep.iter().map(|i| self.clock[*i].clone()).collect();
        let net_worth = pick(&self.net_worth);
        let flow = pick(&self.flow);
        let mut rets = Vec::with_capacity(dates.len().saturating_sub(1));
        for i in 1..net_worth.len() {
            let prev = net_worth[i - 1];
            rets.push(if prev > 1e-9 { (net_worth[i] - flow[i]) / prev - 1.0 } else { 0.0 });
        }
        let equity = quant::compound(&rets);
        Curve {
            currency: self.currency.clone(),
            dates,
            clock,
            net_worth,
            flow,
            income: pick(&self.income),
            fees: pick(&self.fees),
            rets,
            equity,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.dates.is_empty()
    }

    pub fn len(&self) -> usize {
        self.dates.len()
    }

    /// Observations per year, **measured** off the clock rather than read from a table.
    ///
    /// The same daily timeframe is ~252 periods a year on an exchange and ~365 on a 24/7
    /// market, a rebuilt curve may hold every calendar day while a live one only holds the
    /// days the job ran, and for a mixed book both constants are wrong. Falls back to 252
    /// under 30 observations, which is where the measurement stops meaning anything.
    pub fn ppy(&self) -> f64 {
        crate::align::observed_ppy(&self.clock, 252.0)
    }

    /// Total time-weighted return over the whole curve.
    pub fn total_return(&self) -> Option<f64> {
        let last = *self.equity.last()?;
        (self.equity.len() > 1).then_some(last - 1.0)
    }

    /// Calendar years between the curve's first day and row `i`.
    ///
    /// Read off the dates, never off the index. A rebuilt curve holds every calendar day, so
    /// the two agree; a live one only holds the days the snapshot job ran, and there row 50
    /// of 100 is not the halfway point in time. Anything that discounts by elapsed time has
    /// to ask the date.
    fn years_at(&self, i: usize) -> f64 {
        let parse =
            |s: &str| time::Date::parse(s, &time::format_description::well_known::Iso8601::DATE).ok();
        match (self.dates.first().and_then(|s| parse(s)), self.dates.get(i).and_then(|s| parse(s))) {
            (Some(a), Some(b)) => (b - a).whole_days() as f64 / 365.25,
            _ => 0.0,
        }
    }

    /// Calendar years the curve spans, for annualizing.
    pub fn years(&self) -> f64 {
        self.years_at(self.dates.len().saturating_sub(1))
    }

    /// Dated external flows for a money-weighted return: every deposit negative (money in),
    /// every withdrawal positive, and the closing value as the final inflow.
    pub fn money_flows(&self) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        // The opening value is money already in the book at the start of the window.
        if let Some(v0) = self.net_worth.first() {
            if v0.abs() > 1e-9 {
                out.push((0.0, -(v0 - self.flow.first().copied().unwrap_or(0.0))));
            }
        }
        for i in 0..self.dates.len() {
            let f = self.flow[i];
            if f.abs() > 1e-9 {
                out.push((self.years_at(i), -f));
            }
        }
        if let Some(last) = self.net_worth.last() {
            out.push((self.years(), *last));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Month};

    fn snap(day: u8, value: f64, flow: f64) -> Snapshot {
        snap_on(Month::January, day, value, flow)
    }

    fn snap_on(month: Month, day: u8, value: f64, flow: f64) -> Snapshot {
        Snapshot {
            snap_date: Date::from_calendar_date(2026, month, day).unwrap(),
            currency: "EUR".into(),
            market_value: value,
            cost_basis: 0.0,
            cash: 0.0,
            flow,
            income: 0.0,
            fees: 0.0,
            source: "live".into(),
        }
    }

    /// The whole point of the type: a deposit is not a return.
    #[test]
    fn a_deposit_is_not_a_rally() {
        // 1000 → a 1000 deposit lands → 2000. Nothing was earned.
        let c = Curve::build("EUR", &[snap(1, 1000.0, 0.0), snap(2, 2000.0, 1000.0)]);
        assert_eq!(c.rets.len(), 1);
        assert!(c.rets[0].abs() < 1e-12, "{:?}", c.rets);
        assert!((c.equity[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_real_move_is_a_return() {
        let c = Curve::build("EUR", &[snap(1, 1000.0, 0.0), snap(2, 1100.0, 0.0)]);
        assert!((c.rets[0] - 0.1).abs() < 1e-12);
        assert!((c.total_return().unwrap() - 0.1).abs() < 1e-12);
    }

    /// A withdrawal is the same fact with the other sign.
    #[test]
    fn a_withdrawal_is_not_a_crash() {
        let c = Curve::build("EUR", &[snap(1, 1000.0, 0.0), snap(2, 500.0, -500.0)]);
        assert!(c.rets[0].abs() < 1e-12);
    }

    /// Snapshots taken before the portfolio held anything would make the first real day an
    /// infinite return, so they are dropped rather than divided by.
    #[test]
    fn leading_empty_days_are_dropped() {
        let c = Curve::build(
            "EUR",
            &[snap(1, 0.0, 0.0), snap(2, 0.0, 0.0), snap(3, 1000.0, 1000.0), snap(4, 1100.0, 0.0)],
        );
        assert_eq!(c.len(), 2);
        assert_eq!(c.dates[0], "2026-01-03");
        assert!((c.rets[0] - 0.1).abs() < 1e-12);
    }

    /// A window's return is its own, not the parent history's growth wearing a short label.
    #[test]
    fn a_slice_re_derives_its_own_returns() {
        let c = Curve::build(
            "EUR",
            &[snap(1, 100.0, 0.0), snap(2, 200.0, 0.0), snap(3, 220.0, 0.0)],
        );
        assert!((c.total_return().unwrap() - 1.2).abs() < 1e-12);
        let tail = c.slice(Some("2026-01-02"), None);
        assert_eq!(tail.len(), 2);
        assert!((tail.total_return().unwrap() - 0.1).abs() < 1e-12);
    }

    #[test]
    fn a_slice_outside_the_history_is_empty() {
        let c = Curve::build("EUR", &[snap(1, 100.0, 0.0), snap(2, 200.0, 0.0)]);
        assert!(c.slice(Some("2027-01-01"), None).is_empty());
    }

    /// A flow is discounted by when it happened, not by where it sits in the array. A live
    /// curve only holds the days the snapshot job ran, so the two are not the same thing.
    #[test]
    fn a_flow_is_dated_by_its_day_not_its_index() {
        // Three rows spanning a year, the middle one two days in: by index it would land at
        // half a year, which is what the IRR would then discount it by.
        let c = Curve::build(
            "EUR",
            &[
                snap_on(Month::January, 1, 1000.0, 0.0),
                snap_on(Month::January, 3, 2000.0, 1000.0),
                snap_on(Month::December, 31, 2200.0, 0.0),
            ],
        );
        let flows = c.money_flows();
        // The opening value is booked at t=0 and is also -1000 here, so the deposit is the
        // second one: the point of the test is the date it carries.
        let deposit = flows
            .iter()
            .filter(|(_, amount)| (*amount + 1000.0).abs() < 1e-9)
            .nth(1)
            .unwrap();
        assert!((deposit.0 - 2.0 / 365.25).abs() < 1e-9, "{:?}", flows);
        // The last row still closes the series at the curve's own span.
        assert!((flows.last().unwrap().0 - c.years()).abs() < 1e-12);
    }

    /// A negative opening value has no return: dividing by it inverts the sign, so a book
    /// climbing out of a hole would read as a loss.
    #[test]
    fn a_negative_opening_value_yields_no_return() {
        let c = Curve::build(
            "EUR",
            &[snap(1, -1000.0, 0.0), snap(2, -900.0, 0.0), snap(3, 1000.0, 0.0)],
        );
        assert_eq!(c.rets[0], 0.0);
        assert_eq!(c.rets[1], 0.0);
    }

    #[test]
    fn an_empty_history_is_empty_not_a_panic() {
        let c = Curve::build("EUR", &[]);
        assert!(c.is_empty());
        assert_eq!(c.total_return(), None);
        assert_eq!(c.years(), 0.0);
    }
}
