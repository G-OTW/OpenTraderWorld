//! `risk` — volatility, drawdown, and return per unit of each.
//!
//! Everything here reads the **deposit-adjusted** curve and nothing else. On raw market value
//! a monthly savings plan would post a heroic Sharpe and a drawdown of zero, because every
//! contribution reads as a gain.

use serde_json::json;

use super::{Analysis, Block, Needs, Sample, Substrate};
use crate::quant;

pub struct Risk;

/// Below this many observations a volatility is noise. Reporting one anyway is how a
/// three-week-old portfolio gets told its Sharpe is 4.
const MIN_ROWS: usize = 30;

/// Drawdowns shallower than this are market breathing, not episodes worth a table row.
const DD_FLOOR: f64 = 0.02;

impl Analysis for Risk {
    fn key(&self) -> &'static str {
        "risk"
    }
    fn needs(&self) -> Needs {
        Needs::CURVE
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let Some(full) = s.curve.as_ref() else {
            return Block::unavailable(&["no_history"]);
        };
        let c = full.slice(
            s.window.from.map(|d| d.to_string()).as_deref(),
            s.window.to.map(|d| d.to_string()).as_deref(),
        );
        if c.rets.len() < MIN_ROWS {
            return Block::unavailable(&["too_short"]).with(Sample::rows(c.len()));
        }

        let ppy = c.ppy();
        let sd = quant::stddev(&c.rets);
        let vol = sd * ppy.sqrt();
        let total = c.total_return().unwrap_or(0.0);
        let years = c.years();
        let ann = quant::cagr(total, years).unwrap_or(total);
        let rf = s.pf.risk_free;
        // The risk-free rate has to be brought to the same per-period scale as the returns
        // before the shortfall is measured against it, or a 3% annual floor would read as a
        // 3% daily one and every day would be a shortfall.
        let rf_period = if ppy > 0.0 { rf / ppy } else { 0.0 };
        let downside = quant::downside_deviation(&c.rets, rf_period, ppy);
        let (max_dd, dd_curve) = quant::drawdown(&c.equity, &c.dates);
        let episodes = quant::underwater(&c.equity, &c.dates, DD_FLOOR);

        Block::ok(json!({
            "currency": c.currency,
            "window": s.window.label,
            // Measured, not read from a table: the same daily series is ~252 periods a year
            // on an exchange and ~365 on a 24/7 book, and a rebuilt curve holds every
            // calendar day while a live one only holds the days the job ran.
            "periods_per_year": ppy,
            "volatility_pct": vol * 100.0,
            "volatility_period_pct": sd * 100.0,
            "downside_deviation_pct": downside * 100.0,
            "max_drawdown_pct": max_dd * 100.0,
            "annualized_pct": ann * 100.0,
            "risk_free_pct": rf * 100.0,
            "sharpe": quant::sharpe(ann, vol, rf),
            "sortino": quant::sortino(ann, downside, rf),
            "calmar": quant::calmar(ann, max_dd),
            "best_day_pct": c.rets.iter().cloned().fold(f64::NEG_INFINITY, f64::max) * 100.0,
            "worst_day_pct": c.rets.iter().cloned().fold(f64::INFINITY, f64::min) * 100.0,
            "positive_days_pct": c.rets.iter().filter(|r| **r > 0.0).count() as f64
                / c.rets.len() as f64 * 100.0,
            "periods": calendar_periods(&c),
            // "Recovery time" is this table and not a scalar: one 40% hole that took two
            // years to fill and twelve 5% dips that filled in a week average the same and
            // have nothing else in common.
            "episodes": episodes,
            "drawdown_curve": dd_curve,
        }))
        .with(
            Sample::rows(c.len())
                .span(c.dates[0].clone(), c.dates[c.len() - 1].clone()),
        )
    }
}

/// Best and worst calendar week, month, quarter and year of the window.
///
/// Calendar buckets, not rolling ones: "worst month" means March, not the worst 30 days,
/// and a reader comparing against a statement wants the former.
fn calendar_periods(c: &super::Curve) -> serde_json::Value {
    let bucket = |grain: usize| -> Vec<(String, f64)> {
        let mut out: Vec<(String, f64)> = Vec::new();
        for i in 0..c.rets.len() {
            let d = &c.dates[i + 1];
            let key = period_key(d, grain);
            match out.last_mut() {
                Some((k, v)) if *k == key => *v = (*v + 1.0) * (1.0 + c.rets[i]) - 1.0,
                _ => out.push((key, c.rets[i])),
            }
        }
        out
    };
    let describe = |rows: Vec<(String, f64)>| {
        let best = rows.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let worst = rows.iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        json!({
            "count": rows.len(),
            "best": best.map(|(k, v)| json!({ "label": k, "pct": v * 100.0 })),
            "worst": worst.map(|(k, v)| json!({ "label": k, "pct": v * 100.0 })),
        })
    };
    json!({
        "month": describe(bucket(7)),
        "quarter": describe(bucket(0)),
        "year": describe(bucket(4)),
    })
}

/// The calendar bucket an ISO date falls in. `grain` is the prefix length (4 = year,
/// 7 = month); 0 means quarter, which needs the month to be read.
fn period_key(iso: &str, grain: usize) -> String {
    if grain == 0 {
        let year = &iso[..4.min(iso.len())];
        let month: u8 = iso.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(1);
        return format!("{year}-Q{}", (month.saturating_sub(1)) / 3 + 1);
    }
    iso[..grain.min(iso.len())].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_keys_bucket_by_calendar() {
        assert_eq!(period_key("2026-03-17", 4), "2026");
        assert_eq!(period_key("2026-03-17", 7), "2026-03");
        assert_eq!(period_key("2026-03-17", 0), "2026-Q1");
        assert_eq!(period_key("2026-12-31", 0), "2026-Q4");
        assert_eq!(period_key("2026-04-01", 0), "2026-Q2");
    }
}
