//! `benchmark` — the performance against the risk that was taken.
//!
//! The question the roadmap calls "mais surtout": a return with no benchmark and no risk
//! denominator is a number, not an answer. Beating an index by taking twice its risk is not
//! beating it, so the headline here is the **risk-matched return**: what the benchmark would
//! have returned at the portfolio's own volatility, next to what the portfolio actually did.

use std::collections::HashMap;

use serde_json::json;

use super::substrate::Series;
use super::{Analysis, Block, Needs, Sample, Substrate};
use crate::quant;

pub struct Benchmark;

/// Below this many shared days a beta, a tracking error and a capture ratio are all noise.
const MIN_ROWS: usize = 30;

impl Analysis for Benchmark {
    fn key(&self) -> &'static str {
        "benchmark"
    }
    fn needs(&self) -> Needs {
        Needs::CURVE | Needs::BENCHMARK
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let Some(full) = s.curve.as_ref() else {
            return Block::unavailable(&["no_history"]);
        };
        let Some(bench) = s.benchmark.as_ref() else {
            // No benchmark set, or one set to a symbol with no stored bars. Two different
            // problems, and the screen says which.
            return Block::unavailable(&[if s.pf.benchmark.is_none() {
                "no_benchmark"
            } else {
                "no_bars"
            }]);
        };
        let c = full.slice(
            s.window.from.map(|d| d.to_string()).as_deref(),
            s.window.to.map(|d| d.to_string()).as_deref(),
        );

        // The portfolio's curve is one point per day it was snapshotted; the benchmark is one
        // per session it traded. Aligning on the date is the only way the two mean the same
        // thing on the same row.
        let (pr, br, dates) = align(&c, bench);
        if pr.len() < MIN_ROWS {
            return Block::unavailable(&["too_short"]).with(Sample::rows(pr.len()));
        }

        let ppy = crate::align::observed_ppy(
            &dates.iter().map(|d| format!("{d}T00:00:00Z")).collect::<Vec<_>>(),
            252.0,
        );
        // Read off the calendar, not off the row count: the aligned series skips every day
        // one side did not trade, so `rows / ppy` is the span the rows imply, not the span
        // they cover, and it is what the annualized figures below are divided by.
        let years = span_years(&dates);
        let p_total = quant::compound(&pr).last().copied().unwrap_or(1.0) - 1.0;
        let b_total = quant::compound(&br).last().copied().unwrap_or(1.0) - 1.0;
        let p_ann = quant::cagr(p_total, years).unwrap_or(p_total);
        let b_ann = quant::cagr(b_total, years).unwrap_or(b_total);
        let p_vol = quant::stddev(&pr) * ppy.sqrt();
        let b_vol = quant::stddev(&br) * ppy.sqrt();
        let rf = s.pf.risk_free;

        let fit = quant::ols(&pr, &br);
        let (beta, r2) = fit.map(|(_, b, r)| (Some(b), Some(r))).unwrap_or((None, None));
        // Jensen's alpha on the annual figures, not the fitted intercept: the intercept is a
        // per-period number and reading it as an annual one overstates it by ~252.
        let alpha = beta.map(|b| p_ann - (rf + b * (b_ann - rf)));

        let diff: Vec<f64> = pr.iter().zip(&br).map(|(p, b)| p - b).collect();
        let tracking = quant::stddev(&diff) * ppy.sqrt();
        let info = (tracking > 1e-9).then(|| (p_ann - b_ann) / tracking);

        // Up and down capture: what share of the benchmark's good days and bad days the book
        // took. A defensive portfolio earns its keep in the second number.
        let capture = |up: bool| -> Option<f64> {
            let mut pp = Vec::new();
            let mut bb = Vec::new();
            for (p, b) in pr.iter().zip(&br) {
                if (up && *b > 0.0) || (!up && *b < 0.0) {
                    pp.push(*p);
                    bb.push(*b);
                }
            }
            let (pm, bm) = (quant::mean(&pp), quant::mean(&bb));
            (pp.len() >= 5 && bm.abs() > 1e-12).then(|| pm / bm * 100.0)
        };

        // The answer to the question. Levering the benchmark to the book's own volatility is
        // the only comparison that holds risk constant, and it is why a book that beat the
        // index can still lose this line.
        let risk_matched = (b_vol > 1e-9).then(|| rf + (b_ann - rf) * (p_vol / b_vol));

        let (p_dd, _) = quant::drawdown(&quant::compound(&pr), &dates);
        let (b_dd, _) = quant::drawdown(&quant::compound(&br), &dates);

        Block::ok(json!({
            "symbol": s.pf.benchmark.as_ref().and_then(super::substrate::benchmark_symbol),
            "periods_per_year": ppy,
            "portfolio": {
                "total_pct": p_total * 100.0,
                "annualized_pct": p_ann * 100.0,
                "volatility_pct": p_vol * 100.0,
                "max_drawdown_pct": p_dd * 100.0,
                "sharpe": quant::sharpe(p_ann, p_vol, rf),
            },
            "benchmark": {
                "total_pct": b_total * 100.0,
                "annualized_pct": b_ann * 100.0,
                "volatility_pct": b_vol * 100.0,
                "max_drawdown_pct": b_dd * 100.0,
                "sharpe": quant::sharpe(b_ann, b_vol, rf),
            },
            "excess_pct": (p_total - b_total) * 100.0,
            "alpha_pct": alpha.map(|a| a * 100.0),
            "beta": beta,
            "r_squared": r2,
            "tracking_error_pct": tracking * 100.0,
            "information_ratio": info,
            "up_capture_pct": capture(true),
            "down_capture_pct": capture(false),
            "risk_matched_pct": risk_matched.map(|r| r * 100.0),
            // The line the screen leads with: positive means the extra return was worth the
            // extra risk, negative means the same risk in the index would have paid more.
            "risk_adjusted_edge_pct": risk_matched.map(|r| (p_ann - r) * 100.0),
        }))
        .with(
            Sample::rows(pr.len())
                .span(dates.first().cloned().unwrap_or_default(), dates.last().cloned().unwrap_or_default()),
        )
    }
}

/// Calendar years between the first and last of a set of ISO days.
fn span_years(dates: &[String]) -> f64 {
    let parse = |s: &String| {
        time::Date::parse(s, &time::format_description::well_known::Iso8601::DATE).ok()
    };
    match (dates.first().and_then(parse), dates.last().and_then(parse)) {
        (Some(a), Some(b)) => (b - a).whole_days() as f64 / 365.25,
        _ => 0.0,
    }
}

/// Portfolio and benchmark returns on the **benchmark's own trading days**, plus those days.
///
/// The two series do not have the same shape: the curve holds every calendar day, the index
/// holds the sessions it traded. Dropping the pair whenever one side is missing a day is what
/// a naive alignment does, and it costs every Monday, every holiday and the day after each of
/// them: the portfolio's total came back at 11.8% where `performance` said 29.4%, and the
/// difference was the weekends. So the span between two consecutive sessions is measured
/// whole on both sides, with the external money that arrived inside it removed from the
/// portfolio's end value.
fn align(c: &super::Curve, bench: &Series) -> (Vec<f64>, Vec<f64>, Vec<String>) {
    let idx: HashMap<&str, usize> =
        c.dates.iter().enumerate().map(|(i, d)| (d.as_str(), i)).collect();
    // Flow up to and including each row, so a span's own flow is one subtraction whatever
    // its width. A single day's `flow[i]` is only right when the span is a single day.
    let mut cum = Vec::with_capacity(c.flow.len());
    let mut running = 0.0;
    for f in &c.flow {
        running += f;
        cum.push(running);
    }

    let (mut pr, mut br, mut dates) = (Vec::new(), Vec::new(), Vec::new());
    let mut prev: Option<(usize, f64)> = None;
    for (d, bv) in bench.dates.iter().zip(&bench.closes) {
        let Some(&i) = idx.get(d.as_str()) else {
            // A session the curve has no row for. `prev` is deliberately kept: the next pair
            // then spans both sessions on both sides, which is still like for like.
            continue;
        };
        if let Some((j, pb)) = prev {
            let pp = c.net_worth[j];
            // A positive opening value, never `abs()`: dividing by a negative one inverts the
            // sign, so a book climbing out of a hole would post a loss against the index.
            if pp > 1e-9 && pb > 0.0 {
                // The portfolio side stays deposit-adjusted: comparing a funded book against
                // an index would credit the savings rate to the strategy.
                pr.push((c.net_worth[i] - (cum[i] - cum[j])) / pp - 1.0);
                br.push(bv / pb - 1.0);
                dates.push(d.clone());
            }
        }
        prev = Some((i, *bv));
    }
    (pr, br, dates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use otw_store::portfolios::Snapshot;
    use time::{Date, Month};

    fn snap(day: u8, value: f64, flow: f64) -> Snapshot {
        Snapshot {
            snap_date: Date::from_calendar_date(2026, Month::June, day).unwrap(),
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

    fn series(days: &[(&str, f64)]) -> Series {
        Series {
            dates: days.iter().map(|(d, _)| d.to_string()).collect(),
            closes: days.iter().map(|(_, c)| *c).collect(),
        }
    }

    /// June 2026: the 5th is a Friday and the 8th a Monday. The curve holds the weekend, the
    /// index does not, and the weekend must not cost the comparison Monday's move.
    #[test]
    fn a_weekend_the_index_never_traded_is_not_a_hole() {
        let c = super::super::Curve::build(
            "EUR",
            &[
                snap(5, 100.0, 0.0),
                snap(6, 100.0, 0.0),
                snap(7, 100.0, 0.0),
                snap(8, 110.0, 0.0),
            ],
        );
        let b = series(&[("2026-06-05", 50.0), ("2026-06-08", 52.0)]);
        let (pr, br, dates) = align(&c, &b);
        assert_eq!(dates, vec!["2026-06-08"]);
        // Monday's 10% is the span's return, not a pair the alignment threw away.
        assert!((pr[0] - 0.1).abs() < 1e-12, "{pr:?}");
        assert!((br[0] - 0.04).abs() < 1e-12, "{br:?}");
    }

    /// The money that arrived inside a span is removed from that span, not from a single day:
    /// `flow[i]` alone would credit a Saturday deposit to Monday's performance.
    #[test]
    fn a_deposit_inside_a_span_is_removed_from_the_span() {
        let c = super::super::Curve::build(
            "EUR",
            &[
                snap(5, 100.0, 0.0),
                snap(6, 150.0, 50.0),
                snap(7, 150.0, 0.0),
                snap(8, 150.0, 0.0),
            ],
        );
        let b = series(&[("2026-06-05", 50.0), ("2026-06-08", 50.0)]);
        let (pr, _, _) = align(&c, &b);
        assert!(pr[0].abs() < 1e-12, "a deposit is not a 50% gain: {pr:?}");
    }
}
