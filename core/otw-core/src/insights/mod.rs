//! Insights: predefined researchers over one list of closed trades.
//!
//! Three sources feed the same projection ([`TradeSet`]): the journal's closed trades (its
//! filters and category), a recorded backtest run (its trades recomputed the way the run
//! history replays them), and a paper session's round trips. A **researcher** reads the set
//! and returns the statements the numbers support, each with its evidence and, when one
//! exists, the what-if scenario of the journal lab ([`Scenario`]) that measures it.
//!
//! The money behind a statement is never computed twice. The set is mapped into the lab's
//! own [`LabTrade`] and the researchers run the lab's pure scenario functions
//! ([`journal_whatif::filter`], [`journal_whatif::fees`], [`journal_whatif::summarize`]),
//! so an insight's `delta_pnl` is the lab's `delta_net` for the same scenario, to the cent.
//!
//! Like the behaviour module, a statement fires only above the sample floor
//! ([`MIN_SAMPLE`]) **and** above an effect threshold ([`EFFECT_SHARE`]). The server sends
//! keys and numbers; the wording lives in the language packs (`insights.statement.<key>`).
//! Nothing is guessed: a sector is named only from an EDGAR SIC code the fundamentals
//! module stored, a fee only from one of the user's own fee schedules.

pub mod researchers;
pub mod sources;

use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

use otw_store::journal::FeeSchedule;

use crate::journal_whatif::{self, LabTrade, Scenario, Unmeasured};

pub use crate::journal_whatif::MIN_SAMPLE;

/// Share of the set's turnover (the sum of every trade's absolute result) a money effect
/// must reach before it is stated: below it, the difference is noise on this book.
pub const EFFECT_SHARE: f64 = 0.05;
/// Trades an hour or a weekday needs before it can be called the best or the worst one.
pub const MIN_BUCKET: usize = 5;
/// Trades each side of a comparison needs (the behaviour module's floor).
pub const MIN_SIDE: usize = otw_store::journal_behavior::MIN_SIDE as usize;

/// Where a set of trades comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Journal,
    Backtest,
    Paper,
}

/// One closed trade as the researchers read it: the lab's own projection, plus the two
/// facts some researchers need and the lab does not carry.
#[derive(Debug, Clone)]
pub struct SetTrade {
    pub lab: LabTrade,
    /// Entry notional in the set's money, when the size and the price are known.
    pub notional: Option<f64>,
    /// Planned stop distance, in percent of the entry, when a stop was set.
    pub stop_pct: Option<f64>,
}

/// The common projection of the three sources. Trades are closed and in exit order.
#[derive(Debug, Clone)]
pub struct TradeSet {
    pub kind: SourceKind,
    /// The money the results are in: the journal's display currency. `None` = the
    /// account currency of a simulation, which the run does not name.
    pub currency: Option<String>,
    pub trades: Vec<SetTrade>,
    /// Trades of the source left out of the set, with the reason.
    pub unmeasured: Vec<Unmeasured>,
}

impl TradeSet {
    pub fn lab(&self) -> Vec<LabTrade> {
        self.trades.iter().map(|t| t.lab.clone()).collect()
    }

    /// Sum of every trade's absolute result: the scale an effect is read against.
    pub fn turnover(&self) -> f64 {
        self.trades.iter().map(|t| t.lab.net.abs()).sum()
    }

    /// Whether a money effect clears [`EFFECT_SHARE`] of this set's turnover.
    pub fn material(&self, delta: f64) -> bool {
        let scale = self.turnover();
        scale > 0.0 && delta.is_finite() && delta.abs() >= EFFECT_SHARE * scale
    }

    /// `delta` as a share of the turnover, the order the page reads findings in.
    pub fn share(&self, delta: f64) -> f64 {
        let scale = self.turnover();
        if scale > 0.0 {
            delta.abs() / scale
        } else {
            0.0
        }
    }
}

/// What a researcher may read besides the trades. Loaded once per request.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The user's own fee schedules (empty when the journal is not installed).
    pub schedules: Vec<FeeSchedule>,
    /// Per schedule: one unit of the schedule's money in the set's money, per trade of the
    /// set (aligned with `TradeSet::trades`). `None` = it cannot be priced.
    pub schedule_rates: HashMap<Uuid, Vec<Option<f64>>>,
    /// Ticker (upper case) to its EDGAR SIC code and industry, as the fundamentals module
    /// stored them. A ticker missing here has no sector, and none is stated.
    pub sectors: HashMap<String, (String, String)>,
    /// Whether fee schedules can be added at all (the journal module is installed).
    pub journal_installed: bool,
}

/// The numbers behind a statement.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Evidence {
    /// Trades the comparison reads.
    pub n: usize,
    /// What the change would have done to the result, in the set's money.
    pub delta_pnl: Option<f64>,
    /// What it would have done to the max drawdown (negative = smaller).
    pub delta_dd: Option<f64>,
    /// Change of the risk taken per trade, in percent (negative = less risk).
    pub delta_risk: Option<f64>,
}

/// One statement with its evidence. `key` is `insights.statement.<key>` in the language
/// packs; `values` are raw numbers and `labels` names (`{0}`, `{1}`), as in the behaviour
/// module's insights.
#[derive(Debug, Clone, Serialize)]
pub struct Insight {
    pub key: String,
    pub researcher: &'static str,
    /// `warn` = a leak to fix, `good` = something done right, `info` = neutral.
    pub severity: &'static str,
    pub values: BTreeMap<String, f64>,
    pub labels: Vec<String>,
    pub evidence: Evidence,
    /// The lab scenario that measures it: "Open in lab" in the journal, "Add as filter" on
    /// a backtest (filters only).
    pub apply: Option<Scenario>,
    /// Effect size the page orders findings by, strongest first.
    pub strength: f64,
}

impl Insight {
    pub(crate) fn new(researcher: &'static str, key: &str, severity: &'static str) -> Insight {
        Insight {
            key: key.to_string(),
            researcher,
            severity,
            values: BTreeMap::new(),
            labels: Vec::new(),
            evidence: Evidence::default(),
            apply: None,
            strength: 0.0,
        }
    }

    pub(crate) fn value(mut self, k: &str, v: f64) -> Insight {
        self.values.insert(k.to_string(), v);
        self
    }

    pub(crate) fn label(mut self, l: impl Into<String>) -> Insight {
        self.labels.push(l.into());
        self
    }
}

pub type Researcher = fn(&TradeSet, &Context) -> Vec<Insight>;

/// Every researcher, in the order they are listed when the page asks for all of them.
pub const RESEARCHERS: &[(&str, Researcher)] = &[
    ("hours", researchers::hours),
    ("weekday", researchers::weekday),
    ("hold_time", researchers::hold_time),
    ("stop_geometry", researchers::stop_geometry),
    ("concentration", researchers::concentration),
    ("instrument_fees", researchers::instrument_fees),
    ("sizing_streaks", researchers::sizing_streaks),
    ("symmetry", researchers::symmetry),
];

pub fn names() -> Vec<&'static str> {
    RESEARCHERS.iter().map(|(n, _)| *n).collect()
}

pub fn known(name: &str) -> bool {
    RESEARCHERS.iter().any(|(n, _)| *n == name)
}

/// Run the researchers (all, or the ones named) and order the findings: those that move
/// money first, by the share of the turnover they move, then the others by effect size.
///
/// A scenario is carried only where the reader can act on it: any lab scenario on the
/// journal, only a filter on a backtest or a paper session (the strategy's trading window).
pub fn research(set: &TradeSet, ctx: &Context, only: Option<&[String]>) -> Vec<Insight> {
    let mut out: Vec<Insight> = RESEARCHERS
        .iter()
        .filter(|(name, _)| only.is_none_or(|o| o.iter().any(|x| x == name)))
        .flat_map(|(_, f)| f(set, ctx))
        .collect();
    for i in &mut out {
        let keep = match (&i.apply, set.kind) {
            (Some(_), SourceKind::Journal) => true,
            (Some(Scenario::Filter(f)), _) => f.signal.is_none() && f.window.is_some(),
            _ => false,
        };
        if !keep {
            i.apply = None;
        }
    }
    out.sort_by(|a, b| {
        let money = |i: &Insight| i.evidence.delta_pnl.is_some();
        money(b)
            .cmp(&money(a))
            .then(b.strength.total_cmp(&a.strength))
            .then(a.key.cmp(&b.key))
    });
    out
}

/// The lab's own sample check, shared by every researcher.
pub(crate) fn enough(set: &TradeSet) -> bool {
    set.trades.len() >= MIN_SAMPLE
}

/// Median of an unsorted slice.
pub(crate) fn median(v: &[f64]) -> Option<f64> {
    quantile(v, 0.5)
}

/// Linear-interpolated quantile, the lab's definition.
pub(crate) fn quantile(v: &[f64], q: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let pos = q * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(s[lo] + (s[hi] - s[lo]) * (pos - lo as f64))
}

/// Before and after summaries of the set when some trades are changed: `change(i)` gives
/// trade `i`'s new result (`None` = not taken). Uses the lab's `summarize`, so the curve,
/// the drawdown and the result read the same as the lab's.
pub(crate) fn compare(
    lab: &[LabTrade],
    change: impl Fn(usize, &LabTrade) -> Option<journal_whatif::Sim>,
) -> (journal_whatif::Summary, journal_whatif::Summary) {
    let real: Vec<Option<journal_whatif::Sim>> =
        lab.iter().map(|t| Some(journal_whatif::Sim { net: t.net, fees: t.fees })).collect();
    let sims: Vec<Option<journal_whatif::Sim>> = lab.iter().enumerate().map(|(i, t)| change(i, t)).collect();
    (journal_whatif::summarize(lab, &real), journal_whatif::summarize(lab, &sims))
}

#[cfg(test)]
mod tests;
