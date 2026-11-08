//! Open-position risk for the Trading Journal.
//!
//! Every other analytics file in this module answers a question about the past. This one
//! answers the only question that can still be acted on: **what am I exposed to right
//! now**, and is it more than I think.
//!
//! Three measures, and the third is the one a trade log alone always gets wrong:
//!
//! * **Simultaneous exposure**: how many positions are open, what they are worth, and how
//!   much of the account is at stake if every planned stop is hit.
//! * **Concentration**: which symbol, which asset class, which side, which strategy
//!   carries that risk. Reported with a Herfindahl index, so "five positions" and "five
//!   positions that are 90 % one name" do not read the same.
//! * **Correlation** (filled in by `otw-core`, which owns the bars): two positions at 0.9
//!   are one position with twice the size. The naive sum of risks says 3 %; the
//!   correlation-adjusted figure says what is actually on the line.
//!
//! Only *open* positions (`open_qty > 0`) are in scope, partially closed ones included at
//! their remaining size. A closed trade carries no exposure, so it is not a risk question
//! any more, it is a performance one and the rest of the module already answers it.
//!
//! Money is FX-converted into the journal's display currency exactly like the breakdown
//! and the analytics do, at the position's entry date.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::journal::{self, Trade, TradeFilter};
use crate::journal_analytics::trade_stop;
use crate::journal_behavior::Insight;

/// A position needs at least this many peers before a concentration statement is made:
/// a lone position is 100 % of the book and saying so helps nobody.
const MIN_POSITIONS: usize = 2;
/// Share of the open risk above which one line is called a concentration, in percent.
const CONCENTRATED_PCT: f64 = 40.0;
/// Pairwise correlation above which two positions are called one.
const CLUSTER_CORR: f64 = 0.8;

// ── One open position ────────────────────────────────────────────────────────

/// An open trade reduced to what a risk read needs. Prices and quantity are in the
/// instrument's own units; money is in the display currency.
#[derive(Debug, Serialize)]
pub struct Position {
    pub id: Uuid,
    pub ticker: String,
    pub asset_class: String,
    pub side: String,
    pub currency: String,
    /// Size still open (a partial scale-out leaves the remainder here).
    pub open_qty: f64,
    pub avg_entry: Option<f64>,
    pub stop_price: Option<f64>,
    pub leverage: f64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub entry_at: Option<OffsetDateTime>,
    /// Calendar days the position has been open.
    pub days_open: Option<i64>,
    /// Open size at cost, in the display currency.
    pub notional: f64,
    /// What is lost if the planned stop is hit, in the display currency. `None` = the
    /// trade carries no stop, so its risk is unknown rather than zero.
    pub risk: Option<f64>,
    pub risk_pct: Option<f64>,
    /// Share of the book's total open risk, in percent.
    pub risk_share: Option<f64>,
    /// Last stored close and the moment it was stamped; filled by `otw-core` when bars
    /// are available for the instrument.
    pub last_price: Option<f64>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_at: Option<OffsetDateTime>,
    /// Open PnL at that price, in the display currency, and against the entry notional.
    pub unrealized: Option<f64>,
    pub unrealized_pct: Option<f64>,
    pub strategy_id: Option<Uuid>,
    pub strategy: String,
}

impl Position {
    /// +1 for a long, −1 for a short: the direction the position profits from.
    fn dir(&self) -> f64 {
        if self.side.eq_ignore_ascii_case("short") {
            -1.0
        } else {
            1.0
        }
    }
}

// ── Concentration ────────────────────────────────────────────────────────────

/// One line of a concentration table: a ticker, an asset class, a side or a strategy.
#[derive(Debug, Serialize)]
pub struct ExposureRow {
    pub key: String,
    pub name: String,
    pub positions: i64,
    pub notional: f64,
    /// Share of the gross open notional, in percent.
    pub notional_share: f64,
    /// Summed planned risk of the lines that carry a stop, and its share of the total.
    pub risk: f64,
    pub risk_share: Option<f64>,
    pub unrealized: f64,
}

/// Herfindahl index over a set of shares expressed in percent: the sum of the squared
/// weights, `1/n` when everything is even and `1` when one line is everything.
///
/// Its reciprocal is the honest headline: `1/H` is the **effective number of positions**,
/// which is what "diversified" actually means. Five lines with a Herfindahl of 0.6 are
/// 1.7 positions wearing five names.
fn herfindahl(shares_pct: &[f64]) -> Option<f64> {
    let total: f64 = shares_pct.iter().sum();
    if total <= 0.0 {
        return None;
    }
    Some(shares_pct.iter().map(|s| (s / total).powi(2)).sum())
}

// ── Correlation (filled in by the caller, which owns the bars) ────────────────

/// How the open positions move together, over the bars the catalog already holds.
#[derive(Debug, Serialize)]
pub struct CorrelationBlock {
    /// Instruments that had enough aligned bars to be correlated, in matrix order.
    pub labels: Vec<String>,
    /// Row-major square matrix of Pearson correlations over aligned returns.
    pub matrix: Vec<Vec<f64>>,
    /// Rows the aligned clock kept, and the timeframe it was built on.
    pub observations: i64,
    pub timeframe: String,
    /// Open positions left out because their instrument had no usable bars.
    pub uncovered: Vec<String>,
    /// Mean of the off-diagonal correlations.
    pub avg_corr: Option<f64>,
    /// The most correlated pair, when there is one.
    pub max_pair: Option<(String, String, f64)>,
    /// Naive total risk: every stop hit at once, added up.
    pub risk_sum: Option<f64>,
    /// Risk if the positions were independent, `sqrt(Σ rᵢ²)`.
    pub risk_independent: Option<f64>,
    /// Risk at the measured correlations, `sqrt(rᵀCr)`. Between the two above, and the
    /// only one of the three that describes the book as it actually is.
    pub risk_correlated: Option<f64>,
    /// `risk_correlated / risk_independent`: 1 = uncorrelated, higher = stacked.
    pub stacking: Option<f64>,
    /// Effective number of independent bets behind the open positions.
    pub effective_bets: Option<f64>,
}

// ── Payload ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Exposure {
    pub display_currency: String,
    pub invested_capital: f64,
    pub positions: Vec<Position>,
    pub open_count: i64,
    /// Positions whose money could not be converted on their entry date; excluded from
    /// every total rather than counted at the wrong rate.
    pub unconverted: i64,
    /// Open size at cost: gross adds the shorts, net nets them off.
    pub gross_notional: f64,
    pub net_notional: f64,
    pub long_notional: f64,
    pub short_notional: f64,
    pub gross_pct: Option<f64>,
    pub net_pct: Option<f64>,
    /// Summed planned risk, and how much of the account that is.
    pub total_risk: f64,
    pub total_risk_pct: Option<f64>,
    /// Positions carrying a planned stop, and those that do not.
    pub with_stop: i64,
    pub without_stop: i64,
    /// Open PnL at the last stored price, over the positions that could be marked.
    pub unrealized: f64,
    pub marked: i64,
    /// The single largest line, by notional and by risk, in percent.
    pub top_notional_share: Option<f64>,
    pub top_risk_share: Option<f64>,
    /// Herfindahl over the open risk (falling back to notional when no stop is logged)
    /// and the effective number of positions it implies.
    pub herfindahl: Option<f64>,
    pub effective_positions: Option<f64>,
    pub by_ticker: Vec<ExposureRow>,
    pub by_asset_class: Vec<ExposureRow>,
    pub by_side: Vec<ExposureRow>,
    pub by_strategy: Vec<ExposureRow>,
    /// Filled by `otw-core` once the bars are read; `None` = nothing to correlate.
    pub correlation: Option<CorrelationBlock>,
    /// Statements that cleared both a sample floor and a threshold, same shape as the
    /// behaviour tab's: the server decides *whether*, the client formats the numbers.
    pub warnings: Vec<Insight>,
}

/// Invested capital for the scope, mirroring `journal_analytics`: capital is a property
/// of the category, not of the trade filters.
async fn invested_capital(
    pool: &PgPool,
    fx: &mut crate::journal_fx::FxCache,
    category_id: Option<Uuid>,
    display_currency: &str,
) -> anyhow::Result<f64> {
    let events = sqlx::query_as::<_, (f64, String, OffsetDateTime, String)>(
        "SELECT amount, currency, occurred_at, kind FROM journal_capital_events \
         WHERE ($1::uuid IS NULL OR category_id = $1)",
    )
    .bind(category_id)
    .fetch_all(pool)
    .await
    .context("loading capital events")?;
    let mut total = 0.0;
    for (amount, currency, occurred_at, kind) in &events {
        let signed = if kind == "withdrawal" { -amount } else { *amount };
        if let Some(v) = fx
            .convert(pool, signed, currency, display_currency, occurred_at.date())
            .await?
        {
            total += v;
        }
    }
    Ok(total)
}

/// Risk still on the table for an open position: the distance from the average entry to
/// the planned stop, over the size that is **still open**.
///
/// Deliberately not `journal_analytics::trade_risk`, which sizes on the entry quantity:
/// after a scale-out only the remainder can still be lost.
fn open_risk(t: &Trade) -> Option<f64> {
    let entry = t.avg_entry?;
    let stop = trade_stop(t)?;
    let qty = t.open_qty;
    if qty <= 0.0 || t.multiplier <= 0.0 {
        return None;
    }
    let per_unit = (entry - stop).abs();
    (per_unit > 0.0).then_some(per_unit * qty * t.multiplier)
}

/// Build one concentration table. `key_of` yields the group key and its display name.
fn group_by<F>(
    positions: &[Position],
    gross_notional: f64,
    total_risk: f64,
    mut key_of: F,
) -> Vec<ExposureRow>
where
    F: FnMut(&Position) -> (String, String),
{
    #[derive(Default)]
    struct Acc {
        name: String,
        positions: i64,
        notional: f64,
        risk: f64,
        unrealized: f64,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for p in positions {
        let (key, name) = key_of(p);
        let e = acc.entry(key).or_default();
        if e.name.is_empty() {
            e.name = name;
        }
        e.positions += 1;
        e.notional += p.notional;
        e.risk += p.risk.unwrap_or(0.0);
        e.unrealized += p.unrealized.unwrap_or(0.0);
    }
    let mut rows: Vec<ExposureRow> = acc
        .into_iter()
        .map(|(key, a)| ExposureRow {
            key,
            name: a.name,
            positions: a.positions,
            notional: a.notional,
            notional_share: if gross_notional > 0.0 {
                a.notional / gross_notional * 100.0
            } else {
                0.0
            },
            risk: a.risk,
            risk_share: (total_risk > 0.0).then(|| a.risk / total_risk * 100.0),
            unrealized: a.unrealized,
        })
        .collect();
    // Biggest exposure first: the table is read to find where the money is.
    rows.sort_by(|a, b| {
        b.risk
            .total_cmp(&a.risk)
            .then(b.notional.total_cmp(&a.notional))
    });
    rows
}

/// The open book, converted and grouped. The correlation block and the marks are added
/// by `otw-core`, which is the crate that may read bars.
pub async fn exposure(
    pool: &PgPool,
    filter: &TradeFilter,
    display_currency: &str,
) -> anyhow::Result<Exposure> {
    let trades = journal::list_trades(pool, filter).await?;
    let strategies = journal::strategy_names(pool).await?;
    let mut fx = crate::journal_fx::FxCache::new();
    let capital = invested_capital(pool, &mut fx, filter.category_id, display_currency).await?;
    let now = OffsetDateTime::now_utc();

    let mut positions = Vec::new();
    let mut unconverted = 0i64;
    for t in &trades {
        // Open means size still on: a partial scale-out leaves the remainder exposed.
        if t.open_qty <= 0.0 {
            continue;
        }
        let on = t.entry_at.unwrap_or(t.created_at).date();
        let native_notional = t.avg_entry.unwrap_or(0.0).abs() * t.open_qty * t.multiplier;
        let Some(notional) = fx
            .convert(pool, native_notional, &t.currency, display_currency, on)
            .await?
        else {
            unconverted += 1;
            continue;
        };
        let risk = match open_risk(t) {
            Some(r) => fx.convert(pool, r, &t.currency, display_currency, on).await?,
            None => None,
        };
        positions.push(Position {
            id: t.id,
            ticker: t.ticker.trim().to_uppercase(),
            asset_class: t.asset_class.clone(),
            side: t.side.clone(),
            currency: t.currency.clone(),
            open_qty: t.open_qty,
            avg_entry: t.avg_entry,
            stop_price: trade_stop(t),
            leverage: t.leverage,
            entry_at: t.entry_at,
            days_open: t.entry_at.map(|e| (now - e).whole_days().max(0)),
            notional,
            risk,
            risk_pct: match risk {
                Some(r) if capital > 0.0 => Some(r / capital * 100.0),
                _ => None,
            },
            risk_share: None,
            last_price: None,
            last_at: None,
            unrealized: None,
            unrealized_pct: None,
            strategy_id: t.strategy_id,
            strategy: t
                .strategy_id
                .and_then(|id| strategies.get(&id).cloned())
                .unwrap_or_default(),
        });
    }
    // Biggest first, so the read starts where the money is.
    positions.sort_by(|a, b| b.notional.total_cmp(&a.notional));

    let total_risk: f64 = positions.iter().filter_map(|p| p.risk).sum();
    for p in positions.iter_mut() {
        p.risk_share = match p.risk {
            Some(r) if total_risk > 0.0 => Some(r / total_risk * 100.0),
            _ => None,
        };
    }

    Ok(seal(
        positions,
        capital,
        unconverted,
        display_currency,
        total_risk,
    ))
}

/// Totals, concentration tables and warnings over an already converted position list.
/// Split out so `otw-core` can mark the positions and rebuild the aggregates over the
/// same code, rather than a second implementation that drifts.
pub fn seal(
    positions: Vec<Position>,
    capital: f64,
    unconverted: i64,
    display_currency: &str,
    total_risk: f64,
) -> Exposure {
    let long_notional: f64 = positions
        .iter()
        .filter(|p| p.dir() > 0.0)
        .map(|p| p.notional)
        .sum();
    let short_notional: f64 = positions
        .iter()
        .filter(|p| p.dir() < 0.0)
        .map(|p| p.notional)
        .sum();
    let gross_notional = long_notional + short_notional;

    let by_ticker = group_by(&positions, gross_notional, total_risk, |p| {
        (p.ticker.clone(), p.ticker.clone())
    });
    let by_asset_class = group_by(&positions, gross_notional, total_risk, |p| {
        (p.asset_class.clone(), p.asset_class.clone())
    });
    let by_side = group_by(&positions, gross_notional, total_risk, |p| {
        (p.side.clone(), p.side.clone())
    });
    let by_strategy = group_by(&positions, gross_notional, total_risk, |p| {
        match p.strategy_id {
            Some(id) => (id.to_string(), p.strategy.clone()),
            None => (String::new(), String::new()),
        }
    });

    // Concentration is measured on risk when stops are logged, on notional otherwise:
    // the question is "how much of what is at stake sits on one name", and without a
    // stop the size is the only proxy for what is at stake.
    let shares: Vec<f64> = if total_risk > 0.0 {
        by_ticker.iter().map(|r| r.risk).collect()
    } else {
        by_ticker.iter().map(|r| r.notional).collect()
    };
    let h = herfindahl(&shares);

    let unrealized: f64 = positions.iter().filter_map(|p| p.unrealized).sum();
    let marked = positions.iter().filter(|p| p.last_price.is_some()).count() as i64;
    let with_stop = positions.iter().filter(|p| p.risk.is_some()).count() as i64;

    let mut e = Exposure {
        display_currency: display_currency.to_string(),
        invested_capital: capital,
        open_count: positions.len() as i64,
        unconverted,
        gross_notional,
        net_notional: long_notional - short_notional,
        long_notional,
        short_notional,
        gross_pct: (capital > 0.0).then(|| gross_notional / capital * 100.0),
        net_pct: (capital > 0.0).then(|| (long_notional - short_notional) / capital * 100.0),
        total_risk,
        total_risk_pct: (capital > 0.0).then(|| total_risk / capital * 100.0),
        with_stop,
        without_stop: positions.len() as i64 - with_stop,
        unrealized,
        marked,
        top_notional_share: by_ticker
            .iter()
            .map(|r| r.notional_share)
            .reduce(f64::max),
        top_risk_share: by_ticker.iter().filter_map(|r| r.risk_share).reduce(f64::max),
        herfindahl: h,
        effective_positions: h.filter(|v| *v > 0.0).map(|v| 1.0 / v),
        by_ticker,
        by_asset_class,
        by_side,
        by_strategy,
        correlation: None,
        warnings: Vec::new(),
        positions,
    };
    e.warnings = warnings(&e);
    e
}

fn insight(key: &str, severity: &str, values: &[(&str, f64)]) -> Insight {
    Insight {
        key: key.to_string(),
        severity: severity.to_string(),
        values: values.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
        labels: Vec::new(),
    }
}

/// What is worth saying about this book. Recomputed after the correlation block lands,
/// so the correlated statements can fire too.
pub fn warnings(e: &Exposure) -> Vec<Insight> {
    let mut out = Vec::new();
    if e.positions.is_empty() {
        return out;
    }

    // A position with no stop has no measurable risk, and that is the finding.
    if e.without_stop > 0 {
        out.push(insight(
            "noStop",
            if e.without_stop == e.open_count {
                "warn"
            } else {
                "info"
            },
            &[
                ("count", e.without_stop as f64),
                ("total", e.open_count as f64),
            ],
        ));
    }

    if e.positions.len() >= MIN_POSITIONS {
        if let (Some(top), Some(row)) = (e.top_risk_share, e.by_ticker.first()) {
            if top >= CONCENTRATED_PCT {
                out.push(insight(
                    "concentratedTicker",
                    "warn",
                    &[("share", top), ("risk", row.risk)],
                ));
            }
        }
        if let Some(row) = e.by_asset_class.first() {
            if row.notional_share >= 80.0 {
                out.push(insight(
                    "concentratedClass",
                    "info",
                    &[("share", row.notional_share)],
                ));
            }
        }
        if let Some(eff) = e.effective_positions {
            // Half the lines or fewer behaving as one book.
            if eff <= e.positions.len() as f64 / 2.0 {
                out.push(insight(
                    "thinBook",
                    "warn",
                    &[("effective", eff), ("positions", e.positions.len() as f64)],
                ));
            }
        }
    }

    if let Some(c) = &e.correlation {
        if let Some((a, b, r)) = &c.max_pair {
            if *r >= CLUSTER_CORR {
                // The pair names ride in the key's own values, so the sentence stays in
                // the language pack: two labels and one number.
                out.push(Insight {
                    key: "correlatedPair".into(),
                    severity: "warn".into(),
                    values: [("corr".to_string(), *r)].into_iter().collect(),
                    labels: vec![a.clone(), b.clone()],
                });
            }
        }
        if let (Some(stack), Some(corr), Some(indep)) =
            (c.stacking, c.risk_correlated, c.risk_independent)
        {
            if stack >= 1.3 {
                out.push(insight(
                    "stacked",
                    "warn",
                    &[("stacking", stack), ("correlated", corr), ("independent", indep)],
                ));
            }
        }
        if let Some(avg) = c.avg_corr {
            if avg <= 0.3 && c.labels.len() >= 3 {
                out.push(insight("diversified", "good", &[("avg", avg)]));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(ticker: &str, class: &str, side: &str, notional: f64, risk: Option<f64>) -> Position {
        Position {
            id: Uuid::new_v4(),
            ticker: ticker.into(),
            asset_class: class.into(),
            side: side.into(),
            currency: "EUR".into(),
            open_qty: 1.0,
            avg_entry: Some(100.0),
            stop_price: risk.map(|_| 95.0),
            leverage: 1.0,
            entry_at: None,
            days_open: None,
            notional,
            risk,
            risk_pct: risk.map(|r| r / 10_000.0 * 100.0),
            risk_share: None,
            last_price: None,
            last_at: None,
            unrealized: None,
            unrealized_pct: None,
            strategy_id: None,
            strategy: String::new(),
        }
    }

    fn build(positions: Vec<Position>) -> Exposure {
        let total_risk = positions.iter().filter_map(|p| p.risk).sum();
        seal(positions, 10_000.0, 0, "EUR", total_risk)
    }

    #[test]
    fn an_empty_book_says_nothing() {
        let e = build(vec![]);
        assert_eq!(e.open_count, 0);
        assert_eq!(e.gross_notional, 0.0);
        assert!(e.herfindahl.is_none());
        assert!(e.warnings.is_empty());
    }

    #[test]
    fn longs_and_shorts_gross_up_and_net_off() {
        let e = build(vec![
            pos("AAA", "stock", "long", 6000.0, Some(200.0)),
            pos("BBB", "stock", "short", 2000.0, Some(100.0)),
        ]);
        assert_eq!(e.gross_notional, 8000.0);
        assert_eq!(e.net_notional, 4000.0);
        assert_eq!(e.long_notional, 6000.0);
        assert_eq!(e.short_notional, 2000.0);
        assert_eq!(e.gross_pct, Some(80.0));
        assert_eq!(e.net_pct, Some(40.0));
        assert_eq!(e.total_risk, 300.0);
        assert_eq!(e.total_risk_pct, Some(3.0));
        assert_eq!(e.by_side.len(), 2);
    }

    #[test]
    fn an_even_book_has_as_many_effective_positions_as_lines() {
        let e = build(vec![
            pos("AAA", "stock", "long", 1000.0, Some(100.0)),
            pos("BBB", "stock", "long", 1000.0, Some(100.0)),
            pos("CCC", "stock", "long", 1000.0, Some(100.0)),
            pos("DDD", "stock", "long", 1000.0, Some(100.0)),
        ]);
        assert!((e.herfindahl.unwrap() - 0.25).abs() < 1e-9);
        assert!((e.effective_positions.unwrap() - 4.0).abs() < 1e-9);
        // Nothing to warn about: even risk, every stop logged.
        assert!(e.warnings.iter().all(|w| w.key != "thinBook"));
        assert!(e.warnings.iter().all(|w| w.key != "concentratedTicker"));
    }

    #[test]
    fn one_name_carrying_the_book_is_called_out() {
        let e = build(vec![
            pos("AAA", "stock", "long", 9000.0, Some(900.0)),
            pos("BBB", "stock", "long", 500.0, Some(50.0)),
            pos("CCC", "stock", "long", 500.0, Some(50.0)),
        ]);
        assert!(e.top_risk_share.unwrap() > 85.0);
        let keys: Vec<&str> = e.warnings.iter().map(|w| w.key.as_str()).collect();
        assert!(keys.contains(&"concentratedTicker"), "{keys:?}");
        assert!(keys.contains(&"thinBook"), "{keys:?}");
        // Three lines behaving as barely more than one.
        assert!(e.effective_positions.unwrap() < 1.5);
    }

    #[test]
    fn a_book_without_stops_measures_concentration_on_size() {
        let e = build(vec![
            pos("AAA", "crypto", "long", 8000.0, None),
            pos("BBB", "crypto", "long", 2000.0, None),
        ]);
        assert_eq!(e.total_risk, 0.0);
        assert!(e.total_risk_pct.is_some_and(|v| v == 0.0));
        assert_eq!(e.without_stop, 2);
        // Herfindahl fell back to notional: 0.8² + 0.2² = 0.68.
        assert!((e.herfindahl.unwrap() - 0.68).abs() < 1e-9);
        let keys: Vec<&str> = e.warnings.iter().map(|w| w.key.as_str()).collect();
        assert!(keys.contains(&"noStop"), "{keys:?}");
        assert_eq!(e.by_asset_class.len(), 1);
        assert_eq!(e.by_asset_class[0].notional_share, 100.0);
    }

    #[test]
    fn herfindahl_ignores_an_empty_book() {
        assert!(herfindahl(&[]).is_none());
        assert!(herfindahl(&[0.0, 0.0]).is_none());
        assert_eq!(herfindahl(&[100.0]), Some(1.0));
    }
}
