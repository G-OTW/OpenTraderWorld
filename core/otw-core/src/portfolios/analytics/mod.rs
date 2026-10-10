//! Portfolio analytics: a registry of independent analyses over a shared substrate.
//!
//! The rule this module exists for: **no analysis depends on another analysis.** Each one
//! declares what it needs, is handed it, and either answers or says why it cannot. Adding
//! the next measure is one file and one line in [`ANALYSES`], never a migration, an endpoint
//! or a refactor of what already ships.
//!
//! Three layers, and nothing skips one:
//!
//! ```text
//! Ledger      portfolio_operations, append-only, the only source of truth
//!    ↓ fold
//! Substrate   Book · Curve · Marks · Benchmark · Factors · Targets
//!    ↓ borrowed, never rebuilt per analysis
//! Analyzers   book · costs · performance · risk · benchmark · drift · stress
//! ```
//!
//! Two properties follow, and both are load-bearing:
//!
//! - **The substrate is built once per request.** The API takes the union of the requested
//!   analyzers' [`Needs`], builds exactly that, and passes references. N analyses cost one
//!   ledger walk and one bar read, not N. A request for `book` alone issues no bar query and
//!   no connector call, so a fresh install with nothing configured still renders instantly.
//! - **An analyzer never fails the request.** An unmet need is a [`Block`] with
//!   `status: "unavailable"` naming what is missing. A book with no bars, no benchmark and no
//!   history still returns every block that does not need them, which is why the analyses can
//!   ship in any order.

pub mod benchmark;
pub mod book;
pub mod costs;
pub mod drift;
pub mod performance;
pub mod risk;
pub mod stress;

mod curve;
pub mod substrate;
pub mod window;

pub use curve::Curve;
pub use substrate::Substrate;
pub use window::{Window, WINDOWS};

use serde::Serialize;
use serde_json::{json, Value};

// ── What an analysis needs ────────────────────────────────────────────────────

/// The substrates one analysis reads. A bitset rather than a list so the API can take the
/// union over a request in one fold.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Needs(u16);

impl Needs {
    /// Positions, cash, income and costs: one ledger walk, no market data.
    pub const BOOK: Needs = Needs(1 << 0);
    /// The daily deposit-adjusted curve, read from the stored snapshots.
    pub const CURVE: Needs = Needs(1 << 1);
    /// Daily closes of the held instruments.
    pub const MARKS: Needs = Needs(1 << 2);
    /// Daily closes of the portfolio's benchmark.
    pub const BENCHMARK: Needs = Needs(1 << 3);
    /// Factor proxy series and the betas measured against them.
    pub const FACTORS: Needs = Needs(1 << 4);
    /// The user's target allocation.
    pub const TARGETS: Needs = Needs(1 << 5);

    pub const fn has(self, other: Needs) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Needs {
    type Output = Needs;
    fn bitor(self, rhs: Needs) -> Needs {
        Needs(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Needs {
    fn bitor_assign(&mut self, rhs: Needs) {
        self.0 |= rhs.0;
    }
}

// ── What an analysis returns ──────────────────────────────────────────────────

/// How much evidence a block is standing on. Sent with every answer so the screen can say
/// "over 43 days" instead of implying a decade.
#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    /// Observations the measures were computed over.
    pub rows: usize,
    /// First and last day covered, ISO, when the block has a span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Share of the book the block could actually measure, 0..1. Below 1 the headline is
    /// partial, and saying so is not a caveat, it is the number's meaning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<f64>,
}

impl Sample {
    pub fn rows(n: usize) -> Self {
        Self { rows: n, from: None, to: None, coverage: None }
    }
    pub fn span(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self.to = Some(to.into());
        self
    }
    /// `+ 0.0` is not noise: Rust sums an empty `f64` iterator from `-0.0`, so a block that
    /// could measure none of the book serializes its coverage as `-0.0` and the screen prints
    /// "-0%".
    pub fn coverage(mut self, c: f64) -> Self {
        self.coverage = Some(c + 0.0);
        self
    }
}

/// One analysis' answer.
#[derive(Debug, Serialize)]
pub struct Block {
    /// `ok` when the block answered, `unavailable` when it could not and said why.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// Machine-readable reasons the block has no answer (`no_history`, `no_benchmark`,
    /// `no_bars`, `too_short`, `no_targets`). The language packs own the wording.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<Sample>,
}

impl Block {
    pub fn ok(data: Value) -> Self {
        Self { status: "ok", data: Some(data), missing: Vec::new(), sample: None }
    }
    pub fn with(mut self, sample: Sample) -> Self {
        self.sample = Some(sample);
        self
    }
    /// The honest answer when a need is unmet. Never a zero-filled table: a reader cannot
    /// tell a measured zero from a missing one, and both appear in this module.
    pub fn unavailable(missing: &[&'static str]) -> Self {
        Self { status: "unavailable", data: None, missing: missing.to_vec(), sample: None }
    }
}

// ── The registry ──────────────────────────────────────────────────────────────

/// One analysis.
///
/// `run` is **synchronous on purpose**: every read happens while the substrate is built, so
/// an analyzer is a pure function of what it was handed. That is what makes it testable on a
/// fixture with no database, and what stops one analysis from quietly issuing the query
/// another one already paid for.
pub trait Analysis: Send + Sync {
    fn key(&self) -> &'static str;
    fn needs(&self) -> Needs;
    fn run(&self, s: &Substrate<'_>) -> Block;
}

/// Every registered analysis, in the order the page shows them.
pub static ANALYSES: &[&dyn Analysis] = &[
    &book::Book,
    &costs::Costs,
    &performance::Performance,
    &risk::Risk,
    &benchmark::Benchmark,
    &drift::Drift,
    &stress::Stress,
];

/// The analyses a request asked for. An unknown key is ignored rather than refused: a client
/// from a newer build asking for a block this one does not have must still get its answer.
pub fn selected(keys: Option<&str>) -> Vec<&'static dyn Analysis> {
    match keys {
        None => ANALYSES.to_vec(),
        Some(list) => {
            let want: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
            ANALYSES.iter().copied().filter(|a| want.contains(&a.key())).collect()
        }
    }
}

/// The union of what the selected analyses need.
pub fn needs_of(selected: &[&'static dyn Analysis]) -> Needs {
    selected.iter().fold(Needs::default(), |acc, a| acc | a.needs())
}

/// Run each selected analysis over the prepared substrate.
pub fn run(selected: &[&'static dyn Analysis], s: &Substrate<'_>) -> Value {
    let mut blocks = serde_json::Map::new();
    for a in selected {
        blocks.insert(a.key().to_string(), json!(a.run(s)));
    }
    Value::Object(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_union_covers_every_member() {
        let n = Needs::BOOK | Needs::CURVE;
        assert!(n.has(Needs::BOOK));
        assert!(n.has(Needs::CURVE));
        assert!(!n.has(Needs::MARKS));
    }

    /// A request naming only cheap blocks must not drag the expensive substrates in behind
    /// it: that is the property the whole three-layer split exists to guarantee.
    #[test]
    fn a_book_only_request_needs_no_market_data() {
        let sel = selected(Some("book"));
        assert_eq!(sel.len(), 1);
        let n = needs_of(&sel);
        assert!(n.has(Needs::BOOK));
        assert!(!n.has(Needs::MARKS));
        assert!(!n.has(Needs::CURVE));
        assert!(!n.has(Needs::FACTORS));
    }

    #[test]
    fn an_unknown_block_is_ignored_not_refused() {
        assert_eq!(selected(Some("book,from_the_future")).len(), 1);
        assert_eq!(selected(Some("")).len(), 0);
        assert_eq!(selected(None).len(), ANALYSES.len());
    }

    #[test]
    fn every_key_is_unique() {
        let mut keys: Vec<&str> = ANALYSES.iter().map(|a| a.key()).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "two analyses share a key");
    }
}
