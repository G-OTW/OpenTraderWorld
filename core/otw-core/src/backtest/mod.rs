//! Backtest engine: runs a signal-combination strategy over stored OHLCV bars.
//!
//! Strategy model (v2): each side (long/short) has an **entry group** — one or more signal
//! conditions combined with ALL (and) or ANY (or) — and an optional **exit group** of the
//! same shape. Entries fill at the next bar's open after the group fires (no lookahead).
//! Optional **stop-loss** / **take-profit** (percent from the average entry), exit when the
//! entry group stops holding (`exit_on_reverse`), and **pyramiding** (up to N stacked
//! entries per position; SL/TP track the volume-weighted average entry). Position sizing is
//! percent-of-equity or fixed quantity per entry. Fees, spread and leverage are modeled.
//! The simulation is deterministic and stateless — settings in, result out.
//!
//! v1 settings (a single `signal` per side) still deserialize: the engine folds the legacy
//! field into a one-condition entry group so old saved runs rerun unchanged.

mod custom;
mod indicators;
pub mod report;

pub use custom::CustomIndicatorDef;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Engine semantics version. Stamped into every run result and persisted on save so that
/// when the simulation's meaning changes later, old saved runs stay explainable. **Bump this
/// whenever a change alters the trades/equity a given settings JSON produces** (the golden
/// tests are the tripwire that tells you a bump is due).
pub const ENGINE_VERSION: u32 = 1;

/// A price/indicator series a signal can reference.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operand {
    /// A raw OHLCV series: "open" | "high" | "low" | "close" | "volume".
    Price { field: String },
    /// A constant value (e.g. RSI 70).
    Const { value: f64 },
    /// An indicator with params (unused params default to 0 and are ignored).
    Indicator {
        indicator: String,
        #[serde(default)]
        period: usize,
        #[serde(default)]
        fast: usize,
        #[serde(default)]
        slow: usize,
        /// Band/step multiplier (Bollinger, Keltner, SuperTrend, PSAR step).
        #[serde(default)]
        mult: f64,
        /// Signal/smoothing length (MACD signal, stochastic smoothing).
        #[serde(default)]
        signal_period: usize,
    },
    /// A saved custom indicator, referenced by id. Its node-graph definition is looked up in
    /// the run's embedded `indicators` map and evaluated by the DAG evaluator (see `custom`).
    CustomIndicator { id: String },
}

/// Embedded custom-indicator definitions for a run, keyed by indicator id. Strategy settings
/// carry the defs they use so a saved run never changes meaning when the library is edited.
pub type IndicatorDefs = std::collections::BTreeMap<String, custom::CustomIndicatorDef>;

/// The comparison a signal makes between a `left` and `right` operand.
/// `cross` = left crosses above OR below; directionless crosses use `crosses_above`/below.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Above,
    Below,
    CrossesAbove,
    CrossesBelow,
    Cross,
    Rising,
    Falling,
    ClosingAbove,
    ClosingBelow,
    OpeningAbove,
    OpeningBelow,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Signal {
    pub left: Operand,
    pub op: Op,
    /// Right operand. Optional for unary ops (rising/falling on `left`).
    #[serde(default)]
    pub right: Option<Operand>,
}

/// A combination of signal conditions: ALL must hold ("all") or ANY may hold ("any").
#[derive(Debug, Clone, Deserialize)]
pub struct SignalGroup {
    #[serde(default = "default_logic")]
    pub logic: String,
    #[serde(default)]
    pub conditions: Vec<Signal>,
}
fn default_logic() -> String {
    "all".into()
}

/// One row of an equity-tier table: at/above this equity, use `value`.
#[derive(Debug, Clone, Deserialize)]
pub struct Tier {
    pub above: f64,
    pub value: f64,
}

/// How to size each entry (each pyramiding add is sized independently by the same rule).
/// Sizing is evaluated against **portfolio equity** at entry time.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Sizing {
    /// Percent of current equity put into the position notional.
    PercentEquity { percent: f64 },
    /// Fixed quantity / lots / contracts (units of the instrument).
    FixedQty { qty: f64 },
    /// Risk a fixed % of equity per trade: size so `(entry − SL) × qty = risk_pct% of equity`.
    /// Requires an active stop-loss on the side (validated before the run).
    Risk { risk_pct: f64 },
    /// Equity-tier step table. `metric` selects what `value` means: "qty" (fixed lots),
    /// "risk_pct" (per-trade risk %, needs an SL) or "percent_equity" (% of equity notional).
    /// Highest `above ≤ equity` wins; tiers must be strictly increasing (validated).
    EquityTiers {
        #[serde(default = "default_tier_metric")]
        metric: String,
        #[serde(default)]
        tiers: Vec<Tier>,
    },
    /// Fractional Kelly from the run's own closed trades. Rolling window of the last `window`
    /// closed trades → win rate `p` and payoff `b`; `kelly = p − (1−p)/b`; sized as
    /// `fraction × kelly × equity` notional, capped at `cap_pct` of equity, floored at 0
    /// (negative Kelly ⇒ skip the entry). Until `window` trades exist, `warmup` sizing is used.
    Kelly {
        #[serde(default = "default_kelly_fraction")]
        fraction: f64,
        #[serde(default = "default_kelly_window")]
        window: usize,
        #[serde(default = "default_kelly_cap")]
        cap_pct: f64,
        #[serde(default)]
        warmup: Option<Box<Sizing>>,
    },
}
fn default_tier_metric() -> String {
    "qty".into()
}
fn default_kelly_fraction() -> f64 {
    0.5
}
fn default_kelly_window() -> usize {
    30
}
fn default_kelly_cap() -> f64 {
    20.0
}

/// Money-management / portfolio limit layer. Every field optional; unset = today's behavior.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Risk {
    /// Max simultaneously-open positions across all assets (0/None = unlimited).
    #[serde(default)]
    pub max_open_positions: Option<usize>,
    /// Max total open notional as a % of current equity (0/None = unlimited).
    #[serde(default)]
    pub max_exposure_pct: Option<f64>,
    /// Max open notional per single asset as a % of current equity (0/None = unlimited).
    #[serde(default)]
    pub max_exposure_per_asset_pct: Option<f64>,
    /// Circuit breaker: halt new entries for the rest of a calendar day once that day's
    /// realized+unrealized loss reaches this % of the day-start equity (0/None = off).
    #[serde(default)]
    pub max_daily_loss_pct: Option<f64>,
    /// Circuit breaker: halt new entries once portfolio drawdown from peak reaches this %
    /// (0/None = off). Existing positions still manage their own exits.
    #[serde(default)]
    pub max_drawdown_pct: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Fees {
    /// "fixed" | "pct"
    #[serde(default = "default_fee_kind")]
    pub amount_kind: String,
    /// "trade" | "unit"
    #[serde(default = "default_fee_per")]
    pub per: String,
    #[serde(default)]
    pub amount: f64,
}
fn default_fee_kind() -> String {
    "pct".into()
}
fn default_fee_per() -> String {
    "trade".into()
}
impl Default for Fees {
    fn default() -> Self {
        Fees { amount_kind: default_fee_kind(), per: default_fee_per(), amount: 0.0 }
    }
}

/// Which sides the strategy may take. `Both` lets long and short positions open from their
/// own entry groups (still one direction at a time).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Long,
    Short,
    Both,
}

/// A stop-loss / take-profit distance rule. `pct` = fraction of the average entry price (today's
/// behavior); `atr` = a multiple of ATR(`period`) sampled at the position's first-entry bar, held
/// as an absolute price distance and re-anchored to the average entry when pyramiding adds.
#[derive(Debug, Clone, Deserialize)]
pub struct Stop {
    /// "pct" | "atr"
    #[serde(default = "default_stop_kind")]
    pub kind: String,
    /// Fraction (pct) or ATR multiple (atr).
    #[serde(default)]
    pub value: f64,
    /// ATR lookback (atr kind only; defaults to 14).
    #[serde(default)]
    pub period: usize,
}
fn default_stop_kind() -> String {
    "pct".into()
}
impl Stop {
    fn is_atr(&self) -> bool {
        self.kind == "atr"
    }
}

/// Per-side configuration. A side is only consulted when the `Mode` enables it.
#[derive(Debug, Clone, Deserialize)]
pub struct Side {
    /// Legacy v1 single entry signal — folded into `entry` when present.
    #[serde(default)]
    pub signal: Option<Signal>,
    /// Entry condition group (v2).
    #[serde(default)]
    pub entry: Option<SignalGroup>,
    /// Explicit exit condition group; fires like an entry (exit at next bar open).
    #[serde(default)]
    pub exit: Option<SignalGroup>,
    /// Stop-loss as a fraction (0.02 = 2%) from the average entry; 0/absent disables.
    #[serde(default)]
    pub stop_loss_pct: f64,
    /// Take-profit as a fraction from the average entry; 0/absent disables.
    #[serde(default)]
    pub take_profit_pct: f64,
    /// v2 stop-loss rule (pct or ATR). When present, overrides `stop_loss_pct`.
    #[serde(default)]
    pub stop_loss: Option<Stop>,
    /// v2 take-profit rule (pct or ATR). When present, overrides `take_profit_pct`.
    #[serde(default)]
    pub take_profit: Option<Stop>,
    /// Exit (at close) when this side's entry group no longer holds.
    #[serde(default)]
    pub exit_on_reverse: bool,
}

impl Side {
    /// Effective stop-loss rule: the object form if present, else the legacy `stop_loss_pct`.
    fn sl_rule(&self) -> Option<Stop> {
        if let Some(s) = &self.stop_loss {
            return Some(s.clone());
        }
        (self.stop_loss_pct > 0.0).then(|| Stop { kind: "pct".into(), value: self.stop_loss_pct, period: 0 })
    }
    /// Effective take-profit rule: the object form if present, else the legacy `take_profit_pct`.
    fn tp_rule(&self) -> Option<Stop> {
        if let Some(t) = &self.take_profit {
            return Some(t.clone());
        }
        (self.take_profit_pct > 0.0).then(|| Stop { kind: "pct".into(), value: self.take_profit_pct, period: 0 })
    }
    /// Whether this side has *any* stop-loss configured (for risk-sizing validation).
    fn has_stop(&self) -> bool {
        self.sl_rule().is_some()
    }
}

impl Side {
    /// The effective entry group, folding a legacy single `signal` in.
    fn entry_group(&self) -> Option<SignalGroup> {
        if let Some(g) = &self.entry {
            if !g.conditions.is_empty() {
                return Some(g.clone());
            }
        }
        self.signal
            .as_ref()
            .map(|s| SignalGroup { logic: "all".into(), conditions: vec![s.clone()] })
    }
}

/// Fine pyramiding controls (each add is still sized by the base `Sizing`, then scaled).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PyramidSteps {
    /// Per-add size factors, e.g. [1.0, 0.5, 0.25] (add k multiplies its base size by scale[k];
    /// beyond the list length the last factor repeats). Empty = every add at full base size.
    #[serde(default)]
    pub scale: Vec<f64>,
    /// Minimum favorable move since the last add before another add may fill, as a fraction of
    /// price (>0) — prevents same-signal stacking on consecutive bars. 0 = no gate.
    #[serde(default)]
    pub min_distance_pct: f64,
    /// What happens to the stop when an add fills: "none" | "breakeven" | "trail_avg".
    #[serde(default = "default_after_add_sl")]
    pub after_add_sl: String,
}
fn default_after_add_sl() -> String {
    "none".into()
}

/// Instrument profile — lot/contract handling. Sizing in bare "qty" is ambiguous for
/// futures/forex; this rounds and gates it and applies a contract multiplier to PnL.
#[derive(Debug, Clone, Deserialize)]
pub struct Instrument {
    /// Contract point value / PnL multiplier (1 = spot-like).
    #[serde(default = "default_one")]
    pub multiplier: f64,
    /// Quantity is rounded down to a multiple of this step (0 = no rounding).
    #[serde(default)]
    pub lot_step: f64,
    /// Entries below this rounded size are refused and counted (0 = no minimum).
    #[serde(default)]
    pub min_qty: f64,
}
fn default_one() -> f64 {
    1.0
}
impl Default for Instrument {
    fn default() -> Self {
        Instrument { multiplier: 1.0, lot_step: 0.0, min_qty: 0.0 }
    }
}
impl Instrument {
    /// Round a raw quantity down to `lot_step`; return None if it falls below `min_qty`.
    fn round_qty(&self, raw: f64) -> Option<f64> {
        let q = if self.lot_step > 0.0 { (raw / self.lot_step).floor() * self.lot_step } else { raw };
        if q <= 0.0 || (self.min_qty > 0.0 && q < self.min_qty) {
            None
        } else {
            Some(q)
        }
    }
}

/// Fill-realism slippage applied on top of spread, worsening every fill price.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Slippage {
    /// "pct" (fraction of price) | "ticks" (value × tick_size)
    #[serde(default = "default_slip_kind")]
    pub kind: String,
    #[serde(default)]
    pub value: f64,
    /// Tick size for "ticks" mode.
    #[serde(default)]
    pub tick_size: f64,
}
fn default_slip_kind() -> String {
    "pct".into()
}
impl Slippage {
    /// Absolute price slippage for a fill at `px` (always ≥ 0).
    fn amount(&self, px: f64) -> f64 {
        if self.value <= 0.0 {
            return 0.0;
        }
        match self.kind.as_str() {
            "ticks" => self.value * self.tick_size,
            _ => px.abs() * self.value,
        }
    }
}

/// Perpetual-funding estimate. A constant annualized rate charged on open notional at each
/// funding interval — longs pay a positive rate, shorts receive. A deliberately simple,
/// clearly-*estimated* model (historical per-exchange series are a separate data project).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Funding {
    /// Annual funding rate in percent (e.g. 10.95 ≈ 0.01%/8h). 0 = off.
    #[serde(default)]
    pub annual_rate_pct: f64,
    /// Funding interval in hours (default 8, the common perp cadence). The annual rate is
    /// quoted per this interval, then accrued by the fraction of an interval each bar spans.
    #[serde(default = "default_funding_interval")]
    pub interval_hours: f64,
}
fn default_funding_interval() -> f64 {
    8.0
}

/// Grid-trading configuration. A ladder of `levels` evenly spaced in `[lower, upper]`; each
/// gap between adjacent levels is a buy-low/sell-high round trip. Filled on bar high/low
/// crossings (not signals). A dedicated strategy kind — the signal builder is not involved.
#[derive(Debug, Clone, Deserialize)]
pub struct GridConfig {
    pub lower: f64,
    pub upper: f64,
    /// Number of grid lines (≥ 2). `levels - 1` round-trip cells.
    #[serde(default = "default_grid_levels")]
    pub levels: usize,
    /// Fixed quantity traded per level. When 0, `total_budget` is split across the cells.
    #[serde(default)]
    pub qty_per_level: f64,
    /// Total capital to deploy across the grid (used when `qty_per_level` is 0).
    #[serde(default)]
    pub total_budget: f64,
    /// "long" (buy dips, sell rallies) | "short" (mirror) | "neutral" (both around mid).
    #[serde(default = "default_grid_direction")]
    pub direction: String,
    /// Liquidate everything and stop if price trades below this (0/absent = no floor).
    #[serde(default)]
    pub stop_below: f64,
    /// Liquidate everything and stop if price trades above this (0/absent = no ceiling).
    #[serde(default)]
    pub stop_above: f64,
}
fn default_grid_levels() -> usize {
    10
}
fn default_grid_direction() -> String {
    "long".into()
}

/// Full run configuration. Mirrors the settings JSON the frontend posts and stores.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// Strategy kind: "signals" (default, the signal-combination engine) | "grid".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Grid config, required when `kind == "grid"`.
    #[serde(default)]
    pub grid: Option<GridConfig>,
    pub mode: Mode,
    /// Long-side config (required for mode long/both).
    #[serde(default)]
    pub long: Option<Side>,
    /// Short-side config (required for mode short/both). When the UI's "reverse side" box is
    /// on, the frontend fills this with the inverse of `long` before posting.
    #[serde(default)]
    pub short: Option<Side>,
    /// When in a position and the *opposite* side's entry fires, close and immediately open
    /// the reverse position (stop-and-reverse). Only meaningful for `Mode::Both`.
    #[serde(default)]
    pub stop_and_reverse: bool,
    /// Max stacked entries per position (1 = no pyramiding). Re-fires of the entry group
    /// while in a position add another sized entry, up to this count.
    #[serde(default = "default_pyramiding")]
    pub pyramiding: usize,
    pub sizing: Sizing,
    #[serde(default = "default_capital")]
    pub starting_capital: f64,
    #[serde(default = "default_leverage")]
    pub leverage: f64,
    /// Spread as a fraction of price applied on entry and exit (half-spread each side).
    #[serde(default)]
    pub spread_pct: f64,
    #[serde(default)]
    pub fees: Fees,
    /// Portfolio money-management limits + circuit breakers. Unset = no limits.
    #[serde(default)]
    pub risk: Risk,
    /// Fine pyramiding controls (scale sequence, min-distance gate, after-add stop move).
    #[serde(default)]
    pub pyramid_steps: PyramidSteps,
    /// Instrument profile (lot rounding, min size, contract multiplier). Unset = spot-like.
    #[serde(default)]
    pub instrument: Instrument,
    /// Slippage model applied on top of spread. Unset = none.
    #[serde(default)]
    pub slippage: Slippage,
    /// In-sample / out-of-sample split: fraction (0–1) of bars in the *in-sample* head. When
    /// > 0, the result carries a second stat block computed on the out-of-sample tail.
    #[serde(default)]
    pub oos_split_pct: f64,
    /// Custom-indicator definitions referenced by `Operand::CustomIndicator`, embedded so a
    /// saved run stays reproducible even after the indicator library changes.
    #[serde(default)]
    pub indicators: IndicatorDefs,
    /// Perpetual-funding estimate (crypto). Unset = no funding.
    #[serde(default)]
    pub funding: Funding,
}
fn default_kind() -> String {
    "signals".into()
}
fn default_capital() -> f64 {
    10_000.0
}
fn default_leverage() -> f64 {
    1.0
}
fn default_pyramiding() -> usize {
    1
}

impl Settings {
    fn allow_long(&self) -> bool {
        matches!(self.mode, Mode::Long | Mode::Both)
    }
    fn allow_short(&self) -> bool {
        matches!(self.mode, Mode::Short | Mode::Both)
    }

    /// Validation error string, or None if the settings are runnable. Cheap structural checks
    /// the API surfaces before simulating (risk sizing needs a stop; tiers must be increasing).
    pub fn validate(&self) -> Option<String> {
        if sizing_needs_stop(&self.sizing) {
            let sl_ok = |side: Option<&Side>| side.map(|s| s.has_stop()).unwrap_or(false);
            let needs = (self.allow_long() && !sl_ok(self.long.as_ref()))
                || (self.allow_short() && !sl_ok(self.short.as_ref()));
            if needs {
                return Some(
                    "risk-based sizing requires a stop-loss on every enabled side".into(),
                );
            }
        }
        if let Sizing::EquityTiers { tiers, .. } = &self.sizing {
            if tiers.is_empty() {
                return Some("equity-tier sizing needs at least one tier".into());
            }
            if tiers.windows(2).any(|w| w[1].above <= w[0].above) {
                return Some("equity tiers must be strictly increasing by `above`".into());
            }
        }
        if self.oos_split_pct != 0.0 && !(self.oos_split_pct > 0.0 && self.oos_split_pct < 1.0) {
            return Some("out-of-sample split must be between 0 and 1 (e.g. 0.7)".into());
        }
        // Every embedded custom-indicator definition must be structurally valid.
        for (id, def) in &self.indicators {
            if let Some(err) = def.validate() {
                return Some(format!("custom indicator '{id}': {err}"));
            }
        }
        None
    }
}

/// OHLCV input (parallel arrays mirror the histdata bars endpoint). One asset's dense series.
pub struct Bars<'a> {
    /// Asset label; populated for multi-asset runs, empty for the legacy single-asset call.
    pub ticker: &'a str,
    pub ts: &'a [String],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub close: &'a [f64],
    pub volume: &'a [f64],
}

#[derive(Debug, Serialize)]
pub struct Trade {
    /// Ticker of the asset this trade belongs to (empty for a single-asset run).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ticker: String,
    pub entry_ts: String,
    pub exit_ts: String,
    /// Volume-weighted average entry price across pyramided entries.
    pub entry_price: f64,
    pub exit_price: f64,
    pub qty: f64,
    /// Number of stacked entries that built the position (1 = no pyramiding).
    pub entries: usize,
    pub direction: String,
    /// What closed the trade: "signal" | "exit_signal" | "stop_loss" | "take_profit" |
    /// "reverse" | "end".
    pub exit_reason: String,
    pub pnl: f64,
    pub fees: f64,
    pub return_pct: f64,
    /// Bars the position was held (exit bar index − first entry bar index).
    pub bars_held: usize,
    /// Max adverse excursion: worst open loss (account currency, ≥ 0) reached while in-trade.
    pub mae: f64,
    /// Max favorable excursion: best open profit (account currency, ≥ 0) reached while in-trade.
    pub mfe: f64,
}

#[derive(Debug, Serialize)]
pub struct EquityPoint {
    pub ts: String,
    pub equity: f64,
}

/// Trade-derived metrics for one scope (all trades, longs only, shorts only).
#[derive(Debug, Serialize, Default)]
pub struct SideStats {
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub profit_factor: f64,
    pub total_fees: f64,
    pub avg_trade: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    /// avg_win / avg_loss (0 when no losses).
    pub payoff_ratio: f64,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub max_consec_wins: usize,
    pub max_consec_losses: usize,
    pub avg_bars_held: f64,
    /// Mean per-trade return on position notional, in percent (expectancy).
    pub expectancy_pct: f64,
}

fn side_stats<'a>(trades: impl Iterator<Item = &'a Trade>) -> SideStats {
    let mut s = SideStats::default();
    let (mut streak_w, mut streak_l, mut bars, mut ret_sum) = (0usize, 0usize, 0usize, 0.0);
    for t in trades {
        s.trades += 1;
        s.net_pnl += t.pnl;
        s.total_fees += t.fees;
        bars += t.bars_held;
        ret_sum += t.return_pct;
        if t.pnl >= 0.0 {
            s.wins += 1;
            s.gross_profit += t.pnl;
            s.largest_win = s.largest_win.max(t.pnl);
            streak_w += 1;
            streak_l = 0;
        } else {
            s.losses += 1;
            s.gross_loss += -t.pnl;
            s.largest_loss = s.largest_loss.max(-t.pnl);
            streak_l += 1;
            streak_w = 0;
        }
        s.max_consec_wins = s.max_consec_wins.max(streak_w);
        s.max_consec_losses = s.max_consec_losses.max(streak_l);
    }
    if s.trades > 0 {
        let n = s.trades as f64;
        s.win_rate = s.wins as f64 / n * 100.0;
        s.avg_trade = s.net_pnl / n;
        s.avg_bars_held = bars as f64 / n;
        s.expectancy_pct = ret_sum / n;
    }
    if s.wins > 0 {
        s.avg_win = s.gross_profit / s.wins as f64;
    }
    if s.losses > 0 {
        s.avg_loss = s.gross_loss / s.losses as f64;
    }
    if s.gross_loss > 0.0 {
        s.profit_factor = s.gross_profit / s.gross_loss;
    }
    if s.avg_loss > 0.0 {
        s.payoff_ratio = s.avg_win / s.avg_loss;
    }
    s
}

#[derive(Debug, Serialize, Default)]
pub struct Stats {
    /// Engine semantics version this result was produced under (see `ENGINE_VERSION`).
    pub engine_version: u32,
    // Flat headline fields (also kept for saved-run history compatibility).
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub return_pct: f64,
    pub total_fees: f64,
    pub max_drawdown_pct: f64,
    /// Max peak-to-trough equity drop in account currency.
    pub max_drawdown: f64,
    pub profit_factor: f64,
    pub avg_trade: f64,
    pub final_equity: f64,
    /// Return of buying at the first close and holding to the last, in percent.
    pub buy_hold_return_pct: f64,
    /// Annualized Sharpe on per-bar equity returns (None when not computable).
    pub sharpe: Option<f64>,
    pub sortino: Option<f64>,
    /// Mean per-trade return on notional, in percent.
    pub expectancy_pct: f64,
    /// Trade count per exit reason ("signal", "stop_loss", "take_profit", …).
    pub exit_reasons: BTreeMap<String, usize>,
    /// Scoped breakdowns for the All / Long / Short performance table.
    pub all: SideStats,
    pub long: SideStats,
    pub short: SideStats,
}

/// Annualized Sharpe + Sortino from the equity curve. Needs parseable RFC3339 timestamps
/// (to derive bars-per-year) and non-degenerate returns; otherwise None.
fn risk_ratios(equity: &[EquityPoint]) -> (Option<f64>, Option<f64>) {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    let n = equity.len();
    if n < 3 {
        return (None, None);
    }
    let span_secs = match (
        OffsetDateTime::parse(&equity[0].ts, &Rfc3339),
        OffsetDateTime::parse(&equity[n - 1].ts, &Rfc3339),
    ) {
        (Ok(a), Ok(z)) => (z - a).whole_seconds() as f64,
        _ => return (None, None),
    };
    if span_secs <= 0.0 {
        return (None, None);
    }
    let bars_per_year = (n - 1) as f64 / (span_secs / (365.25 * 24.0 * 3600.0));

    let rets: Vec<f64> = equity
        .windows(2)
        .filter(|w| w[0].equity > 0.0)
        .map(|w| w[1].equity / w[0].equity - 1.0)
        .collect();
    if rets.len() < 2 {
        return (None, None);
    }
    let m = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / rets.len() as f64;
    let dvar = rets.iter().map(|r| r.min(0.0).powi(2)).sum::<f64>() / rets.len() as f64;
    let ann = bars_per_year.sqrt();
    let sharpe = (var > 0.0).then(|| m / var.sqrt() * ann);
    let sortino = (dvar > 0.0).then(|| m / dvar.sqrt() * ann);
    (sharpe, sortino)
}

/// Per-asset breakdown inside a portfolio run.
#[derive(Debug, Serialize)]
pub struct AssetStats {
    pub ticker: String,
    pub trades: usize,
    pub wins: usize,
    pub win_rate: f64,
    pub net_pnl: f64,
    pub total_fees: f64,
    /// Fraction of the merged clock this asset spent in a position, in percent.
    pub exposure_pct: f64,
    /// Bars this asset contributed to the merged clock (its own bar count).
    pub bars: usize,
    /// Merged-clock rows where this asset had no bar (marked-to-market at last price).
    pub inactive_bars: usize,
}

/// Alignment preview for a (multi-asset) run — computed without simulating.
#[derive(Debug, Serialize)]
pub struct AssetAlignment {
    pub ticker: String,
    pub bars: usize,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// Merged-clock rows this asset is missing a bar for.
    pub inactive_bars: usize,
}

#[derive(Debug, Serialize)]
pub struct AlignmentReport {
    /// Length of the merged clock (sorted union of all assets' timestamps).
    pub clock_len: usize,
    /// Overlap window where *every* asset has a bar (the fully-aligned span), if any.
    pub overlap_from: Option<String>,
    pub overlap_to: Option<String>,
    /// Merged-clock rows inside the overlap window (all assets active).
    pub overlap_bars: usize,
    /// Warm-up bars the settings' indicators need before signals can be defined.
    pub warmup_bars: usize,
    pub assets: Vec<AssetAlignment>,
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub trades: Vec<Trade>,
    pub equity: Vec<EquityPoint>,
    pub stats: Stats,
    /// Per-asset breakdown (one entry even for a single-asset run).
    pub per_asset: Vec<AssetStats>,
    /// Warm-up bars before indicators are defined; effective trading start on the merged clock.
    pub warmup_bars: usize,
    pub trading_start_ts: Option<String>,
    /// Present for multi-asset runs (None for a single-asset run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<AlignmentReport>,
    /// Entries refused by the instrument profile (below min size after lot rounding).
    pub skipped_min_size: usize,
    /// Entries refused because required margin would exceed available equity.
    pub skipped_margin: usize,
    /// Merged-clock rows where new entries were halted by a circuit breaker.
    pub halted_bars: usize,
    /// Out-of-sample stat block (in-sample = `stats`), present only when `oos_split_pct > 0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos: Option<OosSplit>,
    /// Equal-weight buy-and-hold equity curve over the same clock and starting capital (one
    /// fee on the initial buy). Powers the results-chart benchmark overlay and the report.
    pub benchmark: Vec<EquityPoint>,
    /// Net funding paid (negative) or received (positive) over the run — estimate only.
    pub total_funding: f64,
    /// Grid-specific stats, present only for a grid-kind run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridStats>,
}

/// Grid-mode summary: fills, completed round trips, and leftover inventory at the end.
#[derive(Debug, Serialize, Default)]
pub struct GridStats {
    pub fills: usize,
    pub round_trips: usize,
    /// Units still held at the end (unrealized inventory).
    pub end_inventory: f64,
    /// Mark-to-market value of that inventory at the last close.
    pub end_inventory_value: f64,
    pub levels: usize,
}

/// Side-by-side in-sample / out-of-sample stat blocks for the overfitting check.
#[derive(Debug, Serialize)]
pub struct OosSplit {
    /// Fraction of bars in the in-sample head (0–1).
    pub split_pct: f64,
    pub in_sample: Stats,
    pub out_sample: Stats,
    /// Timestamp where out-of-sample begins.
    pub split_ts: Option<String>,
}

/// Resolve an operand to a per-bar series (None where undefined for that bar).
fn resolve(op: &Operand, b: &Bars, defs: &IndicatorDefs) -> Vec<Option<f64>> {
    match op {
        Operand::Const { value } => vec![Some(*value); b.close.len()],
        Operand::Price { field } => price_series(field, b),
        Operand::Indicator { indicator, period, fast, slow, mult, signal_period } => {
            resolve_builtin(indicator, *period, *fast, *slow, *mult, *signal_period, b)
        }
        Operand::CustomIndicator { id } => match defs.get(id) {
            Some(def) => custom::eval(def, b),
            None => vec![None; b.close.len()],
        },
    }
}

/// A raw OHLCV series by field name (defaults to close for unknown names).
fn price_series(field: &str, b: &Bars) -> Vec<Option<f64>> {
    let src = match field {
        "open" => b.open,
        "high" => b.high,
        "low" => b.low,
        "volume" => b.volume,
        _ => b.close,
    };
    src.iter().map(|&v| Some(v)).collect()
}

/// Resolve one built-in indicator by name + params into a per-bar series. Shared by
/// `Operand::Indicator` and the custom-indicator DAG's leaf nodes. 0/absent params default.
pub(crate) fn resolve_builtin(
    indicator: &str,
    period: usize,
    fast: usize,
    slow: usize,
    mult: f64,
    signal_period: usize,
    b: &Bars,
) -> Vec<Option<f64>> {
    use indicators as ind;
    let p = |d: usize| if period == 0 { d } else { period };
    let f = |d: usize| if fast == 0 { d } else { fast };
    let sl = |d: usize| if slow == 0 { d } else { slow };
    let m = |d: f64| if mult <= 0.0 { d } else { mult };
    let sp = |d: usize| if signal_period == 0 { d } else { signal_period };
    match indicator {
        "sma" => ind::sma(b.close, p(20)),
        "ema" => ind::ema(b.close, p(20)),
        "dema" => ind::dema(b.close, p(20)),
        "tema" => ind::tema(b.close, p(20)),
        "wma" => ind::wma(b.close, p(20)),
        "hma" => ind::hma(b.close, p(20)),
        "vwap" => ind::vwap(b.high, b.low, b.close, b.volume, p(20)),
        "rsi" => ind::rsi(b.close, p(14)),
        "stoch_k" => ind::stoch_k(b.high, b.low, b.close, p(14), sp(3)),
        "stoch_d" => ind::stoch_d(b.high, b.low, b.close, p(14), sp(3)),
        "cci" => ind::cci(b.high, b.low, b.close, p(20)),
        "willr" => ind::willr(b.high, b.low, b.close, p(14)),
        "roc" => ind::roc(b.close, p(12)),
        "momentum" => ind::momentum(b.close, p(10)),
        "macd" => ind::macd_line(b.close, f(12), sl(26)),
        "macd_signal" => ind::macd_signal(b.close, f(12), sl(26), sp(9)),
        "macd_hist" => ind::macd_hist(b.close, f(12), sl(26), sp(9)),
        "adx" => ind::adx(b.high, b.low, b.close, p(14)),
        "mfi" => ind::mfi(b.high, b.low, b.close, b.volume, p(14)),
        "obv" => ind::obv(b.close, b.volume),
        "atr" => ind::atr(b.high, b.low, b.close, p(14)),
        "stddev" => ind::stddev(b.close, p(20)),
        "bb_upper" => ind::bollinger(b.close, p(20), m(2.0), 1),
        "bb_mid" => ind::bollinger(b.close, p(20), m(2.0), 0),
        "bb_lower" => ind::bollinger(b.close, p(20), m(2.0), -1),
        "keltner_upper" => ind::keltner(b.high, b.low, b.close, p(20), m(2.0), 1),
        "keltner_lower" => ind::keltner(b.high, b.low, b.close, p(20), m(2.0), -1),
        "donchian_upper" => ind::donchian(b.high, b.low, p(20), 1),
        "donchian_mid" => ind::donchian(b.high, b.low, p(20), 0),
        "donchian_lower" => ind::donchian(b.high, b.low, p(20), -1),
        "supertrend" => ind::supertrend(b.high, b.low, b.close, p(10), m(3.0)),
        "psar" => ind::psar(b.high, b.low, m(0.02)),
        _ => vec![None; b.close.len()],
    }
}

/// Evaluate one signal into a per-bar boolean (true = condition holds at bar i).
fn eval_signal(s: &Signal, b: &Bars, defs: &IndicatorDefs) -> Vec<bool> {
    let n = b.close.len();
    let left = resolve(&s.left, b, defs);
    let right = s.right.as_ref().map(|r| resolve(r, b, defs));
    let mut out = vec![false; n];
    for i in 0..n {
        let l = left[i];
        let r = right.as_ref().and_then(|rr| rr[i]);
        let lp = if i > 0 { left[i - 1] } else { None };
        let rp = if i > 0 { right.as_ref().and_then(|rr| rr[i - 1]) } else { None };
        out[i] = match s.op {
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
                (Some(lp), Some(rp), Some(l), Some(r)) if (lp - rp).signum() != (l - r).signum() && (lp - rp) != 0.0
            ),
            Op::Rising => matches!((lp, l), (Some(lp), Some(l)) if l > lp),
            Op::Falling => matches!((lp, l), (Some(lp), Some(l)) if l < lp),
            Op::ClosingAbove => matches!(r, Some(r) if b.close[i] > r),
            Op::ClosingBelow => matches!(r, Some(r) if b.close[i] < r),
            Op::OpeningAbove => matches!(r, Some(r) if b.open[i] > r),
            Op::OpeningBelow => matches!(r, Some(r) if b.open[i] < r),
        };
    }
    out
}

/// Evaluate a condition group: AND ("all") or OR ("any") of its conditions per bar.
/// An empty group never fires.
fn eval_group(g: &SignalGroup, b: &Bars, defs: &IndicatorDefs) -> Vec<bool> {
    let n = b.close.len();
    if g.conditions.is_empty() {
        return vec![false; n];
    }
    let per: Vec<Vec<bool>> = g.conditions.iter().map(|s| eval_signal(s, b, defs)).collect();
    let any = g.logic == "any";
    (0..n)
        .map(|i| {
            if any {
                per.iter().any(|v| v[i])
            } else {
                per.iter().all(|v| v[i])
            }
        })
        .collect()
}

fn fee_for(f: &Fees, qty: f64, price: f64, mult: f64) -> f64 {
    let notional = qty.abs() * price.abs() * mult;
    match (f.per.as_str(), f.amount_kind.as_str()) {
        ("trade", "pct") => notional * f.amount / 100.0,
        ("trade", _) => f.amount,
        (_, "pct") => notional * f.amount / 100.0,
        (_, _) => f.amount * qty.abs(),
    }
}

/// One stacked entry inside a live position.
struct Lot {
    price: f64,
    qty: f64,
    fee: f64,
}

/// Live position state during the simulation (one per asset that is currently in a trade).
struct Pos {
    long: bool,
    lots: Vec<Lot>,
    /// This asset's own bar index at first entry (for entry_ts + bars_held, asset-local).
    entry_bar: usize,
    /// Asset bar index of the latest add — guards against double-adding on one bar.
    last_add_bar: usize,
    /// ATR value sampled at the first-entry bar (for ATR-based SL/TP distances; None for pct).
    atr_entry: Option<f64>,
    /// Favorable price extreme reached while in-trade (for min-distance pyramiding).
    best_price: f64,
    /// Max adverse / favorable excursion in account currency (MAE/MFE), gross of exit fee.
    mae: f64,
    mfe: f64,
    /// Once an add has moved the stop to breakeven / trail (after_add_sl), the override price.
    stop_override: Option<f64>,
}

impl Pos {
    /// Fresh single-lot position.
    fn new(long: bool, lot: Lot, entry_bar: usize, atr_entry: Option<f64>, first_px: f64) -> Pos {
        Pos {
            long,
            lots: vec![lot],
            entry_bar,
            last_add_bar: entry_bar,
            atr_entry,
            best_price: first_px,
            mae: 0.0,
            mfe: 0.0,
            stop_override: None,
        }
    }
}

impl Pos {
    fn qty(&self) -> f64 {
        self.lots.iter().map(|l| l.qty).sum()
    }
    fn avg_price(&self) -> f64 {
        let q = self.qty();
        if q > 0.0 {
            self.lots.iter().map(|l| l.price * l.qty).sum::<f64>() / q
        } else {
            0.0
        }
    }
    fn entry_fees(&self) -> f64 {
        self.lots.iter().map(|l| l.fee).sum()
    }
    /// Unrealized PnL (gross) at `px`, scaled by the contract multiplier.
    fn unrealized(&self, px: f64, mult: f64) -> f64 {
        let d = if self.long { 1.0 } else { -1.0 };
        self.lots.iter().map(|l| d * l.qty * (px - l.price) * mult).sum()
    }
    /// Open notional at `px` (|qty| × price × multiplier) — for exposure limits and margin.
    fn notional(&self, px: f64, mult: f64) -> f64 {
        self.qty().abs() * px.abs() * mult
    }
}

/// Per-side precomputed signals for the run loop.
struct SideSignals {
    entry: Option<Vec<bool>>,
    exit: Option<Vec<bool>>,
    sl: Option<Stop>,
    tp: Option<Stop>,
    /// ATR series for the SL/TP period, precomputed once when either rule is ATR-based.
    atr: Option<Vec<Option<f64>>>,
    exit_on_reverse: bool,
}

fn side_signals(side: Option<&Side>, enabled: bool, b: &Bars, defs: &IndicatorDefs) -> SideSignals {
    let side = side.filter(|_| enabled);
    let sl = side.and_then(|sd| sd.sl_rule());
    let tp = side.and_then(|sd| sd.tp_rule());
    // If any stop is ATR-based, precompute ATR at its period (SL wins the period if both set).
    let atr_period = [sl.as_ref(), tp.as_ref()]
        .into_iter()
        .flatten()
        .filter(|st| st.is_atr())
        .map(|st| if st.period == 0 { 14 } else { st.period })
        .max();
    let atr = atr_period.map(|p| indicators::atr(b.high, b.low, b.close, p));
    SideSignals {
        entry: side.and_then(|sd| sd.entry_group()).map(|g| eval_group(&g, b, defs)),
        exit: side
            .and_then(|sd| sd.exit.as_ref())
            .filter(|g| !g.conditions.is_empty())
            .map(|g| eval_group(g, b, defs)),
        sl,
        tp,
        atr,
        exit_on_reverse: side.map(|x| x.exit_on_reverse).unwrap_or(false),
    }
}

/// Resolve a stop rule to an **absolute price distance** from the average entry. For `pct` the
/// distance scales with `avg` (so it re-anchors when pyramiding moves the average); for `atr` it
/// is the ATR sampled at the position's first-entry bar times the multiple (fixed distance).
fn stop_distance(rule: &Stop, avg: f64, atr_entry: Option<f64>) -> Option<f64> {
    if rule.value <= 0.0 {
        return None;
    }
    if rule.is_atr() {
        atr_entry.filter(|a| *a > 0.0).map(|a| a * rule.value)
    } else {
        Some(avg * rule.value)
    }
}

/// One asset's precomputed state, indexed into the shared merged clock.
struct Asset<'a> {
    b: &'a Bars<'a>,
    long: SideSignals,
    short: SideSignals,
    /// Merged-clock row → this asset's bar index (None where the asset has no bar).
    bar_at_row: Vec<Option<usize>>,
    pos: Option<Pos>,
    /// Rows this asset held a position (for exposure %).
    active_pos_rows: usize,
    /// Last known close (carried forward across missing bars for mark-to-market).
    last_close: f64,
}

/// A closed-trade summary the sizing layer reads (Kelly window).
struct ClosedTrade {
    /// Per-trade return on notional, as a fraction (net_pnl / entry notional).
    ret: f64,
    win: bool,
}

/// Ambient context a sizing rule reads to compute a per-lot quantity.
struct SizeCtx<'a> {
    equity: f64,
    entry_px: f64,
    /// Stop-loss price for this side (Some only when an SL is configured).
    stop_px: Option<f64>,
    leverage: f64,
    closed: &'a [ClosedTrade],
}

/// Resolve the per-entry quantity for a sizing mode. Returns 0 to skip the entry. `sizing` is
/// evaluated against portfolio `equity`; risk modes need `stop_px` (validated up-front).
fn resolve_qty(sizing: &Sizing, c: &SizeCtx) -> f64 {
    match sizing {
        Sizing::PercentEquity { percent } => {
            let notional = c.equity * (percent / 100.0) * c.leverage;
            if c.entry_px > 0.0 { notional / c.entry_px } else { 0.0 }
        }
        Sizing::FixedQty { qty } => *qty,
        Sizing::Risk { risk_pct } => qty_for_risk(*risk_pct, c),
        Sizing::EquityTiers { metric, tiers } => {
            // Highest `above ≤ equity` wins; empty/none-matched ⇒ skip.
            let val = tiers
                .iter()
                .filter(|t| c.equity >= t.above)
                .max_by(|a, b| a.above.partial_cmp(&b.above).unwrap())
                .map(|t| t.value);
            let Some(val) = val else { return 0.0 };
            match metric.as_str() {
                "risk_pct" => qty_for_risk(val, c),
                "percent_equity" => {
                    let notional = c.equity * (val / 100.0) * c.leverage;
                    if c.entry_px > 0.0 { notional / c.entry_px } else { 0.0 }
                }
                _ => val, // "qty" (default): fixed lots
            }
        }
        Sizing::Kelly { fraction, window, cap_pct, warmup } => {
            if c.closed.len() < *window {
                // Warm-up: use the fallback sizing (default 2%-of-equity notional).
                let fallback = warmup.as_deref().cloned().unwrap_or(Sizing::PercentEquity { percent: 2.0 });
                return resolve_qty(&fallback, c);
            }
            let recent = &c.closed[c.closed.len() - window..];
            let wins = recent.iter().filter(|t| t.win).count();
            let p = wins as f64 / recent.len() as f64;
            let avg_win: f64 = {
                let ws: Vec<f64> = recent.iter().filter(|t| t.win).map(|t| t.ret).collect();
                if ws.is_empty() { 0.0 } else { ws.iter().sum::<f64>() / ws.len() as f64 }
            };
            let avg_loss: f64 = {
                let ls: Vec<f64> = recent.iter().filter(|t| !t.win).map(|t| -t.ret).collect();
                if ls.is_empty() { 0.0 } else { ls.iter().sum::<f64>() / ls.len() as f64 }
            };
            // Payoff ratio b; with no losses treat as very favorable (cap catches it anyway).
            let b = if avg_loss > 0.0 { avg_win / avg_loss } else { f64::INFINITY };
            let kelly = if b.is_finite() { p - (1.0 - p) / b } else { p };
            let frac = (fraction * kelly).clamp(0.0, cap_pct / 100.0);
            if frac <= 0.0 {
                return 0.0; // negative/zero Kelly ⇒ skip the entry
            }
            let notional = c.equity * frac * c.leverage;
            if c.entry_px > 0.0 { notional / c.entry_px } else { 0.0 }
        }
    }
}

/// Quantity so that `|entry − stop| × qty = risk_pct% of equity`. Needs a stop price.
fn qty_for_risk(risk_pct: f64, c: &SizeCtx) -> f64 {
    let Some(stop) = c.stop_px else { return 0.0 };
    let per_unit = (c.entry_px - stop).abs();
    if per_unit <= 0.0 {
        return 0.0;
    }
    c.equity * (risk_pct / 100.0) / per_unit
}

/// Whether a sizing mode needs a stop-loss to size at all (risk-based modes). The API rejects
/// a run that pairs one of these with a side that has no stop-loss.
pub fn sizing_needs_stop(sizing: &Sizing) -> bool {
    match sizing {
        Sizing::Risk { .. } => true,
        Sizing::EquityTiers { metric, .. } => metric == "risk_pct",
        _ => false,
    }
}

/// The largest indicator lookback the settings reference — bars before signals are defined.
/// Custom indicators contribute their DAG's cumulative lookback (see `CustomIndicatorDef::lookback`),
/// so a chain like `HullMA(RSI(close,9),30)` warms up correctly instead of counting as 0.
fn warmup_bars(s: &Settings) -> usize {
    fn op_lookback(o: &Operand, defs: &IndicatorDefs) -> usize {
        match o {
            Operand::Indicator { period, fast, slow, signal_period, .. } => {
                // Conservative upper bound: the largest configured length (defaults folded in).
                [*period, *fast, *slow, *signal_period].into_iter().max().unwrap_or(0)
            }
            Operand::CustomIndicator { id } => defs.get(id).map(|d| d.lookback()).unwrap_or(0),
            _ => 0,
        }
    }
    fn group_lookback(g: &SignalGroup, defs: &IndicatorDefs) -> usize {
        g.conditions
            .iter()
            .map(|c| {
                let r = c.right.as_ref().map(|o| op_lookback(o, defs)).unwrap_or(0);
                op_lookback(&c.left, defs).max(r)
            })
            .max()
            .unwrap_or(0)
    }
    let mut w = 0;
    for side in [s.long.as_ref(), s.short.as_ref()].into_iter().flatten() {
        if let Some(g) = side.entry_group() {
            w = w.max(group_lookback(&g, &s.indicators));
        }
        if let Some(g) = &side.exit {
            w = w.max(group_lookback(g, &s.indicators));
        }
    }
    w
}

/// Build the merged clock (sorted union of every asset's timestamps) plus, for each asset, a
/// row→bar-index map. Timestamps compare lexically (RFC3339 is sort-safe) so no parsing.
fn merged_clock(assets: &[&Bars]) -> (Vec<String>, Vec<Vec<Option<usize>>>) {
    let mut set: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for a in assets {
        for t in a.ts {
            set.insert(t.as_str());
        }
    }
    let clock: Vec<String> = set.into_iter().map(|s| s.to_string()).collect();
    let row_of: std::collections::HashMap<&str, usize> =
        clock.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();
    let maps: Vec<Vec<Option<usize>>> = assets
        .iter()
        .map(|a| {
            let mut m = vec![None; clock.len()];
            for (bi, t) in a.ts.iter().enumerate() {
                if let Some(&r) = row_of.get(t.as_str()) {
                    m[r] = Some(bi);
                }
            }
            m
        })
        .collect();
    (clock, maps)
}

/// Seconds elapsed at each clock row from the previous row (row 0 = 0). RFC3339 timestamps;
/// unparseable rows contribute 0 (funding simply doesn't accrue across them).
fn row_seconds(clock: &[String]) -> Vec<f64> {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    let parsed: Vec<Option<OffsetDateTime>> =
        clock.iter().map(|t| OffsetDateTime::parse(t, &Rfc3339).ok()).collect();
    (0..clock.len())
        .map(|i| {
            if i == 0 {
                return 0.0;
            }
            match (parsed[i - 1], parsed[i]) {
                (Some(a), Some(b)) => (b - a).whole_seconds().max(0) as f64,
                _ => 0.0,
            }
        })
        .collect()
}

/// Compute the alignment report for a set of assets under `s` — no simulation.
pub fn align(s: &Settings, assets: &[&Bars]) -> AlignmentReport {
    let (clock, maps) = merged_clock(assets);
    let asset_reports: Vec<AssetAlignment> = assets
        .iter()
        .zip(&maps)
        .map(|(a, m)| AssetAlignment {
            ticker: a.ticker.to_string(),
            bars: a.ts.len(),
            first_ts: a.ts.first().cloned(),
            last_ts: a.ts.last().cloned(),
            inactive_bars: m.iter().filter(|x| x.is_none()).count(),
        })
        .collect();
    // Overlap = rows where every asset has a bar.
    let full: Vec<usize> = (0..clock.len())
        .filter(|&r| maps.iter().all(|m| m[r].is_some()))
        .collect();
    AlignmentReport {
        clock_len: clock.len(),
        overlap_from: full.first().map(|&r| clock[r].clone()),
        overlap_to: full.last().map(|&r| clock[r].clone()),
        overlap_bars: full.len(),
        warmup_bars: warmup_bars(s),
        assets: asset_reports,
    }
}

/// Legacy single-asset entry point — delegates to the portfolio loop with one asset so the
/// two never diverge. Preserves the original signature and semantics. The API now always
/// calls `run_portfolio`; this remains the single-asset convenience the golden tests exercise.
#[allow(dead_code)]
pub fn run(s: &Settings, b: &Bars) -> RunResult {
    run_portfolio(s, &[b])
}

/// Portfolio backtest over one or more assets sharing a single equity/cash balance.
///
/// The loop walks a **merged clock** (sorted union of all assets' timestamps). Each asset is
/// active only on rows where it has a bar; signals fire and prices are read on the asset's own
/// (dense) bars, so no-lookahead is per-asset. Assets missing a bar at a row are held and
/// marked-to-market at their last known close. Per row: (1) manage exits for every open
/// position, then (2) evaluate entries asset-by-asset in submission order, checking portfolio
/// limits against the *current* state. Exit priority per bar matches the single-asset engine:
/// explicit exit group (at open) → stop-and-reverse (at open) → SL → TP (intrabar, stop first)
/// → entry-group reversal (at close) → the asset's last bar (at close).
pub fn run_portfolio(s: &Settings, inputs: &[&Bars]) -> RunResult {
    // Grid is a separate order generator — dispatch to it (single-asset: the first dataset).
    if s.kind == "grid" {
        return run_grid(s, inputs[0]);
    }
    let half_spread = s.spread_pct / 2.0;
    let pyramiding = s.pyramiding.clamp(1, 20);
    let allow_long = s.allow_long();
    let allow_short = s.allow_short();
    let mult = if s.instrument.multiplier > 0.0 { s.instrument.multiplier } else { 1.0 };
    // Fill mechanics bundled once (spread + slippage), threaded to entry/exit pricing.
    let fill = FillModel { half_spread, slippage: &s.slippage };

    let (clock, maps) = merged_clock(inputs);
    let m = clock.len();

    let mut assets: Vec<Asset> = inputs
        .iter()
        .zip(maps.into_iter())
        .map(|(b, bar_at_row)| Asset {
            b,
            long: side_signals(s.long.as_ref(), allow_long, b, &s.indicators),
            short: side_signals(s.short.as_ref(), allow_short, b, &s.indicators),
            bar_at_row,
            pos: None,
            active_pos_rows: 0,
            last_close: b.close.first().copied().unwrap_or(0.0),
        })
        .collect();

    let mut trades: Vec<Trade> = Vec::new();
    let mut equity = Vec::with_capacity(m);
    let mut cash = s.starting_capital;
    let mut peak = cash;
    let mut max_dd = 0.0_f64;
    let mut max_dd_abs = 0.0_f64;
    // Closed-trade history feeding Kelly (chronological order of closing).
    let mut closed: Vec<ClosedTrade> = Vec::new();
    // Instrument-profile / circuit-breaker counters surfaced in the result.
    let mut skipped_min_size = 0usize;
    let mut skipped_margin = 0usize;
    let mut halted_bars = 0usize;
    // Circuit breaker: day-start equity keyed by the calendar-day prefix of the timestamp.
    let mut day_key = String::new();
    let mut day_start_equity = cash;
    // Funding: seconds elapsed at each row (from the previous row) for continuous accrual.
    let funding_on = s.funding.annual_rate_pct != 0.0;
    let row_secs: Vec<f64> = if funding_on { row_seconds(&clock) } else { Vec::new() };
    let mut total_funding = 0.0f64;

    let fired = |sig: &Option<Vec<bool>>, bar: usize| bar > 0 && sig.as_ref().is_some_and(|v| v[bar - 1]);
    let holds = |sig: &Option<Vec<bool>>, bar: usize| sig.as_ref().is_some_and(|v| v[bar]);

    for r in 0..m {
        // Roll the daily-loss baseline at each new calendar day (RFC3339 date prefix).
        let today = clock[r].get(0..10).unwrap_or("").to_string();
        if today != day_key {
            day_key = today;
            day_start_equity = equity.last().map(|e: &EquityPoint| e.equity).unwrap_or(cash);
        }

        // ── (1) Manage every open position at this row ──
        for ai in 0..assets.len() {
            let Some(bar) = assets[ai].bar_at_row[r] else { continue };
            let b = assets[ai].b;
            assets[ai].last_close = b.close[bar];
            let is_last_bar = bar == b.ts.len() - 1;

            if assets[ai].pos.is_none() {
                continue;
            }
            let (p_long, avg, atr_entry, stop_override) = {
                let p = assets[ai].pos.as_ref().unwrap();
                (p.long, p.avg_price(), p.atr_entry, p.stop_override)
            };
            // Track MAE/MFE against this bar's adverse/favorable extreme (gross, ×multiplier).
            {
                let adverse = if p_long { b.low[bar] } else { b.high[bar] };
                let favorable = if p_long { b.high[bar] } else { b.low[bar] };
                let p = assets[ai].pos.as_mut().unwrap();
                let loss = -p.unrealized(adverse, mult);
                let gain = p.unrealized(favorable, mult);
                p.mae = p.mae.max(loss.max(0.0));
                p.mfe = p.mfe.max(gain.max(0.0));
                // Track the favorable extreme for the pyramiding min-distance gate.
                p.best_price = if p_long { p.best_price.max(favorable) } else { p.best_price.min(favorable) };
            }
            let own = if p_long { &assets[ai].long } else { &assets[ai].short };
            let own_exit = &own.exit;
            let own_reverse = own.exit_on_reverse;
            // ATR for a potential stop-and-reverse entry comes from the *opposite* side.
            let reverse_atr = if p_long { &assets[ai].short } else { &assets[ai].long }
                .atr
                .as_ref()
                .and_then(|v| v[bar]);
            // SL/TP distances resolved from the position's average and (for ATR) entry ATR.
            let sl_px = own
                .sl
                .as_ref()
                .and_then(|rule| stop_distance(rule, avg, atr_entry))
                .map(|d| if p_long { avg - d } else { avg + d });
            // `after_add_sl` may have moved the stop tighter than the rule stop.
            let sl_px = match (sl_px, stop_override) {
                (Some(rule), Some(ov)) => Some(if p_long { rule.max(ov) } else { rule.min(ov) }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            };
            let tp_px = own
                .tp
                .as_ref()
                .and_then(|rule| stop_distance(rule, avg, atr_entry))
                .map(|d| if p_long { avg + d } else { avg - d });
            let own_entry_holds = holds(&own.entry, bar);
            let opp_fires = if p_long { fired(&assets[ai].short.entry, bar) } else { fired(&assets[ai].long.entry, bar) };

            let mut exit = None; // (price, reason, reverse?)
            if fired(own_exit, bar) {
                exit = Some((b.open[bar], "exit_signal", false));
            }
            if exit.is_none() && s.stop_and_reverse && opp_fires {
                exit = Some((b.open[bar], "reverse", true));
            }
            if exit.is_none() {
                if let Some(stop) = sl_px {
                    if if p_long { b.low[bar] <= stop } else { b.high[bar] >= stop } {
                        exit = Some((stop, "stop_loss", false));
                    }
                }
            }
            if exit.is_none() {
                if let Some(target) = tp_px {
                    if if p_long { b.high[bar] >= target } else { b.low[bar] <= target } {
                        exit = Some((target, "take_profit", false));
                    }
                }
            }
            if exit.is_none() && own_reverse && !own_entry_holds {
                exit = Some((b.close[bar], "signal", false));
            }
            // The asset's own last bar closes any still-open position (mirrors single-asset "end").
            if exit.is_none() && is_last_bar {
                exit = Some((b.close[bar], "end", false));
            }

            if let Some((raw_px, reason, reverse)) = exit {
                let p = assets[ai].pos.take().unwrap();
                let ct = close_trade(&mut trades, &mut cash, &s.fees, &fill, mult, b, &p, raw_px, reason, bar);
                closed.push(ct);
                // Stop-and-reverse: open the opposite side immediately at this bar's open.
                if reverse && !is_last_bar {
                    let ctx = SizeCtx { equity: cash, entry_px: 0.0, stop_px: None, leverage: s.leverage, closed: &closed };
                    if let Some(lot) = make_lot(&s.sizing, &s.fees, &fill, &s.instrument, mult, b, bar, !p.long, 1.0, cash, &ctx) {
                        assets[ai].pos = Some(Pos::new(!p.long, lot, bar, reverse_atr, b.open[bar]));
                    }
                }
            }
        }

        // ── Circuit breakers: are NEW entries halted this row? (open positions still exit) ──
        let cur_eq = {
            let mut e = cash;
            for a in &assets {
                if let Some(p) = &a.pos {
                    e += p.unrealized(a.last_close, mult);
                }
            }
            e
        };
        let halted = breaker_halted(&s.risk, cur_eq, peak, day_start_equity);
        if halted {
            halted_bars += 1;
        }

        // ── (2) Entries / pyramiding adds, asset by asset (submission order) ──
        if !halted {
            for ai in 0..assets.len() {
                let Some(bar) = assets[ai].bar_at_row[r] else { continue };
                let b = assets[ai].b;
                if bar == b.ts.len() - 1 {
                    continue; // no entry on the asset's final bar (nothing to hold into)
                }

                match &assets[ai].pos {
                    None => {
                        let long_fired = fired(&assets[ai].long.entry, bar);
                        let short_fired = fired(&assets[ai].short.entry, bar);
                        let want_long = if long_fired { Some(true) } else if short_fired { Some(false) } else { None };
                        let Some(long) = want_long else { continue };
                        if !limits_allow_open(s, &assets, ai, b, bar, long, &fill, mult, cash, false) {
                            continue;
                        }
                        let sig = if long { &assets[ai].long } else { &assets[ai].short };
                        let entry_atr = sig.atr.as_ref().and_then(|v| v[bar]);
                        let stop_px = first_stop_px(s, long, entry_atr, b, bar, &fill);
                        let ctx = SizeCtx { equity: cash, entry_px: fill.entry(b.open[bar], long), stop_px, leverage: s.leverage, closed: &closed };
                        // scale[0] for the first lot.
                        let scale0 = s.pyramid_steps.scale.first().copied().unwrap_or(1.0);
                        match make_lot_checked(&s.sizing, &s.fees, &fill, &s.instrument, mult, b, bar, long, scale0, cash, &ctx) {
                            LotOutcome::Ok(lot) => {
                                assets[ai].pos = Some(Pos::new(long, lot, bar, entry_atr, b.open[bar]));
                            }
                            LotOutcome::BelowMin => skipped_min_size += 1,
                            LotOutcome::Margin => skipped_margin += 1,
                            LotOutcome::Zero => {}
                        }
                    }
                    Some(p) => {
                        let long = p.long;
                        let own_fires = if long { fired(&assets[ai].long.entry, bar) } else { fired(&assets[ai].short.entry, bar) };
                        let n_lots = p.lots.len();
                        let last_add = p.last_add_bar;
                        // Min-distance gate: favorable move since entry must exceed the threshold.
                        let dist_ok = pyramid_distance_ok(&s.pyramid_steps, p, fill.entry(b.open[bar], long));
                        let can_add = own_fires && n_lots < pyramiding && last_add != bar && dist_ok;
                        if !can_add {
                            continue;
                        }
                        if !limits_allow_open(s, &assets, ai, b, bar, long, &fill, mult, cash, true) {
                            continue;
                        }
                        let entry_atr = p.atr_entry;
                        let stop_px = first_stop_px(s, long, entry_atr, b, bar, &fill);
                        let ctx = SizeCtx { equity: cash, entry_px: fill.entry(b.open[bar], long), stop_px, leverage: s.leverage, closed: &closed };
                        // scale[k] for add index k (clamp to last factor beyond the list).
                        let scale_k = step_scale(&s.pyramid_steps.scale, n_lots);
                        match make_lot_checked(&s.sizing, &s.fees, &fill, &s.instrument, mult, b, bar, long, scale_k, cash, &ctx) {
                            LotOutcome::Ok(lot) => {
                                let new_avg = {
                                    let p = assets[ai].pos.as_ref().unwrap();
                                    let q = p.qty() + lot.qty;
                                    (p.lots.iter().map(|l| l.price * l.qty).sum::<f64>() + lot.price * lot.qty) / q
                                };
                                let p = assets[ai].pos.as_mut().unwrap();
                                p.lots.push(lot);
                                p.last_add_bar = bar;
                                apply_after_add_sl(&s.pyramid_steps, p, new_avg);
                            }
                            LotOutcome::BelowMin => skipped_min_size += 1,
                            LotOutcome::Margin => skipped_margin += 1,
                            LotOutcome::Zero => {}
                        }
                    }
                }
            }
        }

        // ── Funding accrual on open notional (perp estimate) ──
        if funding_on && r > 0 {
            let dt = row_secs.get(r).copied().unwrap_or(0.0);
            if dt > 0.0 {
                // Quote the annual rate per funding interval, then accrue by how many
                // intervals the bar spanned. Equivalent to continuous accrual on the total,
                // but `interval_hours` is a live input: the per-interval rate is what real
                // perps charge, and a longer interval means fewer, larger discrete charges.
                let interval_secs = (s.funding.interval_hours * 3600.0).max(1.0);
                let year_secs = 365.25 * 24.0 * 3600.0;
                let per_interval = s.funding.annual_rate_pct / 100.0 * (interval_secs / year_secs);
                let rate = per_interval * (dt / interval_secs);
                for a in &assets {
                    if let Some(p) = &a.pos {
                        // Longs pay (cash down), shorts receive (cash up).
                        let charge = p.notional(a.last_close, mult) * rate;
                        cash += if p.long { -charge } else { charge };
                        total_funding += if p.long { -charge } else { charge };
                    }
                }
            }
        }

        // ── Mark-to-market equity across all assets at this row's (last-known) closes ──
        let mut eq = cash;
        for a in &mut assets {
            if let Some(p) = &a.pos {
                eq += p.unrealized(a.last_close, mult);
                a.active_pos_rows += 1;
            }
        }
        peak = peak.max(eq);
        if peak > 0.0 {
            max_dd = max_dd.max((peak - eq) / peak * 100.0);
        }
        max_dd_abs = max_dd_abs.max(peak - eq);
        equity.push(EquityPoint { ts: clock[r].clone(), equity: eq });
    }

    // ── Stats ──
    let stats = compute_stats(s, &trades, &equity, cash, inputs[0], max_dd, max_dd_abs);

    // Per-asset breakdown.
    let per_asset: Vec<AssetStats> = assets
        .iter()
        .map(|a| {
            let tk = a.b.ticker;
            let atr: Vec<&Trade> = trades.iter().filter(|t| t.ticker == tk).collect();
            let st = side_stats(atr.iter().copied());
            let inactive = a.bar_at_row.iter().filter(|x| x.is_none()).count();
            AssetStats {
                ticker: tk.to_string(),
                trades: st.trades,
                wins: st.wins,
                win_rate: st.win_rate,
                net_pnl: st.net_pnl,
                total_fees: st.total_fees,
                exposure_pct: if m > 0 { a.active_pos_rows as f64 / m as f64 * 100.0 } else { 0.0 },
                bars: a.b.ts.len(),
                inactive_bars: inactive,
            }
        })
        .collect();

    // Warm-up / effective trading start on the merged clock.
    let wu = warmup_bars(s);
    let trading_start_ts = clock.get(wu.min(m.saturating_sub(1))).cloned();
    let alignment = if inputs.len() > 1 { Some(align(s, inputs)) } else { None };

    // In-sample / out-of-sample split: partition trades + equity at the split timestamp.
    let oos = oos_split(s, &clock, &trades, &equity, inputs[0]);

    // Equal-weight buy-and-hold benchmark over the merged clock.
    let benchmark = buy_hold_curve(s, &clock, &assets, mult);

    RunResult {
        trades,
        equity,
        stats,
        per_asset,
        warmup_bars: wu,
        trading_start_ts,
        alignment,
        skipped_min_size,
        skipped_margin,
        halted_bars,
        oos,
        benchmark,
        total_funding,
        grid: None,
    }
}

/// Grid backtest: a ladder of levels in `[lower, upper]`. Each adjacent pair of levels is a
/// buy-low/sell-high cell. A long grid buys `qty` when the bar's low crosses a level downward
/// (fill at the level) and sells that lot when the bar's high reaches the next level up (TP at
/// the adjacent level). A short grid mirrors it. Optional stop_below/stop_above liquidate and
/// halt. Fees + spread apply to each fill. Returns the standard `RunResult` (trades = completed
/// round trips) plus grid-specific stats. Single-asset.
pub fn run_grid(s: &Settings, b: &Bars) -> RunResult {
    let n = b.close.len();
    let half_spread = s.spread_pct / 2.0;
    let mult = if s.instrument.multiplier > 0.0 { s.instrument.multiplier } else { 1.0 };
    let cfg = s.grid.clone().unwrap_or(GridConfig {
        lower: b.low.iter().copied().fold(f64::INFINITY, f64::min),
        upper: b.high.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        levels: 10,
        qty_per_level: 0.0,
        total_budget: 0.0,
        direction: "long".into(),
        stop_below: 0.0,
        stop_above: 0.0,
    });

    let levels = cfg.levels.clamp(2, 200);
    let (lo, hi) = (cfg.lower.min(cfg.upper), cfg.lower.max(cfg.upper));
    let step = if levels > 1 { (hi - lo) / (levels - 1) as f64 } else { 0.0 };
    let level_px: Vec<f64> = (0..levels).map(|i| lo + step * i as f64).collect();
    let is_short = cfg.direction == "short";
    // Per-cell qty: fixed, or budget split across cells at the mid price.
    let cells = levels.saturating_sub(1).max(1);
    let mid = (lo + hi) / 2.0;
    let qty = if cfg.qty_per_level > 0.0 {
        cfg.qty_per_level
    } else if cfg.total_budget > 0.0 && mid > 0.0 {
        cfg.total_budget / cells as f64 / (mid * mult)
    } else {
        // Fall back to spreading the account across cells.
        s.starting_capital / cells as f64 / (mid.max(1.0) * mult)
    };

    let mut trades: Vec<Trade> = Vec::new();
    let mut equity = Vec::with_capacity(n);
    let mut cash = s.starting_capital;
    let mut peak = cash;
    let mut max_dd = 0.0;
    let mut max_dd_abs = 0.0;
    // Open lot per cell: Some(entry_bar) when that cell currently holds inventory.
    let mut held: Vec<Option<usize>> = vec![None; cells];
    let mut fills = 0usize;
    let mut round_trips = 0usize;
    let mut halted = false;

    let buy_px = |raw: f64| raw * (1.0 + half_spread);
    let sell_px = |raw: f64| raw * (1.0 - half_spread);

    // Settle one completed cell (entry line → exit line) as a trade. Long: buy at entry_raw,
    // sell at exit_raw; short: sell at entry_raw, buy back at exit_raw. Spread + exit fee apply.
    let settle = |trades: &mut Vec<Trade>,
                  cash: &mut f64,
                  entry_bar: usize,
                  exit_bar: usize,
                  entry_raw: f64,
                  exit_raw: f64,
                  reason: &str| {
        let (entry_fill, exit_fill) = if is_short {
            (sell_px(entry_raw), buy_px(exit_raw))
        } else {
            (buy_px(entry_raw), sell_px(exit_raw))
        };
        let dir = if is_short { -1.0 } else { 1.0 };
        let gross = dir * qty * (exit_fill - entry_fill) * mult;
        let exit_fee = fee_for(&s.fees, qty, exit_fill, mult);
        let net = gross - exit_fee;
        *cash += net;
        let notional = entry_fill * qty * mult;
        trades.push(Trade {
            ticker: b.ticker.to_string(),
            entry_ts: b.ts[entry_bar].clone(),
            exit_ts: b.ts[exit_bar].clone(),
            entry_price: entry_fill,
            exit_price: exit_fill,
            qty,
            entries: 1,
            direction: if is_short { "short".into() } else { "long".into() },
            exit_reason: reason.into(),
            pnl: net,
            fees: exit_fee,
            return_pct: if notional != 0.0 { net / notional * 100.0 } else { 0.0 },
            bars_held: exit_bar.saturating_sub(entry_bar),
            mae: 0.0,
            mfe: 0.0,
        });
    };

    for i in 0..n {
        if !halted {
            // Circuit stops: liquidate all inventory at this bar's open and halt.
            let breach = (cfg.stop_below > 0.0 && b.low[i] <= cfg.stop_below)
                || (cfg.stop_above > 0.0 && b.high[i] >= cfg.stop_above);
            if breach {
                for c in 0..cells {
                    if let Some(entry_bar) = held[c].take() {
                        let entry_line = if is_short { level_px[c + 1] } else { level_px[c] };
                        settle(&mut trades, &mut cash, entry_bar, i, entry_line, b.close[i], "stop_loss");
                    }
                }
                halted = true;
            } else {
                // For each cell, entry when the bar reaches the buy level, exit at the sell level.
                for c in 0..cells {
                    // Long: buy at lower line (level_px[c]), sell at upper line (level_px[c+1]).
                    // Short: sell at upper line, buy back at lower line.
                    let (entry_line, exit_line) = if is_short {
                        (level_px[c + 1], level_px[c])
                    } else {
                        (level_px[c], level_px[c + 1])
                    };
                    if held[c].is_none() {
                        // Entry fills when price trades through the entry line.
                        let hit = if is_short { b.high[i] >= entry_line } else { b.low[i] <= entry_line };
                        if hit {
                            held[c] = Some(i);
                            fills += 1;
                            // Entry fee taken on fill.
                            let fpx = if is_short { sell_px(entry_line) } else { buy_px(entry_line) };
                            cash -= fee_for(&s.fees, qty, fpx, mult);
                        }
                    } else {
                        // Exit fills when price reaches the exit line → completed round trip.
                        let hit = if is_short { b.low[i] <= exit_line } else { b.high[i] >= exit_line };
                        if hit {
                            let entry_bar = held[c].take().unwrap();
                            settle(&mut trades, &mut cash, entry_bar, i, entry_line, exit_line, "take_profit");
                            fills += 1;
                            round_trips += 1;
                        }
                    }
                }
            }
        }

        // Mark-to-market: cash + open inventory valued at this close.
        let mut inv_units = 0.0;
        let mut inv_val = 0.0;
        for c in 0..cells {
            if held[c].is_some() {
                let entry_line = if is_short { level_px[c + 1] } else { level_px[c] };
                let dir = if is_short { -1.0 } else { 1.0 };
                inv_units += dir * qty;
                inv_val += dir * qty * (b.close[i] - entry_line) * mult;
            }
        }
        let eq = cash + inv_val;
        peak = peak.max(eq);
        if peak > 0.0 {
            max_dd = f64::max(max_dd, (peak - eq) / peak * 100.0);
        }
        max_dd_abs = f64::max(max_dd_abs, peak - eq);
        equity.push(EquityPoint { ts: b.ts[i].clone(), equity: eq });
        let _ = inv_units;
    }

    // Leftover inventory at the end (unrealized) — reported, not force-closed.
    let open_cells = held.iter().filter(|h| h.is_some()).count();
    let dir = if is_short { -1.0 } else { 1.0 };
    let end_units = dir * qty * open_cells as f64;
    let end_val = {
        let mut v = 0.0;
        for c in 0..cells {
            if held[c].is_some() {
                let entry_line = if is_short { level_px[c + 1] } else { level_px[c] };
                v += dir * qty * (b.close[n - 1] - entry_line) * mult;
            }
        }
        v
    };

    let stats = compute_stats(s, &trades, &equity, cash + end_val, b, max_dd, max_dd_abs);
    let per_asset = vec![AssetStats {
        ticker: b.ticker.to_string(),
        trades: stats.trades,
        wins: stats.wins,
        win_rate: stats.win_rate,
        net_pnl: stats.net_pnl,
        total_fees: stats.total_fees,
        exposure_pct: 0.0,
        bars: n,
        inactive_bars: 0,
    }];
    let benchmark = {
        let assets = [Asset {
            b,
            long: side_signals(None, false, b, &s.indicators),
            short: side_signals(None, false, b, &s.indicators),
            bar_at_row: (0..n).map(Some).collect(),
            pos: None,
            active_pos_rows: 0,
            last_close: b.close.first().copied().unwrap_or(0.0),
        }];
        let clock: Vec<String> = b.ts.to_vec();
        buy_hold_curve(s, &clock, &assets, mult)
    };

    RunResult {
        trades,
        equity,
        stats,
        per_asset,
        warmup_bars: 0,
        trading_start_ts: b.ts.first().cloned(),
        alignment: None,
        skipped_min_size: 0,
        skipped_margin: 0,
        halted_bars: if halted { 1 } else { 0 },
        oos: None,
        benchmark,
        total_funding: 0.0,
        grid: Some(GridStats {
            fills,
            round_trips,
            end_inventory: end_units,
            end_inventory_value: end_val,
            levels,
        }),
    }
}


/// Equal-weight buy-and-hold equity over the merged clock: split the starting capital across the
/// assets, buy each at its first available (spread-free) close minus one entry fee, hold to the
/// end. Marked-to-market at each asset's last-known price each row. Cash-neutral for assets that
/// haven't started yet (their slice stays as cash). A fair "did the strategy beat holding?" line.
fn buy_hold_curve(s: &Settings, clock: &[String], assets: &[Asset], mult: f64) -> Vec<EquityPoint> {
    let n_assets = assets.len().max(1);
    let per_asset_cap = s.starting_capital / n_assets as f64;
    // Fixed quantity bought for each asset at its first close (None until it starts).
    let mut qty = vec![0.0f64; assets.len()];
    let mut fee_paid = vec![0.0f64; assets.len()];
    let mut bought = vec![false; assets.len()];
    let mut last_px = vec![0.0f64; assets.len()];

    clock
        .iter()
        .enumerate()
        .map(|(r, ts)| {
            let mut eq = 0.0;
            for (ai, a) in assets.iter().enumerate() {
                if let Some(bar) = a.bar_at_row[r] {
                    let px = a.b.close[bar];
                    last_px[ai] = px;
                    if !bought[ai] && px > 0.0 {
                        qty[ai] = per_asset_cap / (px * mult);
                        fee_paid[ai] = fee_for(&s.fees, qty[ai], px, mult);
                        bought[ai] = true;
                    }
                }
                if bought[ai] {
                    // Position value at last-known price, less the one-time entry fee.
                    eq += qty[ai] * last_px[ai] * mult - fee_paid[ai];
                } else {
                    // Not started yet → its capital slice sits in cash.
                    eq += per_asset_cap;
                }
            }
            EquityPoint { ts: ts.clone(), equity: eq }
        })
        .collect()
}

/// Spread + slippage fill model. Slippage always worsens a fill; spread is symmetric half-each.
struct FillModel<'a> {
    half_spread: f64,
    slippage: &'a Slippage,
}
impl FillModel<'_> {
    /// Entry fill price (long pays up, short sells down).
    fn entry(&self, raw: f64, long: bool) -> f64 {
        let base = if long { raw * (1.0 + self.half_spread) } else { raw * (1.0 - self.half_spread) };
        let slip = self.slippage.amount(base);
        if long { base + slip } else { base - slip }
    }
    /// Exit fill price (long sells down, short buys up).
    fn exit(&self, raw: f64, long: bool) -> f64 {
        let base = if long { raw * (1.0 - self.half_spread) } else { raw * (1.0 + self.half_spread) };
        let slip = self.slippage.amount(base);
        if long { base - slip } else { base + slip }
    }
}

/// The stop price for a *fresh* lot (first entry) — from the side's SL rule, sampling ATR when
/// the rule is ATR-based. Used by risk-based sizing at entry time (before a Pos exists).
fn first_stop_px(s: &Settings, long: bool, entry_atr: Option<f64>, b: &Bars, bar: usize, fill: &FillModel) -> Option<f64> {
    let side = if long { s.long.as_ref() } else { s.short.as_ref() };
    let rule = side.and_then(|sd| sd.sl_rule())?;
    let entry = fill.entry(b.open[bar], long);
    let dist = stop_distance(&rule, entry, entry_atr)?;
    Some(if long { entry - dist } else { entry + dist })
}

/// Per-add size factor: `scale[k]`, or the last factor for adds beyond the list, or 1.0 if empty.
fn step_scale(scale: &[f64], k: usize) -> f64 {
    if scale.is_empty() {
        1.0
    } else {
        scale[k.min(scale.len() - 1)]
    }
}

/// Min-distance pyramiding gate: the favorable move since entry (against the average) must
/// exceed `min_distance_pct` before another add is allowed. 0 = no gate.
fn pyramid_distance_ok(ps: &PyramidSteps, p: &Pos, cur_entry_px: f64) -> bool {
    if ps.min_distance_pct <= 0.0 {
        return true;
    }
    let avg = p.avg_price();
    if avg <= 0.0 {
        return true;
    }
    let move_frac = if p.long { (cur_entry_px - avg) / avg } else { (avg - cur_entry_px) / avg };
    move_frac >= ps.min_distance_pct
}

/// Apply `after_add_sl` when an add fills: move the stop to breakeven (average) or trail to the
/// new average. "none" leaves it to the SL rule.
fn apply_after_add_sl(ps: &PyramidSteps, p: &mut Pos, new_avg: f64) {
    match ps.after_add_sl.as_str() {
        "breakeven" | "trail_avg" => {
            // Both anchor the override at the new average entry (breakeven-on-average).
            p.stop_override = Some(new_avg);
        }
        _ => {}
    }
}

/// Circuit breaker: true when new entries should be halted this row.
fn breaker_halted(risk: &Risk, equity: f64, peak: f64, day_start: f64) -> bool {
    if let Some(dd) = risk.max_drawdown_pct {
        if dd > 0.0 && peak > 0.0 && (peak - equity) / peak * 100.0 >= dd {
            return true;
        }
    }
    if let Some(dl) = risk.max_daily_loss_pct {
        if dl > 0.0 && day_start > 0.0 && (day_start - equity) / day_start * 100.0 >= dl {
            return true;
        }
    }
    false
}

/// Outcome of attempting to build a lot — distinguishes the reasons an entry is refused so the
/// engine can count them (silent skips destroy trust).
enum LotOutcome {
    Ok(Lot),
    /// Rounded below the instrument minimum size.
    BelowMin,
    /// Required margin would exceed available equity.
    Margin,
    /// Sizing resolved to zero (e.g. negative Kelly) — an intentional skip, not an error.
    Zero,
}

/// Build a sized lot applying the per-add `scale`, instrument lot rounding/min-size, and a
/// margin check (required margin = notional / leverage ≤ equity).
#[allow(clippy::too_many_arguments)]
fn make_lot_checked(
    sizing: &Sizing,
    fees: &Fees,
    fill: &FillModel,
    inst: &Instrument,
    mult: f64,
    b: &Bars,
    bar: usize,
    long: bool,
    scale: f64,
    equity: f64,
    ctx: &SizeCtx,
) -> LotOutcome {
    let px = fill.entry(b.open[bar], long);
    let ctx = SizeCtx { entry_px: px, ..reborrow_ctx(ctx) };
    let raw = resolve_qty(sizing, &ctx) * scale.max(0.0);
    if raw <= 0.0 {
        return LotOutcome::Zero;
    }
    let Some(q) = inst.round_qty(raw) else {
        return LotOutcome::BelowMin;
    };
    // Margin: notional / leverage must fit in equity (guards fixed-qty over-allocation).
    let lev = if ctx.leverage > 0.0 { ctx.leverage } else { 1.0 };
    let notional = q * px * mult;
    if notional / lev > equity + 1e-9 {
        return LotOutcome::Margin;
    }
    LotOutcome::Ok(Lot { price: px, qty: q, fee: fee_for(fees, q, px, mult) })
}

/// Convenience for the stop-and-reverse path (no scale/skip-reason bookkeeping needed).
#[allow(clippy::too_many_arguments)]
fn make_lot(
    sizing: &Sizing,
    fees: &Fees,
    fill: &FillModel,
    inst: &Instrument,
    mult: f64,
    b: &Bars,
    bar: usize,
    long: bool,
    scale: f64,
    equity: f64,
    ctx: &SizeCtx,
) -> Option<Lot> {
    match make_lot_checked(sizing, fees, fill, inst, mult, b, bar, long, scale, equity, ctx) {
        LotOutcome::Ok(lot) => Some(lot),
        _ => None,
    }
}

/// Shallow copy of a SizeCtx (its only borrow is `closed`), so callers can override entry_px.
fn reborrow_ctx<'a>(c: &SizeCtx<'a>) -> SizeCtx<'a> {
    SizeCtx { equity: c.equity, entry_px: c.entry_px, stop_px: c.stop_px, leverage: c.leverage, closed: c.closed }
}

/// Close position `p` at `raw_px`, record the trade, settle cash, and return a Kelly summary.
#[allow(clippy::too_many_arguments)]
fn close_trade(
    trades: &mut Vec<Trade>,
    cash: &mut f64,
    fees: &Fees,
    fill: &FillModel,
    mult: f64,
    b: &Bars,
    p: &Pos,
    raw_px: f64,
    reason: &str,
    exit_bar: usize,
) -> ClosedTrade {
    let px = fill.exit(raw_px, p.long);
    let qty = p.qty();
    let avg = p.avg_price();
    let exit_fee = fee_for(fees, qty, px, mult);
    let gross = p.unrealized(px, mult);
    let entry_fees = p.entry_fees();
    let net = gross - entry_fees - exit_fee;
    *cash += net;
    let notional = avg * qty.abs() * mult;
    let ret = if notional != 0.0 { net / notional * 100.0 } else { 0.0 };
    trades.push(Trade {
        ticker: b.ticker.to_string(),
        entry_ts: b.ts[p.entry_bar].clone(),
        exit_ts: b.ts[exit_bar].clone(),
        entry_price: avg,
        exit_price: px,
        qty,
        entries: p.lots.len(),
        direction: if p.long { "long".into() } else { "short".into() },
        exit_reason: reason.into(),
        pnl: net,
        fees: entry_fees + exit_fee,
        return_pct: ret,
        bars_held: exit_bar.saturating_sub(p.entry_bar),
        mae: p.mae,
        mfe: p.mfe,
    });
    ClosedTrade { ret: if notional != 0.0 { net / notional } else { 0.0 }, win: net >= 0.0 }
}

/// Assemble the portfolio `Stats` block from the trade list + equity curve.
fn compute_stats(
    s: &Settings,
    trades: &[Trade],
    equity: &[EquityPoint],
    cash: f64,
    a0: &Bars,
    max_dd: f64,
    max_dd_abs: f64,
) -> Stats {
    let all = side_stats(trades.iter());
    let long_s = side_stats(trades.iter().filter(|t| t.direction == "long"));
    let short_s = side_stats(trades.iter().filter(|t| t.direction == "short"));
    let mut exit_reasons = BTreeMap::new();
    for t in trades {
        *exit_reasons.entry(t.exit_reason.clone()).or_insert(0) += 1;
    }
    let (sharpe, sortino) = risk_ratios(equity);
    let bh = if a0.close.first().copied().unwrap_or(0.0) > 0.0 {
        (a0.close[a0.close.len() - 1] / a0.close[0] - 1.0) * 100.0
    } else {
        0.0
    };
    let net_pnl = cash - s.starting_capital;
    Stats {
        engine_version: ENGINE_VERSION,
        trades: all.trades,
        wins: all.wins,
        losses: all.losses,
        win_rate: all.win_rate,
        net_pnl,
        return_pct: if s.starting_capital > 0.0 { net_pnl / s.starting_capital * 100.0 } else { 0.0 },
        total_fees: all.total_fees,
        max_drawdown_pct: max_dd,
        max_drawdown: max_dd_abs,
        profit_factor: all.profit_factor,
        avg_trade: all.avg_trade,
        final_equity: cash,
        buy_hold_return_pct: bh,
        sharpe,
        sortino,
        expectancy_pct: all.expectancy_pct,
        exit_reasons,
        all,
        long: long_s,
        short: short_s,
    }
}

/// Build the in-sample / out-of-sample split from the same run's trades + equity. Trades are
/// bucketed by entry timestamp against the split point; each block's stats are trade-derived
/// with its own equity segment feeding drawdown/Sharpe. Cheapest overfitting alarm (spec §5).
fn oos_split(
    s: &Settings,
    clock: &[String],
    trades: &[Trade],
    equity: &[EquityPoint],
    a0: &Bars,
) -> Option<OosSplit> {
    let split = s.oos_split_pct;
    if !(split > 0.0 && split < 1.0) || clock.len() < 4 {
        return None;
    }
    let idx = ((clock.len() as f64) * split).round() as usize;
    let idx = idx.clamp(1, clock.len() - 1);
    let split_ts = clock[idx].clone();

    let (is_eq, oos_eq): (Vec<_>, Vec<_>) = equity.iter().partition(|e| e.ts < split_ts);
    let is_trades: Vec<&Trade> = trades.iter().filter(|t| t.entry_ts < split_ts).collect();
    let oos_trades: Vec<&Trade> = trades.iter().filter(|t| t.entry_ts >= split_ts).collect();

    // Trade-derived stats for one segment, with drawdown/Sharpe from its own equity slice.
    let block = |trs: &[&Trade], eq: &[&EquityPoint], start_cap: f64| -> Stats {
        let mut dd = 0.0_f64;
        let mut dd_abs = 0.0_f64;
        let mut peak = eq.first().map(|e| e.equity).unwrap_or(start_cap);
        for e in eq {
            peak = peak.max(e.equity);
            if peak > 0.0 {
                dd = dd.max((peak - e.equity) / peak * 100.0);
            }
            dd_abs = dd_abs.max(peak - e.equity);
        }
        let all = side_stats(trs.iter().copied());
        let long_s = side_stats(trs.iter().copied().filter(|t| t.direction == "long"));
        let short_s = side_stats(trs.iter().copied().filter(|t| t.direction == "short"));
        let mut exit_reasons = BTreeMap::new();
        for t in trs {
            *exit_reasons.entry(t.exit_reason.clone()).or_insert(0) += 1;
        }
        // risk_ratios wants a slice of owned points; clone the light segment for it.
        let eq_owned: Vec<EquityPoint> = eq.iter().map(|e| EquityPoint { ts: e.ts.clone(), equity: e.equity }).collect();
        let (sharpe, sortino) = risk_ratios(&eq_owned);
        let final_eq = eq.last().map(|e| e.equity).unwrap_or(start_cap);
        let net = all.net_pnl;
        Stats {
            engine_version: ENGINE_VERSION,
            trades: all.trades,
            wins: all.wins,
            losses: all.losses,
            win_rate: all.win_rate,
            net_pnl: net,
            return_pct: if start_cap > 0.0 { net / start_cap * 100.0 } else { 0.0 },
            total_fees: all.total_fees,
            max_drawdown_pct: dd,
            max_drawdown: dd_abs,
            profit_factor: all.profit_factor,
            avg_trade: all.avg_trade,
            final_equity: final_eq,
            buy_hold_return_pct: 0.0,
            sharpe,
            sortino,
            expectancy_pct: all.expectancy_pct,
            exit_reasons,
            all,
            long: long_s,
            short: short_s,
        }
    };
    let _ = a0;
    Some(OosSplit {
        split_pct: split,
        in_sample: block(&is_trades, &is_eq, s.starting_capital),
        out_sample: block(&oos_trades, &oos_eq, is_eq.last().map(|e| e.equity).unwrap_or(s.starting_capital)),
        split_ts: Some(split_ts),
    })
}

/// Whether the portfolio limits permit opening/adding to a position on asset `ai` at `bar`.
/// `is_add` distinguishes a pyramiding add (position already counts toward open count).
#[allow(clippy::too_many_arguments)]
fn limits_allow_open(
    s: &Settings,
    assets: &[Asset],
    ai: usize,
    b: &Bars,
    bar: usize,
    long: bool,
    fill: &FillModel,
    mult: f64,
    equity: f64,
    is_add: bool,
) -> bool {
    let risk = &s.risk;
    if !is_add {
        if let Some(maxp) = risk.max_open_positions {
            if maxp > 0 && assets.iter().filter(|a| a.pos.is_some()).count() >= maxp {
                return false;
            }
        }
    }
    let px = fill.entry(b.open[bar], long);
    if let Some(cap) = risk.max_exposure_pct {
        if cap > 0.0 {
            let open: f64 = assets.iter().filter_map(|a| a.pos.as_ref().map(|p| p.notional(a.last_close, mult))).sum();
            if open > equity * cap / 100.0 {
                return false;
            }
        }
    }
    if let Some(cap) = risk.max_exposure_per_asset_pct {
        if cap > 0.0 {
            let this = assets[ai].pos.as_ref().map(|p| p.notional(px, mult)).unwrap_or(0.0);
            if this > equity * cap / 100.0 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat synthetic market: n bars, constant price.
    fn flat_bars(n: usize) -> (Vec<String>, Vec<f64>) {
        let ts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        let px = vec![100.0; n];
        (ts, px)
    }

    fn bars<'a>(ts: &'a [String], px: &'a [f64]) -> Bars<'a> {
        Bars { ticker: "", ts, open: px, high: px, low: px, close: px, volume: px }
    }

    /// Const 2 > const 1 — always true. Const 0 > const 1 — always false.
    fn cond(l: f64, r: f64) -> Signal {
        Signal {
            left: Operand::Const { value: l },
            op: Op::Above,
            right: Some(Operand::Const { value: r }),
        }
    }

    fn base_settings(entry: SignalGroup) -> Settings {
        Settings {
            kind: "signals".into(),
            grid: None,
            mode: Mode::Long,
            long: Some(Side {
                signal: None,
                entry: Some(entry),
                exit: None,
                stop_loss_pct: 0.0,
                take_profit_pct: 0.0,
                stop_loss: None,
                take_profit: None,
                exit_on_reverse: false,
            }),
            short: None,
            stop_and_reverse: false,
            pyramiding: 1,
            sizing: Sizing::FixedQty { qty: 1.0 },
            starting_capital: 10_000.0,
            leverage: 1.0,
            spread_pct: 0.0,
            fees: Fees::default(),
            risk: Risk::default(),
            pyramid_steps: PyramidSteps::default(),
            instrument: Instrument::default(),
            slippage: Slippage::default(),
            oos_split_pct: 0.0,
            indicators: IndicatorDefs::new(),
            funding: Funding::default(),
        }
    }

    #[test]
    fn legacy_v1_settings_deserialize_and_run() {
        let json = serde_json::json!({
            "mode": "long",
            "long": {
                "signal": { "left": {"kind":"const","value":2.0}, "op": "above",
                            "right": {"kind":"const","value":1.0} },
                "exit_on_reverse": false
            },
            "sizing": { "mode": "fixed_qty", "qty": 1.0 }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        let (ts, px) = flat_bars(10);
        let r = run(&s, &bars(&ts, &px));
        // Always-true signal → one position opened at bar 1, held to the end.
        assert_eq!(r.trades.len(), 1);
        assert_eq!(r.trades[0].exit_reason, "end");
    }

    #[test]
    fn group_all_vs_any() {
        let (ts, px) = flat_bars(10);
        // ALL of {true, false} → never fires.
        let all = base_settings(SignalGroup {
            logic: "all".into(),
            conditions: vec![cond(2.0, 1.0), cond(0.0, 1.0)],
        });
        assert!(run(&all, &bars(&ts, &px)).trades.is_empty());
        // ANY of {true, false} → fires.
        let any = base_settings(SignalGroup {
            logic: "any".into(),
            conditions: vec![cond(2.0, 1.0), cond(0.0, 1.0)],
        });
        assert_eq!(run(&any, &bars(&ts, &px)).trades.len(), 1);
    }

    #[test]
    fn pyramiding_stacks_entries() {
        let (ts, px) = flat_bars(10);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.pyramiding = 3;
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades.len(), 1);
        assert_eq!(r.trades[0].entries, 3);
        assert_eq!(r.trades[0].qty, 3.0);
    }

    #[test]
    fn exit_group_closes_position() {
        let n = 10;
        let ts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        // Price ramps up; exit when close > 104 (fires at bar 5 → exit at bar 6 open).
        let px: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.long.as_mut().unwrap().exit = Some(SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Price { field: "close".into() },
                op: Op::Above,
                right: Some(Operand::Const { value: 104.0 }),
            }],
        });
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].exit_reason, "exit_signal");
        assert_eq!(r.trades[0].exit_ts, "t6");
    }

    // ── Golden tests: pin exact engine outputs before the multi-asset/sizing refactor.
    //    These are the regression safety net — if a number here changes, semantics changed. ──

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "expected {b}, got {a}");
    }

    /// Captured once from the current engine — see `golden_v1_regression`. Do not edit unless
    /// engine semantics deliberately change (and bump `ENGINE_VERSION` when they do).
    const V1_FINAL_EQUITY: f64 = 9071.235667736271;

    /// Distinct OHLC bars so SL/TP intrabar fills and spread are exercised (not a flat price).
    fn ohlc<'a>(
        ts: &'a [String],
        o: &'a [f64],
        h: &'a [f64],
        l: &'a [f64],
        c: &'a [f64],
    ) -> Bars<'a> {
        Bars { ticker: "", ts, open: o, high: h, low: l, close: c, volume: c }
    }

    /// Long entry (always fires), fixed 1 qty, SL 5% / TP 10%, zero spread/fees.
    /// Price ramps so TP hits: entry at bar1 open=101, avg=101, TP target=111.1, first
    /// bar whose high ≥ 111.1 closes the trade at exactly the target.
    #[test]
    fn golden_take_profit_fill_and_pnl() {
        let n = 15;
        let ts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        let o: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 0.5).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 0.5).collect();
        let c = o.clone();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        let sd = s.long.as_mut().unwrap();
        sd.take_profit_pct = 0.10;
        sd.stop_loss_pct = 0.05;
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        // Always-true entry re-fires after each TP exit, so multiple trades; the golden
        // is the first trade's fill/pnl, which pins TP intrabar-fill semantics.
        assert!(!r.trades.is_empty());
        let t = &r.trades[0];
        assert_eq!(t.exit_reason, "take_profit");
        approx(t.entry_price, 101.0); // fill at bar1 open
        approx(t.exit_price, 111.1); // TP = avg * 1.10
        approx(t.pnl, 10.1); // (111.1 - 101) * 1 qty, no fees/spread
        approx(t.qty, 1.0);
    }

    /// Pyramiding + spread + pct fee: pin qty, weighted-avg entry, fee accounting.
    #[test]
    fn golden_pyramiding_spread_fees() {
        let n = 8;
        let ts: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
        let px = vec![100.0; n]; // flat: avg entry = spread-adjusted open, no TP/SL
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.pyramiding = 3;
        s.spread_pct = 0.01; // half-spread 0.5% → long entry px = 100 * 1.005 = 100.5
        s.fees = Fees { amount_kind: "pct".into(), per: "trade".into(), amount: 0.1 };
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades.len(), 1);
        let t = &r.trades[0];
        assert_eq!(t.entries, 3);
        approx(t.qty, 3.0);
        approx(t.entry_price, 100.5); // all three lots fill at the same spread-adjusted open
        // Exit at end: close=100 → spread-adjusted long exit px = 100 * 0.995 = 99.5.
        approx(t.exit_price, 99.5);
        // Entry fees: 3 lots × (0.1% of 1×100.5) = 3 × 0.1005 = 0.3015.
        // Exit fee: 0.1% of (3 × 99.5) = 0.2985. Total 0.6.
        approx(t.fees, 0.6);
        // Gross = (99.5 - 100.5) × 3 = -3.0; net = -3.0 - 0.6 = -3.6.
        approx(t.pnl, -3.6);
    }

    /// Captured v1 settings JSON (single `signal` per side, legacy `sizing`) must still
    /// deserialize and produce identical trades — the superset guarantee.
    #[test]
    fn golden_v1_regression() {
        let json = serde_json::json!({
            "mode": "long",
            "long": {
                "signal": { "left": {"kind":"indicator","indicator":"sma","period":3},
                            "op": "crosses_above",
                            "right": {"kind":"price","field":"close"} },
                "stop_loss_pct": 0.03,
                "take_profit_pct": 0.06,
                "exit_on_reverse": true
            },
            "pyramiding": 1,
            "sizing": { "mode": "percent_equity", "percent": 100 },
            "starting_capital": 10000,
            "leverage": 1,
            "spread_pct": 0,
            "fees": { "amount_kind": "pct", "per": "trade", "amount": 0.1 }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        // Deterministic wavy series so the SMA cross actually triggers.
        let n = 40;
        let ts: Vec<String> = (0..n)
            .map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1))
            .collect();
        let c: Vec<f64> = (0..n)
            .map(|i| 100.0 + 8.0 * ((i as f64) * 0.5).sin())
            .collect();
        let o = c.clone();
        let h: Vec<f64> = c.iter().map(|x| x + 1.0).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 1.0).collect();
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        // Pin the shape: this asserts the legacy path stays stable across the refactor.
        assert_eq!(r.trades.len(), 3, "v1 trade count drifted");
        assert_eq!(r.stats.trades, 3);
        assert!(r.stats.final_equity.is_finite());
        // Pin final equity to lock legacy PnL semantics across the refactor.
        approx(r.stats.final_equity, V1_FINAL_EQUITY);
    }

    // ── Multi-asset + sizing modes ──

    fn tk_bars<'a>(tk: &'a str, ts: &'a [String], px: &'a [f64]) -> Bars<'a> {
        Bars { ticker: tk, ts, open: px, high: px, low: px, close: px, volume: px }
    }

    /// Two assets, always-true long entry, fixed 1 qty each, held to end. Portfolio should
    /// produce one trade per asset, tagged with the right ticker, and per-asset stats.
    #[test]
    fn portfolio_two_assets_independent() {
        let n = 6;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let a: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect(); // rising
        let b: Vec<f64> = (0..n).map(|i| 200.0 - i as f64).collect(); // falling
        let s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        let r = run_portfolio(&s, &[&tk_bars("AAA", &ts, &a), &tk_bars("BBB", &ts, &b)]);
        assert_eq!(r.trades.len(), 2);
        assert_eq!(r.per_asset.len(), 2);
        assert_eq!(r.per_asset[0].ticker, "AAA");
        assert_eq!(r.per_asset[1].ticker, "BBB");
        assert_eq!(r.per_asset[0].trades, 1);
        assert_eq!(r.per_asset[1].trades, 1);
        // AAA long into a rising market wins; BBB long into a falling market loses.
        assert!(r.per_asset[0].net_pnl > 0.0);
        assert!(r.per_asset[1].net_pnl < 0.0);
        assert!(r.alignment.is_some());
    }

    /// Merged clock + mark-to-market: asset B starts one bar late. B is inactive at row 0 and
    /// the merged clock spans the union (6 rows). B's trade still opens on its own first bar.
    #[test]
    fn portfolio_staggered_start_marks_to_market() {
        let ts_a: Vec<String> = (0..6).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let ts_b: Vec<String> = (1..6).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let pa = vec![100.0; 6];
        let pb = vec![50.0; 5];
        let s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        let r = run_portfolio(&s, &[&tk_bars("AAA", &ts_a, &pa), &tk_bars("BBB", &ts_b, &pb)]);
        assert_eq!(r.equity.len(), 6, "merged clock is the union");
        let al = r.alignment.unwrap();
        assert_eq!(al.clock_len, 6);
        assert_eq!(al.overlap_bars, 5); // rows both are present
        // BBB missing at row 0 → one inactive bar.
        let bbb = al.assets.iter().find(|x| x.ticker == "BBB").unwrap();
        assert_eq!(bbb.inactive_bars, 1);
    }

    /// Risk-per-trade sizing: risk 1% of 10k = $100; SL 2% below a 100 entry ⇒ $2/unit ⇒ 50 qty.
    #[test]
    fn sizing_risk_per_trade() {
        let (ts, px) = flat_bars(6);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.sizing = Sizing::Risk { risk_pct: 1.0 };
        s.long.as_mut().unwrap().stop_loss_pct = 0.02;
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades.len(), 1);
        approx(r.trades[0].qty, 50.0);
    }

    /// Equity-tier sizing (metric=qty): equity 10k → the "above 1000" tier's 0.5 qty wins.
    #[test]
    fn sizing_equity_tiers_qty() {
        let (ts, px) = flat_bars(6);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.sizing = Sizing::EquityTiers {
            metric: "qty".into(),
            tiers: vec![
                Tier { above: 0.0, value: 0.1 },
                Tier { above: 1000.0, value: 0.5 },
                Tier { above: 100000.0, value: 2.0 },
            ],
        };
        let r = run(&s, &bars(&ts, &px));
        approx(r.trades[0].qty, 0.5);
    }

    /// Kelly warm-up: with window 30 and only a handful of trades, sizing falls back to the
    /// warmup rule (2%-equity notional) — never zero, never the Kelly formula.
    #[test]
    fn sizing_kelly_warmup_fallback() {
        // Ramp so trades close via TP repeatedly, exercising the warm-up branch each time.
        let n = 30;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-02-{:02}T00:00:00Z", i + 1)).collect();
        let o: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let h: Vec<f64> = o.iter().map(|x| x + 2.0).collect();
        let l: Vec<f64> = o.iter().map(|x| x - 2.0).collect();
        let c = o.clone();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.long.as_mut().unwrap().take_profit_pct = 0.01;
        s.sizing = Sizing::Kelly {
            fraction: 0.5,
            window: 30,
            cap_pct: 20.0,
            warmup: Some(Box::new(Sizing::PercentEquity { percent: 2.0 })),
        };
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        assert!(!r.trades.is_empty());
        // First trade sized by warm-up: 2% of 10k notional / entry px (101) ≈ 1.980 qty.
        approx(r.trades[0].qty, 200.0 / 101.0);
    }

    /// Max-open-positions limit: 3 assets all fire, cap = 2 → only 2 positions open.
    #[test]
    fn limit_max_open_positions() {
        let n = 6;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-03-{:02}T00:00:00Z", i + 1)).collect();
        let p = vec![100.0; n];
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.risk.max_open_positions = Some(2);
        let r = run_portfolio(
            &s,
            &[&tk_bars("A", &ts, &p), &tk_bars("B", &ts, &p), &tk_bars("C", &ts, &p)],
        );
        // Exactly two assets ever open a trade.
        let with_trades = r.per_asset.iter().filter(|a| a.trades > 0).count();
        assert_eq!(with_trades, 2);
    }

    /// Warm-up bars = the largest indicator lookback referenced by the settings.
    #[test]
    fn warmup_reflects_indicator_lookback() {
        let mut s = base_settings(SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Indicator {
                    indicator: "sma".into(),
                    period: 34,
                    fast: 0,
                    slow: 0,
                    mult: 0.0,
                    signal_period: 0,
                },
                op: Op::Above,
                right: Some(Operand::Price { field: "close".into() }),
            }],
        });
        s.mode = Mode::Long;
        assert_eq!(warmup_bars(&s), 34);
    }

    // ── Phase 2: risk layer ──

    /// Instrument lot rounding + min size: raw qty rounds DOWN to lot_step; below min is skipped.
    #[test]
    fn instrument_lot_rounding_and_min() {
        let inst = Instrument { multiplier: 1.0, lot_step: 0.5, min_qty: 1.0 };
        assert_eq!(inst.round_qty(1.7), Some(1.5)); // 1.7 → floor to 0.5 step = 1.5
        assert_eq!(inst.round_qty(0.9), None); // below min 1.0 → skip
        assert_eq!(inst.round_qty(2.0), Some(2.0));
    }

    /// Contract multiplier scales PnL: 1 lot, price +10, ×5 multiplier → pnl 50 (not 10).
    #[test]
    fn instrument_multiplier_scales_pnl() {
        let n = 12;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let o: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let c = o.clone();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.instrument = Instrument { multiplier: 5.0, lot_step: 0.0, min_qty: 0.0 };
        let r = run(&s, &ohlc(&ts, &o, &o, &o, &c));
        // Entry bar1 @101, exit "end" bar11 @111 → gross (111-101)*1*5 = 50.
        approx(r.trades[0].pnl, 50.0);
    }

    /// ATR stop-loss: stop distance = ATR(period) at entry × multiple; a drop past it exits.
    /// Entry is delayed past ATR warm-up (close crosses above 99) so ATR is defined at entry.
    #[test]
    fn atr_stop_loss_fires() {
        let n = 40;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-05-01T{:02}:00:00Z", i)).collect();
        // Below 99 for the ATR warm-up, cross above at bar ~20, then a steep drop.
        let mut c = vec![98.0; n];
        for i in 20..30 {
            c[i] = 100.0; // above 99 → entry fires here (ATR(14) defined by now)
        }
        for i in 30..n {
            c[i] = 100.0 - (i - 29) as f64 * 3.0; // steep decline past the ATR stop
        }
        let o = c.clone();
        let h: Vec<f64> = c.iter().map(|x| x + 0.5).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 0.5).collect();
        let entry = SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::Price { field: "close".into() },
                op: Op::CrossesAbove,
                right: Some(Operand::Const { value: 99.0 }),
            }],
        };
        let mut s = base_settings(entry);
        s.long.as_mut().unwrap().stop_loss = Some(Stop { kind: "atr".into(), value: 2.0, period: 14 });
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        assert!(!r.trades.is_empty());
        assert_eq!(r.trades[0].exit_reason, "stop_loss");
    }

    /// Slippage worsens fills: a long pays more on entry and receives less on exit than spread
    /// alone, so a flat market shows a loss equal to 2× slippage × qty.
    #[test]
    fn slippage_worsens_fills() {
        let (ts, px) = flat_bars(6);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.slippage = Slippage { kind: "pct".into(), value: 0.01, tick_size: 0.0 }; // 1% each side
        let r = run(&s, &bars(&ts, &px));
        // entry 100*1.01=101, exit 100*0.99=99 → gross (99-101)*1 = -2.
        approx(r.trades[0].pnl, -2.0);
    }

    /// Pyramiding scale sequence: adds sized [1.0, 0.5] → 2 lots of qty 1 and 0.5 = 1.5 total.
    #[test]
    fn pyramiding_scale_sequence() {
        let (ts, px) = flat_bars(10);
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.pyramiding = 2;
        s.pyramid_steps.scale = vec![1.0, 0.5];
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades[0].entries, 2);
        approx(r.trades[0].qty, 1.5);
    }

    /// Circuit breaker: a max-drawdown halt stops NEW entries once drawdown is breached.
    #[test]
    fn circuit_breaker_halts_entries() {
        // Long into a persistent decline; without a breaker it re-enters repeatedly.
        let n = 40;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-06-01T{:02}:00:00Z", i)).collect();
        let c: Vec<f64> = (0..n).map(|i| 100.0 - i as f64 * 2.0).collect();
        let o = c.clone();
        let h: Vec<f64> = c.iter().map(|x| x + 0.2).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 0.2).collect();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.sizing = Sizing::PercentEquity { percent: 100.0 };
        s.risk.max_drawdown_pct = Some(10.0);
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        assert!(r.halted_bars > 0, "expected some halted bars");
    }

    /// MAE/MFE: a long that dips then recovers records both a nonzero adverse and favorable
    /// excursion.
    #[test]
    fn mae_mfe_tracked() {
        let n = 8;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-07-{:02}T00:00:00Z", i + 1)).collect();
        // open flat 100; bar mid dips low then rallies high before end.
        let o = vec![100.0; n];
        let c = vec![100.0; n];
        let mut h = vec![100.5; n];
        let mut l = vec![99.5; n];
        l[3] = 95.0; // adverse
        h[5] = 108.0; // favorable
        let s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        let t = &r.trades[0];
        approx(t.mae, 5.0); // 100 - 95
        approx(t.mfe, 8.0); // 108 - 100
    }

    /// OOS split: a 50/50 split yields two stat blocks whose trade counts sum to the total.
    #[test]
    fn oos_split_partitions_trades() {
        let n = 40;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-08-{:02}T00:00:00Z", i + 1)).collect();
        // Alternating up/down so TP-based trades occur throughout both halves.
        let c: Vec<f64> = (0..n).map(|i| 100.0 + ((i % 4) as f64 - 1.5) * 2.0).collect();
        let o = c.clone();
        let h: Vec<f64> = c.iter().map(|x| x + 1.0).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 1.0).collect();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.long.as_mut().unwrap().take_profit_pct = 0.01;
        s.oos_split_pct = 0.5;
        let r = run(&s, &ohlc(&ts, &o, &h, &l, &c));
        let oos = r.oos.expect("oos block present");
        assert_eq!(oos.in_sample.trades + oos.out_sample.trades, r.stats.trades);
        assert!(oos.split_ts.is_some());
    }

    // ── Phase 3: custom indicators ──

    /// A run whose entry references a custom indicator (close + 5) resolves it through the DAG
    /// and trades as if against that derived series. Here entry = close > custom(close+5) is
    /// never true, so no trades; flipping to custom > close (always true) opens one.
    #[test]
    fn custom_indicator_operand_resolves() {
        use custom::{CustomIndicatorDef, Node};
        let def = CustomIndicatorDef {
            nodes: vec![
                Node::Price { field: "close".into() },
                Node::Const { value: 5.0 },
                Node::Add { a: 0, b: 1 },
            ],
            output: 2,
        };
        let (ts, px) = flat_bars(10);
        // Entry: custom(close+5) > close  → 105 > 100 always true → one position, held to end.
        let entry = SignalGroup {
            logic: "all".into(),
            conditions: vec![Signal {
                left: Operand::CustomIndicator { id: "plus5".into() },
                op: Op::Above,
                right: Some(Operand::Price { field: "close".into() }),
            }],
        };
        let mut s = base_settings(entry);
        s.indicators.insert("plus5".into(), def);
        assert!(s.validate().is_none());
        let r = run(&s, &bars(&ts, &px));
        assert_eq!(r.trades.len(), 1);
        assert_eq!(r.trades[0].exit_reason, "end");
    }

    /// Buy-and-hold benchmark: one asset, no fees, price doubles → benchmark ends at 2× capital.
    #[test]
    fn benchmark_buy_hold_curve() {
        let n = 6;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-{:02}T00:00:00Z", i + 1)).collect();
        let c: Vec<f64> = (0..n).map(|i| 100.0 + i as f64 * 20.0).collect(); // 100 → 200
        // No entry signal → no trades, but the benchmark is always computed.
        let s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(0.0, 1.0)] });
        let r = run(&s, &bars(&ts, &c));
        assert_eq!(r.benchmark.len(), n);
        approx(r.benchmark[0].equity, 10_000.0); // bought at 100 → full capital
        approx(r.benchmark[n - 1].equity, 20_000.0); // held to 200 → doubled
    }

    /// Funding: a long held through positive-rate funding pays it (net_funding < 0, final
    /// equity dented vs no funding).
    #[test]
    fn funding_charges_longs() {
        // Hourly bars, flat price, always-long, held to end.
        let n = 25;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-01T{:02}:00:00Z", i)).collect();
        let px = vec![100.0; n];
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.sizing = Sizing::FixedQty { qty: 10.0 };
        s.funding = Funding { annual_rate_pct: 100.0, interval_hours: 8.0 };
        let r = run(&s, &bars(&ts, &px));
        assert!(r.total_funding < 0.0, "long should pay funding, got {}", r.total_funding);
        // ~24h of 100%/yr on notional 1000 ≈ 1000 * 1.0 * (24/8760) ≈ 2.74.
        assert!((r.total_funding.abs() - 2.74).abs() < 0.2, "funding {}", r.total_funding);
    }

    /// Grid mode: an oscillating price across grid levels completes round trips at a profit.
    #[test]
    fn grid_round_trips() {
        // Price zig-zags 90↔110 so cells fill and take profit repeatedly.
        let n = 40;
        let ts: Vec<String> = (0..n).map(|i| format!("2020-01-01T{:02}:00:00Z", i)).collect();
        let c: Vec<f64> = (0..n).map(|i| 100.0 + 10.0 * ((i as f64) * 0.6).sin()).collect();
        let h: Vec<f64> = c.iter().map(|x| x + 1.0).collect();
        let l: Vec<f64> = c.iter().map(|x| x - 1.0).collect();
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(0.0, 1.0)] });
        s.kind = "grid".into();
        s.grid = Some(GridConfig {
            lower: 90.0,
            upper: 110.0,
            levels: 5,
            qty_per_level: 1.0,
            total_budget: 0.0,
            direction: "long".into(),
            stop_below: 0.0,
            stop_above: 0.0,
        });
        let r = run(&s, &ohlc(&ts, &c, &h, &l, &c));
        let g = r.grid.expect("grid stats present");
        assert!(g.round_trips > 0, "expected grid round trips, got {}", g.round_trips);
        assert_eq!(g.levels, 5);
        // Completed round trips are recorded as trades.
        assert_eq!(r.stats.trades, g.round_trips);
    }

    /// An invalid embedded custom-indicator definition is rejected by validate().
    #[test]
    fn custom_indicator_invalid_rejected() {
        use custom::{CustomIndicatorDef, Node};
        let bad = CustomIndicatorDef {
            nodes: vec![Node::Add { a: 0, b: 1 }], // self/forward reference
            output: 0,
        };
        let mut s = base_settings(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
        s.indicators.insert("bad".into(), bad);
        assert!(s.validate().is_some());
    }
}
