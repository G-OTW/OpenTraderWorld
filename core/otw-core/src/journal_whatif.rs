//! What-if lab for the Trading Journal.
//!
//! One lab, five scenario kinds over the trader's own closed trades:
//!
//! * `size`: every trade at `factor` times its size, optionally stopped for the day once the
//!   day's realized loss reached a cap;
//! * `filter`: only the trades a condition would have let through (a backtest signal group
//!   read on the last bar closed at the entry, or an entry time window);
//! * `reverse`: the same entries and exits with the side flipped, fees unchanged;
//! * `fees`: the fees paid against one of the user's own fee schedules, optionally on the
//!   other contract of a micro/full-size family;
//! * `exits`: the exit-signal lab of [`crate::journal_exits`], unchanged, carried here so a
//!   researcher can hand any of the five to the lab as one [`Scenario`].
//!
//! The scenario functions ([`size`], [`filter`], [`reverse`], [`fees`]) are pure: they read
//! a slice of [`LabTrade`] and return what each trade would have made. [`assemble`] turns
//! that into an [`Outcome`] (before and after, the curves, the statement). The loaders at
//! the bottom are the only part that reads the database.
//!
//! Nothing is guessed. A trade whose FX cannot be priced, whose entry time is unknown when
//! the scenario needs it, or whose candles are not stored when an indicator needs them is
//! reported in `unmeasured` with the reason and left out of both sides of the comparison.
//! No fee is built in: the alternative is always a schedule the user wrote, and a contract
//! family carries only the ratio between its two multipliers. A statement fires only above
//! the behaviour module's sample floor ([`MIN_SAMPLE`]).

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata::{self as hist_store, Bar};
use otw_store::journal::{self, FeeSchedule, Trade, TradeFilter};
use otw_store::journal_analytics::trade_risk;
use otw_store::journal_market as market_store;

pub use otw_store::journal_behavior::{Insight, MIN_SAMPLE};

use crate::backtest::{self, Defs, IndicatorDefs, Op, Operand, SignalGroup};
use crate::journal_exits;
use crate::journal_market::{self, pick_connector, provider_symbol, trade_window};

/// Size factor bounds.
pub const MIN_FACTOR: f64 = 0.5;
pub const MAX_FACTOR: f64 = 3.0;
/// Trades one indicator filter reads candles for: same ceiling as the exit lab.
pub const MAX_BAR_TRADES: usize = journal_exits::MAX_TRADES;
/// Candles of one series an indicator filter computes over: the whole stored series up to
/// the last entry, newest first, so a long history is read back this far and no further.
const MAX_SERIES_BARS: i64 = 200_000;

// ── Scenario ─────────────────────────────────────────────────────────────────

/// One what-if, as the lab runs it and as a researcher carries it (`apply`). The trades it
/// runs on are not part of it: the same scenario applies to any scope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scenario {
    Size(SizeParams),
    Filter(FilterParams),
    Reverse(ReverseParams),
    Fees(FeesParams),
    Exits(ExitsParams),
}

impl Scenario {
    pub fn kind(&self) -> &'static str {
        match self {
            Scenario::Size(_) => "size",
            Scenario::Filter(_) => "filter",
            Scenario::Reverse(_) => "reverse",
            Scenario::Fees(_) => "fees",
            Scenario::Exits(_) => "exits",
        }
    }
}

pub const KINDS: [&str; 5] = ["exits", "size", "filter", "reverse", "fees"];

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SizeParams {
    /// Size multiplier applied to every trade, 0.5 to 3. Fees scale with it.
    pub factor: f64,
    /// Realized loss of the day, in the display currency, past which no new trade is
    /// opened that day. Omitted = no cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_loss_cap: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FilterParams {
    /// Conditions (backtest signal group, built-in indicators) that must hold on the last
    /// candle closed at the entry. Reads the trade's stored candles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalGroup>,
    /// Entry time window, in the viewer's local time (`tz_offset`). Needs no candle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<TimeWindow>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimeWindow {
    /// Entry hours, 0 to 23. Empty = every hour.
    #[serde(default)]
    pub hours: Vec<u8>,
    /// Entry weekdays, 1 = Monday to 7 = Sunday. Empty = every day.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// false (default): keep only the trades inside the window. true: drop them.
    #[serde(default)]
    pub exclude: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReverseParams {
    /// Flip only the trades this filter matches. Omitted = every trade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<FilterParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FeesParams {
    /// One of the user's fee schedules (GET /api/journal/fee-schedules).
    pub schedule_id: Uuid,
    /// A contract family id from the lab catalog: the trades on the family's other
    /// contract are priced as if traded on `to`. Omitted = every trade, same instrument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// The contract the family scenario moves to (default large).
    #[serde(default)]
    pub to: FamilySide,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FamilySide {
    #[default]
    Large,
    Small,
}

/// The exit-signal lab's run settings, without the trades.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExitsParams {
    /// Omitted = the catalog presets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals: Option<Vec<journal_exits::SignalSpec>>,
    #[serde(default)]
    pub mode: journal_exits::Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extend_bars: Option<usize>,
    #[serde(default)]
    pub objective: journal_exits::Objective,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo: Option<bool>,
}

impl ExitsParams {
    pub fn into_body(self, trade_ids: Vec<Uuid>) -> journal_exits::SimulateBody {
        journal_exits::SimulateBody {
            trade_ids,
            signals: self.signals,
            mode: self.mode,
            extend_bars: self.extend_bars,
            objective: self.objective,
            combo: self.combo,
            view: None,
        }
    }
}

/// Why a scenario cannot run, as a sentence naming the fix. The handler turns it into a 400.
pub fn validate(s: &Scenario) -> Result<(), String> {
    match s {
        Scenario::Size(p) => {
            if !p.factor.is_finite() || p.factor < MIN_FACTOR || p.factor > MAX_FACTOR {
                return Err(format!(
                    "size factor must be between {MIN_FACTOR} and {MAX_FACTOR} (got {})",
                    p.factor
                ));
            }
            if let Some(cap) = p.daily_loss_cap {
                if !cap.is_finite() || cap <= 0.0 {
                    return Err(
                        "daily loss cap must be a positive amount in the display currency, or left empty".into(),
                    );
                }
            }
            Ok(())
        }
        Scenario::Filter(f) => validate_filter(f),
        Scenario::Reverse(r) => match &r.only {
            Some(f) => validate_filter(f),
            None => Ok(()),
        },
        Scenario::Fees(f) => match &f.family {
            Some(id) if family(id).is_none() => {
                Err(format!("unknown contract family '{id}': pick one from the lab catalog"))
            }
            _ => Ok(()),
        },
        // The exit lab validates its own signals against its catalog.
        Scenario::Exits(_) => Ok(()),
    }
}

fn validate_filter(f: &FilterParams) -> Result<(), String> {
    if f.signal.is_none() && f.window.is_none() {
        return Err("a filter needs an indicator condition, a time window, or both".into());
    }
    if let Some(g) = &f.signal {
        if g.logic != "all" && g.logic != "any" {
            return Err("condition logic must be all or any".into());
        }
        if g.conditions.is_empty() {
            return Err("the indicator condition is empty: add at least one rule".into());
        }
        for (i, c) in g.conditions.iter().enumerate() {
            let n = i + 1;
            let binary = !matches!(c.op, Op::Rising | Op::Falling);
            if binary && c.right.is_none() {
                return Err(format!("rule {n} compares with nothing: give it a right side"));
            }
            for op in std::iter::once(&c.left).chain(c.right.iter()) {
                if matches!(op, Operand::CustomIndicator { .. }) {
                    return Err(format!(
                        "rule {n}: custom indicators are not read by the journal lab yet, use a built-in indicator"
                    ));
                }
                crate::histviz_alerts::check_operand(op).map_err(|e| format!("rule {n}: {e}"))?;
            }
        }
    }
    if let Some(w) = &f.window {
        if w.hours.is_empty() && w.weekdays.is_empty() {
            return Err("the time window is empty: pick hours, weekdays, or both".into());
        }
        if w.hours.iter().any(|h| *h > 23) {
            return Err("hours run from 0 to 23".into());
        }
        if w.weekdays.iter().any(|d| !(1..=7).contains(d)) {
            return Err("weekdays run from 1 (Monday) to 7 (Sunday)".into());
        }
    }
    Ok(())
}

// ── Contract families ────────────────────────────────────────────────────────

/// A micro and a full-size contract on the same underlying. `ratio` small contracts make
/// one large one. No fee, no multiplier: only the ratio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    pub id: String,
    pub small: String,
    pub large: String,
    pub ratio: u32,
}

#[derive(Deserialize)]
struct FamilyFile {
    family: Vec<Family>,
}

pub fn families() -> &'static [Family] {
    static TABLE: OnceLock<Vec<Family>> = OnceLock::new();
    TABLE.get_or_init(|| {
        toml::from_str::<FamilyFile>(include_str!("../data/contract_families.toml"))
            .expect("contract_families.toml is valid")
            .family
    })
}

pub fn family(id: &str) -> Option<&'static Family> {
    families().iter().find(|f| f.id == id)
}

/// Whether a journal ticker names a contract on `root`: the root itself, the root with a
/// month code and a year (`MNQZ6`, `MNQZ26`, `MNQZ2026`), or a continuous suffix (`MNQ1!`).
/// Leading `/` and anything after a space, dot or dash are ignored. Nothing looser.
pub fn ticker_on_root(ticker: &str, root: &str) -> bool {
    let t = ticker.trim().trim_start_matches('/').to_ascii_uppercase();
    let head = t.split([' ', '.', '-']).next().unwrap_or("");
    let Some(rest) = head.strip_prefix(root) else { return false };
    if rest.is_empty() {
        return true;
    }
    let b = rest.as_bytes();
    if b.len() >= 2 && b[b.len() - 1] == b'!' && b[..b.len() - 1].iter().all(u8::is_ascii_digit) {
        return true;
    }
    const MONTHS: &[u8] = b"FGHJKMNQUVXZ";
    MONTHS.contains(&b[0])
        && matches!(b.len() - 1, 1 | 2 | 4)
        && b[1..].iter().all(u8::is_ascii_digit)
}

// ── Trades ───────────────────────────────────────────────────────────────────

/// One closed trade as the lab reads it. Money is in the display currency.
#[derive(Debug, Clone)]
pub struct LabTrade {
    pub id: Uuid,
    pub ticker: String,
    pub asset_class: String,
    pub side: String,
    /// Effective exit, in UTC.
    pub exit_at: OffsetDateTime,
    /// The same instants in the viewer's local time (UTC minus `tz_offset`), as the
    /// analytics shift them: hours, weekdays and days are read on these.
    pub entry_local: Option<OffsetDateTime>,
    pub exit_local: OffsetDateTime,
    /// Entry quantity (absolute), in the trade's own units.
    pub qty: f64,
    pub entry_price: Option<f64>,
    pub net: f64,
    pub fees: f64,
    /// Planned risk, when a stop was logged.
    pub risk: Option<f64>,
    /// One unit of the trade's currency in the display currency, on the exit date.
    pub fx: f64,
    pub regime: Option<String>,
    /// Worst and best excursion while open, in percent of the entry price, when measured.
    pub mae_pct: Option<f64>,
    pub mfe_pct: Option<f64>,
}

impl LabTrade {
    fn gross(&self) -> f64 {
        self.net + self.fees
    }
    fn win(&self) -> bool {
        self.net > 0.0
    }
}

/// A trade left out of a scenario, with the reason (`fx`, `open`, `undated`, `unsupported`,
/// `unmapped`, `no_bars`, `thin`, `no_price`, `not_found`).
#[derive(Debug, Clone, Serialize)]
pub struct Unmeasured {
    pub id: Uuid,
    pub ticker: String,
    pub reason: &'static str,
}

// ── Scenario results ─────────────────────────────────────────────────────────

/// What a trade makes under a scenario. `None` from a scenario = the trade is not taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sim {
    pub net: f64,
    pub fees: f64,
}

/// Per-trade fee detail of the `fees` scenario.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FeeRow {
    pub paid: f64,
    pub alternative: f64,
    /// Contracts (or units) of the trade.
    pub qty: f64,
    /// Contracts traded instead on the family's other contract (= `qty` without a family).
    pub target_qty: f64,
    /// Contracts left on the original contract because they do not make a whole one.
    pub remainder_qty: f64,
}

/// One trade under the scenario.
#[derive(Debug, Clone)]
pub struct Step {
    pub sim: Option<Sim>,
    /// Why the trade changed: `capped`, `removed`, `flipped`, `partial`, `too_small`.
    pub note: Option<&'static str>,
    pub fee: Option<FeeRow>,
}

impl Step {
    fn same(t: &LabTrade) -> Step {
        Step { sim: Some(Sim { net: t.net, fees: t.fees }), note: None, fee: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Summary {
    /// Trades taken.
    pub n: usize,
    pub net: f64,
    pub fees: f64,
    pub win_rate: Option<f64>,
    pub expectancy: Option<f64>,
    pub profit_factor: Option<f64>,
    pub max_drawdown: f64,
    /// Worst local day, by exit.
    pub worst_day: Option<f64>,
    /// Cumulative net after each trade of the sample, in exit order. A trade not taken
    /// leaves the curve flat, so the before and after curves share one x axis.
    pub curve: Vec<f64>,
}

/// Summarize a sample under a set of steps (aligned with `trades`).
pub fn summarize(trades: &[LabTrade], steps: &[Option<Sim>]) -> Summary {
    let mut s = Summary::default();
    let (mut wins, mut gross_win, mut gross_loss) = (0usize, 0.0, 0.0);
    let (mut cum, mut peak) = (0.0f64, 0.0f64);
    let mut days: BTreeMap<Date, f64> = BTreeMap::new();
    for (t, step) in trades.iter().zip(steps) {
        if let Some(sim) = step {
            s.n += 1;
            s.net += sim.net;
            s.fees += sim.fees;
            if sim.net > 0.0 {
                wins += 1;
                gross_win += sim.net;
            } else {
                gross_loss -= sim.net;
            }
            *days.entry(t.exit_local.date()).or_default() += sim.net;
            cum += sim.net;
        }
        peak = peak.max(cum);
        s.max_drawdown = s.max_drawdown.max(peak - cum);
        s.curve.push(cum);
    }
    if s.n > 0 {
        s.win_rate = Some(wins as f64 / s.n as f64 * 100.0);
        s.expectancy = Some(s.net / s.n as f64);
    }
    s.profit_factor = (gross_loss > 0.0).then(|| gross_win / gross_loss);
    s.worst_day = days.values().copied().reduce(f64::min);
    s
}

/// Kind-specific totals.
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Detail {
    Size {
        factor: f64,
        daily_loss_cap: Option<f64>,
        /// Days the cap stopped trading on.
        days_capped: usize,
        trades_skipped: usize,
    },
    Filter {
        kept: usize,
        removed: usize,
    },
    Reverse {
        flipped: usize,
    },
    Fees {
        schedule_id: Uuid,
        schedule_name: String,
        family: Option<String>,
        to: Option<FamilySide>,
        paid: f64,
        alternative: f64,
        /// paid minus alternative: positive = the alternative is cheaper.
        saving: f64,
        target_qty: f64,
        remainder_qty: f64,
        /// Trades of the scope that are not on the family's source contract.
        excluded: usize,
    },
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub id: Uuid,
    pub ticker: String,
    pub side: String,
    #[serde(with = "time::serde::rfc3339")]
    pub exit_at: OffsetDateTime,
    pub before: f64,
    /// None = the scenario does not take this trade.
    pub after: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<FeeRow>,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub currency: String,
    pub scenario: Scenario,
    pub before: Summary,
    pub after: Summary,
    pub delta_net: f64,
    /// After minus before: negative = a smaller drawdown.
    pub delta_drawdown: f64,
    pub rows: Vec<Row>,
    pub unmeasured: Vec<Unmeasured>,
    pub min_sample: usize,
    /// One statement (`journal.lab.statement.<key>` in the language packs), only when the
    /// sample clears [`MIN_SAMPLE`] and the scenario moved the result.
    pub statement: Option<Insight>,
    pub detail: Detail,
    /// An indicator condition read on a short history, for the trades it concerns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warmup: Option<Warmup>,
}

/// The trades an indicator condition was read on with less history than it needs to stop
/// depending on where the stored series starts: their EMA, RSI or MACD may differ from the
/// same indicator over a longer history. They stay measured; the caller warns.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Warmup {
    /// Candles the condition needs before the entry. None = never: a cumulative indicator
    /// (OBV) depends on the whole history, wherever it starts.
    pub bars: Option<usize>,
    pub trades: Vec<Uuid>,
}

/// Build the outcome of a scenario from its steps.
pub fn assemble(
    trades: &[LabTrade],
    steps: Vec<Step>,
    detail: Detail,
    scenario: Scenario,
    unmeasured: Vec<Unmeasured>,
    currency: &str,
) -> Outcome {
    let real: Vec<Option<Sim>> = trades.iter().map(|t| Some(Sim { net: t.net, fees: t.fees })).collect();
    let sims: Vec<Option<Sim>> = steps.iter().map(|s| s.sim).collect();
    let before = summarize(trades, &real);
    let after = summarize(trades, &sims);
    let delta_net = after.net - before.net;
    let delta_drawdown = after.max_drawdown - before.max_drawdown;
    let statement = statement(scenario.kind(), before.n, delta_net, delta_drawdown, &detail_values(&detail));
    let rows = trades
        .iter()
        .zip(steps)
        .map(|(t, s)| Row {
            id: t.id,
            ticker: t.ticker.clone(),
            side: t.side.clone(),
            exit_at: t.exit_at,
            before: t.net,
            after: s.sim.map(|x| x.net),
            note: s.note,
            fee: s.fee,
        })
        .collect();
    Outcome {
        currency: currency.to_string(),
        scenario,
        before,
        after,
        delta_net,
        delta_drawdown,
        rows,
        unmeasured,
        min_sample: MIN_SAMPLE,
        statement,
        detail,
        warmup: None,
    }
}

/// The numbers a statement names besides `n`, the delta and the drawdown change.
pub fn detail_values(detail: &Detail) -> Vec<(&'static str, f64)> {
    match detail {
        Detail::Size { factor, trades_skipped, .. } => vec![("factor", *factor), ("skipped", *trades_skipped as f64)],
        Detail::Filter { removed, .. } => vec![("removed", *removed as f64)],
        Detail::Reverse { flipped } => vec![("flipped", *flipped as f64)],
        Detail::Fees { saving, .. } => vec![("saving", *saving)],
    }
}

/// The scenario's one statement. Keys: `<kind>_better` (the scenario would have made more)
/// and `<kind>_worse` (what the trader did holds up). Values are raw numbers; the language
/// packs own the sentence and the client formats money and counts.
pub fn statement(kind: &str, n: usize, delta: f64, dd_delta: f64, extra: &[(&str, f64)]) -> Option<Insight> {
    if n < MIN_SAMPLE || delta.abs() < 1e-9 {
        return None;
    }
    let mut values: BTreeMap<String, f64> = BTreeMap::new();
    values.insert("n".into(), n as f64);
    values.insert("delta".into(), delta);
    values.insert("dd_delta".into(), dd_delta);
    for (k, v) in extra {
        values.insert((*k).to_string(), *v);
    }
    let better = delta > 0.0;
    Some(Insight {
        key: format!("{kind}_{}", if better { "better" } else { "worse" }),
        severity: if better { "info" } else { "good" }.into(),
        values,
        labels: Vec::new(),
    })
}

/// A refusal the caller can act on (too many trades for a candle read, a schedule that is
/// gone). The API answers it with a 400 and the sentence; anything else is a server fault.
#[derive(Debug)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

// ── The scenarios (pure) ─────────────────────────────────────────────────────

/// Every trade at `factor` times its size. With a cap, a trade is not opened once the
/// realized loss of its local day (trades taken that day and closed before it opened)
/// reached the cap; the trade that crosses the cap runs to its end, as it would have.
pub fn size(trades: &[LabTrade], p: &SizeParams) -> (Vec<Step>, Detail) {
    let k = p.factor;
    let mut steps: Vec<Step> = trades
        .iter()
        .map(|t| Step { sim: Some(Sim { net: t.net * k, fees: t.fees * k }), note: None, fee: None })
        .collect();
    let mut days_capped: HashSet<Date> = HashSet::new();
    let mut skipped = 0usize;
    if let Some(cap) = p.daily_loss_cap {
        let start = |t: &LabTrade| t.entry_local.unwrap_or(t.exit_local);
        let mut order: Vec<usize> = (0..trades.len()).collect();
        order.sort_by_key(|&i| start(&trades[i]));
        // Per local day: (exit, net) of the trades taken.
        let mut taken: HashMap<Date, Vec<(OffsetDateTime, f64)>> = HashMap::new();
        for i in order {
            let t = &trades[i];
            let at = start(t);
            let day = at.date();
            let realized: f64 = taken
                .get(&day)
                .map(|v| v.iter().filter(|(x, _)| *x <= at).map(|(_, n)| n).sum())
                .unwrap_or(0.0);
            if realized <= -cap {
                steps[i] = Step { sim: None, note: Some("capped"), fee: None };
                days_capped.insert(day);
                skipped += 1;
                continue;
            }
            let net = steps[i].sim.map(|s| s.net).unwrap_or(0.0);
            taken.entry(t.exit_local.date()).or_default().push((t.exit_local, net));
        }
    }
    let detail = Detail::Size {
        factor: k,
        daily_loss_cap: p.daily_loss_cap,
        days_capped: days_capped.len(),
        trades_skipped: skipped,
    };
    (steps, detail)
}

/// Whether a trade's entry falls inside the window. `None` = the trade has no entry time,
/// which a window cannot be read on.
pub fn in_window(t: &LabTrade, w: &TimeWindow) -> Option<bool> {
    let at = t.entry_local?;
    let hour_ok = w.hours.is_empty() || w.hours.contains(&at.hour());
    let day_ok = w.weekdays.is_empty() || w.weekdays.contains(&at.weekday().number_from_monday());
    Some(hour_ok && day_ok)
}

/// Whether the window lets the trade through (`exclude` flips the window).
pub fn window_keeps(t: &LabTrade, w: &TimeWindow) -> Option<bool> {
    in_window(t, w).map(|inside| inside != w.exclude)
}

/// Keep the trades `keep` lets through; the others are not taken.
pub fn filter(trades: &[LabTrade], keep: &[bool]) -> (Vec<Step>, Detail) {
    let mut kept = 0;
    let steps = trades
        .iter()
        .zip(keep)
        .map(|(t, k)| {
            if *k {
                kept += 1;
                Step::same(t)
            } else {
                Step { sim: None, note: Some("removed"), fee: None }
            }
        })
        .collect();
    (steps, Detail::Filter { kept, removed: trades.len() - kept })
}

/// Flip the side of the trades `flip` names: same entry, same exit, so the gross changes
/// sign and the fees are paid again as they were.
pub fn reverse(trades: &[LabTrade], flip: &[bool]) -> (Vec<Step>, Detail) {
    let mut flipped = 0;
    let steps = trades
        .iter()
        .zip(flip)
        .map(|(t, f)| {
            if *f {
                flipped += 1;
                Step { sim: Some(Sim { net: -t.gross() - t.fees, fees: t.fees }), note: Some("flipped"), fee: None }
            } else {
                Step::same(t)
            }
        })
        .collect();
    (steps, Detail::Reverse { flipped })
}

/// The family move a fees scenario makes: the contract it leaves and the one it trades.
#[derive(Debug, Clone, Copy)]
pub struct FamilyMove<'a> {
    pub family: &'a Family,
    pub to: FamilySide,
}

impl FamilyMove<'_> {
    pub fn source_root(&self) -> &str {
        match self.to {
            FamilySide::Large => &self.family.small,
            FamilySide::Small => &self.family.large,
        }
    }
    /// Whether the trade is on the contract this move leaves.
    pub fn applies(&self, t: &LabTrade) -> bool {
        t.asset_class == "future" && ticker_on_root(&t.ticker, self.source_root())
    }
    /// (contracts on the target, contracts left on the source).
    pub fn split(&self, qty: f64) -> (f64, f64) {
        let r = self.family.ratio as f64;
        match self.to {
            FamilySide::Large => {
                let whole = (qty / r + 1e-9).floor();
                (whole, (qty - whole * r).max(0.0))
            }
            FamilySide::Small => (qty * r, 0.0),
        }
    }
}

/// The fees each trade paid against what the schedule would have charged. `sched_fx[i]`
/// converts one unit of the schedule's currency into the display currency on trade `i`'s
/// exit date (read only for a fixed-amount schedule; the caller has already moved the
/// trades it could not price to `unmeasured`). A percent schedule is in the trade's own
/// currency. The schedule charges the entry quantity at the average entry, the basis the
/// trade form uses. With a family move, the contracts that do not make a whole target
/// contract stay where they were, at the share of the fees they actually paid.
pub fn fees(
    trades: &[LabTrade],
    schedule: &FeeSchedule,
    mv: Option<FamilyMove>,
    sched_fx: &[f64],
    excluded: usize,
) -> (Vec<Step>, Detail) {
    let (mut paid, mut alt_total, mut target_total, mut rem_total) = (0.0, 0.0, 0.0, 0.0);
    let steps = trades
        .iter()
        .zip(sched_fx)
        .map(|(t, fx)| {
            let price = t.entry_price.unwrap_or(0.0);
            let (target_qty, remainder_qty) = match mv {
                Some(m) => m.split(t.qty),
                None => (t.qty, 0.0),
            };
            let rate = if schedule.amount_kind == "pct" { t.fx } else { *fx };
            let charged = if target_qty > 0.0 {
                journal::compute_fee(schedule, target_qty, price) * rate
            } else {
                0.0
            };
            let kept = if t.qty > 0.0 { t.fees * remainder_qty / t.qty } else { 0.0 };
            let alternative = charged + kept;
            let note = if mv.is_some() && target_qty <= 0.0 {
                Some("too_small")
            } else if remainder_qty > 0.0 {
                Some("partial")
            } else {
                None
            };
            paid += t.fees;
            alt_total += alternative;
            target_total += target_qty;
            rem_total += remainder_qty;
            Step {
                sim: Some(Sim { net: t.net + t.fees - alternative, fees: alternative }),
                note,
                fee: Some(FeeRow { paid: t.fees, alternative, qty: t.qty, target_qty, remainder_qty }),
            }
        })
        .collect();
    let detail = Detail::Fees {
        schedule_id: schedule.id,
        schedule_name: schedule.name.clone(),
        family: mv.map(|m| m.family.id.clone()),
        to: mv.map(|m| m.to),
        paid,
        alternative: alt_total,
        saving: paid - alt_total,
        target_qty: target_total,
        remainder_qty: rem_total,
        excluded,
    };
    (steps, detail)
}

/// Whether a signal group holds on each bar of `bars`, read with the backtest's own
/// `resolve` (the math a chart alert and a strategy read) over the whole series, so a
/// recursive indicator (EMA, RSI, MACD) is seeded as the chart seeds it. `None` = an
/// operand has no value there yet (not enough history), which is not a "no".
pub fn holds_series(group: &SignalGroup, bars: &[Bar]) -> Vec<Option<bool>> {
    use time::format_description::well_known::Rfc3339;
    let ts: Vec<String> = bars.iter().map(|b| b.ts.format(&Rfc3339).unwrap_or_default()).collect();
    let col = |f: fn(&Bar) -> f64| bars.iter().map(f).collect::<Vec<f64>>();
    let (open, high, low, close, volume) =
        (col(|b| b.open), col(|b| b.high), col(|b| b.low), col(|b| b.close), col(|b| b.volume));
    let b = backtest::Bars {
        ticker: "",
        ts: &ts,
        open: &open,
        high: &high,
        low: &low,
        close: &close,
        volume: &volume,
        quotes: None,
        sub: None,
        script: None,
    };
    let dag = IndicatorDefs::new();
    let scripts = backtest::script::ScriptDefs::new();
    let defs = Defs { dag: &dag, scripts: &scripts };
    // Every operand a condition reads must exist on the bar (and the one before, for a
    // crossing or a slope): otherwise the answer would be "false" for a want of history.
    let mut known = vec![true; bars.len()];
    for c in &group.conditions {
        let needs_prev = matches!(c.op, Op::CrossesAbove | Op::CrossesBelow | Op::Cross | Op::Rising | Op::Falling);
        for op in std::iter::once(&c.left).chain(c.right.iter()) {
            let s = backtest::resolve(op, &b, &defs);
            for (i, k) in known.iter_mut().enumerate() {
                if s[i].is_none() || (needs_prev && (i == 0 || s[i - 1].is_none())) {
                    *k = false;
                }
            }
        }
    }
    let held = backtest::eval_group(group, &b, &defs);
    known.into_iter().zip(held).map(|(k, h)| k.then_some(h)).collect()
}

// ── Strategy report card (pure part) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CardBucket {
    pub key: String,
    pub trades: usize,
    pub net: f64,
    pub avg: f64,
    pub win_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct CardStats {
    pub summary: Summary,
    pub avg_r: Option<f64>,
    /// Trades carrying an R (a logged stop).
    pub r_n: usize,
    pub r_dist: Vec<CardBucket>,
    pub by_hour: Vec<CardBucket>,
    pub by_weekday: Vec<CardBucket>,
    pub by_regime: Vec<CardBucket>,
    pub by_instrument: Vec<CardBucket>,
}

const R_EDGES: [(f64, f64, &str); 7] = [
    (f64::NEG_INFINITY, -2.0, "lt_-2"),
    (-2.0, -1.0, "-2_-1"),
    (-1.0, 0.0, "-1_0"),
    (0.0, 1.0, "0_1"),
    (1.0, 2.0, "1_2"),
    (2.0, 3.0, "2_3"),
    (3.0, f64::INFINITY, "ge_3"),
];

fn buckets<K: Ord + ToString>(trades: &[LabTrade], key: impl Fn(&LabTrade) -> Option<K>) -> Vec<CardBucket> {
    let mut m: BTreeMap<K, (usize, usize, f64)> = BTreeMap::new();
    for t in trades {
        if let Some(k) = key(t) {
            let e = m.entry(k).or_default();
            e.0 += 1;
            e.1 += t.win() as usize;
            e.2 += t.net;
        }
    }
    m.into_iter()
        .map(|(k, (n, w, net))| CardBucket {
            key: k.to_string(),
            trades: n,
            net,
            avg: net / n as f64,
            win_rate: w as f64 / n as f64 * 100.0,
        })
        .collect()
}

pub fn card_stats(trades: &[LabTrade]) -> CardStats {
    let real: Vec<Option<Sim>> = trades.iter().map(|t| Some(Sim { net: t.net, fees: t.fees })).collect();
    let rs: Vec<f64> = trades
        .iter()
        .filter_map(|t| t.risk.filter(|r| *r > 0.0).map(|r| t.net / r))
        .collect();
    let r_dist = R_EDGES
        .iter()
        .map(|(lo, hi, key)| {
            let inside: Vec<f64> = rs.iter().copied().filter(|r| r >= lo && r < hi).collect();
            let n = inside.len();
            CardBucket {
                key: (*key).to_string(),
                trades: n,
                net: inside.iter().sum(),
                avg: if n > 0 { inside.iter().sum::<f64>() / n as f64 } else { 0.0 },
                win_rate: if n > 0 { inside.iter().filter(|r| **r > 0.0).count() as f64 / n as f64 * 100.0 } else { 0.0 },
            }
        })
        .collect();
    CardStats {
        summary: summarize(trades, &real),
        avg_r: (!rs.is_empty()).then(|| rs.iter().sum::<f64>() / rs.len() as f64),
        r_n: rs.len(),
        r_dist,
        by_hour: buckets(trades, |t| t.entry_local.map(|e| e.hour())),
        by_weekday: buckets(trades, |t| t.entry_local.map(|e| e.weekday().number_from_monday())),
        by_regime: buckets(trades, |t| t.regime.clone()),
        by_instrument: buckets(trades, |t| Some(t.ticker.clone())),
    }
}

fn quantile(v: &[f64], q: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let pos = q * (s.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(s[lo] + (s[hi] - s[lo]) * (pos - lo as f64))
}

/// One of the four standard scenarios of the card: its parameters, or why it cannot be
/// set from this book (`no_mae`, `no_mfe`, `no_losing_hour`).
#[derive(Debug, Clone, Serialize)]
pub struct Standard {
    /// `stop_mae` | `target_mfe` | `drop_hour` | `size_up`
    pub key: &'static str,
    pub scenario: Option<Scenario>,
    pub missing: Option<&'static str>,
    /// The parameter the statement names (pct, hour, factor, cap).
    pub values: BTreeMap<String, f64>,
}

/// Size the card scales the book by.
pub const CARD_SIZE_FACTOR: f64 = 1.5;
/// Trades an hour needs before it can be called the worst one.
const CARD_MIN_HOUR_TRADES: usize = 3;

fn round_to(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

/// The four scenarios the card runs on its own: a stop at the winners' median MAE, a
/// target at the 75th percentile of the MFE, the worst entry hour dropped, and the size at
/// 1.5 times under the given daily loss cap.
pub fn standard_scenarios(trades: &[LabTrade], loss_cap: Option<f64>) -> Vec<Standard> {
    let mut out = Vec::with_capacity(4);
    let one = |k: &str, v: f64| -> BTreeMap<String, f64> { [(k.to_string(), v)].into() };
    let exit = |id: &str, rule: &str, key: &str, v: f64| {
        Scenario::Exits(ExitsParams {
            signals: Some(vec![journal_exits::SignalSpec {
                id: id.into(),
                rule: rule.into(),
                params: [(key.to_string(), v)].into(),
            }]),
            combo: Some(false),
            ..Default::default()
        })
    };

    let winners_mae: Vec<f64> = trades.iter().filter(|t| t.win()).filter_map(|t| t.mae_pct).collect();
    out.push(match quantile(&winners_mae, 0.5) {
        Some(m) => {
            let pct = round_to(m, 0.1).clamp(0.1, 50.0);
            Standard { key: "stop_mae", scenario: Some(exit("stop_mae", "sl_pct", "pct", pct)), missing: None, values: one("pct", pct) }
        }
        None => Standard { key: "stop_mae", scenario: None, missing: Some("no_mae"), values: BTreeMap::new() },
    });

    let mfe: Vec<f64> = trades.iter().filter_map(|t| t.mfe_pct).collect();
    out.push(match quantile(&mfe, 0.75) {
        Some(m) => {
            let pct = round_to(m, 0.1).clamp(0.1, 200.0);
            Standard { key: "target_mfe", scenario: Some(exit("target_mfe", "tp_pct", "pct", pct)), missing: None, values: one("pct", pct) }
        }
        None => Standard { key: "target_mfe", scenario: None, missing: Some("no_mfe"), values: BTreeMap::new() },
    });

    let worst = buckets(trades, |t| t.entry_local.map(|e| e.hour()))
        .into_iter()
        .filter(|b| b.trades >= CARD_MIN_HOUR_TRADES && b.net < 0.0)
        .min_by(|a, b| a.net.total_cmp(&b.net));
    out.push(match worst {
        Some(b) => {
            let hour: u8 = b.key.parse().unwrap_or(0);
            Standard {
                key: "drop_hour",
                scenario: Some(Scenario::Filter(FilterParams {
                    signal: None,
                    window: Some(TimeWindow { hours: vec![hour], weekdays: Vec::new(), exclude: true }),
                })),
                missing: None,
                values: one("hour", hour as f64),
            }
        }
        None => Standard { key: "drop_hour", scenario: None, missing: Some("no_losing_hour"), values: BTreeMap::new() },
    });

    let mut values = one("factor", CARD_SIZE_FACTOR);
    if let Some(c) = loss_cap {
        values.insert("cap".into(), c);
    }
    out.push(Standard {
        key: "size_up",
        scenario: Some(Scenario::Size(SizeParams { factor: CARD_SIZE_FACTOR, daily_loss_cap: loss_cap })),
        missing: None,
        values,
    });
    out
}

// ── Loading (I/O) ────────────────────────────────────────────────────────────

/// Which trades a run reads.
pub enum Scope<'a> {
    Filter(&'a TradeFilter),
    Ids(&'a [Uuid]),
}

pub struct Book {
    /// Closed trades in exit order.
    pub trades: Vec<LabTrade>,
    /// The stored rows behind them, for the candle reads.
    pub raw: HashMap<Uuid, Trade>,
    pub unmeasured: Vec<Unmeasured>,
}

/// Load the scope's closed trades into the display currency. A trade the FX table cannot
/// price is named in `unmeasured`; an open trade is too when it was asked for by id.
pub async fn load(pool: &sqlx::PgPool, scope: Scope<'_>, currency: &str, tz_offset: i32) -> Result<Book> {
    let (trades, by_id) = match scope {
        Scope::Filter(f) => (journal::list_trades(pool, f).await?, false),
        Scope::Ids(ids) => {
            let mut v = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(t) = journal::get_trade(pool, *id).await? {
                    v.push(t);
                }
            }
            (v, true)
        }
    };
    let offset = Duration::minutes(tz_offset as i64);
    let ids: Vec<Uuid> = trades.iter().map(|t| t.id).collect();
    let metrics = market_store::metrics_for(pool, &ids).await?;
    let mut fx = otw_store::journal_fx::FxCache::new();
    let mut book = Book { trades: Vec::new(), raw: HashMap::new(), unmeasured: Vec::new() };
    for t in trades {
        let gone = |t: &Trade, reason| Unmeasured { id: t.id, ticker: t.ticker.clone(), reason };
        let Some(native_net) = t.net_pnl.filter(|_| t.open_qty <= 0.0) else {
            if by_id {
                book.unmeasured.push(gone(&t, "open"));
            }
            continue;
        };
        let (entry_at, exit_at) = match trade_window(&t) {
            Some((a, b)) => (Some(a), b),
            None => (t.entry_at, t.exit_at.or(t.entry_at).unwrap_or(t.created_at)),
        };
        let Some(rate) = fx.convert(pool, 1.0, &t.currency, currency, exit_at.date()).await? else {
            book.unmeasured.push(gone(&t, "fx"));
            continue;
        };
        let entry_price = t.avg_entry.filter(|p| *p > 0.0);
        let m = metrics.get(&t.id);
        let pct = |p: Option<f64>| match (p, entry_price) {
            (Some(p), Some(e)) => Some((p - e).abs() / e * 100.0),
            _ => None,
        };
        book.trades.push(LabTrade {
            id: t.id,
            ticker: t.ticker.clone(),
            asset_class: t.asset_class.clone(),
            side: t.side.clone(),
            exit_at,
            entry_local: entry_at.map(|e| e - offset),
            exit_local: exit_at - offset,
            qty: journal::trade_entry_qty(&t).abs(),
            entry_price,
            net: native_net * rate,
            fees: journal::trade_total_fees(&t) * rate,
            risk: trade_risk(&t).map(|r| r * rate),
            fx: rate,
            regime: m.and_then(|m| m.regime.clone()),
            mae_pct: pct(m.and_then(|m| m.mae_price)),
            mfe_pct: pct(m.and_then(|m| m.mfe_price)),
        });
        book.raw.insert(t.id, t);
    }
    book.trades.sort_by_key(|t| t.exit_at);
    Ok(book)
}

/// Whether each trade passes a filter: `Ok(bool)`, or `Err(reason)` when it cannot be read
/// (no entry time for a window, no candles for a condition). With an indicator condition,
/// also the trades it was read on with too short a history (see [`Warmup`]).
#[allow(clippy::type_complexity)]
pub async fn filter_mask(
    pool: &sqlx::PgPool,
    book: &Book,
    f: &FilterParams,
) -> Result<(Vec<Result<bool, &'static str>>, Option<Warmup>)> {
    let mut out: Vec<Result<bool, &'static str>> = book
        .trades
        .iter()
        .map(|t| match &f.window {
            Some(w) => window_keeps(t, w).ok_or("undated"),
            None => Ok(true),
        })
        .collect();
    if let Some(group) = &f.signal {
        let need: Vec<&Trade> = book
            .trades
            .iter()
            .zip(&out)
            .filter(|(_, r)| matches!(r, Ok(true)))
            .filter_map(|(t, _)| book.raw.get(&t.id))
            .collect();
        if need.len() > MAX_BAR_TRADES {
            return Err(Refused(format!(
                "an indicator condition reads candles for at most {MAX_BAR_TRADES} trades ({} here): narrow the filters or pick fewer trades",
                need.len()
            ))
            .into());
        }
        let read = entry_signals(pool, &need, group).await?;
        let settle = group_settle(group);
        let mut short = Vec::new();
        for (t, r) in book.trades.iter().zip(out.iter_mut()) {
            if !matches!(r, Ok(true)) {
                continue;
            }
            *r = match read.get(&t.id).copied().unwrap_or(Err("no_bars")) {
                Ok((v, history)) => {
                    if settle.is_none_or(|s| history < s) {
                        short.push(t.id);
                    }
                    Ok(v)
                }
                Err(why) => Err(why),
            };
        }
        let warmup = (!short.is_empty()).then_some(Warmup { bars: settle, trades: short });
        return Ok((out, warmup));
    }
    Ok((out, None))
}

/// The signal group read on the last candle closed at each trade's entry. The group is
/// computed once per series, over every stored candle up to the series' last entry (at most
/// [`MAX_SERIES_BARS`]): weekends, nights and holidays are simply not in the series, and an
/// indicator sees the same history the chart does. The series is the one the enrichment
/// downloads (same connector choice, same grain). Only the trades are filtered, never the
/// candles. Reasons: `undated`, `unsupported`, `unmapped`, `no_bars`, `thin`.
#[allow(clippy::type_complexity)]
async fn entry_signals(
    pool: &sqlx::PgPool,
    trades: &[&Trade],
    group: &SignalGroup,
) -> Result<HashMap<Uuid, Result<(bool, usize), &'static str>>> {
    let settings = market_store::get_settings(pool).await?;
    let granted = conn_store::list_for_module(pool, journal_market::MODULE).await?;
    let timeframe = journal_market::resolve_timeframe(&settings.timeframe, trades);
    let bar = Duration::seconds(crate::histdata::timeframe_secs(&timeframe)?);
    let mut out = HashMap::new();
    // Per series: the trades on it and their entries.
    let mut series: HashMap<(String, String), Vec<(Uuid, OffsetDateTime)>> = HashMap::new();
    for t in trades {
        let Some((entry_at, _)) = trade_window(t) else {
            out.insert(t.id, Err("undated"));
            continue;
        };
        let Some(asset_type) = journal_market::asset_type_for(&t.asset_class) else {
            out.insert(t.id, Err("unsupported"));
            continue;
        };
        let Some(ticker) = provider_symbol(&settings.symbols, &t.asset_class, &t.ticker) else {
            out.insert(t.id, Err("unmapped"));
            continue;
        };
        series.entry((asset_type.to_string(), ticker)).or_default().push((t.id, entry_at));
    }
    for ((asset_type, ticker), entries) in series {
        let preferred = pick_connector(&granted, &settings.connectors, &asset_type).map(|c| c.provider.clone());
        let dataset = hist_store::find_dataset_any(pool, &asset_type, &ticker, &timeframe, preferred.as_deref()).await?;
        let Some(dataset) = dataset else {
            out.extend(entries.iter().map(|(id, _)| (*id, Err("no_bars"))));
            continue;
        };
        let last = entries.iter().map(|(_, at)| *at).max();
        let bars = hist_store::read_bars(pool, dataset.id, None, last, MAX_SERIES_BARS).await?;
        let held = holds_series(group, &bars);
        out.extend(entries.into_iter().map(|(id, at)| (id, read_at_entry(&bars, &held, bar, at))));
    }
    Ok(out)
}

/// Relative error, as a share of the indicator's range, under which a value read on a short
/// history counts as the value of the long one.
const SETTLE_TOLERANCE: f64 = 1e-3;

/// Candles an operand needs before its value stops depending on where the series starts.
/// Measured on the indicator itself over a synthetic series, like the backtest measures its
/// lookback: the value on the last bars of a truncated copy is compared with the full one.
/// A moving average or a channel settles at its period; an EMA, a Wilder RSI or a MACD needs
/// several times it. `None` = it never settles (a cumulative one such as OBV).
pub fn settle_bars(op: &Operand) -> Option<usize> {
    let dag = IndicatorDefs::new();
    let scripts = backtest::script::ScriptDefs::new();
    let defs = Defs { dag: &dag, scripts: &scripts };
    let first = backtest::op_lookback(op, &defs).max(1);
    let n = first * 20 + 1_000;
    let close: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64 * 0.7).sin() * 5.0 + (i as f64 * 0.13).cos() * 3.0).collect();
    let bars: Vec<Bar> = (0..n)
        .map(|i| {
            let open = if i == 0 { close[0] } else { close[i - 1] };
            Bar {
                ts: OffsetDateTime::UNIX_EPOCH + Duration::hours(i as i64),
                open,
                high: open.max(close[i]) + 1.0,
                low: open.min(close[i]) - 1.0,
                close: close[i],
                volume: 1000.0 + (i % 7) as f64 * 100.0,
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            }
        })
        .collect();
    let series = |b: &[Bar]| -> Vec<Option<f64>> {
        use time::format_description::well_known::Rfc3339;
        let ts: Vec<String> = b.iter().map(|x| x.ts.format(&Rfc3339).unwrap_or_default()).collect();
        let col = |f: fn(&Bar) -> f64| b.iter().map(f).collect::<Vec<f64>>();
        let (o, h, l, c, v) = (col(|x| x.open), col(|x| x.high), col(|x| x.low), col(|x| x.close), col(|x| x.volume));
        let bb = backtest::Bars {
            ticker: "",
            ts: &ts,
            open: &o,
            high: &h,
            low: &l,
            close: &c,
            volume: &v,
            quotes: None,
            sub: None,
            script: None,
        };
        backtest::resolve(op, &bb, &defs)
    };
    let full = series(&bars);
    let values: Vec<f64> = full.iter().flatten().copied().collect();
    let range = values.iter().copied().fold(f64::NEG_INFINITY, f64::max) - values.iter().copied().fold(f64::INFINITY, f64::min);
    if !range.is_finite() || range <= 0.0 {
        return Some(first);
    }
    // The value read on `len` bars of history, against the full series, at a few ends so
    // one lucky bar of the synthetic wave does not settle it.
    const ENDS: [usize; 5] = [0, 7, 13, 29, 41];
    let matches = |len: usize| -> bool {
        ENDS.iter().all(|k| {
            let end = n - k;
            match (series(&bars[end - len..end])[len - 1], full[end - 1]) {
                (Some(a), Some(b)) => (a - b).abs() <= SETTLE_TOLERANCE * range,
                _ => false,
            }
        })
    };
    let (mut lo, mut hi) = (first, n / 2 - ENDS[ENDS.len() - 1]);
    if !matches(hi) {
        return None;
    }
    while lo < hi {
        let mid = (lo + hi) / 2;
        if matches(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}

/// The history a signal group needs: its slowest operand. `None` = one never settles.
pub fn group_settle(group: &SignalGroup) -> Option<usize> {
    let mut need = 0;
    for c in &group.conditions {
        for op in std::iter::once(&c.left).chain(c.right.iter()) {
            need = need.max(settle_bars(op)?);
        }
    }
    Some(need)
}

/// The signal on the last candle closed at `entry_at`: a bar is a period, and the last one
/// whose period ended at or before the entry is the newest thing the trader could have read.
/// `thin` = no candle closed before the entry, or the indicator has no value there yet.
/// `Ok` carries the candles of history it was read on (that one included).
pub fn read_at_entry(
    bars: &[Bar],
    held: &[Option<bool>],
    bar: Duration,
    entry_at: OffsetDateTime,
) -> Result<(bool, usize), &'static str> {
    let closed = bars.partition_point(|b| b.ts + bar <= entry_at);
    closed.checked_sub(1).and_then(|i| held[i]).map(|v| (v, closed)).ok_or("thin")
}

/// Each trade's fixed-fee rate: one unit of the schedule's currency in the display
/// currency on the trade's exit date. `None` = the FX table cannot price it.
pub async fn schedule_rates(
    pool: &sqlx::PgPool,
    trades: &[LabTrade],
    schedule: &FeeSchedule,
    currency: &str,
) -> Result<Vec<Option<f64>>> {
    let mut fx = otw_store::journal_fx::FxCache::new();
    let mut out = Vec::with_capacity(trades.len());
    for t in trades {
        out.push(if schedule.amount_kind == "pct" {
            Some(t.fx)
        } else {
            fx.convert(pool, 1.0, &schedule.currency, currency, t.exit_at.date()).await?
        });
    }
    Ok(out)
}

/// Keep the trades whose flag is `Ok`, and add the others to `unmeasured`.
pub fn split_measured<T: Copy>(
    trades: &[LabTrade],
    unmeasured: &[Unmeasured],
    flags: Vec<Result<T, &'static str>>,
) -> (Vec<LabTrade>, Vec<T>, Vec<Unmeasured>) {
    let mut gone = unmeasured.to_vec();
    let mut kept_trades = Vec::new();
    let mut kept = Vec::new();
    for (t, f) in trades.iter().zip(flags) {
        match f {
            Ok(v) => {
                kept_trades.push(t.clone());
                kept.push(v);
            }
            Err(reason) => gone.push(Unmeasured { id: t.id, ticker: t.ticker.clone(), reason }),
        }
    }
    (kept_trades, kept, gone)
}

/// Run one scenario other than `exits` over a loaded book.
pub async fn run(pool: &sqlx::PgPool, book: &Book, scenario: &Scenario, currency: &str) -> Result<Outcome> {
    match scenario {
        Scenario::Size(p) => {
            let (steps, detail) = size(&book.trades, p);
            Ok(assemble(&book.trades, steps, detail, scenario.clone(), book.unmeasured.clone(), currency))
        }
        Scenario::Filter(f) => {
            let (mask, warmup) = filter_mask(pool, book, f).await?;
            let (trades, keep, unmeasured) = split_measured(&book.trades, &book.unmeasured, mask);
            let (steps, detail) = filter(&trades, &keep);
            let mut o = assemble(&trades, steps, detail, scenario.clone(), unmeasured, currency);
            o.warmup = warmup;
            Ok(o)
        }
        Scenario::Reverse(r) => {
            let (mask, warmup) = match &r.only {
                Some(f) => filter_mask(pool, book, f).await?,
                None => (vec![Ok(true); book.trades.len()], None),
            };
            let (trades, flip, unmeasured) = split_measured(&book.trades, &book.unmeasured, mask);
            let (steps, detail) = reverse(&trades, &flip);
            let mut o = assemble(&trades, steps, detail, scenario.clone(), unmeasured, currency);
            o.warmup = warmup;
            Ok(o)
        }
        Scenario::Fees(p) => {
            let schedule = journal::get_fee_schedule(pool, p.schedule_id).await?.ok_or_else(|| {
                Refused("fee schedule not found: add one under Journal, Fees, then pick it in the lab".into())
            })?;
            let mv = p.family.as_deref().and_then(family).map(|family| FamilyMove { family, to: p.to });
            let (scope, excluded): (Vec<LabTrade>, usize) = match mv {
                Some(m) => {
                    let inside: Vec<LabTrade> = book.trades.iter().filter(|t| m.applies(t)).cloned().collect();
                    let out = book.trades.len() - inside.len();
                    (inside, out)
                }
                None => (book.trades.clone(), 0),
            };
            let rates = schedule_rates(pool, &scope, &schedule, currency).await?;
            let flags: Vec<Result<f64, &'static str>> = rates
                .into_iter()
                .zip(&scope)
                .map(|(r, t)| if t.entry_price.is_none() { Err("no_price") } else { r.ok_or("fx") })
                .collect();
            let (trades, rates, unmeasured) = split_measured(&scope, &book.unmeasured, flags);
            let (steps, detail) = fees(&trades, &schedule, mv, &rates, excluded);
            Ok(assemble(&trades, steps, detail, scenario.clone(), unmeasured, currency))
        }
        Scenario::Exits(_) => Err(anyhow!("the exit lab runs through journal_exits::simulate")),
    }
}

/// One standard scenario of the strategy card, run: the totals only.
#[derive(Debug, Serialize)]
pub struct CardRun {
    pub key: &'static str,
    /// The scenario as run, to open it in the lab.
    pub scenario: Option<Scenario>,
    /// Why it was not run: `no_mae`, `no_mfe`, `no_losing_hour`, `no_bars`.
    pub missing: Option<&'static str>,
    pub values: BTreeMap<String, f64>,
    pub n: usize,
    pub before_net: f64,
    pub after_net: f64,
    pub delta_net: f64,
    pub before_drawdown: f64,
    pub after_drawdown: f64,
    pub delta_drawdown: f64,
    /// Trades of the strategy the scenario could not read.
    pub unmeasured: usize,
    pub statement: Option<Insight>,
}

impl CardRun {
    pub fn missing(s: Standard, why: &'static str) -> CardRun {
        CardRun {
            key: s.key,
            scenario: s.scenario,
            missing: Some(why),
            values: s.values,
            n: 0,
            before_net: 0.0,
            after_net: 0.0,
            delta_net: 0.0,
            before_drawdown: 0.0,
            after_drawdown: 0.0,
            delta_drawdown: 0.0,
            unmeasured: 0,
            statement: None,
        }
    }

    pub fn from_outcome(s: Standard, o: &Outcome) -> CardRun {
        CardRun {
            key: s.key,
            scenario: s.scenario,
            missing: None,
            values: s.values,
            n: o.before.n,
            before_net: o.before.net,
            after_net: o.after.net,
            delta_net: o.delta_net,
            before_drawdown: o.before.max_drawdown,
            after_drawdown: o.after.max_drawdown,
            delta_drawdown: o.delta_drawdown,
            unmeasured: o.unmeasured.len(),
            statement: o.statement.clone(),
        }
    }

    /// From an exit-lab report run on one signal.
    pub fn from_exits(s: Standard, r: &journal_exits::Report) -> CardRun {
        let n = r.trades.len();
        let (after_net, after_dd) = r
            .results
            .first()
            .map(|x| (x.stats.net, x.stats.max_drawdown))
            .unwrap_or((r.baseline.net, r.baseline.max_drawdown));
        let delta = after_net - r.baseline.net;
        let dd_delta = after_dd - r.baseline.max_drawdown;
        let extra: Vec<(&str, f64)> = s.values.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let statement = statement("exits", n, delta, dd_delta, &extra);
        CardRun {
            key: s.key,
            missing: None,
            n,
            before_net: r.baseline.net,
            after_net,
            delta_net: delta,
            before_drawdown: r.baseline.max_drawdown,
            after_drawdown: after_dd,
            delta_drawdown: dd_delta,
            unmeasured: r.skipped.len(),
            statement,
            scenario: s.scenario,
            values: s.values,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn trade(net: f64, fees: f64, at: OffsetDateTime) -> LabTrade {
        LabTrade {
            id: Uuid::new_v4(),
            ticker: "AAPL".into(),
            asset_class: "stock".into(),
            side: "long".into(),
            exit_at: at,
            entry_local: Some(at - Duration::hours(1)),
            exit_local: at,
            qty: 1.0,
            entry_price: Some(100.0),
            net,
            fees,
            risk: Some(50.0),
            fx: 1.0,
            regime: None,
            mae_pct: None,
            mfe_pct: None,
        }
    }

    fn book(nets: &[f64]) -> Vec<LabTrade> {
        let t0 = datetime!(2026-01-05 10:00 UTC);
        nets.iter().enumerate().map(|(i, n)| trade(*n, 1.0, t0 + Duration::days(i as i64))).collect()
    }

    fn schedule(amount: f64, kind: &str, per: &str, currency: &str) -> FeeSchedule {
        FeeSchedule {
            id: Uuid::nil(),
            name: "alt".into(),
            amount,
            amount_kind: kind.into(),
            per: per.into(),
            currency: currency.into(),
            position: 0.0,
        }
    }

    #[test]
    fn summary_curve_stays_flat_on_a_trade_not_taken() {
        let t = book(&[10.0, -20.0, 5.0]);
        let s = summarize(&t, &[Some(Sim { net: 10.0, fees: 1.0 }), None, Some(Sim { net: 5.0, fees: 1.0 })]);
        assert_eq!(s.curve, vec![10.0, 10.0, 15.0]);
        assert_eq!(s.n, 2);
        assert_eq!(s.max_drawdown, 0.0);
        let real = summarize(&t, &t.iter().map(|x| Some(Sim { net: x.net, fees: x.fees })).collect::<Vec<_>>());
        assert_eq!(real.max_drawdown, 20.0);
        assert_eq!(real.worst_day, Some(-20.0));
    }

    #[test]
    fn size_scales_net_and_fees() {
        let t = book(&[10.0, -4.0]);
        let (steps, detail) = size(&t, &SizeParams { factor: 2.0, daily_loss_cap: None });
        assert_eq!(steps[0].sim, Some(Sim { net: 20.0, fees: 2.0 }));
        assert_eq!(steps[1].sim, Some(Sim { net: -8.0, fees: 2.0 }));
        assert_eq!(detail, Detail::Size { factor: 2.0, daily_loss_cap: None, days_capped: 0, trades_skipped: 0 });
    }

    #[test]
    fn size_cap_stops_the_day_after_the_loss_and_not_before() {
        // Three trades on one day: a loss that reaches the cap, then two more entries.
        let day = datetime!(2026-03-02 09:00 UTC);
        let mut a = trade(-60.0, 0.0, day + Duration::hours(1));
        a.entry_local = Some(day);
        let mut b = trade(30.0, 0.0, day + Duration::hours(3));
        b.entry_local = Some(day + Duration::hours(2));
        // Opened before the loss closed: it runs.
        let mut c = trade(5.0, 0.0, day + Duration::hours(2));
        c.entry_local = Some(day + Duration::minutes(30));
        let mut t = vec![a, c, b];
        t.sort_by_key(|x| x.exit_local);
        let (steps, detail) = size(&t, &SizeParams { factor: 1.5, daily_loss_cap: Some(80.0) });
        // -60 x 1.5 = -90 reaches the 80 cap at 10:00; the 11:00 entry is skipped.
        let skipped: Vec<_> = steps.iter().map(|s| s.note).collect();
        assert_eq!(skipped, vec![None, None, Some("capped")]);
        assert_eq!(detail, Detail::Size { factor: 1.5, daily_loss_cap: Some(80.0), days_capped: 1, trades_skipped: 1 });
    }

    #[test]
    fn reverse_flips_the_gross_and_pays_the_fees_again() {
        let t = book(&[9.0, -11.0]); // gross 10 and -10, fees 1 each
        let (steps, detail) = reverse(&t, &[true, false]);
        assert_eq!(steps[0].sim, Some(Sim { net: -11.0, fees: 1.0 }));
        assert_eq!(steps[1].sim, Some(Sim { net: -11.0, fees: 1.0 }));
        assert_eq!(detail, Detail::Reverse { flipped: 1 });
        let t = book(&[-11.0]);
        let (steps, _) = reverse(&t, &[true]);
        assert_eq!(steps[0].sim.unwrap().net, 9.0);
    }

    #[test]
    fn window_reads_the_local_entry_and_exclude_flips_it() {
        // Monday 2026-01-05 09:00 entry.
        let t = trade(1.0, 0.0, datetime!(2026-01-05 10:00 UTC));
        let w = TimeWindow { hours: vec![9], weekdays: vec![1], exclude: false };
        assert_eq!(window_keeps(&t, &w), Some(true));
        assert_eq!(window_keeps(&t, &TimeWindow { exclude: true, ..w.clone() }), Some(false));
        assert_eq!(window_keeps(&t, &TimeWindow { hours: vec![14], ..w.clone() }), Some(false));
        let mut undated = t.clone();
        undated.entry_local = None;
        assert_eq!(window_keeps(&undated, &w), None);
    }

    #[test]
    fn filter_removes_and_counts() {
        let t = book(&[10.0, -20.0, 5.0]);
        let (steps, detail) = filter(&t, &[true, false, true]);
        assert_eq!(detail, Detail::Filter { kept: 2, removed: 1 });
        assert_eq!(steps[1].note, Some("removed"));
        let o = assemble(&t, steps, detail, Scenario::Filter(FilterParams::default()), vec![], "USD");
        assert_eq!(o.delta_net, 20.0);
        assert_eq!(o.after.max_drawdown, 0.0);
        // Three trades are under the floor: nothing is asserted.
        assert!(o.statement.is_none());
    }

    #[test]
    fn unmeasured_trades_leave_both_sides_and_are_named() {
        let t = book(&[10.0, -20.0, 5.0]);
        let flags = vec![Ok(true), Err("no_bars"), Ok(false)];
        let ids: Vec<Uuid> = t.iter().map(|x| x.id).collect();
        let (trades, keep, unmeasured) = split_measured(&t, &[], flags);
        assert_eq!(trades.len(), 2);
        assert_eq!(keep, vec![true, false]);
        assert_eq!(unmeasured.len(), 1);
        assert_eq!((unmeasured[0].id, unmeasured[0].reason), (ids[1], "no_bars"));
        let (steps, detail) = filter(&trades, &keep);
        let o = assemble(&trades, steps, detail, Scenario::Filter(FilterParams::default()), unmeasured, "USD");
        assert_eq!(o.before.n, 2);
        assert_eq!(o.before.net, 15.0);
    }

    #[test]
    fn statement_fires_only_above_the_sample_floor() {
        let small = book(&vec![1.0; MIN_SAMPLE - 1]);
        let (steps, detail) = size(&small, &SizeParams { factor: 2.0, daily_loss_cap: None });
        let o = assemble(&small, steps, detail, Scenario::Size(SizeParams { factor: 2.0, daily_loss_cap: None }), vec![], "USD");
        assert!(o.statement.is_none());
        let big = book(&vec![1.0; MIN_SAMPLE]);
        let (steps, detail) = size(&big, &SizeParams { factor: 2.0, daily_loss_cap: None });
        let o = assemble(&big, steps, detail, Scenario::Size(SizeParams { factor: 2.0, daily_loss_cap: None }), vec![], "USD");
        let s = o.statement.expect("fires at the floor");
        assert_eq!(s.key, "size_better");
        assert_eq!(s.values["n"], MIN_SAMPLE as f64);
        assert_eq!(s.values["factor"], 2.0);
    }

    #[test]
    fn fees_without_family_price_the_whole_trade() {
        let mut t = book(&[100.0]);
        t[0].qty = 4.0;
        t[0].fees = 10.0;
        let s = schedule(1.5, "fixed", "contract", "USD");
        let (steps, detail) = fees(&t, &s, None, &[1.0], 0);
        let f = steps[0].fee.clone().unwrap();
        assert_eq!((f.paid, f.alternative, f.target_qty, f.remainder_qty), (10.0, 6.0, 4.0, 0.0));
        assert_eq!(steps[0].sim.unwrap().net, 104.0);
        match detail {
            Detail::Fees { saving, .. } => assert_eq!(saving, 4.0),
            _ => panic!("fees detail"),
        }
    }

    #[test]
    fn fees_family_substitution_shows_the_remainder() {
        let fam = family("mnq_nq").expect("mnq_nq is in the table");
        assert_eq!(fam.ratio, 10);
        let mut t = book(&[200.0]);
        t[0].ticker = "MNQZ6".into();
        t[0].asset_class = "future".into();
        t[0].qty = 25.0;
        t[0].fees = 25.0; // 1 per micro
        let mv = FamilyMove { family: fam, to: FamilySide::Large };
        assert!(mv.applies(&t[0]));
        // 2.5 per full-size contract, charged in EUR at 1.2 USD.
        let s = schedule(2.5, "fixed", "contract", "EUR");
        let (steps, detail) = fees(&t, &s, Some(mv), &[1.2], 3);
        let f = steps[0].fee.clone().unwrap();
        assert_eq!((f.target_qty, f.remainder_qty), (2.0, 5.0));
        // 2 NQ x 2.5 EUR x 1.2 = 6, plus the 5 micros left at their own 5.
        assert!((f.alternative - 11.0).abs() < 1e-9);
        assert_eq!(steps[0].note, Some("partial"));
        match detail {
            Detail::Fees { excluded, remainder_qty, saving, .. } => {
                assert_eq!((excluded, remainder_qty), (3, 5.0));
                assert!((saving - 14.0).abs() < 1e-9);
            }
            _ => panic!("fees detail"),
        }
        // Fewer micros than one full contract: nothing moves.
        t[0].qty = 7.0;
        let (steps, _) = fees(&t, &s, Some(mv), &[1.2], 0);
        assert_eq!(steps[0].note, Some("too_small"));
        assert!((steps[0].fee.as_ref().unwrap().alternative - 25.0).abs() < 1e-9);
        // Down to micros: always whole.
        assert_eq!(FamilyMove { family: fam, to: FamilySide::Small }.split(3.0), (30.0, 0.0));
    }

    #[test]
    fn family_roots_match_contracts_and_nothing_looser() {
        assert!(ticker_on_root("MNQ", "MNQ"));
        assert!(ticker_on_root("MNQZ6", "MNQ"));
        assert!(ticker_on_root("mnqz26", "MNQ"));
        assert!(ticker_on_root("/MNQZ2026", "MNQ"));
        assert!(ticker_on_root("MNQ1!", "MNQ"));
        assert!(ticker_on_root("MNQ DEC 26", "MNQ"));
        assert!(!ticker_on_root("MNQZ6", "NQ"));
        assert!(!ticker_on_root("MESZ6", "ES"));
        assert!(!ticker_on_root("ESX", "ES"));
        assert!(!ticker_on_root("BTCUSDT", "BTC"));
        // A crypto ticker is never a CME contract, even on the root.
        let mut t = book(&[1.0]).remove(0);
        t.ticker = "BTC".into();
        t.asset_class = "crypto".into();
        let fam = family("mbt_btc").unwrap();
        assert!(!FamilyMove { family: fam, to: FamilySide::Small }.applies(&t));
    }

    #[test]
    fn every_family_has_a_ratio_and_distinct_roots() {
        let f = families();
        assert_eq!(f.len(), 7);
        for x in f {
            assert!(x.ratio > 1, "{}", x.id);
            assert_ne!(x.small, x.large);
        }
        assert_eq!(family("mbt_btc").unwrap().ratio, 50);
    }

    fn bars(close: &[f64]) -> Vec<Bar> {
        let t0 = datetime!(2026-01-01 00:00 UTC);
        close
            .iter()
            .enumerate()
            .map(|(i, c)| Bar {
                ts: t0 + Duration::days(i as i64),
                open: *c,
                high: c + 1.0,
                low: c - 1.0,
                close: *c,
                volume: 100.0,
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            })
            .collect()
    }

    fn group(json: serde_json::Value) -> SignalGroup {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn signal_is_read_with_the_backtest_math_on_the_entry_bar() {
        let b = bars(&[10.0, 11.0, 12.0, 13.0, 9.0]);
        let g = group(serde_json::json!({
            "logic": "all",
            "conditions": [{ "left": { "kind": "price", "field": "close" }, "op": "above",
                             "right": { "kind": "indicator", "indicator": "sma", "period": 3 } }]
        }));
        // SMA(3) on bar 3 = 12, close 13: holds. Bar 4: SMA 11.33, close 9: does not.
        // Bars 0 and 1 have no SMA(3) yet: unknown, not "no".
        assert_eq!(holds_series(&g, &b), vec![None, None, Some(true), Some(true), Some(false)]);
    }

    #[test]
    fn an_indicator_reads_the_whole_series_across_weekends() {
        // 40 weekday-only daily candles, rising: no Saturday or Sunday in the series.
        let mut b = Vec::new();
        let mut day = datetime!(2026-01-05 00:00 UTC); // a Monday
        while b.len() < 40 {
            if day.weekday().number_from_monday() <= 5 {
                let mut x = bars(&[100.0 + b.len() as f64]).remove(0);
                x.ts = day;
                b.push(x);
            }
            day += Duration::days(1);
        }
        let g = group(serde_json::json!({
            "logic": "all",
            "conditions": [{ "left": { "kind": "price", "field": "close" }, "op": "above",
                             "right": { "kind": "indicator", "indicator": "sma", "period": 20 } }]
        }));
        let held = holds_series(&g, &b);
        let d = Duration::days(1);
        // A Monday entry after 30 candles (6 weeks, 42 calendar days): read on Friday's
        // candle, with the SMA(20) over the 20 candles before it, weekends skipped.
        let monday = b[30].ts + Duration::hours(15);
        assert_eq!(monday.weekday().number_from_monday(), 1);
        assert_eq!(read_at_entry(&b, &held, d, monday), Ok((true, 30)));
        // Before the 20th candle the SMA has no value: thin, not "no".
        assert_eq!(read_at_entry(&b, &held, d, b[10].ts + Duration::hours(15)), Err("thin"));
        // Before the first candle: thin.
        assert_eq!(read_at_entry(&b, &held, d, b[0].ts), Err("thin"));
    }

    #[test]
    fn settle_follows_the_chosen_indicator() {
        let ind = |json: serde_json::Value| -> Operand { serde_json::from_value(json).unwrap() };
        // A window indicator settles at its period.
        let sma = settle_bars(&ind(serde_json::json!({ "kind": "indicator", "indicator": "sma", "period": 50 }))).unwrap();
        assert!((50..=52).contains(&sma), "sma {sma}");
        // A recursive one needs several times it, and more the longer it is.
        let ema20 = settle_bars(&ind(serde_json::json!({ "kind": "indicator", "indicator": "ema", "period": 20 }))).unwrap();
        let ema200 = settle_bars(&ind(serde_json::json!({ "kind": "indicator", "indicator": "ema", "period": 200 }))).unwrap();
        let rsi = settle_bars(&ind(serde_json::json!({ "kind": "indicator", "indicator": "rsi", "period": 14 }))).unwrap();
        assert!(ema20 > 30 && ema200 > 300 && ema200 > ema20, "ema20 {ema20} ema200 {ema200}");
        assert!(rsi > 14, "rsi {rsi}");
        // Price itself needs nothing; a group takes its slowest operand.
        assert!(settle_bars(&ind(serde_json::json!({ "kind": "price", "field": "close" }))).unwrap() <= 1);
        let g = group(serde_json::json!({
            "logic": "all",
            "conditions": [{ "left": { "kind": "price", "field": "close" }, "op": "above",
                             "right": { "kind": "indicator", "indicator": "ema", "period": 200 } }]
        }));
        assert_eq!(group_settle(&g), Some(ema200));
    }

    #[test]
    fn validation_names_the_fix() {
        let bad = Scenario::Size(SizeParams { factor: 4.0, daily_loss_cap: None });
        assert!(validate(&bad).unwrap_err().contains("between 0.5 and 3"));
        let bad = Scenario::Size(SizeParams { factor: 1.0, daily_loss_cap: Some(-5.0) });
        assert!(validate(&bad).is_err());
        assert!(validate(&Scenario::Filter(FilterParams::default())).is_err());
        let w = |hours: Vec<u8>, weekdays: Vec<u8>| {
            Scenario::Filter(FilterParams { signal: None, window: Some(TimeWindow { hours, weekdays, exclude: false }) })
        };
        assert!(validate(&w(vec![24], vec![])).is_err());
        assert!(validate(&w(vec![], vec![0])).is_err());
        assert!(validate(&w(vec![], vec![])).is_err());
        assert!(validate(&w(vec![9], vec![1, 5])).is_ok());
        let custom = Scenario::Filter(FilterParams {
            signal: Some(group(serde_json::json!({
                "conditions": [{ "left": { "kind": "custom_indicator", "id": "x" }, "op": "rising" }]
            }))),
            window: None,
        });
        assert!(validate(&custom).unwrap_err().contains("built-in"));
        let no_right = Scenario::Filter(FilterParams {
            signal: Some(group(serde_json::json!({
                "conditions": [{ "left": { "kind": "price", "field": "close" }, "op": "above" }]
            }))),
            window: None,
        });
        assert!(validate(&no_right).unwrap_err().contains("right side"));
        let fam = Scenario::Fees(FeesParams { schedule_id: Uuid::nil(), family: Some("zz".into()), to: FamilySide::Large });
        assert!(validate(&fam).unwrap_err().contains("lab catalog"));
    }

    #[test]
    fn scenario_round_trips_as_tagged_json() {
        let s = Scenario::Filter(FilterParams {
            signal: None,
            window: Some(TimeWindow { hours: vec![15], weekdays: vec![], exclude: true }),
        });
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["kind"], "filter");
        assert_eq!(v["window"]["hours"][0], 15);
        let back: Scenario = serde_json::from_value(v).unwrap();
        assert_eq!(back.kind(), "filter");
        let e: Scenario = serde_json::from_value(serde_json::json!({ "kind": "exits" })).unwrap();
        assert_eq!(e.kind(), "exits");
    }

    #[test]
    fn card_sets_the_standard_scenarios_from_the_book() {
        let mut t = book(&[10.0, 20.0, -5.0, -6.0, -7.0, 30.0]);
        for (i, x) in t.iter_mut().enumerate() {
            x.mae_pct = Some(1.0 + i as f64 * 0.1);
            x.mfe_pct = Some(2.0 + i as f64);
        }
        // The three losers entered at 14:00 local.
        for x in t.iter_mut().filter(|x| x.net < 0.0) {
            x.entry_local = Some(x.entry_local.unwrap().replace_hour(14).unwrap());
        }
        let s = standard_scenarios(&t, Some(100.0));
        assert_eq!(s.iter().map(|x| x.key).collect::<Vec<_>>(), ["stop_mae", "target_mfe", "drop_hour", "size_up"]);
        // Winners' MAE: 1.0, 1.1, 1.5: median 1.1.
        assert!((s[0].values["pct"] - 1.1).abs() < 1e-9);
        assert_eq!(s[2].values["hour"], 14.0);
        assert_eq!(s[3].values["cap"], 100.0);
        let none = standard_scenarios(&book(&[1.0]), None);
        assert_eq!((none[0].missing, none[1].missing, none[2].missing), (Some("no_mae"), Some("no_mfe"), Some("no_losing_hour")));
        let c = card_stats(&t);
        assert_eq!(c.r_n, 6);
        assert_eq!(c.by_hour.iter().find(|b| b.key == "14").unwrap().trades, 3);
        assert_eq!(c.r_dist.iter().map(|b| b.trades).sum::<usize>(), 6);
    }
}
