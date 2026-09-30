//! DCA mode: accumulate a basket over time instead of trading it.
//!
//! A DCA run is not a strategy that opens and closes positions. It is a **savings plan** with
//! rules. The user states four things and the engine does exactly that, with fees:
//!
//! 1. **A basket**: the selected assets with a fixed weight each (they never rebalance; a weight
//!    is how every euro deployed is split, not a target the engine chases).
//! 2. **The starting capital**: deployed in one shot at each asset's first bar, split by those
//!    weights. It is the money already there, so it is not a deposit.
//! 3. **Contributions**: an optional recurring amount (every N bars/days/weeks/months/quarters/
//!    years), always new money from outside the portfolio, invested on arrival or left as dry
//!    powder.
//! 4. **Tranches**: conditional buys that stack more on top, and sells that take some back out.
//!    A rule's condition is evaluated **on the basket** (a weighted, 100-based index of the
//!    selected assets) unless `per_asset` is set, in which case the same condition is evaluated
//!    on each asset's own bars and only the assets that satisfy it are bought. Either way the
//!    tranche is split by the fixed weights, so a rule that fires on one asset out of five
//!    deploys that asset's share and nothing else.
//!
//! Conditions are the ordinary signal groups of the rest of the engine (indicators, price,
//! custom indicators), which is what makes "buy when RSI < 30" and "buy 20% below the high"
//! the same kind of object. Two operand families exist for this mode: `Operand::Metric`
//! (drawdown from the running high, rise from the running low, change over N bars) and
//! `Operand::Position` (live state: unrealized PnL %, drift since the last buy, weight, cash).
//!
//! No-lookahead is the engine's usual rule: a condition that holds on bar *i* fills at bar
//! *i+1*'s open, through the same spread/slippage/fee model as every other mode.
//!
//! What comes back is measured the way a savings plan is measured, not the way a trading system
//! is: money in (contributions), cost basis, current value, realized and unrealized PnL, the
//! **time-weighted** return (which deposits cannot inflate) and the **money-weighted** return
//! (IRR, which is what the user actually earned), against a lump-sum-at-the-start benchmark of
//! the same total. Drawdown and Sharpe are computed on the deposit-adjusted (TWR) curve:
//! measured on raw portfolio value, every deposit would read as a rally.

use super::*;

// ── Configuration ────────────────────────────────────────────────────────────────────────

/// One asset's fixed share of every amount deployed.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DcaWeight {
    pub ticker: String,
    pub weight: f64,
}

/// Recurring contribution: new money paid into the plan every period, the way a savings plan
/// is funded.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DcaContribution {
    pub amount: f64,
    /// "bar" | "day" | "week" | "month" | "quarter" | "year"
    #[serde(default = "default_period")]
    pub period: String,
    /// Fire every N periods (1 = every one).
    #[serde(default = "default_every")]
    pub every: usize,
    /// Invest it on arrival by the fixed weights (true) or leave it as dry powder (false).
    #[serde(default = "yes")]
    pub invest: bool,
}
fn default_period() -> String {
    "month".into()
}
fn default_every() -> usize {
    1
}
fn yes() -> bool {
    true
}

/// A conditional buy tranche.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DcaBuy {
    /// Label shown on the fill and in the exit-reason breakdown.
    #[serde(default)]
    pub name: String,
    /// What `amount` means: "fixed" (account currency) | "pct_cash" | "pct_equity" |
    /// "pct_invested" (percent of the cost basis deployed so far).
    #[serde(default = "default_amount_kind")]
    pub amount_kind: String,
    pub amount: f64,
    /// Evaluate the condition on each asset's own bars (true) or on the basket index (false).
    #[serde(default)]
    pub per_asset: bool,
    #[serde(default)]
    pub condition: SignalGroup,
    /// Stop after this many fires (0 = unlimited). Counted per asset when `per_asset`.
    #[serde(default)]
    pub max_fires: usize,
    /// Bars to wait after a fire before this rule may fire again.
    #[serde(default)]
    pub cooldown_bars: usize,
}
fn default_amount_kind() -> String {
    "fixed".into()
}

/// A sell rule. Fires when **every** trigger it declares holds: the profit objective (when
/// `target_gain_pct > 0`) and the condition group (when it has conditions). A rule with
/// neither never fires.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DcaSell {
    #[serde(default)]
    pub name: String,
    /// What `amount` means: "pct_position" (percent of the units held) | "all" | "units" |
    /// "amount" (account currency; a basket rule splits it by the fixed weights, a per-asset
    /// rule applies it whole to the asset that fired).
    #[serde(default = "default_sell_kind")]
    pub amount_kind: String,
    #[serde(default)]
    pub amount: f64,
    /// Evaluate per asset (true) or on the basket / whole book (false).
    #[serde(default)]
    pub per_asset: bool,
    /// Profit objective: unrealized gain over the average cost, in percent (0 = no objective).
    #[serde(default)]
    pub target_gain_pct: f64,
    #[serde(default)]
    pub condition: SignalGroup,
    #[serde(default)]
    pub max_fires: usize,
    #[serde(default)]
    pub cooldown_bars: usize,
    /// Take the proceeds out of the portfolio instead of leaving them as cash.
    #[serde(default)]
    pub withdraw: bool,
}
fn default_sell_kind() -> String {
    "pct_position".into()
}

/// Full DCA plan. Required when `Settings.kind == "dca"`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct DcaConfig {
    /// Per-ticker share of every amount deployed. Missing or empty = equal weights.
    #[serde(default)]
    pub weights: Vec<DcaWeight>,
    #[serde(default)]
    pub contribution: Option<DcaContribution>,
    #[serde(default)]
    pub buys: Vec<DcaBuy>,
    #[serde(default)]
    pub sells: Vec<DcaSell>,
}

/// What a buy tranche's `amount` may mean. A value outside this list used to fall through to
/// "fixed" without a word, so it is checked instead of defaulted.
pub const BUY_KINDS: [&str; 4] = ["fixed", "pct_cash", "pct_equity", "pct_invested"];
/// What a sell rule's `amount` may mean (same reason).
pub const SELL_KINDS: [&str; 4] = ["pct_position", "all", "units", "amount"];
/// Derived price statistics an `Operand::Metric` may name (mirrors `metric_series`).
const METRIC_NAMES: [&str; 4] = ["dd_from_high", "up_from_low", "change_pct", "change_from_start"];

/// Operand names a rule may reference. A typo here used to compile into a series that is
/// undefined on every bar: the rule then ran for the whole backtest without a single fire and
/// nothing said so, which is worse than a refusal.
fn check_operand(o: &Operand, rule: &str) -> Option<String> {
    match o {
        Operand::Position { field } if pos_field_index(field).is_none() => Some(format!(
            "dca: {rule} reads an unknown portfolio field \"{field}\" ({})",
            POS_FIELDS.join("|")
        )),
        Operand::Metric { metric, .. } if !METRIC_NAMES.contains(&metric.as_str()) => {
            Some(format!(
                "dca: {rule} reads an unknown metric \"{metric}\" ({})",
                METRIC_NAMES.join("|")
            ))
        }
        _ => None,
    }
}

fn check_group(g: &SignalGroup, rule: &str) -> Option<String> {
    for c in &g.conditions {
        if let Some(err) = check_operand(&c.left, rule) {
            return Some(err);
        }
        if let Some(err) = c.right.as_ref().and_then(|r| check_operand(r, rule)) {
            return Some(err);
        }
    }
    None
}

/// Weight-table mistakes only the loaded datasets can reveal: a ticker the basket does not
/// contain, and an asset the table forgot. Both used to run as an equal-weight plan, or as a
/// zero-weight leg, without a word. Called by the API once the assets are known.
pub fn weight_warnings(cfg: &DcaConfig, tickers: &[&str]) -> Vec<String> {
    if cfg.weights.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let unknown: Vec<&str> = cfg
        .weights
        .iter()
        .filter(|w| !tickers.iter().any(|t| t.eq_ignore_ascii_case(&w.ticker)))
        .map(|w| w.ticker.as_str())
        .collect();
    let matched = cfg.weights.len() - unknown.len();
    if !unknown.is_empty() {
        out.push(format!(
            "dca: the weight table names {} the basket does not contain, {}",
            unknown.join(", "),
            if matched == 0 {
                "and nothing else matches, so every asset was given an equal share"
            } else {
                "so that share was never deployed"
            }
        ));
    }
    // When nothing matched, the engine fell back to equal weights and every asset IS bought:
    // repeating "never buys it" per asset would contradict the line above.
    if matched > 0 {
        for t in tickers {
            if !cfg.weights.iter().any(|w| w.ticker.eq_ignore_ascii_case(t)) {
                out.push(format!("dca: {t} has no weight, so the plan never buys it"));
            }
        }
    }
    out
}

impl DcaConfig {
    /// Normalized weight per input asset, in the order the assets were submitted. Unknown or
    /// unlisted tickers fall back to an equal share; an all-zero table is equal-weight.
    fn normalized(&self, tickers: &[&str]) -> Vec<f64> {
        let n = tickers.len().max(1);
        let mut w: Vec<f64> = if self.weights.is_empty() {
            vec![1.0; tickers.len()]
        } else {
            tickers
                .iter()
                .map(|t| {
                    self.weights
                        .iter()
                        .find(|w| w.ticker.eq_ignore_ascii_case(t))
                        .map(|w| w.weight.max(0.0))
                        .unwrap_or(0.0)
                })
                .collect()
        };
        // The same ticker selected twice (two timeframes, two providers) states one share of
        // the basket, not one each: split it between the inputs that carry it.
        for i in 0..tickers.len() {
            let dup = tickers.iter().filter(|t| t.eq_ignore_ascii_case(tickers[i])).count();
            if dup > 1 {
                w[i] /= dup as f64;
            }
        }
        let sum: f64 = w.iter().sum();
        if sum <= 0.0 {
            return vec![1.0 / n as f64; tickers.len()];
        }
        for x in w.iter_mut() {
            *x /= sum;
        }
        w
    }

    /// Structural errors, surfaced before a run.
    pub fn validate(&self) -> Option<String> {
        let named = |n: &str, w: &str| if n.trim().is_empty() { format!("unnamed {w}") } else { n.trim().to_string() };
        if self.weights.iter().any(|w| w.weight < 0.0) {
            return Some("dca: a weight cannot be negative".into());
        }
        // A table that is all zeros used to fall back to equal weights, which is the opposite
        // of what "hold none of these" says.
        if !self.weights.is_empty() && self.weights.iter().all(|w| w.weight <= 0.0) {
            return Some("dca: every weight is zero, so the plan has nothing to buy".into());
        }
        if let Some(c) = &self.contribution {
            if c.amount < 0.0 {
                return Some("dca: the contribution amount cannot be negative".into());
            }
            if !matches!(c.period.as_str(), "bar" | "day" | "week" | "month" | "quarter" | "year") {
                return Some(format!(
                    "dca: unknown contribution period \"{}\" (bar|day|week|month|quarter|year)",
                    c.period
                ));
            }
        }
        for b in &self.buys {
            let n = named(&b.name, "buy rule");
            if b.amount <= 0.0 {
                return Some(format!("dca: buy rule \"{n}\" has no amount"));
            }
            if !BUY_KINDS.contains(&b.amount_kind.as_str()) {
                return Some(format!(
                    "dca: buy rule \"{n}\" has an unknown amount kind \"{}\" ({})",
                    b.amount_kind,
                    BUY_KINDS.join("|")
                ));
            }
            if let Some(err) = check_group(&b.condition, &n) {
                return Some(err);
            }
        }
        for s in &self.sells {
            let n = named(&s.name, "sell rule");
            if s.amount_kind != "all" && s.amount <= 0.0 {
                return Some(format!("dca: sell rule \"{n}\" has no amount"));
            }
            if !SELL_KINDS.contains(&s.amount_kind.as_str()) {
                return Some(format!(
                    "dca: sell rule \"{n}\" has an unknown amount kind \"{}\" ({})",
                    s.amount_kind,
                    SELL_KINDS.join("|")
                ));
            }
            if s.target_gain_pct < 0.0 {
                return Some(format!(
                    "dca: sell rule \"{n}\" has a negative profit objective, which never holds"
                ));
            }
            if let Some(err) = check_group(&s.condition, &n) {
                return Some(err);
            }
        }
        None
    }

    /// Plans that run and measure nothing (or something else than intended).
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.contribution.is_none() && self.buys.is_empty() {
            out.push(
                "dca: no contribution and no buy rule, so the plan only buys the starting capital"
                    .into(),
            );
        }
        for b in &self.buys {
            if b.condition.conditions.is_empty() {
                out.push(format!(
                    "dca: buy rule \"{}\" has no condition and never fires",
                    if b.name.is_empty() { "unnamed" } else { &b.name }
                ));
            }
        }
        for b in &self.buys {
            if b.amount_kind == "pct_invested" && b.max_fires == 0 {
                out.push(format!(
                    "dca: buy rule \"{}\" deploys a share of the cost basis it keeps growing and \
                     has no fire cap, so each tranche is larger than the last",
                    if b.name.is_empty() { "unnamed" } else { &b.name }
                ));
            }
        }
        for s in &self.sells {
            if s.amount_kind == "units" && !s.per_asset {
                out.push(format!(
                    "dca: sell rule \"{}\" states units on a basket rule, so it sells that many \
                     units of every asset (an amount would be split by the weights instead)",
                    if s.name.is_empty() { "unnamed" } else { &s.name }
                ));
            }
            if s.condition.conditions.is_empty() && s.target_gain_pct <= 0.0 {
                out.push(format!(
                    "dca: sell rule \"{}\" has neither an objective nor a condition, so it never fires",
                    if s.name.is_empty() { "unnamed" } else { &s.name }
                ));
            }
        }
        out
    }
}

// ── Result ───────────────────────────────────────────────────────────────────────────────

/// One asset's standing at the end of a DCA run.
#[derive(Debug, Serialize, Default)]
pub struct DcaAsset {
    pub ticker: String,
    /// Units still held.
    pub units: f64,
    /// Weighted-average cost of those units (gross of fees).
    pub avg_cost: f64,
    pub last_price: f64,
    /// Mark-to-market value of the units at the last known price.
    pub value: f64,
    /// Share of the portfolio's holdings this asset is worth today, in percent.
    pub weight_pct: f64,
    /// Share the plan aimed to deploy into it, in percent.
    pub target_weight_pct: f64,
    /// Gross cost of every buy over the run (not netted by sells).
    pub invested: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub fees: f64,
    pub buys: usize,
    pub sells: usize,
}

/// One fill of the plan. The DCA equivalent of a trade list: every euro that moved, why, and
/// what the position looked like afterwards.
#[derive(Debug, Serialize)]
pub struct DcaEvent {
    pub ts: String,
    pub ticker: String,
    /// "buy" | "sell"
    pub action: String,
    /// What ordered it: "initial" | "contribution" | the rule's name (or "buy N" / "sell N").
    pub source: String,
    pub price: f64,
    pub qty: f64,
    /// Cash moved, fee included (negative for a buy, positive for a sell).
    pub amount: f64,
    pub fee: f64,
    pub units_after: f64,
    pub avg_cost_after: f64,
    pub cash_after: f64,
}

/// DCA-specific result block.
#[derive(Debug, Serialize, Default)]
pub struct DcaStats {
    /// Every euro the user put in: starting capital + external deposits.
    pub contributed: f64,
    /// Of which came from outside the account during the run.
    pub deposits: f64,
    pub withdrawn: f64,
    /// Gross cost of every buy (cost basis deployed over the run).
    pub invested: f64,
    /// Gross proceeds of every sell.
    pub proceeds: f64,
    pub fees: f64,
    pub buys: usize,
    pub sells: usize,
    pub cash: f64,
    pub holdings_value: f64,
    /// cash + holdings at the last bar.
    pub final_value: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    /// (final_value − contributed) / contributed, in percent. What the money in has earned.
    pub total_return_pct: f64,
    /// Time-weighted return over the whole window, in percent (deposit-proof).
    pub twr_pct: f64,
    /// Money-weighted (internal rate of) return, annualized, in percent. None when the cash
    /// flows have no sign change to solve over.
    pub irr_pct: Option<f64>,
    /// Same total contributed, all deployed at the start by the same weights.
    pub lump_sum_value: f64,
    pub lump_sum_return_pct: f64,
    /// Tranches that could not be funded in full and bought what the cash allowed.
    pub underfunded: usize,
    /// How many fills the run made. `events` carries the last `EVENT_CAP` of them.
    pub events_total: usize,
    pub assets: Vec<DcaAsset>,
    /// Money-in over time (contributed), to draw against the value curve.
    pub contributed_curve: Vec<EquityPoint>,
    /// Cost basis of the open holdings over time.
    pub cost_curve: Vec<EquityPoint>,
    pub events: Vec<DcaEvent>,
}

/// How many fills travel back with the result. A per-bar contribution over a long dataset
/// produced one event per bar (20 000 rows, 6 MB of JSON, one chart mark each), so the list is
/// the tail of the plan and `events_total` says how long the real one is.
pub const EVENT_CAP: usize = 2000;

// ── Condition plumbing ───────────────────────────────────────────────────────────────────

/// Live portfolio fields an `Operand::Position` can read, in evaluation order.
const POS_FIELDS: [&str; 8] = [
    "pnl_pct",
    "since_last_buy_pct",
    "avg_cost",
    "units",
    "value",
    "weight_pct",
    "cash_pct",
    "drawdown_pct",
];

fn pos_field_index(field: &str) -> Option<usize> {
    POS_FIELDS.iter().position(|f| *f == field)
}

/// One resolved operand: either a series over the target's own bars, or a live portfolio field.
enum Src {
    Series(Vec<Option<f64>>),
    Pos(usize),
    /// Unknown position field: never defined, so the signal never holds.
    Void,
}

struct Cond {
    left: Src,
    right: Option<Src>,
    op: Op,
}

fn resolve_src(op: &Operand, b: &Bars, defs: &IndicatorDefs) -> Src {
    match op {
        Operand::Position { field } => match pos_field_index(field) {
            Some(i) => Src::Pos(i),
            None => Src::Void,
        },
        other => Src::Series(resolve(other, b, defs)),
    }
}

impl Src {
    /// Value at `bar` of the target's own series (or the live field), plus the previous one.
    fn at(&self, bar: usize, now: &[f64; 8], prev: &Option<[f64; 8]>) -> (Option<f64>, Option<f64>) {
        match self {
            Src::Series(v) => (
                v.get(bar).copied().flatten(),
                bar.checked_sub(1).and_then(|i| v.get(i).copied().flatten()),
            ),
            Src::Pos(i) => (Some(now[*i]), prev.map(|p| p[*i])),
            Src::Void => (None, None),
        }
    }
}

/// One rule's conditions compiled against one target (an asset, or the basket index).
struct Compiled {
    conds: Vec<Cond>,
    any: bool,
}

impl Compiled {
    fn build(g: &SignalGroup, b: &Bars, defs: &IndicatorDefs) -> Self {
        Compiled {
            conds: g
                .conditions
                .iter()
                .map(|c| Cond {
                    left: resolve_src(&c.left, b, defs),
                    right: c.right.as_ref().map(|r| resolve_src(r, b, defs)),
                    op: c.op,
                })
                .collect(),
            any: g.logic == "any",
        }
    }

    /// Does the group hold at `bar` of the target, given the live state?
    fn holds(&self, b: &Bars, bar: usize, now: &[f64; 8], prev: &Option<[f64; 8]>) -> bool {
        if self.conds.is_empty() {
            return false;
        }
        let one = |c: &Cond| {
            let (l, lp) = c.left.at(bar, now, prev);
            let (r, rp) = match &c.right {
                Some(s) => s.at(bar, now, prev),
                None => (None, None),
            };
            match c.op {
                Op::Above => matches!((l, r), (Some(l), Some(r)) if l > r),
                Op::Below => matches!((l, r), (Some(l), Some(r)) if l < r),
                Op::CrossesAbove => {
                    matches!((lp, rp, l, r), (Some(lp), Some(rp), Some(l), Some(r)) if lp <= rp && l > r)
                }
                Op::CrossesBelow => {
                    matches!((lp, rp, l, r), (Some(lp), Some(rp), Some(l), Some(r)) if lp >= rp && l < r)
                }
                Op::Cross => matches!(
                    (lp, rp, l, r),
                    (Some(lp), Some(rp), Some(l), Some(r))
                        if (lp - rp).signum() != (l - r).signum() && (lp - rp) != 0.0
                ),
                Op::Rising => matches!((lp, l), (Some(lp), Some(l)) if l > lp),
                Op::Falling => matches!((lp, l), (Some(lp), Some(l)) if l < lp),
                Op::ClosingAbove => matches!(r, Some(r) if b.close[bar] > r),
                Op::ClosingBelow => matches!(r, Some(r) if b.close[bar] < r),
                Op::OpeningAbove => matches!(r, Some(r) if b.open[bar] > r),
                Op::OpeningBelow => matches!(r, Some(r) if b.open[bar] < r),
            }
        };
        if self.any {
            self.conds.iter().any(one)
        } else {
            self.conds.iter().all(one)
        }
    }
}

// ── Basket index ─────────────────────────────────────────────────────────────────────────

/// Weighted, 100-based index of the basket over the merged clock, so a "condition on the
/// basket" is a condition on a real series: every indicator, metric and operator works on it
/// unchanged. Each asset enters at 100 on its first bar and carries its last known price on
/// rows where it has none, so an asset that starts later dilutes the index rather than
/// jolting it.
fn basket_index(inputs: &[&Bars], maps: &[Vec<Option<usize>>], clock: &[String], w: &[f64]) -> OwnedBars {
    let m = clock.len();
    let mut open = vec![0.0; m];
    let mut high = vec![0.0; m];
    let mut low = vec![0.0; m];
    let mut close = vec![0.0; m];
    let mut volume = vec![0.0; m];
    let mut base = vec![0.0f64; inputs.len()];
    let mut last = vec![1.0f64; inputs.len()];
    for r in 0..m {
        for (ai, b) in inputs.iter().enumerate() {
            let wi = w.get(ai).copied().unwrap_or(0.0);
            if wi <= 0.0 {
                continue;
            }
            let (o, h, l, c, v) = match maps[ai][r] {
                Some(bar) => {
                    if base[ai] <= 0.0 {
                        base[ai] = b.close[bar];
                    }
                    (b.open[bar], b.high[bar], b.low[bar], b.close[bar], b.volume[bar])
                }
                // Not trading on this row: held flat at its last known price.
                None => {
                    let p = if base[ai] > 0.0 { last[ai] * base[ai] } else { 0.0 };
                    (p, p, p, p, 0.0)
                }
            };
            let bs = if base[ai] > 0.0 { base[ai] } else { 0.0 };
            let norm = |px: f64| if bs > 0.0 { px / bs } else { 1.0 };
            if bs > 0.0 {
                last[ai] = norm(c);
            }
            open[r] += wi * norm(o) * 100.0;
            high[r] += wi * norm(h) * 100.0;
            low[r] += wi * norm(l) * 100.0;
            close[r] += wi * norm(c) * 100.0;
            volume[r] += v;
        }
    }
    OwnedBars {
        ticker: "BASKET".into(),
        timeframe: String::new(),
        ts: clock.to_vec(),
        open,
        high,
        low,
        close,
        volume,
    }
}

// ── Calendar ─────────────────────────────────────────────────────────────────────────────

/// The contribution period a timestamp belongs to, as a **consecutive ordinal**: a change of
/// value opens a new period, and the distance between two values is the number of periods
/// between them. That is what makes `every N` count calendar periods; a key that was only
/// tested for equality made it count the periods the data happened to carry, so a file with
/// bars in January, March and May paid January and May and skipped March.
///
/// Row index backs "bar"; everything else is read off the RFC3339 date, so no timezone
/// assumption is made beyond the data's own. An unparseable timestamp falls back to the row.
fn period_ordinal(period: &str, ts: &str, row: usize) -> i64 {
    if period == "bar" {
        return row as i64;
    }
    use time::format_description::well_known::Rfc3339;
    let Ok(d) = time::OffsetDateTime::parse(ts, &Rfc3339) else { return row as i64 };
    let date = d.date();
    let (y, m) = (date.year() as i64, date.month() as u8 as i64);
    match period {
        "day" => date.to_julian_day() as i64,
        // Julian day 2440592 (Monday 5 January 1970) is a multiple of 7, so a plain floor
        // division lands every ISO week on its own ordinal.
        "week" => (date.to_julian_day() as i64).div_euclid(7),
        "quarter" => y * 4 + (m - 1) / 3,
        "year" => y,
        // "month" and anything unknown.
        _ => y * 12 + (m - 1),
    }
}

// ── Money-weighted return ────────────────────────────────────────────────────────────────

/// Annualized IRR of the run's own cash flows (negative = money in, positive = money out),
/// solved by bisection on the NPV. None when the flows never change sign (nothing to solve).
fn irr(flows: &[(f64, f64)]) -> Option<f64> {
    if flows.len() < 2 {
        return None;
    }
    let has_in = flows.iter().any(|(_, a)| *a < 0.0);
    let has_out = flows.iter().any(|(_, a)| *a > 0.0);
    if !has_in || !has_out {
        return None;
    }
    let npv = |r: f64| -> f64 {
        flows.iter().map(|(t, a)| a / (1.0 + r).powf(*t)).sum::<f64>()
    };
    let (mut lo, mut hi) = (-0.9999, 10.0);
    let (mut flo, fhi) = (npv(lo), npv(hi));
    if flo.signum() == fhi.signum() {
        return None;
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let f = npv(mid);
        if f.abs() < 1e-9 {
            return Some(mid);
        }
        if f.signum() == flo.signum() {
            lo = mid;
            flo = f;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// Years elapsed between two RFC3339 timestamps (0 when either is unparseable).
fn years_between(a: &str, b: &str) -> f64 {
    use time::format_description::well_known::Rfc3339;
    match (
        time::OffsetDateTime::parse(a, &Rfc3339),
        time::OffsetDateTime::parse(b, &Rfc3339),
    ) {
        (Ok(x), Ok(y)) => (y - x).whole_seconds() as f64 / (365.25 * 24.0 * 3600.0),
        _ => 0.0,
    }
}


// ── State ────────────────────────────────────────────────────────────────────────────────

/// One asset's accumulated position. `cost` is gross of fees; `open_fees` carries the entry
/// fees still attached to the open units, so a partial sell hands back its own share of them
/// and the trade list reconciles with the equity curve.
#[derive(Default)]
struct Holding {
    /// Contract multiplier, so `cost` (a notional) can be read back as a price.
    mult: f64,
    units: f64,
    cost: f64,
    open_fees: f64,
    /// Buys in the current leg (reset once the position is fully sold).
    leg_buys: usize,
    leg_start_ts: String,
    leg_start_row: usize,
    /// Worst / best unrealized PnL reached during the current leg, in account currency.
    leg_mae: f64,
    leg_mfe: f64,
    last_fill_px: f64,
    invested: f64,
    realized: f64,
    fees: f64,
    buys: usize,
    sells: usize,
    rows_held: usize,
    last_close: f64,
}

impl Holding {
    /// Weighted-average **price** of the open units. `cost` is a notional (quantity x price x
    /// multiplier), so the multiplier has to come back out: without it every x10 instrument
    /// reported an average cost ten times its fill price, and every rule reading `pnl_pct`
    /// saw a position 90% under water on a flat market.
    fn avg_cost(&self) -> f64 {
        if self.units > 0.0 && self.mult > 0.0 {
            self.cost / (self.units * self.mult)
        } else {
            0.0
        }
    }
    fn value(&self, mult: f64) -> f64 {
        self.units * self.last_close * mult
    }
}

/// The whole mutable state of a run. Fills are methods on it so cash, holdings, the trade list
/// and the cash-flow ledger can never drift apart.
struct Sim<'a> {
    s: &'a Settings,
    inputs: &'a [&'a Bars<'a>],
    maps: &'a [Vec<Option<usize>>],
    clock: &'a [String],
    tickers: Vec<String>,
    mult: f64,
    fill: FillModel<'a>,
    hold: Vec<Holding>,
    cash: f64,
    /// Every euro the user put in: starting capital + deposits made during the run.
    contributed: f64,
    deposits: f64,
    withdrawn: f64,
    invested_total: f64,
    proceeds_total: f64,
    fees_total: f64,
    underfunded: usize,
    skipped_min_size: usize,
    trades: Vec<Trade>,
    events: Vec<DcaEvent>,
    /// Net external flow on the row being processed (deposits − withdrawals).
    row_flow: f64,
}

impl Sim<'_> {
    fn holdings_value(&self) -> f64 {
        self.hold.iter().map(|h| h.value(self.mult)).sum()
    }
    fn equity(&self) -> f64 {
        self.cash + self.holdings_value()
    }
    fn cost_basis(&self) -> f64 {
        self.hold.iter().map(|h| h.cost).sum()
    }

    /// Add new money from outside the portfolio.
    fn deposit(&mut self, amount: f64) {
        if amount <= 0.0 {
            return;
        }
        self.cash += amount;
        self.deposits += amount;
        self.contributed += amount;
        self.row_flow += amount;
    }

    /// Deploy `amount` of cash into asset `ai` at row `r`'s open. `external` tops the account
    /// up with new money when the cash is short instead of trimming the tranche.
    /// Returns true when something was actually bought.
    ///
    /// New money is paid in **after** the order is known to fill, never before: a tranche the
    /// lot step or the minimum size refuses used to book its deposit anyway, so the run
    /// reported money in that was never paid and holdings that were never bought.
    fn buy(&mut self, ai: usize, r: usize, amount: f64, external: bool, source: &str) -> bool {
        let Some(bar) = self.maps[ai][r] else { return false };
        if amount <= 0.0 {
            return false;
        }
        let px = self.fill.entry(self.inputs[ai].open[bar], true);
        if !px.is_finite() || px <= 0.0 {
            return false;
        }
        let mut spend = amount;
        // An ordinary buy is capped by the cash on hand; a conditional tranche pays the rest
        // in as new money once it knows what the order actually costs.
        let trimmed = !external && spend > self.cash;
        if trimmed {
            spend = self.cash;
        }
        if spend <= 0.0 {
            if trimmed {
                self.underfunded += 1;
            }
            return false;
        }
        // Fees are charged on top of the notional, so the notional shrinks until the two fit
        // inside the amount. Converges in one step for every fee model the engine has.
        let mut qty = spend / (px * self.mult);
        for _ in 0..4 {
            let fee = fee_for(&self.s.fees, qty, px, self.mult);
            let total = qty * px * self.mult + fee;
            if total <= spend || total <= 0.0 {
                break;
            }
            qty *= spend / total;
        }
        let Some(qty) = self.s.instrument.round_qty(qty) else {
            self.skipped_min_size += 1;
            if trimmed {
                self.underfunded += 1;
            }
            return false;
        };
        let fee = fee_for(&self.s.fees, qty, px, self.mult);
        let notional = qty * px * self.mult;
        if notional + fee > self.cash + 1e-9 {
            if !external {
                self.underfunded += 1;
                return false;
            }
            let top = notional + fee - self.cash;
            self.deposit(top);
        }
        if trimmed {
            self.underfunded += 1;
        }
        self.cash -= notional + fee;
        self.invested_total += notional;
        self.fees_total += fee;
        let ts = self.clock[r].clone();
        let h = &mut self.hold[ai];
        if h.units <= 0.0 {
            h.leg_start_ts = ts.clone();
            h.leg_start_row = r;
            h.leg_buys = 0;
            h.leg_mae = 0.0;
            h.leg_mfe = 0.0;
        }
        h.units += qty;
        h.cost += notional;
        h.open_fees += fee;
        h.leg_buys += 1;
        h.buys += 1;
        h.invested += notional;
        h.fees += fee;
        h.last_fill_px = px;
        let (units_after, avg_after) = (h.units, h.avg_cost());
        self.events.push(DcaEvent {
            ts,
            ticker: self.tickers[ai].clone(),
            action: "buy".into(),
            source: source.to_string(),
            price: px,
            qty,
            amount: -(notional + fee),
            fee,
            units_after,
            avg_cost_after: avg_after,
            cash_after: self.cash,
        });
        true
    }

    /// Sell asset `ai` at row `r`'s open. `withdraw` takes the proceeds out of the portfolio
    /// instead of leaving them as cash. Emits one closed trade for the sold units.
    ///
    /// A money-sized rule states an amount, not a quantity, and it is converted **at the
    /// price the order fills at**: converting at the row's close was lookahead, and a bar that
    /// opened at 100 and closed at 200 sold half of what the rule asked for.
    fn sell(&mut self, ai: usize, r: usize, size: SellSize, withdraw: bool, source: &str) -> bool {
        let Some(bar) = self.maps[ai][r] else { return false };
        let held = self.hold[ai].units;
        if held <= 0.0 {
            return false;
        }
        let px = self.fill.exit(self.inputs[ai].open[bar], true);
        if !px.is_finite() || px <= 0.0 {
            return false;
        }
        let units = match size {
            SellSize::Units(u) => u,
            SellSize::Amount(a) => a / (px * self.mult),
        };
        if units <= 0.0 {
            return false;
        }
        // A sell must always be able to close the position out, whatever the lot step says:
        // rounding is what the *instrument* allows on a fresh order, not a reason to strand
        // the last fraction of an existing one.
        let qty = if units >= held * (1.0 - 1e-9) {
            held
        } else {
            match self.s.instrument.round_qty(units.min(held)) {
                Some(q) => q.min(held),
                None => return false,
            }
        };
        let fee = fee_for(&self.s.fees, qty, px, self.mult);
        let gross = qty * px * self.mult;
        let avg = self.hold[ai].avg_cost();
        let share = qty / held;
        let entry_fee = self.hold[ai].open_fees * share;
        let pnl = (px - avg) * qty * self.mult - fee - entry_fee;
        let ts = self.clock[r].clone();
        let (leg_ts, leg_row, leg_buys, leg_mae, leg_mfe) = {
            let h = &self.hold[ai];
            (h.leg_start_ts.clone(), h.leg_start_row, h.leg_buys.max(1), h.leg_mae, h.leg_mfe)
        };
        if withdraw {
            self.withdrawn += gross - fee;
            self.row_flow -= gross - fee;
        } else {
            self.cash += gross - fee;
        }
        self.proceeds_total += gross;
        self.fees_total += fee;
        let notional = avg * qty * self.mult;
        self.trades.push(Trade {
            ticker: self.tickers[ai].clone(),
            entry_ts: if leg_ts.is_empty() { ts.clone() } else { leg_ts },
            exit_ts: ts.clone(),
            entry_price: avg,
            exit_price: px,
            qty,
            entries: leg_buys,
            direction: "long".into(),
            exit_reason: source.to_string(),
            pnl,
            fees: fee + entry_fee,
            return_pct: if notional != 0.0 { pnl / notional * 100.0 } else { 0.0 },
            bars_held: r.saturating_sub(leg_row),
            mae: leg_mae * share,
            mfe: leg_mfe * share,
        });
        let cash_after = self.cash;
        let h = &mut self.hold[ai];
        h.units -= qty;
        h.cost -= avg * qty * self.mult;
        h.open_fees -= entry_fee;
        // The excursions belong to the units still open, so the part just sold leaves with
        // its share instead of being reported again by the next partial sell.
        h.leg_mae *= 1.0 - share;
        h.leg_mfe *= 1.0 - share;
        h.realized += pnl;
        h.fees += fee;
        h.sells += 1;
        if h.units <= 1e-12 {
            h.units = 0.0;
            h.cost = 0.0;
            h.open_fees = 0.0;
            h.leg_buys = 0;
            h.leg_start_ts.clear();
            h.leg_mae = 0.0;
            h.leg_mfe = 0.0;
            // The leg is gone, so `since_last_buy_pct` must not keep quoting its fill price.
            h.last_fill_px = 0.0;
        }
        let (units_after, avg_after) = (h.units, h.avg_cost());
        self.events.push(DcaEvent {
            ts,
            ticker: self.tickers[ai].clone(),
            action: "sell".into(),
            source: source.to_string(),
            price: px,
            qty,
            amount: gross - fee,
            fee,
            units_after,
            avg_cost_after: avg_after,
            cash_after,
        });
        true
    }
}

/// How much a sell rule takes: a quantity, or an amount of money the fill price converts.
enum SellSize {
    Units(f64),
    Amount(f64),
}

/// Rule label for the fill list and the exit-reason breakdown.
fn label(name: &str, prefix: &str, i: usize) -> String {
    if name.trim().is_empty() {
        format!("{prefix} {}", i + 1)
    } else {
        name.trim().to_string()
    }
}

// ── The run ──────────────────────────────────────────────────────────────────────────────

/// Simulate the plan: one shared cash balance, one fixed weight table, long-only accumulation.
pub fn run_dca(s: &Settings, inputs: &[&Bars]) -> RunResult {
    let cfg = s.dca.clone().unwrap_or_default();
    let mult = if s.instrument.multiplier > 0.0 { s.instrument.multiplier } else { 1.0 };
    let (clock, maps) = merged_clock(inputs);
    let m = clock.len();
    let na = inputs.len();
    let tickers: Vec<&str> = inputs.iter().map(|b| b.ticker).collect();
    let w = cfg.normalized(&tickers);

    // Every rule is compiled against the targets it can be evaluated on: each asset's own bars
    // when `per_asset`, the basket index otherwise. Index `na` is the basket.
    let basket = basket_index(inputs, &maps, &clock, &w);
    let basket_bars = basket.as_bars();
    let compile = |g: &SignalGroup, per_asset: bool| -> Vec<Option<Compiled>> {
        let mut v: Vec<Option<Compiled>> = Vec::with_capacity(na + 1);
        if g.conditions.is_empty() {
            return (0..=na).map(|_| None).collect();
        }
        for b in inputs.iter() {
            v.push(per_asset.then(|| Compiled::build(g, b, &s.indicators)));
        }
        v.push((!per_asset).then(|| Compiled::build(g, &basket_bars, &s.indicators)));
        v
    };
    let buy_conds: Vec<Vec<Option<Compiled>>> =
        cfg.buys.iter().map(|r| compile(&r.condition, r.per_asset)).collect();
    let sell_conds: Vec<Vec<Option<Compiled>>> =
        cfg.sells.iter().map(|r| compile(&r.condition, r.per_asset)).collect();
    let buy_names: Vec<String> =
        cfg.buys.iter().enumerate().map(|(i, r)| label(&r.name, "buy", i)).collect();
    let sell_names: Vec<String> =
        cfg.sells.iter().enumerate().map(|(i, r)| label(&r.name, "sell", i)).collect();

    let mut sim = Sim {
        s,
        inputs,
        maps: &maps,
        clock: &clock,
        tickers: tickers.iter().map(|t| t.to_string()).collect(),
        mult,
        fill: FillModel { half_spread: s.spread_pct / 2.0, slippage: &s.slippage },
        hold: (0..na).map(|_| Holding { mult, ..Default::default() }).collect(),
        cash: s.starting_capital,
        contributed: s.starting_capital,
        deposits: 0.0,
        withdrawn: 0.0,
        invested_total: 0.0,
        proceeds_total: 0.0,
        fees_total: 0.0,
        underfunded: 0,
        skipped_min_size: 0,
        trades: Vec::new(),
        events: Vec::new(),
        row_flow: 0.0,
    };

    let mut equity: Vec<EquityPoint> = Vec::with_capacity(m);
    let mut twr: Vec<EquityPoint> = Vec::with_capacity(m);
    let mut contributed_curve: Vec<EquityPoint> = Vec::with_capacity(m);
    let mut cost_curve: Vec<EquityPoint> = Vec::with_capacity(m);
    // Cash flows for the IRR: (years from the start, amount), money in negative.
    let mut flows: Vec<(f64, f64)> = Vec::new();

    // Fire counters and cooldown marks per rule per target (index `na` = the basket).
    let mut buy_fires = vec![vec![0usize; na + 1]; cfg.buys.len()];
    let mut sell_fires = vec![vec![0usize; na + 1]; cfg.sells.len()];
    let mut buy_last = vec![vec![usize::MAX; na + 1]; cfg.buys.len()];
    let mut sell_last = vec![vec![usize::MAX; na + 1]; cfg.sells.len()];
    // Cooldown is counted from the row the rule *fired on*, not from the row its order
    // filled on: stamping the fill added the no-lookahead bar to every gap.
    let mut buy_gate = vec![vec![usize::MAX; na + 1]; cfg.buys.len()];
    let mut sell_gate = vec![vec![usize::MAX; na + 1]; cfg.sells.len()];

    // Live portfolio state per target, and the previous row's copy (crosses need both).
    let mut pos_now: Vec<[f64; 8]> = vec![[0.0; 8]; na + 1];
    let mut pos_prev: Vec<Option<[f64; 8]>> = vec![None; na + 1];

    // Decisions taken on row r-1, filled at row r's open (no lookahead).
    let mut pending: Vec<(bool, usize, usize)> = Vec::new(); // (is_buy, rule, asset)
    let mut deployed = vec![false; na];
    // The contribution calendar: the period last seen, the first one (so `every` counts
    // calendar periods and not periods that happen to carry bars), and what the plan owes
    // but could not pay yet because the window was closed.
    let mut period_seen: Option<i64> = None;
    let mut period_anchor: Option<i64> = None;
    let mut contrib_owed = 0.0f64;
    // Per asset: the share of a contribution its asset had no bar for, waiting for its first
    // one instead of sitting in cash for the rest of the run.
    let mut owed_share = vec![0.0f64; na];
    // Basket index level at the last buy, backing the basket's `since_last_buy_pct`.
    let mut basket_last_buy = 0.0f64;
    let mut events_seen = 0usize;

    let gate = clock_gate(&s.filters, &clock);
    let mut filtered_bars = 0usize;
    let mut peak_twr = 0.0f64;
    let mut max_dd = 0.0f64;
    let mut max_dd_abs = 0.0f64;
    let mut dd_now = 0.0f64;
    // Cumulative external flow, so the currency drawdown is measured on the money the plan
    // made and not on the money the user added.
    let mut cum_flow = 0.0f64;
    let mut peak_adj = s.starting_capital;
    let base_index = if s.starting_capital > 0.0 { s.starting_capital } else { 100.0 };
    let mut twr_index = base_index;
    let mut prev_equity = s.starting_capital;

    if s.starting_capital > 0.0 {
        flows.push((0.0, -s.starting_capital));
    }

    for r in 0..m {
        let window_open = gate.as_ref().is_none_or(|g| g[r]);
        if !window_open {
            filtered_bars += 1;
        }
        sim.row_flow = 0.0;
        // The book as this row opens, marked at the row's own opens before a single euro moves:
        // the pivot the time-weighted return is chained on (see block 4). An asset with no bar
        // here keeps its last close, the same stand-in the excursions use.
        let eq_open = sim.cash
            + (0..na)
                .map(|ai| {
                    let h = &sim.hold[ai];
                    let px = match maps[ai][r] {
                        Some(bar) => inputs[ai].open[bar],
                        None => h.last_close,
                    };
                    h.units * px * mult
                })
                .sum::<f64>();

        // ── (1) The recurring contribution lands first, so this row's tranches can spend it ──
        if let Some(c) = &cfg.contribution {
            if c.amount > 0.0 {
                let ord = period_ordinal(&c.period, &clock[r], r);
                if period_seen != Some(ord) {
                    let anchor = *period_anchor.get_or_insert(ord);
                    period_seen = Some(ord);
                    let every = (c.every.max(1)) as i64;
                    if (ord - anchor).rem_euclid(every) == 0 {
                        contrib_owed += c.amount;
                    }
                }
                // A contribution whose period opens on a bar the filters close waits for the
                // next open bar instead of being lost: a weekly plan under a Tue-Fri filter
                // used to pay one contribution out of thirteen.
                if contrib_owed > 0.0 && window_open {
                    let due = std::mem::take(&mut contrib_owed);
                    sim.deposit(due);
                    if c.invest {
                        // The deposit landed first, so the cash covers the tranche.
                        for (ai, wi) in w.iter().enumerate().take(na) {
                            let share = due * wi;
                            if share > 0.0 && !sim.buy(ai, r, share, false, "contribution") && maps[ai][r].is_none() {
                                owed_share[ai] += share;
                            }
                        }
                    }
                }
            }
        }

        // An asset that had not listed yet gets its share as soon as it does, instead of
        // leaving that money as cash for the rest of the run.
        for ai in 0..na {
            if owed_share[ai] > 0.0 && maps[ai][r].is_some() && window_open {
                let due = std::mem::take(&mut owed_share[ai]);
                if !sim.buy(ai, r, due, false, "contribution") {
                    owed_share[ai] = due;
                }
            }
        }

        // ── (2) The starting capital, deployed in one shot at each asset's first tradable
        // bar the window allows. It is the cash already in the account, never a deposit. ──
        if s.starting_capital > 0.0 && window_open {
            for ai in 0..na {
                if deployed[ai] || maps[ai][r].is_none() || w[ai] <= 0.0 {
                    continue;
                }
                deployed[ai] = true;
                sim.buy(ai, r, s.starting_capital * w[ai], false, "initial");
            }
        }

        // ── (3) Fills ordered on the previous row ──
        let due = std::mem::take(&mut pending);
        // A tranche states ONE amount, however many legs it fills. Resolving it inside the leg
        // loop let each leg read a book the previous one had already moved: a "100% of cash"
        // tranche over three assets deployed 1000, then 667, then 444.
        let mut tranche: Vec<Option<f64>> = vec![None; cfg.buys.len()];
        for (is_buy, ri, _) in due.iter() {
            if *is_buy && tranche[*ri].is_none() {
                let rule = &cfg.buys[*ri];
                tranche[*ri] = Some(match rule.amount_kind.as_str() {
                    "pct_cash" => sim.cash * rule.amount / 100.0,
                    "pct_equity" => sim.equity() * rule.amount / 100.0,
                    "pct_invested" => sim.invested_total * rule.amount / 100.0,
                    _ => rule.amount,
                });
            }
        }
        for (is_buy, ri, ai) in due {
            if is_buy {
                let rule = &cfg.buys[ri];
                let amount = tranche[ri].unwrap_or(rule.amount);
                // Fixed weights: a tranche always deploys the asset's share of it, whether the
                // condition fired on the basket or on that one asset.
                let share = amount * w[ai];
                // A conditional tranche is money the user pays in when it fires: cash first,
                // the rest as new money.
                if share > 0.0 && sim.buy(ai, r, share, true, &buy_names[ri]) {
                    let t = if rule.per_asset { ai } else { na };
                    // One fire, however many legs it filled: a basket rule orders one tranche,
                    // and `max_fires` counts tranches, not the assets they touched.
                    if buy_last[ri][t] != r {
                        buy_fires[ri][t] += 1;
                        buy_last[ri][t] = r;
                    }
                }
            } else {
                let rule = &cfg.sells[ri];
                let held = sim.hold[ai].units;
                if held <= 0.0 {
                    continue;
                }
                let size = match rule.amount_kind.as_str() {
                    "all" => SellSize::Units(held),
                    "units" => SellSize::Units(rule.amount),
                    // An amount is money, and a basket rule states one amount for the basket:
                    // it is split by the fixed weights exactly like a buy tranche. A per-asset
                    // rule states it for the asset that fired, so it applies whole.
                    "amount" => SellSize::Amount(if rule.per_asset {
                        rule.amount
                    } else {
                        rule.amount * w[ai]
                    }),
                    // "pct_position"
                    _ => SellSize::Units(held * rule.amount / 100.0),
                };
                if sim.sell(ai, r, size, rule.withdraw, &sell_names[ri]) {
                    let t = if rule.per_asset { ai } else { na };
                    if sell_last[ri][t] != r {
                        sell_fires[ri][t] += 1;
                        sell_last[ri][t] = r;
                    }
                }
            }
        }

        // Mark every asset to this row's close, after the fills: an order placed on row r-1
        // fills at row r's OPEN, so nothing it is sized on may read row r's close.
        for ai in 0..na {
            if let Some(bar) = maps[ai][r] {
                sim.hold[ai].last_close = inputs[ai].close[bar];
            } else if sim.hold[ai].last_close == 0.0 {
                sim.hold[ai].last_close = inputs[ai].close.first().copied().unwrap_or(0.0);
            }
        }

        if sim.events.len() != events_seen {
            if sim.events[events_seen..].iter().any(|e| e.action == "buy") {
                basket_last_buy = basket.close.get(r).copied().unwrap_or(basket_last_buy);
            }
            events_seen = sim.events.len();
        }

        // ── (4) Mark to market, curves, drawdown ──
        let holdings = sim.holdings_value();
        let eq = sim.cash + holdings;
        // Leg excursions, tracked on the bar's own extremes when the asset traded.
        for ai in 0..na {
            let h = &mut sim.hold[ai];
            if h.units <= 0.0 {
                continue;
            }
            h.rows_held += 1;
            let avg = h.cost / h.units;
            let (lo, hi) = match maps[ai][r] {
                Some(bar) => (inputs[ai].low[bar], inputs[ai].high[bar]),
                None => (h.last_close, h.last_close),
            };
            h.leg_mae = h.leg_mae.min((lo - avg) * h.units * mult);
            h.leg_mfe = h.leg_mfe.max((hi - avg) * h.units * mult);
        }
        // Time-weighted return, chained over the two halves the row's money actually splits it
        // into: close(r-1) → open(r) belongs to the capital already invested, open(r) → close(r)
        // to everything present once this row's contribution has landed and filled. Neutralising
        // the flow against the closing value instead (`(eq - flow) / prev_equity`) credited the
        // new money's own intraday gain to the capital that was already there: a plan whose
        // monthly payment is worth as much as it holds, which is every savings plan in its first
        // months, read a +10% day as +15%, and those inflated rows set the peak every drawdown
        // is then measured from.
        if prev_equity > 0.0 {
            twr_index *= eq_open / prev_equity;
        }
        // The flow is present for the second half: it belongs in the denominator, not subtracted
        // from the numerator. A withdrawal that empties the account leaves nothing invested.
        let invested = eq_open + sim.row_flow;
        if invested > 0.0 {
            twr_index *= eq / invested;
        } else if twr_index <= 0.0 {
            twr_index = base_index;
        }
        prev_equity = eq;
        peak_twr = peak_twr.max(twr_index);
        if peak_twr > 0.0 {
            dd_now = (peak_twr - twr_index) / peak_twr * 100.0;
            max_dd = max_dd.max(dd_now);
        }
        // Currency drawdown, measured on equity net of everything paid in since the start.
        cum_flow += sim.row_flow;
        let adj = eq - cum_flow;
        peak_adj = peak_adj.max(adj);
        max_dd_abs = max_dd_abs.max(peak_adj - adj);
        if sim.row_flow != 0.0 {
            flows.push((years_between(&clock[0], &clock[r]), -sim.row_flow));
        }
        equity.push(EquityPoint { ts: clock[r].clone(), equity: eq });
        twr.push(EquityPoint { ts: clock[r].clone(), equity: twr_index });
        contributed_curve.push(EquityPoint { ts: clock[r].clone(), equity: sim.contributed });
        cost_curve.push(EquityPoint { ts: clock[r].clone(), equity: sim.cost_basis() });

        // ── (5) Refresh the live state each rule reads, then evaluate this row's conditions ──
        let total = if eq != 0.0 { eq } else { 1.0 };
        for (ai, slot) in pos_now.iter_mut().enumerate().take(na) {
            let h = &sim.hold[ai];
            let value = h.value(mult);
            let avg = h.avg_cost();
            *slot = [
                if avg > 0.0 { (h.last_close / avg - 1.0) * 100.0 } else { 0.0 },
                if h.last_fill_px > 0.0 { (h.last_close / h.last_fill_px - 1.0) * 100.0 } else { 0.0 },
                avg,
                h.units,
                value,
                value / total * 100.0,
                sim.cash / total * 100.0,
                dd_now,
            ];
        }
        {
            let cost = sim.cost_basis();
            let units: f64 = sim.hold.iter().map(|h| h.units).sum();
            let bk = basket.close.get(r).copied().unwrap_or(0.0);
            pos_now[na] = [
                if cost > 0.0 { (holdings - cost) / cost * 100.0 } else { 0.0 },
                if basket_last_buy > 0.0 && bk > 0.0 { (bk / basket_last_buy - 1.0) * 100.0 } else { 0.0 },
                if units > 0.0 { cost / units } else { 0.0 },
                units,
                holdings,
                holdings / total * 100.0,
                sim.cash / total * 100.0,
                dd_now,
            ];
        }

        if window_open {
            for (ri, rule) in cfg.buys.iter().enumerate() {
                let targets: Vec<usize> = if rule.per_asset { (0..na).collect() } else { vec![na] };
                for t in targets {
                    if rule.max_fires > 0 && buy_fires[ri][t] >= rule.max_fires {
                        continue;
                    }
                    if buy_gate[ri][t] != usize::MAX
                        && r < buy_gate[ri][t].saturating_add(rule.cooldown_bars)
                    {
                        continue;
                    }
                    let Some(c) = buy_conds[ri][t].as_ref() else { continue };
                    let (bars, bar) =
                        if t == na { (&basket_bars, Some(r)) } else { (inputs[t], maps[t][r]) };
                    let Some(bar) = bar else { continue };
                    if !c.holds(bars, bar, &pos_now[t], &pos_prev[t]) {
                        continue;
                    }
                    buy_gate[ri][t] = r;
                    if rule.per_asset {
                        pending.push((true, ri, t));
                    } else {
                        for (ai, wi) in w.iter().enumerate().take(na) {
                            if *wi > 0.0 {
                                pending.push((true, ri, ai));
                            }
                        }
                    }
                }
            }
        }
        for (ri, rule) in cfg.sells.iter().enumerate() {
            let targets: Vec<usize> = if rule.per_asset { (0..na).collect() } else { vec![na] };
            for t in targets {
                if rule.max_fires > 0 && sell_fires[ri][t] >= rule.max_fires {
                    continue;
                }
                if sell_gate[ri][t] != usize::MAX
                    && r < sell_gate[ri][t].saturating_add(rule.cooldown_bars)
                {
                    continue;
                }
                // The objective is a trigger of its own: gain over the average cost.
                if rule.target_gain_pct > 0.0 && pos_now[t][0] < rule.target_gain_pct {
                    continue;
                }
                if !rule.condition.conditions.is_empty() {
                    let Some(c) = sell_conds[ri][t].as_ref() else { continue };
                    let (bars, bar) =
                        if t == na { (&basket_bars, Some(r)) } else { (inputs[t], maps[t][r]) };
                    let Some(bar) = bar else { continue };
                    if !c.holds(bars, bar, &pos_now[t], &pos_prev[t]) {
                        continue;
                    }
                } else if rule.target_gain_pct <= 0.0 {
                    continue;
                }
                sell_gate[ri][t] = r;
                if rule.per_asset {
                    if sim.hold[t].units > 0.0 {
                        pending.push((false, ri, t));
                    }
                } else {
                    for ai in 0..na {
                        if sim.hold[ai].units > 0.0 {
                            pending.push((false, ri, ai));
                        }
                    }
                }
            }
        }
        for t in 0..=na {
            pos_prev[t] = Some(pos_now[t]);
        }
    }

    build_result(s, &cfg, inputs, &clock, &maps, &w, sim, equity, twr, contributed_curve, cost_curve, flows, max_dd, max_dd_abs, filtered_bars)
}

/// Same total money, all of it deployed at the start by the same weights: the line a savings
/// plan is honestly compared against. Each asset's slice is bought at its first bar (one fee)
/// and held; slices whose asset has not started yet sit in cash.
fn lump_sum_curve(
    s: &Settings,
    inputs: &[&Bars],
    maps: &[Vec<Option<usize>>],
    clock: &[String],
    w: &[f64],
    total: f64,
    mult: f64,
) -> Vec<EquityPoint> {
    let fill = FillModel { half_spread: s.spread_pct / 2.0, slippage: &s.slippage };
    let na = inputs.len();
    let mut qty = vec![0.0f64; na];
    let mut fee_paid = vec![0.0f64; na];
    let mut bought = vec![false; na];
    let mut last = vec![0.0f64; na];
    clock
        .iter()
        .enumerate()
        .map(|(r, ts)| {
            let mut eq = 0.0;
            for ai in 0..na {
                let slice = total * w.get(ai).copied().unwrap_or(0.0);
                if let Some(bar) = maps[ai][r] {
                    last[ai] = inputs[ai].close[bar];
                    if !bought[ai] {
                        let px = fill.entry(inputs[ai].open[bar], true);
                        if px > 0.0 && slice > 0.0 {
                            // The fee comes out of the slice, so the units bought are what is
                            // left after paying it, the same trim a real order takes. The fee
                            // is then re-read on the trimmed quantity, which is the only one
                            // that is actually ordered (it differs under a per-unit model).
                            qty[ai] = slice / (px * mult);
                            for _ in 0..3 {
                                fee_paid[ai] = fee_for(&s.fees, qty[ai], px, mult);
                                qty[ai] = ((slice - fee_paid[ai]).max(0.0)) / (px * mult);
                            }
                        }
                        bought[ai] = true;
                    }
                }
                eq += if bought[ai] { qty[ai] * last[ai] * mult } else { slice };
            }
            EquityPoint { ts: ts.clone(), equity: eq }
        })
        .collect()
}

/// Assemble the result: trade-derived stats over the sells, portfolio measures over the
/// deposit-adjusted curve, and the DCA block the UI reads.
#[allow(clippy::too_many_arguments)]
fn build_result(
    s: &Settings,
    cfg: &DcaConfig,
    inputs: &[&Bars],
    clock: &[String],
    maps: &[Vec<Option<usize>>],
    w: &[f64],
    mut sim: Sim,
    equity: Vec<EquityPoint>,
    twr: Vec<EquityPoint>,
    contributed_curve: Vec<EquityPoint>,
    cost_curve: Vec<EquityPoint>,
    mut flows: Vec<(f64, f64)>,
    max_dd: f64,
    max_dd_abs: f64,
    filtered_bars: usize,
) -> RunResult {
    let mult = sim.mult;
    let na = inputs.len();
    let m = clock.len();
    let holdings = sim.holdings_value();
    let final_value = sim.cash + holdings;
    let realized: f64 = sim.hold.iter().map(|h| h.realized).sum();
    let unrealized: f64 = sim
        .hold
        .iter()
        .map(|h| (h.last_close - h.avg_cost()) * h.units * mult - h.open_fees)
        .sum();

    // IRR: everything the user put in, closed at the last bar by what the plan is worth then.
    // Withdrawals are NOT added here: each one already entered the series as a positive flow
    // on its own date, and counting them twice roughly doubled the reported rate.
    if let Some(last) = clock.last() {
        flows.push((years_between(&clock[0], last), final_value));
    }
    let irr_pct = irr(&flows).map(|r| r * 100.0);

    let contributed = sim.contributed;
    let lump = lump_sum_curve(s, inputs, maps, clock, w, contributed, mult);
    let lump_value = lump.last().map(|p| p.equity).unwrap_or(0.0);

    let assets: Vec<DcaAsset> = (0..na)
        .map(|ai| {
            let h = &sim.hold[ai];
            let value = h.value(mult);
            DcaAsset {
                ticker: sim.tickers[ai].clone(),
                units: h.units,
                avg_cost: h.avg_cost(),
                last_price: h.last_close,
                value,
                weight_pct: if holdings > 0.0 { value / holdings * 100.0 } else { 0.0 },
                target_weight_pct: w.get(ai).copied().unwrap_or(0.0) * 100.0,
                invested: h.invested,
                realized_pnl: h.realized,
                unrealized_pnl: (h.last_close - h.avg_cost()) * h.units * mult - h.open_fees,
                fees: h.fees,
                buys: h.buys,
                sells: h.sells,
            }
        })
        .collect();

    let per_asset: Vec<AssetStats> = (0..na)
        .map(|ai| {
            let h = &sim.hold[ai];
            let trs: Vec<&Trade> = sim.trades.iter().filter(|t| t.ticker == sim.tickers[ai]).collect();
            let st = side_stats(trs.into_iter());
            AssetStats {
                ticker: sim.tickers[ai].clone(),
                trades: st.trades,
                wins: st.wins,
                win_rate: st.win_rate,
                net_pnl: st.net_pnl,
                total_fees: h.fees,
                exposure_pct: if m > 0 { h.rows_held as f64 / m as f64 * 100.0 } else { 0.0 },
                bars: inputs[ai].ts.len(),
                inactive_bars: maps[ai].iter().filter(|x| x.is_none()).count(),
            }
        })
        .collect();

    let all = side_stats(sim.trades.iter());
    let mut exit_reasons: BTreeMap<String, usize> = BTreeMap::new();
    for t in &sim.trades {
        *exit_reasons.entry(t.exit_reason.clone()).or_insert(0) += 1;
    }
    // Risk ratios read the deposit-adjusted curve: on raw value, every contribution would
    // count as a return and the Sharpe would measure the savings rate, not the plan.
    let (sharpe, sortino) = risk_ratios(&twr);
    let twr_pct = match (twr.first(), twr.last()) {
        (Some(a), Some(z)) if a.equity > 0.0 => (z.equity / a.equity - 1.0) * 100.0,
        _ => 0.0,
    };
    let net_pnl = final_value + sim.withdrawn - contributed;
    let stats = Stats {
        engine_version: ENGINE_VERSION,
        trades: all.trades,
        wins: all.wins,
        losses: all.losses,
        win_rate: all.win_rate,
        net_pnl,
        return_pct: if contributed > 0.0 { net_pnl / contributed * 100.0 } else { 0.0 },
        total_fees: sim.fees_total,
        max_drawdown_pct: max_dd,
        max_drawdown: max_dd_abs,
        profit_factor: all.profit_factor,
        avg_trade: all.avg_trade,
        final_equity: final_value,
        buy_hold_return_pct: if contributed > 0.0 {
            (lump_value - contributed) / contributed * 100.0
        } else {
            0.0
        },
        sharpe,
        sortino,
        expectancy_pct: all.expectancy_pct,
        exit_reasons,
        all,
        long: side_stats(sim.trades.iter()),
        short: SideStats::default(),
    };

    let events_total = sim.events.len();
    let mut events = std::mem::take(&mut sim.events);
    if events_total > EVENT_CAP {
        events.drain(..events_total - EVENT_CAP);
    }

    let dca = DcaStats {
        contributed,
        deposits: sim.deposits,
        withdrawn: sim.withdrawn,
        invested: sim.invested_total,
        proceeds: sim.proceeds_total,
        fees: sim.fees_total,
        buys: sim.hold.iter().map(|h| h.buys).sum(),
        sells: sim.hold.iter().map(|h| h.sells).sum(),
        cash: sim.cash,
        holdings_value: holdings,
        final_value,
        realized_pnl: realized,
        unrealized_pnl: unrealized,
        total_return_pct: if contributed > 0.0 {
            (final_value + sim.withdrawn - contributed) / contributed * 100.0
        } else {
            0.0
        },
        twr_pct,
        irr_pct,
        lump_sum_value: lump_value,
        lump_sum_return_pct: if contributed > 0.0 {
            (lump_value - contributed) / contributed * 100.0
        } else {
            0.0
        },
        underfunded: sim.underfunded,
        events_total,
        assets,
        contributed_curve,
        cost_curve,
        events,
    };

    let warmup = dca_warmup(cfg, &s.indicators);
    // A savings plan starts on its first fill, not on the warmup of an indicator only one of
    // its rules reads: the starting capital and the contributions never wait for it.
    let start_ts = dca
        .events
        .first()
        .map(|e| e.ts.clone())
        .or_else(|| clock.get(warmup).or_else(|| clock.first()).cloned());
    RunResult {
        trades: sim.trades,
        equity,
        stats,
        per_asset,
        warmup_bars: warmup,
        trading_start_ts: start_ts,
        alignment: (na > 1).then(|| align(s, inputs)),
        skipped_min_size: sim.skipped_min_size,
        skipped_margin: 0,
        skipped_exposure: 0,
        halted_bars: 0,
        filtered_bars,
        oos: None,
        benchmark: lump,
        total_funding: 0.0,
        grid: None,
        dca: Some(dca),
    }
}

/// Largest indicator lookback the plan's conditions reference.
pub(crate) fn dca_warmup(cfg: &DcaConfig, defs: &IndicatorDefs) -> usize {
    let mut w = 0;
    for g in cfg.buys.iter().map(|b| &b.condition).chain(cfg.sells.iter().map(|s| &s.condition)) {
        w = w.max(group_lookback(g, defs));
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Daily bars from a close series (flat OHLC: open == close, so a fill at the next open is
    /// the next close and every expectation stays arithmetic).
    fn series(n: usize, f: impl Fn(usize) -> f64) -> (Vec<String>, Vec<f64>) {
        let ts: Vec<String> = (0..n)
            .map(|i| format!("2024-{:02}-{:02}T00:00:00Z", 1 + i / 28, 1 + i % 28))
            .collect();
        (ts, (0..n).map(f).collect())
    }

    fn owned(ticker: &str, ts: &[String], px: &[f64]) -> OwnedBars {
        OwnedBars {
            ticker: ticker.into(),
            timeframe: "1d".into(),
            ts: ts.to_vec(),
            open: px.to_vec(),
            high: px.to_vec(),
            low: px.to_vec(),
            close: px.to_vec(),
            volume: vec![1.0; px.len()],
        }
    }

    fn settings(cfg: DcaConfig, capital: f64) -> Settings {
        let mut s: Settings = serde_json::from_value(serde_json::json!({
            "kind": "dca",
            "mode": "long",
            "sizing": { "mode": "fixed_qty", "qty": 1.0 },
            "starting_capital": capital,
            "fees": { "amount_kind": "pct", "per": "trade", "amount": 0.0 }
        }))
        .expect("settings");
        s.dca = Some(cfg);
        s
    }

    fn buy_rule(amount: f64, per_asset: bool, conds: Vec<Signal>) -> DcaBuy {
        DcaBuy {
            name: String::new(),
            amount_kind: "fixed".into(),
            amount,
            per_asset,
            condition: SignalGroup { logic: "all".into(), conditions: conds },
            max_fires: 0,
            cooldown_bars: 0,
        }
    }

    /// A condition that always holds, to fire a tranche on the first bar.
    fn always() -> Signal {
        Signal {
            left: Operand::Const { value: 1.0 },
            op: Op::Above,
            right: Some(Operand::Const { value: 0.0 }),
        }
    }

    fn metric(m: &str, op: Op, v: f64) -> Signal {
        Signal {
            left: Operand::Metric { metric: m.into(), period: 0 },
            op,
            right: Some(Operand::Const { value: v }),
        }
    }

    /// The starting capital is deployed in one shot, split by the fixed weights, at each
    /// asset's first bar.
    #[test]
    fn starting_capital_splits_by_weight() {
        let (ts, a) = series(10, |_| 100.0);
        let (_, b) = series(10, |_| 50.0);
        let (ba, bb) = (owned("A", &ts, &a), owned("B", &ts, &b));
        let cfg = DcaConfig {
            weights: vec![
                DcaWeight { ticker: "A".into(), weight: 70.0 },
                DcaWeight { ticker: "B".into(), weight: 30.0 },
            ],
            ..Default::default()
        };
        let s = settings(cfg, 1_000.0);
        let r = run_dca(&s, &[&ba.as_bars(), &bb.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.buys, 2, "one fill per asset");
        assert!((d.assets[0].units - 7.0).abs() < 1e-9, "700 / 100 = 7 units, got {}", d.assets[0].units);
        assert!((d.assets[1].units - 6.0).abs() < 1e-9, "300 / 50 = 6 units, got {}", d.assets[1].units);
        assert!((d.invested - 1000.0).abs() < 1e-9);
        assert!(d.cash.abs() < 1e-9, "the capital is fully deployed, got {}", d.cash);
    }

    /// A monthly contribution deposits new money and invests it: money in grows, the return on
    /// a flat market stays 0 (a deposit is not a gain).
    #[test]
    fn monthly_contribution_is_not_a_return() {
        let (ts, px) = series(84, |_| 100.0); // 3 months of 28 days
        let ba = owned("A", &ts, &px);
        let cfg = DcaConfig {
            contribution: Some(DcaContribution {
                amount: 100.0,
                period: "month".into(),
                every: 1,
                invest: true,
            }),
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.buys, 3, "one buy per month");
        assert!((d.deposits - 300.0).abs() < 1e-9);
        assert!((d.contributed - 300.0).abs() < 1e-9);
        assert!((d.final_value - 300.0).abs() < 1e-6, "flat market keeps the money in");
        assert!(d.total_return_pct.abs() < 1e-6);
        assert!(d.twr_pct.abs() < 1e-6, "twr must ignore deposits, got {}", d.twr_pct);
    }

    /// The time-weighted return of a contribution row is the market's move, not the market's
    /// move levered by the money that arrived that morning.
    ///
    /// A real plan: 1 000 in the account, 500 a month, one asset.
    ///   month 1 (flat at 100): 1 000 buys 10 units, the 500 contribution buys 5 more. 15 units.
    ///   month 2, the market opens at 100 and closes at 110 (+10%): the 500 buys 5 units at the
    ///     open, so the day ends on 20 units worth 2 200.
    ///   month 3, it opens at 110 and closes at 99 (-10%): the 500 buys 4.5454 units at the open,
    ///     so the day ends on 24.5454 units worth 2 430.
    ///
    /// The plan earned exactly what the market did on both days: +10% then -10%, so a TWR of
    /// -1.00% and a drawdown of 10.00%. Subtracting the contribution from the closing value
    /// instead read the +10% day as +13.33% (the new money's own 50 credited to the 1 000 that
    /// was already invested) and the -10% day as -12.27%, for a TWR of -0.58% and a drawdown of
    /// 12.27%: two numbers no market in the file ever printed.
    #[test]
    fn twr_of_a_contribution_row_is_the_market_move() {
        let n = 84; // 3 months of 28 days
        let (ts, _) = series(n, |_| 0.0);
        let mut open: Vec<f64> = vec![100.0; n];
        let mut close: Vec<f64> = vec![100.0; n];
        for i in 29..56 {
            open[i] = 110.0;
            close[i] = 110.0;
        }
        // The contribution days: the market moves between the open it buys at and the close.
        open[28] = 100.0;
        close[28] = 110.0;
        open[56] = 110.0;
        close[56] = 99.0;
        for i in 57..n {
            open[i] = 99.0;
            close[i] = 99.0;
        }
        let ba = OwnedBars {
            ticker: "A".into(),
            timeframe: "1d".into(),
            ts,
            high: open.iter().zip(&close).map(|(o, c)| o.max(*c)).collect(),
            low: open.iter().zip(&close).map(|(o, c)| o.min(*c)).collect(),
            open,
            close,
            volume: vec![1.0; n],
        };
        let cfg = DcaConfig {
            contribution: Some(DcaContribution {
                amount: 500.0,
                period: "month".into(),
                every: 1,
                invest: true,
            }),
            ..Default::default()
        };
        let s = settings(cfg, 1000.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.as_ref().unwrap();

        // The book, so the scenario is the one the doc comment describes.
        assert!((d.contributed - 2500.0).abs() < 1e-9, "1000 + 3 x 500, got {}", d.contributed);
        assert!((d.assets[0].units - 24.545454545454547).abs() < 1e-9, "got {}", d.assets[0].units);
        assert!((d.final_value - 2430.0).abs() < 1e-6, "24.5454 units at 99, got {}", d.final_value);

        // The measures. TWR is 1.10 x 0.90 - 1, and the drawdown is the fall from that 1.10 peak.
        assert!((d.twr_pct + 1.0).abs() < 1e-6, "twr must be -1.00%, got {}", d.twr_pct);
        assert!(
            (r.stats.max_drawdown_pct - 10.0).abs() < 1e-6,
            "drawdown must be 10.00%, got {}",
            r.stats.max_drawdown_pct
        );
        // Money-weighted is a different question and must stay different: -70 on 2 500 in.
        assert!(
            (d.total_return_pct + 2.8).abs() < 1e-6,
            "money-weighted return must be -2.80%, got {}",
            d.total_return_pct
        );
    }

    /// A basket condition buys every asset; the tranche is still split by the fixed weights.
    #[test]
    fn basket_condition_buys_the_whole_basket() {
        // A falls 30%, B is flat: the basket index is down ~15%.
        let (ts, a) = series(20, |i| if i < 5 { 100.0 } else { 70.0 });
        let (_, b) = series(20, |_| 100.0);
        let (ba, bb) = (owned("A", &ts, &a), owned("B", &ts, &b));
        let cfg = DcaConfig {
            buys: vec![DcaBuy {
                max_fires: 1,
                ..buy_rule(1000.0, false, vec![metric("dd_from_high", Op::Above, 10.0)])
            }],
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let r = run_dca(&s, &[&ba.as_bars(), &bb.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.buys, 2, "both legs of the basket are bought");
        assert!((d.invested - 1000.0).abs() < 1e-6, "the tranche is 1000, split 50/50");
        assert!((d.assets[0].invested - 500.0).abs() < 1e-6);
        assert!((d.assets[1].invested - 500.0).abs() < 1e-6);
    }

    /// The same condition per asset only buys the asset that satisfies it, for that asset's
    /// share of the tranche: fixed weights, not a redistribution.
    #[test]
    fn per_asset_condition_buys_only_the_firing_asset() {
        let (ts, a) = series(20, |i| if i < 5 { 100.0 } else { 70.0 });
        let (_, b) = series(20, |_| 100.0);
        let (ba, bb) = (owned("A", &ts, &a), owned("B", &ts, &b));
        let cfg = DcaConfig {
            buys: vec![DcaBuy {
                max_fires: 1,
                ..buy_rule(1000.0, true, vec![metric("dd_from_high", Op::Above, 10.0)])
            }],
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let r = run_dca(&s, &[&ba.as_bars(), &bb.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.buys, 1);
        assert!((d.assets[0].invested - 500.0).abs() < 1e-6, "A's half of the tranche");
        assert_eq!(d.assets[1].buys, 0, "B never dropped");
    }

    /// No lookahead: a condition that holds on bar i fills at bar i+1's open.
    #[test]
    fn fills_on_the_next_bar_open() {
        let ts: Vec<String> = (0..6).map(|i| format!("2024-01-0{}T00:00:00Z", i + 1)).collect();
        // Close falls 20% on bar 2; the open of bar 3 is 90 (a gap the fill must pay).
        let open = vec![100.0, 100.0, 100.0, 90.0, 80.0, 80.0];
        let close = vec![100.0, 100.0, 80.0, 80.0, 80.0, 80.0];
        let ba = OwnedBars {
            ticker: "A".into(),
            timeframe: "1d".into(),
            ts,
            open: open.clone(),
            high: close.clone(),
            low: close.clone(),
            close,
            volume: vec![1.0; 6],
        };
        let cfg = DcaConfig {
            buys: vec![DcaBuy {
                max_fires: 1,
                ..buy_rule(900.0, false, vec![metric("dd_from_high", Op::Above, 10.0)])
            }],
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.events.len(), 1);
        assert_eq!(d.events[0].ts, "2024-01-04T00:00:00Z", "the bar after the drop");
        assert!((d.events[0].price - 90.0).abs() < 1e-9, "filled at that bar's open");
        assert!((d.assets[0].units - 10.0).abs() < 1e-9);
    }

    /// A profit objective sells, and the realized trade reconciles with the cash it produced.
    #[test]
    fn target_gain_sells_and_reconciles() {
        let (ts, px) = series(20, |i| if i < 10 { 100.0 } else { 130.0 });
        let ba = owned("A", &ts, &px);
        let cfg = DcaConfig {
            sells: vec![DcaSell {
                name: "target".into(),
                amount_kind: "all".into(),
                amount: 0.0,
                per_asset: true,
                target_gain_pct: 20.0,
                condition: SignalGroup::default(),
                max_fires: 0,
                cooldown_bars: 0,
                withdraw: false,
            }],
            ..Default::default()
        };
        let s = settings(cfg, 1_000.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.as_ref().unwrap();
        assert_eq!(d.sells, 1);
        assert_eq!(r.trades.len(), 1);
        let t = &r.trades[0];
        assert!((t.entry_price - 100.0).abs() < 1e-9);
        assert!((t.exit_price - 130.0).abs() < 1e-9);
        assert!((t.pnl - 300.0).abs() < 1e-6, "10 units × 30, got {}", t.pnl);
        assert!((d.cash - 1_300.0).abs() < 1e-6);
        assert!((d.final_value - 1_300.0).abs() < 1e-6);
        assert!((r.stats.net_pnl - 300.0).abs() < 1e-6);
    }

    /// Fees come out of the tranche, never on top of it: the cash spent is exactly the amount.
    #[test]
    fn fee_is_taken_inside_the_tranche() {
        let (ts, px) = series(10, |_| 100.0);
        let ba = owned("A", &ts, &px);
        let cfg = DcaConfig::default();
        let mut s = settings(cfg, 1_000.0);
        s.fees = Fees { amount_kind: "pct".into(), per: "trade".into(), amount: 1.0 };
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        assert!(d.cash.abs() < 1e-6, "exactly 1000 left the cash, got {}", 1_000.0 - d.cash);
        assert!((d.fees - 1000.0 / 101.0).abs() < 1e-6, "1% of the notional");
        assert!(d.invested + d.fees - 1000.0 < 1e-6);
    }

    /// Money-weighted return: a plan that doubles a single lump sum over a year reports ~100%.
    #[test]
    fn irr_reads_the_money_weighted_return() {
        let n = 366;
        let ts: Vec<String> = (0..n)
            .map(|i| {
                let d = time::Date::from_ordinal_date(2024, 1).unwrap() + time::Duration::days(i as i64);
                format!("{}T00:00:00Z", d)
            })
            .collect();
        let px: Vec<f64> = (0..n).map(|i| 100.0 * (1.0 + i as f64 / (n - 1) as f64)).collect();
        let ba = owned("A", &ts, &px);
        let s = settings(DcaConfig::default(), 10_000.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        let irr = d.irr_pct.expect("irr");
        assert!((irr - 100.0).abs() < 2.0, "≈100%/y, got {irr}");
        assert!((d.total_return_pct - 100.0).abs() < 1e-6);
    }

    /// The lump-sum benchmark deploys the same total money at the start, so a plan that only
    /// buys later must lag it in a rising market.
    #[test]
    fn lump_sum_benchmark_is_the_same_money_at_the_start() {
        let (ts, px) = series(28, |i| 100.0 + i as f64);
        let ba = owned("A", &ts, &px);
        let cfg = DcaConfig {
            contribution: Some(DcaContribution {
                amount: 1000.0,
                period: "week".into(),
                every: 1,
                invest: true,
            }),
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        assert!(d.contributed > 0.0);
        assert!(
            d.lump_sum_return_pct > d.total_return_pct,
            "lump sum {} should beat staged buying {} in a straight line up",
            d.lump_sum_return_pct,
            d.total_return_pct
        );
    }

    /// A conditional tranche is money paid in when it fires: it buys in full whatever the cash
    /// held, books the deposit, and never goes negative.
    #[test]
    fn a_conditional_tranche_is_funded_by_new_money() {
        let (ts, px) = series(10, |_| 100.0);
        let ba = owned("A", &ts, &px);
        let cfg = DcaConfig {
            buys: vec![DcaBuy { max_fires: 1, ..buy_rule(5_000.0, false, vec![always()]) }],
            ..Default::default()
        };
        let s = settings(cfg, 1_000.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        assert!(d.cash >= -1e-9, "cash went negative: {}", d.cash);
        assert!((d.invested - 6_000.0).abs() < 1e-6, "the capital, then the tranche in full");
        assert!((d.deposits - 5_000.0).abs() < 1e-6, "the tranche was new money");
        assert!((d.contributed - 6_000.0).abs() < 1e-6);
        assert_eq!(d.underfunded, 0);
    }

    /// The settings the UI posts (frontend `defaultSettings()` + `defaultDca()`, with a buy and
    /// a sell rule) must deserialize and run. This is the contract between the two halves: a
    /// field renamed on one side fails here rather than as a 400 in the browser.
    #[test]
    fn the_settings_the_ui_posts_deserialize_and_run() {
        let raw = serde_json::json!({
            "kind": "dca",
            "grid": null,
            "dca": {
                "weights": [{ "ticker": "A", "weight": 2 }, { "ticker": "B", "weight": 1 }],
                "contribution": { "amount": 100, "period": "month", "every": 1, "invest": true },
                "buys": [{
                    "name": "dip",
                    "amount_kind": "fixed",
                    "amount": 500,
                    "per_asset": false,
                    "condition": { "logic": "all", "conditions": [
                        { "left": { "kind": "metric", "metric": "dd_from_high", "period": 0 },
                          "op": "above",
                          "right": { "kind": "const", "value": 10 } }
                    ]},
                    "max_fires": 0,
                    "cooldown_bars": 0
                }],
                "sells": [{
                    "name": "take",
                    "amount_kind": "pct_position",
                    "amount": 25,
                    "per_asset": true,
                    "target_gain_pct": 50,
                    "condition": { "logic": "all", "conditions": [] },
                    "max_fires": 0,
                    "cooldown_bars": 0,
                    "withdraw": false
                }]
            },
            "mode": "long",
            "long": null,
            "short": null,
            "stop_and_reverse": false,
            "pyramiding": 1,
            "sizing": { "mode": "percent_equity", "percent": 100 },
            "starting_capital": 10000,
            "leverage": 1,
            "spread_pct": 0,
            "fees": { "amount_kind": "pct", "per": "trade", "amount": 0.1 },
            "risk": {},
            "pyramid_steps": { "scale": [], "min_distance_pct": 0, "after_add_sl": "none" },
            "instrument": { "multiplier": 1, "lot_step": 0, "min_qty": 0 },
            "slippage": { "kind": "pct", "value": 0, "tick_size": 0 },
            "oos_split_pct": 0,
            "funding": { "annual_rate_pct": 0, "interval_hours": 8 },
            "filters": { "tz_offset_min": 0, "weekdays": [], "sessions": [],
                         "include_dates": [], "exclude_dates": [], "on_window_end": "hold",
                         "block_adds": true }
        });
        let s: Settings = serde_json::from_value(raw).expect("the UI's settings must deserialize");
        assert!(s.validate().is_none(), "{:?}", s.validate());
        let (ts, a) = series(84, |i| if i < 40 { 100.0 } else { 60.0 });
        let (_, b) = series(84, |i| 100.0 + i as f64);
        let (ba, bb) = (owned("A", &ts, &a), owned("B", &ts, &b));
        let r = run_portfolio(&s, &[&ba.as_bars(), &bb.as_bars()]);
        let d = r.dca.expect("a dca run reports a dca block");
        assert!(d.buys > 0);
        assert!(d.assets.len() == 2);
        // Two thirds / one third, as the weights say.
        assert!(d.assets[0].invested > d.assets[1].invested);
        assert!(r.warmup_bars == 0);
    }

    /// `max_fires` counts tranches, not legs: a basket rule capped at one fire buys every asset
    /// of the basket once, and never again.
    #[test]
    fn max_fires_counts_tranches_not_legs() {
        let (ts, a) = series(30, |i| 100.0 - i as f64);
        let (_, b) = series(30, |i| 100.0 - i as f64);
        let (ba, bb) = (owned("A", &ts, &a), owned("B", &ts, &b));
        let cfg = DcaConfig {
            buys: vec![DcaBuy {
                max_fires: 1,
                ..buy_rule(1000.0, false, vec![metric("dd_from_high", Op::Above, 5.0)])
            }],
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let r = run_dca(&s, &[&ba.as_bars(), &bb.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.buys, 2, "one tranche, two legs");
        assert!((d.invested - 1000.0).abs() < 1e-6);
    }

    /// A basket sell stated in money splits by the fixed weights, exactly like a buy tranche:
    /// one amount for the basket, not that amount per asset.
    #[test]
    fn a_basket_amount_sell_splits_by_weight() {
        let (ts, a) = series(20, |_| 100.0);
        let (_, b) = series(20, |_| 100.0);
        let (ba, bb) = (owned("A", &ts, &a), owned("B", &ts, &b));
        let cfg = DcaConfig {
            sells: vec![DcaSell {
                name: "trim".into(),
                amount_kind: "amount".into(),
                amount: 400.0,
                per_asset: false,
                target_gain_pct: 0.0,
                condition: SignalGroup {
                    logic: "all".into(),
                    conditions: vec![Signal {
                        left: Operand::Const { value: 1.0 },
                        op: Op::Above,
                        right: Some(Operand::Const { value: 0.0 }),
                    }],
                },
                max_fires: 1,
                cooldown_bars: 0,
                withdraw: false,
            }],
            ..Default::default()
        };
        let s = settings(cfg, 2_000.0);
        let r = run_dca(&s, &[&ba.as_bars(), &bb.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.sells, 2);
        // 400 over the basket at equal weights = 200 each = 2 units each at 100.
        assert!((d.proceeds - 400.0).abs() < 1e-6, "sold {} in total", d.proceeds);
    }

    /// A position operand reads the live book: "add when the position is 10% under water".
    #[test]
    fn position_operand_reads_the_live_position() {
        let (ts, px) = series(20, |i| if i < 5 { 100.0 } else { 80.0 });
        let ba = owned("A", &ts, &px);
        let cfg = DcaConfig {
            buys: vec![DcaBuy {
                max_fires: 1,
                ..buy_rule(
                    500.0,
                    true,
                    vec![Signal {
                        left: Operand::Position { field: "pnl_pct".into() },
                        op: Op::Below,
                        right: Some(Operand::Const { value: -10.0 }),
                    }],
                )
            }],
            ..Default::default()
        };
        let s = settings(cfg, 1_000.0);
        let r = run_dca(&s, &[&ba.as_bars()]);
        let d = r.dca.unwrap();
        assert_eq!(d.buys, 2, "the starting capital, then the top-up");
        assert!((d.assets[0].invested - 1500.0).abs() < 1e-6);
        // 10 units at 100 then 6.25 at 80 → average cost 92.31.
        assert!((d.assets[0].avg_cost - 1500.0 / 16.25).abs() < 1e-6);
    }

    // ── Regressions from the 2026-08-28 audit ────────────────────────────────────────────

    /// Consecutive daily timestamps, so a test can span more than a year.
    fn daily(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| {
                let d = time::Date::from_ordinal_date(2020, 1).unwrap() + time::Duration::days(i as i64);
                format!("{d}T00:00:00Z")
            })
            .collect()
    }

    fn flat(ticker: &str, ts: &[String], px: f64) -> OwnedBars {
        owned(ticker, ts, &vec![px; ts.len()])
    }

    /// cash == contributed - invested - fees + proceeds - withdrawn, on every plan.
    fn check_cash(d: &DcaStats, tag: &str) {
        let want = d.contributed - d.invested - d.fees + d.proceeds - d.withdrawn;
        assert!((d.cash - want).abs() < 1e-6, "[{tag}] cash {} vs ledger {}", d.cash, want);
    }

    /// New money is paid in only once the order is known to fill. A tranche the minimum size
    /// refuses used to book its deposit anyway: money in, and holdings, out of nothing.
    #[test]
    fn a_refused_tranche_deposits_nothing() {
        let ts = daily(20);
        let b = flat("A", &ts, 100.0);
        let mut s = settings(
            DcaConfig {
                buys: vec![DcaBuy { max_fires: 1, ..buy_rule(500.0, false, vec![always()]) }],
                ..Default::default()
            },
            0.0,
        );
        s.instrument = Instrument { multiplier: 1.0, lot_step: 0.0, min_qty: 1000.0 };
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        assert_eq!(d.buys, 0);
        assert!(d.deposits.abs() < 1e-9, "deposited {} for an order that never filled", d.deposits);
        assert!(d.contributed.abs() < 1e-9);
        assert!(d.final_value.abs() < 1e-9);
        check_cash(&d, "refused tranche");
    }

    /// `cost` is a notional, `avg_cost` is a price: the contract multiplier has to come back
    /// out, or every x10 instrument reads ten times its fill price and shows a 90% loss flat.
    #[test]
    fn the_multiplier_is_not_part_of_the_average_cost() {
        let ts = daily(20);
        let px: Vec<f64> = (0..20).map(|i| if i < 10 { 100.0 } else { 110.0 }).collect();
        let b = owned("A", &ts, &px);
        let mut s = settings(
            DcaConfig {
                sells: vec![DcaSell {
                    name: "target".into(),
                    amount_kind: "all".into(),
                    amount: 0.0,
                    per_asset: true,
                    target_gain_pct: 5.0,
                    condition: SignalGroup::default(),
                    max_fires: 1,
                    cooldown_bars: 0,
                    withdraw: false,
                }],
                ..Default::default()
            },
            10_000.0,
        );
        s.instrument = Instrument { multiplier: 10.0, lot_step: 0.0, min_qty: 0.0 };
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        let entry = &d.events[0];
        assert!((entry.qty - 10.0).abs() < 1e-9, "10 contracts, got {}", entry.qty);
        assert!(
            (entry.avg_cost_after - 100.0).abs() < 1e-9,
            "the average cost is a price, got {}",
            entry.avg_cost_after
        );
        // 10 contracts x10 at 100, sold at 110: +10 points is +1000.
        assert!((d.realized_pnl - 1_000.0).abs() < 1e-6, "realized {}", d.realized_pnl);
        assert!((d.final_value - 11_000.0).abs() < 1e-6, "final {}", d.final_value);
        check_cash(&d, "multiplier");
    }

    /// A withdrawal already enters the cash-flow series on its own date: adding it again to
    /// the terminal flow roughly doubled the reported money-weighted return.
    #[test]
    fn a_withdrawal_is_one_cash_flow_not_two() {
        let ts = daily(366);
        let px: Vec<f64> = (0..366).map(|i| if i < 180 { 100.0 } else { 200.0 }).collect();
        let b = owned("A", &ts, &px);
        let s = settings(
            DcaConfig {
                sells: vec![DcaSell {
                    name: "cash out".into(),
                    amount_kind: "all".into(),
                    amount: 0.0,
                    per_asset: true,
                    target_gain_pct: 50.0,
                    condition: SignalGroup::default(),
                    max_fires: 1,
                    cooldown_bars: 0,
                    withdraw: true,
                }],
                ..Default::default()
            },
            10_000.0,
        );
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        assert!((d.withdrawn - 20_000.0).abs() < 1e-6);
        assert!((d.total_return_pct - 100.0).abs() < 1.0, "total {}", d.total_return_pct);
        // 10 000 in, 20 000 out at roughly half a year: about +308%/y, never +650%.
        let irr = d.irr_pct.expect("irr");
        assert!((irr - 308.0).abs() < 15.0, "irr {irr}");
    }

    /// A tranche states ONE amount, whatever number of legs it fills. Resolving the percentage
    /// inside the leg loop let each leg read a book the previous one had already moved.
    #[test]
    fn a_percent_tranche_states_one_amount_for_the_basket() {
        let ts = daily(20);
        let (a, b, c) = (flat("A", &ts, 100.0), flat("B", &ts, 100.0), flat("C", &ts, 100.0));
        let cfg = DcaConfig {
            contribution: Some(DcaContribution {
                amount: 3_000.0,
                period: "year".into(),
                every: 1,
                invest: false,
            }),
            buys: vec![DcaBuy {
                max_fires: 1,
                amount_kind: "pct_cash".into(),
                ..buy_rule(100.0, false, vec![always()])
            }],
            ..Default::default()
        };
        let s = settings(cfg, 0.0);
        let d = run_dca(&s, &[&a.as_bars(), &b.as_bars(), &c.as_bars()]).dca.unwrap();
        assert!((d.invested - 3_000.0).abs() < 1e-6, "deployed {} of 3000", d.invested);
        for x in &d.assets {
            assert!((x.invested - 1_000.0).abs() < 1e-6, "{} took {}", x.ticker, x.invested);
        }
    }

    /// A contribution whose period opens on a bar the filters close waits for the next open
    /// bar instead of being lost.
    #[test]
    fn a_contribution_waits_for_the_first_open_bar() {
        let ts = daily(90);
        let b = flat("A", &ts, 100.0);
        let mut s = settings(
            DcaConfig {
                contribution: Some(DcaContribution {
                    amount: 100.0,
                    period: "week".into(),
                    every: 1,
                    invest: true,
                }),
                ..Default::default()
            },
            0.0,
        );
        // Tuesday to Friday, so every ISO week opens on a bar the filter closes.
        s.filters = Filters { weekdays: vec![2, 3, 4, 5], ..Default::default() };
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        assert!(d.deposits >= 1_200.0, "13 weekly contributions, deposited {}", d.deposits);
    }

    /// A money-sized sell converts at the price it fills at. Converting at the row's close was
    /// lookahead, and a bar that opened at 100 and closed at 200 sold half of what was asked.
    #[test]
    fn a_money_sized_sell_converts_at_the_fill_price() {
        let ts = daily(10);
        let open = vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 200.0, 200.0, 200.0, 200.0];
        let close = vec![100.0, 100.0, 100.0, 100.0, 100.0, 200.0, 200.0, 200.0, 200.0, 200.0];
        let b = OwnedBars {
            ticker: "A".into(),
            timeframe: "1d".into(),
            ts,
            open: open.clone(),
            high: close.clone(),
            low: open,
            close,
            volume: vec![1.0; 10],
        };
        let s = settings(
            DcaConfig {
                sells: vec![DcaSell {
                    name: "trim".into(),
                    amount_kind: "amount".into(),
                    amount: 1_000.0,
                    per_asset: true,
                    target_gain_pct: 0.0,
                    condition: SignalGroup {
                        logic: "all".into(),
                        conditions: vec![metric("change_pct", Op::Below, 1000.0)],
                    },
                    max_fires: 1,
                    cooldown_bars: 0,
                    withdraw: false,
                }],
                ..Default::default()
            },
            10_000.0,
        );
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        let sell = d.events.iter().find(|e| e.action == "sell").expect("a sell");
        assert!(
            (sell.qty * sell.price - 1_000.0).abs() < 1e-6,
            "asked for 1000 worth, sold {} at {}",
            sell.qty,
            sell.price
        );
    }

    /// An unknown vocabulary value is refused instead of falling through to the default arm.
    #[test]
    fn unknown_vocabulary_is_refused() {
        let mut cfg = DcaConfig {
            buys: vec![DcaBuy { amount_kind: "pct_capital".into(), ..buy_rule(50.0, false, vec![always()]) }],
            ..Default::default()
        };
        assert!(cfg.validate().unwrap().contains("amount kind"));
        cfg.buys.clear();
        cfg.sells = vec![DcaSell {
            name: String::new(),
            amount_kind: "pct_book".into(),
            amount: 50.0,
            per_asset: true,
            target_gain_pct: 10.0,
            condition: SignalGroup::default(),
            max_fires: 0,
            cooldown_bars: 0,
            withdraw: false,
        }];
        assert!(cfg.validate().unwrap().contains("amount kind"));
        cfg.sells.clear();
        cfg.buys = vec![buy_rule(
            50.0,
            true,
            vec![Signal {
                left: Operand::Position { field: "pnl".into() },
                op: Op::Below,
                right: Some(Operand::Const { value: -10.0 }),
            }],
        )];
        assert!(cfg.validate().unwrap().contains("portfolio field"));
        cfg.buys[0].condition.conditions[0].left = Operand::Metric { metric: "dd".into(), period: 0 };
        assert!(cfg.validate().unwrap().contains("metric"));
    }

    /// A weight table only the loaded datasets can judge: a ticker the basket does not carry
    /// used to fall back to equal weights in silence.
    #[test]
    fn a_weight_table_that_matches_nothing_is_reported() {
        let cfg = DcaConfig {
            weights: vec![
                DcaWeight { ticker: "AAPL.US".into(), weight: 70.0 },
                DcaWeight { ticker: "MSFT.US".into(), weight: 30.0 },
            ],
            ..Default::default()
        };
        let w = weight_warnings(&cfg, &["AAPL", "MSFT"]);
        assert_eq!(w.len(), 1, "nothing matched, so one line says so: {w:?}");
        assert!(w[0].contains("equal share"), "{w:?}");
        // One good row and one asset the table forgot: that asset is really never bought.
        let half = DcaConfig {
            weights: vec![DcaWeight { ticker: "AAPL".into(), weight: 70.0 }],
            ..Default::default()
        };
        let w = weight_warnings(&half, &["AAPL", "MSFT"]);
        assert!(w.iter().any(|m| m.contains("MSFT has no weight")), "{w:?}");
        assert!(weight_warnings(&DcaConfig::default(), &["AAPL"]).is_empty());
    }

    /// The starting capital is an entry, so the trading window gates it like every other one.
    #[test]
    fn the_starting_capital_waits_for_the_window() {
        let ts = daily(30);
        let b = flat("A", &ts, 100.0);
        let mut s = settings(DcaConfig::default(), 1_000.0);
        s.filters = Filters {
            include_dates: vec![DateRange { from: "2020-01-15".into(), to: Some("2020-01-30".into()) }],
            ..Default::default()
        };
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        assert_eq!(d.buys, 1);
        assert_eq!(d.events[0].ts, "2020-01-15T00:00:00Z");
    }

    /// The share of a contribution an asset had no bar for waits for its first one, instead of
    /// sitting in cash for the rest of the run.
    #[test]
    fn a_share_for_an_asset_that_has_not_listed_waits_for_it() {
        let ts = daily(200);
        let a = flat("A", &ts, 100.0);
        let b = flat("B", &ts[100..], 100.0);
        let s = settings(
            DcaConfig {
                contribution: Some(DcaContribution {
                    amount: 100.0,
                    period: "week".into(),
                    every: 1,
                    invest: true,
                }),
                ..Default::default()
            },
            0.0,
        );
        let d = run_dca(&s, &[&a.as_bars(), &b.as_bars()]).dca.unwrap();
        assert!(d.cash.abs() < 1e-6, "{} left idle", d.cash);
        assert!((d.assets[0].invested - d.assets[1].invested).abs() < 1e-6);
        check_cash(&d, "late listing");
    }

    /// The same ticker picked twice (two timeframes, two providers) states one share of the
    /// basket, not one each.
    #[test]
    fn the_same_ticker_twice_still_takes_one_share() {
        let ts = daily(20);
        let (a1, a2, b) = (flat("BTC", &ts, 100.0), flat("BTC", &ts, 100.0), flat("ETH", &ts, 100.0));
        let cfg = DcaConfig {
            weights: vec![
                DcaWeight { ticker: "BTC".into(), weight: 50.0 },
                DcaWeight { ticker: "ETH".into(), weight: 50.0 },
            ],
            ..Default::default()
        };
        let s = settings(cfg, 1_000.0);
        let d = run_dca(&s, &[&a1.as_bars(), &a2.as_bars(), &b.as_bars()]).dca.unwrap();
        assert!((d.assets[2].target_weight_pct - 50.0).abs() < 1e-9);
        assert!((d.assets[0].target_weight_pct - 25.0).abs() < 1e-9);
    }

    /// `every N` counts calendar periods, not the periods the data happens to carry: a file
    /// with bars in January, March and May used to pay January and May and skip March.
    #[test]
    fn every_n_counts_calendar_periods() {
        let mut ts: Vec<String> = Vec::new();
        for month in ["01", "03", "05"] {
            for day in 1..=28 {
                ts.push(format!("2020-{month}-{day:02}T00:00:00Z"));
            }
        }
        let b = flat("A", &ts, 100.0);
        let s = settings(
            DcaConfig {
                contribution: Some(DcaContribution {
                    amount: 100.0,
                    period: "month".into(),
                    every: 2,
                    invest: true,
                }),
                ..Default::default()
            },
            0.0,
        );
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        let months: Vec<&str> = d.events.iter().map(|e| &e.ts[5..7]).collect();
        assert_eq!(months, vec!["01", "03", "05"], "January, March, May");
    }

    /// Cooldown is counted from the row the rule fired on, not from the row its order filled
    /// on: stamping the fill added the no-lookahead bar to every gap.
    #[test]
    fn cooldown_is_counted_from_the_signal() {
        let ts = daily(40);
        let b = flat("A", &ts, 100.0);
        let s = settings(
            DcaConfig {
                buys: vec![DcaBuy { cooldown_bars: 5, ..buy_rule(100.0, false, vec![always()]) }],
                ..Default::default()
            },
            0.0,
        );
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        assert_eq!(d.buys, 8, "40 bars, one fill every 5");
    }

    /// The fill list travels back capped, with the real count beside it.
    #[test]
    fn the_fill_list_is_capped() {
        let n = EVENT_CAP + 500;
        let ts = daily(n);
        let b = flat("A", &ts, 100.0);
        let s = settings(
            DcaConfig {
                contribution: Some(DcaContribution {
                    amount: 10.0,
                    period: "bar".into(),
                    every: 1,
                    invest: true,
                }),
                ..Default::default()
            },
            0.0,
        );
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        assert_eq!(d.events_total, n);
        assert_eq!(d.events.len(), EVENT_CAP);
        assert_eq!(d.events.last().unwrap().ts, ts[n - 1]);
    }

    /// A percent-of-equity tranche is sized on the book as it stood when the order was placed.
    /// Reading the close of the row it fills on was lookahead, and it let a tranche deploy
    /// twice what the plan had.
    #[test]
    fn a_percent_of_equity_tranche_cannot_see_the_close_it_fills_before() {
        let ts = daily(10);
        let open = vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 200.0, 200.0, 200.0, 200.0];
        let close = vec![100.0, 100.0, 100.0, 100.0, 100.0, 200.0, 200.0, 200.0, 200.0, 200.0];
        let b = OwnedBars {
            ticker: "A".into(),
            timeframe: "1d".into(),
            ts,
            open: open.clone(),
            high: close.clone(),
            low: open,
            close,
            volume: vec![1.0; 10],
        };
        let s = settings(
            DcaConfig {
                buys: vec![DcaBuy {
                    max_fires: 1,
                    amount_kind: "pct_equity".into(),
                    ..buy_rule(100.0, false, vec![metric("change_pct", Op::Below, 1000.0)])
                }],
                ..Default::default()
            },
            1_000.0,
        );
        let d = run_dca(&s, &[&b.as_bars()]).dca.unwrap();
        let tranche = d.events.iter().find(|e| e.source != "initial").expect("a tranche");
        assert!(
            (-tranche.amount - 1_000.0).abs() < 1e-6,
            "100% of a 1000 book, spent {}",
            -tranche.amount
        );
    }

    /// A savings plan produces no out-of-sample block, so the split is refused rather than
    /// accepted and ignored.
    #[test]
    fn a_dca_run_refuses_an_out_of_sample_split() {
        let mut s = settings(DcaConfig::default(), 1_000.0);
        s.oos_split_pct = 0.7;
        assert!(s.validate().unwrap().contains("out-of-sample"));
    }
}
