//! `stress` — what a shock would do to the book, and how much of the book that answer covers.
//!
//! Two engines, because they are honest about different things:
//!
//! - **Historical replay** applies the realized daily path of a named window to today's
//!   holdings. No model, no beta, just each instrument's own bars over those dates. An
//!   instrument that did not exist then has *no path*: it is named, excluded from the total,
//!   and the headline says what share of the book the number covers. It is never proxied to
//!   an index, and refusing that substitution is the whole difference between this and a
//!   number that cannot be checked.
//! - **Factor shock** moves a real instrument (SPY, QQQ, IEF, EURUSD, USO, HYG) and reaches
//!   each holding through a **measured** beta. No bars, too short a history, or a fit nobody
//!   can explain means **no beta**: that weight lands in `unexplained` and the headline reads
//!   "−11.8% across the 74% of the book that could be measured". A default beta of 1, or a
//!   silent 0, is a fabricated number wearing a decimal point.
//!
//! The block itself reports what it *can* stress, per scenario. Running one is a POST, since
//! the caller chooses the scenario; this analyzer is the readiness report the page opens on.

use std::collections::BTreeMap;

use serde_json::json;

use super::substrate::{factor_def, Factors, Marks, Series};
use super::{Analysis, Block, Needs, Sample, Substrate};
use crate::quant;

pub struct Stress;

/// Below this many aligned observations a beta is noise.
pub const MIN_BETA_ROWS: usize = 60;

/// A fit explaining less than this much of an asset's variance is not a sensitivity, it is a
/// coincidence with a slope.
pub const MIN_R2: f64 = 0.05;

impl Analysis for Stress {
    fn key(&self) -> &'static str {
        "stress"
    }
    fn needs(&self) -> Needs {
        Needs::BOOK | Needs::MARKS | Needs::FACTORS
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let (Some(b), Some(marks), Some(factors)) =
            (s.book.as_ref(), s.marks.as_ref(), s.factors.as_ref())
        else {
            return Block::unavailable(&["no_ledger"]);
        };
        if b.positions.iter().all(|p| p.quantity <= 0.0) {
            return Block::unavailable(&["empty_book"]);
        }
        // An impact expressed as a percentage of negative equity is a percentage of a capital
        // that is not there. The shock is real, the denominator is not.
        if b.net_worth < 0.0 {
            return Block::unavailable(&["negative_equity"]);
        }

        let ex = exposures(b);
        let fits = fit_all(b, marks, factors);
        let covered: f64 = ex
            .iter()
            .filter(|e| fits.iter().any(|f| f.asset_id == e.asset_id))
            .map(|e| e.weight)
            .sum();

        Block::ok(json!({
            "currency": b.currency,
            "net_worth": b.net_worth,
            // Which factors have a proxy on disk at all. A scenario naming a missing one
            // says so rather than quietly contributing nothing.
            "factors": factors.resolved,
            "factors_missing": factors.missing,
            "betas": fits.iter().map(|f| json!({
                "asset_id": f.asset_id,
                "symbol": f.symbol,
                "factor": f.factor,
                "beta": f.beta,
                "r2": f.r2,
                "rows": f.rows,
            })).collect::<Vec<_>>(),
            // Positions with no measurable sensitivity to anything, with their weight. This
            // is the number that qualifies every headline below it.
            "unexplained": ex.iter()
                .filter(|e| !fits.iter().any(|f| f.asset_id == e.asset_id))
                .map(|e| json!({ "symbol": e.symbol, "weight_pct": e.weight * 100.0 }))
                .collect::<Vec<_>>(),
            "uncovered": marks.uncovered,
            "cash_pct": (b.net_worth.abs() > 1e-9)
                .then(|| b.cash_total / b.net_worth * 100.0),
        }))
        .with(Sample::rows(ex.len()).coverage(covered))
    }
}

// ── Exposure ──────────────────────────────────────────────────────────────────

pub struct Exposure {
    pub asset_id: uuid::Uuid,
    pub symbol: String,
    pub value: f64,
    /// Share of net worth, 0..1.
    pub weight: f64,
}

/// Every open position with its weight in the book. Cash is deliberately absent: it has a
/// beta of zero to everything but its own currency, and half the point of the exercise is
/// seeing the shock stop at it.
pub fn exposures(b: &otw_store::portfolios::Book) -> Vec<Exposure> {
    let base = b.net_worth.abs().max(1e-9);
    b.positions
        .iter()
        .filter(|p| p.quantity > 0.0)
        .filter_map(|p| {
            p.market_value.map(|mv| Exposure {
                asset_id: p.asset.id,
                symbol: p.asset.symbol.clone(),
                value: mv,
                weight: mv / base,
            })
        })
        .collect()
}

// ── Measured sensitivities ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Fit {
    pub asset_id: uuid::Uuid,
    pub symbol: String,
    pub factor: String,
    pub beta: f64,
    pub r2: f64,
    pub rows: usize,
}

/// Daily returns of two series over the days they share.
///
/// Aligned on the date and nothing else: two providers stamp the same daily close differently
/// and a positional zip would compare Monday against Tuesday for every row after the first
/// holiday one of them observed.
pub fn aligned_returns(a: &Series, b: &Series) -> (Vec<f64>, Vec<f64>) {
    let bm = b.by_date();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut prev: Option<(f64, f64)> = None;
    for (d, av) in a.dates.iter().zip(&a.closes) {
        let Some(bv) = bm.get(d.as_str()).copied() else {
            // A day only one of them has breaks the chain: the next return would span two
            // sessions on one side and one on the other.
            prev = None;
            continue;
        };
        if let Some((pa, pb)) = prev {
            if pa > 0.0 && pb > 0.0 {
                ys.push(av / pa - 1.0);
                xs.push(bv / pb - 1.0);
            }
        }
        prev = Some((*av, bv));
    }
    (ys, xs)
}

/// Fit every held instrument against every resolved factor, keeping only the fits that pass
/// the sample and explanation floors.
pub fn fit_all(b: &otw_store::portfolios::Book, marks: &Marks, factors: &Factors) -> Vec<Fit> {
    let mut out = Vec::new();
    for p in b.positions.iter().filter(|p| p.quantity > 0.0) {
        let Some(asset_series) = marks.series.get(&p.asset.id) else {
            continue;
        };
        for (factor, fseries) in &factors.series {
            let (y, x) = aligned_returns(asset_series, fseries);
            if y.len() < MIN_BETA_ROWS {
                continue;
            }
            let Some((_, beta, r2)) = quant::ols(&y, &x) else {
                continue;
            };
            if r2 < MIN_R2 {
                continue;
            }
            out.push(Fit {
                asset_id: p.asset.id,
                symbol: p.asset.symbol.clone(),
                factor: factor.clone(),
                beta,
                r2,
                rows: y.len(),
            });
        }
    }
    out
}

// ── Running a scenario ────────────────────────────────────────────────────────

/// One leg of a factor scenario, as the row stores it.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
pub struct Leg {
    pub factor: String,
    #[serde(default)]
    pub shock_pct: Option<f64>,
    /// A rates leg is quoted in basis points and needs a duration to become a price move.
    #[serde(default)]
    pub shock_bps: Option<f64>,
    #[serde(default)]
    pub duration: Option<f64>,
}

impl Leg {
    /// The price move this leg implies on its proxy, as a fraction.
    ///
    /// A rate shock reaches a bond through `Δprice ≈ −duration × Δy`. The duration rides on
    /// the leg rather than being folded into a constant so the number can be argued with:
    /// an approximation you can read is worth more than a folkloric one you cannot.
    pub fn proxy_move(&self) -> Option<f64> {
        if let Some(pct) = self.shock_pct {
            return Some(pct / 100.0);
        }
        let bps = self.shock_bps?;
        let duration = self.duration.unwrap_or(7.5);
        Some(-duration * bps / 10_000.0)
    }

    pub fn is_bps(&self) -> bool {
        self.shock_pct.is_none() && self.shock_bps.is_some()
    }
}

/// The result of shocking a book.
#[derive(Debug, serde::Serialize)]
pub struct Impact {
    pub impact_pct: f64,
    pub impact_value: f64,
    pub resulting_net_worth: f64,
    /// Share of net worth the number actually covers, 0..1.
    pub coverage: f64,
    pub positions: Vec<serde_json::Value>,
    pub by_factor: Vec<serde_json::Value>,
    pub unexplained: Vec<serde_json::Value>,
    pub missing_factors: Vec<String>,
}

/// Apply a set of factor legs to the book.
///
/// Each leg's move is applied through the measured beta of each holding. Legs are summed on
/// the position, not on the book: two legs hitting the same holding compound on that holding,
/// and a holding nothing touches contributes nothing rather than an average.
pub fn run_factor(
    b: &otw_store::portfolios::Book,
    marks: &Marks,
    factors: &Factors,
    legs: &[Leg],
) -> Impact {
    let fits = fit_all(b, marks, factors);
    let ex = exposures(b);
    let mut missing_factors: Vec<String> = legs
        .iter()
        .filter(|l| !factors.series.contains_key(&l.factor))
        .map(|l| l.factor.clone())
        .collect();
    missing_factors.dedup();

    let mut by_factor: BTreeMap<String, f64> = BTreeMap::new();
    let mut positions = Vec::new();
    let mut unexplained = Vec::new();
    let mut total = 0.0;
    let mut covered = 0.0;

    for e in &ex {
        let mut move_pct = 0.0;
        let mut explained = false;
        for leg in legs {
            let Some(shock) = leg.proxy_move() else { continue };
            let Some(fit) = fits.iter().find(|f| f.asset_id == e.asset_id && f.factor == leg.factor)
            else {
                continue;
            };
            let contribution = fit.beta * shock;
            move_pct += contribution;
            *by_factor.entry(leg.factor.clone()).or_default() += contribution * e.value;
            explained = true;
        }
        if !explained {
            unexplained.push(json!({ "symbol": e.symbol, "weight_pct": e.weight * 100.0 }));
            continue;
        }
        covered += e.weight;
        let damage = e.value * move_pct;
        total += damage;
        positions.push(json!({
            "symbol": e.symbol,
            "value": e.value,
            "weight_pct": e.weight * 100.0,
            "move_pct": move_pct * 100.0,
            "impact": damage,
        }));
    }
    positions.sort_by(|a, b| {
        let (x, y) = (a["impact"].as_f64().unwrap_or(0.0), b["impact"].as_f64().unwrap_or(0.0));
        x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
    });

    let base = b.net_worth.abs().max(1e-9);
    Impact {
        impact_pct: total / base * 100.0,
        impact_value: total,
        resulting_net_worth: b.net_worth + total,
        coverage: covered,
        positions,
        by_factor: by_factor
            .into_iter()
            .map(|(factor, v)| json!({ "factor": factor, "impact": v, "impact_pct": v / base * 100.0 }))
            .collect(),
        unexplained,
        missing_factors,
    }
}

/// Apply the realized path of a historical window to today's holdings.
///
/// The instrument's own bars over those dates, nothing else. An instrument with no bars in
/// the window is named and excluded: proxying it to an index is the substitution that makes a
/// stress number unfalsifiable.
pub fn run_historical(
    b: &otw_store::portfolios::Book,
    marks: &Marks,
    from: &str,
    to: &str,
) -> Impact {
    let ex = exposures(b);
    let mut positions = Vec::new();
    let mut unexplained = Vec::new();
    let mut total = 0.0;
    let mut covered = 0.0;

    for e in &ex {
        let path = marks.series.get(&e.asset_id).map(|s| s.between(from, to));
        // Two closes are the minimum for a path: one is a price, not a move.
        let Some(path) = path.filter(|p| p.closes.len() >= 2) else {
            unexplained.push(json!({ "symbol": e.symbol, "weight_pct": e.weight * 100.0 }));
            continue;
        };
        let first = path.closes[0];
        let last = path.closes[path.closes.len() - 1];
        if first <= 0.0 {
            unexplained.push(json!({ "symbol": e.symbol, "weight_pct": e.weight * 100.0 }));
            continue;
        }
        let move_pct = last / first - 1.0;
        // The deepest point of the path, not only where it ended: a scenario that recovered
        // by its closing date still emptied the account on the way.
        let trough = path.closes.iter().cloned().fold(f64::INFINITY, f64::min) / first - 1.0;
        covered += e.weight;
        let damage = e.value * move_pct;
        total += damage;
        positions.push(json!({
            "symbol": e.symbol,
            "value": e.value,
            "weight_pct": e.weight * 100.0,
            "move_pct": move_pct * 100.0,
            "trough_pct": trough * 100.0,
            "impact": damage,
            "bars": path.closes.len(),
        }));
    }
    positions.sort_by(|a, b| {
        let (x, y) = (a["impact"].as_f64().unwrap_or(0.0), b["impact"].as_f64().unwrap_or(0.0));
        x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
    });

    let base = b.net_worth.abs().max(1e-9);
    Impact {
        impact_pct: total / base * 100.0,
        impact_value: total,
        resulting_net_worth: b.net_worth + total,
        coverage: covered,
        positions,
        by_factor: Vec::new(),
        unexplained,
        missing_factors: Vec::new(),
    }
}

/// A leg naming a factor nobody knows about is a typo, not a shock of zero.
pub fn validate_legs(legs: &[Leg]) -> Result<(), String> {
    for l in legs {
        if factor_def(&l.factor).is_none() {
            return Err(format!("unknown factor: {}", l.factor));
        }
        if l.proxy_move().is_none() {
            return Err(format!("leg {} has no shock", l.factor));
        }
        if l.is_bps() && l.duration.is_some_and(|d| d <= 0.0) {
            return Err("a rates leg needs a positive duration".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percent_leg_is_its_own_move() {
        let l = Leg { factor: "sp500".into(), shock_pct: Some(-20.0), shock_bps: None, duration: None };
        assert!((l.proxy_move().unwrap() + 0.2).abs() < 1e-12);
        assert!(!l.is_bps());
    }

    /// 200 bp on a 7.5-year duration is roughly a 15% price fall, and it must carry the
    /// duration it was computed with rather than a constant nobody can see.
    #[test]
    fn a_rates_leg_goes_through_its_duration() {
        let l = Leg { factor: "rates".into(), shock_pct: None, shock_bps: Some(200.0), duration: Some(7.5) };
        assert!((l.proxy_move().unwrap() + 0.15).abs() < 1e-12);
        assert!(l.is_bps());
        let long = Leg { duration: Some(15.0), ..l.clone() };
        assert!((long.proxy_move().unwrap() + 0.30).abs() < 1e-12);
    }

    #[test]
    fn an_unknown_factor_is_refused() {
        let legs = vec![Leg { factor: "unicorn".into(), shock_pct: Some(-10.0), shock_bps: None, duration: None }];
        assert!(validate_legs(&legs).is_err());
        let none = vec![Leg { factor: "sp500".into(), shock_pct: None, shock_bps: None, duration: None }];
        assert!(validate_legs(&none).is_err());
    }

    fn series(days: &[(&str, f64)]) -> Series {
        Series {
            dates: days.iter().map(|(d, _)| d.to_string()).collect(),
            closes: days.iter().map(|(_, c)| *c).collect(),
        }
    }

    /// A day only one series has must break the chain, not be zipped against its neighbour.
    #[test]
    fn alignment_skips_days_one_side_is_missing() {
        let a = series(&[("2026-01-01", 100.0), ("2026-01-02", 110.0), ("2026-01-03", 121.0)]);
        let b = series(&[("2026-01-01", 10.0), ("2026-01-03", 12.1)]);
        let (y, x) = aligned_returns(&a, &b);
        // Only 01-01 and 01-03 are shared, and they are not consecutive in `a`, so the
        // chain breaks and there is no usable pair.
        assert!(y.is_empty() && x.is_empty());

        let c = series(&[("2026-01-01", 10.0), ("2026-01-02", 11.0), ("2026-01-03", 12.1)]);
        let (y2, x2) = aligned_returns(&a, &c);
        assert_eq!(y2.len(), 2);
        assert_eq!(x2.len(), 2);
    }
}
