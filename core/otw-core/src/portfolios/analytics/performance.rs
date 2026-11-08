//! `performance` — what the book returned, per window, and what that is per year.
//!
//! Two numbers that answer different questions and are both reported because neither is the
//! other: **TWR** is what the investments did, and **IRR** is what the investor got. A book
//! that bought the dip beats its own TWR; one that bought the top loses to it.

use serde_json::json;

use super::{Analysis, Block, Needs, Sample, Substrate, Window};
use crate::quant;

pub struct Performance;

/// Windows the grid always reports, shortest first. `inception` is appended by `run`.
const GRID: &[&str] = &["1m", "3m", "6m", "ytd", "1y", "3y", "5y"];

/// Under a fifth of a year, an annual rate is an extrapolation. Same floor as [`quant::cagr`],
/// so TWR and IRR either both annualize or neither does.
const MIN_ANNUALIZED_YEARS: f64 = 0.2;

impl Analysis for Performance {
    fn key(&self) -> &'static str {
        "performance"
    }
    fn needs(&self) -> Needs {
        // Marks and the ledger feed the contribution bars; the headline needs the curve only.
        Needs::CURVE | Needs::MARKS | Needs::LEDGER
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let Some(full) = s.curve.as_ref() else {
            return Block::unavailable(&["no_history"]);
        };
        if full.len() < 2 {
            // One point is a value, not a performance. Saying "0%" here would be a number
            // the reader cannot tell from a flat year.
            return Block::unavailable(&["no_history"]).with(Sample::rows(full.len()));
        }

        let mut windows = Vec::new();
        for name in GRID.iter().chain(std::iter::once(&"inception")) {
            let w = Window::named(name, s.today).expect("the grid names known windows");
            let c = full.slice(w.from.map(|d| d.to_string()).as_deref(), None);
            windows.push(measure(name, w.from, &c));
        }

        let cur = full.slice(
            s.window.from.map(|d| d.to_string()).as_deref(),
            s.window.to.map(|d| d.to_string()).as_deref(),
        );
        Block::ok(json!({
            "currency": full.currency,
            "window": s.window.label,
            "current": measure(s.window.label, s.window.from, &cur),
            "windows": windows,
            "contributions": contributions(s, &cur),
            "series": {
                "dates": cur.dates,
                "equity": cur.equity,
                "net_worth": cur.net_worth,
            },
        }))
        .with(
            Sample::rows(cur.len())
                .span(cur.dates.first().cloned().unwrap_or_default(), cur.dates.last().cloned().unwrap_or_default()),
        )
    }
}

/// One window's return, annualized only when the window is long enough to mean it.
///
/// `from` is the day the *label* asked for, which is not the day the curve starts on.
fn measure(label: &str, from: Option<time::Date>, c: &super::Curve) -> serde_json::Value {
    let years = c.years();
    let twr = c.total_return();
    // An IRR is an annual rate exactly like a CAGR, so it earns the same floor: over three
    // weeks it extrapolates a fortnight into a year and prints 35%.
    let irr = (years >= MIN_ANNUALIZED_YEARS).then(|| quant::irr(&c.money_flows())).flatten();
    // `covered` is the honest half of the label: a "3y" row on a book that started eight
    // months ago is eight months, and reporting it as three years is how a tracker gets to
    // print an impressive annualized number. The curve has to reach back to the day the
    // window asked for; `None` (inception) asks for nothing and is always covered.
    let covered = c.len() > 1
        && from.is_none_or(|f| c.dates.first().is_some_and(|d| d.as_str() <= f.to_string().as_str()));
    json!({
        "window": label,
        "rows": c.len(),
        "years": years,
        "covered": covered,
        "twr_pct": twr.map(|r| r * 100.0),
        "annualized_pct": twr.and_then(|r| quant::cagr(r, years)).map(|r| r * 100.0),
        "irr_pct": irr.map(|r| r * 100.0),
        "from": c.dates.first(),
        "to": c.dates.last(),
        "start_value": c.net_worth.first(),
        "end_value": c.net_worth.last(),
        // `+ 0.0`: an empty window sums from `-0.0` and would print a negative zero.
        "net_flow": c.flow.iter().sum::<f64>() + 0.0,
    })
}

/// Each holding's part of the window's return, or why it cannot be said.
///
/// Every line held in the window needs a daily close: one without is named, and the bars
/// are not drawn, because folding its result into "cash" would credit the wrong line.
fn contributions(s: &Substrate<'_>, cur: &super::Curve) -> serde_json::Value {
    use otw_store::portfolios::ledger::{Ledger, Method};
    let (Some(book), Some(marks), Some(ops)) = (s.book.as_ref(), s.marks.as_ref(), s.ledger.as_ref()) else {
        return json!({ "status": "unavailable", "missing": ["no_bars"] });
    };
    if cur.len() < 2 {
        return json!({ "status": "unavailable", "missing": ["no_history"] });
    }
    // Units held at each day's close, through the same engine as the book.
    let ids: Vec<uuid::Uuid> = book.positions.iter().map(|p| p.asset.id).collect();
    let mut qty: Vec<Vec<f64>> = vec![Vec::with_capacity(cur.len()); ids.len()];
    let mut legs = Ledger::new(Method::Average);
    let mut i = 0;
    for d in &cur.dates {
        while i < ops.len() && ops[i].date.to_string().as_str() <= d.as_str() {
            legs.apply(&ops[i]);
            i += 1;
        }
        for (k, id) in ids.iter().enumerate() {
            qty[k].push(legs.position(id).map(|p| p.qty()).unwrap_or(0.0).max(0.0));
        }
    }
    let mut lines = Vec::new();
    let mut uncovered = Vec::new();
    for (k, p) in book.positions.iter().enumerate() {
        if qty[k].iter().all(|q| *q <= 1e-12) {
            continue;
        }
        let Some(series) = marks.series.get(&p.asset.id) else {
            uncovered.push(p.asset.symbol.clone());
            continue;
        };
        // The last close carries over a day the listing did not trade (a weekend), inside the
        // series only: past its last bar there is no mark to carry.
        let by = series.by_date();
        let last = series.dates.last().cloned().unwrap_or_default();
        let mut prev: Option<f64> = None;
        let close: Vec<Option<f64>> = cur
            .dates
            .iter()
            .map(|d| {
                if let Some(c) = by.get(d.as_str()) {
                    prev = Some(*c);
                } else if d.as_str() > last.as_str() {
                    prev = None;
                }
                prev
            })
            .collect();
        lines.push(super::attribution::Line {
            label: p.asset.symbol.clone(),
            qty: qty[k].clone(),
            close,
            fx: s.bar_fx.get(&p.asset.id).copied().unwrap_or(1.0),
        });
    }
    if !uncovered.is_empty() {
        return json!({ "status": "unavailable", "missing": ["no_bars"], "uncovered": uncovered });
    }
    match super::attribution::attribute(&lines, &cur.net_worth) {
        Some(a) => json!({ "status": "ok", "data": a }),
        None => json!({ "status": "unavailable", "missing": ["no_bars"] }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otw_store::portfolios::Snapshot;
    use time::{Date, Month};

    fn snap(month: Month, day: u8, value: f64, flow: f64) -> Snapshot {
        Snapshot {
            snap_date: Date::from_calendar_date(2026, month, day).unwrap(),
            currency: "EUR".into(),
            market_value: value,
            cost_basis: 0.0,
            cash: 0.0,
            flow,
            income: 0.0,
            fees: 0.0,
            source: "rebuilt".into(),
        }
    }

    /// A "3y" row on a book eight months old is eight months. Saying `covered` anyway is how
    /// a tracker prints a three-year number it does not have.
    #[test]
    fn a_window_the_curve_does_not_reach_back_to_is_not_covered() {
        let c = super::super::Curve::build(
            "EUR",
            &[
                snap(Month::January, 1, 1000.0, 1000.0),
                snap(Month::June, 30, 1200.0, 0.0),
            ],
        );
        let asked = Date::from_calendar_date(2023, Month::June, 30).unwrap();
        let three_years = measure("3y", Some(asked), &c);
        assert_eq!(three_years["covered"], serde_json::json!(false));

        let reached = Date::from_calendar_date(2026, Month::January, 1).unwrap();
        assert_eq!(measure("ytd", Some(reached), &c)["covered"], serde_json::json!(true));
        // Inception asks for nothing, so it is covered by whatever there is.
        assert_eq!(measure("inception", None, &c)["covered"], serde_json::json!(true));
    }

    /// Half a year of data annualizes; three weeks does not, and that has to hold for the
    /// money-weighted rate too or the two headline numbers disagree about their own units.
    #[test]
    fn a_short_window_annualizes_neither_return() {
        let c = super::super::Curve::build(
            "EUR",
            &[
                snap(Month::January, 1, 1000.0, 1000.0),
                snap(Month::January, 20, 1100.0, 0.0),
            ],
        );
        let m = measure("1m", None, &c);
        assert!(m["annualized_pct"].is_null());
        assert!(m["irr_pct"].is_null(), "{m}");
        assert!(m["twr_pct"].as_f64().unwrap() > 0.0);
    }
}
